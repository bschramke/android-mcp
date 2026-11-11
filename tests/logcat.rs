mod support;

use rmcp::{model::CallToolRequestParams, service::ServiceError};
use support::adb::{adb_fixture, connect};
use support::{Client, TIMEOUT, TestResult, connect_command, disconnect, server_command};
use tokio::{process::Child, time::timeout};

fn logcat_request(arguments: serde_json::Value) -> CallToolRequestParams {
    let mut request = CallToolRequestParams::new("adb_logcat");
    request.arguments = arguments.as_object().cloned();
    request
}

#[tokio::test]
async fn logcat_defaults_to_200_lines_and_returns_only_stdout() -> TestResult {
    let (client, child) = connect("logcat").await?;
    let result = timeout(
        TIMEOUT,
        client.call_tool(logcat_request(
            serde_json::json!({"serial": "emulator-5554"}),
        )),
    )
    .await??;
    assert_ne!(result.is_error, Some(true));
    assert_eq!(result.content.len(), 1);
    let snapshot: serde_json::Value = serde_json::from_str(
        &result.content[0]
            .as_text()
            .ok_or("Expected snapshot JSON")?
            .text,
    )?;
    assert_eq!(
        snapshot,
        serde_json::json!({
            "serial": "emulator-5554", "format": "threadtime",
            "output": include_str!("fixtures/logcat/threadtime.txt"), "truncated": false
        })
    );
    disconnect(client, child).await
}

#[tokio::test]
async fn logcat_passes_explicit_limit_and_literal_serial() -> TestResult {
    let (client, child) = connect("logcat_explicit").await?;
    let result = timeout(
        TIMEOUT,
        client.call_tool(logcat_request(
            serde_json::json!({"serial": "serial;not-a-shell-command", "max_lines": 17}),
        )),
    )
    .await??;
    assert_ne!(result.is_error, Some(true));
    let snapshot: serde_json::Value =
        serde_json::from_str(&result.content[0].as_text().ok_or("Expected JSON")?.text)?;
    assert_eq!(snapshot["serial"], "serial;not-a-shell-command");
    disconnect(client, child).await
}

#[tokio::test]
async fn logcat_accepts_minimum_and_maximum_line_limits() -> TestResult {
    let (client, child) = connect("logcat_bounds").await?;
    for max_lines in [1, 2000] {
        let result = timeout(
            TIMEOUT,
            client.call_tool(logcat_request(
                serde_json::json!({"serial": "emulator-5554", "max_lines": max_lines}),
            )),
        )
        .await??;
        assert_ne!(result.is_error, Some(true));
    }
    disconnect(client, child).await
}

#[tokio::test]
async fn logcat_rejects_invalid_serials_and_line_limits() -> TestResult {
    let (client, child) = connect("logcat").await?;
    for arguments in [
        serde_json::json!({}),
        serde_json::json!({"serial": 123}),
        serde_json::json!({"serial": ""}),
        serde_json::json!({"serial": "device serial"}),
        serde_json::json!({"serial": "device\u{0000}serial"}),
        serde_json::json!({"serial": "emulator-5554", "max_lines": 0}),
        serde_json::json!({"serial": "emulator-5554", "max_lines": 2001}),
        serde_json::json!({"serial": "emulator-5554", "max_lines": -1}),
        serde_json::json!({"serial": "emulator-5554", "max_lines": 1.5}),
        serde_json::json!({"serial": "emulator-5554", "max_lines": "200"}),
        serde_json::json!({"serial": "emulator-5554", "max_lines": null}),
    ] {
        let result = timeout(TIMEOUT, client.call_tool(logcat_request(arguments))).await?;
        match result {
            Ok(result) => assert_eq!(result.is_error, Some(true)),
            Err(ServiceError::McpError(_)) => {}
            Err(error) => return Err(error.into()),
        }
    }
    disconnect(client, child).await
}

#[tokio::test]
async fn empty_logcat_is_a_successful_empty_snapshot() -> TestResult {
    let (client, child) = connect("logcat_empty").await?;
    let result = timeout(
        TIMEOUT,
        client.call_tool(logcat_request(
            serde_json::json!({"serial": "emulator-5554"}),
        )),
    )
    .await??;
    assert_ne!(result.is_error, Some(true));
    let snapshot: serde_json::Value =
        serde_json::from_str(&result.content[0].as_text().ok_or("Expected JSON")?.text)?;
    assert_eq!(snapshot["output"], "");
    assert_eq!(snapshot["truncated"], false);
    disconnect(client, child).await
}

async fn connect_logcat_file(mode: &str) -> TestResult<(Client, Child, tempfile::TempDir)> {
    let directory = tempfile::tempdir()?;
    let mut command = server_command();
    command
        .env("ANDROID_MCP_ADB", adb_fixture()?)
        .env("ANDROID_MCP_TEST_ADB_MODE", mode);
    let (client, child) = connect_command(command).await?;
    Ok((client, child, directory))
}

#[tokio::test]
async fn logcat_saves_to_file_and_returns_only_the_path() -> TestResult {
    let (client, child, directory) = connect_logcat_file("logcat").await?;
    let path = directory.path().join("snapshot.log");
    let result = timeout(
        TIMEOUT,
        client.call_tool(logcat_request(serde_json::json!({
            "serial": "emulator-5554",
            "output_path": path,
        }))),
    )
    .await??;
    assert_ne!(result.is_error, Some(true));
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(
            &result.content[0]
                .as_text()
                .ok_or("Expected path JSON")?
                .text
        )?,
        serde_json::json!({"path": path})
    );
    assert_eq!(
        std::fs::read_to_string(&path)?,
        include_str!("fixtures/logcat/threadtime.txt")
    );
    disconnect(client, child).await
}

#[tokio::test]
async fn file_output_uses_its_own_larger_bounded_limit() -> TestResult {
    let (client, child, directory) = connect_logcat_file("logcat_file_large").await?;
    let path = directory.path().join("large.log");
    let result = timeout(
        TIMEOUT,
        client.call_tool(logcat_request(serde_json::json!({
            "serial": "emulator-5554",
            "output_path": path,
        }))),
    )
    .await??;
    assert_ne!(result.is_error, Some(true));
    let response: serde_json::Value = serde_json::from_str(
        &result.content[0]
            .as_text()
            .ok_or("Expected path JSON")?
            .text,
    )?;
    assert_eq!(response, serde_json::json!({"path": path}));
    assert_eq!(std::fs::metadata(path)?.len(), 300 * 1024);
    disconnect(client, child).await
}

#[tokio::test]
async fn file_output_rejects_overwrite_and_allows_explicit_replacement() -> TestResult {
    let (client, child, directory) = connect_logcat_file("logcat").await?;
    let path = directory.path().join("snapshot.log");
    std::fs::write(&path, "keep existing")?;
    let result = timeout(
        TIMEOUT,
        client.call_tool(logcat_request(serde_json::json!({
            "serial": "emulator-5554",
            "output_path": path,
        }))),
    )
    .await??;
    assert_eq!(result.is_error, Some(true));
    assert_eq!(std::fs::read_to_string(&path)?, "keep existing");

    let result = timeout(
        TIMEOUT,
        client.call_tool(logcat_request(serde_json::json!({
            "serial": "emulator-5554",
            "output_path": path,
            "overwrite": true,
        }))),
    )
    .await??;
    assert_ne!(result.is_error, Some(true));
    assert_eq!(
        std::fs::read_to_string(&path)?,
        include_str!("fixtures/logcat/threadtime.txt")
    );
    disconnect(client, child).await
}

#[tokio::test]
async fn oversized_file_output_fails_without_leaving_a_partial_file() -> TestResult {
    let (client, child, directory) = connect_logcat_file("logcat_file_oversized").await?;
    let path = directory.path().join("oversized.log");
    let result = timeout(
        TIMEOUT,
        client.call_tool(logcat_request(serde_json::json!({
            "serial": "emulator-5554",
            "output_path": path,
        }))),
    )
    .await??;
    assert_eq!(result.is_error, Some(true));
    assert!(!path.exists());
    disconnect(client, child).await
}

#[tokio::test]
async fn oversized_logcat_is_capped_and_server_remains_usable() -> TestResult {
    let (client, child) = connect("logcat_large").await?;
    let result = timeout(
        TIMEOUT,
        client.call_tool(logcat_request(
            serde_json::json!({"serial": "emulator-5554"}),
        )),
    )
    .await??;
    assert_ne!(result.is_error, Some(true));
    let snapshot: serde_json::Value =
        serde_json::from_str(&result.content[0].as_text().ok_or("Expected JSON")?.text)?;
    assert_eq!(snapshot["truncated"], true);
    assert_eq!(
        snapshot["output"].as_str().ok_or("Missing output")?.len(),
        256 * 1024
    );
    assert!(
        timeout(TIMEOUT, client.list_all_tools())
            .await??
            .iter()
            .any(|tool| tool.name == "adb_logcat")
    );
    disconnect(client, child).await
}

#[tokio::test]
async fn oversized_logcat_from_an_exited_process_is_still_truncated_successfully() -> TestResult {
    let (client, child) = connect("logcat_large_exit").await?;
    let result = timeout(
        TIMEOUT,
        client.call_tool(logcat_request(
            serde_json::json!({"serial": "emulator-5554"}),
        )),
    )
    .await??;
    assert_ne!(result.is_error, Some(true));
    let snapshot: serde_json::Value = serde_json::from_str(
        &result.content[0]
            .as_text()
            .ok_or("Expected snapshot JSON")?
            .text,
    )?;
    assert_eq!(snapshot["truncated"], true);
    assert_eq!(snapshot["output"].as_str().unwrap().len(), 256 * 1024);
    disconnect(client, child).await
}

async fn assert_logcat_error(mode: &str, message: &str) -> TestResult {
    let (client, child) = connect(mode).await?;
    let result = timeout(
        TIMEOUT,
        client.call_tool(logcat_request(
            serde_json::json!({"serial": "emulator-5554"}),
        )),
    )
    .await??;
    assert_eq!(result.is_error, Some(true));
    assert!(
        result.content[0]
            .as_text()
            .ok_or("Expected error text")?
            .text
            .contains(message)
    );
    assert!(
        timeout(TIMEOUT, client.list_all_tools())
            .await??
            .iter()
            .any(|tool| tool.name == "adb_logcat")
    );
    disconnect(client, child).await
}

#[tokio::test]
async fn logcat_process_failure_is_a_tool_error() -> TestResult {
    assert_logcat_error("logcat_failure", "device offline").await
}

#[tokio::test]
async fn logcat_timeout_is_a_tool_error() -> TestResult {
    assert_logcat_error("logcat_timeout", "ADB logcat timed out after 5 seconds").await
}

#[tokio::test]
async fn excessive_logcat_stderr_is_bounded() -> TestResult {
    assert_logcat_error("logcat_stderr_large", "stderr exceeded the 16 KiB limit").await
}

#[tokio::test]
async fn non_utf8_logcat_is_a_tool_error() -> TestResult {
    assert_logcat_error("logcat_invalid_utf8", "not valid UTF-8").await
}

#[tokio::test]
async fn logcat_filters_are_passed_as_exact_native_arguments() -> TestResult {
    for (mode, filters) in [
        ("logcat_pid", serde_json::json!({"pid": 1000})),
        ("logcat_pid_max", serde_json::json!({"pid": 2147483647u32})),
        (
            "logcat_tags",
            serde_json::json!({"tags": ["ExampleApp", "AudioController"]}),
        ),
        (
            "logcat_pid_tags",
            serde_json::json!({"pid": 1000, "tags": ["ExampleApp"]}),
        ),
        (
            "logcat_pid_priority",
            serde_json::json!({"pid": 1000, "priority": "error"}),
        ),
        (
            "logcat_tags_priority",
            serde_json::json!({"tags": ["ExampleApp", "AudioController"], "priority": "warn"}),
        ),
        (
            "logcat_combined",
            serde_json::json!({"pid": 2210, "tags": ["ErrorProcessor", "AudioController"], "priority": "warn"}),
        ),
    ] {
        let (client, child) = connect(mode).await?;
        let mut arguments = filters
            .as_object()
            .cloned()
            .ok_or("Expected filter object")?;
        arguments.insert("serial".into(), serde_json::json!("emulator-5554"));
        let result = timeout(TIMEOUT, client.call_tool(logcat_request(arguments.into()))).await??;
        assert_ne!(result.is_error, Some(true), "Filter mode failed: {mode}");
        let snapshot: serde_json::Value =
            serde_json::from_str(&result.content[0].as_text().ok_or("Expected JSON")?.text)?;
        assert_eq!(
            snapshot,
            serde_json::json!({"serial": "emulator-5554", "format": "threadtime", "output": include_str!("fixtures/logcat/threadtime.txt"), "truncated": false})
        );
        disconnect(client, child).await?;
    }
    Ok(())
}

#[tokio::test]
async fn logcat_supports_every_minimum_priority() -> TestResult {
    for priority in ["verbose", "debug", "info", "warn", "error", "fatal"] {
        let (client, child) = connect(&format!("logcat_{priority}")).await?;
        let result = timeout(
            TIMEOUT,
            client.call_tool(logcat_request(
                serde_json::json!({"serial": "emulator-5554", "priority": priority}),
            )),
        )
        .await??;
        assert_ne!(result.is_error, Some(true), "Priority failed: {priority}");
        disconnect(client, child).await?;
    }
    Ok(())
}

#[tokio::test]
async fn empty_or_null_optional_filters_do_not_change_logcat_arguments() -> TestResult {
    let (client, child) = connect("logcat").await?;
    let result = timeout(TIMEOUT, client.call_tool(logcat_request(serde_json::json!({"serial": "emulator-5554", "pid": null, "tags": [], "priority": null})))).await??;
    assert_ne!(result.is_error, Some(true));
    disconnect(client, child).await
}

#[tokio::test]
async fn logcat_rejects_invalid_filter_values_without_losing_connection() -> TestResult {
    let (client, child) = connect("logcat").await?;
    let mut invalid = vec![
        serde_json::json!({"pid": 0}),
        serde_json::json!({"pid": -1}),
        serde_json::json!({"pid": 2147483648u32}),
        serde_json::json!({"pid": "123"}),
        serde_json::json!({"pid": 1.5}),
        serde_json::json!({"priority": "W"}),
        serde_json::json!({"priority": "silent"}),
        serde_json::json!({"priority": "unknown"}),
        serde_json::json!({"priority": 3}),
        serde_json::json!({"tags": "ExampleApp"}),
        serde_json::json!({"tags": null}),
        serde_json::json!({"tags": [123]}),
        serde_json::json!({"tags": ["Tag".repeat(43)]}),
        serde_json::json!({"tags": vec!["Tag"; 21]}),
    ];
    for tag in [
        "",
        "*",
        "Example:W",
        "--pid=123",
        "tag with space",
        "line\nbreak",
        "a\0b",
        "tag;id",
        "$(id)",
        "tag|id",
        "tag&id",
        "`id`",
        "a'b",
        "a\"b",
    ] {
        invalid.push(serde_json::json!({"tags": [tag]}));
    }
    for filters in invalid {
        let mut arguments = filters.as_object().cloned().ok_or("Expected filters")?;
        arguments.insert("serial".into(), serde_json::json!("emulator-5554"));
        let result = timeout(TIMEOUT, client.call_tool(logcat_request(arguments.into()))).await?;
        match result {
            Ok(result) => assert_eq!(result.is_error, Some(true)),
            Err(ServiceError::McpError(_)) => {}
            Err(error) => return Err(error.into()),
        }
    }
    let result = timeout(
        TIMEOUT,
        client.call_tool(logcat_request(
            serde_json::json!({"serial": "emulator-5554"}),
        )),
    )
    .await??;
    assert_ne!(result.is_error, Some(true));
    disconnect(client, child).await
}

async fn connect_package(mode: &str) -> TestResult<(Client, Child, tempfile::TempDir)> {
    let directory = tempfile::tempdir()?;
    let mut command = server_command();
    command
        .env("ANDROID_MCP_ADB", adb_fixture()?)
        .env("ANDROID_MCP_TEST_ADB_MODE", mode)
        .env(
            "ANDROID_MCP_TEST_PACKAGE_TRACE",
            directory.path().join("trace"),
        );
    let (client, child) = connect_command(command).await?;
    Ok((client, child, directory))
}

fn package_request() -> CallToolRequestParams {
    logcat_request(
        serde_json::json!({"serial": "emulator-5554", "package": "com.github.bschramke.myfinance"}),
    )
}

#[tokio::test]
async fn package_logcat_resolves_the_selected_devices_pid() -> TestResult {
    let (client, child, directory) = connect_package("logcat_package").await?;
    let result = timeout(TIMEOUT, client.call_tool(package_request())).await??;
    assert_ne!(result.is_error, Some(true));
    let snapshot: serde_json::Value = serde_json::from_str(
        &result.content[0]
            .as_text()
            .ok_or("Expected snapshot JSON")?
            .text,
    )?;
    assert_eq!(
        snapshot,
        serde_json::json!({"serial": "emulator-5554", "format": "threadtime", "output": include_str!("fixtures/logcat/threadtime.txt"), "truncated": false})
    );
    assert_eq!(
        std::fs::read_to_string(directory.path().join("trace"))?,
        "pidof\nlogcat\n"
    );
    disconnect(client, child).await
}

#[tokio::test]
async fn package_logcat_combines_tags_and_priority() -> TestResult {
    let (client, child, _directory) = connect_package("logcat_package_combined").await?;
    let result = timeout(TIMEOUT, client.call_tool(logcat_request(serde_json::json!({"serial": "emulator-5554", "package": "com.github.bschramke.myfinance", "tags": ["ExampleApp", "AudioController"], "priority": "warn"})))).await??;
    assert_ne!(result.is_error, Some(true));
    disconnect(client, child).await
}

#[tokio::test]
async fn package_pid_is_resolved_again_for_every_snapshot() -> TestResult {
    let (client, child, directory) = connect_package("logcat_package_fresh").await?;
    for _ in 0..2 {
        let result = timeout(TIMEOUT, client.call_tool(package_request())).await??;
        assert_ne!(result.is_error, Some(true));
    }
    assert_eq!(
        std::fs::read_to_string(directory.path().join("trace"))?,
        "pidof\nlogcat\npidof\nlogcat\n"
    );
    disconnect(client, child).await
}

#[tokio::test]
async fn package_lookup_failures_never_fall_back_to_unfiltered_logs() -> TestResult {
    for (mode, message) in [
        ("logcat_package_absent", "No running main process"),
        ("logcat_package_empty", "No running main process"),
        ("logcat_package_offline", "device offline"),
        ("logcat_package_unsupported", "pidof: not found"),
        ("logcat_package_invalid", "not a valid process ID"),
        ("logcat_package_zero", "between 1 and 2147483647"),
        ("logcat_package_multiple", "Multiple main-process PIDs"),
        ("logcat_package_invalid_utf8", "not valid UTF-8"),
        (
            "logcat_package_stdout_large",
            "stdout exceeded the 4 KiB limit",
        ),
        (
            "logcat_package_stderr_large",
            "stderr exceeded the 16 KiB limit",
        ),
    ] {
        let (client, child, directory) = connect_package(mode).await?;
        let result = timeout(TIMEOUT, client.call_tool(package_request())).await??;
        assert_eq!(result.is_error, Some(true), "Unexpected success for {mode}");
        let message_text = &result.content[0]
            .as_text()
            .ok_or("Expected error text")?
            .text;
        assert!(
            message_text.contains(message),
            "Unexpected error for {mode}: {message_text}"
        );
        assert_eq!(
            std::fs::read_to_string(directory.path().join("trace"))?,
            "pidof\n"
        );
        assert!(
            timeout(TIMEOUT, client.list_all_tools())
                .await??
                .iter()
                .any(|tool| tool.name == "adb_logcat")
        );
        disconnect(client, child).await?;
    }
    Ok(())
}

#[tokio::test]
async fn stalled_package_lookup_is_bounded_and_does_not_run_logcat() -> TestResult {
    let (client, child, directory) = connect_package("logcat_package_timeout").await?;
    let result = timeout(TIMEOUT, client.call_tool(package_request())).await??;
    assert_eq!(result.is_error, Some(true));
    assert!(
        result.content[0]
            .as_text()
            .ok_or("Expected error")?
            .text
            .contains("during package PID lookup")
    );
    assert_eq!(
        std::fs::read_to_string(directory.path().join("trace"))?,
        "pidof\n"
    );
    disconnect(client, child).await
}

#[tokio::test]
async fn package_lookup_and_logcat_share_one_time_budget() -> TestResult {
    let (client, child, directory) = connect_package("logcat_package_budget").await?;
    let result = timeout(TIMEOUT, client.call_tool(package_request())).await??;
    assert_eq!(result.is_error, Some(true));
    assert!(
        result.content[0]
            .as_text()
            .ok_or("Expected timeout")?
            .text
            .contains("ADB logcat timed out after 5 seconds")
    );
    assert_eq!(
        std::fs::read_to_string(directory.path().join("trace"))?,
        "pidof\nlogcat\n"
    );
    disconnect(client, child).await
}

#[tokio::test]
async fn invalid_packages_and_pid_conflicts_are_rejected_before_pidof() -> TestResult {
    let (client, child, directory) = connect_package("logcat_package").await?;
    let mut invalid = vec![
        serde_json::json!({"package": "com.github.bschramke.myfinance", "pid": 1000}),
        serde_json::json!({"package": 123}),
        serde_json::json!({"package": "com.example", "tags": ["*:V"]}),
        serde_json::json!({"package": format!("com.{}", "a".repeat(252))}),
    ];
    for package in [
        "",
        "android",
        ".com.example",
        "com..example",
        "com.example.",
        "com.example:worker",
        "com.1example",
        "com._example",
        "com.example;id",
        "com.example$(id)",
        "com.example app",
        "com.example\napp",
        "com.example\u{0000}app",
        "com.éxample",
        "--help",
        "com.example/app",
        "com.example*",
    ] {
        invalid.push(serde_json::json!({"package": package}));
    }
    for arguments in invalid {
        let mut arguments = arguments.as_object().cloned().ok_or("Expected arguments")?;
        arguments.insert("serial".into(), serde_json::json!("emulator-5554"));
        let result = timeout(TIMEOUT, client.call_tool(logcat_request(arguments.into()))).await?;
        match result {
            Ok(result) => assert_eq!(result.is_error, Some(true)),
            Err(ServiceError::McpError(_)) => {}
            Err(error) => return Err(error.into()),
        }
    }
    assert!(!directory.path().join("trace").exists());
    disconnect(client, child).await
}

#[tokio::test]
async fn null_package_preserves_the_original_logcat_command() -> TestResult {
    let (client, child) = connect("logcat").await?;
    let result = timeout(
        TIMEOUT,
        client.call_tool(logcat_request(
            serde_json::json!({"serial": "emulator-5554", "package": null}),
        )),
    )
    .await??;
    assert_ne!(result.is_error, Some(true));
    disconnect(client, child).await
}
