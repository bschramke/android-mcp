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
    assert_eq!(tools.len(), 7);
    let tool = tools
        .iter()
        .find(|tool| tool.name == "adb_list_devices")
        .ok_or("Missing list tool")?;
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

    let info_tool = tools
        .iter()
        .find(|tool| tool.name == "adb_device_info")
        .ok_or("Missing info tool")?;
    assert_eq!(
        info_tool.input_schema.get("type"),
        Some(&serde_json::json!("object"))
    );
    assert_eq!(
        info_tool.input_schema.get("required"),
        Some(&serde_json::json!(["serial"]))
    );
    let properties = info_tool
        .input_schema
        .get("properties")
        .and_then(|value| value.as_object())
        .ok_or("Missing properties")?;
    assert_eq!(properties.len(), 1);
    assert_eq!(properties["serial"]["type"], "string");

    let logcat_tool = tools
        .iter()
        .find(|tool| tool.name == "adb_logcat")
        .ok_or("Missing logcat tool")?;
    assert_eq!(
        logcat_tool.input_schema.get("required"),
        Some(&serde_json::json!(["serial"]))
    );
    let properties = logcat_tool
        .input_schema
        .get("properties")
        .and_then(|value| value.as_object())
        .ok_or("Missing logcat properties")?;
    assert_eq!(properties.len(), 6);
    assert_eq!(properties["serial"]["type"], "string");
    assert_eq!(properties["max_lines"]["default"], 200);
    assert_eq!(properties["max_lines"]["minimum"], 1);
    assert_eq!(properties["max_lines"]["maximum"], 2000);
    assert!(properties.contains_key("pid"));
    assert!(properties.contains_key("package"));
    assert_eq!(properties["tags"]["type"], "array");
    assert_eq!(properties["tags"]["items"]["type"], "string");
    assert_eq!(properties["tags"]["maxItems"], 20);
    assert!(properties.contains_key("priority"));

    let health_tool = tools
        .iter()
        .find(|tool| tool.name == "adb_health_check")
        .ok_or("Missing health-check tool")?;
    assert_eq!(
        health_tool.input_schema.get("type"),
        Some(&serde_json::json!("object"))
    );
    let health_properties = health_tool
        .input_schema
        .get("properties")
        .and_then(|value| value.as_object())
        .ok_or("Missing health-check properties")?;
    assert_eq!(health_properties.len(), 1);
    assert!(health_properties.contains_key("serial"));
    assert!(
        health_tool
            .input_schema
            .get("required")
            .is_none_or(|value| value.as_array().is_some_and(|required| required.is_empty()))
    );
    assert!(
        health_tool
            .description
            .as_deref()
            .is_some_and(|description| description.contains("may start the local ADB server"))
    );

    let display_tool = tools
        .iter()
        .find(|tool| tool.name == "adb_display_info")
        .ok_or("Missing display-info tool")?;
    assert_eq!(
        display_tool.input_schema.get("required"),
        Some(&serde_json::json!(["serial"]))
    );
    assert_eq!(
        display_tool.input_schema["properties"]["serial"]["type"],
        "string"
    );

    let screenshot_tool = tools
        .iter()
        .find(|tool| tool.name == "adb_take_screenshot")
        .ok_or("Missing take-screenshot tool")?;
    assert_eq!(
        screenshot_tool.description.as_deref(),
        Some("Take a screenshot to understand what's on screen")
    );
    assert_eq!(
        screenshot_tool.input_schema.get("required"),
        Some(&serde_json::json!(["serial"]))
    );

    let save_screenshot_tool = tools
        .iter()
        .find(|tool| tool.name == "adb_save_screenshot")
        .ok_or("Missing save-screenshot tool")?;
    assert_eq!(
        save_screenshot_tool.description.as_deref(),
        Some("Save a screenshot to a file")
    );
    assert_eq!(
        save_screenshot_tool.input_schema.get("required"),
        Some(&serde_json::json!(["serial", "path"]))
    );
    assert_eq!(
        save_screenshot_tool.input_schema["properties"]["overwrite"]["type"],
        "boolean"
    );
    assert!(
        save_screenshot_tool.input_schema["required"]
            .as_array()
            .unwrap()
            .iter()
            .all(|required| required != "overwrite")
    );

    disconnect(client, child).await
}

fn screenshot_request(serial: &str) -> CallToolRequestParams {
    let mut request = CallToolRequestParams::new("adb_take_screenshot");
    request.arguments = serde_json::json!({"serial": serial}).as_object().cloned();
    request
}

fn save_screenshot_request(
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
    let mut request = CallToolRequestParams::new("adb_save_screenshot");
    request.arguments = arguments.as_object().cloned();
    request
}

#[tokio::test]
async fn take_screenshot_returns_a_png_image_content_block() -> TestResult {
    let (client, child) = connect("screenshot").await?;
    let result = timeout(
        TIMEOUT,
        client.call_tool(screenshot_request("emulator-5554")),
    )
    .await??;
    assert_ne!(result.is_error, Some(true));
    let image = result.content[0]
        .as_image()
        .ok_or("Expected image content")?;
    assert_eq!(image.mime_type, "image/png");
    assert_eq!(
        image.data,
        "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+/r9sAAAAASUVORK5CYII="
    );
    disconnect(client, child).await
}

#[tokio::test]
async fn save_screenshot_defaults_to_no_overwrite_and_allows_explicit_overwrite() -> TestResult {
    let directory = tempfile::tempdir()?;
    let existing_path = directory.path().join("existing.png");
    std::fs::write(&existing_path, b"keep this file")?;
    let new_path = directory.path().join("capture.png");
    let (client, child) = connect("screenshot").await?;

    let result = timeout(
        TIMEOUT,
        client.call_tool(save_screenshot_request(
            "emulator-5554",
            &existing_path,
            None,
        )),
    )
    .await??;
    assert_eq!(result.is_error, Some(true));
    assert_eq!(std::fs::read(&existing_path)?, b"keep this file");

    let result = timeout(
        TIMEOUT,
        client.call_tool(save_screenshot_request(
            "emulator-5554",
            &existing_path,
            Some(true),
        )),
    )
    .await??;
    assert_ne!(result.is_error, Some(true));
    assert_eq!(
        std::fs::read(&existing_path)?,
        base64::Engine::decode(
            &base64::engine::general_purpose::STANDARD,
            "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+/r9sAAAAASUVORK5CYII=",
        )?
    );

    let result = timeout(
        TIMEOUT,
        client.call_tool(save_screenshot_request("emulator-5554", &new_path, None)),
    )
    .await??;
    assert_ne!(result.is_error, Some(true));
    assert!(new_path.exists());
    disconnect(client, child).await
}

#[tokio::test]
async fn screenshot_rejects_invalid_png_adb_failures_and_oversized_output() -> TestResult {
    for (mode, expected) in [
        ("screenshot_invalid", "not a PNG image"),
        ("screenshot_failure", "device offline"),
        ("screenshot_large", "exceeded the 16 MiB limit"),
    ] {
        let (client, child) = connect(mode).await?;
        let result = timeout(
            TIMEOUT,
            client.call_tool(screenshot_request("emulator-5554")),
        )
        .await??;
        assert_eq!(result.is_error, Some(true), "Unexpected result for {mode}");
        assert!(
            result.content[0]
                .as_text()
                .ok_or("Expected screenshot error")?
                .text
                .contains(expected)
        );
        disconnect(client, child).await?;
    }
    Ok(())
}

#[tokio::test]
async fn screenshot_timeout_is_bounded() -> TestResult {
    let (client, child) = connect("screenshot_timeout").await?;
    let result = timeout(
        TIMEOUT,
        client.call_tool(screenshot_request("emulator-5554")),
    )
    .await??;
    assert_eq!(result.is_error, Some(true));
    assert!(
        result.content[0]
            .as_text()
            .ok_or("Expected screenshot timeout")?
            .text
            .contains("ADB screenshot timed out after 5 seconds")
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

fn health_check_request(serial: Option<&str>) -> CallToolRequestParams {
    let mut request = CallToolRequestParams::new("adb_health_check");
    if let Some(serial) = serial {
        request.arguments = serde_json::json!({"serial": serial}).as_object().cloned();
    }
    request
}

#[tokio::test]
async fn health_check_without_serial_skips_device_commands_and_reports_devices() -> TestResult {
    let (client, child) = connect("devices").await?;
    let result = timeout(TIMEOUT, client.call_tool(health_check_request(None))).await??;
    assert_ne!(result.is_error, Some(true));
    let report: serde_json::Value = serde_json::from_str(
        &result.content[0]
            .as_text()
            .ok_or("Expected health-check JSON")?
            .text,
    )?;
    assert_eq!(report["status"], "healthy");
    assert_eq!(report["devices"].as_array().unwrap().len(), 3);
    assert_eq!(report["shell_uid"], serde_json::Value::Null);
    assert_eq!(report["shell_is_root"], serde_json::Value::Null);
    assert_eq!(report["checks"][0]["name"], "adb_binary");
    assert_eq!(report["checks"][0]["status"], "passed");
    assert_eq!(report["checks"][1]["name"], "adb_server");
    assert_eq!(report["checks"][1]["status"], "passed");
    for check in report["checks"].as_array().unwrap().iter().skip(2) {
        assert_eq!(check["status"], "skipped");
    }
    disconnect(client, child).await
}

#[tokio::test]
async fn health_check_online_device_checks_shell_and_reports_non_root_uid() -> TestResult {
    let (client, child) = connect("health_online").await?;
    let result = timeout(
        TIMEOUT,
        client.call_tool(health_check_request(Some("emulator-5554"))),
    )
    .await??;
    assert_ne!(result.is_error, Some(true));
    let report: serde_json::Value = serde_json::from_str(
        &result.content[0]
            .as_text()
            .ok_or("Expected health-check JSON")?
            .text,
    )?;
    assert_eq!(report["status"], "healthy");
    assert_eq!(report["shell_uid"], 2000);
    assert_eq!(report["shell_is_root"], false);
    assert_eq!(report["checks"][2]["status"], "passed");
    assert_eq!(report["checks"][3]["status"], "passed");
    assert_eq!(report["checks"][4]["status"], "passed");
    assert!(
        report["checks"][4]["message"]
            .as_str()
            .unwrap()
            .contains("adb root was not tested")
    );
    disconnect(client, child).await
}

#[tokio::test]
async fn health_check_reports_unauthorized_device_without_running_shell() -> TestResult {
    let (client, child) = connect("devices").await?;
    let result = timeout(
        TIMEOUT,
        client.call_tool(health_check_request(Some("phone"))),
    )
    .await??;
    let report: serde_json::Value = serde_json::from_str(
        &result.content[0]
            .as_text()
            .ok_or("Expected health-check JSON")?
            .text,
    )?;
    assert_eq!(report["status"], "unhealthy");
    assert_eq!(report["checks"][2]["status"], "failed");
    assert_eq!(report["checks"][3]["status"], "failed");
    assert_eq!(report["checks"][4]["status"], "skipped");
    disconnect(client, child).await
}

#[tokio::test]
async fn health_check_reports_offline_and_missing_devices() -> TestResult {
    for serial in ["other", "missing"] {
        let (client, child) = connect("devices").await?;
        let result = timeout(
            TIMEOUT,
            client.call_tool(health_check_request(Some(serial))),
        )
        .await??;
        let report: serde_json::Value = serde_json::from_str(
            &result.content[0]
                .as_text()
                .ok_or("Expected health-check JSON")?
                .text,
        )?;
        assert_eq!(report["status"], "unhealthy");
        assert_eq!(report["checks"][2]["status"], "failed");
        assert_eq!(report["checks"][4]["status"], "skipped");
        disconnect(client, child).await?;
    }
    Ok(())
}

#[tokio::test]
async fn health_check_reports_shell_failures_and_root_uid_without_adb_root() -> TestResult {
    let (client, child) = connect("health_shell_failure").await?;
    let result = timeout(
        TIMEOUT,
        client.call_tool(health_check_request(Some("emulator-5554"))),
    )
    .await??;
    let report: serde_json::Value = serde_json::from_str(
        &result.content[0]
            .as_text()
            .ok_or("Expected health-check JSON")?
            .text,
    )?;
    assert_eq!(report["status"], "unhealthy");
    assert_eq!(report["checks"][4]["status"], "failed");
    assert_eq!(report["shell_uid"], serde_json::Value::Null);
    disconnect(client, child).await?;

    let (client, child) = connect("health_root").await?;
    let result = timeout(
        TIMEOUT,
        client.call_tool(health_check_request(Some("emulator-5554"))),
    )
    .await??;
    let report: serde_json::Value = serde_json::from_str(
        &result.content[0]
            .as_text()
            .ok_or("Expected health-check JSON")?
            .text,
    )?;
    assert_eq!(report["status"], "healthy");
    assert_eq!(report["shell_uid"], 0);
    assert_eq!(report["shell_is_root"], true);
    disconnect(client, child).await
}

#[tokio::test]
async fn health_check_reports_invalid_shell_uid_as_a_check_failure() -> TestResult {
    let (client, child) = connect("health_invalid_uid").await?;
    let result = timeout(
        TIMEOUT,
        client.call_tool(health_check_request(Some("emulator-5554"))),
    )
    .await??;
    let report: serde_json::Value = serde_json::from_str(
        &result.content[0]
            .as_text()
            .ok_or("Expected health-check JSON")?
            .text,
    )?;
    assert_eq!(report["status"], "unhealthy");
    assert_eq!(report["checks"][4]["status"], "failed");
    assert_eq!(report["shell_uid"], serde_json::Value::Null);
    disconnect(client, child).await
}

#[tokio::test]
async fn health_check_reports_adb_server_failure_without_mcp_error() -> TestResult {
    let (client, child) = connect("failure").await?;
    let result = timeout(TIMEOUT, client.call_tool(health_check_request(None))).await??;
    assert_ne!(result.is_error, Some(true));
    let report: serde_json::Value = serde_json::from_str(
        &result.content[0]
            .as_text()
            .ok_or("Expected health-check JSON")?
            .text,
    )?;
    assert_eq!(report["status"], "unhealthy");
    assert_eq!(report["checks"][1]["status"], "failed");
    for check in report["checks"].as_array().unwrap().iter().skip(2) {
        assert_eq!(check["status"], "skipped");
    }
    disconnect(client, child).await
}

#[tokio::test]
async fn health_check_bounds_shell_timeout_and_keeps_server_usable() -> TestResult {
    let (client, child) = connect("health_shell_timeout").await?;
    let result = timeout(
        TIMEOUT,
        client.call_tool(health_check_request(Some("emulator-5554"))),
    )
    .await??;
    assert_ne!(result.is_error, Some(true));
    let report: serde_json::Value = serde_json::from_str(
        &result.content[0]
            .as_text()
            .ok_or("Expected health-check JSON")?
            .text,
    )?;
    assert_eq!(report["status"], "unhealthy");
    assert_eq!(report["checks"][4]["status"], "failed");
    assert!(
        report["checks"][4]["message"]
            .as_str()
            .unwrap()
            .contains("timed out")
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
    assert!(tools.iter().any(|tool| tool.name == "adb_list_devices"));
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

fn device_info_request(serial: &str) -> CallToolRequestParams {
    let mut request = CallToolRequestParams::new("adb_device_info");
    request.arguments = serde_json::json!({"serial": serial}).as_object().cloned();
    request
}

#[tokio::test]
async fn device_info_returns_nested_supported_properties_only() -> TestResult {
    let (client, child) = connect("info").await?;
    let result = timeout(
        TIMEOUT,
        client.call_tool(device_info_request("emulator-5554")),
    )
    .await??;
    assert_ne!(result.is_error, Some(true));
    assert_eq!(result.content.len(), 1);
    let content = result.content[0].as_text().ok_or("Expected text")?;
    let info: serde_json::Value = serde_json::from_str(&content.text)?;
    assert_eq!(
        info,
        serde_json::json!({
            "serial": "emulator-5554",
            "product": {"brand": "example", "manufacturer": "Example Inc", "model": "Example Phone", "device": "example_device", "name": "example_product", "locale": "en-US", "first_api_level": 30},
            "hardware": {"board": "board", "hardware": "hardware", "soc_manufacturer": "soc-vendor", "soc_model": "soc-model", "cpu": {"abi": "arm64-v8a", "abilist": ["arm64-v8a", "armeabi-v7a"], "abilist32": ["armeabi-v7a"], "abilist64": ["arm64-v8a"]}},
            "android": {"release": "15", "api_level": 35, "security_patch": "2026-10-05"},
            "build": {"id": "BUILD.ID", "fingerprint": "brand/product/device:15/BUILD.ID/123:user/release-keys", "incremental": "123", "build_type": "user", "tags": "release-keys"},
            "boot_completed": true, "is_emulator": true,
            "display": {
                "physical_resolution_px": {"width": 1080, "height": 2400},
                "override_resolution_px": {"width": 720, "height": 1600},
                "current_resolution_px": {"width": 1600, "height": 720},
                "density_dpi": 440,
                "size_dp": {"width": 582, "height": 262},
                "rotation_degrees": 90,
                "orientation": "landscape"
            }
        })
    );
    assert!(!content.text.contains("never expose this"));
    disconnect(client, child).await
}

fn display_info_request(serial: &str) -> CallToolRequestParams {
    let mut request = CallToolRequestParams::new("adb_display_info");
    request.arguments = serde_json::json!({"serial": serial}).as_object().cloned();
    request
}

#[tokio::test]
async fn display_info_reports_resolution_density_rotation_and_orientation() -> TestResult {
    let (client, child) = connect("display").await?;
    let result = timeout(
        TIMEOUT,
        client.call_tool(display_info_request("emulator-5554")),
    )
    .await??;
    assert_ne!(result.is_error, Some(true));
    let info: serde_json::Value = serde_json::from_str(
        &result.content[0]
            .as_text()
            .ok_or("Expected display-info JSON")?
            .text,
    )?;
    assert_eq!(info["physical_resolution_px"]["width"], 1080);
    assert_eq!(info["physical_resolution_px"]["height"], 2400);
    assert_eq!(info["override_resolution_px"]["width"], 720);
    assert_eq!(info["current_resolution_px"]["width"], 1600);
    assert_eq!(info["current_resolution_px"]["height"], 720);
    assert_eq!(info["density_dpi"], 440);
    assert_eq!(info["size_dp"]["width"], 582);
    assert_eq!(info["size_dp"]["height"], 262);
    assert_eq!(info["rotation_degrees"], 90);
    assert_eq!(info["orientation"], "landscape");
    disconnect(client, child).await
}

#[tokio::test]
async fn display_info_unknown_output_is_returned_as_null_fields() -> TestResult {
    let (client, child) = connect("display_unknown").await?;
    let result = timeout(
        TIMEOUT,
        client.call_tool(display_info_request("emulator-5554")),
    )
    .await??;
    assert_ne!(result.is_error, Some(true));
    let info: serde_json::Value = serde_json::from_str(
        &result.content[0]
            .as_text()
            .ok_or("Expected display-info JSON")?
            .text,
    )?;
    for field in [
        "physical_resolution_px",
        "override_resolution_px",
        "current_resolution_px",
        "density_dpi",
        "size_dp",
        "rotation_degrees",
        "orientation",
    ] {
        assert!(info[field].is_null(), "Expected {field} to be null");
    }
    disconnect(client, child).await
}

#[tokio::test]
async fn device_info_keeps_properties_when_display_commands_fail() -> TestResult {
    let (client, child) = connect("info_display_failure").await?;
    let result = timeout(
        TIMEOUT,
        client.call_tool(device_info_request("emulator-5554")),
    )
    .await??;
    assert_ne!(result.is_error, Some(true));
    let info: serde_json::Value = serde_json::from_str(
        &result.content[0]
            .as_text()
            .ok_or("Expected device-info JSON")?
            .text,
    )?;
    assert_eq!(info["product"]["model"], "Example Phone");
    assert!(info["display"]["physical_resolution_px"].is_null());
    assert!(info["display"]["orientation"].is_null());
    disconnect(client, child).await
}

#[tokio::test]
async fn display_info_command_failure_is_a_tool_error() -> TestResult {
    let (client, child) = connect("display_failure").await?;
    let result = timeout(
        TIMEOUT,
        client.call_tool(display_info_request("emulator-5554")),
    )
    .await??;
    assert_eq!(result.is_error, Some(true));
    assert!(
        result.content[0]
            .as_text()
            .ok_or("Expected display-info error")?
            .text
            .contains("ADB wm size failed")
    );
    disconnect(client, child).await
}

#[tokio::test]
async fn display_info_timeout_is_bounded() -> TestResult {
    let (client, child) = connect("display_timeout").await?;
    let result = timeout(
        TIMEOUT,
        client.call_tool(display_info_request("emulator-5554")),
    )
    .await??;
    assert_eq!(result.is_error, Some(true));
    assert!(
        result.content[0]
            .as_text()
            .ok_or("Expected display-info timeout")?
            .text
            .contains("ADB wm size timed out after 5 seconds")
    );
    disconnect(client, child).await
}

#[tokio::test]
async fn display_info_passes_serial_as_one_literal_argument() -> TestResult {
    let (client, child) = connect("display").await?;
    let result = timeout(
        TIMEOUT,
        client.call_tool(display_info_request("serial;not-a-shell-command")),
    )
    .await??;
    assert_ne!(result.is_error, Some(true));
    disconnect(client, child).await
}

async fn assert_dump_profile(
    mode: &str,
    model: &str,
    security_patch: &str,
    emulator: serde_json::Value,
) -> TestResult {
    let (client, child) = connect(mode).await?;
    let result = timeout(
        TIMEOUT,
        client.call_tool(device_info_request("emulator-5554")),
    )
    .await??;
    assert_ne!(result.is_error, Some(true));
    let content = result.content[0]
        .as_text()
        .ok_or("Expected device info JSON")?;
    let info: serde_json::Value = serde_json::from_str(&content.text)?;
    assert_eq!(info["product"]["model"], model);
    assert_eq!(info["android"]["release"], "8.1.0");
    assert_eq!(info["android"]["api_level"], 27);
    assert_eq!(info["android"]["security_patch"], security_patch);
    assert_eq!(info["hardware"]["cpu"]["abi"], "arm64-v8a");
    assert_eq!(info["build"]["id"], "TEST.BUILD");
    assert_eq!(info["is_emulator"], emulator);
    assert!(!content.text.contains("redacted"));
    disconnect(client, child).await
}

#[tokio::test]
async fn device_info_reads_anonymized_tc57_fixture() -> TestResult {
    assert_dump_profile("info_tc57", "TC57", "2021-10-05", serde_json::Value::Null).await
}

#[tokio::test]
async fn device_info_reads_anonymized_emulator_fixture() -> TestResult {
    assert_dump_profile(
        "info_emulator",
        "Android SDK built for arm64",
        "2018-01-05",
        serde_json::json!(true),
    )
    .await
}

#[tokio::test]
async fn device_info_missing_properties_are_null_or_empty_lists() -> TestResult {
    let (client, child) = connect("info_partial").await?;
    let result = timeout(
        TIMEOUT,
        client.call_tool(device_info_request("emulator-5554")),
    )
    .await??;
    assert_ne!(result.is_error, Some(true));
    let content = result.content[0].as_text().ok_or("Expected text")?;
    let info: serde_json::Value = serde_json::from_str(&content.text)?;
    assert_eq!(info["product"]["model"], "Partial Phone");
    assert!(info["product"]["first_api_level"].is_null());
    assert!(info["android"]["api_level"].is_null());
    assert_eq!(info["hardware"]["cpu"]["abilist"], serde_json::json!([]));
    assert!(info["boot_completed"].is_null());
    assert!(info["is_emulator"].is_null());
    disconnect(client, child).await
}

async fn assert_info_error(mode: &str, message: &str) -> TestResult {
    let (client, child) = connect(mode).await?;
    let result = timeout(
        TIMEOUT,
        client.call_tool(device_info_request("emulator-5554")),
    )
    .await??;
    assert_eq!(result.is_error, Some(true));
    let content = result.content[0].as_text().ok_or("Expected error text")?;
    assert!(
        content.text.contains(message),
        "Unexpected error: {}",
        content.text
    );
    let tools = timeout(TIMEOUT, client.list_all_tools()).await??;
    assert!(tools.iter().any(|tool| tool.name == "adb_device_info"));
    disconnect(client, child).await
}

#[tokio::test]
async fn unavailable_devices_produce_tool_errors() -> TestResult {
    for mode in ["info_unauthorized", "info_offline", "info_not_found"] {
        assert_info_error(mode, mode).await?;
    }
    Ok(())
}

#[tokio::test]
async fn device_info_timeout_produces_tool_error() -> TestResult {
    assert_info_error("info_timeout", "ADB device info timed out after 5 seconds").await
}

#[tokio::test]
async fn malformed_device_info_produces_tool_error() -> TestResult {
    assert_info_error("info_malformed", "Malformed ADB getprop entry").await
}

#[tokio::test]
async fn non_utf8_device_info_produces_tool_error() -> TestResult {
    assert_info_error("info_invalid_utf8", "not valid UTF-8").await
}

#[tokio::test]
async fn device_info_requires_string_serial_argument() -> TestResult {
    let (client, child) = connect("info").await?;
    for request in [CallToolRequestParams::new("adb_device_info"), {
        let mut request = CallToolRequestParams::new("adb_device_info");
        request.arguments = serde_json::json!({"serial": 123}).as_object().cloned();
        request
    }] {
        let result = timeout(TIMEOUT, client.call_tool(request)).await?;
        match result {
            Ok(result) => {
                assert_eq!(result.is_error, Some(true));
                assert!(
                    result.content[0]
                        .as_text()
                        .ok_or("Expected validation error")?
                        .text
                        .contains("serial")
                );
            }
            Err(ServiceError::McpError(_)) => {}
            Err(error) => return Err(error.into()),
        }
    }
    disconnect(client, child).await
}

#[tokio::test]
async fn device_info_rejects_empty_or_whitespace_serials() -> TestResult {
    let (client, child) = connect("info").await?;
    for serial in ["", " ", "device serial", "device\0serial"] {
        let result = timeout(TIMEOUT, client.call_tool(device_info_request(serial))).await??;
        assert_eq!(result.is_error, Some(true));
        assert!(
            result.content[0]
                .as_text()
                .ok_or("Expected error text")?
                .text
                .contains("Device serial must be nonempty")
        );
    }
    disconnect(client, child).await
}

#[tokio::test]
async fn device_info_passes_serial_as_one_literal_argument() -> TestResult {
    let (client, child) = connect("info").await?;
    let result = timeout(
        TIMEOUT,
        client.call_tool(device_info_request("serial;not-a-shell-command")),
    )
    .await??;
    assert_ne!(result.is_error, Some(true));
    let info: serde_json::Value =
        serde_json::from_str(&result.content[0].as_text().ok_or("Expected text")?.text)?;
    assert_eq!(info["serial"], "serial;not-a-shell-command");
    disconnect(client, child).await
}

fn logcat_request(arguments: serde_json::Value) -> CallToolRequestParams {
    let mut request = CallToolRequestParams::new("adb_logcat");
    request.arguments = arguments.as_object().cloned();
    request
}

#[tokio::test]
async fn logcat_defaults_to_200_lines_and_returns_only_stdout() -> TestResult {
    let (client, child) = connect("logcat").await?;
    let result = timeout(
        TIMEOUT,
        client.call_tool(logcat_request(
            serde_json::json!({"serial": "emulator-5554"}),
        )),
    )
    .await??;
    assert_ne!(result.is_error, Some(true));
    assert_eq!(result.content.len(), 1);
    let snapshot: serde_json::Value = serde_json::from_str(
        &result.content[0]
            .as_text()
            .ok_or("Expected snapshot JSON")?
            .text,
    )?;
    assert_eq!(
        snapshot,
        serde_json::json!({
            "serial": "emulator-5554", "format": "threadtime",
            "output": include_str!("fixtures/logcat/threadtime.txt"), "truncated": false
        })
    );
    disconnect(client, child).await
}

#[tokio::test]
async fn logcat_passes_explicit_limit_and_literal_serial() -> TestResult {
    let (client, child) = connect("logcat_explicit").await?;
    let result = timeout(
        TIMEOUT,
        client.call_tool(logcat_request(
            serde_json::json!({"serial": "serial;not-a-shell-command", "max_lines": 17}),
        )),
    )
    .await??;
    assert_ne!(result.is_error, Some(true));
    let snapshot: serde_json::Value =
        serde_json::from_str(&result.content[0].as_text().ok_or("Expected JSON")?.text)?;
    assert_eq!(snapshot["serial"], "serial;not-a-shell-command");
    disconnect(client, child).await
}

#[tokio::test]
async fn logcat_accepts_minimum_and_maximum_line_limits() -> TestResult {
    let (client, child) = connect("logcat_bounds").await?;
    for max_lines in [1, 2000] {
        let result = timeout(
            TIMEOUT,
            client.call_tool(logcat_request(
                serde_json::json!({"serial": "emulator-5554", "max_lines": max_lines}),
            )),
        )
        .await??;
        assert_ne!(result.is_error, Some(true));
    }
    disconnect(client, child).await
}

#[tokio::test]
async fn logcat_rejects_invalid_serials_and_line_limits() -> TestResult {
    let (client, child) = connect("logcat").await?;
    for arguments in [
        serde_json::json!({}),
        serde_json::json!({"serial": 123}),
        serde_json::json!({"serial": ""}),
        serde_json::json!({"serial": "device serial"}),
        serde_json::json!({"serial": "device\u{0000}serial"}),
        serde_json::json!({"serial": "emulator-5554", "max_lines": 0}),
        serde_json::json!({"serial": "emulator-5554", "max_lines": 2001}),
        serde_json::json!({"serial": "emulator-5554", "max_lines": -1}),
        serde_json::json!({"serial": "emulator-5554", "max_lines": 1.5}),
        serde_json::json!({"serial": "emulator-5554", "max_lines": "200"}),
        serde_json::json!({"serial": "emulator-5554", "max_lines": null}),
    ] {
        let result = timeout(TIMEOUT, client.call_tool(logcat_request(arguments))).await?;
        match result {
            Ok(result) => assert_eq!(result.is_error, Some(true)),
            Err(ServiceError::McpError(_)) => {}
            Err(error) => return Err(error.into()),
        }
    }
    disconnect(client, child).await
}

#[tokio::test]
async fn empty_logcat_is_a_successful_empty_snapshot() -> TestResult {
    let (client, child) = connect("logcat_empty").await?;
    let result = timeout(
        TIMEOUT,
        client.call_tool(logcat_request(
            serde_json::json!({"serial": "emulator-5554"}),
        )),
    )
    .await??;
    assert_ne!(result.is_error, Some(true));
    let snapshot: serde_json::Value =
        serde_json::from_str(&result.content[0].as_text().ok_or("Expected JSON")?.text)?;
    assert_eq!(snapshot["output"], "");
    assert_eq!(snapshot["truncated"], false);
    disconnect(client, child).await
}

#[tokio::test]
async fn oversized_logcat_is_capped_and_server_remains_usable() -> TestResult {
    let (client, child) = connect("logcat_large").await?;
    let result = timeout(
        TIMEOUT,
        client.call_tool(logcat_request(
            serde_json::json!({"serial": "emulator-5554"}),
        )),
    )
    .await??;
    assert_ne!(result.is_error, Some(true));
    let snapshot: serde_json::Value =
        serde_json::from_str(&result.content[0].as_text().ok_or("Expected JSON")?.text)?;
    assert_eq!(snapshot["truncated"], true);
    assert_eq!(
        snapshot["output"].as_str().ok_or("Missing output")?.len(),
        256 * 1024
    );
    assert!(
        timeout(TIMEOUT, client.list_all_tools())
            .await??
            .iter()
            .any(|tool| tool.name == "adb_logcat")
    );
    disconnect(client, child).await
}

#[tokio::test]
async fn oversized_logcat_from_an_exited_process_is_still_truncated_successfully() -> TestResult {
    let (client, child) = connect("logcat_large_exit").await?;
    let result = timeout(
        TIMEOUT,
        client.call_tool(logcat_request(
            serde_json::json!({"serial": "emulator-5554"}),
        )),
    )
    .await??;
    assert_ne!(result.is_error, Some(true));
    let snapshot: serde_json::Value = serde_json::from_str(
        &result.content[0]
            .as_text()
            .ok_or("Expected snapshot JSON")?
            .text,
    )?;
    assert_eq!(snapshot["truncated"], true);
    assert_eq!(snapshot["output"].as_str().unwrap().len(), 256 * 1024);
    disconnect(client, child).await
}

async fn assert_logcat_error(mode: &str, message: &str) -> TestResult {
    let (client, child) = connect(mode).await?;
    let result = timeout(
        TIMEOUT,
        client.call_tool(logcat_request(
            serde_json::json!({"serial": "emulator-5554"}),
        )),
    )
    .await??;
    assert_eq!(result.is_error, Some(true));
    assert!(
        result.content[0]
            .as_text()
            .ok_or("Expected error text")?
            .text
            .contains(message)
    );
    assert!(
        timeout(TIMEOUT, client.list_all_tools())
            .await??
            .iter()
            .any(|tool| tool.name == "adb_logcat")
    );
    disconnect(client, child).await
}

#[tokio::test]
async fn logcat_process_failure_is_a_tool_error() -> TestResult {
    assert_logcat_error("logcat_failure", "device offline").await
}

#[tokio::test]
async fn logcat_timeout_is_a_tool_error() -> TestResult {
    assert_logcat_error("logcat_timeout", "ADB logcat timed out after 5 seconds").await
}

#[tokio::test]
async fn excessive_logcat_stderr_is_bounded() -> TestResult {
    assert_logcat_error("logcat_stderr_large", "stderr exceeded the 16 KiB limit").await
}

#[tokio::test]
async fn non_utf8_logcat_is_a_tool_error() -> TestResult {
    assert_logcat_error("logcat_invalid_utf8", "not valid UTF-8").await
}

#[tokio::test]
async fn logcat_filters_are_passed_as_exact_native_arguments() -> TestResult {
    for (mode, filters) in [
        ("logcat_pid", serde_json::json!({"pid": 1000})),
        ("logcat_pid_max", serde_json::json!({"pid": 2147483647u32})),
        (
            "logcat_tags",
            serde_json::json!({"tags": ["ExampleApp", "AudioController"]}),
        ),
        (
            "logcat_pid_tags",
            serde_json::json!({"pid": 1000, "tags": ["ExampleApp"]}),
        ),
        (
            "logcat_pid_priority",
            serde_json::json!({"pid": 1000, "priority": "error"}),
        ),
        (
            "logcat_tags_priority",
            serde_json::json!({"tags": ["ExampleApp", "AudioController"], "priority": "warn"}),
        ),
        (
            "logcat_combined",
            serde_json::json!({"pid": 2210, "tags": ["ErrorProcessor", "AudioController"], "priority": "warn"}),
        ),
    ] {
        let (client, child) = connect(mode).await?;
        let mut arguments = filters
            .as_object()
            .cloned()
            .ok_or("Expected filter object")?;
        arguments.insert("serial".into(), serde_json::json!("emulator-5554"));
        let result = timeout(TIMEOUT, client.call_tool(logcat_request(arguments.into()))).await??;
        assert_ne!(result.is_error, Some(true), "Filter mode failed: {mode}");
        let snapshot: serde_json::Value =
            serde_json::from_str(&result.content[0].as_text().ok_or("Expected JSON")?.text)?;
        assert_eq!(
            snapshot,
            serde_json::json!({"serial": "emulator-5554", "format": "threadtime", "output": include_str!("fixtures/logcat/threadtime.txt"), "truncated": false})
        );
        disconnect(client, child).await?;
    }
    Ok(())
}

#[tokio::test]
async fn logcat_supports_every_minimum_priority() -> TestResult {
    for priority in ["verbose", "debug", "info", "warn", "error", "fatal"] {
        let (client, child) = connect(&format!("logcat_{priority}")).await?;
        let result = timeout(
            TIMEOUT,
            client.call_tool(logcat_request(
                serde_json::json!({"serial": "emulator-5554", "priority": priority}),
            )),
        )
        .await??;
        assert_ne!(result.is_error, Some(true), "Priority failed: {priority}");
        disconnect(client, child).await?;
    }
    Ok(())
}

#[tokio::test]
async fn empty_or_null_optional_filters_do_not_change_logcat_arguments() -> TestResult {
    let (client, child) = connect("logcat").await?;
    let result = timeout(TIMEOUT, client.call_tool(logcat_request(serde_json::json!({"serial": "emulator-5554", "pid": null, "tags": [], "priority": null})))).await??;
    assert_ne!(result.is_error, Some(true));
    disconnect(client, child).await
}

#[tokio::test]
async fn logcat_rejects_invalid_filter_values_without_losing_connection() -> TestResult {
    let (client, child) = connect("logcat").await?;
    let mut invalid = vec![
        serde_json::json!({"pid": 0}),
        serde_json::json!({"pid": -1}),
        serde_json::json!({"pid": 2147483648u32}),
        serde_json::json!({"pid": "123"}),
        serde_json::json!({"pid": 1.5}),
        serde_json::json!({"priority": "W"}),
        serde_json::json!({"priority": "silent"}),
        serde_json::json!({"priority": "unknown"}),
        serde_json::json!({"priority": 3}),
        serde_json::json!({"tags": "ExampleApp"}),
        serde_json::json!({"tags": null}),
        serde_json::json!({"tags": [123]}),
        serde_json::json!({"tags": ["Tag".repeat(43)]}),
        serde_json::json!({"tags": vec!["Tag"; 21]}),
    ];
    for tag in [
        "",
        "*",
        "Example:W",
        "--pid=123",
        "tag with space",
        "line\nbreak",
        "a\0b",
        "tag;id",
        "$(id)",
        "tag|id",
        "tag&id",
        "`id`",
        "a'b",
        "a\"b",
    ] {
        invalid.push(serde_json::json!({"tags": [tag]}));
    }
    for filters in invalid {
        let mut arguments = filters.as_object().cloned().ok_or("Expected filters")?;
        arguments.insert("serial".into(), serde_json::json!("emulator-5554"));
        let result = timeout(TIMEOUT, client.call_tool(logcat_request(arguments.into()))).await?;
        match result {
            Ok(result) => assert_eq!(result.is_error, Some(true)),
            Err(ServiceError::McpError(_)) => {}
            Err(error) => return Err(error.into()),
        }
    }
    let result = timeout(
        TIMEOUT,
        client.call_tool(logcat_request(
            serde_json::json!({"serial": "emulator-5554"}),
        )),
    )
    .await??;
    assert_ne!(result.is_error, Some(true));
    disconnect(client, child).await
}

async fn connect_package(mode: &str) -> TestResult<(Client, Child, tempfile::TempDir)> {
    let directory = tempfile::tempdir()?;
    let mut command = server_command();
    command
        .env("ANDROID_MCP_ADB", adb_fixture()?)
        .env("ANDROID_MCP_TEST_ADB_MODE", mode)
        .env(
            "ANDROID_MCP_TEST_PACKAGE_TRACE",
            directory.path().join("trace"),
        );
    let (client, child) = connect_command(command).await?;
    Ok((client, child, directory))
}

fn package_request() -> CallToolRequestParams {
    logcat_request(
        serde_json::json!({"serial": "emulator-5554", "package": "com.github.bschramke.myfinance"}),
    )
}

#[tokio::test]
async fn package_logcat_resolves_the_selected_devices_pid() -> TestResult {
    let (client, child, directory) = connect_package("logcat_package").await?;
    let result = timeout(TIMEOUT, client.call_tool(package_request())).await??;
    assert_ne!(result.is_error, Some(true));
    let snapshot: serde_json::Value = serde_json::from_str(
        &result.content[0]
            .as_text()
            .ok_or("Expected snapshot JSON")?
            .text,
    )?;
    assert_eq!(
        snapshot,
        serde_json::json!({"serial": "emulator-5554", "format": "threadtime", "output": include_str!("fixtures/logcat/threadtime.txt"), "truncated": false})
    );
    assert_eq!(
        std::fs::read_to_string(directory.path().join("trace"))?,
        "pidof\nlogcat\n"
    );
    disconnect(client, child).await
}

#[tokio::test]
async fn package_logcat_combines_tags_and_priority() -> TestResult {
    let (client, child, _directory) = connect_package("logcat_package_combined").await?;
    let result = timeout(TIMEOUT, client.call_tool(logcat_request(serde_json::json!({"serial": "emulator-5554", "package": "com.github.bschramke.myfinance", "tags": ["ExampleApp", "AudioController"], "priority": "warn"})))).await??;
    assert_ne!(result.is_error, Some(true));
    disconnect(client, child).await
}

#[tokio::test]
async fn package_pid_is_resolved_again_for_every_snapshot() -> TestResult {
    let (client, child, directory) = connect_package("logcat_package_fresh").await?;
    for _ in 0..2 {
        let result = timeout(TIMEOUT, client.call_tool(package_request())).await??;
        assert_ne!(result.is_error, Some(true));
    }
    assert_eq!(
        std::fs::read_to_string(directory.path().join("trace"))?,
        "pidof\nlogcat\npidof\nlogcat\n"
    );
    disconnect(client, child).await
}

#[tokio::test]
async fn package_lookup_failures_never_fall_back_to_unfiltered_logs() -> TestResult {
    for (mode, message) in [
        ("logcat_package_absent", "No running main process"),
        ("logcat_package_empty", "No running main process"),
        ("logcat_package_offline", "device offline"),
        ("logcat_package_unsupported", "pidof: not found"),
        ("logcat_package_invalid", "not a valid process ID"),
        ("logcat_package_zero", "between 1 and 2147483647"),
        ("logcat_package_multiple", "Multiple main-process PIDs"),
        ("logcat_package_invalid_utf8", "not valid UTF-8"),
        (
            "logcat_package_stdout_large",
            "stdout exceeded the 4 KiB limit",
        ),
        (
            "logcat_package_stderr_large",
            "stderr exceeded the 16 KiB limit",
        ),
    ] {
        let (client, child, directory) = connect_package(mode).await?;
        let result = timeout(TIMEOUT, client.call_tool(package_request())).await??;
        assert_eq!(result.is_error, Some(true), "Unexpected success for {mode}");
        let message_text = &result.content[0]
            .as_text()
            .ok_or("Expected error text")?
            .text;
        assert!(
            message_text.contains(message),
            "Unexpected error for {mode}: {message_text}"
        );
        assert_eq!(
            std::fs::read_to_string(directory.path().join("trace"))?,
            "pidof\n"
        );
        assert!(
            timeout(TIMEOUT, client.list_all_tools())
                .await??
                .iter()
                .any(|tool| tool.name == "adb_logcat")
        );
        disconnect(client, child).await?;
    }
    Ok(())
}

#[tokio::test]
async fn stalled_package_lookup_is_bounded_and_does_not_run_logcat() -> TestResult {
    let (client, child, directory) = connect_package("logcat_package_timeout").await?;
    let result = timeout(TIMEOUT, client.call_tool(package_request())).await??;
    assert_eq!(result.is_error, Some(true));
    assert!(
        result.content[0]
            .as_text()
            .ok_or("Expected error")?
            .text
            .contains("during package PID lookup")
    );
    assert_eq!(
        std::fs::read_to_string(directory.path().join("trace"))?,
        "pidof\n"
    );
    disconnect(client, child).await
}

#[tokio::test]
async fn package_lookup_and_logcat_share_one_time_budget() -> TestResult {
    let (client, child, directory) = connect_package("logcat_package_budget").await?;
    let result = timeout(TIMEOUT, client.call_tool(package_request())).await??;
    assert_eq!(result.is_error, Some(true));
    assert!(
        result.content[0]
            .as_text()
            .ok_or("Expected timeout")?
            .text
            .contains("ADB logcat timed out after 5 seconds")
    );
    assert_eq!(
        std::fs::read_to_string(directory.path().join("trace"))?,
        "pidof\nlogcat\n"
    );
    disconnect(client, child).await
}

#[tokio::test]
async fn invalid_packages_and_pid_conflicts_are_rejected_before_pidof() -> TestResult {
    let (client, child, directory) = connect_package("logcat_package").await?;
    let mut invalid = vec![
        serde_json::json!({"package": "com.github.bschramke.myfinance", "pid": 1000}),
        serde_json::json!({"package": 123}),
        serde_json::json!({"package": "com.example", "tags": ["*:V"]}),
        serde_json::json!({"package": format!("com.{}", "a".repeat(252))}),
    ];
    for package in [
        "",
        "android",
        ".com.example",
        "com..example",
        "com.example.",
        "com.example:worker",
        "com.1example",
        "com._example",
        "com.example;id",
        "com.example$(id)",
        "com.example app",
        "com.example\napp",
        "com.example\u{0000}app",
        "com.éxample",
        "--help",
        "com.example/app",
        "com.example*",
    ] {
        invalid.push(serde_json::json!({"package": package}));
    }
    for arguments in invalid {
        let mut arguments = arguments.as_object().cloned().ok_or("Expected arguments")?;
        arguments.insert("serial".into(), serde_json::json!("emulator-5554"));
        let result = timeout(TIMEOUT, client.call_tool(logcat_request(arguments.into()))).await?;
        match result {
            Ok(result) => assert_eq!(result.is_error, Some(true)),
            Err(ServiceError::McpError(_)) => {}
            Err(error) => return Err(error.into()),
        }
    }
    assert!(!directory.path().join("trace").exists());
    disconnect(client, child).await
}

#[tokio::test]
async fn null_package_preserves_the_original_logcat_command() -> TestResult {
    let (client, child) = connect("logcat").await?;
    let result = timeout(
        TIMEOUT,
        client.call_tool(logcat_request(
            serde_json::json!({"serial": "emulator-5554", "package": null}),
        )),
    )
    .await??;
    assert_ne!(result.is_error, Some(true));
    disconnect(client, child).await
}
