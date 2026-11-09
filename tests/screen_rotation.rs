#![cfg(unix)]

mod support;

use rmcp::model::CallToolRequestParams;
use support::adb::adb_fixture;
use support::{Client, TIMEOUT, TestResult, connect_command, disconnect, server_command};
use tokio::time::timeout;

fn rotate_request(direction: &str) -> CallToolRequestParams {
    let mut request = CallToolRequestParams::new("adb_emu_rotate");
    request.arguments = serde_json::json!({"serial":"emulator-5554", "direction":direction})
        .as_object()
        .cloned();
    request
}

async fn run_rotation(
    direction: &str,
    mode: &str,
) -> TestResult<(
    Client,
    tokio::process::Child,
    tempfile::TempDir,
    std::path::PathBuf,
)> {
    let directory = tempfile::tempdir()?;
    let trace = directory.path().join("rotate.trace");
    let mut command = server_command();
    command
        .env("ANDROID_MCP_ADB", adb_fixture()?)
        .env("ANDROID_MCP_TEST_ADB_MODE", mode)
        .env("ANDROID_MCP_TEST_ROTATE_TRACE", &trace);
    let (client, child) = connect_command(command).await?;
    let result = timeout(TIMEOUT, client.call_tool(rotate_request(direction))).await??;
    if mode == "rotate" {
        assert_ne!(result.is_error, Some(true));
    } else {
        assert_eq!(result.is_error, Some(true));
        assert!(
            result.content[0]
                .as_text()
                .ok_or("Expected partial-rotation error")?
                .text
                .contains("after 1 of 3 quarter-turns")
        );
    }
    Ok((client, child, directory, trace))
}

#[tokio::test]
async fn rotates_right_once_and_left_three_times() -> TestResult {
    for (direction, expected_turns) in [("right", 1), ("left", 3)] {
        let (client, child, directory, trace) = run_rotation(direction, "rotate").await?;
        let calls = std::fs::read_to_string(&trace)?;
        assert_eq!(calls.lines().count(), expected_turns, "{direction}");
        disconnect(client, child).await?;
        drop(directory);
    }
    Ok(())
}

#[tokio::test]
async fn reports_partial_left_rotation_when_a_console_call_fails() -> TestResult {
    let (client, child, directory, trace) = run_rotation("left", "rotate_fail_second").await?;
    let calls = std::fs::read_to_string(&trace)?;
    assert_eq!(calls.lines().count(), 2);
    disconnect(client, child).await?;
    drop(directory);
    Ok(())
}
