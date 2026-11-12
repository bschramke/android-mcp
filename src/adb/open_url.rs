use std::{io, path::Path, time::Duration};

use tokio::time::Instant;
use url::Url;

use super::{
    command::{CommandOptions, ensure_success, run_shell},
    device_info::validate_serial,
};

const OPEN_URL_TIMEOUT: Duration = Duration::from_secs(15);
const OPEN_URL_OUTPUT_LIMIT: usize = 16 * 1024;

pub async fn open_url(executable: &Path, serial: &str, input: &str) -> io::Result<()> {
    validate_serial(serial)?;
    let url = parse_web_url(input)?;
    let command = format!(
        "am start -a android.intent.action.VIEW -c android.intent.category.BROWSABLE -d {}",
        shell_quote(url.as_str())
    );
    let output = run_shell(
        executable,
        serial,
        [command],
        "open URL",
        CommandOptions::new(
            Instant::now() + OPEN_URL_TIMEOUT,
            OPEN_URL_OUTPUT_LIMIT,
            OPEN_URL_OUTPUT_LIMIT,
        )
        .with_timeout_message("ADB open URL timed out after 15 seconds"),
    )
    .await?;
    ensure_success(&output, "open URL")
}

fn parse_web_url(input: &str) -> io::Result<Url> {
    let url = Url::parse(input).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("URL must be an absolute HTTP or HTTPS URL: {error}"),
        )
    })?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "URL must use HTTP or HTTPS, include a host, and not contain credentials",
        ));
    }
    Ok(url)
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_http_and_https_urls() {
        for (input, expected) in [
            ("http://example.com", "http://example.com/"),
            (
                "https://example.com/path?q=one&two=2",
                "https://example.com/path?q=one&two=2",
            ),
        ] {
            assert_eq!(parse_web_url(input).unwrap().as_str(), expected);
        }
    }

    #[test]
    fn rejects_non_web_urls_and_credentials() {
        for value in [
            "javascript:alert(1)",
            "file:///etc/passwd",
            "intent://example.com/#Intent;scheme=https;end",
            "relative/path",
            "https://user:password@example.com",
        ] {
            assert!(parse_web_url(value).is_err(), "Accepted {value:?}");
        }
    }

    #[test]
    fn shell_quotes_url_as_one_argument() {
        assert_eq!(
            shell_quote("https://example.com/?q=a'b"),
            "'https://example.com/?q=a'\\''b'"
        );
    }
}
