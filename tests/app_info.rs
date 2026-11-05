#![cfg(unix)]

mod support;

use rmcp::model::CallToolRequestParams;
use support::adb::connect;
use support::{TIMEOUT, TestResult, disconnect};
use tokio::time::{Duration, timeout};

fn app_info_request(serial: &str, package: &str) -> CallToolRequestParams {
    let mut request = CallToolRequestParams::new("adb_app_info");
    request.arguments = serde_json::json!({"serial": serial, "package": package})
        .as_object()
        .cloned();
    request
}

#[tokio::test]
async fn returns_structured_package_metadata() -> TestResult {
    let (client, child) = connect("app_info").await?;
    let result = timeout(
        TIMEOUT,
        client.call_tool(app_info_request("emulator-5554", "com.example.app")),
    )
    .await??;
    assert_ne!(result.is_error, Some(true));
    let info: serde_json::Value = serde_json::from_str(
        &result.content[0]
            .as_text()
            .ok_or("Expected app-info JSON")?
            .text,
    )?;
    assert_eq!(
        info,
        serde_json::json!({
            "serial": "emulator-5554",
            "package": "com.example.app",
            "version_code": 42,
            "version_name": "2.3.1",
            "min_sdk": 26,
            "target_sdk": 35,
            "first_install_time": "2025-01-15 10:30:45",
            "last_update_time": "2025-02-20 11:45:00",
            "installer_package_name": "com.android.vending",
            "code_path": "/data/app/com.example.app/base.apk"
        })
    );
    disconnect(client, child).await
}

#[tokio::test]
async fn reports_missing_packages_and_adb_failures() -> TestResult {
    for (mode, expected) in [
        ("app_info_missing", "does not contain package"),
        ("app_info_failure", "dumpsys unavailable"),
    ] {
        let (client, child) = connect(mode).await?;
        let result = timeout(
            TIMEOUT,
            client.call_tool(app_info_request("emulator-5554", "com.example.app")),
        )
        .await??;
        assert_eq!(result.is_error, Some(true));
        assert!(
            result.content[0]
                .as_text()
                .ok_or("Expected app-info error")?
                .text
                .contains(expected)
        );
        disconnect(client, child).await?;
    }
    Ok(())
}

#[tokio::test]
async fn rejects_invalid_package_names() -> TestResult {
    let (client, child) = connect("app_info").await?;
    let result = timeout(
        TIMEOUT,
        client.call_tool(app_info_request("emulator-5554", "com.example;id")),
    )
    .await?;
    assert!(result.is_err() || result.is_ok_and(|result| result.is_error == Some(true)));
    disconnect(client, child).await
}

#[tokio::test]
async fn bounds_dumpsys_timeout() -> TestResult {
    let (client, child) = connect("app_info_timeout").await?;
    let result = timeout(
        TIMEOUT + Duration::from_secs(2),
        client.call_tool(app_info_request("emulator-5554", "com.example.app")),
    )
    .await??;
    assert_eq!(result.is_error, Some(true));
    assert!(
        result.content[0]
            .as_text()
            .ok_or("Expected app-info timeout")?
            .text
            .contains("ADB app info timed out after 5 seconds")
    );
    disconnect(client, child).await
}
