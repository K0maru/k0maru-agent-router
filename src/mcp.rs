//! Model Context Protocol (MCP) JSON-RPC 2.0 stdio server implementation.
//!
//! Exposes k0maru-agent-router capabilities to AI agents (such as Antigravity, Claude Code, Cursor)
//! conforming to the standard MCP specification over stdio.

use crate::coordinator::{RouteOutcome, RouterCoordinator};
use crate::driver::MultiplexerDriver;
use crate::envelope::{TaskEnvelope, DEFAULT_SCHEMA};
use serde::{Deserialize, Serialize};
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

/// JSON-RPC 2.0 Request envelope.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    #[serde(default)]
    pub id: Option<serde_json::Value>,
    pub method: String,
    #[serde(default)]
    pub params: Option<serde_json::Value>,
}

/// JSON-RPC 2.0 Response envelope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub id: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

impl JsonRpcResponse {
    /// Constructs a successful JSON-RPC response.
    pub fn success(id: serde_json::Value, result: serde_json::Value) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: Some(result),
            error: None,
        }
    }

    /// Constructs an error JSON-RPC response.
    pub fn error(id: serde_json::Value, code: i32, message: impl Into<String>) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: None,
            error: Some(JsonRpcError {
                code,
                message: message.into(),
                data: None,
            }),
        }
    }
}

/// JSON-RPC 2.0 Error payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

/// Single content item in an MCP tool call response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpToolContent {
    #[serde(rename = "type")]
    pub content_type: String,
    pub text: String,
}

/// Outcome of executing an MCP tool.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpToolCallResult {
    pub content: Vec<McpToolContent>,
    #[serde(rename = "isError")]
    pub is_error: bool,
}

impl McpToolCallResult {
    /// Creates a successful tool execution result with formatted JSON content.
    pub fn success(value: serde_json::Value) -> Self {
        let text = serde_json::to_string_pretty(&value).unwrap_or_else(|_| value.to_string());
        Self {
            content: vec![McpToolContent {
                content_type: "text".to_string(),
                text,
            }],
            is_error: false,
        }
    }

    /// Creates an error tool execution result.
    pub fn error(message: impl Into<String>) -> Self {
        Self {
            content: vec![McpToolContent {
                content_type: "text".to_string(),
                text: message.into(),
            }],
            is_error: true,
        }
    }
}

/// Model Context Protocol stdio server wrapping `RouterCoordinator`.
#[derive(Debug, Clone)]
pub struct McpServer<D: MultiplexerDriver + Clone> {
    coordinator: RouterCoordinator<D>,
}

impl<D: MultiplexerDriver + Clone> McpServer<D> {
    /// Creates a new `McpServer` backed by the specified coordinator.
    pub fn new(coordinator: RouterCoordinator<D>) -> Self {
        Self { coordinator }
    }

    /// Returns a reference to the underlying coordinator.
    pub fn coordinator(&self) -> &RouterCoordinator<D> {
        &self.coordinator
    }

    /// Returns the typed JSON schemas for all exposed MCP tools.
    pub fn tools_list() -> Vec<serde_json::Value> {
        vec![
            serde_json::json!({
                "name": "route_task",
                "description": "Ingests a TaskEnvelope JSON, evaluates boundary policy, and either stages it in the HITL outbox (external target) or dispatches it to a live multiplexer pane (internal target).",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "envelope": {
                            "type": "object",
                            "description": "Full TaskEnvelope specification conforming to agent-task-v1",
                            "properties": {
                                "id": { "type": "string", "description": "Unique task identifier (e.g. task_9f82a1b4)" },
                                "created_at": { "type": "string", "description": "Creation timestamp in RFC-3339 format" },
                                "caller": {
                                    "type": "object",
                                    "description": "Caller workspace and agent provenance",
                                    "properties": {
                                        "workspace": { "type": "string", "description": "Absolute workspace path of the caller" },
                                        "agent": { "type": "string", "description": "Name or model ID of the calling agent" },
                                        "pane_id": { "type": "string", "description": "Optional multiplexer pane ID" }
                                    },
                                    "required": ["workspace", "agent"]
                                },
                                "target": {
                                    "type": "object",
                                    "description": "Target repository and boundary definition",
                                    "properties": {
                                        "identifier": { "type": "string", "description": "Target repository or service identifier" },
                                        "workspace": { "type": "string", "description": "Target local workspace path (required if internal)" },
                                        "is_external": { "type": "boolean", "description": "True if external/untrusted OSS dependency, false if internal" }
                                    },
                                    "required": ["identifier", "is_external"]
                                },
                                "intent": { "type": "string", "description": "High-level goal or task type (e.g. fix_bug, add_feature)" },
                                "payload": {
                                    "type": "object",
                                    "description": "Detailed task description and reproduction evidence",
                                    "properties": {
                                        "title": { "type": "string", "description": "Short descriptive summary" },
                                        "severity": {
                                            "type": "string",
                                            "enum": ["blocking", "critical", "high", "medium", "low", "normal"],
                                            "description": "Issue priority level"
                                        },
                                        "reproduction": {
                                            "type": "object",
                                            "description": "Reproduction details with snippet and error",
                                            "properties": {
                                                "snippet": { "type": "string", "description": "Code snippet triggering the issue" },
                                                "error": { "type": "string", "description": "Observed error message or stack trace" }
                                            }
                                        },
                                        "expected": { "type": "string", "description": "Expected correct behavior" }
                                    },
                                    "required": ["title", "severity"]
                                }
                            },
                            "required": ["caller", "target", "intent", "payload"]
                        },
                        "dry_run": {
                            "type": "boolean",
                            "description": "If true, simulates routing policy and physical addressing without mutating state or staging files"
                        },
                        "outbox_dir": {
                            "type": "string",
                            "description": "Optional path to the staged outbox directory (defaults to .scratch/staged)"
                        },
                        "candidates": {
                            "type": "array",
                            "description": "Optional candidate targets for decision engine resolution when target is ambiguous",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "identifier": { "type": "string", "description": "Target identifier" },
                                    "description": { "type": "string", "description": "Semantic role or description" }
                                },
                                "required": ["identifier"]
                            }
                        }
                    }
                }
            }),
            serde_json::json!({
                "name": "probe_workspace",
                "description": "Queries multiplexer sessions for a workspace path and determines whether an active session exists (HotSession) or a new pane must be spawned (ColdSpawn).",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "workspace": {
                            "type": "string",
                            "description": "Local workspace directory path to inspect"
                        }
                    },
                    "required": ["workspace"]
                }
            }),
            serde_json::json!({
                "name": "list_outbox",
                "description": "Lists all staged issue drafts in the HITL outbox waiting for human review, approval, or transmission.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "outbox_dir": {
                            "type": "string",
                            "description": "Optional path to the staged outbox directory (defaults to .scratch/staged)"
                        }
                    }
                }
            }),
            serde_json::json!({
                "name": "approve_issue",
                "description": "Approves a staged issue draft in the HITL outbox by ID, authorizing it for outbound transmission.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "id": {
                            "type": "string",
                            "description": "Issue identifier to approve (e.g. task_9f82a1b4)"
                        },
                        "outbox_dir": {
                            "type": "string",
                            "description": "Optional path to the staged outbox directory (defaults to .scratch/staged)"
                        }
                    },
                    "required": ["id"]
                }
            }),
            serde_json::json!({
                "name": "reject_issue",
                "description": "Rejects a staged issue draft in the HITL outbox by ID, barring it from outbound transmission.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "id": {
                            "type": "string",
                            "description": "Issue identifier to reject (e.g. task_9f82a1b4)"
                        },
                        "outbox_dir": {
                            "type": "string",
                            "description": "Optional path to the staged outbox directory (defaults to .scratch/staged)"
                        }
                    },
                    "required": ["id"]
                }
            }),
        ]
    }

    /// Handles a single parsed JSON-RPC request and produces a response if required.
    pub fn handle_request(&self, req: JsonRpcRequest) -> Option<JsonRpcResponse> {
        let is_notification = req.id.is_none();
        let id = req.id.unwrap_or(serde_json::Value::Null);

        match req.method.as_str() {
            "initialize" => {
                if is_notification {
                    return None;
                }
                Some(JsonRpcResponse::success(
                    id,
                    serde_json::json!({
                        "protocolVersion": "2024-11-05",
                        "capabilities": {
                            "tools": {}
                        },
                        "serverInfo": {
                            "name": "k0maru-agent-router",
                            "version": env!("CARGO_PKG_VERSION")
                        },
                        "instructions": "Autonomous, decoupled cross-repository agent routing microkernel."
                    }),
                ))
            }
            "notifications/initialized" => {
                // MCP initialization handshake completion notification
                None
            }
            "ping" => {
                if is_notification {
                    return None;
                }
                Some(JsonRpcResponse::success(id, serde_json::json!({})))
            }
            "tools/list" => {
                if is_notification {
                    return None;
                }
                Some(JsonRpcResponse::success(
                    id,
                    serde_json::json!({
                        "tools": Self::tools_list()
                    }),
                ))
            }
            "tools/call" => {
                if is_notification {
                    return None;
                }
                let params = match req.params {
                    Some(p) => p,
                    None => {
                        return Some(JsonRpcResponse::error(
                            id,
                            -32602,
                            "Missing parameters in 'tools/call'",
                        ));
                    }
                };
                let tool_name = match params.get("name").and_then(|v| v.as_str()) {
                    Some(n) => n,
                    None => {
                        return Some(JsonRpcResponse::error(
                            id,
                            -32602,
                            "Missing tool 'name' parameter in 'tools/call'",
                        ));
                    }
                };
                let arguments = params
                    .get("arguments")
                    .cloned()
                    .unwrap_or(serde_json::json!({}));
                let call_result = self.execute_tool(tool_name, &arguments);
                let result_val = match serde_json::to_value(&call_result) {
                    Ok(v) => v,
                    Err(e) => {
                        return Some(JsonRpcResponse::error(
                            id,
                            -32603,
                            format!("Internal serialization error: {e}"),
                        ));
                    }
                };
                Some(JsonRpcResponse::success(id, result_val))
            }
            "resources/list" => {
                if is_notification {
                    None
                } else {
                    Some(JsonRpcResponse::success(
                        id,
                        serde_json::json!({
                            "resources": []
                        }),
                    ))
                }
            }
            "prompts/list" => {
                if is_notification {
                    None
                } else {
                    Some(JsonRpcResponse::success(
                        id,
                        serde_json::json!({
                            "prompts": []
                        }),
                    ))
                }
            }
            method => {
                if is_notification {
                    None
                } else {
                    Some(JsonRpcResponse::error(
                        id,
                        -32601,
                        format!("Method not found: {method}"),
                    ))
                }
            }
        }
    }

    /// Executes an MCP tool by name with arguments.
    fn execute_tool(&self, name: &str, arguments: &serde_json::Value) -> McpToolCallResult {
        match name {
            "route_task" => self.execute_route_task(arguments),
            "probe_workspace" => self.execute_probe_workspace(arguments),
            "list_outbox" => self.execute_list_outbox(arguments),
            "approve_issue" => self.execute_approve_issue(arguments),
            "reject_issue" => self.execute_reject_issue(arguments),
            unknown => McpToolCallResult::error(format!("Unknown tool: '{unknown}'")),
        }
    }

    /// Handles `route_task` tool invocation.
    fn execute_route_task(&self, args: &serde_json::Value) -> McpToolCallResult {
        let dry_run = args
            .get("dry_run")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let outbox_dir_override = args
            .get("outbox_dir")
            .and_then(|v| v.as_str())
            .map(PathBuf::from);

        let mut envelope_val = if let Some(env_val) = args.get("envelope") {
            if let Some(s) = env_val.as_str() {
                match serde_json::from_str::<serde_json::Value>(s) {
                    Ok(v) => v,
                    Err(e) => {
                        return McpToolCallResult::error(format!(
                            "Failed to parse envelope JSON string: {e}"
                        ));
                    }
                }
            } else if env_val.is_object() {
                env_val.clone()
            } else {
                return McpToolCallResult::error(
                    "Field 'envelope' must be an object or JSON string",
                );
            }
        } else if args.is_object()
            && (args.get("caller").is_some()
                || args.get("target").is_some()
                || args.get("payload").is_some())
        {
            args.clone()
        } else {
            return McpToolCallResult::error(
                "Missing 'envelope' object or task envelope fields in arguments",
            );
        };

        if let Some(obj) = envelope_val.as_object_mut() {
            if !obj.contains_key("created_at") || obj.get("created_at").is_some_and(|v| v.is_null())
            {
                obj.insert(
                    "created_at".to_string(),
                    serde_json::Value::String(chrono::Utc::now().to_rfc3339()),
                );
            }
            if !obj.contains_key("$schema") || obj.get("$schema").is_some_and(|v| v.is_null()) {
                obj.insert(
                    "$schema".to_string(),
                    serde_json::Value::String(DEFAULT_SCHEMA.to_string()),
                );
            }
            if !obj.contains_key("id") || obj.get("id").is_some_and(|v| v.is_null()) {
                obj.insert(
                    "id".to_string(),
                    serde_json::Value::String(format!("task_{}", uuid::Uuid::new_v4().simple())),
                );
            }
        }

        let envelope: TaskEnvelope = match serde_json::from_value(envelope_val) {
            Ok(e) => e,
            Err(e) => {
                return McpToolCallResult::error(format!("Invalid task envelope structure: {e}"));
            }
        };

        let mut candidate_tuples: Vec<(String, String)> = Vec::new();
        if let Some(candidates_arr) = args.get("candidates").and_then(|v| v.as_array()) {
            for item in candidates_arr {
                if let Some(obj) = item.as_object() {
                    let id = obj
                        .get("identifier")
                        .or_else(|| obj.get("id"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    let desc = obj
                        .get("description")
                        .or_else(|| obj.get("desc"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    if !id.is_empty() {
                        candidate_tuples.push((id.to_string(), desc.to_string()));
                    }
                } else if let Some(arr) = item.as_array() {
                    let id = arr.first().and_then(|v| v.as_str()).unwrap_or("");
                    let desc = arr.get(1).and_then(|v| v.as_str()).unwrap_or("");
                    if !id.is_empty() {
                        candidate_tuples.push((id.to_string(), desc.to_string()));
                    }
                } else if let Some(s) = item.as_str() {
                    candidate_tuples.push((s.to_string(), String::new()));
                }
            }
        }

        let is_ambiguous = envelope.target.identifier == "auto"
            || envelope.target.identifier == "unspecified"
            || !candidate_tuples.is_empty();

        if !is_ambiguous {
            if let Err(e) = envelope.validate() {
                return McpToolCallResult::error(format!("Envelope validation failed: {e}"));
            }
        }

        let coordinator = if let Some(dir) = outbox_dir_override {
            self.coordinator.clone().with_outbox_dir(dir)
        } else {
            self.coordinator.clone()
        };

        let candidate_refs: Vec<(&str, &str)> = candidate_tuples
            .iter()
            .map(|(id, desc)| (id.as_str(), desc.as_str()))
            .collect();

        match coordinator.route_envelope_with_candidates(&envelope, &candidate_refs, dry_run) {
            Ok(outcome) => {
                let status_str = match &outcome {
                    RouteOutcome::Dispatched { .. } => "dispatched",
                    RouteOutcome::Staged { .. } => "staged",
                    RouteOutcome::DryRun { .. } => "dry_run",
                };
                let res = serde_json::json!({
                    "task_id": envelope.id,
                    "status": status_str,
                    "outcome": outcome,
                });
                McpToolCallResult::success(res)
            }
            Err(e) => McpToolCallResult::error(format!("Routing error: {e}")),
        }
    }

    /// Handles `probe_workspace` tool invocation.
    fn execute_probe_workspace(&self, args: &serde_json::Value) -> McpToolCallResult {
        let ws_str = match args.get("workspace").and_then(|v| v.as_str()) {
            Some(s) if !s.trim().is_empty() => s,
            _ => return McpToolCallResult::error("Missing or empty 'workspace' parameter"),
        };
        let ws_path = Path::new(ws_str);
        match self.coordinator.probe_workspace(ws_path) {
            Ok(resolution) => {
                let res = serde_json::json!({
                    "workspace": ws_str,
                    "resolution": resolution,
                });
                McpToolCallResult::success(res)
            }
            Err(e) => McpToolCallResult::error(format!("Probe error: {e}")),
        }
    }

    /// Handles `list_outbox` tool invocation.
    fn execute_list_outbox(&self, args: &serde_json::Value) -> McpToolCallResult {
        let outbox_dir_override = args
            .get("outbox_dir")
            .and_then(|v| v.as_str())
            .map(PathBuf::from);

        let coordinator = if let Some(dir) = outbox_dir_override {
            self.coordinator.clone().with_outbox_dir(dir)
        } else {
            self.coordinator.clone()
        };

        match coordinator.list_staged() {
            Ok(staged) => {
                let res = serde_json::json!({
                    "outbox_dir": coordinator.outbox_dir(),
                    "total": staged.len(),
                    "issues": staged,
                });
                McpToolCallResult::success(res)
            }
            Err(e) => McpToolCallResult::error(format!("List outbox error: {e}")),
        }
    }

    /// Handles `approve_issue` tool invocation.
    fn execute_approve_issue(&self, args: &serde_json::Value) -> McpToolCallResult {
        let id = match args.get("id").and_then(|v| v.as_str()) {
            Some(s) if !s.trim().is_empty() => s,
            _ => return McpToolCallResult::error("Missing or empty 'id' parameter"),
        };
        let outbox_dir_override = args
            .get("outbox_dir")
            .and_then(|v| v.as_str())
            .map(PathBuf::from);

        let coordinator = if let Some(dir) = outbox_dir_override {
            self.coordinator.clone().with_outbox_dir(dir)
        } else {
            self.coordinator.clone()
        };

        match coordinator.approve_staged(id) {
            Ok(path) => {
                let res = serde_json::json!({
                    "id": id,
                    "status": "Approved",
                    "file_path": path,
                    "message": format!("Issue '{id}' successfully approved for outbound transmission.")
                });
                McpToolCallResult::success(res)
            }
            Err(e) => McpToolCallResult::error(format!("Approve error: {e}")),
        }
    }

    /// Handles `reject_issue` tool invocation.
    fn execute_reject_issue(&self, args: &serde_json::Value) -> McpToolCallResult {
        let id = match args.get("id").and_then(|v| v.as_str()) {
            Some(s) if !s.trim().is_empty() => s,
            _ => return McpToolCallResult::error("Missing or empty 'id' parameter"),
        };
        let outbox_dir_override = args
            .get("outbox_dir")
            .and_then(|v| v.as_str())
            .map(PathBuf::from);

        let coordinator = if let Some(dir) = outbox_dir_override {
            self.coordinator.clone().with_outbox_dir(dir)
        } else {
            self.coordinator.clone()
        };

        match coordinator.reject_staged(id) {
            Ok(()) => {
                let res = serde_json::json!({
                    "id": id,
                    "status": "Rejected",
                    "message": format!("Issue '{id}' successfully rejected.")
                });
                McpToolCallResult::success(res)
            }
            Err(e) => McpToolCallResult::error(format!("Reject error: {e}")),
        }
    }

    /// Processes a single input line, returning the JSON-RPC response string if appropriate.
    pub fn handle_line(&self, line: &str) -> Option<String> {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            return None;
        }

        // Support JSON-RPC 2.0 batch requests
        if trimmed.starts_with('[') {
            if let Ok(serde_json::Value::Array(requests)) =
                serde_json::from_str::<serde_json::Value>(trimmed)
            {
                let mut responses = Vec::new();
                for val in requests {
                    if let Ok(req) = serde_json::from_value::<JsonRpcRequest>(val.clone()) {
                        if let Some(resp) = self.handle_request(req) {
                            responses.push(resp);
                        }
                    } else {
                        let id = val.get("id").cloned().unwrap_or(serde_json::Value::Null);
                        responses.push(JsonRpcResponse::error(
                            id,
                            -32600,
                            "Invalid Request in batch",
                        ));
                    }
                }
                if !responses.is_empty() {
                    return serde_json::to_string(&responses).ok();
                }
                return None;
            }
        }

        // Single request parsing
        match serde_json::from_str::<JsonRpcRequest>(trimmed) {
            Ok(req) => {
                let resp = self.handle_request(req)?;
                serde_json::to_string(&resp).ok()
            }
            Err(e) => {
                if let Ok(val) = serde_json::from_str::<serde_json::Value>(trimmed) {
                    let id = val.get("id").cloned().unwrap_or(serde_json::Value::Null);
                    let resp = JsonRpcResponse::error(id, -32600, format!("Invalid Request: {e}"));
                    serde_json::to_string(&resp).ok()
                } else {
                    let resp = JsonRpcResponse::error(
                        serde_json::Value::Null,
                        -32700,
                        format!("Parse error: {e}"),
                    );
                    serde_json::to_string(&resp).ok()
                }
            }
        }
    }

    /// Reads JSON-RPC requests from `reader` line-by-line and writes responses to `writer`.
    pub fn run<R: BufRead, W: Write>(&self, mut reader: R, mut writer: W) -> std::io::Result<()> {
        let mut line = String::new();
        while reader.read_line(&mut line)? > 0 {
            if let Some(resp) = self.handle_line(&line) {
                writer.write_all(resp.as_bytes())?;
                writer.write_all(b"\n")?;
                writer.flush()?;
            }
            line.clear();
        }
        Ok(())
    }

    /// Runs the MCP server reading from standard input and writing to standard output.
    pub fn run_stdio(&self) -> std::io::Result<()> {
        let stdin = std::io::stdin();
        let stdout = std::io::stdout();
        self.run(stdin.lock(), stdout.lock())
    }
}
