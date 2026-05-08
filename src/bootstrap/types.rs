use anyhow::{anyhow, bail, Result};
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Host {
    Codex,
    Claude,
    Opencode,
    Pi,
}

impl Host {
    pub(super) fn parse(value: &str) -> Result<Self> {
        match value {
            "codex" => Ok(Self::Codex),
            "claude" | "claude-code" => Ok(Self::Claude),
            "opencode" => Ok(Self::Opencode),
            "pi" => Ok(Self::Pi),
            other => bail!("unknown host `{other}`; use one of: codex, claude, opencode, pi"),
        }
    }

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
    pub(super) host: Option<Host>,
    pub(super) scope: Scope,
    pub(super) config: Option<PathBuf>,
    pub(super) root: Option<PathBuf>,
    pub(super) mode: BootstrapMode,
    pub(super) force: bool,
    pub(super) list_hosts: bool,
}

impl BootstrapInvocation {
    pub(super) fn parse(args: &[String]) -> Result<Self> {
        let mut host = None;
        let mut scope = Scope::Project;
        let mut config = None;
        let mut root = None;
        let mut mode = BootstrapMode::Apply;
        let mut force = false;
        let mut list_hosts = false;

        let mut index = 0;
        while index < args.len() {
            let arg = &args[index];
            match arg.as_str() {
                "hosts" | "list" => list_hosts = true,
                "--project" => scope = Scope::Project,
                "--global" | "--user" => scope = Scope::Global,
                "--dry-run" => mode = BootstrapMode::DryRun,
                "--check" => mode = BootstrapMode::Check,
                "--force" => force = true,
                "--config" => {
                    index += 1;
                    config = Some(value_path(args.get(index), "--config")?);
                }
                "--root" | "--project-root" => {
                    index += 1;
                    root = Some(value_path(args.get(index), arg)?);
                }
                _ if arg.starts_with("--config=") => {
                    config = Some(PathBuf::from(non_empty_value(arg, "--config=")?));
                }
                _ if arg.starts_with("--root=") => {
                    root = Some(PathBuf::from(non_empty_value(arg, "--root=")?));
                }
                _ if arg.starts_with("--project-root=") => {
                    root = Some(PathBuf::from(non_empty_value(arg, "--project-root=")?));
                }
                _ if arg.starts_with("--scope=") => {
                    scope = parse_scope(non_empty_value(arg, "--scope=")?)?;
                }
                _ if arg.starts_with('-') => bail!("unknown bootstrap option `{arg}`"),
                _ => {
                    if host.is_some() {
                        bail!("{}", usage());
                    }
                    host = Some(Host::parse(arg)?);
                }
            }
            index += 1;
        }

        if !list_hosts && host.is_none() {
            bail!("{}", usage());
        }

        Ok(Self {
            host,
            scope,
            config,
            root,
            mode,
            force,
            list_hosts,
        })
    }
}

fn usage() -> &'static str {
    "usage: platypus-mcp bootstrap <host> [--project|--global] [--config <path>] [--root <path>] [--dry-run|--check] [--force]"
}

fn value_path(value: Option<&String>, flag: &str) -> Result<PathBuf> {
    value
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| anyhow!("{flag} requires a value"))
}

fn non_empty_value<'a>(arg: &'a str, prefix: &str) -> Result<&'a str> {
    let value = arg.trim_start_matches(prefix);
    if value.is_empty() {
        bail!("{prefix} requires a value");
    }
    Ok(value)
}

fn parse_scope(value: &str) -> Result<Scope> {
    match value {
        "project" => Ok(Scope::Project),
        "global" | "user" => Ok(Scope::Global),
        other => bail!("unknown scope `{other}`; use project or global"),
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
