# Current MCP Workflow

This document describes how a host such as Codex or Claude should use the
current Platypus MCP tools. The MCP server owns deterministic project state
transitions; the host owns conversation, model turns, and any external worker
execution.

See [diagrams.md](diagrams.md) for state, data, responsibility, and roadmap
diagrams from additional perspectives.

## Host Guidance Resources

MCP hosts should list resources and prompts during setup and cache the guidance
for the active session. The server exposes the same deterministic guidance as
both readable resources and prompts so clients can choose the integration style
that fits their UI.

Resources:

- `platypus://guidance/workflow`: end-to-end host workflow and safety gates.
- `platypus://guidance/spec-driven-development`: goal intake, backlog
  shaping, task planning, dispatch, evidence, and integration loop.
- `platypus://guidance/project-status`: project inspection and setup blockers.
- `platypus://guidance/tool-preload`: optional planning and execution startup
  tool groups for hosts that defer tool schemas.
- `platypus://guidance/backlog-authoring`: declarative backlog authoring rules.
- `platypus://guidance/worker-handoff`: worker dispatch, handoff, progress, and
  completion flow.
- `platypus://guidance/integration-review`: review and integration gates.
- `platypus://guidance/recovery`: inspection and recovery commands.

Equivalent prompts are available as `platypus-workflow`,
`platypus-spec-driven-development`, `platypus-project-status`,
`platypus-tool-preload`, `platypus-backlog-authoring`, `platypus-worker-handoff`,
`platypus-integration-review`, and `platypus-recovery`.

Tool preloading is optional and host-specific. Hosts that support deferred
schema preloading should read `platypus-tool-preload` at session start, load
the planning group for backlog/design/status work, and load the execution group
before dispatch, worker handoff, verification, integration, or recovery. Hosts
without preloading support should use the same tools normally as the workflow
requires them.

## Principles

- Start with `doctor_snapshot` or `inspect_status` when the project state is
  unclear.
- Prefer `inspect_work_queue` for executable backlog selection and
  `next_safe_action` over guessing the next lifecycle command.
- Keep backlog markdown declarative; runtime state belongs in
  `.platy/platypus.sqlite3`.
- Treat worker worktrees as isolated execution spaces until reviewed and
  integrated.
- Record findings and verification evidence instead of hiding limitations in
  chat.

## Bootstrap

1. For a fresh project, run `platypus-mcp bootstrap <host> --root <project>
   --init-project` so host MCP configuration and repository guidance files are
   created in one step.
2. For an already initialized project, run `bootstrap <host>` for host config
   only, or call `init_project` from the MCP host if scaffold files are
   missing.
3. Run `doctor_snapshot` to check config, backlog directories, Git metadata,
   and recovery guidance.
4. Configure manager and worker profiles with `configure_agent_profile`.
5. Inspect workflow policy with `inspect_workflow_config`.

`init_project` also installs project-local agent and workflow guidance. That is
intentional: once an MCP host enters an initialized directory, normal goal
requests should naturally flow through Platypus instead of ad hoc edits.
The transparent steering layer is deliberately layered: MCP server
instructions/resources/prompts are the stable contract, `AGENTS.md` provides
generic agent guidance, and `CLAUDE.md` provides Claude Code guidance. Other
host-specific files should be added only after their local instruction
mechanism is verified.

## Backlog Shaping

1. Use `plan_goal_work` for broad goals when the host needs read-only guidance.
   It classifies whether the next step should be direct scaffolding, a hybrid
   flow, or tracked Platypus workflow, and returns concrete next tool
   arguments without changing project state.
   - Pass `intent=planning_only` when the user is asking to plan, design, shape
     backlog items, or review options without starting execution.
   - Pass `intent=ready_to_execute` when the user has approved creating
     tracking and preparing dispatch.
   - Omit `intent` or pass `auto` when Platypus should infer from goal text.
2. If the host wants Platypus intake in one call, use `start_goal_work`.
   - `start_goal_work` + `dispatch=true` runs tracked dispatch/worktree flow.
   - `start_goal_work` + `dispatch=false` creates or reuses a tracking anchor
     and leaves baseline file edits to the host in the manager workspace. Use
     this only when direct manager-workspace edits are intentional.
3. Use the host model to create concrete backlog items with
   `create_backlog_item` or `create_backlog_items` when the classifier
   recommends `platypus_workflow` or `hybrid`. `draft_backlog_items` is
   optional and skips unless MCP sampling is available.
4. Persist selected work with `create_backlog_items` when related items should
   be created atomically, or `create_backlog_item` for a single item.
5. Run `validate_backlog`.
6. Use `list_backlog` to see runnable candidates.
7. Use `inspect_backlog_inventory` when the runnable queue is empty or unclear;
   it explains closed items from Git trailers and blocked items from open
   dependencies without requiring hosts to read markdown directly.
8. Use `inspect_work_queue` to combine runnable candidates, active task state,
   task-plan state, worker-profile readiness, and the recommended next tool.
   When runnable work exists but no ready worker profile is configured, follow
   the returned `configure_agent_profile` parameters or explicitly use
   `dispatch_ready_work` as a manual handoff path.

Backlog files should contain goal, implementation contract, acceptance
criteria, dependencies, and owned surfaces. They should not contain runtime
status, task attempts, PR metadata, or closure state.

When creating backlog items through tools, use the typed schema:

- minimal input: a meaningful `goal` or `title`; Platypus derives conservative
  defaults for missing title, goal, implementation contract, and first
  acceptance criterion
- rich input: explicit `title`, `goal`, `implementation_contract` or
  `contract`, and `acceptance` criteria when the work is complex or generated
  defaults would be too broad
- priority values: `P0`, `P1`, `P2`
- type values: `foundation`, `feature`, `safety`, `ux`, `test`, `docs`
- default values when omitted: priority `P1`, type `feature`, epic `general`,
  suggested worker `coder`
- `suggested_worker` names a Platypus worker profile, not a host-specific
  subagent type
- failed authoring calls include a specific `next_action`; follow it before
  retrying instead of falling back to manual markdown unless the recovery
  explicitly asks for a file edit
- use `list_epics` before assigning a non-default epic, and `create_epic` when
  a new grouping is needed
- use `client_key` plus `depends_on_keys` in `create_backlog_items` for
  dependencies between newly created items; the tool resolves those keys to
  concrete backlog IDs before writing files

Backlog items may include `external_refs` for intake and reporting surfaces
such as GitHub, Linear, Jira, GitLab, support tickets, specs, or local design
documents. These references are metadata only: the local backlog item remains
the executable snapshot for planning and dispatch. Each reference records a
provider, kind, stable id, and either a URL or locator, with optional import
timestamp and source hash.

## External Reporting

External trackers are never mutated implicitly. When a backlog item carries an
external reference, use:

1. `draft_external_report` to create a provider-neutral report from local
   backlog, task, and evidence state.
2. `request_external_report_approval` to make the intended external mutation
   explicit and durable.
3. Have the MCP host or approved provider plugin send the report after approval.
4. `record_external_report_dispatch` to record the send result, outbound
   reference, provider error if any, and external-report evidence.

This keeps provider credentials outside Platypus state while preserving an
auditable local record of what was approved and reported.

## Task Planning

Use `classify_planning_needs` or `inspect_work_queue` to determine whether an
item needs further planning. `direct` items can be dispatched from the backlog
contract alone. `standard` and `full` items require a committed task plan before
dispatch. Use `draft_task_plan` to produce a starting point, `write_task_plan`
to persist it under `backlog/plans/<ITEM>.yaml`, and `validate_task_plan`
before treating it as executable.

The first policy is deterministic and conservative:

- `direct`: small low-risk docs/test/single-surface work.
- `standard`: feature, foundation, safety, or UX work; multiple owned surfaces;
  MCP schema/server changes; or workflow changes.
- `full`: storage, worktree, approval, assignment, runner, worker, dispatch,
  reconciliation, security, protocol, daemon, network, migration, or
  architecture-sensitive work.

Task plans are strict YAML artifacts. They may contain requirements, a design
summary, owned surfaces, verification commands, and executable planned tasks.
They must not contain runtime fields such as status, completed_at, commit,
attempts, result, evidence, or worker diary comments. Use `inspect_task_plan`
and `list_task_plans` to review committed plans.

For review-sensitive work, request planning approval before dispatch:

1. Call `request_planning_approval` with one `item_id` for a task plan, or
   multiple `item_ids` for a backlog tranche.
2. Respond with `approval_respond`.
3. Call `inspect_work_queue` or `dispatch_ready_work` with
   `require_planning_approval=true`.

Planning approval records live in local approval state, not backlog item
frontmatter. `reconcile_project` reports planned task lifecycles that were
created without an approved planning gate.

## Dispatch And Worker Handoff

1. Call `inspect_work_queue`.
2. If it requires a task plan, use the recommended task-plan tool first.
3. If policy requires reviewed planning, call `request_planning_approval`,
   approve it, and pass `require_planning_approval=true` to queue or dispatch
   tools.
4. Call `next_safe_action`.
5. Choose execution mode:
   - `profiled_worker` requires at least one ready worker profile configured
     with `configure_agent_profile`.
   - `manual_handoff` prepares the same task, assignment, worktree, and bundle
     without requiring a worker profile because the MCP host or a human-managed
     worker will execute it externally.
   - `auto` is the default and behaves like `profiled_worker` unless
     `manual_handoff` is explicitly requested.
6. For normal execution, call `dispatch_ready_work`; it dispatches one or more
   runnable items and prepares assignments, worktrees, and bundles. Pass
   `execution_mode=manual_handoff` when no configured profile should be used.
7. For low-level debugging only, call `dispatch_next_work` and then immediately
   `prepare_worker_handoff`; do not run a worker from a bare task id.
8. Give the assignment bundle and worktree path to the worker harness.
9. Mark the worker active with `start_worker_task` when you need an explicit
   running transition.

The worker should operate in the assigned worktree, not in the manager
workspace. Manual handoff is still lifecycle-tracked: after the host or
external worker edits the worktree, call `start_worker_task` if a running
transition is needed, then `complete_worker_task`, record verification evidence,
and integrate with `integrate_worker_result`.

## Worker Progress And Result

Use `record_worker_progress` for safe progress summaries. Use
`inspect_worktree_changes` to review bounded worktree changes. Finish with
`complete_worker_task`, including:

- terminal status
- summary
- changed files
- verification status

For same-session host-driven edits, `complete_worker_task` can complete a
prepared assignment directly; it auto-starts prepared assignments by default.
Set `auto_start_if_prepared=false` when strict lifecycle enforcement is needed.

When a verification command is configured in the assignment bundle, run
`run_task_verification` after completion to execute it in the task worktree and
persist verification run evidence.

If verification did not pass, record explicit verification evidence or findings
so reconciliation can report the remaining gap.

## Evidence, Findings, And Reconciliation

- Use `record_verification_evidence` for verification results.
- Use `record_finding` for limitations or required follow-up work.
- Use `validate_findings` before claiming a task is handled.
- Use `integrate_worker_result` to bring a completed verified worktree back
  into the manager workspace according to `workflow.integration`.
- Use `reconcile_project` to find missing verification evidence, unresolved
  findings, closure gaps, and evidence attached to missing or incomplete task
  lifecycles.

## Managed Integration

Managed integration from task worktree back into the manager workspace is
handled by `integrate_worker_result`. The tool requires a completed task, a
recorded worktree, a clean manager workspace, and verification evidence by
default.

The tool uses `workflow.integration.merge_style` from `platy.yaml` and creates
closure commits with `Platypus-Closes` and `Platypus-Verification` trailers.
If integration cannot proceed, the tool returns structured recovery guidance.
