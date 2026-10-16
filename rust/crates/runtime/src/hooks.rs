//! Hooks : commandes shell, requêtes HTTP ou questions à un modèle, exécutées
//! à des moments clés de la session.
//!
//! Chaque hook reçoit un objet JSON (`hook_event_name`, `cwd`, `session_id`,
//! plus les champs propres à l'événement) : sur stdin pour une commande, qui
//! a aussi les variables `HOOK_*` et `CLAW_PROJECT_DIR`, dans le corps d'un
//! POST pour un hook `http`, à la place de `$ARGUMENTS` pour un hook `prompt`.
//! Code de sortie 0 : succès (stdout est un retour ou du contexte), 2 :
//! blocage (stderr, sinon stdout, donne la raison), autre : avertissement.
//! Une sortie JSON façon Claude Code est aussi comprise (`decision: "block"`,
//! `hookSpecificOutput.permissionDecision` `allow` / `ask` / `deny`,
//! `hookSpecificOutput.updatedInput`, `hookSpecificOutput.decision.behavior`
//! de `PermissionRequest`, `additionalContext`, `systemMessage`).

use std::collections::BTreeMap;
use std::io::Read;
use std::process::{Command, Stdio};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde_json::{json, Map, Value};

use crate::config::{HookCommandConfig, HookKind, RuntimeFeatureConfig, RuntimeHookConfig};

const DEFAULT_HOOK_TIMEOUT_SECS: u64 = 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum HookEvent {
    PreToolUse,
    PostToolUse,
    UserPromptSubmit,
    Stop,
    SubagentStop,
    SessionStart,
    SessionEnd,
    PreCompact,
    Notification,
    PostToolUseFailure,
    SubagentStart,
    PermissionRequest,
}

impl HookEvent {
    pub const ALL: [Self; 12] = [
        Self::PreToolUse,
        Self::PostToolUse,
        Self::UserPromptSubmit,
        Self::Stop,
        Self::SubagentStop,
        Self::SessionStart,
        Self::SessionEnd,
        Self::PreCompact,
        Self::Notification,
        Self::PostToolUseFailure,
        Self::SubagentStart,
        Self::PermissionRequest,
    ];

    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::PreToolUse => "PreToolUse",
            Self::PostToolUse => "PostToolUse",
            Self::UserPromptSubmit => "UserPromptSubmit",
            Self::Stop => "Stop",
            Self::SubagentStop => "SubagentStop",
            Self::SessionStart => "SessionStart",
            Self::SessionEnd => "SessionEnd",
            Self::PreCompact => "PreCompact",
            Self::Notification => "Notification",
            Self::PostToolUseFailure => "PostToolUseFailure",
            Self::SubagentStart => "SubagentStart",
            Self::PermissionRequest => "PermissionRequest",
        }
    }

    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|event| event.as_str() == name)
    }

    /// Ce que le hook peut faire en sortant avec le code 2.
    #[must_use]
    pub fn blocking_effect(self) -> &'static str {
        match self {
            Self::PreToolUse => "refuse l'appel d'outil",
            Self::PostToolUse | Self::PostToolUseFailure => "marque le résultat comme une erreur",
            Self::UserPromptSubmit => "rejette la demande",
            Self::Stop | Self::SubagentStop => "relance l'agent avec la raison donnée",
            Self::PermissionRequest => "refuse l'autorisation demandée",
            Self::SessionStart
            | Self::SessionEnd
            | Self::PreCompact
            | Self::Notification
            | Self::SubagentStart => "aucun (affiché seulement)",
        }
    }
}

/// Décision d'autorisation rendue par un hook (`PreToolUse` :
/// `permissionDecision`, `PermissionRequest` : `decision.behavior`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookPermissionDecision {
    /// Autorisé sans demander à l'utilisateur (les règles `deny` restent).
    Allow,
    /// Demander à l'utilisateur même si le mode l'autoriserait.
    Ask,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct HookRunResult {
    denied: bool,
    messages: Vec<String>,
    context: Vec<String>,
    decision: Option<HookPermissionDecision>,
    updated_input: Option<String>,
    stop: Option<String>,
}

impl HookRunResult {
    #[must_use]
    pub fn allow(messages: Vec<String>) -> Self {
        Self {
            denied: false,
            context: messages.clone(),
            messages,
            decision: None,
            updated_input: None,
            stop: None,
        }
    }

    /// Arrêt de l'agent demandé par un hook (`"continue": false`), avec sa raison.
    #[must_use]
    pub fn stop_reason(&self) -> Option<&str> {
        self.stop.as_deref()
    }

    /// Décision d'autorisation des hooks (`ask` l'emporte sur `allow`).
    #[must_use]
    pub fn permission_decision(&self) -> Option<HookPermissionDecision> {
        self.decision
    }

    /// Arguments de l'outil remplacés par un hook `PreToolUse` (`updatedInput`).
    #[must_use]
    pub fn updated_input(&self) -> Option<&str> {
        self.updated_input.as_deref()
    }

    #[must_use]
    pub fn is_denied(&self) -> bool {
        self.denied
    }

    /// Tous les retours : sorties, raisons de blocage et avertissements.
    #[must_use]
    pub fn messages(&self) -> &[String] {
        &self.messages
    }

    /// Sorties des hooks réussis, à ajouter au contexte du modèle
    /// (`UserPromptSubmit`, `SessionStart`).
    #[must_use]
    pub fn context(&self) -> &[String] {
        &self.context
    }

    /// Retours à montrer à l'utilisateur : tout sauf le contexte transmis au modèle.
    #[must_use]
    pub fn feedback(&self) -> Vec<&str> {
        self.messages
            .iter()
            .filter(|message| !self.context.contains(message))
            .map(String::as_str)
            .collect()
    }

    /// Raison du blocage (ou à défaut tous les retours).
    #[must_use]
    pub fn reason(&self, fallback: &str) -> String {
        if self.messages.is_empty() {
            fallback.to_string()
        } else {
            self.messages.join("\n")
        }
    }
}

// ------------------------------------------------------------ session active
//
// Une seule session par processus (comme les points de restauration) : son
// identifiant est transmis aux hooks, et le contexte produit par `SessionStart`
// attend le prochain tour, même si le runtime est reconstruit entre-temps
// (changement de modèle ou de mode).

#[derive(Default)]
struct SessionHookState {
    session_id: Option<String>,
    transcript_path: Option<String>,
    pending_context: Vec<String>,
}

static SESSION: Mutex<SessionHookState> = Mutex::new(SessionHookState {
    session_id: None,
    transcript_path: None,
    pending_context: Vec::new(),
});

fn with_session<T>(action: impl FnOnce(&mut SessionHookState) -> T) -> T {
    action(
        &mut SESSION
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner),
    )
}

/// Session courante, transmise aux hooks (`session_id`, `transcript_path`).
pub fn set_session_info(session_id: &str, transcript_path: &std::path::Path) {
    with_session(|state| {
        state.session_id = Some(session_id.to_string());
        state.transcript_path = Some(transcript_path.display().to_string());
    });
}

/// Identifiant de la session courante (skills : `${CLAUDE_SESSION_ID}`).
#[must_use]
pub fn current_session_id() -> Option<String> {
    with_session(|state| state.session_id.clone())
}

/// Contexte `SessionStart` en attente, consommé par le prochain tour.
pub(crate) fn take_pending_context() -> Vec<String> {
    with_session(|state| std::mem::take(&mut state.pending_context))
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct HookRunner {
    config: RuntimeHookConfig,
}

impl HookRunner {
    #[must_use]
    pub fn new(config: RuntimeHookConfig) -> Self {
        Self { config }
    }

    #[must_use]
    pub fn from_feature_config(feature_config: &RuntimeFeatureConfig) -> Self {
        Self::new(feature_config.hooks().clone())
    }

    /// `SessionStart` (`source` : startup, resume, clear) : la sortie des hooks
    /// est ajoutée au contexte du prochain tour.
    #[must_use]
    pub fn session_start(&self, source: &str) -> HookRunResult {
        let result = self.run_event(
            HookEvent::SessionStart,
            Some(source),
            json!({ "source": source }),
            &[],
        );
        with_session(|state| state.pending_context = result.context().to_vec());
        result
    }

    /// `SessionEnd` (`reason` : exit, clear, other).
    #[must_use]
    pub fn session_end(&self, reason: &str) -> HookRunResult {
        self.run_event(
            HookEvent::SessionEnd,
            Some(reason),
            json!({ "reason": reason }),
            &[],
        )
    }

    /// `Notification`, lancé en arrière-plan pour ne pas retarder l'agent.
    pub fn notify_in_background(&self, notification_type: &str, message: &str) {
        if !self.has(HookEvent::Notification) {
            return;
        }
        let runner = self.clone();
        let fields = json!({ "message": message, "notification_type": notification_type });
        let subject = notification_type.to_string();
        std::thread::spawn(move || {
            let _ = runner.run_event(HookEvent::Notification, Some(&subject), fields, &[]);
        });
    }

    #[must_use]
    pub fn config(&self) -> &RuntimeHookConfig {
        &self.config
    }

    #[must_use]
    pub fn has(&self, event: HookEvent) -> bool {
        !self.config.entries(event).is_empty()
    }

    #[must_use]
    pub fn run_pre_tool_use(&self, tool_name: &str, tool_input: &str) -> HookRunResult {
        self.run_event(
            HookEvent::PreToolUse,
            Some(tool_name),
            json!({
                "tool_name": tool_name,
                "tool_input": parse_tool_input(tool_input),
                "tool_input_json": tool_input,
            }),
            &tool_env(tool_name, tool_input, None, false),
        )
    }

    /// `PostToolUse`, puis `PostToolUseFailure` quand l'outil a échoué.
    #[must_use]
    pub fn run_post_tool_use(
        &self,
        tool_name: &str,
        tool_input: &str,
        tool_output: &str,
        is_error: bool,
    ) -> HookRunResult {
        let fields = json!({
            "tool_name": tool_name,
            "tool_input": parse_tool_input(tool_input),
            "tool_input_json": tool_input,
            "tool_output": tool_output,
            "tool_response": tool_output,
            "tool_result_is_error": is_error,
        });
        let env = tool_env(tool_name, tool_input, Some(tool_output), is_error);
        let mut result = self.run_event(
            HookEvent::PostToolUse,
            Some(tool_name),
            fields.clone(),
            &env,
        );
        if is_error && !result.denied {
            let mut failure = fields;
            if let Value::Object(object) = &mut failure {
                object.insert("error".into(), tool_output.into());
            }
            let failed = self.run_event(
                HookEvent::PostToolUseFailure,
                Some(tool_name),
                failure,
                &env,
            );
            result.denied = failed.denied;
            result.messages.extend(failed.messages);
            result.context.extend(failed.context);
        }
        result
    }

    /// `PermissionRequest` : avant de demander une autorisation à
    /// l'utilisateur, un hook peut l'accorder ou la refuser à sa place.
    #[must_use]
    pub fn run_permission_request(&self, tool_name: &str, tool_input: &str) -> HookRunResult {
        self.run_event(
            HookEvent::PermissionRequest,
            Some(tool_name),
            json!({
                "tool_name": tool_name,
                "tool_input": parse_tool_input(tool_input),
                "tool_input_json": tool_input,
            }),
            &tool_env(tool_name, tool_input, None, false),
        )
    }

    /// `SubagentStart` : un sous-agent démarre (`matcher` : son type).
    #[must_use]
    pub fn run_subagent_start(&self, agent_type: &str, prompt: &str) -> HookRunResult {
        self.run_event(
            HookEvent::SubagentStart,
            Some(agent_type),
            json!({ "agent_type": agent_type, "prompt": prompt }),
            &[],
        )
    }

    /// Exécute les hooks d'un événement. `subject` est comparé au `matcher` de
    /// chaque hook (nom d'outil, source de `SessionStart`, déclencheur de
    /// `PreCompact`…) ; `fields` complète la charge utile JSON.
    #[must_use]
    pub fn run_event(
        &self,
        event: HookEvent,
        subject: Option<&str>,
        fields: Value,
        env: &[(&str, String)],
    ) -> HookRunResult {
        let entries = self
            .config
            .entries(event)
            .iter()
            .filter(|entry| matcher_matches(entry.matcher.as_deref(), subject))
            .collect::<Vec<_>>();
        if entries.is_empty() {
            return HookRunResult::default();
        }

        let payload = Self::payload(event, fields).to_string();
        let mut result = HookRunResult::default();
        for entry in entries {
            if entry.run_async {
                // `"async": true` : lancé sans attendre, résultat ignoré.
                let entry = entry.clone();
                let payload = payload.clone();
                let env: Vec<(String, String)> = env
                    .iter()
                    .map(|(key, value)| ((*key).to_string(), value.clone()))
                    .collect();
                std::thread::spawn(move || {
                    let env: Vec<(&str, String)> = env
                        .iter()
                        .map(|(key, value)| (key.as_str(), value.clone()))
                        .collect();
                    let _ = run_command(&entry, event, &payload, &env);
                });
                continue;
            }
            match run_command(entry, event, &payload, env) {
                HookCommandOutcome::Allow {
                    message,
                    context,
                    decision,
                    updated_input,
                    stop,
                } => {
                    result.messages.extend(message.clone());
                    result.context.extend(context.or(message));
                    if result.stop.is_none() {
                        result.stop = stop;
                    }
                    if decision.is_some() && result.decision != Some(HookPermissionDecision::Ask) {
                        result.decision = decision;
                    }
                    if updated_input.is_some() {
                        result.updated_input = updated_input;
                    }
                }
                HookCommandOutcome::Deny { message } => {
                    result.messages.push(message.unwrap_or_else(|| {
                        format!("hook {} : blocage sans raison donnée", event.as_str())
                    }));
                    result.denied = true;
                    return result;
                }
                HookCommandOutcome::Warn { message } => result.messages.push(message),
            }
        }
        result
    }

    fn payload(event: HookEvent, fields: Value) -> Value {
        let mut payload = Map::new();
        payload.insert("hook_event_name".into(), event.as_str().into());
        payload.insert(
            "cwd".into(),
            std::env::current_dir()
                .map(|cwd| cwd.display().to_string())
                .unwrap_or_default()
                .into(),
        );
        with_session(|state| {
            if let Some(session_id) = &state.session_id {
                payload.insert("session_id".into(), session_id.clone().into());
            }
            if let Some(path) = &state.transcript_path {
                payload.insert("transcript_path".into(), path.clone().into());
            }
        });
        if let Value::Object(fields) = fields {
            payload.extend(fields);
        }
        Value::Object(payload)
    }
}

fn tool_env(
    tool_name: &str,
    tool_input: &str,
    tool_output: Option<&str>,
    is_error: bool,
) -> Vec<(&'static str, String)> {
    let mut env = vec![
        ("HOOK_TOOL_NAME", tool_name.to_string()),
        ("HOOK_TOOL_INPUT", tool_input.to_string()),
        (
            "HOOK_TOOL_IS_ERROR",
            if is_error { "1" } else { "0" }.to_string(),
        ),
    ];
    if let Some(output) = tool_output {
        env.push(("HOOK_TOOL_OUTPUT", output.to_string()));
    }
    env
}

/// Noms d'outils façon Claude Code, pour réutiliser ses `matcher`.
fn tool_alias(name: &str) -> Option<&'static str> {
    Some(match name {
        "bash" => "Bash",
        "read_file" => "Read",
        "write_file" => "Write",
        "edit_file" => "Edit",
        "glob_search" => "Glob",
        "grep_search" => "Grep",
        "Agent" => "Task",
        _ => return None,
    })
}

/// `matcher` absent, vide ou `*` : tout ; `A|B` : noms exacts ; sinon une
/// expression régulière qui doit couvrir tout le nom.
fn matcher_matches(matcher: Option<&str>, subject: Option<&str>) -> bool {
    let Some(matcher) = matcher
        .map(str::trim)
        .filter(|m| !m.is_empty() && *m != "*")
    else {
        return true;
    };
    let Some(subject) = subject else {
        return true;
    };
    let candidates = std::iter::once(subject)
        .chain(tool_alias(subject))
        .collect::<Vec<_>>();
    if matcher
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '|'))
    {
        return matcher
            .split('|')
            .any(|option| candidates.contains(&option.trim()));
    }
    regex::Regex::new(&format!("^(?:{matcher})$"))
        .is_ok_and(|pattern| candidates.iter().any(|name| pattern.is_match(name)))
}

enum HookCommandOutcome {
    Allow {
        message: Option<String>,
        context: Option<String>,
        decision: Option<HookPermissionDecision>,
        updated_input: Option<String>,
        /// `"continue": false` : l'agent s'arrête après cette étape.
        stop: Option<String>,
    },
    Deny {
        message: Option<String>,
    },
    Warn {
        message: String,
    },
}

fn run_command(
    entry: &HookCommandConfig,
    event: HookEvent,
    payload: &str,
    env: &[(&str, String)],
) -> HookCommandOutcome {
    match &entry.kind {
        HookKind::Command => run_shell_hook(entry, event, payload, env),
        HookKind::Http { headers } => run_http_hook(entry, event, payload, headers),
        HookKind::Prompt { model } => run_prompt_hook(entry, event, payload, model.as_deref()),
    }
}

/// Dossier du projet pour les hooks : la racine git, sinon le dossier courant.
fn project_dir() -> String {
    let cwd = std::env::current_dir().unwrap_or_default();
    crate::worktree::repo_root(&cwd)
        .unwrap_or(cwd)
        .display()
        .to_string()
}

fn run_shell_hook(
    entry: &HookCommandConfig,
    event: HookEvent,
    payload: &str,
    env: &[(&str, String)],
) -> HookCommandOutcome {
    let command = entry.command.as_str();
    let timeout = Duration::from_secs(entry.timeout_secs.unwrap_or(DEFAULT_HOOK_TIMEOUT_SECS));
    let mut process = plugins::script_or_shell_command(command);
    let project = project_dir();
    process
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("HOOK_EVENT", event.as_str())
        .env("CLAW_PROJECT_DIR", &project)
        .env("CLAUDE_PROJECT_DIR", &project);
    // Sous Windows, Python écrit en cp1252 dans un tube : on impose l'UTF-8.
    if cfg!(windows) && std::env::var_os("PYTHONUTF8").is_none() {
        process.env("PYTHONUTF8", "1");
    }
    for (key, value) in env {
        process.env(key, value);
    }

    let output = match run_with_timeout(&mut process, payload, timeout) {
        Ok(Some(output)) => output,
        Ok(None) => {
            return HookCommandOutcome::Warn {
                message: format!(
                    "hook {} `{command}` interrompu après {} s",
                    event.as_str(),
                    timeout.as_secs()
                ),
            }
        }
        Err(error) => {
            return HookCommandOutcome::Warn {
                message: format!(
                    "hook {} `{command}` : lancement impossible ({error})",
                    event.as_str()
                ),
            }
        }
    };

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    match output.status.code() {
        Some(0) => parse_success_output(&stdout),
        Some(2) => HookCommandOutcome::Deny {
            message: [stderr, stdout].into_iter().find(|text| !text.is_empty()),
        },
        Some(code) => {
            let detail = [stdout, stderr]
                .into_iter()
                .find(|text| !text.is_empty())
                .map(|text| format!(" : {text}"))
                .unwrap_or_default();
            HookCommandOutcome::Warn {
                message: format!(
                    "hook {} `{command}` terminé avec le code {code}, on continue{detail}",
                    event.as_str()
                ),
            }
        }
        None => HookCommandOutcome::Warn {
            message: format!("hook {} `{command}` arrêté par un signal", event.as_str()),
        },
    }
}

/// Sortie d'un hook réussi : texte libre, ou JSON au format Claude Code.
fn parse_success_output(stdout: &str) -> HookCommandOutcome {
    let parsed = stdout
        .starts_with('{')
        .then(|| serde_json::from_str::<Value>(stdout).ok())
        .flatten();
    let Some(Value::Object(object)) = parsed else {
        return HookCommandOutcome::Allow {
            message: (!stdout.is_empty()).then(|| stdout.to_string()),
            context: None,
            decision: None,
            updated_input: None,
            stop: None,
        };
    };
    let text = |value: Option<&Value>| value.and_then(Value::as_str).map(ToOwned::to_owned);
    let specific = object.get("hookSpecificOutput");
    let field = |key: &str| specific.and_then(|value| value.get(key));
    // `PermissionRequest` : `decision: {behavior: "allow" | "deny", message}`.
    let request = field("decision").filter(|value| value.is_object());
    let behavior = request
        .and_then(|value| value.get("behavior"))
        .and_then(Value::as_str);
    let permission = field("permissionDecision").and_then(Value::as_str);
    let blocked = object.get("decision").and_then(Value::as_str) == Some("block")
        || permission == Some("deny")
        || behavior == Some("deny");
    if blocked {
        return HookCommandOutcome::Deny {
            message: text(object.get("reason"))
                .or_else(|| text(field("permissionDecisionReason")))
                .or_else(|| text(request.and_then(|value| value.get("message")))),
        };
    }
    let decision = match permission.or(behavior) {
        Some("allow") => Some(HookPermissionDecision::Allow),
        Some("ask") => Some(HookPermissionDecision::Ask),
        _ => None,
    };
    let updated_input = field("updatedInput")
        .or_else(|| request.and_then(|value| value.get("updatedInput")))
        .filter(|value| value.is_object())
        .map(Value::to_string);
    let context = text(field("additionalContext"));
    let message = text(object.get("systemMessage"))
        .or_else(|| context.clone())
        .or_else(|| text(field("permissionDecisionReason")));
    let stop = (object.get("continue").and_then(Value::as_bool) == Some(false)).then(|| {
        text(object.get("stopReason")).unwrap_or_else(|| "un hook a demandé l'arrêt".to_string())
    });
    HookCommandOutcome::Allow {
        message,
        context,
        decision,
        updated_input,
        stop,
    }
}

// ------------------------------------------------------------ hooks http et prompt
//
// Le runtime n'a ni client HTTP ni modèle : le programme qui l'utilise fournit
// l'envoi d'une requête et la question à un modèle au démarrage.

/// Envoi d'un POST : `(url, en-têtes, corps JSON, délai)` → `(statut, corps)`.
pub type HttpHookSender = Box<
    dyn Fn(&str, &BTreeMap<String, String>, &str, Duration) -> Result<(u16, String), String>
        + Send
        + Sync,
>;

/// Question à un modèle : `(consigne système, demande, modèle ?)` → réponse.
pub type PromptHookEvaluator =
    Box<dyn Fn(&str, &str, Option<&str>) -> Result<String, String> + Send + Sync>;

static HTTP_SENDER: OnceLock<HttpHookSender> = OnceLock::new();
static PROMPT_EVALUATOR: OnceLock<PromptHookEvaluator> = OnceLock::new();

/// Fournit l'envoi des hooks `http` (une fois par processus).
pub fn set_http_hook_sender(sender: HttpHookSender) {
    let _ = HTTP_SENDER.set(sender);
}

/// Fournit le modèle des hooks `prompt` (une fois par processus).
pub fn set_prompt_hook_evaluator(evaluator: PromptHookEvaluator) {
    let _ = PROMPT_EVALUATOR.set(evaluator);
}

/// `${VAR}` remplacé par la variable d'environnement (vide si absente).
fn expand_env(value: &str) -> String {
    let mut expanded = String::new();
    let mut rest = value;
    while let Some(start) = rest.find("${") {
        expanded.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let Some(end) = after.find('}') else {
            expanded.push_str(&rest[start..]);
            return expanded;
        };
        expanded.push_str(&std::env::var(&after[..end]).unwrap_or_default());
        rest = &after[end + 1..];
    }
    expanded.push_str(rest);
    expanded
}

/// Hook `http` : POST de la charge utile ; réponse 2xx lue comme la sortie
/// d'une commande réussie (texte ou JSON), autre statut : avertissement.
fn run_http_hook(
    entry: &HookCommandConfig,
    event: HookEvent,
    payload: &str,
    headers: &BTreeMap<String, String>,
) -> HookCommandOutcome {
    let url = expand_env(&entry.command);
    let Some(sender) = HTTP_SENDER.get() else {
        return HookCommandOutcome::Warn {
            message: format!(
                "hook {} `{url}` : envoi HTTP indisponible ici",
                event.as_str()
            ),
        };
    };
    let headers: BTreeMap<String, String> = headers
        .iter()
        .map(|(name, value)| (name.clone(), expand_env(value)))
        .collect();
    let timeout = Duration::from_secs(entry.timeout_secs.unwrap_or(DEFAULT_HOOK_TIMEOUT_SECS));
    match sender(&url, &headers, payload, timeout) {
        Ok((status, body)) if (200..300).contains(&status) => parse_success_output(body.trim()),
        Ok((status, body)) => HookCommandOutcome::Warn {
            message: format!(
                "hook {} `{url}` : réponse HTTP {status}, on continue{}",
                event.as_str(),
                Some(body.trim())
                    .filter(|body| !body.is_empty())
                    .map(|body| format!(" : {}", body.chars().take(200).collect::<String>()))
                    .unwrap_or_default()
            ),
        },
        Err(error) => HookCommandOutcome::Warn {
            message: format!("hook {} `{url}` : {error}", event.as_str()),
        },
    }
}

const PROMPT_HOOK_SYSTEM: &str = "You evaluate a hook condition for a coding agent. You receive the hook instructions and the event data (JSON). Decide strictly according to the instructions. Answer with one JSON object only, no other text: {\"ok\": true} to let the action go on, or {\"ok\": false, \"reason\": \"...\"} to block it, the reason being addressed to the agent and saying what to do instead.";

/// Hook `prompt` : la consigne (avec `$ARGUMENTS` remplacé par la charge
/// utile) est posée à un modèle, qui répond `{"ok": …, "reason": …}`.
fn run_prompt_hook(
    entry: &HookCommandConfig,
    event: HookEvent,
    payload: &str,
    model: Option<&str>,
) -> HookCommandOutcome {
    let Some(evaluator) = PROMPT_EVALUATOR.get() else {
        return HookCommandOutcome::Warn {
            message: format!(
                "hook {} de type prompt : aucun modèle disponible ici",
                event.as_str()
            ),
        };
    };
    let prompt = if entry.command.contains("$ARGUMENTS") {
        entry.command.replace("$ARGUMENTS", payload)
    } else {
        format!("{}\n\nEvent data:\n{payload}", entry.command)
    };
    match evaluator(PROMPT_HOOK_SYSTEM, &prompt, model) {
        Ok(answer) => parse_prompt_answer(&answer).unwrap_or_else(|| HookCommandOutcome::Warn {
            message: format!(
                "hook {} de type prompt : réponse illisible, on continue ({})",
                event.as_str(),
                answer.trim().chars().take(160).collect::<String>()
            ),
        }),
        Err(error) => HookCommandOutcome::Warn {
            message: format!("hook {} de type prompt : {error}", event.as_str()),
        },
    }
}

/// `{"ok": false, "reason": "..."}` dans la réponse du modèle (éventuellement
/// entourée de texte ou d'un bloc de code).
fn parse_prompt_answer(answer: &str) -> Option<HookCommandOutcome> {
    let start = answer.find('{')?;
    let end = answer.rfind('}')?;
    let value: Value = serde_json::from_str(answer.get(start..=end)?).ok()?;
    let ok = value.get("ok").and_then(Value::as_bool)?;
    let reason = value
        .get("reason")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .filter(|reason| !reason.trim().is_empty());
    Some(if ok {
        HookCommandOutcome::Allow {
            message: None,
            context: None,
            decision: None,
            updated_input: None,
            stop: None,
        }
    } else {
        HookCommandOutcome::Deny { message: reason }
    })
}

/// Lance la commande, écrit `stdin`, attend au plus `timeout`. `Ok(None)` :
/// délai dépassé ou tour interrompu (le processus est tué).
fn run_with_timeout(
    process: &mut Command,
    stdin: &str,
    timeout: Duration,
) -> std::io::Result<Option<std::process::Output>> {
    // Sous Unix, un groupe de processus dédié : un hook arrêté pour délai
    // dépassé l'est avec tout ce qu'il a lancé.
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        process.process_group(0);
    }
    let mut child = process.spawn()?;
    // Écritures et lectures dans des fils dédiés : un hook qui ne lit pas
    // stdin ou qui écrit beaucoup ne doit pas bloquer l'agent.
    let writer = child.stdin.take().map(|mut pipe| {
        let payload = stdin.as_bytes().to_vec();
        std::thread::spawn(move || {
            use std::io::Write;
            let _ = pipe.write_all(&payload);
        })
    });
    let stdout = child.stdout.take().map(spawn_reader);
    let stderr = child.stderr.take().map(spawn_reader);
    let deadline = Instant::now() + timeout;
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break Some(status);
        }
        if Instant::now() >= deadline || crate::interrupt::interrupt_requested() {
            crate::bash::kill_process_tree(Some(child.id()));
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    // Après un arrêt forcé, un petit-enfant peut garder les tubes ouverts : on
    // n'attend pas les fils de lecture.
    let Some(status) = status else {
        return Ok(None);
    };
    if let Some(writer) = writer {
        let _ = writer.join();
    }
    let collect = |reader: Option<std::thread::JoinHandle<Vec<u8>>>| {
        reader
            .and_then(|handle| handle.join().ok())
            .unwrap_or_default()
    };
    Ok(Some(std::process::Output {
        status,
        stdout: collect(stdout),
        stderr: collect(stderr),
    }))
}

fn spawn_reader<R: Read + Send + 'static>(mut pipe: R) -> std::thread::JoinHandle<Vec<u8>> {
    std::thread::spawn(move || {
        let mut buffer = Vec::new();
        let _ = pipe.read_to_end(&mut buffer);
        buffer
    })
}

fn parse_tool_input(tool_input: &str) -> Value {
    serde_json::from_str(tool_input).unwrap_or_else(|_| json!({ "raw": tool_input }))
}

#[cfg(test)]
mod tests {
    use super::{matcher_matches, HookEvent, HookRunResult, HookRunner};
    use crate::config::{HookCommandConfig, RuntimeFeatureConfig, RuntimeHookConfig};
    use serde_json::json;

    #[test]
    fn allows_exit_code_zero_and_captures_stdout() {
        let runner = HookRunner::new(RuntimeHookConfig::new(
            vec![shell_snippet("printf 'pre ok'")],
            Vec::new(),
        ));

        let result = runner.run_pre_tool_use("Read", r#"{"path":"README.md"}"#);

        assert_eq!(result, HookRunResult::allow(vec!["pre ok".to_string()]));
    }

    #[test]
    fn denies_exit_code_two() {
        let runner = HookRunner::new(RuntimeHookConfig::new(
            vec![shell_snippet("printf 'blocked by hook'; exit 2")],
            Vec::new(),
        ));

        let result = runner.run_pre_tool_use("Bash", r#"{"command":"pwd"}"#);

        assert!(result.is_denied());
        assert_eq!(result.messages(), &["blocked by hook".to_string()]);
    }

    #[test]
    fn warns_for_other_non_zero_statuses() {
        let runner = HookRunner::from_feature_config(&RuntimeFeatureConfig::default().with_hooks(
            RuntimeHookConfig::new(
                vec![shell_snippet("printf 'warning hook'; exit 1")],
                Vec::new(),
            ),
        ));

        let result = runner.run_pre_tool_use("Edit", r#"{"file":"src/lib.rs"}"#);

        assert!(!result.is_denied());
        assert!(result
            .messages()
            .iter()
            .any(|message| message.contains("on continue")));
        assert!(
            result.context().is_empty(),
            "un avertissement n'est pas du contexte"
        );
    }

    #[test]
    fn matchers_accept_names_alternatives_aliases_and_regexes() {
        assert!(matcher_matches(None, Some("bash")));
        assert!(matcher_matches(Some("*"), Some("bash")));
        assert!(
            matcher_matches(Some("Bash"), Some("bash")),
            "alias Claude Code"
        );
        assert!(matcher_matches(Some("Edit|Write"), Some("write_file")));
        assert!(!matcher_matches(Some("Edit|Write"), Some("bash")));
        assert!(matcher_matches(
            Some("mcp__.*"),
            Some("mcp__github__search")
        ));
        assert!(!matcher_matches(Some("mcp__.*"), Some("read_file")));
        assert!(matcher_matches(Some("startup"), Some("startup")));
        assert!(!matcher_matches(Some("startup"), Some("resume")));
        assert!(matcher_matches(Some("Bash"), None), "événement sans sujet");
    }

    #[test]
    fn runs_generic_events_with_payload_matcher_and_json_output() {
        let config = RuntimeHookConfig::default()
            .with_entries(
                HookEvent::UserPromptSubmit,
                vec![HookCommandConfig::new(
                    r#"printf '{"hookSpecificOutput":{"additionalContext":"branche main"}}'"#
                        .to_string(),
                )],
            )
            .with_entries(
                HookEvent::SessionStart,
                vec![HookCommandConfig {
                    matcher: Some("resume".to_string()),
                    ..HookCommandConfig::new("printf reprise".to_string())
                }],
            )
            .with_entries(
                HookEvent::Stop,
                vec![HookCommandConfig::new(
                    r#"printf '{"decision":"block","reason":"les tests échouent"}'"#.to_string(),
                )],
            );
        let runner = HookRunner::new(config);

        let prompt = runner.run_event(
            HookEvent::UserPromptSubmit,
            None,
            json!({ "prompt": "salut" }),
            &[],
        );
        assert!(!prompt.is_denied());
        assert_eq!(prompt.context(), &["branche main".to_string()]);

        let startup = runner.run_event(HookEvent::SessionStart, Some("startup"), json!({}), &[]);
        assert!(startup.context().is_empty(), "le matcher filtre la source");
        let resume = runner.run_event(HookEvent::SessionStart, Some("resume"), json!({}), &[]);
        assert_eq!(resume.context(), &["reprise".to_string()]);

        let stop = runner.run_event(HookEvent::Stop, None, json!({}), &[]);
        assert!(stop.is_denied());
        assert_eq!(stop.reason(""), "les tests échouent");
    }

    #[test]
    fn hooks_receive_the_json_payload_on_stdin() {
        let file =
            std::env::temp_dir().join(format!("claw-hook-payload-{}.json", std::process::id()));
        let target = file.display().to_string().replace('\\', "/");
        let runner = HookRunner::new(RuntimeHookConfig::default().with_entries(
            HookEvent::PreCompact,
            vec![HookCommandConfig::new(format!("cat > '{target}'"))],
        ));
        let result = runner.run_event(
            HookEvent::PreCompact,
            Some("manual"),
            json!({ "trigger": "manual" }),
            &[],
        );
        assert!(result.messages().is_empty(), "{result:?}");
        let payload: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&file).expect("payload écrit"))
                .expect("payload JSON");
        let _ = std::fs::remove_file(&file);
        assert_eq!(payload["hook_event_name"], "PreCompact");
        assert_eq!(payload["trigger"], "manual");
        assert!(payload["cwd"].is_string());
    }

    #[test]
    fn slow_hooks_are_killed_after_their_timeout() {
        let runner = HookRunner::new(RuntimeHookConfig::default().with_entries(
            HookEvent::Notification,
            vec![HookCommandConfig {
                timeout_secs: Some(1),
                ..HookCommandConfig::new("sleep 5".to_string())
            }],
        ));
        let started = std::time::Instant::now();
        let result = runner.run_event(HookEvent::Notification, None, json!({}), &[]);
        assert!(started.elapsed() < std::time::Duration::from_secs(4));
        assert!(
            result.messages()[0].contains("interrompu après 1 s"),
            "{result:?}"
        );
    }

    #[test]
    fn json_decisions_allow_ask_and_rewrite_the_tool_input() {
        use super::{parse_success_output, HookCommandOutcome, HookPermissionDecision};
        match parse_success_output(
            r#"{"hookSpecificOutput":{"permissionDecision":"allow","permissionDecisionReason":"lecture sûre","updatedInput":{"command":"ls -la"}}}"#,
        ) {
            HookCommandOutcome::Allow {
                decision,
                updated_input,
                message,
                ..
            } => {
                assert_eq!(decision, Some(HookPermissionDecision::Allow));
                assert_eq!(updated_input.as_deref(), Some(r#"{"command":"ls -la"}"#));
                assert_eq!(message.as_deref(), Some("lecture sûre"));
            }
            _ => panic!("allow attendu"),
        }
        assert!(matches!(
            parse_success_output(r#"{"hookSpecificOutput":{"permissionDecision":"ask"}}"#),
            HookCommandOutcome::Allow {
                decision: Some(HookPermissionDecision::Ask),
                ..
            }
        ));
        // `PermissionRequest` répond avec `decision.behavior`.
        assert!(matches!(
            parse_success_output(
                r#"{"hookSpecificOutput":{"hookEventName":"PermissionRequest","decision":{"behavior":"allow"}}}"#
            ),
            HookCommandOutcome::Allow {
                decision: Some(HookPermissionDecision::Allow),
                ..
            }
        ));
        match parse_success_output(
            r#"{"hookSpecificOutput":{"decision":{"behavior":"deny","message":"pas en production"}}}"#,
        ) {
            HookCommandOutcome::Deny { message } => {
                assert_eq!(message.as_deref(), Some("pas en production"));
            }
            _ => panic!("refus attendu"),
        }
    }

    #[test]
    fn continue_false_asks_the_agent_to_stop() {
        use super::{parse_success_output, HookCommandOutcome};
        match parse_success_output(r#"{"continue": false, "stopReason": "budget atteint"}"#) {
            HookCommandOutcome::Allow { stop, .. } => {
                assert_eq!(stop.as_deref(), Some("budget atteint"));
            }
            _ => panic!("arrêt attendu"),
        }
        match parse_success_output(r#"{"continue": false}"#) {
            HookCommandOutcome::Allow { stop, .. } => assert!(stop.is_some()),
            _ => panic!("arrêt attendu"),
        }
        assert!(matches!(
            parse_success_output(r#"{"continue": true}"#),
            HookCommandOutcome::Allow { stop: None, .. }
        ));
    }

    #[test]
    fn prompt_hook_answers_are_read_even_inside_text() {
        use super::{parse_prompt_answer, HookCommandOutcome};
        assert!(matches!(
            parse_prompt_answer("```json\n{\"ok\": true}\n```"),
            Some(HookCommandOutcome::Allow { .. })
        ));
        match parse_prompt_answer(r#"Réponse : {"ok": false, "reason": "lance d'abord les tests"}"#)
        {
            Some(HookCommandOutcome::Deny { message }) => {
                assert_eq!(message.as_deref(), Some("lance d'abord les tests"));
            }
            _ => panic!("refus attendu"),
        }
        assert!(parse_prompt_answer("je ne sais pas").is_none());
    }

    #[test]
    fn http_and_prompt_hooks_use_the_transports_given_at_startup() {
        use crate::config::HookKind;
        let _env = crate::test_env_lock();
        std::env::set_var("CLAW_TEST_HOOK_TOKEN", "secret-de-test");
        super::set_http_hook_sender(Box::new(|url, headers, body, _| {
            if url.ends_with("/refuse") {
                return Ok((
                    200,
                    r#"{"decision":"block","reason":"interdit par le serveur"}"#.to_string(),
                ));
            }
            if url.ends_with("/panne") {
                return Ok((503, "indisponible".to_string()));
            }
            assert_eq!(
                headers.get("Authorization").map(String::as_str),
                Some("Bearer secret-de-test")
            );
            assert!(body.contains(r#""hook_event_name":"PreToolUse""#), "{body}");
            Ok((200, "contexte du serveur".to_string()))
        }));
        super::set_prompt_hook_evaluator(Box::new(|_, prompt, model| {
            assert_eq!(model, Some("petit-modele"));
            Ok(if prompt.contains("rm -rf") {
                r#"{"ok": false, "reason": "commande destructrice"}"#.to_string()
            } else {
                r#"{"ok": true}"#.to_string()
            })
        }));
        let http = |url: &str| HookCommandConfig {
            kind: HookKind::Http {
                headers: [(
                    "Authorization".to_string(),
                    "Bearer ${CLAW_TEST_HOOK_TOKEN}".to_string(),
                )]
                .into(),
            },
            ..HookCommandConfig::new(url.to_string())
        };
        let run = |entry: HookCommandConfig, command: &str| {
            HookRunner::new(
                RuntimeHookConfig::default().with_entries(HookEvent::PreToolUse, vec![entry]),
            )
            .run_pre_tool_use("bash", &format!(r#"{{"command":"{command}"}}"#))
        };
        let ok = run(http("https://hooks.example/ok"), "ls");
        assert!(!ok.is_denied());
        assert_eq!(ok.messages(), ["contexte du serveur"]);
        let refused = run(http("https://hooks.example/refuse"), "ls");
        assert!(refused.is_denied());
        assert_eq!(refused.messages(), ["interdit par le serveur"]);
        let down = run(http("https://hooks.example/panne"), "ls");
        assert!(!down.is_denied());
        assert!(down.messages()[0].contains("HTTP 503"), "{down:?}");

        let guard = HookCommandConfig {
            kind: HookKind::Prompt {
                model: Some("petit-modele".to_string()),
            },
            ..HookCommandConfig::new("Refuse les commandes destructrices : $ARGUMENTS".to_string())
        };
        let blocked = run(guard.clone(), "rm -rf /");
        assert!(blocked.is_denied());
        assert_eq!(blocked.messages(), ["commande destructrice"]);
        assert!(!run(guard, "ls").is_denied());
        std::env::remove_var("CLAW_TEST_HOOK_TOKEN");
    }

    #[test]
    fn async_hooks_do_not_hold_the_agent() {
        let runner = HookRunner::new(RuntimeHookConfig::default().with_entries(
            HookEvent::PostToolUse,
            vec![HookCommandConfig {
                run_async: true,
                ..HookCommandConfig::new("sleep 3".to_string())
            }],
        ));
        let started = std::time::Instant::now();
        let result = runner.run_post_tool_use("bash", "{}", "ok", false);
        assert!(started.elapsed() < std::time::Duration::from_secs(2));
        assert!(result.messages().is_empty());
    }

    #[test]
    fn failures_also_run_post_tool_use_failure_hooks() {
        let runner = HookRunner::new(RuntimeHookConfig::default().with_entries(
            HookEvent::PostToolUseFailure,
            vec![HookCommandConfig::new(shell_snippet("printf 'échec noté'"))],
        ));
        assert!(runner
            .run_post_tool_use("bash", "{}", "ok", false)
            .messages()
            .is_empty());
        assert_eq!(
            runner
                .run_post_tool_use("bash", "{}", "boom", true)
                .messages(),
            ["échec noté"]
        );
        assert_eq!(
            HookEvent::from_name("PermissionRequest"),
            Some(HookEvent::PermissionRequest)
        );
        assert_eq!(
            HookEvent::from_name("SubagentStart"),
            Some(HookEvent::SubagentStart)
        );
    }

    #[cfg(windows)]
    fn shell_snippet(script: &str) -> String {
        script.replace('\'', "\"")
    }

    #[cfg(not(windows))]
    fn shell_snippet(script: &str) -> String {
        script.to_string()
    }
}
