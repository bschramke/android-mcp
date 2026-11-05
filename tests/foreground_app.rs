#![cfg(unix)]

mod support;

use rmcp::model::CallToolRequestParams;
use support::adb::connect;
use support::{TIMEOUT, TestResult, disconnect};
use tokio::time::{Duration, timeout};

fn foreground_app_request(serial: &str) -> CallToolRequestParams {
    let mut request = CallToolRequestParams::new("adb_get_foreground_app");
    request.arguments = serde_json::json!({"serial": serial}).as_object().cloned();
    request
}

#[tokio::test]
async fn returns_foreground_app_from_supported_activity_markers() -> TestResult {
    for (serial, mode, activity) in [
        (
            "emulator-5554",
            "foreground_mresumed",
            "com.example.app/.MainActivity",
        ),
        (
            "emulator-5556",
            "foreground_resumed",
            "com.example.app/com.example.app.Home",
        ),
    ] {
        let (client, child) = connect(mode).await?;
        let result = timeout(TIMEOUT, client.call_tool(foreground_app_request(serial))).await??;
        assert_ne!(result.is_error, Some(true));
        let app: serde_json::Value = serde_json::from_str(
            &result.content[0]
                .as_text()
                .ok_or("Expected foreground-app JSON")?
                .text,
        )?;
        assert_eq!(
            app,
            serde_json::json!({
                "serial": serial,
                "package": "com.example.app",
                "activity": activity
            })
        );
        disconnect(client, child).await?;
    }
    Ok(())
}

#[tokio::test]
async fn returns_unknown_when_no_foreground_activity_is_reported() -> TestResult {
    let (client, child) = connect("foreground_none").await?;
    let result = timeout(
        TIMEOUT,
        client.call_tool(foreground_app_request("emulator-5554")),
    )
    .await??;
    assert_ne!(result.is_error, Some(true));
    let app: serde_json::Value = serde_json::from_str(
        &result.content[0]
            .as_text()
            .ok_or("Expected foreground-app JSON")?
            .text,
    )?;
    assert_eq!(app["package"], serde_json::Value::Null);
    assert_eq!(app["activity"], serde_json::Value::Null);
    disconnect(client, child).await
}

#[tokio::test]
async fn reports_adb_failure_and_bounds_timeout() -> TestResult {
    let (client, child) = connect("foreground_failure").await?;
    let result = timeout(
        TIMEOUT,
        client.call_tool(foreground_app_request("emulator-5554")),
    )
    .await??;
    assert_eq!(result.is_error, Some(true));
    assert!(
        result.content[0]
            .as_text()
            .ok_or("Expected foreground-app error")?
            .text
            .contains("activity dump unavailable")
    );
    disconnect(client, child).await?;

    let (client, child) = connect("foreground_timeout").await?;
    let result = timeout(
        TIMEOUT + Duration::from_secs(2),
        client.call_tool(foreground_app_request("emulator-5554")),
    )
    .await??;
    assert_eq!(result.is_error, Some(true));
    assert!(
        result.content[0]
            .as_text()
            .ok_or("Expected foreground-app timeout")?
            .text
            .contains("ADB foreground app lookup timed out after 5 seconds")
    );
    disconnect(client, child).await
}
