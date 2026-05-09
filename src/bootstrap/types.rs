use anyhow::Result;
use clap::{Args, Parser, Subcommand, ValueEnum};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

pub(super) const SERVER_NAME: &str = "platypus";
pub(super) const SERVER_LAUNCHER: &str = "exec \"$HOME/.cargo/bin/platypus-mcp\"";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BootstrapMode {
    Apply,
    DryRun,
    Check,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Scope {
    Project,
    Global,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(super) enum Host {
    Codex,
    Claude,
    Opencode,
    Pi,
}

impl Host {
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Codex => "Codex",
            Self::Claude => "Claude Code",
            Self::Opencode => "OpenCode",
            Self::Pi => "Pi",
        }
    }
}

#[derive(Debug, Clone)]
pub(super) struct BootstrapInvocation {
    pub(super) host: Host,
    pub(super) scope: Scope,
    pub(super) config: Option<PathBuf>,
    pub(super) root: Option<PathBuf>,
    pub(super) project_name: Option<String>,
    pub(super) mode: BootstrapMode,
    pub(super) force: bool,
    pub(super) init_project: bool,
    pub(super) list_hosts: bool,
}

impl BootstrapInvocation {
    pub(super) fn parse(args: &[String]) -> Result<Self> {
        let cli = BootstrapCli::try_parse_from(
            std::iter::once("bootstrap").chain(args.iter().map(String::as_str)),
        )?;
        Ok(invocation_from_command(cli.command))
    }
}

#[derive(Debug, Parser)]
#[command(
    name = "bootstrap",
    about = "Configure an MCP host to launch Platypus MCP"
)]
struct BootstrapCli {
    #[command(subcommand)]
    command: BootstrapCommand,
}

#[derive(Debug, Subcommand)]
pub enum BootstrapCommand {
    /// List supported MCP hosts.
    #[command(alias = "list")]
    Hosts,
    /// Configure Codex CLI TOML MCP config.
    Codex(HostOptions),
    /// Configure Claude Code project MCP config.
    #[command(alias = "claude-code")]
    Claude(HostOptions),
    /// Configure OpenCode JSON MCP config.
    Opencode(HostOptions),
    /// Configure Pi shared MCP config.
    Pi(HostOptions),
}

#[derive(Debug, Clone, Args)]
pub struct HostOptions {
    /// Write project-local host configuration.
    #[arg(long, conflicts_with = "global")]
    project: bool,
    /// Write user/global host configuration where supported.
    #[arg(long, alias = "user", conflicts_with = "project")]
    global: bool,
    /// Explicit config file path to write or inspect.
    #[arg(long)]
    config: Option<PathBuf>,
    /// Bind Platypus MCP tools to this project root.
    #[arg(long, alias = "project-root")]
    root: Option<PathBuf>,
    /// Also initialize Platypus project files such as AGENTS.md and backlog/.
    #[arg(long)]
    init_project: bool,
    /// Project name to use when --init-project creates platy.yaml.
    #[arg(long, requires = "init_project")]
    project_name: Option<String>,
    /// Print the planned configuration without writing it.
    #[arg(long, conflicts_with = "check")]
    dry_run: bool,
    /// Return success only when the host is already configured.
    #[arg(long, conflicts_with = "dry_run")]
    check: bool,
    /// Replace an existing platypus server entry even if it is not recognized.
    #[arg(long)]
    force: bool,
}

impl HostOptions {
    pub(super) fn into_invocation(self, host: Host) -> BootstrapInvocation {
        let scope = if self.global {
            Scope::Global
        } else {
            Scope::Project
        };
        let mode = if self.check {
            BootstrapMode::Check
        } else if self.dry_run {
            BootstrapMode::DryRun
        } else {
            BootstrapMode::Apply
        };
        BootstrapInvocation {
            host,
            scope,
            config: self.config,
            root: self.root,
            project_name: self.project_name,
            mode,
            force: self.force,
            init_project: self.init_project,
            list_hosts: false,
        }
    }
}

pub(super) fn invocation_from_command(command: BootstrapCommand) -> BootstrapInvocation {
    match command {
        BootstrapCommand::Hosts => BootstrapInvocation {
            host: Host::Codex,
            scope: Scope::Project,
            config: None,
            root: None,
            project_name: None,
            mode: BootstrapMode::Apply,
            force: false,
            init_project: false,
            list_hosts: true,
        },
        BootstrapCommand::Codex(options) => options.into_invocation(Host::Codex),
        BootstrapCommand::Claude(options) => options.into_invocation(Host::Claude),
        BootstrapCommand::Opencode(options) => options.into_invocation(Host::Opencode),
        BootstrapCommand::Pi(options) => options.into_invocation(Host::Pi),
    }
}

#[derive(Debug, Clone)]
pub(super) struct ServerConfig {
    pub(super) root: Option<String>,
}

impl ServerConfig {
    pub(super) fn new(root: Option<&Path>) -> Self {
        Self {
            root: root.map(|path| path.to_string_lossy().to_string()),
        }
    }

    pub(super) fn command(&self) -> Value {
        json!("sh")
    }

    pub(super) fn args(&self) -> Value {
        json!(["-lc", SERVER_LAUNCHER])
    }

    pub(super) fn command_array(&self) -> Value {
        json!(["sh", "-lc", SERVER_LAUNCHER])
    }

    pub(super) fn env_map(&self) -> Option<Value> {
        self.root
            .as_ref()
            .map(|root| json!({ "PLATYPUS_MCP_ROOT": root }))
    }
}
