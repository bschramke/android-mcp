use std::{io, path::Path, time::Duration};

use rmcp::schemars;
use tokio::time::Instant;

use super::super::{
    command::{CommandOptions, ensure_success, run_adb},
    device_info::validate_serial,
};

const ROTATION_TIMEOUT: Duration = Duration::from_secs(10);
const ROTATION_OUTPUT_LIMIT: usize = 16 * 1024;

#[derive(Clone, Copy, Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum EmuRotationDirection {
    Left,
    Right,
}

impl EmuRotationDirection {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Left => "left",
            Self::Right => "right",
        }
    }

    fn quarter_turns(self) -> usize {
        match self {
            Self::Left => 3,
            Self::Right => 1,
        }
    }
}

pub async fn emu_rotate(
    executable: &Path,
    serial: &str,
    direction: EmuRotationDirection,
) -> io::Result<()> {
    validate_serial(serial)?;
    let quarter_turns = direction.quarter_turns();
    let deadline = Instant::now() + ROTATION_TIMEOUT;
    for completed_turns in 0..quarter_turns {
        let result = run_adb(
            executable,
            ["-s", serial, "emu", "rotate"],
            "emulator device rotation",
            CommandOptions::new(deadline, ROTATION_OUTPUT_LIMIT, ROTATION_OUTPUT_LIMIT)
                .with_timeout_message("ADB emulator device rotation timed out after 10 seconds"),
        )
        .await
        .and_then(|output| ensure_success(&output, "emulator device rotation"));
        if let Err(error) = result {
            return Err(io::Error::new(
                error.kind(),
                format!(
                    "Cannot rotate emulator device {} after {completed_turns} of {quarter_turns} quarter-turns: {error}",
                    direction.as_str()
                ),
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_rotation_direction_to_clockwise_quarter_turns() {
        assert_eq!(EmuRotationDirection::Right.quarter_turns(), 1);
        assert_eq!(EmuRotationDirection::Left.quarter_turns(), 3);
    }
}
