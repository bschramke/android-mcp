use std::{path::PathBuf, sync::OnceLock};

use super::{Client, TestResult, connect_command, server_command};
use tokio::process::Child;

pub fn adb_fixture() -> TestResult<PathBuf> {
    static FIXTURE: OnceLock<Result<(tempfile::TempDir, PathBuf), String>> = OnceLock::new();
    let fixture = FIXTURE.get_or_init(|| {
        let directory = tempfile::tempdir().map_err(|error| error.to_string())?;
        let executable = directory.path().join("adb-stub");
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

#[allow(dead_code)]
pub async fn connect(mode: &str) -> TestResult<(Client, Child)> {
    let mut command = server_command();
    command
        .env("ANDROID_MCP_ADB", adb_fixture()?)
        .env("ANDROID_MCP_TEST_ADB_MODE", mode);
    connect_command(command).await
}
