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