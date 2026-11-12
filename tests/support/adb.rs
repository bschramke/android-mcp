use std::{
    fs,
    path::{Path, PathBuf},
    sync::OnceLock,
};

use super::{Client, TestResult, connect_command, server_command};
use tokio::process::Child;

pub fn adb_fixture() -> TestResult<PathBuf> {
    static FIXTURE: OnceLock<Result<(tempfile::TempDir, PathBuf), String>> = OnceLock::new();
    let fixture = FIXTURE.get_or_init(|| {
        let directory = tempfile::tempdir().map_err(|error| error.to_string())?;
        let executable = directory
            .path()
            .join(format!("adb-stub{}", std::env::consts::EXE_SUFFIX));
        let output =
            std::process::Command::new(std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into()))
                .arg("--edition=2024")
                .arg(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/support/adb_stub.rs"))
                .arg("-o")
                .arg(&executable)
                .output()
                .map_err(|error| error.to_string())?;
        if !output.status.success() {
            return Err(format!(
                "Cannot compile ADB fixture: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }
        Ok((directory, executable))
    });
    match fixture {
        Ok((_, executable)) => Ok(executable.clone()),
        Err(error) => Err(std::io::Error::other(error.clone()).into()),
    }
}

pub fn adb_executable_name() -> &'static str {
    if cfg!(windows) { "adb.exe" } else { "adb" }
}

pub fn copy_adb_fixture(directory: &Path) -> TestResult<PathBuf> {
    fs::create_dir_all(directory)?;
    let executable = directory.join(adb_executable_name());
    fs::copy(adb_fixture()?, &executable)?;
    Ok(executable)
}

pub fn set_startup_mode(executable: &Path, mode: &str) -> TestResult {
    let mut sidecar = executable.as_os_str().to_os_string();
    sidecar.push(".mode");
    fs::write(PathBuf::from(sidecar), mode)?;
    Ok(())
}

#[allow(dead_code)]
pub async fn connect(mode: &str) -> TestResult<(Client, Child)> {
    let mut command = server_command();
    command
        .env("ANDROID_MCP_ADB", adb_fixture()?)
        .env("ANDROID_MCP_TEST_ADB_MODE", mode);
    connect_command(command).await
}
