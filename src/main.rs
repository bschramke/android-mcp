use rmcp::{
    ServiceExt,
    model::{CallToolResult, ContentBlock},
    tool, tool_router,
    transport::stdio,
};
use std::{path::PathBuf, process::ExitCode};

mod adb;

#[derive(Clone)]
struct AndroidMcpServer {
    adb_executable: PathBuf,
}

#[tool_router(server_handler)]
impl AndroidMcpServer {
    #[tool(
        description = "List Android devices visible to the local ADB server, including offline and unauthorized devices"
    )]
    async fn adb_list_devices(&self) -> CallToolResult {
        match adb::list_devices(&self.adb_executable).await {
            Ok(devices) => match serde_json::to_string(&devices) {
                Ok(json) => CallToolResult::success(vec![ContentBlock::text(json)]),
                Err(error) => CallToolResult::error(vec![ContentBlock::text(format!(
                    "Cannot serialize ADB device list: {error}"
                ))]),
            },
            Err(error) => CallToolResult::error(vec![ContentBlock::text(error.to_string())]),
        }
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("Cannot start android-mcp: {error}");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let adb_executable = adb::resolve_executable().await?;
    let service = AndroidMcpServer { adb_executable }.serve(stdio()).await?;
    service.waiting().await?;
    Ok(())
}
