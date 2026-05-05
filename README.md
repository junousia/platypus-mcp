# Platypus MCP (Rust)

This directory contains a Rust MCP server implementation based on RMCP.

## Current scope

- `ping`: health-check tool
- `project_status`: quick project-shape check (`platy.yaml`, `backlog/`, `.git`)

## Run

From repository root:

```bash
make mcp-rs-check
make mcp-rs-run
```

Or directly:

```bash
cargo check --manifest-path mcp-rs/Cargo.toml
cargo run --manifest-path mcp-rs/Cargo.toml
```
