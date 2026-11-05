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
struct ListAppsRequest {
    #[schemars(description = "ADB serial number identifying the device to inspect")]
    serial: String,
    #[serde(default)]
    #[schemars(description = "Exclude preinstalled system packages; defaults to false")]
    exclude_system: bool,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct AppInfoRequest {
    #[schemars(description = "ADB serial number identifying the device to inspect")]
    serial: String,
    #[schemars(description = "Android application package name")]
    package: String,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct ScreenElementsRequest {
    #[schemars(description = "ADB serial number identifying the device to inspect")]
    serial: String,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct ScreenshotRequest {
    #[schemars(description = "ADB serial number identifying the device to capture")]
    serial: String,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct SaveScreenshotRequest {
    #[schemars(description = "ADB serial number identifying the device to capture")]
    serial: String,
    #[schemars(description = "Absolute destination path ending in .png")]
    path: String,
    #[serde(default)]
    #[schemars(description = "Replace an existing file; defaults to false")]
    overwrite: bool,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct StartScreenRecordingRequest {
    #[schemars(description = "ADB serial number identifying the device to record")]
    serial: String,
    #[schemars(description = "Absolute destination path ending in .mp4")]
    path: String,
    #[serde(default)]
    #[schemars(description = "Replace an existing file after recording; defaults to false")]
    overwrite: bool,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct StopScreenRecordingRequest {
    #[schemars(description = "ADB serial number identifying the active recording")]
    serial: String,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct HealthCheckRequest {
    #[schemars(description = "Optional ADB serial number for device-specific checks")]
    serial: Option<String>,
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
    #[schemars(
        description = "Optional process ID; combined with tag and priority filters",
        range(min = 1, max = 2147483647)
    )]
    pid: Option<u32>,
    #[schemars(
        description = "Optional Android application package; resolve its running main-process PID for this snapshot; cannot be combined with pid",
        length(min = 1, max = 255)
    )]
    package: Option<String>,
    #[serde(default)]
    #[schemars(
        description = "Exact tag names (OR); at most 20 names of 1-128 ASCII letters/digits or ._/+-@, no leading '-', whitespace, wildcards, or filter expressions",
        length(max = 20)
    )]
    tags: Vec<String>,
    #[schemars(
        description = "Minimum severity; warn includes warn, error, and fatal; without a priority, selected tags include all levels"
    )]
    priority: Option<adb::LogPriority>,
}

fn default_logcat_lines() -> u32 {
    200
}

#[derive(Clone)]
struct AndroidMcpServer {
    adb_executable: PathBuf,
    screen_recordings: adb::ScreenRecordingManager,
}

#[tool_router(server_handler)]
impl AndroidMcpServer {
    #[tool(description = "Start recording the device screen to a video file")]
    async fn adb_start_screen_recording(
        &self,
        Parameters(request): Parameters<StartScreenRecordingRequest>,
    ) -> CallToolResult {
        match self
            .screen_recordings
            .start(
                &self.adb_executable,
                &request.serial,
                std::path::Path::new(&request.path),
                request.overwrite,
            )
            .await
        {
            Ok(()) => CallToolResult::success(vec![ContentBlock::text(format!(
                "Started screen recording for {} to {} (maximum 180 seconds)",
                request.serial, request.path
            ))]),
            Err(error) => CallToolResult::error(vec![ContentBlock::text(error.to_string())]),
        }
    }

    #[tool(description = "Stop the active screen recording and save the video")]
    async fn adb_stop_screen_recording(
        &self,
        Parameters(request): Parameters<StopScreenRecordingRequest>,
    ) -> CallToolResult {
        match self
            .screen_recordings
            .stop(&self.adb_executable, &request.serial)
            .await
        {
            Ok(result) => CallToolResult::success(vec![ContentBlock::text(format!(
                "Saved screen recording to {} ({} bytes)",
                result.path.display(),
                result.size_bytes
            ))]),
            Err(error) => CallToolResult::error(vec![ContentBlock::text(error.to_string())]),
        }
    }

    #[tool(description = "List UI elements with their coordinates and properties")]
    async fn adb_list_elements_on_screen(
        &self,
        Parameters(request): Parameters<ScreenElementsRequest>,
    ) -> CallToolResult {
        match adb::list_elements_on_screen(&self.adb_executable, &request.serial).await {
            Ok(elements) => match serde_json::to_string(&elements) {
                Ok(json) => CallToolResult::success(vec![ContentBlock::text(json)]),
                Err(error) => CallToolResult::error(vec![ContentBlock::text(format!(
                    "Cannot serialize screen elements: {error}"
                ))]),
            },
            Err(error) => CallToolResult::error(vec![ContentBlock::text(error.to_string())]),
        }
    }

    #[tool(description = "Get the nested UI hierarchy for the current Android screen")]
    async fn adb_get_screen_hierarchy(
        &self,
        Parameters(request): Parameters<ScreenElementsRequest>,
    ) -> CallToolResult {
        match adb::get_screen_hierarchy(&self.adb_executable, &request.serial).await {
            Ok(hierarchy) => match serde_json::to_string(&hierarchy) {
                Ok(json) => CallToolResult::success(vec![ContentBlock::text(json)]),
                Err(error) => CallToolResult::error(vec![ContentBlock::text(format!(
                    "Cannot serialize screen hierarchy: {error}"
                ))]),
            },
            Err(error) => CallToolResult::error(vec![ContentBlock::text(error.to_string())]),
        }
    }

    #[tool(description = "Take a screenshot to understand what's on screen")]
    async fn adb_take_screenshot(
        &self,
        Parameters(request): Parameters<ScreenshotRequest>,
    ) -> CallToolResult {
        match adb::take_screenshot(&self.adb_executable, &request.serial).await {
            Ok(image) => CallToolResult::success(vec![ContentBlock::image(image, "image/png")]),
            Err(error) => CallToolResult::error(vec![ContentBlock::text(error.to_string())]),
        }
    }

    #[tool(description = "Save a screenshot to a file")]
    async fn adb_save_screenshot(
        &self,
        Parameters(request): Parameters<SaveScreenshotRequest>,
    ) -> CallToolResult {
        match adb::save_screenshot(
            &self.adb_executable,
            &request.serial,
            std::path::Path::new(&request.path),
            request.overwrite,
        )
        .await
        {
            Ok(size) => CallToolResult::success(vec![ContentBlock::text(format!(
                "Saved screenshot to {} ({size} bytes)",
                request.path
            ))]),
            Err(error) => CallToolResult::error(vec![ContentBlock::text(error.to_string())]),
        }
    }

    #[tool(
        description = "Diagnose the ADB executable, server, visible devices, authorization, and read-only shell access; adb devices may start the local ADB server"
    )]
    async fn adb_health_check(
        &self,
        Parameters(request): Parameters<HealthCheckRequest>,
    ) -> CallToolResult {
        let report = adb::health_check(&self.adb_executable, request.serial.as_deref()).await;
        match serde_json::to_string(&report) {
            Ok(json) => CallToolResult::success(vec![ContentBlock::text(json)]),
            Err(error) => CallToolResult::error(vec![ContentBlock::text(format!(
                "Cannot serialize ADB health check: {error}"
            ))]),
        }
    }

    #[tool(
        description = "Read a bounded snapshot of recent device logcat output without clearing the log buffer; logs may contain sensitive data"
    )]
    async fn adb_logcat(&self, Parameters(request): Parameters<LogcatRequest>) -> CallToolResult {
        match adb::logcat(
            &self.adb_executable,
            &request.serial,
            request.max_lines,
            request.pid,
            &request.tags,
            request.priority,
            request.package.as_deref(),
        )
        .await
        {
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
        description = "Read display resolution, density, rotation, and orientation for an Android device by serial"
    )]
    async fn adb_display_info(
        &self,
        Parameters(request): Parameters<DeviceInfoRequest>,
    ) -> CallToolResult {
        match adb::display_info(&self.adb_executable, &request.serial).await {
            Ok(info) => match serde_json::to_string(&info) {
                Ok(json) => CallToolResult::success(vec![ContentBlock::text(json)]),
                Err(error) => CallToolResult::error(vec![ContentBlock::text(format!(
                    "Cannot serialize ADB display info: {error}"
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

    #[tool(description = "List all installed apps on the device")]
    async fn adb_list_apps(
        &self,
        Parameters(request): Parameters<ListAppsRequest>,
    ) -> CallToolResult {
        match adb::list_apps(
            &self.adb_executable,
            &request.serial,
            request.exclude_system,
        )
        .await
        {
            Ok(apps) => match serde_json::to_string(&apps) {
                Ok(json) => CallToolResult::success(vec![ContentBlock::text(json)]),
                Err(error) => CallToolResult::error(vec![ContentBlock::text(format!(
                    "Cannot serialize ADB app list: {error}"
                ))]),
            },
            Err(error) => CallToolResult::error(vec![ContentBlock::text(error.to_string())]),
        }
    }

    #[tool(description = "Get details for an installed app on the device")]
    async fn adb_app_info(
        &self,
        Parameters(request): Parameters<AppInfoRequest>,
    ) -> CallToolResult {
        match adb::app_info(&self.adb_executable, &request.serial, &request.package).await {
            Ok(info) => match serde_json::to_string(&info) {
                Ok(json) => CallToolResult::success(vec![ContentBlock::text(json)]),
                Err(error) => CallToolResult::error(vec![ContentBlock::text(format!(
                    "Cannot serialize ADB app info: {error}"
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
    let screen_recordings = adb::ScreenRecordingManager::default();
    let service = AndroidMcpServer {
        adb_executable: adb_executable.clone(),
        screen_recordings: screen_recordings.clone(),
    }
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
