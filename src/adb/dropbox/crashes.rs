use std::{
    ffi::OsString,
    fs, io,
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use base64::{Engine, engine::general_purpose::STANDARD, engine::general_purpose::URL_SAFE_NO_PAD};
use protobuf::Message;
use serde::Serialize;
use tokio::time::Instant;

use super::super::{
    command::{CommandOptions, ensure_success, run_adb, run_shell},
    device_info::validate_serial,
};
use super::proto::DropBoxManagerServiceDumpProto;

const LIST_OUTPUT_LIMIT: usize = 1024 * 1024;
const LIST_STDERR_LIMIT: usize = 16 * 1024;
const PROTO_OUTPUT_LIMIT: usize = 512 * 1024;
const PULL_OUTPUT_LIMIT: usize = 16 * 1024;
const CRASH_TIMEOUT: Duration = Duration::from_secs(10);
const FILE_PULL_TIMEOUT: Duration = Duration::from_secs(60);
const PROTO_ENTRY_DATA_LIMIT: usize = 256 * 1024;

#[derive(Debug, Serialize)]
pub struct CrashList {
    pub serial: String,
    pub reports: Vec<CrashReport>,
    pub truncated: bool,
}

#[derive(Debug, Serialize)]
pub struct CrashReport {
    pub id: String,
    pub timestamp: String,
    pub tag: String,
    pub content_type: Option<String>,
    pub compressed: bool,
    pub size_bytes: Option<u64>,
    pub has_file: bool,
}

#[derive(Debug, Serialize)]
pub struct CrashContent {
    pub id: String,
    pub timestamp: String,
    pub tag: String,
    pub timestamp_ms: Option<i64>,
    pub encoding: String,
    pub content: String,
    pub possibly_truncated: bool,
}

#[derive(Debug, Serialize)]
pub struct CrashFileResult {
    pub path: PathBuf,
    pub size_bytes: u64,
}

#[derive(Debug)]
struct CrashIdentity {
    date: String,
    time: String,
    tag: String,
}

#[derive(Debug)]
struct ParsedReport {
    report: CrashReport,
    device_path: Option<String>,
}

pub async fn list_crashes(
    executable: &Path,
    serial: &str,
    date: Option<&str>,
    time: Option<&str>,
    tag: Option<&str>,
    limit: u32,
) -> io::Result<CrashList> {
    validate_serial(serial)?;
    validate_filters(date, time, tag)?;
    if !(1..=1000).contains(&limit) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Crash limit must be between 1 and 1000",
        ));
    }

    let output = run_dropbox_dump(
        executable,
        serial,
        DumpMode::Metadata,
        date,
        time,
        tag,
        Instant::now() + CRASH_TIMEOUT,
    )
    .await?;
    ensure_success(&output, "crash report listing")?;
    let stdout = std::str::from_utf8(&output.stdout).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("ADB DropBox listing is not valid UTF-8: {error}"),
        )
    })?;
    let mut reports: Vec<_> = parse_dropbox_dump(stdout)
        .into_iter()
        .map(|entry| entry.report)
        .filter(|report| tag.map_or_else(|| is_crash_tag(&report.tag), |tag| report.tag == tag))
        .collect();
    let truncated = reports.len() > limit as usize;
    reports.truncate(limit as usize);
    Ok(CrashList {
        serial: serial.to_owned(),
        reports,
        truncated,
    })
}

pub async fn get_crash(executable: &Path, serial: &str, id: &str) -> io::Result<CrashContent> {
    validate_serial(serial)?;
    let identity = decode_id(id)?;
    let output = run_dropbox_dump(
        executable,
        serial,
        DumpMode::Proto,
        Some(&identity.date),
        Some(&identity.time),
        Some(&identity.tag),
        Instant::now() + CRASH_TIMEOUT,
    )
    .await?;
    ensure_success(&output, "crash report protobuf")?;
    let proto =
        DropBoxManagerServiceDumpProto::parse_from_bytes(&output.stdout).map_err(|error| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("ADB DropBox protobuf is malformed: {error}"),
            )
        })?;
    let mut entries = proto.entries.into_iter();
    let entry = entries.next().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            format!("No crash report found for id {id}"),
        )
    })?;
    if entries.next().is_some() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Crash ID matched multiple DropBox entries",
        ));
    }
    let data = entry.data.ok_or_else(|| {
        io::Error::new(io::ErrorKind::NotFound, "Crash report has no readable data")
    })?;
    let (encoding, content) = match String::from_utf8(data.clone()) {
        Ok(text) => ("utf-8", text),
        Err(_) => ("base64", STANDARD.encode(&data)),
    };
    Ok(CrashContent {
        id: id.to_owned(),
        timestamp: identity.timestamp(),
        tag: identity.tag,
        timestamp_ms: entry.time_ms,
        encoding: encoding.to_owned(),
        possibly_truncated: data.len() >= PROTO_ENTRY_DATA_LIMIT,
        content,
    })
}

pub async fn get_crash_file(
    executable: &Path,
    serial: &str,
    id: &str,
    output_path: &Path,
    overwrite: bool,
) -> io::Result<CrashFileResult> {
    validate_serial(serial)?;
    let identity = decode_id(id)?;
    validate_destination(output_path, overwrite)?;

    let output = run_dropbox_dump(
        executable,
        serial,
        DumpMode::File,
        Some(&identity.date),
        Some(&identity.time),
        Some(&identity.tag),
        Instant::now() + CRASH_TIMEOUT,
    )
    .await?;
    ensure_success(&output, "crash report file lookup")?;
    let stdout = std::str::from_utf8(&output.stdout).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("ADB DropBox file listing is not valid UTF-8: {error}"),
        )
    })?;
    let remote_path = parse_dropbox_dump(stdout)
        .into_iter()
        .find(|entry| entry.report.id == id)
        .and_then(|entry| entry.device_path)
        .filter(|path| path.starts_with("/data/system/dropbox/"))
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!("No readable DropBox file found for crash id {id}"),
            )
        })?;

    let staging = staging_path(output_path)?;
    let pull = run_adb(
        executable,
        [
            OsString::from("-s"),
            OsString::from(serial),
            OsString::from("pull"),
            OsString::from(remote_path),
            staging.as_os_str().to_os_string(),
        ],
        "crash report file download",
        CommandOptions::new(
            Instant::now() + FILE_PULL_TIMEOUT,
            PULL_OUTPUT_LIMIT,
            PULL_OUTPUT_LIMIT,
        )
        .with_timeout_message("ADB crash report file download timed out after 60 seconds"),
    )
    .await;
    let output = match pull {
        Ok(output) => output,
        Err(error) => {
            let _ = fs::remove_file(&staging);
            return Err(error);
        }
    };
    if let Err(error) = ensure_success(&output, "crash report file download") {
        let _ = fs::remove_file(&staging);
        return Err(error);
    }
    let size_bytes = match fs::metadata(&staging) {
        Ok(metadata) => metadata.len(),
        Err(error) => {
            let _ = fs::remove_file(&staging);
            return Err(error);
        }
    };
    let finalize = if overwrite {
        fs::rename(&staging, output_path)
    } else {
        fs::hard_link(&staging, output_path).and_then(|()| fs::remove_file(&staging))
    };
    if let Err(error) = finalize {
        let _ = fs::remove_file(&staging);
        return Err(error);
    }
    Ok(CrashFileResult {
        path: output_path.to_owned(),
        size_bytes,
    })
}

#[derive(Clone, Copy)]
enum DumpMode {
    Metadata,
    File,
    Proto,
}

async fn run_dropbox_dump(
    executable: &Path,
    serial: &str,
    mode: DumpMode,
    date: Option<&str>,
    time: Option<&str>,
    tag: Option<&str>,
    deadline: Instant,
) -> io::Result<super::super::command::CommandOutput> {
    let arguments = dropbox_arguments(mode, date, time, tag);
    let is_proto = matches!(mode, DumpMode::Proto);
    let options = CommandOptions::new(
        deadline,
        if is_proto {
            PROTO_OUTPUT_LIMIT
        } else {
            LIST_OUTPUT_LIMIT
        },
        LIST_STDERR_LIMIT,
    )
    .with_timeout_message("ADB DropBox query timed out after 10 seconds");
    if is_proto {
        let mut adb_arguments = vec![
            OsString::from("-s"),
            OsString::from(serial),
            OsString::from("exec-out"),
        ];
        adb_arguments.extend(arguments);
        run_adb(executable, adb_arguments, "DropBox protobuf query", options).await
    } else {
        run_shell(executable, serial, arguments, "DropBox listing", options).await
    }
}

fn dropbox_arguments(
    mode: DumpMode,
    date: Option<&str>,
    time: Option<&str>,
    tag: Option<&str>,
) -> Vec<OsString> {
    let mut args = match mode {
        DumpMode::Proto => vec![
            OsString::from("dumpsys"),
            OsString::from("dropbox"),
            OsString::from("--proto"),
        ],
        DumpMode::Metadata | DumpMode::File => {
            let mut args = vec![OsString::from("dumpsys"), OsString::from("dropbox")];
            if matches!(mode, DumpMode::File) {
                args.push(OsString::from("--file"));
            }
            args
        }
    };
    for filter in [date, time, tag].into_iter().flatten() {
        args.push(OsString::from(filter));
    }
    args
}

fn validate_filters(date: Option<&str>, time: Option<&str>, tag: Option<&str>) -> io::Result<()> {
    if date.is_some_and(|date| {
        date.len() != 10
            || !date.bytes().enumerate().all(|(index, byte)| {
                if matches!(index, 4 | 7) {
                    byte == b'-'
                } else {
                    byte.is_ascii_digit()
                }
            })
    }) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "date must use YYYY-MM-DD format",
        ));
    }
    if time.is_some_and(|time| !valid_time(time)) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "time must use HH:MM:SS or HH:MM:SS.mmm format",
        ));
    }
    if tag.is_some_and(|tag| {
        tag.is_empty()
            || tag.len() > 128
            || !tag
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
    }) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "tag must contain 1-128 ASCII letters, digits, dots, underscores, or hyphens",
        ));
    }
    Ok(())
}

fn valid_time(time: &str) -> bool {
    let bytes = time.as_bytes();
    let valid_clock = bytes.len() >= 8
        && bytes[2] == b':'
        && bytes[5] == b':'
        && bytes[..2].iter().all(u8::is_ascii_digit)
        && bytes[3..5].iter().all(u8::is_ascii_digit)
        && bytes[6..8].iter().all(u8::is_ascii_digit);
    match bytes.len() {
        8 => valid_clock,
        12 => valid_clock && bytes[8] == b'.' && bytes[9..].iter().all(u8::is_ascii_digit),
        _ => false,
    }
}

fn parse_dropbox_dump(output: &str) -> Vec<ParsedReport> {
    let mut entries: Vec<ParsedReport> = Vec::new();
    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("/data/system/dropbox/") {
            if let Some(entry) = entries.last_mut() {
                entry.device_path = Some(trimmed.to_owned());
            }
            continue;
        }
        let Some((date, time, tag, details)) = parse_entry_line(trimmed) else {
            continue;
        };
        let timestamp = format!("{date} {time}");
        let details = details.trim_start_matches('(').trim_end_matches(')');
        let compressed = details.starts_with("compressed ");
        let details = details.strip_prefix("compressed ").unwrap_or(details);
        let content_type = details
            .split_once(',')
            .map(|(kind, _)| kind.trim().to_owned())
            .filter(|kind| matches!(kind.as_str(), "text" | "data"));
        let size_bytes = details
            .rsplit_once(", ")
            .and_then(|(_, size)| size.strip_suffix(" bytes"))
            .and_then(|size| size.parse().ok());
        let has_file = !details.starts_with("no file") && !details.starts_with("contents lost");
        entries.push(ParsedReport {
            report: CrashReport {
                id: make_id(date, time, tag),
                timestamp: timestamp.clone(),
                tag: tag.to_owned(),
                content_type,
                compressed,
                size_bytes,
                has_file,
            },
            device_path: None,
        });
    }
    entries
}

fn parse_entry_line(line: &str) -> Option<(&str, &str, &str, &str)> {
    let (date, remainder) = line.split_once(' ')?;
    if validate_filters(Some(date), None, None).is_err() {
        return None;
    }
    let remainder = remainder.trim_start();
    let (time, remainder) = remainder.split_once(' ')?;
    if !valid_time(time) {
        return None;
    }
    let remainder = remainder.trim_start();
    let separator = remainder.find(char::is_whitespace)?;
    let (tag, details) = remainder.split_at(separator);
    if validate_filters(None, None, Some(tag)).is_err() {
        return None;
    }
    Some((date, time, tag, details.trim()))
}

fn is_crash_tag(tag: &str) -> bool {
    let tag = tag.to_ascii_lowercase();
    tag.contains("crash") || tag.contains("anr") || tag.contains("tombstone")
}

fn make_id(date: &str, time: &str, tag: &str) -> String {
    URL_SAFE_NO_PAD.encode(format!("{date}\n{time}\n{tag}"))
}

fn decode_id(id: &str) -> io::Result<CrashIdentity> {
    let decoded = URL_SAFE_NO_PAD.decode(id).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("Invalid crash id: {error}"),
        )
    })?;
    let decoded = std::str::from_utf8(&decoded).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("Invalid crash id: {error}"),
        )
    })?;
    let mut parts = decoded.splitn(3, '\n');
    let date = parts.next().unwrap_or_default();
    let time = parts.next().unwrap_or_default();
    let tag = parts.next().unwrap_or_default();
    validate_filters(Some(date), Some(time), Some(tag))?;
    Ok(CrashIdentity {
        date: date.to_owned(),
        time: time.to_owned(),
        tag: tag.to_owned(),
    })
}

fn validate_destination(path: &Path, overwrite: bool) -> io::Result<()> {
    if !path.is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Crash output path must be absolute",
        ));
    }
    if !path.parent().is_some_and(Path::is_dir) {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "Crash output directory does not exist",
        ));
    }
    if !overwrite && path.exists() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("Crash output file already exists: {}", path.display()),
        ));
    }
    Ok(())
}

fn staging_path(destination: &Path) -> io::Result<PathBuf> {
    let parent = destination.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "Crash output path must have a parent directory",
        )
    })?;
    let name = destination
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "Invalid crash output path"))?;
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(io::Error::other)?
        .as_nanos();
    let mut staging_name = OsString::from(".");
    staging_name.push(name);
    staging_name.push(format!(".android-mcp-{}-{nonce}.tmp", std::process::id()));
    Ok(parent.join(staging_name))
}

impl CrashIdentity {
    fn timestamp(&self) -> String {
        format!("{} {}", self.date, self.time)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_crash_metadata_and_device_file_path() {
        let output = "Drop box contents: 2 entries\n2026-01-02 03:04:05.006 data_app_crash (compressed text, 42 bytes)\n /data/system/dropbox/data_app_crash@1.txt.gz\n2026-01-02 03:05:06.007 system_server_anr (data, 64 bytes)\n";
        let entries = parse_dropbox_dump(output);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].report.tag, "data_app_crash");
        assert_eq!(entries[0].report.timestamp, "2026-01-02 03:04:05.006");
        assert_eq!(entries[0].report.content_type.as_deref(), Some("text"));
        assert!(entries[0].report.compressed);
        assert_eq!(entries[0].report.size_bytes, Some(42));
        assert_eq!(
            entries[0].device_path.as_deref(),
            Some("/data/system/dropbox/data_app_crash@1.txt.gz")
        );
        assert_eq!(entries[1].report.content_type.as_deref(), Some("data"));
    }

    #[test]
    fn crash_ids_round_trip_and_reject_invalid_filters() {
        let id = make_id("2026-01-02", "03:04:05.006", "data_app_crash");
        let decoded = decode_id(&id).unwrap();
        assert_eq!(decoded.timestamp(), "2026-01-02 03:04:05.006");
        assert_eq!(decoded.tag, "data_app_crash");
        assert!(decode_id("../../bad").is_err());
        assert!(validate_filters(None, Some("03:04:05"), None).is_ok());
        assert!(validate_filters(None, Some("03:04:05;id"), None).is_err());
        assert!(validate_filters(None, None, Some("data_app_crash;id")).is_err());
        assert!(validate_filters(None, Some("03x04x05"), None).is_err());
    }

    #[test]
    fn builds_exact_dropbox_command_filters_without_shell_tokens() {
        assert_eq!(
            dropbox_arguments(
                DumpMode::Metadata,
                Some("2026-01-02"),
                Some("03:04:05.006"),
                Some("data_app_crash")
            ),
            [
                "dumpsys",
                "dropbox",
                "2026-01-02",
                "03:04:05.006",
                "data_app_crash"
            ]
        );
        assert_eq!(
            dropbox_arguments(
                DumpMode::Proto,
                Some("2026-01-02"),
                Some("03:04:05.006"),
                Some("data_app_crash")
            ),
            [
                "dumpsys",
                "dropbox",
                "--proto",
                "2026-01-02",
                "03:04:05.006",
                "data_app_crash"
            ]
        );
    }

    #[test]
    fn identifies_crash_anr_and_tombstone_tags() {
        for tag in ["data_app_crash", "system_server_anr", "SYSTEM_TOMBSTONE"] {
            assert!(is_crash_tag(tag));
        }
        assert!(!is_crash_tag("system_app_wtf"));
    }
}
