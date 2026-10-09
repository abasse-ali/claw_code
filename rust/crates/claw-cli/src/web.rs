//! `claw serve` : interface web locale de l'agent.
//!
//! Le serveur n'écoute par défaut que sur la boucle locale et exige un jeton
//! secret, généré au démarrage, pour chaque appel d'API : une page web
//! quelconque ouverte dans le navigateur ne doit pas pouvoir piloter un agent
//! capable d'exécuter des commandes. Les en-têtes `Host` et `Origin` sont aussi
//! vérifiés (protection contre le DNS rebinding).
//!
//! L'agent tourne dans un thread dédié, car son code est synchrone ; ses
//! événements (texte, appels d'outils, demandes de permission…) sont diffusés
//! aux pages ouvertes par SSE.

use std::collections::HashMap;
use std::convert::Infallible;
use std::fmt::Write as _;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

use async_stream::stream;
use axum::extract::{DefaultBodyLimit, Query, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use commands::SlashCommand;
use runtime::{
    ContentBlock, ConversationMessage, ConversationRuntime, MessageRole, PermissionMode,
    PermissionPromptDecision, PermissionPrompter, PermissionRequest, Session,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::sync::broadcast;

use crate::{
    build_runtime_with_ui, build_system_prompt, create_managed_session_handle,
    delete_managed_session, delete_other_sessions, describe_permission_target, error_hint,
    expert_command_prompt, format_compact_report, format_cost_report, format_model_report,
    format_permissions_report, list_managed_sessions, models, normalize_permission_mode,
    open_browser, permission_mode_from_label, persist_allow_rule, rename_managed_session,
    render_slash_command_help, resolve_model_alias, resolve_session_reference,
    rule_is_session_only, search_managed_sessions, session_titles, AllowedToolSet, CliToolExecutor,
    DefaultRuntimeClient, RuntimeUi, SessionHandle, TurnObserver,
};

const PAGE: &str = include_str!("web/index.html");
const MAX_TOOL_OUTPUT_CHARS: usize = 20_000;
const PERMISSION_TIMEOUT: Duration = Duration::from_secs(15 * 60);
const BROADCAST_CAPACITY: usize = 1_024;

pub(crate) struct ServeOptions {
    pub host: String,
    pub port: u16,
    pub open_browser: bool,
    pub model: String,
    pub permission_mode: PermissionMode,
    pub allowed_tools: Option<AllowedToolSet>,
}

/// Événement affiché par la page. Les événements « de contenu » sont conservés
/// dans le fil de la conversation pour être rejoués à la reconnexion.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum WebEvent {
    UserMessage {
        text: String,
    },
    /// Image jointe par l'utilisateur (URL `data:`).
    UserImage {
        url: String,
    },
    /// Raisonnement du modèle, affiché dans un bloc repliable.
    Thinking {
        text: String,
    },
    AssistantText {
        text: String,
    },
    ToolCall {
        name: String,
        input: String,
    },
    ToolResult {
        name: String,
        output: String,
        is_error: bool,
    },
    Permission {
        id: u64,
        tool: String,
        target: String,
        mode: String,
        suggested_rule: Option<String>,
        session_only: bool,
    },
    PermissionResolved {
        id: u64,
        decision: String,
    },
    Info {
        text: String,
    },
    Error {
        message: String,
        hint: String,
    },
    TurnDone,
    Status {
        status: StatusView,
    },
    /// Crédits `OpenRouter` à jour (après chaque étape de l'agent, puis
    /// confirmés par `OpenRouter`).
    Credits {
        credits: serde_json::Value,
    },
    Reset,
}

#[derive(Debug, Clone, Default, Serialize)]
struct StatusView {
    model: String,
    provider: String,
    permission_mode: String,
    busy: bool,
    session_id: String,
    cwd: String,
    input_tokens: u64,
    output_tokens: u64,
    cost: Option<String>,
}

#[derive(Default)]
struct SharedView {
    transcript: Vec<WebEvent>,
    status: StatusView,
}

enum PermissionReply {
    Once,
    Always,
    Deny(Option<String>),
}

enum WorkerCommand {
    Message(String, Vec<runtime::attachments::ImageAttachment>),
    SetModel(String),
    SetPermissions(String),
    NewConversation,
    Resume(String),
}

#[derive(Clone)]
struct WebApp {
    token: Arc<String>,
    allowed_hosts: Arc<Vec<String>>,
    events: broadcast::Sender<WebEvent>,
    commands: mpsc::Sender<WorkerCommand>,
    view: Arc<Mutex<SharedView>>,
    pending: Arc<Mutex<HashMap<u64, mpsc::Sender<PermissionReply>>>>,
    next_permission_id: Arc<AtomicU64>,
    /// Pages ouvertes (navigateur, panneau de VS Code), sans les abonnements
    /// de la barre d'état (`?role=status`).
    page_viewers: Arc<AtomicU64>,
}

/// Retire la page du compte quand son flux d'événements se ferme.
struct ViewerGuard(Option<Arc<AtomicU64>>);

impl Drop for ViewerGuard {
    fn drop(&mut self) {
        if let Some(count) = &self.0 {
            count.fetch_sub(1, Ordering::SeqCst);
        }
    }
}

impl WebApp {
    /// Met à jour la vue partagée puis diffuse l'événement, sous le même verrou
    /// qu'une nouvelle connexion SSE : aucun événement n'est perdu ni doublé.
    fn publish(&self, event: WebEvent) {
        let mut view = self
            .view
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let event = match event {
            WebEvent::ToolResult {
                name,
                output,
                is_error,
            } => WebEvent::ToolResult {
                name,
                output: truncate_chars(&output, MAX_TOOL_OUTPUT_CHARS),
                is_error,
            },
            other => other,
        };
        match &event {
            WebEvent::Status { status } => view.status = status.clone(),
            WebEvent::Reset => view.transcript.clear(),
            WebEvent::AssistantText { text } => {
                if let Some(WebEvent::AssistantText { text: last }) = view.transcript.last_mut() {
                    last.push_str(text);
                } else {
                    view.transcript.push(event.clone());
                }
            }
            // Pas rejoués à la reconnexion : la page relit /api/credits.
            WebEvent::TurnDone | WebEvent::Credits { .. } => {}
            _ => view.transcript.push(event.clone()),
        }
        let _ = self.events.send(event);
    }

    fn is_busy(&self) -> bool {
        self.view
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .status
            .busy
    }

    fn current_session_id(&self) -> String {
        self.view
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .status
            .session_id
            .clone()
    }

    fn lock_pending(
        &self,
    ) -> std::sync::MutexGuard<'_, HashMap<u64, mpsc::Sender<PermissionReply>>> {
        self.pending
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

fn truncate_chars(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_string();
    }
    let kept = text.chars().take(limit).collect::<String>();
    format!("{kept}\n… (sortie tronquée pour l'affichage)")
}

struct WebObserver {
    app: WebApp,
}

impl TurnObserver for WebObserver {
    fn text_delta(&self, text: &str) {
        self.app.publish(WebEvent::AssistantText {
            text: text.to_string(),
        });
    }

    fn thinking_delta(&self, text: &str) {
        self.app.publish(WebEvent::Thinking {
            text: text.to_string(),
        });
    }

    fn tool_call(&self, name: &str, input: &str) {
        self.app.publish(WebEvent::ToolCall {
            name: name.to_string(),
            input: input.to_string(),
        });
    }

    fn tool_result(&self, name: &str, output: &str, is_error: bool) {
        self.app.publish(WebEvent::ToolResult {
            name: name.to_string(),
            output: output.to_string(),
            is_error,
        });
    }

    fn notice(&self, text: &str) {
        self.app.publish(WebEvent::Info {
            text: format!("⟳ {text}"),
        });
    }
}

/// Demande l'approbation dans la page et attend la réponse (15 min au plus).
#[derive(Clone)]
struct WebPrompter {
    app: WebApp,
    mode: PermissionMode,
}

impl PermissionPrompter for WebPrompter {
    fn decide(&mut self, request: &PermissionRequest) -> PermissionPromptDecision {
        let id = self.app.next_permission_id.fetch_add(1, Ordering::Relaxed);
        let (sender, receiver) = mpsc::channel();
        self.app.lock_pending().insert(id, sender);
        self.app.publish(WebEvent::Permission {
            id,
            tool: request.tool_name.clone(),
            target: describe_permission_target(&request.tool_name, &request.input),
            mode: self.mode.as_str().to_string(),
            suggested_rule: request.suggested_rule.clone(),
            session_only: request
                .suggested_rule
                .as_deref()
                .is_some_and(rule_is_session_only),
        });
        let deadline = std::time::Instant::now() + PERMISSION_TIMEOUT;
        let reply = loop {
            match receiver.recv_timeout(Duration::from_millis(300)) {
                Err(mpsc::RecvTimeoutError::Timeout)
                    if !runtime::interrupt::interrupt_requested()
                        && std::time::Instant::now() < deadline => {}
                other => break other,
            }
        };
        self.app.lock_pending().remove(&id);

        let (decision, label) = match reply {
            Ok(PermissionReply::Once) => (
                PermissionPromptDecision::Allow,
                "autorisé une fois".to_string(),
            ),
            Ok(PermissionReply::Always) => match &request.suggested_rule {
                Some(rule) => {
                    let mut label = format!("toujours autorisé : {rule}");
                    if !rule_is_session_only(rule) {
                        match std::env::current_dir()
                            .map_err(|error| error.to_string())
                            .and_then(|cwd| persist_allow_rule(&cwd, rule))
                        {
                            Ok(_) => label.push_str(" (enregistré dans .claw/settings.local.json)"),
                            Err(error) => {
                                let _ = write!(label, " (non enregistré : {error})");
                            }
                        }
                    }
                    (
                        PermissionPromptDecision::AllowAlways { rule: rule.clone() },
                        label,
                    )
                }
                None => (PermissionPromptDecision::Allow, "autorisé".to_string()),
            },
            Ok(PermissionReply::Deny(feedback)) => {
                let reason = feedback.as_deref().map_or_else(
                    || {
                        format!(
                            "l'utilisateur a refusé l'outil '{}' ; demandez-lui comment poursuivre",
                            request.tool_name
                        )
                    },
                    |feedback| {
                        format!(
                            "l'utilisateur a refusé l'outil '{}' avec cette consigne : {feedback}",
                            request.tool_name
                        )
                    },
                );
                let label = feedback.map_or_else(
                    || "refusé".to_string(),
                    |feedback| format!("refusé : {feedback}"),
                );
                (PermissionPromptDecision::Deny { reason }, label)
            }
            Err(_) if runtime::interrupt::interrupt_requested() => (
                PermissionPromptDecision::Deny {
                    reason: runtime::interrupt::INTERRUPTED_MESSAGE.to_string(),
                },
                "annulé (tour interrompu)".to_string(),
            ),
            Err(_) => (
                PermissionPromptDecision::Deny {
                    reason: "aucune réponse dans l'interface web".to_string(),
                },
                "refusé (pas de réponse)".to_string(),
            ),
        };
        self.app.publish(WebEvent::PermissionResolved {
            id,
            decision: label,
        });
        decision
    }
}

type WebRuntime = ConversationRuntime<DefaultRuntimeClient, CliToolExecutor>;

/// Thread propriétaire de la conversation : il traite les commandes une à une.
struct Worker {
    app: WebApp,
    model: String,
    permission_mode: PermissionMode,
    allowed_tools: Option<AllowedToolSet>,
    system_prompt: Vec<String>,
    session: SessionHandle,
    runtime: WebRuntime,
}

fn web_ui(app: &WebApp, mode: PermissionMode) -> RuntimeUi {
    let prompter_app = app.clone();
    RuntimeUi {
        observer: Arc::new(WebObserver { app: app.clone() }),
        subagent_prompter: Arc::new(move || {
            Box::new(WebPrompter {
                app: prompter_app.clone(),
                mode,
            }) as tools::SubagentPrompter
        }),
    }
}

fn build_web_runtime(
    app: &WebApp,
    model: &str,
    mode: PermissionMode,
    allowed_tools: Option<&AllowedToolSet>,
    system_prompt: &[String],
    session: Session,
) -> Result<WebRuntime, String> {
    build_runtime_with_ui(
        session,
        model.to_string(),
        system_prompt.to_vec(),
        true,
        false,
        allowed_tools.cloned(),
        mode,
        None,
        Some(web_ui(app, mode)),
    )
    .map_err(|error| error.to_string())
}

impl Worker {
    fn build(&self, session: Session) -> Result<WebRuntime, String> {
        build_web_runtime(
            &self.app,
            &self.model,
            self.permission_mode,
            self.allowed_tools.as_ref(),
            &self.system_prompt,
            session,
        )
    }

    fn rebuild(&mut self) -> Result<(), String> {
        self.runtime = self.build(self.runtime.session().clone())?;
        Ok(())
    }

    fn persist(&self) {
        if let Err(error) = self.runtime.session().save_to_path(&self.session.path) {
            self.app.publish(WebEvent::Error {
                message: format!("sauvegarde de la session impossible : {error}"),
                hint: String::new(),
            });
        }
    }

    fn status(&self, busy: bool) -> StatusView {
        let usage = self.runtime.usage().cumulative_usage();
        StatusView {
            model: self.model.clone(),
            provider: api::detect_provider_kind(&self.model).label().to_string(),
            permission_mode: self.permission_mode.as_str().to_string(),
            busy,
            session_id: self.session.id.clone(),
            cwd: std::env::current_dir()
                .map(|cwd| cwd.display().to_string())
                .unwrap_or_default(),
            input_tokens: u64::from(usage.input_tokens),
            output_tokens: u64::from(usage.output_tokens),
            cost: models::estimated_cost(&self.model, usage),
        }
    }

    fn publish_status(&self, busy: bool) {
        self.app.publish(WebEvent::Status {
            status: self.status(busy),
        });
    }

    fn info(&self, text: impl Into<String>) {
        self.app.publish(WebEvent::Info { text: text.into() });
    }

    /// Retours de hooks à montrer dans le fil.
    fn hook_feedback(&self, event: runtime::HookEvent, result: &runtime::HookRunResult) {
        for message in result.feedback() {
            self.info(format!("hook {} : {message}", event.as_str()));
        }
    }

    fn session_hook_start(&self, source: &str) {
        let result = self.runtime.hooks().session_start(source);
        self.hook_feedback(runtime::HookEvent::SessionStart, &result);
    }

    fn run(mut self, commands: &mpsc::Receiver<WorkerCommand>) {
        self.session_hook_start("startup");
        self.publish_status(false);
        for command in commands {
            self.publish_status(true);
            if let Err(error) = self.handle(command) {
                self.app.publish(WebEvent::Error {
                    hint: error_hint(&error).to_string(),
                    message: error,
                });
            }
            self.publish_status(false);
        }
    }

    fn handle(&mut self, command: WorkerCommand) -> Result<(), String> {
        match command {
            WorkerCommand::Message(text, mut images) => {
                if !text.trim().is_empty() {
                    self.app
                        .publish(WebEvent::UserMessage { text: text.clone() });
                }
                if text.trim_start().starts_with('/') && images.is_empty() {
                    return self.slash_command(text.trim());
                }
                // `@capture.png` dans le texte, comme dans le terminal.
                let cwd = std::env::current_dir().unwrap_or_default();
                let files = runtime::attachments::file_mention_blocks(&text, &cwd);
                let (mut text, cited, errors) =
                    runtime::attachments::extract_image_attachments(&text, &cwd);
                if !files.is_empty() {
                    text = format!("{text}\n\n{}", files.join("\n\n"));
                }
                for error in errors {
                    self.info(format!("Image ignorée : {error}"));
                }
                images.extend(cited);
                for image in &images {
                    self.app.publish(WebEvent::UserImage {
                        url: format!("data:{};base64,{}", image.media_type, image.data),
                    });
                }
                self.run_turn_with_images(&text, images);
                Ok(())
            }
            WorkerCommand::SetModel(model) => self.set_model(&model),
            WorkerCommand::SetPermissions(mode) => self.set_permissions(&mode),
            WorkerCommand::NewConversation => self.new_conversation(),
            WorkerCommand::Resume(id) => self.resume(&id),
        }
    }

    fn run_turn(&mut self, prompt: &str) {
        self.run_turn_with_images(prompt, Vec::new());
    }

    fn run_turn_with_images(
        &mut self,
        prompt: &str,
        images: Vec<runtime::attachments::ImageAttachment>,
    ) {
        let mut prompter = WebPrompter {
            app: self.app.clone(),
            mode: self.permission_mode,
        };
        let result = self
            .runtime
            .run_turn_with_images(prompt, images, Some(&mut prompter));
        self.persist();
        // Plan approuvé pendant le tour : la session reste en workspace-write.
        let active = self.runtime.permission_policy().active_mode();
        if active != self.permission_mode {
            if let Err(error) = self.set_permissions(active.as_str()) {
                self.info(format!("Changement de mode impossible : {error}"));
            }
        }
        match result {
            Ok(_) => self.app.publish(WebEvent::TurnDone),
            Err(error) if runtime::interrupt::is_interruption(&error.to_string()) => {
                self.info("■ Tour interrompu.");
                self.app.publish(WebEvent::TurnDone);
            }
            Err(error) => {
                let message = error.to_string();
                self.app.publish(WebEvent::Error {
                    hint: error_hint(&message).to_string(),
                    message,
                });
            }
        }
    }

    #[allow(clippy::too_many_lines)]
    fn slash_command(&mut self, text: &str) -> Result<(), String> {
        match SlashCommand::parse(text) {
            Some(SlashCommand::Expert { command, args }) => {
                match expert_command_prompt(command, args.as_deref()) {
                    Ok(prompt) => self.run_turn(&prompt),
                    Err(usage) => self.info(usage),
                }
            }
            Some(SlashCommand::Models { args })
                if args
                    .as_deref()
                    .is_some_and(|args| args == "bench" || args.starts_with("bench ")) =>
            {
                // Plusieurs minutes : l'avancement s'affiche dans la page au fil de l'eau.
                let report = crate::bench::run(args.as_deref().unwrap_or_default(), &mut |line| {
                    self.info(line);
                })?;
                self.info(report);
            }
            Some(SlashCommand::Models { args }) => {
                self.info(models::models_report(args.as_deref(), &self.model)?);
            }
            Some(SlashCommand::Credits) => self.info(crate::credits::report(true)),
            Some(SlashCommand::Doctor) => self.info(crate::transcript::strip_ansi(
                &crate::doctor::report(&self.model, &crate::version_summary()),
            )),
            Some(SlashCommand::Mcp) => {
                let cwd = std::env::current_dir().map_err(|error| error.to_string())?;
                let config = runtime::ConfigLoader::default_for(cwd)
                    .load()
                    .map_err(|error| error.to_string())?;
                self.info(tools::mcp_status_report(&config));
            }
            Some(SlashCommand::Model { model: Some(model) }) => self.set_model(&model)?,
            Some(SlashCommand::Model { model: None }) => self.info(format_model_report(
                &self.model,
                self.runtime.session().messages.len(),
                self.runtime.usage().turns(),
            )),
            Some(SlashCommand::Permissions { mode: Some(mode) }) => self.set_permissions(&mode)?,
            Some(SlashCommand::Permissions { mode: None }) => self.info(format_permissions_report(
                self.permission_mode.as_str(),
                self.runtime.permission_policy().rules(),
            )),
            Some(SlashCommand::Clear { .. }) => self.new_conversation()?,
            Some(SlashCommand::Undo { count }) => {
                let report = crate::undo_turns(&mut self.runtime, count.as_deref())?;
                self.persist();
                self.app.publish(WebEvent::Reset);
                for event in transcript_from_messages(&self.runtime.session().messages) {
                    self.app.publish(event);
                }
                self.info(report);
            }
            Some(SlashCommand::Checkpoints) => self.info(crate::render_checkpoints()),
            Some(SlashCommand::Hooks) => {
                self.info(crate::render_hooks_report(self.runtime.hooks().config()));
            }
            Some(SlashCommand::Think { level }) => {
                self.info(crate::apply_think_command(level.as_deref()));
            }
            Some(SlashCommand::Bashes { args }) => {
                self.info(crate::render_background_tasks(args.as_deref()));
            }
            Some(SlashCommand::Compact) => {
                let result = self.runtime.compact(runtime::CompactionConfig::default());
                let removed = result.removed_message_count;
                let kept = result.compacted_session.messages.len();
                self.runtime = self.build(result.compacted_session)?;
                self.persist();
                self.info(format_compact_report(removed, kept, removed == 0));
            }
            Some(SlashCommand::Context) => {
                let report = crate::context_report_for(
                    &self.model,
                    &self.system_prompt,
                    self.allowed_tools.as_ref(),
                    self.runtime.session(),
                );
                self.info(report);
            }
            Some(SlashCommand::Cost) => {
                let usage = self.runtime.usage().cumulative_usage();
                let mut report = format_cost_report(usage);
                if let Some(line) = models::cost_line(&self.model, usage) {
                    report.push('\n');
                    report.push_str(&line);
                }
                self.info(report);
            }
            Some(SlashCommand::Help) => self.info(format!(
                "{}\n\nDans l'interface web : /review /audit /archi /adr /fix /undo /checkpoints /hooks /think /bashes /models /mcp /model /permissions /cost /compact /clear.\nLes autres commandes s'utilisent dans le terminal.\n\n{}",
                render_slash_command_help(),
                commands::render_custom_commands_help(
                    &std::env::current_dir().unwrap_or_default()
                )
            )),
            Some(SlashCommand::Unknown(name)) => {
                let cwd = std::env::current_dir().map_err(|error| error.to_string())?;
                match commands::resolve_custom_command(&cwd, text) {
                    Some(invocation) => {
                        let invocation = invocation?;
                        // Modèle propre à la commande : le temps de ce tour.
                        match invocation.command.model.clone() {
                            Some(model) => {
                                let previous = self.model.clone();
                                self.set_model(&model)?;
                                self.run_turn(&invocation.prompt);
                                self.set_model(&previous)?;
                            }
                            None => self.run_turn(&invocation.prompt),
                        }
                    }
                    None => match tools::mcp_prompt_invocation(text) {
                        Some(prompt) => self.run_turn(&prompt?),
                        None => self.info(format!("Commande inconnue : /{name}. Tapez /help.")),
                    },
                }
            }
            Some(_) => self.info(
                "Cette commande n'est disponible que dans le terminal (claw).".to_string(),
            ),
            None => self.run_turn(text),
        }
        Ok(())
    }

    fn set_model(&mut self, model: &str) -> Result<(), String> {
        let model = resolve_model_alias(model.trim());
        if model.is_empty() {
            return Err("nom de modèle vide".to_string());
        }
        let previous = std::mem::replace(&mut self.model, model);
        if let Err(error) = self.rebuild() {
            self.model = previous;
            return Err(error);
        }
        self.info(format!("Modèle : {}", self.model));
        Ok(())
    }

    fn set_permissions(&mut self, mode: &str) -> Result<(), String> {
        let label = normalize_permission_mode(mode).ok_or_else(|| {
            format!("mode de permission inconnu « {mode} » (prompt, read-only, workspace-write, danger-full-access)")
        })?;
        self.permission_mode = permission_mode_from_label(label);
        self.rebuild()?;
        self.info(format!("Permissions : {label}"));
        Ok(())
    }

    fn new_conversation(&mut self) -> Result<(), String> {
        let ended = self.runtime.hooks().session_end("clear");
        self.session = create_managed_session_handle().map_err(|error| error.to_string())?;
        self.runtime = self.build(Session::new())?;
        self.app.publish(WebEvent::Reset);
        self.hook_feedback(runtime::HookEvent::SessionEnd, &ended);
        self.session_hook_start("clear");
        Ok(())
    }

    fn resume(&mut self, reference: &str) -> Result<(), String> {
        let handle = resolve_session_reference(reference).map_err(|error| error.to_string())?;
        let session = Session::load_from_path(&handle.path).map_err(|error| error.to_string())?;
        self.runtime = self.build(session)?;
        self.session = handle;
        self.app.publish(WebEvent::Reset);
        for event in transcript_from_messages(&self.runtime.session().messages) {
            self.app.publish(event);
        }
        self.session_hook_start("resume");
        Ok(())
    }
}

/// Reconstitue le fil affichable d'une session sauvegardée.
fn transcript_from_messages(messages: &[ConversationMessage]) -> Vec<WebEvent> {
    let mut events = Vec::new();
    for message in messages {
        for block in &message.blocks {
            events.push(match (block, message.role) {
                (ContentBlock::Text { text }, MessageRole::Assistant) => {
                    WebEvent::AssistantText { text: text.clone() }
                }
                // Contexte ajouté par les hooks : destiné au modèle seulement.
                (ContentBlock::Text { text }, _)
                    if text.starts_with(runtime::HOOK_CONTEXT_PREFIX) =>
                {
                    continue;
                }
                (ContentBlock::Text { text }, _) if text.starts_with(runtime::STOP_HOOK_PREFIX) => {
                    WebEvent::Info { text: text.clone() }
                }
                (ContentBlock::Text { text }, _) => WebEvent::UserMessage { text: text.clone() },
                (ContentBlock::ToolUse { name, input, .. }, _) => WebEvent::ToolCall {
                    name: name.clone(),
                    input: input.clone(),
                },
                (
                    ContentBlock::ToolResult {
                        tool_name,
                        output,
                        is_error,
                        ..
                    },
                    _,
                ) => WebEvent::ToolResult {
                    name: tool_name.clone(),
                    output: output.clone(),
                    is_error: *is_error,
                },
                (ContentBlock::Image { media_type, data }, _) => WebEvent::UserImage {
                    url: format!("data:{media_type};base64,{data}"),
                },
                (ContentBlock::Thinking { thinking, .. }, _) => WebEvent::Thinking {
                    text: thinking.clone(),
                },
            });
        }
    }
    events
}

// ------------------------------------------------------------------ HTTP

#[derive(Deserialize)]
struct TokenQuery {
    token: Option<String>,
    /// `status` : abonnement de la barre d'état de VS Code (pas une page).
    #[serde(default)]
    role: Option<String>,
}

#[derive(Deserialize)]
struct MessageBody {
    #[serde(default)]
    text: String,
    #[serde(default)]
    images: Vec<ImageBody>,
}

#[derive(Deserialize)]
struct ImageBody {
    media_type: String,
    data: String,
}

#[derive(Deserialize)]
struct PermissionBody {
    id: u64,
    decision: String,
    feedback: Option<String>,
}

#[derive(Deserialize)]
struct ValueBody {
    value: String,
}

type ApiResult = Result<Response, (StatusCode, Json<Value>)>;

fn api_error(status: StatusCode, message: &str) -> (StatusCode, Json<Value>) {
    (status, Json(json!({ "error": message })))
}

/// Vérifie l'hôte (anti DNS rebinding), l'origine et le jeton d'accès.
fn authorize(
    app: &WebApp,
    headers: &HeaderMap,
    query_token: Option<&str>,
) -> Result<(), (StatusCode, Json<Value>)> {
    let host = headers
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    if !app.allowed_hosts.iter().any(|allowed| allowed == host) {
        return Err(api_error(StatusCode::FORBIDDEN, "hôte non autorisé"));
    }
    if let Some(origin) = headers
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok())
    {
        let origin_host = origin
            .trim_start_matches("http://")
            .trim_start_matches("https://");
        if !app
            .allowed_hosts
            .iter()
            .any(|allowed| allowed == origin_host)
        {
            return Err(api_error(StatusCode::FORBIDDEN, "origine non autorisée"));
        }
    }
    let token = headers
        .get("x-claw-token")
        .and_then(|value| value.to_str().ok())
        .or(query_token)
        .unwrap_or_default();
    if !constant_time_eq(token.as_bytes(), app.token.as_bytes()) {
        return Err(api_error(
            StatusCode::UNAUTHORIZED,
            "jeton d'accès invalide",
        ));
    }
    Ok(())
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    left.len() == right.len()
        && left
            .iter()
            .zip(right)
            .fold(0_u8, |difference, (a, b)| difference | (a ^ b))
            == 0
}

async fn index() -> Response {
    // `claw serve --embed` : la page peut s'afficher dans un panneau de VS Code
    // (et seulement là) ; sinon elle refuse tout cadre.
    let embed = std::env::var("CLAW_WEB_EMBED").is_ok_and(|value| value.trim() == "1");
    let mut response = Html(PAGE).into_response();
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(if embed {
            "default-src 'self'; script-src 'self' 'unsafe-inline' https://cdn.jsdelivr.net; style-src 'self' 'unsafe-inline'; img-src 'self' data:; connect-src 'self'; frame-ancestors 'self' vscode-webview: vscode-file:"
        } else {
            "default-src 'self'; script-src 'self' 'unsafe-inline' https://cdn.jsdelivr.net; style-src 'self' 'unsafe-inline'; img-src 'self' data:; connect-src 'self'; frame-ancestors 'none'"
        }),
    );
    headers.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    if !embed {
        headers.insert(
            header::HeaderName::from_static("x-frame-options"),
            HeaderValue::from_static("DENY"),
        );
    }
    response
}

async fn events(
    State(app): State<WebApp>,
    headers: HeaderMap,
    Query(query): Query<TokenQuery>,
) -> ApiResult {
    authorize(&app, &headers, query.token.as_deref())?;
    let (snapshot, mut receiver) = {
        let view = app
            .view
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        (
            json!({ "status": view.status, "transcript": view.transcript }),
            app.events.subscribe(),
        )
    };
    let is_page = query.role.as_deref() != Some("status");
    if is_page {
        app.page_viewers.fetch_add(1, Ordering::SeqCst);
    }
    let guard = ViewerGuard(is_page.then(|| Arc::clone(&app.page_viewers)));
    let stream = stream! {
        let _guard = guard;
        yield Ok::<Event, Infallible>(Event::default().event("snapshot").data(snapshot.to_string()));
        loop {
            match receiver.recv().await {
                Ok(event) => {
                    if let Ok(data) = serde_json::to_string(&event) {
                        yield Ok(Event::default().event("event").data(data));
                    }
                }
                // Trop de retard : la page se resynchronise sur un nouvel instantané.
                Err(broadcast::error::RecvError::Lagged(_)) => {
                    yield Ok(Event::default().event("resync").data("{}"));
                    break;
                }
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    };
    Ok(Sse::new(stream)
        .keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
        .into_response())
}

async fn post_message(
    State(app): State<WebApp>,
    headers: HeaderMap,
    Json(body): Json<MessageBody>,
) -> ApiResult {
    authorize(&app, &headers, None)?;
    if body.text.trim().is_empty() && body.images.is_empty() {
        return Err(api_error(StatusCode::BAD_REQUEST, "message vide"));
    }
    let mut images = Vec::new();
    for image in body.images {
        if !matches!(
            image.media_type.as_str(),
            "image/png" | "image/jpeg" | "image/gif" | "image/webp"
        ) {
            return Err(api_error(
                StatusCode::BAD_REQUEST,
                "format d'image non pris en charge (png, jpg, gif, webp)",
            ));
        }
        // ~5 Mo une fois décodée.
        if image.data.len() > runtime::attachments::MAX_IMAGE_BYTES / 3 * 4 + 4 {
            return Err(api_error(
                StatusCode::PAYLOAD_TOO_LARGE,
                "image trop lourde (5 Mo maximum)",
            ));
        }
        images.push(runtime::attachments::ImageAttachment {
            media_type: image.media_type,
            data: image.data,
            path: std::path::PathBuf::from("collée"),
        });
    }
    app.commands
        .send(WorkerCommand::Message(body.text, images))
        .map_err(|_| api_error(StatusCode::SERVICE_UNAVAILABLE, "l'agent est arrêté"))?;
    Ok(StatusCode::ACCEPTED.into_response())
}

async fn post_permission(
    State(app): State<WebApp>,
    headers: HeaderMap,
    Json(body): Json<PermissionBody>,
) -> ApiResult {
    authorize(&app, &headers, None)?;
    let reply = match body.decision.as_str() {
        "once" => PermissionReply::Once,
        "always" => PermissionReply::Always,
        "deny" => PermissionReply::Deny(
            body.feedback
                .map(|feedback| feedback.trim().to_string())
                .filter(|feedback| !feedback.is_empty()),
        ),
        _ => return Err(api_error(StatusCode::BAD_REQUEST, "décision inconnue")),
    };
    let sender = app
        .lock_pending()
        .remove(&body.id)
        .ok_or_else(|| api_error(StatusCode::NOT_FOUND, "demande déjà traitée ou expirée"))?;
    let _ = sender.send(reply);
    Ok(StatusCode::NO_CONTENT.into_response())
}

async fn post_model(
    State(app): State<WebApp>,
    headers: HeaderMap,
    Json(body): Json<ValueBody>,
) -> ApiResult {
    authorize(&app, &headers, None)?;
    send_command(&app, WorkerCommand::SetModel(body.value))
}

async fn post_permissions(
    State(app): State<WebApp>,
    headers: HeaderMap,
    Json(body): Json<ValueBody>,
) -> ApiResult {
    authorize(&app, &headers, None)?;
    send_command(&app, WorkerCommand::SetPermissions(body.value))
}

async fn post_new(State(app): State<WebApp>, headers: HeaderMap) -> ApiResult {
    authorize(&app, &headers, None)?;
    send_command(&app, WorkerCommand::NewConversation)
}

async fn post_resume(
    State(app): State<WebApp>,
    headers: HeaderMap,
    Json(body): Json<ValueBody>,
) -> ApiResult {
    authorize(&app, &headers, None)?;
    send_command(&app, WorkerCommand::Resume(body.value))
}

fn send_command(app: &WebApp, command: WorkerCommand) -> ApiResult {
    if app.is_busy() {
        return Err(api_error(
            StatusCode::CONFLICT,
            "l'agent est déjà en train de travailler",
        ));
    }
    app.commands
        .send(command)
        .map_err(|_| api_error(StatusCode::SERVICE_UNAVAILABLE, "l'agent est arrêté"))?;
    Ok(StatusCode::ACCEPTED.into_response())
}

/// Demande l'arrêt du tour en cours (accepté même pendant un tour).
async fn post_interrupt(State(app): State<WebApp>, headers: HeaderMap) -> ApiResult {
    authorize(&app, &headers, None)?;
    if app.is_busy() {
        runtime::interrupt::request_interrupt();
    }
    Ok(StatusCode::ACCEPTED.into_response())
}

/// `?fresh=1` relit le catalogue chez `OpenRouter` (bouton « Rechercher »).
#[derive(Deserialize)]
struct ModelsQuery {
    #[serde(default)]
    fresh: Option<String>,
}

async fn get_models(
    State(app): State<WebApp>,
    headers: HeaderMap,
    Query(query): Query<ModelsQuery>,
) -> ApiResult {
    authorize(&app, &headers, None)?;
    let fresh = query.fresh.as_deref() == Some("1");
    // Catalogue et quota passent par le réseau : hors du fil asynchrone.
    let result = tokio::task::spawn_blocking(move || models::free_coding_models_json(fresh))
        .await
        .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, &error.to_string()))?;
    match result {
        Ok(value) => Ok(Json(value).into_response()),
        Err(error) => Err(api_error(StatusCode::BAD_GATEWAY, &error)),
    }
}

#[derive(Deserialize)]
struct ModelInfoQuery {
    id: String,
}

/// Fiche d'un modèle (bouton d'information du menu des modèles).
async fn get_model_info(
    State(app): State<WebApp>,
    headers: HeaderMap,
    Query(query): Query<ModelInfoQuery>,
) -> ApiResult {
    authorize(&app, &headers, None)?;
    let id = query.id.trim().to_string();
    if id.is_empty() || id.len() > 200 {
        return Err(api_error(
            StatusCode::BAD_REQUEST,
            "identifiant de modèle invalide",
        ));
    }
    let info = tokio::task::spawn_blocking(move || models::model_info_json(&id))
        .await
        .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, &error.to_string()))?;
    Ok(Json(info).into_response())
}

/// Commandes personnalisées, pour les suggestions du champ de saisie.
/// Crédits en temps réel : chaque requête de l'agent est décomptée et envoyée
/// aussitôt aux pages ouvertes ; les chiffres d'`OpenRouter` sont relus 5 s
/// après une activité (confirmation) et toutes les 20 s, seulement si une page
/// regarde.
fn spawn_credits_pump(app: WebApp) {
    if std::env::var("OPENROUTER_API_KEY").map_or(true, |key| key.trim().is_empty()) {
        return;
    }
    let _ = std::thread::Builder::new()
        .name("claw-web-credits".to_string())
        .spawn(move || {
            let mut published = u64::MAX;
            let mut last_fetch: Option<std::time::Instant> = None;
            let mut awaiting_confirmation = false;
            loop {
                std::thread::sleep(std::time::Duration::from_millis(400));
                if app.events.receiver_count() == 0 {
                    continue;
                }
                let since_fetch = last_fetch.map_or(std::time::Duration::MAX, |at| at.elapsed());
                if since_fetch >= std::time::Duration::from_secs(20)
                    || (awaiting_confirmation && since_fetch >= std::time::Duration::from_secs(5))
                {
                    let _ = crate::credits::account_status(true);
                    last_fetch = Some(std::time::Instant::now());
                    awaiting_confirmation = false;
                }
                let generation = crate::credits::generation();
                if generation != published {
                    // Une requête notée (et non une relecture) attend confirmation.
                    awaiting_confirmation |= last_fetch
                        .is_some_and(|at| at.elapsed() >= std::time::Duration::from_millis(400));
                    published = generation;
                    app.publish(WebEvent::Credits {
                        credits: crate::credits::json(false),
                    });
                }
            }
        });
}

/// Crédits `OpenRouter` (en-tête de la page, barre d'état de VS Code) ; mis en
/// cache 15 s côté serveur.
/// `?fresh=1` relit les chiffres chez `OpenRouter` (fenêtre de détail).
#[derive(Deserialize)]
struct CreditsQuery {
    #[serde(default)]
    fresh: Option<String>,
}

async fn get_credits(
    State(app): State<WebApp>,
    headers: HeaderMap,
    Query(query): Query<CreditsQuery>,
) -> ApiResult {
    authorize(&app, &headers, None)?;
    let fresh = query.fresh.as_deref() == Some("1");
    let credits = tokio::task::spawn_blocking(move || crate::credits::json(fresh))
        .await
        .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, &error.to_string()))?;
    Ok(Json(credits).into_response())
}

/// Pages connectées au flux d'événements : prouve qu'une page (navigateur,
/// panneau de VS Code) s'est réellement chargée et exécute son script.
async fn get_viewers(State(app): State<WebApp>, headers: HeaderMap) -> ApiResult {
    authorize(&app, &headers, None)?;
    Ok(Json(json!({
        "viewers": app.page_viewers.load(Ordering::SeqCst),
        "subscribers": app.events.receiver_count(),
    }))
    .into_response())
}

async fn get_commands(State(app): State<WebApp>, headers: HeaderMap) -> ApiResult {
    authorize(&app, &headers, None)?;
    let cwd = std::env::current_dir()
        .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, &error.to_string()))?;
    let commands = commands::discover_custom_commands(&cwd)
        .into_iter()
        .map(|command| {
            json!({
                "name": format!("/{}", command.qualified_name()),
                "hint": command.argument_hint,
                "description": command.description,
                "scope": command.scope,
            })
        })
        .collect::<Vec<_>>();
    Ok(Json(json!({ "commands": commands })).into_response())
}

/// `?q=texte` ne garde que les sessions qui contiennent ce texte (titre,
/// identifiant ou message).
#[derive(Deserialize)]
struct SessionsQuery {
    #[serde(default)]
    q: Option<String>,
}

async fn get_sessions(
    State(app): State<WebApp>,
    headers: HeaderMap,
    Query(query): Query<SessionsQuery>,
) -> ApiResult {
    authorize(&app, &headers, None)?;
    let failed = |error: Box<dyn std::error::Error>| {
        api_error(StatusCode::INTERNAL_SERVER_ERROR, &error.to_string())
    };
    let search = query.q.as_deref().map(str::trim).filter(|q| !q.is_empty());
    let found = match search {
        Some(text) => search_managed_sessions(text).map_err(failed)?,
        None => list_managed_sessions()
            .map_err(failed)?
            .into_iter()
            .map(|session| (session, None))
            .collect(),
    };
    let titles = session_titles();
    let sessions = found
        .into_iter()
        .map(|(session, snippet)| {
            json!({
                "id": session.id,
                "modified": session.modified_epoch_secs,
                "messages": session.message_count,
                // Titre donné par l'utilisateur, s'il y en a un.
                "title": titles.get(&session.id),
                // Début de la première demande, pour reconnaître la session.
                "preview": crate::session_preview(&session.path),
                // Extrait du message où le texte cherché a été trouvé.
                "snippet": snippet,
            })
        })
        .collect::<Vec<_>>();
    Ok(Json(json!({ "sessions": sessions })).into_response())
}

#[derive(Deserialize)]
struct RenameBody {
    id: String,
    #[serde(default)]
    title: String,
}

/// Donne un titre à une session (vide : titre automatique).
async fn post_session_rename(
    State(app): State<WebApp>,
    headers: HeaderMap,
    Json(body): Json<RenameBody>,
) -> ApiResult {
    authorize(&app, &headers, None)?;
    let title = rename_managed_session(&body.id, &body.title)
        .map_err(|error| api_error(StatusCode::BAD_REQUEST, &error))?;
    Ok(Json(json!({ "title": title })).into_response())
}

#[derive(Deserialize)]
struct FavoriteBody {
    value: String,
    on: bool,
}

/// Épingle un modèle en haut du menu, ou le retire des favoris.
async fn post_model_favorite(
    State(app): State<WebApp>,
    headers: HeaderMap,
    Json(body): Json<FavoriteBody>,
) -> ApiResult {
    authorize(&app, &headers, None)?;
    let favorites = models::set_favorite_model(&body.value, body.on)
        .map_err(|error| api_error(StatusCode::BAD_REQUEST, &error))?;
    Ok(Json(json!({ "favorites": favorites })).into_response())
}

/// Efface une session précédente (jamais celle en cours).
async fn post_session_delete(
    State(app): State<WebApp>,
    headers: HeaderMap,
    Json(body): Json<ValueBody>,
) -> ApiResult {
    authorize(&app, &headers, None)?;
    if body.value == app.current_session_id() {
        return Err(api_error(
            StatusCode::CONFLICT,
            "la session en cours ne peut pas être effacée",
        ));
    }
    delete_managed_session(&body.value)
        .map_err(|error| api_error(StatusCode::BAD_REQUEST, &error))?;
    Ok(Json(json!({ "deleted": 1 })).into_response())
}

/// Efface toutes les sessions précédentes du projet ; celle en cours est gardée.
async fn post_sessions_clear(State(app): State<WebApp>, headers: HeaderMap) -> ApiResult {
    authorize(&app, &headers, None)?;
    let deleted = delete_other_sessions(&app.current_session_id())
        .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, &error))?;
    Ok(Json(json!({ "deleted": deleted })).into_response())
}

fn router(app: WebApp) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/api/events", get(events))
        .route(
            "/api/message",
            post(post_message).layer(DefaultBodyLimit::max(48 * 1024 * 1024)),
        )
        .route("/api/permission", post(post_permission))
        .route("/api/model", post(post_model).get(get_model_info))
        .route("/api/permissions", post(post_permissions))
        .route("/api/new", post(post_new))
        .route("/api/resume", post(post_resume))
        .route("/api/interrupt", post(post_interrupt))
        .route("/api/models", get(get_models))
        .route("/api/sessions", get(get_sessions))
        .route("/api/sessions/delete", post(post_session_delete))
        .route("/api/sessions/rename", post(post_session_rename))
        .route("/api/models/favorite", post(post_model_favorite))
        .route("/api/sessions/clear", post(post_sessions_clear))
        .route("/api/commands", get(get_commands))
        .route("/api/viewers", get(get_viewers))
        .route("/api/credits", get(get_credits))
        .with_state(app)
}

fn allowed_hosts(host: &str, port: u16) -> Vec<String> {
    let mut hosts = vec![
        format!("127.0.0.1:{port}"),
        format!("localhost:{port}"),
        format!("[::1]:{port}"),
    ];
    let custom = format!("{host}:{port}");
    if !hosts.contains(&custom) {
        hosts.push(custom);
    }
    hosts
}

/// Lance l'interface web et bloque jusqu'à l'arrêt (Ctrl+C).
pub(crate) fn serve(options: ServeOptions) -> Result<(), Box<dyn std::error::Error>> {
    let token = runtime::generate_state()?;
    let (events, _) = broadcast::channel(BROADCAST_CAPACITY);
    let (commands, receiver) = mpsc::channel();
    let app = WebApp {
        token: Arc::new(token.clone()),
        allowed_hosts: Arc::new(allowed_hosts(&options.host, options.port)),
        events,
        commands,
        view: Arc::new(Mutex::new(SharedView::default())),
        pending: Arc::new(Mutex::new(HashMap::new())),
        next_permission_id: Arc::new(AtomicU64::new(1)),
        page_viewers: Arc::new(AtomicU64::new(0)),
    };

    spawn_credits_pump(app.clone());

    // Construction du runtime dans le thread de l'agent (MCP, plugins…).
    let worker_app = app.clone();
    let (ready_sender, ready_receiver) = mpsc::channel::<Result<(), String>>();
    std::thread::Builder::new()
        .name("claw-web-agent".to_string())
        .spawn(move || {
            let setup = (|| -> Result<Worker, String> {
                let system_prompt = build_system_prompt().map_err(|error| error.to_string())?;
                let session = create_managed_session_handle().map_err(|error| error.to_string())?;
                let runtime = build_web_runtime(
                    &worker_app,
                    &options.model,
                    options.permission_mode,
                    options.allowed_tools.as_ref(),
                    &system_prompt,
                    Session::new(),
                )?;
                Ok(Worker {
                    app: worker_app,
                    model: options.model,
                    permission_mode: options.permission_mode,
                    allowed_tools: options.allowed_tools,
                    system_prompt,
                    session,
                    runtime,
                })
            })();
            match setup {
                Ok(worker) => {
                    let _ = ready_sender.send(Ok(()));
                    worker.run(&receiver);
                }
                Err(error) => {
                    let _ = ready_sender.send(Err(error));
                }
            }
        })?;
    ready_receiver
        .recv()
        .map_err(|_| "le thread de l'agent s'est arrêté au démarrage")??;

    let address = format!("{}:{}", options.host, options.port);
    let url = format!(
        "http://{}:{}/?token={token}",
        display_host(&options.host),
        options.port
    );
    let tokio_runtime = tokio::runtime::Runtime::new()?;
    tokio_runtime.block_on(async move {
        let listener = tokio::net::TcpListener::bind(&address)
            .await
            .map_err(|error| format!("impossible d'écouter sur {address} : {error}"))?;
        println!("Claw · interface web");
        println!("  Adresse          {url}");
        println!("  Dossier          {}", std::env::current_dir().map(|cwd| cwd.display().to_string()).unwrap_or_default());
        if !matches!(options.host.as_str(), "127.0.0.1" | "localhost" | "::1") {
            println!("  Attention        accessible depuis le réseau : ne partagez pas l'adresse (elle contient le jeton)");
        }
        println!("  Arrêt            Ctrl+C");
        if options.open_browser {
            if let Err(error) = open_browser(&url) {
                println!("  Navigateur       ouverture impossible ({error}) : copiez l'adresse ci-dessus");
            }
        }
        axum::serve(listener, router(app))
            .with_graceful_shutdown(async {
                let _ = tokio::signal::ctrl_c().await;
            })
            .await
            .map_err(|error| error.to_string())
    })?;
    Ok(())
}

fn display_host(host: &str) -> &str {
    if host == "0.0.0.0" || host == "::" {
        "127.0.0.1"
    } else {
        host
    }
}

#[cfg(test)]
mod tests {
    use super::{
        allowed_hosts, constant_time_eq, transcript_from_messages, truncate_chars, WebEvent,
    };
    use runtime::ConversationMessage;

    #[test]
    fn compares_tokens_in_constant_time() {
        assert!(constant_time_eq(b"abc", b"abc"));
        assert!(!constant_time_eq(b"abc", b"abd"));
        assert!(!constant_time_eq(b"abc", b"abcd"));
    }

    #[test]
    fn allows_only_loopback_hosts_and_the_configured_one() {
        let hosts = allowed_hosts("127.0.0.1", 8787);
        assert!(hosts.contains(&"localhost:8787".to_string()));
        assert!(!hosts.iter().any(|host| host.starts_with("evil")));
        assert_eq!(
            allowed_hosts("192.168.1.10", 9000)
                .last()
                .map(String::as_str),
            Some("192.168.1.10:9000")
        );
    }

    #[test]
    fn truncates_long_tool_outputs() {
        assert_eq!(truncate_chars("court", 10), "court");
        assert!(truncate_chars(&"x".repeat(50), 10).ends_with("tronquée pour l'affichage)"));
    }

    #[test]
    fn rebuilds_transcript_from_saved_messages() {
        let events = transcript_from_messages(&[
            ConversationMessage::user_text("bonjour"),
            ConversationMessage::tool_result("t1", "bash", "ok", false),
        ]);
        assert!(matches!(&events[0], WebEvent::UserMessage { text } if text == "bonjour"));
        assert!(matches!(&events[1], WebEvent::ToolResult { name, .. } if name == "bash"));
    }
}
