use super::types::{EpicFrontmatter, ParsedBacklogItem};
use std::{collections::BTreeSet, fs, path::Path};

pub(super) fn parse_epic(path: &Path) -> std::result::Result<EpicFrontmatter, String> {
    let text = fs::read_to_string(path).map_err(|error| error.to_string())?;
    let (frontmatter, _) = split_frontmatter(&text)?;
    serde_yaml::from_str(frontmatter).map_err(|error| error.to_string())
}

pub(super) fn parse_backlog_item(path: &Path) -> std::result::Result<ParsedBacklogItem, String> {
    let text = fs::read_to_string(path).map_err(|error| error.to_string())?;
    let (frontmatter, body) = split_frontmatter(&text)?;
    let frontmatter = serde_yaml::from_str(frontmatter).map_err(|error| error.to_string())?;
    Ok(ParsedBacklogItem {
        frontmatter,
        path: path.to_path_buf(),
        sections: markdown_sections(body),
    })
}

fn split_frontmatter(text: &str) -> std::result::Result<(&str, &str), String> {
    let rest = text
        .strip_prefix("---\n")
        .ok_or_else(|| "missing YAML frontmatter".to_string())?;
    let marker = "\n---\n";
    let end = rest
        .find(marker)
        .ok_or_else(|| "unterminated YAML frontmatter".to_string())?;
    let frontmatter = &rest[..end];
    let body = &rest[end + marker.len()..];
    Ok((frontmatter, body))
}

fn markdown_sections(body: &str) -> BTreeSet<String> {
    body.lines()
        .filter_map(|line| line.strip_prefix("## "))
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}
