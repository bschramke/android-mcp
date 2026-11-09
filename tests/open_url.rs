#![cfg(unix)]

mod support;

use rmcp::model::CallToolRequestParams;
use support::adb::adb_fixture;
use support::{Client, TIMEOUT, TestResult, connect_command, disconnect, server_command};
use tokio::{process::Child, time::timeout};

fn request(url: &str) -> CallToolRequestParams {
    let mut request = CallToolRequestParams::new("adb_open_url");
    request.arguments = serde_json::json!({"serial":"emulator-5554", "url":url})
        .as_object()
        .cloned();
    request
}

async fn call_open_url(
    mode: &str,
    expected_command: &str,
    url: &str,
) -> TestResult<rmcp::model::CallToolResult> {
    let mut command = server_command();
    command
        .env("ANDROID_MCP_ADB", adb_fixture()?)
        .env("ANDROID_MCP_TEST_ADB_MODE", mode)
        .env("ANDROID_MCP_TEST_OPEN_URL_COMMAND", expected_command);
    let (client, child): (Client, Child) = connect_command(command).await?;
    let result = timeout(TIMEOUT, client.call_tool(request(url))).await??;
    disconnect(client, child).await?;
    Ok(result)
}

#[tokio::test]
async fn opens_url_with_android_view_handler_and_quotes_query_arguments() -> TestResult {
    let url = "https://example.com/path?q=one&next=two";
    let command = format!(
        "am start -a android.intent.action.VIEW -c android.intent.category.BROWSABLE -d '{url}'"
    );
    let result = call_open_url("open_url", &command, url).await?;
    assert_ne!(result.is_error, Some(true));
    assert!(
        result.content[0]
            .as_text()
            .ok_or("Expected open-URL success text")?
            .text
            .contains(url)
    );
    Ok(())
}

#[tokio::test]
async fn rejects_non_web_urls_before_running_adb() -> TestResult {
    for url in [
        "javascript:alert(1)",
        "file:///etc/passwd",
        "intent://example.com/#Intent;scheme=https;end",
        "relative/path",
        "https://user:password@example.com",
    ] {
        let result = call_open_url("empty", "unused", url).await?;
        assert_eq!(result.is_error, Some(true), "Accepted {url:?}");
        let message = result.content[0]
            .as_text()
            .ok_or("Expected URL validation error")?
            .text
            .clone();
        assert!(message.contains("URL must"), "Unexpected error: {message}");
    }
    Ok(())
}

#[tokio::test]
async fn reports_adb_failure_to_the_mcp_client() -> TestResult {
    let url = "https://example.com/";
    let command = format!(
        "am start -a android.intent.action.VIEW -c android.intent.category.BROWSABLE -d '{url}'"
    );
    let result = call_open_url("open_url_failure", &command, url).await?;
    assert_eq!(result.is_error, Some(true));
    assert!(
        result.content[0]
            .as_text()
            .ok_or("Expected ADB error text")?
            .text
            .contains("No activity found to handle URL")
    );
    Ok(())
}
