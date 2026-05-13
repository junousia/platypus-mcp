use crate::{
    models::{
        AcquireLeaseParams, ActionResult, ActionStatus, LeaseListData, LeaseRecordData,
        ListLeasesParams, ReleaseLeaseParams, RenewLeaseParams,
    },
    storage::{self, LeaseInsert, LeaseStore, RepositoryError},
};
use std::path::Path;

const DEFAULT_LIMIT: usize = 50;
const MAX_LIMIT: usize = 200;
const DEFAULT_TTL_SECONDS: u64 = 900;
const MAX_TTL_SECONDS: u64 = 86_400;

pub fn acquire_lease(
    default_root: &Path,
    params: AcquireLeaseParams,
) -> ActionResult<LeaseRecordData> {
    let action = "acquire_lease";
    let scope = match clean_scope(&params.scope) {
        Ok(scope) => scope,
        Err(error) => return ActionResult::failed(action, "Could not acquire lease.", error),
    };
    let target_id = match clean_required("target_id", &params.target_id) {
        Ok(target_id) => target_id,
        Err(error) => return ActionResult::failed(action, "Could not acquire lease.", error),
    };
    let owner = match clean_required("owner", &params.owner) {
        Ok(owner) => owner,
        Err(error) => return ActionResult::failed(action, "Could not acquire lease.", error),
    };
    let storage = match storage::connect(default_root, params.root.as_deref()) {
        Ok(storage) => storage,
        Err(error) => {
            return ActionResult::failed(action, "Could not open lease storage.", error.to_string())
        }
    };
    let lease = match storage.repository().leases().acquire(LeaseInsert {
        scope,
        target_id,
        owner,
        ttl_seconds: bounded_ttl(params.ttl_seconds),
        metadata: params.metadata,
    }) {
        Ok(lease) => lease,
        Err(RepositoryError::Conflict { message }) => {
            return ActionResult::skipped(action, message, "Inspect leases or wait for expiry.")
        }
        Err(error) => {
            return ActionResult::failed(action, "Could not acquire lease.", error.to_string())
        }
    };
    ActionResult::completed(
        action,
        format!("Lease `{}` acquired.", lease.id),
        LeaseRecordData {
            root: storage.storage.root.display().to_string(),
            lease,
        },
    )
}

pub fn list_leases(default_root: &Path, params: ListLeasesParams) -> ActionResult<LeaseListData> {
    let action = "list_leases";
    let storage = match storage::connect(default_root, params.root.as_deref()) {
        Ok(storage) => storage,
        Err(error) => {
            return ActionResult::failed(action, "Could not open lease storage.", error.to_string())
        }
    };
    let scope = params
        .scope
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty());
    let target_id = params
        .target_id
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty());
    let status = params
        .status
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty());
    let leases = match storage.repository().leases().list(
        scope,
        target_id,
        status,
        params.include_expired.unwrap_or(false),
        bounded_limit(params.limit),
    ) {
        Ok(leases) => leases,
        Err(error) => {
            return ActionResult::failed(action, "Could not list leases.", error.to_string())
        }
    };
    let returned = leases.len();
    let data = LeaseListData {
        root: storage.storage.root.display().to_string(),
        leases,
        returned,
    };
    if returned == 0 {
        ActionResult {
            action: action.to_string(),
            status: ActionStatus::Skipped,
            summary: "No leases matched the filters.".to_string(),
            next_action: Some("Acquire a lease before listing active leases.".to_string()),
            recovery_action: None,
            data: Some(data),
            error: None,
        }
    } else {
        ActionResult::completed(action, format!("Returned {returned} lease(s)."), data)
    }
}

pub fn renew_lease(default_root: &Path, params: RenewLeaseParams) -> ActionResult<LeaseRecordData> {
    let action = "renew_lease";
    let storage = match storage::connect(default_root, params.root.as_deref()) {
        Ok(storage) => storage,
        Err(error) => {
            return ActionResult::failed(action, "Could not open lease storage.", error.to_string())
        }
    };
    let lease = match storage.repository().leases().renew(
        params.lease_id.trim(),
        params.owner.trim(),
        bounded_ttl(params.ttl_seconds),
    ) {
        Ok(lease) => lease,
        Err(RepositoryError::NotFound) => {
            return ActionResult::skipped(
                action,
                "Lease was not found, expired, released, or owned by another owner.",
                "List active leases and retry with the correct lease id and owner.",
            )
        }
        Err(error) => {
            return ActionResult::failed(action, "Could not renew lease.", error.to_string())
        }
    };
    ActionResult::completed(
        action,
        format!("Lease `{}` renewed.", lease.id),
        LeaseRecordData {
            root: storage.storage.root.display().to_string(),
            lease,
        },
    )
}

pub fn release_lease(
    default_root: &Path,
    params: ReleaseLeaseParams,
) -> ActionResult<LeaseRecordData> {
    let action = "release_lease";
    let storage = match storage::connect(default_root, params.root.as_deref()) {
        Ok(storage) => storage,
        Err(error) => {
            return ActionResult::failed(action, "Could not open lease storage.", error.to_string())
        }
    };
    let lease = match storage
        .repository()
        .leases()
        .release(params.lease_id.trim(), params.owner.trim())
    {
        Ok(lease) => lease,
        Err(RepositoryError::NotFound) => {
            return ActionResult::skipped(
                action,
                "Lease was not found, released, or owned by another owner.",
                "List active leases and retry with the correct lease id and owner.",
            )
        }
        Err(error) => {
            return ActionResult::failed(action, "Could not release lease.", error.to_string())
        }
    };
    ActionResult::completed(
        action,
        format!("Lease `{}` released.", lease.id),
        LeaseRecordData {
            root: storage.storage.root.display().to_string(),
            lease,
        },
    )
}

fn clean_scope(value: &str) -> Result<String, String> {
    let scope = clean_required("scope", value)?;
    if matches!(scope.as_str(), "project" | "task") {
        Ok(scope)
    } else {
        Err("lease scope must be project or task".to_string())
    }
}

fn clean_required(field: &str, value: &str) -> Result<String, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        Err(format!("{field} is required"))
    } else {
        Ok(trimmed.to_string())
    }
}

fn bounded_limit(limit: Option<usize>) -> usize {
    limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT)
}

fn bounded_ttl(ttl_seconds: Option<u64>) -> u64 {
    ttl_seconds
        .unwrap_or(DEFAULT_TTL_SECONDS)
        .clamp(1, MAX_TTL_SECONDS)
}
