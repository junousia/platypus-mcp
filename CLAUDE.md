# Claude Instructions

This repository is the standalone Rust implementation of the Platypus MCP
server. The MCP tool contract is the product boundary; Claude owns the chat
session and model turns, while Platypus owns deterministic project state
transitions.

## Startup

At the start of a project session:

1. List MCP resources and prompts.
2. Read `platypus://guidance/workflow`,
   `platypus://guidance/project-status`, and
   `platypus://guidance/tool-preload`, or the equivalent prompts
   `platypus-workflow`, `platypus-project-status`, and
   `platypus-tool-preload`.
3. Call `inspect_session`.

If `inspect_session` is unavailable or too broad for the current context, fall
back deterministically to `doctor_snapshot`, `inspect_status`,
`inspect_workflow_config`, `inspect_queue_status`, then `inspect_work_queue` as
needed.

Tool schemas are client-driven and may be deferred until discovery or
selection. If Claude can preload schemas, load the Startup Inspection group
first, then load Backlog Planning, Direct Execution, Worker Handoff, Evidence
And Findings, or Recovery only when that phase starts. If preloading is awkward
or unavailable, call the same tools on demand.
`inspect_session` and `inspect_work_queue` return
`schemas_likely_needed_next` with 1-4 likely next tool schemas. Use the
included literal ToolSearch selectors when loading deferred schemas. When
multiple likely schemas are listed, prefer the response-level
`claude_toolsearch_batch_selector` if your Claude Code build accepts
comma-separated selectors. Codex-style text-search hosts should use
`codex_tool_search_query` or the response-level
`host_neutral_tool_search_query`.

When Claude Code defers a Platypus tool schema, use ToolSearch with
`select:mcp__platypus__<tool>`, for example
`select:mcp__platypus__inspect_session` or
`select:mcp__platypus__complete_backlog_item`. The `mcp__platypus__` prefix
comes from the configured MCP server name; the actual Platypus tool names stay
unprefixed in docs and tool results.

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

Alias expectations: `contract` is only an alias for
`implementation_contract`; `quick_create_backlog_item` is simple shorthand for
one item.

Tool naming map: prefer `inspect_status` over alias `project_status`,
`inspect_worktree_changes` over low-level `worktree_diff`, `start_worker_task`
over alias `start_worker_execution`, `record_worker_progress` over alias
`record_worker_event`, and `finish_work` over low-level
`complete_worker_task`/alias `complete_worker_execution`.

## Working Rules

- Keep changes scoped and preserve unrelated user edits.
- Prefer typed MCP tools and structured results over chat-only state.
- Keep backlog markdown declarative; runtime state belongs in `.platy/`.
- Record verification evidence and findings through MCP tools when relevant;
  direct completion normally records verification through
  `complete_backlog_item` itself.
- Run the cheapest relevant verification before reporting completion.
