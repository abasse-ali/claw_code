//! Pont entre l'agent et les serveurs MCP déclarés dans la config (`mcpServers`).
//!
//! Les serveurs sont connectés une seule fois par processus, à la première
//! construction du runtime, puis réutilisés ; leurs outils sont exposés au modèle
//! sous la forme `mcp__<serveur>__<outil>`. Transports pris en charge : `stdio`
//! (processus local), `http` (« Streamable HTTP ») et `sse` (ancien transport
//! des serveurs distants). Les ressources MCP sont lisibles par deux outils
//! (`ListMcpResources`, `ReadMcpResource`) et les prompts MCP deviennent des
//! commandes `/mcp__<serveur>__<prompt>`.

use std::collections::hash_map::DefaultHasher;
use std::collections::BTreeMap;
use std::hash::{Hash, Hasher};
use std::io::BufRead;
use std::sync::{mpsc, Mutex, OnceLock};
use std::time::{Duration, Instant};

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
    resources: Vec<McpResourceEntry>,
    prompts: Vec<McpPromptEntry>,
}

/// Ressource MCP (fichier, schéma, document…) d'un serveur.
#[derive(Debug, Clone, PartialEq)]
pub struct McpResourceEntry {
    pub server: String,
    pub uri: String,
    pub name: String,
    pub description: Option<String>,
    pub mime_type: Option<String>,
}

/// Prompt MCP, invoqué comme une commande `/mcp__<serveur>__<prompt>`.
#[derive(Debug, Clone, PartialEq)]
pub struct McpPromptEntry {
    pub server: String,
    pub name: String,
    pub command: String,
    pub description: Option<String>,
    /// (nom, obligatoire) dans l'ordre déclaré.
    pub arguments: Vec<(String, bool)>,
}

const LIST_RESOURCES_TOOL: &str = "ListMcpResources";
const READ_RESOURCE_TOOL: &str = "ReadMcpResource";

impl McpBridge {
    /// Requête JSON-RPC vers un serveur, quel que soit son transport.
    fn request(
        &mut self,
        server: &str,
        method: &'static str,
        params: Value,
    ) -> Result<Value, String> {
        if let Some(client) = self.http.get_mut(server) {
            return client.request(method, &params);
        }
        let McpBridge { runtime, stdio, .. } = self;
        let timeout = env_duration("CLAW_MCP_TOOL_TIMEOUT_MS", DEFAULT_CALL_TIMEOUT);
        runtime
            .block_on(async {
                tokio::time::timeout(timeout, stdio.request_raw(server, method, Some(params))).await
            })
            .map_err(|_| format!("le serveur MCP `{server}` n'a pas répondu à {method}"))?
            .map_err(|error| error.to_string())
    }

    /// Ressources et prompts des serveurs connectés (ignorés s'ils n'en ont pas).
    fn discover_extras(&mut self) {
        let connected = self
            .servers
            .iter()
            .filter(|(_, state)| matches!(state, ServerState::Connected { .. }))
            .map(|(name, _)| name.clone())
            .collect::<Vec<_>>();
        for server in connected {
            if let Ok(result) = self.request(&server, "resources/list", json!({})) {
                for resource in result
                    .get("resources")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    let Some(uri) = resource.get("uri").and_then(Value::as_str) else {
                        continue;
                    };
                    let text = |key: &str| {
                        resource
                            .get(key)
                            .and_then(Value::as_str)
                            .map(ToOwned::to_owned)
                    };
                    self.resources.push(McpResourceEntry {
                        server: server.clone(),
                        uri: uri.to_string(),
                        name: text("name").unwrap_or_else(|| uri.to_string()),
                        description: text("description"),
                        mime_type: text("mimeType"),
                    });
                }
            }
            if let Ok(result) = self.request(&server, "prompts/list", json!({})) {
                for prompt in result
                    .get("prompts")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    let Some(name) = prompt.get("name").and_then(Value::as_str) else {
                        continue;
                    };
                    let arguments = prompt
                        .get("arguments")
                        .and_then(Value::as_array)
                        .map(|items| {
                            items
                                .iter()
                                .filter_map(|argument| {
                                    let name = argument.get("name")?.as_str()?.to_string();
                                    let required = argument
                                        .get("required")
                                        .and_then(Value::as_bool)
                                        .unwrap_or(false);
                                    Some((name, required))
                                })
                                .collect()
                        })
                        .unwrap_or_default();
                    self.prompts.push(McpPromptEntry {
                        server: server.clone(),
                        name: name.to_string(),
                        command: mcp_tool_name(&server, name),
                        description: prompt
                            .get("description")
                            .and_then(Value::as_str)
                            .map(ToOwned::to_owned),
                        arguments,
                    });
                }
            }
        }
        if !self.resources.is_empty() {
            self.tools.push(resource_tool(
                LIST_RESOURCES_TOOL,
                "List the resources (files, documents, schemas…) exposed by the connected MCP servers. Optional `server` filters by server name.",
                json!({ "type": "object", "properties": { "server": { "type": "string" } } }),
            ));
            self.tools.push(resource_tool(
                READ_RESOURCE_TOOL,
                "Read an MCP resource by its URI (from ListMcpResources).",
                json!({
                    "type": "object",
                    "properties": { "server": { "type": "string" }, "uri": { "type": "string" } },
                    "required": ["server", "uri"]
                }),
            ));
        }
    }
}

fn resource_tool(name: &str, description: &str, schema: Value) -> McpToolEntry {
    McpToolEntry {
        name: name.to_string(),
        qualified_name: name.to_string(),
        raw_name: name.to_string(),
        server: String::new(),
        description: Some(description.to_string()),
        input_schema: schema,
        read_only: true,
    }
}

/// Outils `ListMcpResources` / `ReadMcpResource`.
fn call_resource_tool(bridge: &mut McpBridge, name: &str, input: &Value) -> Result<String, String> {
    let server = input.get("server").and_then(Value::as_str);
    if name == LIST_RESOURCES_TOOL {
        let listed = bridge
            .resources
            .iter()
            .filter(|resource| server.is_none_or(|server| resource.server == server))
            .map(|resource| {
                json!({
                    "server": resource.server,
                    "uri": resource.uri,
                    "name": resource.name,
                    "description": resource.description,
                    "mimeType": resource.mime_type,
                })
            })
            .collect::<Vec<_>>();
        return serde_json::to_string_pretty(&listed).map_err(|error| error.to_string());
    }
    let server = server.ok_or("le champ `server` est requis")?;
    let uri = input
        .get("uri")
        .and_then(Value::as_str)
        .ok_or("le champ `uri` est requis")?;
    let result = bridge.request(server, "resources/read", json!({ "uri": uri }))?;
    let mut parts = Vec::new();
    for content in result
        .get("contents")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if let Some(text) = content.get("text").and_then(Value::as_str) {
            parts.push(text.to_string());
        } else if let Some(blob) = content.get("blob").and_then(Value::as_str) {
            parts.push(format!(
                "[contenu binaire {} : {} octets en base64]",
                content
                    .get("mimeType")
                    .and_then(Value::as_str)
                    .unwrap_or("?"),
                blob.len()
            ));
        }
    }
    Ok(parts.join("\n\n"))
}

/// Prompts MCP connus, pour la complétion et `/help`.
#[must_use]
pub fn mcp_prompt_commands() -> Vec<(String, Option<String>)> {
    lock_bridge()
        .as_ref()
        .map(|bridge| {
            bridge
                .prompts
                .iter()
                .map(|prompt| (format!("/{}", prompt.command), prompt.description.clone()))
                .collect()
        })
        .unwrap_or_default()
}

/// `/mcp__serveur__prompt arguments` → texte de la demande fourni par le serveur.
#[must_use]
pub fn mcp_prompt_invocation(input: &str) -> Option<Result<String, String>> {
    let rest = input.trim_start().strip_prefix('/')?;
    let (name, arguments) = rest
        .split_once(char::is_whitespace)
        .map_or((rest, ""), |(name, arguments)| (name, arguments.trim()));
    let mut slot = lock_bridge();
    let bridge = slot.as_mut()?;
    let prompt = bridge
        .prompts
        .iter()
        .find(|prompt| prompt.command == name)?
        .clone();
    let values = split_prompt_arguments(arguments);
    let mut params = serde_json::Map::new();
    for (index, (argument, required)) in prompt.arguments.iter().enumerate() {
        // Le dernier argument reçoit le reste du texte.
        let value = if index + 1 == prompt.arguments.len() && values.len() > index {
            values[index..].join(" ")
        } else {
            values.get(index).cloned().unwrap_or_default()
        };
        if value.is_empty() && *required {
            return Some(Err(format!(
                "argument « {argument} » requis : /{} {}",
                prompt.command,
                prompt
                    .arguments
                    .iter()
                    .map(|(name, required)| if *required {
                        format!("<{name}>")
                    } else {
                        format!("[{name}]")
                    })
                    .collect::<Vec<_>>()
                    .join(" ")
            )));
        }
        if !value.is_empty() {
            params.insert(argument.clone(), Value::String(value));
        }
    }
    let result = bridge.request(
        &prompt.server,
        "prompts/get",
        json!({ "name": prompt.name, "arguments": params }),
    );
    Some(result.map(|result| {
        result
            .get("messages")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|message| {
                let content = message.get("content")?;
                content
                    .get("text")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned)
                    .or_else(|| {
                        content.get("resource").and_then(|resource| {
                            resource
                                .get("text")
                                .and_then(Value::as_str)
                                .map(ToOwned::to_owned)
                        })
                    })
            })
            .collect::<Vec<_>>()
            .join("\n\n")
    }))
}

fn split_prompt_arguments(text: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;
    for c in text.chars() {
        match (quote, c) {
            (Some(open), c) if c == open => quote = None,
            (Some(_), c) => current.push(c),
            (None, '"' | '\'') => quote = Some(c),
            (None, c) if c.is_whitespace() => {
                if !current.is_empty() {
                    parts.push(std::mem::take(&mut current));
                }
            }
            (None, c) => current.push(c),
        }
    }
    if !current.is_empty() {
        parts.push(current);
    }
    parts
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
    if name == LIST_RESOURCES_TOOL || name == READ_RESOURCE_TOOL {
        return call_resource_tool(bridge, name, input);
    }
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
    if !bridge.resources.is_empty() {
        lines.push(format!(
            "  Ressources       {} (outils ListMcpResources / ReadMcpResource)",
            bridge.resources.len()
        ));
    }
    if !bridge.prompts.is_empty() {
        lines.push(String::new());
        lines.push("Prompts (commandes)".to_string());
        for prompt in &bridge.prompts {
            lines.push(format!(
                "  /{}  {}",
                prompt.command,
                prompt.description.as_deref().unwrap_or("")
            ));
        }
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
        let mut transports = BTreeMap::new();
        for (name, server) in servers {
            match &server.config {
                McpServerConfig::Stdio(_) => {}
                McpServerConfig::Http(remote) | McpServerConfig::Sse(remote) => {
                    let sse = matches!(server.config, McpServerConfig::Sse(_));
                    transports.insert(name.clone(), if sse { "sse" } else { "http" });
                    let client = if sse {
                        HttpMcpClient::new_sse(
                            name,
                            &remote.url,
                            &remote.headers,
                            discovery_timeout,
                        )
                    } else {
                        Ok(HttpMcpClient::new(
                            name,
                            &remote.url,
                            &remote.headers,
                            discovery_timeout,
                        ))
                    };
                    let connected = client.and_then(|mut client| {
                        client.initialize()?;
                        let tools = client.list_tools()?;
                        Ok((client, tools))
                    });
                    match connected.map(|(client, tools)| (Some(client), tools)) {
                        Ok((client, remote_tools)) => {
                            tools.extend(remote_tools.iter().map(|tool| {
                                McpToolEntry::from_tool(name, mcp_tool_name(name, &tool.name), tool)
                            }));
                            if let Some(client) = client {
                                http.insert(name.clone(), client);
                            }
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
                            "transport {:?} non pris en charge (utilisez stdio, http ou sse)",
                            other_transport_label(other)
                        )),
                    );
                }
            }
        }

        for name in servers.keys() {
            if !states.contains_key(name) {
                let count = tools.iter().filter(|tool| &tool.server == name).count();
                let transport = transports.get(name).copied().unwrap_or("stdio");
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

        let mut bridge = Self {
            runtime,
            stdio,
            http,
            signature,
            tools,
            servers: states,
            resources: Vec::new(),
            prompts: Vec::new(),
        };
        bridge.discover_extras();
        Ok(bridge)
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
    server: String,
    /// Ancien transport SSE : adresse des POST et messages reçus du flux.
    sse: Option<SseChannel>,
}

struct SseChannel {
    endpoint: String,
    messages: mpsc::Receiver<Value>,
}

impl HttpMcpClient {
    fn new(server: &str, url: &str, headers: &BTreeMap<String, String>, timeout: Duration) -> Self {
        let mut client = Self::without_auth(server, url, headers, timeout);
        // Jeton obtenu par `claw mcp login` si aucun en-tête n'est configuré.
        if !client
            .headers
            .iter()
            .any(|(name, _)| name.eq_ignore_ascii_case("authorization"))
        {
            if let Some(token) = crate::mcp_oauth::access_token(server) {
                client
                    .headers
                    .push(("Authorization".to_string(), format!("Bearer {token}")));
            }
        }
        client
    }

    /// Ancien transport SSE : un flux GET reçoit l'adresse des POST puis les réponses.
    fn new_sse(
        server: &str,
        url: &str,
        headers: &BTreeMap<String, String>,
        timeout: Duration,
    ) -> Result<Self, String> {
        let mut client = Self::new(server, url, headers, timeout);
        let mut request = reqwest::blocking::Client::builder()
            .build()
            .map_err(|error| error.to_string())?
            .get(&client.url)
            .header(reqwest::header::ACCEPT, "text/event-stream");
        for (name, value) in &client.headers {
            request = request.header(name.as_str(), value.as_str());
        }
        let response = request
            .send()
            .map_err(|error| format!("connexion SSE impossible à {} : {error}", client.url))?;
        if !response.status().is_success() {
            return Err(auth_hint(server, response.status(), "connexion SSE"));
        }
        let (endpoint_tx, endpoint_rx) = mpsc::channel::<String>();
        let (message_tx, message_rx) = mpsc::channel::<Value>();
        std::thread::spawn(move || {
            let reader = std::io::BufReader::new(response);
            let mut event = String::new();
            let mut data = Vec::new();
            for line in reader.lines() {
                let Ok(line) = line else { return };
                if line.is_empty() {
                    let payload = data.join("\n");
                    if event == "endpoint" {
                        let _ = endpoint_tx.send(payload);
                    } else if let Ok(message) = serde_json::from_str::<Value>(&payload) {
                        if message_tx.send(message).is_err() {
                            return;
                        }
                    }
                    event.clear();
                    data.clear();
                } else if let Some(value) = line.strip_prefix("event:") {
                    event = value.trim().to_string();
                } else if let Some(value) = line.strip_prefix("data:") {
                    data.push(value.trim_start().to_string());
                }
            }
        });
        let endpoint = endpoint_rx.recv_timeout(timeout).map_err(|_| {
            format!(
                "le serveur SSE {} n'a pas indiqué son adresse de messages",
                client.url
            )
        })?;
        let endpoint = reqwest::Url::parse(&client.url)
            .and_then(|base| base.join(&endpoint))
            .map_err(|error| format!("adresse de messages invalide : {error}"))?
            .to_string();
        client.sse = Some(SseChannel {
            endpoint,
            messages: message_rx,
        });
        Ok(client)
    }

    fn without_auth(
        server: &str,
        url: &str,
        headers: &BTreeMap<String, String>,
        timeout: Duration,
    ) -> Self {
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
            server: server.to_string(),
            sse: None,
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
        if self.sse.is_some() {
            if !response.status().is_success() {
                return Err(auth_hint(&self.server, response.status(), method));
            }
            return self.wait_sse_response(id, method);
        }
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
            return Err(format!(
                "{} : {}",
                auth_hint(&self.server, status, method),
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

    /// Réponse au message `id`, reçue sur le flux SSE.
    fn wait_sse_response(&mut self, id: u64, method: &str) -> Result<Value, String> {
        let timeout = env_duration("CLAW_MCP_TOOL_TIMEOUT_MS", DEFAULT_CALL_TIMEOUT);
        let deadline = Instant::now() + timeout;
        let channel = self.sse.as_ref().ok_or("transport SSE fermé")?;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            let message = channel
                .messages
                .recv_timeout(remaining)
                .map_err(|_| format!("pas de réponse SSE à {method}"))?;
            if message.get("id") != Some(&json!(id)) {
                continue;
            }
            if let Some(error) = message.get("error") {
                return Err(format!(
                    "erreur MCP pour {method} : {}",
                    error
                        .get("message")
                        .and_then(Value::as_str)
                        .unwrap_or("erreur inconnue")
                ));
            }
            return Ok(message.get("result").cloned().unwrap_or(Value::Null));
        }
    }

    fn post(&self, body: &Value) -> Result<reqwest::blocking::Response, String> {
        let target = self
            .sse
            .as_ref()
            .map_or(self.url.as_str(), |channel| channel.endpoint.as_str());
        let mut request = self
            .http
            .post(target)
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

/// Message d'erreur HTTP, avec la marche à suivre si une connexion est requise.
fn auth_hint(server: &str, status: reqwest::StatusCode, what: &str) -> String {
    if status.as_u16() == 401 || status.as_u16() == 403 {
        format!(
            "HTTP {status} pour {what} : connexion requise - lancez `claw mcp login {server}` (ou ajoutez un en-tête Authorization dans \"headers\")"
        )
    } else {
        format!("HTTP {status} pour {what}")
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
