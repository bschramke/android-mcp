use rmcp::{ServiceExt, tool, tool_router, transport::stdio};
use std::process::ExitCode;

mod adb;

#[derive(Clone)]
struct HelloWorldServer;

#[tool_router(server_handler)]
impl HelloWorldServer {
    #[tool(description = "Return a Hello World greeting")]
    fn hello_world(&self) -> String {
        "Hello, world!".to_owned()
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
    let _adb_executable = adb::resolve_executable().await?;
    let service = HelloWorldServer.serve(stdio()).await?;
    service.waiting().await?;
    Ok(())
}
