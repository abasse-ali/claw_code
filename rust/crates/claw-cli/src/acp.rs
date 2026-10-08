//! `claw acp` : Claw comme agent de l'Agent Client Protocol (JSON-RPC sur
//! stdio), utilisable depuis Zed, Neovim (`CodeCompanion`), Emacs et les autres
//! éditeurs compatibles ACP.
//!
//! Zed (`settings.json`) :
//! ```json
//! { "agent_servers": { "Claw": { "command": "claw", "args": ["acp"] } } }
//! ```
//! Méthodes : `initialize`, `session/new`, `session/load`, `session/prompt`,
//! `session/set_mode`, notification `session/cancel`. L'agent envoie
//! `session/update` (texte, raisonnement, appels d'outils) et
//! `session/request_permission` (l'éditeur affiche la demande d'autorisation).

use std::collections::{BTreeMap, VecDeque};
use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

use runtime::{
    ContentBlock, ConversationRuntime, MessageRole, PermissionMode, PermissionPromptDecision,
    PermissionPrompter, PermissionRequest, Session,
};
use serde_json::{json, Value};

use crate::{
    build_runtime_with_ui, build_system_prompt, create_managed_session_handle,
    describe_permission_target, persist_allow_rule, resolve_session_reference,
    rule_is_session_only, AllowedToolSet, CliToolExecutor, DefaultRuntimeClient, RuntimeUi,
    SessionHandle, SharedObserver, TurnObserver,
};

const PROTOCOL_VERSION: u64 = 1;
const PERMISSION_TIMEOUT: Duration = Duration::from_secs(600);

type AcpRuntime = ConversationRuntime<DefaultRuntimeClient, CliToolExecutor>;

/// Sortie JSON-RPC partagée (une ligne par message) et requêtes en attente de réponse.
#[derive(Clone)]
struct Channel {
    pending: Arc<Mutex<BTreeMap<u64, mpsc::Sender<Value>>>>,
    next_id: Arc<AtomicU64>,
}

impl Channel {
    fn send(value: &Value) {
        let mut stdout = std::io::stdout().lock();
        let _ = writeln!(stdout, "{value}");
        let _ = stdout.flush();
    }

    fn notify(method: &str, params: Value) {
        Self::send(&json!({ "jsonrpc": "2.0", "method": method, "params": params }));
    }

    /// Requête vers l'éditeur ; attend sa réponse (ou l'annulation du tour).
    fn request(&self, method: &str, params: Value) -> Option<Value> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (sender, receiver) = mpsc::channel();
        self.pending
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(id, sender);
        Self::send(&json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }));
        let deadline = std::time::Instant::now() + PERMISSION_TIMEOUT;
        let reply = loop {
            match receiver.recv_timeout(Duration::from_millis(200)) {
                Ok(value) => break Some(value),
                Err(mpsc::RecvTimeoutError::Timeout)
                    if !runtime::interrupt::interrupt_requested()
                        && std::time::Instant::now() < deadline => {}
                Err(_) => break None,
            }
        };
        self.pending
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&id);
        reply
    }
}

/// Appels d'outils annoncés, en attente de résultat (pour relier résultat et
/// demande d'autorisation au bon `toolCallId`).
#[derive(Default)]
struct ToolCalls {
    next: u64,
    open: VecDeque<(String, String, String)>,
}

fn tool_kind(name: &str) -> &'static str {
    match name {
        "read_file" | "BashOutput" | "ReadMcpResource" | "ListMcpResources" => "read",
        "write_file" | "edit_file" | "NotebookEdit" => "edit",
        "bash" | "PowerShell" | "REPL" | "KillShell" => "execute",
        "glob_search" | "grep_search" | "ToolSearch" => "search",
        "WebFetch" | "WebSearch" => "fetch",
        "Agent" | "ExitPlanMode" | "TodoWrite" => "think",
        _ => "other",
    }
}

fn tool_title(name: &str, input: &str) -> String {
    let described = describe_permission_target(name, input);
    let detail = described
        .split_once(char::is_whitespace)
        .map_or(described.as_str(), |(_, rest)| rest.trim());
    let first_line = detail.lines().next().unwrap_or_default();
    format!(
        "{name} · {}",
        first_line.chars().take(120).collect::<String>()
    )
}

fn tool_locations(input: &str) -> Value {
    let value: Value = serde_json::from_str(input).unwrap_or_default();
    let path = value
        .get("path")
        .or_else(|| value.get("file_path"))
        .or_else(|| value.get("notebook_path"))
        .and_then(Value::as_str);
    match path {
        Some(path) => {
            let absolute = std::env::current_dir().unwrap_or_default().join(path);
            json!([{ "path": absolute.display().to_string() }])
        }
        None => json!([]),
    }
}

struct AcpObserver {
    session_id: String,
    calls: Arc<Mutex<ToolCalls>>,
}

impl AcpObserver {
    fn update(&self, update: Value) {
        Channel::notify(
            "session/update",
            json!({ "sessionId": self.session_id, "update": update }),
        );
    }
}

impl TurnObserver for AcpObserver {
    fn text_delta(&self, text: &str) {
        self.update(json!({
            "sessionUpdate": "agent_message_chunk",
            "content": { "type": "text", "text": text },
        }));
    }

    fn thinking_delta(&self, text: &str) {
        self.update(json!({
            "sessionUpdate": "agent_thought_chunk",
            "content": { "type": "text", "text": text },
        }));
    }

    fn tool_call(&self, name: &str, input: &str) {
        let id = {
            let mut calls = self
                .calls
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            calls.next += 1;
            let id = format!("call_{}", calls.next);
            calls
                .open
                .push_back((id.clone(), name.to_string(), input.to_string()));
            id
        };
        self.update(json!({
            "sessionUpdate": "tool_call",
            "toolCallId": id,
            "title": tool_title(name, input),
            "kind": tool_kind(name),
            "status": "pending",
            "rawInput": serde_json::from_str::<Value>(input).unwrap_or(Value::Null),
            "locations": tool_locations(input),
        }));
    }

    fn tool_result(&self, name: &str, output: &str, is_error: bool) {
        let id = {
            let mut calls = self
                .calls
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let position = calls
                .open
                .iter()
                .position(|(_, open_name, _)| open_name == name);
            position
                .and_then(|position| calls.open.remove(position))
                .map(|(id, _, _)| id)
        };
        let Some(id) = id else {
            return;
        };
        let shown: String = output.chars().take(8_000).collect();
        self.update(json!({
            "sessionUpdate": "tool_call_update",
            "toolCallId": id,
            "status": if is_error { "failed" } else { "completed" },
            "content": [{ "type": "content", "content": { "type": "text", "text": shown } }],
        }));
    }

    fn notice(&self, text: &str) {
        self.update(json!({
            "sessionUpdate": "agent_thought_chunk",
            "content": { "type": "text", "text": format!("▲ {text}\n") },
        }));
    }
}

/// Demande d'autorisation affichée par l'éditeur (`session/request_permission`).
struct AcpPrompter {
    channel: Channel,
    session_id: String,
    calls: Arc<Mutex<ToolCalls>>,
}

impl PermissionPrompter for AcpPrompter {
    fn decide(&mut self, request: &PermissionRequest) -> PermissionPromptDecision {
        let call_id = self
            .calls
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .open
            .iter()
            .find(|(_, name, input)| *name == request.tool_name && *input == request.input)
            .map_or_else(|| "call_permission".to_string(), |(id, _, _)| id.clone());
        let mut options =
            vec![json!({ "optionId": "allow_once", "name": "Autoriser", "kind": "allow_once" })];
        if request.suggested_rule.is_some() {
            options.push(json!({
                "optionId": "allow_always",
                "name": format!("Toujours autoriser ({})", request.suggested_rule.as_deref().unwrap_or_default()),
                "kind": "allow_always",
            }));
        }
        options
            .push(json!({ "optionId": "reject_once", "name": "Refuser", "kind": "reject_once" }));
        let reply = self.channel.request(
            "session/request_permission",
            json!({
                "sessionId": self.session_id,
                "toolCall": {
                    "toolCallId": call_id,
                    "title": tool_title(&request.tool_name, &request.input),
                    "kind": tool_kind(&request.tool_name),
                    "status": "pending",
                    "rawInput": serde_json::from_str::<Value>(&request.input).unwrap_or(Value::Null),
                },
                "options": options,
            }),
        );
        let selected = reply
            .as_ref()
            .and_then(|reply| reply.get("result"))
            .and_then(|result| result.get("outcome"))
            .filter(|outcome| outcome.get("outcome").and_then(Value::as_str) == Some("selected"))
            .and_then(|outcome| outcome.get("optionId"))
            .and_then(Value::as_str)
            .map(ToOwned::to_owned);
        // Refus : l'outil ne s'exécutera pas, l'éditeur doit voir l'appel clos.
        if !matches!(selected.as_deref(), Some("allow_once" | "allow_always")) {
            let mut calls = self
                .calls
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if let Some(position) = calls.open.iter().position(|(id, _, _)| *id == call_id) {
                calls.open.remove(position);
            }
            drop(calls);
            Channel::notify(
                "session/update",
                json!({
                    "sessionId": self.session_id,
                    "update": {
                        "sessionUpdate": "tool_call_update",
                        "toolCallId": call_id,
                        "status": "failed",
                        "content": [{ "type": "content", "content": { "type": "text", "text": "Refusé par l'utilisateur." } }],
                    },
                }),
            );
        }
        match selected.as_deref() {
            Some("allow_once") => PermissionPromptDecision::Allow,
            Some("allow_always") => match &request.suggested_rule {
                Some(rule) => {
                    if !rule_is_session_only(rule) {
                        if let Ok(cwd) = std::env::current_dir() {
                            let _ = persist_allow_rule(&cwd, rule);
                        }
                    }
                    PermissionPromptDecision::AllowAlways { rule: rule.clone() }
                }
                None => PermissionPromptDecision::Allow,
            },
            Some(_) => PermissionPromptDecision::Deny {
                reason: "refusé dans l'éditeur".to_string(),
            },
            None => PermissionPromptDecision::Deny {
                reason: "demande d'autorisation annulée".to_string(),
            },
        }
    }
}

struct AcpSession {
    runtime: AcpRuntime,
    handle: SessionHandle,
    mode: PermissionMode,
    calls: Arc<Mutex<ToolCalls>>,
}

struct AcpServer {
    channel: Channel,
    model: String,
    default_mode: PermissionMode,
    allowed_tools: Option<AllowedToolSet>,
    sessions: BTreeMap<String, AcpSession>,
}

fn modes_value(current: PermissionMode) -> Value {
    json!({
        "currentModeId": current.as_str(),
        "availableModes": [
            { "id": "prompt", "name": "Demander", "description": "Lecture libre, demande avant chaque modification ou commande" },
            { "id": "read-only", "name": "Plan", "description": "Lecture seule : explore et propose un plan" },
            { "id": "workspace-write", "name": "Modifier", "description": "Modifie les fichiers sans demander, demande avant les commandes" },
            { "id": "danger-full-access", "name": "Tout autoriser", "description": "Tous les outils sans demande" },
        ],
    })
}

impl AcpServer {
    fn build_runtime(
        &self,
        session_id: &str,
        session: Session,
        mode: PermissionMode,
        calls: &Arc<Mutex<ToolCalls>>,
    ) -> Result<AcpRuntime, String> {
        let observer: SharedObserver = Arc::new(AcpObserver {
            session_id: session_id.to_string(),
            calls: Arc::clone(calls),
        });
        let channel = self.channel.clone();
        let prompter_calls = Arc::clone(calls);
        let prompter_session = session_id.to_string();
        let system_prompt = build_system_prompt().map_err(|error| error.to_string())?;
        build_runtime_with_ui(
            session,
            self.model.clone(),
            system_prompt,
            true,
            false,
            self.allowed_tools.clone(),
            mode,
            None,
            Some(RuntimeUi {
                observer,
                subagent_prompter: Arc::new(move || {
                    Box::new(AcpPrompter {
                        channel: channel.clone(),
                        session_id: prompter_session.clone(),
                        calls: Arc::clone(&prompter_calls),
                    }) as tools::SubagentPrompter
                }),
            }),
        )
        .map_err(|error| error.to_string())
    }

    fn handle(&mut self, method: &str, params: &Value) -> Result<Value, (i64, String)> {
        match method {
            "initialize" => Ok(json!({
                "protocolVersion": PROTOCOL_VERSION,
                "agentCapabilities": {
                    "loadSession": true,
                    "promptCapabilities": { "image": true, "audio": false, "embeddedContext": true },
                },
                "authMethods": [],
                "agentInfo": { "name": "claw", "title": "Claw Code", "version": env!("CARGO_PKG_VERSION") },
            })),
            "authenticate" => Ok(json!({})),
            "session/new" => self.new_session(params, None),
            "session/load" => {
                let id = params
                    .get("sessionId")
                    .and_then(Value::as_str)
                    .ok_or((-32602, "sessionId manquant".to_string()))?
                    .to_string();
                self.new_session(params, Some(id)).map(|_| Value::Null)
            }
            "session/set_mode" => {
                let id = session_id(params)?;
                let mode = params
                    .get("modeId")
                    .and_then(Value::as_str)
                    .and_then(|mode| crate::normalize_permission_mode(mode))
                    .map(crate::permission_mode_from_label)
                    .ok_or((-32602, "mode inconnu".to_string()))?;
                let (session, calls) = {
                    let current = self
                        .sessions
                        .get(&id)
                        .ok_or((-32602, "session inconnue".to_string()))?;
                    (
                        current.runtime.session().clone(),
                        Arc::clone(&current.calls),
                    )
                };
                let runtime = self
                    .build_runtime(&id, session, mode, &calls)
                    .map_err(|error| (-32603, error))?;
                if let Some(current) = self.sessions.get_mut(&id) {
                    current.runtime = runtime;
                    current.mode = mode;
                }
                Ok(Value::Null)
            }
            "session/prompt" => self.prompt(params),
            other => Err((-32601, format!("méthode inconnue : {other}"))),
        }
    }

    fn new_session(
        &mut self,
        params: &Value,
        load: Option<String>,
    ) -> Result<Value, (i64, String)> {
        if let Some(cwd) = params.get("cwd").and_then(Value::as_str) {
            std::env::set_current_dir(cwd)
                .map_err(|error| (-32602, format!("dossier {cwd} : {error}")))?;
        }
        let (handle, session) = match &load {
            Some(id) => {
                let handle =
                    resolve_session_reference(id).map_err(|error| (-32602, error.to_string()))?;
                let session = Session::load_from_path(&handle.path)
                    .map_err(|error| (-32603, error.to_string()))?;
                (handle, session)
            }
            None => (
                create_managed_session_handle().map_err(|error| (-32603, error.to_string()))?,
                Session::new(),
            ),
        };
        let id = handle.id.clone();
        let calls = Arc::new(Mutex::new(ToolCalls::default()));
        let runtime = self
            .build_runtime(&id, session, self.default_mode, &calls)
            .map_err(|error| (-32603, error))?;
        if load.is_some() {
            replay_history(&id, runtime.session());
        }
        let _ = runtime.session().save_to_path(&handle.path);
        self.sessions.insert(
            id.clone(),
            AcpSession {
                runtime,
                handle,
                mode: self.default_mode,
                calls,
            },
        );
        Ok(json!({ "sessionId": id, "modes": modes_value(self.default_mode) }))
    }

    fn prompt(&mut self, params: &Value) -> Result<Value, (i64, String)> {
        let id = session_id(params)?;
        let (text, images) = prompt_content(params.get("prompt"));
        let channel = self.channel.clone();
        let session = self
            .sessions
            .get_mut(&id)
            .ok_or((-32602, "session inconnue".to_string()))?;
        let mut prompter = AcpPrompter {
            channel,
            session_id: id.clone(),
            calls: Arc::clone(&session.calls),
        };
        runtime::interrupt::clear_interrupt();
        let result = session
            .runtime
            .run_turn_with_images(text, images, Some(&mut prompter));
        let _ = session.runtime.session().save_to_path(&session.handle.path);
        // Plan approuvé pendant le tour : le mode reste celui de la politique.
        session.mode = session.runtime.permission_policy().active_mode();
        match result {
            Ok(_) => Ok(json!({ "stopReason": "end_turn" })),
            Err(error) if runtime::interrupt::is_interruption(&error.to_string()) => {
                Ok(json!({ "stopReason": "cancelled" }))
            }
            Err(error) => Err((-32603, error.to_string())),
        }
    }
}

fn session_id(params: &Value) -> Result<String, (i64, String)> {
    params
        .get("sessionId")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .ok_or((-32602, "sessionId manquant".to_string()))
}

/// Contenu d'un `session/prompt` : texte, ressources jointes, images.
fn prompt_content(prompt: Option<&Value>) -> (String, Vec<runtime::attachments::ImageAttachment>) {
    let mut parts = Vec::new();
    let mut images = Vec::new();
    for block in prompt.and_then(Value::as_array).into_iter().flatten() {
        match block.get("type").and_then(Value::as_str) {
            Some("text") => parts.push(
                block
                    .get("text")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
            ),
            Some("resource") => {
                let resource = block.get("resource").cloned().unwrap_or_default();
                let uri = resource
                    .get("uri")
                    .and_then(Value::as_str)
                    .unwrap_or("ressource");
                if let Some(text) = resource.get("text").and_then(Value::as_str) {
                    let path = uri.strip_prefix("file://").unwrap_or(uri);
                    parts.push(format!("<file path=\"{path}\">\n{text}\n</file>"));
                }
            }
            Some("resource_link") => {
                let uri = block.get("uri").and_then(Value::as_str).unwrap_or_default();
                let path = uri.strip_prefix("file://").unwrap_or(uri);
                parts.push(format!("@{path}"));
            }
            Some("image") => {
                if let (Some(data), Some(mime)) = (
                    block.get("data").and_then(Value::as_str),
                    block.get("mimeType").and_then(Value::as_str),
                ) {
                    images.push(runtime::attachments::ImageAttachment {
                        media_type: mime.to_string(),
                        data: data.to_string(),
                        path: PathBuf::from("éditeur"),
                    });
                }
            }
            _ => {}
        }
    }
    (parts.join("\n\n"), images)
}

/// `session/load` : l'éditeur reçoit l'historique sous forme de mises à jour.
fn replay_history(session_id: &str, session: &Session) {
    for message in &session.messages {
        for block in &message.blocks {
            let update = match (block, message.role) {
                (ContentBlock::Text { text }, MessageRole::Assistant) => json!({
                    "sessionUpdate": "agent_message_chunk",
                    "content": { "type": "text", "text": text },
                }),
                (ContentBlock::Text { text }, MessageRole::User)
                    if !text.starts_with(runtime::HOOK_CONTEXT_PREFIX) =>
                {
                    json!({
                        "sessionUpdate": "user_message_chunk",
                        "content": { "type": "text", "text": text },
                    })
                }
                _ => continue,
            };
            Channel::notify(
                "session/update",
                json!({ "sessionId": session_id, "update": update }),
            );
        }
    }
}

/// Boucle principale : messages de l'éditeur lus sur l'entrée standard.
pub(crate) fn serve(
    model: String,
    permission_mode: PermissionMode,
    allowed_tools: Option<AllowedToolSet>,
) -> Result<(), Box<dyn std::error::Error>> {
    let channel = Channel {
        pending: Arc::new(Mutex::new(BTreeMap::new())),
        next_id: Arc::new(AtomicU64::new(1_000)),
    };
    let (requests_tx, requests_rx) = mpsc::channel::<Value>();
    let reader_channel = channel.clone();
    std::thread::spawn(move || {
        let stdin = std::io::stdin();
        for line in stdin.lock().lines() {
            let Ok(line) = line else { break };
            let Ok(message) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            let method = message
                .get("method")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned);
            match (method.as_deref(), message.get("id")) {
                (Some("session/cancel"), _) => runtime::interrupt::request_interrupt(),
                (Some(_), Some(_)) => {
                    if requests_tx.send(message).is_err() {
                        break;
                    }
                }
                (Some(_), None) => {}
                (None, Some(id)) => {
                    if let Some(sender) = id.as_u64().and_then(|id| {
                        reader_channel
                            .pending
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner)
                            .remove(&id)
                    }) {
                        let _ = sender.send(message);
                    }
                }
                (None, None) => {}
            }
        }
    });
    let mut server = AcpServer {
        channel,
        model,
        default_mode: permission_mode,
        allowed_tools,
        sessions: BTreeMap::new(),
    };
    for message in requests_rx {
        let id = message.get("id").cloned().unwrap_or(Value::Null);
        let method = message
            .get("method")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let params = message.get("params").cloned().unwrap_or(Value::Null);
        let reply = match server.handle(&method, &params) {
            Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
            Err((code, error)) => {
                json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": error } })
            }
        };
        Channel::send(&reply);
    }
    Ok(())
}
