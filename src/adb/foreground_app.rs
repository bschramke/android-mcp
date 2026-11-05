use std::{io, path::Path, time::Duration};

use tokio::time::Instant;

use super::{
    command::{CommandOptions, ensure_success, run_shell},
    device_info::validate_serial,
};

const FOREGROUND_OUTPUT_LIMIT: usize = 128 * 1024;
const FOREGROUND_STDERR_LIMIT: usize = 16 * 1024;

#[derive(Debug, PartialEq, serde::Serialize)]
pub struct ForegroundApp {
    pub serial: String,
    pub package: Option<String>,
    pub activity: Option<String>,
}

pub async fn get_foreground_app(executable: &Path, serial: &str) -> io::Result<ForegroundApp> {
    validate_serial(serial)?;
    let output = run_shell(
        executable,
        serial,
        ["dumpsys", "activity", "activities"],
        "foreground app lookup",
        CommandOptions::new(
            Instant::now() + Duration::from_secs(5),
            FOREGROUND_OUTPUT_LIMIT,
            FOREGROUND_STDERR_LIMIT,
        ),
    )
    .await?;
    ensure_success(&output, "foreground app lookup")?;
    let stdout = std::str::from_utf8(&output.stdout).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("ADB foreground app output is not valid UTF-8: {error}"),
        )
    })?;
    parse_foreground_app(serial, stdout)
}

fn parse_foreground_app(serial: &str, output: &str) -> io::Result<ForegroundApp> {
    let mut result = ForegroundApp {
        serial: serial.to_owned(),
        package: None,
        activity: None,
    };
    for line in output.lines().map(str::trim) {
        let marker = [
            "mResumedActivity:",
            "ResumedActivity:",
            "topResumedActivity=",
        ]
        .iter()
        .find_map(|marker| line.strip_prefix(marker));
        let Some(record) = marker else {
            continue;
        };
        let record = record
            .split_once("ActivityRecord{")
            .map(|(_, record)| record)
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Malformed ADB foreground activity record",
                )
            })?;
        let record = record
            .split_once('}')
            .map(|(record, _)| record)
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Malformed ADB foreground activity record",
                )
            })?;
        let component = record
            .split_whitespace()
            .find(|field| field.contains('/'))
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "ADB foreground activity record has no package component",
                )
            })?;
        let (package, _) = component.split_once('/').ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "ADB foreground activity component is malformed",
            )
        })?;
        if package.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "ADB foreground activity component has an empty package",
            ));
        }
        result.package = Some(package.to_owned());
        result.activity = Some(component.to_owned());
        return Ok(result);
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_m_resumed_activity() {
        let output = "mResumedActivity: ActivityRecord{abc u0 com.example.app/.MainActivity t42}\n";
        assert_eq!(
            parse_foreground_app("emulator-5554", output).unwrap(),
            ForegroundApp {
                serial: "emulator-5554".into(),
                package: Some("com.example.app".into()),
                activity: Some("com.example.app/.MainActivity".into()),
            }
        );
    }

    #[test]
    fn parses_resumed_activity_fallback() {
        let output =
            "ResumedActivity: ActivityRecord{abc u0 com.example.app/com.example.app.Home t42}\n";
        let result = parse_foreground_app("emulator-5556", output).unwrap();
        assert_eq!(result.package.as_deref(), Some("com.example.app"));
        assert_eq!(
            result.activity.as_deref(),
            Some("com.example.app/com.example.app.Home")
        );
    }

    #[test]
    fn returns_unknown_when_no_foreground_activity_is_reported() {
        let result =
            parse_foreground_app("emulator-5554", "ACTIVITY MANAGER ACTIVITIES\n").unwrap();
        assert_eq!(result.package, None);
        assert_eq!(result.activity, None);
    }

    #[test]
    fn rejects_malformed_foreground_activity_records() {
        assert!(parse_foreground_app("emulator-5554", "mResumedActivity: invalid\n").is_err());
        assert!(
            parse_foreground_app(
                "emulator-5554",
                "mResumedActivity: ActivityRecord{no-component}\n"
            )
            .is_err()
        );
    }
}
