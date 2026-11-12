use std::{ffi::OsString, fs, io, path::Path, time::Duration};

use tokio::time::Instant;

use super::{
    command::{CommandOptions, ensure_success, run_adb, run_shell},
    device_info::validate_serial,
    logcat::validate_package,
};

const APPS_OUTPUT_LIMIT: usize = 256 * 1024;
const APPS_STDERR_LIMIT: usize = 16 * 1024;
const APP_INSTALL_TIMEOUT: Duration = Duration::from_secs(120);
const APP_UNINSTALL_TIMEOUT: Duration = Duration::from_secs(30);
const APP_CLEAR_DATA_TIMEOUT: Duration = Duration::from_secs(30);
const APP_LAUNCH_TIMEOUT: Duration = Duration::from_secs(30);
const APP_TERMINATE_TIMEOUT: Duration = Duration::from_secs(30);
const APP_ACTION_STDOUT_LIMIT: usize = 64 * 1024;
const APP_ACTION_STDERR_LIMIT: usize = 16 * 1024;

#[derive(Debug, PartialEq, serde::Serialize)]
pub struct AppList {
    pub serial: String,
    pub apps: Vec<String>,
}

pub async fn install_app(
    executable: &Path,
    serial: &str,
    apk_path: &Path,
    replace: bool,
    downgrade: bool,
) -> io::Result<()> {
    validate_serial(serial)?;
    validate_apk_path(apk_path)?;

    let mut arguments = vec![
        OsString::from("-s"),
        OsString::from(serial),
        OsString::from("install"),
    ];
    if replace || downgrade {
        arguments.push(OsString::from("-r"));
    }
    if downgrade {
        arguments.push(OsString::from("-d"));
    }
    arguments.push(apk_path.as_os_str().to_os_string());

    let output = run_adb(
        executable,
        arguments,
        "app installation",
        CommandOptions::new(
            Instant::now() + APP_INSTALL_TIMEOUT,
            APP_ACTION_STDOUT_LIMIT,
            APP_ACTION_STDERR_LIMIT,
        )
        .with_timeout_message("ADB app installation timed out after 120 seconds"),
    )
    .await?;
    ensure_success(&output, "app installation")
}

pub async fn uninstall_app(executable: &Path, serial: &str, package: &str) -> io::Result<()> {
    validate_serial(serial)?;
    validate_package(package)?;

    let output = run_adb(
        executable,
        ["-s", serial, "uninstall", package],
        "app uninstallation",
        CommandOptions::new(
            Instant::now() + APP_UNINSTALL_TIMEOUT,
            APP_ACTION_STDOUT_LIMIT,
            APP_ACTION_STDERR_LIMIT,
        )
        .with_timeout_message("ADB app uninstallation timed out after 30 seconds"),
    )
    .await?;
    ensure_success(&output, "app uninstallation")
}

pub async fn clear_app_data(executable: &Path, serial: &str, package: &str) -> io::Result<()> {
    validate_serial(serial)?;
    validate_package(package)?;

    let output = run_shell(
        executable,
        serial,
        ["pm", "clear", package],
        "app data clear",
        CommandOptions::new(
            Instant::now() + APP_CLEAR_DATA_TIMEOUT,
            APP_ACTION_STDOUT_LIMIT,
            APP_ACTION_STDERR_LIMIT,
        )
        .with_timeout_message("ADB app data clear timed out after 30 seconds"),
    )
    .await?;
    ensure_success(&output, "app data clear")?;
    let stdout = std::str::from_utf8(&output.stdout).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("ADB app data clear output is not valid UTF-8: {error}"),
        )
    })?;
    if stdout.trim() != "Success" {
        return Err(io::Error::other(format!(
            "ADB app data clear did not report Success for {package}: {}",
            stdout.trim()
        )));
    }
    Ok(())
}

pub async fn launch_app(executable: &Path, serial: &str, package: &str) -> io::Result<()> {
    validate_serial(serial)?;
    validate_package(package)?;

    let output = run_shell(
        executable,
        serial,
        [
            "am",
            "start",
            "-W",
            "-a",
            "android.intent.action.MAIN",
            "-c",
            "android.intent.category.LAUNCHER",
            "-p",
            package,
        ],
        "app launch",
        CommandOptions::new(
            Instant::now() + APP_LAUNCH_TIMEOUT,
            APP_ACTION_STDOUT_LIMIT,
            APP_ACTION_STDERR_LIMIT,
        )
        .with_timeout_message("ADB app launch timed out after 30 seconds"),
    )
    .await?;
    ensure_success(&output, "app launch")
}

pub async fn terminate_app(executable: &Path, serial: &str, package: &str) -> io::Result<()> {
    validate_serial(serial)?;
    validate_package(package)?;

    let output = run_shell(
        executable,
        serial,
        ["am", "force-stop", package],
        "app termination",
        CommandOptions::new(
            Instant::now() + APP_TERMINATE_TIMEOUT,
            APP_ACTION_STDOUT_LIMIT,
            APP_ACTION_STDERR_LIMIT,
        )
        .with_timeout_message("ADB app termination timed out after 30 seconds"),
    )
    .await?;
    ensure_success(&output, "app termination")
}

fn validate_apk_path(path: &Path) -> io::Result<()> {
    if !path.is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "APK path must be absolute",
        ));
    }
    if !path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("apk"))
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "APK path must use the .apk extension",
        ));
    }
    let metadata = fs::metadata(path)?;
    if !metadata.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "APK path must point to a file",
        ));
    }
    Ok(())
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
