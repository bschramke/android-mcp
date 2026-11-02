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
    assert_eq!(tools.len(), 3);
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
    assert_eq!(properties.len(), 2);
    assert_eq!(properties["serial"]["type"], "string");
    assert_eq!(properties["max_lines"]["default"], 200);
    assert_eq!(properties["max_lines"]["minimum"], 1);
    assert_eq!(properties["max_lines"]["maximum"], 2000);

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
            "boot_completed": true, "is_emulator": true
        })
    );
    assert!(!content.text.contains("never expose this"));
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
