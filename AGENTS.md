# Agent Instructions

This repository is the standalone Rust implementation of the Platypus MCP
server. It is intentionally separate from the Python Platypus prototype so the
MCP contract can become the stable product boundary.

The server uses Rust, Tokio, RMCP, Serde, Schemars, and JSON-RPC over MCP
transports. The first supported transport is stdio.

## Project Commands

- Build-check: `cargo check`
- Test: `cargo test`
- Format check: `cargo fmt --check`
- Format: `cargo fmt`
- Run stdio server: `cargo run`

If `cargo fmt` is unavailable, report that `rustfmt` is missing and still run
`cargo check` and `cargo test`.

## Working Rules

- Inspect before editing. Use `rg`, file reads, and existing tests to understand
  the target area.
- Use repository skills from `.agents/skills/<skill-name>/SKILL.md` when a task
  matches one.
- Keep changes small and scoped to the MCP server.
- Preserve unrelated user changes.
- Do not commit secrets, local caches, build output, or `.env` files.
- Keep `target/` out of Git.
- Prefer typed request/response structs with `serde` and `schemars::JsonSchema`
  for MCP tools.
- Return structured errors from tools instead of panicking.
- Keep all file and workspace operations bounded to an explicit project root.
- Avoid arbitrary shell execution inside tools. If command execution becomes
  necessary, add an explicit allowlist, timeout, audit metadata, and tests.
- Build durable product behavior around MCP tools, not client-specific chat UI
  assumptions.
- Do not expose hidden chain-of-thought. It is fine to expose safe status,
  tool calls, events, and summaries.

## MCP Design Rules

- MCP tools perform deterministic state transitions or inspection.
- The MCP host (Codex, Claude, or another client) owns conversation and model
  turn lifecycle.
- Mutating tools must be easy to identify from their name, schema, and result.
- Tool responses should include `status`, `summary`, and `next_action` when the
  caller needs a recovery path.
- Sampling is optional. Do not make core project state transitions depend on
  client-side sampling support.
- Prefer stable tool names and versioned schemas over hidden behavior changes.

## Initial Tool Surface

The first production tool set should cover:

- `inspect_status`
- `list_backlog`
- `validate_backlog`
- `draft_backlog_items`
- `create_backlog_item`
- `dispatch_next_work`
- `inspect_task_events`
- `send_worker_guidance`
- `list_findings`
- `validate_findings`
- `update_finding_disposition`

Near-term extensions:

- `worktree_create`
- `worktree_status`
- `worktree_diff`
- `worktree_cleanup`
- `approval_list`
- `approval_respond`
- `events_replay`
- `doctor_snapshot`

## Branch And PR Workflow

- Check the current branch before starting work.
- Use feature branches for publishable work once this repo has a remote.
- Keep PRs as isolated units of change.
- Do not push directly to `main` unless the user explicitly asks for it.
- Wait for review and CI before merging once GitHub is configured.

## Commit Policy

- Commit when the user asks for a commit or when a meaningful checkpoint is
  reached during longer work.
- Inspect `git status --short --untracked-files=all` before staging.
- Stage only files that belong to the current task.
- Keep unrelated changes out of the commit.
- Run verification before committing:
  - `cargo fmt --check` when `rustfmt` is available
  - `cargo check`
  - `cargo test`
- Use concise imperative commit messages, for example
  `Add backlog validation tool`.

## Architecture Notes

- Current entrypoint: `src/main.rs`.
- Current server framework: RMCP.
- Tool schemas should come from Rust structs, not hand-written JSON.
- Stdio is the default transport. Add streamable HTTP only when the stdio tool
  contract is stable.
- Keep integration with the Python Platypus repo behind explicit boundaries.
  Prefer file/config/state formats over importing Python runtime behavior.

## Safety Expectations

- Resolve paths before checking containment.
- Block path traversal and symlink escape risks for project-root operations.
- Bound large outputs.
- Redact secrets before returning tool output.
- Mutating tools should preserve audit-friendly result data.
- Add or update tests for safety boundaries.

## Completion Criteria

Before saying work is complete:

1. Run `cargo fmt --check` when available.
2. Run `cargo check`.
3. Run `cargo test`.
4. Summarize changed behavior and any remaining gaps.
5. If verification cannot run, explain exactly why.
