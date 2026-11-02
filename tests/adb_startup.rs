#![cfg(unix)]

mod support;

use std::{
    fs,
    os::unix::fs::symlink,
    path::{Path, PathBuf},
    process::Stdio,
};

use support::{TIMEOUT, TestResult, connect_command, disconnect, server_command};
use tokio::{process::Command, time::timeout};

fn sdk_directory(parent: &Path, name: &str, executable: &str) -> TestResult<PathBuf> {
    let root = parent.join(name);
    let tools = root.join("platform-tools");
    fs::create_dir_all(&tools)?;
    symlink(executable, tools.join("adb"))?;
    Ok(root)
}

async fn assert_startup_failure(executable: &str, expected_message: &str) -> TestResult {
    let mut command = server_command();
    command.env("ANDROID_MCP_ADB", executable);
    assert_command_failure(command, expected_message).await?;
    Ok(())
}

async fn assert_command_failure(
    mut command: Command,
    expected_message: &str,
) -> TestResult<String> {
    let child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;
    let output = timeout(TIMEOUT, child.wait_with_output()).await??;

    assert!(
        !output.status.success(),
        "Startup must fail when ADB is unavailable"
    );
    assert!(
        output.stdout.is_empty(),
        "Startup diagnostics must not pollute MCP stdout"
    );
    let stderr = String::from_utf8(output.stderr)?;
    assert!(
        stderr.contains(expected_message),
        "Unexpected startup diagnostic: {stderr}"
    );
    Ok(stderr)
}

#[tokio::test]
async fn missing_adb_fails_before_mcp_startup() -> TestResult {
    assert_startup_failure("/dev/null/android-mcp-missing-adb", "Cannot execute ADB").await
}

#[tokio::test]
async fn non_executable_adb_fails_before_mcp_startup() -> TestResult {
    assert_startup_failure("/", "Cannot execute ADB").await
}

#[tokio::test]
async fn unsuccessful_adb_version_fails_before_mcp_startup() -> TestResult {
    assert_startup_failure("/usr/bin/false", "ADB startup check failed").await
}

#[tokio::test]
async fn unresponsive_adb_fails_before_mcp_startup() -> TestResult {
    assert_startup_failure(
        "/usr/bin/yes",
        "ADB startup check timed out after 5 seconds",
    )
    .await
}

#[tokio::test]
async fn path_adb_takes_precedence_over_sdk_variables() -> TestResult {
    let directory = tempfile::tempdir()?;
    let sdk = sdk_directory(directory.path(), "sdk", "/usr/bin/false")?;
    let path_directory = directory.path().join("bin");
    fs::create_dir(&path_directory)?;
    symlink("/usr/bin/true", path_directory.join("adb"))?;
    let mut command = server_command();
    command
        .env("PATH", path_directory)
        .env("ANDROID_SDK_ROOT", &sdk)
        .env("ANDROID_HOME", &sdk);

    let (client, child) = connect_command(command).await?;
    disconnect(client, child).await
}

#[tokio::test]
async fn sdk_root_is_used_before_android_home() -> TestResult {
    let directory = tempfile::tempdir()?;
    let sdk = sdk_directory(directory.path(), "sdk with spaces", "/usr/bin/true")?;
    let home = sdk_directory(directory.path(), "home", "/usr/bin/false")?;
    let mut command = server_command();
    command
        .env("ANDROID_SDK_ROOT", sdk)
        .env("ANDROID_HOME", home);

    let (client, child) = connect_command(command).await?;
    disconnect(client, child).await
}

#[tokio::test]
async fn android_home_is_used_when_sdk_root_is_missing() -> TestResult {
    let directory = tempfile::tempdir()?;
    let home = sdk_directory(directory.path(), "home", "/usr/bin/true")?;
    let mut command = server_command();
    command.env("ANDROID_HOME", home);

    let (client, child) = connect_command(command).await?;
    disconnect(client, child).await
}

#[tokio::test]
async fn android_home_is_used_after_sdk_root_fails() -> TestResult {
    let directory = tempfile::tempdir()?;
    let sdk = sdk_directory(directory.path(), "sdk", "/usr/bin/false")?;
    let home = sdk_directory(directory.path(), "home", "/usr/bin/true")?;
    let mut command = server_command();
    command
        .env("ANDROID_SDK_ROOT", sdk)
        .env("ANDROID_HOME", home);

    let (client, child) = connect_command(command).await?;
    disconnect(client, child).await
}

#[tokio::test]
async fn failing_path_adb_falls_back_to_sdk_root() -> TestResult {
    let directory = tempfile::tempdir()?;
    let sdk = sdk_directory(directory.path(), "sdk", "/usr/bin/true")?;
    let path_directory = directory.path().join("bin");
    fs::create_dir(&path_directory)?;
    symlink("/usr/bin/false", path_directory.join("adb"))?;
    let mut command = server_command();
    command
        .env("PATH", path_directory)
        .env("ANDROID_SDK_ROOT", sdk);

    let (client, child) = connect_command(command).await?;
    disconnect(client, child).await
}

#[tokio::test]
async fn invalid_override_does_not_fall_back_to_valid_sdk() -> TestResult {
    let directory = tempfile::tempdir()?;
    let sdk = sdk_directory(directory.path(), "sdk", "/usr/bin/true")?;
    let mut command = server_command();
    command
        .env("ANDROID_MCP_ADB", "/usr/bin/false")
        .env("ANDROID_SDK_ROOT", &sdk)
        .env("ANDROID_HOME", sdk);

    assert_command_failure(command, "ADB startup check failed").await?;
    Ok(())
}

#[tokio::test]
async fn all_failed_candidates_are_reported() -> TestResult {
    let directory = tempfile::tempdir()?;
    let sdk = sdk_directory(directory.path(), "sdk", "/usr/bin/false")?;
    let home = directory.path().join("missing sdk");
    let mut command = server_command();
    command
        .env("ANDROID_SDK_ROOT", &sdk)
        .env("ANDROID_HOME", &home);

    let diagnostic = assert_command_failure(command, "No working ADB executable found").await?;
    assert!(diagnostic.contains("adb: Cannot execute ADB"));
    assert!(diagnostic.contains(&sdk.join("platform-tools/adb").display().to_string()));
    assert!(diagnostic.contains(&home.join("platform-tools/adb").display().to_string()));
    Ok(())
}

#[tokio::test]
async fn empty_sdk_variables_are_ignored() -> TestResult {
    let mut command = server_command();
    command.env("ANDROID_SDK_ROOT", "").env("ANDROID_HOME", "");

    let diagnostic = assert_command_failure(command, "No working ADB executable found").await?;
    assert!(!diagnostic.contains("platform-tools"));
    Ok(())
}
