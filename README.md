# Platypus MCP (Rust)

This repository contains the Rust MCP server for Platypus. The MCP contract is
the product boundary for Codex, Claude, and other harnesses: clients own the
chat session, while this server exposes deterministic project-management tools.

## Current scope

- `ping`: health-check tool
- `inspect_status` / `project_status`: inspect project shape and runnable work
- `list_backlog`: list runnable backlog candidates
- `validate_backlog`: validate structured backlog files
- `doctor_snapshot`: inspect setup issues and recovery guidance
- `init_project`: create missing project/backlog scaffold files
- `draft_backlog_items`: draft typed candidate items from a goal
- `create_backlog_item`: write one valid backlog item
- `record_finding`: persist a worker or manager follow-up finding
- `list_findings`: list stored findings with filters
- `validate_findings`: fail when required findings remain unresolved
- `update_finding_disposition`: resolve, reject, defer, or assign findings
- `inspect_task_events`: replay bounded task supervision events
- local SQLite storage foundation under `.platy/platypus.sqlite3` for task
  events, findings, and schema metadata
- runtime dispatch tools are exposed with structured `skipped` responses until
  worker dispatch is implemented

## Structure

- `src/main.rs`: stdio entrypoint
- `src/server.rs`: RMCP server and tool router
- `src/models.rs`: public tool request and response schemas
- `src/backlog/`: backlog parsing, validation, listing, drafting, and creation

Keep new behavior in focused modules. Avoid adding large all-purpose files.

## Verify

```bash
make check
```

## Run

```bash
make run
```

By default the server uses the current directory as the Platypus project root.
Set `PLATYPUS_MCP_ROOT` to bind tools to a specific project:

```bash
PLATYPUS_MCP_ROOT=/path/to/project cargo run
```

## Client Configuration

Use stdio while the tool contract stabilizes.

Codex-style configuration:

```toml
[mcp_servers.platypus]
command = "cargo"
args = ["run", "--manifest-path", "/path/to/platypus-mcp-rs/Cargo.toml"]
env = { PLATYPUS_MCP_ROOT = "/path/to/project" }
```

Claude-style configuration:

```json
{
  "mcpServers": {
    "platypus": {
      "command": "cargo",
      "args": ["run", "--manifest-path", "/path/to/platypus-mcp-rs/Cargo.toml"],
      "env": {
        "PLATYPUS_MCP_ROOT": "/path/to/project"
      }
    }
  }
}
```
