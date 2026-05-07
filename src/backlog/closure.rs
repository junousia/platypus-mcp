use crate::git_trailers::parse_platypus_trailers;
use std::{
    collections::BTreeSet,
    path::Path,
    process::{Command, Stdio},
};

pub(crate) fn closed_item_ids(root: &Path) -> BTreeSet<String> {
    let output = Command::new("git")
        .args(["log", "--format=%B%x00", "--all"])
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
    parse_platypus_trailers(&String::from_utf8_lossy(&output.stdout)).closes
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
}
