use crate::adb;
use rmcp::{
    handler::server::wrapper::Parameters,
    model::{CallToolResult, ContentBlock},
    schemars, tool, tool_router,
};
use std::path::PathBuf;

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct DeviceInfoRequest {
    #[schemars(description = "ADB serial number identifying the device to inspect")]
    serial: String,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct TapRequest {
    serial: String,
    #[schemars(range(min = 0, max = 2147483647))]
    x: u32,
    #[schemars(range(min = 0, max = 2147483647))]
    y: u32,
    #[serde(default)]
    #[schemars(
        description = "Send two consecutive taps; best effort, recognition by the app is not guaranteed"
    )]
    double_tap: bool,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct LongPressRequest {
    serial: String,
    #[schemars(range(min = 0, max = 2147483647))]
    x: u32,
    #[schemars(range(min = 0, max = 2147483647))]
    y: u32,
    #[serde(default = "default_long_press_duration")]
    #[schemars(range(min = 1, max = 10000))]
    duration_ms: u32,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct SwipeRequest {
    serial: String,
    #[schemars(range(min = 0, max = 2147483647))]
    start_x: u32,
    #[schemars(range(min = 0, max = 2147483647))]
    start_y: u32,
    #[schemars(range(min = 0, max = 2147483647))]
    end_x: u32,
    #[schemars(range(min = 0, max = 2147483647))]
    end_y: u32,
    #[serde(default = "default_swipe_duration")]
    #[schemars(range(min = 1, max = 10000))]
    duration_ms: u32,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct InputTextRequest {
    serial: String,
    #[schemars(
        description = "1-1024 printable ASCII characters; Unicode and control characters are not supported",
        length(min = 1, max = 1024)
    )]
    text: String,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct PressKeyRequest {
    serial: String,
    keycode: adb::Keycode,
}

fn default_long_press_duration() -> u32 {
    1000
}

fn default_swipe_duration() -> u32 {
    300
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
struct InstallAppRequest {
    #[schemars(description = "ADB serial number identifying the device to install to")]
    serial: String,
    #[schemars(description = "Absolute path to an APK file on the MCP server host")]
    apk_path: String,
    #[serde(default)]
    #[schemars(
        description = "Replace an installed app while preserving its data; defaults to false"
    )]
    replace: bool,
    #[serde(default)]
    #[schemars(description = "Allow a version downgrade; implies replace; defaults to false")]
    downgrade: bool,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct UninstallAppRequest {
    #[schemars(description = "ADB serial number identifying the device to uninstall from")]
    serial: String,
    #[schemars(description = "Android application package name (application ID)")]
    package: String,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct ClearAppDataRequest {
    #[schemars(description = "ADB serial number identifying the device to clear")]
    serial: String,
    #[schemars(description = "Android application package name (application ID)")]
    package: String,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct OpenUrlRequest {
    #[schemars(description = "ADB serial number identifying the device to open the URL on")]
    serial: String,
    #[schemars(description = "Absolute HTTP or HTTPS URL to open with Android's default handler")]
    url: String,
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
pub(crate) struct AndroidMcpServer {
    adb_executable: PathBuf,
    screen_recordings: adb::ScreenRecordingManager,
}

impl AndroidMcpServer {
    pub(crate) fn new(
        adb_executable: PathBuf,
        screen_recordings: adb::ScreenRecordingManager,
    ) -> Self {
        Self {
            adb_executable,
            screen_recordings,
        }
    }

    async fn perform_input(&self, serial: &str, action: adb::InputAction) -> CallToolResult {
        match adb::send_input(&self.adb_executable, serial, action).await {
            Ok(()) => CallToolResult::success(vec![ContentBlock::text("Input command completed")]),
            Err(error) => CallToolResult::error(vec![ContentBlock::text(error.to_string())]),
        }
    }
}

#[tool_router(server_handler)]
impl AndroidMcpServer {
    #[tool(description = "Tap on the screen; optionally send a best-effort double tap")]
    async fn adb_tap_on_screen(
        &self,
        Parameters(request): Parameters<TapRequest>,
    ) -> CallToolResult {
        self.perform_input(
            &request.serial,
            adb::InputAction::Tap {
                x: request.x,
                y: request.y,
                double_tap: request.double_tap,
            },
        )
        .await
    }

    #[tool(description = "Long press on the screen at the specified coordinates")]
    async fn adb_long_press_on_screen(
        &self,
        Parameters(request): Parameters<LongPressRequest>,
    ) -> CallToolResult {
        self.perform_input(
            &request.serial,
            adb::InputAction::LongPress {
                x: request.x,
                y: request.y,
                duration_ms: request.duration_ms,
            },
        )
        .await
    }

    #[tool(description = "Swipe between two screen coordinates")]
    async fn adb_swipe_on_screen(
        &self,
        Parameters(request): Parameters<SwipeRequest>,
    ) -> CallToolResult {
        self.perform_input(
            &request.serial,
            adb::InputAction::Swipe {
                start_x: request.start_x,
                start_y: request.start_y,
                end_x: request.end_x,
                end_y: request.end_y,
                duration_ms: request.duration_ms,
            },
        )
        .await
    }

    #[tool(description = "Enter printable ASCII text into the currently focused field")]
    async fn adb_input_text(
        &self,
        Parameters(request): Parameters<InputTextRequest>,
    ) -> CallToolResult {
        self.perform_input(&request.serial, adb::InputAction::Text(request.text))
            .await
    }

    #[tool(description = "Press a supported Android key")]
    async fn adb_press_key(
        &self,
        Parameters(request): Parameters<PressKeyRequest>,
    ) -> CallToolResult {
        self.perform_input(&request.serial, adb::InputAction::Key(request.keycode))
            .await
    }

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

    #[tool(description = "Install an app from file (.apk)")]
    async fn adb_install_app(
        &self,
        Parameters(request): Parameters<InstallAppRequest>,
    ) -> CallToolResult {
        match adb::install_app(
            &self.adb_executable,
            &request.serial,
            std::path::Path::new(&request.apk_path),
            request.replace,
            request.downgrade,
        )
        .await
        {
            Ok(()) => CallToolResult::success(vec![ContentBlock::text(format!(
                "Installed APK on {}",
                request.serial
            ))]),
            Err(error) => CallToolResult::error(vec![ContentBlock::text(error.to_string())]),
        }
    }

    #[tool(description = "Uninstall an app by its package name or bundle ID")]
    async fn adb_uninstall_app(
        &self,
        Parameters(request): Parameters<UninstallAppRequest>,
    ) -> CallToolResult {
        match adb::uninstall_app(&self.adb_executable, &request.serial, &request.package).await {
            Ok(()) => CallToolResult::success(vec![ContentBlock::text(format!(
                "Uninstalled {} from {}",
                request.package, request.serial
            ))]),
            Err(error) => CallToolResult::error(vec![ContentBlock::text(error.to_string())]),
        }
    }

    #[tool(description = "Clear an app's data so it starts fresh")]
    async fn adb_clear_app_data(
        &self,
        Parameters(request): Parameters<ClearAppDataRequest>,
    ) -> CallToolResult {
        match adb::clear_app_data(&self.adb_executable, &request.serial, &request.package).await {
            Ok(()) => CallToolResult::success(vec![ContentBlock::text(format!(
                "Cleared data for {} on {}",
                request.package, request.serial
            ))]),
            Err(error) => CallToolResult::error(vec![ContentBlock::text(error.to_string())]),
        }
    }

    #[tool(
        description = "Open an HTTP(S) URL with the Android default handler, usually the default browser"
    )]
    async fn adb_open_url(
        &self,
        Parameters(request): Parameters<OpenUrlRequest>,
    ) -> CallToolResult {
        match adb::open_url(&self.adb_executable, &request.serial, &request.url).await {
            Ok(()) => CallToolResult::success(vec![ContentBlock::text(format!(
                "Opened URL on {}: {}",
                request.serial, request.url
            ))]),
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

    #[tool(description = "Get the app currently in the foreground")]
    async fn adb_get_foreground_app(
        &self,
        Parameters(request): Parameters<DeviceInfoRequest>,
    ) -> CallToolResult {
        match adb::get_foreground_app(&self.adb_executable, &request.serial).await {
            Ok(app) => match serde_json::to_string(&app) {
                Ok(json) => CallToolResult::success(vec![ContentBlock::text(json)]),
                Err(error) => CallToolResult::error(vec![ContentBlock::text(format!(
                    "Cannot serialize foreground app: {error}"
                ))]),
            },
            Err(error) => CallToolResult::error(vec![ContentBlock::text(error.to_string())]),
        }
    }
}
