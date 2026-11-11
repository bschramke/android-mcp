# android-mcp

A local Rust MCP server for controlling Android devices through ADB. MCP clients launch it over stdio and discover its typed tools automatically.

## Requirements

- Rust toolchain for edition 2024
- Android Debug Bridge and an authorized device or emulator

## Build

```sh
cargo build --locked --release
```

The executable is `target/release/android-mcp` (`android-mcp.exe` on Windows).

### ADB discovery

At startup the server verifies `adb version` (five-second timeout). It uses:

1. `ANDROID_MCP_ADB`, if set; an invalid override fails startup.
2. `adb` on `PATH`.
3. `$ANDROID_SDK_ROOT/platform-tools/adb`, then `$ANDROID_HOME/platform-tools/adb`.

SDK paths use `adb.exe` on Windows. The check confirms the executable only; device connectivity is checked by tools such as `adb_health_check`.

## Tools

**Device and diagnostics**

- `adb_list_devices`, `adb_health_check`
- `adb_device_info`, `adb_display_info`
- `adb_get_screen_orientation`, `adb_set_screen_orientation`, `adb_emu_rotate`
- `adb_list_crashes`, `adb_get_crash`, `adb_get_crash_file`

**Apps**

- `adb_list_apps`, `adb_app_info`, `adb_get_foreground_app`
- `adb_launch_app`, `adb_terminate_app`
- `adb_install_app`, `adb_uninstall_app`, `adb_clear_app_data`

**Input and navigation**

- `adb_tap_on_screen`, `adb_long_press_on_screen`, `adb_swipe_on_screen`
- `adb_input_text`, `adb_press_key`, `adb_open_url`

**Logs and screen capture**

- `adb_logcat`
- `adb_list_elements_on_screen`, `adb_get_screen_hierarchy`
- `adb_take_screenshot`, `adb_save_screenshot`
- `adb_start_screen_recording`, `adb_stop_screen_recording`

Tool schemas describe each tool's arguments. Key behavior and limits:

- Device operations require a serial; `adb_list_devices` does not.
- `adb_launch_app` starts the package's `MAIN`/`LAUNCHER` activity. `adb_terminate_app` force-stops the package without clearing its data.
- `adb_clear_app_data` deletes local app data. Uninstalling also changes device state; use client approvals for state-changing tools.
- Input coordinates are display pixels and are not scaled. `adb_input_text` accepts printable ASCII only. Successful input means ADB completed the command, not necessarily that the app handled it.
- Logcat and screen content may contain sensitive data. They are returned to the MCP client and are not saved on the host unless a file path is supplied.
- Existing output files are preserved by default. File-producing tools require absolute paths; overwrite must be explicitly enabled.
- Bounded ADB failures and timeouts are returned as tool errors; they do not terminate the MCP server.

## Limits worth knowing

- `adb_logcat` defaults to 200 lines (1-2000). It supports PID/package, tag, and severity filters; inline output is capped at 256 KiB and file output at 16 MiB.
- UI hierarchy capture has a 10-second timeout and a 1-MiB XML limit. Screenshots are capped at 16 MiB.
- Screen recording is limited to 180 seconds and one active recording per device; call `adb_stop_screen_recording` to save it.
- Crash listing defaults to crash, ANR, and tombstone entries (limit 1-1000). Structured crash content is capped at 256 KiB and may be truncated; raw files can be blocked by device permissions.
- `adb_set_screen_orientation` changes the device-wide rotation mode; `auto` releases the lock. `adb_emu_rotate` works only with Android Emulator.

## Connect a client

The client starts the server over stdio; no port or separate daemon is required. Use absolute paths and set `ANDROID_MCP_ADB` explicitly when the client may not inherit your shell environment.

### VS Code

Run **MCP: Add Server**, choose **Command (stdio)**, and point to the binary. A project-scoped `.vscode/mcp.json` example:

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

Review workspace trust and enable the tools in the agent chat. For the portable `.mcp.json` format, use `mcpServers` instead of `servers`; do not register the same server in both files.

### IntelliJ IDEA

In **Settings | Tools | AI Assistant | Model Context Protocol (MCP)**, add a stdio server with the executable path and `ANDROID_MCP_ADB` environment variable. Choose project or global scope, then verify the connection and tool list.

### GitHub Copilot CLI

```sh
copilot mcp add android-mcp \
  --env ANDROID_MCP_ADB=/absolute/path/to/android-sdk/platform-tools/adb \
  --tools adb_list_devices,adb_device_info,adb_display_info,adb_get_screen_orientation,adb_set_screen_orientation,adb_emu_rotate,adb_health_check,adb_logcat,adb_list_crashes,adb_get_crash,adb_get_crash_file,adb_take_screenshot,adb_save_screenshot,adb_list_elements_on_screen,adb_get_screen_hierarchy,adb_start_screen_recording,adb_stop_screen_recording,adb_list_apps,adb_app_info,adb_get_foreground_app,adb_launch_app,adb_terminate_app,adb_install_app,adb_uninstall_app,adb_clear_app_data,adb_tap_on_screen,adb_long_press_on_screen,adb_swipe_on_screen,adb_input_text,adb_press_key,adb_open_url \
  -- /absolute/path/to/android-mcp/target/release/android-mcp
```

Quote paths or `KEY=value` arguments that contain spaces. Use `/mcp show android-mcp` to inspect the registered tools. Copilot CLI uses `~/.copilot/mcp-config.json` (or `$COPILOT_HOME/mcp-config.json`) for user scope and `.mcp.json` for project scope; it does not read `.vscode/mcp.json`.

## Development and tests

```sh
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
cargo fmt --check
```

Integration tests use a controlled ADB stub and fixtures; they do not require a connected device. The subprocess integration suites currently run on Unix (macOS/Linux). See [getprop fixture notes](tests/fixtures/getprop/README.md) for fixture provenance and anonymization.

Local stdio transport does not prevent a client from sending tool results to its configured model provider. Review tool approvals and avoid committing personal machine paths or credentials in shared configurations.

## References

- [VS Code MCP configuration](https://code.visualstudio.com/docs/agents/reference/mcp-configuration)
- [JetBrains AI Assistant MCP](https://www.jetbrains.com/help/ai-assistant/mcp.html)
- [GitHub Copilot CLI MCP servers](https://docs.github.com/en/copilot/how-tos/copilot-cli/customize-copilot/add-mcp-servers)
