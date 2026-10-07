//! Pont entre l'agent et les serveurs MCP déclarés dans la config (`mcpServers`).
//!
//! Les serveurs sont connectés une seule fois par processus, à la première
//! construction du runtime, puis réutilisés ; leurs outils sont exposés au modèle
//! sous la forme `mcp__<serveur>__<outil>`. Transports pris en charge : `stdio`
//! (processus local) et `http` (« Streamable HTTP », serveurs distants).

use std::collections::hash_map::DefaultHasher;
use std::collections::BTreeMap;
use std::hash::{Hash, Hasher};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use runtime::{
    mcp_server_log_path, mcp_tool_name, ManagedMcpTool, McpServerConfig, McpServerManager, McpTool,
    McpToolCallResult, PermissionMode, RuntimeConfig, ScopedMcpServerConfig,
};
use serde_json::{json, Value};

const DEFAULT_DISCOVERY_TIMEOUT: Duration = Duration::from_secs(60);
const DEFAULT_CALL_TIMEOUT: Duration = Duration::from_secs(300);
const PROTOCOL_VERSION: &str = "2025-03-26";
// Limite des noms de fonctions de l'API OpenAI (et donc d'OpenRouter).
const MAX_TOOL_NAME_LEN: usize = 64;

/// Outil MCP tel qu'il est présenté au modèle.
#[derive(Debug, Clone, PartialEq)]
pub struct McpToolEntry {
    /// Nom exposé au modèle (≤ 64 caractères).
    pub name: String,
    pub qualified_name: String,
    pub raw_name: String,
    pub server: String,
    pub description: Option<String>,
    pub input_schema: Value,
    /// Annotation `readOnlyHint` du serveur : l'outil ne modifie rien.
    pub read_only: bool,
}

impl McpToolEntry {
    #[must_use]
    pub fn required_permission(&self) -> PermissionMode {
        if self.read_only {
            PermissionMode::ReadOnly
        } else {
            PermissionMode::DangerFullAccess
        }
    }

    fn from_tool(server: &str, qualified_name: String, tool: &McpTool) -> Self {
        let mut input_schema = tool
            .input_schema
            .clone()
            .unwrap_or_else(|| json!({ "type": "object", "properties": {} }));
        if let Some(schema) = input_schema.as_object_mut() {
            schema
                .entry("type")
                .or_insert_with(|| Value::String("object".to_string()));
        }
        Self {
            name: exposed_tool_name(&qualified_name),
            qualified_name,
            raw_name: tool.name.clone(),
            server: server.to_string(),
            description: tool.description.clone(),
            input_schema,
            read_only: tool
                .annotations
                .as_ref()
                .and_then(|annotations| annotations.get("readOnlyHint"))
                .and_then(Value::as_bool)
                .unwrap_or(false),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ServerState {
    Connected {
        transport: &'static str,
        tools: usize,
    },
    Failed(String),
    Unsupported(String),
}

struct McpBridge {
    runtime: tokio::runtime::Runtime,
    stdio: McpServerManager,
    http: BTreeMap<String, HttpMcpClient>,
    signature: String,
    tools: Vec<McpToolEntry>,
    servers: BTreeMap<String, ServerState>,
}

fn bridge_slot() -> &'static Mutex<Option<McpBridge>> {
    static SLOT: OnceLock<Mutex<Option<McpBridge>>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(None))
}

fn lock_bridge() -> std::sync::MutexGuard<'static, Option<McpBridge>> {
    bridge_slot()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Connecte (une seule fois, ou à nouveau si la config change) les serveurs MCP
/// de la config et renvoie leurs outils.
#[must_use]
pub fn ensure_mcp_started(config: &RuntimeConfig) -> Vec<McpToolEntry> {
    let servers = config.mcp().servers();
    let signature = format!("{servers:?}");
    let mut slot = lock_bridge();
    if let Some(bridge) = slot.as_ref() {
        if bridge.signature == signature {
            return bridge.tools.clone();
        }
    }
    if let Some(mut previous) = slot.take() {
        previous.shutdown();
    }
    if servers.is_empty() {
        return Vec::new();
    }
    match McpBridge::start(servers, signature) {
        Ok(bridge) => {
            let tools = bridge.tools.clone();
            *slot = Some(bridge);
            tools
        }
        Err(error) => {
            eprintln!("MCP : démarrage impossible : {error}");
            Vec::new()
        }
    }
}

/// Outils MCP actuellement connectés (vide si aucun serveur).
pub(crate) fn connected_mcp_tools() -> Vec<McpToolEntry> {
    lock_bridge()
        .as_ref()
        .map(|bridge| bridge.tools.clone())
        .unwrap_or_default()
}

/// Appelle un outil MCP par le nom exposé au modèle.
pub fn call_mcp_tool(name: &str, input: &Value) -> Result<String, String> {
    let mut slot = lock_bridge();
    let bridge = slot
        .as_mut()
        .ok_or_else(|| "aucun serveur MCP n'est connecté".to_string())?;
    let entry = bridge
        .tools
        .iter()
        .find(|tool| tool.name == name)
        .cloned()
        .ok_or_else(|| format!("outil MCP inconnu : {name}"))?;
    let arguments = (!input.is_null()).then(|| input.clone());
    let timeout = env_duration("CLAW_MCP_TOOL_TIMEOUT_MS", DEFAULT_CALL_TIMEOUT);

    let result = if let Some(client) = bridge.http.get_mut(&entry.server) {
        client.call_tool(&entry.raw_name, arguments)?
    } else {
        let McpBridge { runtime, stdio, .. } = bridge;
        let response = runtime
            .block_on(async {
                tokio::time::timeout(timeout, stdio.call_tool(&entry.qualified_name, arguments))
                    .await
            })
            .map_err(|_| {
                format!(
                    "l'outil MCP `{name}` n'a pas répondu après {} s",
                    timeout.as_secs()
                )
            })?
            .map_err(|error| error.to_string())?;
        if let Some(error) = response.error {
            return Err(format!("erreur MCP {} : {}", error.code, error.message));
        }
        response
            .result
            .ok_or_else(|| "réponse MCP sans résultat".to_string())?
    };

    let text = render_tool_result(&result);
    if result.is_error.unwrap_or(false) {
        Err(text)
    } else {
        Ok(text)
    }
}

/// Rapport de `/mcp` : état des serveurs et liste des outils.
#[must_use]
pub fn mcp_status_report(config: &RuntimeConfig) -> String {
    let slot = lock_bridge();
    let Some(bridge) = slot.as_ref().filter(|_| !config.mcp().servers().is_empty()) else {
        return [
            "MCP",
            "  Aucun serveur MCP configuré. Exemple pour .claw.json :",
            "",
            r#"  { "mcpServers": {"#,
            r#"      "fichiers": { "command": "npx", "args": ["-y", "@modelcontextprotocol/server-filesystem", "."] },"#,
            r#"      "distant":  { "type": "http", "url": "https://exemple.com/mcp", "headers": { "Authorization": "Bearer ${MON_TOKEN}" } }"#,
            "  } }",
        ]
        .join("\n");
    };

    let mut lines = vec!["MCP".to_string(), "Serveurs".to_string()];
    for (name, state) in &bridge.servers {
        lines.push(match state {
            ServerState::Connected { transport, tools } => {
                format!("  ● {name:<20} connecté ({transport}) · {tools} outil(s)")
            }
            ServerState::Failed(reason) => format!("  ✘ {name:<20} échec · {reason}"),
            ServerState::Unsupported(reason) => format!("  ○ {name:<20} ignoré · {reason}"),
        });
    }
    if !bridge.tools.is_empty() {
        lines.push(String::new());
        lines.push("Outils".to_string());
        for tool in &bridge.tools {
            let description = tool
                .description
                .as_deref()
                .unwrap_or_default()
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            let short = description.chars().take(80).collect::<String>();
            let marker = if tool.read_only { " (lecture)" } else { "" };
            lines.push(format!("  {}{marker}  {short}", tool.name));
        }
    }
    lines.push(String::new());
    lines.push(
        "Autoriser sans demande : \"permissions\": { \"allow\": [\"mcp__<serveur>\"] }".to_string(),
    );
    lines.join("\n")
}

/// Arrête les serveurs MCP (à appeler avant la fin du programme).
pub fn shutdown_mcp() {
    if let Some(mut bridge) = lock_bridge().take() {
        bridge.shutdown();
    }
}

impl McpBridge {
    fn start(
        servers: &BTreeMap<String, ScopedMcpServerConfig>,
        signature: String,
    ) -> Result<Self, String> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .map_err(|error| error.to_string())?;
        eprintln!("MCP : connexion à {} serveur(s)…", servers.len());
        let discovery_timeout = env_duration("CLAW_MCP_TIMEOUT_MS", DEFAULT_DISCOVERY_TIMEOUT);

        let mut stdio = McpServerManager::from_servers(servers);
        let discovery = runtime.block_on(stdio.discover_tools_resilient(discovery_timeout));
        let mut tools = discovery
            .tools
            .iter()
            .map(|managed: &ManagedMcpTool| {
                McpToolEntry::from_tool(
                    &managed.server_name,
                    managed.qualified_name.clone(),
                    &managed.tool,
                )
            })
            .collect::<Vec<_>>();
        let mut states = BTreeMap::new();
        for (name, reason) in &discovery.failures {
            states.insert(
                name.clone(),
                ServerState::Failed(format!(
                    "{reason} (journal : {})",
                    mcp_server_log_path(name).display()
                )),
            );
        }

        let mut http = BTreeMap::new();
        for (name, server) in servers {
            match &server.config {
                McpServerConfig::Stdio(_) => {}
                McpServerConfig::Http(remote) => {
                    let mut client =
                        HttpMcpClient::new(&remote.url, &remote.headers, discovery_timeout);
                    match client.initialize().and_then(|()| client.list_tools()) {
                        Ok(remote_tools) => {
                            tools.extend(remote_tools.iter().map(|tool| {
                                McpToolEntry::from_tool(name, mcp_tool_name(name, &tool.name), tool)
                            }));
                            http.insert(name.clone(), client);
                        }
                        Err(error) => {
                            states.insert(name.clone(), ServerState::Failed(error));
                        }
                    }
                }
                other => {
                    states.insert(
                        name.clone(),
                        ServerState::Unsupported(format!(
                            "transport {:?} non pris en charge (utilisez stdio ou http)",
                            other_transport_label(other)
                        )),
                    );
                }
            }
        }

        for name in servers.keys() {
            if !states.contains_key(name) {
                let count = tools.iter().filter(|tool| &tool.server == name).count();
                let transport = if http.contains_key(name) {
                    "http"
                } else {
                    "stdio"
                };
                states.insert(
                    name.clone(),
                    ServerState::Connected {
                        transport,
                        tools: count,
                    },
                );
            }
        }

        let failed = states
            .values()
            .filter(|state| !matches!(state, ServerState::Connected { .. }))
            .count();
        eprintln!(
            "MCP : {} serveur(s) connecté(s), {} outil(s){}",
            states.len() - failed,
            tools.len(),
            if failed > 0 {
                format!(" ; {failed} en échec ou ignoré(s) (voir /mcp)")
            } else {
                String::new()
            }
        );

        Ok(Self {
            runtime,
            stdio,
            http,
            signature,
            tools,
            servers: states,
        })
    }

    fn shutdown(&mut self) {
        let _ = self.runtime.block_on(self.stdio.shutdown());
        for client in self.http.values_mut() {
            client.close();
        }
    }
}

fn other_transport_label(config: &McpServerConfig) -> &'static str {
    match config {
        McpServerConfig::Stdio(_) => "stdio",
        McpServerConfig::Sse(_) => "sse",
        McpServerConfig::Http(_) => "http",
        McpServerConfig::Ws(_) => "ws",
        McpServerConfig::Sdk(_) => "sdk",
        McpServerConfig::ManagedProxy(_) => "claudeai-proxy",
    }
}

/// Raccourcit les noms trop longs pour l'API (préfixe + empreinte stable).
fn exposed_tool_name(qualified_name: &str) -> String {
    if qualified_name.len() <= MAX_TOOL_NAME_LEN {
        return qualified_name.to_string();
    }
    let mut hasher = DefaultHasher::new();
    qualified_name.hash(&mut hasher);
    let digest = format!("{:08x}", hasher.finish() & 0xffff_ffff);
    let keep = MAX_TOOL_NAME_LEN - digest.len() - 1;
    format!("{}_{digest}", &qualified_name[..keep])
}

fn render_tool_result(result: &McpToolCallResult) -> String {
    let parts = result
        .content
        .iter()
        .map(|item| {
            let field = |key: &str| item.data.get(key).and_then(Value::as_str);
            match item.kind.as_str() {
                "text" => field("text").unwrap_or_default().to_string(),
                "image" | "audio" => format!(
                    "[{} {} non affichable]",
                    item.kind,
                    field("mimeType").unwrap_or("?")
                ),
                "resource" => item
                    .data
                    .get("resource")
                    .and_then(|resource| {
                        resource
                            .get("text")
                            .or_else(|| resource.get("uri"))
                            .and_then(Value::as_str)
                    })
                    .unwrap_or_default()
                    .to_string(),
                "resource_link" => format!("[ressource : {}]", field("uri").unwrap_or("?")),
                _ => serde_json::to_string(&item.data).unwrap_or_default(),
            }
        })
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    if !parts.is_empty() {
        return parts.join("\n");
    }
    result
        .structured_content
        .as_ref()
        .and_then(|value| serde_json::to_string_pretty(value).ok())
        .unwrap_or_else(|| "(aucun contenu)".to_string())
}

fn env_duration(key: &str, default: Duration) -> Duration {
    std::env::var(key)
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .filter(|millis| *millis > 0)
        .map_or(default, Duration::from_millis)
}

/// Remplace `${VAR}` par la valeur de la variable d'environnement (vide si absente).
fn expand_env_vars(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut rest = value;
    while let Some(start) = rest.find("${") {
        output.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let Some(end) = after.find('}') else {
            output.push_str(&rest[start..]);
            return output;
        };
        output.push_str(&std::env::var(&after[..end]).unwrap_or_default());
        rest = &after[end + 1..];
    }
    output.push_str(rest);
    output
}

/// Client MCP « Streamable HTTP » : chaque message JSON-RPC est un POST ; la
/// réponse est du JSON ou un flux SSE contenant la réponse.
struct HttpMcpClient {
    http: reqwest::blocking::Client,
    url: String,
    headers: Vec<(String, String)>,
    session_id: Option<String>,
    next_id: u64,
}

impl HttpMcpClient {
    fn new(url: &str, headers: &BTreeMap<String, String>, timeout: Duration) -> Self {
        Self {
            http: reqwest::blocking::Client::builder()
                .timeout(timeout.max(env_duration(
                    "CLAW_MCP_TOOL_TIMEOUT_MS",
                    DEFAULT_CALL_TIMEOUT,
                )))
                .build()
                .unwrap_or_else(|_| reqwest::blocking::Client::new()),
            url: expand_env_vars(url),
            headers: headers
                .iter()
                .map(|(name, value)| (name.clone(), expand_env_vars(value)))
                .collect(),
            session_id: None,
            next_id: 1,
        }
    }

    fn initialize(&mut self) -> Result<(), String> {
        self.request(
            "initialize",
            &json!({
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": {},
                "clientInfo": { "name": "claw", "version": env!("CARGO_PKG_VERSION") }
            }),
        )?;
        self.notify("notifications/initialized")
    }

    fn list_tools(&mut self) -> Result<Vec<McpTool>, String> {
        let mut tools = Vec::new();
        let mut cursor: Option<String> = None;
        loop {
            let params = cursor
                .as_ref()
                .map_or_else(|| json!({}), |cursor| json!({ "cursor": cursor }));
            let result = self.request("tools/list", &params)?;
            let page: Vec<McpTool> =
                serde_json::from_value(result.get("tools").cloned().unwrap_or_else(|| json!([])))
                    .map_err(|error| format!("liste d'outils invalide : {error}"))?;
            tools.extend(page);
            cursor = result
                .get("nextCursor")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned);
            if cursor.is_none() {
                return Ok(tools);
            }
        }
    }

    fn call_tool(
        &mut self,
        name: &str,
        arguments: Option<Value>,
    ) -> Result<McpToolCallResult, String> {
        let result = self.request(
            "tools/call",
            &json!({ "name": name, "arguments": arguments.unwrap_or_else(|| json!({})) }),
        )?;
        serde_json::from_value(result).map_err(|error| format!("résultat MCP invalide : {error}"))
    }

    fn request(&mut self, method: &str, params: &Value) -> Result<Value, String> {
        let id = self.next_id;
        self.next_id += 1;
        let body = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
        let response = self.post(&body)?;
        if let Some(session_id) = response
            .headers()
            .get("mcp-session-id")
            .and_then(|value| value.to_str().ok())
        {
            self.session_id = Some(session_id.to_string());
        }
        let status = response.status();
        let is_sse = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.starts_with("text/event-stream"));
        let text = response.text().map_err(|error| error.to_string())?;
        if !status.is_success() {
            let hint = if status.as_u16() == 401 || status.as_u16() == 403 {
                " (vérifiez l'en-tête Authorization dans \"headers\")"
            } else {
                ""
            };
            return Err(format!(
                "HTTP {status} pour {method}{hint} : {}",
                text.chars().take(300).collect::<String>()
            ));
        }

        let messages = if is_sse {
            parse_sse_messages(&text)
        } else {
            vec![serde_json::from_str::<Value>(&text)
                .map_err(|error| format!("réponse JSON invalide pour {method} : {error}"))?]
        };
        let message = messages
            .into_iter()
            .find(|message| message.get("id") == Some(&json!(id)))
            .ok_or_else(|| format!("aucune réponse à {method} dans la réponse du serveur"))?;
        if let Some(error) = message.get("error") {
            return Err(format!(
                "erreur MCP pour {method} : {}",
                error
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("erreur inconnue")
            ));
        }
        Ok(message.get("result").cloned().unwrap_or(Value::Null))
    }

    fn notify(&mut self, method: &str) -> Result<(), String> {
        let response = self.post(&json!({ "jsonrpc": "2.0", "method": method }))?;
        if response.status().is_success() {
            Ok(())
        } else {
            Err(format!("HTTP {} pour {method}", response.status()))
        }
    }

    fn post(&self, body: &Value) -> Result<reqwest::blocking::Response, String> {
        let mut request = self
            .http
            .post(&self.url)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .header(
                reqwest::header::ACCEPT,
                "application/json, text/event-stream",
            )
            .header("MCP-Protocol-Version", PROTOCOL_VERSION);
        if let Some(session_id) = &self.session_id {
            request = request.header("Mcp-Session-Id", session_id);
        }
        for (name, value) in &self.headers {
            request = request.header(name.as_str(), value.as_str());
        }
        request
            .body(body.to_string())
            .send()
            .map_err(|error| format!("connexion impossible à {} : {error}", self.url))
    }

    /// Termine la session côté serveur (facultatif dans le protocole).
    fn close(&mut self) {
        if let Some(session_id) = self.session_id.take() {
            let _ = self
                .http
                .delete(&self.url)
                .header("Mcp-Session-Id", session_id)
                .send();
        }
    }
}

fn parse_sse_messages(body: &str) -> Vec<Value> {
    body.replace("\r\n", "\n")
        .split("\n\n")
        .filter_map(|event| {
            let data = event
                .lines()
                .filter_map(|line| line.strip_prefix("data:"))
                .map(str::trim_start)
                .collect::<Vec<_>>()
                .join("\n");
            (!data.is_empty())
                .then(|| serde_json::from_str::<Value>(&data).ok())
                .flatten()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{
        expand_env_vars, exposed_tool_name, parse_sse_messages, render_tool_result, McpToolEntry,
        MAX_TOOL_NAME_LEN,
    };
    use runtime::{McpTool, McpToolCallResult, PermissionMode};
    use serde_json::json;

    #[test]
    fn long_tool_names_are_shortened_deterministically() {
        let short = "mcp__fs__read_file";
        assert_eq!(exposed_tool_name(short), short);
        let long = format!("mcp__server__{}", "x".repeat(80));
        let exposed = exposed_tool_name(&long);
        assert_eq!(exposed.len(), MAX_TOOL_NAME_LEN);
        assert_eq!(exposed, exposed_tool_name(&long));
        assert!(exposed.starts_with("mcp__server__"));
    }

    #[test]
    fn tool_entries_follow_read_only_hints_and_default_schema() {
        let tool = McpTool {
            name: "search".to_string(),
            description: Some("Search docs".to_string()),
            input_schema: None,
            annotations: Some(json!({ "readOnlyHint": true })),
            meta: None,
        };
        let entry = McpToolEntry::from_tool("docs", "mcp__docs__search".to_string(), &tool);
        assert_eq!(entry.required_permission(), PermissionMode::ReadOnly);
        assert_eq!(entry.input_schema["type"], "object");

        let writer = McpToolEntry::from_tool(
            "docs",
            "mcp__docs__write".to_string(),
            &McpTool {
                annotations: None,
                input_schema: Some(json!({ "properties": {} })),
                ..tool
            },
        );
        assert_eq!(
            writer.required_permission(),
            PermissionMode::DangerFullAccess
        );
        assert_eq!(writer.input_schema["type"], "object");
    }

    #[test]
    fn renders_text_resource_and_structured_results() {
        let result: McpToolCallResult = serde_json::from_value(json!({
            "content": [
                { "type": "text", "text": "ligne 1" },
                { "type": "resource", "resource": { "uri": "file:///a", "text": "contenu" } },
                { "type": "image", "data": "AAAA", "mimeType": "image/png" }
            ]
        }))
        .expect("result");
        assert_eq!(
            render_tool_result(&result),
            "ligne 1\ncontenu\n[image image/png non affichable]"
        );

        let structured: McpToolCallResult =
            serde_json::from_value(json!({ "content": [], "structuredContent": { "n": 3 } }))
                .expect("structured");
        assert!(render_tool_result(&structured).contains("\"n\": 3"));
    }

    #[test]
    fn parses_sse_bodies_and_expands_env_vars() {
        let messages = parse_sse_messages(
            "event: message\r\ndata: {\"jsonrpc\":\"2.0\",\"method\":\"notifications/progress\"}\r\n\r\nevent: message\r\ndata: {\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{}}\r\n\r\n",
        );
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[1]["id"], 1);

        std::env::set_var("CLAW_TEST_MCP_TOKEN", "abc");
        assert_eq!(
            expand_env_vars("Bearer ${CLAW_TEST_MCP_TOKEN}/${CLAW_TEST_MISSING_VAR}"),
            "Bearer abc/"
        );
        assert_eq!(expand_env_vars("no ${unterminated"), "no ${unterminated");
        std::env::remove_var("CLAW_TEST_MCP_TOKEN");
    }
}
