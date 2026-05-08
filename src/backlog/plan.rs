use super::{
    filesystem::resolve_root,
    validate::{valid_item_id, validate_backlog_at_root},
};
use crate::models::{
    ActionResult, ActionStatus, DraftTaskPlanParams, PlannedTask, TaskPlanData, TaskPlanDesign,
    TaskPlanFile, TaskPlanItemParams, TaskPlanListData, TaskPlanQueryParams, TaskPlanRequirement,
    TaskPlanSummary, TaskPlanValidationData, TaskPlanWriteData, WriteTaskPlanParams,
};
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

const PLAN_MODES: &[&str] = &["direct", "standard", "full"];

pub fn draft_task_plan(
    default_root: &Path,
    params: DraftTaskPlanParams,
) -> ActionResult<TaskPlanData> {
    let action = "draft_task_plan";
    let root = match resolve_root(default_root, params.root.as_deref()) {
        Ok(root) => root,
        Err(error) => return ActionResult::failed(action, "Could not draft task plan.", error),
    };
    let item_id = normalize_item_id(&params.item_id);
    let validation = validate_backlog_at_root(&root, true);
    if !validation.ok {
        return ActionResult::failed(
            action,
            "Could not draft task plan.",
            "backlog must validate before drafting task plans",
        );
    }
    let Some(item) = validation
        .items
        .iter()
        .find(|item| item.frontmatter.id == item_id)
    else {
        return ActionResult::failed(
            action,
            "Could not draft task plan.",
            format!("unknown backlog item `{item_id}`"),
        );
    };

    let owned_surfaces = if item.frontmatter.owned_surfaces.is_empty() {
        vec![format!("backlog/items/{item_id}.md")]
    } else {
        item.frontmatter.owned_surfaces.clone()
    };

    let plan = TaskPlanFile {
        item_id: item_id.clone(),
        version: 1,
        mode: "standard".to_string(),
        requirements: vec![TaskPlanRequirement {
            id: "R1".to_string(),
            text: format!(
                "Deliver the accepted outcome for backlog item {}: {}.",
                item.frontmatter.id, item.frontmatter.title
            ),
        }],
        design: TaskPlanDesign {
            summary: format!(
                "Implement a focused slice for {} while keeping runtime state outside the plan file.",
                item.frontmatter.id
            ),
            owned_surfaces: owned_surfaces.clone(),
            notes: None,
        },
        tasks: vec![PlannedTask {
            id: format!("{item_id}-T01"),
            title: format!("Implement {}", item.frontmatter.title),
            goal: format!("Complete the implementation contract for {}.", item.frontmatter.id),
            requirement_refs: vec!["R1".to_string()],
            depends_on: Vec::new(),
            owned_surfaces,
            suggested_worker: item.frontmatter.suggested_worker.clone(),
            verification: vec!["make check".to_string()],
            acceptance: vec![format!(
                "{} acceptance criteria are satisfied.",
                item.frontmatter.id
            )],
            notes: None,
        }],
    };

    ActionResult::completed(
        action,
        format!("Drafted task plan for {item_id}."),
        TaskPlanData {
            root: root.display().to_string(),
            item_id,
            path: None,
            plan,
        },
    )
}

pub fn inspect_task_plan(
    default_root: &Path,
    params: TaskPlanItemParams,
) -> ActionResult<TaskPlanData> {
    let action = "inspect_task_plan";
    let root = match resolve_root(default_root, params.root.as_deref()) {
        Ok(root) => root,
        Err(error) => return ActionResult::failed(action, "Could not inspect task plan.", error),
    };
    let item_id = normalize_item_id(&params.item_id);
    match read_plan_for_item(&root, &item_id) {
        Ok((path, plan)) => ActionResult::completed(
            action,
            format!("Read task plan for {item_id}."),
            TaskPlanData {
                root: root.display().to_string(),
                item_id,
                path: Some(path.display().to_string()),
                plan,
            },
        ),
        Err(error) => ActionResult::failed(action, "Could not inspect task plan.", error),
    }
}

pub fn list_task_plans(
    default_root: &Path,
    params: TaskPlanQueryParams,
) -> ActionResult<TaskPlanListData> {
    let action = "list_task_plans";
    let root = match resolve_root(default_root, params.root.as_deref()) {
        Ok(root) => root,
        Err(error) => return ActionResult::failed(action, "Could not list task plans.", error),
    };
    let paths = match plan_paths(&root) {
        Ok(paths) => paths,
        Err(error) => return ActionResult::failed(action, "Could not list task plans.", error),
    };
    let item_filter = params.item_id.as_deref().map(normalize_item_id);
    let mut plans = Vec::new();
    for path in paths {
        let stem = path
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("")
            .to_string();
        if let Some(filter) = &item_filter {
            if normalize_item_id(&stem) != *filter {
                continue;
            }
        }
        let parsed = read_plan_path(&path);
        let (item_id, mode, task_count, requirement_count, valid) = match parsed {
            Ok(plan) => (
                plan.item_id,
                Some(plan.mode),
                plan.tasks.len(),
                plan.requirements.len(),
                true,
            ),
            Err(_) => (stem, None, 0, 0, false),
        };
        plans.push(TaskPlanSummary {
            item_id,
            path: path.display().to_string(),
            mode,
            task_count,
            requirement_count,
            valid,
        });
    }
    let returned = plans.len();
    ActionResult::completed(
        action,
        format!("Listed {returned} task plan(s)."),
        TaskPlanListData {
            root: root.display().to_string(),
            plans,
            returned,
        },
    )
}

pub fn validate_task_plan(
    default_root: &Path,
    params: TaskPlanQueryParams,
) -> ActionResult<TaskPlanValidationData> {
    let action = "validate_task_plan";
    let root = match resolve_root(default_root, params.root.as_deref()) {
        Ok(root) => root,
        Err(error) => return ActionResult::failed(action, "Could not validate task plan.", error),
    };
    let include_errors = params.include_errors.unwrap_or(true);
    let errors = validate_plans_at_root(&root, params.item_id.as_deref());
    let paths = plan_paths(&root).unwrap_or_default();
    let mut task_count = 0;
    for path in &paths {
        if let Ok(plan) = read_plan_path(path) {
            task_count += plan.tasks.len();
        }
    }
    let data = TaskPlanValidationData {
        root: root.display().to_string(),
        ok: errors.is_empty(),
        plan_count: paths.len(),
        task_count,
        errors: if include_errors {
            errors.clone()
        } else {
            Vec::new()
        },
    };
    if data.ok {
        ActionResult::completed(
            action,
            format!("Task plans valid: {} plan(s).", data.plan_count),
            data,
        )
    } else {
        ActionResult {
            action: action.to_string(),
            status: ActionStatus::Failed,
            summary: format!("Task plans have {} validation issue(s).", errors.len()),
            next_action: Some(
                "Fix task plan YAML schema, IDs, dependencies, or runtime-only fields.".to_string(),
            ),
            data: Some(data),
            error: Some(errors.join("\n")),
        }
    }
}

pub fn write_task_plan(
    default_root: &Path,
    params: WriteTaskPlanParams,
) -> ActionResult<TaskPlanWriteData> {
    let action = "write_task_plan";
    let root = match resolve_root(default_root, params.root.as_deref()) {
        Ok(root) => root,
        Err(error) => return ActionResult::failed(action, "Could not write task plan.", error),
    };
    let item_id = normalize_item_id(&params.item_id);
    if item_id != params.plan.item_id {
        return ActionResult::failed(
            action,
            "Could not write task plan.",
            "request item_id must match plan.item_id",
        );
    }
    let errors = validate_plan_with_backlog(&root, &params.plan);
    if !errors.is_empty() {
        return ActionResult::failed(action, "Could not write task plan.", errors.join("\n"));
    }
    let plans_dir = root.join("backlog").join("plans");
    if let Err(error) = fs::create_dir_all(&plans_dir) {
        return ActionResult::failed(action, "Could not write task plan.", error.to_string());
    }
    if let Err(error) = ensure_inside_root(&root, &plans_dir) {
        return ActionResult::failed(action, "Could not write task plan.", error);
    }
    let path = plans_dir.join(format!("{item_id}.yaml"));
    if path.exists() && !params.overwrite.unwrap_or(false) {
        return ActionResult::failed(
            action,
            "Could not write task plan.",
            "task plan already exists; set overwrite=true to replace it",
        );
    }
    if let Err(error) = ensure_write_target_inside_root(&root, &path) {
        return ActionResult::failed(action, "Could not write task plan.", error);
    }
    let existed = path.exists();
    let text = match serde_yaml::to_string(&params.plan) {
        Ok(text) => text,
        Err(error) => {
            return ActionResult::failed(action, "Could not write task plan.", error.to_string())
        }
    };
    if let Err(error) = fs::write(&path, text) {
        return ActionResult::failed(action, "Could not write task plan.", error.to_string());
    }
    ActionResult::completed(
        action,
        format!("Wrote task plan for {item_id}."),
        TaskPlanWriteData {
            root: root.display().to_string(),
            item_id,
            path: path.display().to_string(),
            created: !existed,
            overwritten: existed,
        },
    )
}

fn validate_plans_at_root(root: &Path, item_id: Option<&str>) -> Vec<String> {
    let mut errors = Vec::new();
    let paths = match plan_paths(root) {
        Ok(paths) => paths,
        Err(error) => return vec![error],
    };
    let item_filter = item_id.map(normalize_item_id);
    for path in paths {
        let stem = path
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("");
        if let Some(filter) = &item_filter {
            if normalize_item_id(stem) != *filter {
                continue;
            }
        }
        match read_plan_path(&path) {
            Ok(plan) => errors.extend(
                validate_plan_with_backlog(root, &plan)
                    .into_iter()
                    .map(|error| format!("{}: {}", path.display(), error)),
            ),
            Err(error) => errors.push(format!("{}: {}", path.display(), error)),
        }
    }
    if item_filter.is_some() && errors.is_empty() {
        let item_id = item_filter.expect("checked");
        if !root
            .join("backlog")
            .join("plans")
            .join(format!("{item_id}.yaml"))
            .is_file()
        {
            errors.push(format!("missing task plan for `{item_id}`"));
        }
    }
    errors
}

fn validate_plan_with_backlog(root: &Path, plan: &TaskPlanFile) -> Vec<String> {
    let mut errors = validate_plan_shape(plan);
    let backlog = validate_backlog_at_root(root, true);
    if !backlog.ok {
        errors.push("backlog must validate before task plans are valid".to_string());
        return errors;
    }
    if !backlog
        .items
        .iter()
        .any(|item| item.frontmatter.id == plan.item_id)
    {
        errors.push(format!("unknown backlog item `{}`", plan.item_id));
    }
    errors
}

fn validate_plan_shape(plan: &TaskPlanFile) -> Vec<String> {
    let mut errors = Vec::new();
    if !valid_item_id(&plan.item_id) {
        errors.push(format!("invalid item_id `{}`", plan.item_id));
    }
    if plan.version == 0 {
        errors.push("version must be at least 1".to_string());
    }
    if !PLAN_MODES.contains(&plan.mode.as_str()) {
        errors.push(format!("invalid mode `{}`", plan.mode));
    }
    if plan.design.summary.trim().is_empty() {
        errors.push("design.summary is required".to_string());
    }
    if plan.tasks.is_empty() {
        errors.push("tasks must contain at least one task".to_string());
    }

    let mut requirement_ids = BTreeSet::new();
    for requirement in &plan.requirements {
        if requirement.id.trim().is_empty() || requirement.text.trim().is_empty() {
            errors.push("requirements require non-empty id and text".to_string());
        }
        if !requirement_ids.insert(requirement.id.clone()) {
            errors.push(format!("duplicate requirement id `{}`", requirement.id));
        }
    }

    let mut task_ids = BTreeSet::new();
    for task in &plan.tasks {
        if !valid_task_id(&plan.item_id, &task.id) {
            errors.push(format!(
                "invalid task id `{}`; expected {}-TNN",
                task.id, plan.item_id
            ));
        }
        if !task_ids.insert(task.id.clone()) {
            errors.push(format!("duplicate task id `{}`", task.id));
        }
        if task.title.trim().is_empty() || task.goal.trim().is_empty() {
            errors.push(format!("{}: title and goal are required", task.id));
        }
        if task.owned_surfaces.is_empty() {
            errors.push(format!("{}: owned_surfaces must not be empty", task.id));
        }
        if task.verification.is_empty() {
            errors.push(format!("{}: verification must not be empty", task.id));
        }
        if task.acceptance.is_empty() {
            errors.push(format!("{}: acceptance must not be empty", task.id));
        }
        for reference in &task.requirement_refs {
            if !requirement_ids.contains(reference) {
                errors.push(format!(
                    "{}: unknown requirement_ref `{}`",
                    task.id, reference
                ));
            }
        }
    }
    for task in &plan.tasks {
        for dependency in &task.depends_on {
            if dependency == &task.id {
                errors.push(format!("{}: task cannot depend on itself", task.id));
            } else if !task_ids.contains(dependency) {
                errors.push(format!("{}: unknown dependency `{}`", task.id, dependency));
            }
        }
    }

    errors
}

fn valid_task_id(item_id: &str, task_id: &str) -> bool {
    let Some(suffix) = task_id.strip_prefix(item_id) else {
        return false;
    };
    let Some(number) = suffix.strip_prefix("-T") else {
        return false;
    };
    number.len() == 2 && number.chars().all(|character| character.is_ascii_digit())
}

fn read_plan_for_item(
    root: &Path,
    item_id: &str,
) -> std::result::Result<(PathBuf, TaskPlanFile), String> {
    let path = root
        .join("backlog")
        .join("plans")
        .join(format!("{item_id}.yaml"));
    ensure_inside_root(root, &path)?;
    if !path.is_file() {
        return Err(format!("{} does not exist", path.display()));
    }
    let plan = read_plan_path(&path)?;
    Ok((path, plan))
}

fn read_plan_path(path: &Path) -> std::result::Result<TaskPlanFile, String> {
    let text = fs::read_to_string(path).map_err(|error| error.to_string())?;
    serde_yaml::from_str(&text).map_err(|error| error.to_string())
}

fn plan_paths(root: &Path) -> std::result::Result<Vec<PathBuf>, String> {
    let plans_dir = root.join("backlog").join("plans");
    if !plans_dir.exists() {
        return Ok(Vec::new());
    }
    ensure_inside_root(root, &plans_dir)?;
    let mut paths = Vec::new();
    for entry in fs::read_dir(&plans_dir).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let path = entry.path();
        let extension = path.extension().and_then(|value| value.to_str());
        if matches!(extension, Some("yaml" | "yml")) {
            ensure_inside_root(root, &path)?;
            paths.push(path);
        }
    }
    paths.sort();
    Ok(paths)
}

fn ensure_inside_root(root: &Path, path: &Path) -> std::result::Result<(), String> {
    let canonical =
        fs::canonicalize(path).map_err(|error| format!("{}: {}", path.display(), error))?;
    if !canonical.starts_with(root) {
        return Err(format!("{} escapes project root", canonical.display()));
    }
    Ok(())
}

fn ensure_write_target_inside_root(root: &Path, path: &Path) -> std::result::Result<(), String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() {
                return Err(format!(
                    "{} is a symlink and cannot be overwritten",
                    path.display()
                ));
            }
            return ensure_inside_root(root, path);
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(format!("{}: {}", path.display(), error)),
    }
    let Some(parent) = path.parent() else {
        return Err(format!("{} has no parent directory", path.display()));
    };
    ensure_inside_root(root, parent)
}

fn normalize_item_id(value: &str) -> String {
    value.trim().to_ascii_uppercase()
}
