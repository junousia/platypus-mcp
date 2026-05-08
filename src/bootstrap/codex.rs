use super::types::{ServerConfig, SERVER_NAME};
use anyhow::{bail, Result};

pub(super) fn is_configured(existing: &str, server: &ServerConfig) -> bool {
    let lines: Vec<&str> = existing.lines().collect();
    let Some(start) = lines
        .iter()
        .position(|line| is_platypus_header(line.trim()))
    else {
        return false;
    };
    let end = lines
        .iter()
        .enumerate()
        .skip(start + 1)
        .find_map(|(index, line)| line.trim_start().starts_with('[').then_some(index))
        .unwrap_or(lines.len());
    let current = lines[start..end].join("\n");
    current.contains("command = \"sh\"")
        && current.contains("platypus-mcp")
        && server
            .root
            .as_deref()
            .map(|root| current.contains("PLATYPUS_MCP_ROOT") && current.contains(root))
            .unwrap_or(true)
}

pub(super) fn render(existing: &str, server: &ServerConfig, force: bool) -> Result<String> {
    let entry = server_entry(server);
    if existing.trim().is_empty() {
        return Ok(format!("{entry}\n"));
    }

    let lines: Vec<&str> = existing.lines().collect();
    let start = lines
        .iter()
        .position(|line| is_platypus_header(line.trim()));
    let Some(start) = start else {
        let mut text = existing.trim_end().to_string();
        text.push_str("\n\n");
        text.push_str(&entry);
        text.push('\n');
        return Ok(text);
    };

    let end = lines
        .iter()
        .enumerate()
        .skip(start + 1)
        .find_map(|(index, line)| line.trim_start().starts_with('[').then_some(index))
        .unwrap_or(lines.len());
    let current = lines[start..end].join("\n");
    ensure_replace_allowed(&current, force)?;

    let mut output = Vec::new();
    output.extend(lines[..start].iter().map(|line| (*line).to_string()));
    output.extend(entry.lines().map(str::to_string));
    output.extend(lines[end..].iter().map(|line| (*line).to_string()));
    Ok(output.join("\n") + "\n")
}

fn server_entry(server: &ServerConfig) -> String {
    let mut lines = vec![
        format!("[mcp_servers.\"{SERVER_NAME}\"]"),
        "command = \"sh\"".to_string(),
        "args = [\"-lc\", \"exec \\\"$HOME/.cargo/bin/platypus-mcp\\\"\"]".to_string(),
    ];
    if let Some(root) = server.root.as_deref() {
        lines.push(format!(
            "env = {{ PLATYPUS_MCP_ROOT = \"{}\" }}",
            escape_toml_string(root)
        ));
    }
    lines.join("\n")
}

fn is_platypus_header(line: &str) -> bool {
    line == "[mcp_servers.platypus]" || line == "[mcp_servers.\"platypus\"]"
}

fn ensure_replace_allowed(current: &str, force: bool) -> Result<()> {
    if force || current.contains("platypus-mcp") || current.contains("cargo") {
        return Ok(());
    }
    bail!("existing platypus server config does not look like Platypus MCP; pass --force to replace it")
}

fn escape_toml_string(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}
