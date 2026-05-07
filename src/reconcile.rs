use crate::{
    backlog, git_trailers,
    models::{ActionResult, ReconcileParams, ReconciliationData, ReconciliationGap},
    storage,
};
use rusqlite::params;
use std::{
    collections::BTreeSet,
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

const GIT_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug)]
struct CompletedTask {
    id: String,
    source_item_id: String,
}

#[derive(Debug)]
struct RequiredFinding {
    id: String,
    title: String,
}

pub fn reconcile_project(
    default_root: &Path,
    params: ReconcileParams,
) -> ActionResult<ReconciliationData> {
    let action = "reconcile_project";
    let storage = match storage::connect(default_root, params.root.as_deref()) {
        Ok(storage) => storage,
        Err(error) => {
            return ActionResult::failed(
                action,
                "Could not open reconciliation storage.",
                error.to_string(),
            )
        }
    };
    let root = storage.storage.root;
    let closed_item_ids = backlog::closed_item_ids(&root)
        .into_iter()
        .collect::<Vec<_>>();
    let completed_tasks = match query_completed_tasks(&storage.connection) {
        Ok(tasks) => tasks,
        Err(error) => {
            return ActionResult::failed(action, "Could not inspect tasks.", error.to_string())
        }
    };
    let unresolved_required = match query_unresolved_required_findings(&storage.connection) {
        Ok(findings) => findings,
        Err(error) => {
            return ActionResult::failed(action, "Could not inspect findings.", error.to_string())
        }
    };
    let mut gaps = Vec::new();

    for task in &completed_tasks {
        let verification_count =
            match query_evidence_count(&storage.connection, &task.id, "verification") {
                Ok(count) => count,
                Err(error) => {
                    return ActionResult::failed(
                        action,
                        "Could not inspect verification evidence.",
                        error.to_string(),
                    )
                }
            };
        if verification_count == 0 {
            gaps.push(ReconciliationGap {
                kind: "missing_verification_evidence".to_string(),
                summary: format!(
                    "Completed task `{}` for `{}` has no verification evidence.",
                    task.id, task.source_item_id
                ),
                next_action: "Record verification evidence or rerun verification.".to_string(),
            });
        }
        let integration_refs = match query_evidence_refs(&storage.connection, &task.id, "commit") {
            Ok(refs) => refs,
            Err(error) => {
                return ActionResult::failed(
                    action,
                    "Could not inspect integration evidence.",
                    error.to_string(),
                )
            }
        };
        if integration_refs.is_empty() {
            gaps.push(ReconciliationGap {
                kind: "missing_integration_evidence".to_string(),
                summary: format!(
                    "Completed task `{}` for `{}` has not been integrated.",
                    task.id, task.source_item_id
                ),
                next_action: "Integrate the worker result with integrate_worker_result."
                    .to_string(),
            });
            continue;
        }

        let mut closure_seen = false;
        let mut verification_seen = false;
        for commit in integration_refs
            .iter()
            .filter_map(|reference| commit_ref(reference))
        {
            match commit_trailers(&root, &commit) {
                Ok(trailers) => {
                    if trailers.closes.contains(&task.source_item_id) {
                        closure_seen = true;
                    }
                    if trailers.verification_present {
                        verification_seen = true;
                    }
                }
                Err(error) => gaps.push(ReconciliationGap {
                    kind: "invalid_integration_commit_ref".to_string(),
                    summary: format!(
                        "Integration evidence for task `{}` references commit `{commit}` that could not be inspected.",
                        task.id
                    ),
                    next_action: format!("Inspect or replace the invalid commit evidence reference: {error}"),
                }),
            }
        }
        if !closure_seen {
            gaps.push(ReconciliationGap {
                kind: "missing_closure_trailer".to_string(),
                summary: format!(
                    "Integrated task `{}` for `{}` has no matching Platypus-Closes trailer.",
                    task.id, task.source_item_id
                ),
                next_action:
                    "Create or fix an integration commit with Platypus-Closes for the source item."
                        .to_string(),
            });
        }
        if !verification_seen {
            gaps.push(ReconciliationGap {
                kind: "missing_verification_trailer".to_string(),
                summary: format!(
                    "Integrated task `{}` for `{}` has no Platypus-Verification trailer.",
                    task.id, task.source_item_id
                ),
                next_action:
                    "Create or fix an integration commit with a Platypus-Verification trailer."
                        .to_string(),
            });
        }
    }

    for finding in &unresolved_required {
        gaps.push(ReconciliationGap {
            kind: "unresolved_required_finding".to_string(),
            summary: format!(
                "Required finding `{}` is unresolved: {}.",
                finding.id, finding.title
            ),
            next_action: "Resolve, reject, defer, or mark the finding duplicate.".to_string(),
        });
    }

    let data = ReconciliationData {
        root: root.display().to_string(),
        ok: gaps.is_empty(),
        closed_item_ids,
        completed_tasks: completed_tasks.len(),
        unresolved_required_findings: unresolved_required.len(),
        gaps,
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

fn query_completed_tasks(
    connection: &rusqlite::Connection,
) -> rusqlite::Result<Vec<CompletedTask>> {
    let mut statement = connection.prepare(
        r#"
        SELECT id, source_item_id
        FROM tasks
        WHERE status = 'completed'
        ORDER BY updated_at ASC, id ASC
        "#,
    )?;
    let rows = statement.query_map([], |row| {
        Ok(CompletedTask {
            id: row.get("id")?,
            source_item_id: row.get("source_item_id")?,
        })
    })?;
    rows.collect()
}

fn query_unresolved_required_findings(
    connection: &rusqlite::Connection,
) -> rusqlite::Result<Vec<RequiredFinding>> {
    let mut statement = connection.prepare(
        r#"
        SELECT id, title
        FROM findings
        WHERE required = 1
          AND status NOT IN ('resolved', 'rejected', 'duplicate')
        ORDER BY updated_at ASC, id ASC
        "#,
    )?;
    let rows = statement.query_map([], |row| {
        Ok(RequiredFinding {
            id: row.get("id")?,
            title: row.get("title")?,
        })
    })?;
    rows.collect()
}

fn query_evidence_refs(
    connection: &rusqlite::Connection,
    task_id: &str,
    kind: &str,
) -> rusqlite::Result<Vec<String>> {
    let mut statement = connection.prepare(
        r#"
            SELECT refs_json
            FROM evidence
            WHERE source_task_id = ?1 AND kind = ?2
            ORDER BY created_at ASC, id ASC
            "#,
    )?;
    let rows = statement.query_map(params![task_id, kind], |row| {
        row.get::<_, Option<String>>("refs_json")
    })?;
    let mut refs = Vec::new();
    for row in rows {
        if let Some(refs_json) = row? {
            refs.extend(serde_json::from_str::<Vec<String>>(&refs_json).unwrap_or_default());
        }
    }
    Ok(refs)
}

fn query_evidence_count(
    connection: &rusqlite::Connection,
    task_id: &str,
    kind: &str,
) -> rusqlite::Result<i64> {
    connection.query_row(
        r#"
        SELECT COUNT(*)
        FROM evidence
        WHERE source_task_id = ?1 AND kind = ?2
        "#,
        params![task_id, kind],
        |row| row.get(0),
    )
}

fn commit_ref(reference: &str) -> Option<String> {
    reference
        .trim()
        .strip_prefix("commit:")
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

#[derive(Debug)]
struct CommitTrailers {
    closes: BTreeSet<String>,
    verification_present: bool,
}

fn commit_trailers(root: &Path, commit: &str) -> Result<CommitTrailers, String> {
    let message = run_git(root, &["show", "-s", "--format=%B", commit])?;
    let trailers = git_trailers::parse_platypus_trailers(&message);
    Ok(CommitTrailers {
        closes: trailers.closes,
        verification_present: trailers.verification_present,
    })
}

fn run_git(root: &Path, args: &[&str]) -> Result<String, String> {
    let mut child = Command::new("git")
        .args(args)
        .current_dir(root)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("could not run git: {error}"))?;
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_status)) => break,
            Ok(None) if started.elapsed() < GIT_TIMEOUT => {
                std::thread::sleep(Duration::from_millis(10));
            }
            Ok(None) => {
                let _ = child.kill();
                return Err("git command timed out".to_string());
            }
            Err(error) => return Err(format!("could not wait for git: {error}")),
        }
    }
    let output = child
        .wait_with_output()
        .map_err(|error| format!("could not collect git output: {error}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
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
