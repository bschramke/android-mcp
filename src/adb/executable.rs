use std::{
    env, io,
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};

use tokio::{process::Command, time::timeout};

pub async fn resolve_executable() -> io::Result<PathBuf> {
    if let Some(executable) = env::var_os("ANDROID_MCP_ADB") {
        let executable = PathBuf::from(executable);
        verify_executable(&executable).await?;
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
        match verify_executable(&candidate).await {
            Ok(()) => return Ok(candidate),
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

async fn verify_executable(executable: &Path) -> io::Result<()> {
    let mut command = Command::new(executable);
    command
        .arg("version")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true);

    let status = timeout(Duration::from_secs(5), command.status())
        .await
        .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "ADB startup check timed out after 5 seconds"))?
        .map_err(|error| {
            io::Error::new(
                error.kind(),
                format!(
                    "Cannot execute ADB ({executable:?}): {error}. Install Android SDK Platform-Tools and add adb to PATH, or set ANDROID_MCP_ADB to its executable path"
                ),
            )
        })?;

    if !status.success() {
        return Err(io::Error::other(format!(
            "ADB startup check failed: adb version returned {status}"
        )));
    }

    Ok(())
}
