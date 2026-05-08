mod codex;
mod json_hosts;
mod paths;
#[cfg(test)]
mod tests;
mod types;

use anyhow::{Context, Result};
use std::{env, fs, path::PathBuf};

use types::{BootstrapInvocation, BootstrapMode, Host, ServerConfig, SERVER_LAUNCHER, SERVER_NAME};

pub fn run_cli(args: &[String]) -> Result<i32> {
    let invocation = BootstrapInvocation::parse(args)?;
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
                if plan.already_configured {
                    "configured"
                } else {
                    "missing"
                },
                &plan,
            );
            Ok(i32::from(!plan.already_configured))
        }
        BootstrapMode::Apply => {
            plan.apply()?;
            print_plan("completed", &plan);
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
    force: bool,
}

impl BootstrapPlan {
    fn for_invocation(invocation: &BootstrapInvocation) -> Result<Self> {
        let host = invocation.host.expect("host is required");
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
            Host::Claude | Host::Pi => {
                json_hosts::shared_mcp_config(&existing, &server, invocation.force)?
            }
            Host::Opencode => json_hosts::opencode_config(&existing, &server, invocation.force)?,
        };
        let already_configured = existed
            && match host {
                Host::Codex => codex::is_configured(&existing, &server),
                Host::Claude | Host::Pi => json_hosts::shared_mcp_is_configured(&existing, &server),
                Host::Opencode => json_hosts::opencode_is_configured(&existing, &server),
            };

        Ok(Self {
            host,
            path,
            rendered,
            existed,
            already_configured,
            force: invocation.force,
        })
    }

    fn apply(&self) -> Result<()> {
        if self.already_configured {
            return Ok(());
        }
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
        }
        fs::write(&self.path, &self.rendered)
            .with_context(|| format!("write {}", self.path.display()))?;
        Ok(())
    }
}

fn print_hosts() {
    println!("Supported bootstrap hosts:");
    println!("  codex      Codex CLI TOML config");
    println!("  claude     Claude Code project .mcp.json config");
    println!("  opencode   OpenCode JSON config");
    println!("  pi         Pi MCP adapter shared .mcp.json config");
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
    println!("server: {SERVER_NAME}");
    println!("command: sh -lc '{}'", SERVER_LAUNCHER);
}
