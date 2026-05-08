use super::types::{ServerConfig, SERVER_NAME};
use anyhow::{anyhow, bail, Context, Result};
use serde_json::{json, Map, Value};

pub(super) fn shared_mcp_is_configured(existing: &str, server: &ServerConfig) -> bool {
    let Ok(root) = parse_or_empty_object(existing) else {
        return false;
    };
    let Some(config) = root
        .get("mcpServers")
        .and_then(Value::as_object)
        .and_then(|servers| servers.get(SERVER_NAME))
    else {
        return false;
    };
    json_server_matches(config, server)
}

pub(super) fn opencode_is_configured(existing: &str, server: &ServerConfig) -> bool {
    let Ok(root) = parse_or_empty_object(existing) else {
        return false;
    };
    let Some(config) = root
        .get("mcp")
        .and_then(Value::as_object)
        .and_then(|servers| servers.get(SERVER_NAME))
    else {
        return false;
    };
    config
        .get("type")
        .and_then(Value::as_str)
        .map(|value| value == "local")
        .unwrap_or(false)
        && config
            .get("enabled")
            .and_then(Value::as_bool)
            .unwrap_or(true)
        && config
            .get("command")
            .and_then(Value::as_array)
            .map(|values| values.iter().any(|value| value == "sh"))
            .unwrap_or(false)
        && config.to_string().contains("platypus-mcp")
        && root_matches(config.get("environment"), server)
}

pub(super) fn shared_mcp_config(
    existing: &str,
    server: &ServerConfig,
    force: bool,
) -> Result<String> {
    let mut root = parse_or_empty_object(existing)?;
    let servers = ensure_object_field(&mut root, "mcpServers")?;
    if let Some(existing_server) = servers.get(SERVER_NAME) {
        ensure_replace_allowed(existing_server, force)?;
    }

    let mut entry = Map::new();
    entry.insert("command".to_string(), server.command());
    entry.insert("args".to_string(), server.args());
    if let Some(env) = server.env_map() {
        entry.insert("env".to_string(), env);
    }
    servers.insert(SERVER_NAME.to_string(), Value::Object(entry));
    render_json_object(root)
}

pub(super) fn opencode_config(
    existing: &str,
    server: &ServerConfig,
    force: bool,
) -> Result<String> {
    let mut root = parse_or_empty_object(existing)?;
    root.entry("$schema".to_string())
        .or_insert_with(|| json!("https://opencode.ai/config.json"));
    let servers = ensure_object_field(&mut root, "mcp")?;
    if let Some(existing_server) = servers.get(SERVER_NAME) {
        ensure_replace_allowed(existing_server, force)?;
    }

    let mut entry = Map::new();
    entry.insert("type".to_string(), json!("local"));
    entry.insert("command".to_string(), server.command_array());
    entry.insert("enabled".to_string(), json!(true));
    if let Some(env) = server.env_map() {
        entry.insert("environment".to_string(), env);
    }
    servers.insert(SERVER_NAME.to_string(), Value::Object(entry));
    render_json_object(root)
}

fn parse_or_empty_object(existing: &str) -> Result<Map<String, Value>> {
    if existing.trim().is_empty() {
        return Ok(Map::new());
    }
    match serde_json::from_str::<Value>(existing)
        .with_context(|| "existing config must be valid JSON without comments")?
    {
        Value::Object(map) => Ok(map),
        _ => bail!("existing config must be a JSON object"),
    }
}

fn ensure_object_field<'a>(
    root: &'a mut Map<String, Value>,
    field: &str,
) -> Result<&'a mut Map<String, Value>> {
    root.entry(field.to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    root.get_mut(field)
        .and_then(Value::as_object_mut)
        .ok_or_else(|| anyhow!("config field `{field}` must be an object"))
}

fn ensure_replace_allowed(current: &Value, force: bool) -> Result<()> {
    if force
        || current.to_string().contains("platypus-mcp")
        || current.to_string().contains("cargo")
    {
        return Ok(());
    }
    bail!("existing platypus server config does not look like Platypus MCP; pass --force to replace it")
}

fn json_server_matches(config: &Value, server: &ServerConfig) -> bool {
    config
        .get("command")
        .and_then(Value::as_str)
        .map(|value| value == "sh")
        .unwrap_or(false)
        && config.to_string().contains("platypus-mcp")
        && root_matches(config.get("env"), server)
}

fn root_matches(env: Option<&Value>, server: &ServerConfig) -> bool {
    server
        .root
        .as_deref()
        .map(|root| {
            env.and_then(Value::as_object)
                .and_then(|env| env.get("PLATYPUS_MCP_ROOT"))
                .and_then(Value::as_str)
                == Some(root)
        })
        .unwrap_or(true)
}

fn render_json_object(root: Map<String, Value>) -> Result<String> {
    Ok(format!(
        "{}\n",
        serde_json::to_string_pretty(&Value::Object(root))?
    ))
}
