use crate::{
    models::{
        ActionResult, AgentProfile, AgentProfileData, AgentProfilesData, AgentProfilesParams,
        ConfigureAgentProfileParams,
    },
    storage,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    env, fs,
    path::{Path, PathBuf},
};

const VALID_ROLES: &[&str] = &["manager", "worker"];
const VALID_HARNESSES: &[&str] = &["codex", "claude", "fake", "custom"];

#[derive(Debug, Default, Serialize, Deserialize)]
struct ProjectConfig {
    #[serde(default)]
    project: serde_yaml::Value,
    #[serde(default)]
    backlog: serde_yaml::Value,
    #[serde(default)]
    agents: AgentsConfig,
    #[serde(flatten)]
    extra: BTreeMap<String, serde_yaml::Value>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct AgentsConfig {
    default: Option<String>,
    #[serde(default)]
    profiles: BTreeMap<String, AgentProfileConfig>,
}

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
struct AgentProfileConfig {
    role: String,
    harness: String,
    executable: String,
    #[serde(default)]
    capabilities: Vec<String>,
    #[serde(default)]
    metadata: BTreeMap<String, Value>,
}

pub fn list_agent_profiles(
    default_root: &Path,
    params: AgentProfilesParams,
) -> ActionResult<AgentProfilesData> {
    let action = "list_agent_profiles";
    let storage = match storage::open(default_root, params.root.as_deref()) {
        Ok(storage) => storage,
        Err(error) => {
            return ActionResult::failed(
                action,
                "Could not open project config.",
                error.to_string(),
            )
        }
    };
    let config = match read_config(&storage.root) {
        Ok(config) => config,
        Err(error) => return ActionResult::failed(action, "Could not read project config.", error),
    };
    let profiles = config
        .agents
        .profiles
        .iter()
        .map(|(name, profile)| profile_data(name, profile))
        .collect::<Vec<_>>();
    let returned = profiles.len();
    let data = AgentProfilesData {
        root: storage.root.display().to_string(),
        profiles,
        returned,
    };
    ActionResult::completed(
        action,
        format!("Returned {returned} agent profile(s)."),
        data,
    )
}

pub fn configure_agent_profile(
    default_root: &Path,
    params: ConfigureAgentProfileParams,
) -> ActionResult<AgentProfileData> {
    let action = "configure_agent_profile";
    let name = match clean_name(&params.name) {
        Ok(name) => name,
        Err(error) => {
            return ActionResult::failed(action, "Could not configure agent profile.", error)
        }
    };
    let role = match clean_choice("role", &params.role, VALID_ROLES) {
        Ok(role) => role,
        Err(error) => {
            return ActionResult::failed(action, "Could not configure agent profile.", error)
        }
    };
    let harness = match clean_choice("harness", &params.harness, VALID_HARNESSES) {
        Ok(harness) => harness,
        Err(error) => {
            return ActionResult::failed(action, "Could not configure agent profile.", error)
        }
    };
    let executable = match clean_required("executable", &params.executable) {
        Ok(executable) => executable,
        Err(error) => {
            return ActionResult::failed(action, "Could not configure agent profile.", error)
        }
    };
    if let Err(error) = reject_secret_metadata(&params.metadata) {
        return ActionResult::failed(action, "Could not configure agent profile.", error);
    }
    let storage = match storage::open(default_root, params.root.as_deref()) {
        Ok(storage) => storage,
        Err(error) => {
            return ActionResult::failed(
                action,
                "Could not open project config.",
                error.to_string(),
            )
        }
    };
    let mut config = match read_config(&storage.root) {
        Ok(config) => config,
        Err(error) => return ActionResult::failed(action, "Could not read project config.", error),
    };
    let profile = AgentProfileConfig {
        role,
        harness,
        executable,
        capabilities: clean_vec(params.capabilities),
        metadata: params.metadata,
    };
    config.agents.profiles.insert(name.clone(), profile.clone());
    if config.agents.default.is_none() && profile.role == "manager" {
        config.agents.default = Some(name.clone());
    }
    if let Err(error) = write_config(&storage.root, &config) {
        return ActionResult::failed(action, "Could not persist project config.", error);
    }
    let data = AgentProfileData {
        root: storage.root.display().to_string(),
        profile: profile_data(&name, &profile),
    };
    ActionResult::completed(action, format!("Configured agent profile `{name}`."), data)
}

fn read_config(root: &Path) -> Result<ProjectConfig, String> {
    let path = config_path(root);
    if !path.is_file() {
        return Ok(ProjectConfig::default());
    }
    let text = fs::read_to_string(&path).map_err(|error| format!("{}: {error}", path.display()))?;
    serde_yaml::from_str(&text).map_err(|error| error.to_string())
}

fn write_config(root: &Path, config: &ProjectConfig) -> Result<(), String> {
    let path = config_path(root);
    let text = serde_yaml::to_string(config).map_err(|error| error.to_string())?;
    fs::write(&path, text).map_err(|error| format!("{}: {error}", path.display()))
}

fn config_path(root: &Path) -> PathBuf {
    root.join("platy.yaml")
}

fn profile_data(name: &str, profile: &AgentProfileConfig) -> AgentProfile {
    let mut issues = Vec::new();
    if resolve_executable(&profile.executable).is_none() {
        issues.push(format!(
            "executable `{}` was not found on PATH or as an absolute path",
            profile.executable
        ));
    }
    AgentProfile {
        name: name.to_string(),
        role: profile.role.clone(),
        harness: profile.harness.clone(),
        executable: profile.executable.clone(),
        capabilities: profile.capabilities.clone(),
        metadata: redact_metadata(&profile.metadata),
        ready: issues.is_empty(),
        issues,
    }
}

fn resolve_executable(value: &str) -> Option<PathBuf> {
    let path = Path::new(value);
    if path.is_absolute() || value.contains(std::path::MAIN_SEPARATOR) {
        return fs::metadata(path)
            .ok()
            .filter(|metadata| metadata.is_file())
            .map(|_| path.to_path_buf());
    }
    for directory in env::split_paths(&env::var_os("PATH").unwrap_or_default()) {
        let candidate = directory.join(value);
        if fs::metadata(&candidate)
            .map(|metadata| metadata.is_file())
            .unwrap_or(false)
        {
            return Some(candidate);
        }
    }
    None
}

fn clean_name(value: &str) -> Result<String, String> {
    let name = clean_required("name", value)?;
    if name
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || character == '-' || character == '_')
    {
        Ok(name)
    } else {
        Err("name must contain only ASCII letters, numbers, '-' or '_'".to_string())
    }
}

fn clean_choice(field: &str, value: &str, valid: &[&str]) -> Result<String, String> {
    let value = clean_required(field, value)?.to_ascii_lowercase();
    if valid.contains(&value.as_str()) {
        Ok(value)
    } else {
        Err(format!("invalid {field} `{value}`"))
    }
}

fn clean_required(field: &str, value: &str) -> Result<String, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        Err(format!("{field} is required"))
    } else {
        Ok(trimmed.to_string())
    }
}

fn clean_vec(values: Vec<String>) -> Vec<String> {
    values
        .into_iter()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect()
}

fn reject_secret_metadata(metadata: &BTreeMap<String, Value>) -> Result<(), String> {
    for (key, value) in metadata {
        if secretish(key) || value_contains_secret(value) {
            return Err(format!("metadata key `{key}` may contain a secret"));
        }
    }
    Ok(())
}

fn value_contains_secret(value: &Value) -> bool {
    match value {
        Value::String(value) => secretish(value),
        Value::Array(values) => values.iter().any(value_contains_secret),
        Value::Object(values) => values
            .iter()
            .any(|(key, value)| secretish(key) || value_contains_secret(value)),
        _ => false,
    }
}

fn secretish(value: &str) -> bool {
    let value = value.to_ascii_lowercase();
    [
        "secret",
        "token",
        "apikey",
        "api_key",
        "password",
        "credential",
    ]
    .iter()
    .any(|needle| value.contains(needle))
}

fn redact_metadata(metadata: &BTreeMap<String, Value>) -> BTreeMap<String, Value> {
    metadata
        .iter()
        .filter(|(key, value)| !secretish(key) && !value_contains_secret(value))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::TempDir;

    #[test]
    fn configures_manager_and_worker_profiles() {
        let project = TempDir::new().expect("temp dir");
        let current_exe = std::env::current_exe().expect("current exe");
        let manager = configure_agent_profile(
            project.path(),
            ConfigureAgentProfileParams {
                root: None,
                name: "manager".to_string(),
                role: "manager".to_string(),
                harness: "codex".to_string(),
                executable: current_exe.display().to_string(),
                capabilities: vec!["planning".to_string(), "backlog".to_string()],
                metadata: BTreeMap::new(),
            },
        );
        let worker = configure_agent_profile(
            project.path(),
            ConfigureAgentProfileParams {
                root: None,
                name: "coder".to_string(),
                role: "worker".to_string(),
                harness: "claude".to_string(),
                executable: current_exe.display().to_string(),
                capabilities: vec!["implementation".to_string()],
                metadata: BTreeMap::new(),
            },
        );

        assert!(matches!(
            manager.status,
            crate::models::ActionStatus::Completed
        ));
        assert!(matches!(
            worker.status,
            crate::models::ActionStatus::Completed
        ));

        let listed = list_agent_profiles(project.path(), AgentProfilesParams { root: None });
        let data = listed.data.expect("profiles");
        assert_eq!(data.returned, 2);
        assert!(data.profiles.iter().all(|profile| profile.ready));
        assert!(project.path().join("platy.yaml").is_file());
    }

    #[test]
    fn rejects_invalid_role() {
        let project = TempDir::new().expect("temp dir");
        let result = configure_agent_profile(
            project.path(),
            ConfigureAgentProfileParams {
                root: None,
                name: "agent".to_string(),
                role: "owner".to_string(),
                harness: "codex".to_string(),
                executable: "codex".to_string(),
                capabilities: Vec::new(),
                metadata: BTreeMap::new(),
            },
        );

        assert!(matches!(result.status, crate::models::ActionStatus::Failed));
    }

    #[test]
    fn rejects_secret_metadata() {
        let project = TempDir::new().expect("temp dir");
        let mut metadata = BTreeMap::new();
        metadata.insert("api_token".to_string(), json!("secret"));

        let result = configure_agent_profile(
            project.path(),
            ConfigureAgentProfileParams {
                root: None,
                name: "agent".to_string(),
                role: "worker".to_string(),
                harness: "codex".to_string(),
                executable: "codex".to_string(),
                capabilities: Vec::new(),
                metadata,
            },
        );

        assert!(matches!(result.status, crate::models::ActionStatus::Failed));
        assert!(!project.path().join("platy.yaml").exists());
    }

    #[test]
    fn reports_missing_executable_guidance() {
        let project = TempDir::new().expect("temp dir");
        let result = configure_agent_profile(
            project.path(),
            ConfigureAgentProfileParams {
                root: None,
                name: "coder".to_string(),
                role: "worker".to_string(),
                harness: "codex".to_string(),
                executable: "definitely-missing-platypus-agent".to_string(),
                capabilities: Vec::new(),
                metadata: BTreeMap::new(),
            },
        );
        let profile = result.data.expect("profile").profile;

        assert!(!profile.ready);
        assert_eq!(profile.issues.len(), 1);
    }

    #[test]
    fn preserves_unknown_project_config_keys() {
        let project = TempDir::new().expect("temp dir");
        fs::write(
            project.path().join("platy.yaml"),
            "project:\n  name: example\ncustom:\n  mode: keep\n",
        )
        .expect("write config");
        let current_exe = std::env::current_exe().expect("current exe");

        let result = configure_agent_profile(
            project.path(),
            ConfigureAgentProfileParams {
                root: None,
                name: "manager".to_string(),
                role: "manager".to_string(),
                harness: "codex".to_string(),
                executable: current_exe.display().to_string(),
                capabilities: Vec::new(),
                metadata: BTreeMap::new(),
            },
        );

        assert!(matches!(
            result.status,
            crate::models::ActionStatus::Completed
        ));
        let config = fs::read_to_string(project.path().join("platy.yaml")).expect("config");
        assert!(config.contains("custom:"));
        assert!(config.contains("mode: keep"));
    }
}
