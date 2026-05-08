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
pub const BACKLOG_AUTHORING_URI: &str = "platypus://guidance/backlog-authoring";
pub const WORKER_HANDOFF_URI: &str = "platypus://guidance/worker-handoff";
pub const INTEGRATION_REVIEW_URI: &str = "platypus://guidance/integration-review";
pub const RECOVERY_URI: &str = "platypus://guidance/recovery";

const WORKFLOW_TEXT: &str = r#"# Platypus Host Workflow

Use Platypus MCP tools for deterministic project state. The MCP host owns chat,
model turns, and external worker execution.

1. Inspect setup with `doctor_snapshot`, `inspect_status`, and
   `inspect_workflow_config`.
2. Shape work with `draft_backlog_items`, `create_backlog_item`,
   `validate_backlog`, and `list_backlog`.
3. Use `inspect_work_queue` or `classify_planning_needs` to choose executable
   backlog work and understand direct, standard, or full planning needs.
4. For non-trivial work, use `draft_task_plan`, `write_task_plan`,
   `validate_task_plan`, `inspect_task_plan`, and `list_task_plans` to create a
   committed executable plan.
5. Ask `next_safe_action` before advancing lifecycle state.
6. Dispatch only through `dispatch_next_work`, then prepare the worker with
   `prepare_worker_handoff`.
7. Start, track, complete, verify, integrate, and reconcile with
   `start_worker_task`, `record_worker_progress`, `complete_worker_task`,
   `record_verification_evidence`, `integrate_worker_result`, and
   `reconcile_project`.

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
2. Draft a small backlog set with `draft_backlog_items`. Keep items
   independently reviewable and executable.
3. Persist only accepted items with `create_backlog_item`; then run
   `validate_backlog`.
4. Use `inspect_work_queue` and `classify_planning_needs` to determine whether
   the next item is direct, standard, or full.
5. For standard or full items, create a strict task plan with
   `draft_task_plan`, `write_task_plan`, and `validate_task_plan`.
6. Dispatch with `dispatch_next_work`, prepare handoff with
   `prepare_worker_handoff`, and run implementation in the assigned worktree.
7. Record progress, verification evidence, findings, and completion through
   Platypus tools.
8. Integrate with `integrate_worker_result`, then reconcile with
   `reconcile_project`.

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
- `inspect_work_queue` combines runnable backlog candidates with planning mode
  and task-plan readiness.
- `classify_planning_needs` explains whether runnable items need direct,
  standard, or full planning.
- `next_safe_action` converts the current lifecycle state into the next safe
  tool call.

Report setup blockers directly from structured tool output. Do not invent
missing state from chat context.
"#;

const BACKLOG_AUTHORING_TEXT: &str = r#"# Backlog Authoring Guidance

Create concise, agent-readable backlog items. Use `draft_backlog_items` for
candidates and `create_backlog_item` for accepted work. Validate with
`validate_backlog`, then inspect runnable work with `list_backlog`.

Backlog markdown should contain goal, implementation contract, acceptance
criteria, dependencies, suggested worker, and owned surfaces. It should not
contain runtime status, assignment attempts, task IDs, PR metadata, closure
state, or blocked/done fields. Closure is derived from Git trailers such as
`Platypus-Closes` and `Platypus-Verification`.

There is no manual `backlog/index.md`; hosts should compute queue state through
`list_backlog`, `inspect_work_queue`, `inspect_status`, and `next_safe_action`.

For non-trivial items, use `draft_task_plan` and `write_task_plan` to create
`backlog/plans/<ITEM>.yaml`. Plan YAML is strict and reviewable, but still not
runtime state.

Planning policy is deterministic: direct items can be dispatched from the
backlog contract; standard and full items require a valid task plan first.
"#;

const WORKER_HANDOFF_TEXT: &str = r#"# Worker Handoff Guidance

Workers run outside the MCP server. Platypus prepares and records their
execution state.

1. Use `dispatch_next_work` to create a durable task.
2. Use `prepare_worker_handoff` to create the assignment, worktree, and bundle.
3. Give the bundle and worktree path to the selected worker harness.
4. Mark execution with `start_worker_task`.
5. Persist safe progress with `record_worker_progress`.
6. Inspect bounded worktree changes with `inspect_worktree_changes`.
7. Complete with `complete_worker_task`, including terminal status, summary,
   changed files, and verification status.

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
