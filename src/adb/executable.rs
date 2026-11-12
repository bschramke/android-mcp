use std::{
    env, io,
    path::{Path, PathBuf},
    time::Duration,
};

use tokio::time::Instant;

use super::command::{CommandOptions, ensure_success, run_adb};

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
    let output = run_adb(
        executable,
        ["version"],
        "version",
        CommandOptions::new(
            Instant::now() + Duration::from_secs(5),
            VERSION_OUTPUT_LIMIT,
            VERSION_STDERR_LIMIT,
        )
        .with_timeout_message("ADB startup check timed out after 5 seconds")
        .with_stdout_label("output"),
    )
    .await?;
    ensure_success(&output, "startup check")?;
    Ok(output.stdout)
}
