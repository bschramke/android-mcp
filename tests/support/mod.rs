use std::{error::Error, process::Stdio, time::Duration};

use rmcp::{RoleClient, ServiceExt, service::RunningService};
use tokio::{
    process::{Child, Command},
    time::timeout,
};

pub type TestResult<T = ()> = Result<T, Box<dyn Error>>;
pub type Client = RunningService<RoleClient, ()>;

pub const TIMEOUT: Duration = Duration::from_secs(10);

pub fn server_command() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_android-mcp"));
    command
        .env_remove("ANDROID_MCP_ADB")
        .env_remove("ANDROID_SDK_ROOT")
        .env_remove("ANDROID_HOME")
        .env("PATH", "")
        .kill_on_drop(true);
    command
}

pub async fn connect_command(mut command: Command) -> TestResult<(Client, Child)> {
    let mut child = command
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

pub async fn disconnect(mut client: Client, mut child: Child) -> TestResult {
    timeout(TIMEOUT, client.close()).await??;
    let exit = timeout(TIMEOUT, child.wait()).await??;
    assert!(exit.success(), "Server exited unsuccessfully: {exit}");
    Ok(())
}
