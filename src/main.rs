use rmcp::{ServiceExt, tool, tool_router, transport::stdio};

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
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let service = HelloWorldServer.serve(stdio()).await?;
    service.waiting().await?;
    Ok(())
}
