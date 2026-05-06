use super::{WorkerAdapter, WorkerEvent, WorkerExitStatus, WorkerRequest, WorkerResult};
use serde_json::Value;
use std::{
    env, fmt, fs,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone)]
pub struct ClaudeAdapterConfig {
    pub executable: String,
    pub args: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ClaudeWorkerAdapter {
    config: ClaudeAdapterConfig,
    executable_path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaudeConfigError {
    message: String,
}

impl fmt::Display for ClaudeConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.message)
    }
}

impl std::error::Error for ClaudeConfigError {}

impl ClaudeWorkerAdapter {
    pub fn new(config: ClaudeAdapterConfig) -> Result<Self, ClaudeConfigError> {
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

impl WorkerAdapter for ClaudeWorkerAdapter {
    fn name(&self) -> &str {
        "claude"
    }

    fn run(&self, request: WorkerRequest, emit_event: &mut dyn FnMut(WorkerEvent)) -> WorkerResult {
        emit_event(WorkerEvent {
            event_type: "worker_configured".to_string(),
            summary: format!(
                "Claude adapter configured for {}.",
                self.executable_path.display()
            ),
            payload: Some(serde_json::json!({
                "args": &self.config.args,
                "task_id": request.task_id
            })),
        });
        WorkerResult {
            status: WorkerExitStatus::Failed,
            summary: "Claude live execution is not implemented in the Rust MCP adapter yet."
                .to_string(),
            changed_files: Vec::new(),
            verification_status: Some("not_run".to_string()),
            findings: Vec::new(),
        }
    }
}

pub fn map_stream_json_event(value: &Value) -> Option<WorkerEvent> {
    let event_kind = string_field(value, &["type", "kind"])?;
    let summary = string_field(value, &["summary", "text", "message", "content", "result"])
        .or_else(|| nested_text(value))
        .unwrap_or(event_kind);
    let event_type = match event_kind {
        "assistant" | "assistant_message" | "message" => "worker_message",
        "tool_use" | "toolCall" | "tool_call" => "worker_tool_call",
        "tool_result" | "toolResult" => "worker_tool_result",
        "result" => "worker_result",
        "error" => "worker_error",
        "system" => "worker_system",
        _ => "worker_event",
    };
    Some(WorkerEvent {
        event_type: event_type.to_string(),
        summary: summary.to_string(),
        payload: Some(value.clone()),
    })
}

fn nested_text(value: &Value) -> Option<&str> {
    value
        .pointer("/message/content/0/text")
        .and_then(Value::as_str)
        .or_else(|| value.pointer("/content/0/text").and_then(Value::as_str))
}

fn resolve_executable(value: &str) -> Result<PathBuf, ClaudeConfigError> {
    let executable = value.trim();
    if executable.is_empty() {
        return Err(config_error("Claude executable is required."));
    }
    let path = Path::new(executable);
    if path.is_absolute() || executable.contains(std::path::MAIN_SEPARATOR) {
        if is_executable_file(path) {
            return Ok(path.to_path_buf());
        }
        return Err(config_error(format!(
            "Claude executable `{}` was not found.",
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
        "Claude executable `{executable}` was not found on PATH."
    )))
}

fn is_executable_file(path: &Path) -> bool {
    fs::metadata(path)
        .map(|metadata| metadata.is_file())
        .unwrap_or(false)
}

fn config_error(message: impl Into<String>) -> ClaudeConfigError {
    ClaudeConfigError {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_missing_executable() {
        let error = ClaudeWorkerAdapter::new(ClaudeAdapterConfig {
            executable: "definitely-missing-platypus-claude".to_string(),
            args: Vec::new(),
        })
        .expect_err("missing executable");

        assert!(error.to_string().contains("not found"));
    }

    #[test]
    fn accepts_existing_absolute_executable() {
        let current_exe = std::env::current_exe().expect("current exe");
        let adapter = ClaudeWorkerAdapter::new(ClaudeAdapterConfig {
            executable: current_exe.display().to_string(),
            args: vec![
                "--print".to_string(),
                "--output-format=stream-json".to_string(),
            ],
        })
        .expect("adapter");

        assert_eq!(adapter.executable_path(), current_exe.as_path());
    }

    #[test]
    fn maps_stream_json_events_to_worker_events() {
        let assistant = map_stream_json_event(&serde_json::json!({
            "type": "assistant",
            "message": { "content": [{ "type": "text", "text": "Implemented." }] }
        }))
        .expect("assistant");
        let tool = map_stream_json_event(&serde_json::json!({
            "type": "tool_use",
            "summary": "Read README.md"
        }))
        .expect("tool");
        let result = map_stream_json_event(&serde_json::json!({
            "type": "result",
            "result": "done"
        }))
        .expect("result");

        assert_eq!(assistant.event_type, "worker_message");
        assert_eq!(assistant.summary, "Implemented.");
        assert_eq!(tool.event_type, "worker_tool_call");
        assert_eq!(result.event_type, "worker_result");
    }

    #[test]
    fn live_run_fails_closed_until_implemented() {
        let current_exe = std::env::current_exe().expect("current exe");
        let adapter = ClaudeWorkerAdapter::new(ClaudeAdapterConfig {
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
