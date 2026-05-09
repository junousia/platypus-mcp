use crate::{
    models::{ActionResult, ReconcileParams, ReconciliationData, ReconciliationGap},
    state::{sqlite::SqliteProjectState, ProjectState, ReconcileProjectQuery},
};
use std::path::Path;

pub fn reconcile_project(
    default_root: &Path,
    params: ReconcileParams,
) -> ActionResult<ReconciliationData> {
    let action = "reconcile_project";
    let state = match SqliteProjectState::open(default_root, params.root.as_deref()) {
        Ok(state) => state,
        Err(error) => {
            return ActionResult::failed(
                action,
                "Could not open reconciliation storage.",
                error.to_string(),
            )
        }
    };
    let snapshot = match state.reconcile_project(ReconcileProjectQuery {
        include_closed_items: true,
    }) {
        Ok(snapshot) => snapshot,
        Err(error) => {
            return ActionResult::failed(
                action,
                "Could not inspect reconciliation state.",
                error.to_string(),
            )
        }
    };
    let data = ReconciliationData {
        root: state.root().display().to_string(),
        ok: snapshot.ok,
        closed_item_ids: snapshot.closed_item_ids.into_iter().collect(),
        completed_tasks: snapshot.completed_tasks,
        unresolved_required_findings: snapshot.unresolved_required_findings,
        gaps: snapshot
            .gaps
            .into_iter()
            .map(|gap| ReconciliationGap {
                kind: gap.kind,
                summary: gap.summary,
                next_action: gap.next_action,
            })
            .collect(),
    };
    if data.ok {
        ActionResult::completed(action, "Reconciliation found no required gaps.", data)
    } else {
        ActionResult {
            action: action.to_string(),
            status: crate::models::ActionStatus::Failed,
            summary: format!("Reconciliation found {} gap(s).", data.gaps.len()),
            next_action: Some(
                "Address each reconciliation gap before claiming completion.".to_string(),
            ),
            data: Some(data),
            error: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        evidence::record_evidence,
        findings::record_finding,
        models::{ClaimNextTaskParams, RecordEvidenceParams, RecordFindingParams},
        tasks::{claim_next_task, create_task_record, finish_task, mark_task_running, NewTask},
    };
    use std::{collections::BTreeMap, fs, process::Command};
    use tempfile::TempDir;

    #[test]
    fn clean_snapshot_reports_closed_items_and_no_gaps() {
        let project = git_project_with_closure("PROJ-001");
        let task = completed_task(project.path(), "PROJ-001");
        let commit = git_stdout(project.path(), &["rev-parse", "--verify", "HEAD^{commit}"]);
        record_evidence(
            project.path(),
            RecordEvidenceParams {
                root: None,
                id: None,
                source_item_id: Some("PROJ-001".to_string()),
                source_task_id: Some(task.id.clone()),
                kind: "verification".to_string(),
                summary: "make check passed".to_string(),
                refs: vec!["commit:HEAD".to_string()],
                metadata: BTreeMap::new(),
            },
        );
        record_evidence(
            project.path(),
            RecordEvidenceParams {
                root: None,
                id: None,
                source_item_id: Some("PROJ-001".to_string()),
                source_task_id: Some(task.id),
                kind: "commit".to_string(),
                summary: "Integrated task.".to_string(),
                refs: vec![format!("commit:{commit}")],
                metadata: BTreeMap::new(),
            },
        );

        let reconciled = reconcile_project(project.path(), ReconcileParams { root: None });
        let data = reconciled.data.expect("reconcile data");

        assert!(
            matches!(reconciled.status, crate::models::ActionStatus::Completed),
            "gaps: {}",
            serde_json::to_string(&data.gaps).expect("gaps json")
        );
        assert!(data.ok);
        assert_eq!(data.closed_item_ids, vec!["PROJ-001"]);
        assert_eq!(data.completed_tasks, 1);
        assert!(data.gaps.is_empty());
    }

    #[test]
    fn reports_completed_tasks_without_integration_evidence() {
        let project = git_project_with_closure("PROJ-001");
        let task = completed_task(project.path(), "PROJ-001");
        record_evidence(
            project.path(),
            RecordEvidenceParams {
                root: None,
                id: None,
                source_item_id: Some("PROJ-001".to_string()),
                source_task_id: Some(task.id),
                kind: "verification".to_string(),
                summary: "make check passed".to_string(),
                refs: vec!["local".to_string()],
                metadata: BTreeMap::new(),
            },
        );

        let reconciled = reconcile_project(project.path(), ReconcileParams { root: None });
        let data = reconciled.data.expect("reconcile data");

        assert!(matches!(
            reconciled.status,
            crate::models::ActionStatus::Failed
        ));
        assert!(data
            .gaps
            .iter()
            .any(|gap| gap.kind == "missing_integration_evidence"));
    }

    #[test]
    fn reports_integration_commits_missing_required_trailers() {
        let project = git_project_without_trailers();
        let task = completed_task(project.path(), "PROJ-001");
        let commit = git_stdout(project.path(), &["rev-parse", "--verify", "HEAD^{commit}"]);
        record_evidence(
            project.path(),
            RecordEvidenceParams {
                root: None,
                id: None,
                source_item_id: Some("PROJ-001".to_string()),
                source_task_id: Some(task.id.clone()),
                kind: "verification".to_string(),
                summary: "make check passed".to_string(),
                refs: vec!["local".to_string()],
                metadata: BTreeMap::new(),
            },
        );
        record_evidence(
            project.path(),
            RecordEvidenceParams {
                root: None,
                id: None,
                source_item_id: Some("PROJ-001".to_string()),
                source_task_id: Some(task.id),
                kind: "commit".to_string(),
                summary: "Integrated task.".to_string(),
                refs: vec![format!("commit:{commit}")],
                metadata: BTreeMap::new(),
            },
        );

        let reconciled = reconcile_project(project.path(), ReconcileParams { root: None });
        let data = reconciled.data.expect("reconcile data");

        assert!(matches!(
            reconciled.status,
            crate::models::ActionStatus::Failed
        ));
        assert!(data
            .gaps
            .iter()
            .any(|gap| gap.kind == "missing_closure_trailer"));
        assert!(data
            .gaps
            .iter()
            .any(|gap| gap.kind == "missing_verification_trailer"));
    }

    #[test]
    fn failing_snapshot_reports_unverified_tasks_and_unresolved_findings() {
        let project = git_project_with_closure("PROJ-001");
        completed_task(project.path(), "PROJ-001");
        record_finding(
            project.path(),
            RecordFindingParams {
                root: None,
                id: None,
                source_item_id: Some("PROJ-001".to_string()),
                source_task_id: None,
                source_finding_ref: None,
                title: "Follow-up".to_string(),
                summary: "Required follow-up.".to_string(),
                severity: Some("high".to_string()),
                required: Some(true),
                evidence_refs: Vec::new(),
                metadata: BTreeMap::new(),
            },
        );

        let reconciled = reconcile_project(project.path(), ReconcileParams { root: None });
        let data = reconciled.data.expect("reconcile data");

        assert!(matches!(
            reconciled.status,
            crate::models::ActionStatus::Failed
        ));
        assert!(!data.ok);
        assert_eq!(data.gaps.len(), 3);
        assert!(data
            .gaps
            .iter()
            .any(|gap| gap.kind == "missing_verification_evidence"));
        assert!(data
            .gaps
            .iter()
            .any(|gap| gap.kind == "missing_integration_evidence"));
        assert!(data
            .gaps
            .iter()
            .any(|gap| gap.kind == "unresolved_required_finding"));
    }

    fn completed_task(root: &Path, source_item_id: &str) -> crate::models::TaskRecord {
        let task = create_task_record(
            root,
            None,
            NewTask {
                source_item_id: source_item_id.to_string(),
                title: "Completed task".to_string(),
                worker: Some("coder".to_string()),
            },
        )
        .expect("task");
        claim_next_task(
            root,
            ClaimNextTaskParams {
                root: None,
                worker: Some("coder".to_string()),
                claimant: None,
            },
        );
        mark_task_running(root, None, &task.id).expect("running");
        finish_task(root, None, &task.id, "completed").expect("finished")
    }

    fn git_project_with_closure(item_id: &str) -> TempDir {
        let project = TempDir::new().expect("temp dir");
        git(&project, &["init"]);
        git(&project, &["config", "user.name", "Platypus Test"]);
        git(
            &project,
            &["config", "user.email", "platypus@example.invalid"],
        );
        fs::write(project.path().join("README.md"), "# Test\n").expect("readme");
        git(&project, &["add", "README.md"]);
        git(
            &project,
            &[
                "commit",
                "-m",
                "Initial commit",
                "-m",
                &format!("Platypus-Closes: {item_id}\nPlatypus-Verification: make check passed"),
            ],
        );
        project
    }

    fn git_project_without_trailers() -> TempDir {
        let project = TempDir::new().expect("temp dir");
        git(&project, &["init"]);
        git(&project, &["config", "user.name", "Platypus Test"]);
        git(
            &project,
            &["config", "user.email", "platypus@example.invalid"],
        );
        fs::write(project.path().join("README.md"), "# Test\n").expect("readme");
        git(&project, &["add", "README.md"]);
        git(&project, &["commit", "-m", "Initial commit"]);
        project
    }

    fn git_stdout(root: &Path, args: &[&str]) -> String {
        let output = Command::new("git")
            .arg("-C")
            .arg(root)
            .args(args)
            .output()
            .expect("git");
        assert!(
            output.status.success(),
            "git {:?}: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    fn git(project: &TempDir, args: &[&str]) {
        let output = Command::new("git")
            .arg("-C")
            .arg(project.path())
            .args(args)
            .output()
            .expect("git");
        assert!(
            output.status.success(),
            "git {:?}: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
