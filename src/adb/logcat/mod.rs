use std::{ffi::OsString, io, path::Path, time::Duration};

use tokio::time::Instant;

mod filters;
mod package;

pub(super) use package::validate as validate_package;

use super::{
    command::{CommandOptions, OverflowBehavior, ensure_success, run_adb},
    model::{LogPriority, LogcatSnapshot},
};

const OUTPUT_LIMIT: usize = 256 * 1024;
const STDERR_LIMIT: usize = 16 * 1024;

pub async fn logcat(
    executable: &Path,
    serial: &str,
    max_lines: u32,
    pid: Option<u32>,
    tags: &[String],
    priority: Option<LogPriority>,
    package: Option<&str>,
) -> io::Result<LogcatSnapshot> {
    if serial.is_empty()
        || serial
            .chars()
            .any(|character| character.is_whitespace() || character == '\0')
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Device serial must be nonempty and contain no whitespace or NUL characters",
        ));
    }
    if !(1..=2000).contains(&max_lines) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "max_lines must be between 1 and 2000",
        ));
    }
    if package.is_some() && pid.is_some() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "package and pid cannot be combined",
        ));
    }
    if let Some(package) = package {
        package::validate(package)?;
    }
    let mut filters = filters::filter_arguments(pid, tags, priority)?;
    let deadline = Instant::now() + Duration::from_secs(5);
    if let Some(package) = package {
        let package_pid = package::resolve_pid(executable, serial, package, deadline).await?;
        filters.insert(0, format!("--pid={package_pid}"));
    }
    if Instant::now() >= deadline {
        return Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "ADB logcat timed out after 5 seconds",
        ));
    }
    let lines = max_lines.to_string();
    let mut arguments = vec![
        OsString::from("-s"),
        OsString::from(serial),
        OsString::from("logcat"),
        OsString::from("-d"),
        OsString::from("-t"),
        OsString::from(lines),
        OsString::from("-v"),
        OsString::from("threadtime"),
    ];
    arguments.extend(filters.into_iter().map(OsString::from));
    let output = run_adb(
        executable,
        arguments,
        "logcat",
        CommandOptions::new(deadline, OUTPUT_LIMIT, STDERR_LIMIT)
            .with_overflow_behavior(OverflowBehavior::TruncateStdout),
    )
    .await?;
    if output.stdout_truncated {
        return snapshot(serial, output.stdout, true);
    }
    ensure_success(&output, "logcat")?;
    snapshot(serial, output.stdout, false)
}

fn snapshot(serial: &str, bytes: Vec<u8>, truncated: bool) -> io::Result<LogcatSnapshot> {
    let output = match std::str::from_utf8(&bytes) {
        Ok(text) => text.to_owned(),
        Err(error) if truncated && error.error_len().is_none() => {
            String::from_utf8(bytes[..error.valid_up_to()].to_vec()).map_err(io::Error::other)?
        }
        Err(_) => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "ADB logcat output is not valid UTF-8",
            ));
        }
    };
    Ok(LogcatSnapshot {
        serial: serial.to_owned(),
        format: "threadtime".to_owned(),
        output,
        truncated,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adb::command::read_capped;

    #[tokio::test]
    async fn capped_reader_distinguishes_exact_limit_from_overflow() {
        assert_eq!(
            read_capped(&b"1234"[..], 4).await.unwrap(),
            (b"1234".to_vec(), false)
        );
        assert_eq!(
            read_capped(&b"12345"[..], 4).await.unwrap(),
            (b"1234".to_vec(), true)
        );
    }

    #[test]
    fn truncation_drops_only_an_incomplete_utf8_suffix() {
        let bytes = vec![b'a', 0xe2, 0x82];
        assert_eq!(snapshot("serial", bytes.clone(), true).unwrap().output, "a");
        assert!(snapshot("serial", bytes, false).is_err());
        assert!(snapshot("serial", vec![0xff], true).is_err());
    }
}
