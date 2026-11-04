use std::{
    collections::HashMap,
    ffi::OsString,
    fs::{self, File},
    io::{self, Read},
    path::{Path, PathBuf},
    process::Stdio,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use tokio::{
    io::AsyncWriteExt,
    process::{Child, ChildStdin, Command},
    sync::Mutex,
    time::{Instant, timeout},
};

use crate::adb::{
    command::{CommandOptions, ensure_success, run_adb, run_shell},
    device_info::validate_serial,
};

const MAX_RECORDING_SECONDS: u32 = 180;
const STOP_TIMEOUT: Duration = Duration::from_secs(10);
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(180);
const COMMAND_STDOUT_LIMIT: usize = 64 * 1024;
const COMMAND_STDERR_LIMIT: usize = 16 * 1024;

#[derive(Clone, Default)]
pub struct ScreenRecordingManager {
    sessions: Arc<Mutex<HashMap<String, ActiveRecording>>>,
}

struct ActiveRecording {
    child: Child,
    stdin: ChildStdin,
    remote_path: String,
    local_path: PathBuf,
    overwrite: bool,
    stopped: bool,
}

pub struct ScreenRecordingResult {
    pub path: PathBuf,
    pub size_bytes: u64,
}

impl ScreenRecordingManager {
    pub async fn start(
        &self,
        executable: &Path,
        serial: &str,
        local_path: &Path,
        overwrite: bool,
    ) -> io::Result<()> {
        validate_serial(serial)?;
        validate_destination(local_path)?;
        if !overwrite && local_path.exists() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!(
                    "Recording destination already exists: {}",
                    local_path.display()
                ),
            ));
        }

        let mut sessions = self.sessions.lock().await;
        if sessions.contains_key(serial) {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!("A screen recording is already active for {serial}"),
            ));
        }

        let remote_path = remote_recording_path()?;
        let mut child = Command::new(executable)
            .args([
                "-s",
                serial,
                "shell",
                "-tt",
                "screenrecord",
                "--time-limit",
                &MAX_RECORDING_SECONDS.to_string(),
                &remote_path,
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .map_err(|error| {
                io::Error::new(
                    error.kind(),
                    format!("Cannot start ADB screen recording: {error}"),
                )
            })?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| io::Error::other("Missing ADB screen recording stdin"))?;
        if let Some(status) = child.try_wait()? {
            return Err(io::Error::other(format!(
                "ADB screen recording exited before it started ({status})"
            )));
        }

        sessions.insert(
            serial.to_owned(),
            ActiveRecording {
                child,
                stdin,
                remote_path,
                local_path: local_path.to_owned(),
                overwrite,
                stopped: false,
            },
        );
        Ok(())
    }

    pub async fn stop(&self, executable: &Path, serial: &str) -> io::Result<ScreenRecordingResult> {
        validate_serial(serial)?;
        let mut sessions = self.sessions.lock().await;
        let session = sessions.get_mut(serial).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!("No active screen recording for {serial}"),
            )
        })?;

        if !session.stopped {
            if session.child.try_wait()?.is_none() {
                if let Err(error) = session.stdin.write_all(&[0x03]).await
                    && session.child.try_wait()?.is_none()
                {
                    return Err(io::Error::new(
                        error.kind(),
                        format!("Cannot stop screen recording for {serial}: {error}"),
                    ));
                }
                let _ = session.stdin.flush().await;
            }
            match timeout(STOP_TIMEOUT, session.child.wait()).await {
                Ok(result) => {
                    result?;
                    session.stopped = true;
                }
                Err(_) => {
                    let _ = session.child.kill().await;
                    return Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        format!("Timed out stopping screen recording for {serial}"),
                    ));
                }
            }
        }

        let remote_path = session.remote_path.clone();
        let local_path = session.local_path.clone();
        let overwrite = session.overwrite;
        let staging_path = staging_path(&local_path)?;
        let result = download_recording(
            executable,
            serial,
            &remote_path,
            &local_path,
            &staging_path,
            overwrite,
        )
        .await;
        let size_bytes = match result {
            Ok(size_bytes) => size_bytes,
            Err(error) => {
                let _ = fs::remove_file(&staging_path);
                return Err(error);
            }
        };

        if let Err(error) = remove_remote_recording(executable, serial, &remote_path).await {
            eprintln!("Cannot remove temporary screen recording {remote_path}: {error}");
        }
        sessions.remove(serial);
        Ok(ScreenRecordingResult {
            path: local_path,
            size_bytes,
        })
    }

    pub async fn stop_all(&self, executable: &Path) -> Vec<(String, String)> {
        let serials: Vec<_> = self.sessions.lock().await.keys().cloned().collect();
        let mut failures = Vec::new();
        for serial in serials {
            if let Err(error) = self.stop(executable, &serial).await {
                failures.push((serial, error.to_string()));
            }
        }
        failures
    }
}

fn validate_destination(path: &Path) -> io::Result<()> {
    if !path.is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Recording path must be absolute",
        ));
    }
    if !path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("mp4"))
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Recording path must use the .mp4 extension",
        ));
    }
    if !path.parent().is_some_and(Path::is_dir) {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "Recording destination directory does not exist",
        ));
    }
    Ok(())
}

fn remote_recording_path() -> io::Result<String> {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(io::Error::other)?
        .as_nanos();
    Ok(format!(
        "/sdcard/.android-mcp-recording-{}-{timestamp}.mp4",
        std::process::id()
    ))
}

fn staging_path(destination: &Path) -> io::Result<PathBuf> {
    let parent = destination.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "Recording destination must have a parent directory",
        )
    })?;
    let file_name = destination
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "Invalid recording path"))?;
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(io::Error::other)?
        .as_nanos();
    let mut staging_name = OsString::from(".");
    staging_name.push(file_name);
    staging_name.push(format!(
        ".android-mcp-{}-{timestamp}.tmp",
        std::process::id()
    ));
    Ok(parent.join(staging_name))
}

async fn download_recording(
    executable: &Path,
    serial: &str,
    remote_path: &str,
    local_path: &Path,
    staging_path: &Path,
    overwrite: bool,
) -> io::Result<u64> {
    let arguments = [
        OsString::from("-s"),
        OsString::from(serial),
        OsString::from("pull"),
        OsString::from(remote_path),
        staging_path.as_os_str().to_os_string(),
    ];
    let output = run_adb(
        executable,
        arguments,
        "screen recording download",
        CommandOptions::new(
            Instant::now() + DOWNLOAD_TIMEOUT,
            COMMAND_STDOUT_LIMIT,
            COMMAND_STDERR_LIMIT,
        )
        .with_timeout_message("ADB screen recording download timed out after 180 seconds"),
    )
    .await?;
    ensure_success(&output, "screen recording download")?;

    let size_bytes = validate_mp4(staging_path)?;
    if overwrite {
        fs::rename(staging_path, local_path)?;
    } else {
        fs::hard_link(staging_path, local_path)?;
        fs::remove_file(staging_path)?;
    }
    Ok(size_bytes)
}

fn validate_mp4(path: &Path) -> io::Result<u64> {
    let metadata = fs::metadata(path)?;
    if metadata.len() < 12 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Screen recording is not a valid MP4 file",
        ));
    }
    let mut header = [0; 8];
    File::open(path)?.read_exact(&mut header)?;
    if &header[4..8] != b"ftyp" {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Screen recording is not a valid MP4 file",
        ));
    }
    Ok(metadata.len())
}

async fn remove_remote_recording(
    executable: &Path,
    serial: &str,
    remote_path: &str,
) -> io::Result<()> {
    let output = run_shell(
        executable,
        serial,
        ["rm", "-f", remote_path],
        "temporary screen recording cleanup",
        CommandOptions::new(
            Instant::now() + Duration::from_secs(5),
            COMMAND_STDOUT_LIMIT,
            COMMAND_STDERR_LIMIT,
        ),
    )
    .await?;
    ensure_success(&output, "temporary screen recording cleanup")
}
