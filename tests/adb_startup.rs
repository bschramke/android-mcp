mod support;

use std::{
    path::{Path, PathBuf},
    process::Stdio,
};

use support::adb::{adb_executable_name, copy_adb_fixture, set_startup_mode};
use support::{TIMEOUT, TestResult, connect_command, disconnect, server_command};
use tokio::{process::Command, time::timeout};

fn sdk_directory(parent: &Path, name: &str, startup_mode: &str) -> TestResult<PathBuf> {
    let root = parent.join(name);
    let tools = root.join("platform-tools");
    let executable = copy_adb_fixture(&tools)?;
    set_startup_mode(&executable, startup_mode)?;
    Ok(root)
}

async fn assert_startup_failure(executable: &Path, expected_message: &str) -> TestResult {
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
    let directory = tempfile::tempdir()?;
    let missing = directory.path().join(adb_executable_name());
    assert_startup_failure(&missing, "Cannot execute ADB").await
}

#[tokio::test]
async fn directory_adb_fails_before_mcp_startup() -> TestResult {
    let directory = tempfile::tempdir()?;
    assert_startup_failure(directory.path(), "Cannot execute ADB").await
}

#[tokio::test]
async fn unsuccessful_adb_version_fails_before_mcp_startup() -> TestResult {
    let directory = tempfile::tempdir()?;
    let executable = copy_adb_fixture(directory.path())?;
    set_startup_mode(&executable, "version_failure")?;
    assert_startup_failure(&executable, "ADB startup check failed").await
}

#[tokio::test]
async fn excessive_adb_version_output_fails_before_mcp_startup() -> TestResult {
    let directory = tempfile::tempdir()?;
    let executable = copy_adb_fixture(directory.path())?;
    set_startup_mode(&executable, "version_large")?;
    assert_startup_failure(&executable, "ADB version output exceeded the 4 KiB limit").await
}

#[tokio::test]
async fn path_adb_takes_precedence_over_sdk_variables() -> TestResult {
    let directory = tempfile::tempdir()?;
    let sdk = sdk_directory(directory.path(), "sdk", "version_failure")?;
    let path_directory = directory.path().join("bin");
    let path_adb = copy_adb_fixture(&path_directory)?;
    set_startup_mode(&path_adb, "version_success")?;
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
    let sdk = sdk_directory(directory.path(), "sdk with spaces", "version_success")?;
    let home = sdk_directory(directory.path(), "home", "version_failure")?;
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
    let home = sdk_directory(directory.path(), "home", "version_success")?;
    let mut command = server_command();
    command.env("ANDROID_HOME", home);

    let (client, child) = connect_command(command).await?;
    disconnect(client, child).await
}

#[tokio::test]
async fn android_home_is_used_after_sdk_root_fails() -> TestResult {
    let directory = tempfile::tempdir()?;
    let sdk = sdk_directory(directory.path(), "sdk", "version_failure")?;
    let home = sdk_directory(directory.path(), "home", "version_success")?;
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
    let sdk = sdk_directory(directory.path(), "sdk", "version_success")?;
    let path_directory = directory.path().join("bin");
    let path_adb = copy_adb_fixture(&path_directory)?;
    set_startup_mode(&path_adb, "version_failure")?;
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
    let sdk = sdk_directory(directory.path(), "sdk", "version_success")?;
    let override_directory = directory.path().join("override");
    let override_adb = copy_adb_fixture(&override_directory)?;
    set_startup_mode(&override_adb, "version_failure")?;
    let mut command = server_command();
    command
        .env("ANDROID_MCP_ADB", override_adb)
        .env("ANDROID_SDK_ROOT", &sdk)
        .env("ANDROID_HOME", sdk);

    assert_command_failure(command, "ADB startup check failed").await?;
    Ok(())
}

#[tokio::test]
async fn all_failed_candidates_are_reported() -> TestResult {
    let directory = tempfile::tempdir()?;
    let sdk = sdk_directory(directory.path(), "sdk", "version_failure")?;
    let home = directory.path().join("missing sdk");
    let mut command = server_command();
    command
        .env("ANDROID_SDK_ROOT", &sdk)
        .env("ANDROID_HOME", &home);

    let diagnostic = assert_command_failure(command, "No working ADB executable found").await?;
    assert!(diagnostic.contains("adb: Cannot execute ADB"));
    assert!(
        diagnostic.contains(
            &sdk.join("platform-tools")
                .join(adb_executable_name())
                .display()
                .to_string()
        )
    );
    assert!(
        diagnostic.contains(
            &home
                .join("platform-tools")
                .join(adb_executable_name())
                .display()
                .to_string()
        )
    );
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
