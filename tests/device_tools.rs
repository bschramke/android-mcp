#![cfg(unix)]

mod support;

use rmcp::{model::CallToolRequestParams, service::ServiceError};
use support::adb::{adb_fixture, connect};
use support::{TIMEOUT, TestResult, connect_command, disconnect, server_command};
use tokio::time::timeout;

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
