use crate::{
    git_trailers::parse_platypus_trailers,
    storage::{self, EventStore},
};
use serde_json::Value;
use std::{
    collections::BTreeSet,
    path::Path,
    process::{Command, Stdio},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ClosureSources {
    pub runtime_completion: bool,
    pub git_trailer: bool,
}

pub(crate) fn closed_item_ids(root: &Path) -> BTreeSet<String> {
    let mut ids = runtime_completed_item_ids(root);
    ids.extend(git_trailer_closed_item_ids(root));
    ids
}

pub(crate) fn closure_sources(root: &Path, item_id: &str) -> ClosureSources {
    let item_id = item_id.trim().to_ascii_uppercase();
    ClosureSources {
        runtime_completion: runtime_completed_item_ids(root).contains(&item_id),
        git_trailer: git_trailer_closed_item_ids(root).contains(&item_id),
    }
}

fn git_trailer_closed_item_ids(root: &Path) -> BTreeSet<String> {
    let output = Command::new("git")
        .args(["log", "--format=%B%x00", "--all"])
        .current_dir(root)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output();
    let Ok(output) = output else {
        return BTreeSet::new();
    };
    if output.status.success() {
        parse_platypus_trailers(&String::from_utf8_lossy(&output.stdout)).closes
    } else {
        BTreeSet::new()
    }
}

fn runtime_completed_item_ids(root: &Path) -> BTreeSet<String> {
    let Ok(Some(storage)) = storage::connect_existing_read_only(root, None) else {
        return BTreeSet::new();
    };
    let Ok(events) = storage
        .repository()
        .events()
        .list(Some("backlog"), None, 10_000)
    else {
        return BTreeSet::new();
    };
    events
        .into_iter()
        .filter(|event| event.event_type == "backlog_item_completed")
        .filter_map(|event| item_id_from_payload(event.payload.as_ref()))
        .collect()
}

fn item_id_from_payload(payload: Option<&Value>) -> Option<String> {
    payload?
        .get("item_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .map(|value| value.to_ascii_uppercase())
        .filter(|value| valid_item_id(value))
}

fn valid_item_id(value: &str) -> bool {
    let Some((prefix, number)) = value.split_once('-') else {
        return false;
    };
    !prefix.is_empty()
        && prefix
            .chars()
            .all(|character| character.is_ascii_uppercase())
        && number.len() == 3
        && number.chars().all(|character| character.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git_trailers::CLOSE_TRAILER;

    #[test]
    fn parses_closure_trailers_with_multiple_values() {
        let ids = parse_platypus_trailers(
            "Platypus-Closes: MCP-001\nPlatypus-Closes: MCP-002, MCP-003\nnot-an-id\n",
        )
        .closes;

        assert!(ids.contains("MCP-001"));
        assert!(ids.contains("MCP-002"));
        assert!(ids.contains("MCP-003"));
        assert!(!ids.contains("NOT-AN-ID"));
    }

    #[test]
    fn close_trailer_key_is_stable() {
        assert_eq!(CLOSE_TRAILER, "Platypus-Closes");
    }

    #[test]
    fn parses_runtime_completion_payload_item_ids() {
        let payload = serde_json::json!({ "item_id": "mcp-001" });

        assert_eq!(
            item_id_from_payload(Some(&payload)).as_deref(),
            Some("MCP-001")
        );
    }
}
