use crate::models::{ActionResult, ActionStatus, InspectToolsetsParams, ToolsetData, ToolsetInfo};
use std::fmt::Write;

#[derive(Debug, Clone, Copy)]
pub struct ToolsetDefinition {
    pub name: &'static str,
    pub title: &'static str,
    pub purpose: &'static str,
    pub when_to_use: &'static str,
    pub recommended_first_tool: &'static str,
    pub tools: &'static [&'static str],
}

pub const TOOLSETS: &[ToolsetDefinition] = &[
    ToolsetDefinition {
        name: "startup",
        title: "Startup Inspection",
        purpose: "Orient a fresh or stale session without guessing project state.",
        when_to_use: "At session start, after context compaction, or whenever chat state may be stale.",
        recommended_first_tool: "inspect_session",
        tools: &[
            "inspect_toolsets",
            "inspect_session",
            "doctor_snapshot",
            "inspect_status",
            "inspect_workflow_config",
            "inspect_queue_status",
        ],
    },
    ToolsetDefinition {
        name: "backlog_planning",
        title: "Backlog Planning",
        purpose: "Create, inspect, update, and validate declarative backlog and task-plan artifacts.",
        when_to_use: "Before backlog shaping, item updates, dependency review, task-plan work, or planning approvals.",
        recommended_first_tool: "inspect_work_queue",
        tools: &[
            "inspect_work_queue",
            "inspect_queue_status",
            "create_backlog_item",
            "quick_create_backlog_item",
            "create_backlog_items",
            "update_backlog_item",
            "create_epic",
            "list_epics",
            "validate_backlog",
            "list_backlog",
            "get_backlog_item",
            "inspect_item",
            "write_task_plan",
            "validate_task_plan",
            "inspect_task_plan",
            "request_planning_approval",
            "approval_respond",
        ],
    },
    ToolsetDefinition {
        name: "direct_execution",
        title: "Direct Execution",
        purpose: "Complete manager-workspace direct edits without creating worker tasks or worktrees.",
        when_to_use: "When inspect_work_queue reports direct_ready or a host is doing a small tracked manager-workspace change.",
        recommended_first_tool: "inspect_work_queue",
        tools: &[
            "inspect_session",
            "inspect_queue_status",
            "inspect_work_queue",
            "prepare_work",
            "complete_backlog_item",
            "record_verification_evidence",
            "record_finding",
            "validate_findings",
        ],
    },
    ToolsetDefinition {
        name: "worker_handoff",
        title: "Worker Handoff",
        purpose: "Prepare, monitor, finish, verify, and integrate isolated worktree handoffs.",
        when_to_use: "When work needs an isolated worker handoff, parallel implementation, or integration review.",
        recommended_first_tool: "prepare_work",
        tools: &[
            "inspect_session",
            "inspect_work_queue",
            "prepare_work",
            "dispatch_ready_work",
            "commit_planning_artifacts",
            "generate_task_bundle",
            "inspect_task",
            "inspect_task_events",
            "events_replay",
            "worktree_status",
            "inspect_worktree_changes",
            "send_worker_guidance",
            "start_worker_task",
            "record_worker_progress",
            "complete_worker_task",
            "finish_work",
            "complete_backlog_item",
            "run_task_verification",
            "record_verification_evidence",
            "record_finding",
            "validate_findings",
            "inspect_integration_gates",
            "integrate_worker_result",
            "worktree_cleanup",
            "reconcile_project",
        ],
    },
    ToolsetDefinition {
        name: "evidence_findings",
        title: "Evidence And Findings",
        purpose: "Record, inspect, validate, and disposition audit evidence and follow-up findings.",
        when_to_use: "When work reports verification, limitations, risks, required follow-up, or audit context.",
        recommended_first_tool: "record_evidence",
        tools: &[
            "record_evidence",
            "record_verification_evidence",
            "list_evidence",
            "record_finding",
            "list_findings",
            "validate_findings",
            "update_finding_disposition",
        ],
    },
    ToolsetDefinition {
        name: "recovery",
        title: "Recovery",
        purpose: "Diagnose failed, blocked, or unclear lifecycle state and choose a safe continuation.",
        when_to_use: "When a tool fails, returns recovery_action, or the project lifecycle state is unclear.",
        recommended_first_tool: "inspect_session",
        tools: &[
            "inspect_session",
            "doctor_snapshot",
            "inspect_work_queue",
            "events_replay",
            "inspect_task_events",
            "approval_list",
            "approval_respond",
            "list_evidence",
            "list_findings",
            "validate_findings",
            "update_finding_disposition",
            "inspect_integration_gates",
            "reconcile_project",
        ],
    },
];

pub fn inspect_toolsets(params: InspectToolsetsParams) -> ActionResult<ToolsetData> {
    let action = "inspect_toolsets";
    let selected = match params.toolset.as_deref().and_then(normalize_toolset_name) {
        Some(name) => match find_toolset(&name) {
            Some(toolset) => vec![toolset],
            None => {
                let available = available_toolset_names();
                return ActionResult {
                    action: action.to_string(),
                    status: ActionStatus::Failed,
                    summary: format!("Unknown toolset `{name}`."),
                    next_action: Some(format!("Use one of: {}.", available.join(", "))),
                    recovery_action: Some(format!("Use one of: {}.", available.join(", "))),
                    data: None,
                    error: Some(format!("unknown toolset `{name}`")),
                };
            }
        },
        None if params.toolset.is_some() => {
            let available = available_toolset_names();
            let requested = params.toolset.unwrap_or_default();
            return ActionResult {
                action: action.to_string(),
                status: ActionStatus::Failed,
                summary: format!("Unknown toolset `{requested}`."),
                next_action: Some(format!("Use one of: {}.", available.join(", "))),
                recovery_action: Some(format!("Use one of: {}.", available.join(", "))),
                data: None,
                error: Some(format!("unknown toolset `{requested}`")),
            };
        }
        None => TOOLSETS.iter().collect(),
    };
    let toolsets = selected
        .iter()
        .map(|toolset| toolset_info(toolset))
        .collect::<Vec<_>>();
    ActionResult {
        action: action.to_string(),
        status: ActionStatus::Completed,
        summary: format!("Returned {} Platypus toolset(s).", toolsets.len()),
        next_action: Some(
            "Use toolsets as optional discovery metadata; normal workflow still follows inspect_session, recommended_tool, and tool responses."
                .to_string(),
        ),
        recovery_action: None,
        data: Some(ToolsetData {
            total: TOOLSETS.len(),
            returned: toolsets.len(),
            toolsets,
        }),
        error: None,
    }
}

pub fn tool_preload_markdown() -> String {
    let mut text = String::from(
        r#"# Tool Preload Guidance

Some MCP hosts defer tool schemas until a tool is discovered, searched, or
selected. Platypus does not require any specific preload mechanism. The
toolsets below are optional discovery metadata, not required workflow steps and
not separate MCP servers.

Preloading is optional and host-specific; if a host cannot preload schemas,
continue normally and call the same tools on demand when the workflow reaches
that phase.

Start with `inspect_toolsets` when a host needs a map of available tool groups.
Otherwise call `inspect_session` directly and follow `recommended_tool`,
`schemas_likely_needed_next`, and each tool response.

`inspect_session` and `inspect_work_queue` return only 1-4 likely next tool
schema hints to keep normal workflow payloads small. Full group metadata lives
in `inspect_toolsets` and this guidance resource.

Claude Code can use `claude_selector`. Codex-style text-search hosts can use
`codex_query`. Other hosts can use `host_neutral_query` or call tools by name.

## Tool Naming Map

Prefer these host-facing tool names. Compatibility aliases remain callable, but
guidance should use the preferred name unless it is explicitly documenting an
alias.

- `inspect_status`; alias `project_status`.
- `inspect_worktree_changes`; low-level alias `worktree_diff`.
- `prepare_work`; low-level handoff tools `prepare_worker_handoff` and
  `prepare_worker_assignment`.
- `start_worker_task`; alias `start_worker_execution`.
- `record_worker_progress`; alias `record_worker_event`.
- `finish_work`; low-level `complete_worker_task` and alias
  `complete_worker_execution`.

"#,
    );

    for toolset in TOOLSETS {
        let info = toolset_info(toolset);
        let _ = writeln!(text, "## {} Group\n", toolset.title);
        let _ = writeln!(text, "- name: `{}`", info.name);
        let _ = writeln!(text, "- purpose: {}", info.purpose);
        let _ = writeln!(text, "- when: {}", info.when_to_use);
        let _ = writeln!(
            text,
            "- recommended first tool: `{}`",
            info.recommended_first_tool
        );
        let _ = writeln!(text, "- Claude selector: `{}`", info.claude_selector);
        let _ = writeln!(text, "- Codex query: `{}`", info.codex_query);
        let _ = writeln!(
            text,
            "- Host-neutral query: `{}`\n",
            info.host_neutral_query
        );
        for tool in &info.tools {
            let _ = writeln!(text, "- `{tool}`");
        }
        text.push('\n');
    }

    text.push_str(
        "If a host cannot preload schemas, continue normally and call the same tools on demand when the workflow reaches that phase.\n",
    );
    text
}

pub fn available_toolset_names() -> Vec<String> {
    TOOLSETS
        .iter()
        .map(|toolset| toolset.name.to_string())
        .collect()
}

fn find_toolset(name: &str) -> Option<&'static ToolsetDefinition> {
    TOOLSETS.iter().find(|toolset| toolset.name == name)
}

fn toolset_info(toolset: &ToolsetDefinition) -> ToolsetInfo {
    let tools = toolset
        .tools
        .iter()
        .map(|tool| (*tool).to_string())
        .collect::<Vec<_>>();
    ToolsetInfo {
        name: toolset.name.to_string(),
        title: toolset.title.to_string(),
        purpose: toolset.purpose.to_string(),
        when_to_use: toolset.when_to_use.to_string(),
        recommended_first_tool: toolset.recommended_first_tool.to_string(),
        tools: tools.clone(),
        claude_selector: claude_selector(&tools),
        codex_query: codex_query(toolset.name, &tools),
        host_neutral_query: host_neutral_query(toolset.name, &tools),
    }
}

fn claude_selector(tools: &[String]) -> String {
    format!(
        "select:{}",
        tools
            .iter()
            .map(|tool| format!("mcp__platypus__{tool}"))
            .collect::<Vec<_>>()
            .join(",")
    )
}

fn codex_query(name: &str, tools: &[String]) -> String {
    format!("platypus {name} tools {}", tools.join(" "))
}

fn host_neutral_query(name: &str, tools: &[String]) -> String {
    format!("platypus toolset {name} {}", tools.join(" "))
}

fn normalize_toolset_name(name: &str) -> Option<String> {
    let normalized = name.trim().to_ascii_lowercase().replace(['-', ' '], "_");
    (!normalized.is_empty()).then_some(normalized)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_toolsets_have_discovery_metadata() {
        for toolset in TOOLSETS {
            assert!(!toolset.name.is_empty());
            assert!(!toolset.purpose.is_empty());
            assert!(!toolset.when_to_use.is_empty());
            assert!(!toolset.recommended_first_tool.is_empty());
            assert!(!toolset.tools.is_empty());
            assert!(toolset.tools.contains(&toolset.recommended_first_tool));

            let info = toolset_info(toolset);
            assert!(info.claude_selector.starts_with("select:mcp__platypus__"));
            assert!(info.codex_query.contains(toolset.name));
            assert!(info.host_neutral_query.contains(toolset.name));
        }
    }

    #[test]
    fn inspect_toolsets_filters_by_name() {
        let result = inspect_toolsets(InspectToolsetsParams {
            toolset: Some("Direct Execution".to_string()),
        });
        let data = result.data.expect("toolset data");

        assert_eq!(result.status, ActionStatus::Completed);
        assert_eq!(data.returned, 1);
        assert_eq!(data.toolsets[0].name, "direct_execution");
    }

    #[test]
    fn inspect_toolsets_fails_unknown_toolset_with_available_names() {
        let result = inspect_toolsets(InspectToolsetsParams {
            toolset: Some("unknown".to_string()),
        });

        assert_eq!(result.status, ActionStatus::Failed);
        assert!(result
            .next_action
            .as_deref()
            .expect("next action")
            .contains("startup"));
    }
}
