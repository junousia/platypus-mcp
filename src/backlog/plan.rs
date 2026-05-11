use super::{
    filesystem::resolve_root,
    validate::{valid_item_id, validate_backlog_at_root},
};
use crate::{
    models::{
        ActionResult, ActionStatus, DraftTaskPlanParams, PlannedTask, TaskPlanData, TaskPlanDesign,
        TaskPlanFile, TaskPlanItemParams, TaskPlanListData, TaskPlanQueryParams,
        TaskPlanRequirement, TaskPlanSummary, TaskPlanValidationData, TaskPlanWriteData,
        WriteTaskPlanParams,
    },
    sampling,
};
use serde::Deserialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

pub fn draft_task_plan(
    default_root: &Path,
    params: DraftTaskPlanParams,
) -> ActionResult<TaskPlanData> {
    let action = "draft_task_plan";
    let context = match task_plan_draft_context(default_root, &params) {
        Ok(context) => context,
        Err(error) => return ActionResult::failed(action, "Could not draft task plan.", error),
    };
    let plan = starter_task_plan(&context);
    ActionResult {
        action: action.to_string(),
        status: ActionStatus::Completed,
        summary: "MCP sampling is unavailable; drafted a conservative starter task plan."
            .to_string(),
        next_action: Some(
            "Review data.plan and decide whether planning is worth it for this item. For direct-scaffold work, you can skip this starter plan and continue without task-plan enforcement. If you keep it, call write_task_plan, commit backlog/plans/<ITEM>.yaml, and dispatch_ready_work."
                .to_string(),
        ),
        data: Some(TaskPlanData {
            root: context.root.display().to_string(),
            item_id: plan.item_id.clone(),
            path: None,
            plan,
        }),
        error: None,
    }
}

pub fn draft_task_plan_sampling_prompt(
    default_root: &Path,
    params: &DraftTaskPlanParams,
) -> Result<String, String> {
    let context = task_plan_draft_context(default_root, params)?;
    let surfaces = if context.owned_surfaces.is_empty() {
        "none declared".to_string()
    } else {
        context.owned_surfaces.join(", ")
    };
    let acceptance = context
        .acceptance
        .iter()
        .map(|criterion| format!("- {criterion}"))
        .collect::<Vec<_>>()
        .join("\n");
    Ok(format!(
        "Draft a strict Platypus task plan for this backlog item.\n\n\
         Item: {item_id}\n\
         Title: {title}\n\
         Goal:\n{goal}\n\n\
         Implementation contract:\n{contract}\n\n\
         Acceptance criteria:\n{acceptance}\n\n\
         Suggested worker: {worker}\n\
         Owned surfaces: {surfaces}\n\n\
         Return only JSON for one TaskPlanFile object or an object with a `plan` field. \
         The plan must use this shape: item_id, version, mode, requirements, design, tasks. \
         Use mode direct, standard, or full. Task IDs should be three digit values such as {item_id}-T001, {item_id}-T002. \
         Map acceptance criteria into concrete executable tasks with non-empty verification commands.",
        item_id = context.item_id,
        title = context.title,
        goal = context.goal,
        contract = context.implementation_contract,
        worker = context.suggested_worker.unwrap_or_else(|| "coder".to_string()),
    ))
}

pub fn draft_task_plan_from_sample(
    default_root: &Path,
    params: &DraftTaskPlanParams,
    text: &str,
) -> ActionResult<TaskPlanData> {
    let action = "draft_task_plan";
    let context = match task_plan_draft_context(default_root, params) {
        Ok(context) => context,
        Err(error) => return ActionResult::failed(action, "Could not draft task plan.", error),
    };
    let mut plan = match sampling::parse_sampled_json::<SampledTaskPlan>(text) {
        Ok(SampledTaskPlan::Wrapped { plan }) | Ok(SampledTaskPlan::Bare(plan)) => plan,
        Err(error) => return ActionResult::failed(action, "Could not draft task plan.", error),
    };
    if normalize_item_id(&plan.item_id) != context.item_id {
        return ActionResult::failed(
            action,
            "Could not draft task plan.",
            format!(
                "sampled plan item_id `{}` did not match requested item `{}`",
                plan.item_id, context.item_id
            ),
        );
    }
    plan.item_id = context.item_id.clone();
    canonicalize_plan_mode(&mut plan);
    canonicalize_task_ids(&mut plan);
    apply_task_defaults(&mut plan);
    let errors = validate_plan_with_backlog(&context.root, &plan);
    if !errors.is_empty() {
        return ActionResult::failed(
            action,
            "Could not draft task plan.",
            format!("sampled task plan was invalid: {}", errors.join("\n")),
        );
    }

    ActionResult::completed(
        action,
        format!(
            "Drafted task plan for {} with host sampling.",
            context.item_id
        ),
        TaskPlanData {
            root: context.root.display().to_string(),
            item_id: context.item_id,
            path: None,
            plan,
        },
    )
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum SampledTaskPlan {
    Wrapped { plan: TaskPlanFile },
    Bare(TaskPlanFile),
}

struct TaskPlanDraftContext {
    root: PathBuf,
    item_id: String,
    title: String,
    goal: String,
    implementation_contract: String,
    acceptance: Vec<String>,
    owned_surfaces: Vec<String>,
    suggested_worker: Option<String>,
}

fn task_plan_draft_context(
    default_root: &Path,
    params: &DraftTaskPlanParams,
) -> Result<TaskPlanDraftContext, String> {
    let root = resolve_root(default_root, params.root.as_deref())?;
    let item_id = normalize_item_id(&params.item_id);
    let validation = validate_backlog_at_root(&root, true);
    if !validation.ok {
        return Err("backlog must validate before drafting task plans".to_string());
    }
    let item = validation
        .items
        .iter()
        .find(|item| item.frontmatter.id == item_id)
        .ok_or_else(|| format!("unknown backlog item `{item_id}`"))?;
    let text = fs::read_to_string(&item.path).map_err(|error| error.to_string())?;
    let body = text
        .split_once("\n---\n")
        .map(|(_, body)| body)
        .ok_or_else(|| format!("{} missing markdown body", item.path.display()))?;
    let goal = section(body, "Goal")
        .ok_or_else(|| format!("{} missing Goal section", item.path.display()))?;
    let implementation_contract = section(body, "Implementation Contract").ok_or_else(|| {
        format!(
            "{} missing Implementation Contract section",
            item.path.display()
        )
    })?;
    let acceptance_text = section(body, "Acceptance")
        .ok_or_else(|| format!("{} missing Acceptance section", item.path.display()))?;
    let acceptance = parse_acceptance(&acceptance_text);
    if acceptance.is_empty() {
        return Err(format!(
            "{} Acceptance section is empty",
            item.path.display()
        ));
    }
    Ok(TaskPlanDraftContext {
        root,
        item_id,
        title: item.frontmatter.title.clone(),
        goal,
        implementation_contract,
        acceptance,
        owned_surfaces: item.frontmatter.owned_surfaces.clone(),
        suggested_worker: item.frontmatter.suggested_worker.clone(),
    })
}

fn section(body: &str, heading: &str) -> Option<String> {
    let marker = format!("## {heading}");
    let start = body.find(&marker)?;
    let after_heading = &body[start + marker.len()..];
    let after_heading = after_heading.strip_prefix('\n').unwrap_or(after_heading);
    let end = after_heading.find("\n## ").unwrap_or(after_heading.len());
    Some(after_heading[..end].trim().to_string())
}

fn parse_acceptance(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|line| {
            line.trim()
                .strip_prefix("- ")
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned)
        })
        .collect()
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
        ActionResult {
            action: action.to_string(),
            status: ActionStatus::Completed,
            summary: format!("Task plans valid: {} plan(s).", data.plan_count),
            next_action: Some(
                "Commit task-plan artifacts before dispatching work if this validation followed writes."
                    .to_string(),
            ),
            data: Some(data),
            error: None,
        }
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
    let mut plan = params.plan;
    if item_id != plan.item_id {
        return ActionResult::failed(
            action,
            "Could not write task plan.",
            "request item_id must match plan.item_id",
        );
    }
    canonicalize_plan_mode(&mut plan);
    canonicalize_task_ids(&mut plan);
    apply_task_defaults(&mut plan);
    let errors = validate_plan_with_backlog(&root, &plan);
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
    let previous_content = if existed {
        match fs::read(&path) {
            Ok(content) => Some(content),
            Err(error) => {
                return ActionResult::failed(
                    action,
                    "Could not write task plan.",
                    error.to_string(),
                )
            }
        }
    } else {
        None
    };
    let text = match serde_yaml::to_string(&plan) {
        Ok(text) => text,
        Err(error) => {
            return ActionResult::failed(action, "Could not write task plan.", error.to_string())
        }
    };
    if let Err(error) = fs::write(&path, text) {
        return ActionResult::failed(action, "Could not write task plan.", error.to_string());
    }
    let post_write_errors = validate_plans_at_root(&root, Some(&item_id));
    if !post_write_errors.is_empty() {
        let rollback_error = restore_written_plan(&path, previous_content.as_deref());
        let mut error = format!(
            "task plan validation failed after write; rolled back {}. {}",
            path.display(),
            post_write_errors.join("\n")
        );
        if let Err(rollback_error) = rollback_error {
            error.push_str(&format!("\nrollback failed: {rollback_error}"));
        }
        return ActionResult {
            action: action.to_string(),
            status: ActionStatus::Failed,
            summary: format!("Could not write task plan for {item_id}."),
            next_action: Some(
                "Fix the task plan input or backlog state, then retry write_task_plan.".to_string(),
            ),
            data: None,
            error: Some(error),
        };
    }
    let validation = TaskPlanValidationData {
        root: root.display().to_string(),
        ok: true,
        plan_count: 1,
        task_count: plan.tasks.len(),
        errors: Vec::new(),
    };
    ActionResult {
        action: action.to_string(),
        status: ActionStatus::Completed,
        summary: format!("Wrote and validated task plan for {item_id}."),
        next_action: Some(
            "Task plan was validated before write. Commit backlog/plans/<ITEM>.yaml, then dispatch ready work."
                .to_string(),
        ),
        data: Some(TaskPlanWriteData {
            root: root.display().to_string(),
            item_id,
            path: path.display().to_string(),
            created: !existed,
            overwritten: existed,
            validation,
        }),
        error: None,
    }
}

fn restore_written_plan(path: &Path, previous_content: Option<&[u8]>) -> std::io::Result<()> {
    if let Some(content) = previous_content {
        fs::write(path, content)
    } else {
        match fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error),
        }
    }
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
    if canonical_plan_mode(&plan.mode).is_none() {
        errors.push(format!(
            "invalid mode `{}`; expected one of: direct, standard, full. Accepted aliases: direct_scaffold -> direct, platypus_workflow -> full",
            plan.mode
        ));
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
                "invalid task id `{}`; expected task IDs such as {}",
                task.id,
                task_id_hint(&plan.item_id)
            ));
        }
        if !task_ids.insert(task.id.clone()) {
            errors.push(format!("duplicate task id `{}`", task.id));
        }
        if task.title.trim().is_empty() || task.goal.trim().is_empty() {
            errors.push(format!("{}: title and goal are required", task.id));
        }
        if task.owned_surfaces.is_empty() && plan.design.owned_surfaces.is_empty() {
            errors.push(format!("{}: owned_surfaces must not be empty", task.id));
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
    errors.extend(find_dependency_cycles(plan));

    errors
}

fn apply_task_defaults(plan: &mut TaskPlanFile) {
    if plan.design.owned_surfaces.is_empty() {
        return;
    }
    for task in &mut plan.tasks {
        if task.owned_surfaces.is_empty() {
            task.owned_surfaces = plan.design.owned_surfaces.clone();
        }
    }
}

fn canonicalize_task_ids(plan: &mut TaskPlanFile) {
    let mut rewrite = BTreeMap::new();
    for task in &mut plan.tasks {
        let original = task.id.clone();
        let normalized = normalize_task_id_hint(&plan.item_id, &task.id);
        task.id = normalized.clone();
        rewrite.insert(original, normalized);
    }
    for task in &mut plan.tasks {
        task.depends_on = task
            .depends_on
            .iter()
            .map(|dependency| {
                rewrite
                    .get(dependency)
                    .cloned()
                    .unwrap_or_else(|| normalize_task_id_hint(&plan.item_id, dependency))
            })
            .collect();
    }
}

fn normalize_task_id_hint(item_id: &str, value: &str) -> String {
    if valid_task_id(item_id, value) {
        return value.to_string();
    }
    let trimmed = value.trim();
    if let Some(number) = parse_task_number(trimmed) {
        return format!("{item_id}-T{number:03}");
    }
    value.to_string()
}

fn parse_task_number(value: &str) -> Option<u16> {
    let upper = value.to_ascii_uppercase();
    let candidate = upper
        .strip_prefix("TASK-")
        .or_else(|| upper.strip_prefix("TASK"))
        .unwrap_or(upper.as_str());
    let candidate = candidate
        .strip_prefix("T-")
        .or_else(|| candidate.strip_prefix("T"))
        .unwrap_or(candidate);
    if candidate.is_empty() || !candidate.chars().all(|ch| ch.is_ascii_digit()) {
        return None;
    }
    candidate.parse::<u16>().ok().filter(|number| *number > 0)
}

fn starter_task_plan(context: &TaskPlanDraftContext) -> TaskPlanFile {
    let worker = context
        .suggested_worker
        .clone()
        .unwrap_or_else(|| "coder".to_string());
    let surfaces = if context.owned_surfaces.is_empty() {
        vec![".".to_string()]
    } else {
        context.owned_surfaces.clone()
    };
    let acceptance = if context.acceptance.is_empty() {
        vec!["Primary objective is implemented and verification notes are recorded.".to_string()]
    } else {
        context.acceptance.clone()
    };
    TaskPlanFile {
        item_id: context.item_id.clone(),
        version: 1,
        mode: "standard".to_string(),
        requirements: vec![TaskPlanRequirement {
            id: "R1".to_string(),
            text: context.goal.clone(),
        }],
        design: TaskPlanDesign {
            summary: context.implementation_contract.clone(),
            owned_surfaces: surfaces.clone(),
            notes: Some(
                "Starter plan generated without MCP sampling; review and adjust before writing."
                    .to_string(),
            ),
        },
        tasks: vec![PlannedTask {
            id: format!("{}-T001", context.item_id),
            title: format!("Deliver {}", context.title),
            goal: context.goal.clone(),
            requirement_refs: vec!["R1".to_string()],
            depends_on: Vec::new(),
            owned_surfaces: surfaces,
            suggested_worker: Some(worker),
            verification: Vec::new(),
            acceptance,
            notes: Some(vec![
                "Add project-specific verification commands before dispatch when needed."
                    .to_string(),
            ]),
        }],
    }
}

fn canonicalize_plan_mode(plan: &mut TaskPlanFile) {
    if let Some(mode) = canonical_plan_mode(&plan.mode) {
        plan.mode = mode.to_string();
    }
}

fn canonical_plan_mode(mode: &str) -> Option<&'static str> {
    match mode.trim().to_ascii_lowercase().as_str() {
        "direct" | "direct_scaffold" => Some("direct"),
        "standard" | "hybrid" => Some("standard"),
        "full" | "platypus_workflow" => Some("full"),
        _ => None,
    }
}

fn task_id_hint(item_id: &str) -> String {
    format!("{item_id}-T001, {item_id}-T002, ...")
}

fn find_dependency_cycles(plan: &TaskPlanFile) -> Vec<String> {
    let graph = plan
        .tasks
        .iter()
        .map(|task| (task.id.as_str(), task.depends_on.as_slice()))
        .collect::<BTreeMap<_, _>>();
    let mut errors = Vec::new();
    for task in &plan.tasks {
        let mut path = Vec::new();
        let mut visiting = BTreeSet::new();
        detect_cycle_from(
            task.id.as_str(),
            &graph,
            &mut visiting,
            &mut path,
            &mut errors,
        );
    }
    errors.sort();
    errors.dedup();
    errors
}

fn detect_cycle_from<'a>(
    task_id: &'a str,
    graph: &BTreeMap<&'a str, &'a [String]>,
    visiting: &mut BTreeSet<&'a str>,
    path: &mut Vec<&'a str>,
    errors: &mut Vec<String>,
) {
    if !visiting.insert(task_id) {
        if let Some(position) = path.iter().position(|seen| *seen == task_id) {
            let mut cycle = path[position..].to_vec();
            cycle.push(task_id);
            errors.push(format!("circular task dependency: {}", cycle.join(" -> ")));
        }
        return;
    }
    path.push(task_id);
    if let Some(dependencies) = graph.get(task_id) {
        for dependency in *dependencies {
            if graph.contains_key(dependency.as_str()) {
                detect_cycle_from(dependency, graph, visiting, path, errors);
            }
        }
    }
    path.pop();
    visiting.remove(task_id);
}

fn valid_task_id(item_id: &str, task_id: &str) -> bool {
    let Some(suffix) = task_id.strip_prefix(item_id) else {
        return false;
    };
    let Some(number) = suffix.strip_prefix("-T") else {
        return false;
    };
    matches!(number.len(), 2 | 3) && number.chars().all(|character| character.is_ascii_digit())
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
