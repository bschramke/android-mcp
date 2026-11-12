use std::{io, path::Path, time::Duration};

use rmcp::schemars;
use serde::Serialize;
use tokio::time::Instant;

use super::super::{
    command::{CommandOptions, ensure_success, run_shell},
    device_info::validate_serial,
    model::{DisplayOrientation, ScreenDimensions},
};
use super::display_info::{display_info, physical_resolution_until};

const ORIENTATION_TIMEOUT: Duration = Duration::from_secs(5);
const ORIENTATION_OUTPUT_LIMIT: usize = 16 * 1024;

#[derive(Clone, Copy, Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ScreenOrientationMode {
    Portrait,
    Landscape,
    Auto,
}

impl ScreenOrientationMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Portrait => "portrait",
            Self::Landscape => "landscape",
            Self::Auto => "auto",
        }
    }
}

#[derive(Debug, Serialize)]
pub struct ScreenOrientationInfo {
    pub orientation: Option<DisplayOrientation>,
    pub rotation_degrees: Option<u16>,
}

pub async fn get_screen_orientation(
    executable: &Path,
    serial: &str,
) -> io::Result<ScreenOrientationInfo> {
    let display = display_info(executable, serial).await?;
    Ok(ScreenOrientationInfo {
        orientation: display.orientation,
        rotation_degrees: display.rotation_degrees,
    })
}

pub async fn set_screen_orientation(
    executable: &Path,
    serial: &str,
    mode: ScreenOrientationMode,
) -> io::Result<()> {
    validate_serial(serial)?;
    let deadline = Instant::now() + ORIENTATION_TIMEOUT;
    let arguments = match mode {
        ScreenOrientationMode::Auto => vec!["wm", "set-user-rotation", "free"],
        ScreenOrientationMode::Portrait | ScreenOrientationMode::Landscape => {
            let natural_resolution =
                physical_resolution_until(executable, serial, deadline).await?;
            let rotation = rotation_for(mode, natural_resolution);
            vec!["wm", "set-user-rotation", "lock", rotation]
        }
    };
    let output = run_shell(
        executable,
        serial,
        arguments,
        "set screen orientation",
        CommandOptions::new(deadline, ORIENTATION_OUTPUT_LIMIT, ORIENTATION_OUTPUT_LIMIT)
            .with_timeout_message("ADB set screen orientation timed out after 5 seconds"),
    )
    .await?;
    ensure_success(&output, "set screen orientation")
}

fn rotation_for(mode: ScreenOrientationMode, natural_resolution: ScreenDimensions) -> &'static str {
    let natural_is_landscape = natural_resolution.width > natural_resolution.height;
    match (mode, natural_is_landscape) {
        (ScreenOrientationMode::Portrait, false) | (ScreenOrientationMode::Landscape, true) => "0",
        (ScreenOrientationMode::Portrait, true) | (ScreenOrientationMode::Landscape, false) => "1",
        (ScreenOrientationMode::Auto, _) => unreachable!("auto orientation is never locked"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_requested_orientation_from_natural_display_shape() {
        let portrait_display = ScreenDimensions {
            width: 1080,
            height: 2400,
        };
        let landscape_display = ScreenDimensions {
            width: 2400,
            height: 1080,
        };

        assert_eq!(
            rotation_for(ScreenOrientationMode::Portrait, portrait_display),
            "0"
        );
        assert_eq!(
            rotation_for(ScreenOrientationMode::Landscape, portrait_display),
            "1"
        );
        assert_eq!(
            rotation_for(ScreenOrientationMode::Portrait, landscape_display),
            "1"
        );
        assert_eq!(
            rotation_for(ScreenOrientationMode::Landscape, landscape_display),
            "0"
        );
    }
}
