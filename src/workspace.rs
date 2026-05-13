use crate::{
    config, events, evidence, findings,
    git_readiness::{inspect_git_readiness, GitReadinessStatus},
    models::{
        ActionResult, ActionStatus, InspectIntegrationGatesParams, IntegrateWorkerResultParams,
        IntegrationGate, IntegrationGateData, RecordEvidenceParams, ValidateFindingsParams,
        WorkerResultIntegrationData, WorktreeCleanupData, WorktreeCleanupParams,
        WorktreeCreateParams, WorktreeData, WorktreeDiffData, WorktreeDiffFile, WorktreeDiffParams,
        WorktreeStatusParams,
    },
    state::{
        sqlite::SqliteProjectState, ClearWorkspaceCommand, EvidenceQuery, ProjectState,
        ProjectStateError, RecordWorkspaceCommand, TaskQuery, TaskSnapshot,
    },
    tasks::{self, NewTaskEvent},
};
use std::{
    collections::BTreeMap,
    fs,
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

const GIT_TIMEOUT: Duration = Duration::from_secs(10);
const OUTPUT_LIMIT: usize = 4_000;

#[derive(Debug)]
struct WorkspaceTask {
    id: String,
    source_item_id: String,
    title: String,
    status: String,
    workspace_path: Option<String>,
    workspace_branch: Option<String>,
    workspace_base_ref: Option<String>,
}

#[derive(Debug)]
struct RecordedWorktree {
    task_id: String,
    source_item_id: String,
    title: String,
    task_status: String,
    path: PathBuf,
    branch: String,
    base_ref: String,
}

#[derive(Debug)]
struct ChangedFile {
    status: String,
    path: String,
    previous_path: Option<String>,
}

pub fn worktree_create(
    default_root: &Path,
    params: WorktreeCreateParams,
) -> ActionResult<WorktreeData> {
    let action = "worktree_create";
    let task_id = match clean_task_id(&params.task_id) {
        Ok(task_id) => task_id,
        Err(error) => return ActionResult::failed(action, "Could not create worktree.", error),
    };
    let base_ref = match clean_base_ref(params.base_ref.as_deref()) {
        Ok(base_ref) => base_ref,
        Err(error) => return ActionResult::failed(action, "Could not create worktree.", error),
    };
    let state = match SqliteProjectState::open(default_root, params.root.as_deref()) {
        Ok(state) => state,
        Err(error) => {
            return ActionResult::failed(
                action,
                "Could not open workspace storage.",
                error.to_string(),
            )
        }
    };
    let root = state.root().to_path_buf();
    let root_string = root.display().to_string();
    let git_readiness = inspect_git_readiness(&root, true);
    if !git_readiness.ready() {
        return ActionResult {
            action: action.to_string(),
            status: ActionStatus::Failed,
            summary: git_readiness.summary,
            next_action: git_readiness.next_action.clone(),
            recovery_action: git_readiness.next_action,
            data: None,
            error: git_readiness.details,
        };
    }

    let task = match load_task(&state, &task_id) {
        Ok(task) => task,
        Err(WorkspaceLoadError::Missing) => {
            return ActionResult::skipped(
                action,
                format!("Task `{task_id}` was not found."),
                "Dispatch work before creating a task worktree.",
            )
        }
        Err(WorkspaceLoadError::Backend(error)) => {
            return ActionResult::failed(action, "Could not inspect task.", error)
        }
    };
    if let Some(existing) = existing_workspace_data(action, &root, &task, false, true) {
        return existing;
    }

    let safe_task_id = safe_task_component(&task_id);
    let worktrees_dir = match prepare_worktrees_dir(&root) {
        Ok(path) => path,
        Err(error) => return ActionResult::failed(action, "Could not create worktree.", error),
    };
    let worktree_path = worktrees_dir.join(&safe_task_id);
    if worktree_path.exists() {
        return ActionResult::failed(
            action,
            "Could not create worktree.",
            format!(
                "{} already exists without matching task workspace metadata",
                worktree_path.display()
            ),
        );
    }

    let commit = match resolve_base_commit(&root, &base_ref) {
        Ok(commit) => commit,
        Err(error) => return ActionResult::failed(action, "Could not create worktree.", error),
    };
    let branch = format!("platy/task/{safe_task_id}");
    let worktree_path_arg = path_arg(&worktree_path);
    let add_result = if branch_exists(&root, &branch) {
        run_git(
            &root,
            &[
                "worktree",
                "add",
                worktree_path_arg.as_str(),
                branch.as_str(),
            ],
        )
    } else {
        run_git(
            &root,
            &[
                "worktree",
                "add",
                "-b",
                branch.as_str(),
                worktree_path_arg.as_str(),
                commit.as_str(),
            ],
        )
    };
    if let Err(error) = add_result {
        return ActionResult::failed(action, "Could not create worktree.", error);
    }
    let canonical_worktree = match fs::canonicalize(&worktree_path) {
        Ok(path) => path,
        Err(error) => {
            return ActionResult::failed(
                action,
                "Could not inspect created worktree.",
                error.to_string(),
            )
        }
    };
    if !canonical_worktree.starts_with(&worktrees_dir) {
        return ActionResult::failed(
            action,
            "Created worktree escaped project state.",
            format!(
                "{} is outside {}",
                canonical_worktree.display(),
                worktrees_dir.display()
            ),
        );
    }

    if let Err(error) = state.record_workspace(RecordWorkspaceCommand {
        task_id: task_id.clone(),
        path: canonical_worktree.display().to_string(),
        branch: branch.clone(),
        base_ref: commit.clone(),
    }) {
        return ActionResult::failed(
            action,
            "Could not persist worktree metadata.",
            error.to_string(),
        );
    }
    if let Err(error) = tasks::record_task_event(
        default_root,
        Some(root_string.as_str()),
        NewTaskEvent {
            task_id: task_id.clone(),
            sequence: None,
            event_type: "worktree_created".to_string(),
            summary: format!("Created worktree for task `{task_id}`."),
            payload: Some(serde_json::json!({
                "path": canonical_worktree.display().to_string(),
                "branch": branch,
                "base_ref": commit
            })),
        },
    ) {
        return ActionResult::failed(action, "Could not record worktree event.", error);
    }

    ActionResult::completed(
        action,
        format!("Created worktree for task `{task_id}`."),
        WorktreeData {
            root: root_string,
            task_id,
            path: canonical_worktree.display().to_string(),
            branch,
            base_ref: commit,
            ready: true,
            exists: true,
            created: true,
        },
    )
}

pub fn worktree_status(
    default_root: &Path,
    params: WorktreeStatusParams,
) -> ActionResult<WorktreeData> {
    let action = "worktree_status";
    let task_id = match clean_task_id(&params.task_id) {
        Ok(task_id) => task_id,
        Err(error) => return ActionResult::failed(action, "Could not inspect worktree.", error),
    };
    let state = match SqliteProjectState::open(default_root, params.root.as_deref()) {
        Ok(state) => state,
        Err(error) => {
            return ActionResult::failed(
                action,
                "Could not open workspace storage.",
                error.to_string(),
            )
        }
    };
    let root = state.root().to_path_buf();
    let task = match load_task(&state, &task_id) {
        Ok(task) => task,
        Err(WorkspaceLoadError::Missing) => {
            return ActionResult::skipped(
                action,
                format!("Task `{task_id}` was not found."),
                "Dispatch work before inspecting a task worktree.",
            )
        }
        Err(WorkspaceLoadError::Backend(error)) => {
            return ActionResult::failed(action, "Could not inspect task.", error)
        }
    };
    existing_workspace_data(action, &root, &task, false, false).unwrap_or_else(|| {
        ActionResult::skipped(
            action,
            format!("Task `{task_id}` does not have a worktree yet."),
            "Run worktree_create for the task.",
        )
    })
}

pub fn worktree_diff(
    default_root: &Path,
    params: WorktreeDiffParams,
) -> ActionResult<WorktreeDiffData> {
    let action = "worktree_diff";
    let task_id = match clean_task_id(&params.task_id) {
        Ok(task_id) => task_id,
        Err(error) => {
            return ActionResult::failed(action, "Could not inspect worktree diff.", error)
        }
    };
    let state = match SqliteProjectState::open(default_root, params.root.as_deref()) {
        Ok(state) => state,
        Err(error) => {
            return ActionResult::failed(
                action,
                "Could not open workspace storage.",
                error.to_string(),
            )
        }
    };
    let root = state.root().to_path_buf();
    let worktree = match recorded_worktree(action, &state, &root, &task_id) {
        Ok(worktree) => worktree,
        Err(result) => return result,
    };
    let status = match run_git(
        &worktree.path,
        &["status", "--porcelain=v1", "--untracked-files=all"],
    ) {
        Ok(status) => status,
        Err(error) => return ActionResult::failed(action, "Could not inspect worktree.", error),
    };
    let mut files = parse_status_files(&status);
    let dirty = !files.is_empty();
    let committed_files = match changed_files_between(&root, &worktree.base_ref, &worktree.branch) {
        Ok(files) => files,
        Err(error) => {
            return ActionResult::failed(action, "Could not inspect worktree diff.", error)
        }
    };
    if !committed_files.is_empty() {
        merge_committed_files(&mut files, &committed_files);
    }
    let has_committed_changes = !committed_files.is_empty();
    let meaningful_changes = dirty || has_committed_changes;
    let state_reason = worktree_change_reason(&worktree, dirty, has_committed_changes);
    let mut diff_sections = Vec::new();
    if dirty {
        let workspace_diff = match run_git(&worktree.path, &["diff", "--stat", "--patch"]) {
            Ok(diff) => diff,
            Err(error) => {
                return ActionResult::failed(action, "Could not inspect worktree diff.", error)
            }
        };
        if !workspace_diff.trim().is_empty() {
            diff_sections.push(workspace_diff);
        }
        let untracked_diff = untracked_file_diff(&worktree.path, &files);
        if !untracked_diff.trim().is_empty() {
            diff_sections.push(untracked_diff);
        }
    }
    if has_committed_changes {
        let range = format!("{}..{}", worktree.base_ref, worktree.branch);
        let committed_diff = match run_git(&root, &["diff", "--stat", "--patch", range.as_str()]) {
            Ok(diff) => diff,
            Err(error) => {
                return ActionResult::failed(action, "Could not inspect worktree diff.", error)
            }
        };
        if !committed_diff.trim().is_empty() {
            diff_sections.push(committed_diff);
        }
    }
    let diff = if diff_sections.is_empty() {
        String::new()
    } else {
        diff_sections.join("\n\n")
    };
    let truncated = diff.ends_with("...");
    let data = WorktreeDiffData {
        root: root.display().to_string(),
        task_id: worktree.task_id,
        path: worktree.path.display().to_string(),
        dirty,
        meaningful_changes,
        state_reason,
        files,
        diff,
        truncated,
    };
    ActionResult::completed(
        action,
        format!("Inspected worktree diff for task `{task_id}`."),
        data,
    )
}

pub fn worktree_cleanup(
    default_root: &Path,
    params: WorktreeCleanupParams,
) -> ActionResult<WorktreeCleanupData> {
    let action = "worktree_cleanup";
    let task_id = match clean_task_id(&params.task_id) {
        Ok(task_id) => task_id,
        Err(error) => return ActionResult::failed(action, "Could not clean up worktree.", error),
    };
    let force = params.force.unwrap_or(false);
    let state = match SqliteProjectState::open(default_root, params.root.as_deref()) {
        Ok(state) => state,
        Err(error) => {
            return ActionResult::failed(
                action,
                "Could not open workspace storage.",
                error.to_string(),
            )
        }
    };
    let root = state.root().to_path_buf();
    let root_string = root.display().to_string();
    let worktree = match recorded_worktree(action, &state, &root, &task_id) {
        Ok(worktree) => worktree,
        Err(result) => return result,
    };
    let status = match run_git(
        &worktree.path,
        &["status", "--porcelain=v1", "--untracked-files=all"],
    ) {
        Ok(status) => status,
        Err(error) => return ActionResult::failed(action, "Could not inspect worktree.", error),
    };
    if !status.trim().is_empty() && !force {
        return ActionResult {
            action: action.to_string(),
            status: ActionStatus::Skipped,
            summary: format!("Task `{task_id}` worktree has local changes."),
            next_action: Some("Inspect worktree_diff, then retry with force=true only if the changes can be discarded.".to_string()),
            recovery_action: None,
            data: None,
            error: None,
        };
    }

    let path = path_arg(&worktree.path);
    let remove = if force {
        run_git(&root, &["worktree", "remove", "--force", path.as_str()])
    } else {
        run_git(&root, &["worktree", "remove", path.as_str()])
    };
    if let Err(error) = remove {
        return ActionResult::failed(action, "Could not remove worktree.", error);
    }
    if let Err(error) = state.clear_workspace(ClearWorkspaceCommand {
        task_id: task_id.clone(),
    }) {
        return ActionResult::failed(
            action,
            "Could not clear worktree metadata.",
            error.to_string(),
        );
    }
    if let Err(error) = tasks::record_task_event(
        default_root,
        Some(root_string.as_str()),
        NewTaskEvent {
            task_id: task_id.clone(),
            sequence: None,
            event_type: "worktree_cleaned_up".to_string(),
            summary: format!("Cleaned up worktree for task `{task_id}`."),
            payload: Some(serde_json::json!({
                "path": worktree.path.display().to_string(),
                "branch": worktree.branch,
                "base_ref": worktree.base_ref,
                "forced": force
            })),
        },
    ) {
        return ActionResult::failed(action, "Could not record cleanup event.", error);
    }

    ActionResult::completed(
        action,
        format!("Cleaned up worktree for task `{task_id}`."),
        WorktreeCleanupData {
            root: root_string,
            task_id,
            path: worktree.path.display().to_string(),
            removed: true,
            forced: force,
        },
    )
}

pub fn integrate_worker_result(
    default_root: &Path,
    params: IntegrateWorkerResultParams,
) -> ActionResult<WorkerResultIntegrationData> {
    let action = "integrate_worker_result";
    let task_id = match clean_task_id(&params.task_id) {
        Ok(task_id) => task_id,
        Err(error) => {
            return ActionResult::failed(action, "Could not integrate worker result.", error)
        }
    };
    let state = match SqliteProjectState::open(default_root, params.root.as_deref()) {
        Ok(state) => state,
        Err(error) => {
            return ActionResult::failed(
                action,
                "Could not open workspace storage.",
                error.to_string(),
            )
        }
    };
    let root = state.root().to_path_buf();
    let root_string = root.display().to_string();
    let readiness = inspect_git_readiness(&root, false);
    if !readiness.ready() {
        return ActionResult {
            action: action.to_string(),
            status: ActionStatus::Failed,
            summary: readiness.summary,
            next_action: readiness.next_action.clone(),
            recovery_action: readiness.next_action,
            data: None,
            error: readiness.details,
        };
    }
    let config = match config::effective_workflow_config(&root) {
        Ok(config) => config,
        Err(error) => {
            return ActionResult::failed(action, "Could not inspect workflow config.", error)
        }
    };
    if let Some(operation) = in_progress_git_operation(&root) {
        return ActionResult::skipped(
            action,
            format!("Git operation `{operation}` is already in progress."),
            "Finish or abort the current Git operation before integrating worker results.",
        );
    }
    let strategy = match integration_strategy(params.strategy.as_deref(), &config.merge_style) {
        Ok(strategy) => strategy,
        Err(error) => {
            return ActionResult::failed(action, "Could not integrate worker result.", error)
        }
    };
    let worktree = match recorded_worktree(action, &state, &root, &task_id) {
        Ok(worktree) => worktree,
        Err(result) => return result,
    };
    if worktree.task_status != "completed" {
        return ActionResult::skipped(
            action,
            format!("Task `{task_id}` is `{}`.", worktree.task_status),
            integration_prerequisite_guidance(&worktree.task_status),
        );
    }
    let verification = match latest_verification_summary(&state, &task_id) {
        Some(summary) => summary,
        None if config.require_verification_evidence && !params.allow_unverified.unwrap_or(false) => {
            return ActionResult::skipped(
                action,
                format!("Task `{task_id}` has no verification evidence."),
                "Record verification evidence before integrating worker results, or retry with allow_unverified=true for low-risk work.",
            )
        }
        None => "verification not recorded".to_string(),
    };
    let findings_validation = findings::validate_findings(
        default_root,
        ValidateFindingsParams {
            root: Some(root_string.clone()),
            source_item_id: Some(worktree.source_item_id.clone()),
            source_task_id: Some(task_id.clone()),
        },
    );
    match findings_validation {
        ActionResult {
            status: ActionStatus::Completed,
            ..
        } => {}
        ActionResult {
            status: ActionStatus::Failed,
            data: Some(data),
            error,
            ..
        } => {
            return ActionResult {
                action: action.to_string(),
                status: ActionStatus::Skipped,
                summary: format!(
                    "Task `{task_id}` has {} open required finding(s).",
                    data.unresolved_required_count
                ),
                next_action: Some(
                    "Accept, defer, resolve, reject, or mark required findings as duplicate before integrating worker results."
                        .to_string(),
                ),
                recovery_action: None,
                data: None,
                error,
            }
        }
        ActionResult { summary, error, .. } => {
            return ActionResult::failed(
                action,
                "Could not validate task findings before integration.",
                error.unwrap_or(summary),
            )
        }
    }
    if config.require_clean_manager_workspace {
        let readiness = inspect_git_readiness(&root, true);
        match readiness.status {
            GitReadinessStatus::Ready => {}
            GitReadinessStatus::Dirty => {
                return ActionResult::skipped(
                    action,
                    readiness.summary,
                    &format!(
                        "{}\n{}",
                        readiness.next_action.unwrap_or_else(|| {
                            "Review manager workspace changes before integration.".to_string()
                        }),
                        readiness.details.unwrap_or_default()
                    ),
                )
            }
            _ => {
                return ActionResult {
                    action: action.to_string(),
                    status: ActionStatus::Failed,
                    summary: readiness.summary,
                    next_action: readiness.next_action.clone(),
                    recovery_action: readiness.next_action,
                    data: None,
                    error: readiness.details,
                }
            }
        }
    }

    let worker_dirty = match run_git(
        &worktree.path,
        &["status", "--porcelain=v1", "--untracked-files=all"],
    ) {
        Ok(status) => !status.trim().is_empty(),
        Err(error) => {
            return ActionResult::failed(action, "Could not inspect worker worktree.", error)
        }
    };
    if worker_dirty {
        if let Err(error) = commit_worker_changes(&worktree, &strategy, &verification) {
            return ActionResult::failed(action, "Could not commit worker changes.", error);
        }
    }
    let ahead = match branch_ahead_count(&root, &worktree.base_ref, &worktree.branch) {
        Ok(ahead) => ahead,
        Err(error) => return ActionResult::failed(action, "Could not inspect task branch.", error),
    };
    if ahead == 0 {
        return ActionResult::skipped(
            action,
            format!("Task `{task_id}` branch has no changes to integrate."),
            "Inspect the task worktree before integrating. Ensure edits were made in the task worktree branch, not in the manager workspace.",
        );
    }

    let commit = match strategy.as_str() {
        "fast_forward" => match run_git(&root, &["merge", "--ff-only", worktree.branch.as_str()]) {
            Ok(_) => head_commit(&root),
            Err(error) => Err(error),
        },
        "squash" => integrate_squash(&root, &worktree, &verification),
        "merge_commit" => integrate_merge_commit(&root, &worktree, &verification),
        "apply_changed_files" => integrate_apply_changed_files(&root, &worktree, &verification),
        other => Err(format!("unsupported merge style `{other}`")),
    };
    let commit = match commit {
        Ok(commit) => commit,
        Err(error) => {
            let conflicts = conflicted_files(&root);
            let _ = run_git(&root, &["merge", "--abort"]);
            let _ = run_git(&root, &["reset", "--merge"]);
            let detail = if conflicts.is_empty() {
                error
            } else {
                format!("{error}\nconflicting files: {}", conflicts.join(", "))
            };
            return ActionResult {
                action: action.to_string(),
                status: ActionStatus::Failed,
                summary: "Could not integrate worker result.".to_string(),
                next_action: Some("Resolve the source conflict, or retry with strategy=apply_changed_files when replacing changed worker files is acceptable.".to_string()),
                recovery_action: None,
                data: None,
                error: Some(detail),
            };
        }
    };

    if let Err(error) = tasks::record_task_event(
        default_root,
        Some(root_string.as_str()),
        NewTaskEvent {
            task_id: task_id.clone(),
            sequence: None,
            event_type: "worker_result_integrated".to_string(),
            summary: format!("Integrated worker result for task `{task_id}`."),
            payload: Some(serde_json::json!({
                "branch": worktree.branch.clone(),
                "commit": commit.clone(),
                "merge_style": strategy.clone(),
                "source_item_id": worktree.source_item_id.clone()
            })),
        },
    ) {
        return ActionResult::failed(action, "Could not record integration event.", error);
    }
    let evidence_result = evidence::record_evidence(
        default_root,
        RecordEvidenceParams {
            root: Some(root_string.clone()),
            id: None,
            source_item_id: Some(worktree.source_item_id.clone()),
            source_task_id: Some(task_id.clone()),
            kind: "commit".to_string(),
            summary: format!("Integrated task `{task_id}` with `{}`.", config.merge_style),
            refs: vec![format!("commit:{commit}")],
            metadata: BTreeMap::from([
                (
                    "merge_style".to_string(),
                    serde_json::Value::String(strategy.clone()),
                ),
                (
                    "branch".to_string(),
                    serde_json::Value::String(worktree.branch.clone()),
                ),
            ]),
        },
    );
    if !matches!(evidence_result.status, ActionStatus::Completed) {
        return ActionResult::failed(
            action,
            "Worker result was integrated but commit evidence could not be recorded.",
            evidence_result
                .error
                .unwrap_or_else(|| evidence_result.summary),
        );
    }
    if let Err(error) = events::record_event(
        default_root,
        Some(root_string.as_str()),
        events::NewEvent {
            event_type: "worker_result_integrated".to_string(),
            scope: "project".to_string(),
            task_id: Some(task_id.clone()),
            summary: format!("Integrated worker result for task `{task_id}`."),
            payload: Some(serde_json::json!({
                "branch": worktree.branch.clone(),
                "commit": commit.clone(),
                "merge_style": strategy.clone(),
                "source_item_id": worktree.source_item_id.clone()
            })),
        },
    ) {
        return ActionResult::failed(
            action,
            "Worker result was integrated but project event could not be recorded.",
            error,
        );
    }

    let (cleaned_up, cleanup_error) = if params.cleanup_after.unwrap_or(false) {
        match cleanup_integrated_worktree(&state, &root, &worktree) {
            Ok(()) => (true, None),
            Err(error) => (false, Some(error)),
        }
    } else {
        (false, None)
    };
    let next_action = if let Some(error) = cleanup_error.as_ref() {
        Some(format!(
            "Worker result was integrated, but cleanup failed: {error}"
        ))
    } else if params.cleanup_after.unwrap_or(false) {
        Some("Worker result was integrated and the clean worktree was removed.".to_string())
    } else {
        Some(
            "Worker result was integrated. Run worktree_cleanup for the task when the worktree is no longer needed, or pass cleanup_after=true on future integrations."
                .to_string(),
        )
    };

    ActionResult {
        action: action.to_string(),
        status: ActionStatus::Completed,
        summary: format!("Integrated worker result for task `{task_id}`."),
        next_action,
        recovery_action: None,
        data: Some(WorkerResultIntegrationData {
            root: root_string,
            task_id,
            source_item_id: worktree.source_item_id,
            merge_style: strategy,
            branch: worktree.branch,
            commit,
            cleaned_up,
            cleanup_error,
        }),
        error: None,
    }
}

pub fn inspect_integration_gates(
    default_root: &Path,
    params: InspectIntegrationGatesParams,
) -> ActionResult<IntegrationGateData> {
    let action = "inspect_integration_gates";
    let task_id = match clean_task_id(&params.task_id) {
        Ok(task_id) => task_id,
        Err(error) => {
            return ActionResult::failed(action, "Could not inspect integration gates.", error)
        }
    };
    let state = match SqliteProjectState::open(default_root, params.root.as_deref()) {
        Ok(state) => state,
        Err(error) => {
            return ActionResult::failed(action, "Could not open project state.", error.to_string())
        }
    };
    let root = state.root().to_path_buf();
    let root_string = root.display().to_string();
    let config = match config::effective_workflow_config(&root) {
        Ok(config) => config,
        Err(error) => {
            return ActionResult::failed(action, "Could not inspect workflow config.", error)
        }
    };

    let mut gates = Vec::new();
    let task = match load_task(&state, &task_id) {
        Ok(task) => {
            let (status, blocking, summary, tool, next_action) = if task.status == "completed" {
                (
                    "ready",
                    false,
                    format!("Task `{task_id}` is completed."),
                    None,
                    None,
                )
            } else {
                (
                    "blocked",
                    true,
                    format!("Task `{task_id}` is `{}`.", task.status),
                    Some(lifecycle_recovery_tool(&task.status)),
                    Some(integration_prerequisite_guidance(&task.status).to_string()),
                )
            };
            gates.push(integration_gate(
                "task_lifecycle",
                status,
                blocking,
                summary,
                tool,
                next_action,
            ));
            Some(task)
        }
        Err(WorkspaceLoadError::Missing) => {
            gates.push(integration_gate(
                "task_lifecycle",
                "blocked",
                true,
                format!("Task `{task_id}` was not found."),
                Some("dispatch_ready_work"),
                Some("Dispatch backlog work before inspecting integration gates.".to_string()),
            ));
            None
        }
        Err(WorkspaceLoadError::Backend(error)) => {
            return ActionResult::failed(action, "Could not inspect task.", error)
        }
    };

    let source_item_id = task.as_ref().map(|task| task.source_item_id.clone());
    let worktree = if task.is_some() {
        match recorded_worktree::<IntegrationGateData>(action, &state, &root, &task_id) {
            Ok(worktree) => {
                gates.push(integration_gate(
                    "worktree",
                    "ready",
                    false,
                    format!(
                        "Task `{task_id}` has recorded worktree `{}`.",
                        worktree.path.display()
                    ),
                    None,
                    None,
                ));
                Some(worktree)
            }
            Err(result) => {
                gates.push(integration_gate(
                    "worktree",
                    "blocked",
                    true,
                    result.summary,
                    Some("worktree_create"),
                    result.next_action,
                ));
                None
            }
        }
    } else {
        None
    };

    if let Some(operation) = in_progress_git_operation(&root) {
        gates.push(integration_gate(
            "git_operation",
            "blocked",
            true,
            format!("Git operation `{operation}` is already in progress."),
            None,
            Some(
                "Finish or abort the current Git operation before integrating worker results."
                    .to_string(),
            ),
        ));
    } else {
        gates.push(integration_gate(
            "git_operation",
            "ready",
            false,
            "No in-progress Git operation was detected.".to_string(),
            None,
            None,
        ));
    }

    if config.require_clean_manager_workspace {
        let readiness = inspect_git_readiness(&root, true);
        match readiness.status {
            GitReadinessStatus::Ready => gates.push(integration_gate(
                "manager_workspace",
                "ready",
                false,
                readiness.summary,
                None,
                None,
            )),
            GitReadinessStatus::Dirty => gates.push(integration_gate(
                "manager_workspace",
                "blocked",
                true,
                readiness.summary,
                None,
                readiness.next_action,
            )),
            _ => gates.push(integration_gate(
                "manager_workspace",
                "blocked",
                true,
                readiness.summary,
                Some("doctor_snapshot"),
                readiness.next_action.or(readiness.details),
            )),
        }
    } else {
        gates.push(integration_gate(
            "manager_workspace",
            "ready",
            false,
            "Clean manager workspace is not required by workflow policy.".to_string(),
            None,
            None,
        ));
    }

    match latest_verification_summary(&state, &task_id) {
        Some(summary) => gates.push(integration_gate(
            "verification",
            "ready",
            false,
            format!("Verification evidence is recorded: {summary}"),
            None,
            None,
        )),
        None if config.require_verification_evidence => gates.push(integration_gate(
            "verification",
            "blocked",
            true,
            format!("Task `{task_id}` has no verification evidence."),
            Some("record_verification_evidence"),
            Some(
                "Run verification with run_task_verification or record verification evidence before integrating, or retry integration with allow_unverified=true for low-risk work."
                    .to_string(),
            ),
        )),
        None => gates.push(integration_gate(
            "verification",
            "warning",
            false,
            "Verification evidence is not required and was not recorded.".to_string(),
            Some("record_verification_evidence"),
            Some("Record verification evidence when useful before integrating.".to_string()),
        )),
    }

    if let Some(source_item_id) = source_item_id.as_ref() {
        match findings::validate_findings(
            default_root,
            ValidateFindingsParams {
                root: Some(root_string.clone()),
                source_item_id: Some(source_item_id.clone()),
                source_task_id: Some(task_id.clone()),
            },
        ) {
            ActionResult {
                status: ActionStatus::Completed,
                ..
            } => gates.push(integration_gate(
                "findings",
                "ready",
                false,
                "No open required findings block integration.".to_string(),
                None,
                None,
            )),
            ActionResult {
                status: ActionStatus::Failed,
                data: Some(data),
                error,
                ..
            } => gates.push(integration_gate(
                "findings",
                "blocked",
                true,
                format!(
                    "{} required finding(s) still need disposition.",
                    data.unresolved_required_count
                ),
                Some("update_finding_disposition"),
                Some(error.unwrap_or_else(|| {
                    "Accept, defer, resolve, reject, or mark required findings as duplicate before integrating."
                        .to_string()
                })),
            )),
            ActionResult { summary, error, .. } => gates.push(integration_gate(
                "findings",
                "blocked",
                true,
                format!("Could not validate findings: {}", error.unwrap_or(summary)),
                Some("validate_findings"),
                Some("Inspect findings before integrating worker results.".to_string()),
            )),
        }
    } else {
        gates.push(integration_gate(
            "findings",
            "blocked",
            true,
            "Task source item is unknown, so required findings cannot be checked.".to_string(),
            Some("inspect_task"),
            Some("Inspect or recreate the task before integrating worker results.".to_string()),
        ));
    }

    if let Some(worktree) = worktree.as_ref() {
        let dirty = match run_git(
            &worktree.path,
            &["status", "--porcelain=v1", "--untracked-files=all"],
        ) {
            Ok(status) => !status.trim().is_empty(),
            Err(error) => {
                gates.push(integration_gate(
                    "branch_changes",
                    "blocked",
                    true,
                    format!("Could not inspect worker worktree: {error}"),
                    Some("worktree_status"),
                    Some("Inspect the task worktree before integrating.".to_string()),
                ));
                false
            }
        };
        if !gates.iter().any(|gate| gate.name == "branch_changes") {
            match branch_ahead_count(&root, &worktree.base_ref, &worktree.branch) {
                Ok(ahead) if dirty || ahead > 0 => gates.push(integration_gate(
                    "branch_changes",
                    "ready",
                    false,
                    if dirty {
                        "Worker worktree has local changes ready to commit during integration."
                            .to_string()
                    } else {
                        format!("Worker branch is {ahead} commit(s) ahead of `{}`.", worktree.base_ref)
                    },
                    Some("worktree_diff"),
                    None,
                )),
                Ok(_) => gates.push(integration_gate(
                    "branch_changes",
                    "blocked",
                    true,
                    format!("Task `{task_id}` branch has no changes to integrate."),
                    Some("worktree_diff"),
                    Some("Inspect the task worktree and ensure edits were made in the task branch, not the manager workspace.".to_string()),
                )),
                Err(error) => gates.push(integration_gate(
                    "branch_changes",
                    "blocked",
                    true,
                    format!("Could not inspect task branch: {error}"),
                    Some("worktree_status"),
                    Some("Inspect the task worktree before integrating.".to_string()),
                )),
            }
        }
    }

    let ok = gates.iter().all(|gate| !gate.blocking);
    let next_action = gates
        .iter()
        .find(|gate| gate.blocking)
        .and_then(|gate| gate.next_action.clone())
        .unwrap_or_else(|| format!("Task `{task_id}` is ready for integrate_worker_result."));
    let summary = if ok {
        format!("Task `{task_id}` is ready for integration.")
    } else {
        format!(
            "Task `{task_id}` has {} blocking integration gate(s).",
            gates.iter().filter(|gate| gate.blocking).count()
        )
    };

    ActionResult {
        action: action.to_string(),
        status: ActionStatus::Completed,
        summary,
        next_action: Some(next_action.clone()),
        recovery_action: None,
        data: Some(IntegrationGateData {
            root: root_string,
            task_id,
            source_item_id,
            ok,
            gates,
            next_action,
        }),
        error: None,
    }
}

fn integration_gate(
    name: &str,
    status: &str,
    blocking: bool,
    summary: String,
    recommended_tool: Option<&str>,
    next_action: Option<String>,
) -> IntegrationGate {
    IntegrationGate {
        name: name.to_string(),
        status: status.to_string(),
        blocking,
        summary,
        recommended_tool: recommended_tool.map(str::to_string),
        next_action,
    }
}

fn lifecycle_recovery_tool(task_status: &str) -> &'static str {
    match task_status {
        "queued" => "prepare_work",
        "claimed" | "running" => "finish_work",
        "failed" | "cancelled" => "inspect_task_events",
        _ => "inspect_task",
    }
}

fn latest_verification_summary(state: &SqliteProjectState, task_id: &str) -> Option<String> {
    state
        .list_evidence(EvidenceQuery {
            source_item_id: None,
            source_task_id: Some(task_id.to_string()),
            kind: Some("verification".to_string()),
            limit: Some(200),
        })
        .ok()
        .and_then(|snapshot| snapshot.evidence.into_iter().last())
        .map(|evidence| evidence.summary)
}

fn commit_worker_changes(
    worktree: &RecordedWorktree,
    merge_style: &str,
    verification: &str,
) -> Result<(), String> {
    run_git(&worktree.path, &["add", "--all"])?;
    let title = format!(
        "Worker result for {}: {}",
        worktree.source_item_id, worktree.title
    );
    if merge_style == "fast_forward" {
        commit_with_message(
            &worktree.path,
            &title,
            &[
                format!("Platypus-Closes: {}", worktree.source_item_id),
                format!("Platypus-Verification: {}", one_line(verification)),
            ],
        )
    } else {
        commit_with_message(
            &worktree.path,
            &title,
            &[format!("Platypus-Task: {}", worktree.task_id)],
        )
    }
}

fn branch_ahead_count(root: &Path, base_ref: &str, branch: &str) -> Result<i64, String> {
    let range = format!("{base_ref}..{branch}");
    let output = run_git(root, &["rev-list", "--count", range.as_str()])?;
    output
        .trim()
        .parse::<i64>()
        .map_err(|error| format!("could not parse branch ahead count: {error}"))
}

fn integrate_merge_commit(
    root: &Path,
    worktree: &RecordedWorktree,
    verification: &str,
) -> Result<String, String> {
    run_git(
        root,
        &["merge", "--no-ff", "--no-commit", worktree.branch.as_str()],
    )?;
    commit_with_message(
        root,
        &format!("Integrate {}: {}", worktree.source_item_id, worktree.title),
        &[
            format!("Platypus-Closes: {}", worktree.source_item_id),
            format!("Platypus-Verification: {}", one_line(verification)),
        ],
    )?;
    head_commit(root)
}

fn integrate_squash(
    root: &Path,
    worktree: &RecordedWorktree,
    verification: &str,
) -> Result<String, String> {
    if let Err(error) = run_git(root, &["merge", "--squash", worktree.branch.as_str()]) {
        let _ = run_git(root, &["reset", "--merge"]);
        return Err(error);
    }
    commit_with_message(
        root,
        &format!("Integrate {}: {}", worktree.source_item_id, worktree.title),
        &[
            format!("Platypus-Closes: {}", worktree.source_item_id),
            format!("Platypus-Verification: {}", one_line(verification)),
        ],
    )?;
    head_commit(root)
}

fn integrate_apply_changed_files(
    root: &Path,
    worktree: &RecordedWorktree,
    verification: &str,
) -> Result<String, String> {
    let changed_files = changed_files_between(root, &worktree.base_ref, &worktree.branch)?;
    if changed_files.is_empty() {
        return Err("worker branch has no changed files to apply".to_string());
    }
    apply_changed_files_to_manager(root, &worktree.branch, &changed_files)?;
    commit_with_message(
        root,
        &format!("Integrate {}: {}", worktree.source_item_id, worktree.title),
        &[
            format!("Platypus-Closes: {}", worktree.source_item_id),
            format!("Platypus-Verification: {}", one_line(verification)),
            format!("Platypus-Integration-Strategy: apply_changed_files"),
        ],
    )?;
    head_commit(root)
}

fn changed_files_between(
    root: &Path,
    base_ref: &str,
    branch: &str,
) -> Result<Vec<ChangedFile>, String> {
    let range = format!("{base_ref}..{branch}");
    let output = run_git(root, &["diff", "--name-status", "-M", range.as_str()])?;
    output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(parse_changed_file)
        .collect()
}

fn parse_changed_file(line: &str) -> Result<ChangedFile, String> {
    let parts = line.split('\t').collect::<Vec<_>>();
    let status = parts.first().copied().unwrap_or_default();
    if status.is_empty() {
        return Err("empty changed file status".to_string());
    }
    let (previous_path, path) = if status.starts_with('R') || status.starts_with('C') {
        let previous = parts
            .get(1)
            .copied()
            .ok_or_else(|| format!("missing previous path in changed file line `{line}`"))?;
        let current = parts
            .get(2)
            .copied()
            .ok_or_else(|| format!("missing current path in changed file line `{line}`"))?;
        (
            Some(validate_relative_git_path(previous)?),
            validate_relative_git_path(current)?,
        )
    } else {
        let current = parts
            .get(1)
            .copied()
            .ok_or_else(|| format!("missing path in changed file line `{line}`"))?;
        (None, validate_relative_git_path(current)?)
    };
    Ok(ChangedFile {
        status: status.to_string(),
        path,
        previous_path,
    })
}

fn validate_relative_git_path(path: &str) -> Result<String, String> {
    let candidate = Path::new(path);
    if candidate.is_absolute()
        || path.contains('\0')
        || candidate
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
        || path.split('/').any(|part| part == ".git")
    {
        return Err(format!("unsafe changed file path `{path}`"));
    }
    Ok(path.to_string())
}

fn apply_changed_files_to_manager(
    root: &Path,
    branch: &str,
    files: &[ChangedFile],
) -> Result<(), String> {
    let mut remove_paths = Vec::new();
    let mut checkout_paths = Vec::new();
    for file in files {
        if file.status.starts_with('D') {
            remove_paths.push(file.path.clone());
            continue;
        }
        if file.status.starts_with('R') {
            if let Some(previous) = &file.previous_path {
                if previous != &file.path {
                    remove_paths.push(previous.clone());
                }
            }
        }
        checkout_paths.push(file.path.clone());
    }
    if !remove_paths.is_empty() {
        let mut args = vec![
            "rm".to_string(),
            "-q".to_string(),
            "--ignore-unmatch".to_string(),
            "--".to_string(),
        ];
        args.extend(remove_paths);
        let arg_refs = args.iter().map(String::as_str).collect::<Vec<_>>();
        run_git(root, &arg_refs)?;
    }
    if !checkout_paths.is_empty() {
        let mut args = vec!["checkout".to_string(), branch.to_string(), "--".to_string()];
        args.extend(checkout_paths);
        let arg_refs = args.iter().map(String::as_str).collect::<Vec<_>>();
        run_git(root, &arg_refs)?;
    }
    Ok(())
}

fn cleanup_integrated_worktree(
    state: &SqliteProjectState,
    root: &Path,
    worktree: &RecordedWorktree,
) -> Result<(), String> {
    let status = run_git(
        &worktree.path,
        &["status", "--porcelain=v1", "--untracked-files=all"],
    )?;
    if !status.trim().is_empty() {
        return Err(
            "integrated worktree still has local changes; inspect before cleanup".to_string(),
        );
    }
    let path = path_arg(&worktree.path);
    run_git(root, &["worktree", "remove", path.as_str()])?;
    state
        .clear_workspace(ClearWorkspaceCommand {
            task_id: worktree.task_id.clone(),
        })
        .map(|_| ())
        .map_err(|error| error.to_string())
}

fn integration_strategy(requested: Option<&str>, configured: &str) -> Result<String, String> {
    let strategy = requested
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(configured)
        .to_ascii_lowercase();
    if matches!(
        strategy.as_str(),
        "merge_commit" | "fast_forward" | "squash" | "apply_changed_files"
    ) {
        Ok(strategy)
    } else {
        Err(format!(
            "unsupported integration strategy `{strategy}`; expected merge_commit, fast_forward, squash, or apply_changed_files"
        ))
    }
}

fn integration_prerequisite_guidance(task_status: &str) -> &'static str {
    match task_status {
        "queued" => {
            "Claim the task and run the worker lifecycle first: claim_next_task, start_worker_task, then complete_worker_task. Only completed tasks can be integrated."
        }
        "claimed" => {
            "The task is claimed but not running or complete. Use start_worker_task when the external worker begins, then complete_worker_task after it writes its result. Only completed tasks can be integrated."
        }
        "running" => {
            "The task is still running. Use complete_worker_task after the external worker finishes and writes its result. Only completed tasks can be integrated."
        }
        "failed" | "cancelled" => {
            "This task ended without a completed worker result. Inspect task events, create a follow-up task if needed, or retry dispatch before integration."
        }
        _ => "Complete the worker task before integrating its result.",
    }
}

fn worktree_change_reason(
    worktree: &RecordedWorktree,
    dirty: bool,
    has_committed_changes: bool,
) -> String {
    if dirty {
        return format!(
            "Worktree has local changes for `{}` while task is `{}`.",
            worktree.title, worktree.task_status
        );
    }
    if has_committed_changes {
        return format!(
            "Worktree branch for task `{}` has committed changes ahead of `{}`.",
            worktree.task_id, worktree.base_ref
        );
    }
    match worktree.task_status.as_str() {
        "queued" | "claimed" | "running" => format!(
            "No meaningful worker changes are present yet; task `{}` is `{}` and may not have produced output.",
            worktree.task_id, worktree.task_status
        ),
        "completed" => format!(
            "No worktree changes remain for completed task `{}`; the result may already be integrated or the worker produced no file changes.",
            worktree.task_id
        ),
        status => format!(
            "No worktree changes are present while task `{}` is `{status}`.",
            worktree.task_id
        ),
    }
}

fn merge_committed_files(files: &mut Vec<WorktreeDiffFile>, committed_files: &[ChangedFile]) {
    let mut by_path = files
        .iter()
        .enumerate()
        .map(|(index, file)| (file.path.clone(), index))
        .collect::<BTreeMap<_, _>>();
    for file in committed_files {
        let status = normalize_changed_file_status(&file.status);
        if let Some(index) = by_path.get(&file.path).copied() {
            files[index].status = status;
            continue;
        }
        files.push(WorktreeDiffFile {
            status,
            path: file.path.clone(),
        });
        by_path.insert(file.path.clone(), files.len() - 1);
    }
}

fn untracked_file_diff(worktree: &Path, files: &[WorktreeDiffFile]) -> String {
    let sections = files
        .iter()
        .filter(|file| file.status == "??")
        .take(8)
        .filter_map(|file| untracked_file_diff_section(worktree, &file.path))
        .collect::<Vec<_>>();
    if sections.is_empty() {
        String::new()
    } else {
        limit_output(&sections.join("\n"))
    }
}

fn untracked_file_diff_section(worktree: &Path, relative_path: &str) -> Option<String> {
    let relative = Path::new(relative_path);
    if relative.is_absolute()
        || relative
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return None;
    }
    let path = worktree.join(relative);
    let canonical_worktree = fs::canonicalize(worktree).ok()?;
    let canonical_path = fs::canonicalize(&path).ok()?;
    if !canonical_path.starts_with(&canonical_worktree) || !canonical_path.is_file() {
        return None;
    }
    let mut file = fs::File::open(&canonical_path).ok()?;
    let mut bytes = Vec::new();
    let mut limited = file.by_ref().take(OUTPUT_LIMIT as u64 + 1);
    limited.read_to_end(&mut bytes).ok()?;
    let truncated = bytes.len() > OUTPUT_LIMIT;
    if truncated {
        bytes.truncate(OUTPUT_LIMIT);
    }
    let body = String::from_utf8_lossy(&bytes);
    let mut section = format!(
        "diff --git a/{relative_path} b/{relative_path}\nnew file mode 100644\n--- /dev/null\n+++ b/{relative_path}\n@@\n"
    );
    for line in body.lines() {
        section.push('+');
        section.push_str(line);
        section.push('\n');
    }
    if truncated {
        section.push_str("+...\n");
    }
    Some(section)
}

fn normalize_changed_file_status(status: &str) -> String {
    if status.starts_with('A') {
        "A".to_string()
    } else if status.starts_with('M') {
        "M".to_string()
    } else if status.starts_with('D') {
        "D".to_string()
    } else if status.starts_with('R') {
        "R".to_string()
    } else if status.starts_with('C') {
        "C".to_string()
    } else if status.starts_with('?') {
        "??".to_string()
    } else {
        status.to_string()
    }
}

fn in_progress_git_operation(root: &Path) -> Option<String> {
    let git_dir = git_dir(root).ok()?;
    [
        ("merge", "MERGE_HEAD"),
        ("cherry-pick", "CHERRY_PICK_HEAD"),
        ("revert", "REVERT_HEAD"),
        ("rebase", "rebase-merge"),
        ("rebase", "rebase-apply"),
    ]
    .iter()
    .find_map(|(operation, marker)| git_dir.join(marker).exists().then(|| operation.to_string()))
}

fn git_dir(root: &Path) -> Result<PathBuf, String> {
    let output = run_git(root, &["rev-parse", "--git-dir"])?;
    let raw = output.trim();
    if raw.is_empty() {
        return Err("git directory did not resolve".to_string());
    }
    let path = PathBuf::from(raw);
    let path = if path.is_absolute() {
        path
    } else {
        root.join(path)
    };
    fs::canonicalize(&path).map_err(|error| format!("{}: {error}", path.display()))
}

fn conflicted_files(root: &Path) -> Vec<String> {
    run_git(root, &["diff", "--name-only", "--diff-filter=U"])
        .unwrap_or_default()
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(ToString::to_string)
        .collect()
}

fn commit_with_message(root: &Path, subject: &str, body_lines: &[String]) -> Result<(), String> {
    let mut args = vec!["commit".to_string(), "-m".to_string(), one_line(subject)];
    if !body_lines.is_empty() {
        args.push("-m".to_string());
        args.push(
            body_lines
                .iter()
                .map(|line| one_line(line))
                .collect::<Vec<_>>()
                .join("\n"),
        );
    }
    let arg_refs = args.iter().map(String::as_str).collect::<Vec<_>>();
    run_git(root, &arg_refs).map(|_| ())
}

fn head_commit(root: &Path) -> Result<String, String> {
    let output = run_git(root, &["rev-parse", "--verify", "HEAD^{commit}"])?;
    let commit = output.lines().next().unwrap_or_default().trim();
    if commit.is_empty() {
        Err("HEAD did not resolve to a commit".to_string())
    } else {
        Ok(commit.to_string())
    }
}

fn one_line(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn existing_workspace_data(
    action: &str,
    root: &Path,
    task: &WorkspaceTask,
    created: bool,
    skipped: bool,
) -> Option<ActionResult<WorktreeData>> {
    let path = task.workspace_path.as_ref()?;
    let branch = task.workspace_branch.as_ref()?;
    let base_ref = task.workspace_base_ref.as_ref()?;
    let worktrees_dir = match canonical_worktrees_dir(root) {
        Ok(path) => path,
        Err(error) => {
            return Some(ActionResult::failed(
                action,
                "Could not inspect worktree.",
                error,
            ))
        }
    };
    let canonical = match fs::canonicalize(path) {
        Ok(path) => path,
        Err(error) => {
            return Some(ActionResult::failed(
                action,
                "Persisted worktree path is not available.",
                error.to_string(),
            ))
        }
    };
    if !canonical.starts_with(&worktrees_dir) {
        return Some(ActionResult::failed(
            action,
            "Persisted worktree path escaped project state.",
            format!(
                "{} is outside {}",
                canonical.display(),
                worktrees_dir.display()
            ),
        ));
    }
    Some(ActionResult {
        action: action.to_string(),
        status: if skipped {
            ActionStatus::Skipped
        } else {
            ActionStatus::Completed
        },
        summary: format!("Task `{}` already has a worktree.", task.id),
        next_action: if created {
            None
        } else {
            Some("Use the persisted worktree for task execution.".to_string())
        },
        recovery_action: None,
        data: Some(WorktreeData {
            root: root.display().to_string(),
            task_id: task.id.clone(),
            path: canonical.display().to_string(),
            branch: branch.clone(),
            base_ref: base_ref.clone(),
            ready: true,
            exists: true,
            created,
        }),
        error: None,
    })
}

fn recorded_worktree<T>(
    action: &str,
    state: &SqliteProjectState,
    root: &Path,
    task_id: &str,
) -> Result<RecordedWorktree, ActionResult<T>>
where
    T: serde::Serialize + schemars::JsonSchema,
{
    let task = match load_task(state, task_id) {
        Ok(task) => task,
        Err(WorkspaceLoadError::Missing) => {
            return Err(ActionResult::skipped(
                action,
                format!("Task `{task_id}` was not found."),
                "Dispatch work before inspecting a task worktree.",
            ))
        }
        Err(WorkspaceLoadError::Backend(error)) => {
            return Err(ActionResult::failed(
                action,
                "Could not inspect task.",
                error,
            ))
        }
    };
    let Some(path) = task.workspace_path.as_ref() else {
        return Err(ActionResult::skipped(
            action,
            format!("Task `{task_id}` does not have a worktree yet."),
            "Run worktree_create for the task.",
        ));
    };
    let Some(branch) = task.workspace_branch.as_ref() else {
        return Err(ActionResult::failed(
            action,
            "Persisted worktree metadata is incomplete.",
            "workspace_branch is missing",
        ));
    };
    let Some(base_ref) = task.workspace_base_ref.as_ref() else {
        return Err(ActionResult::failed(
            action,
            "Persisted worktree metadata is incomplete.",
            "workspace_base_ref is missing",
        ));
    };
    let worktrees_dir = match canonical_worktrees_dir(root) {
        Ok(path) => path,
        Err(error) => {
            return Err(ActionResult::failed(
                action,
                "Could not inspect worktree.",
                error,
            ))
        }
    };
    let canonical = match fs::canonicalize(path) {
        Ok(path) => path,
        Err(error) => {
            return Err(ActionResult::failed(
                action,
                "Persisted worktree path is not available.",
                error.to_string(),
            ))
        }
    };
    if !canonical.starts_with(&worktrees_dir) {
        return Err(ActionResult::failed(
            action,
            "Persisted worktree path escaped project state.",
            format!(
                "{} is outside {}",
                canonical.display(),
                worktrees_dir.display()
            ),
        ));
    }
    Ok(RecordedWorktree {
        task_id: task.id,
        source_item_id: task.source_item_id,
        title: task.title,
        task_status: task.status,
        path: canonical,
        branch: branch.clone(),
        base_ref: base_ref.clone(),
    })
}

#[derive(Debug)]
enum WorkspaceLoadError {
    Missing,
    Backend(String),
}

fn load_task(
    state: &SqliteProjectState,
    task_id: &str,
) -> Result<WorkspaceTask, WorkspaceLoadError> {
    let task = state
        .inspect_task(TaskQuery {
            task_id: task_id.to_string(),
        })
        .map_err(|error| match error {
            ProjectStateError::NotFound { .. } => WorkspaceLoadError::Missing,
            other => WorkspaceLoadError::Backend(other.to_string()),
        })?;
    Ok(workspace_task(task))
}

fn workspace_task(task: TaskSnapshot) -> WorkspaceTask {
    let (workspace_path, workspace_branch, workspace_base_ref) = match task.worker_workspace {
        Some(workspace) => (
            Some(workspace.path),
            Some(workspace.branch),
            Some(workspace.base_ref),
        ),
        None => (None, None, None),
    };
    WorkspaceTask {
        id: task.id,
        source_item_id: task.source_item_id,
        title: task.title,
        status: task.status,
        workspace_path,
        workspace_branch,
        workspace_base_ref,
    }
}

fn resolve_base_commit(root: &Path, base_ref: &str) -> Result<String, String> {
    let rev = format!("{base_ref}^{{commit}}");
    let output = run_git(root, &["rev-parse", "--verify", rev.as_str()])?;
    let commit = output.lines().next().unwrap_or_default().trim();
    if commit.is_empty() {
        Err(format!("base_ref `{base_ref}` is not a commit"))
    } else {
        Ok(commit.to_string())
    }
}

fn branch_exists(root: &Path, branch: &str) -> bool {
    let reference = format!("refs/heads/{branch}");
    run_git(
        root,
        &["show-ref", "--verify", "--quiet", reference.as_str()],
    )
    .is_ok()
}

fn prepare_worktrees_dir(root: &Path) -> Result<PathBuf, String> {
    let state_dir = root.join(".platy");
    fs::create_dir_all(&state_dir).map_err(|error| error.to_string())?;
    let state_dir = fs::canonicalize(&state_dir).map_err(|error| error.to_string())?;
    if !state_dir.starts_with(root) {
        return Err(format!(
            "state directory {} escapes project root {}",
            state_dir.display(),
            root.display()
        ));
    }
    let worktrees_dir = state_dir.join("worktrees");
    fs::create_dir_all(&worktrees_dir).map_err(|error| error.to_string())?;
    fs::canonicalize(&worktrees_dir).map_err(|error| error.to_string())
}

fn canonical_worktrees_dir(root: &Path) -> Result<PathBuf, String> {
    let worktrees_dir = root.join(".platy").join("worktrees");
    let worktrees_dir = fs::canonicalize(&worktrees_dir).map_err(|error| error.to_string())?;
    if !worktrees_dir.starts_with(root) {
        return Err(format!(
            "worktrees directory {} escapes project root {}",
            worktrees_dir.display(),
            root.display()
        ));
    }
    Ok(worktrees_dir)
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

fn clean_task_id(value: &str) -> Result<String, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err("task_id is required".to_string());
    }
    if trimmed != safe_task_component(trimmed) {
        return Err("task_id must contain only ASCII letters, numbers, '-' or '_'".to_string());
    }
    Ok(trimmed.to_string())
}

fn safe_task_component(value: &str) -> String {
    value
        .chars()
        .filter(|character| {
            character.is_ascii_alphanumeric() || *character == '-' || *character == '_'
        })
        .collect()
}

fn clean_base_ref(value: Option<&str>) -> Result<String, String> {
    let base_ref = value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("HEAD");
    if base_ref.starts_with('-')
        || base_ref.contains("..")
        || base_ref.contains("@{")
        || !base_ref
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "/._-".contains(character))
    {
        return Err("base_ref contains unsupported characters".to_string());
    }
    Ok(base_ref.to_string())
}

fn path_arg(path: &Path) -> String {
    path.as_os_str().to_string_lossy().into_owned()
}

fn limit_output(value: &str) -> String {
    let mut output = value.trim().to_string();
    if output.len() > OUTPUT_LIMIT {
        output.truncate(OUTPUT_LIMIT);
        output.push_str("...");
    }
    output
}

fn parse_status_files(status: &str) -> Vec<WorktreeDiffFile> {
    status
        .lines()
        .filter_map(|line| {
            if line.len() < 4 {
                return None;
            }
            let code = line[..2].trim().to_string();
            let path = line[2..].trim().to_string();
            if path.is_empty() {
                None
            } else {
                Some(WorktreeDiffFile { status: code, path })
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        events::events_replay,
        evidence::{list_evidence, record_evidence},
        findings::{record_finding, update_finding_disposition},
        models::{
            ClaimNextTaskParams, EventsReplayParams, IntegrateWorkerResultParams,
            ListEvidenceParams, RecordEvidenceParams, RecordFindingParams,
            UpdateFindingDispositionParams,
        },
        tasks::{create_task_record, inspect_task_events, NewTask},
    };
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn creates_worktree_and_persists_task_metadata() {
        let project = git_project();
        let task = create_task_record(
            project.path(),
            None,
            NewTask {
                source_item_id: "PROJ-001".to_string(),
                title: "Workspace task".to_string(),
                worker: Some("coder".to_string()),
            },
        )
        .expect("task");

        let result = worktree_create(
            project.path(),
            WorktreeCreateParams {
                root: None,
                task_id: task.id.clone(),
                base_ref: None,
            },
        );
        let data = result.data.expect("worktree data");

        assert!(matches!(result.status, ActionStatus::Completed));
        assert!(Path::new(&data.path).is_dir());
        assert_eq!(data.branch, format!("platy/task/{}", task.id));
        assert!(data.created);

        let status = worktree_status(
            project.path(),
            WorktreeStatusParams {
                root: None,
                task_id: task.id.clone(),
            },
        );
        let status_data = status.data.expect("status data");
        assert!(matches!(status.status, ActionStatus::Completed));
        assert_eq!(status_data.path, data.path);

        let task = tasks::get_task_by_id(project.path(), None, &task.id).expect("task");
        assert_eq!(task.workspace_path.as_deref(), Some(data.path.as_str()));
        assert_eq!(task.workspace_branch.as_deref(), Some(data.branch.as_str()));
        assert_eq!(
            task.workspace_base_ref.as_deref(),
            Some(data.base_ref.as_str())
        );

        let events = inspect_task_events(
            project.path(),
            crate::models::InspectTaskEventsParams {
                root: None,
                task_id: task.id,
                limit: None,
            },
        );
        let events = events.data.expect("event data");
        assert_eq!(events.returned, 1);
        assert_eq!(events.events[0].event_type, "worktree_created");
    }

    #[test]
    fn inspects_and_cleans_worktree_changes_safely() {
        let project = git_project();
        let task = create_task_record(
            project.path(),
            None,
            NewTask {
                source_item_id: "PROJ-001".to_string(),
                title: "Workspace task".to_string(),
                worker: Some("coder".to_string()),
            },
        )
        .expect("task");
        let created = worktree_create(
            project.path(),
            WorktreeCreateParams {
                root: None,
                task_id: task.id.clone(),
                base_ref: None,
            },
        )
        .data
        .expect("worktree data");
        fs::write(Path::new(&created.path).join("README.md"), "# Changed\n").expect("change file");

        let diff = worktree_diff(
            project.path(),
            WorktreeDiffParams {
                root: None,
                task_id: task.id.clone(),
            },
        );
        let diff_data = diff.data.expect("diff data");
        assert!(matches!(diff.status, ActionStatus::Completed));
        assert!(diff_data.dirty);
        assert!(diff_data.meaningful_changes);
        assert!(
            diff_data
                .files
                .iter()
                .any(|file| file.path.ends_with("README.md")),
            "files: {:?}",
            diff_data.files
        );

        let refused = worktree_cleanup(
            project.path(),
            WorktreeCleanupParams {
                root: None,
                task_id: task.id.clone(),
                force: None,
            },
        );
        assert!(matches!(refused.status, ActionStatus::Skipped));
        assert!(Path::new(&created.path).exists());

        let cleaned = worktree_cleanup(
            project.path(),
            WorktreeCleanupParams {
                root: None,
                task_id: task.id.clone(),
                force: Some(true),
            },
        );
        assert!(matches!(cleaned.status, ActionStatus::Completed));
        assert!(!Path::new(&created.path).exists());
        let task = tasks::get_task_by_id(project.path(), None, &task.id).expect("task");
        assert!(task.workspace_path.is_none());

        let recreated = worktree_create(
            project.path(),
            WorktreeCreateParams {
                root: None,
                task_id: task.id.clone(),
                base_ref: None,
            },
        );
        let recreated_data = recreated.data.expect("recreated data");
        assert!(matches!(recreated.status, ActionStatus::Completed));
        assert!(Path::new(&recreated_data.path).exists());

        let events = inspect_task_events(
            project.path(),
            crate::models::InspectTaskEventsParams {
                root: None,
                task_id: task.id,
                limit: None,
            },
        )
        .data
        .expect("event data");
        assert!(events
            .events
            .iter()
            .any(|event| event.event_type == "worktree_cleaned_up"));
    }

    #[test]
    fn clean_untouched_worktree_reports_no_worker_output_yet() {
        let project = git_project();
        let task = create_task_record(
            project.path(),
            None,
            NewTask {
                source_item_id: "PROJ-001".to_string(),
                title: "Workspace task".to_string(),
                worker: Some("coder".to_string()),
            },
        )
        .expect("task");
        worktree_create(
            project.path(),
            WorktreeCreateParams {
                root: None,
                task_id: task.id.clone(),
                base_ref: None,
            },
        )
        .data
        .expect("worktree data");

        let diff = worktree_diff(
            project.path(),
            WorktreeDiffParams {
                root: None,
                task_id: task.id,
            },
        );
        let data = diff.data.expect("diff data");

        assert!(matches!(diff.status, ActionStatus::Completed));
        assert!(!data.dirty);
        assert!(!data.meaningful_changes);
        assert!(data.state_reason.contains("may not have produced output"));
    }

    #[test]
    fn worktree_diff_reports_committed_changes_ahead_of_base() {
        let project = git_project();
        let task = create_task_record(
            project.path(),
            None,
            NewTask {
                source_item_id: "PROJ-001".to_string(),
                title: "Workspace task".to_string(),
                worker: Some("coder".to_string()),
            },
        )
        .expect("task");
        let created = worktree_create(
            project.path(),
            WorktreeCreateParams {
                root: None,
                task_id: task.id.clone(),
                base_ref: None,
            },
        )
        .data
        .expect("worktree data");
        fs::create_dir_all(Path::new(&created.path).join("backend")).expect("create backend dir");
        fs::write(
            Path::new(&created.path).join("backend/main.py"),
            "from fastapi import FastAPI\napp = FastAPI()\n",
        )
        .expect("write backend file");
        run_git(Path::new(&created.path), &["add", "backend/main.py"]).expect("git add");
        run_git(
            Path::new(&created.path),
            &["commit", "-m", "Add backend stub"],
        )
        .expect("git commit");

        let diff = worktree_diff(
            project.path(),
            WorktreeDiffParams {
                root: None,
                task_id: task.id,
            },
        );
        let data = diff.data.expect("diff data");
        assert!(matches!(diff.status, ActionStatus::Completed));
        assert!(!data.dirty);
        assert!(data.meaningful_changes);
        assert!(data.state_reason.contains("committed changes ahead"));
        assert!(data.files.iter().any(|file| file.path == "backend/main.py"));
        assert!(data.diff.contains("backend/main.py"));
    }

    #[test]
    fn integrate_claimed_task_explains_worker_lifecycle_prerequisites() {
        let project = git_project();
        let task = create_task_record(
            project.path(),
            None,
            NewTask {
                source_item_id: "PROJ-001".to_string(),
                title: "Workspace task".to_string(),
                worker: Some("coder".to_string()),
            },
        )
        .expect("task");
        worktree_create(
            project.path(),
            WorktreeCreateParams {
                root: None,
                task_id: task.id.clone(),
                base_ref: None,
            },
        )
        .data
        .expect("worktree data");
        tasks::claim_next_task(
            project.path(),
            ClaimNextTaskParams {
                root: None,
                worker: None,
                claimant: Some("runner".to_string()),
            },
        );

        let result = integrate_worker_result(
            project.path(),
            IntegrateWorkerResultParams {
                root: None,
                task_id: task.id,
                strategy: None,
                allow_unverified: None,
                cleanup_after: None,
            },
        );

        assert!(matches!(result.status, ActionStatus::Skipped));
        let next_action = result.next_action.expect("next action");
        assert!(next_action.contains("start_worker_task"));
        assert!(next_action.contains("complete_worker_task"));
        assert!(next_action.contains("Only completed tasks can be integrated"));
    }

    #[test]
    fn detects_untracked_files_when_git_config_hides_them() {
        let project = git_project();
        run_git(
            project.path(),
            &["config", "status.showUntrackedFiles", "no"],
        )
        .expect("hide untracked files");
        let task = create_task_record(
            project.path(),
            None,
            NewTask {
                source_item_id: "PROJ-001".to_string(),
                title: "Workspace task".to_string(),
                worker: Some("coder".to_string()),
            },
        )
        .expect("task");
        let created = worktree_create(
            project.path(),
            WorktreeCreateParams {
                root: None,
                task_id: task.id.clone(),
                base_ref: None,
            },
        )
        .data
        .expect("worktree data");
        fs::write(Path::new(&created.path).join("untracked.txt"), "new\n").expect("new file");

        let diff = worktree_diff(
            project.path(),
            WorktreeDiffParams {
                root: None,
                task_id: task.id.clone(),
            },
        );
        let diff_data = diff.data.expect("diff data");
        assert!(matches!(diff.status, ActionStatus::Completed));
        assert!(diff_data.dirty);
        assert!(
            diff_data
                .files
                .iter()
                .any(|file| file.path == "untracked.txt"),
            "files: {:?}",
            diff_data.files
        );
        assert!(diff_data.diff.contains("diff --git a/untracked.txt"));
        assert!(diff_data.diff.contains("+new"));

        let refused = worktree_cleanup(
            project.path(),
            WorktreeCleanupParams {
                root: None,
                task_id: task.id,
                force: None,
            },
        );
        assert!(matches!(refused.status, ActionStatus::Skipped));
    }

    #[test]
    fn cleans_clean_worktree_without_force() {
        let project = git_project();
        let task = create_task_record(
            project.path(),
            None,
            NewTask {
                source_item_id: "PROJ-001".to_string(),
                title: "Workspace task".to_string(),
                worker: Some("coder".to_string()),
            },
        )
        .expect("task");
        let created = worktree_create(
            project.path(),
            WorktreeCreateParams {
                root: None,
                task_id: task.id.clone(),
                base_ref: None,
            },
        )
        .data
        .expect("worktree data");

        let cleaned = worktree_cleanup(
            project.path(),
            WorktreeCleanupParams {
                root: None,
                task_id: task.id,
                force: None,
            },
        );

        assert!(matches!(cleaned.status, ActionStatus::Completed));
        assert!(!Path::new(&created.path).exists());
    }

    #[test]
    fn integrates_completed_verified_worker_result_with_merge_commit() {
        let project = git_project();
        let (task_id, _worktree) = completed_task_with_change(&project, "# Integrated\n");

        let result = integrate_worker_result(
            project.path(),
            IntegrateWorkerResultParams {
                root: None,
                task_id: task_id.clone(),
                strategy: None,
                allow_unverified: None,
                cleanup_after: None,
            },
        );
        let data = result.data.expect("integration data");

        assert!(matches!(result.status, ActionStatus::Completed));
        assert_eq!(data.task_id, task_id);
        assert_eq!(data.source_item_id, "PROJ-001");
        assert_eq!(data.merge_style, "merge_commit");
        assert!(!data.cleaned_up);
        assert_eq!(
            fs::read_to_string(project.path().join("README.md")).expect("readme"),
            "# Integrated\n"
        );
        let log = run_git(project.path(), &["log", "-1", "--pretty=%B"]).expect("log");
        assert!(log.contains("Platypus-Closes: PROJ-001"));
        assert!(log.contains("Platypus-Verification: make check passed"));
        let evidence = list_evidence(
            project.path(),
            ListEvidenceParams {
                root: None,
                source_item_id: Some("PROJ-001".to_string()),
                source_task_id: Some(task_id.clone()),
                kind: Some("commit".to_string()),
                limit: None,
            },
        )
        .data
        .expect("evidence data");
        assert_eq!(evidence.returned, 1);
        assert!(evidence.evidence[0]
            .refs
            .iter()
            .any(|reference| reference.starts_with("commit:")));
        let events = events_replay(
            project.path(),
            EventsReplayParams {
                root: None,
                task_id: Some(task_id),
                scope: Some("project".to_string()),
                limit: None,
            },
        )
        .data
        .expect("events data");
        assert!(events
            .events
            .iter()
            .any(|event| event.event_type == "worker_result_integrated"));
    }

    #[test]
    fn refuses_integration_with_unresolved_required_findings() {
        let project = git_project();
        let (task_id, _worktree) = completed_task_with_change(&project, "# Integrated\n");
        let finding = record_finding(
            project.path(),
            RecordFindingParams {
                root: None,
                id: None,
                source_item_id: Some("PROJ-001".to_string()),
                source_task_id: Some(task_id.clone()),
                source_finding_ref: None,
                title: "Missing product acceptance".to_string(),
                summary: "A required follow-up must be dispositioned before integration."
                    .to_string(),
                severity: Some("medium".to_string()),
                required: Some(true),
                evidence_refs: Vec::new(),
                metadata: BTreeMap::new(),
            },
        );
        assert!(matches!(finding.status, ActionStatus::Completed));

        let result = integrate_worker_result(
            project.path(),
            IntegrateWorkerResultParams {
                root: None,
                task_id,
                strategy: None,
                allow_unverified: None,
                cleanup_after: None,
            },
        );

        assert!(matches!(result.status, ActionStatus::Skipped));
        assert!(result.summary.contains("open required finding"));
        assert!(result.next_action.expect("next action").contains("Accept"));
    }

    #[test]
    fn integrates_completed_verified_worker_result_with_squash_config() {
        let project = git_project();
        fs::write(
            project.path().join("platy.yaml"),
            "workflow:\n  integration:\n    merge_style: squash\n",
        )
        .expect("config");
        run_git(project.path(), &["add", "platy.yaml"]).expect("git add config");
        run_git(
            project.path(),
            &["commit", "-m", "Configure squash integration"],
        )
        .expect("git commit config");
        let (task_id, _worktree) = completed_task_with_change(&project, "# Squashed\n");

        let result = integrate_worker_result(
            project.path(),
            IntegrateWorkerResultParams {
                root: None,
                task_id,
                strategy: None,
                allow_unverified: None,
                cleanup_after: None,
            },
        );
        let data = result.data.expect("integration data");

        assert!(matches!(result.status, ActionStatus::Completed));
        assert_eq!(data.merge_style, "squash");
        let log = run_git(project.path(), &["log", "-1", "--pretty=%B"]).expect("log");
        assert!(log.contains("Platypus-Closes: PROJ-001"));
    }

    #[test]
    fn integrates_completed_verified_worker_result_with_fast_forward_config() {
        let project = git_project();
        fs::write(
            project.path().join("platy.yaml"),
            "workflow:\n  integration:\n    merge_style: fast_forward\n",
        )
        .expect("config");
        run_git(project.path(), &["add", "platy.yaml"]).expect("git add config");
        run_git(
            project.path(),
            &["commit", "-m", "Configure fast-forward integration"],
        )
        .expect("git commit config");
        let (task_id, _worktree) = completed_task_with_change(&project, "# Fast forward\n");

        let result = integrate_worker_result(
            project.path(),
            IntegrateWorkerResultParams {
                root: None,
                task_id,
                strategy: None,
                allow_unverified: None,
                cleanup_after: None,
            },
        );
        let data = result.data.expect("integration data");

        assert!(matches!(result.status, ActionStatus::Completed));
        assert_eq!(data.merge_style, "fast_forward");
        let log = run_git(project.path(), &["log", "-1", "--pretty=%B"]).expect("log");
        assert!(log.contains("Platypus-Closes: PROJ-001"));
    }

    #[test]
    fn integrates_worker_result_with_apply_changed_files_strategy() {
        let project = git_project();
        let (first_task, _first_worktree) = completed_task_with_change(&project, "# First\n");
        let (second_task, _second_worktree) = completed_task_with_change(&project, "# Second\n");

        let first = integrate_worker_result(
            project.path(),
            IntegrateWorkerResultParams {
                root: None,
                task_id: first_task,
                strategy: None,
                allow_unverified: None,
                cleanup_after: None,
            },
        );
        assert!(matches!(first.status, ActionStatus::Completed));

        let second = integrate_worker_result(
            project.path(),
            IntegrateWorkerResultParams {
                root: None,
                task_id: second_task,
                strategy: Some("apply_changed_files".to_string()),
                allow_unverified: None,
                cleanup_after: None,
            },
        );
        let data = second.data.expect("integration data");

        assert!(matches!(second.status, ActionStatus::Completed));
        assert_eq!(data.merge_style, "apply_changed_files");
        assert_eq!(
            fs::read_to_string(project.path().join("README.md")).expect("readme"),
            "# Second\n"
        );
        let log = run_git(project.path(), &["log", "-1", "--pretty=%B"]).expect("log");
        assert!(log.contains("Platypus-Integration-Strategy: apply_changed_files"));
    }

    #[test]
    fn apply_changed_files_strategy_handles_deleted_files() {
        let project = git_project();
        fs::write(project.path().join("remove-me.txt"), "remove\n").expect("file");
        run_git(project.path(), &["add", "remove-me.txt"]).expect("git add file");
        run_git(project.path(), &["commit", "-m", "Add removable file"]).expect("git commit file");
        let task = create_task_record(
            project.path(),
            None,
            NewTask {
                source_item_id: "PROJ-001".to_string(),
                title: "Delete stale file".to_string(),
                worker: Some("coder".to_string()),
            },
        )
        .expect("task");
        let created = worktree_create(
            project.path(),
            WorktreeCreateParams {
                root: None,
                task_id: task.id.clone(),
                base_ref: None,
            },
        )
        .data
        .expect("worktree data");
        fs::remove_file(Path::new(&created.path).join("remove-me.txt")).expect("delete file");
        finish_task(project.path(), &task.id);
        record_evidence(
            project.path(),
            RecordEvidenceParams {
                root: None,
                id: None,
                source_item_id: Some("PROJ-001".to_string()),
                source_task_id: Some(task.id.clone()),
                kind: "verification".to_string(),
                summary: "Deletion reviewed".to_string(),
                refs: vec!["local".to_string()],
                metadata: BTreeMap::new(),
            },
        );

        let result = integrate_worker_result(
            project.path(),
            IntegrateWorkerResultParams {
                root: None,
                task_id: task.id,
                strategy: Some("apply_changed_files".to_string()),
                allow_unverified: None,
                cleanup_after: None,
            },
        );

        assert!(matches!(result.status, ActionStatus::Completed));
        assert!(!project.path().join("remove-me.txt").exists());
        let log = run_git(project.path(), &["log", "-1", "--pretty=%B"]).expect("log");
        assert!(log.contains("Platypus-Integration-Strategy: apply_changed_files"));
    }

    #[test]
    fn reports_conflicts_and_aborts_failed_merge_integration() {
        let project = git_project();
        let (first_task, _first_worktree) = completed_task_with_change(&project, "# First\n");
        let (second_task, _second_worktree) = completed_task_with_change(&project, "# Second\n");

        let first = integrate_worker_result(
            project.path(),
            IntegrateWorkerResultParams {
                root: None,
                task_id: first_task,
                strategy: None,
                allow_unverified: None,
                cleanup_after: None,
            },
        );
        assert!(matches!(first.status, ActionStatus::Completed));

        let second = integrate_worker_result(
            project.path(),
            IntegrateWorkerResultParams {
                root: None,
                task_id: second_task,
                strategy: None,
                allow_unverified: None,
                cleanup_after: None,
            },
        );

        assert!(matches!(second.status, ActionStatus::Failed));
        assert!(second
            .error
            .expect("error")
            .contains("conflicting files: README.md"));
        assert!(second
            .next_action
            .expect("next action")
            .contains("apply_changed_files"));
        assert!(!project.path().join(".git/MERGE_HEAD").exists());
    }

    #[test]
    fn refuses_integration_when_git_operation_is_in_progress() {
        let project = git_project();
        let (task_id, _worktree) = completed_task_with_change(&project, "# Integrated\n");
        fs::write(project.path().join(".git/MERGE_HEAD"), "pending\n").expect("merge head");

        let result = integrate_worker_result(
            project.path(),
            IntegrateWorkerResultParams {
                root: None,
                task_id,
                strategy: None,
                allow_unverified: None,
                cleanup_after: None,
            },
        );

        assert!(matches!(result.status, ActionStatus::Skipped));
        assert!(result.summary.contains("merge"));
    }

    #[test]
    fn allows_explicit_unverified_integration_under_strict_policy() {
        let project = git_project();
        fs::write(
            project.path().join("platy.yaml"),
            "workflow:\n  integration:\n    require_verification_evidence: true\n",
        )
        .expect("config");
        run_git(project.path(), &["add", "platy.yaml"]).expect("git add config");
        run_git(
            project.path(),
            &["commit", "-m", "Require verification evidence"],
        )
        .expect("git commit config");
        let (task_id, _worktree) =
            completed_task_with_change_without_evidence(&project, "# Changed\n");

        let result = integrate_worker_result(
            project.path(),
            IntegrateWorkerResultParams {
                root: None,
                task_id,
                strategy: None,
                allow_unverified: Some(true),
                cleanup_after: None,
            },
        );

        assert!(matches!(result.status, ActionStatus::Completed));
        let log = run_git(project.path(), &["log", "-1", "--pretty=%B"]).expect("log");
        assert!(log.contains("Platypus-Verification: verification not recorded"));
    }

    #[test]
    fn cleanup_after_integration_removes_worktree_metadata() {
        let project = git_project();
        let (task_id, worktree) = completed_task_with_change(&project, "# Integrated\n");

        let result = integrate_worker_result(
            project.path(),
            IntegrateWorkerResultParams {
                root: None,
                task_id: task_id.clone(),
                strategy: None,
                allow_unverified: None,
                cleanup_after: Some(true),
            },
        );
        let data = result.data.expect("integration data");

        assert!(matches!(result.status, ActionStatus::Completed));
        assert!(data.cleaned_up);
        assert!(!Path::new(&worktree.path).exists());
        let task = tasks::get_task_by_id(project.path(), None, &task_id).expect("task");
        assert!(task.workspace_path.is_none());
    }

    #[test]
    fn refuses_integration_without_verification_evidence() {
        let project = git_project();
        fs::write(
            project.path().join("platy.yaml"),
            "workflow:\n  integration:\n    require_verification_evidence: true\n",
        )
        .expect("config");
        run_git(project.path(), &["add", "platy.yaml"]).expect("git add config");
        run_git(
            project.path(),
            &["commit", "-m", "Require verification evidence"],
        )
        .expect("git commit config");
        let task = create_task_record(
            project.path(),
            None,
            NewTask {
                source_item_id: "PROJ-001".to_string(),
                title: "Workspace task".to_string(),
                worker: Some("coder".to_string()),
            },
        )
        .expect("task");
        let created = worktree_create(
            project.path(),
            WorktreeCreateParams {
                root: None,
                task_id: task.id.clone(),
                base_ref: None,
            },
        )
        .data
        .expect("worktree data");
        fs::write(Path::new(&created.path).join("README.md"), "# Changed\n").expect("change");
        finish_task(project.path(), &task.id);

        let result = integrate_worker_result(
            project.path(),
            IntegrateWorkerResultParams {
                root: None,
                task_id: task.id,
                strategy: None,
                allow_unverified: None,
                cleanup_after: None,
            },
        );

        assert!(matches!(result.status, ActionStatus::Skipped));
        assert!(result
            .next_action
            .expect("next action")
            .contains("Record verification evidence"));
    }

    #[test]
    fn integration_gates_report_ready_task() {
        let project = git_project();
        let (task_id, _worktree) = completed_task_with_change(&project, "# Integrated\n");

        let result = inspect_integration_gates(
            project.path(),
            InspectIntegrationGatesParams {
                root: None,
                task_id,
            },
        );

        assert!(matches!(result.status, ActionStatus::Completed));
        let data = result.data.expect("gate data");
        assert!(data.ok);
        assert!(data
            .gates
            .iter()
            .any(|gate| gate.name == "branch_changes" && !gate.blocking));
    }

    #[test]
    fn integration_gates_report_missing_verification_evidence() {
        let project = git_project();
        fs::write(
            project.path().join("platy.yaml"),
            "workflow:\n  integration:\n    require_verification_evidence: true\n",
        )
        .expect("config");
        run_git(project.path(), &["add", "platy.yaml"]).expect("git add config");
        run_git(
            project.path(),
            &["commit", "-m", "Require verification evidence"],
        )
        .expect("git commit config");
        let (task_id, _worktree) =
            completed_task_with_change_without_evidence(&project, "# Changed\n");

        let result = inspect_integration_gates(
            project.path(),
            InspectIntegrationGatesParams {
                root: None,
                task_id,
            },
        );

        let data = result.data.expect("gate data");
        assert!(!data.ok);
        let verification = data
            .gates
            .iter()
            .find(|gate| gate.name == "verification")
            .expect("verification gate");
        assert!(verification.blocking);
        assert_eq!(
            verification.recommended_tool.as_deref(),
            Some("record_verification_evidence")
        );
    }

    #[test]
    fn accepted_required_finding_does_not_block_integration_gate() {
        let project = git_project();
        let (task_id, _worktree) = completed_task_with_change(&project, "# Integrated\n");
        let recorded = record_finding(
            project.path(),
            RecordFindingParams {
                root: None,
                id: None,
                source_item_id: Some("PROJ-001".to_string()),
                source_task_id: Some(task_id.clone()),
                source_finding_ref: None,
                title: "Track follow-up".to_string(),
                summary: "A required finding that can be accepted.".to_string(),
                severity: Some("medium".to_string()),
                required: Some(true),
                evidence_refs: Vec::new(),
                metadata: BTreeMap::new(),
            },
        );
        let finding = recorded.data.expect("finding").finding;

        let blocked = inspect_integration_gates(
            project.path(),
            InspectIntegrationGatesParams {
                root: None,
                task_id: task_id.clone(),
            },
        )
        .data
        .expect("blocked gates");
        assert!(!blocked.ok);

        let updated = update_finding_disposition(
            project.path(),
            UpdateFindingDispositionParams {
                root: None,
                finding_id: finding.id,
                status: "accepted".to_string(),
                owner: Some("manager".to_string()),
                disposition_reason: Some("accepted as explicit follow-up".to_string()),
                evidence_refs: Vec::new(),
                metadata: BTreeMap::new(),
            },
        );
        assert!(matches!(updated.status, ActionStatus::Completed));

        let ready = inspect_integration_gates(
            project.path(),
            InspectIntegrationGatesParams {
                root: None,
                task_id,
            },
        )
        .data
        .expect("ready gates");
        assert!(ready.ok);
    }

    #[test]
    fn refuses_integration_with_dirty_manager_workspace() {
        let project = git_project();
        let (task_id, _worktree) = completed_task_with_change(&project, "# Integrated\n");
        fs::write(project.path().join("local.txt"), "local\n").expect("local change");

        let result = integrate_worker_result(
            project.path(),
            IntegrateWorkerResultParams {
                root: None,
                task_id,
                strategy: None,
                allow_unverified: None,
                cleanup_after: None,
            },
        );

        assert!(matches!(result.status, ActionStatus::Skipped));
        assert!(result.summary.contains("local changes"));
        assert!(result.next_action.unwrap().contains("commit, stash"));
    }

    #[test]
    fn rejects_unborn_head() {
        let project = TempDir::new().expect("temp dir");
        run_git(project.path(), &["init"]).expect("git init");
        let task = create_task_record(
            project.path(),
            None,
            NewTask {
                source_item_id: "PROJ-001".to_string(),
                title: "Workspace task".to_string(),
                worker: None,
            },
        )
        .expect("task");

        let result = worktree_create(
            project.path(),
            WorktreeCreateParams {
                root: None,
                task_id: task.id,
                base_ref: None,
            },
        );

        assert!(matches!(result.status, ActionStatus::Failed));
        assert!(result.summary.contains("no initial commit"));
        assert!(result.next_action.unwrap().contains("initial commit"));
    }

    #[test]
    fn rejects_missing_git_repository_with_recovery_guidance() {
        let project = TempDir::new().expect("temp dir");
        let task = create_task_record(
            project.path(),
            None,
            NewTask {
                source_item_id: "PROJ-001".to_string(),
                title: "Workspace task".to_string(),
                worker: None,
            },
        )
        .expect("task");

        let result = worktree_create(
            project.path(),
            WorktreeCreateParams {
                root: None,
                task_id: task.id,
                base_ref: None,
            },
        );

        assert!(matches!(result.status, ActionStatus::Failed));
        assert!(result.summary.contains("not initialized"));
        assert!(result
            .next_action
            .expect("next action")
            .contains("git init"));
    }

    #[test]
    fn rejects_dirty_manager_workspace_before_worktree_creation() {
        let project = git_project();
        let task = create_task_record(
            project.path(),
            None,
            NewTask {
                source_item_id: "PROJ-001".to_string(),
                title: "Workspace task".to_string(),
                worker: None,
            },
        )
        .expect("task");
        fs::write(project.path().join("local.txt"), "local\n").expect("local change");

        let result = worktree_create(
            project.path(),
            WorktreeCreateParams {
                root: None,
                task_id: task.id,
                base_ref: None,
            },
        );

        assert!(matches!(result.status, ActionStatus::Failed));
        assert!(result.summary.contains("local changes"));
        assert!(result.next_action.unwrap().contains("commit, stash"));
    }

    #[test]
    fn rejects_task_id_path_escape() {
        let project = git_project();

        let result = worktree_create(
            project.path(),
            WorktreeCreateParams {
                root: None,
                task_id: "../task".to_string(),
                base_ref: None,
            },
        );

        assert!(matches!(result.status, ActionStatus::Failed));
        assert!(result.error.expect("error").contains("task_id"));
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

    fn completed_task_with_change(project: &TempDir, readme: &str) -> (String, WorktreeData) {
        let (task_id, created) = completed_task_with_change_without_evidence(project, readme);
        record_evidence(
            project.path(),
            RecordEvidenceParams {
                root: None,
                id: None,
                source_item_id: Some("PROJ-001".to_string()),
                source_task_id: Some(task_id.clone()),
                kind: "verification".to_string(),
                summary: "make check passed".to_string(),
                refs: vec!["local".to_string()],
                metadata: BTreeMap::new(),
            },
        );
        (task_id, created)
    }

    fn completed_task_with_change_without_evidence(
        project: &TempDir,
        readme: &str,
    ) -> (String, WorktreeData) {
        let task = create_task_record(
            project.path(),
            None,
            NewTask {
                source_item_id: "PROJ-001".to_string(),
                title: "Workspace task".to_string(),
                worker: Some("coder".to_string()),
            },
        )
        .expect("task");
        let created = worktree_create(
            project.path(),
            WorktreeCreateParams {
                root: None,
                task_id: task.id.clone(),
                base_ref: None,
            },
        )
        .data
        .expect("worktree data");
        fs::write(Path::new(&created.path).join("README.md"), readme).expect("change");
        finish_task(project.path(), &task.id);
        (task.id, created)
    }

    fn finish_task(root: &Path, task_id: &str) {
        tasks::claim_next_task(
            root,
            ClaimNextTaskParams {
                root: None,
                worker: None,
                claimant: Some("test".to_string()),
            },
        );
        tasks::mark_task_running(root, None, task_id).expect("running");
        tasks::finish_task(root, None, task_id, "completed").expect("completed");
    }
}
