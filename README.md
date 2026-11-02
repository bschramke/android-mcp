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

### Tests

Run `cargo test` to execute the stdio integration tests. They cover MCP
initialization, tool discovery, Hello World, unknown tools, clean server shutdown,
ADB startup failures, and PATH/SDK fallback ordering. Each server process gets an
isolated executable-search environment. Temporary SDK layouts are created with
the test-only `tempfile` dependency. The process fixtures currently target Unix systems
(macOS/Linux) and use `/usr/bin/true`, `/usr/bin/false`, and `/usr/bin/yes` as
controlled executables, so no real ADB installation or Android device is needed.
Both integration test suites are skipped on non-Unix platforms.

Run the suites independently with:

```sh
cargo test --test stdio_server
cargo test --test adb_startup
```

`stdio_server` covers the MCP contract; `adb_startup` covers ADB prerequisites,
path resolution, and startup diagnostics. Shared subprocess and connection
lifecycle helpers live in `tests/support/mod.rs`.