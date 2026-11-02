use rmcp::{
    ServiceExt,
    handler::server::wrapper::Parameters,
    model::{CallToolResult, ContentBlock},
    schemars, tool, tool_router,
    transport::stdio,
};
use std::{path::PathBuf, process::ExitCode};

mod adb;

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct DeviceInfoRequest {
    #[schemars(description = "ADB serial number identifying the device to inspect")]
    serial: String,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct LogcatRequest {
    #[schemars(description = "ADB serial number identifying the device to read logs from")]
    serial: String,
    #[serde(default = "default_logcat_lines")]
    #[schemars(
        description = "Maximum number of recent log lines; defaults to 200",
        range(min = 1, max = 2000)
    )]
    max_lines: u32,
}

fn default_logcat_lines() -> u32 {
    200
}

#[derive(Clone)]
struct AndroidMcpServer {
    adb_executable: PathBuf,
}

#[tool_router(server_handler)]
impl AndroidMcpServer {
    #[tool(
        description = "Read a bounded snapshot of recent device logcat output without clearing the log buffer; logs may contain sensitive data"
    )]
    async fn adb_logcat(&self, Parameters(request): Parameters<LogcatRequest>) -> CallToolResult {
        match adb::logcat(&self.adb_executable, &request.serial, request.max_lines).await {
            Ok(snapshot) => match serde_json::to_string(&snapshot) {
                Ok(json) => CallToolResult::success(vec![ContentBlock::text(json)]),
                Err(error) => CallToolResult::error(vec![ContentBlock::text(format!(
                    "Cannot serialize ADB logcat: {error}"
                ))]),
            },
            Err(error) => CallToolResult::error(vec![ContentBlock::text(error.to_string())]),
        }
    }

    #[tool(description = "Read Android device details by serial number using adb shell getprop")]
    async fn adb_device_info(
        &self,
        Parameters(request): Parameters<DeviceInfoRequest>,
    ) -> CallToolResult {
        match adb::device_info(&self.adb_executable, &request.serial).await {
            Ok(info) => match serde_json::to_string(&info) {
                Ok(json) => CallToolResult::success(vec![ContentBlock::text(json)]),
                Err(error) => CallToolResult::error(vec![ContentBlock::text(format!(
                    "Cannot serialize ADB device info: {error}"
                ))]),
            },
            Err(error) => CallToolResult::error(vec![ContentBlock::text(error.to_string())]),
        }
    }

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
