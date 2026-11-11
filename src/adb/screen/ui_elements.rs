use std::{
    collections::HashMap,
    io,
    path::Path,
    sync::{Arc, Mutex as StdMutex, OnceLock, Weak},
    time::Duration,
};

use roxmltree::{Document, Node};
use serde::Serialize;
use tokio::{
    sync::Mutex as AsyncMutex,
    time::{Instant, timeout_at},
};

use super::super::{
    command::{CommandOptions, ensure_success, run_shell},
    device_info::validate_serial,
};

const UI_HIERARCHY_LIMIT: usize = 1024 * 1024;
const UI_HIERARCHY_STDERR_LIMIT: usize = 16 * 1024;
const UI_HIERARCHY_TIMEOUT: Duration = Duration::from_secs(10);
const UI_HIERARCHY_TIMEOUT_MESSAGE: &str = "ADB UI hierarchy capture timed out after 10 seconds";
static UI_HIERARCHY_LOCKS: OnceLock<StdMutex<HashMap<String, Weak<AsyncMutex<()>>>>> =
    OnceLock::new();
// uiautomator dump requires a device-side file; the trap removes it even when the dump fails.
const UI_HIERARCHY_SCRIPT: &str = concat!(
    "tmp=/sdcard/.android-mcp-ui-$$.xml; ",
    "trap 'rm -f \"$tmp\"' EXIT; ",
    "uiautomator dump \"$tmp\" >/dev/null || exit $?; ",
    "cat \"$tmp\""
);

#[derive(Debug, Serialize)]
pub struct UiElementList {
    pub serial: String,
    pub elements: Vec<UiElement>,
}

#[derive(Debug, Serialize)]
pub struct UiHierarchyTree {
    pub serial: String,
    pub roots: Vec<UiHierarchyNode>,
}

#[derive(Debug, Serialize)]
pub struct UiHierarchyNode {
    pub element: UiElement,
    pub children: Vec<UiHierarchyNode>,
}

#[derive(Debug, Serialize)]
pub struct UiElement {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub class: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resource_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_desc: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub package: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bounds: Option<ElementBounds>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub clickable: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub focusable: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub focused: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scrollable: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub long_clickable: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub checkable: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub checked: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selected: Option<bool>,
}

#[derive(Debug, Serialize)]
pub struct ElementBounds {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

pub async fn list_elements_on_screen(executable: &Path, serial: &str) -> io::Result<UiElementList> {
    let xml = capture_hierarchy(executable, serial).await?;
    parse_hierarchy(serial, &xml)
}

pub async fn get_screen_hierarchy(executable: &Path, serial: &str) -> io::Result<UiHierarchyTree> {
    let xml = capture_hierarchy(executable, serial).await?;
    parse_hierarchy_tree(serial, &xml)
}

async fn capture_hierarchy(executable: &Path, serial: &str) -> io::Result<String> {
    validate_serial(serial)?;
    let deadline = Instant::now() + UI_HIERARCHY_TIMEOUT;
    let lock = hierarchy_capture_lock(serial)?;
    let _guard = timeout_at(deadline, lock.lock())
        .await
        .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, UI_HIERARCHY_TIMEOUT_MESSAGE))?;
    let output = run_shell(
        executable,
        serial,
        [UI_HIERARCHY_SCRIPT],
        "UI hierarchy capture",
        CommandOptions::new(deadline, UI_HIERARCHY_LIMIT, UI_HIERARCHY_STDERR_LIMIT)
            .with_timeout_message(UI_HIERARCHY_TIMEOUT_MESSAGE)
            .with_stdout_label("output"),
    )
    .await?;
    ensure_success(&output, "UI hierarchy capture")?;
    let xml = std::str::from_utf8(&output.stdout).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("ADB UI hierarchy is not valid UTF-8: {error}"),
        )
    })?;
    Ok(xml.to_owned())
}

fn hierarchy_capture_lock(serial: &str) -> io::Result<Arc<AsyncMutex<()>>> {
    let locks = UI_HIERARCHY_LOCKS.get_or_init(|| StdMutex::new(HashMap::new()));
    let mut locks = locks
        .lock()
        .map_err(|_| io::Error::other("UI hierarchy capture lock map is poisoned"))?;
    locks.retain(|_, lock| lock.strong_count() > 0);
    if let Some(lock) = locks.get(serial).and_then(Weak::upgrade) {
        return Ok(lock);
    }

    let lock = Arc::new(AsyncMutex::new(()));
    locks.insert(serial.to_owned(), Arc::downgrade(&lock));
    Ok(lock)
}

fn parse_hierarchy(serial: &str, xml: &str) -> io::Result<UiElementList> {
    let document = Document::parse(xml).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("ADB UI hierarchy XML is malformed: {error}"),
        )
    })?;
    let elements = document
        .descendants()
        .filter(|node| node.has_tag_name("node"))
        .map(parse_element)
        .collect();
    Ok(UiElementList {
        serial: serial.to_owned(),
        elements,
    })
}

fn parse_hierarchy_tree(serial: &str, xml: &str) -> io::Result<UiHierarchyTree> {
    let document = Document::parse(xml).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("ADB UI hierarchy XML is malformed: {error}"),
        )
    })?;
    let roots = document
        .root_element()
        .children()
        .filter(|node| node.has_tag_name("node"))
        .map(parse_tree_node)
        .collect();
    Ok(UiHierarchyTree {
        serial: serial.to_owned(),
        roots,
    })
}

fn parse_tree_node(node: Node<'_, '_>) -> UiHierarchyNode {
    let children = node
        .children()
        .filter(|child| child.has_tag_name("node"))
        .map(parse_tree_node)
        .collect();
    UiHierarchyNode {
        element: parse_element(node),
        children,
    }
}

fn parse_element(node: Node<'_, '_>) -> UiElement {
    let text = |name| {
        node.attribute(name)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
    };
    let boolean = |name| node.attribute(name).and_then(|value| value.parse().ok());

    UiElement {
        class: text("class"),
        text: text("text"),
        resource_id: text("resource-id"),
        content_desc: text("content-desc"),
        package: text("package"),
        bounds: node.attribute("bounds").and_then(parse_bounds),
        clickable: boolean("clickable"),
        enabled: boolean("enabled"),
        focusable: boolean("focusable"),
        focused: boolean("focused"),
        scrollable: boolean("scrollable"),
        long_clickable: boolean("long-clickable"),
        checkable: boolean("checkable"),
        checked: boolean("checked"),
        selected: boolean("selected"),
    }
}

fn parse_bounds(value: &str) -> Option<ElementBounds> {
    let value = value.strip_prefix('[')?;
    let (left_top, right_bottom) = value.split_once("][")?;
    let right_bottom = right_bottom.strip_suffix(']')?;
    let (left, top) = left_top.split_once(',')?;
    let (right, bottom) = right_bottom.split_once(',')?;
    Some(ElementBounds {
        left: left.parse().ok()?,
        top: top.parse().ok()?,
        right: right.parse().ok()?,
        bottom: bottom.parse().ok()?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_nested_nodes_and_android_bounds() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<hierarchy rotation="0"><node index="0" class="android.widget.FrameLayout" bounds="[0,0][1080,2400]" enabled="true"><node index="1" text="Continue" resource-id="app:id/continue" content-desc="Continue" package="app" class="android.widget.Button" bounds="[20,30][220,90]" clickable="true" enabled="true" selected="false"/></node></hierarchy>"#;
        let result = parse_hierarchy("emulator-5554", xml).unwrap();
        assert_eq!(result.serial, "emulator-5554");
        assert_eq!(result.elements.len(), 2);
        assert_eq!(result.elements[1].text.as_deref(), Some("Continue"));
        assert_eq!(
            result.elements[1].resource_id.as_deref(),
            Some("app:id/continue")
        );
        assert_eq!(result.elements[1].bounds.as_ref().unwrap().left, 20);
        assert_eq!(result.elements[1].bounds.as_ref().unwrap().bottom, 90);
        assert_eq!(result.elements[1].clickable, Some(true));
    }

    #[test]
    fn keeps_missing_and_malformed_bounds_unknown() {
        let result = parse_hierarchy(
            "serial",
            "<hierarchy><node class=\"A\"/><node bounds=\"invalid\"/></hierarchy>",
        )
        .unwrap();
        assert_eq!(result.elements.len(), 2);
        assert!(result.elements[0].bounds.is_none());
        assert!(result.elements[1].bounds.is_none());
        assert!(parse_hierarchy("serial", "not xml").is_err());
    }

    #[test]
    fn preserves_parent_child_relationships() {
        let xml = "<hierarchy><node class=\"parent\"><node class=\"child\"><node class=\"grandchild\"/></node></node><node class=\"second-root\"/></hierarchy>";
        let tree = parse_hierarchy_tree("serial", xml).unwrap();
        assert_eq!(tree.roots.len(), 2);
        assert_eq!(tree.roots[0].element.class.as_deref(), Some("parent"));
        assert_eq!(tree.roots[0].children.len(), 1);
        assert_eq!(
            tree.roots[0].children[0].children[0]
                .element
                .class
                .as_deref(),
            Some("grandchild")
        );
        assert!(tree.roots[1].children.is_empty());
    }
}
