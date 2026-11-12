use std::{io, path::Path, time::Duration};

use rmcp::schemars;

use super::{
    command::{CommandOptions, deadline_after, ensure_success, run_shell},
    device_info::validate_serial,
};

#[derive(Clone, Copy, Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Keycode {
    Home,
    Back,
    Enter,
    Tab,
    Del,
    ForwardDel,
    DpadUp,
    DpadDown,
    DpadLeft,
    DpadRight,
    DpadCenter,
    VolumeUp,
    VolumeDown,
}

impl Keycode {
    fn argument(self) -> &'static str {
        match self {
            Self::Home => "KEYCODE_HOME",
            Self::Back => "KEYCODE_BACK",
            Self::Enter => "KEYCODE_ENTER",
            Self::Tab => "KEYCODE_TAB",
            Self::Del => "KEYCODE_DEL",
            Self::ForwardDel => "KEYCODE_FORWARD_DEL",
            Self::DpadUp => "KEYCODE_DPAD_UP",
            Self::DpadDown => "KEYCODE_DPAD_DOWN",
            Self::DpadLeft => "KEYCODE_DPAD_LEFT",
            Self::DpadRight => "KEYCODE_DPAD_RIGHT",
            Self::DpadCenter => "KEYCODE_DPAD_CENTER",
            Self::VolumeUp => "KEYCODE_VOLUME_UP",
            Self::VolumeDown => "KEYCODE_VOLUME_DOWN",
        }
    }
}

pub enum InputAction {
    Tap {
        x: u32,
        y: u32,
        double_tap: bool,
    },
    LongPress {
        x: u32,
        y: u32,
        duration_ms: u32,
    },
    Swipe {
        start_x: u32,
        start_y: u32,
        end_x: u32,
        end_y: u32,
        duration_ms: u32,
    },
    Text(String),
    Key(Keycode),
}

impl InputAction {
    fn command(&self) -> io::Result<String> {
        match self {
            Self::Tap { x, y, double_tap } => tap_command(*x, *y, *double_tap),
            Self::LongPress { x, y, duration_ms } => long_press_command(*x, *y, *duration_ms),
            Self::Swipe {
                start_x,
                start_y,
                end_x,
                end_y,
                duration_ms,
            } => swipe_command(*start_x, *start_y, *end_x, *end_y, *duration_ms),
            Self::Text(text) => text_command(text),
            Self::Key(key) => Ok(format!("input keyevent {}", key.argument())),
        }
    }
}

fn tap_command(x: u32, y: u32, double_tap: bool) -> io::Result<String> {
    validate_coordinates(&[x, y])?;
    let command = format!("input tap {x} {y}");
    Ok(if double_tap {
        format!("{command} && {command}")
    } else {
        command
    })
}

fn long_press_command(x: u32, y: u32, duration_ms: u32) -> io::Result<String> {
    swipe_command(x, y, x, y, duration_ms)
}

fn swipe_command(
    start_x: u32,
    start_y: u32,
    end_x: u32,
    end_y: u32,
    duration_ms: u32,
) -> io::Result<String> {
    validate_coordinates(&[start_x, start_y, end_x, end_y])?;
    validate_duration(duration_ms)?;
    Ok(format!(
        "input swipe {start_x} {start_y} {end_x} {end_y} {duration_ms}"
    ))
}

pub async fn send_input(executable: &Path, serial: &str, action: InputAction) -> io::Result<()> {
    validate_serial(serial)?;
    let command = action.command()?;
    let output = run_shell(
        executable,
        serial,
        [command],
        "input",
        CommandOptions::new(
            deadline_after(Duration::from_secs(15)), // deadline
            CommandOptions::LIMIT_16_KIB,            // stdout_limit
            CommandOptions::LIMIT_16_KIB,            // stderr_limit
        )
        .with_timeout_message("ADB input timed out after 15 seconds"),
    )
    .await?;
    ensure_success(&output, "input")
}

fn validate_coordinates(coordinates: &[u32]) -> io::Result<()> {
    if coordinates
        .iter()
        .any(|coordinate| *coordinate > i32::MAX as u32)
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Coordinates must be between 0 and 2147483647",
        ));
    }
    Ok(())
}

fn validate_duration(duration_ms: u32) -> io::Result<()> {
    if !(1..=10_000).contains(&duration_ms) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "duration_ms must be between 1 and 10000",
        ));
    }
    Ok(())
}

fn text_command(text: &str) -> io::Result<String> {
    if text.is_empty() || text.len() > 1024 || !text.bytes().all(|byte| (32..=126).contains(&byte))
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "text must contain 1-1024 printable ASCII characters",
        ));
    }
    let mut commands = Vec::new();
    // Android interprets "%s" as a space, so literal occurrences must be sent separately.
    for (index, part) in text.split("%s").enumerate() {
        if index > 0 {
            commands.push("input text '%'".to_owned());
            commands.push("input text 's'".to_owned());
        }
        if !part.is_empty() {
            let encoded = part.replace(' ', "%s").replace('\'', "'\\''");
            commands.push(format!("input text '{encoded}'"));
        }
    }
    Ok(commands.join(" && "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn double_tap_uses_one_script_with_two_identical_taps() {
        // Given
        let action = InputAction::Tap {
            x: 10,
            y: 20,
            double_tap: true,
        };
        // When
        let command = action.command().unwrap();
        // Then
        assert_eq!(command, "input tap 10 20 && input tap 10 20");
    }

    #[test]
    fn text_quotes_shell_syntax_and_preserves_literal_percent_s() {
        // Given
        let text = "Hello 'world' $HOME; %s";
        // When
        let command = text_command(text).unwrap();
        // Then
        assert_eq!(
            command,
            "input text 'Hello%s'\\''world'\\''%s$HOME;%s' && input text '%' && input text 's'"
        );
    }

    #[test]
    fn input_limits_reject_unsupported_values() {
        // Given
        let invalid_texts = ["", "line\nbreak", "\t", "Grüße"];
        // When / Then
        for text in invalid_texts {
            assert!(text_command(text).is_err());
        }
        assert!(text_command(&"a".repeat(1025)).is_err());
        assert!(validate_coordinates(&[u32::MAX]).is_err());
        assert!(validate_duration(0).is_err());
        assert!(validate_duration(10001).is_err());
        assert!(validate_duration(1).is_ok());
        assert!(validate_duration(10000).is_ok());
    }
}
