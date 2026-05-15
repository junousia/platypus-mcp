use super::paths::resolve_root;
use crate::{
    git_readiness::{inspect_git_readiness, GitReadinessStatus},
    models::{ActionResult, DoctorCheck, DoctorCheckStatus, DoctorSnapshotData},
};
use std::{fs, path::Path};

pub fn doctor_snapshot(
    default_root: &Path,
    root: Option<&str>,
) -> ActionResult<DoctorSnapshotData> {
    let action = "doctor_snapshot";
    let root = match resolve_root(default_root, root) {
        Ok(root) => root,
        Err(error) => return ActionResult::failed(action, "Could not inspect project.", error),
    };
    let mut checks = Vec::new();
    checks.push(file_check(
        "project_config",
        &root.join("platy.yaml"),
        "platy.yaml exists.",
        "Call init_project with this root to create platy.yaml and standard scaffold files, then rerun doctor_snapshot.",
    ));
    checks.push(directory_check(
        "backlog_items",
        &root.join("backlog/items"),
        "backlog/items exists.",
        "Call init_project with this root to create backlog/items and templates, then rerun doctor_snapshot.",
    ));
    checks.push(directory_check(
        "backlog_epics",
        &root.join("backlog/epics"),
        "backlog/epics exists.",
        "Call init_project with this root to create backlog/epics/general.md, then rerun doctor_snapshot.",
    ));
    checks.push(git_check(&root));
    checks.push(backlog_count_check(&root));

    let has_failures = checks
        .iter()
        .any(|check| matches!(check.status, DoctorCheckStatus::Fail));
    let ok = !has_failures;
    let data = DoctorSnapshotData {
        root: root.display().to_string(),
        ok,
        checks,
    };
    if ok {
        let summary = if data
            .checks
            .iter()
            .any(|check| matches!(check.status, DoctorCheckStatus::Warn))
        {
            "Project doctor checks passed with warnings."
        } else {
            "Project doctor checks passed."
        };
        ActionResult::completed(action, summary, data)
    } else {
        let recovery_action = doctor_recovery_action(&data.checks);
        ActionResult {
            action: action.to_string(),
            status: crate::models::ActionStatus::Failed,
            summary: "Project doctor found setup issues.".to_string(),
            next_action: Some(recovery_action.clone()),
            recovery_action: Some(recovery_action),
            data: Some(data),
            error: None,
        }
    }
}

fn doctor_recovery_action(checks: &[DoctorCheck]) -> String {
    let failed = checks
        .iter()
        .filter(|check| matches!(check.status, DoctorCheckStatus::Fail))
        .collect::<Vec<_>>();
    if failed.is_empty() {
        return "Rerun doctor_snapshot after resolving reported warnings.".to_string();
    }

    let names = failed
        .iter()
        .map(|check| check.name.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    let first_action = failed
        .iter()
        .find_map(|check| check.next_action.as_deref())
        .unwrap_or("Resolve the first failed check, then rerun doctor_snapshot.");
    format!("Resolve failed doctor checks ({names}). Start with: {first_action}")
}

fn file_check(name: &str, path: &Path, pass_summary: &str, next_action: &str) -> DoctorCheck {
    if path.is_file() {
        DoctorCheck {
            name: name.to_string(),
            status: DoctorCheckStatus::Pass,
            summary: pass_summary.to_string(),
            next_action: None,
        }
    } else {
        DoctorCheck {
            name: name.to_string(),
            status: DoctorCheckStatus::Fail,
            summary: format!("{} is missing.", path.display()),
            next_action: Some(next_action.to_string()),
        }
    }
}

fn directory_check(name: &str, path: &Path, pass_summary: &str, next_action: &str) -> DoctorCheck {
    if path.is_dir() {
        DoctorCheck {
            name: name.to_string(),
            status: DoctorCheckStatus::Pass,
            summary: pass_summary.to_string(),
            next_action: None,
        }
    } else {
        DoctorCheck {
            name: name.to_string(),
            status: DoctorCheckStatus::Fail,
            summary: format!("{} is missing.", path.display()),
            next_action: Some(next_action.to_string()),
        }
    }
}

fn git_check(root: &Path) -> DoctorCheck {
    let readiness = inspect_git_readiness(root, false);
    let status = match readiness.status {
        GitReadinessStatus::Ready => DoctorCheckStatus::Pass,
        GitReadinessStatus::MissingRepository | GitReadinessStatus::UnbornHead => {
            DoctorCheckStatus::Fail
        }
        GitReadinessStatus::NotTopLevel | GitReadinessStatus::Dirty | GitReadinessStatus::Error => {
            DoctorCheckStatus::Warn
        }
    };
    DoctorCheck {
        name: "git_readiness".to_string(),
        status,
        summary: readiness.summary,
        next_action: readiness.next_action,
    }
}

fn backlog_count_check(root: &Path) -> DoctorCheck {
    let items_dir = root.join("backlog/items");
    let count = fs::read_dir(items_dir)
        .ok()
        .into_iter()
        .flat_map(|entries| entries.filter_map(Result::ok))
        .filter(|entry| entry.path().extension().and_then(|value| value.to_str()) == Some("md"))
        .count();
    if count > 0 {
        DoctorCheck {
            name: "backlog_items_count".to_string(),
            status: DoctorCheckStatus::Pass,
            summary: format!("{} backlog item(s) found.", count),
            next_action: None,
        }
    } else {
        DoctorCheck {
            name: "backlog_items_count".to_string(),
            status: DoctorCheckStatus::Warn,
            summary: "No backlog items found.".to_string(),
            next_action: Some(
                "Use the host model to decide concrete backlog items, then call create_backlog_items and inspect_work_queue. validate_backlog is optional after successful typed creation."
                    .to_string(),
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{ActionStatus, DoctorCheckStatus};
    use std::{fs, process::Command};
    use tempfile::TempDir;

    #[test]
    fn doctor_snapshot_reports_ready_project() {
        let temp = project_fixture(true);
        init_git(temp.path());
        fs::write(temp.path().join("backlog/items/PROJ-001.md"), "# item\n").expect("backlog item");

        let root = temp.path().to_string_lossy().into_owned();
        let result = doctor_snapshot(temp.path(), Some(root.as_str()));

        assert!(matches!(result.status, ActionStatus::Completed));
        let data = result.data.expect("data");
        assert!(data.ok);
        assert!(data
            .checks
            .iter()
            .all(|check| matches!(check.status, DoctorCheckStatus::Pass)));
    }

    #[test]
    fn doctor_snapshot_reports_recovery_guidance() {
        let temp = project_fixture(false);
        let root = temp.path().to_string_lossy().into_owned();
        let result = doctor_snapshot(temp.path(), Some(root.as_str()));

        assert!(matches!(result.status, ActionStatus::Failed));
        assert!(result
            .recovery_action
            .as_deref()
            .expect("recovery action")
            .contains("Resolve failed doctor checks"));
        assert!(result
            .recovery_action
            .as_deref()
            .expect("recovery action")
            .contains("init_project"));
        let data = result.data.expect("data");
        assert!(!data.ok);
        assert!(data.checks.iter().any(|check| {
            check.name == "project_config" && matches!(check.status, DoctorCheckStatus::Fail)
        }));
        assert!(data.checks.iter().any(|check| check.next_action.is_some()));
    }

    #[test]
    fn doctor_snapshot_reports_unborn_head_recovery() {
        let temp = project_fixture(true);
        run_git(temp.path(), &["init"]);

        let root = temp.path().to_string_lossy().into_owned();
        let result = doctor_snapshot(temp.path(), Some(root.as_str()));

        assert!(matches!(result.status, ActionStatus::Failed));
        let data = result.data.expect("data");
        let git = data
            .checks
            .iter()
            .find(|check| check.name == "git_readiness")
            .expect("git readiness check");
        assert!(matches!(git.status, DoctorCheckStatus::Fail));
        assert!(git.summary.contains("no initial commit"));
        assert!(git.next_action.as_ref().unwrap().contains("initial commit"));
    }

    #[test]
    fn doctor_snapshot_treats_empty_backlog_as_warning_not_failure() {
        let temp = project_fixture(true);
        init_git(temp.path());

        let root = temp.path().to_string_lossy().into_owned();
        let result = doctor_snapshot(temp.path(), Some(root.as_str()));

        assert!(matches!(result.status, ActionStatus::Completed));
        assert!(result.summary.contains("warnings"));
        let data = result.data.expect("data");
        assert!(data.ok);
        let backlog = data
            .checks
            .iter()
            .find(|check| check.name == "backlog_items_count")
            .expect("backlog count check");
        assert!(matches!(backlog.status, DoctorCheckStatus::Warn));
        assert!(backlog.summary.contains("No backlog items"));
        assert!(backlog
            .next_action
            .as_ref()
            .expect("next action")
            .contains("create_backlog_items"));
    }

    fn project_fixture(with_backlog_dirs: bool) -> TempDir {
        let temp = TempDir::new().expect("temp dir");
        if with_backlog_dirs {
            fs::write(temp.path().join("platy.yaml"), "project: test\n").expect("config");
            fs::create_dir_all(temp.path().join("backlog/items")).expect("items dir");
            fs::create_dir_all(temp.path().join("backlog/epics")).expect("epics dir");
        }
        temp
    }

    fn init_git(root: &Path) {
        run_git(root, &["init"]);
        run_git(root, &["config", "user.name", "Platypus Test"]);
        run_git(root, &["config", "user.email", "platypus@example.invalid"]);
        fs::write(root.join("README.md"), "# Test\n").expect("readme");
        run_git(root, &["add", "README.md"]);
        run_git(root, &["commit", "-m", "Initial commit"]);
    }

    fn run_git(root: &Path, args: &[&str]) {
        let status = Command::new("git")
            .arg("-C")
            .arg(root)
            .args(args)
            .status()
            .expect("git");
        assert!(status.success(), "git {:?} failed", args);
    }
}
