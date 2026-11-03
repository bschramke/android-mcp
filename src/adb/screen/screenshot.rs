use std::{
    fs::OpenOptions,
    io::{self, Write},
    path::Path,
    time::Duration,
};

use base64::{Engine, engine::general_purpose::STANDARD};
use tokio::time::Instant;

use super::super::{
    command::{CommandOptions, ensure_success, run_adb},
    device_info::validate_serial,
};

const SCREENSHOT_LIMIT: usize = 16 * 1024 * 1024;
const SCREENSHOT_STDERR_LIMIT: usize = 16 * 1024;
const PNG_SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";

pub async fn take_screenshot(executable: &Path, serial: &str) -> io::Result<String> {
    let screenshot = capture(executable, serial).await?;
    Ok(STANDARD.encode(screenshot))
}

pub async fn save_screenshot(
    executable: &Path,
    serial: &str,
    path: &Path,
    overwrite: bool,
) -> io::Result<usize> {
    validate_destination(path)?;
    let screenshot = capture(executable, serial).await?;
    let mut file = if overwrite {
        OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(path)?
    } else {
        OpenOptions::new().write(true).create_new(true).open(path)?
    };
    file.write_all(&screenshot)?;
    Ok(screenshot.len())
}

async fn capture(executable: &Path, serial: &str) -> io::Result<Vec<u8>> {
    validate_serial(serial)?;
    let output = run_adb(
        executable,
        ["-s", serial, "exec-out", "screencap", "-p"],
        "screenshot",
        CommandOptions::new(
            Instant::now() + Duration::from_secs(5),
            SCREENSHOT_LIMIT,
            SCREENSHOT_STDERR_LIMIT,
        ),
    )
    .await?;
    ensure_success(&output, "screenshot")?;
    if !output.stdout.starts_with(PNG_SIGNATURE) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "ADB screenshot output is not a PNG image",
        ));
    }
    Ok(output.stdout)
}

fn validate_destination(path: &Path) -> io::Result<()> {
    if !path.is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Screenshot path must be absolute",
        ));
    }
    if !path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("png"))
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Screenshot path must use the .png extension",
        ));
    }
    Ok(())
}
