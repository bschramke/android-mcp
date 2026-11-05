use std::{io, path::Path, time::Duration};

use tokio::time::Instant;

use super::{
    command::{CommandOptions, ensure_success, run_shell},
    device_info::validate_serial,
    logcat::validate_package,
};

const APP_INFO_OUTPUT_LIMIT: usize = 256 * 1024;
const APP_INFO_STDERR_LIMIT: usize = 16 * 1024;

#[derive(Debug, PartialEq, serde::Serialize)]
pub struct AppInfo {
    pub serial: String,
    pub package: String,
    pub version_code: Option<u64>,
    pub version_name: Option<String>,
    pub min_sdk: Option<u32>,
    pub target_sdk: Option<u32>,
    pub first_install_time: Option<String>,
    pub last_update_time: Option<String>,
    pub installer_package_name: Option<String>,
    pub code_path: Option<String>,
}

pub async fn app_info(executable: &Path, serial: &str, package: &str) -> io::Result<AppInfo> {
    validate_serial(serial)?;
    validate_package(package)?;

    let output = run_shell(
        executable,
        serial,
        ["dumpsys", "package", package],
        "app info",
        CommandOptions::new(
            Instant::now() + Duration::from_secs(5),
            APP_INFO_OUTPUT_LIMIT,
            APP_INFO_STDERR_LIMIT,
        ),
    )
    .await?;
    ensure_success(&output, "app info")?;
    let stdout = std::str::from_utf8(&output.stdout).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("ADB app info output is not valid UTF-8: {error}"),
        )
    })?;
    parse_app_info(serial, package, stdout)
}

fn parse_app_info(serial: &str, package: &str, output: &str) -> io::Result<AppInfo> {
    let mut info = AppInfo {
        serial: serial.to_owned(),
        package: package.to_owned(),
        version_code: None,
        version_name: None,
        min_sdk: None,
        target_sdk: None,
        first_install_time: None,
        last_update_time: None,
        installer_package_name: None,
        code_path: None,
    };
    let mut in_package = false;
    let mut found_package = false;

    for line in output.lines() {
        let line = line.trim();
        if let Some(header_package) = package_header_name(line) {
            if in_package {
                break;
            }
            in_package = header_package == package;
            found_package |= in_package;
            continue;
        }
        if !in_package {
            continue;
        }

        if line.starts_with("versionCode=") {
            for field in line.split_whitespace() {
                let Some((key, value)) = field.split_once('=') else {
                    continue;
                };
                match key {
                    "versionCode" => info.version_code = parse_number(value, key)?.map(u64::from),
                    "minSdk" => info.min_sdk = parse_number(value, key)?,
                    "targetSdk" => info.target_sdk = parse_number(value, key)?,
                    _ => {}
                }
            }
        } else if let Some(value) = line.strip_prefix("versionName=") {
            info.version_name = parse_text(value);
        } else if let Some(value) = line.strip_prefix("firstInstallTime=") {
            info.first_install_time = parse_text(value);
        } else if let Some(value) = line.strip_prefix("lastUpdateTime=") {
            info.last_update_time = parse_text(value);
        } else if let Some(value) = line.strip_prefix("installerPackageName=") {
            info.installer_package_name = parse_text(value);
        } else if let Some(value) = line.strip_prefix("codePath=") {
            info.code_path = parse_text(value);
        }
    }

    if !found_package {
        let kind = if output.contains("Unable to find package") {
            io::ErrorKind::NotFound
        } else {
            io::ErrorKind::InvalidData
        };
        return Err(io::Error::new(
            kind,
            format!("ADB package dump does not contain package {package}"),
        ));
    }
    Ok(info)
}

fn package_header_name(line: &str) -> Option<&str> {
    line.strip_prefix("Package [")?
        .split_once(']')
        .map(|(package, _)| package)
}

fn parse_text(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty() && value != "null").then(|| value.to_owned())
}

fn parse_number(value: &str, field: &str) -> io::Result<Option<u32>> {
    if value.is_empty() || value == "null" || value == "none" {
        return Ok(None);
    }
    value.parse().map(Some).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("ADB app info field {field} is not a valid unsigned integer"),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_package_metadata_from_dumpsys_output() {
        let output = include_str!("../../tests/fixtures/apps/app-info-dumpsys.txt");
        assert_eq!(
            parse_app_info("emulator-5554", "com.example.app", output).unwrap(),
            AppInfo {
                serial: "emulator-5554".into(),
                package: "com.example.app".into(),
                version_code: Some(42),
                version_name: Some("2.3.1".into()),
                min_sdk: Some(26),
                target_sdk: Some(35),
                first_install_time: Some("2025-01-15 10:30:45".into()),
                last_update_time: Some("2025-02-20 11:45:00".into()),
                installer_package_name: Some("com.android.vending".into()),
                code_path: Some("/data/app/com.example.app/base.apk".into()),
            }
        );
    }

    #[test]
    fn parses_full_chrome_dumpsys_fixture() {
        let output = include_str!(
            "../../tests/fixtures/apps/emulator-5554-dumpsyspkg-com.android.chrome.txt"
        );
        let info = parse_app_info("emulator-5554", "com.android.chrome", output).unwrap();
        assert_eq!(info.version_code, Some(447211484));
        assert_eq!(info.version_name.as_deref(), Some("91.0.4472.114"));
        assert_eq!(info.min_sdk, Some(29));
        assert_eq!(info.target_sdk, Some(30));
        assert_eq!(
            info.first_install_time.as_deref(),
            Some("2024-07-24 23:42:36")
        );
        assert_eq!(
            info.last_update_time.as_deref(),
            Some("2024-07-24 23:42:36")
        );
        assert_eq!(info.installer_package_name, None);
        assert_eq!(info.code_path.as_deref(), Some("/product/app/Chrome64"));
    }

    #[test]
    fn missing_optional_fields_remain_unknown() {
        let output = "Packages:\n  Package [com.example.app] (id):\n    versionCode=1\n";
        let info = parse_app_info("emulator-5554", "com.example.app", output).unwrap();
        assert_eq!(info.version_code, Some(1));
        assert_eq!(info.version_name, None);
        assert_eq!(info.last_update_time, None);
    }

    #[test]
    fn rejects_missing_package_blocks() {
        assert!(parse_app_info("emulator-5554", "com.example.app", "Packages:\n").is_err());
        assert!(
            parse_app_info(
                "emulator-5554",
                "com.example.app",
                "Unable to find package: com.example.app\n"
            )
            .is_err()
        );
    }
}
