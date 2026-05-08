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
- `platypus://guidance/project-status`: project inspection and setup blockers.
- `platypus://guidance/backlog-authoring`: declarative backlog authoring rules.
- `platypus://guidance/worker-handoff`: worker dispatch, handoff, progress, and
  completion flow.
- `platypus://guidance/integration-review`: review and integration gates.
- `platypus://guidance/recovery`: inspection and recovery commands.

Equivalent prompts are available as `platypus-workflow`,
`platypus-project-status`, `platypus-backlog-authoring`,
`platypus-worker-handoff`, `platypus-integration-review`, and
`platypus-recovery`.

## Principles

- Start with `doctor_snapshot` or `inspect_status` when the project state is
  unclear.
- Prefer `next_safe_action` over guessing the next lifecycle command.
- Keep backlog markdown declarative; runtime state belongs in
  `.platy/platypus.sqlite3`.
- Treat worker worktrees as isolated execution spaces until reviewed and
  integrated.
- Record findings and verification evidence instead of hiding limitations in
  chat.

## Bootstrap

1. Run `init_project` for missing scaffold files.
2. Run `doctor_snapshot` to check config, backlog directories, Git metadata,
   and recovery guidance.
3. Configure manager and worker profiles with `configure_agent_profile`.
4. Inspect workflow policy with `inspect_workflow_config`.

## Backlog Shaping

1. Use `draft_backlog_items` for a deterministic first pass from a goal.
2. Persist selected work with `create_backlog_item`.
3. Run `validate_backlog`.
4. Use `list_backlog` to see runnable candidates.

Backlog files should contain goal, implementation contract, acceptance
criteria, dependencies, and owned surfaces. They should not contain runtime
status, task attempts, PR metadata, or closure state.

## Task Planning

For non-trivial items, create a committed task plan before dispatch. Use
`draft_task_plan` to produce a starting point, `write_task_plan` to persist it
under `backlog/plans/<ITEM>.yaml`, and `validate_task_plan` before treating it
as executable.

Task plans are strict YAML artifacts. They may contain requirements, a design
summary, owned surfaces, verification commands, and executable planned tasks.
They must not contain runtime fields such as status, completed_at, commit,
attempts, result, evidence, or worker diary comments. Use `inspect_task_plan`
and `list_task_plans` to review committed plans.

## Dispatch And Worker Handoff

1. Call `next_safe_action`.
2. If it recommends `dispatch_next_work`, dispatch the next runnable item.
3. Call `next_safe_action` again.
4. If it recommends `prepare_worker_handoff`, prepare the persisted assignment.
5. Give the assignment bundle and worktree path to the worker harness.
6. Mark the worker active with `start_worker_task`.

The worker should operate in the assigned worktree, not in the manager
workspace.

## Worker Progress And Result

Use `record_worker_progress` for safe progress summaries. Use
`inspect_worktree_changes` to review bounded worktree changes. Finish with
`complete_worker_task`, including:

- terminal status
- summary
- changed files
- verification status

If verification did not pass, record explicit verification evidence or findings
so reconciliation can report the remaining gap.

## Evidence, Findings, And Reconciliation

- Use `record_verification_evidence` for verification results.
- Use `record_finding` for limitations or required follow-up work.
- Use `validate_findings` before claiming a task is handled.
- Use `integrate_worker_result` to bring a completed verified worktree back
  into the manager workspace according to `workflow.integration`.
- Use `reconcile_project` to find missing verification evidence, unresolved
  findings, and closure gaps.

## Managed Integration

Managed integration from task worktree back into the manager workspace is
handled by `integrate_worker_result`. The tool requires a completed task, a
recorded worktree, a clean manager workspace, and verification evidence by
default.

The tool uses `workflow.integration.merge_style` from `platy.yaml` and creates
closure commits with `Platypus-Closes` and `Platypus-Verification` trailers.
If integration cannot proceed, the tool returns structured recovery guidance.
