mod support;

use rmcp::{model::CallToolRequestParams, service::ServiceError};
use support::adb::adb_fixture;
use support::{Client, TIMEOUT, TestResult, connect_command, disconnect, server_command};
use tokio::{
    process::Child,
    time::{Duration, timeout},
};

async fn connect_input(mode: &str, expected: &str) -> TestResult<(Client, Child)> {
    let mut command = server_command();
    command
        .env("ANDROID_MCP_ADB", adb_fixture()?)
        .env("ANDROID_MCP_TEST_ADB_MODE", mode)
        .env("ANDROID_MCP_TEST_INPUT_COMMAND", expected);
    connect_command(command).await
}

fn request(name: &str, arguments: serde_json::Value) -> CallToolRequestParams {
    let mut request = CallToolRequestParams::new(name.to_owned());
    request.arguments = arguments.as_object().cloned();
    request
}

async fn assert_input(name: &str, arguments: serde_json::Value, expected: &str) -> TestResult {
    // Given
    let (client, child) = connect_input("input", expected).await?;
    // When
    let result = timeout(TIMEOUT, client.call_tool(request(name, arguments))).await??;
    // Then
    assert_ne!(result.is_error, Some(true), "{name}: {result:?}");
    assert_eq!(
        result.content[0].as_text().unwrap().text,
        "Input command completed"
    );
    disconnect(client, child).await
}

#[tokio::test]
async fn tap_defaults_to_single_and_supports_double_tap() -> TestResult {
    assert_input(
        "adb_tap_on_screen",
        serde_json::json!({"serial":"emulator-5554","x":10,"y":20}),
        "input tap 10 20",
    )
    .await?;
    assert_input(
        "adb_tap_on_screen",
        serde_json::json!({"serial":"emulator-5554","x":10,"y":20,"double_tap":false}),
        "input tap 10 20",
    )
    .await?;
    assert_input(
        "adb_tap_on_screen",
        serde_json::json!({"serial":"emulator-5554","x":10,"y":20,"double_tap":true}),
        "input tap 10 20 && input tap 10 20",
    )
    .await
}

#[tokio::test]
async fn long_press_uses_identical_endpoints_and_duration() -> TestResult {
    assert_input(
        "adb_long_press_on_screen",
        serde_json::json!({"serial":"emulator-5554","x":10,"y":20}),
        "input swipe 10 20 10 20 1000",
    )
    .await?;
    assert_input(
        "adb_long_press_on_screen",
        serde_json::json!({"serial":"emulator-5554","x":10,"y":20,"duration_ms":2000}),
        "input swipe 10 20 10 20 2000",
    )
    .await
}

#[tokio::test]
async fn swipe_uses_endpoints_and_duration() -> TestResult {
    assert_input("adb_swipe_on_screen", serde_json::json!({"serial":"emulator-5554","start_x":10,"start_y":20,"end_x":30,"end_y":40}), "input swipe 10 20 30 40 300").await?;
    assert_input("adb_swipe_on_screen", serde_json::json!({"serial":"emulator-5554","start_x":10,"start_y":20,"end_x":30,"end_y":40,"duration_ms":10000}), "input swipe 10 20 30 40 10000").await
}

#[tokio::test]
async fn text_encodes_spaces_quotes_and_literal_percent_s() -> TestResult {
    assert_input(
        "adb_input_text",
        serde_json::json!({"serial":"emulator-5554","text":"Hello world"}),
        "input text 'Hello%sworld'",
    )
    .await?;
    assert_input(
        "adb_input_text",
        serde_json::json!({"serial":"emulator-5554","text":"'$(id); %s"}),
        "input text ''\\''$(id);%s' && input text '%' && input text 's'",
    )
    .await
}

#[tokio::test]
async fn key_maps_symbolic_enum_to_android_keyevent() -> TestResult {
    for (key, argument) in [
        ("HOME", "KEYCODE_HOME"),
        ("BACK", "KEYCODE_BACK"),
        ("ENTER", "KEYCODE_ENTER"),
        ("TAB", "KEYCODE_TAB"),
        ("DEL", "KEYCODE_DEL"),
        ("FORWARD_DEL", "KEYCODE_FORWARD_DEL"),
        ("DPAD_UP", "KEYCODE_DPAD_UP"),
        ("DPAD_DOWN", "KEYCODE_DPAD_DOWN"),
        ("DPAD_LEFT", "KEYCODE_DPAD_LEFT"),
        ("DPAD_RIGHT", "KEYCODE_DPAD_RIGHT"),
        ("DPAD_CENTER", "KEYCODE_DPAD_CENTER"),
        ("VOLUME_UP", "KEYCODE_VOLUME_UP"),
        ("VOLUME_DOWN", "KEYCODE_VOLUME_DOWN"),
    ] {
        assert_input(
            "adb_press_key",
            serde_json::json!({"serial":"emulator-5554","keycode":key}),
            &format!("input keyevent {argument}"),
        )
        .await?;
    }
    Ok(())
}

#[tokio::test]
async fn invalid_input_is_rejected_without_executing_a_command() -> TestResult {
    // Given: the stub fails any unexpected command.
    let directory = tempfile::tempdir()?;
    let trace = directory.path().join("input.trace");
    let mut command = server_command();
    command
        .env("ANDROID_MCP_ADB", adb_fixture()?)
        .env("ANDROID_MCP_TEST_ADB_MODE", "input")
        .env("ANDROID_MCP_TEST_INPUT_COMMAND", "no command expected")
        .env("ANDROID_MCP_TEST_INPUT_TRACE", &trace);
    let (client, child) = connect_command(command).await?;
    for (name, arguments) in [
        (
            "adb_tap_on_screen",
            serde_json::json!({"serial":"emulator-5554","x":-1,"y":0}),
        ),
        (
            "adb_tap_on_screen",
            serde_json::json!({"serial":"emulator-5554","x":2147483648u32,"y":0}),
        ),
        (
            "adb_tap_on_screen",
            serde_json::json!({"serial":"bad serial","x":0,"y":0}),
        ),
        (
            "adb_long_press_on_screen",
            serde_json::json!({"serial":"emulator-5554","x":0,"y":0,"duration_ms":0}),
        ),
        (
            "adb_swipe_on_screen",
            serde_json::json!({"serial":"emulator-5554","start_x":0,"start_y":0,"end_x":1,"end_y":1,"duration_ms":10001}),
        ),
        (
            "adb_press_key",
            serde_json::json!({"serial":"emulator-5554","keycode":"HOME;id"}),
        ),
        (
            "adb_input_text",
            serde_json::json!({"serial":"emulator-5554","text":"Grüße"}),
        ),
        (
            "adb_input_text",
            serde_json::json!({"serial":"emulator-5554","text":"line\nbreak"}),
        ),
        (
            "adb_input_text",
            serde_json::json!({"serial":"emulator-5554","text":""}),
        ),
        (
            "adb_input_text",
            serde_json::json!({"serial":"emulator-5554","text":"a".repeat(1025)}),
        ),
    ] {
        // When
        let result = timeout(TIMEOUT, client.call_tool(request(name, arguments))).await?;
        // Then
        match result {
            Ok(result) => assert_eq!(result.is_error, Some(true)),
            Err(ServiceError::McpError(_)) => {}
            Err(error) => return Err(error.into()),
        }
    }
    assert!(
        !trace.exists(),
        "Invalid inputs must not spawn an ADB command"
    );
    disconnect(client, child).await
}

#[tokio::test]
async fn input_failures_and_output_overflows_are_tool_errors() -> TestResult {
    for (mode, expected) in [
        ("input_failure", "input injection denied"),
        ("input_stdout_large", "stdout exceeded"),
        ("input_stderr_large", "stderr exceeded"),
    ] {
        // Given
        let (client, child) = connect_input(mode, "input tap 10 20").await?;
        // When
        let result = timeout(
            TIMEOUT,
            client.call_tool(request(
                "adb_tap_on_screen",
                serde_json::json!({"serial":"emulator-5554","x":10,"y":20}),
            )),
        )
        .await??;
        // Then
        assert_eq!(result.is_error, Some(true));
        assert!(result.content[0].as_text().unwrap().text.contains(expected));
        disconnect(client, child).await?;
    }
    Ok(())
}

#[tokio::test]
async fn double_tap_timeout_is_bounded_and_connection_survives() -> TestResult {
    // Given
    let (client, child) =
        connect_input("input_timeout", "input tap 10 20 && input tap 10 20").await?;
    // When
    let result = timeout(
        Duration::from_secs(20),
        client.call_tool(request(
            "adb_tap_on_screen",
            serde_json::json!({"serial":"emulator-5554","x":10,"y":20,"double_tap":true}),
        )),
    )
    .await??;
    // Then
    assert_eq!(result.is_error, Some(true));
    assert!(
        result.content[0]
            .as_text()
            .unwrap()
            .text
            .contains("ADB input timed out after 15 seconds")
    );
    assert!(!timeout(TIMEOUT, client.list_all_tools()).await??.is_empty());
    disconnect(client, child).await
}
