use super::{
    closure::closed_item_ids, filesystem::resolve_root, types::ParsedBacklogItem,
    validate::validate_backlog_at_root,
};
use crate::models::{
    ActionResult, ActionStatus, BacklogDependencyEdge, BacklogDependencyGraphData,
    BacklogDependencyNode, BacklogMissingDependency, InspectDependencyGraphParams,
};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    path::Path,
};

const DEFAULT_GRAPH_LIMIT: usize = 200;
const MAX_GRAPH_LIMIT: usize = 500;

pub fn inspect_dependency_graph(
    default_root: &Path,
    params: InspectDependencyGraphParams,
) -> ActionResult<BacklogDependencyGraphData> {
    let action = "inspect_dependency_graph";
    let root = match resolve_root(default_root, params.root.as_deref()) {
        Ok(root) => root,
        Err(error) => {
            return ActionResult::failed(action, "Could not inspect dependency graph.", error)
        }
    };
    let focus_item_id = params
        .focus_item_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);
    let include_closed = params.include_closed.unwrap_or(true);
    let limit = params
        .limit
        .unwrap_or(DEFAULT_GRAPH_LIMIT)
        .clamp(1, MAX_GRAPH_LIMIT);
    let validation = validate_backlog_at_root(&root, true);
    let closed_ids = closed_item_ids(&root);
    let item_by_id = validation
        .items
        .iter()
        .map(|item| (item.frontmatter.id.clone(), item))
        .collect::<BTreeMap<_, _>>();
    let dependencies = dependency_map(&validation.items);
    let dependents = dependent_map(&dependencies);
    let mut validation_errors = validation.errors.clone();

    let mut scoped_ids = item_by_id.keys().cloned().collect::<BTreeSet<_>>();
    if let Some(focus) = focus_item_id.as_deref() {
        if !item_by_id.contains_key(focus) {
            validation_errors.push(format!("focus item `{focus}` was not found"));
            scoped_ids.clear();
        } else {
            scoped_ids = focus_neighborhood(focus, &dependencies, &dependents);
        }
    }
    if !include_closed {
        scoped_ids.retain(|id| !closed_ids.contains(id));
    }

    let total = scoped_ids.len();
    let mut returned_ids = scoped_ids.iter().cloned().collect::<Vec<_>>();
    let truncated = returned_ids.len() > limit;
    returned_ids.truncate(limit);
    let returned_scope = returned_ids.iter().cloned().collect::<BTreeSet<_>>();

    let missing_dependencies = missing_dependencies(&scoped_ids, &dependencies, &item_by_id);
    for missing in &missing_dependencies {
        validation_errors.push(format!(
            "{}: missing dependency `{}`",
            missing.item_id, missing.missing_dependency
        ));
    }
    let cycles = dependency_cycles(&scoped_ids, &dependencies);
    for cycle in &cycles {
        validation_errors.push(format!("dependency cycle: {}", cycle.join(" -> ")));
    }

    let edges = dependency_edges(&returned_scope, &dependencies);
    let incoming = edges
        .iter()
        .map(|edge| edge.dependent.clone())
        .collect::<BTreeSet<_>>();
    let outgoing = edges
        .iter()
        .map(|edge| edge.dependency.clone())
        .collect::<BTreeSet<_>>();
    let nodes = returned_ids
        .iter()
        .filter_map(|id| {
            item_by_id.get(id).map(|item| {
                graph_node(
                    item,
                    dependencies.get(id).cloned().unwrap_or_default(),
                    dependents.get(id).cloned().unwrap_or_default(),
                    &closed_ids,
                    &item_by_id,
                )
            })
        })
        .collect::<Vec<_>>();
    let roots = returned_ids
        .iter()
        .filter(|id| !incoming.contains(*id))
        .cloned()
        .collect::<Vec<_>>();
    let leaves = returned_ids
        .iter()
        .filter(|id| !outgoing.contains(*id))
        .cloned()
        .collect::<Vec<_>>();
    let runnable_nodes = nodes
        .iter()
        .filter(|node| node.runnable)
        .map(|node| node.item_id.clone())
        .collect::<Vec<_>>();
    let closed_nodes = nodes
        .iter()
        .filter(|node| node.closed)
        .map(|node| node.item_id.clone())
        .collect::<Vec<_>>();
    let blocked_nodes = nodes
        .iter()
        .filter(|node| !node.closed && !node.blocked_by.is_empty())
        .map(|node| node.item_id.clone())
        .collect::<Vec<_>>();
    let topological_order = topological_order(&returned_scope, &dependencies);

    let data = BacklogDependencyGraphData {
        root: root.display().to_string(),
        focus_item_id,
        include_closed,
        limit,
        total,
        returned: nodes.len(),
        truncated,
        nodes,
        edges,
        roots,
        leaves,
        topological_order,
        runnable_nodes,
        closed_nodes,
        blocked_nodes,
        missing_dependencies,
        cycles,
        validation_errors,
    };

    if data.validation_errors.is_empty() {
        ActionResult::completed(
            action,
            format!(
                "Dependency graph inspected: {} node(s), {} edge(s).",
                data.returned,
                data.edges.len()
            ),
            data,
        )
    } else {
        ActionResult {
            action: action.to_string(),
            status: ActionStatus::Failed,
            summary: format!(
                "Dependency graph has {} validation issue(s).",
                data.validation_errors.len()
            ),
            next_action: Some(
                "Fix missing dependency references or cycles, then rerun inspect_dependency_graph."
                    .to_string(),
            ),
            recovery_action: None,
            data: Some(data),
            error: Some("Backlog dependency graph is invalid.".to_string()),
        }
    }
}

fn dependency_map(items: &[ParsedBacklogItem]) -> BTreeMap<String, Vec<String>> {
    items
        .iter()
        .map(|item| {
            (
                item.frontmatter.id.clone(),
                item.frontmatter.depends_on.clone(),
            )
        })
        .collect()
}

fn dependent_map(dependencies: &BTreeMap<String, Vec<String>>) -> BTreeMap<String, Vec<String>> {
    let mut dependents: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (item_id, item_dependencies) in dependencies {
        for dependency in item_dependencies {
            dependents
                .entry(dependency.clone())
                .or_default()
                .push(item_id.clone());
        }
    }
    dependents
}

fn missing_dependencies(
    scope: &BTreeSet<String>,
    dependencies: &BTreeMap<String, Vec<String>>,
    item_by_id: &BTreeMap<String, &ParsedBacklogItem>,
) -> Vec<BacklogMissingDependency> {
    scope
        .iter()
        .flat_map(|item_id| {
            dependencies
                .get(item_id)
                .into_iter()
                .flatten()
                .filter(|dependency| !item_by_id.contains_key(*dependency))
                .map(|dependency| BacklogMissingDependency {
                    item_id: item_id.clone(),
                    missing_dependency: dependency.clone(),
                })
        })
        .collect()
}

fn dependency_edges(
    scope: &BTreeSet<String>,
    dependencies: &BTreeMap<String, Vec<String>>,
) -> Vec<BacklogDependencyEdge> {
    scope
        .iter()
        .flat_map(|item_id| {
            dependencies
                .get(item_id)
                .into_iter()
                .flatten()
                .filter(|dependency| scope.contains(*dependency))
                .map(|dependency| BacklogDependencyEdge {
                    dependency: dependency.clone(),
                    dependent: item_id.clone(),
                })
        })
        .collect()
}

fn graph_node(
    item: &ParsedBacklogItem,
    depends_on: Vec<String>,
    dependents: Vec<String>,
    closed_ids: &BTreeSet<String>,
    item_by_id: &BTreeMap<String, &ParsedBacklogItem>,
) -> BacklogDependencyNode {
    let item_id = item.frontmatter.id.clone();
    let closed = closed_ids.contains(&item_id);
    let blocked_by = depends_on
        .iter()
        .filter(|dependency| !closed_ids.contains(*dependency))
        .cloned()
        .collect::<Vec<_>>();
    let missing_dependencies = depends_on
        .iter()
        .filter(|dependency| !item_by_id.contains_key(*dependency))
        .cloned()
        .collect::<Vec<_>>();
    let runnable = !closed && blocked_by.is_empty() && missing_dependencies.is_empty();
    BacklogDependencyNode {
        item_id,
        title: item.frontmatter.title.clone(),
        priority: item.frontmatter.priority.clone(),
        item_type: item.frontmatter.item_type.clone(),
        area: item.frontmatter.area.clone(),
        depends_on,
        dependents,
        blocked_by,
        missing_dependencies,
        closed,
        runnable,
    }
}

fn focus_neighborhood(
    focus: &str,
    dependencies: &BTreeMap<String, Vec<String>>,
    dependents: &BTreeMap<String, Vec<String>>,
) -> BTreeSet<String> {
    let mut visited = BTreeSet::new();
    let mut queue = VecDeque::from([focus.to_string()]);
    while let Some(item_id) = queue.pop_front() {
        if !visited.insert(item_id.clone()) {
            continue;
        }
        for neighbor in dependencies
            .get(&item_id)
            .into_iter()
            .flatten()
            .chain(dependents.get(&item_id).into_iter().flatten())
        {
            if dependencies.contains_key(neighbor) && !visited.contains(neighbor) {
                queue.push_back(neighbor.clone());
            }
        }
    }
    visited
}

fn topological_order(
    scope: &BTreeSet<String>,
    dependencies: &BTreeMap<String, Vec<String>>,
) -> Vec<String> {
    let mut indegree = scope
        .iter()
        .map(|id| (id.clone(), 0usize))
        .collect::<BTreeMap<_, _>>();
    let mut dependents = BTreeMap::<String, Vec<String>>::new();
    for item_id in scope {
        for dependency in dependencies.get(item_id).into_iter().flatten() {
            if scope.contains(dependency) {
                *indegree.entry(item_id.clone()).or_default() += 1;
                dependents
                    .entry(dependency.clone())
                    .or_default()
                    .push(item_id.clone());
            }
        }
    }
    let mut ready = indegree
        .iter()
        .filter(|(_, count)| **count == 0)
        .map(|(id, _)| id.clone())
        .collect::<VecDeque<_>>();
    let mut order = Vec::new();
    while let Some(item_id) = ready.pop_front() {
        order.push(item_id.clone());
        for dependent in dependents.get(&item_id).into_iter().flatten() {
            if let Some(count) = indegree.get_mut(dependent) {
                *count -= 1;
                if *count == 0 {
                    ready.push_back(dependent.clone());
                }
            }
        }
    }
    order
}

fn dependency_cycles(
    scope: &BTreeSet<String>,
    dependencies: &BTreeMap<String, Vec<String>>,
) -> Vec<Vec<String>> {
    let mut states = BTreeMap::<String, VisitState>::new();
    let mut stack = Vec::<String>::new();
    let mut seen = BTreeSet::<String>::new();
    let mut cycles = Vec::<Vec<String>>::new();
    for item_id in scope {
        detect_cycle_from(
            item_id,
            scope,
            dependencies,
            &mut states,
            &mut stack,
            &mut seen,
            &mut cycles,
        );
    }
    cycles
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum VisitState {
    Visiting,
    Visited,
}

fn detect_cycle_from(
    item_id: &str,
    scope: &BTreeSet<String>,
    dependencies: &BTreeMap<String, Vec<String>>,
    states: &mut BTreeMap<String, VisitState>,
    stack: &mut Vec<String>,
    seen: &mut BTreeSet<String>,
    cycles: &mut Vec<Vec<String>>,
) {
    match states.get(item_id).copied() {
        Some(VisitState::Visited) => return,
        Some(VisitState::Visiting) => {
            if let Some(index) = stack.iter().position(|id| id == item_id) {
                let mut cycle = stack[index..].to_vec();
                cycle.push(item_id.to_string());
                let key = cycle.join(" -> ");
                if seen.insert(key) {
                    cycles.push(cycle);
                }
            }
            return;
        }
        None => {}
    }
    states.insert(item_id.to_string(), VisitState::Visiting);
    stack.push(item_id.to_string());
    for dependency in dependencies.get(item_id).into_iter().flatten() {
        if scope.contains(dependency) {
            detect_cycle_from(dependency, scope, dependencies, states, stack, seen, cycles);
        }
    }
    stack.pop();
    states.insert(item_id.to_string(), VisitState::Visited);
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use std::{fs, process::Command};
    use tempfile::TempDir;

    #[test]
    fn graph_reports_linear_chain_and_independent_roots() {
        let project = project_fixture();
        write_item(project.path(), "PROJ-001", "Root", "P1", &[]);
        write_item(project.path(), "PROJ-002", "Middle", "P1", &["PROJ-001"]);
        write_item(project.path(), "PROJ-003", "Leaf", "P1", &["PROJ-002"]);
        write_item(project.path(), "PROJ-004", "Independent", "P2", &[]);

        let graph = inspect_dependency_graph(
            project.path(),
            InspectDependencyGraphParams {
                root: Some(project.path().to_string_lossy().into_owned()),
                focus_item_id: None,
                include_closed: None,
                limit: None,
            },
        );
        let data = graph.data.expect("graph data");

        assert!(matches!(graph.status, ActionStatus::Completed));
        assert_eq!(data.edges.len(), 2);
        assert_eq!(
            data.topological_order,
            ["PROJ-001", "PROJ-004", "PROJ-002", "PROJ-003"]
        );
        assert_eq!(data.roots, ["PROJ-001", "PROJ-004"]);
        assert_eq!(data.leaves, ["PROJ-003", "PROJ-004"]);
        assert_eq!(data.runnable_nodes, ["PROJ-001", "PROJ-004"]);
        assert_eq!(data.blocked_nodes, ["PROJ-002", "PROJ-003"]);
    }

    #[test]
    fn graph_respects_git_closed_dependencies() {
        let project = project_fixture();
        write_item(project.path(), "PROJ-001", "Closed", "P1", &[]);
        write_item(project.path(), "PROJ-002", "Runnable", "P1", &["PROJ-001"]);
        init_git(project.path());
        git(project.path(), &["add", "--all"]);
        git(
            project.path(),
            &[
                "commit",
                "-m",
                "Complete first",
                "-m",
                "Platypus-Closes: PROJ-001",
                "-m",
                "Platypus-Verification: make check",
            ],
        );

        let graph = inspect_dependency_graph(
            project.path(),
            InspectDependencyGraphParams {
                root: Some(project.path().to_string_lossy().into_owned()),
                focus_item_id: None,
                include_closed: Some(true),
                limit: None,
            },
        );
        let data = graph.data.expect("graph data");

        assert!(matches!(graph.status, ActionStatus::Completed));
        assert_eq!(data.closed_nodes, ["PROJ-001"]);
        assert_eq!(data.runnable_nodes, ["PROJ-002"]);
        assert!(data
            .nodes
            .iter()
            .any(|node| node.item_id == "PROJ-002" && node.blocked_by.is_empty()));
    }

    #[test]
    fn graph_reports_missing_dependencies() {
        let project = project_fixture();
        write_item(project.path(), "PROJ-001", "Missing", "P1", &["PROJ-999"]);

        let graph = inspect_dependency_graph(
            project.path(),
            InspectDependencyGraphParams {
                root: Some(project.path().to_string_lossy().into_owned()),
                focus_item_id: None,
                include_closed: None,
                limit: None,
            },
        );
        let data = graph.data.expect("graph data");

        assert!(matches!(graph.status, ActionStatus::Failed));
        assert_eq!(data.missing_dependencies.len(), 1);
        assert_eq!(data.missing_dependencies[0].item_id, "PROJ-001");
        assert_eq!(data.missing_dependencies[0].missing_dependency, "PROJ-999");
    }

    #[test]
    fn graph_detects_cycles() {
        let project = project_fixture();
        write_item(project.path(), "PROJ-001", "First", "P1", &["PROJ-002"]);
        write_item(project.path(), "PROJ-002", "Second", "P1", &["PROJ-001"]);

        let graph = inspect_dependency_graph(
            project.path(),
            InspectDependencyGraphParams {
                root: Some(project.path().to_string_lossy().into_owned()),
                focus_item_id: None,
                include_closed: None,
                limit: None,
            },
        );
        let data = graph.data.expect("graph data");

        assert!(matches!(graph.status, ActionStatus::Failed));
        assert_eq!(data.cycles.len(), 1);
        assert_eq!(data.cycles[0], ["PROJ-001", "PROJ-002", "PROJ-001"]);
    }

    #[test]
    fn graph_focuses_on_dependency_neighborhood() {
        let project = project_fixture();
        write_item(project.path(), "PROJ-001", "Ancestor", "P1", &[]);
        write_item(project.path(), "PROJ-002", "Focus", "P1", &["PROJ-001"]);
        write_item(project.path(), "PROJ-003", "Dependent", "P1", &["PROJ-002"]);
        write_item(project.path(), "PROJ-004", "Unrelated", "P1", &[]);

        let graph = inspect_dependency_graph(
            project.path(),
            InspectDependencyGraphParams {
                root: Some(project.path().to_string_lossy().into_owned()),
                focus_item_id: Some("PROJ-002".to_string()),
                include_closed: None,
                limit: None,
            },
        );
        let data = graph.data.expect("graph data");

        assert!(matches!(graph.status, ActionStatus::Completed));
        assert_eq!(data.nodes.len(), 3);
        assert_eq!(data.roots, ["PROJ-001"]);
        assert_eq!(data.leaves, ["PROJ-003"]);
        assert!(!data.nodes.iter().any(|node| node.item_id == "PROJ-004"));
    }

    #[test]
    fn graph_reports_truncation() {
        let project = project_fixture();
        write_item(project.path(), "PROJ-001", "First", "P1", &[]);
        write_item(project.path(), "PROJ-002", "Second", "P1", &[]);

        let graph = inspect_dependency_graph(
            project.path(),
            InspectDependencyGraphParams {
                root: Some(project.path().to_string_lossy().into_owned()),
                focus_item_id: None,
                include_closed: None,
                limit: Some(1),
            },
        );
        let data = graph.data.expect("graph data");

        assert!(matches!(graph.status, ActionStatus::Completed));
        assert_eq!(data.total, 2);
        assert_eq!(data.returned, 1);
        assert!(data.truncated);
    }

    #[test]
    fn graph_detects_cycles_outside_return_limit() {
        let project = project_fixture();
        write_item(project.path(), "PROJ-001", "Returned", "P1", &[]);
        write_item(project.path(), "PROJ-002", "Cycle one", "P1", &["PROJ-003"]);
        write_item(project.path(), "PROJ-003", "Cycle two", "P1", &["PROJ-002"]);

        let graph = inspect_dependency_graph(
            project.path(),
            InspectDependencyGraphParams {
                root: Some(project.path().to_string_lossy().into_owned()),
                focus_item_id: None,
                include_closed: None,
                limit: Some(1),
            },
        );
        let data = graph.data.expect("graph data");

        assert!(matches!(graph.status, ActionStatus::Failed));
        assert_eq!(data.returned, 1);
        assert!(data.truncated);
        assert_eq!(data.cycles[0], ["PROJ-002", "PROJ-003", "PROJ-002"]);
    }

    fn project_fixture() -> TempDir {
        let temp = TempDir::new().expect("temp dir");
        fs::write(temp.path().join("platy.yaml"), "project: test\n").expect("config");
        fs::create_dir_all(temp.path().join("backlog/items")).expect("items dir");
        fs::create_dir_all(temp.path().join("backlog/epics")).expect("epics dir");
        fs::write(
            temp.path().join("backlog/epics/general.md"),
            "---\nid: general\ntitle: General\nstatus: active\npriority: P1\narea: general\n---\n\n# General\n",
        )
        .expect("epic");
        temp
    }

    fn write_item(root: &Path, id: &str, title: &str, priority: &str, depends_on: &[&str]) {
        let depends = if depends_on.is_empty() {
            "[]".to_string()
        } else {
            format!(
                "\n{}",
                depends_on
                    .iter()
                    .map(|dependency| format!("  - {}", dependency))
                    .collect::<Vec<_>>()
                    .join("\n")
            )
        };
        let text = format!(
            "---\nid: {id}\ntitle: {title}\npriority: {priority}\ntype: feature\narea: tooling\nepic: general\ndepends_on: {depends}\nowned_surfaces: []\n---\n\n# {id} {title}\n\n## Goal\n\nGoal.\n\n## Implementation Contract\n\nContract.\n\n## Acceptance\n\n- Done.\n"
        );
        fs::write(root.join(format!("backlog/items/{id}.md")), text).expect("item");
    }

    fn init_git(root: &Path) {
        git(root, &["init"]);
        git(root, &["config", "user.name", "Platypus Test"]);
        git(root, &["config", "user.email", "platypus@example.invalid"]);
    }

    fn git(root: &Path, args: &[&str]) {
        let output = Command::new("git")
            .args(args)
            .current_dir(root)
            .output()
            .expect("git command");
        assert!(
            output.status.success(),
            "git {:?} failed\nstdout:\n{}\nstderr:\n{}",
            args,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
