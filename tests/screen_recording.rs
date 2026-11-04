#![cfg(unix)]

mod support;

use rmcp::model::CallToolRequestParams;
use support::adb::adb_fixture;
use support::{Client, TIMEOUT, TestResult, connect_command, disconnect, server_command};
use tokio::{process::Child, time::timeout};

async fn connect_recording() -> TestResult<(Client, Child, tempfile::TempDir)> {
    let trace_directory = tempfile::tempdir()?;
    let trace_path = trace_directory.path().join("recording.trace");
    let mut command = server_command();
    command
        .env("ANDROID_MCP_ADB", adb_fixture()?)
        .env("ANDROID_MCP_TEST_ADB_MODE", "recording")
        .env("ANDROID_MCP_TEST_RECORDING_TRACE", trace_path);
    let (client, child) = connect_command(command).await?;
    Ok((client, child, trace_directory))
}

fn start_screen_recording_request(
    serial: &str,
    path: &std::path::Path,
    overwrite: Option<bool>,
) -> CallToolRequestParams {
    let mut arguments = serde_json::json!({
        "serial": serial,
        "path": path.to_string_lossy()
    });
    if let Some(overwrite) = overwrite {
        arguments["overwrite"] = serde_json::json!(overwrite);
    }
    let mut request = CallToolRequestParams::new("adb_start_screen_recording");
    request.arguments = arguments.as_object().cloned();
    request
}

fn stop_screen_recording_request(serial: &str) -> CallToolRequestParams {
    let mut request = CallToolRequestParams::new("adb_stop_screen_recording");
    request.arguments = serde_json::json!({"serial": serial}).as_object().cloned();
    request
}

#[tokio::test]
async fn screen_recording_start_stop_downloads_and_cleans_up() -> TestResult {
    let (client, child, trace_directory) = connect_recording().await?;
    let output_directory = tempfile::tempdir()?;
    let output_path = output_directory.path().join("capture.mp4");

    let started = timeout(
        TIMEOUT,
        client.call_tool(start_screen_recording_request(
            "emulator-5554",
            &output_path,
            None,
        )),
    )
    .await??;
    assert_ne!(started.is_error, Some(true));

    let duplicate = timeout(
        TIMEOUT,
        client.call_tool(start_screen_recording_request(
            "emulator-5554",
            &output_path,
            None,
        )),
    )
    .await??;
    assert_eq!(duplicate.is_error, Some(true));
    assert!(
        duplicate.content[0]
            .as_text()
            .ok_or("Expected duplicate-start error")?
            .text
            .contains("already active")
    );

    let stopped = timeout(
        TIMEOUT,
        client.call_tool(stop_screen_recording_request("emulator-5554")),
    )
    .await??;
    assert_ne!(stopped.is_error, Some(true));
    assert_eq!(
        std::fs::read(&output_path)?,
        b"\x00\x00\x00\x18ftypisom\x00\x00\x02\x00isomiso2"
    );
    let trace = std::fs::read_to_string(trace_directory.path().join("recording.trace"))?;
    let events: Vec<_> = trace.lines().collect();
    assert_eq!(events.len(), 4);
    assert!(events[0].starts_with("start /sdcard/.android-mcp-recording-"));
    assert_eq!(&events[1..], ["stopped", "pulled", "cleaned"]);
    disconnect(client, child).await
}

#[tokio::test]
async fn screen_recording_preserves_existing_file_without_overwrite() -> TestResult {
    let (client, child, _) = connect_recording().await?;
    let output_directory = tempfile::tempdir()?;
    let output_path = output_directory.path().join("existing.mp4");
    std::fs::write(&output_path, b"keep this file")?;

    let result = timeout(
        TIMEOUT,
        client.call_tool(start_screen_recording_request(
            "emulator-5554",
            &output_path,
            None,
        )),
    )
    .await??;
    assert_eq!(result.is_error, Some(true));
    assert_eq!(std::fs::read(&output_path)?, b"keep this file");
    disconnect(client, child).await
}

#[tokio::test]
async fn screen_recording_replaces_existing_file_when_overwrite_is_true() -> TestResult {
    let (client, child, _trace_directory) = connect_recording().await?;
    let output_directory = tempfile::tempdir()?;
    let output_path = output_directory.path().join("existing.mp4");
    std::fs::write(&output_path, b"replace this file")?;

    let started = timeout(
        TIMEOUT,
        client.call_tool(start_screen_recording_request(
            "emulator-5554",
            &output_path,
            Some(true),
        )),
    )
    .await??;
    assert_ne!(started.is_error, Some(true));
    let stopped = timeout(
        TIMEOUT,
        client.call_tool(stop_screen_recording_request("emulator-5554")),
    )
    .await??;
    assert_ne!(stopped.is_error, Some(true));
    assert_eq!(
        std::fs::read(&output_path)?,
        b"\x00\x00\x00\x18ftypisom\x00\x00\x02\x00isomiso2"
    );
    disconnect(client, child).await
}

#[tokio::test]
async fn screen_recording_stops_and_saves_when_server_closes() -> TestResult {
    let (client, child, _trace_directory) = connect_recording().await?;
    let output_directory = tempfile::tempdir()?;
    let output_path = output_directory.path().join("shutdown.mp4");

    let result = timeout(
        TIMEOUT,
        client.call_tool(start_screen_recording_request(
            "emulator-5554",
            &output_path,
            None,
        )),
    )
    .await??;
    assert_ne!(result.is_error, Some(true));

    disconnect(client, child).await?;
    assert_eq!(
        std::fs::read(&output_path)?,
        b"\x00\x00\x00\x18ftypisom\x00\x00\x02\x00isomiso2"
    );
    Ok(())
}
