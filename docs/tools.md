# MCP Tool Contract

Platypus tools return typed JSON with a shared envelope:

- `action`: stable tool action name
- `status`: `completed`, `skipped`, or `failed`
- `summary`: concise human-readable result
- `next_action`: optional recovery or continuation instruction
- `data`: typed result payload
- `error`: optional failure detail

Hosts should prefer `inspect_work_queue` when choosing runnable backlog work,
`classify_planning_needs` when explaining whether design/task planning is
required, and `next_safe_action` when deciding the next lifecycle command. The
lower-level tools remain available for precise control and testing.

## Host Guidance Resources And Prompts

Before using lifecycle tools, MCP hosts should list resources/prompts and read
the Platypus guidance that matches the current activity. The guidance is
deterministic and references the current public tool names.

- Resource `platypus://guidance/workflow` / prompt `platypus-workflow`
- Resource `platypus://guidance/spec-driven-development` / prompt
  `platypus-spec-driven-development`
- Resource `platypus://guidance/project-status` / prompt
  `platypus-project-status`
- Resource `platypus://guidance/backlog-authoring` / prompt
  `platypus-backlog-authoring`
- Resource `platypus://guidance/worker-handoff` / prompt
  `platypus-worker-handoff`
- Resource `platypus://guidance/integration-review` / prompt
  `platypus-integration-review`
- Resource `platypus://guidance/recovery` / prompt `platypus-recovery`

## Recommended Host Flow

1. Bootstrap and inspect: `platypus-mcp bootstrap <host> --init-project` for
   fresh projects, or `init_project`, `doctor_snapshot`, `inspect_status`,
   `inspect_workflow_config` from an MCP host.
2. Shape backlog: `create_backlog_item` or `create_backlog_items`,
   `validate_backlog`, `list_backlog`, `draft_external_backlog_items`,
   `import_github_issues`. Use
   `draft_backlog_items` only when host sampling is available.
3. Plan non-trivial work: `write_task_plan`,
   `validate_task_plan`, `inspect_task_plan`, `list_task_plans`.
   Use `draft_task_plan` only when host sampling is available.
4. Inspect the executable queue: `inspect_work_queue`.
5. Dispatch work: prefer `dispatch_ready_work` for one or more ready items;
   use `dispatch_next_work` and `prepare_worker_handoff` only for precise
   single-step control.
6. Run worker externally: pass the generated bundle/worktree to Codex, Claude,
   or another harness.
7. Record worker activity: `start_worker_task`, `record_worker_progress`,
   `complete_worker_task`, `run_task_verification`.
8. Inspect and verify: `inspect_worktree_changes`,
   `record_verification_evidence`, `record_finding`, `validate_findings`.
9. Integrate and clean up: `integrate_worker_result`, `reconcile_project`,
   `worktree_cleanup`.

```mermaid
flowchart LR
    bootstrap["Bootstrap<br/>init and doctor"]
    backlog["Backlog<br/>draft, create, validate, list"]
    dispatch["Dispatch<br/>ready work and handoff"]
    worker["Worker<br/>external harness"]
    result["Result<br/>progress, complete, verify"]
    integrate["Integrate<br/>merge policy and trailers"]
    reconcile["Reconcile<br/>evidence, findings, closure"]

    bootstrap --> backlog --> dispatch --> worker --> result --> integrate --> reconcile
```

## Tool Groups

### Project And Configuration

- `ping`: health check.
- `init_project`: create missing project scaffold files. Fresh CLI bootstrap
  can also run this through `bootstrap <host> --init-project`.
- `doctor_snapshot`: inspect setup issues and recovery guidance.
- `inspect_status` / `project_status`: inspect project shape and runnable work.
- `inspect_workflow_config`: inspect effective workflow integration defaults.
- `list_agent_profiles`: list configured manager and worker profiles.
- `configure_agent_profile`: create or update one agent profile.

## Local Tool Smoke Helper

For development, the binary can invoke one MCP tool through its own stdio
server and print the structured result as formatted JSON:

```bash
cargo run -- tool --root "$PWD" inspect_work_queue '{"limit":5}'
```

The helper exits non-zero when the MCP tool returns a failed structured result
or when the protocol call itself fails. It is intended for local smoke testing
and scripts; MCP hosts should still call tools through MCP directly.

The Makefile wraps the most common smoke checks:

```bash
make smoke
make smoke-queue
make smoke-storage
```

### Backlog

- `draft_backlog_items`: optionally ask a sampling-capable MCP host to draft
  concrete backlog candidates from a goal. If sampling is unavailable, the tool
  skips instead of returning generic templates; the host should call
  `create_backlog_item` directly.
- `draft_external_backlog_items`: draft provider-neutral candidates from
  host-provided external work records and skip already-imported references.
- `import_github_issues`: import host-provided GitHub issue records as local
  backlog snapshots. The first implementation does not call GitHub directly;
  the MCP host supplies issue JSON from its approved GitHub integration.
- `draft_external_report`: draft a provider-neutral external report payload
  from local backlog, task, and evidence state without contacting a provider.
- `request_external_report_approval`: create a durable approval request for a
  drafted external report.
- `record_external_report_dispatch`: record the approved host/plugin provider
  dispatch result, including safe evidence and redacted metadata.
- `create_backlog_item`: write one structured backlog item.
- `create_backlog_items`: atomically write related backlog items in one call.
- `create_epic`: write one structured backlog epic.
- `list_epics`: list existing backlog epics and their metadata.
- `validate_backlog`: validate backlog item and epic files.
- `list_backlog`: list runnable backlog candidates.
- `inspect_backlog_inventory`: inspect all backlog items with runnable,
  blocked, and Git-trailer closure reasons. Use this when `list_backlog` says
  no work is runnable but the host needs to explain whether items are closed,
  dependency-blocked, or ready. Without `limit`, the tool returns the full
  inventory; limited calls include `returned` and `truncated` fields.

Backlog schema quick reference:

- Minimal `create_backlog_item` input: a meaningful `goal` or `title`.
  Platypus derives conservative title, goal, implementation contract, and first
  acceptance text when those fields are omitted.
- Rich `create_backlog_item` input: provide explicit `title`, `goal`,
  `implementation_contract` or `contract`, and `acceptance` when the work is
  complex or the generated defaults would be too broad.
- Use `create_backlog_items` for related items that should land together. It
  accepts per-item `client_key` values and resolves `depends_on_keys` to the
  created item IDs; if any item fails validation, no batch files are written.
- Priority values: `P0`, `P1`, `P2`.
- Type values: `foundation`, `feature`, `safety`, `ux`, `test`, `docs`.
- Defaults when omitted: priority `P1`, type `feature`, epic `general`,
  suggested worker `coder`.
- `suggested_worker` is a Platypus worker profile name. It is not guaranteed
  to match a host-specific subagent type such as `explore` or `general`.
- Failed backlog authoring calls return actionable `next_action` guidance for
  common recovery cases such as unknown epics, missing dependencies, duplicate
  IDs, invalid enum values, missing scaffold directories, and malformed
  external refs.
- Use `list_epics` before assigning a non-default epic. Use `create_epic` to
  add a missing grouping instead of hand-writing `backlog/epics/*.md`.

Backlog item frontmatter may include provider-neutral `external_refs`. Use them
to preserve where work came from or where results should be reported without
making an external tracker the execution contract.

External intake adapters should map provider-specific records into the same
draft shape before creating local backlog items. The local backlog item remains
the stable executable snapshot.

#### External Reporting And Sync Policy

Platypus should treat external trackers as integration surfaces, not hidden
runtime state. Supported policy modes:

- **Import-only:** external records are copied into local backlog snapshots.
  Execution, closure, evidence, and findings stay local. This is the
  recommended first product mode and is what `import_github_issues` implements.
- **Import plus explicit reporting:** local execution remains canonical, but a
  future approved tool may post a summary, PR link, verification result, or
  follow-up finding back to the external record.
- **Mirror:** selected local state is reflected into external fields or labels.
  This needs drift detection and conflict handling before it is safe.
- **Bidirectional sync:** external edits can update local backlog snapshots.
  This is high-risk and requires provider-specific merge policy, audit, and
  user approval.
- **External-source mode:** the tracker is treated as the source of backlog
  truth. This is intentionally not the default because it makes local Git
  history and MCP runtime state harder to reason about.

Safe automation:

- read/import host-approved external records
- draft local backlog snapshots
- detect duplicate external refs and source-hash drift
- draft external report payloads without sending them

Approval-required actions:

- posting issue comments, PR links, labels, statuses, or closure updates
- mutating external issue fields
- resolving conflicts where external and local state disagree
- enabling bidirectional sync or external-source mode

Security rules for future provider tools:

- credentials stay in the MCP host or an approved secret store, never in backlog
  frontmatter, task plans, evidence, events, or Git commits
- tool output must redact tokens, cookies, private headers, and provider error
  payloads that may contain secrets
- provider rate limits and outages return structured skipped/failed results
  with retry guidance
- plugin-backed providers must map through the provider-neutral external record
  and external ref schemas before creating or updating local backlog items

### Task Plans

- `draft_task_plan`: draft a strict plan for one backlog item without writing
  it.
- `write_task_plan`: write one reviewable plan to `backlog/plans/<ITEM>.yaml`.
- `validate_task_plan`: validate strict plan YAML, task IDs, dependencies,
  requirement references, owned surfaces, verification, and acceptance.
- `inspect_task_plan`: read one committed task plan.
- `list_task_plans`: list committed task plans.

Task plan YAML is planned work only. It must not contain runtime status,
completion commits, attempts, evidence, or worker diary fields. Runtime state
belongs in `.platy/platypus.sqlite3`, evidence records, task events, and Git
trailers.

### Task And Workspace Lifecycle

- `plan_goal_work`: read-only intake guidance for a broad user goal. It
  classifies mode and returns the concrete next `start_goal_work` arguments
  without creating files, backlog items, tasks, or state.
- `start_goal_work`: mutating intake for new goals. It can classify mode,
  create or reuse a lightweight tracking backlog item, and optionally dispatch
  prepared work.
- `next_safe_action`: recommend the next safe tool call and parameters.
- `classify_workflow_fit`: classify broad user goals as `direct_scaffold`,
  `platypus_workflow`, or `hybrid` before forcing backlog ceremony.
- `inspect_work_queue`: inspect runnable backlog candidates with task-plan
  state, active task counts, and a recommended next tool. Items already
  dispatched are omitted from ready candidates and reported through
  `active_count` and `active_item_ids`.
- `classify_planning_needs`: classify runnable items as `direct`, `standard`,
  or `full` planning mode with structured reasons.
- `dispatch_ready_work`: preferred host flow for executable backlog work. It
  checks Git readiness, dispatches up to `max_tasks` runnable independent
  items, skips already-active items, and prepares worker assignments,
  worktrees, and bundles in one call.
  Auto-commit of backlog artifacts is opt-in via
  `auto_commit_artifacts=true`.
  For direct-scaffold goals, call `plan_goal_work` first when unsure. Its
  recommended arguments prefer `dispatch=true` so one mutating call can create
  tracking and prepare the first task worktree. Use `dispatch=false` only when
  intentionally editing the manager workspace directly.
- `dispatch_next_work`: low-level single-item queueing. It also checks Git
  readiness before creating task state, but hosts usually want
  `dispatch_ready_work` so worker assignment state cannot be accidentally
  skipped.
- `inspect_task`: inspect one task lifecycle record.
- `claim_next_task`: atomically claim a queued task.
- `worktree_create`: create an isolated task worktree.
- `worktree_status`: inspect recorded worktree metadata. `exists` means the
  recorded worktree path is present on disk; `created` only means the current
  tool call created a new worktree.
- `worktree_diff` / `inspect_worktree_changes`: inspect bounded worktree
  changes.
- `worktree_cleanup`: remove a clean or explicitly forced task worktree.
- `integrate_worker_result`: merge, fast-forward, or squash a completed
  verified task branch into the manager workspace.
- `generate_task_bundle`: generate a deterministic worker brief.
- `runner_prepare_next`: claim queued tasks and prepare worktrees/bundles.

### Worker Handoff And Results

- `prepare_worker_assignment` / `prepare_worker_handoff`: persist a worker
  handoff object.
- `inspect_worker_assignment`: inspect a persisted worker assignment.
- `start_worker_execution` / `start_worker_task`: mark a prepared assignment as
  running.
- `record_worker_event` / `record_worker_progress`: persist worker progress.
- `complete_worker_execution` / `complete_worker_task`: finish a worker task
  with result, changed files, and verification status. For same-session host
  flows, completion can auto-start a prepared assignment by default. Set
  `auto_start_if_prepared=false` only when strict running-only completion is
  required.
- `run_task_verification`: execute the assignment verification command in the
  task worktree and persist a verification run event/evidence record.
- `send_worker_guidance`: persist steering messages for active tasks.

### Supervision, Evidence, And Findings

- `approval_list` / `approval_respond`: inspect and resolve durable approval
  requests.
- `events_replay`: replay bounded project/task/approval/worker events.
- `storage_capability_probe`: run the documented storage backend contract probe
  against an isolated in-memory backend.
- `inspect_task_events`: replay task-scoped events.
- `record_evidence` / `record_verification_evidence`: persist audit evidence.
- `list_evidence`: inspect evidence records.
- `reconcile_project`: report required gaps across tasks, findings, evidence,
  and closure trailers.
- `record_finding`: persist a follow-up finding.
- `list_findings`: list stored findings.
- `validate_findings`: fail when required findings remain unresolved.
- `update_finding_disposition`: resolve, reject, defer, or assign findings.

## Example: Guided Dispatch

```json
{
  "tool": "next_safe_action",
  "arguments": {}
}
```

When a backlog item is runnable, the result recommends `dispatch_ready_work`.
It returns task ids, assignment ids, worktree paths, and per-item skipped or
failed reasons in one response. After dispatch, use `start_worker_task` when
you need an explicit running transition, then `record_worker_progress` while
the assignment is running. Same-session host flows can complete a prepared
assignment directly with `complete_worker_task`.

## Workflow Configuration

New projects include:

```yaml
workflow:
  integration:
    merge_style: merge_commit
    require_clean_manager_workspace: true
    require_verification_evidence: true
```

Current valid merge styles are `merge_commit`, `fast_forward`, and `squash`.
`integrate_worker_result` uses this policy when bringing completed worker
branches back into the manager workspace.
