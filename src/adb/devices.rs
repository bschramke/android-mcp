use std::{io, path::Path, time::Duration};

use tokio::time::Instant;

use super::{
    command::{CommandOptions, ensure_success, run_adb},
    model::{AndroidDevice, DeviceList, DeviceStatus},
};

pub async fn list_devices(executable: &Path) -> io::Result<DeviceList> {
    let output = run_adb(
        executable,
        ["devices", "-l"],
        "device listing",
        CommandOptions::new(
            Instant::now() + Duration::from_secs(5),
            64 * 1024,
            16 * 1024,
        ),
    )
    .await?;
    ensure_success(&output, "device listing")?;
    let stdout = std::str::from_utf8(&output.stdout).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("ADB device listing is not valid UTF-8: {error}"),
        )
    })?;
    parse_devices(stdout)
}

fn parse_devices(output: &str) -> io::Result<DeviceList> {
    let mut lines = output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty());
    if lines.next() != Some("List of devices attached") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "ADB device listing is missing its header",
        ));
    }

    let mut devices = Vec::new();
    for (index, line) in lines.enumerate() {
        let fields: Vec<_> = line.split_whitespace().collect();
        if fields.len() < 2 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "Malformed ADB device entry {}: expected serial and state",
                    index + 1
                ),
            ));
        }

        let no_permissions = fields.get(1..3) == Some(&["no", "permissions"][..]);
        let adb_state = if no_permissions {
            "no permissions"
        } else {
            fields[1]
        };
        let mut device = AndroidDevice {
            serial: fields[0].to_owned(),
            status: DeviceStatus::from_adb_state(adb_state),
            adb_state: adb_state.to_owned(),
            model: None,
            product: None,
            device: None,
            transport_id: None,
        };
        for field in &fields[if no_permissions { 3 } else { 2 }..] {
            if let Some((key, value)) = field.split_once(':') {
                match key {
                    "model" => device.model = Some(value.to_owned()),
                    "product" => device.product = Some(value.to_owned()),
                    "device" => device.device = Some(value.to_owned()),
                    "transport_id" => device.transport_id = Some(value.to_owned()),
                    _ => {}
                }
            }
        }
        devices.push(device);
    }
    Ok(DeviceList { devices })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_empty_device_list() {
        assert_eq!(
            parse_devices("List of devices attached\n\n")
                .unwrap()
                .devices,
            vec![]
        );
    }

    #[test]
    fn parses_devices_and_optional_metadata() {
        let result = parse_devices("List of devices attached\r\n\r\nemulator-5554 device product:sdk model:Pixel_8 device:emu transport_id:1 extra:ignored\r\nphone unauthorized\r\nother offline transport_id:2\r\n").unwrap();
        assert_eq!(result.devices.len(), 3);
        assert_eq!(
            result.devices[0],
            AndroidDevice {
                serial: "emulator-5554".into(),
                status: DeviceStatus::Online,
                adb_state: "device".into(),
                model: Some("Pixel_8".into()),
                product: Some("sdk".into()),
                device: Some("emu".into()),
                transport_id: Some("1".into()),
            }
        );
        assert_eq!(result.devices[1].status, DeviceStatus::Unauthorized);
        assert_eq!(result.devices[1].adb_state, "unauthorized");
        assert_eq!(result.devices[1].model, None);
        assert_eq!(result.devices[2].status, DeviceStatus::Offline);
        assert_eq!(result.devices[2].adb_state, "offline");
    }

    #[test]
    fn preserves_no_permissions_and_unknown_states() {
        let result = parse_devices("List of devices attached\nusb no permissions (user is not in the plugdev group); see [https://developer.android.com/tools/device.html] transport_id:3\nfuture new_state\n").unwrap();
        assert_eq!(result.devices[0].status, DeviceStatus::PermissionDenied);
        assert_eq!(result.devices[0].adb_state, "no permissions");
        assert_eq!(result.devices[0].transport_id.as_deref(), Some("3"));
        assert_eq!(result.devices[1].status, DeviceStatus::Unknown);
        assert_eq!(result.devices[1].adb_state, "new_state");
    }

    #[test]
    fn serializes_friendly_status_and_original_adb_state_without_legacy_field() {
        let result = parse_devices("List of devices attached\nonline device\noffline offline\nphone unauthorized\nusb no permissions\nfuture new_state\n").unwrap();
        let json = serde_json::to_value(result).unwrap();
        let devices = json["devices"].as_array().unwrap();
        for (device, (status, adb_state)) in devices.iter().zip([
            ("online", "device"),
            ("offline", "offline"),
            ("unauthorized", "unauthorized"),
            ("permission_denied", "no permissions"),
            ("unknown", "new_state"),
        ]) {
            assert_eq!(device["status"], status);
            assert_eq!(device["adb_state"], adb_state);
            assert!(device.get("state").is_none());
        }
    }

    #[test]
    fn rejects_missing_header() {
        assert!(parse_devices("").is_err());
        assert!(parse_devices("unexpected output").is_err());
    }

    #[test]
    fn rejects_incomplete_device_entry() {
        assert!(parse_devices("List of devices attached\nserial_only\n").is_err());
    }
}
