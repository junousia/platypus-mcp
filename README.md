# Platypus MCP (Rust)

This repository contains the Rust MCP server for Platypus. The MCP contract is
the product boundary for Codex, Claude, and other harnesses: clients own the
chat session, while this server exposes deterministic project-management tools.

## Current scope

Platypus currently supports project initialization, backlog authoring,
deterministic task dispatch, isolated Git worktrees, worker handoff bundles,
worker progress/result recording, approvals, events, findings, evidence, agent
profiles, workflow integration configuration, and reconciliation.

Use `next_safe_action` as the host-facing guide for the next safe tool call.
The preferred workflow is documented in [docs/workflow.md](docs/workflow.md),
and the full tool surface is documented in [docs/tools.md](docs/tools.md).

Near-term work now focuses on tightening integration evidence, reconciliation,
full lifecycle smoke coverage, and MCP host guidance. That roadmap is
documented in [docs/roadmap.md](docs/roadmap.md).

## Product Boundary

```mermaid
flowchart LR
    host["MCP host<br/>Codex, Claude, or another client"]
    mcp["Platypus MCP server<br/>deterministic local tools"]
    state[".platy/platypus.sqlite3<br/>runtime state"]
    files["Repository files<br/>backlog, platy.yaml, worktrees"]
    git["Git history<br/>closure and verification trailers"]
    worker["Worker harness<br/>runs in isolated worktree"]

    host -->|MCP tool calls| mcp
    mcp --> state
    mcp --> files
    mcp --> git
    host -->|assignment bundle| worker
    worker -->|progress and result tools| mcp
```

The host manages chat, model context, and worker execution. Platypus manages
the durable project state transitions that must be deterministic, inspectable,
and recoverable.

## Structure

- `src/main.rs`: stdio entrypoint
- `src/server.rs`: RMCP server and tool router
- `src/models.rs`: public tool request and response schemas
- `src/backlog/`: backlog parsing, validation, listing, drafting, and creation
- `src/storage/`: SQLite setup plus typed repository APIs; see
  [docs/storage.md](docs/storage.md)
- `docs/workflow.md`: current MCP host workflow
- `docs/roadmap.md`: product direction and near-term roadmap

Keep new behavior in focused modules. Avoid adding large all-purpose files.

## Backlog State Model

Backlog markdown is intentionally declarative. Items describe intent,
constraints, dependencies, surfaces, and acceptance criteria. They do not carry
runtime fields such as status, assignment, PRs, task attempts, or closure
metadata.

Closure is derived from reachable Git trailers:

```text
Platypus-Closes: MCP-123
Platypus-Verification: make check
```

Runtime state such as queued/running tasks, task events, findings, claims, and
future worker attempts belongs in `.platy/platypus.sqlite3`.

## Verify

```bash
make check
```

## Run

```bash
make run
```

Run the local preparation runner:

```bash
cargo run -- runner --max-tasks 1
```

Inspect the current workflow integration policy:

```bash
PLATYPUS_MCP_ROOT=/path/to/project cargo run
```

Then call the MCP tool `inspect_workflow_config` from the host.

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
