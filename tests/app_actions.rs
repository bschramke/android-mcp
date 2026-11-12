mod support;

use rmcp::model::CallToolRequestParams;
use support::adb::connect;
use support::{TIMEOUT, TestResult, disconnect};
use tokio::time::timeout;

fn install_request(
    serial: &str,
    apk_path: &std::path::Path,
    replace: bool,
    downgrade: bool,
) -> CallToolRequestParams {
    let mut request = CallToolRequestParams::new("adb_install_app");
    request.arguments = serde_json::json!({
        "serial": serial,
        "apk_path": apk_path.to_string_lossy(),
        "replace": replace,
        "downgrade": downgrade,
    })
    .as_object()
    .cloned();
    request
}

fn uninstall_request(serial: &str, package: &str) -> CallToolRequestParams {
    let mut request = CallToolRequestParams::new("adb_uninstall_app");
    request.arguments = serde_json::json!({"serial": serial, "package": package})
        .as_object()
        .cloned();
    request
}

fn clear_app_data_request(serial: &str, package: &str) -> CallToolRequestParams {
    let mut request = CallToolRequestParams::new("adb_clear_app_data");
    request.arguments = serde_json::json!({"serial": serial, "package": package})
        .as_object()
        .cloned();
    request
}

fn app_process_request(
    tool_name: &'static str,
    serial: &str,
    package: &str,
) -> CallToolRequestParams {
    let mut request = CallToolRequestParams::new(tool_name);
    request.arguments = serde_json::json!({"serial": serial, "package": package})
        .as_object()
        .cloned();
    request
}

fn create_apk_file(directory: &tempfile::TempDir) -> TestResult<std::path::PathBuf> {
    let apk_path = directory.path().join("sample app.apk");
    std::fs::write(&apk_path, b"APK fixture")?;
    Ok(apk_path)
}

#[tokio::test]
async fn installs_apk_without_replace_by_default() -> TestResult {
    let directory = tempfile::tempdir()?;
    let apk_path = create_apk_file(&directory)?;
    let (client, child) = connect("install").await?;
    let result = timeout(
        TIMEOUT,
        client.call_tool(install_request("emulator-5554", &apk_path, false, false)),
    )
    .await??;
    assert_ne!(result.is_error, Some(true));
    disconnect(client, child).await
}

#[tokio::test]
async fn replace_and_downgrade_flags_are_passed_to_adb() -> TestResult {
    let directory = tempfile::tempdir()?;
    let apk_path = create_apk_file(&directory)?;
    for (mode, replace, downgrade) in [
        ("install_replace", true, false),
        ("install_downgrade", false, true),
    ] {
        let (client, child) = connect(mode).await?;
        let result = timeout(
            TIMEOUT,
            client.call_tool(install_request(
                "emulator-5554",
                &apk_path,
                replace,
                downgrade,
            )),
        )
        .await??;
        assert_ne!(result.is_error, Some(true));
        disconnect(client, child).await?;
    }
    Ok(())
}

#[tokio::test]
async fn rejects_non_apk_and_missing_files() -> TestResult {
    let directory = tempfile::tempdir()?;
    let wrong_extension = directory.path().join("sample app.txt");
    std::fs::write(&wrong_extension, b"not an APK")?;
    let missing_apk = directory.path().join("missing.apk");

    for path in [&wrong_extension, &missing_apk] {
        let (client, child) = connect("install").await?;
        let result = timeout(
            TIMEOUT,
            client.call_tool(install_request("emulator-5554", path, false, false)),
        )
        .await??;
        assert_eq!(result.is_error, Some(true));
        disconnect(client, child).await?;
    }
    Ok(())
}

#[tokio::test]
async fn uninstalls_app_by_package_name() -> TestResult {
    let (client, child) = connect("uninstall").await?;
    let result = timeout(
        TIMEOUT,
        client.call_tool(uninstall_request("emulator-5554", "com.example.app")),
    )
    .await??;
    assert_ne!(result.is_error, Some(true));
    disconnect(client, child).await
}

#[tokio::test]
async fn clears_app_data_for_valid_package() -> TestResult {
    let (client, child) = connect("clear_app_data").await?;
    let result = timeout(
        TIMEOUT,
        client.call_tool(clear_app_data_request("emulator-5554", "com.example.app")),
    )
    .await??;
    assert_ne!(result.is_error, Some(true));
    disconnect(client, child).await
}

#[tokio::test]
async fn launches_app_by_application_id() -> TestResult {
    let (client, child) = connect("launch_app").await?;
    let result = timeout(
        TIMEOUT,
        client.call_tool(app_process_request(
            "adb_launch_app",
            "emulator-5554",
            "com.example.app",
        )),
    )
    .await??;
    assert_ne!(result.is_error, Some(true));
    disconnect(client, child).await
}

#[tokio::test]
async fn terminates_app_by_application_id() -> TestResult {
    let (client, child) = connect("terminate_app").await?;
    let result = timeout(
        TIMEOUT,
        client.call_tool(app_process_request(
            "adb_terminate_app",
            "emulator-5554",
            "com.example.app",
        )),
    )
    .await??;
    assert_ne!(result.is_error, Some(true));
    disconnect(client, child).await
}

#[tokio::test]
async fn rejects_invalid_package_names_for_app_process_tools() -> TestResult {
    for tool_name in ["adb_launch_app", "adb_terminate_app"] {
        let mode = if tool_name == "adb_launch_app" {
            "launch_app"
        } else {
            "terminate_app"
        };
        let (client, child) = connect(mode).await?;
        let result = timeout(
            TIMEOUT,
            client.call_tool(app_process_request(
                tool_name,
                "emulator-5554",
                "com.example;app",
            )),
        )
        .await??;
        assert_eq!(result.is_error, Some(true));
        disconnect(client, child).await?;
    }
    Ok(())
}

#[tokio::test]
async fn reports_app_process_failures() -> TestResult {
    for (tool_name, mode, expected) in [
        (
            "adb_launch_app",
            "launch_app_failure",
            "ADB app launch failed",
        ),
        (
            "adb_terminate_app",
            "terminate_app_failure",
            "ADB app termination failed",
        ),
    ] {
        let (client, child) = connect(mode).await?;
        let result = timeout(
            TIMEOUT,
            client.call_tool(app_process_request(
                tool_name,
                "emulator-5554",
                "com.example.app",
            )),
        )
        .await??;
        assert_eq!(result.is_error, Some(true));
        assert!(
            result.content[0]
                .as_text()
                .ok_or("Expected app-process error")?
                .text
                .contains(expected)
        );
        disconnect(client, child).await?;
    }
    Ok(())
}

#[tokio::test]
async fn rejects_invalid_package_for_app_data_clear() -> TestResult {
    let (client, child) = connect("clear_app_data").await?;
    let result = timeout(
        TIMEOUT,
        client.call_tool(clear_app_data_request("emulator-5554", "com.example;id")),
    )
    .await??;
    assert_eq!(result.is_error, Some(true));
    disconnect(client, child).await
}

#[tokio::test]
async fn reports_app_data_clear_failures() -> TestResult {
    for (mode, expected) in [
        ("clear_app_data_failure", "ADB app data clear failed"),
        ("clear_app_data_unexpected", "did not report Success"),
    ] {
        let (client, child) = connect(mode).await?;
        let result = timeout(
            TIMEOUT,
            client.call_tool(clear_app_data_request("emulator-5554", "com.example.app")),
        )
        .await??;
        assert_eq!(result.is_error, Some(true));
        assert!(
            result.content[0]
                .as_text()
                .ok_or("Expected app-data-clear error")?
                .text
                .contains(expected)
        );
        disconnect(client, child).await?;
    }
    Ok(())
}

#[tokio::test]
async fn rejects_invalid_uninstall_package_names() -> TestResult {
    let (client, child) = connect("uninstall").await?;
    let result = timeout(
        TIMEOUT,
        client.call_tool(uninstall_request("emulator-5554", "com.example;id")),
    )
    .await??;
    assert_eq!(result.is_error, Some(true));
    disconnect(client, child).await
}

#[tokio::test]
async fn returns_adb_install_and_uninstall_failures() -> TestResult {
    let directory = tempfile::tempdir()?;
    let apk_path = create_apk_file(&directory)?;
    let (client, child) = connect("install_failure").await?;
    let result = timeout(
        TIMEOUT,
        client.call_tool(install_request("emulator-5554", &apk_path, false, false)),
    )
    .await??;
    assert_eq!(result.is_error, Some(true));
    assert!(
        result.content[0]
            .as_text()
            .ok_or("Expected install error")?
            .text
            .contains("INSTALL_FAILED_INVALID_APK")
    );
    disconnect(client, child).await?;

    let (client, child) = connect("uninstall_failure").await?;
    let result = timeout(
        TIMEOUT,
        client.call_tool(uninstall_request("emulator-5554", "com.example.app")),
    )
    .await??;
    assert_eq!(result.is_error, Some(true));
    assert!(
        result.content[0]
            .as_text()
            .ok_or("Expected uninstall error")?
            .text
            .contains("DELETE_FAILED_INTERNAL_ERROR")
    );
    disconnect(client, child).await
}
