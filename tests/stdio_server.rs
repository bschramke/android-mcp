use std::{error::Error, process::Stdio, time::Duration};

use rmcp::{
    RoleClient, ServiceExt,
    model::CallToolRequestParams,
    service::{RunningService, ServiceError},
};
use tokio::{
    process::{Child, Command},
    time::timeout,
};

type TestResult<T = ()> = Result<T, Box<dyn Error>>;
type Client = RunningService<RoleClient, ()>;

const TIMEOUT: Duration = Duration::from_secs(10);

async fn connect() -> TestResult<(Client, Child)> {
    let mut child = Command::new(env!("CARGO_BIN_EXE_android-mcp"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .kill_on_drop(true)
        .spawn()?;

    let stdout = child.stdout.take().ok_or("Server stdout is unavailable")?;
    let stdin = child.stdin.take().ok_or("Server stdin is unavailable")?;
    let client = timeout(TIMEOUT, ().serve((stdout, stdin))).await??;
    Ok((client, child))
}

async fn disconnect(mut client: Client, mut child: Child) -> TestResult {
    timeout(TIMEOUT, client.close()).await??;
    let exit = timeout(TIMEOUT, child.wait()).await??;
    assert!(exit.success(), "Server exited unsuccessfully: {exit}");
    Ok(())
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
