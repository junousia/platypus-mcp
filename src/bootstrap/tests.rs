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
fn bootstrap_init_project_creates_repo_guidance() {
    let temp = TempDir::new().expect("temp dir");
    let args = vec![
        "codex".to_string(),
        "--config".to_string(),
        temp.path().join("config.toml").display().to_string(),
        "--root".to_string(),
        temp.path().display().to_string(),
        "--init-project".to_string(),
        "--project-name".to_string(),
        "Bootstrap Test".to_string(),
    ];

    assert_eq!(run_cli(&args).expect("bootstrap"), 0);

    assert!(temp.path().join("config.toml").is_file());
    assert!(temp.path().join("AGENTS.md").is_file());
    assert!(temp.path().join("CLAUDE.md").is_file());
    assert!(temp.path().join("WORKFLOW.md").is_file());
    assert!(temp.path().join("backlog/README.md").is_file());
    let config = fs::read_to_string(temp.path().join("platy.yaml")).expect("project config");
    assert!(config.contains("Bootstrap Test"));
}

#[test]
fn bootstrap_init_project_dry_run_does_not_create_repo_guidance() {
    let temp = TempDir::new().expect("temp dir");
    let args = vec![
        "codex".to_string(),
        "--dry-run".to_string(),
        "--config".to_string(),
        temp.path().join("config.toml").display().to_string(),
        "--root".to_string(),
        temp.path().display().to_string(),
        "--init-project".to_string(),
    ];

    assert_eq!(run_cli(&args).expect("dry run"), 0);

    assert!(!temp.path().join("config.toml").exists());
    assert!(!temp.path().join("AGENTS.md").exists());
}

#[test]
fn bootstrap_check_with_init_project_requires_repo_guidance() {
    let temp = TempDir::new().expect("temp dir");
    let config = temp.path().join("config.toml");
    let apply = vec![
        "codex".to_string(),
        "--config".to_string(),
        config.display().to_string(),
        "--root".to_string(),
        temp.path().display().to_string(),
    ];
    assert_eq!(run_cli(&apply).expect("bootstrap"), 0);

    let check = vec![
        "codex".to_string(),
        "--check".to_string(),
        "--config".to_string(),
        config.display().to_string(),
        "--root".to_string(),
        temp.path().display().to_string(),
        "--init-project".to_string(),
    ];
    assert_eq!(run_cli(&check).expect("check missing project"), 1);

    let init = vec![
        "codex".to_string(),
        "--config".to_string(),
        config.display().to_string(),
        "--root".to_string(),
        temp.path().display().to_string(),
        "--init-project".to_string(),
    ];
    assert_eq!(run_cli(&init).expect("init project"), 0);
    assert_eq!(run_cli(&check).expect("check initialized project"), 0);
}

#[test]
fn bootstrap_check_with_init_project_requires_scaffold_directories() {
    let temp = TempDir::new().expect("temp dir");
    let config = temp.path().join("config.toml");
    let apply = vec![
        "codex".to_string(),
        "--config".to_string(),
        config.display().to_string(),
        "--root".to_string(),
        temp.path().display().to_string(),
        "--init-project".to_string(),
    ];
    assert_eq!(run_cli(&apply).expect("bootstrap"), 0);
    fs::remove_dir_all(temp.path().join("backlog/items")).expect("remove items");

    let check = vec![
        "codex".to_string(),
        "--check".to_string(),
        "--config".to_string(),
        config.display().to_string(),
        "--root".to_string(),
        temp.path().display().to_string(),
        "--init-project".to_string(),
    ];

    assert_eq!(run_cli(&check).expect("check missing directory"), 1);
}

#[test]
fn bootstrap_check_with_init_project_requires_direction_docs() {
    let temp = TempDir::new().expect("temp dir");
    let config = temp.path().join("config.toml");
    let apply = vec![
        "codex".to_string(),
        "--config".to_string(),
        config.display().to_string(),
        "--root".to_string(),
        temp.path().display().to_string(),
        "--init-project".to_string(),
    ];
    assert_eq!(run_cli(&apply).expect("bootstrap"), 0);
    fs::remove_file(temp.path().join("docs/product.md")).expect("remove product direction");

    let check = vec![
        "codex".to_string(),
        "--check".to_string(),
        "--config".to_string(),
        config.display().to_string(),
        "--root".to_string(),
        temp.path().display().to_string(),
        "--init-project".to_string(),
    ];

    assert_eq!(run_cli(&check).expect("check missing direction doc"), 1);
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
fn pi_bootstrap_writes_project_package_settings_and_init_project() {
    let temp = TempDir::new().expect("temp dir");
    let args = vec![
        "pi".to_string(),
        "--root".to_string(),
        temp.path().display().to_string(),
        "--init-project".to_string(),
        "--project-name".to_string(),
        "Pi Bootstrap Test".to_string(),
    ];

    assert_eq!(run_cli(&args).expect("bootstrap"), 0);

    let settings_path = temp.path().join(".pi/settings.json");
    let json: Value =
        serde_json::from_str(&fs::read_to_string(settings_path).expect("settings")).expect("json");
    assert_eq!(json["packages"][0], "npm:platypus-pi");
    assert!(temp.path().join("AGENTS.md").is_file());
    assert!(temp.path().join("CLAUDE.md").is_file());
    assert!(temp.path().join("WORKFLOW.md").is_file());
    assert!(temp.path().join("backlog/README.md").is_file());
    let config = fs::read_to_string(temp.path().join("platy.yaml")).expect("project config");
    assert!(config.contains("Pi Bootstrap Test"));
}

#[test]
fn pi_bootstrap_dry_run_does_not_write_settings_or_project_files() {
    let temp = TempDir::new().expect("temp dir");
    let args = vec![
        "pi".to_string(),
        "--dry-run".to_string(),
        "--root".to_string(),
        temp.path().display().to_string(),
        "--init-project".to_string(),
    ];

    assert_eq!(run_cli(&args).expect("dry run"), 0);

    assert!(!temp.path().join(".pi/settings.json").exists());
    assert!(!temp.path().join("AGENTS.md").exists());
}

#[test]
fn pi_bootstrap_check_requires_settings_and_requested_project_files() {
    let temp = TempDir::new().expect("temp dir");
    let check = vec![
        "pi".to_string(),
        "--check".to_string(),
        "--root".to_string(),
        temp.path().display().to_string(),
        "--init-project".to_string(),
    ];
    assert_eq!(run_cli(&check).expect("check missing"), 1);

    let apply = vec![
        "pi".to_string(),
        "--root".to_string(),
        temp.path().display().to_string(),
        "--init-project".to_string(),
    ];
    assert_eq!(run_cli(&apply).expect("bootstrap"), 0);
    assert_eq!(run_cli(&check).expect("check configured"), 0);
}

#[test]
fn pi_bootstrap_preserves_existing_settings_and_force_replaces_platypus_package() {
    let temp = TempDir::new().expect("temp dir");
    let settings = temp.path().join(".pi/settings.json");
    fs::create_dir_all(settings.parent().expect("settings parent")).expect("settings dir");
    fs::write(
        &settings,
        serde_json::json!({
            "theme": "dark",
            "packages": ["pi-skills", "git:github.com/junousia/platypus-mcp"]
        })
        .to_string(),
    )
    .expect("settings");

    let apply = vec![
        "pi".to_string(),
        "--root".to_string(),
        temp.path().display().to_string(),
    ];
    assert_eq!(run_cli(&apply).expect("already configured"), 0);
    let json: Value =
        serde_json::from_str(&fs::read_to_string(&settings).expect("settings")).expect("json");
    assert_eq!(json["theme"], "dark");
    assert_eq!(json["packages"][0], "pi-skills");
    assert_eq!(json["packages"][1], "git:github.com/junousia/platypus-mcp");

    let force = vec![
        "pi".to_string(),
        "--force".to_string(),
        "--root".to_string(),
        temp.path().display().to_string(),
    ];
    assert_eq!(run_cli(&force).expect("force"), 0);
    let json: Value =
        serde_json::from_str(&fs::read_to_string(settings).expect("settings")).expect("json");
    assert_eq!(json["theme"], "dark");
    assert_eq!(json["packages"][0], "pi-skills");
    assert_eq!(json["packages"][1], "npm:platypus-pi");
}

#[test]
fn bootstrap_smoke_covers_supported_host_startup_shapes() {
    let temp = TempDir::new().expect("temp dir");

    let codex_config = temp.path().join("codex.toml");
    assert_eq!(
        run_cli(&[
            "codex".to_string(),
            "--config".to_string(),
            codex_config.display().to_string(),
            "--root".to_string(),
            temp.path().display().to_string(),
        ])
        .expect("codex bootstrap"),
        0
    );
    let codex = fs::read_to_string(&codex_config).expect("codex config");
    assert!(codex.contains("[mcp_servers.\"platypus\"]"));
    assert!(codex.contains("PLATYPUS_MCP_ROOT"));
    assert!(codex.contains(temp.path().to_string_lossy().as_ref()));
    assert!(codex.contains("platypus-mcp"));

    let claude_config = temp.path().join(".mcp.json");
    assert_eq!(
        run_cli(&[
            "claude".to_string(),
            "--config".to_string(),
            claude_config.display().to_string(),
            "--root".to_string(),
            temp.path().display().to_string(),
        ])
        .expect("claude bootstrap"),
        0
    );
    let claude: Value =
        serde_json::from_str(&fs::read_to_string(&claude_config).expect("claude config"))
            .expect("claude json");
    assert_eq!(claude["mcpServers"]["platypus"]["command"], "sh");
    let claude_args = claude["mcpServers"]["platypus"]["args"]
        .as_array()
        .expect("claude args");
    assert!(claude_args.iter().any(|arg| arg
        .as_str()
        .is_some_and(|value| value.contains("platypus-mcp"))));
    assert_eq!(
        claude["mcpServers"]["platypus"]["env"]["PLATYPUS_MCP_ROOT"],
        temp.path().display().to_string()
    );

    let opencode_config = temp.path().join("opencode.json");
    assert_eq!(
        run_cli(&[
            "opencode".to_string(),
            "--config".to_string(),
            opencode_config.display().to_string(),
            "--root".to_string(),
            temp.path().display().to_string(),
        ])
        .expect("opencode bootstrap"),
        0
    );
    let opencode: Value =
        serde_json::from_str(&fs::read_to_string(&opencode_config).expect("opencode config"))
            .expect("opencode json");
    assert_eq!(opencode["mcp"]["platypus"]["type"], "local");
    assert_eq!(opencode["mcp"]["platypus"]["command"][0], "sh");
    assert_eq!(opencode["mcp"]["platypus"]["enabled"], true);

    assert_eq!(
        run_cli(&[
            "pi".to_string(),
            "--root".to_string(),
            temp.path().display().to_string(),
        ])
        .expect("pi bootstrap"),
        0
    );
    let pi: Value = serde_json::from_str(
        &fs::read_to_string(temp.path().join(".pi/settings.json")).expect("pi settings"),
    )
    .expect("pi json");
    assert!(pi["packages"]
        .as_array()
        .expect("packages")
        .iter()
        .any(|package| package == "npm:platypus-pi"));
}

#[test]
fn install_docs_describe_bootstrap_verification_matrix() {
    let docs = include_str!("../../docs/install.md");

    assert!(docs.contains("Bootstrap Verification Matrix"));
    assert!(docs.contains("bootstrap_smoke_covers_supported_host_startup_shapes"));
    assert!(docs.contains("invoke Codex, Claude, OpenCode, Pi"));
    assert!(docs.contains("| Codex | `codex.toml` |"));
    assert!(docs.contains("| Claude | `.mcp.json` |"));
    assert!(docs.contains("| OpenCode | `opencode.json` |"));
    assert!(docs.contains("| Pi | `.pi/settings.json` |"));
    assert!(docs.contains("`make check`"));
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
