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

### Available tools

#### adb_list_devices

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

#### adb_device_info

Provide a device serial number as a required string argument:

```json
{"serial": "emulator-5554"}
```

The tool runs exactly `adb -s <serial> shell getprop` once using the resolved ADB
executable, without shell interpolation. The serial is passed as one literal
argument and must not be empty or contain whitespace or NUL characters. The
entire command has a five-second timeout.

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

Missing or empty scalar properties are `null`; missing ABI lists are `[]`.
Unknown boolean values remain `null`, so a missing emulator flag is not treated
as proof of a physical device. Invalid nonempty API-level numbers, malformed or
empty property output, invalid UTF-8, unavailable/offline/unauthorized devices,
and command failures/timeouts produce tool errors. The MCP connection remains
available after a tool error.

Only the documented properties are returned, never the complete property dump.
This tool does not add fields or extra device queries to `adb_list_devices`.

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
	--tools adb_list_devices,adb_device_info \
	-- /absolute/path/to/android-mcp/target/release/android-mcp
```

Quote paths or the complete `KEY=value` argument when they contain spaces.
Alternatively, start `copilot`, enter `/mcp add`, choose **STDIO**, and provide the
binary path, environment variables, and `adb_list_devices,adb_device_info` as the enabled tools.
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
initialization, tool discovery, device listing, device info, unknown tools, clean server shutdown,
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

Run the suites independently with:

```sh
cargo test --test stdio_server
cargo test --test adb_startup
```

`stdio_server` covers the MCP contract; `adb_startup` covers ADB prerequisites,
path resolution, and startup diagnostics. Shared subprocess and connection
lifecycle helpers live in `tests/support/mod.rs`.