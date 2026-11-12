mod support;

use rmcp::model::CallToolRequestParams;
use support::adb::connect;
use support::{TIMEOUT, TestResult, disconnect};
use tokio::time::{Duration, timeout};

fn list_apps_request(serial: &str) -> CallToolRequestParams {
    let mut request = CallToolRequestParams::new("adb_list_apps");
    request.arguments = serde_json::json!({"serial": serial}).as_object().cloned();
    request
}

fn list_apps_request_with_system_filter(
    serial: &str,
    exclude_system: bool,
) -> CallToolRequestParams {
    let mut request = list_apps_request(serial);
    request
        .arguments
        .as_mut()
        .unwrap()
        .insert("exclude_system".into(), serde_json::json!(exclude_system));
    request
}

#[tokio::test]
async fn returns_sorted_installed_package_names() -> TestResult {
    let (client, child) = connect("apps").await?;
    let result = timeout(
        TIMEOUT,
        client.call_tool(list_apps_request("emulator-5554")),
    )
    .await??;
    assert_ne!(result.is_error, Some(true));
    let apps: serde_json::Value = serde_json::from_str(
        &result.content[0]
            .as_text()
            .ok_or("Expected app-list JSON")?
            .text,
    )?;
    assert_eq!(
        apps,
        serde_json::json!({
            "serial": "emulator-5554",
            "apps": ["com.example.app", "org.example"]
        })
    );
    disconnect(client, child).await
}

#[tokio::test]
async fn returns_packages_from_emulator_fixtures() -> TestResult {
    for (serial, mode, fixture) in [
        (
            "emulator-5554",
            "apps_emulator_5554",
            include_str!("fixtures/apps/emulator-5554-packages-list.txt"),
        ),
        (
            "emulator-5556",
            "apps_emulator_5556",
            include_str!("fixtures/apps/emulator-5556-packages-list.txt"),
        ),
    ] {
        let (client, child) = connect(mode).await?;
        let result = timeout(TIMEOUT, client.call_tool(list_apps_request(serial))).await??;
        assert_ne!(result.is_error, Some(true));
        let apps: serde_json::Value = serde_json::from_str(
            &result.content[0]
                .as_text()
                .ok_or("Expected app-list JSON")?
                .text,
        )?;
        let mut expected: Vec<_> = fixture
            .lines()
            .map(|line| {
                line.strip_prefix("package:")
                    .ok_or("Malformed app fixture entry")
            })
            .collect::<Result<_, _>>()?;
        expected.sort_unstable();
        expected.dedup();
        assert_eq!(
            apps,
            serde_json::json!({"serial": serial, "apps": expected})
        );
        disconnect(client, child).await?;
    }
    Ok(())
}

#[tokio::test]
async fn excludes_system_packages_when_requested() -> TestResult {
    for serial in ["emulator-5554", "emulator-5556"] {
        let (client, child) = connect("apps_exclude_system").await?;
        let result = timeout(
            TIMEOUT,
            client.call_tool(list_apps_request_with_system_filter(serial, true)),
        )
        .await??;
        assert_ne!(result.is_error, Some(true));
        let apps: serde_json::Value = serde_json::from_str(
            &result.content[0]
                .as_text()
                .ok_or("Expected filtered app-list JSON")?
                .text,
        )?;
        assert_eq!(
            apps,
            serde_json::json!({
                "serial": serial,
                "apps": [
                    "com.dhl.hasci.clickdummy",
                    "com.dhl.hasci.clickdummy.test",
                    "com.github.bschramke.myfinance"
                ]
            })
        );
        disconnect(client, child).await?;
    }
    Ok(())
}

#[tokio::test]
async fn accepts_empty_app_list() -> TestResult {
    let (client, child) = connect("apps_empty").await?;
    let result = timeout(
        TIMEOUT,
        client.call_tool(list_apps_request("emulator-5554")),
    )
    .await??;
    assert_ne!(result.is_error, Some(true));
    let apps: serde_json::Value = serde_json::from_str(
        &result.content[0]
            .as_text()
            .ok_or("Expected app-list JSON")?
            .text,
    )?;
    assert_eq!(apps["apps"], serde_json::json!([]));
    disconnect(client, child).await
}

#[tokio::test]
async fn reports_malformed_output_and_adb_failures() -> TestResult {
    for (mode, expected) in [
        ("apps_malformed", "Malformed ADB app listing"),
        ("apps_invalid_utf8", "not valid UTF-8"),
        ("apps_failure", "package manager unavailable"),
    ] {
        let (client, child) = connect(mode).await?;
        let result = timeout(
            TIMEOUT,
            client.call_tool(list_apps_request("emulator-5554")),
        )
        .await??;
        assert_eq!(result.is_error, Some(true));
        assert!(
            result.content[0]
                .as_text()
                .ok_or("Expected app-list error")?
                .text
                .contains(expected)
        );
        disconnect(client, child).await?;
    }
    Ok(())
}

#[tokio::test]
async fn rejects_shell_like_serial_and_passes_other_serials_literally() -> TestResult {
    let (client, child) = connect("apps").await?;
    let result = timeout(
        TIMEOUT,
        client.call_tool(list_apps_request("serial;not-a-shell-command")),
    )
    .await??;
    assert_ne!(result.is_error, Some(true));
    disconnect(client, child).await
}

#[tokio::test]
async fn bounds_command_timeout() -> TestResult {
    let (client, child) = connect("apps_timeout").await?;
    let result = timeout(
        TIMEOUT + Duration::from_secs(2),
        client.call_tool(list_apps_request("emulator-5554")),
    )
    .await??;
    assert_eq!(result.is_error, Some(true));
    assert!(
        result.content[0]
            .as_text()
            .ok_or("Expected app-list timeout")?
            .text
            .contains("ADB app listing timed out after 5 seconds")
    );
    disconnect(client, child).await
}
