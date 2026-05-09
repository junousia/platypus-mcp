use std::{
    fs,
    path::Path,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

const GIT_TIMEOUT: Duration = Duration::from_secs(10);
const OUTPUT_LIMIT: usize = 4_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GitReadinessStatus {
    Ready,
    MissingRepository,
    NotTopLevel,
    UnbornHead,
    Dirty,
    Error,
}

#[derive(Debug, Clone)]
pub struct GitReadiness {
    pub status: GitReadinessStatus,
    pub summary: String,
    pub next_action: Option<String>,
    pub details: Option<String>,
    pub head_commit: Option<String>,
}

impl GitReadiness {
    pub fn ready(&self) -> bool {
        matches!(self.status, GitReadinessStatus::Ready)
    }
}

pub fn inspect_git_readiness(root: &Path, include_dirty: bool) -> GitReadiness {
    let top_level = match run_git(root, &["rev-parse", "--show-toplevel"]) {
        Ok(value) => value,
        Err(error) => {
            return if looks_like_missing_repository(&error) {
                GitReadiness {
                    status: GitReadinessStatus::MissingRepository,
                    summary: "Git repository is not initialized.".to_string(),
                    next_action: Some(
                        "Run `git init`, stage the initial project files, and create an initial commit before dispatching worktree-based tasks."
                            .to_string(),
                    ),
                    details: Some(error),
                    head_commit: None,
                }
            } else {
                git_error("Could not inspect Git repository.", error)
            };
        }
    };
    let top_level = match fs::canonicalize(top_level.trim()) {
        Ok(path) => path,
        Err(error) => return git_error("Could not inspect Git top-level.", error.to_string()),
    };
    let root = match fs::canonicalize(root) {
        Ok(path) => path,
        Err(error) => return git_error("Could not inspect project root.", error.to_string()),
    };
    if top_level != root {
        return GitReadiness {
            status: GitReadinessStatus::NotTopLevel,
            summary: "Project root is not the Git top-level.".to_string(),
            next_action: Some(format!(
                "Run Platypus from the Git top-level `{}` or choose that directory as the project root.",
                top_level.display()
            )),
            details: Some(format!(
                "project root `{}` is inside Git repository `{}`",
                root.display(),
                top_level.display()
            )),
            head_commit: None,
        };
    }

    let head = match run_git(&root, &["rev-parse", "--verify", "HEAD^{commit}"]) {
        Ok(value) => value.lines().next().unwrap_or_default().trim().to_string(),
        Err(error) => {
            return if looks_like_unborn_head(&error) {
                GitReadiness {
                    status: GitReadinessStatus::UnbornHead,
                    summary: "Git repository has no initial commit.".to_string(),
                    next_action: Some(
                        "Create an initial commit before dispatching worktree-based tasks."
                            .to_string(),
                    ),
                    details: Some(error),
                    head_commit: None,
                }
            } else {
                git_error("Could not resolve HEAD.", error)
            };
        }
    };
    if head.is_empty() {
        return GitReadiness {
            status: GitReadinessStatus::UnbornHead,
            summary: "Git repository has no initial commit.".to_string(),
            next_action: Some(
                "Create an initial commit before dispatching worktree-based tasks.".to_string(),
            ),
            details: Some("HEAD did not resolve to a commit".to_string()),
            head_commit: None,
        };
    }

    if include_dirty {
        match run_git(
            &root,
            &["status", "--porcelain=v1", "--untracked-files=all"],
        ) {
            Ok(status) => {
                let status = manager_relevant_status(&status);
                if status.is_empty() {
                    return GitReadiness {
                        status: GitReadinessStatus::Ready,
                        summary: "Git repository is ready for worktree-based tasks.".to_string(),
                        next_action: None,
                        details: None,
                        head_commit: Some(head),
                    };
                }
                return GitReadiness {
                    status: GitReadinessStatus::Dirty,
                    summary: "Manager workspace has local changes.".to_string(),
                    next_action: Some(
                        "Review the manager workspace, then commit, stash, or intentionally discard local changes before integrating worker results."
                            .to_string(),
                    ),
                    details: Some(status),
                    head_commit: Some(head),
                };
            }
            Err(error) => return git_error("Could not inspect manager workspace.", error),
        }
    }

    GitReadiness {
        status: GitReadinessStatus::Ready,
        summary: "Git repository is ready for worktree-based tasks.".to_string(),
        next_action: None,
        details: None,
        head_commit: Some(head),
    }
}

fn git_error(summary: &str, error: String) -> GitReadiness {
    GitReadiness {
        status: GitReadinessStatus::Error,
        summary: summary.to_string(),
        next_action: Some("Inspect Git setup and retry the Platypus command.".to_string()),
        details: Some(error),
        head_commit: None,
    }
}

fn looks_like_missing_repository(error: &str) -> bool {
    error.contains("not a git repository") || error.contains("not in a git directory")
}

fn looks_like_unborn_head(error: &str) -> bool {
    error.contains("Needed a single revision")
        || error.contains("unknown revision")
        || error.contains("ambiguous argument")
        || error.contains("bad revision")
}

fn run_git(root: &Path, args: &[&str]) -> Result<String, String> {
    let mut child = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("failed to start git: {error}"))?;
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_status)) => {
                let output = child
                    .wait_with_output()
                    .map_err(|error| format!("failed to collect git output: {error}"))?;
                if output.status.success() {
                    return Ok(limit_output(
                        String::from_utf8_lossy(&output.stdout).as_ref(),
                    ));
                }
                let stderr = limit_output(String::from_utf8_lossy(&output.stderr).as_ref());
                return Err(if stderr.is_empty() {
                    format!("git {:?} failed with {}", args, output.status)
                } else {
                    stderr
                });
            }
            Ok(None) => {
                if started.elapsed() > GIT_TIMEOUT {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(format!("git {:?} timed out", args));
                }
                thread::sleep(Duration::from_millis(10));
            }
            Err(error) => return Err(format!("failed to poll git: {error}")),
        }
    }
}

fn limit_output(value: &str) -> String {
    let mut output = value.trim().to_string();
    if output.len() > OUTPUT_LIMIT {
        output.truncate(OUTPUT_LIMIT);
        output.push_str("...");
    }
    output
}

fn manager_relevant_status(status: &str) -> String {
    status
        .lines()
        .filter(|line| {
            let path = line.get(3..).unwrap_or_default();
            !path.starts_with(".platy/")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn reports_missing_repository() {
        let project = TempDir::new().expect("temp dir");

        let readiness = inspect_git_readiness(project.path(), false);

        assert_eq!(readiness.status, GitReadinessStatus::MissingRepository);
        assert!(readiness.summary.contains("not initialized"));
        assert!(readiness.next_action.unwrap().contains("git init"));
    }

    #[test]
    fn reports_unborn_head() {
        let project = TempDir::new().expect("temp dir");
        run_git(project.path(), &["init"]).expect("git init");

        let readiness = inspect_git_readiness(project.path(), false);

        assert_eq!(readiness.status, GitReadinessStatus::UnbornHead);
        assert!(readiness.next_action.unwrap().contains("initial commit"));
    }

    #[test]
    fn reports_dirty_workspace_when_requested() {
        let project = git_project();
        fs::write(project.path().join("local.txt"), "local\n").expect("local");

        let readiness = inspect_git_readiness(project.path(), true);

        assert_eq!(readiness.status, GitReadinessStatus::Dirty);
        assert!(readiness.next_action.unwrap().contains("commit, stash"));
    }

    fn git_project() -> TempDir {
        let project = TempDir::new().expect("temp dir");
        run_git(project.path(), &["init"]).expect("git init");
        run_git(project.path(), &["config", "user.name", "Platypus Test"]).expect("git name");
        run_git(
            project.path(),
            &["config", "user.email", "platypus@example.invalid"],
        )
        .expect("git email");
        fs::write(project.path().join("README.md"), "# Test\n").expect("readme");
        run_git(project.path(), &["add", "README.md"]).expect("git add");
        run_git(project.path(), &["commit", "-m", "Initial commit"]).expect("git commit");
        project
    }
}
