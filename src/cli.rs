use anyhow::{anyhow, bail, Context, Result};
use clap::{Args, Parser};
use rmcp::{
    model::{CallToolRequestParams, JsonObject},
    transport::TokioChildProcess,
    ServiceExt,
};
use serde_json::Value;
use tokio::process::Command;

#[derive(Debug, Clone, Args)]
pub struct ToolCli {
    /// Project root to bind the MCP server to.
    #[arg(long)]
    pub root: Option<String>,
    /// MCP tool name to invoke.
    pub name: String,
    /// JSON object passed as tool arguments.
    #[arg(default_value = "{}")]
    pub arguments: String,
}

pub async fn run_tool_cli(args: &[String]) -> Result<i32> {
    let cli = ToolCliParser::try_parse_from(
        std::iter::once("tool").chain(args.iter().map(String::as_str)),
    )?;
    run_tool_command(cli.command).await
}

pub async fn run_tool_command(command: ToolCli) -> Result<i32> {
    let mut arguments = parse_json_object(&command.arguments)?;
    if let Some(root) = command.root.as_deref() {
        arguments
            .entry("root".to_string())
            .or_insert_with(|| Value::String(root.to_string()));
    }
    run_invocation(ToolInvocation {
        name: command.name,
        arguments,
        root: command.root,
    })
    .await
}

async fn run_invocation(invocation: ToolInvocation) -> Result<i32> {
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

#[derive(Debug, Parser)]
#[command(name = "tool")]
struct ToolCliParser {
    #[command(flatten)]
    command: ToolCli,
}

fn parse_json_object(value: &str) -> Result<JsonObject> {
    let parsed: Value =
        serde_json::from_str(value).with_context(|| "tool arguments must be valid JSON")?;
    parsed
        .as_object()
        .cloned()
        .ok_or_else(|| anyhow!("tool arguments must be a JSON object"))
}
