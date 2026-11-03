use std::{io, path::Path, process::Stdio, time::Duration};

use serde::Serialize;
use tokio::{process::Command, time::timeout};

use super::{devices::list_devices, executable::adb_version, model::DeviceStatus};

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HealthStatus {
    Healthy,
    Unhealthy,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckStatus {
    Passed,
    Failed,
    Skipped,
}

#[derive(Debug, Serialize)]
pub struct HealthCheck {
    pub name: &'static str,
    pub status: CheckStatus,
    pub message: String,
}

#[derive(Debug, Serialize)]
pub struct HealthCheckReport {
    pub status: HealthStatus,
    pub checks: Vec<HealthCheck>,
    pub devices: Vec<super::model::AndroidDevice>,
    pub shell_uid: Option<u32>,
    pub shell_is_root: Option<bool>,
}

pub async fn health_check(executable: &Path, serial: Option<&str>) -> HealthCheckReport {
    let mut checks = Vec::new();
    let mut devices = Vec::new();
    let mut shell_uid = None;
    let mut shell_is_root = None;

    match adb_version(executable).await {
        Ok(output) => match parse_version_output(&output) {
            Ok(version) => checks.push(HealthCheck {
                name: "adb_binary",
                status: CheckStatus::Passed,
                message: version,
            }),
            Err(error) => checks.push(failed("adb_binary", error.to_string())),
        },
        Err(error) => checks.push(failed("adb_binary", error.to_string())),
    }

    match list_devices(executable).await {
        Ok(device_list) => {
            devices = device_list.devices;
            checks.push(HealthCheck {
                name: "adb_server",
                status: CheckStatus::Passed,
                message: format!("ADB server responded; {} device(s) listed", devices.len()),
            });
            match serial {
                Some(serial) => {
                    check_device(
                        executable,
                        serial,
                        &devices,
                        &mut checks,
                        &mut shell_uid,
                        &mut shell_is_root,
                    )
                    .await
                }
                None => skip_device_checks(&mut checks),
            }
        }
        Err(error) => {
            checks.push(failed("adb_server", error.to_string()));
            skip_device_checks(&mut checks);
        }
    }

    let status = if checks
        .iter()
        .any(|check| matches!(check.status, CheckStatus::Failed))
    {
        HealthStatus::Unhealthy
    } else {
        HealthStatus::Healthy
    };

    HealthCheckReport {
        status,
        checks,
        devices,
        shell_uid,
        shell_is_root,
    }
}

fn parse_version_output(output: &[u8]) -> io::Result<String> {
    std::str::from_utf8(output)
        .map_err(|error| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("ADB version output is not valid UTF-8: {error}"),
            )
        })?
        .lines()
        .find(|line| !line.trim().is_empty())
        .map(str::to_owned)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "ADB version output is empty"))
}

async fn check_device(
    executable: &Path,
    serial: &str,
    devices: &[super::model::AndroidDevice],
    checks: &mut Vec<HealthCheck>,
    shell_uid: &mut Option<u32>,
    shell_is_root: &mut Option<bool>,
) {
    if serial.is_empty()
        || serial
            .chars()
            .any(|character| character.is_whitespace() || character == '\0')
    {
        checks.push(failed(
            "device_connection",
            "Device serial must be nonempty and contain no whitespace or NUL characters".into(),
        ));
        skip_check(checks, "device_authorization", "Invalid device serial");
        skip_check(checks, "shell_access", "Invalid device serial");
        return;
    }

    let Some(device) = devices.iter().find(|device| device.serial == serial) else {
        checks.push(failed(
            "device_connection",
            format!("Device {serial} is not listed by ADB"),
        ));
        skip_check(
            checks,
            "device_authorization",
            "Device is not listed by ADB",
        );
        skip_check(checks, "shell_access", "Device is not listed by ADB");
        return;
    };

    match device.status {
        DeviceStatus::Online => {
            checks.push(HealthCheck {
                name: "device_connection",
                status: CheckStatus::Passed,
                message: format!("Device {serial} is online"),
            });
            checks.push(HealthCheck {
                name: "device_authorization",
                status: CheckStatus::Passed,
                message: "ADB reports the device as authorized".into(),
            });
            match shell_uid_for(executable, serial).await {
                Ok(uid) => {
                    *shell_uid = Some(uid);
                    *shell_is_root = Some(uid == 0);
                    checks.push(HealthCheck {
                        name: "shell_access",
                        status: CheckStatus::Passed,
                        message: format!(
                            "ADB shell command succeeded with UID {uid}; adb root was not tested"
                        ),
                    });
                }
                Err(error) => checks.push(failed("shell_access", error.to_string())),
            }
        }
        DeviceStatus::Unauthorized => {
            checks.push(failed(
                "device_connection",
                format!("Device {serial} is not available for ADB commands"),
            ));
            checks.push(failed(
                "device_authorization",
                format!("Device {serial} is unauthorized; approve the debugging prompt"),
            ));
            skip_check(checks, "shell_access", "Device is unauthorized");
        }
        DeviceStatus::Offline => {
            checks.push(failed(
                "device_connection",
                format!("Device {serial} is offline"),
            ));
            skip_check(checks, "device_authorization", "Device is offline");
            skip_check(checks, "shell_access", "Device is offline");
        }
        DeviceStatus::PermissionDenied => {
            checks.push(failed(
                "device_connection",
                format!("Host permissions prevent access to device {serial}"),
            ));
            skip_check(
                checks,
                "device_authorization",
                "Host permissions prevent access",
            );
            skip_check(checks, "shell_access", "Host permissions prevent access");
        }
        DeviceStatus::Unknown => {
            checks.push(failed(
                "device_connection",
                format!(
                    "Device {serial} has unsupported ADB state: {}",
                    device.adb_state
                ),
            ));
            skip_check(
                checks,
                "device_authorization",
                "ADB device state is unknown",
            );
            skip_check(checks, "shell_access", "ADB device state is unknown");
        }
    }
}

async fn shell_uid_for(executable: &Path, serial: &str) -> io::Result<u32> {
    let output = timeout(
        Duration::from_secs(5),
        Command::new(executable)
            .args(["-s", serial, "shell", "id", "-u"])
            .stdin(Stdio::null())
            .kill_on_drop(true)
            .output(),
    )
    .await
    .map_err(|_| {
        io::Error::new(
            io::ErrorKind::TimedOut,
            "ADB shell access check timed out after 5 seconds",
        )
    })?
    .map_err(|error| {
        io::Error::new(
            error.kind(),
            format!("Cannot execute ADB shell access check: {error}"),
        )
    })?;

    if !output.status.success() {
        return Err(io::Error::other(format!(
            "ADB shell access check failed ({}): {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }

    std::str::from_utf8(&output.stdout)
        .map_err(|error| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("ADB shell UID output is not valid UTF-8: {error}"),
            )
        })?
        .trim()
        .parse::<u32>()
        .map_err(|error| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("ADB shell returned an invalid UID: {error}"),
            )
        })
}

fn failed(name: &'static str, error: String) -> HealthCheck {
    HealthCheck {
        name,
        status: CheckStatus::Failed,
        message: error,
    }
}

fn skip_device_checks(checks: &mut Vec<HealthCheck>) {
    skip_check(
        checks,
        "device_connection",
        "Provide a serial to check a device",
    );
    skip_check(
        checks,
        "device_authorization",
        "Provide a serial to check authorization",
    );
    skip_check(
        checks,
        "shell_access",
        "Provide a serial to check shell access",
    );
}

fn skip_check(checks: &mut Vec<HealthCheck>, name: &'static str, message: &str) {
    checks.push(HealthCheck {
        name,
        status: CheckStatus::Skipped,
        message: message.to_owned(),
    });
}
