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
- `dispatch_next_work`: select the next runnable backlog item and create a
  queued task record for external harness execution
- `inspect_task`: inspect one persisted task lifecycle record
- `claim_next_task`: atomically claim the next queued task for an external
  runner
- `worktree_create`: create an isolated Git worktree for a task
- `worktree_status`: inspect a task worktree recorded in storage
- `worktree_diff` / `worktree_cleanup`: inspect bounded worktree changes and
  safely remove clean or explicitly forced task worktrees
- `generate_task_bundle`: generate a deterministic worker brief for a task
- `runner_prepare_next`: claim queued tasks and prepare worktrees and bundles
  without executing workers
- `send_worker_guidance`: persist task-scoped worker steering messages for
  queued, claimed, or running tasks
- `approval_list` / `approval_respond`: inspect and resolve durable approval
  requests
- `events_replay`: replay bounded project, task, approval, and worker events
- `record_evidence` / `list_evidence`: store and inspect audit evidence for
  tasks, findings, and backlog items
- `reconcile_project`: compare Git trailer closure, task state, findings, and
  evidence for required gaps
- `list_agent_profiles` / `configure_agent_profile`: inspect and update
  manager and worker profile configuration in `platy.yaml`
- worker adapter boundary with a fake adapter test path for runner integration
- Codex adapter skeleton for typed configuration validation and safe app-server
  event mapping
- Claude adapter skeleton for typed configuration validation and safe stream JSON
  event mapping
- local SQLite storage foundation under `.platy/platypus.sqlite3` for task
  events, findings, and schema metadata
- worker execution remains external; the Rust MCP server does not run arbitrary
  shell commands or harnesses

## Structure

- `src/main.rs`: stdio entrypoint
- `src/server.rs`: RMCP server and tool router
- `src/models.rs`: public tool request and response schemas
- `src/backlog/`: backlog parsing, validation, listing, drafting, and creation
- `src/storage/`: SQLite setup plus typed repository APIs; see
  [docs/storage.md](docs/storage.md)

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
