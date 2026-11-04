#![cfg(unix)]

mod support;

use std::time::Duration;

use rmcp::model::CallToolRequestParams;
use support::adb::connect;
use support::{TIMEOUT, TestResult, disconnect};
use tokio::time::timeout;

fn screen_elements_request(serial: &str) -> CallToolRequestParams {
    let mut request = CallToolRequestParams::new("adb_list_elements_on_screen");
    request.arguments = serde_json::json!({"serial": serial}).as_object().cloned();
    request
}

fn screen_hierarchy_request(serial: &str) -> CallToolRequestParams {
    let mut request = CallToolRequestParams::new("adb_get_screen_hierarchy");
    request.arguments = serde_json::json!({"serial": serial}).as_object().cloned();
    request
}

fn hierarchy_node_count(nodes: &serde_json::Value) -> usize {
    nodes
        .as_array()
        .unwrap()
        .iter()
        .map(|node| 1 + hierarchy_node_count(&node["children"]))
        .sum()
}
#[tokio::test]
async fn list_elements_on_screen_parses_both_device_fixtures() -> TestResult {
    let (client, child) = connect("ui_elements").await?;
    for (serial, expected_count) in [("emulator-5554", 991), ("emulator-5556", 69)] {
        let result = timeout(TIMEOUT, client.call_tool(screen_elements_request(serial))).await??;
        assert_ne!(result.is_error, Some(true));
        let hierarchy: serde_json::Value = serde_json::from_str(
            &result.content[0]
                .as_text()
                .ok_or("Expected UI hierarchy JSON")?
                .text,
        )?;
        assert_eq!(hierarchy["serial"], serial);
        let elements = hierarchy["elements"].as_array().unwrap();
        assert_eq!(elements.len(), expected_count);
        assert!(elements.iter().any(|element| element["bounds"].is_object()));
        assert!(
            elements
                .iter()
                .all(|element| element.get("index").is_none())
        );

        let result = timeout(TIMEOUT, client.call_tool(screen_hierarchy_request(serial))).await??;
        assert_ne!(result.is_error, Some(true));
        let hierarchy: serde_json::Value = serde_json::from_str(
            &result.content[0]
                .as_text()
                .ok_or("Expected nested hierarchy JSON")?
                .text,
        )?;
        assert_eq!(hierarchy["serial"], serial);
        assert!(hierarchy["elements"].is_null());
        assert_eq!(hierarchy_node_count(&hierarchy["roots"]), expected_count);
        assert!(hierarchy["roots"].as_array().is_some_and(|roots| {
            roots
                .iter()
                .any(|root| !root["children"].as_array().unwrap().is_empty())
        }));
    }
    disconnect(client, child).await
}

#[tokio::test]
async fn list_elements_on_screen_rejects_malformed_xml_and_adb_failures() -> TestResult {
    for (mode, expected) in [
        ("ui_elements_malformed", "UI hierarchy XML is malformed"),
        ("ui_elements_failure", "uiautomator unavailable"),
    ] {
        let (client, child) = connect(mode).await?;
        let result = timeout(
            TIMEOUT,
            client.call_tool(screen_elements_request("emulator-5554")),
        )
        .await??;
        assert_eq!(result.is_error, Some(true));
        assert!(
            result.content[0]
                .as_text()
                .ok_or("Expected hierarchy error")?
                .text
                .contains(expected)
        );
        disconnect(client, child).await?;
    }
    Ok(())
}

#[tokio::test]
async fn list_elements_on_screen_timeout_and_output_are_bounded() -> TestResult {
    let (client, child) = connect("ui_elements_timeout").await?;
    let result = timeout(
        TIMEOUT + Duration::from_secs(2),
        client.call_tool(screen_elements_request("emulator-5554")),
    )
    .await??;
    assert_eq!(result.is_error, Some(true));
    assert!(
        result.content[0]
            .as_text()
            .ok_or("Expected hierarchy timeout")?
            .text
            .contains("ADB UI hierarchy capture timed out after 10 seconds")
    );
    disconnect(client, child).await?;

    let (client, child) = connect("ui_elements_large").await?;
    let result = timeout(
        TIMEOUT,
        client.call_tool(screen_elements_request("emulator-5554")),
    )
    .await??;
    assert_eq!(result.is_error, Some(true));
    assert!(
        result.content[0]
            .as_text()
            .ok_or("Expected hierarchy output-limit error")?
            .text
            .contains("exceeded the 1 MiB limit")
    );
    disconnect(client, child).await
}
