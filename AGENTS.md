# Agent Instructions

This repository is the standalone Rust implementation of the Platypus MCP
server. It is intentionally separate from the Python Platypus prototype so the
MCP contract can become the stable product boundary.

The server uses Rust, Tokio, RMCP, Serde, Schemars, and JSON-RPC over MCP
transports. The first supported transport is stdio.

## Project Commands

- Full verification: `make check`
- Format: `make format`
- Lint/build-check: `make lint`
- Test: `make test`
- Run stdio server: `make run`
- Build-check: `cargo check`
- Test: `cargo test`
- Format check: `cargo fmt --check`
- Format: `cargo fmt`
- Run stdio server: `cargo run`

Prefer Makefile targets for normal development and CI-style checks. Use direct
Cargo commands when debugging a specific compiler or test issue.

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
- Tool responses should include `status`, `summary`, `next_action` for normal
  continuation, and `recovery_action` when the caller needs a recovery path.
- Model judgment belongs to the MCP host. Do not expose pseudo-intelligent MCP
  tools that hide model decisions behind deterministic-looking names.
- Prefer stable tool names and versioned schemas over hidden behavior changes.

## Harness Startup

For Codex, Claude, opencode, and other MCP hosts, begin a fresh project session
by listing MCP resources and prompts. Read `platypus://guidance/workflow`,
`platypus://guidance/project-status`, and
`platypus://guidance/tool-preload`, or the equivalent prompts
`platypus-workflow`, `platypus-project-status`, and
`platypus-tool-preload`.

Tool schemas are client-driven and may be deferred until discovery or
selection. If the host supports schema preloading, load the Startup Inspection
group first, then load Backlog Planning, Direct Execution, Worker Handoff,
Evidence And Findings, or Recovery only when that phase starts. If preloading
is awkward or unavailable, call the same tools on demand.
`inspect_session` and `inspect_work_queue` return
`schemas_likely_needed_next` with 1-4 likely next tool schemas; Claude hints
include literal ToolSearch selectors and a combined
`claude_toolsearch_batch_selector`. Codex-style text-search hosts should use
`codex_tool_search_query` or the response-level
`host_neutral_tool_search_query`. Other hosts should use `tool_name` or
`host_neutral_query` values with their own discovery UI.

Claude Code uses ToolSearch selectors such as
`select:mcp__platypus__inspect_session`; the `mcp__platypus__` prefix comes
from the configured server name. Codex and opencode may expose the same schemas
through their own MCP discovery surfaces.

Direct quick path: load Startup Inspection, call `inspect_session`, load Direct
Execution, inspect for `direct_ready`, edit the manager workspace, then call
`complete_backlog_item`. Call `prepare_work` first only when response-local
guidance is useful. Prefer the structured `minimal_direct_loop` and
`recommended_tool=complete_backlog_item` over prose when both are present.
For direct work, pass verification fields to `complete_backlog_item` and leave
`record_auto_evidence` omitted unless explicit evidence already exists; separate
`record_verification_evidence` calls are for extra evidence, worker handoff, or
recovery.
If multiple sessions might edit the same direct item, optionally claim it with
`acquire_lease(scope=task, target_id=<item id>)`, renew while working, and
release after `complete_backlog_item`. Queue tools show these claims through
`active_lease_id`; the lease is not a completion record.

Tool naming map: prefer `inspect_status` over alias `project_status`,
`inspect_worktree_changes` over low-level `worktree_diff`, `start_worker_task`
over alias `start_worker_execution`, `record_worker_progress` over alias
`record_worker_event`, and `finish_work` over low-level
`complete_worker_task`/alias `complete_worker_execution`.

After guidance is loaded, call `inspect_session`. If that broad snapshot is
unavailable, fall back to `doctor_snapshot`, `inspect_status`,
`inspect_workflow_config`, `inspect_queue_status`, then `inspect_work_queue` as
needed.

## Initial Tool Surface

The first production tool set should cover:

- `inspect_status`
- `init_project`
- `list_backlog`
- `validate_backlog`
- `get_backlog_item`
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
  - `make check`
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
