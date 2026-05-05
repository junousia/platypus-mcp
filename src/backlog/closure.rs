use std::{
    collections::BTreeSet,
    path::Path,
    process::{Command, Stdio},
};

const CLOSE_TRAILER: &str = "Platypus-Closes";

pub(super) fn closed_item_ids(root: &Path) -> BTreeSet<String> {
    let trailer_format = format!("--format=%(trailers:key={CLOSE_TRAILER},valueonly)");
    let output = Command::new("git")
        .args(["log", trailer_format.as_str(), "--all"])
        .current_dir(root)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output();
    let Ok(output) = output else {
        return BTreeSet::new();
    };
    if !output.status.success() {
        return BTreeSet::new();
    }
    parse_closed_item_ids(&String::from_utf8_lossy(&output.stdout))
}

fn parse_closed_item_ids(output: &str) -> BTreeSet<String> {
    output
        .lines()
        .flat_map(|line| {
            line.split([',', ' '])
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(normalize_item_id)
                .collect::<Vec<_>>()
        })
        .filter(|value| valid_item_id_shape(value))
        .collect()
}

fn normalize_item_id(value: &str) -> String {
    value.trim().to_ascii_uppercase()
}

fn valid_item_id_shape(value: &str) -> bool {
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

    #[test]
    fn parses_closure_trailers_with_multiple_values() {
        let ids = parse_closed_item_ids("MCP-001\nMCP-002, MCP-003\nnot-an-id\n");

        assert!(ids.contains("MCP-001"));
        assert!(ids.contains("MCP-002"));
        assert!(ids.contains("MCP-003"));
        assert!(!ids.contains("NOT-AN-ID"));
    }

    #[test]
    fn close_trailer_key_is_stable() {
        assert_eq!(CLOSE_TRAILER, "Platypus-Closes");
    }
}
