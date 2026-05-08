pub mod claude;
pub mod codex;

use serde_json::Value;
use std::{
    io::{ErrorKind, Write},
    path::Path,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

pub(crate) const HARNESS_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Debug, Clone)]
pub struct WorkerRequest {
    pub task_id: String,
    pub item_id: String,
    pub title: String,
    pub workspace_path: String,
    pub brief: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkerExitStatus {
    Completed,
    Failed,
}

impl WorkerExitStatus {
    pub fn as_task_status(&self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone)]
pub struct WorkerEvent {
    pub event_type: String,
    pub summary: String,
    pub payload: Option<Value>,
}

#[derive(Debug, Clone)]
pub struct WorkerFinding {
    pub title: String,
    pub summary: String,
    pub severity: Option<String>,
    pub required: bool,
}

#[derive(Debug, Clone)]
pub struct WorkerResult {
    pub status: WorkerExitStatus,
    pub summary: String,
    pub changed_files: Vec<String>,
    pub verification_status: Option<String>,
    pub findings: Vec<WorkerFinding>,
}

pub trait WorkerAdapter {
    fn name(&self) -> &str;

    fn run(&self, request: WorkerRequest, emit_event: &mut dyn FnMut(WorkerEvent)) -> WorkerResult;
}

pub(crate) struct HarnessOutput {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
}

pub(crate) fn run_harness_process(
    executable: &Path,
    args: &[String],
    cwd: &str,
    stdin: &str,
    timeout: Duration,
) -> Result<HarnessOutput, String> {
    let mut child = Command::new(executable)
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("failed to start worker harness: {error}"))?;

    if let Some(mut child_stdin) = child.stdin.take() {
        if let Err(error) = child_stdin.write_all(stdin.as_bytes()) {
            if error.kind() != ErrorKind::BrokenPipe {
                return Err(format!("failed to write worker brief: {error}"));
            }
        }
    }

    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_status)) => {
                let output = child
                    .wait_with_output()
                    .map_err(|error| format!("failed to collect worker output: {error}"))?;
                return Ok(HarnessOutput {
                    success: output.status.success(),
                    stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
                    stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
                });
            }
            Ok(None) => {
                if started.elapsed() > timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err("worker harness timed out".to_string());
                }
                thread::sleep(Duration::from_millis(20));
            }
            Err(error) => return Err(format!("failed to poll worker harness: {error}")),
        }
    }
}

#[derive(Debug, Clone)]
pub struct FakeWorkerAdapter {
    name: String,
    result: WorkerResult,
}

impl FakeWorkerAdapter {
    pub fn completed(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            result: WorkerResult {
                status: WorkerExitStatus::Completed,
                summary: "Fake worker completed task.".to_string(),
                changed_files: vec!["README.md".to_string()],
                verification_status: Some("skipped".to_string()),
                findings: Vec::new(),
            },
        }
    }

    pub fn failed(name: impl Into<String>, summary: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            result: WorkerResult {
                status: WorkerExitStatus::Failed,
                summary: summary.into(),
                changed_files: Vec::new(),
                verification_status: Some("failed".to_string()),
                findings: Vec::new(),
            },
        }
    }
}

impl WorkerAdapter for FakeWorkerAdapter {
    fn name(&self) -> &str {
        &self.name
    }

    fn run(&self, request: WorkerRequest, emit_event: &mut dyn FnMut(WorkerEvent)) -> WorkerResult {
        emit_event(WorkerEvent {
            event_type: "worker_message".to_string(),
            summary: format!(
                "{} received {} in {}.",
                self.name, request.task_id, request.workspace_path
            ),
            payload: Some(serde_json::json!({
                "item_id": request.item_id,
                "title": request.title,
                "brief_bytes": request.brief.len()
            })),
        });
        self.result.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fake_adapter_emits_event_and_returns_result() {
        let adapter = FakeWorkerAdapter::completed("fake");
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

        assert_eq!(result.status, WorkerExitStatus::Completed);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, "worker_message");
    }
}
