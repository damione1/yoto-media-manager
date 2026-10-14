use rmcp::{
    ErrorData as McpError, RoleServer, ServerHandler, ServiceExt, model::*, service::RequestContext,
};
use std::sync::Arc;
use yoto_domain::*;
use yoto_infra::{
    Command,
    bridge::{self, Request},
    media::Tools,
};
#[derive(Clone)]
struct Server {
    session: Arc<yoto_infra::Session>,
}
impl Server {
    async fn dispatch(&self, request: Request) -> Result<serde_json::Value> {
        match request {
            Request::Execute(command) => self.session.execute(command).await.and_then(|r| {
                serde_json::to_value(r)
                    .map(bridge::sanitize)
                    .map_err(|e| Failure::Storage(e.to_string()))
            }),
            Request::Health => self
                .session
                .revision()
                .await
                .map(|revision| serde_json::json!({"revision":revision})),
            Request::SnapshotChunk { .. } | Request::InterfaceChunk { .. } => {
                Err(Failure::Validation("Internal IPC chunk request".into()))
            }
            Request::Snapshot => self.session.snapshot().await.and_then(|s| {
                serde_json::to_value(s)
                    .map(bridge::sanitize)
                    .map_err(|e| Failure::Storage(e.to_string()))
            }),
        }
    }
}
#[derive(serde::Deserialize, schemars::JsonSchema)]
struct Execute {
    command: Command,
}
fn tools() -> Vec<Tool> {
    let execute = serde_json::to_value(schemars::schema_for!(Execute))
        .expect("schema")
        .as_object()
        .expect("object")
        .clone();
    vec![
        Tool::new(
            "yoto_execute",
            "Execute any typed Yoto action. The command union contains every service action: local imports, recipes, playlists, OAuth, sync, publication, jobs, players, settings and application update check/state/download/cancel. Installation and restart require the desktop app. Network mutations and physical commands have real effects; invoke only for the user's requested action.",
            Arc::new(execute),
        ),
        Tool::new(
            "yoto_snapshot",
            "Read the catalogue, recipes, durable jobs, active account, remote library, players, media associations and publication bindings. No OAuth credentials or signed upload URLs are returned.",
            Arc::new(
                serde_json::json!({"type":"object","properties":{},"additionalProperties":false})
                    .as_object()
                    .expect("schema")
                    .clone(),
            ),
        ),
    ]
}
impl ServerHandler for Server {
    fn get_info(&self) -> ServerConfig {
        let mut config = ServerConfig::default();
        config.capabilities = ServerCapabilities::builder().enable_tools().build();
        config.instructions=Some("Local Yoto catalogue. Read the snapshot before acting. Every execute command has real effects.".into());
        config
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> std::result::Result<ListToolsResult, McpError> {
        Ok(ListToolsResult {
            tools: tools(),
            ..Default::default()
        })
    }
    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> std::result::Result<CallToolResponse, McpError> {
        let result = match request.name.as_ref() {
            "yoto_snapshot" => self.dispatch(Request::Snapshot).await,
            "yoto_execute" => match serde_json::from_value::<Execute>(serde_json::Value::Object(
                request.arguments.unwrap_or_default(),
            )) {
                Ok(args) => self.dispatch(Request::Execute(args.command)).await,
                Err(e) => Err(Failure::Validation(e.to_string())),
            },
            _ => return Err(McpError::invalid_params("Unknown tool", None)),
        };
        Ok(match result {
            Ok(value) => CallToolResult::success(vec![ContentBlock::text(
                serde_json::to_string(&value).expect("JSON"),
            )])
            .into(),
            Err(e) => CallToolResult::error(vec![ContentBlock::text(e.to_string())]).into(),
        })
    }
}
#[tokio::main]
async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    if std::env::args().any(|a| a == "--self-test") {
        yoto_infra::diagnostics::smoke(Tools::bundled()?).await?;
        eprintln!("Offline bundle self-test passed");
        return Ok(());
    }
    let profile = yoto_infra::default_profile()?;
    let isolated = cfg!(debug_assertions) && std::env::args().any(|a| a == "--isolated");
    // Probe IPC first. A running UI retains ownership; proxies do not obtain a second DB connection.
    let session =
        yoto_infra::Session::connect(profile.clone(), Tools::bundled()?, isolated).await?;
    session.start_tool_updates(!cfg!(debug_assertions));
    let server = Server {
        session: session.clone(),
    }
    .serve(rmcp::transport::stdio())
    .await?;
    server.waiting().await?;
    session.shutdown();
    Ok(())
}
