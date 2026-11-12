mod support;

use rmcp::model::CallToolRequestParams;
use support::adb::adb_fixture;
use support::{Client, TIMEOUT, TestResult, connect_command, disconnect, server_command};
use tokio::{process::Child, time::timeout};

const CRASH_FILE_BYTES: &[u8] = b"\0synthetic crash bytes\xff";

fn request(name: &str, arguments: serde_json::Value) -> CallToolRequestParams {
    let mut request = CallToolRequestParams::new(name.to_owned());
    request.arguments = arguments.as_object().cloned();
    request
}

async fn connect_crashes(content_mode: &str) -> TestResult<(Client, Child)> {
    let mut command = server_command();
    command
        .env("ANDROID_MCP_ADB", adb_fixture()?)
        .env("ANDROID_MCP_TEST_ADB_MODE", "crash_all")
        .env("ANDROID_MCP_TEST_CRASH_CONTENT", content_mode);
    connect_command(command).await
}

async fn first_crash_id(client: &Client) -> TestResult<String> {
    let result = timeout(
        TIMEOUT,
        client.call_tool(request(
            "adb_list_crashes",
            serde_json::json!({"serial":"emulator-5554"}),
        )),
    )
    .await??;
    assert_ne!(result.is_error, Some(true));
    let listing: serde_json::Value = serde_json::from_str(
        &result.content[0]
            .as_text()
            .ok_or("Expected crash list JSON")?
            .text,
    )?;
    Ok(listing["reports"][0]["id"]
        .as_str()
        .ok_or("Missing crash ID")?
        .to_owned())
}

#[tokio::test]
async fn lists_crash_metadata_without_report_contents_and_applies_limit() -> TestResult {
    let (client, child) = connect_crashes("").await?;
    let result = timeout(
        TIMEOUT,
        client.call_tool(request(
            "adb_list_crashes",
            serde_json::json!({"serial":"emulator-5554", "limit":2}),
        )),
    )
    .await??;
    assert_ne!(result.is_error, Some(true));
    let listing: serde_json::Value = serde_json::from_str(
        &result.content[0]
            .as_text()
            .ok_or("Expected crash list JSON")?
            .text,
    )?;
    assert_eq!(listing["reports"].as_array().unwrap().len(), 2);
    assert_eq!(listing["reports"][0]["tag"], "data_app_crash");
    assert_eq!(listing["reports"][1]["tag"], "system_server_anr");
    assert_eq!(listing["truncated"], true);
    assert!(listing["reports"][0].get("content").is_none());
    disconnect(client, child).await
}

#[tokio::test]
async fn applies_dropbox_date_time_and_tag_filters() -> TestResult {
    let (client, child) = connect_crashes("").await?;
    let result = timeout(
        TIMEOUT,
        client.call_tool(request(
            "adb_list_crashes",
            serde_json::json!({
                "serial":"emulator-5554",
                "date":"2026-01-02",
                "time":"03:04:05.006",
                "tag":"data_app_crash"
            }),
        )),
    )
    .await??;
    assert_ne!(result.is_error, Some(true));
    let listing: serde_json::Value = serde_json::from_str(
        &result.content[0]
            .as_text()
            .ok_or("Expected filtered crash list JSON")?
            .text,
    )?;
    assert_eq!(listing["reports"].as_array().unwrap().len(), 1);
    assert_eq!(listing["reports"][0]["tag"], "data_app_crash");
    disconnect(client, child).await
}

#[tokio::test]
async fn gets_crash_text_from_protobuf() -> TestResult {
    let (client, child) = connect_crashes("").await?;
    let id = first_crash_id(&client).await?;
    let result = timeout(
        TIMEOUT,
        client.call_tool(request(
            "adb_get_crash",
            serde_json::json!({"serial":"emulator-5554", "id":id}),
        )),
    )
    .await??;
    assert_ne!(result.is_error, Some(true));
    let crash: serde_json::Value = serde_json::from_str(
        &result.content[0]
            .as_text()
            .ok_or("Expected crash JSON")?
            .text,
    )?;
    assert_eq!(crash["tag"], "data_app_crash");
    assert_eq!(crash["encoding"], "utf-8");
    assert_eq!(crash["content"], "synthetic crash report body");
    assert_eq!(crash["possibly_truncated"], false);
    disconnect(client, child).await
}

#[tokio::test]
async fn represents_binary_crash_content_as_base64() -> TestResult {
    let (client, child) = connect_crashes("crash_proto_binary").await?;
    let id = first_crash_id(&client).await?;
    let result = timeout(
        TIMEOUT,
        client.call_tool(request(
            "adb_get_crash",
            serde_json::json!({"serial":"emulator-5554", "id":id}),
        )),
    )
    .await??;
    assert_ne!(result.is_error, Some(true));
    let crash: serde_json::Value = serde_json::from_str(
        &result.content[0]
            .as_text()
            .ok_or("Expected crash JSON")?
            .text,
    )?;
    assert_eq!(crash["encoding"], "base64");
    assert_eq!(crash["content"], "AHN5bnRoZXRpYyBjcmFzaCBieXRlc/8=");
    disconnect(client, child).await
}

#[tokio::test]
async fn copies_original_crash_file_without_modifying_its_bytes() -> TestResult {
    let (client, child) = connect_crashes("").await?;
    let id = first_crash_id(&client).await?;
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("crash.bin");
    let result = timeout(
        TIMEOUT,
        client.call_tool(request(
            "adb_get_crash_file",
            serde_json::json!({
                "serial":"emulator-5554",
                "id":id,
                "output_path":path,
            }),
        )),
    )
    .await??;
    assert_ne!(result.is_error, Some(true));
    let file: serde_json::Value = serde_json::from_str(
        &result.content[0]
            .as_text()
            .ok_or("Expected crash file JSON")?
            .text,
    )?;
    assert_eq!(file["path"], path.to_string_lossy().as_ref());
    assert_eq!(file["size_bytes"], CRASH_FILE_BYTES.len());
    assert_eq!(std::fs::read(path)?, CRASH_FILE_BYTES);
    disconnect(client, child).await
}
