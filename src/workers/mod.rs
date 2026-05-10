pub mod claude;
pub mod codex;

use serde_json::Value;
use std::{
    io::{ErrorKind, Read, Write},
    path::Path,
    process::{Command, Stdio},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

pub(crate) const HARNESS_TIMEOUT: Duration = Duration::from_secs(120);
const HARNESS_CAPTURE_LIMIT: usize = 1024 * 1024;

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
    pub exit_code: Option<i32>,
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

    let stdout_handle = child.stdout.take().map(spawn_output_reader);
    let stderr_handle = child.stderr.take().map(spawn_output_reader);

    if let Some(mut child_stdin) = child.stdin.take() {
        if let Err(error) = child_stdin.write_all(stdin.as_bytes()) {
            if error.kind() != ErrorKind::BrokenPipe {
                let _ = child.kill();
                let _ = child.wait();
                let _ = join_output(stdout_handle);
                let _ = join_output(stderr_handle);
                return Err(format!("failed to write worker brief: {error}"));
            }
        }
    }

    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let stdout = join_output(stdout_handle);
                let stderr = join_output(stderr_handle);
                return Ok(HarnessOutput {
                    success: status.success(),
                    exit_code: status.code(),
                    stdout,
                    stderr,
                });
            }
            Ok(None) => {
                if started.elapsed() > timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    let _ = join_output(stdout_handle);
                    let _ = join_output(stderr_handle);
                    return Err("worker harness timed out".to_string());
                }
                thread::sleep(Duration::from_millis(20));
            }
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = join_output(stdout_handle);
                let _ = join_output(stderr_handle);
                return Err(format!("failed to poll worker harness: {error}"));
            }
        }
    }
}

fn spawn_output_reader<R>(mut reader: R) -> JoinHandle<Vec<u8>>
where
    R: Read + Send + 'static,
{
    thread::spawn(move || {
        let mut captured = Vec::new();
        let mut buffer = [0_u8; 8192];
        loop {
            match reader.read(&mut buffer) {
                Ok(0) => break,
                Ok(bytes_read) => {
                    let remaining = HARNESS_CAPTURE_LIMIT.saturating_sub(captured.len());
                    if remaining > 0 {
                        let to_capture = bytes_read.min(remaining);
                        captured.extend_from_slice(&buffer[..to_capture]);
                    }
                }
                Err(_) => break,
            }
        }
        captured
    })
}

fn join_output(handle: Option<JoinHandle<Vec<u8>>>) -> String {
    handle
        .and_then(|handle| handle.join().ok())
        .map(|output| String::from_utf8_lossy(&output).into_owned())
        .unwrap_or_default()
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
    use std::{path::Path, time::Duration};
    use tempfile::TempDir;

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

    #[test]
    fn run_harness_process_drains_large_output_while_running() {
        let project = TempDir::new().expect("temp dir");
        let script = "i=0; while [ \"$i\" -lt 20000 ]; do printf 1234567890; i=$((i + 1)); done";
        let output = run_harness_process(
            Path::new("sh"),
            &["-c".to_string(), script.to_string()],
            project.path().to_str().expect("utf8 path"),
            "",
            Duration::from_secs(5),
        )
        .expect("harness output");

        assert!(output.success);
        assert_eq!(output.stdout.len(), 200_000);
        assert!(output.stderr.is_empty());
    }
}
