use anyhow::{anyhow, bail, Context, Result};
use serde_json::{json, Map, Value};
use std::{
    env,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct BinaryStatus {
    pub(super) ready: bool,
    pub(super) detail: String,
    pub(super) next_action: String,
}

pub(super) fn binary_status() -> BinaryStatus {
    if let Ok(path) = env::var("PLATYPUS_MCP_BIN") {
        let path = PathBuf::from(path);
        if path.is_file() {
            return BinaryStatus {
                ready: true,
                detail: format!("PLATYPUS_MCP_BIN resolves to {}", path.display()),
                next_action: "none".to_string(),
            };
        }
        return BinaryStatus {
            ready: false,
            detail: format!("PLATYPUS_MCP_BIN points to missing file {}", path.display()),
            next_action:
                "set PLATYPUS_MCP_BIN to a working platypus-mcp binary or install platypus-mcp"
                    .to_string(),
        };
    }

    if let Some(path) = cargo_home_binary() {
        return BinaryStatus {
            ready: true,
            detail: format!("found {}", path.display()),
            next_action: "none".to_string(),
        };
    }

    if let Some(path) = path_binary() {
        return BinaryStatus {
            ready: true,
            detail: format!("found {}", path.display()),
            next_action: "none".to_string(),
        };
    }

    BinaryStatus {
        ready: false,
        detail: "no platypus-mcp binary found in PLATYPUS_MCP_BIN, ~/.cargo/bin, or PATH"
            .to_string(),
        next_action: "install with `cargo install platypus-mcp`, install the Pi npm package with a bundled binary, or set PLATYPUS_MCP_BIN".to_string(),
    }
}

fn cargo_home_binary() -> Option<PathBuf> {
    let home = env::var_os("HOME")?;
    let path = PathBuf::from(home)
        .join(".cargo")
        .join("bin")
        .join(executable_name());
    path.is_file().then_some(path)
}

fn path_binary() -> Option<PathBuf> {
    let path_env = env::var_os("PATH")?;
    env::split_paths(&path_env)
        .map(|path| path.join(executable_name()))
        .find(|path| path.is_file())
}

fn executable_name() -> &'static str {
    if cfg!(windows) {
        "platypus-mcp.exe"
    } else {
        "platypus-mcp"
    }
}

pub(super) fn settings_is_configured(
    existing: &str,
    package_source: &str,
    settings_path: &Path,
) -> bool {
    let Ok(root) = parse_or_empty_object(existing) else {
        return false;
    };
    root.get("packages")
        .and_then(Value::as_array)
        .map(|packages| {
            packages
                .iter()
                .filter_map(package_source_value)
                .any(|source| package_matches(source, package_source, settings_path))
        })
        .unwrap_or(false)
}

pub(super) fn settings_config(
    existing: &str,
    package_source: &str,
    settings_path: &Path,
    force: bool,
) -> Result<String> {
    let mut root = parse_or_empty_object(existing)?;
    let packages = ensure_packages_array(&mut root)?;

    let mut retained = Vec::new();
    let mut found = false;
    for package in packages.drain(..) {
        let source = package_source_value(&package);
        if source
            .map(|source| package_matches(source, package_source, settings_path))
            .unwrap_or(false)
        {
            found = true;
            if !force {
                retained.push(package);
            }
        } else {
            retained.push(package);
        }
    }

    if force || !found {
        retained.push(json!(package_source));
    }

    *packages = retained;
    render_json_object(root)
}

fn parse_or_empty_object(existing: &str) -> Result<Map<String, Value>> {
    if existing.trim().is_empty() {
        return Ok(Map::new());
    }
    match serde_json::from_str::<Value>(existing)
        .with_context(|| "existing Pi settings must be valid JSON without comments")?
    {
        Value::Object(map) => Ok(map),
        _ => bail!("existing Pi settings must be a JSON object"),
    }
}

fn ensure_packages_array(root: &mut Map<String, Value>) -> Result<&mut Vec<Value>> {
    root.entry("packages".to_string())
        .or_insert_with(|| Value::Array(Vec::new()));
    root.get_mut("packages")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| anyhow!("Pi settings field `packages` must be an array"))
}

fn package_source_value(package: &Value) -> Option<&str> {
    package
        .as_str()
        .or_else(|| package.as_object()?.get("source")?.as_str())
}

fn package_matches(source: &str, package_source: &str, settings_path: &Path) -> bool {
    let source = source.trim();
    source == package_source
        || source == "platypus-pi"
        || source.starts_with("npm:platypus-pi")
        || source.contains("junousia/platypus-mcp")
        || source.ends_with("platypus-mcp")
        || local_package_matches(source, settings_path)
}

fn local_package_matches(source: &str, settings_path: &Path) -> bool {
    if looks_like_remote_source(source) {
        return false;
    }
    let source_path = Path::new(source);
    if !source_path.is_relative() && !source_path.is_absolute() {
        return false;
    }
    let package_root = if source_path.is_absolute() {
        PathBuf::from(source_path)
    } else {
        settings_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(source_path)
    };
    let package_json = package_root.join("package.json");
    let Ok(contents) = std::fs::read_to_string(package_json) else {
        return false;
    };
    let Ok(json) = serde_json::from_str::<Value>(&contents) else {
        return false;
    };
    json.get("name").and_then(Value::as_str) == Some("platypus-pi")
}

fn looks_like_remote_source(source: &str) -> bool {
    source.contains(':') || source.starts_with('@')
}

fn render_json_object(root: Map<String, Value>) -> Result<String> {
    Ok(format!(
        "{}\n",
        serde_json::to_string_pretty(&Value::Object(root))?
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bootstrap::types::PI_PACKAGE_SOURCE;
    use tempfile::TempDir;

    #[test]
    fn pi_settings_adds_package_without_losing_existing_settings() {
        let existing = serde_json::json!({
            "theme": "dark",
            "packages": ["pi-skills"]
        })
        .to_string();

        let settings_path = Path::new(".pi/settings.json");
        let rendered =
            settings_config(&existing, PI_PACKAGE_SOURCE, settings_path, false).expect("config");
        let json: Value = serde_json::from_str(&rendered).expect("json");

        assert_eq!(json["theme"], "dark");
        assert_eq!(json["packages"][0], "pi-skills");
        assert_eq!(json["packages"][1], PI_PACKAGE_SOURCE);
        assert!(settings_is_configured(
            &rendered,
            PI_PACKAGE_SOURCE,
            settings_path
        ));
    }

    #[test]
    fn pi_settings_recognizes_object_and_versioned_sources() {
        let object_source = serde_json::json!({
            "packages": [{
                "source": "npm:platypus-pi@0.2.0",
                "skills": []
            }]
        })
        .to_string();
        let settings_path = Path::new(".pi/settings.json");
        assert!(settings_is_configured(
            &object_source,
            PI_PACKAGE_SOURCE,
            settings_path
        ));

        let git_source = serde_json::json!({
            "packages": ["git:github.com/junousia/platypus-mcp"]
        })
        .to_string();
        assert!(settings_is_configured(
            &git_source,
            PI_PACKAGE_SOURCE,
            settings_path
        ));
    }

    #[test]
    fn pi_settings_force_replaces_existing_platypus_source() {
        let existing = serde_json::json!({
            "packages": ["pi-skills", "../platypus-mcp"]
        })
        .to_string();

        let rendered = settings_config(
            &existing,
            PI_PACKAGE_SOURCE,
            Path::new(".pi/settings.json"),
            true,
        )
        .expect("config");
        let json: Value = serde_json::from_str(&rendered).expect("json");

        assert_eq!(json["packages"].as_array().expect("packages").len(), 2);
        assert_eq!(json["packages"][0], "pi-skills");
        assert_eq!(json["packages"][1], PI_PACKAGE_SOURCE);
    }

    #[test]
    fn pi_settings_rejects_non_array_packages() {
        let existing = serde_json::json!({
            "packages": "platypus-pi"
        })
        .to_string();

        let error = settings_config(
            &existing,
            PI_PACKAGE_SOURCE,
            Path::new(".pi/settings.json"),
            false,
        )
        .expect_err("error");
        assert!(error.to_string().contains("packages"));
    }

    #[test]
    fn pi_settings_recognizes_local_package_only_when_package_name_matches() {
        let temp = TempDir::new().expect("temp dir");
        let settings_dir = temp.path().join(".pi");
        std::fs::create_dir_all(&settings_dir).expect("settings dir");
        std::fs::write(
            temp.path().join("package.json"),
            serde_json::json!({"name": "platypus-pi"}).to_string(),
        )
        .expect("package json");
        let settings_path = settings_dir.join("settings.json");
        let existing = serde_json::json!({"packages": [".."]}).to_string();

        assert!(settings_is_configured(
            &existing,
            PI_PACKAGE_SOURCE,
            &settings_path
        ));

        std::fs::write(
            temp.path().join("package.json"),
            serde_json::json!({"name": "not-platypus"}).to_string(),
        )
        .expect("package json");
        assert!(!settings_is_configured(
            &existing,
            PI_PACKAGE_SOURCE,
            &settings_path
        ));
    }
}
