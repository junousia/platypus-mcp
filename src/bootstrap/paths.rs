use super::types::{Host, Scope};
use anyhow::{anyhow, bail, Result};
use std::{
    env,
    path::{Path, PathBuf},
};

pub(super) fn default_config_path(host: Host, scope: Scope, root: &Path) -> Result<PathBuf> {
    match (host, scope) {
        (Host::Codex, Scope::Project) => Ok(root.join(".codex/config.toml")),
        (Host::Codex, Scope::Global) => {
            if let Ok(codex_home) = env::var("CODEX_HOME") {
                Ok(PathBuf::from(codex_home).join("config.toml"))
            } else {
                Ok(home_dir()?.join(".codex/config.toml"))
            }
        }
        (Host::Claude, Scope::Project) => Ok(root.join(".mcp.json")),
        (Host::Claude, Scope::Global) => {
            bail!("Claude Code global bootstrap writes project-scoped entries inside ~/.claude.json; use `--project` or pass `--config <path>` for an explicit JSON config")
        }
        (Host::Opencode, Scope::Project) => Ok(root.join("opencode.json")),
        (Host::Opencode, Scope::Global) => Ok(home_dir()?.join(".config/opencode/opencode.json")),
        (Host::Pi, Scope::Project) => Ok(root.join(".pi/settings.json")),
        (Host::Pi, Scope::Global) => Ok(home_dir()?.join(".pi/agent/settings.json")),
    }
}

fn home_dir() -> Result<PathBuf> {
    env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| anyhow!("HOME is not set; pass --config <path>"))
}
