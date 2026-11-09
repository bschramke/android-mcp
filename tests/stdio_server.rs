#![cfg(unix)]

mod support;

use rmcp::{
    model::{CallToolRequestParams, Tool},
    service::ServiceError,
};
use support::adb::connect;
use support::{Client, TIMEOUT, TestResult, disconnect};
use tokio::{process::Child, time::timeout};

const TOOL_NAMES: [&str; 22] = [
    "adb_list_devices",
    "adb_device_info",
    "adb_display_info",
    "adb_health_check",
    "adb_logcat",
    "adb_take_screenshot",
    "adb_save_screenshot",
    "adb_list_elements_on_screen",
    "adb_get_screen_hierarchy",
    "adb_start_screen_recording",
    "adb_stop_screen_recording",
    "adb_list_apps",
    "adb_app_info",
    "adb_get_foreground_app",
    "adb_install_app",
    "adb_uninstall_app",
    "adb_clear_app_data",
    "adb_tap_on_screen",
    "adb_long_press_on_screen",
    "adb_swipe_on_screen",
    "adb_input_text",
    "adb_press_key",
];

async fn discover_tools() -> TestResult<(Client, Child, Vec<Tool>)> {
    let (client, child) = connect("empty").await?;
    let info = client
        .peer_info()
        .ok_or("Missing server initialization result")?;
    assert!(
        info.capabilities.tools.is_some(),
        "Server must advertise tools"
    );
    let tools = timeout(TIMEOUT, client.list_all_tools()).await??;
    Ok((client, child, tools))
}

async fn discover_tool(name: &str) -> TestResult<(Client, Child, Tool)> {
    let (client, child, tools) = discover_tools().await?;
    let tool = tools
        .into_iter()
        .find(|tool| tool.name == name)
        .ok_or_else(|| std::io::Error::other(format!("Missing {name} tool")))?;
    Ok((client, child, tool))
}

#[tokio::test]
async fn initializes_and_discovers_registered_tools() -> TestResult {
    // Given: the MCP server is initialized and its tools are listed.
    let (client, child, tools) = discover_tools().await?;

    // When: the registered tool names are collected.
    // Then: every expected tool is present.
    assert_eq!(tools.len(), TOOL_NAMES.len());
    for name in TOOL_NAMES {
        assert!(tools.iter().any(|tool| tool.name == name), "Missing {name}");
    }
    disconnect(client, child).await
}

#[tokio::test]
async fn exposes_adb_list_devices_schema() -> TestResult {
    // Given: the MCP server is initialized.
    let (client, child, tool) = discover_tool("adb_list_devices").await?;

    // When: its tool schema is inspected.
    let schema = &tool.input_schema;

    // Then: device listing takes no arguments.
    assert_eq!(
        tool.description.as_deref(),
        Some(
            "List Android devices visible to the local ADB server, including offline and unauthorized devices"
        )
    );
    assert_eq!(schema["type"], "object");
    assert!(schema["properties"].as_object().unwrap().is_empty());
    assert!(schema.get("required").is_none());
    disconnect(client, child).await
}

#[tokio::test]
async fn exposes_adb_device_info_schema() -> TestResult {
    // Given: the MCP server is initialized.
    let (client, child, tool) = discover_tool("adb_device_info").await?;

    // When: its tool schema is inspected.
    let schema = &tool.input_schema;

    // Then: serial is the only required string argument.
    assert_eq!(schema.get("required"), Some(&serde_json::json!(["serial"])));
    assert_eq!(schema["properties"]["serial"]["type"], "string");
    disconnect(client, child).await
}

#[tokio::test]
async fn exposes_adb_display_info_schema() -> TestResult {
    // Given: the MCP server is initialized.
    let (client, child, tool) = discover_tool("adb_display_info").await?;

    // When: its tool schema is inspected.
    let schema = &tool.input_schema;

    // Then: serial is the only required string argument.
    assert_eq!(schema.get("required"), Some(&serde_json::json!(["serial"])));
    assert_eq!(schema["properties"]["serial"]["type"], "string");
    disconnect(client, child).await
}

#[tokio::test]
async fn exposes_adb_health_check_schema() -> TestResult {
    // Given: the MCP server is initialized.
    let (client, child, tool) = discover_tool("adb_health_check").await?;

    // When: its tool schema is inspected.
    let schema = &tool.input_schema;

    // Then: serial is optional and the description explains the ADB server side effect.
    assert_eq!(
        schema["properties"]["serial"]["type"],
        serde_json::json!(["string", "null"])
    );
    assert!(schema.get("required").is_none());
    assert!(
        tool.description
            .as_deref()
            .is_some_and(|description| { description.contains("may start the local ADB server") })
    );
    disconnect(client, child).await
}

#[tokio::test]
async fn exposes_adb_list_apps_schema() -> TestResult {
    // Given: the MCP server is initialized.
    let (client, child, tool) = discover_tool("adb_list_apps").await?;

    // When: its tool schema is inspected.
    let schema = &tool.input_schema;

    // Then: serial is required and system exclusion is optional.
    assert_eq!(
        tool.description.as_deref(),
        Some("List all installed apps on the device")
    );
    assert_eq!(schema.get("required"), Some(&serde_json::json!(["serial"])));
    assert_eq!(schema["properties"]["serial"]["type"], "string");
    assert_eq!(schema["properties"]["exclude_system"]["type"], "boolean");
    disconnect(client, child).await
}

#[tokio::test]
async fn exposes_adb_app_info_schema() -> TestResult {
    // Given: the MCP server is initialized.
    let (client, child, tool) = discover_tool("adb_app_info").await?;

    // When: its tool schema is inspected.
    let schema = &tool.input_schema;

    // Then: serial and package are required strings.
    assert_eq!(
        tool.description.as_deref(),
        Some("Get details for an installed app on the device")
    );
    assert_eq!(
        schema.get("required"),
        Some(&serde_json::json!(["serial", "package"]))
    );
    assert_eq!(schema["properties"]["serial"]["type"], "string");
    assert_eq!(schema["properties"]["package"]["type"], "string");
    disconnect(client, child).await
}

#[tokio::test]
async fn exposes_adb_get_foreground_app_schema() -> TestResult {
    // Given: the MCP server is initialized.
    let (client, child, tool) = discover_tool("adb_get_foreground_app").await?;

    // When: its tool schema is inspected.
    let schema = &tool.input_schema;

    // Then: serial is the only required string argument.
    assert_eq!(
        tool.description.as_deref(),
        Some("Get the app currently in the foreground")
    );
    assert_eq!(schema.get("required"), Some(&serde_json::json!(["serial"])));
    assert_eq!(schema["properties"]["serial"]["type"], "string");
    disconnect(client, child).await
}

#[tokio::test]
async fn exposes_adb_install_app_schema() -> TestResult {
    // Given: the MCP server is initialized.
    let (client, child, tool) = discover_tool("adb_install_app").await?;

    // When: its tool schema is inspected.
    let schema = &tool.input_schema;

    // Then: serial and APK path are required; replace and downgrade are optional flags.
    assert_eq!(
        tool.description.as_deref(),
        Some("Install an app from file (.apk)")
    );
    assert_eq!(
        schema.get("required"),
        Some(&serde_json::json!(["serial", "apk_path"]))
    );
    assert_eq!(schema["properties"]["serial"]["type"], "string");
    assert_eq!(schema["properties"]["apk_path"]["type"], "string");
    assert_eq!(schema["properties"]["replace"]["type"], "boolean");
    assert_eq!(schema["properties"]["downgrade"]["type"], "boolean");
    disconnect(client, child).await
}

#[tokio::test]
async fn exposes_adb_uninstall_app_schema() -> TestResult {
    // Given: the MCP server is initialized.
    let (client, child, tool) = discover_tool("adb_uninstall_app").await?;

    // When: its tool schema is inspected.
    let schema = &tool.input_schema;

    // Then: serial and package are required strings.
    assert_eq!(
        tool.description.as_deref(),
        Some("Uninstall an app by its package name or bundle ID")
    );
    assert_eq!(
        schema.get("required"),
        Some(&serde_json::json!(["serial", "package"]))
    );
    assert_eq!(schema["properties"]["serial"]["type"], "string");
    assert_eq!(schema["properties"]["package"]["type"], "string");
    disconnect(client, child).await
}

#[tokio::test]
async fn exposes_adb_clear_app_data_schema() -> TestResult {
    // Given: the MCP server is initialized.
    let (client, child, tool) = discover_tool("adb_clear_app_data").await?;

    // When: its tool schema is inspected.
    let schema = &tool.input_schema;

    // Then: serial and package are required strings and the destructive effect is described.
    assert_eq!(
        tool.description.as_deref(),
        Some("Clear an app's data so it starts fresh")
    );
    assert_eq!(
        schema.get("required"),
        Some(&serde_json::json!(["serial", "package"]))
    );
    assert_eq!(schema["properties"]["serial"]["type"], "string");
    assert_eq!(schema["properties"]["package"]["type"], "string");
    disconnect(client, child).await
}

#[tokio::test]
async fn exposes_adb_tap_on_screen_schema() -> TestResult {
    // Given
    let (client, child, tool) = discover_tool("adb_tap_on_screen").await?;
    // When
    let schema = &tool.input_schema;
    // Then
    assert_eq!(
        tool.description.as_deref(),
        Some("Tap on the screen; optionally send a best-effort double tap")
    );
    assert_eq!(schema["required"], serde_json::json!(["serial", "x", "y"]));
    assert_eq!(schema["properties"]["serial"]["type"], "string");
    for coordinate in ["x", "y"] {
        assert_eq!(schema["properties"][coordinate]["type"], "integer");
        assert_eq!(schema["properties"][coordinate]["minimum"], 0);
        assert_eq!(schema["properties"][coordinate]["maximum"], 2147483647);
    }
    assert_eq!(schema["properties"]["double_tap"]["type"], "boolean");
    assert_eq!(schema["properties"]["double_tap"]["default"], false);
    disconnect(client, child).await
}

#[tokio::test]
async fn exposes_adb_long_press_on_screen_schema() -> TestResult {
    // Given
    let (client, child, tool) = discover_tool("adb_long_press_on_screen").await?;
    // When
    let schema = &tool.input_schema;
    // Then
    assert_eq!(
        tool.description.as_deref(),
        Some("Long press on the screen at the specified coordinates")
    );
    assert_eq!(schema["required"], serde_json::json!(["serial", "x", "y"]));
    assert_eq!(schema["properties"]["duration_ms"]["default"], 1000);
    assert_eq!(schema["properties"]["duration_ms"]["minimum"], 1);
    assert_eq!(schema["properties"]["duration_ms"]["maximum"], 10000);
    disconnect(client, child).await
}

#[tokio::test]
async fn exposes_adb_swipe_on_screen_schema() -> TestResult {
    // Given
    let (client, child, tool) = discover_tool("adb_swipe_on_screen").await?;
    // When
    let schema = &tool.input_schema;
    // Then
    assert_eq!(
        tool.description.as_deref(),
        Some("Swipe between two screen coordinates")
    );
    assert_eq!(
        schema["required"],
        serde_json::json!(["serial", "start_x", "start_y", "end_x", "end_y"])
    );
    for coordinate in ["start_x", "start_y", "end_x", "end_y"] {
        assert_eq!(schema["properties"][coordinate]["type"], "integer");
        assert_eq!(schema["properties"][coordinate]["minimum"], 0);
        assert_eq!(schema["properties"][coordinate]["maximum"], 2147483647);
    }
    assert_eq!(schema["properties"]["duration_ms"]["default"], 300);
    assert_eq!(schema["properties"]["duration_ms"]["minimum"], 1);
    assert_eq!(schema["properties"]["duration_ms"]["maximum"], 10000);
    disconnect(client, child).await
}

#[tokio::test]
async fn exposes_adb_input_text_schema() -> TestResult {
    // Given
    let (client, child, tool) = discover_tool("adb_input_text").await?;
    // When
    let schema = &tool.input_schema;
    // Then
    assert_eq!(
        tool.description.as_deref(),
        Some("Enter printable ASCII text into the currently focused field")
    );
    assert_eq!(schema["required"], serde_json::json!(["serial", "text"]));
    assert_eq!(schema["properties"]["text"]["type"], "string");
    assert_eq!(schema["properties"]["text"]["minLength"], 1);
    assert_eq!(schema["properties"]["text"]["maxLength"], 1024);
    disconnect(client, child).await
}

#[tokio::test]
async fn exposes_adb_press_key_schema() -> TestResult {
    // Given
    let (client, child, tool) = discover_tool("adb_press_key").await?;
    // When
    let schema = &tool.input_schema;
    // Then
    assert_eq!(
        tool.description.as_deref(),
        Some("Press a supported Android key")
    );
    assert_eq!(schema["required"], serde_json::json!(["serial", "keycode"]));
    let reference = schema["properties"]["keycode"]["$ref"]
        .as_str()
        .ok_or("Missing keycode schema reference")?;
    let document = serde_json::to_value(schema)?;
    let keycode = document
        .pointer(
            reference
                .strip_prefix('#')
                .ok_or("Expected local schema reference")?,
        )
        .ok_or("Missing keycode definition")?;
    assert_eq!(keycode["type"], "string");
    assert_eq!(
        keycode["enum"],
        serde_json::json!([
            "HOME",
            "BACK",
            "ENTER",
            "TAB",
            "DEL",
            "FORWARD_DEL",
            "DPAD_UP",
            "DPAD_DOWN",
            "DPAD_LEFT",
            "DPAD_RIGHT",
            "DPAD_CENTER",
            "VOLUME_UP",
            "VOLUME_DOWN"
        ])
    );
    disconnect(client, child).await
}

#[tokio::test]
async fn exposes_adb_logcat_schema() -> TestResult {
    // Given: the MCP server is initialized.
    let (client, child, tool) = discover_tool("adb_logcat").await?;

    // When: its tool schema is inspected.
    let schema = &tool.input_schema;
    let properties = schema
        .get("properties")
        .and_then(|value| value.as_object())
        .ok_or("Missing logcat properties")?;

    // Then: serial, limits, and optional filters match the public schema.
    assert_eq!(schema.get("required"), Some(&serde_json::json!(["serial"])));
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
    disconnect(client, child).await
}

#[tokio::test]
async fn exposes_adb_take_screenshot_schema() -> TestResult {
    // Given: the MCP server is initialized.
    let (client, child, tool) = discover_tool("adb_take_screenshot").await?;

    // When: its tool schema is inspected.
    let schema = &tool.input_schema;

    // Then: the tool has its documented description and required serial.
    assert_eq!(
        tool.description.as_deref(),
        Some("Take a screenshot to understand what's on screen")
    );
    assert_eq!(schema.get("required"), Some(&serde_json::json!(["serial"])));
    disconnect(client, child).await
}

#[tokio::test]
async fn exposes_adb_save_screenshot_schema() -> TestResult {
    // Given: the MCP server is initialized.
    let (client, child, tool) = discover_tool("adb_save_screenshot").await?;

    // When: its tool schema is inspected.
    let schema = &tool.input_schema;

    // Then: serial/path are required and overwrite is an optional boolean.
    assert_eq!(
        tool.description.as_deref(),
        Some("Save a screenshot to a file")
    );
    assert_eq!(
        schema.get("required"),
        Some(&serde_json::json!(["serial", "path"]))
    );
    assert_eq!(schema["properties"]["overwrite"]["type"], "boolean");
    assert!(
        schema["required"]
            .as_array()
            .unwrap()
            .iter()
            .all(|required| required != "overwrite")
    );
    disconnect(client, child).await
}

#[tokio::test]
async fn exposes_adb_list_elements_on_screen_schema() -> TestResult {
    // Given: the MCP server is initialized.
    let (client, child, tool) = discover_tool("adb_list_elements_on_screen").await?;

    // When: its tool schema is inspected.
    let schema = &tool.input_schema;

    // Then: serial is required and the UI-element description is exposed.
    assert_eq!(
        tool.description.as_deref(),
        Some("List UI elements with their coordinates and properties")
    );
    assert_eq!(schema.get("required"), Some(&serde_json::json!(["serial"])));
    disconnect(client, child).await
}

#[tokio::test]
async fn exposes_adb_get_screen_hierarchy_schema() -> TestResult {
    // Given: the MCP server is initialized.
    let (client, child, tool) = discover_tool("adb_get_screen_hierarchy").await?;

    // When: its tool schema is inspected.
    let schema = &tool.input_schema;

    // Then: serial is required.
    assert_eq!(schema.get("required"), Some(&serde_json::json!(["serial"])));
    disconnect(client, child).await
}

#[tokio::test]
async fn exposes_adb_start_screen_recording_schema() -> TestResult {
    // Given: the MCP server is initialized.
    let (client, child, tool) = discover_tool("adb_start_screen_recording").await?;

    // When: its tool schema is inspected.
    let schema = &tool.input_schema;

    // Then: serial/path are required and overwrite is optional.
    assert_eq!(
        tool.description.as_deref(),
        Some("Start recording the device screen to a video file")
    );
    assert_eq!(
        schema.get("required"),
        Some(&serde_json::json!(["serial", "path"]))
    );
    assert_eq!(schema["properties"]["overwrite"]["type"], "boolean");
    assert!(
        schema["required"]
            .as_array()
            .unwrap()
            .iter()
            .all(|required| required != "overwrite")
    );
    disconnect(client, child).await
}

#[tokio::test]
async fn exposes_adb_stop_screen_recording_schema() -> TestResult {
    // Given: the MCP server is initialized.
    let (client, child, tool) = discover_tool("adb_stop_screen_recording").await?;

    // When: its tool schema is inspected.
    let schema = &tool.input_schema;

    // Then: serial is the only required string argument.
    assert_eq!(
        tool.description.as_deref(),
        Some("Stop the active screen recording and save the video")
    );
    assert_eq!(schema.get("required"), Some(&serde_json::json!(["serial"])));
    disconnect(client, child).await
}

#[tokio::test]
async fn unknown_tool_returns_protocol_error_without_stopping_server() -> TestResult {
    // Given: the MCP server is connected.
    let (client, child) = connect("empty").await?;

    // When: an unknown tool is called.
    let result = timeout(
        TIMEOUT,
        client.call_tool(CallToolRequestParams::new("unknown_tool")),
    )
    .await?;

    // Then: the call returns a protocol error and the server remains usable.
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
