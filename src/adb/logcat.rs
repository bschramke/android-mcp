use std::{io, path::Path, process::Stdio, time::Duration};

use tokio::{
    io::{AsyncRead, AsyncReadExt},
    process::{Child, Command},
    time::timeout,
};

use super::model::LogcatSnapshot;

const OUTPUT_LIMIT: usize = 256 * 1024;
const STDERR_LIMIT: usize = 16 * 1024;

pub async fn logcat(executable: &Path, serial: &str, max_lines: u32) -> io::Result<LogcatSnapshot> {
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
    let lines = max_lines.to_string();
    let mut child = Command::new(executable)
        .args([
            "-s",
            serial,
            "logcat",
            "-d",
            "-t",
            &lines,
            "-v",
            "threadtime",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|error| {
            io::Error::new(error.kind(), format!("Cannot execute ADB logcat: {error}"))
        })?;

    let result = match timeout(Duration::from_secs(5), capture(&mut child, serial)).await {
        Ok(result) => result,
        Err(_) => Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "ADB logcat timed out after 5 seconds",
        )),
    };
    if result.is_err() {
        let _ = timeout(Duration::from_secs(1), child.kill()).await;
    }
    result
}

async fn read_capped(reader: impl AsyncRead + Unpin, limit: usize) -> io::Result<(Vec<u8>, bool)> {
    let mut bytes = Vec::new();
    reader
        .take((limit + 1) as u64)
        .read_to_end(&mut bytes)
        .await?;
    let truncated = bytes.len() > limit;
    bytes.truncate(limit);
    Ok((bytes, truncated))
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

async fn capture(child: &mut Child, serial: &str) -> io::Result<LogcatSnapshot> {
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| io::Error::other("Missing logcat stdout"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| io::Error::other("Missing logcat stderr"))?;
    let stdout_read = read_capped(stdout, OUTPUT_LIMIT);
    let stderr_read = read_capped(stderr, STDERR_LIMIT);
    tokio::pin!(stdout_read, stderr_read);
    let mut output = None;
    let mut diagnostic = None;

    while output.is_none() || diagnostic.is_none() {
        tokio::select! {
            result = &mut stdout_read, if output.is_none() => {
                let (bytes, truncated) = result?;
                if truncated {
                    if child.try_wait()?.is_none() { child.kill().await?; }
                    return snapshot(serial, bytes, true);
                }
                output = Some(bytes);
            }
            result = &mut stderr_read, if diagnostic.is_none() => {
                let (bytes, truncated) = result?;
                if truncated {
                    return Err(io::Error::other("ADB logcat stderr exceeded the 16 KiB limit"));
                }
                diagnostic = Some(bytes);
            }
        }
    }

    let status = child.wait().await?;
    if !status.success() {
        return Err(io::Error::other(format!(
            "ADB logcat failed ({status}): {}",
            String::from_utf8_lossy(&diagnostic.unwrap_or_default()).trim()
        )));
    }
    snapshot(serial, output.unwrap_or_default(), false)
}

#[cfg(test)]
mod tests {
    use super::*;

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
