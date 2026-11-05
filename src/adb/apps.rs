use std::{io, path::Path, time::Duration};

use tokio::time::Instant;

use super::{
    command::{CommandOptions, ensure_success, run_shell},
    device_info::validate_serial,
};

const APPS_OUTPUT_LIMIT: usize = 256 * 1024;
const APPS_STDERR_LIMIT: usize = 16 * 1024;

#[derive(Debug, PartialEq, serde::Serialize)]
pub struct AppList {
    pub serial: String,
    pub apps: Vec<String>,
}

pub async fn list_apps(
    executable: &Path,
    serial: &str,
    exclude_system: bool,
) -> io::Result<AppList> {
    validate_serial(serial)?;
    let mut arguments = vec!["pm", "list", "packages"];
    if exclude_system {
        arguments.push("-3");
    }
    let output = run_shell(
        executable,
        serial,
        arguments,
        "app listing",
        CommandOptions::new(
            Instant::now() + Duration::from_secs(5),
            APPS_OUTPUT_LIMIT,
            APPS_STDERR_LIMIT,
        ),
    )
    .await?;
    ensure_success(&output, "app listing")?;
    let stdout = std::str::from_utf8(&output.stdout).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("ADB app listing is not valid UTF-8: {error}"),
        )
    })?;
    parse_apps(serial, stdout)
}

fn parse_apps(serial: &str, output: &str) -> io::Result<AppList> {
    let mut apps = Vec::new();
    for (index, line) in output.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let app = line
            .strip_prefix("package:")
            .filter(|app| !app.is_empty() && !app.chars().any(char::is_whitespace))
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("Malformed ADB app listing entry on line {}", index + 1),
                )
            })?;
        apps.push(app.to_owned());
    }
    apps.sort_unstable();
    apps.dedup();
    Ok(AppList {
        serial: serial.to_owned(),
        apps,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_sorts_package_names() {
        let result = parse_apps(
            "emulator-5554",
            "package:org.example\npackage:com.example.app\npackage:org.example\n",
        )
        .unwrap();
        assert_eq!(
            result,
            AppList {
                serial: "emulator-5554".into(),
                apps: vec!["com.example.app".into(), "org.example".into()],
            }
        );
    }

    #[test]
    fn accepts_empty_output() {
        assert!(parse_apps("emulator-5554", "\n").unwrap().apps.is_empty());
    }

    #[test]
    fn rejects_malformed_entries() {
        assert!(parse_apps("emulator-5554", "not package output\n").is_err());
        assert!(parse_apps("emulator-5554", "package:\n").is_err());
    }
}
