# android-mcp
A Model Context Protocol (MCP) server that provides tools for use by agents working on Android projects.
It offers simple tools that can be invoked by LLMs to automate actions on Android devices.

The goal is to make it easy for any client supporting MCP to control an Android device using natural language, 
through tools that are both secure and transparent.

## Why
Controlling Android manually requires a long sequence of raw `adb` commands—a process prone to errors. 
This MCP server embeds the necessary knowledge directly into its tools, 
so the agent doesn’t have to relearn it every single time.

### Why Rust?

This project is primarily a learning exercise: an opportunity to refresh my Rust
skills while exploring MCP, from tool definitions and protocol
initialization to communication over stdio.

Rust's strong type system and explicit error handling provide a solid foundation
for building reliable tools that interact with Android devices through ADB.
The server uses [rmcp](https://github.com/modelcontextprotocol/rust-sdk), the
official Rust SDK for the Model Context Protocol. It handles MCP messaging,
tool registration, and transport integration, allowing this project to focus on
Android-specific functionality without reimplementing the protocol.

## Getting started
### ADB prerequisite

Before serving MCP over stdio, the server resolves a working ADB executable by
running its `version` command with a five-second timeout per attempt:

1. If `ANDROID_MCP_ADB` is set, use only that executable. An invalid override
	fails startup without falling back to another installation.
2. Otherwise, try `adb` on `PATH`.
3. If that fails, try `$ANDROID_SDK_ROOT/platform-tools/adb`.
4. Finally, try `$ANDROID_HOME/platform-tools/adb`.

SDK paths use `adb.exe` on Windows. Empty SDK variables are ignored, and identical
SDK candidates are checked only once. Paths containing spaces are supported
without shell interpolation. The resolver returns the successful invocation path
(the command name `adb` for PATH discovery, or the configured executable path)
for reuse by future ADB tools.

If no candidate can be executed successfully within its timeout, startup fails
with the attempted candidates and their errors on stderr and a nonzero exit
code. The check does not start the ADB
daemon or require a connected device. It verifies executable availability, not
device access or future ADB server health. Tool calls must still handle their own
ADB failures. stdout remains reserved for MCP messages.

## 🔧 Available tools

Tools are grouped by purpose. Detailed behavior and arguments follow below.

### Device & Diagnostics
- **`adb_list_devices`** - List connected, offline, and unauthorized devices
- **`adb_device_info`** - Read supported Android properties and display information
- **`adb_display_info`** - Read display resolution, density, rotation, and orientation
- **`adb_health_check`** - Diagnose ADB, device connectivity, and read-only shell access

### App Management
- **`adb_list_apps`** - List all installed apps on the device
- **`adb_app_info`** - Get version, install, and update details for one app
- **`adb_get_foreground_app`** - Get the app currently in the foreground
- **`adb_install_app`** - Install an APK on a selected device
- **`adb_uninstall_app`** - Uninstall an app by package name
- **`adb_clear_app_data`** - Clear an app's data so it starts fresh

### Input & Navigation
- **`adb_tap_on_screen`** - Tap at pixel coordinates, optionally twice
- **`adb_long_press_on_screen`** - Hold a touch at pixel coordinates
- **`adb_swipe_on_screen`** - Swipe between pixel coordinates
- **`adb_input_text`** - Enter printable ASCII text into the focused field
- **`adb_press_key`** - Press a supported Android key

### Logs
- **`adb_logcat`** - Read a bounded snapshot of recent device logs with optional filters

### Screen Inspection & Capture
- **`adb_list_elements_on_screen`** - List visible UI elements and their properties
- **`adb_get_screen_hierarchy`** - Read the nested UI hierarchy
- **`adb_take_screenshot`** - Capture a screenshot for inspection
- **`adb_save_screenshot`** - Save a screenshot as a PNG file

### Screen Recording
- **`adb_start_screen_recording`** - Start recording a device screen to an MP4 file
- **`adb_stop_screen_recording`** - Stop and save the active recording

## Tool details

### Input behavior and limits

All input tools require `serial` and change device/UI state. Coordinates are
nonnegative integer display pixels up to 2147483647; they are not scaled or
checked against the current display dimensions. Use `adb_display_info` to inspect
the display when needed. Durations must be between 1 and 10000 milliseconds.
Each operation shares one 15-second time budget and caps stdout and stderr at
16 KiB each. Failures are MCP tool errors; partially completed gestures or text
cannot be rolled back. The server never retries input automatically.
Success means the input command completed, not that the app handled the gesture
or produced a particular result. Verify effects using the app's visible state.

### adb_tap_on_screen

```json
{"serial": "emulator-5554", "x": 240, "y": 480, "double_tap": true}
```

`x` and `y` are required. `double_tap` defaults to `false` and executes a single
`input tap`. When `true`, a single remote shell invocation runs two identical
`input tap` commands joined with `&&`, without an intentional pause. The second
tap runs only if the first command succeeds. This reduces host/ADB round trips,
but it is not an atomic gesture and does not guarantee double-tap recognition.
App gesture rules, Android timing settings, and device load still apply.

### adb_long_press_on_screen

```json
{"serial": "emulator-5554", "x": 240, "y": 480, "duration_ms": 1000}
```

`x` and `y` are required; `duration_ms` defaults to 1000. The tool uses
`input swipe x y x y duration_ms`, keeping the touch at the same position.
Whether a particular duration triggers a long press depends on the app/device.

### adb_swipe_on_screen

```json
{"serial": "emulator-5554", "start_x": 240, "start_y": 800, "end_x": 240, "end_y": 200, "duration_ms": 300}
```

Both start and end coordinates are required; `duration_ms` defaults to 300.
The tool executes `input swipe start_x start_y end_x end_y duration_ms`.

### adb_input_text

```json
{"serial": "emulator-5554", "text": "Hello world"}
```

`text` must contain 1-1024 printable ASCII characters (U+0020-U+007E).
Unicode, tabs, newlines, and other control characters are rejected. Text targets
the currently focused input field; this tool does not focus a field or clear it.
Spaces are encoded as Android's `%s` sequence. Shell metacharacters, including
quotes, are safely quoted for the device shell. Literal `%s` is entered with
separate commands to prevent Android interpreting it as a space. All commands
are executed in order under one shared deadline, stopping on command failure.
Text is not persisted or logged by the server, but is passed in process arguments
and may contain sensitive data. Device key-character-map limitations still apply;
a successful command does not guarantee the text was accepted by the app.

### adb_press_key

```json
{"serial": "emulator-5554", "keycode": "BACK"}
```

`keycode` is a case-sensitive enum: `HOME`, `BACK`, `ENTER`, `TAB`, `DEL`,
`FORWARD_DEL`, `DPAD_UP`, `DPAD_DOWN`, `DPAD_LEFT`, `DPAD_RIGHT`, `DPAD_CENTER`,
`VOLUME_UP`, or `VOLUME_DOWN`. It maps to the corresponding Android `KEYCODE_*`
constant and executes `input keyevent`. Numeric codes and arbitrary flags are
not accepted.

### adb_list_devices

`adb_list_devices` takes no arguments and runs `adb devices -l` using the
executable resolved during startup. The call has a five-second timeout and may
start the local ADB daemon. It does not require selecting or authorizing a device.

The result is a JSON text block containing a `devices` array. Each entry has
`serial`, `status`, `adb_state`, and nullable `model`, `product`, `device`, and
`transport_id` fields. `status` provides a normalized value while `adb_state`
preserves the original ADB state:

| `adb_state` | `status` |
| --- | --- |
| `device` | `online` |
| `offline` | `offline` |
| `unauthorized` | `unauthorized` |
| `no permissions` | `permission_denied` |
| Any other value | `unknown` |

`online` means ADB can communicate with the device, not that Android has finished
booting. Offline, unauthorized, and no-permissions entries remain in the result.
Unknown ADB states are preserved in `adb_state`; unknown metadata fields are
ignored. This replaces the earlier `state` field; clients consuming that field
must migrate to `status` and/or `adb_state`.

No connected devices is a successful `{"devices":[]}` result. Failed processes,
timeouts, malformed output, and invalid UTF-8 produce MCP tool results with
`isError: true`, without terminating the MCP connection. ADB stdout and stderr
are captured, never forwarded directly to the MCP protocol stream. Device serial
numbers and metadata are returned to the calling MCP client.

### adb_list_apps

Provide a device serial number as a required string argument:

```json
{"serial": "emulator-5554"}
```

The tool runs `adb -s <serial> shell pm list packages` and returns a JSON text
object containing the serial and a sorted `apps` array of package names. The
list includes packages reported by Android's package manager, including
preinstalled/system packages; it does not resolve human-readable app labels.
The optional `exclude_system` boolean defaults to `false`; when set to `true`,
the tool runs `pm list packages -3` and excludes system packages. An empty result
is returned as an empty array. ADB failures, malformed output, invalid UTF-8,
and the five-second timeout produce tool errors.

### adb_app_info

Provide a device serial and an Android package name:

```json
{"serial": "emulator-5554", "package": "com.example.app"}
```

The tool runs `adb -s <serial> shell dumpsys package <package>` and returns
structured package metadata parsed from that dump: version code and name, min
and target SDK, first-install and last-update timestamps, installer package name,
and APK code path. Timestamps are returned as reported by Android; their format
may vary by Android version. Missing fields are `null`. The raw package dump is
not returned. ADB failures, missing packages, malformed output, invalid UTF-8,
and the five-second timeout produce tool errors.

### adb_install_app

Provide a device serial and an absolute APK path on the MCP server host:

```json
{"serial": "emulator-5554", "apk_path": "/absolute/path/to/app.apk"}
```

The optional `replace` and `downgrade` booleans default to `false`. `replace: true`
passes `-r` to ADB and preserves app data. `downgrade: true` implies replacement
and passes both `-r` and `-d`; Android may reject downgrades that it does not
allow. Without replacement, installing an already-installed package returns an
ADB error. The APK must exist on the MCP server host, use the `.apk` extension,
and be accessible to the server process. Installation changes device state.

### adb_uninstall_app

Provide a device serial and Android application ID (also commonly called a
package name or bundle ID):

```json
{"serial": "emulator-5554", "package": "com.example.app"}
```

The tool runs `adb -s <serial> uninstall <package>`. It validates the package
name and returns an error if ADB cannot uninstall it, including when it is not
installed or the device rejects removal. Uninstallation changes device state.

### adb_clear_app_data

Provide a device serial and Android application ID:

```json
{"serial": "emulator-5554", "package": "com.example.app"}
```

The tool runs `adb -s <serial> shell pm clear <package>` and succeeds only when
Android reports `Success`. This deletes the app's local data and resets its
state; it does not uninstall the app. ADB failures, invalid package names, an
unsuccessful package-manager response, and the 30-second timeout produce tool
errors. This operation is destructive and cannot be undone.

### adb_get_foreground_app

Provide a device serial number as a required string argument:

```json
{"serial": "emulator-5554"}
```

The tool runs `adb -s <serial> shell dumpsys activity activities` and returns
the serial plus the resumed app's package and activity component. It supports
the `mResumedActivity`, `ResumedActivity`, and `topResumedActivity` markers. If
Android reports no resumed activity, `package` and `activity` are `null`.
Malformed activity records, ADB failures, invalid UTF-8, and the five-second
timeout produce tool errors.

### adb_health_check

Run the ADB binary, server, and visible-device checks. An optional serial enables
device-specific connection, authorization, and read-only shell checks:

```json
{}
```

```json
{"serial": "emulator-5554"}
```

The tool runs `adb version` and `adb devices -l`. The latter may start the local
ADB daemon. Without a serial, device-specific checks are marked `skipped`; the
visible device list is still included in the report. With a serial, the report
checks its ADB state and, only when online, runs `adb -s <serial> shell id -u`.
This verifies shell access and reports the observed `shell_uid` and
`shell_is_root` values. It never runs `adb root` or changes device state.

The JSON result contains an overall `status` (`healthy` or `unhealthy`),
individual `checks` (`passed`, `failed`, or `skipped`), the visible `devices`,
and nullable shell identity fields. Expected diagnostic failures are returned
in the report rather than as MCP tool errors; skipped checks do not make the
overall status unhealthy. Each ADB command has a five-second timeout. The tool
exposes device serials and ADB metadata to the calling MCP client.

### adb_device_info

Provide a device serial number as a required string argument:

```json
{"serial": "emulator-5554"}
```

The tool runs `adb -s <serial> shell getprop` using the resolved ADB executable,
without shell interpolation. It then reads display data using `wm size`,
`wm density`, and `dumpsys window displays`. The serial is passed as one literal
argument and must not be empty or contain whitespace or NUL characters. All
commands share one five-second timeout.

The JSON text result preserves the nested `DeviceInfo` structure:

| Result group | Fields and Android properties |
| --- | --- |
| `serial` | The requested device serial number |
| `product` | `brand`, `manufacturer`, `model`, `device`, `name`, `locale`, `first_api_level` from the corresponding `ro.product.*` properties |
| `hardware` | `board` from `ro.product.board`, `hardware` from `ro.hardware`, and `soc_manufacturer`/`soc_model` from `ro.soc.manufacturer`/`ro.soc.model` |
| `hardware.cpu` | `abi`, `abilist`, `abilist32`, `abilist64` from `ro.product.cpu.*`; ABI lists are split on commas |
| `android` | `release`, `api_level`, `security_patch` from `ro.build.version.release`, `ro.build.version.sdk`, `ro.build.version.security_patch` |
| `build` | `id`, `fingerprint`, `incremental`, `build_type`, `tags` from `ro.build.id`, `ro.build.fingerprint`, `ro.build.version.incremental`, `ro.build.type`, `ro.build.tags` |
| `boot_completed` | `sys.boot_completed`: `1` becomes `true`, `0` becomes `false` |
| `is_emulator` | `ro.kernel.qemu`: `1` becomes `true`, `0` becomes `false` |
| `display` | Physical, overridden, and current pixel resolution; effective density in dpi; derived size in dp; rotation and orientation from `wm size`, `wm density`, and `dumpsys window displays` |

Missing or empty scalar properties are `null`; missing ABI lists are `[]`.
Unknown boolean values remain `null`, so a missing emulator flag is not treated
as proof of a physical device. Invalid nonempty API-level numbers, malformed or
empty property output, invalid UTF-8, unavailable/offline/unauthorized devices,
and command failures/timeouts produce tool errors. The MCP connection remains
available after a tool error.

Only the documented properties and parsed display fields are returned, never
the complete property dump or raw `dumpsys` output. Display query failures leave
the display fields unknown without discarding the other device information.
This tool does not add fields or extra device queries to `adb_list_devices`.

### adb_display_info

Provide a device serial number as a required string argument. This tool returns
the same `DisplayInfo` object embedded in `adb_device_info`, without querying
the other device properties:

```json
{"serial": "emulator-5554"}
```

It uses `wm size` for physical and override resolution, `wm density` for
physical and override density, and `dumpsys window displays` for the current
rotation. `current_resolution_px` applies any override and the current rotation;
`size_dp` is derived from that resolution and the effective density. Orientation
accounts for the device's natural display orientation. Unknown output formats
produce `null` fields. ADB command failures, output-limit violations, and the
shared five-second timeout produce tool errors.

### adb_logcat

Read a bounded snapshot of recent device logs:

```json
{"serial": "emulator-5554", "max_lines": 200}
```

`serial` is required. `max_lines` is optional, defaults to 200, and must be an
integer between 1 and 2000. The tool uses the resolved ADB executable to run
`adb -s <serial> logcat -d -t <max_lines> -v threadtime`, without a shell or
user-supplied flags. It is not a continuous stream and never clears the log buffer.

Optional filters can be used individually or combined:

```json
{
	"serial": "emulator-5556",
	"max_lines": 200,
	"pid": 2210,
	"tags": ["ErrorProcessor", "AudioController"],
	"priority": "warn"
}
```

| Parameter | Meaning |
| --- | --- |
| `pid` | Process ID from 1 to 2147483647; passed as `--pid=<pid>` |
| `package` | Android application ID; resolve its running main-process PID for this snapshot; mutually exclusive with `pid` |
| `tags` | Exact tag names combined with OR; omitted or `[]` means no tag restriction |
| `priority` | Minimum level: `verbose`, `debug`, `info`, `warn`, `error`, or `fatal` |

PID, tag selection, and minimum priority are combined with AND. For example,
`warn` includes warnings, errors, and fatal entries. The example produces these
additional native logcat arguments: `--pid=2210`, `ErrorProcessor:W`,
`AudioController:W`, and `*:S`. With tags but no priority, each tag uses `:V` to
include all levels; `*:S` suppresses other tags. With priority but no tags, the
filter is `*:<level>`, applying the minimum priority to every tag.

At most 20 tags are accepted, each containing 1-128 ASCII letters/digits or
`._/+-@`, without a leading `-`. Whitespace, wildcards, colons, shell
metacharacters, and arbitrary filter expressions are rejected. Tag matching is
case-sensitive. `pid`, `package`, and `priority` may also be `null` to leave them unset;
`tags` must be an array when supplied.

To select an app's running main process instead of supplying its PID:

```json
{
	"serial": "emulator-5554",
	"package": "com.github.bschramke.myfinance",
	"priority": "warn"
}
```

Before each snapshot, the server runs
`adb -s <serial> shell pidof <package>` and passes the resulting PID to logcat as
`--pid=<pid>`. The package name is passed as a literal argument and must contain
at least two dot-separated ASCII segments, each starting with a letter and
containing only letters, digits, or underscores, with a total length of at most
255 characters. `package` and `pid` cannot both be set. Package selection can be
combined with tags and minimum priority in the same way as an explicit PID.

PID lookup must return exactly one positive process ID. A non-running app,
multiple PIDs, malformed output, unavailable device, or unsupported `pidof`
produces a tool error; the server never falls back to unfiltered logs. Lookup
stdout is capped at 4 KiB and stderr at 16 KiB while reading. These checks do not
start the app. No PID is cached between requests.

Package filtering covers only the currently running main process with that exact
name, not additional processes such as `<package>:worker` or logs from previous
process IDs. An app can restart between lookup and capture, so the snapshot is
not an atomic guarantee of package identity across process restarts.

Filtering is performed by device logcat, not by parsing the returned text in the
server. No filters preserves the previous command and device-default filtering.
Filters can yield fewer than the requested number of entries or an empty result;
`max_lines` does not promise that many matching entries, and stack traces can
span multiple text lines. Devices that do not support native PID filtering return
a tool error rather than silently ignoring the filter. All existing timeout,
output-size, and privacy protections apply unchanged.

The JSON text result contains `serial`, `format` (`threadtime`), `output`, and
`truncated`. Empty logs are a successful result with `output: ""`. The collection
and process wait, including package PID lookup when requested, share one
five-second timeout; failure cleanup is bounded to one
additional second. stdout and stderr are read concurrently and bounded while
reading, not collected without limits and shortened afterward.

Output is capped at 256 KiB. If exceeded, the ADB process is terminated and the
captured prefix is returned with `truncated: true`; the final line may be partial.
An incomplete UTF-8 character at the byte boundary is omitted. Invalid UTF-8
elsewhere produces a tool error. stderr is capped at 16 KiB; exceeding that limit
produces a tool error. Command failures and timeouts also produce tool errors,
while the MCP server remains available.

Logcat can contain personal data, credentials, or tokens. Logs are returned to
the calling client without automatic redaction and may be sent to its configured
model provider. The server does not additionally log or persist the captured
output. Review the requested device and tool approval before retrieving logs.

#### adb_take_screenshot

Provide a device serial number as a required string argument:

```json
{"serial": "emulator-5554"}
```

The tool runs `adb -s <serial> exec-out screencap -p` and returns the PNG as an
`image/png` MCP image content block. It uses a five-second timeout and rejects
invalid PNG output or captures larger than 16 MiB. Screenshots are returned to
the calling client and are not persisted by this tool. Screen contents may be
sensitive.

### adb_save_screenshot

Provide a serial and an absolute `.png` destination path:

```json
{"serial": "emulator-5554", "path": "/absolute/path/to/screenshot.png"}
```

The optional `overwrite` parameter defaults to `false`:

```json
{
	"serial": "emulator-5554",
	"path": "/absolute/path/to/screenshot.png",
	"overwrite": true
}
```

Without `overwrite: true`, an existing file is left untouched and the tool
returns an error. With it, the existing file is replaced. The tool captures and
validates the PNG before opening the destination, then returns the saved path
and byte size. The same five-second timeout and 16-MiB PNG limit apply. Saved
screenshots persist at the specified path and may contain sensitive screen
contents.

### adb_list_elements_on_screen

Provide a device serial number as a required string argument:

```json
{"serial": "emulator-5554"}
```

The tool captures the Android UI Automator hierarchy and returns a JSON object
with the serial and a flat preorder list of nodes. Each element includes any
available `class`, `text`, `resource_id`, `content_desc`, `package`,
`bounds` (`left`, `top`, `right`, `bottom`), and interaction flags such as
`clickable`, `enabled`, `focused`, `scrollable`, `long_clickable`, `checkable`,
`checked`, and `selected`. Missing attributes are omitted; unrecognized bounds
remain `null`.

The hierarchy is written to a unique temporary file on the device, read back,
and removed by a shell cleanup trap. The tool does not leave a hierarchy file
behind during normal completion. Capture has a ten-second timeout, and XML
output is capped at 1 MiB. Malformed XML, ADB failures, timeouts, and output
overflows produce tool errors. UI text and labels may contain sensitive content
and are returned to the calling MCP client.

### adb_get_screen_hierarchy

Provide a device serial number as a required string argument. This tool uses the
same UI Automator capture as `adb_list_elements_on_screen`, but returns nodes as
a nested tree: the response contains `serial` and `roots`; each node contains an
`element` with the same properties as the flat-list tool and a `children` array
for its direct child nodes. The UI Automator sibling `index` is intentionally
omitted. The same temporary-file cleanup, ten-second timeout, 1-MiB XML limit,
and sensitive-content considerations apply.

### adb_start_screen_recording

Start a recording for a device and choose its eventual absolute `.mp4` path:

```json
{"serial": "emulator-5554", "path": "/absolute/path/to/recording.mp4"}
```

The optional `overwrite` parameter defaults to `false`; an existing destination
is rejected unless `overwrite: true` is specified. The server allows one active
recording per serial. Android's `screenrecord` runs with its 180-second maximum.
The recording is held as a PTY session in the MCP server and uses a unique
temporary `.mp4` path on the device.

### adb_stop_screen_recording

Stop and save the active recording for a serial:

```json
{"serial": "emulator-5554"}
```

The tool sends Ctrl-C to `screenrecord`, waits for the MP4 to finalize, downloads
it to a staging path, validates its MP4 header, then installs it at the path
provided to `adb_start_screen_recording`. Temporary local and remote files are
cleaned up after successful completion. On normal MCP transport closure or
Ctrl-C, the server also attempts to stop and save every active recording. A hard
process kill or host failure cannot guarantee finalization; Android's
180-second recording limit bounds the device-side process lifetime.

Recordings persist at the selected local path and may contain sensitive screen
content. Review the destination and tool approval before starting a recording.

## Connect to an MCP client

Build the executable from the android-mcp repository:

```sh
cargo build --release
```

Use the absolute path to `target/release/android-mcp` (`android-mcp.exe` on
Windows). The client launches the server over stdio; no port or separately running
server is required. Replace every example path below with a real absolute path.
Set `ANDROID_MCP_ADB` explicitly if the client does not inherit your terminal's ADB
environment. The SDK fallback variables described above are also supported when
passed to the server.

### VS Code

Open the project where you want to use the tools. Run **MCP: Add Server** from the
Command Palette, select **Command (stdio)**, enter the binary path, and name the
server `android-mcp`. Choose the workspace destination offered by your selected
agent harness.

For the VS Code-specific `.vscode/mcp.json` format:

```json
{
	"servers": {
		"android-mcp": {
			"type": "stdio",
			"command": "/absolute/path/to/android-mcp/target/release/android-mcp",
			"env": {
				"ANDROID_MCP_ADB": "/absolute/path/to/android-sdk/platform-tools/adb"
			}
		}
	}
}
```

Start the server with **MCP: List Servers**, review any trust prompts, and enable
its tool in the agent chat's tool selection. Use **Show Output** to inspect startup
diagnostics. If using the portable workspace-root `.mcp.json` format instead, use
`mcpServers` rather than `servers`; do not register the same server in both places.

### IntelliJ IDEA (JetBrains AI Assistant)

These steps configure JetBrains AI Assistant as an MCP client, not IDEA's own MCP
server or the GitHub Copilot plugin.

1. With AI Assistant enabled, open **Settings | Tools | AI Assistant | Model Context Protocol (MCP)**.
2. Add a server, select **STDIO**, and paste:

```json
{
	"mcpServers": {
		"android-mcp": {
			"command": "/absolute/path/to/android-mcp/target/release/android-mcp",
			"args": [],
			"env": {
				"ANDROID_MCP_ADB": "/absolute/path/to/android-sdk/platform-tools/adb"
			}
		}
	}
}
```

3. Choose project or global scope, then apply the settings to start the server.
4. Check its connection status and available tools, then use it from AI Assistant chat.

### GitHub Copilot CLI

Register a local stdio server from your terminal:

```sh
copilot mcp add android-mcp \
	--env ANDROID_MCP_ADB=/absolute/path/to/android-sdk/platform-tools/adb \
	--tools adb_list_devices,adb_list_apps,adb_app_info,adb_get_foreground_app,adb_install_app,adb_uninstall_app,adb_clear_app_data,adb_device_info,adb_display_info,adb_logcat,adb_take_screenshot,adb_save_screenshot,adb_list_elements_on_screen,adb_get_screen_hierarchy,adb_start_screen_recording,adb_stop_screen_recording,adb_tap_on_screen,adb_long_press_on_screen,adb_swipe_on_screen,adb_input_text,adb_press_key \
	-- /absolute/path/to/android-mcp/target/release/android-mcp
```

Quote paths or the complete `KEY=value` argument when they contain spaces.
Alternatively, start `copilot`, enter `/mcp add`, choose **STDIO**, and provide the
binary path, environment variables, and `adb_list_devices,adb_list_apps,adb_app_info,adb_get_foreground_app,adb_install_app,adb_uninstall_app,adb_clear_app_data,adb_device_info,adb_display_info,adb_logcat,adb_take_screenshot,adb_save_screenshot,adb_list_elements_on_screen,adb_get_screen_hierarchy,adb_start_screen_recording,adb_stop_screen_recording,adb_tap_on_screen,adb_long_press_on_screen,adb_swipe_on_screen,adb_input_text,adb_press_key` as the enabled tools.
Use `/mcp show android-mcp` to inspect status and tools.

The default user configuration is `~/.copilot/mcp-config.json` (or
`$COPILOT_HOME/mcp-config.json` when set), using the `mcpServers` object. For
project-scoped configuration, use `.mcp.json` in the project root and review the
folder-trust prompt. Copilot CLI does not read `.vscode/mcp.json`. Configure SDK
environment variables explicitly if relying on them; do not assume all terminal
variables are forwarded.

### Verify and troubleshoot

Ask the client: "Use the android-mcp server's adb_list_devices tool to list
connected Android devices. Do not run terminal commands or change files." Inspect
the actual tool call and its JSON result, not just the assistant's answer.

After changing server code, rebuild with `cargo build --release` and restart or
reconnect the client-managed server. For Remote SSH, WSL, or Dev Containers, the
binary and ADB must be available on the host where the MCP process runs, with
access to the intended ADB server/devices.

Local stdio transport does not prevent the client from sending tool results to
its configured model provider. Review trust and tool approvals, and avoid
committing personal machine paths or credentials in shared configurations.

Official references:
[VS Code](https://code.visualstudio.com/docs/agents/reference/mcp-configuration),
[JetBrains AI Assistant](https://www.jetbrains.com/help/ai-assistant/mcp.html),
[GitHub Copilot CLI](https://docs.github.com/en/copilot/how-tos/copilot-cli/customize-copilot/add-mcp-servers).

### Tests

Run `cargo test` to execute the stdio integration tests. They cover MCP
initialization, tool discovery, device listing, device info, logcat snapshots, unknown tools, clean server shutdown,
ADB startup failures, and PATH/SDK fallback ordering. Each server process gets an
isolated executable-search environment. Temporary SDK layouts are created with
the test-only `tempfile` dependency. The process fixtures currently target Unix systems
(macOS/Linux) and use `/usr/bin/true`, `/usr/bin/false`, and `/usr/bin/yes` as
controlled executables for startup tests. MCP device-list tests compile a small
Rust ADB fixture from `tests/support/adb_stub.rs` once per suite using `rustc`.
The fixture checks the exact ADB arguments and simulates device lists and
failures, so no real ADB installation or Android device is needed.
Full getprop responses are loaded from files with `include_str!()`, including
anonymized TC57 and ARM64 emulator dumps and a synthetic full-mapping profile.
See [getprop fixtures](tests/fixtures/getprop/README.md) for provenance,
anonymization rules, and the expected differences between the device profiles.
Both integration test suites are skipped on non-Unix platforms.

Logcat tests use only synthetic data in `tests/fixtures/logcat/threadtime.txt` and
controlled fixture modes for exact filter arguments, validation, empty output,
limits, failures, and timeouts. They
do not read actual device logs.

Run the suites independently with:

```sh
cargo test --test stdio_server
cargo test --test adb_startup
```

`stdio_server` covers the MCP contract; `adb_startup` covers ADB prerequisites,
path resolution, and startup diagnostics. Shared subprocess and connection
lifecycle helpers live in `tests/support/mod.rs`.