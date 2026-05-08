use anyhow::{anyhow, bail, Context, Result};
use rmcp::{
    model::{CallToolRequestParams, JsonObject},
    transport::TokioChildProcess,
    ServiceExt,
};
use serde_json::Value;
use tokio::process::Command;

pub async fn run_tool_cli(args: &[String]) -> Result<i32> {
    let invocation = ToolInvocation::parse(args)?;
    let mut command = Command::new(std::env::current_exe().context("current executable")?);
    if let Some(root) = invocation.root.as_deref() {
        command.env("PLATYPUS_MCP_ROOT", root);
    }

    let transport = TokioChildProcess::new(command)?;
    let client = ().serve(transport).await?;
    let result = client
        .call_tool(CallToolRequestParams {
            meta: None,
            name: invocation.name.into(),
            arguments: Some(invocation.arguments),
            task: None,
        })
        .await;

    let result = match result {
        Ok(result) => result,
        Err(error) => {
            let _ = client.cancel().await;
            return Err(error.into());
        }
    };
    let Some(structured) = result.structured_content else {
        let _ = client.cancel().await;
        bail!("tool response did not include structured content");
    };
    println!("{}", serde_json::to_string_pretty(&structured)?);
    let _ = client.cancel().await;

    if structured.get("status").and_then(Value::as_str) == Some("failed") {
        Ok(1)
    } else {
        Ok(0)
    }
}

#[derive(Debug)]
struct ToolInvocation {
    name: String,
    arguments: JsonObject,
    root: Option<String>,
}

impl ToolInvocation {
    fn parse(args: &[String]) -> Result<Self> {
        let mut root = None;
        let mut positional = Vec::new();
        let mut index = 0;
        while index < args.len() {
            let arg = &args[index];
            if arg == "--root" {
                index += 1;
                root = Some(
                    args.get(index)
                        .ok_or_else(|| anyhow!("--root requires a value"))?
                        .to_string(),
                );
            } else if let Some(value) = arg.strip_prefix("--root=") {
                if value.is_empty() {
                    bail!("--root requires a value");
                }
                root = Some(value.to_string());
            } else {
                positional.push(arg.to_string());
            }
            index += 1;
        }

        if positional.is_empty() {
            bail!("usage: platypus-mcp-rs tool [--root <path>] <tool-name> [json-object]");
        }
        if positional.len() > 2 {
            bail!("usage: platypus-mcp-rs tool [--root <path>] <tool-name> [json-object]");
        }

        let name = positional[0].clone();
        let argument_text = positional.get(1).map(String::as_str).unwrap_or("{}");
        let mut arguments = parse_json_object(argument_text)?;
        if let Some(root) = root.as_deref() {
            arguments
                .entry("root".to_string())
                .or_insert_with(|| Value::String(root.to_string()));
        }
        Ok(Self {
            name,
            arguments,
            root,
        })
    }
}

fn parse_json_object(value: &str) -> Result<JsonObject> {
    let parsed: Value =
        serde_json::from_str(value).with_context(|| "tool arguments must be valid JSON")?;
    parsed
        .as_object()
        .cloned()
        .ok_or_else(|| anyhow!("tool arguments must be a JSON object"))
}
