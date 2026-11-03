use std::{ffi::OsStr, io, path::Path, time::Duration};

use tokio::time::Instant;

use super::super::{
    command::{CommandOptions, ensure_success, run_shell},
    device_info::validate_serial,
    model::{DisplayInfo, DisplayOrientation, ScreenDimensions},
};

const DISPLAY_OUTPUT_LIMIT: usize = 128 * 1024;
const DISPLAY_STDERR_LIMIT: usize = 16 * 1024;

pub async fn display_info(executable: &Path, serial: &str) -> io::Result<DisplayInfo> {
    validate_serial(serial)?;
    display_info_until(executable, serial, Instant::now() + Duration::from_secs(5)).await
}

pub(in crate::adb) async fn display_info_until(
    executable: &Path,
    serial: &str,
    deadline: Instant,
) -> io::Result<DisplayInfo> {
    let size = run_display_query(executable, serial, ["wm", "size"], "wm size", deadline).await?;
    let density = run_display_query(
        executable,
        serial,
        ["wm", "density"],
        "wm density",
        deadline,
    )
    .await?;
    let window = run_display_query(
        executable,
        serial,
        ["dumpsys", "window", "displays"],
        "dumpsys window displays",
        deadline,
    )
    .await?;

    Ok(parse_display_info(&size, &density, &window))
}

async fn run_display_query<I, S>(
    executable: &Path,
    serial: &str,
    arguments: I,
    description: &str,
    deadline: Instant,
) -> io::Result<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let output = run_shell(
        executable,
        serial,
        arguments,
        description,
        CommandOptions::new(deadline, DISPLAY_OUTPUT_LIMIT, DISPLAY_STDERR_LIMIT)
            .with_stdout_label("output"),
    )
    .await?;
    ensure_success(&output, description)?;
    String::from_utf8(output.stdout).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("ADB {description} output is not valid UTF-8: {error}"),
        )
    })
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
