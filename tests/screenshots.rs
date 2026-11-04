#![cfg(unix)]

mod support;

use rmcp::model::CallToolRequestParams;
use support::adb::connect;
use support::{TIMEOUT, TestResult, disconnect};
use tokio::time::timeout;

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
