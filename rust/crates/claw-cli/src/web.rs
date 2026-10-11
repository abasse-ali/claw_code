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
/// Début du résultat rendu au modèle quand personne ne répond à une demande
/// d'autorisation (la page l'affiche comme « sans réponse »).
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
        /// Heure d'envoi (millisecondes Unix) ; absente pour une session reprise.
        #[serde(skip_serializing_if = "Option::is_none")]
        at: Option<u64>,
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
        /// Modification proposée (`write_file`, `edit_file`) : l'éditeur relié
        /// l'affiche dans un diff avant que l'utilisateur ne réponde.
        #[serde(skip_serializing_if = "Option::is_none")]
        proposal: Option<EditProposal>,
    },
    PermissionResolved {
        id: u64,
        decision: String,
    },
    Info {
        text: String,
    },
    /// Avis du runtime (vérification, relecture, escalade…), affiché comme une
    /// ligne de la frise et non comme un rapport.
    Notice {
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
    /// Le modèle de la session ne répond plus, ou l'agent n'avance plus :
    /// l'utilisateur fait réessayer le même modèle, en choisit un autre, ou
    /// arrête le tour (`POST /api/model-question`).
    ModelQuestion {
        id: u64,
        model: String,
        reason: String,
        stuck: bool,
        suggestions: Vec<String>,
        /// Sans réponse dans ce délai (secondes), le même modèle reprend de
        /// lui-même ; absent : la question attend sa réponse.
        #[serde(skip_serializing_if = "Option::is_none")]
        auto_retry_secs: Option<u64>,
        /// Essai automatique à venir et nombre d'essais prévus.
        #[serde(skip_serializing_if = "Option::is_none")]
        attempt: Option<u32>,
        #[serde(skip_serializing_if = "Option::is_none")]
        attempts: Option<u32>,
        /// Faux quand le même essai échouerait de la même façon (clé refusée,
        /// modèle introuvable) : la page propose d'abord de changer de modèle.
        retry_useful: bool,
    },
    ModelQuestionResolved {
        id: u64,
        decision: String,
    },
    /// L'agent pose des questions à choix (outil `AskUserQuestion`) : la
    /// page les présente, l'utilisateur répond par `POST /api/question`.
    Question {
        id: u64,
        questions: Vec<tools::UserQuestion>,
    },
    QuestionResolved {
        id: u64,
        answers: Vec<tools::UserAnswer>,
        /// L'utilisateur a écarté les questions, ou le tour a été interrompu.
        dismissed: bool,
    },
    /// Le modèle vient de répondre sans appeler d'outil : sa réponse finale,
    /// sauf si un contrôle de Claw le renvoie au travail.
    AnswerDrafted,
    /// Un contrôle de Claw renvoie le modèle au travail : la réponse qu'il
    /// venait de donner est provisoire, la page la replie.
    AnswerSuperseded,
    /// La réponse en cours est abandonnée (flux coupé) et redemandée au
    /// modèle : les appels d'outils qu'elle annonçait ne seront pas exécutés,
    /// la page retire leurs étapes.
    ReplyDiscarded,
    /// Demande à l'éditeur relié : quels problèmes voit-il dans ces fichiers
    /// (tous ceux de l'espace de travail si la liste est vide) ? Il répond
    /// par `POST /api/editor/diagnostics` avec le même identifiant.
    EditorSync {
        id: u64,
        paths: Vec<String>,
    },
    Reset,
}

/// Fichier tel qu'il serait après une modification demandée par l'agent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct EditProposal {
    /// Chemin absolu.
    path: String,
    /// Le fichier existe déjà (sinon il serait créé).
    exists: bool,
    after: String,
}

/// Au-delà, la modification n'est pas jointe à la demande de permission.
const MAX_PROPOSAL_BYTES: usize = 512 * 1024;

/// Contenu qu'aurait le fichier si l'appel `tool` était autorisé, calculé
/// comme le ferait l'outil, sans rien écrire.
fn edit_proposal(tool: &str, input: &str) -> Option<EditProposal> {
    let value = serde_json::from_str::<Value>(input).ok()?;
    let text = |key: &str| value.get(key).and_then(Value::as_str);
    let preview = match tool {
        "write_file" => runtime::preview_write(text("path")?, text("content")?).ok()?,
        "edit_file" => runtime::preview_edit(
            text("path")?,
            text("old_string")?,
            text("new_string")?,
            value
                .get("replace_all")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        )
        .ok()?,
        _ => return None,
    };
    (preview.updated.len() <= MAX_PROPOSAL_BYTES).then(|| EditProposal {
        path: preview.path,
        exists: preview.original.is_some(),
        after: preview.updated,
    })
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
    /// Taille estimée de ce que le modèle relit à chaque requête.
    context_tokens: u64,
    cost: Option<String>,
    /// Déroulé ultracode actif pour la session.
    ultracode: bool,
    /// Mode équipe actif pour la session.
    team: bool,
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

/// Réponse à la question sur le modèle.
enum ModelReply {
    Retry,
    Switch(String),
    Stop,
    /// L'utilisateur choisit un modèle : l'essai automatique attend sa réponse.
    Hold,
}

enum WorkerCommand {
    Message(String, Vec<runtime::attachments::ImageAttachment>),
    SetModel(String),
    SetPermissions(String),
    NewConversation,
    Resume(String),
}

/// Demandes posées dans la page et en attente de réponse, par identifiant.
type Pending<T> = Arc<Mutex<HashMap<u64, mpsc::Sender<T>>>>;

#[derive(Clone)]
struct WebApp {
    token: Arc<String>,
    allowed_hosts: Arc<Vec<String>>,
    events: broadcast::Sender<WebEvent>,
    commands: mpsc::Sender<WorkerCommand>,
    view: Arc<Mutex<SharedView>>,
    pending: Pending<PermissionReply>,
    /// Questions sur le modèle en attente de réponse.
    pending_models: Pending<ModelReply>,
    /// Questions de l'agent en attente : `None` quand elles sont écartées.
    pending_questions: Pending<Option<Vec<tools::UserAnswer>>>,
    /// Modèle choisi par l'utilisateur pendant un tour : il devient celui de
    /// la session à la fin du tour.
    chosen_model: Arc<Mutex<Option<String>>>,
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
            // Pas rejoués à la reconnexion : la page relit /api/credits, et une
            // demande à l'éditeur ne vaut que sur le moment.
            WebEvent::TurnDone | WebEvent::Credits { .. } | WebEvent::EditorSync { .. } => {}
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
        self.app.publish(WebEvent::Notice {
            text: text.to_string(),
        });
    }

    fn answer_drafted(&self) {
        self.app.publish(WebEvent::AnswerDrafted);
    }

    fn answer_superseded(&self) {
        self.app.publish(WebEvent::AnswerSuperseded);
    }

    fn reply_discarded(&self) {
        self.app.publish(WebEvent::ReplyDiscarded);
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
            proposal: edit_proposal(&request.tool_name, &request.input),
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
            // Sans cette consigne, un modèle redemandait la même autorisation
            // plusieurs fois de suite, un quart d'heure d'attente à chaque fois,
            // en croyant la commande bloquée. Les demandes suivantes du tour ne
            // sont plus posées (runtime).
            Err(_) => (
                runtime::unanswered_permission(&request.tool_name),
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

/// Pose dans la page la question « réessayer, changer de modèle ou arrêter »
/// et attend la réponse (15 min au plus, ou jusqu'à l'interruption du tour).
struct WebModelPrompter {
    app: WebApp,
}

impl crate::model_switch::ModelSwitchPrompter for WebModelPrompter {
    fn ask(
        &self,
        question: &crate::model_switch::ModelQuestion,
    ) -> crate::model_switch::ModelDecision {
        use crate::model_switch::ModelDecision;
        let id = self.app.next_permission_id.fetch_add(1, Ordering::Relaxed);
        let (sender, mut receiver) = mpsc::channel();
        self.app
            .pending_models
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(id, sender);
        self.app.publish(WebEvent::ModelQuestion {
            id,
            model: question.model.clone(),
            reason: question.reason.clone(),
            stuck: question.stuck,
            suggestions: question.suggestions.clone(),
            auto_retry_secs: question.auto_retry.map(|delay| delay.as_secs()),
            attempt: question.attempt.map(|(attempt, _)| attempt),
            attempts: question.attempt.map(|(_, attempts)| attempts),
            retry_useful: question.retry_useful,
        });
        // Essai automatique : sans réponse dans le délai, le même modèle
        // reprend. Si l'utilisateur ouvre le choix d'un modèle, on l'attend.
        let mut automatic = question.auto_retry.is_some();
        let mut deadline =
            std::time::Instant::now() + question.auto_retry.unwrap_or(PERMISSION_TIMEOUT);
        let reply = loop {
            match receiver.recv_timeout(Duration::from_millis(200)) {
                Ok(ModelReply::Hold) => {
                    automatic = false;
                    deadline = std::time::Instant::now() + PERMISSION_TIMEOUT;
                    // La réponse suivante arrive par le même canal.
                    let (sender, next) = mpsc::channel();
                    self.app
                        .pending_models
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .insert(id, sender);
                    receiver = next;
                }
                Err(mpsc::RecvTimeoutError::Timeout)
                    if !runtime::interrupt::interrupt_requested()
                        && std::time::Instant::now() < deadline => {}
                other => break other,
            }
        };
        self.app
            .pending_models
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&id);
        let (decision, label) = match reply {
            Ok(ModelReply::Retry) if question.stuck => {
                (ModelDecision::Retry, "le même modèle continue".to_string())
            }
            Ok(ModelReply::Retry) => (
                ModelDecision::Retry,
                "nouvel essai avec le même modèle".to_string(),
            ),
            Ok(ModelReply::Switch(model)) => {
                let model = resolve_model_alias(model.trim());
                *self
                    .app
                    .chosen_model
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(model.clone());
                let label = format!("modèle choisi : {model}");
                (ModelDecision::Switch(model), label)
            }
            Ok(ModelReply::Stop | ModelReply::Hold) => {
                (ModelDecision::Stop, "tour arrêté".to_string())
            }
            Err(_) if runtime::interrupt::interrupt_requested() => {
                (ModelDecision::Stop, "tour interrompu".to_string())
            }
            // Personne n'a répondu : c'était prévu, le même modèle reprend.
            Err(_) if automatic && question.stuck => (
                ModelDecision::Retry,
                "sans réponse, le même modèle continue".to_string(),
            ),
            Err(_) if automatic => (ModelDecision::Retry, "nouvel essai automatique".to_string()),
            Err(_) => (
                ModelDecision::Stop,
                "pas de réponse : tour arrêté".to_string(),
            ),
        };
        self.app.publish(WebEvent::ModelQuestionResolved {
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
        model_prompter: Some(Arc::new(WebModelPrompter { app: app.clone() })),
        question_asker: Some(web_question_asker(app)),
    }
}

/// Pose les questions de l'agent dans la page et attend les réponses, sans
/// limite de temps : seule l'interruption du tour y met fin.
fn web_question_asker(app: &WebApp) -> tools::SharedQuestionAsker {
    let app = app.clone();
    Arc::new(move |questions: &[tools::UserQuestion]| {
        let id = app.next_permission_id.fetch_add(1, Ordering::Relaxed);
        let (sender, receiver) = mpsc::channel();
        app.pending_questions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(id, sender);
        app.publish(WebEvent::Question {
            id,
            questions: questions.to_vec(),
        });
        let reply = loop {
            match receiver.recv_timeout(Duration::from_millis(300)) {
                Err(mpsc::RecvTimeoutError::Timeout)
                    if !runtime::interrupt::interrupt_requested() => {}
                other => break other,
            }
        };
        app.pending_questions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&id);
        let answers = reply.ok().flatten();
        app.publish(WebEvent::QuestionResolved {
            id,
            answers: answers.clone().unwrap_or_default(),
            dismissed: answers.is_none(),
        });
        answers.ok_or_else(|| crate::questions::DISMISSED.to_string())
    })
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
            context_tokens: u64::try_from(self.runtime.estimated_tokens()).unwrap_or(u64::MAX),
            cost: models::estimated_cost(&self.model, usage),
            ultracode: runtime::workflow::ultracode(),
            team: runtime::team::team(),
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
                    let at = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .ok()
                        .and_then(|elapsed| u64::try_from(elapsed.as_millis()).ok());
                    self.app.publish(WebEvent::UserMessage {
                        text: text.clone(),
                        at,
                    });
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
        // Modèle choisi par l'utilisateur pendant le tour : il reste celui de la session.
        let chosen = self
            .app
            .chosen_model
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        if let Some(model) = chosen.filter(|model| *model != self.model) {
            if let Err(error) = self.set_model(&model) {
                self.info(format!("Changement de modèle impossible : {error}"));
            }
        }
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
                // `/think ultracode` : l'interrupteur de la page suit.
                self.publish_status(false);
            }
            Some(SlashCommand::Team { args }) => {
                self.info(crate::apply_team_command(args.as_deref(), &self.model));
                self.publish_status(false);
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
                "{}\n\nDans l'interface web : /review /audit /archi /adr /fix /undo /checkpoints /hooks /think /equipe /bashes /models /mcp /model /permissions /cost /compact /clear.\nLes autres commandes s'utilisent dans le terminal.\n\n{}",
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
        // Réponse sans appel d'outil : finale, sauf contrôle de Claw ensuite.
        let drafted = message.role == MessageRole::Assistant
            && !message
                .blocks
                .iter()
                .any(|block| matches!(block, ContentBlock::ToolUse { .. }));
        for block in &message.blocks {
            let event = match (block, message.role) {
                (ContentBlock::Text { text }, MessageRole::Assistant) => {
                    WebEvent::AssistantText { text: text.clone() }
                }
                // Dossier ultracode joint à la demande : pour le modèle, la
                // page le signale d'une ligne.
                (ContentBlock::Text { text }, _)
                    if text.starts_with(runtime::workflow::BRIEF_PREFIX) =>
                {
                    WebEvent::Notice {
                        text: "Ultracode : dossier préparé par un agent au contexte vierge"
                            .to_string(),
                    }
                }
                // Contexte ajouté par les hooks, guide de Claw joint à la
                // demande : destinés au modèle seulement.
                (ContentBlock::Text { text }, _)
                    if text.starts_with(runtime::HOOK_CONTEXT_PREFIX)
                        || text.starts_with(runtime::harness::GUIDE_PREFIX) =>
                {
                    continue;
                }
                (ContentBlock::Text { text }, _) if text.starts_with(runtime::STOP_HOOK_PREFIX) => {
                    events.push(WebEvent::AnswerSuperseded);
                    WebEvent::Info { text: text.clone() }
                }
                // Contrôle de Claw adressé au modèle : pas un message de
                // l'utilisateur ; la réponse qui le précède était provisoire.
                (ContentBlock::Text { text }, MessageRole::User)
                    if text.starts_with(runtime::harness::CLAW_NOTE_PREFIX) =>
                {
                    WebEvent::AnswerSuperseded
                }
                (ContentBlock::Text { text }, _) => WebEvent::UserMessage {
                    text: text.clone(),
                    at: None,
                },
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
            };
            events.push(event);
        }
        if drafted {
            events.push(WebEvent::AnswerDrafted);
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
    /// Nom de l'éditeur qui s'abonne (`VS Code`) : il répond aux demandes
    /// de diagnostics tant que son flux reste ouvert.
    #[serde(default)]
    editor: Option<String>,
}

#[derive(Deserialize)]
struct QuestionBody {
    id: u64,
    /// Une réponse par question ; absent avec `dismissed`.
    #[serde(default)]
    answers: Vec<tools::UserAnswer>,
    /// L'utilisateur ne souhaite pas répondre.
    #[serde(default)]
    dismissed: bool,
}

#[derive(Deserialize)]
struct ModelQuestionBody {
    id: u64,
    /// `retry`, `switch` (avec `model`), `stop`, ou `hold` (l'utilisateur
    /// choisit un modèle : l'essai automatique l'attend).
    decision: String,
    #[serde(default)]
    model: Option<String>,
}

#[derive(Deserialize)]
struct EditorDiagnosticsBody {
    id: u64,
    #[serde(default)]
    files: Vec<runtime::editor::FileDiagnostics>,
}

#[derive(Deserialize)]
struct ChangesQuery {
    /// `turn` (le dernier tour, par défaut) ou `session`.
    #[serde(default)]
    scope: Option<String>,
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
    // Un éditeur relié le reste tant que son flux d'événements est ouvert.
    let editor = query
        .editor
        .as_deref()
        .map(str::trim)
        .filter(|name| !name.is_empty() && name.len() <= 40)
        .map(runtime::editor::connect);
    let stream = stream! {
        let _guard = guard;
        let _editor = editor;
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

/// Réponses de l'utilisateur aux questions de l'agent.
async fn post_question(
    State(app): State<WebApp>,
    headers: HeaderMap,
    Json(body): Json<QuestionBody>,
) -> ApiResult {
    authorize(&app, &headers, None)?;
    let answers: Vec<tools::UserAnswer> = body
        .answers
        .into_iter()
        .filter(|answer| !answer.answer.trim().is_empty())
        .collect();
    if !body.dismissed && answers.is_empty() {
        return Err(api_error(StatusCode::BAD_REQUEST, "aucune réponse"));
    }
    let sender = app
        .pending_questions
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .remove(&body.id)
        .ok_or_else(|| api_error(StatusCode::NOT_FOUND, "question déjà traitée ou expirée"))?;
    let _ = sender.send((!body.dismissed).then_some(answers));
    Ok(StatusCode::NO_CONTENT.into_response())
}

/// Réponse de l'utilisateur à la question sur le modèle.
async fn post_model_question(
    State(app): State<WebApp>,
    headers: HeaderMap,
    Json(body): Json<ModelQuestionBody>,
) -> ApiResult {
    authorize(&app, &headers, None)?;
    let reply = match body.decision.as_str() {
        "retry" => ModelReply::Retry,
        "stop" => ModelReply::Stop,
        "hold" => ModelReply::Hold,
        "switch" => {
            let model = body
                .model
                .map(|model| model.trim().to_string())
                .filter(|model| !model.is_empty())
                .ok_or_else(|| api_error(StatusCode::BAD_REQUEST, "modèle manquant"))?;
            ModelReply::Switch(model)
        }
        _ => return Err(api_error(StatusCode::BAD_REQUEST, "décision inconnue")),
    };
    let sender = app
        .pending_models
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .remove(&body.id)
        .ok_or_else(|| api_error(StatusCode::NOT_FOUND, "question déjà traitée ou expirée"))?;
    let _ = sender.send(reply);
    Ok(StatusCode::NO_CONTENT.into_response())
}

/// Réponse de l'éditeur relié à une demande de diagnostics.
async fn post_editor_diagnostics(
    State(app): State<WebApp>,
    headers: HeaderMap,
    Json(body): Json<EditorDiagnosticsBody>,
) -> ApiResult {
    authorize(&app, &headers, None)?;
    if runtime::editor::deliver(body.id, body.files) {
        Ok(StatusCode::NO_CONTENT.into_response())
    } else {
        Err(api_error(
            StatusCode::NOT_FOUND,
            "demande déjà traitée ou expirée",
        ))
    }
}

/// Au-delà, un fichier modifié est listé sans son contenu d'origine.
const MAX_CHANGE_BYTES: usize = 2 * 1024 * 1024;

/// Fichiers modifiés par les outils de l'agent, avec leur contenu d'avant :
/// ceux du dernier tour, ou de toute la session (le plus ancien état l'emporte).
/// Un fichier revenu à son contenu d'origine n'est pas listé.
fn changed_files(turns: &[runtime::checkpoint::TurnCheckpoint], whole_session: bool) -> Value {
    let turns = if whole_session {
        turns
    } else {
        // Le dernier tour qui a modifié quelque chose.
        turns
            .iter()
            .rposition(|turn| !turn.files.is_empty())
            .map_or(&turns[..0], |index| &turns[index..=index])
    };
    let mut seen = std::collections::BTreeSet::new();
    let mut files = Vec::new();
    for snapshot in turns.iter().flat_map(|turn| &turn.files) {
        if !seen.insert(snapshot.path.clone()) {
            continue;
        }
        let current = std::fs::read_to_string(&snapshot.path).ok();
        if !snapshot.binary && current == snapshot.content {
            continue;
        }
        let too_large = snapshot
            .content
            .as_ref()
            .is_some_and(|content| content.len() > MAX_CHANGE_BYTES);
        files.push(json!({
            "path": snapshot.path.to_string_lossy(),
            "existed": snapshot.content.is_some() || snapshot.binary,
            "exists": snapshot.path.exists(),
            "comparable": !snapshot.binary && !too_large,
            "before": if snapshot.binary || too_large { None } else { snapshot.content.clone() },
        }));
    }
    json!({ "scope": if whole_session { "session" } else { "turn" }, "files": files })
}

async fn get_changes(
    State(app): State<WebApp>,
    headers: HeaderMap,
    Query(query): Query<ChangesQuery>,
) -> ApiResult {
    authorize(&app, &headers, None)?;
    let whole_session = query.scope.as_deref() == Some("session");
    Ok(Json(changed_files(&runtime::checkpoint::list(), whole_session)).into_response())
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

#[derive(Deserialize)]
struct UltracodeBody {
    on: bool,
}

/// Interrupteur « Ultracode » de la page : déroulé pour la session (comme
/// `/think ultracode`), pris en compte dès la demande suivante.
async fn post_ultracode(
    State(app): State<WebApp>,
    headers: HeaderMap,
    Json(body): Json<UltracodeBody>,
) -> ApiResult {
    authorize(&app, &headers, None)?;
    let text = crate::apply_ultracode_command(Some(if body.on { "on" } else { "off" }));
    Ok(Json(json!({ "ultracode": runtime::workflow::ultracode(), "text": text })).into_response())
}

#[derive(Deserialize)]
struct TeamBody {
    on: bool,
}

/// Interrupteur « Équipe » de la page : mode équipe pour la session (comme
/// `/equipe`), pris en compte dès l'étape suivante.
async fn post_team(
    State(app): State<WebApp>,
    headers: HeaderMap,
    Json(body): Json<TeamBody>,
) -> ApiResult {
    authorize(&app, &headers, None)?;
    if body.on && !runtime::harness::enabled(runtime::harness::Aid::Team) {
        return Err(api_error(
            StatusCode::CONFLICT,
            "le mode équipe est coupé par CLAW_TEAM=0 ou CLAW_HARNESS=0",
        ));
    }
    runtime::team::set_team(body.on);
    Ok(Json(json!({ "team": runtime::team::team() })).into_response())
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
        .route("/api/model-question", post(post_model_question))
        .route("/api/question", post(post_question))
        .route("/api/model", post(post_model).get(get_model_info))
        .route("/api/permissions", post(post_permissions))
        .route("/api/new", post(post_new))
        .route("/api/resume", post(post_resume))
        .route("/api/interrupt", post(post_interrupt))
        .route("/api/ultracode", post(post_ultracode))
        .route("/api/team", post(post_team))
        .route("/api/models", get(get_models))
        .route("/api/sessions", get(get_sessions))
        .route("/api/sessions/delete", post(post_session_delete))
        .route("/api/sessions/rename", post(post_session_rename))
        .route("/api/models/favorite", post(post_model_favorite))
        .route("/api/sessions/clear", post(post_sessions_clear))
        .route("/api/commands", get(get_commands))
        .route("/api/viewers", get(get_viewers))
        .route("/api/editor/diagnostics", post(post_editor_diagnostics))
        .route("/api/changes", get(get_changes))
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
        pending_models: Arc::new(Mutex::new(HashMap::new())),
        pending_questions: Arc::new(Mutex::new(HashMap::new())),
        chosen_model: Arc::new(Mutex::new(None)),
        next_permission_id: Arc::new(AtomicU64::new(1)),
        page_viewers: Arc::new(AtomicU64::new(0)),
    };

    spawn_credits_pump(app.clone());

    // Les demandes de Claw à l'éditeur relié partent par le flux d'événements.
    let editor_app = app.clone();
    runtime::editor::set_requester(Arc::new(move |id, paths| {
        editor_app.publish(WebEvent::EditorSync {
            id,
            paths: paths.to_vec(),
        });
    }));

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
    fn a_permission_request_carries_the_proposed_file_content() {
        let directory = std::env::temp_dir().join(format!(
            "claw-proposal-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("horloge")
                .as_nanos()
        ));
        std::fs::create_dir_all(&directory).expect("dossier");
        let existing = directory.join("existant.txt");
        std::fs::write(&existing, "un\ndeux\ntrois\n").expect("fichier");
        let path = existing.to_string_lossy().to_string();

        // edit_file : le fichier entier tel qu'il serait, sans y toucher.
        let input = serde_json::json!({ "path": path, "old_string": "deux", "new_string": "DEUX" });
        let proposal = super::edit_proposal("edit_file", &input.to_string()).expect("aperçu");
        assert!(proposal.exists);
        assert_eq!(proposal.after, "un\nDEUX\ntrois\n");
        assert!(std::path::Path::new(&proposal.path).is_absolute());
        assert_eq!(
            std::fs::read_to_string(&existing).expect("lecture"),
            "un\ndeux\ntrois\n"
        );

        // write_file sur un fichier qui n'existe pas : une création.
        let new_path = directory.join("nouveau.txt").to_string_lossy().to_string();
        let input = serde_json::json!({ "path": new_path, "content": "bonjour\n" });
        let proposal = super::edit_proposal("write_file", &input.to_string()).expect("aperçu");
        assert!(!proposal.exists);
        assert_eq!(proposal.after, "bonjour\n");
        assert!(!directory.join("nouveau.txt").exists());

        // Texte introuvable ou autre outil : pas d'aperçu, la demande reste posée.
        let input = serde_json::json!({ "path": path, "old_string": "absent", "new_string": "x" });
        assert_eq!(super::edit_proposal("edit_file", &input.to_string()), None);
        assert_eq!(super::edit_proposal("bash", r#"{"command":"ls"}"#), None);
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn lists_the_files_changed_by_the_last_turn_or_the_session() {
        use runtime::checkpoint::{FileSnapshot, TurnCheckpoint};
        let directory = std::env::temp_dir().join(format!(
            "claw-changes-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("horloge")
                .as_nanos()
        ));
        std::fs::create_dir_all(&directory).expect("dossier");
        let write = |name: &str, content: &str| {
            let path = directory.join(name);
            std::fs::write(&path, content).expect("fichier");
            path
        };
        let modified = write("modifie.txt", "après\n");
        let created = write("cree.txt", "nouveau\n");
        let restored = write("retabli.txt", "pareil\n");
        let snapshot = |path: &std::path::Path, content: Option<&str>| FileSnapshot {
            path: path.to_path_buf(),
            content: content.map(ToOwned::to_owned),
            binary: false,
        };
        let turn = |index: usize, files: Vec<FileSnapshot>| TurnCheckpoint {
            message_index: index,
            request: format!("tour {index}"),
            created_at: 0,
            files,
        };
        let turns = vec![
            turn(
                0,
                vec![
                    snapshot(&modified, Some("au début\n")),
                    snapshot(&restored, Some("pareil\n")),
                ],
            ),
            turn(
                2,
                vec![
                    snapshot(&modified, Some("entre deux\n")),
                    snapshot(&created, None),
                ],
            ),
            // Un tour sans modification ne cache pas le précédent.
            turn(4, Vec::new()),
        ];

        let last = super::changed_files(&turns, false);
        assert_eq!(last["scope"], "turn");
        let files = last["files"].as_array().expect("fichiers");
        assert_eq!(files.len(), 2);
        assert_eq!(files[0]["before"], "entre deux\n");
        assert_eq!(files[1]["existed"], false);
        assert_eq!(files[1]["before"], serde_json::Value::Null);
        assert_eq!(files[1]["comparable"], true);

        // Toute la session : l'état le plus ancien, sans le fichier revenu à l'identique.
        let session = super::changed_files(&turns, true);
        let files = session["files"].as_array().expect("fichiers");
        assert_eq!(files.len(), 2);
        assert_eq!(files[0]["before"], "au début\n");
        assert!(files.iter().all(|file| !file["path"]
            .as_str()
            .expect("chemin")
            .ends_with("retabli.txt")));
        assert_eq!(
            super::changed_files(&[], false)["files"],
            serde_json::json!([])
        );
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn truncates_long_tool_outputs() {
        assert_eq!(truncate_chars("court", 10), "court");
        assert!(truncate_chars(&"x".repeat(50), 10).ends_with("tronquée pour l'affichage)"));
    }

    #[test]
    fn a_replayed_session_folds_answers_superseded_by_a_check_of_claw() {
        use runtime::ContentBlock;
        let text = |text: &str| ContentBlock::Text {
            text: text.to_string(),
        };
        let events = transcript_from_messages(&[
            ConversationMessage::user_text("écris le site"),
            // Texte accompagné d'un appel d'outil : pas une réponse finale.
            ConversationMessage::assistant(vec![
                text("Je commence."),
                ContentBlock::ToolUse {
                    id: "t1".to_string(),
                    name: "write_file".to_string(),
                    input: "{}".to_string(),
                },
            ]),
            ConversationMessage::tool_result("t1", "write_file", "ok", false),
            ConversationMessage::assistant(vec![text("Projet terminé.")]),
            ConversationMessage::user_text(
                "[Claw] Before you finish, check your work against every requirement.",
            ),
            ConversationMessage::assistant(vec![text("Voici comment lancer le projet.")]),
        ]);
        let kinds: Vec<String> = events
            .iter()
            .map(|event| {
                serde_json::to_value(event).expect("événement")["type"]
                    .as_str()
                    .expect("type")
                    .to_string()
            })
            .collect();
        assert_eq!(
            kinds,
            [
                "user_message",
                "assistant_text",
                "tool_call",
                "tool_result",
                "assistant_text",
                "answer_drafted",
                // Le contrôle de Claw n'est pas affiché comme un message de l'utilisateur.
                "answer_superseded",
                "assistant_text",
                "answer_drafted",
            ]
        );
    }

    #[test]
    fn the_page_never_relies_on_browser_dialogs() {
        // `confirm`, `alert` et `prompt` sont bloqués dans le panneau de VS Code :
        // ils répondent « non » sans rien afficher, et l'action ne se fait pas
        // (« Nouvelle session » ne faisait plus rien). La page a ses propres boîtes.
        for call in [
            "!confirm(",
            " confirm(",
            "window.confirm(",
            "!alert(",
            " alert(",
            "window.alert(",
            " prompt(",
            "window.prompt(",
        ] {
            assert!(
                !super::PAGE.contains(call),
                "boîte du navigateur dans la page : {call}"
            );
        }
        assert!(super::PAGE.contains("function askConfirm("));
    }

    #[test]
    fn the_copy_button_does_not_depend_on_the_clipboard_api() {
        // Dans le panneau de VS Code, `navigator.clipboard.writeText` est refusé
        // à la page (« Write permission denied ») : le bouton « Copier » doit
        // se rabattre sur la commande de copie, qui y est permise.
        assert_eq!(super::PAGE.matches("navigator.clipboard.").count(), 1);
        assert!(super::PAGE.contains("return copyWithCommand(text);"));
        assert!(super::PAGE.contains(r#"document.execCommand("copy")"#));
    }

    #[test]
    fn the_page_recalls_earlier_prompts_and_shows_changes_before_and_after() {
        // Flèche du haut dans la zone de saisie : les demandes précédentes,
        // gardées d'une session à l'autre.
        assert!(super::PAGE.contains(r#"recallPrompt(event.key === "ArrowUp" ? -1 : 1)"#));
        assert!(super::PAGE.contains(r#"store.set("claw-prompts", "#));
        // Modification : deux colonnes, AVANT et APRÈS, sur les seules lignes
        // changées, renumérotées d'après le fichier entier au résultat.
        assert!(super::PAGE
            .contains("showDiff(holder, diffGroups(value.old_string, value.new_string), false)"));
        assert!(
            super::PAGE.contains(r#"el("div", "sbs-h", "AVANT"), el("div", "sbs-h n", "APRÈS")"#)
        );
        assert!(super::PAGE.contains("refineDiff(entry, parseJson(output))"));
        assert!(
            super::PAGE.contains(r#"showContent(holder, value.content, value.path, "CONTENU")"#)
        );
    }

    #[test]
    fn the_ultracode_brief_is_not_replayed_as_a_user_message() {
        let mut request = ConversationMessage::user_text("ajoute la page de traduction");
        request.blocks.push(runtime::ContentBlock::Text {
            text: runtime::workflow::brief_block("R1 la page"),
        });
        let events = transcript_from_messages(&[request]);
        assert_eq!(events.len(), 2);
        assert!(
            matches!(&events[0], WebEvent::UserMessage { text, .. } if text == "ajoute la page de traduction")
        );
        assert!(
            matches!(&events[1], WebEvent::Notice { text } if text.starts_with("Ultracode : dossier"))
        );
        // L'interrupteur de la page, et le rapport d'un sous-agent lisible.
        assert!(super::PAGE.contains(r#"id="ultracode-btn""#));
        assert!(super::PAGE.contains(r#"api("/api/ultracode", { on: !status.ultracode })"#));
        assert!(super::PAGE.contains(r#"typeof value.result === "string" && "agentId" in value"#));
    }

    #[test]
    fn the_page_has_a_team_switch_and_explains_the_gemini_category() {
        assert!(super::PAGE.contains(r#"id="team-btn""#));
        assert!(super::PAGE.contains(r#"api("/api/team", { on: !status.team })"#));
        assert!(super::PAGE.contains(r#"category.key === "gemini""#));
    }

    #[test]
    fn rebuilds_transcript_from_saved_messages() {
        let events = transcript_from_messages(&[
            ConversationMessage::user_text("bonjour"),
            ConversationMessage::tool_result("t1", "bash", "ok", false),
        ]);
        assert!(
            matches!(&events[0], WebEvent::UserMessage { text, at: None } if text == "bonjour")
        );
        assert!(matches!(&events[1], WebEvent::ToolResult { name, .. } if name == "bash"));
    }
}
