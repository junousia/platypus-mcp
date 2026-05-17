mod codex;
mod json_hosts;
mod paths;
mod pi;
#[cfg(test)]
mod tests;
mod types;

use crate::{models::InitProjectParams, project};
use anyhow::{Context, Result};
use std::{env, fs, path::PathBuf};

pub use types::BootstrapCommand;
use types::{
    BootstrapInvocation, BootstrapMode, Host, ServerConfig, PI_PACKAGE_SOURCE, SERVER_LAUNCHER,
    SERVER_NAME,
};

pub fn run_cli(args: &[String]) -> Result<i32> {
    let invocation = BootstrapInvocation::parse(args)?;
    run_invocation(invocation)
}

pub fn run_command(command: BootstrapCommand) -> Result<i32> {
    run_invocation(types::invocation_from_command(command))
}

fn run_invocation(invocation: BootstrapInvocation) -> Result<i32> {
    if invocation.list_hosts {
        print_hosts();
        return Ok(0);
    }

    let plan = BootstrapPlan::for_invocation(&invocation)?;
    match invocation.mode {
        BootstrapMode::DryRun => {
            print_plan("planned", &plan);
            Ok(0)
        }
        BootstrapMode::Check => {
            print_plan(
                if plan.already_configured && plan.project_ready_for_check() {
                    "configured"
                } else {
                    "missing"
                },
                &plan,
            );
            Ok(i32::from(
                !plan.already_configured || !plan.project_ready_for_check(),
            ))
        }
        BootstrapMode::Apply => {
            plan.apply()?;
            let refreshed = BootstrapPlan::for_invocation(&invocation)?;
            print_plan("completed", &refreshed);
            Ok(0)
        }
    }
}

#[derive(Debug, Clone)]
struct BootstrapPlan {
    host: Host,
    path: PathBuf,
    rendered: String,
    existed: bool,
    already_configured: bool,
    project_root: PathBuf,
    init_project: bool,
    project_name: Option<String>,
    project_already_initialized: bool,
    force: bool,
}

impl BootstrapPlan {
    fn for_invocation(invocation: &BootstrapInvocation) -> Result<Self> {
        let host = invocation.host;
        let root = invocation.root.clone().unwrap_or(env::current_dir()?);
        let path = match invocation.config.clone() {
            Some(path) => path,
            None => paths::default_config_path(host, invocation.scope, &root)?,
        };
        let existing = fs::read_to_string(&path).ok();
        let existed = existing.is_some();
        let existing = existing.unwrap_or_default();
        let server = ServerConfig::new(invocation.root.as_deref());
        let rendered = match host {
            Host::Codex => codex::render(&existing, &server, invocation.force)?,
            Host::Claude => json_hosts::shared_mcp_config(&existing, &server, invocation.force)?,
            Host::Pi => pi::settings_config(&existing, PI_PACKAGE_SOURCE, &path, invocation.force)?,
            Host::Opencode => json_hosts::opencode_config(&existing, &server, invocation.force)?,
        };
        let already_configured = existed
            && match host {
                Host::Codex => codex::is_configured(&existing, &server),
                Host::Claude => json_hosts::shared_mcp_is_configured(&existing, &server),
                Host::Pi => pi::settings_is_configured(&existing, PI_PACKAGE_SOURCE, &path),
                Host::Opencode => json_hosts::opencode_is_configured(&existing, &server),
            };

        Ok(Self {
            host,
            path,
            rendered,
            existed,
            already_configured,
            project_root: root.clone(),
            init_project: invocation.init_project,
            project_name: invocation.project_name.clone(),
            project_already_initialized: project_scaffold_ready(&root),
            force: invocation.force,
        })
    }

    fn apply(&self) -> Result<()> {
        if self.force || !self.already_configured {
            if let Some(parent) = self.path.parent() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("create {}", parent.display()))?;
            }
            fs::write(&self.path, &self.rendered)
                .with_context(|| format!("write {}", self.path.display()))?;
        }
        if self.init_project {
            let result = project::init_project(
                &self.project_root,
                InitProjectParams {
                    root: Some(self.project_root.to_string_lossy().to_string()),
                    project_name: self.project_name.clone(),
                    overwrite: Some(false),
                },
            );
            if result.error.is_some() {
                anyhow::bail!(
                    "{}",
                    result
                        .error
                        .unwrap_or_else(|| "project initialization failed".to_string())
                );
            }
        }
        Ok(())
    }

    fn project_ready_for_check(&self) -> bool {
        !self.init_project || self.project_already_initialized
    }
}

fn print_hosts() {
    println!("Supported bootstrap hosts:");
    println!("  codex      Codex CLI TOML config");
    println!("  claude     Claude Code project .mcp.json config");
    println!("  opencode   OpenCode JSON config");
    println!("  pi         Pi project package settings");
}

fn print_plan(status: &str, plan: &BootstrapPlan) {
    println!("{} {} bootstrap", status, plan.host.label());
    println!("path: {}", plan.path.display());
    println!(
        "state: {}",
        if plan.already_configured {
            "already configured"
        } else if plan.existed {
            "update required"
        } else {
            "create required"
        }
    );
    if plan.force {
        println!("force: true");
    }
    if plan.host == Host::Pi {
        let binary = pi::binary_status();
        println!("package: {PI_PACKAGE_SOURCE}");
        println!("settings key: packages");
        println!("mcp server: provided by the Pi extension package");
        println!(
            "binary: {}",
            if binary.ready {
                binary.detail
            } else {
                format!("{}; {}", binary.detail, binary.next_action)
            }
        );
    } else {
        println!("server: {SERVER_NAME}");
        println!("command: sh -lc '{}'", SERVER_LAUNCHER);
    }
    if plan.init_project {
        println!("project init: requested");
        println!("project root: {}", plan.project_root.display());
        println!(
            "project state: {}",
            if plan.project_already_initialized {
                "already initialized"
            } else {
                "init required"
            }
        );
    } else {
        println!("project init: not requested");
        println!(
            "next: run with --init-project to create AGENTS.md, WORKFLOW.md, docs/, and backlog/"
        );
    }
}

fn project_scaffold_ready(root: &std::path::Path) -> bool {
    [
        "backlog",
        "backlog/items",
        "backlog/plans",
        "backlog/epics",
        "backlog/templates",
        "docs",
    ]
    .iter()
    .all(|relative| root.join(relative).is_dir())
        && [
            "AGENTS.md",
            "CLAUDE.md",
            "WORKFLOW.md",
            "platy.yaml",
            "backlog/README.md",
            "backlog/epics/general.md",
            "backlog/templates/item.md",
            "backlog/templates/plan.yaml",
            "docs/product.md",
            "docs/architecture.md",
            "docs/testing.md",
            "docs/engineering.md",
        ]
        .iter()
        .all(|relative| root.join(relative).is_file())
}
