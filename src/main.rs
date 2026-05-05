use anyhow::Result;
use rmcp::{
    ServerHandler, ServiceExt,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    tool, tool_handler, tool_router, transport::stdio,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::PathBuf;

#[derive(Clone, Debug)]
struct PlatypusMcp {
    tool_router: ToolRouter<Self>,
}

impl PlatypusMcp {
    fn new() -> Self {
        Self {
            tool_router: Self::tool_router(),
        }
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
struct PingParams {
    message: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct ProjectStatusParams {
    root: Option<String>,
}

#[derive(Debug, Serialize)]
struct ProjectStatus {
    root: String,
    platy_yaml: bool,
    backlog_dir: bool,
    git_dir: bool,
}

#[tool_handler]
impl ServerHandler for PlatypusMcp {}

#[tool_router(router = tool_router)]
impl PlatypusMcp {
    #[tool(description = "Health check for the Platypus MCP server.")]
    async fn ping(
        &self,
        Parameters(PingParams { message }): Parameters<PingParams>,
    ) -> String {
        let echoed = message.unwrap_or_else(|| "pong".to_string());
        json!({
            "status": "ok",
            "echo": echoed,
        })
        .to_string()
    }

    #[tool(description = "Inspect whether a directory looks like a Platypus project.")]
    async fn project_status(
        &self,
        Parameters(ProjectStatusParams { root }): Parameters<ProjectStatusParams>,
    ) -> String {
        let root_path = root
            .map(PathBuf::from)
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
        let status = ProjectStatus {
            root: root_path.display().to_string(),
            platy_yaml: root_path.join("platy.yaml").is_file(),
            backlog_dir: root_path.join("backlog").is_dir(),
            git_dir: root_path.join(".git").is_dir(),
        };
        serde_json::to_string(&status).unwrap_or_else(|_| {
            json!({
                "status": "error",
                "summary": "failed to serialize project status"
            })
            .to_string()
        })
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let server = PlatypusMcp::new().serve(stdio()).await?;
    server.waiting().await?;
    Ok(())
}
