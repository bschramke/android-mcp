use rmcp::{ServiceExt, transport::stdio};
use std::process::ExitCode;

mod adb;
mod server;

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
    let screen_recordings = adb::ScreenRecordingManager::default();
    let service = server::AndroidMcpServer::new(adb_executable.clone(), screen_recordings.clone())
        .serve(stdio())
        .await?;
    let cancellation = service.cancellation_token();
    let waiting = service.waiting();
    tokio::pin!(waiting);
    let service_result = tokio::select! {
        result = &mut waiting => result,
        signal = tokio::signal::ctrl_c() => {
            if let Err(error) = signal {
                eprintln!("Cannot listen for Ctrl-C during shutdown: {error}");
            }
            cancellation.cancel();
            waiting.await
        }
    };

    for (serial, error) in screen_recordings.stop_all(&adb_executable).await {
        eprintln!("Cannot stop screen recording for {serial} during shutdown: {error}");
    }
    service_result?;
    Ok(())
}
