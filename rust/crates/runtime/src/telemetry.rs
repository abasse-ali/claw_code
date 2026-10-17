//! Télémétrie OpenTelemetry, comme le suivi d'usage de Claude Code : des
//! compteurs (sessions, tokens, coût, lignes modifiées, commits, pull
//! requests, décisions sur les éditions, temps actif) et des événements
//! (demande, requête au modèle, résultat d'outil, décision) envoyés à votre
//! collecteur en OTLP : `http/json`, `http/protobuf` ou `grpc`
//! (`OTEL_EXPORTER_OTLP_PROTOCOL`, ou par signal).
//!
//! Rien n'est mesuré ni envoyé sans `CLAW_ENABLE_TELEMETRY=1` (ou
//! `CLAUDE_CODE_ENABLE_TELEMETRY=1`). Les variables `OTEL_*` usuelles règlent
//! les exporteurs (`otlp`, `console`, `none`), l'adresse, les en-têtes et
//! les intervalles. Le texte des demandes et le détail des outils ne partent
//! qu'avec `OTEL_LOG_USER_PROMPTS=1` et `OTEL_LOG_TOOL_DETAILS=1`.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};

use crate::usage::TokenUsage;

/// Un envoi vers le collecteur.
pub struct Export<'a> {
    pub url: &'a str,
    pub headers: &'a [(String, String)],
    pub content_type: &'a str,
    pub body: &'a [u8],
    /// Appel gRPC unaire (HTTP/2, message encadré par l'envoi).
    pub grpc: bool,
}

/// Envoi d'un lot au collecteur.
pub type Sender = Box<dyn Fn(&Export<'_>) -> Result<(), String> + Send + Sync>;

/// Protocole OTLP d'un signal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Protocol {
    HttpJson,
    HttpProtobuf,
    Grpc,
}
/// Coût estimé d'une requête (dollars).
pub type CostEstimator = Box<dyn Fn(&str, &TokenUsage) -> Option<f64> + Send + Sync>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Exporter {
    Otlp,
    Console,
}

/// Réglages lus dans l'environnement au premier usage.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone)]
struct Config {
    metrics: Vec<Exporter>,
    logs: Vec<Exporter>,
    metrics_endpoint: String,
    logs_endpoint: String,
    metrics_protocol: Protocol,
    logs_protocol: Protocol,
    metrics_headers: Vec<(String, String)>,
    logs_headers: Vec<(String, String)>,
    metric_interval: Duration,
    logs_interval: Duration,
    cumulative: bool,
    log_prompts: bool,
    log_tool_details: bool,
    include_version: bool,
    resource: Vec<(String, String)>,
    prefix: String,
    /// Protocole inconnu demandé (signalé une fois ; `http/json` le remplace).
    unsupported_protocol: Option<String>,
}

fn env(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn truthy(name: &str) -> bool {
    env(name)
        .is_some_and(|value| matches!(value.to_lowercase().as_str(), "1" | "true" | "yes" | "on"))
}

/// `k=v,k2=v2` (valeurs encodées comme dans une URL).
fn parse_pairs(text: &str) -> Vec<(String, String)> {
    text.split(',')
        .filter_map(|pair| {
            let (key, value) = pair.split_once('=')?;
            let key = key.trim();
            (!key.is_empty()).then(|| (percent_decode(key), percent_decode(value.trim())))
        })
        .collect()
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            if let Ok(byte) = u8::from_str_radix(&text[index + 1..index + 3], 16) {
                out.push(byte);
                index += 3;
                continue;
            }
        }
        out.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn exporters(name: &str, default: &str) -> Vec<Exporter> {
    env(name)
        .unwrap_or_else(|| default.to_string())
        .split(',')
        .filter_map(|value| match value.trim().to_lowercase().as_str() {
            "otlp" => Some(Exporter::Otlp),
            "console" => Some(Exporter::Console),
            _ => None,
        })
        .collect()
}

/// Protocole d'un signal : `OTEL_EXPORTER_OTLP_<SIGNAL>_PROTOCOL`, sinon
/// `OTEL_EXPORTER_OTLP_PROTOCOL`, sinon `http/json` ; `Err` : valeur inconnue.
fn protocol(signal: &str) -> Result<Protocol, String> {
    let value = env(&format!("OTEL_EXPORTER_OTLP_{signal}_PROTOCOL"))
        .or_else(|| env("OTEL_EXPORTER_OTLP_PROTOCOL"))
        .map(|value| value.to_lowercase());
    match value.as_deref() {
        None | Some("http/json") => Ok(Protocol::HttpJson),
        Some("http/protobuf") => Ok(Protocol::HttpProtobuf),
        Some("grpc") => Ok(Protocol::Grpc),
        Some(other) => Err(other.to_string()),
    }
}

/// `http://hote:port` d'une adresse (gRPC : le chemin est celui du service).
fn origin(url: &str) -> String {
    let (scheme, rest) = url.split_once("://").unwrap_or(("http", url));
    let authority = rest.split('/').next().unwrap_or(rest);
    format!("{scheme}://{authority}")
}

fn endpoint(signal: &str, path: &str, protocol: Protocol) -> String {
    let service = if signal == "METRICS" {
        "opentelemetry.proto.collector.metrics.v1.MetricsService/Export"
    } else {
        "opentelemetry.proto.collector.logs.v1.LogsService/Export"
    };
    if let Some(endpoint) = env(&format!("OTEL_EXPORTER_OTLP_{signal}_ENDPOINT")) {
        return if protocol == Protocol::Grpc {
            format!("{}/{service}", origin(&endpoint))
        } else {
            endpoint
        };
    }
    let default = if protocol == Protocol::Grpc {
        "http://localhost:4317"
    } else {
        "http://localhost:4318"
    };
    let base = env("OTEL_EXPORTER_OTLP_ENDPOINT").unwrap_or_else(|| default.to_string());
    if protocol == Protocol::Grpc {
        format!("{}/{service}", origin(&base))
    } else {
        format!("{}/{path}", base.trim_end_matches('/'))
    }
}

fn interval(name: &str, default_ms: u64) -> Duration {
    Duration::from_millis(
        env(name)
            .and_then(|value| value.parse::<u64>().ok())
            .filter(|value| *value >= 100)
            .unwrap_or(default_ms),
    )
}

impl Config {
    fn from_env() -> Option<Self> {
        if !(truthy("CLAW_ENABLE_TELEMETRY") || truthy("CLAUDE_CODE_ENABLE_TELEMETRY")) {
            return None;
        }
        let headers = env("OTEL_EXPORTER_OTLP_HEADERS")
            .map(|text| parse_pairs(&text))
            .unwrap_or_default();
        let signal_headers = |signal: &str| {
            let mut merged = headers.clone();
            if let Some(text) = env(&format!("OTEL_EXPORTER_OTLP_{signal}_HEADERS")) {
                for (key, value) in parse_pairs(&text) {
                    merged.retain(|(existing, _)| !existing.eq_ignore_ascii_case(&key));
                    merged.push((key, value));
                }
            }
            merged
        };
        let mut unsupported_protocol = None;
        let mut checked = |signal: &str| {
            protocol(signal).unwrap_or_else(|unknown| {
                unsupported_protocol = Some(unknown);
                Protocol::HttpJson
            })
        };
        let metrics_protocol = checked("METRICS");
        let logs_protocol = checked("LOGS");
        Some(Self {
            metrics: exporters("OTEL_METRICS_EXPORTER", "otlp"),
            logs: exporters("OTEL_LOGS_EXPORTER", "otlp"),
            metrics_endpoint: endpoint("METRICS", "v1/metrics", metrics_protocol),
            logs_endpoint: endpoint("LOGS", "v1/logs", logs_protocol),
            metrics_protocol,
            logs_protocol,
            metrics_headers: signal_headers("METRICS"),
            logs_headers: signal_headers("LOGS"),
            metric_interval: interval("OTEL_METRIC_EXPORT_INTERVAL", 60_000),
            logs_interval: interval("OTEL_LOGS_EXPORT_INTERVAL", 5_000),
            cumulative: env("OTEL_EXPORTER_OTLP_METRICS_TEMPORALITY_PREFERENCE")
                .is_some_and(|value| value.eq_ignore_ascii_case("cumulative")),
            log_prompts: truthy("OTEL_LOG_USER_PROMPTS"),
            log_tool_details: truthy("OTEL_LOG_TOOL_DETAILS"),
            include_version: truthy("OTEL_METRICS_INCLUDE_VERSION"),
            resource: env("OTEL_RESOURCE_ATTRIBUTES")
                .map(|text| parse_pairs(&text))
                .unwrap_or_default(),
            prefix: env("CLAW_OTEL_METRIC_PREFIX").unwrap_or_else(|| "claw_code".to_string()),
            unsupported_protocol,
        })
    }
}

/// Un point de compteur : nom, attributs (triés), valeur.
type CounterKey = (String, Vec<(String, String)>);

#[derive(Default)]
struct State {
    counters: BTreeMap<CounterKey, f64>,
    /// Totaux depuis le démarrage (temporalité cumulative).
    totals: BTreeMap<CounterKey, f64>,
    logs: Vec<Value>,
    session_id: Option<String>,
    model: Option<String>,
    prompt_id: Option<String>,
    started_ns: u128,
    last_metric_export_ns: u128,
}

struct Telemetry {
    config: Config,
    state: Mutex<State>,
    sender: Mutex<Option<Sender>>,
    cost: Mutex<Option<CostEstimator>>,
    user_id: String,
    version: String,
}

fn now_ns() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_nanos())
}

fn telemetry() -> Option<&'static Telemetry> {
    static TELEMETRY: OnceLock<Option<Telemetry>> = OnceLock::new();
    TELEMETRY
        .get_or_init(|| {
            let config = Config::from_env()?;
            let started = now_ns();
            Some(Telemetry {
                config,
                state: Mutex::new(State {
                    started_ns: started,
                    last_metric_export_ns: started,
                    ..State::default()
                }),
                sender: Mutex::new(None),
                cost: Mutex::new(None),
                user_id: user_id(),
                version: VERSION.get().cloned().unwrap_or_default(),
            })
        })
        .as_ref()
}

static VERSION: OnceLock<String> = OnceLock::new();

/// Identifiant anonyme gardé dans `~/.claw/telemetry-id` (aucune donnée
/// personnelle, nouveau si le fichier est supprimé).
fn user_id() -> String {
    let path = crate::config::default_config_home().join("telemetry-id");
    if let Ok(existing) = std::fs::read_to_string(&path) {
        let existing = existing.trim().to_string();
        if !existing.is_empty() {
            return existing;
        }
    }
    let mut bytes = [0u8; 16];
    let _ = getrandom::getrandom(&mut bytes);
    let id = hex(&bytes);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&path, &id);
    id
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes.iter().fold(String::new(), |mut text, byte| {
        let _ = write!(text, "{byte:02x}");
        text
    })
}

/// La télémétrie est active.
#[must_use]
pub fn enabled() -> bool {
    telemetry().is_some()
}

/// À l'initialisation de la CLI : version, envoi HTTP et estimation du coût.
pub fn install(version: &str, sender: Sender, cost: CostEstimator) {
    let _ = VERSION.set(version.to_string());
    let Some(telemetry) = telemetry() else {
        return;
    };
    *telemetry
        .sender
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(sender);
    *telemetry
        .cost
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(cost);
    if let Some(protocol) = &telemetry.config.unsupported_protocol {
        eprintln!(
            "[télémétrie] protocole OTLP inconnu « {protocol} » : http/json est utilisé (valeurs possibles : http/json, http/protobuf, grpc)."
        );
    }
    start_exporter();
}

fn with_state(change: impl FnOnce(&mut State, &Config)) {
    if let Some(telemetry) = telemetry() {
        let mut state = telemetry
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        change(&mut state, &telemetry.config);
    }
}

pub fn set_session(session_id: &str) {
    with_state(|state, _| state.session_id = Some(session_id.to_string()));
}

pub fn set_model(model: &str) {
    with_state(|state, _| state.model = Some(model.to_string()));
}

fn standard_attributes(state: &State, telemetry: &Telemetry) -> Vec<(String, String)> {
    let mut attributes = vec![("user.id".to_string(), telemetry.user_id.clone())];
    if let Some(session) = &state.session_id {
        attributes.push(("session.id".to_string(), session.clone()));
    }
    if telemetry.config.include_version {
        attributes.push(("app.version".to_string(), telemetry.version.clone()));
    }
    if let Some(terminal) = env("TERM_PROGRAM") {
        attributes.push(("terminal.type".to_string(), terminal));
    }
    attributes
}

fn add_counter(name: &str, value: f64, extra: &[(&str, String)]) {
    let Some(telemetry) = telemetry() else {
        return;
    };
    if telemetry.config.metrics.is_empty() || value == 0.0 {
        return;
    }
    let mut state = telemetry
        .state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut attributes = standard_attributes(&state, telemetry);
    attributes.extend(
        extra
            .iter()
            .map(|(key, value)| ((*key).to_string(), value.clone())),
    );
    attributes.sort();
    let key = (format!("{}.{name}", telemetry.config.prefix), attributes);
    *state.counters.entry(key.clone()).or_default() += value;
    *state.totals.entry(key).or_default() += value;
}

fn add_event(name: &str, extra: Vec<(&str, Value)>) {
    let Some(telemetry) = telemetry() else {
        return;
    };
    if telemetry.config.logs.is_empty() {
        return;
    }
    let mut state = telemetry
        .state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut attributes: Vec<Value> = standard_attributes(&state, telemetry)
        .into_iter()
        .map(|(key, value)| attribute(&key, &Value::String(value)))
        .collect();
    attributes.push(attribute("event.name", &Value::String(name.to_string())));
    attributes.push(attribute("event.timestamp", &Value::String(iso_now())));
    if let Some(prompt) = &state.prompt_id {
        attributes.push(attribute("prompt.id", &Value::String(prompt.clone())));
    }
    for (key, value) in extra {
        attributes.push(attribute(key, &value));
    }
    let now = now_ns().to_string();
    state.logs.push(json!({
        "timeUnixNano": now,
        "observedTimeUnixNano": now,
        "severityNumber": 9,
        "severityText": "INFO",
        "body": { "stringValue": format!("{}.{name}", telemetry.config.prefix) },
        "attributes": attributes,
    }));
}

/// `2026-10-17T08:30:12Z`.
fn iso_now() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs());
    let time = crate::cron::LocalTime::from_minutes(i64::try_from(seconds / 60).unwrap_or(0));
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        time.year,
        time.month,
        time.day,
        time.hour,
        time.minute,
        seconds % 60
    )
}

fn attribute(key: &str, value: &Value) -> Value {
    let value = match value {
        Value::Bool(flag) => json!({ "boolValue": flag }),
        Value::Number(number) if number.is_i64() || number.is_u64() => {
            json!({ "intValue": number.to_string() })
        }
        Value::Number(number) => json!({ "doubleValue": number.as_f64().unwrap_or(0.0) }),
        Value::String(text) => json!({ "stringValue": text }),
        other => json!({ "stringValue": other.to_string() }),
    };
    json!({ "key": key, "value": value })
}

// --- Ce que mesure Claw ----------------------------------------------------------

/// Début d'une session (`fresh`, `resume`, `continue`, `agents_view`, `background`).
pub fn session_started(start_type: &str) {
    add_counter(
        "session.count",
        1.0,
        &[("start_type", start_type.to_string())],
    );
}

/// Nouvelle demande de l'utilisateur.
pub fn user_prompt(text: &str) {
    if !enabled() {
        return;
    }
    let mut bytes = [0u8; 8];
    let _ = getrandom::getrandom(&mut bytes);
    let id = hex(&bytes);
    let log_prompts = telemetry().is_some_and(|telemetry| telemetry.config.log_prompts);
    with_state(|state, _| state.prompt_id = Some(id));
    let mut extra = vec![("prompt_length", json!(text.chars().count()))];
    extra.push((
        "prompt",
        Value::String(if log_prompts {
            text.chars().take(60_000).collect()
        } else {
            "<REDACTED>".to_string()
        }),
    ));
    add_event("user_prompt", extra);
}

/// Requête au modèle terminée (tokens, coût estimé, durée).
pub fn api_request(usage: &TokenUsage, duration: Duration, subagent: bool) {
    let Some(telemetry) = telemetry() else {
        return;
    };
    let model = telemetry
        .state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .model
        .clone()
        .unwrap_or_else(|| "unknown".to_string());
    let cost = telemetry
        .cost
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .as_ref()
        .and_then(|estimate| estimate(&model, usage))
        .unwrap_or(0.0);
    let source = if subagent { "subagent" } else { "main" }.to_string();
    for (kind, value) in [
        ("input", usage.input_tokens),
        ("output", usage.output_tokens),
        ("cacheRead", usage.cache_read_input_tokens),
        ("cacheCreation", usage.cache_creation_input_tokens),
    ] {
        add_counter(
            "token.usage",
            f64::from(value),
            &[
                ("type", kind.to_string()),
                ("model", model.clone()),
                ("query_source", source.clone()),
            ],
        );
    }
    add_counter(
        "cost.usage",
        cost,
        &[("model", model.clone()), ("query_source", source.clone())],
    );
    add_event(
        "api_request",
        vec![
            ("model", json!(model)),
            ("cost_usd", json!(cost)),
            (
                "duration_ms",
                json!(u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)),
            ),
            ("input_tokens", json!(usage.input_tokens)),
            ("output_tokens", json!(usage.output_tokens)),
            ("cache_read_tokens", json!(usage.cache_read_input_tokens)),
            (
                "cache_creation_tokens",
                json!(usage.cache_creation_input_tokens),
            ),
            ("query_source", json!(source)),
        ],
    );
}

/// Requête au modèle en échec.
pub fn api_error(error: &str, duration: Duration) {
    let model = telemetry().and_then(|telemetry| {
        telemetry
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .model
            .clone()
    });
    add_event(
        "api_error",
        vec![
            ("model", json!(model.unwrap_or_default())),
            ("error", json!(error.chars().take(500).collect::<String>())),
            (
                "duration_ms",
                json!(u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)),
            ),
        ],
    );
}

/// Nom des outils d'édition dans les compteurs (ceux de Claude Code).
fn edit_tool_name(tool: &str) -> Option<&'static str> {
    match tool {
        "edit_file" | "Edit" => Some("Edit"),
        "write_file" | "Write" => Some("Write"),
        "NotebookEdit" => Some("NotebookEdit"),
        _ => None,
    }
}

/// Langage d'un fichier d'après son extension.
fn language_of(path: &str) -> &'static str {
    let extension = path.rsplit('.').next().unwrap_or_default().to_lowercase();
    match extension.as_str() {
        "rs" => "Rust",
        "py" => "Python",
        "ts" | "tsx" => "TypeScript",
        "js" | "jsx" | "mjs" | "cjs" => "JavaScript",
        "go" => "Go",
        "java" => "Java",
        "kt" => "Kotlin",
        "c" | "h" => "C",
        "cpp" | "cc" | "hpp" => "C++",
        "cs" => "C#",
        "rb" => "Ruby",
        "php" => "PHP",
        "swift" => "Swift",
        "md" => "Markdown",
        "json" => "JSON",
        "yml" | "yaml" => "YAML",
        "toml" => "TOML",
        "html" => "HTML",
        "css" => "CSS",
        "sh" | "bash" => "Shell",
        "sql" => "SQL",
        _ => "unknown",
    }
}

fn input_field(input: &str, key: &str) -> Option<String> {
    serde_json::from_str::<Value>(input)
        .ok()?
        .get(key)?
        .as_str()
        .map(str::to_string)
}

/// Décision d'autorisation sur un outil (`config` : règles ou mode ;
/// `user` : réponse à la question).
pub fn tool_decision(tool: &str, input: &str, accepted: bool) {
    // `note_asked` : la question a été posée pendant cette autorisation.
    let asked = ASKED.swap(false, Ordering::SeqCst);
    if !enabled() {
        return;
    }
    let decision = if accepted { "accept" } else { "reject" };
    let source = match (asked, accepted) {
        (false, _) => "config",
        (true, true) => "user_temporary",
        (true, false) => "user_reject",
    };
    if let Some(name) = edit_tool_name(tool) {
        let path = input_field(input, "path")
            .or_else(|| input_field(input, "file_path"))
            .or_else(|| input_field(input, "notebook_path"))
            .unwrap_or_default();
        add_counter(
            "code_edit_tool.decision",
            1.0,
            &[
                ("tool_name", name.to_string()),
                ("decision", decision.to_string()),
                ("source", source.to_string()),
                ("language", language_of(&path).to_string()),
            ],
        );
    }
    add_event(
        "tool_decision",
        vec![
            ("tool_name", json!(tool)),
            ("decision", json!(decision)),
            ("source", json!(source)),
        ],
    );
}

static ASKED: AtomicBool = AtomicBool::new(false);

/// L'utilisateur vient d'être interrogé sur une autorisation.
pub fn note_asked() {
    ASKED.store(true, Ordering::SeqCst);
}

/// Résultat d'un outil ; compte aussi les lignes modifiées, commits et
/// pull requests.
pub fn tool_result(tool: &str, input: &str, success: bool) {
    let Some(telemetry) = telemetry() else {
        return;
    };
    let mut extra = vec![("tool_name", json!(tool)), ("success", json!(success))];
    if telemetry.config.log_tool_details {
        extra.push((
            "tool_parameters",
            Value::String(input.chars().take(4_000).collect()),
        ));
    }
    add_event("tool_result", extra);
    if !success {
        return;
    }
    let model = telemetry
        .state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .model
        .clone()
        .unwrap_or_default();
    let lines = |text: Option<String>| text.map_or(0, |text| text.lines().count());
    match tool {
        "edit_file" | "Edit" => {
            let removed = lines(input_field(input, "old_string"));
            let added = lines(input_field(input, "new_string"));
            lines_changed(added, removed, &model);
        }
        "write_file" | "Write" => lines_changed(lines(input_field(input, "content")), 0, &model),
        "bash" | "Bash" | "PowerShell" => {
            let command = input_field(input, "command").unwrap_or_default();
            // `--amend` modifie un commit existant : il ne compte pas.
            if crate::settings::runs_git_commit(&command) && !command.contains("--amend") {
                add_counter("commit.count", 1.0, &[]);
            }
            let words: Vec<&str> = command.split_whitespace().collect();
            let creates_pr = words
                .windows(3)
                .any(|window| matches!(window, ["gh", "pr", "create"] | ["glab", "mr", "create"]));
            if creates_pr {
                add_counter("pull_request.count", 1.0, &[]);
            }
        }
        _ => {}
    }
}

fn lines_changed(added: usize, removed: usize, model: &str) {
    #[allow(clippy::cast_precision_loss)]
    for (kind, count) in [("added", added), ("removed", removed)] {
        add_counter(
            "lines_of_code.count",
            count as f64,
            &[("type", kind.to_string()), ("model", model.to_string())],
        );
    }
}

/// Temps passé par Claw sur un tour (`active_time.total`, type `cli`).
pub fn active_time(duration: Duration) {
    add_counter(
        "active_time.total",
        duration.as_secs_f64(),
        &[("type", "cli".to_string())],
    );
}

// --- Export ------------------------------------------------------------------------

fn resource(telemetry: &Telemetry) -> Value {
    let mut attributes = vec![
        attribute("service.name", &json!("claw-code")),
        attribute("service.version", &json!(telemetry.version)),
    ];
    for (key, value) in &telemetry.config.resource {
        attributes.push(attribute(key, &json!(value)));
    }
    json!({ "attributes": attributes })
}

fn scope(telemetry: &Telemetry) -> Value {
    json!({ "name": "claw_code", "version": telemetry.version })
}

fn metric_unit(name: &str) -> &'static str {
    if name.ends_with("cost.usage") {
        "USD"
    } else if name.ends_with("token.usage") {
        "tokens"
    } else if name.ends_with("active_time.total") {
        "s"
    } else {
        ""
    }
}

/// Corps OTLP des compteurs depuis le dernier envoi (ou depuis le début).
fn metrics_payload(telemetry: &Telemetry, state: &mut State) -> Option<Value> {
    let now = now_ns();
    let (points, start) = if telemetry.config.cumulative {
        (state.totals.clone(), state.started_ns)
    } else {
        (
            std::mem::take(&mut state.counters),
            state.last_metric_export_ns,
        )
    };
    state.counters.clear();
    state.last_metric_export_ns = now;
    if points.is_empty() {
        return None;
    }
    let mut by_name: BTreeMap<String, Vec<Value>> = BTreeMap::new();
    for ((name, attributes), value) in points {
        by_name.entry(name).or_default().push(json!({
            "attributes": attributes
                .iter()
                .map(|(key, value)| attribute(key, &json!(value)))
                .collect::<Vec<_>>(),
            "startTimeUnixNano": start.to_string(),
            "timeUnixNano": now.to_string(),
            "asDouble": value,
        }));
    }
    let metrics: Vec<Value> = by_name
        .into_iter()
        .map(|(name, points)| {
            json!({
                "name": name,
                "unit": metric_unit(&name),
                "sum": {
                    "aggregationTemporality": if telemetry.config.cumulative { 2 } else { 1 },
                    "isMonotonic": true,
                    "dataPoints": points,
                }
            })
        })
        .collect();
    Some(json!({
        "resourceMetrics": [{
            "resource": resource(telemetry),
            "scopeMetrics": [{ "scope": scope(telemetry), "metrics": metrics }]
        }]
    }))
}

fn logs_payload(telemetry: &Telemetry, state: &mut State) -> Option<Value> {
    let records = std::mem::take(&mut state.logs);
    if records.is_empty() {
        return None;
    }
    Some(json!({
        "resourceLogs": [{
            "resource": resource(telemetry),
            "scopeLogs": [{ "scope": scope(telemetry), "logRecords": records }]
        }]
    }))
}

/// Un signal prêt à partir : exporteurs, protocole, adresse, en-têtes et
/// encodage protobuf de son message.
struct Signal<'a> {
    exporters: &'a [Exporter],
    protocol: Protocol,
    url: &'a str,
    headers: &'a [(String, String)],
    encode: fn(&Value) -> Vec<u8>,
}

fn deliver(telemetry: &Telemetry, signal: &Signal<'_>, body: &Value) {
    let text = body.to_string();
    for exporter in signal.exporters {
        match exporter {
            Exporter::Console => eprintln!("[télémétrie] {text}"),
            Exporter::Otlp => {
                let (content_type, payload) = match signal.protocol {
                    Protocol::HttpJson => ("application/json", text.as_bytes().to_vec()),
                    Protocol::HttpProtobuf => ("application/x-protobuf", (signal.encode)(body)),
                    Protocol::Grpc => ("application/grpc", (signal.encode)(body)),
                };
                let export = Export {
                    url: signal.url,
                    headers: signal.headers,
                    content_type,
                    body: &payload,
                    grpc: signal.protocol == Protocol::Grpc,
                };
                let url = signal.url;
                let sender = telemetry
                    .sender
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                if let Some(send) = sender.as_ref() {
                    if let Err(error) = send(&export) {
                        static WARNED: AtomicBool = AtomicBool::new(false);
                        if !WARNED.swap(true, Ordering::SeqCst) {
                            eprintln!("[télémétrie] envoi impossible vers {url} : {error}");
                        }
                    }
                }
            }
        }
    }
}

/// Envoie ce qui attend : événements, et compteurs si `metrics` est vrai.
fn flush_now(metrics: bool) {
    let Some(telemetry) = telemetry() else {
        return;
    };
    let (metric_body, log_body) = {
        let mut state = telemetry
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let metric_body = if metrics {
            metrics_payload(telemetry, &mut state)
        } else {
            None
        };
        (metric_body, logs_payload(telemetry, &mut state))
    };
    let config = &telemetry.config;
    if let Some(body) = log_body {
        let signal = Signal {
            exporters: &config.logs,
            protocol: config.logs_protocol,
            url: &config.logs_endpoint,
            headers: &config.logs_headers,
            encode: crate::otlp_proto::logs_request,
        };
        deliver(telemetry, &signal, &body);
    }
    if let Some(body) = metric_body {
        let signal = Signal {
            exporters: &config.metrics,
            protocol: config.metrics_protocol,
            url: &config.metrics_endpoint,
            headers: &config.metrics_headers,
            encode: crate::otlp_proto::metrics_request,
        };
        deliver(telemetry, &signal, &body);
    }
}

fn start_exporter() {
    static STARTED: AtomicBool = AtomicBool::new(false);
    let Some(telemetry) = telemetry() else {
        return;
    };
    if STARTED.swap(true, Ordering::SeqCst) {
        return;
    }
    let logs_every = telemetry.config.logs_interval;
    let metrics_every = telemetry.config.metric_interval;
    let _ = std::thread::Builder::new()
        .name("claw-telemetry".to_string())
        .spawn(move || {
            let mut last_metrics = Instant::now();
            loop {
                std::thread::sleep(logs_every.min(metrics_every));
                let metrics_due = last_metrics.elapsed() >= metrics_every;
                if metrics_due {
                    last_metrics = Instant::now();
                }
                flush_now(metrics_due);
            }
        });
}

/// À la sortie : tout ce qui attend part.
pub fn shutdown() {
    if enabled() {
        flush_now(true);
    }
}

#[cfg(test)]
mod tests {
    use super::{language_of, origin, parse_pairs, percent_decode};

    #[test]
    fn headers_and_languages_parse() {
        assert_eq!(
            parse_pairs("Authorization=Bearer%20abc, x-team = core"),
            vec![
                ("Authorization".to_string(), "Bearer abc".to_string()),
                ("x-team".to_string(), "core".to_string())
            ]
        );
        assert_eq!(percent_decode("a%2Cb%"), "a,b%");
        assert_eq!(language_of("src/main.rs"), "Rust");
        assert_eq!(language_of("README"), "unknown");
        assert_eq!(origin("http://otel:4317/v1/metrics"), "http://otel:4317");
        assert_eq!(origin("localhost:4317"), "http://localhost:4317");
    }
}
