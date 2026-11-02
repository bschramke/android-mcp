#![cfg(unix)]

mod support;

use std::{path::PathBuf, sync::OnceLock};

use rmcp::{model::CallToolRequestParams, service::ServiceError};
use support::{Client, TIMEOUT, TestResult, connect_command, disconnect, server_command};
use tokio::{process::Child, time::timeout};

fn adb_fixture() -> TestResult<PathBuf> {
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

async fn connect(mode: &str) -> TestResult<(Client, Child)> {
    let mut command = server_command();
    command
        .env("ANDROID_MCP_ADB", adb_fixture()?)
        .env("ANDROID_MCP_TEST_ADB_MODE", mode);
    connect_command(command).await
}

#[tokio::test]
async fn initializes_and_discovers_adb_list_devices_tool() -> TestResult {
    let (client, child) = connect("empty").await?;
    let info = client
        .peer_info()
        .ok_or("Missing server initialization result")?;
    assert!(
        info.capabilities.tools.is_some(),
        "Server must advertise tools"
    );

    let tools = timeout(TIMEOUT, client.list_all_tools()).await??;
    assert_eq!(tools.len(), 1);
    let tool = &tools[0];
    assert_eq!(tool.name, "adb_list_devices");
    assert_eq!(
        tool.description.as_deref(),
        Some(
            "List Android devices visible to the local ADB server, including offline and unauthorized devices"
        )
    );
    assert_eq!(
        tool.input_schema
            .get("type")
            .and_then(|value| value.as_str()),
        Some("object")
    );
    assert!(
        tool.input_schema
            .get("properties")
            .and_then(|value| value.as_object())
            .is_some_and(|properties| properties.is_empty())
    );
    assert!(
        tool.input_schema
            .get("required")
            .is_none_or(|value| value.as_array().is_some_and(|required| required.is_empty()))
    );

    disconnect(client, child).await
}

#[tokio::test]
async fn adb_list_devices_returns_device_states_and_metadata() -> TestResult {
    let (client, child) = connect("devices").await?;
    let result = timeout(
        TIMEOUT,
        client.call_tool(CallToolRequestParams::new("adb_list_devices")),
    )
    .await??;

    assert_ne!(result.is_error, Some(true));
    assert_eq!(result.content.len(), 1);
    let content = result.content[0].as_text().ok_or("Expected text content")?;
    let devices: serde_json::Value = serde_json::from_str(&content.text)?;
    assert_eq!(
        devices,
        serde_json::json!({"devices": [
            {"serial": "emulator-5554", "status": "online", "adb_state": "device", "model": "Pixel_8", "product": "sdk", "device": "emu", "transport_id": "1"},
            {"serial": "phone", "status": "unauthorized", "adb_state": "unauthorized", "model": null, "product": null, "device": null, "transport_id": null},
            {"serial": "other", "status": "offline", "adb_state": "offline", "model": null, "product": null, "device": null, "transport_id": null}
        ]})
    );

    disconnect(client, child).await
}

#[tokio::test]
async fn unknown_tool_returns_protocol_error_without_stopping_server() -> TestResult {
    let (client, child) = connect("empty").await?;
    let result = timeout(
        TIMEOUT,
        client.call_tool(CallToolRequestParams::new("unknown_tool")),
    )
    .await?;
    assert!(
        matches!(result, Err(ServiceError::McpError(_))),
        "Expected an MCP protocol error, got {result:?}"
    );

    let listing = timeout(
        TIMEOUT,
        client.call_tool(CallToolRequestParams::new("adb_list_devices")),
    )
    .await??;
    assert_ne!(listing.is_error, Some(true));

    disconnect(client, child).await
}

#[tokio::test]
async fn adb_list_devices_returns_empty_list_successfully() -> TestResult {
    let (client, child) = connect("empty").await?;
    let result = timeout(
        TIMEOUT,
        client.call_tool(CallToolRequestParams::new("adb_list_devices")),
    )
    .await??;
    assert_ne!(result.is_error, Some(true));
    assert_eq!(result.content.len(), 1);
    let content = result.content[0].as_text().ok_or("Expected text content")?;
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&content.text)?,
        serde_json::json!({"devices": []})
    );
    disconnect(client, child).await
}

async fn assert_listing_error(mode: &str, message: &str) -> TestResult {
    let (client, child) = connect(mode).await?;
    let result = timeout(
        TIMEOUT,
        client.call_tool(CallToolRequestParams::new("adb_list_devices")),
    )
    .await??;
    assert_eq!(result.is_error, Some(true));
    let content = result.content[0].as_text().ok_or("Expected error text")?;
    assert!(
        content.text.contains(message),
        "Unexpected tool error: {}",
        content.text
    );
    let tools = timeout(TIMEOUT, client.list_all_tools()).await??;
    assert_eq!(tools[0].name, "adb_list_devices");
    disconnect(client, child).await
}

#[tokio::test]
async fn adb_process_failure_is_a_tool_error() -> TestResult {
    assert_listing_error("failure", "ADB fixture cannot contact server").await
}

#[tokio::test]
async fn adb_timeout_is_a_tool_error() -> TestResult {
    assert_listing_error("timeout", "ADB device listing timed out after 5 seconds").await
}

#[tokio::test]
async fn malformed_adb_output_is_a_tool_error() -> TestResult {
    assert_listing_error("malformed", "missing its header").await
}

#[tokio::test]
async fn non_utf8_adb_output_is_a_tool_error() -> TestResult {
    assert_listing_error("invalid_utf8", "not valid UTF-8").await
}

#[tokio::test]
async fn device_listing_reuses_resolved_sdk_executable() -> TestResult {
    let directory = tempfile::tempdir()?;
    let tools = directory.path().join("platform-tools");
    std::fs::create_dir(&tools)?;
    std::os::unix::fs::symlink(adb_fixture()?, tools.join("adb"))?;
    let mut command = server_command();
    command
        .env("ANDROID_SDK_ROOT", directory.path())
        .env("ANDROID_MCP_TEST_ADB_MODE", "empty");
    let (client, child) = connect_command(command).await?;

    let result = timeout(
        TIMEOUT,
        client.call_tool(CallToolRequestParams::new("adb_list_devices")),
    )
    .await??;
    assert_ne!(result.is_error, Some(true));
    let content = result.content[0].as_text().ok_or("Expected text content")?;
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&content.text)?,
        serde_json::json!({"devices": []})
    );
    disconnect(client, child).await
}

#[tokio::test]
async fn adb_removed_after_startup_is_a_tool_error() -> TestResult {
    let directory = tempfile::tempdir()?;
    let executable = directory.path().join("adb");
    std::os::unix::fs::symlink(adb_fixture()?, &executable)?;
    let mut command = server_command();
    command
        .env("ANDROID_MCP_ADB", &executable)
        .env("ANDROID_MCP_TEST_ADB_MODE", "empty");
    let (client, child) = connect_command(command).await?;
    std::fs::remove_file(executable)?;

    let result = timeout(
        TIMEOUT,
        client.call_tool(CallToolRequestParams::new("adb_list_devices")),
    )
    .await??;
    assert_eq!(result.is_error, Some(true));
    let content = result.content[0].as_text().ok_or("Expected error text")?;
    assert!(content.text.contains("Cannot execute ADB device listing"));
    disconnect(client, child).await
}
