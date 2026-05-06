use super::{WorkerAdapter, WorkerEvent, WorkerExitStatus, WorkerRequest, WorkerResult};
use serde_json::Value;
use std::{
    env, fmt, fs,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone)]
pub struct CodexAdapterConfig {
    pub executable: String,
    pub args: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct CodexWorkerAdapter {
    config: CodexAdapterConfig,
    executable_path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodexConfigError {
    message: String,
}

impl fmt::Display for CodexConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.message)
    }
}

impl std::error::Error for CodexConfigError {}

impl CodexWorkerAdapter {
    pub fn new(config: CodexAdapterConfig) -> Result<Self, CodexConfigError> {
        let executable_path = resolve_executable(&config.executable)?;
        Ok(Self {
            config,
            executable_path,
        })
    }

    pub fn executable_path(&self) -> &Path {
        &self.executable_path
    }
}

impl WorkerAdapter for CodexWorkerAdapter {
    fn name(&self) -> &str {
        "codex"
    }

    fn run(&self, request: WorkerRequest, emit_event: &mut dyn FnMut(WorkerEvent)) -> WorkerResult {
        emit_event(WorkerEvent {
            event_type: "worker_configured".to_string(),
            summary: format!(
                "Codex adapter configured for {}.",
                self.executable_path.display()
            ),
            payload: Some(serde_json::json!({
                "args": &self.config.args,
                "task_id": request.task_id
            })),
        });
        WorkerResult {
            status: WorkerExitStatus::Failed,
            summary: "Codex live execution is not implemented in the Rust MCP adapter yet."
                .to_string(),
            changed_files: Vec::new(),
            verification_status: Some("not_run".to_string()),
            findings: Vec::new(),
        }
    }
}

pub fn map_app_server_event(value: &Value) -> Option<WorkerEvent> {
    let event_kind = string_field(value, &["type", "kind"])
        .or_else(|| pointer_string(value, &["/msg/type", "/message/type", "/event/type"]))?;
    let summary = string_field(value, &["summary", "text", "message", "content", "output"])
        .or_else(|| pointer_string(value, &["/msg/text", "/msg/message", "/msg/content"]))
        .unwrap_or(event_kind);
    let event_type = match event_kind {
        "reasoning" | "reasoning_summary" | "reasoningSummary" => "worker_reasoning_summary",
        "commandExecution" | "tool_call" | "toolCall" | "mcpToolCall" => "worker_tool_call",
        "tool_result" | "toolResult" | "commandResult" => "worker_tool_result",
        "agent_message" | "agentMessage" | "assistant_message" | "message" => "worker_message",
        "error" => "worker_error",
        _ => "worker_event",
    };
    Some(WorkerEvent {
        event_type: event_type.to_string(),
        summary: summary.to_string(),
        payload: Some(value.clone()),
    })
}

fn resolve_executable(value: &str) -> Result<PathBuf, CodexConfigError> {
    let executable = value.trim();
    if executable.is_empty() {
        return Err(config_error("Codex executable is required."));
    }
    let path = Path::new(executable);
    if path.is_absolute() || executable.contains(std::path::MAIN_SEPARATOR) {
        if is_executable_file(path) {
            return Ok(path.to_path_buf());
        }
        return Err(config_error(format!(
            "Codex executable `{}` was not found.",
            path.display()
        )));
    }
    for directory in env::split_paths(&env::var_os("PATH").unwrap_or_default()) {
        let candidate = directory.join(executable);
        if is_executable_file(&candidate) {
            return Ok(candidate);
        }
    }
    Err(config_error(format!(
        "Codex executable `{executable}` was not found on PATH."
    )))
}

fn is_executable_file(path: &Path) -> bool {
    fs::metadata(path)
        .map(|metadata| metadata.is_file())
        .unwrap_or(false)
}

fn config_error(message: impl Into<String>) -> CodexConfigError {
    CodexConfigError {
        message: message.into(),
    }
}

fn string_field<'a>(value: &'a Value, keys: &[&str]) -> Option<&'a str> {
    for key in keys {
        if let Some(text) = value.get(*key).and_then(Value::as_str) {
            return Some(text);
        }
    }
    None
}

fn pointer_string<'a>(value: &'a Value, pointers: &[&str]) -> Option<&'a str> {
    for pointer in pointers {
        if let Some(text) = value.pointer(pointer).and_then(Value::as_str) {
            return Some(text);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_missing_executable() {
        let error = CodexWorkerAdapter::new(CodexAdapterConfig {
            executable: "definitely-missing-platypus-codex".to_string(),
            args: Vec::new(),
        })
        .expect_err("missing executable");

        assert!(error.to_string().contains("not found"));
    }

    #[test]
    fn accepts_existing_absolute_executable() {
        let current_exe = std::env::current_exe().expect("current exe");
        let adapter = CodexWorkerAdapter::new(CodexAdapterConfig {
            executable: current_exe.display().to_string(),
            args: vec!["app-server".to_string()],
        })
        .expect("adapter");

        assert_eq!(adapter.executable_path(), current_exe.as_path());
    }

    #[test]
    fn maps_app_server_events_to_worker_events() {
        let reasoning = map_app_server_event(&serde_json::json!({
            "msg": { "type": "reasoning", "text": "Inspecting files." }
        }))
        .expect("reasoning");
        let tool = map_app_server_event(&serde_json::json!({
            "type": "commandExecution",
            "summary": "Read README.md"
        }))
        .expect("tool");
        let message = map_app_server_event(&serde_json::json!({
            "kind": "agent_message",
            "message": "Done."
        }))
        .expect("message");

        assert_eq!(reasoning.event_type, "worker_reasoning_summary");
        assert_eq!(reasoning.summary, "Inspecting files.");
        assert_eq!(tool.event_type, "worker_tool_call");
        assert_eq!(message.event_type, "worker_message");
    }

    #[test]
    fn live_run_fails_closed_until_implemented() {
        let current_exe = std::env::current_exe().expect("current exe");
        let adapter = CodexWorkerAdapter::new(CodexAdapterConfig {
            executable: current_exe.display().to_string(),
            args: Vec::new(),
        })
        .expect("adapter");
        let mut events = Vec::new();
        let result = adapter.run(
            WorkerRequest {
                task_id: "task-1".to_string(),
                item_id: "PROJ-001".to_string(),
                title: "Test".to_string(),
                workspace_path: "/tmp/work".to_string(),
                brief: "brief".to_string(),
            },
            &mut |event| events.push(event),
        );

        assert_eq!(result.status, WorkerExitStatus::Failed);
        assert_eq!(events[0].event_type, "worker_configured");
    }
}
