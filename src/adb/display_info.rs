use std::{io, path::Path, process::Stdio, time::Duration};

use tokio::{
    io::{AsyncRead, AsyncReadExt},
    process::{Child, Command},
    time::{Instant, timeout, timeout_at},
};

use super::{
    device_info::validate_serial,
    model::{DisplayInfo, DisplayOrientation, ScreenDimensions},
};

const DISPLAY_OUTPUT_LIMIT: usize = 128 * 1024;
const DISPLAY_STDERR_LIMIT: usize = 16 * 1024;

pub async fn display_info(executable: &Path, serial: &str) -> io::Result<DisplayInfo> {
    validate_serial(serial)?;
    display_info_until(executable, serial, Instant::now() + Duration::from_secs(5)).await
}

pub(super) async fn display_info_until(
    executable: &Path,
    serial: &str,
    deadline: Instant,
) -> io::Result<DisplayInfo> {
    let size = run_shell(executable, serial, &["wm", "size"], "wm size", deadline).await?;
    let density = run_shell(
        executable,
        serial,
        &["wm", "density"],
        "wm density",
        deadline,
    )
    .await?;
    let window = run_shell(
        executable,
        serial,
        &["dumpsys", "window", "displays"],
        "dumpsys window displays",
        deadline,
    )
    .await?;

    Ok(parse_display_info(&size, &density, &window))
}

async fn run_shell(
    executable: &Path,
    serial: &str,
    arguments: &[&str],
    description: &str,
    deadline: Instant,
) -> io::Result<String> {
    let mut child = Command::new(executable)
        .args(["-s", serial, "shell"])
        .args(arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|error| {
            io::Error::new(
                error.kind(),
                format!("Cannot execute ADB {description}: {error}"),
            )
        })?;

    let result = match timeout_at(deadline, capture(&mut child, description)).await {
        Ok(result) => result,
        Err(_) => Err(io::Error::new(
            io::ErrorKind::TimedOut,
            format!("ADB {description} timed out after 5 seconds"),
        )),
    };
    if result.is_err() {
        let _ = timeout(Duration::from_secs(1), child.kill()).await;
    }
    result
}

async fn capture(child: &mut Child, description: &str) -> io::Result<String> {
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| io::Error::other("Missing ADB display stdout"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| io::Error::other("Missing ADB display stderr"))?;
    let stdout_read = read_capped(stdout, DISPLAY_OUTPUT_LIMIT);
    let stderr_read = read_capped(stderr, DISPLAY_STDERR_LIMIT);
    tokio::pin!(stdout_read, stderr_read);
    let mut output = None;
    let mut diagnostic = None;

    while output.is_none() || diagnostic.is_none() {
        tokio::select! {
            result = &mut stdout_read, if output.is_none() => {
                let (bytes, exceeded) = result?;
                if exceeded {
                    return Err(io::Error::other(format!("ADB {description} output exceeded the 128 KiB limit")));
                }
                output = Some(bytes);
            }
            result = &mut stderr_read, if diagnostic.is_none() => {
                let (bytes, exceeded) = result?;
                if exceeded {
                    return Err(io::Error::other(format!("ADB {description} stderr exceeded the 16 KiB limit")));
                }
                diagnostic = Some(bytes);
            }
        }
    }

    let status = child.wait().await?;
    if !status.success() {
        return Err(io::Error::other(format!(
            "ADB {description} failed ({status}): {}",
            String::from_utf8_lossy(&diagnostic.unwrap_or_default()).trim()
        )));
    }

    String::from_utf8(output.unwrap_or_default()).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("ADB {description} output is not valid UTF-8: {error}"),
        )
    })
}

async fn read_capped(reader: impl AsyncRead + Unpin, limit: usize) -> io::Result<(Vec<u8>, bool)> {
    let mut bytes = Vec::new();
    reader
        .take((limit + 1) as u64)
        .read_to_end(&mut bytes)
        .await?;
    let exceeded = bytes.len() > limit;
    bytes.truncate(limit);
    Ok((bytes, exceeded))
}

fn parse_display_info(size: &str, density: &str, window: &str) -> DisplayInfo {
    let physical_resolution_px = parse_size(size, "Physical size:");
    let override_resolution_px = parse_size(size, "Override size:");
    let physical_density = parse_number(density, "Physical density:");
    let override_density = parse_number(density, "Override density:");
    let density_dpi = override_density.or(physical_density);
    let rotation_degrees = parse_rotation(window);
    let base_resolution = override_resolution_px.or(physical_resolution_px);
    let current_resolution_px = base_resolution.map(|resolution| {
        if matches!(rotation_degrees, Some(90 | 270)) {
            ScreenDimensions {
                width: resolution.height,
                height: resolution.width,
            }
        } else {
            resolution
        }
    });
    let size_dp = current_resolution_px
        .zip(density_dpi)
        .and_then(|(resolution, density)| {
            Some(ScreenDimensions {
                width: pixels_to_dp(resolution.width, density)?,
                height: pixels_to_dp(resolution.height, density)?,
            })
        });
    let orientation = rotation_degrees.and_then(|rotation| {
        let natural_resolution = physical_resolution_px.or(base_resolution)?;
        Some(orientation_for(rotation, natural_resolution))
    });

    DisplayInfo {
        physical_resolution_px,
        override_resolution_px,
        current_resolution_px,
        density_dpi,
        size_dp,
        rotation_degrees,
        orientation,
    }
}

fn parse_size(output: &str, label: &str) -> Option<ScreenDimensions> {
    output.lines().find_map(|line| {
        let value = line.trim().strip_prefix(label)?.trim();
        let (width, height) = value.split_once('x')?;
        let dimensions = ScreenDimensions {
            width: width.parse().ok()?,
            height: height.parse().ok()?,
        };
        (dimensions.width > 0 && dimensions.height > 0).then_some(dimensions)
    })
}

fn parse_number(output: &str, label: &str) -> Option<u32> {
    output.lines().find_map(|line| {
        let value = line.trim().strip_prefix(label)?.trim().parse().ok()?;
        (value > 0).then_some(value)
    })
}

fn parse_rotation(output: &str) -> Option<u16> {
    for line in output.lines() {
        for label in ["mCurrentRotation=", "mRotation="] {
            let Some((_, value)) = line.split_once(label) else {
                continue;
            };
            let value = value
                .split_whitespace()
                .next()?
                .trim_matches(|character: char| {
                    !character.is_ascii_alphanumeric() && character != '_'
                });
            return parse_rotation_value(value);
        }
    }
    None
}

fn parse_rotation_value(value: &str) -> Option<u16> {
    let value = value.strip_prefix("ROTATION_").unwrap_or(value);
    let value = value.parse::<u16>().ok()?;
    match value {
        0..=3 => Some(value * 90),
        90 | 180 | 270 => Some(value),
        _ => None,
    }
}

fn orientation_for(rotation: u16, natural_resolution: ScreenDimensions) -> DisplayOrientation {
    let natural_is_landscape = natural_resolution.width > natural_resolution.height;
    match (rotation, natural_is_landscape) {
        (0, false) => DisplayOrientation::Portrait,
        (0, true) => DisplayOrientation::Landscape,
        (90, false) => DisplayOrientation::Landscape,
        (90, true) => DisplayOrientation::Portrait,
        (180, false) => DisplayOrientation::ReversePortrait,
        (180, true) => DisplayOrientation::ReverseLandscape,
        (270, false) => DisplayOrientation::ReverseLandscape,
        (270, true) => DisplayOrientation::ReversePortrait,
        _ => unreachable!("rotation parser only returns right-angle rotations"),
    }
}

fn pixels_to_dp(pixels: u32, density_dpi: u32) -> Option<u32> {
    if density_dpi == 0 {
        return None;
    }
    u32::try_from((u64::from(pixels) * 160 + u64::from(density_dpi) / 2) / u64::from(density_dpi))
        .ok()
}
