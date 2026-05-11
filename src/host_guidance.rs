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

1. Inspect setup with `doctor_snapshot`, `inspect_status`, and
   `inspect_workflow_config`.
   If the host supports deferred schema preloading, first read
   `platypus://guidance/tool-preload` or the `platypus-tool-preload` prompt
   and load the planning startup group.
2. For broad user goals, call `plan_goal_work` when you need read-only
   guidance. Use `intent=planning_only` for planning, design, or backlog
   shaping conversations. Use `intent=ready_to_execute` only when the user has
   approved creating tracking and optionally dispatching tasks.
3. Shape advanced/manual work by using the host model to call
   `create_backlog_item` directly, then `validate_backlog`, `list_backlog`,
   and `inspect_backlog_inventory`. Use `draft_backlog_items` only when the MCP
   client supports sampling.
4. Use `inspect_work_queue` or `classify_planning_needs` to choose executable
   backlog work and understand direct, standard, or full planning needs.
5. For non-trivial work, create a committed executable plan with
   `write_task_plan`, `validate_task_plan`, `inspect_task_plan`, and
   `list_task_plans`. Use `draft_task_plan` only when MCP sampling is
   available; otherwise the host model should write the plan directly.
6. Ask `next_safe_action` before advancing lifecycle state.
7. Prepare execution with `prepare_work`. It returns a host action:
   `direct_edit` for lightweight manager-workspace edits, or `run_in_worktree`
   with an assignment bundle and worktree for host-run worker execution.
   `dispatch_ready_work`, `dispatch_next_work`, and `prepare_worker_handoff`
   remain low-level lifecycle controls.
8. Start and track active workers with `start_worker_task` and
   `record_worker_progress` when useful. Finish with `finish_work` so worker
   completion, changed files, verification evidence, findings, integration
   guidance, and reconciliation stay connected.
   Follow its `host_action` to call `integrate_worker_result`,
   `reconcile_project`, verification, or recovery tools as needed.

Safety gates: keep runtime state in `.platy/platypus.sqlite3`, keep backlog
markdown declarative, operate workers in task worktrees, and do not bypass
verification evidence before integration.
"#;

const SPEC_DRIVEN_TEXT: &str = r#"# Spec-Driven Development Guidance

Platypus should make structured development the easy path for any MCP-enabled
coding host. When the user gives a goal such as "build a web app", do not jump
straight to broad edits. Convert the goal into a controlled loop:

1. Inspect the project with `doctor_snapshot`, `inspect_status`, and
   `next_safe_action`.
2. Use `plan_goal_work` for broad user goals when you need read-only guidance.
   It wraps `classify_workflow_fit` and returns concrete next-tool arguments
   without changing project state. Pass `intent=planning_only` when the user is
   still shaping backlog or design, and `intent=ready_to_execute` only when
   execution should be prepared.
3. If it recommends `direct_scaffold`, prefer the recommended
   `start_goal_work` arguments with `dispatch=true` so tracking and the first
   task worktree are prepared in one mutating call. Use `dispatch=false` only
   when direct manager-workspace edits are intentional. No Platypus scaffold
   tool is involved: use the host's native file edits or scaffold command,
   commit the baseline, then return to Platypus for follow-up backlog work.
4. For manual control, create a small concrete backlog set with
   `create_backlog_item`. Keep items independently reviewable and executable.
5. Use `draft_backlog_items` only as optional sampling-assisted drafting; if it
   returns skipped, create the items directly. Run `validate_backlog` after
   backlog writes.
6. Use `inspect_work_queue` and `classify_planning_needs` to determine whether
   the next item is direct, standard, or full.
7. For standard or full items, create a strict task plan with `write_task_plan`
   and `validate_task_plan`; use `draft_task_plan` only as optional
   sampling-assisted help.
8. Prepare execution with `prepare_work`. Direct items may return a
   `direct_edit` host action. Standard/full items return a `run_in_worktree`
   action with a worker assignment bundle; the MCP server does not launch the
   external worker.
9. Record progress when useful, then finish with `finish_work`. Workers should
   report changed files, verification status, acceptance coverage, and findings
   or explicitly set `findings_reviewed=true`.
10. Follow the `finish_work.host_action`: verify, record/resolve findings,
    integrate with `integrate_worker_result`, recover, run
    `reconcile_project`, or move to the next item.

The host should present this as natural assistance, not as a manual ceremony:
explain what is being structured, ask for approval only when choices matter,
and use tool results as the source of truth. Backlog markdown and task-plan
YAML are planning artifacts only; runtime state belongs in Platypus state,
events, evidence, findings, and Git trailers.
"#;

const PROJECT_STATUS_TEXT: &str = r#"# Project Status Guidance

Use status tools before making assumptions about the repository or task queue.

- `doctor_snapshot` checks scaffold files, Git metadata, backlog directories,
  agent profiles, and recovery guidance.
- `inspect_status` summarizes project shape, backlog counts, and runnable work.
- `inspect_workflow_config` reports integration policy such as merge style and
  verification gates.
- `list_agent_profiles` shows manager and worker roles.
- `inspect_work_queue` combines runnable backlog candidates with active task
  counts, planning mode, and task-plan readiness.
- `classify_planning_needs` explains whether runnable items need direct,
  standard, or full planning.
- `classify_workflow_fit` decides whether a broad goal should use direct
  scaffolding first, full Platypus workflow, or a hybrid flow.
- `plan_goal_work` converts a broad goal into read-only next-tool guidance and
  respects planning intent so planning-only chats do not recommend dispatch.
- `next_safe_action` converts the current lifecycle state into the next safe
  tool call.

Report setup blockers directly from structured tool output. Do not invent
missing state from chat context.
"#;

const TOOL_PRELOAD_TEXT: &str = r#"# Tool Preload Guidance

Some MCP hosts defer tool schemas until a tool is discovered, searched, or
selected. Platypus does not require any specific preload mechanism, but hosts
that support one should load the common groups below to reduce planning and
execution round trips.

Preloading is optional and host-specific. If the host cannot preload schemas,
continue normally and call the tools as needed. Do not depend on sampling or
hidden client context for core state transitions.

## Planning Startup Group

Load this group at the start of planning, backlog shaping, or project-status
sessions:

- `doctor_snapshot`
- `inspect_status`
- `inspect_workflow_config`
- `next_safe_action`
- `plan_goal_work`
- `classify_workflow_fit`
- `create_backlog_item`
- `create_backlog_items`
- `create_epic`
- `validate_backlog`
- `list_backlog`
- `inspect_backlog_inventory`
- `inspect_work_queue`
- `classify_planning_needs`
- `write_task_plan`
- `validate_task_plan`
- `inspect_task_plan`
- `request_planning_approval`
- `approval_respond`

## Execution Startup Group

Load this group before dispatch, worker handoff, verification, integration, or
recovery sessions:

- `inspect_work_queue`
- `prepare_work`
- `dispatch_ready_work`
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
- `run_task_verification`
- `record_verification_evidence`
- `record_finding`
- `validate_findings`
- `integrate_worker_result`
- `worktree_cleanup`
- `reconcile_project`

Hosts may load additional tools when a user asks for lower-level control, but
these two groups cover the normal long-term product development loop.
"#;

const BACKLOG_AUTHORING_TEXT: &str = r#"# Backlog Authoring Guidance

Create concise, agent-readable backlog items with `create_backlog_item` for one
item or `create_backlog_items` for an atomic related set.
`draft_backlog_items` is optional and requires MCP client sampling support;
when it returns skipped, use the host model's own judgment and call
`create_backlog_item` or `create_backlog_items` directly. Validate with
`validate_backlog`, inspect runnable work with `list_backlog`, and use
`inspect_backlog_inventory` when the host needs to explain closed or blocked
items.

Backlog markdown should contain goal, implementation contract, acceptance
criteria, dependencies, suggested worker, and owned surfaces. It should not
contain runtime status, assignment attempts, task IDs, PR metadata, closure
state, or blocked/done fields. Closure is derived from Git trailers such as
`Platypus-Closes` and `Platypus-Verification`.

Backlog schema quick reference:

- Minimal create input: a meaningful `goal` or `title`. Platypus derives
  conservative title, goal, implementation contract, and first acceptance text
  when those fields are omitted.
- Rich create input: provide explicit `title`, `goal`,
  `implementation_contract` or `contract`, and `acceptance` when the work is
  complex or the defaults would be too broad.
- Priorities: `P0` critical/next, `P1` normal important work, `P2` refinement.
- Types: `foundation`, `feature`, `safety`, `ux`, `test`, `docs`.
- Defaults: priority `P1`, type `feature`, epic `general`, worker `coder`.
- `suggested_worker` is a Platypus worker profile name, not necessarily a
  host-specific subagent type.
- For related items, prefer `create_backlog_items`. Give each item a
  `client_key` and use `depends_on_keys` to reference other items in the same
  batch before their IDs are known. The batch is all-or-nothing.
- Use `list_epics` before assigning a non-default epic. Use `create_epic` to
  add a missing grouping; do not hand-write epic files unless a recovery step
  explicitly asks for manual file edits.

There is no manual `backlog/index.md`; hosts should compute queue state through
`list_backlog`, `inspect_work_queue`, `inspect_status`, and `next_safe_action`.

For non-trivial items, use `draft_task_plan` and `write_task_plan` to create
`backlog/plans/<ITEM>.yaml` only when sampling is available. Otherwise use the
host model to write a concrete plan directly with `write_task_plan`. Plan YAML
is strict and reviewable, but still not runtime state.

Planning policy is deterministic: direct items can be dispatched from the
backlog contract; standard and full items require a valid task plan first.
"#;

const WORKER_HANDOFF_TEXT: &str = r#"# Worker Handoff Guidance

Workers run outside the MCP server. Platypus prepares and records their
execution state.

1. Prefer `prepare_work` for normal host-run execution. It inspects the queue,
   returns direct-edit guidance for direct items, and prepares assignment
   bundles plus worktrees for standard/full worker work.
2. Use `dispatch_ready_work` only when the host needs lower-level batch
   dispatch control. Pass `execution_mode=manual_handoff` when the MCP host or
   a human-managed worker will execute the assignment outside a configured
   Platypus worker profile.
3. For the lowest-level control, use `dispatch_next_work` followed immediately by
   `prepare_worker_handoff`; do not run an external worker from a task id
   without an assignment.
4. Give the bundle, completion contract, and worktree path to the selected
   worker harness.
5. After the external worker harness actually starts, record that lifecycle
   transition with `start_worker_task` when you need an explicit running state.
   This tool does not launch a worker.
6. Persist safe progress with `record_worker_progress`.
7. Finish with `finish_work` so completion, changed files, verification
   evidence, findings, and integration guidance remain one connected result.
   Use `complete_worker_task` only when you need low-level completion control.

For same-session host-driven edits, `finish_work` can finish a prepared
assignment directly; it auto-starts prepared assignments by default. Set
`auto_start_if_prepared=false` when strict running-only completion is required.

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
- `next_safe_action` recommends the next safe lifecycle tool.
- `events_replay` and `inspect_task_events` replay durable activity.
- `approval_list` and `approval_respond` handle durable approvals.
- `list_findings`, `validate_findings`, and `update_finding_disposition` show
  unresolved follow-up obligations.
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
