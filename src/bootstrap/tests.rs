use super::*;
use serde_json::Value;
use tempfile::TempDir;

#[test]
fn codex_bootstrap_creates_project_config() {
    let temp = TempDir::new().expect("temp dir");
    let args = vec![
        "codex".to_string(),
        "--config".to_string(),
        temp.path().join("config.toml").display().to_string(),
        "--root".to_string(),
        temp.path().display().to_string(),
    ];

    assert_eq!(run_cli(&args).expect("bootstrap"), 0);
    let text = fs::read_to_string(temp.path().join("config.toml")).expect("config");

    assert!(text.contains("[mcp_servers.\"platypus\"]"));
    assert!(text.contains("command = \"sh\""));
    assert!(text.contains("platypus-mcp"));
    assert!(text.contains("PLATYPUS_MCP_ROOT"));
}

#[test]
fn codex_bootstrap_preserves_tool_sections() {
    let temp = TempDir::new().expect("temp dir");
    let config = temp.path().join("config.toml");
    fs::write(
        &config,
        "[mcp_servers.\"platypus\"]\ncommand = \"cargo\"\nargs = [\"run\"]\n\n[mcp_servers.\"platypus\".tools.ping]\napproval_mode = \"approve\"\n",
    )
    .expect("write config");

    let args = vec![
        "codex".to_string(),
        "--config".to_string(),
        config.display().to_string(),
    ];
    assert_eq!(run_cli(&args).expect("bootstrap"), 0);
    let text = fs::read_to_string(config).expect("config");

    assert!(text.contains("exec \\\"$HOME/.cargo/bin/platypus-mcp\\\""));
    assert!(text.contains("[mcp_servers.\"platypus\".tools.ping]"));
    assert!(text.contains("approval_mode = \"approve\""));
}

#[test]
fn bootstrap_check_returns_nonzero_when_missing() {
    let temp = TempDir::new().expect("temp dir");
    let args = vec![
        "codex".to_string(),
        "--check".to_string(),
        "--config".to_string(),
        temp.path().join("missing.toml").display().to_string(),
    ];

    assert_eq!(run_cli(&args).expect("check"), 1);
    assert!(!temp.path().join("missing.toml").exists());
}

#[test]
fn bootstrap_dry_run_does_not_write() {
    let temp = TempDir::new().expect("temp dir");
    let config = temp.path().join(".mcp.json");
    let args = vec![
        "claude".to_string(),
        "--dry-run".to_string(),
        "--config".to_string(),
        config.display().to_string(),
    ];

    assert_eq!(run_cli(&args).expect("dry-run"), 0);
    assert!(!config.exists());
}

#[test]
fn claude_bootstrap_merges_mcp_json() {
    let temp = TempDir::new().expect("temp dir");
    let config = temp.path().join(".mcp.json");
    fs::write(
        &config,
        serde_json::json!({
            "mcpServers": {
                "other": {
                    "command": "other-server",
                    "args": []
                }
            }
        })
        .to_string(),
    )
    .expect("write config");

    let args = vec![
        "claude".to_string(),
        "--config".to_string(),
        config.display().to_string(),
    ];
    assert_eq!(run_cli(&args).expect("bootstrap"), 0);
    let json: Value =
        serde_json::from_str(&fs::read_to_string(config).expect("config")).expect("json");

    assert_eq!(json["mcpServers"]["other"]["command"], "other-server");
    assert_eq!(json["mcpServers"]["platypus"]["command"], "sh");
}

#[test]
fn opencode_bootstrap_writes_local_mcp_shape() {
    let temp = TempDir::new().expect("temp dir");
    let config = temp.path().join("opencode.json");
    let args = vec![
        "opencode".to_string(),
        "--config".to_string(),
        config.display().to_string(),
    ];

    assert_eq!(run_cli(&args).expect("bootstrap"), 0);
    let json: Value =
        serde_json::from_str(&fs::read_to_string(config).expect("config")).expect("json");

    assert_eq!(json["$schema"], "https://opencode.ai/config.json");
    assert_eq!(json["mcp"]["platypus"]["type"], "local");
    assert_eq!(json["mcp"]["platypus"]["command"][0], "sh");
    assert_eq!(json["mcp"]["platypus"]["enabled"], true);
}

#[test]
fn bootstrap_refuses_unrecognized_existing_server_without_force() {
    let temp = TempDir::new().expect("temp dir");
    let config = temp.path().join(".mcp.json");
    fs::write(
        &config,
        serde_json::json!({
            "mcpServers": {
                "platypus": {
                    "command": "not-platypus",
                    "args": []
                }
            }
        })
        .to_string(),
    )
    .expect("write config");

    let args = vec![
        "claude".to_string(),
        "--config".to_string(),
        config.display().to_string(),
    ];
    let error = run_cli(&args).expect_err("should refuse");

    assert!(error.to_string().contains("--force"));
}
