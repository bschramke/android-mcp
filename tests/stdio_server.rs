#![cfg(unix)]

mod support;

use rmcp::{model::CallToolRequestParams, service::ServiceError};
use support::{Client, TIMEOUT, TestResult, connect_command, disconnect, server_command};
use tokio::{process::Child, time::timeout};

async fn connect() -> TestResult<(Client, Child)> {
    let mut command = server_command();
    command.env("ANDROID_MCP_ADB", "/usr/bin/true");
    connect_command(command).await
}

#[tokio::test]
async fn initializes_and_discovers_hello_world_tool() -> TestResult {
    let (client, child) = connect().await?;
    let info = client
        .peer_info()
        .ok_or("Missing server initialization result")?;
    assert!(
        info.capabilities.tools.is_some(),
        "Server must advertise tools"
    );

    let tools = timeout(TIMEOUT, client.list_all_tools()).await??;
    assert_eq!(tools.len(), 1);
    let tool = &tools[0];
    assert_eq!(tool.name, "hello_world");
    assert_eq!(
        tool.description.as_deref(),
        Some("Return a Hello World greeting")
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

    disconnect(client, child).await
}

#[tokio::test]
async fn hello_world_returns_exact_greeting() -> TestResult {
    let (client, child) = connect().await?;
    let result = timeout(
        TIMEOUT,
        client.call_tool(CallToolRequestParams::new("hello_world")),
    )
    .await??;

    assert_ne!(result.is_error, Some(true));
    assert_eq!(result.content.len(), 1);
    let content = result.content[0].as_text().ok_or("Expected text content")?;
    assert_eq!(content.text, "Hello, world!");

    disconnect(client, child).await
}

#[tokio::test]
async fn unknown_tool_returns_protocol_error_without_stopping_server() -> TestResult {
    let (client, child) = connect().await?;
    let result = timeout(
        TIMEOUT,
        client.call_tool(CallToolRequestParams::new("unknown_tool")),
    )
    .await?;
    assert!(
        matches!(result, Err(ServiceError::McpError(_))),
        "Expected an MCP protocol error, got {result:?}"
    );

    let greeting = timeout(
        TIMEOUT,
        client.call_tool(CallToolRequestParams::new("hello_world")),
    )
    .await??;
    assert_ne!(greeting.is_error, Some(true));

    disconnect(client, child).await
}
