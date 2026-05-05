use super::paths::resolve_root;
use crate::models::{ActionResult, DoctorCheck, DoctorCheckStatus, DoctorSnapshotData};
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
        "Create project config or run Platypus initialization.",
    ));
    checks.push(directory_check(
        "backlog_items",
        &root.join("backlog/items"),
        "backlog/items exists.",
        "Create backlog directory structure before using backlog tools.",
    ));
    checks.push(directory_check(
        "backlog_epics",
        &root.join("backlog/epics"),
        "backlog/epics exists.",
        "Create at least the general epic before creating backlog items.",
    ));
    checks.push(git_check(&root));
    checks.push(backlog_count_check(&root));

    let ok = checks
        .iter()
        .all(|check| matches!(check.status, DoctorCheckStatus::Pass));
    let data = DoctorSnapshotData {
        root: root.display().to_string(),
        ok,
        checks,
    };
    if ok {
        ActionResult::completed(action, "Project doctor checks passed.", data)
    } else {
        ActionResult {
            action: action.to_string(),
            status: crate::models::ActionStatus::Failed,
            summary: "Project doctor found setup issues.".to_string(),
            next_action: Some(
                "Inspect failed checks and apply the suggested next action.".to_string(),
            ),
            data: Some(data),
            error: None,
        }
    }
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
    let git_path = root.join(".git");
    if git_path.exists() {
        DoctorCheck {
            name: "git_metadata".to_string(),
            status: DoctorCheckStatus::Pass,
            summary: ".git metadata exists.".to_string(),
            next_action: None,
        }
    } else {
        DoctorCheck {
            name: "git_metadata".to_string(),
            status: DoctorCheckStatus::Warn,
            summary: ".git metadata is missing.".to_string(),
            next_action: Some(
                "Initialize git before dispatching worktree-based tasks.".to_string(),
            ),
        }
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
            next_action: Some("Ask the MCP host to draft and create backlog items.".to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{ActionStatus, DoctorCheckStatus};
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn doctor_snapshot_reports_ready_project() {
        let temp = project_fixture(true);
        fs::create_dir(temp.path().join(".git")).expect("git metadata");
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
        let data = result.data.expect("data");
        assert!(!data.ok);
        assert!(data.checks.iter().any(|check| {
            check.name == "project_config" && matches!(check.status, DoctorCheckStatus::Fail)
        }));
        assert!(data.checks.iter().any(|check| check.next_action.is_some()));
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
}
