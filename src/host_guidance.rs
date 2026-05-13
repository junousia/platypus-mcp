use rmcp::model::{AnnotateAble, Prompt, PromptMessage, PromptMessageRole, RawResource, Resource};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GuidanceEntry {
    pub name: &'static str,
    pub uri: &'static str,
    pub title: &'static str,
    pub description: &'static str,
    pub text: &'static str,
}

pub const WORKFLOW_URI: &str = "platypus://guidance/workflow";
pub const SPEC_DRIVEN_URI: &str = "platypus://guidance/spec-driven-development";
pub const PROJECT_STATUS_URI: &str = "platypus://guidance/project-status";
pub const TOOL_PRELOAD_URI: &str = "platypus://guidance/tool-preload";
pub const BACKLOG_AUTHORING_URI: &str = "platypus://guidance/backlog-authoring";
pub const WORKER_HANDOFF_URI: &str = "platypus://guidance/worker-handoff";
pub const INTEGRATION_REVIEW_URI: &str = "platypus://guidance/integration-review";
pub const RECOVERY_URI: &str = "platypus://guidance/recovery";

const WORKFLOW_TEXT: &str = r#"# Platypus Host Workflow

Use Platypus MCP tools for deterministic project state. The MCP host owns chat,
model turns, and external worker execution.

If the host supports deferred schema preloading, first read
`platypus://guidance/tool-preload` or the `platypus-tool-preload` prompt and
load the planning startup group.

## Exact Decision Table

Read this table top to bottom. The first matching state wins. Do not skip the
detecting tool and do not infer hidden state from chat history.

| State | Detect with | Match condition | Required next action | Exit condition |
| --- | --- | --- | --- | --- |
| `unknown` | session start | state was not freshly inspected | call `inspect_session` | setup, project status, workflow config, and queue facts are known |
| `needs_scaffold` | `doctor_snapshot` | scaffold files are missing | call `init_project`, then `doctor_snapshot` | scaffold blockers are gone |
| `empty_backlog` | `inspect_work_queue` | no backlog items exist | host model chooses concrete items; call `create_backlog_items`, then `validate_backlog` | backlog validates and queue is inspected again |
| `dependency_blocked` | `inspect_work_queue` | `inventory.dependency_blocked_count > 0` and no runnable item is selected | call `inspect_item` on the first blocked item; close or create required dependencies | blocked dependencies are resolved |
| `plan_missing` | `inspect_work_queue` | an item recommends `write_task_plan` | host model writes an explicit plan with `write_task_plan`, then calls `validate_task_plan` | task plan validates cleanly |
| `approval_blocked` | `inspect_work_queue` | `queue_state == "approval_blocked"` | call `request_planning_approval`, then `approval_respond` | planning approval is recorded |
| `config_blocked` | `inspect_work_queue` | `queue_state == "config_blocked"` | call `doctor_snapshot` and follow the reported recovery action | setup blocker is resolved |
| `workspace_blocked` | `inspect_work_queue` | `queue_state == "workspace_blocked"` | commit, stash, or finish current manager-workspace changes | worker dispatch can safely create a worktree |
| `direct_ready` | `inspect_work_queue` | `queue_state == "direct_ready"` | call `prepare_work`, edit the manager workspace, then call `complete_backlog_item` | direct completion evidence or closure commit exists |
| `worker_ready` | `inspect_work_queue` | `queue_state == "ready"` | call `prepare_work` for one item or `dispatch_ready_work` for a batch | `run_in_worktree` handoff exists |
| `worker_active` | `inspect_task` or `inspect_work_queue` | task is active or prepared | run the external worker in the assigned worktree; call `finish_work` | `finish_work.host_action` is returned |
| `pending_integration` | `inspect_work_queue` or `inspect_integration_gates` | `queue_state == "completed_pending_integration"` or gates are ready | call `inspect_integration_gates`, then `integrate_worker_result`, then `reconcile_project` | work is integrated or a specific blocker is reported |
| `failed_or_unclear` | any tool result | `status == "failed"` or `recovery_action` is present | follow `recovery_action`; if still unclear call `inspect_session` | a known state above matches |

## Lifecycle Output States

Every structured state below has a single meaning. Hosts should render the
state name and follow the paired tool instead of inventing a hidden lifecycle.

| Output | Emitted by | Meaning | Follow-up |
| --- | --- | --- | --- |
| `direct_ready` | `inspect_work_queue`, `inspect_queue_status`, `inspect_item` | direct manager-workspace work is executable | `prepare_work`, edit, `complete_backlog_item` |
| `ready` | `inspect_work_queue`, `inspect_queue_status`, `inspect_item` | worker handoff can be prepared | `prepare_work` or `dispatch_ready_work` |
| `planning_blocked` | queue tools | a required task plan is missing or invalid | `write_task_plan`, then `validate_task_plan` |
| `approval_blocked` | queue tools | planning approval is required before execution | `request_planning_approval`, then `approval_respond` |
| `dependency_blocked` | queue tools | backlog dependencies are still open | `inspect_item` and close or create the dependencies |
| `config_blocked` | queue tools | project setup blocks worktree dispatch | `doctor_snapshot` and the reported recovery action |
| `workspace_blocked` | queue tools | manager workspace changes block worker dispatch | commit, stash, or finish the current manager-workspace change |
| `active` | queue tools | a task lifecycle already exists | `inspect_task`, `inspect_task_events`, or the recommended active-task tool |
| `completed_pending_integration` | queue tools | worker output is complete and waiting for integration | `inspect_integration_gates`, then `integrate_worker_result` |
| `direct_guidance` | `prepare_work.prepared_state` | response-local direct-edit guidance; no task, assignment, event, or worktree was created | edit manager workspace and call `complete_backlog_item` |
| `worktree_prepared` | `prepare_work.prepared_state` | durable worker handoff state exists with assignment and worktree | run the external worker, then `finish_work` |
| `not_prepared` | `prepare_work.prepared_state` | no selected item could be prepared | follow `next_action` or inspect the queue |
| `direct_edit` | `prepare_work.host_actions[].kind` | host should edit the manager workspace | `complete_backlog_item` |
| `run_in_worktree` | `prepare_work.host_actions[].kind` | host or external harness should work in the assigned worktree | `finish_work` |
| `verify_or_record_risk` | `finish_work.host_action.kind` | verification is missing, failed, or explicitly waived | `run_task_verification`, `record_verification_evidence`, or `record_finding` |
| `resolve_findings` | `finish_work.host_action.kind` | findings must be recorded or dispositioned | `record_finding`, `validate_findings`, `update_finding_disposition` |
| `integrate_result` | `finish_work.host_action.kind` | worker result is ready for integration review | `inspect_integration_gates`, then `integrate_worker_result` |
| `inspect_or_recover` | lifecycle tools | the tool cannot safely continue without inspection | follow `recovery_action`, then inspect again |
| `done` | completion tools | direct or worker work is complete | `reconcile_project`, then inspect the queue |

`inspect_session` may replace separate startup calls to `doctor_snapshot`,
`inspect_status`, `inspect_workflow_config`, `inspect_queue_status`, and
`inspect_work_queue` when it succeeds in a fresh session. Call the narrower
tools after mutations, when a detailed payload is needed, or when the session
snapshot is stale.

Safety gates: keep runtime state in `.platy/platypus.sqlite3`, keep backlog
markdown declarative, operate workers in task worktrees, and do not bypass
verification evidence before integration.
"#;

const SPEC_DRIVEN_TEXT: &str = r#"# Spec-Driven Development Guidance

Platypus should make structured development the easy path for any MCP-enabled
coding host. When the user gives a goal such as "build a web app", do not jump
straight to broad edits. Convert the goal into a controlled loop:

1. Inspect the project with `inspect_session`.
2. Decide the workflow strategy with model judgment and user intent. The MCP
   server supplies facts, schemas, validation, and state transitions; it does
   not classify broad goals.
3. For tiny scaffolds, the host may edit directly after user approval. No
   Platypus scaffold tool is involved: use the host's native file edits or
   scaffold command, commit the baseline, then return to Platypus for follow-up
   backlog work when tracking is useful.
4. For tracked control, create a small concrete backlog set with
   `create_backlog_item` or `create_backlog_items`. Keep items independently
   reviewable and executable.
5. Run `validate_backlog` after backlog writes. Its next action follows
   explicit execution policy: direct work continues through `prepare_work` and
   `complete_backlog_item`; worker handoff keeps planning-commit guidance when
   needed for worktree dispatch.
6. Use `inspect_queue_status` for compact queue counts and top ready/blocked
   work. Use `inspect_work_queue` when full readiness, task-plan state, active
   work, setup blockers, and next-tool parameters are needed.
7. When planning is required by the user or team, create a strict task plan
   with `write_task_plan` and `validate_task_plan`.
8. Prepare execution with `prepare_work`. Direct items may return a
   `direct_edit` host action; that direct action is response-local guidance,
   not persisted preparation state. Complete those with
   `complete_backlog_item`.
   Standard/full items return a `run_in_worktree`
   action with a worker assignment bundle; the MCP server does not launch the
   external worker.
9. Record progress when useful, then finish worker assignments with `finish_work`. Workers should
   report changed files, verification status, acceptance coverage, and findings
   or explicitly set `findings_reviewed=true`.
10. Follow the `finish_work.host_action`: verify, record/resolve findings,
    integrate with `integrate_worker_result`, recover, run
    `reconcile_project`, or move to the next item.

Minimum viable direct-edit loop for tiny, user-approved work:
`inspect_session`, `inspect_queue_status` or `inspect_work_queue`,
`prepare_work`, edit the manager workspace, run relevant verification, then
`complete_backlog_item`. This is a traceability tradeoff: it avoids worktree
overhead for small tasks but still records the durable completion. Use task
plans, worker handoff, findings, and integration gates for long-lived or
parallel product development.

The host should present this as natural assistance, not as a manual ceremony:
explain what is being structured, ask for approval only when choices matter,
and use tool results as the source of truth. Backlog markdown and task-plan
YAML are planning artifacts only; runtime state belongs in Platypus state,
events, evidence, findings, and Git trailers.
"#;

const PROJECT_STATUS_TEXT: &str = r#"# Project Status Guidance

Use status tools before making assumptions about the repository or task queue.

- `inspect_session` is the preferred startup tool. It combines setup checks,
  project status, workflow config, and queue state in one read-only snapshot.
  When it succeeds at session start, it replaces separate startup detector
  calls to `doctor_snapshot`, `inspect_status`, `inspect_workflow_config`,
  `inspect_queue_status`, and `inspect_work_queue` unless a detailed payload
  is needed.
- `doctor_snapshot` checks scaffold files, Git metadata, backlog directories,
  and recovery guidance.
- `inspect_status` summarizes project shape, backlog counts, and runnable work.
- `inspect_workflow_config` reports integration policy such as merge style and
  verification gates.
- `inspect_queue_status` returns compact queue counts, top ready items, top
  blocked items, active tasks, and one-line queue-state descriptions.
- `inspect_work_queue` combines runnable backlog candidates with active task
  counts, explicit task-plan requirements, and task-plan readiness.
- `inspect_work_queue` returns the current queue shape, direct/worktree
  execution path, and the recommended next tool call.

Report setup blockers directly from structured tool output. Do not invent
missing state from chat context.
"#;

const TOOL_PRELOAD_TEXT: &str = r#"# Tool Preload Guidance

Some MCP hosts defer tool schemas until a tool is discovered, searched, or
selected. Platypus does not require any specific preload mechanism, but hosts
that support one should load the common groups below to reduce planning and
execution round trips.

Preloading is optional and host-specific. If the host cannot preload schemas,
continue normally and call the tools as needed. Do not depend on hidden client
context for core state transitions.

## Planning Startup Group

Load this group at the start of planning, backlog shaping, or project-status
sessions:

- `doctor_snapshot`
- `inspect_status`
- `inspect_workflow_config`
- `inspect_session`
- `create_backlog_item`
- `create_backlog_items`
- `create_epic`
- `validate_backlog`
- `list_backlog`
- `inspect_work_queue`
- `inspect_item`
- `write_task_plan`
- `validate_task_plan`
- `inspect_task_plan`
- `request_planning_approval`
- `approval_respond`

## Execution Startup Group

Load this group before dispatch, worker handoff, verification, integration, or
recovery sessions:

- `inspect_work_queue`
- `inspect_session`
- `prepare_work`
- `dispatch_ready_work`
- `commit_planning_artifacts`
- `generate_task_bundle`
- `inspect_task_events`
- `events_replay`
- `worktree_status`
- `inspect_worktree_changes`
- `send_worker_guidance`
- `start_worker_task`
- `record_worker_progress`
- `complete_worker_task`
- `finish_work`
- `complete_backlog_item`
- `run_task_verification`
- `record_verification_evidence`
- `record_finding`
- `validate_findings`
- `inspect_integration_gates`
- `integrate_worker_result`
- `worktree_cleanup`
- `reconcile_project`

Hosts may load additional tools when a user asks for lower-level control, but
these two groups cover the normal long-term product development loop.
"#;

const BACKLOG_AUTHORING_TEXT: &str = r#"# Backlog Authoring Guidance

Create concise, agent-readable backlog items with `create_backlog_item` for one
item or `create_backlog_items` for an atomic related set. Use the host model's
own judgment to choose item boundaries and call the write tools directly.
Validate with `validate_backlog`, inspect compact queue shape with
`inspect_queue_status`, inspect full queue routing with `inspect_work_queue`,
and use `inspect_item` when the host needs the full state for one closed,
blocked, active, or runnable item. `inspect_backlog_inventory` remains a
compatibility tool; normal flows should not need it.

Backlog markdown should contain goal, implementation contract, acceptance
criteria, dependencies, explicit execution policy when needed, and owned
surfaces. It should not
contain runtime status, assignment attempts, task IDs, PR metadata, closure
state, or blocked/done fields. Closure is derived from Git trailers such as
`Platypus-Closes` and `Platypus-Verification`, or from recorded direct
completion events created by `complete_backlog_item`.

Backlog schema quick reference:

- Minimal create input: a meaningful `goal` or `title`. Platypus derives
  conservative title, goal, implementation contract, and first acceptance text
  when those fields are omitted.
- Rich create input: provide explicit `title`, `goal`,
  `implementation_contract` or `contract`, and `acceptance` when the work is
  complex or the defaults would be too broad.
- Priorities: `P0` critical/next, `P1` normal important work, `P2` refinement.
- Types: `foundation`, `feature`, `safety`, `ux`, `test`, `docs`.
- Defaults: priority `P1`, type `feature`, epic `general`.
- Worker selection is runtime state. The manager or MCP host chooses the
  executor when preparing execution; Platypus does not create or configure
  agents.
- For related items, prefer `create_backlog_items`. Give each item a
  `client_key` and use `depends_on_keys` to reference other items in the same
  batch before their IDs are known. The batch is all-or-nothing.
- Use `list_epics` before assigning a non-default epic. Use `create_epic` to
  add a missing grouping; do not hand-write epic files unless a recovery step
  explicitly asks for manual file edits.

There is no manual `backlog/index.md`; hosts should compute queue state through
`list_backlog`, `inspect_work_queue`, and `inspect_status`.

For non-trivial items, use the host model to write a concrete plan directly
with `write_task_plan`. Plan YAML is strict and reviewable, but still not
runtime state.

Execution policy is deterministic and durable. Use `workflow.execution` for
project defaults and backlog item frontmatter `execution_path` plus
`planning_gate` for item overrides. `direct_edit` items finish through
`complete_backlog_item`. `worker_handoff` items use `prepare_work` or
`dispatch_ready_work` after their `none`, `task_plan`, or
`approved_task_plan` gate is satisfied.
"#;

const WORKER_HANDOFF_TEXT: &str = r#"# Worker Handoff Guidance

Workers run outside the MCP server. Platypus prepares and records their
execution state.

1. Prefer `prepare_work` for normal host-run execution. It inspects the queue,
   returns direct-edit guidance for `execution_path=direct_edit`, and prepares
   assignment bundles plus worktrees for `execution_path=worker_handoff`.
2. Use `dispatch_ready_work` only when the host needs lower-level batch
   dispatch control for worker-handoff items. `manual_handoff` means the MCP
   host or a human-managed worker will execute the returned assignment;
   Platypus does not launch or configure that worker.
3. For the lowest-level control, use `dispatch_next_work` followed immediately by
   `prepare_worker_handoff`; do not run an external worker from a task id
   without an assignment.
4. Give the bundle, completion contract, and worktree path to the selected
   worker harness.
5. After the external worker harness actually starts, record that lifecycle
   transition with `start_worker_task` when you need an explicit running state.
   This tool does not launch a worker.
6. Persist safe progress with `record_worker_progress`.
7. Finish direct-edit host actions with `complete_backlog_item`. Finish
   worker assignments with `finish_work` so completion, changed files,
   verification evidence, findings, and integration guidance remain one
   connected result.
   Use `complete_worker_task` only when you need low-level completion control.

For same-session host-driven edits, `finish_work` can finish a prepared
assignment directly; it auto-starts prepared assignments by default. Set
`auto_start_if_prepared=false` when strict running-only completion is required.
For direct manager-workspace edits returned by `prepare_work`, do not call
`finish_work`; call `complete_backlog_item`.

Do not run worker edits in the manager workspace. Use `send_worker_guidance`
for steering active work.
"#;

const INTEGRATION_REVIEW_TEXT: &str = r#"# Integration Review Guidance

Before integrating worker output, verify that the task is completed, the
manager workspace is clean, the worktree changes are understood, and
verification evidence is recorded.

Use `inspect_task`, `inspect_task_events`, `inspect_worktree_changes`, and
`list_evidence` for review. Use `record_verification_evidence` when validation
has been run or explicitly skipped with rationale.

Integrate with `integrate_worker_result`. It follows `workflow.integration` in
`platy.yaml` and records `Platypus-Closes` and `Platypus-Verification`
trailers in Git. After integration, call `reconcile_project` and clean safe
worktrees with `worktree_cleanup`.
"#;

const RECOVERY_TEXT: &str = r#"# Recovery Guidance

When the host is unsure what happened, prefer inspection before mutation.

- `doctor_snapshot` reports setup issues and recovery guidance.
- `inspect_work_queue` recommends the next queue or lifecycle tool.
- `events_replay` and `inspect_task_events` replay durable activity.
- `approval_list` and `approval_respond` handle durable approvals.
- `list_findings`, `validate_findings`, and `update_finding_disposition` show
  open follow-up obligations and explicit dispositions.
- `inspect_integration_gates` explains why a completed task is or is not ready
  for `integrate_worker_result`.
- `reconcile_project` reports missing verification, integration, findings, and
  closure evidence.

If a tool fails, return its structured `status`, `summary`, `error`, and
`next_action` to the user instead of guessing or silently retrying.
"#;

pub const GUIDANCE: &[GuidanceEntry] = &[
    GuidanceEntry {
        name: "platypus-workflow",
        uri: WORKFLOW_URI,
        title: "Platypus Workflow",
        description: "Recommended host workflow and safety gates.",
        text: WORKFLOW_TEXT,
    },
    GuidanceEntry {
        name: "platypus-spec-driven-development",
        uri: SPEC_DRIVEN_URI,
        title: "Spec-Driven Development",
        description: "How hosts turn free-form goals into controlled Platypus work.",
        text: SPEC_DRIVEN_TEXT,
    },
    GuidanceEntry {
        name: "platypus-project-status",
        uri: PROJECT_STATUS_URI,
        title: "Project Status",
        description: "How hosts should inspect project state and setup blockers.",
        text: PROJECT_STATUS_TEXT,
    },
    GuidanceEntry {
        name: "platypus-tool-preload",
        uri: TOOL_PRELOAD_URI,
        title: "Tool Preload",
        description: "Common planning and execution tool groups for hosts with deferred schemas.",
        text: TOOL_PRELOAD_TEXT,
    },
    GuidanceEntry {
        name: "platypus-backlog-authoring",
        uri: BACKLOG_AUTHORING_URI,
        title: "Backlog Authoring",
        description: "How hosts should create and validate declarative backlog items.",
        text: BACKLOG_AUTHORING_TEXT,
    },
    GuidanceEntry {
        name: "platypus-worker-handoff",
        uri: WORKER_HANDOFF_URI,
        title: "Worker Handoff",
        description: "How to dispatch, hand off, track, and complete worker tasks.",
        text: WORKER_HANDOFF_TEXT,
    },
    GuidanceEntry {
        name: "platypus-integration-review",
        uri: INTEGRATION_REVIEW_URI,
        title: "Integration Review",
        description: "Required checks before integrating completed worker output.",
        text: INTEGRATION_REVIEW_TEXT,
    },
    GuidanceEntry {
        name: "platypus-recovery",
        uri: RECOVERY_URI,
        title: "Recovery",
        description: "Inspection and recovery tools for uncertain project state.",
        text: RECOVERY_TEXT,
    },
];

pub fn resource_list() -> Vec<Resource> {
    GUIDANCE
        .iter()
        .map(|entry| {
            let mut raw = RawResource::new(entry.uri, entry.name);
            raw.title = Some(entry.title.to_string());
            raw.description = Some(entry.description.to_string());
            raw.mime_type = Some("text/markdown".to_string());
            raw.size = Some(entry.text.len() as u32);
            raw.no_annotation()
        })
        .collect()
}

pub fn prompt_list() -> Vec<Prompt> {
    GUIDANCE
        .iter()
        .map(|entry| {
            let mut prompt = Prompt::new(entry.name, Some(entry.description), None);
            prompt.title = Some(entry.title.to_string());
            prompt
        })
        .collect()
}

pub fn by_uri(uri: &str) -> Option<&'static GuidanceEntry> {
    GUIDANCE.iter().find(|entry| entry.uri == uri)
}

pub fn by_prompt_name(name: &str) -> Option<&'static GuidanceEntry> {
    GUIDANCE.iter().find(|entry| entry.name == name)
}

pub fn prompt_messages(entry: &GuidanceEntry) -> Vec<PromptMessage> {
    vec![PromptMessage::new_text(PromptMessageRole::User, entry.text)]
}
