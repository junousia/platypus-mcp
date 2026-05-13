use crate::models::{EffectiveExecutionPolicy, WorkflowExecutionConfig};

pub const DIRECT_EDIT: &str = "direct_edit";
pub const WORKER_HANDOFF: &str = "worker_handoff";

pub const GATE_NONE: &str = "none";
pub const GATE_TASK_PLAN: &str = "task_plan";
pub const GATE_APPROVED_TASK_PLAN: &str = "approved_task_plan";

pub fn normalize_execution_path(value: &str) -> Option<&'static str> {
    match value.trim().to_ascii_lowercase().as_str() {
        "direct" | "direct_edit" | "manager_workspace" | "manager" => Some(DIRECT_EDIT),
        "worker" | "worker_handoff" | "worktree" | "handoff" => Some(WORKER_HANDOFF),
        _ => None,
    }
}

pub fn normalize_planning_gate(value: &str) -> Option<&'static str> {
    match value.trim().to_ascii_lowercase().as_str() {
        "none" | "no_plan" | "direct" | "minimal" => Some(GATE_NONE),
        "task_plan" | "plan" | "planned" => Some(GATE_TASK_PLAN),
        "approved_task_plan" | "approval" | "planning_approval" | "approved_plan" => {
            Some(GATE_APPROVED_TASK_PLAN)
        }
        _ => None,
    }
}

pub fn valid_execution_path(value: &str) -> bool {
    normalize_execution_path(value) == Some(value)
}

pub fn valid_planning_gate(value: &str) -> bool {
    normalize_planning_gate(value) == Some(value)
}

pub fn plan_required(planning_gate: &str) -> bool {
    matches!(planning_gate, GATE_TASK_PLAN | GATE_APPROVED_TASK_PLAN)
}

pub fn approval_required(planning_gate: &str) -> bool {
    planning_gate == GATE_APPROVED_TASK_PLAN
}

pub fn default_gate_for_path(config: &WorkflowExecutionConfig, execution_path: &str) -> String {
    if execution_path == WORKER_HANDOFF {
        config.worker_planning_gate.clone()
    } else {
        config.direct_planning_gate.clone()
    }
}

pub fn resolve_effective_policy(
    config: &WorkflowExecutionConfig,
    item_execution_path: Option<&str>,
    item_planning_gate: Option<&str>,
) -> EffectiveExecutionPolicy {
    let execution_path = item_execution_path
        .and_then(normalize_execution_path)
        .unwrap_or(config.default_path.as_str())
        .to_string();
    let planning_gate = item_planning_gate
        .and_then(normalize_planning_gate)
        .map(str::to_string)
        .unwrap_or_else(|| default_gate_for_path(config, &execution_path));
    let source = if item_execution_path.is_some() || item_planning_gate.is_some() {
        "backlog_item".to_string()
    } else {
        "workflow_config".to_string()
    };
    let reason = if source == "backlog_item" {
        format!(
            "Backlog item policy selects execution_path={} and planning_gate={}.",
            execution_path, planning_gate
        )
    } else {
        format!(
            "Workflow execution defaults select execution_path={} and planning_gate={}.",
            execution_path, planning_gate
        )
    };
    EffectiveExecutionPolicy {
        execution_path,
        planning_gate,
        source,
        reason,
    }
}
