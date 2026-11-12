use std::io;

use super::super::model::LogPriority;

pub(super) fn filter_arguments(
    pid: Option<u32>,
    tags: &[String],
    priority: Option<LogPriority>,
) -> io::Result<Vec<String>> {
    if pid.is_some_and(|pid| pid == 0 || pid > i32::MAX as u32) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "pid must be between 1 and 2147483647",
        ));
    }
    if tags.len() > 20
        || tags.iter().any(|tag| {
            tag.is_empty()
                || tag.len() > 128
                || tag.starts_with('-')
                || !tag
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"._/+-@".contains(&byte))
        })
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "tags must contain at most 20 exact names, each 1-128 ASCII letters/digits or ._/+-@, without a leading '-'",
        ));
    }
    let mut args = Vec::new();
    if let Some(pid) = pid {
        args.push(format!("--pid={pid}"));
    }
    if !tags.is_empty() {
        let level = priority.map(LogPriority::code).unwrap_or('V');
        args.extend(tags.iter().map(|tag| format!("{tag}:{level}")));
        args.push("*:S".to_owned());
    } else if let Some(priority) = priority {
        args.push(format!("*:{}", priority.code()));
    }
    Ok(args)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_filters_preserve_the_original_command() {
        assert!(filter_arguments(None, &[], None).unwrap().is_empty());
    }

    #[test]
    fn combines_pid_tag_alternatives_and_minimum_priority() {
        assert_eq!(
            filter_arguments(
                Some(2210),
                &["ErrorProcessor".into(), "AudioController".into()],
                Some(LogPriority::Warn)
            )
            .unwrap(),
            ["--pid=2210", "ErrorProcessor:W", "AudioController:W", "*:S"]
        );
        assert_eq!(
            filter_arguments(None, &["ExampleApp".into()], None).unwrap(),
            ["ExampleApp:V", "*:S"]
        );
        assert_eq!(filter_arguments(Some(1), &[], None).unwrap(), ["--pid=1"]);
        assert_eq!(
            filter_arguments(Some(i32::MAX as u32), &[], None).unwrap(),
            ["--pid=2147483647"]
        );
    }

    #[test]
    fn maps_all_minimum_priorities_to_logcat_levels() {
        for (priority, expected) in [
            (LogPriority::Verbose, "*:V"),
            (LogPriority::Debug, "*:D"),
            (LogPriority::Info, "*:I"),
            (LogPriority::Warn, "*:W"),
            (LogPriority::Error, "*:E"),
            (LogPriority::Fatal, "*:F"),
        ] {
            assert_eq!(
                filter_arguments(None, &[], Some(priority)).unwrap(),
                [expected]
            );
        }
    }

    #[test]
    fn rejects_invalid_pids_and_unsafe_filter_expressions() {
        for pid in [0, i32::MAX as u32 + 1, u32::MAX] {
            assert!(filter_arguments(Some(pid), &[], None).is_err());
        }
        for tag in [
            "",
            "*",
            "Example:W",
            "--pid=1",
            "a b",
            "a\nb",
            "a\0b",
            "a;b",
            "$(id)",
            "a|b",
            "a&b",
            "a`id`",
            "a'b",
            "a\"b",
            "nonascii-é",
        ] {
            assert!(
                filter_arguments(None, &[tag.to_owned()], None).is_err(),
                "Accepted {tag:?}"
            );
        }
        assert!(filter_arguments(None, &["a".repeat(129)], None).is_err());
        assert!(filter_arguments(None, &vec!["Tag".into(); 21], None).is_err());
        assert!(filter_arguments(None, &vec!["Tag".into(); 20], None).is_ok());
        assert!(
            filter_arguments(
                None,
                &[
                    "a".repeat(128),
                    "android.os.Debug".into(),
                    "path/Tag+Native@1".into()
                ],
                None
            )
            .is_ok()
        );
    }
}
