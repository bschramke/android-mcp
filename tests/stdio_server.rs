#![cfg(unix)]

mod support;

use rmcp::{model::CallToolRequestParams, service::ServiceError};
use support::adb::connect;
use support::{TIMEOUT, TestResult, disconnect};
use tokio::time::timeout;

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
    assert_eq!(tools.len(), 16);
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

    let apps_tool = tools
        .iter()
        .find(|tool| tool.name == "adb_list_apps")
        .ok_or("Missing app-list tool")?;
    assert_eq!(
        apps_tool.description.as_deref(),
        Some("List all installed apps on the device")
    );
    assert_eq!(
        apps_tool.input_schema.get("required"),
        Some(&serde_json::json!(["serial"]))
    );

    let app_info_tool = tools
        .iter()
        .find(|tool| tool.name == "adb_app_info")
        .ok_or("Missing app-info tool")?;
    assert_eq!(
        app_info_tool.description.as_deref(),
        Some("Get details for an installed app on the device")
    );
    assert_eq!(
        app_info_tool.input_schema.get("required"),
        Some(&serde_json::json!(["serial", "package"]))
    );
    assert_eq!(
        app_info_tool.input_schema["properties"]["serial"]["type"],
        "string"
    );
    assert_eq!(
        app_info_tool.input_schema["properties"]["package"]["type"],
        "string"
    );

    let foreground_tool = tools
        .iter()
        .find(|tool| tool.name == "adb_get_foreground_app")
        .ok_or("Missing foreground-app tool")?;
    assert_eq!(
        foreground_tool.description.as_deref(),
        Some("Get the app currently in the foreground")
    );
    assert_eq!(
        foreground_tool.input_schema.get("required"),
        Some(&serde_json::json!(["serial"]))
    );
    assert_eq!(
        foreground_tool.input_schema["properties"]["serial"]["type"],
        "string"
    );

    let install_tool = tools
        .iter()
        .find(|tool| tool.name == "adb_install_app")
        .ok_or("Missing app-install tool")?;
    assert_eq!(
        install_tool.description.as_deref(),
        Some("Install an app from file (.apk)")
    );
    assert_eq!(
        install_tool.input_schema.get("required"),
        Some(&serde_json::json!(["serial", "apk_path"]))
    );
    assert_eq!(
        install_tool.input_schema["properties"]["replace"]["type"],
        "boolean"
    );
    assert_eq!(
        install_tool.input_schema["properties"]["downgrade"]["type"],
        "boolean"
    );

    let uninstall_tool = tools
        .iter()
        .find(|tool| tool.name == "adb_uninstall_app")
        .ok_or("Missing app-uninstall tool")?;
    assert_eq!(
        uninstall_tool.description.as_deref(),
        Some("Uninstall an app by its package name or bundle ID")
    );
    assert_eq!(
        uninstall_tool.input_schema.get("required"),
        Some(&serde_json::json!(["serial", "package"]))
    );
    assert_eq!(
        apps_tool.input_schema["properties"]["serial"]["type"],
        "string"
    );
    assert_eq!(
        apps_tool.input_schema["properties"]["exclude_system"]["type"],
        "boolean"
    );
    assert_eq!(
        apps_tool.input_schema.get("required"),
        Some(&serde_json::json!(["serial"]))
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

    let ui_elements_tool = tools
        .iter()
        .find(|tool| tool.name == "adb_list_elements_on_screen")
        .ok_or("Missing UI-elements tool")?;
    assert_eq!(
        ui_elements_tool.description.as_deref(),
        Some("List UI elements with their coordinates and properties")
    );
    assert_eq!(
        ui_elements_tool.input_schema.get("required"),
        Some(&serde_json::json!(["serial"]))
    );

    let hierarchy_tool = tools
        .iter()
        .find(|tool| tool.name == "adb_get_screen_hierarchy")
        .ok_or("Missing screen-hierarchy tool")?;
    assert_eq!(
        hierarchy_tool.input_schema.get("required"),
        Some(&serde_json::json!(["serial"]))
    );

    let start_recording_tool = tools
        .iter()
        .find(|tool| tool.name == "adb_start_screen_recording")
        .ok_or("Missing start-recording tool")?;
    assert_eq!(
        start_recording_tool.description.as_deref(),
        Some("Start recording the device screen to a video file")
    );
    assert_eq!(
        start_recording_tool.input_schema.get("required"),
        Some(&serde_json::json!(["serial", "path"]))
    );
    assert_eq!(
        start_recording_tool.input_schema["properties"]["overwrite"]["type"],
        "boolean"
    );
    assert!(
        start_recording_tool.input_schema["required"]
            .as_array()
            .unwrap()
            .iter()
            .all(|required| required != "overwrite")
    );

    let stop_recording_tool = tools
        .iter()
        .find(|tool| tool.name == "adb_stop_screen_recording")
        .ok_or("Missing stop-recording tool")?;
    assert_eq!(
        stop_recording_tool.description.as_deref(),
        Some("Stop the active screen recording and save the video")
    );
    assert_eq!(
        stop_recording_tool.input_schema.get("required"),
        Some(&serde_json::json!(["serial"]))
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
