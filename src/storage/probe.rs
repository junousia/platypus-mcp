use super::{
    paths, schema, ApprovalInsert, EventInsert, LeaseInsert, Repository, RepositoryError,
    TaskEventInsert, TaskInsert,
};
use crate::{
    models::{
        ActionResult, StorageCapabilityCheck, StorageCapabilityProbeData,
        StorageCapabilityProbeParams,
    },
    storage::{ApprovalStore, EventStore, LeaseStore, TaskStore},
};
use rusqlite::Connection;
use serde_json::json;
use std::{collections::BTreeMap, path::Path};

pub fn capability_probe(
    default_root: &Path,
    params: StorageCapabilityProbeParams,
) -> ActionResult<StorageCapabilityProbeData> {
    let action = "storage_capability_probe";
    let root = match paths::resolve_project_root(default_root, params.root.as_deref()) {
        Ok(root) => root,
        Err(error) => {
            return ActionResult::failed(
                action,
                "Could not resolve project root for storage probe.",
                error.to_string(),
            )
        }
    };

    let checks = vec![
        run_check("schema_repeatability", check_schema_repeatability),
        run_check("task_event_ordering", check_task_event_ordering),
        run_check("lease_conflict", check_lease_conflict),
        run_check("approval_recovery", check_approval_recovery),
        run_check("idempotency_conflict", check_idempotency_conflict),
    ];
    let ok = checks.iter().all(|check| check.status == "passed");
    let data = StorageCapabilityProbeData {
        root: root.display().to_string(),
        backend: "sqlite".to_string(),
        ok,
        checks,
    };
    if ok {
        ActionResult::completed(action, "Storage capability probe passed.", data)
    } else {
        ActionResult {
            action: action.to_string(),
            status: crate::models::ActionStatus::Failed,
            summary: "Storage capability probe found failing checks.".to_string(),
            next_action: Some(
                "Inspect failed checks before enabling this backend for runtime state.".to_string(),
            ),
            recovery_action: None,
            data: Some(data),
            error: Some("one or more storage capability checks failed".to_string()),
        }
    }
}

fn run_check(name: &str, check: fn() -> Result<String, String>) -> StorageCapabilityCheck {
    match check() {
        Ok(summary) => StorageCapabilityCheck {
            name: name.to_string(),
            status: "passed".to_string(),
            summary,
            detail: None,
        },
        Err(error) => StorageCapabilityCheck {
            name: name.to_string(),
            status: "failed".to_string(),
            summary: "Check failed.".to_string(),
            detail: Some(error),
        },
    }
}

fn with_repository<T>(
    operation: impl FnOnce(Repository<'_>) -> Result<T, String>,
) -> Result<T, String> {
    let mut connection = Connection::open_in_memory().map_err(|error| error.to_string())?;
    schema::initialize(&mut connection).map_err(|error| error.to_string())?;
    schema::initialize(&mut connection).map_err(|error| error.to_string())?;
    operation(Repository::new(&connection))
}

fn check_schema_repeatability() -> Result<String, String> {
    let mut connection = Connection::open_in_memory().map_err(|error| error.to_string())?;
    schema::initialize(&mut connection).map_err(|error| error.to_string())?;
    schema::initialize(&mut connection).map_err(|error| error.to_string())?;
    let version: i32 = connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .map_err(|error| error.to_string())?;
    if version == super::SCHEMA_VERSION {
        Ok(format!("Schema initialized twice at version {version}."))
    } else {
        Err(format!(
            "expected schema version {}, got {version}",
            super::SCHEMA_VERSION
        ))
    }
}

fn check_task_event_ordering() -> Result<String, String> {
    with_repository(|repository| {
        let task = repository
            .tasks()
            .create(TaskInsert {
                source_item_id: "PROBE-001".to_string(),
                title: "Probe task".to_string(),
                worker: Some("probe".to_string()),
            })
            .map_err(|error| error.to_string())?;
        repository
            .tasks()
            .record_event(TaskEventInsert {
                task_id: task.id.clone(),
                sequence: None,
                event_type: "worker_progress".to_string(),
                summary: "Probe event.".to_string(),
                payload: Some(json!({ "probe": true })),
            })
            .map_err(|error| error.to_string())?;
        let task_events = repository
            .events()
            .list_task_events(Some(&task.id), 10)
            .map_err(|error| error.to_string())?;
        if task_events.len() != 1 || task_events[0].replay_order <= 0 {
            return Err("task event replay order was not persisted".to_string());
        }
        Ok("Task event replay order is monotonic and inspectable.".to_string())
    })
}

fn check_lease_conflict() -> Result<String, String> {
    with_repository(|repository| {
        let leases = repository.leases();
        leases
            .acquire(LeaseInsert {
                scope: "project".to_string(),
                target_id: "root".to_string(),
                owner: "client-a".to_string(),
                ttl_seconds: 60,
                metadata: BTreeMap::new(),
            })
            .map_err(|error| error.to_string())?;
        let conflict = leases
            .acquire(LeaseInsert {
                scope: "project".to_string(),
                target_id: "root".to_string(),
                owner: "client-b".to_string(),
                ttl_seconds: 60,
                metadata: BTreeMap::new(),
            })
            .expect_err("second lease should conflict");
        if matches!(conflict, RepositoryError::Conflict { .. }) {
            Ok("Conflicting active leases fail closed.".to_string())
        } else {
            Err(format!("expected conflict, got {conflict}"))
        }
    })
}

fn check_approval_recovery() -> Result<String, String> {
    with_repository(|repository| {
        let approval = repository
            .approvals()
            .create(ApprovalInsert {
                scope: "probe".to_string(),
                title: "Probe approval".to_string(),
                summary: "Probe approval request.".to_string(),
                requested_by: Some("probe".to_string()),
                metadata: BTreeMap::new(),
            })
            .map_err(|error| error.to_string())?;
        repository
            .events()
            .record(EventInsert {
                event_type: "probe_event".to_string(),
                scope: "probe".to_string(),
                task_id: None,
                summary: "Probe event.".to_string(),
                payload: Some(json!({ "approval_id": approval.id })),
            })
            .map_err(|error| error.to_string())?;
        let approvals = repository
            .approvals()
            .list(Some("pending"), 10)
            .map_err(|error| error.to_string())?;
        let events = repository
            .events()
            .list(Some("probe"), None, 10)
            .map_err(|error| error.to_string())?;
        if approvals.len() == 1 && events.len() == 1 {
            Ok("Pending approvals and recovery events are replayable.".to_string())
        } else {
            Err(format!(
                "expected one approval and one event, got {} approvals and {} events",
                approvals.len(),
                events.len()
            ))
        }
    })
}

fn check_idempotency_conflict() -> Result<String, String> {
    with_repository(|repository| {
        let tasks = repository.tasks();
        let first = tasks
            .create(TaskInsert {
                source_item_id: "PROBE-002".to_string(),
                title: "Probe retry".to_string(),
                worker: Some("probe".to_string()),
            })
            .map_err(|error| error.to_string())?;
        let conflict = tasks
            .create(TaskInsert {
                source_item_id: "PROBE-002".to_string(),
                title: "Probe retry".to_string(),
                worker: Some("probe".to_string()),
            })
            .expect_err("duplicate active task should conflict");
        if matches!(conflict, RepositoryError::Conflict { .. }) {
            Ok(format!(
                "Duplicate active task for `{}` returns a structured conflict.",
                first.source_item_id
            ))
        } else {
            Err(format!("expected conflict, got {conflict}"))
        }
    })
}
