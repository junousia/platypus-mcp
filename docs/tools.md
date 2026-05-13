# MCP Tool Contract

Platypus tools return typed JSON with a shared envelope:

- `action`: stable tool action name
- `status`: `completed`, `skipped`, or `failed`
- `summary`: concise human-readable result
- `next_action`: optional normal continuation instruction
- `recovery_action`: optional recovery instruction for failed or blocked states
- `data`: typed result payload
- `error`: optional failure detail

Hosts should prefer `inspect_session` at startup and after stale chat context.
Use `inspect_queue_status` for compact dashboards and chat summaries. Use
`inspect_work_queue` when inspecting runnable backlog work, task-plan state,
active lifecycle state, setup blockers, and the recommended next lifecycle
command. The lower-level tools remain available for precise control and
testing.

## Host Guidance Resources And Prompts

Before using lifecycle tools, MCP hosts should list resources/prompts and read
the Platypus guidance that matches the current activity. The guidance is
deterministic and references the current public tool names.

- Resource `platypus://guidance/workflow` / prompt `platypus-workflow`
- Resource `platypus://guidance/spec-driven-development` / prompt
  `platypus-spec-driven-development`
- Resource `platypus://guidance/project-status` / prompt
  `platypus-project-status`
- Resource `platypus://guidance/tool-preload` / prompt
  `platypus-tool-preload`
- Resource `platypus://guidance/backlog-authoring` / prompt
  `platypus-backlog-authoring`
- Resource `platypus://guidance/worker-handoff` / prompt
  `platypus-worker-handoff`
- Resource `platypus://guidance/integration-review` / prompt
  `platypus-integration-review`
- Resource `platypus://guidance/recovery` / prompt `platypus-recovery`

`platypus://guidance/tool-preload` names two optional startup groups:
planning-session tools such as `doctor_snapshot`, `create_backlog_items`,
`inspect_session`, `inspect_queue_status`, `inspect_work_queue`, and
`write_task_plan`; and
execution-session tools such as `dispatch_ready_work`, `inspect_worktree_changes`,
`commit_planning_artifacts`, `complete_backlog_item`, `complete_worker_task`, `finish_work`,
`record_verification_evidence`, `integrate_worker_result`, and
`reconcile_project`. Tool preloading is host-specific. If a host cannot preload
schemas, call the same tools normally when they are needed.

## Recommended Host Flow

1. Bootstrap and inspect: `platypus-mcp bootstrap <host> --init-project` for
   fresh projects, or `init_project` and `inspect_session` from an MCP host.
2. Shape backlog: the host model decides concrete item boundaries, then calls
   `create_backlog_item`, `create_backlog_items`, `update_backlog_item`,
   `validate_backlog`, `list_backlog`, `draft_external_backlog_items`, and
   `import_github_issues`.
3. Plan non-trivial work: `write_task_plan`,
   `validate_task_plan`, `inspect_task_plan`, `list_task_plans`.
4. Inspect the executable queue: `inspect_queue_status` for compact triage,
   then `inspect_work_queue` when the host needs full item state and next-tool
   parameters.
5. Prepare work: prefer `prepare_work`. It follows durable execution policy
   from `workflow.execution` and backlog item `execution_path`/`planning_gate`.
   It returns `direct_edit` guidance for manager-workspace work, or a
   `run_in_worktree` host action with an assignment bundle for worker handoff.
6. Complete direct work: for `direct_edit`, edit in the manager workspace and
   call `complete_backlog_item`.
7. Run worker externally: pass the generated bundle/worktree to Codex, Claude,
   or another harness.
8. Record worker activity: `start_worker_task`, `record_worker_progress`,
   `finish_work`, `run_task_verification`.
9. Inspect and verify worker output: `inspect_worktree_changes`,
   `record_verification_evidence`, `record_finding`, `validate_findings`.
10. Integrate and clean up: `integrate_worker_result`, `reconcile_project`,
   `worktree_cleanup`.

```mermaid
flowchart LR
    bootstrap["Bootstrap<br/>init and doctor"]
    backlog["Backlog<br/>decide, create, validate, list"]
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
- `request_planning_approval`: create a durable planning approval for a task
  plan or backlog tranche before non-direct work is dispatched.
- `create_backlog_item`: write one structured backlog item with the full
  authoring schema.
- `quick_create_backlog_item`: write one structured backlog item from compact
  common fields. It expands to the same canonical creation path as
  `create_backlog_item`.
- `create_backlog_items`: atomically write related backlog items in one call.
  Pass `preview=true` to allocate the same would-be IDs, validate the batch,
  and return exact paths plus generated markdown without writing files.
- `update_backlog_item`: update one existing backlog item through a typed
  patch. It validates before keeping the write, rolls back failed updates, and
  protects closed items unless `force_closed=true` is supplied intentionally.
- `create_epic`: write one structured backlog epic.
- `list_epics`: list existing backlog epics and their metadata.
- `validate_backlog`: validate backlog item and epic files.
  Its `next_action` is policy-aware: direct-only queues point to
  `prepare_work` and `complete_backlog_item`; worker-handoff queues keep the
  `commit_planning_artifacts` guidance needed before worktree dispatch.
- `list_backlog`: list runnable backlog candidates.
- `inspect_queue_status`: compact queue summary for dashboards and chat
  replies. It returns counts, top ready items, top blocked items, active tasks,
  state descriptions, and the recommended next tool without the full queue
  inventory.
- `inspect_work_queue`: inspect executable backlog work and the whole queue
  shape, including dependency-blocked and closed item summaries.
- `inspect_item`: inspect one backlog item with dependency, closure, task-plan,
  active lifecycle, findings, evidence, and recommended next-tool state.
- `inspect_backlog_inventory`: compatibility inventory view. Prefer
  `inspect_work_queue` for normal queue/status guidance and `inspect_item` for
  item-specific state.
- `inspect_dependency_graph`: inspect backlog dependency nodes, edges, roots,
  leaves, topological order, runnable nodes, closed nodes, blocked nodes,
  missing dependency references, and cycles. Use `focus_item_id` to return only
  a dependency neighborhood. The default limit is 200 nodes and the maximum is
  500; limited calls report `total`, `returned`, and `truncated`.

Backlog schema quick reference:

- Minimal `create_backlog_item` input: a meaningful `goal` or `title`.
  Platypus derives conservative title, goal, implementation contract, and first
  acceptance text when those fields are omitted.
- Rich `create_backlog_item` input: provide explicit `title`, `goal`,
  `implementation_contract` or `contract`, and `acceptance` when the work is
  complex or the generated defaults would be too broad.
- Compact `quick_create_backlog_item` input: provide the common fields
  `title`, `goal`, `priority`, `type`, `area`, `owned_surfaces`, and
  `acceptance`. The tool omits advanced fields such as external refs, notes,
  and explicit execution policy, then expands to the canonical item model.
- Use `create_backlog_items` for related items that should land together. It
  accepts per-item `client_key` values and resolves `depends_on_keys` to the
  created item IDs; if any item fails validation, no batch files are written.
- Use `create_backlog_items` with `preview=true` before writing a larger batch
  or when the host wants to show the user the exact target files and generated
  markdown first. Preview is non-mutating and returns `created: 0` with
  per-item `created: false`.
- Use `update_backlog_item` to correct or refine an existing item. Supported
  updates include title, priority, type, area, epic, dependencies, owned
  surfaces, external refs, execution path, planning gate, goal, implementation
  contract, acceptance criteria, and notes. Invalid updates are rolled back.
- Priority values: `P0`, `P1`, `P2`.
- Type values: `foundation`, `feature`, `safety`, `ux`, `test`, `docs`.
- Defaults when omitted: priority `P1`, type `feature`, epic `general`.
- Worker selection is runtime state. The manager or MCP host chooses the
  executor when preparing execution; Platypus does not create or configure
  agents.
- Failed backlog authoring calls return actionable `recovery_action` guidance for
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

- `write_task_plan`: write one reviewable plan to `backlog/plans/<ITEM>.yaml`.
- `validate_task_plan`: validate strict plan YAML, task IDs, dependencies,
  requirement references, owned surfaces, verification, and acceptance.
  Successful validation uses the same policy-aware next action as backlog
  validation, so direct items are not told to commit planning artifacts unless
  the host wants a checkpoint.
- `inspect_task_plan`: read one committed task plan.
- `list_task_plans`: list committed task plans.

Task plan YAML is planned work only. It must not contain runtime status,
completion commits, attempts, evidence, or worker diary fields. Runtime state
belongs in `.platy/platypus.sqlite3`, evidence records, task events, and Git
trailers.

### Task And Workspace Lifecycle

- `inspect_work_queue`: inspect runnable backlog candidates with task-plan
  state, active task counts, setup blockers, and inventory context for
  dependency-blocked or closed items. Queue states are structured:
  `direct_ready` means the host can edit in the manager workspace and complete
  with `complete_backlog_item`; `ready` means worker/worktree preparation is
  possible; `planning_blocked`, `approval_blocked`, `config_blocked`, and
  `workspace_blocked` identify distinct remedies; `active` and
  `completed_pending_integration` identify existing lifecycle state.
- `prepare_work`: preferred high-level host flow for executable backlog work.
  It inspects the queue, returns direct-edit guidance for
  `execution_path=direct_edit`, and prepares safe manual-handoff assignments
  for `execution_path=worker_handoff` once its `planning_gate` is satisfied.
  For direct work, the returned guidance is response-local: no task,
  assignment, event, worktree, or durable prepared marker is created. For worker
  handoff, the MCP server records state and creates worktrees; it does not
  launch Codex, Claude, or any other worker process.
  `prepared_state` is one of `direct_guidance`, `worktree_prepared`, or
  `not_prepared`. Host action kind is `direct_edit` for manager-workspace
  edits or `run_in_worktree` for worker handoff.
- `complete_backlog_item`: complete a direct host-work item without a worker
  task. It records direct completion evidence, a replayable backlog event, and
  can optionally create a closure commit for explicit `changed_files` with
  `commit=true`. The response includes `generated_evidence` IDs and summaries
  for records created by the completion call. Leave `record_auto_evidence`
  omitted to preserve default traceability, or set `record_auto_evidence=false`
  only when `evidence_refs` already point to explicit evidence records managed
  by the host. Use this for `prepare_work` host actions of kind `direct_edit`.
- `dispatch_ready_work`: lower-level batch dispatch for executable backlog
  work. It only accepts durable `worker_handoff` items whose planning gates are
  satisfied. It checks Git readiness, dispatches up to `max_tasks` runnable
  independent worker items, skips already-active items, and prepares worker
  assignments, worktrees, and bundles in one call.
  `execution_mode=manual_handoff` and the default `auto` mode both prepare a
  host-managed assignment worktree. Platypus does not launch or configure the
  worker that receives the handoff.
- `commit_planning_artifacts`: commit only Platypus-owned backlog, task-plan,
  epic, or optional scaffold artifacts before worktree handoff. It refuses to
  commit mixed source changes so worker dispatch cannot accidentally sweep user
  edits into a planning commit.
  Auto-commit of backlog artifacts is opt-in via
  `auto_commit_artifacts=true`.
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
- `inspect_integration_gates`: read-only preflight for one task. It reports the
  lifecycle, worktree, manager workspace, verification, findings, and branch
  change gates that decide whether `integrate_worker_result` can run.
- `integrate_worker_result`: merge, fast-forward, or squash a completed
  verified task branch into the manager workspace. Integration is blocked
  while required findings for the item/task remain undispositioned. Required
  findings can be accepted, deferred, resolved, rejected, or marked duplicate
  before integration.
- `generate_task_bundle`: generate a deterministic worker brief.

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
- `finish_work`: preferred high-level completion flow for worker assignments.
  It can infer changed files from the worktree diff, record verification
  evidence, record findings, optionally integrate when gates are satisfied, and
  return a structured `host_action` for the next step. If called for direct
  work without a task or assignment, it redirects the host to
  `complete_backlog_item`.
  `host_action.kind` is one of `verify_or_record_risk`, `resolve_findings`,
  `integrate_result`, `inspect_or_recover`, or `done`.
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
- `validate_findings`: fail when required findings remain open.
- `update_finding_disposition`: accept, defer, resolve, reject, or mark
  findings duplicate.

## Example: Guided Dispatch

```json
{
  "tool": "inspect_work_queue",
  "arguments": {}
}
```

When a backlog item is runnable, the result recommends `prepare_work`.
For direct items it returns manager-workspace edit guidance. For worker-handoff
items it may return task ids, assignment ids, worktree paths, and per-item
skipped or failed reasons in one response. After worker handoff, use
`start_worker_task` when you need an explicit running transition, then
`record_worker_progress` while
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
