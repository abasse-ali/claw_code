//! Hooks : commandes shell exécutées à des moments clés de la session.
//!
//! Chaque hook reçoit sur stdin un objet JSON (`hook_event_name`, `cwd`,
//! `session_id`, plus les champs propres à l'événement) et les variables
//! d'environnement `HOOK_*`. Code de sortie 0 : succès (stdout est un retour
//! ou du contexte), 2 : blocage (stderr, sinon stdout, donne la raison), autre :
//! avertissement. Une sortie JSON façon Claude Code est aussi comprise
//! (`decision: "block"`, `hookSpecificOutput.permissionDecision: "deny"`,
//! `hookSpecificOutput.additionalContext`, `systemMessage`).

use std::io::Read;
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde_json::{json, Map, Value};

use crate::config::{HookCommandConfig, RuntimeFeatureConfig, RuntimeHookConfig};

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
}

impl HookEvent {
    pub const ALL: [Self; 9] = [
        Self::PreToolUse,
        Self::PostToolUse,
        Self::UserPromptSubmit,
        Self::Stop,
        Self::SubagentStop,
        Self::SessionStart,
        Self::SessionEnd,
        Self::PreCompact,
        Self::Notification,
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
            Self::PostToolUse => "marque le résultat comme une erreur",
            Self::UserPromptSubmit => "rejette la demande",
            Self::Stop | Self::SubagentStop => "relance l'agent avec la raison donnée",
            Self::SessionStart | Self::SessionEnd | Self::PreCompact | Self::Notification => {
                "aucun (affiché seulement)"
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct HookRunResult {
    denied: bool,
    messages: Vec<String>,
    context: Vec<String>,
}

impl HookRunResult {
    #[must_use]
    pub fn allow(messages: Vec<String>) -> Self {
        Self {
            denied: false,
            context: messages.clone(),
            messages,
        }
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

    #[must_use]
    pub fn run_post_tool_use(
        &self,
        tool_name: &str,
        tool_input: &str,
        tool_output: &str,
        is_error: bool,
    ) -> HookRunResult {
        self.run_event(
            HookEvent::PostToolUse,
            Some(tool_name),
            json!({
                "tool_name": tool_name,
                "tool_input": parse_tool_input(tool_input),
                "tool_input_json": tool_input,
                "tool_output": tool_output,
                "tool_response": tool_output,
                "tool_result_is_error": is_error,
            }),
            &tool_env(tool_name, tool_input, Some(tool_output), is_error),
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
            match run_command(entry, event, &payload, env) {
                HookCommandOutcome::Allow { message, context } => {
                    result.messages.extend(message.clone());
                    result.context.extend(context.or(message));
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
    let command = entry.command.as_str();
    let timeout = Duration::from_secs(entry.timeout_secs.unwrap_or(DEFAULT_HOOK_TIMEOUT_SECS));
    let mut process = plugins::script_or_shell_command(command);
    process
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("HOOK_EVENT", event.as_str());
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
        };
    };
    let text = |value: Option<&Value>| value.and_then(Value::as_str).map(ToOwned::to_owned);
    let specific = object.get("hookSpecificOutput");
    let blocked = object.get("decision").and_then(Value::as_str) == Some("block")
        || specific
            .and_then(|value| value.get("permissionDecision"))
            .and_then(Value::as_str)
            == Some("deny");
    if blocked {
        return HookCommandOutcome::Deny {
            message: text(object.get("reason"))
                .or_else(|| text(specific.and_then(|value| value.get("permissionDecisionReason")))),
        };
    }
    let context = text(specific.and_then(|value| value.get("additionalContext")));
    let message = text(object.get("systemMessage")).or_else(|| context.clone());
    HookCommandOutcome::Allow { message, context }
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
                    command: "printf reprise".to_string(),
                    matcher: Some("resume".to_string()),
                    timeout_secs: None,
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
                command: "sleep 5".to_string(),
                matcher: None,
                timeout_secs: Some(1),
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

    #[cfg(windows)]
    fn shell_snippet(script: &str) -> String {
        script.replace('\'', "\"")
    }

    #[cfg(not(windows))]
    fn shell_snippet(script: &str) -> String {
        script.to_string()
    }
}
