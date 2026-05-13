use crate::{
    models::{
        ActionResult, WorkflowConfigData, WorkflowConfigParams, WorkflowDispatchConfig,
        WorkflowExecutionConfig, WorkflowIntegrationConfig,
    },
    project::paths::resolve_root,
};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fs, path::Path};

const VALID_MERGE_STYLES: &[&str] = &["merge_commit", "fast_forward", "squash"];
const DEFAULT_MERGE_STYLE: &str = "merge_commit";
const DEFAULT_EXECUTION_PATH: &str = crate::execution_policy::DIRECT_EDIT;
const DEFAULT_DIRECT_PLANNING_GATE: &str = crate::execution_policy::GATE_NONE;
const DEFAULT_WORKER_PLANNING_GATE: &str = crate::execution_policy::GATE_TASK_PLAN;

#[derive(Debug, Default, Serialize, Deserialize)]
struct ProjectConfig {
    #[serde(default)]
    project: serde_yaml::Value,
    #[serde(default)]
    backlog: serde_yaml::Value,
    #[serde(default)]
    workflow: WorkflowConfig,
    #[serde(flatten)]
    extra: BTreeMap<String, serde_yaml::Value>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct WorkflowConfig {
    #[serde(default)]
    integration: WorkflowIntegrationConfigFile,
    #[serde(default)]
    dispatch: WorkflowDispatchConfigFile,
    #[serde(default)]
    execution: WorkflowExecutionConfigFile,
}

#[derive(Debug, Serialize, Deserialize)]
struct WorkflowIntegrationConfigFile {
    merge_style: Option<String>,
    require_clean_manager_workspace: Option<bool>,
    require_verification_evidence: Option<bool>,
}

impl Default for WorkflowIntegrationConfigFile {
    fn default() -> Self {
        Self {
            merge_style: Some(DEFAULT_MERGE_STYLE.to_string()),
            require_clean_manager_workspace: Some(true),
            require_verification_evidence: Some(false),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct WorkflowDispatchConfigFile {
    auto_commit_artifacts_default: Option<bool>,
}

impl Default for WorkflowDispatchConfigFile {
    fn default() -> Self {
        Self {
            auto_commit_artifacts_default: Some(false),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct WorkflowExecutionConfigFile {
    default_path: Option<String>,
    direct_planning_gate: Option<String>,
    worker_planning_gate: Option<String>,
}

impl Default for WorkflowExecutionConfigFile {
    fn default() -> Self {
        Self {
            default_path: Some(DEFAULT_EXECUTION_PATH.to_string()),
            direct_planning_gate: Some(DEFAULT_DIRECT_PLANNING_GATE.to_string()),
            worker_planning_gate: Some(DEFAULT_WORKER_PLANNING_GATE.to_string()),
        }
    }
}

pub fn inspect_workflow_config(
    default_root: &Path,
    params: WorkflowConfigParams,
) -> ActionResult<WorkflowConfigData> {
    let action = "inspect_workflow_config";
    let root = match resolve_root(default_root, params.root.as_deref()) {
        Ok(root) => root,
        Err(error) => {
            return ActionResult::failed(action, "Could not open workflow config.", error)
        }
    };
    let config = match read_config(&root) {
        Ok(config) => config,
        Err(error) => {
            return ActionResult::failed(action, "Could not inspect workflow config.", error);
        }
    };
    let integration = match effective_integration_config(&config.workflow.integration) {
        Ok(integration) => integration,
        Err(error) => {
            return ActionResult::failed(action, "Could not inspect workflow config.", error);
        }
    };
    let execution = match effective_execution_config_file(&config.workflow.execution) {
        Ok(execution) => execution,
        Err(error) => {
            return ActionResult::failed(action, "Could not inspect workflow config.", error);
        }
    };
    ActionResult::completed(
        action,
        "Inspected workflow integration config.",
        WorkflowConfigData {
            root: root.display().to_string(),
            integration,
            dispatch: effective_dispatch_config(&config.workflow.dispatch),
            execution,
        },
    )
}

pub fn effective_dispatch_defaults(root: &Path) -> Result<WorkflowDispatchConfig, String> {
    let config = read_config(root)?;
    Ok(effective_dispatch_config(&config.workflow.dispatch))
}

pub fn effective_execution_config(root: &Path) -> Result<WorkflowExecutionConfig, String> {
    let config = read_config(root)?;
    effective_execution_config_file(&config.workflow.execution)
}

pub fn effective_workflow_config(root: &Path) -> Result<WorkflowIntegrationConfig, String> {
    let config = read_config(root)?;
    effective_integration_config(&config.workflow.integration)
}

fn effective_integration_config(
    config: &WorkflowIntegrationConfigFile,
) -> Result<WorkflowIntegrationConfig, String> {
    let merge_style = config
        .merge_style
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(DEFAULT_MERGE_STYLE)
        .to_ascii_lowercase();
    if !VALID_MERGE_STYLES.contains(&merge_style.as_str()) {
        return Err(format!(
            "invalid workflow.integration.merge_style `{merge_style}`"
        ));
    }
    Ok(WorkflowIntegrationConfig {
        merge_style,
        require_clean_manager_workspace: config.require_clean_manager_workspace.unwrap_or(true),
        require_verification_evidence: config.require_verification_evidence.unwrap_or(false),
    })
}

fn effective_dispatch_config(config: &WorkflowDispatchConfigFile) -> WorkflowDispatchConfig {
    WorkflowDispatchConfig {
        auto_commit_artifacts_default: config.auto_commit_artifacts_default.unwrap_or(false),
    }
}

fn effective_execution_config_file(
    config: &WorkflowExecutionConfigFile,
) -> Result<WorkflowExecutionConfig, String> {
    let default_path = config
        .default_path
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(DEFAULT_EXECUTION_PATH);
    let default_path = crate::execution_policy::normalize_execution_path(default_path)
        .ok_or_else(|| format!("invalid workflow.execution.default_path `{default_path}`"))?
        .to_string();
    let direct_planning_gate = normalized_gate(
        "workflow.execution.direct_planning_gate",
        config.direct_planning_gate.as_deref(),
        DEFAULT_DIRECT_PLANNING_GATE,
    )?;
    let worker_planning_gate = normalized_gate(
        "workflow.execution.worker_planning_gate",
        config.worker_planning_gate.as_deref(),
        DEFAULT_WORKER_PLANNING_GATE,
    )?;
    Ok(WorkflowExecutionConfig {
        default_path,
        direct_planning_gate,
        worker_planning_gate,
    })
}

fn normalized_gate(name: &str, value: Option<&str>, default: &str) -> Result<String, String> {
    let value = value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(default);
    crate::execution_policy::normalize_planning_gate(value)
        .map(str::to_string)
        .ok_or_else(|| format!("invalid {name} `{value}`"))
}

fn read_config(root: &Path) -> Result<ProjectConfig, String> {
    let path = root.join("platy.yaml");
    if !path.is_file() {
        return Ok(ProjectConfig::default());
    }
    let text = fs::read_to_string(&path).map_err(|error| format!("{}: {error}", path.display()))?;
    serde_yaml::from_str(&text).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn inspects_default_workflow_integration_config() {
        let project = TempDir::new().expect("temp dir");

        let result = inspect_workflow_config(project.path(), WorkflowConfigParams { root: None });
        let data = result.data.expect("workflow config");

        assert!(matches!(
            result.status,
            crate::models::ActionStatus::Completed
        ));
        assert_eq!(data.integration.merge_style, "merge_commit");
        assert!(data.integration.require_clean_manager_workspace);
        assert!(!data.integration.require_verification_evidence);
        assert!(!data.dispatch.auto_commit_artifacts_default);
    }

    #[test]
    fn rejects_invalid_workflow_merge_style() {
        let project = TempDir::new().expect("temp dir");
        fs::write(
            project.path().join("platy.yaml"),
            "workflow:\n  integration:\n    merge_style: surprise\n",
        )
        .expect("write config");

        let result = inspect_workflow_config(project.path(), WorkflowConfigParams { root: None });

        assert!(matches!(result.status, crate::models::ActionStatus::Failed));
        assert!(result
            .error
            .expect("error")
            .contains("workflow.integration.merge_style"));
    }

    #[test]
    fn preserves_unknown_project_config_keys_when_reading() {
        let project = TempDir::new().expect("temp dir");
        fs::write(
            project.path().join("platy.yaml"),
            "project:\n  name: example\ncustom:\n  mode: keep\n",
        )
        .expect("write config");

        let config = read_config(project.path()).expect("config");

        assert!(config.extra.contains_key("custom"));
    }
}
