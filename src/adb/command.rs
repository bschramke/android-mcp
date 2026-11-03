use std::{
    ffi::{OsStr, OsString},
    io,
    path::Path,
    process::{ExitStatus, Stdio},
    time::Duration,
};

use tokio::{
    io::{AsyncRead, AsyncReadExt},
    process::{Child, Command},
    time::{Instant, timeout, timeout_at},
};

#[derive(Clone, Copy)]
pub(super) enum OverflowBehavior {
    Error,
    TruncateStdout,
}

#[derive(Clone, Copy)]
pub(super) struct CommandOptions {
    deadline: Instant,
    stdout_limit: usize,
    stderr_limit: usize,
    overflow_behavior: OverflowBehavior,
    timeout_message: Option<&'static str>,
    stdout_label: &'static str,
}

impl CommandOptions {
    pub(super) fn new(deadline: Instant, stdout_limit: usize, stderr_limit: usize) -> Self {
        Self {
            deadline,
            stdout_limit,
            stderr_limit,
            overflow_behavior: OverflowBehavior::Error,
            timeout_message: None,
            stdout_label: "stdout",
        }
    }

    pub(super) fn with_overflow_behavior(mut self, overflow_behavior: OverflowBehavior) -> Self {
        self.overflow_behavior = overflow_behavior;
        self
    }

    pub(super) fn with_timeout_message(mut self, timeout_message: &'static str) -> Self {
        self.timeout_message = Some(timeout_message);
        self
    }

    pub(super) fn with_stdout_label(mut self, stdout_label: &'static str) -> Self {
        self.stdout_label = stdout_label;
        self
    }
}

pub(super) struct CommandOutput {
    pub(super) status: ExitStatus,
    pub(super) stdout: Vec<u8>,
    pub(super) stderr: Vec<u8>,
    pub(super) stdout_truncated: bool,
}

pub(super) async fn run_adb<I, S>(
    executable: &Path,
    arguments: I,
    operation: &str,
    options: CommandOptions,
) -> io::Result<CommandOutput>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let mut child = Command::new(executable)
        .args(arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|error| {
            io::Error::new(
                error.kind(),
                format!("Cannot execute ADB {operation}: {error}"),
            )
        })?;

    let result = match timeout_at(options.deadline, capture(&mut child, operation, options)).await {
        Ok(result) => result,
        Err(_) => Err(io::Error::new(
            io::ErrorKind::TimedOut,
            options
                .timeout_message
                .map(str::to_owned)
                .unwrap_or_else(|| format!("ADB {operation} timed out after 5 seconds")),
        )),
    };
    if result.is_err() {
        let _ = timeout(Duration::from_secs(1), child.kill()).await;
    }
    result
}

pub(super) async fn run_shell<I, S>(
    executable: &Path,
    serial: &str,
    arguments: I,
    operation: &str,
    options: CommandOptions,
) -> io::Result<CommandOutput>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let mut adb_arguments = vec![
        OsString::from("-s"),
        OsString::from(serial),
        OsString::from("shell"),
    ];
    adb_arguments.extend(
        arguments
            .into_iter()
            .map(|argument| argument.as_ref().to_os_string()),
    );
    run_adb(executable, adb_arguments, operation, options).await
}

pub(super) fn ensure_success(output: &CommandOutput, operation: &str) -> io::Result<()> {
    if output.status.success() {
        return Ok(());
    }
    Err(io::Error::other(format!(
        "ADB {operation} failed ({}): {}",
        output.status,
        String::from_utf8_lossy(&output.stderr).trim()
    )))
}

async fn capture(
    child: &mut Child,
    operation: &str,
    options: CommandOptions,
) -> io::Result<CommandOutput> {
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| io::Error::other(format!("Missing ADB {operation} stdout")))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| io::Error::other(format!("Missing ADB {operation} stderr")))?;
    let stdout_read = read_capped(stdout, options.stdout_limit);
    let stderr_read = read_capped(stderr, options.stderr_limit);
    tokio::pin!(stdout_read, stderr_read);
    let mut output = None;
    let mut diagnostic = None;

    while output.is_none() || diagnostic.is_none() {
        tokio::select! {
            result = &mut stdout_read, if output.is_none() => {
                let (bytes, exceeded) = result?;
                if exceeded {
                    match options.overflow_behavior {
                        OverflowBehavior::Error => {
                            return Err(io::Error::other(format!(
                                "ADB {operation} {} exceeded the {} limit",
                                options.stdout_label,
                                format_limit(options.stdout_limit)
                            )));
                        }
                        OverflowBehavior::TruncateStdout => {
                            kill_for_truncation(child, operation).await?;
                            let status = child.wait().await?;
                            return Ok(CommandOutput {
                                status,
                                stdout: bytes,
                                stderr: Vec::new(),
                                stdout_truncated: true,
                            });
                        }
                    }
                }
                output = Some(bytes);
            }
            result = &mut stderr_read, if diagnostic.is_none() => {
                let (bytes, exceeded) = result?;
                if exceeded {
                    return Err(io::Error::other(format!(
                        "ADB {operation} stderr exceeded the {} limit",
                        format_limit(options.stderr_limit)
                    )));
                }
                diagnostic = Some(bytes);
            }
        }
    }

    let status = child.wait().await?;
    Ok(CommandOutput {
        status,
        stdout: output.unwrap_or_default(),
        stderr: diagnostic.unwrap_or_default(),
        stdout_truncated: false,
    })
}

async fn kill_for_truncation(child: &mut Child, operation: &str) -> io::Result<()> {
    if child.try_wait()?.is_some() {
        return Ok(());
    }
    match timeout(Duration::from_secs(1), child.kill()).await {
        Ok(Ok(())) => Ok(()),
        Ok(Err(_)) if child.try_wait()?.is_some() => Ok(()),
        Ok(Err(error)) => Err(error),
        Err(_) => Err(io::Error::new(
            io::ErrorKind::TimedOut,
            format!("ADB {operation} cleanup timed out"),
        )),
    }
}

pub(super) async fn read_capped(
    reader: impl AsyncRead + Unpin,
    limit: usize,
) -> io::Result<(Vec<u8>, bool)> {
    let mut bytes = Vec::new();
    reader
        .take((limit + 1) as u64)
        .read_to_end(&mut bytes)
        .await?;
    let exceeded = bytes.len() > limit;
    bytes.truncate(limit);
    Ok((bytes, exceeded))
}

fn format_limit(limit: usize) -> String {
    const MIB: usize = 1024 * 1024;
    if limit.is_multiple_of(MIB) {
        format!("{} MiB", limit / MIB)
    } else if limit.is_multiple_of(1024) {
        format!("{} KiB", limit / 1024)
    } else {
        format!("{limit} byte")
    }
}
