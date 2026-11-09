use std::{collections::BTreeMap, io, path::Path, time::Duration};

use tokio::time::Instant;

use super::{
    command::{CommandOptions, ensure_success, run_shell},
    model::{AndroidInfo, BuildInfo, CpuInfo, DeviceInfo, HardwareInfo, ProductInfo},
    screen::display_info_until,
};

pub async fn device_info(executable: &Path, serial: &str) -> io::Result<DeviceInfo> {
    validate_serial(serial)?;

    let deadline = Instant::now() + Duration::from_secs(5);
    let output = run_shell(
        executable,
        serial,
        ["getprop"],
        "device info",
        CommandOptions::new(deadline, 256 * 1024, 16 * 1024),
    )
    .await?;
    ensure_success(&output, "device info")?;
    let stdout = std::str::from_utf8(&output.stdout).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("ADB getprop output is not valid UTF-8: {error}"),
        )
    })?;
    let mut info = parse_device_info(serial, stdout)?;
    info.display = display_info_until(executable, serial, deadline)
        .await
        .unwrap_or_default();
    Ok(info)
}

pub(super) fn validate_serial(serial: &str) -> io::Result<()> {
    if serial.is_empty()
        || serial
            .chars()
            .any(|character| character.is_whitespace() || character == '\0')
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Device serial must be nonempty and contain no whitespace or NUL characters",
        ));
    }
    Ok(())
}

fn parse_device_info(serial: &str, output: &str) -> io::Result<DeviceInfo> {
    let mut properties = BTreeMap::new();
    let mut entries: Vec<(usize, String)> = Vec::new();
    for (index, line) in output.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with('[') && line.contains("]: [") {
            entries.push((index + 1, line.to_owned()));
        } else if let Some((_, entry)) = entries.last_mut() {
            // Some getprop values span lines; attach them until the next property header.
            entry.push('\n');
            entry.push_str(line);
        } else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("Malformed ADB getprop entry on line {}", index + 1),
            ));
        }
    }
    for (line_number, entry) in entries {
        let line = entry.as_str();
        let (key, value) = line
            .strip_prefix('[')
            .and_then(|line| line.split_once("]: ["))
            .and_then(|(key, value)| value.strip_suffix(']').map(|value| (key, value)))
            .filter(|(key, _)| !key.is_empty())
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("Malformed ADB getprop entry on line {line_number}"),
                )
            })?;
        properties.insert(key.to_owned(), value.trim().to_owned());
    }
    if properties.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "ADB getprop output contains no properties",
        ));
    }

    let text = |key: &str| {
        properties
            .get(key)
            .filter(|value| !value.is_empty())
            .cloned()
    };
    let list = |key: &str| -> Vec<String> {
        text(key)
            .map(|value| {
                value
                    .split(',')
                    .map(str::trim)
                    .filter(|item| !item.is_empty())
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default()
    };
    let number = |key: &str| -> io::Result<Option<u32>> {
        text(key)
            .map(|value| {
                value.parse::<u32>().map_err(|_| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("ADB property {key} is not a valid unsigned integer"),
                    )
                })
            })
            .transpose()
    };
    let boolean = |key: &str| match text(key).as_deref() {
        Some("1") => Some(true),
        Some("0") => Some(false),
        _ => None,
    };

    Ok(DeviceInfo {
        serial: serial.to_owned(),
        product: ProductInfo {
            brand: text("ro.product.brand"),
            manufacturer: text("ro.product.manufacturer"),
            model: text("ro.product.model"),
            device: text("ro.product.device"),
            name: text("ro.product.name"),
            locale: text("ro.product.locale"),
            first_api_level: number("ro.product.first_api_level")?,
        },
        hardware: HardwareInfo {
            board: text("ro.product.board"),
            hardware: text("ro.hardware"),
            soc_manufacturer: text("ro.soc.manufacturer"),
            soc_model: text("ro.soc.model"),
            cpu: CpuInfo {
                abi: text("ro.product.cpu.abi"),
                abilist: list("ro.product.cpu.abilist"),
                abilist32: list("ro.product.cpu.abilist32"),
                abilist64: list("ro.product.cpu.abilist64"),
            },
        },
        android: AndroidInfo {
            release: text("ro.build.version.release"),
            api_level: number("ro.build.version.sdk")?,
            security_patch: text("ro.build.version.security_patch"),
        },
        build: BuildInfo {
            id: text("ro.build.id"),
            fingerprint: text("ro.build.fingerprint"),
            incremental: text("ro.build.version.incremental"),
            build_type: text("ro.build.type"),
            tags: text("ro.build.tags"),
        },
        boot_completed: boolean("sys.boot_completed"),
        is_emulator: boolean("ro.kernel.qemu"),
        display: Default::default(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn supports_multiline_property_values_without_exposing_unrelated_data() {
        let info = parse_device_info("serial", "[persist.sys.boot.reason.history]: [reboot,123\nreboot,122\nreboot,factory_reset,121]\n[ro.product.model]: [Example Phone]\n[ro.build.fingerprint]: [example/[value]/fingerprint]\n").unwrap();
        assert_eq!(info.product.model.as_deref(), Some("Example Phone"));
        assert_eq!(
            info.build.fingerprint.as_deref(),
            Some("example/[value]/fingerprint")
        );
        assert!(!serde_json::to_string(&info).unwrap().contains("reboot"));
        assert!(
            parse_device_info(
                "serial",
                "[broken]: [unclosed\n[ro.product.model]: [Phone]\n"
            )
            .is_err()
        );
    }

    #[test]
    fn maps_product_hardware_cpu_android_and_build_properties() {
        let output =
            include_str!("../../tests/fixtures/getprop/synthetic.txt").replace('\n', "\r\n");
        let info = parse_device_info("selected-device", &output).unwrap();
        assert_eq!(info.serial, "selected-device");
        assert_eq!(
            info.product,
            ProductInfo {
                brand: Some("example".into()),
                manufacturer: Some("Example Inc".into()),
                model: Some("Example Phone".into()),
                device: Some("example_device".into()),
                name: Some("example_product".into()),
                locale: Some("en-US".into()),
                first_api_level: Some(30),
            }
        );
        assert_eq!(
            info.hardware,
            HardwareInfo {
                board: Some("board".into()),
                hardware: Some("hardware".into()),
                soc_manufacturer: Some("soc-vendor".into()),
                soc_model: Some("soc-model".into()),
                cpu: CpuInfo {
                    abi: Some("arm64-v8a".into()),
                    abilist: vec!["arm64-v8a".into(), "armeabi-v7a".into()],
                    abilist32: vec!["armeabi-v7a".into()],
                    abilist64: vec!["arm64-v8a".into()]
                },
            }
        );
        assert_eq!(
            info.android,
            AndroidInfo {
                release: Some("15".into()),
                api_level: Some(35),
                security_patch: Some("2026-10-05".into())
            }
        );
        assert_eq!(
            info.build,
            BuildInfo {
                id: Some("BUILD.ID".into()),
                fingerprint: Some("brand/product/device:15/BUILD.ID/123:user/release-keys".into()),
                incremental: Some("123".into()),
                build_type: Some("user".into()),
                tags: Some("release-keys".into())
            }
        );
        assert_eq!(info.boot_completed, Some(true));
        assert_eq!(info.is_emulator, Some(true));
        assert!(
            !serde_json::to_string(&info)
                .unwrap()
                .contains("never expose this")
        );
    }

    #[test]
    fn parses_anonymized_tc57_dump() {
        let info = parse_device_info(
            "fixture-tc57",
            include_str!("../../tests/fixtures/getprop/tc57.txt"),
        )
        .unwrap();
        assert_eq!(info.serial, "fixture-tc57");
        assert_eq!(info.product.model.as_deref(), Some("TC57"));
        assert_eq!(
            info.product.manufacturer.as_deref(),
            Some("Zebra Technologies")
        );
        assert_eq!(info.product.first_api_level, Some(27));
        assert_eq!(info.hardware.board.as_deref(), Some("sdm660"));
        assert_eq!(info.hardware.hardware.as_deref(), Some("qcom"));
        assert_eq!(info.hardware.cpu.abi.as_deref(), Some("arm64-v8a"));
        assert_eq!(
            info.hardware.cpu.abilist,
            ["arm64-v8a", "armeabi-v7a", "armeabi"]
        );
        assert_eq!(info.hardware.cpu.abilist32, ["armeabi-v7a", "armeabi"]);
        assert_eq!(info.hardware.cpu.abilist64, ["arm64-v8a"]);
        assert_eq!(info.android.release.as_deref(), Some("8.1.0"));
        assert_eq!(info.android.api_level, Some(27));
        assert_eq!(info.android.security_patch.as_deref(), Some("2021-10-05"));
        assert_eq!(info.build.id.as_deref(), Some("TEST.BUILD"));
        assert_eq!(info.boot_completed, Some(true));
        assert_eq!(info.is_emulator, None);
        assert!(!serde_json::to_string(&info).unwrap().contains("redacted"));
    }

    #[test]
    fn parses_anonymized_emulator_dump() {
        let info = parse_device_info(
            "fixture-emulator",
            include_str!("../../tests/fixtures/getprop/emulator.txt"),
        )
        .unwrap();
        assert_eq!(info.serial, "fixture-emulator");
        assert_eq!(
            info.product.model.as_deref(),
            Some("Android SDK built for arm64")
        );
        assert_eq!(info.product.manufacturer.as_deref(), Some("Google"));
        assert_eq!(info.product.first_api_level, None);
        assert_eq!(info.hardware.board, None);
        assert_eq!(info.hardware.hardware.as_deref(), Some("ranchu"));
        assert_eq!(info.hardware.cpu.abi.as_deref(), Some("arm64-v8a"));
        assert_eq!(info.hardware.cpu.abilist, ["arm64-v8a"]);
        assert!(info.hardware.cpu.abilist32.is_empty());
        assert_eq!(info.hardware.cpu.abilist64, ["arm64-v8a"]);
        assert_eq!(info.android.release.as_deref(), Some("8.1.0"));
        assert_eq!(info.android.api_level, Some(27));
        assert_eq!(info.android.security_patch.as_deref(), Some("2018-01-05"));
        assert_eq!(info.build.id.as_deref(), Some("TEST.BUILD"));
        assert_eq!(info.boot_completed, Some(true));
        assert_eq!(info.is_emulator, Some(true));
        assert!(!serde_json::to_string(&info).unwrap().contains("redacted"));
    }

    #[test]
    fn missing_and_empty_values_remain_unknown() {
        let info = parse_device_info(
            "serial",
            "[ro.product.model]: []\n[ro.product.cpu.abilist]: [, ,]\n",
        )
        .unwrap();
        assert_eq!(info.product.model, None);
        assert_eq!(info.product.first_api_level, None);
        assert_eq!(info.hardware.cpu.abi, None);
        assert!(info.hardware.cpu.abilist.is_empty());
        assert!(info.hardware.cpu.abilist32.is_empty());
        assert!(info.hardware.cpu.abilist64.is_empty());
        assert_eq!(info.android.api_level, None);
        assert_eq!(info.boot_completed, None);
        assert_eq!(info.is_emulator, None);
    }

    #[test]
    fn parses_false_and_unknown_flags_without_guessing() {
        let info = parse_device_info(
            "serial",
            "[sys.boot_completed]: [0]\n[ro.kernel.qemu]: [0]\n",
        )
        .unwrap();
        assert_eq!(info.boot_completed, Some(false));
        assert_eq!(info.is_emulator, Some(false));
        let unknown = parse_device_info(
            "serial",
            "[sys.boot_completed]: [unknown]\n[ro.kernel.qemu]: []\n",
        )
        .unwrap();
        assert_eq!(unknown.boot_completed, None);
        assert_eq!(unknown.is_emulator, None);
    }

    #[test]
    fn rejects_empty_malformed_and_invalid_numeric_output() {
        for output in [
            "",
            "invalid",
            "[key]: missing brackets",
            "[]: [value]",
            "[ro.build.version.sdk]: [invalid]",
            "[ro.product.first_api_level]: [-1]",
        ] {
            assert!(
                parse_device_info("serial", output).is_err(),
                "Accepted {output:?}"
            );
        }
    }
}
