use std::{
    io::{ErrorKind, Read, Write},
    path::Path,
    process::{Command, Stdio},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

const CAPTURE_LIMIT: usize = 1024 * 1024;

pub(crate) struct ProcessOutput {
    pub success: bool,
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

pub(crate) fn run_process_with_input(
    executable: &Path,
    args: &[String],
    cwd: &str,
    stdin: &str,
    timeout: Duration,
) -> Result<ProcessOutput, String> {
    let mut child = Command::new(executable)
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("failed to start process: {error}"))?;

    let stdout_handle = child.stdout.take().map(spawn_output_reader);
    let stderr_handle = child.stderr.take().map(spawn_output_reader);

    if let Some(mut child_stdin) = child.stdin.take() {
        if let Err(error) = child_stdin.write_all(stdin.as_bytes()) {
            if error.kind() != ErrorKind::BrokenPipe {
                let _ = child.kill();
                let _ = child.wait();
                let _ = join_output(stdout_handle);
                let _ = join_output(stderr_handle);
                return Err(format!("failed to write process input: {error}"));
            }
        }
    }

    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let stdout = join_output(stdout_handle);
                let stderr = join_output(stderr_handle);
                return Ok(ProcessOutput {
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
                    return Err("process timed out".to_string());
                }
                thread::sleep(Duration::from_millis(20));
            }
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = join_output(stdout_handle);
                let _ = join_output(stderr_handle);
                return Err(format!("failed to poll process: {error}"));
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
                    let remaining = CAPTURE_LIMIT.saturating_sub(captured.len());
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

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn process_runner_drains_large_output_while_running() {
        let project = TempDir::new().expect("temp dir");
        let script = "i=0; while [ \"$i\" -lt 20000 ]; do printf 1234567890; i=$((i + 1)); done";
        let output = run_process_with_input(
            Path::new("sh"),
            &["-c".to_string(), script.to_string()],
            project.path().to_str().expect("path"),
            "",
            Duration::from_secs(5),
        )
        .expect("process output");

        assert!(output.success);
        assert_eq!(output.stderr, "");
        assert_eq!(output.stdout.len(), 200_000);
    }
}
