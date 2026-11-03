use std::{
    env, io,
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};

use tokio::{
    io::{AsyncRead, AsyncReadExt},
    process::{Child, Command},
    time::timeout,
};

const VERSION_OUTPUT_LIMIT: usize = 4 * 1024;
const VERSION_STDERR_LIMIT: usize = 16 * 1024;

pub async fn resolve_executable() -> io::Result<PathBuf> {
    if let Some(executable) = env::var_os("ANDROID_MCP_ADB") {
        let executable = PathBuf::from(executable);
        adb_version(&executable).await?;
        return Ok(executable);
    }

    let mut candidates = vec![PathBuf::from("adb")];
    let binary_name = if cfg!(windows) { "adb.exe" } else { "adb" };
    for variable in ["ANDROID_SDK_ROOT", "ANDROID_HOME"] {
        if let Some(root) = env::var_os(variable).filter(|value| !value.is_empty()) {
            let candidate = PathBuf::from(root).join("platform-tools").join(binary_name);
            if !candidates.contains(&candidate) {
                candidates.push(candidate);
            }
        }
    }

    let mut failures = Vec::new();
    let mut last_kind = io::ErrorKind::NotFound;
    for candidate in candidates {
        match adb_version(&candidate).await {
            Ok(_) => return Ok(candidate),
            Err(error) => {
                last_kind = error.kind();
                failures.push(format!("{}: {error}", candidate.display()));
            }
        }
    }

    Err(io::Error::new(
        last_kind,
        format!(
            "No working ADB executable found. Tried:\n{}\nInstall Android SDK Platform-Tools and configure PATH, ANDROID_SDK_ROOT, ANDROID_HOME, or ANDROID_MCP_ADB",
            failures.join("\n")
        ),
    ))
}

pub(super) async fn adb_version(executable: &Path) -> io::Result<Vec<u8>> {
    let mut child = Command::new(executable)
        .arg("version")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|error| {
            io::Error::new(
                error.kind(),
                format!(
                    "Cannot execute ADB ({executable:?}): {error}. Install Android SDK Platform-Tools and add adb to PATH, or set ANDROID_MCP_ADB to its executable path"
                ),
            )
        })?;

    match timeout(Duration::from_secs(5), capture_version(&mut child)).await {
        Ok(result) => result,
        Err(_) => {
            let _ = timeout(Duration::from_secs(1), child.kill()).await;
            Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "ADB startup check timed out after 5 seconds",
            ))
        }
    }
}

async fn capture_version(child: &mut Child) -> io::Result<Vec<u8>> {
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| io::Error::other("Missing ADB version stdout"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| io::Error::other("Missing ADB version stderr"))?;
    let stdout_read = read_capped(stdout, VERSION_OUTPUT_LIMIT);
    let stderr_read = read_capped(stderr, VERSION_STDERR_LIMIT);
    tokio::pin!(stdout_read, stderr_read);
    let mut stdout = None;
    let mut stderr = None;

    while stdout.is_none() || stderr.is_none() {
        tokio::select! {
            result = &mut stdout_read, if stdout.is_none() => {
                let (bytes, exceeded) = result?;
                if exceeded {
                    return Err(io::Error::other("ADB version output exceeded the 4 KiB limit"));
                }
                stdout = Some(bytes);
            }
            result = &mut stderr_read, if stderr.is_none() => {
                let (bytes, exceeded) = result?;
                if exceeded {
                    return Err(io::Error::other("ADB version stderr exceeded the 16 KiB limit"));
                }
                stderr = Some(bytes);
            }
        }
    }

    let status = child.wait().await?;

    if !status.success() {
        return Err(io::Error::other(format!(
            "ADB startup check failed: adb version returned {status}: {}",
            String::from_utf8_lossy(&stderr.unwrap_or_default()).trim()
        )));
    }

    Ok(stdout.unwrap_or_default())
}

async fn read_capped(reader: impl AsyncRead + Unpin, limit: usize) -> io::Result<(Vec<u8>, bool)> {
    let mut bytes = Vec::new();
    reader
        .take((limit + 1) as u64)
        .read_to_end(&mut bytes)
        .await?;
    let exceeded = bytes.len() > limit;
    bytes.truncate(limit);
    Ok((bytes, exceeded))
}
