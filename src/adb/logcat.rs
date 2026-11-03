use std::{io, path::Path, process::Stdio, time::Duration};

use tokio::{
    io::{AsyncRead, AsyncReadExt},
    process::{Child, Command},
    time::{Instant, timeout, timeout_at},
};

mod package;

use super::model::{LogPriority, LogcatSnapshot};

const OUTPUT_LIMIT: usize = 256 * 1024;
const STDERR_LIMIT: usize = 16 * 1024;

pub async fn logcat(
    executable: &Path,
    serial: &str,
    max_lines: u32,
    pid: Option<u32>,
    tags: &[String],
    priority: Option<LogPriority>,
    package: Option<&str>,
) -> io::Result<LogcatSnapshot> {
    if serial.is_empty()
        || serial
            .chars()
            .any(|character| character.is_whitespace() || character == '\0')
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Device serial must be nonempty and contain no whitespace or NUL characters",
        ));
    }
    if !(1..=2000).contains(&max_lines) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "max_lines must be between 1 and 2000",
        ));
    }
    if package.is_some() && pid.is_some() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "package and pid cannot be combined",
        ));
    }
    if let Some(package) = package {
        package::validate(package)?;
    }
    let mut filters = filter_arguments(pid, tags, priority)?;
    let deadline = Instant::now() + Duration::from_secs(5);
    if let Some(package) = package {
        let package_pid = package::resolve_pid(executable, serial, package, deadline).await?;
        filters.insert(0, format!("--pid={package_pid}"));
    }
    if Instant::now() >= deadline {
        return Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "ADB logcat timed out after 5 seconds",
        ));
    }
    let lines = max_lines.to_string();
    let mut child = Command::new(executable)
        .args([
            "-s",
            serial,
            "logcat",
            "-d",
            "-t",
            &lines,
            "-v",
            "threadtime",
        ])
        .args(filters)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|error| {
            io::Error::new(error.kind(), format!("Cannot execute ADB logcat: {error}"))
        })?;

    let result = match timeout_at(deadline, capture(&mut child, serial)).await {
        Ok(result) => result,
        Err(_) => Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "ADB logcat timed out after 5 seconds",
        )),
    };
    if result.is_err() {
        let _ = timeout(Duration::from_secs(1), child.kill()).await;
    }
    result
}

fn filter_arguments(
    pid: Option<u32>,
    tags: &[String],
    priority: Option<LogPriority>,
) -> io::Result<Vec<String>> {
    if pid.is_some_and(|pid| pid == 0 || pid > i32::MAX as u32) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "pid must be between 1 and 2147483647",
        ));
    }
    if tags.len() > 20
        || tags.iter().any(|tag| {
            tag.is_empty()
                || tag.len() > 128
                || tag.starts_with('-')
                || !tag
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"._/+-@".contains(&byte))
        })
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "tags must contain at most 20 exact names, each 1-128 ASCII letters/digits or ._/+-@, without a leading '-'",
        ));
    }
    let mut args = Vec::new();
    if let Some(pid) = pid {
        args.push(format!("--pid={pid}"));
    }
    if !tags.is_empty() {
        let level = priority.map(LogPriority::code).unwrap_or('V');
        args.extend(tags.iter().map(|tag| format!("{tag}:{level}")));
        args.push("*:S".to_owned());
    } else if let Some(priority) = priority {
        args.push(format!("*:{}", priority.code()));
    }
    Ok(args)
}

async fn read_capped(reader: impl AsyncRead + Unpin, limit: usize) -> io::Result<(Vec<u8>, bool)> {
    let mut bytes = Vec::new();
    reader
        .take((limit + 1) as u64)
        .read_to_end(&mut bytes)
        .await?;
    let truncated = bytes.len() > limit;
    bytes.truncate(limit);
    Ok((bytes, truncated))
}

fn snapshot(serial: &str, bytes: Vec<u8>, truncated: bool) -> io::Result<LogcatSnapshot> {
    let output = match std::str::from_utf8(&bytes) {
        Ok(text) => text.to_owned(),
        Err(error) if truncated && error.error_len().is_none() => {
            String::from_utf8(bytes[..error.valid_up_to()].to_vec()).map_err(io::Error::other)?
        }
        Err(_) => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "ADB logcat output is not valid UTF-8",
            ));
        }
    };
    Ok(LogcatSnapshot {
        serial: serial.to_owned(),
        format: "threadtime".to_owned(),
        output,
        truncated,
    })
}

async fn capture(child: &mut Child, serial: &str) -> io::Result<LogcatSnapshot> {
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| io::Error::other("Missing logcat stdout"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| io::Error::other("Missing logcat stderr"))?;
    let stdout_read = read_capped(stdout, OUTPUT_LIMIT);
    let stderr_read = read_capped(stderr, STDERR_LIMIT);
    tokio::pin!(stdout_read, stderr_read);
    let mut output = None;
    let mut diagnostic = None;

    while output.is_none() || diagnostic.is_none() {
        tokio::select! {
            result = &mut stdout_read, if output.is_none() => {
                let (bytes, truncated) = result?;
                if truncated {
                    if child.try_wait()?.is_none() { child.kill().await?; }
                    return snapshot(serial, bytes, true);
                }
                output = Some(bytes);
            }
            result = &mut stderr_read, if diagnostic.is_none() => {
                let (bytes, truncated) = result?;
                if truncated {
                    return Err(io::Error::other("ADB logcat stderr exceeded the 16 KiB limit"));
                }
                diagnostic = Some(bytes);
            }
        }
    }

    let status = child.wait().await?;
    if !status.success() {
        return Err(io::Error::other(format!(
            "ADB logcat failed ({status}): {}",
            String::from_utf8_lossy(&diagnostic.unwrap_or_default()).trim()
        )));
    }
    snapshot(serial, output.unwrap_or_default(), false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_filters_preserve_the_original_command() {
        assert!(filter_arguments(None, &[], None).unwrap().is_empty());
    }

    #[test]
    fn combines_pid_tag_alternatives_and_minimum_priority() {
        assert_eq!(
            filter_arguments(
                Some(2210),
                &["ErrorProcessor".into(), "AudioController".into()],
                Some(LogPriority::Warn)
            )
            .unwrap(),
            ["--pid=2210", "ErrorProcessor:W", "AudioController:W", "*:S"]
        );
        assert_eq!(
            filter_arguments(None, &["ExampleApp".into()], None).unwrap(),
            ["ExampleApp:V", "*:S"]
        );
        assert_eq!(filter_arguments(Some(1), &[], None).unwrap(), ["--pid=1"]);
        assert_eq!(
            filter_arguments(Some(i32::MAX as u32), &[], None).unwrap(),
            ["--pid=2147483647"]
        );
    }

    #[test]
    fn maps_all_minimum_priorities_to_logcat_levels() {
        for (priority, expected) in [
            (LogPriority::Verbose, "*:V"),
            (LogPriority::Debug, "*:D"),
            (LogPriority::Info, "*:I"),
            (LogPriority::Warn, "*:W"),
            (LogPriority::Error, "*:E"),
            (LogPriority::Fatal, "*:F"),
        ] {
            assert_eq!(
                filter_arguments(None, &[], Some(priority)).unwrap(),
                [expected]
            );
        }
    }

    #[test]
    fn rejects_invalid_pids_and_unsafe_filter_expressions() {
        for pid in [0, i32::MAX as u32 + 1, u32::MAX] {
            assert!(filter_arguments(Some(pid), &[], None).is_err());
        }
        for tag in [
            "",
            "*",
            "Example:W",
            "--pid=1",
            "a b",
            "a\nb",
            "a\0b",
            "a;b",
            "$(id)",
            "a|b",
            "a&b",
            "a`id`",
            "a'b",
            "a\"b",
            "nonascii-é",
        ] {
            assert!(
                filter_arguments(None, &[tag.to_owned()], None).is_err(),
                "Accepted {tag:?}"
            );
        }
        assert!(filter_arguments(None, &["a".repeat(129)], None).is_err());
        assert!(filter_arguments(None, &vec!["Tag".into(); 21], None).is_err());
        assert!(filter_arguments(None, &vec!["Tag".into(); 20], None).is_ok());
        assert!(
            filter_arguments(
                None,
                &[
                    "a".repeat(128),
                    "android.os.Debug".into(),
                    "path/Tag+Native@1".into()
                ],
                None
            )
            .is_ok()
        );
    }

    #[tokio::test]
    async fn capped_reader_distinguishes_exact_limit_from_overflow() {
        assert_eq!(
            read_capped(&b"1234"[..], 4).await.unwrap(),
            (b"1234".to_vec(), false)
        );
        assert_eq!(
            read_capped(&b"12345"[..], 4).await.unwrap(),
            (b"1234".to_vec(), true)
        );
    }

    #[test]
    fn truncation_drops_only_an_incomplete_utf8_suffix() {
        let bytes = vec![b'a', 0xe2, 0x82];
        assert_eq!(snapshot("serial", bytes.clone(), true).unwrap().output, "a");
        assert!(snapshot("serial", bytes, false).is_err());
        assert!(snapshot("serial", vec![0xff], true).is_err());
    }
}
