use std::{io, path::Path, process::Stdio, time::Duration};

use tokio::{
    io::{AsyncRead, AsyncReadExt},
    process::Command,
    time::{Instant, timeout, timeout_at},
};

pub(super) fn validate(package: &str) -> io::Result<()> {
    let valid = package.len() <= 255
        && package.contains('.')
        && package.split('.').all(|segment| {
            segment
                .as_bytes()
                .first()
                .is_some_and(u8::is_ascii_alphabetic)
                && segment
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        });
    if !valid {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "package must be a dotted Android application ID of at most 255 ASCII characters; each segment must start with a letter and contain only letters, digits, or underscores",
        ));
    }
    Ok(())
}

pub(super) async fn resolve_pid(
    executable: &Path,
    serial: &str,
    package: &str,
    deadline: Instant,
) -> io::Result<u32> {
    let mut child = Command::new(executable)
        .args(["-s", serial, "shell", "pidof", package])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|error| {
            io::Error::new(
                error.kind(),
                format!("Cannot execute ADB package PID lookup: {error}"),
            )
        })?;
    let result = timeout_at(deadline, async {
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| io::Error::other("Missing pidof stdout"))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| io::Error::other("Missing pidof stderr"))?;
        let (output, diagnostic) = tokio::try_join!(
            read_limited(
                stdout,
                4096,
                "ADB package PID stdout exceeded the 4 KiB limit"
            ),
            read_limited(
                stderr,
                16 * 1024,
                "ADB package PID stderr exceeded the 16 KiB limit"
            ),
        )?;
        let status = child.wait().await?;
        if !status.success()
            && !(status.code() == Some(1) && output.is_empty() && diagnostic.is_empty())
        {
            return Err(io::Error::other(format!(
                "ADB package PID lookup failed ({status}): {}",
                String::from_utf8_lossy(&diagnostic).trim()
            )));
        }
        parse_pid(&output, package)
    })
    .await
    .unwrap_or_else(|_| {
        Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "ADB logcat timed out after 5 seconds during package PID lookup",
        ))
    });
    if result.is_err() {
        let _ = timeout(Duration::from_secs(1), child.kill()).await;
    }
    result
}

async fn read_limited(
    reader: impl AsyncRead + Unpin,
    limit: usize,
    message: &'static str,
) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader
        .take((limit + 1) as u64)
        .read_to_end(&mut bytes)
        .await?;
    if bytes.len() > limit {
        return Err(io::Error::other(message));
    }
    Ok(bytes)
}

fn parse_pid(output: &[u8], package: &str) -> io::Result<u32> {
    let text = std::str::from_utf8(output).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "ADB package PID output is not valid UTF-8",
        )
    })?;
    let mut pids = text.split_whitespace();
    let Some(value) = pids.next() else {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("No running main process found for package {package}"),
        ));
    };
    if pids.next().is_some() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Multiple main-process PIDs found for package; use an explicit pid instead",
        ));
    }
    if !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "ADB package PID output is not a valid process ID",
        ));
    }
    value
        .parse::<u32>()
        .ok()
        .filter(|pid| *pid > 0 && *pid <= i32::MAX as u32)
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "ADB package PID must be between 1 and 2147483647",
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_application_ids_without_allowing_shell_syntax() {
        for package in [
            "com.github.bschramke.myfinance",
            "com.example_app.test1",
            "a.b",
        ] {
            assert!(validate(package).is_ok());
        }
        for package in [
            "",
            "android",
            ".com.example",
            "com..example",
            "com.example.",
            "com.1example",
            "com._example",
            "com.example:worker",
            "com.example;id",
            "com.example$(id)",
            "com.example app",
            "com.example\napp",
            "com.example\0app",
            "com.éxample",
            "--help",
            "com.example/app",
            "com.example*",
            "com.example'",
            "com.example\"",
        ] {
            assert!(validate(package).is_err(), "Accepted {package:?}");
        }
        assert!(validate(&format!("com.{}", "a".repeat(251))).is_ok());
        assert!(validate(&format!("com.{}", "a".repeat(252))).is_err());
    }

    #[test]
    fn parses_only_one_valid_positive_pid() {
        assert_eq!(parse_pid(b" 1234\r\n", "com.example.app").unwrap(), 1234);
        assert_eq!(
            parse_pid(b"2147483647", "com.example.app").unwrap(),
            i32::MAX as u32
        );
        for output in [
            &b""[..],
            b" \n",
            b"0",
            b"-1",
            b"+1",
            b"2147483648",
            b"9999999999999999999",
            b"1000 2000",
            b"1000\n2000",
            b"not-a-pid",
            &[0xff, 0xfe],
        ] {
            assert!(
                parse_pid(output, "com.example.app").is_err(),
                "Accepted {output:?}"
            );
        }
    }
}
