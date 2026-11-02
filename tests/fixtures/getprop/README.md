# getprop Test Fixtures

`tc57.txt` and `emulator.txt` are anonymized snapshots derived from user-provided
`adb shell getprop` output on 2026-11-02. They retain their property inventories,
order, empty values, and multiline value structure, but are not raw device dumps.

| File | Profile | Properties |
| --- | --- | --- |
| `tc57.txt` | Zebra TC57, Android 8.1.0 / API 27, ARM64 with 32-bit ABIs | 764 |
| `emulator.txt` | Google ARM64 emulator, Android 8.1.0 / API 27 | 259 |
| `synthetic.txt` | Synthetic complete mapping contract including SoC fields | 26 |

Only explicitly reviewed device-class properties are retained: product/model,
CPU ABIs, hardware, Android version/security patch, boot flag, and emulator flag
where present. Build ID, fingerprint, incremental version, build type, and tags
are replaced with synthetic values. All other nonempty property values are
replaced with `redacted` on each value line, including serial numbers, SIM/radio
identifiers, MAC addresses, hostnames, UUIDs, timestamps, and unrelated settings.
Empty values stay empty. Property names still identify the device software
families; these fixtures are not intended to hide the publicly documented model.

The TC57 dump has no `ro.kernel.qemu`, so its expected `is_emulator` is `null`,
not an inferred `false`. The emulator has an empty board and no first API level;
those expected values remain `null`.

Parser tests and the Rust ADB stub use `include_str!()` to load the files at
compile time. No device, runtime data-file lookup, or inbox access is needed to
run the tests. Small synthetic strings remain for deliberately malformed inputs
and focused edge cases such as multiline values.

The original device dumps are not included in this repository. 
All files needed to run these tests are committed alongside the tests. 
Review and anonymize any replacement data before adding it. 
Never commit an unreviewed raw device dump.