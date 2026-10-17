//! Workflows dynamiques (comme dans Claude Code) : un script JavaScript
//! orchestre des agents en arrière-plan, par phases, et rend un seul
//! résultat, remis au modèle entre deux tours.
//!
//! Le script tourne sous Node.js (20 ou plus) dans un contexte `vm` vide, et
//! `node` est lancé avec son modèle de permissions : le script ne voit ni
//! fichiers, ni processus, et ne peut qu'appeler les agents de Claw
//! (`agent()`, `parallel()`, `pipeline()`, `phase()`, `log()`). Chaque agent
//! est un sous-agent de Claw, avec les permissions de la session, qui ne peut
//! rien demander à l'utilisateur : une action à approuver est refusée.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::io::{BufRead, BufReader, Read, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};

const RUNNER: &str = include_str!("workflow_runner.mjs");
/// Référence montrée par `/workflow-authoring`.
pub const AUTHORING: &str = include_str!("workflow_authoring.md");
/// Workflows fournis avec Claw.
const BUNDLED: &[(&str, &str)] = &[("deep-research", include_str!("workflows/deep-research.js"))];

/// Agents au plus par exécution (`CLAW_WORKFLOW_MAX_AGENTS`) : chacun coûte
/// une dizaine de requêtes, et le quota gratuit est compté.
const DEFAULT_MAX_AGENTS: usize = 30;
/// Agents qui tournent ensemble (`CLAW_WORKFLOW_MAX_CONCURRENT_AGENTS`) :
/// les modèles gratuits limitent les requêtes par minute.
const DEFAULT_MAX_CONCURRENT: usize = 3;
const MAX_LOGS: usize = 200;
const MAX_RESULT_CHARS: usize = 30_000;
/// Délai accordé à `node` pour lire le script et le compiler.
const START_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunState {
    Running,
    Done,
    Failed,
    Stopped,
}

impl RunState {
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Running => "en cours",
            Self::Done => "terminé",
            Self::Failed => "échec",
            Self::Stopped => "arrêté",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AgentState {
    Waiting,
    Running,
    Done,
    Failed,
    Reused,
    Skipped,
}

impl AgentState {
    fn label(self) -> &'static str {
        match self {
            Self::Waiting => "en attente",
            Self::Running => "en cours",
            Self::Done => "fini",
            Self::Failed => "échec",
            Self::Reused => "repris",
            Self::Skipped => "écarté",
        }
    }
}

#[derive(Debug, Clone)]
struct AgentCall {
    label: String,
    phase: Option<String>,
    state: AgentState,
    started: Option<Instant>,
    elapsed: Option<Duration>,
    transcript: Option<String>,
    error: Option<String>,
}

struct Run {
    id: String,
    name: String,
    description: String,
    phases: Vec<String>,
    current_phase: Option<String>,
    state: RunState,
    started: Instant,
    ended: Option<Instant>,
    agents: Vec<AgentCall>,
    logs: Vec<String>,
    result: Option<String>,
    error: Option<String>,
    /// Résultat remis au modèle (ou arrêt demandé par l'utilisateur).
    delivered: bool,
    dir: PathBuf,
    child: Option<Arc<Mutex<Child>>>,
    stop: Arc<AtomicBool>,
}

fn runs() -> &'static Mutex<Vec<Run>> {
    static RUNS: OnceLock<Mutex<Vec<Run>>> = OnceLock::new();
    RUNS.get_or_init(|| Mutex::new(Vec::new()))
}

fn lock() -> std::sync::MutexGuard<'static, Vec<Run>> {
    runs()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn with_run<T>(id: &str, change: impl FnOnce(&mut Run) -> T) -> Option<T> {
    lock().iter_mut().find(|run| run.id == id).map(change)
}

fn env_flag(names: &[&str]) -> bool {
    names.iter().any(|name| {
        std::env::var(name).is_ok_and(|value| matches!(value.trim(), "1" | "true" | "yes"))
    })
}

fn env_number(names: &[&str], default: usize, max: usize) -> usize {
    names
        .iter()
        .find_map(|name| std::env::var(name).ok()?.trim().parse::<usize>().ok())
        .filter(|value| *value > 0)
        .map_or(default, |value| value.min(max))
}

/// `CLAW_DISABLE_WORKFLOWS=1` (ou celle de Claude Code), ou le réglage
/// `"disableWorkflows": true`.
#[must_use]
pub fn disabled() -> bool {
    env_flag(&["CLAW_DISABLE_WORKFLOWS", "CLAUDE_CODE_DISABLE_WORKFLOWS"])
        || std::env::current_dir()
            .ok()
            .and_then(|cwd| runtime::ConfigLoader::default_for(cwd).load().ok())
            .is_some_and(|config| {
                config
                    .get("disableWorkflows")
                    .and_then(runtime::JsonValue::as_bool)
                    == Some(true)
            })
}

fn max_agents() -> usize {
    env_number(&["CLAW_WORKFLOW_MAX_AGENTS"], DEFAULT_MAX_AGENTS, 1_000)
}

fn max_concurrent() -> usize {
    env_number(
        &[
            "CLAW_WORKFLOW_MAX_CONCURRENT_AGENTS",
            "CLAUDE_CODE_WORKFLOW_MAX_CONCURRENT_AGENTS",
        ],
        DEFAULT_MAX_CONCURRENT,
        256,
    )
}

/// Modèle imposé à tous les agents des workflows, s'il y en a un.
fn forced_model() -> Option<String> {
    [
        "CLAW_WORKFLOW_SUBAGENT_MODEL",
        "CLAUDE_CODE_WORKFLOW_SUBAGENT_MODEL",
    ]
    .iter()
    .find_map(|name| {
        std::env::var(name)
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
    })
}

fn hide_window(command: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    #[cfg(not(windows))]
    let _ = command;
}

/// `node` et l'option de son modèle de permissions (`--permission` depuis
/// Node 23, `--experimental-permission` avant).
fn node() -> Result<(String, &'static str), String> {
    static NODE: OnceLock<Result<(String, &'static str), String>> = OnceLock::new();
    NODE.get_or_init(|| {
        let program = std::env::var("CLAW_WORKFLOW_NODE")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| "node".to_string());
        let mut command = Command::new(&program);
        command.arg("--version").stdin(Stdio::null());
        hide_window(&mut command);
        let output = command.output().map_err(|_| {
            "les workflows ont besoin de Node.js 20 ou plus (https://nodejs.org) : la commande node est introuvable".to_string()
        })?;
        let version = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let major = version
            .trim_start_matches('v')
            .split('.')
            .next()
            .and_then(|major| major.parse::<u32>().ok())
            .unwrap_or(0);
        let flag = match major {
            0..=19 => {
                return Err(format!(
                    "les workflows ont besoin de Node.js 20 ou plus (version trouvée : {version})"
                ))
            }
            20..=22 => "--experimental-permission",
            _ => "--permission",
        };
        Ok((program, flag))
    })
    .clone()
}

/// Position du mot `export` de `export const meta =`.
fn find_meta_export(script: &str) -> Option<usize> {
    let mut from = 0;
    while let Some(found) = script[from..].find("export") {
        let at = from + found;
        let rest = &script[at + "export".len()..];
        let declared = rest
            .strip_prefix(char::is_whitespace)
            .map(str::trim_start)
            .and_then(|rest| rest.strip_prefix("const"))
            .and_then(|rest| rest.strip_prefix(char::is_whitespace))
            .map(str::trim_start)
            .and_then(|rest| rest.strip_prefix("meta"))
            .map(str::trim_start)
            .is_some_and(|rest| rest.starts_with('=') && !rest.starts_with("=="));
        if declared {
            return Some(at);
        }
        from = at + "export".len();
    }
    None
}

/// Accolade fermante qui répond à celle de `open`, en sautant chaînes et
/// commentaires.
fn matching_brace(text: &str, open: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut depth = 0usize;
    let mut index = open;
    while index < bytes.len() {
        match bytes[index] {
            b'{' => depth += 1,
            b'}' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(index);
                }
            }
            quote @ (b'\'' | b'"' | b'`') => {
                index += 1;
                while index < bytes.len() && bytes[index] != quote {
                    if bytes[index] == b'\\' {
                        index += 1;
                    }
                    index += 1;
                }
            }
            b'/' if bytes.get(index + 1) == Some(&b'/') => {
                while index < bytes.len() && bytes[index] != b'\n' {
                    index += 1;
                }
            }
            b'/' if bytes.get(index + 1) == Some(&b'*') => {
                index += 2;
                while index + 1 < bytes.len() && !(bytes[index] == b'*' && bytes[index + 1] == b'/')
                {
                    index += 1;
                }
                index += 1;
            }
            _ => {}
        }
        index += 1;
    }
    None
}

/// Sépare le littéral de `export const meta = {...}` ; le corps garde la
/// déclaration, sans `export`.
fn split_meta(script: &str) -> Result<(String, String), String> {
    const USAGE: &str =
        "le script doit commencer par export const meta = { name: '…', description: '…' }";
    let at = find_meta_export(script).ok_or(USAGE)?;
    let after = at + "export".len();
    let equals = after + script[after..].find('=').ok_or(USAGE)?;
    let open = equals
        + 1
        + script[equals + 1..]
            .find(|character: char| !character.is_whitespace())
            .ok_or(USAGE)?;
    if !script[open..].starts_with('{') {
        return Err(format!("{USAGE} (meta doit être un objet littéral)"));
    }
    let close = matching_brace(script, open).ok_or("accolade de meta non refermée")?;
    Ok((
        script[open..=close].to_string(),
        format!("{}{}", &script[..at], &script[after..]),
    ))
}

/// `description: '…'` du littéral meta, pour les listes.
fn meta_description(meta: &str) -> String {
    let Some(at) = meta.find("description") else {
        return String::new();
    };
    let rest = meta[at + "description".len()..].trim_start();
    let Some(rest) = rest.strip_prefix(':').map(str::trim_start) else {
        return String::new();
    };
    let Some(quote) = rest
        .chars()
        .next()
        .filter(|c| matches!(c, '\'' | '"' | '`'))
    else {
        return String::new();
    };
    rest[1..]
        .split(quote)
        .next()
        .unwrap_or_default()
        .to_string()
}

/// Workflow enregistré ou fourni avec Claw.
#[derive(Debug, Clone)]
pub struct SavedWorkflow {
    pub name: String,
    pub description: String,
    /// Fichier, ou « intégré ».
    pub origin: String,
    script: String,
}

fn workflow_dirs() -> Vec<PathBuf> {
    let cwd = std::env::current_dir().unwrap_or_default();
    let config_home = runtime::ConfigLoader::default_for(&cwd)
        .config_home()
        .to_path_buf();
    let mut dirs = vec![
        cwd.join(".claw").join("workflows"),
        cwd.join(".claude").join("workflows"),
        config_home.join("workflows"),
    ];
    if let Some(home) = config_home.parent() {
        dirs.push(home.join(".claude").join("workflows"));
    }
    dirs
}

/// Workflows enregistrés (projet, puis utilisateur), puis intégrés ; le
/// premier d'un nom l'emporte.
#[must_use]
pub fn saved() -> Vec<SavedWorkflow> {
    let mut found: Vec<SavedWorkflow> = Vec::new();
    for dir in workflow_dirs() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        let mut paths: Vec<PathBuf> = entries
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| {
                path.extension()
                    .is_some_and(|ext| ext == "js" || ext == "mjs")
            })
            .collect();
        paths.sort();
        for path in paths {
            let Some(name) = path.file_stem().and_then(|stem| stem.to_str()) else {
                continue;
            };
            if found
                .iter()
                .any(|entry| entry.name.eq_ignore_ascii_case(name))
            {
                continue;
            }
            let Ok(script) = fs::read_to_string(&path) else {
                continue;
            };
            let description = split_meta(&script)
                .map(|(meta, _)| meta_description(&meta))
                .unwrap_or_default();
            found.push(SavedWorkflow {
                name: name.to_string(),
                description,
                origin: path.display().to_string(),
                script,
            });
        }
    }
    for (name, script) in BUNDLED {
        if !found
            .iter()
            .any(|entry| entry.name.eq_ignore_ascii_case(name))
        {
            found.push(SavedWorkflow {
                name: (*name).to_string(),
                description: split_meta(script)
                    .map(|(meta, _)| meta_description(&meta))
                    .unwrap_or_default(),
                origin: "intégré".to_string(),
                script: (*script).to_string(),
            });
        }
    }
    found
}

#[must_use]
pub fn find_saved(name: &str) -> Option<SavedWorkflow> {
    let name = name.trim().trim_start_matches('/');
    saved()
        .into_iter()
        .find(|entry| entry.name.eq_ignore_ascii_case(name))
}

/// Ce qu'il faut pour lancer un workflow (entrée de l'outil `Workflow`).
#[derive(Debug, Clone, Default)]
pub struct StartRequest {
    pub script: Option<String>,
    pub name: Option<String>,
    pub script_path: Option<String>,
    pub args: Option<Value>,
    pub resume_from: Option<String>,
}

fn runs_root() -> PathBuf {
    std::env::current_dir()
        .unwrap_or_default()
        .join(".claw")
        .join("workflows")
        .join("runs")
}

fn new_run_id() -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    SystemTime::now().hash(&mut hasher);
    std::process::id().hash(&mut hasher);
    lock().len().hash(&mut hasher);
    format!("wf-{:08x}", hasher.finish() & 0xffff_ffff)
}

/// Clé d'un appel `agent()` pour la reprise : la demande et ses options.
fn cache_key(message: &Value) -> String {
    json!([
        message["prompt"],
        message["label"],
        message["schema"],
        message["model"],
        message["agentType"],
        message["isolation"],
    ])
    .to_string()
}

fn load_cache(dir: &Path) -> BTreeMap<String, Value> {
    fs::read_to_string(dir.join("cache.jsonl"))
        .unwrap_or_default()
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter_map(|entry| Some((entry["key"].as_str()?.to_string(), entry["result"].clone())))
        .collect()
}

fn resolve_script(request: &StartRequest, previous: Option<&Path>) -> Result<String, String> {
    if let Some(path) = request
        .script_path
        .as_deref()
        .map(str::trim)
        .filter(|path| !path.is_empty())
    {
        return fs::read_to_string(path).map_err(|error| format!("{path} : {error}"));
    }
    if let Some(script) = request
        .script
        .as_deref()
        .filter(|script| !script.trim().is_empty())
    {
        return Ok(script.to_string());
    }
    if let Some(name) = request
        .name
        .as_deref()
        .map(str::trim)
        .filter(|name| !name.is_empty())
    {
        return find_saved(name).map(|entry| entry.script).ok_or_else(|| {
            format!("aucun workflow « {name} » (.claw/workflows, .claude/workflows ou intégré)")
        });
    }
    if let Some(dir) = previous {
        return fs::read_to_string(dir.join("script.js")).map_err(|error| error.to_string());
    }
    Err("donnez script, name ou scriptPath".to_string())
}

/// Lance un workflow en arrière-plan ; rend aussitôt son identifiant. Une
/// erreur de syntaxe ou un `meta` invalide sont signalés ici, avant tout agent.
#[allow(clippy::too_many_lines)]
pub fn start(request: &StartRequest) -> Result<Value, String> {
    if disabled() {
        return Err(
            "les workflows sont désactivés (CLAW_DISABLE_WORKFLOWS ou disableWorkflows)"
                .to_string(),
        );
    }
    let previous = request
        .resume_from
        .as_deref()
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(|id| runs_root().join(id));
    if let Some(dir) = &previous {
        if !dir.is_dir() {
            return Err(format!("exécution introuvable : {}", dir.display()));
        }
    }
    let reused = Arc::new(previous.as_deref().map(load_cache).unwrap_or_default());
    let args = request.args.clone().or_else(|| {
        previous
            .as_ref()
            .and_then(|dir| fs::read_to_string(dir.join("args.json")).ok())
            .and_then(|text| serde_json::from_str(&text).ok())
    });
    // Un tableau ou un objet passé comme texte JSON est relu.
    let args = match args {
        Some(Value::String(text)) if text.trim_start().starts_with(['[', '{']) => {
            Some(serde_json::from_str(&text).unwrap_or(Value::String(text)))
        }
        other => other,
    };
    let script = resolve_script(request, previous.as_deref())?;
    let (meta, body) = split_meta(&script)?;
    let (program, permission_flag) = node()?;

    let id = new_run_id();
    let dir = runs_root().join(&id);
    fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    fs::write(dir.join("script.js"), &script).map_err(|error| error.to_string())?;
    if let Some(args) = &args {
        let _ = fs::write(
            dir.join("args.json"),
            serde_json::to_string_pretty(args).unwrap_or_default(),
        );
    }
    let runner = dir.join("runner.mjs");
    fs::write(&runner, RUNNER).map_err(|error| error.to_string())?;

    let mut command = Command::new(&program);
    command
        .arg(permission_flag)
        .arg(format!("--allow-fs-read={}", runner.display()))
        .arg(&runner)
        .current_dir(std::env::current_dir().unwrap_or_default())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    hide_window(&mut command);
    let mut child = command
        .spawn()
        .map_err(|error| format!("lancement de node impossible : {error}"))?;
    let (Some(mut stdin), Some(stdout), Some(mut stderr)) =
        (child.stdin.take(), child.stdout.take(), child.stderr.take())
    else {
        let _ = child.kill();
        return Err("tuyaux de node indisponibles".to_string());
    };
    let mut init = json!({ "meta": meta, "body": body });
    if let Some(args) = &args {
        init["args"] = args.clone();
    }
    if writeln!(stdin, "{init}")
        .and_then(|()| stdin.flush())
        .is_err()
    {
        let _ = child.kill();
        return Err("node a refusé le script".to_string());
    }
    let errors = Arc::new(Mutex::new(String::new()));
    {
        let errors = Arc::clone(&errors);
        let _ = std::thread::Builder::new()
            .name("claw-workflow-stderr".to_string())
            .spawn(move || {
                let mut text = String::new();
                let _ = stderr.read_to_string(&mut text);
                *errors
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) = text;
            });
    }
    let child = Arc::new(Mutex::new(child));
    let stop = Arc::new(AtomicBool::new(false));
    lock().push(Run {
        id: id.clone(),
        name: String::new(),
        description: String::new(),
        phases: Vec::new(),
        current_phase: None,
        state: RunState::Running,
        started: Instant::now(),
        ended: None,
        agents: Vec::new(),
        logs: Vec::new(),
        result: None,
        error: None,
        delivered: false,
        dir: dir.clone(),
        child: Some(Arc::clone(&child)),
        stop: Arc::clone(&stop),
    });
    let (ready_sender, ready) = mpsc::channel();
    let controller = Controller {
        id: id.clone(),
        stdin: Arc::new(Mutex::new(stdin)),
        stop,
        reused,
        dir: dir.clone(),
        errors,
        child,
    };
    std::thread::Builder::new()
        .name(format!("claw-{id}"))
        .spawn(move || controller.run(stdout, ready_sender))
        .map_err(|error| error.to_string())?;

    match ready.recv_timeout(START_TIMEOUT) {
        Ok(Ok((name, description))) => Ok(json!({
            "status": "async_launched",
            "taskId": id,
            "runId": id,
            "workflowName": name,
            "summary": description,
            "transcriptDir": dir.display().to_string(),
            "scriptPath": dir.join("script.js").display().to_string(),
            "note": "The workflow runs in the background. Its result reaches you later as a separate message between turns: do not wait, poll or start it again. Tell the user it is running (/workflows to follow it) and end your turn or keep working.",
        })),
        Ok(Err(error)) => {
            discard(&id);
            Err(format!("le workflow n'a pas démarré : {error}"))
        }
        Err(_) => {
            let _ = stop_run(&id);
            discard(&id);
            Err("node n'a pas répondu à temps".to_string())
        }
    }
}

/// Exécution refusée au démarrage : elle ne reste pas dans la liste.
fn discard(id: &str) {
    lock().retain(|run| run.id != id);
}

/// Thread qui lit les demandes du script et répond.
struct Controller {
    id: String,
    stdin: Arc<Mutex<ChildStdin>>,
    stop: Arc<AtomicBool>,
    reused: Arc<BTreeMap<String, Value>>,
    dir: PathBuf,
    errors: Arc<Mutex<String>>,
    child: Arc<Mutex<Child>>,
}

impl Controller {
    #[allow(clippy::too_many_lines)]
    fn run(
        self,
        stdout: std::process::ChildStdout,
        ready: mpsc::Sender<Result<(String, String), String>>,
    ) {
        let mut ready = Some(ready);
        let mut meta = (String::new(), String::new());
        for line in BufReader::new(stdout).lines() {
            let Ok(line) = line else {
                break;
            };
            let Ok(message) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            let text = |key: &str| message[key].as_str().unwrap_or_default().to_string();
            match message["op"].as_str().unwrap_or_default() {
                "meta" => {
                    let name = message["meta"]["name"]
                        .as_str()
                        .unwrap_or_default()
                        .to_string();
                    let description = message["meta"]["description"]
                        .as_str()
                        .unwrap_or_default()
                        .to_string();
                    let phases: Vec<String> = message["meta"]["phases"]
                        .as_array()
                        .map(|phases| {
                            phases
                                .iter()
                                .filter_map(|phase| {
                                    phase["title"]
                                        .as_str()
                                        .or(phase.as_str())
                                        .map(str::to_string)
                                })
                                .collect()
                        })
                        .unwrap_or_default();
                    with_run(&self.id, |run| {
                        run.name.clone_from(&name);
                        run.description.clone_from(&description);
                        run.phases = phases;
                    });
                    meta = (name, description);
                }
                "ready" => {
                    if let Some(sender) = ready.take() {
                        let _ = sender.send(Ok(meta.clone()));
                    }
                }
                "agent" => self.spawn_agent(&message),
                "phase" => {
                    let title = text("title");
                    with_run(&self.id, |run| {
                        if !run.phases.contains(&title) {
                            run.phases.push(title.clone());
                        }
                        run.current_phase = Some(title);
                    });
                }
                "log" => {
                    let entry = text("text");
                    with_run(&self.id, |run| {
                        if run.logs.len() >= MAX_LOGS {
                            run.logs.remove(0);
                        }
                        run.logs.push(entry);
                    });
                }
                "done" => {
                    let result = text("result");
                    with_run(&self.id, |run| {
                        run.result = Some(result);
                        if run.state == RunState::Running {
                            run.state = RunState::Done;
                        }
                    });
                }
                "error" => {
                    let error = text("text");
                    if let Some(sender) = ready.take() {
                        let _ = sender.send(Err(error.clone()));
                    }
                    with_run(&self.id, |run| {
                        run.error = Some(error);
                        if run.state == RunState::Running {
                            run.state = RunState::Failed;
                        }
                    });
                }
                _ => {}
            }
        }
        let _ = self
            .child
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .wait();
        // Laisse au lecteur des erreurs le temps de finir.
        std::thread::sleep(Duration::from_millis(100));
        let errors = self
            .errors
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .trim()
            .chars()
            .rev()
            .take(1_500)
            .collect::<String>()
            .chars()
            .rev()
            .collect::<String>();
        if let Some(sender) = ready.take() {
            let _ = sender.send(Err(if errors.is_empty() {
                "node s'est arrêté".to_string()
            } else {
                errors.clone()
            }));
        }
        let stopped = self.stop.load(Ordering::SeqCst);
        with_run(&self.id, |run| {
            if run.state == RunState::Running {
                run.state = if stopped {
                    RunState::Stopped
                } else {
                    RunState::Failed
                };
                if run.error.is_none() && !stopped {
                    run.error = Some(if errors.is_empty() {
                        "le script s'est arrêté sans résultat".to_string()
                    } else {
                        errors
                    });
                }
            }
            run.ended = Some(Instant::now());
            run.child = None;
            save_summary(run);
        });
        let _ = fs::remove_file(self.dir.join("runner.mjs"));
    }

    fn spawn_agent(&self, message: &Value) {
        let call = message["id"].as_u64().unwrap_or_default();
        let label = message["label"].as_str().map_or_else(
            || short_label(message["prompt"].as_str().unwrap_or_default()),
            str::to_string,
        );
        let phase = message["phase"].as_str().map(str::to_string);
        let Some(index) = with_run(&self.id, |run| {
            run.agents.push(AgentCall {
                label: label.clone(),
                phase,
                state: AgentState::Waiting,
                started: None,
                elapsed: None,
                transcript: None,
                error: None,
            });
            run.agents.len() - 1
        }) else {
            return;
        };
        let id = self.id.clone();
        let stdin = Arc::clone(&self.stdin);
        let stop = Arc::clone(&self.stop);
        let reused = Arc::clone(&self.reused);
        let dir = self.dir.clone();
        let message = message.clone();
        let _ = std::thread::Builder::new()
            .name(format!("claw-{id}-agent-{}", index + 1))
            .spawn(move || {
                let key = cache_key(&message);
                let set = |state: AgentState| {
                    with_run(&id, |run| {
                        if let Some(agent) = run.agents.get_mut(index) {
                            agent.state = state;
                        }
                    });
                };
                let result = if index >= max_agents() {
                    set(AgentState::Skipped);
                    log(&id, &format!(
                        "agent « {label} » écarté : limite de {} agents par exécution (CLAW_WORKFLOW_MAX_AGENTS)",
                        max_agents()
                    ));
                    Value::Null
                } else if let Some(value) = reused.get(&key) {
                    set(AgentState::Reused);
                    value.clone()
                } else {
                    run_agent(&id, index, &label, &message, &stop, &dir, &key)
                };
                let line = json!({ "id": call, "result": result }).to_string();
                let mut stdin = stdin
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                let _ = writeln!(stdin, "{line}");
                let _ = stdin.flush();
            });
    }
}

fn log(id: &str, text: &str) {
    with_run(id, |run| {
        if run.logs.len() >= MAX_LOGS {
            run.logs.remove(0);
        }
        run.logs.push(text.to_string());
    });
}

/// Six premiers mots d'une demande (nom d'un agent sans `label`).
fn short_label(prompt: &str) -> String {
    let words: Vec<&str> = prompt.split_whitespace().take(6).collect();
    let mut label = words.join(" ");
    if prompt.split_whitespace().count() > 6 {
        label.push('…');
    }
    label
}

/// Place parmi les agents qui tournent ensemble (tous workflows confondus).
struct Slot;

fn slots() -> &'static (Mutex<usize>, Condvar) {
    static SLOTS: OnceLock<(Mutex<usize>, Condvar)> = OnceLock::new();
    SLOTS.get_or_init(|| (Mutex::new(0), Condvar::new()))
}

impl Slot {
    /// Attend une place ; `None` si l'exécution est arrêtée entre-temps.
    fn acquire(stop: &AtomicBool) -> Option<Self> {
        let (count, freed) = slots();
        let mut busy = count
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        loop {
            if stop.load(Ordering::SeqCst) {
                return None;
            }
            if *busy < max_concurrent() {
                *busy += 1;
                return Some(Self);
            }
            busy = freed
                .wait_timeout(busy, Duration::from_millis(500))
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .0;
        }
    }
}

impl Drop for Slot {
    fn drop(&mut self) {
        let (count, freed) = slots();
        let mut busy = count
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *busy = busy.saturating_sub(1);
        freed.notify_all();
    }
}

fn run_agent(
    id: &str,
    index: usize,
    label: &str,
    message: &Value,
    stop: &AtomicBool,
    dir: &Path,
    key: &str,
) -> Value {
    let update = |change: &dyn Fn(&mut AgentCall)| {
        with_run(id, |run| {
            if let Some(agent) = run.agents.get_mut(index) {
                change(agent);
            }
        });
    };
    let Some(slot) = Slot::acquire(stop) else {
        update(&|agent| agent.state = AgentState::Skipped);
        return Value::Null;
    };
    let started = Instant::now();
    update(&|agent| {
        agent.state = AgentState::Running;
        agent.started = Some(started);
    });
    let schema = message.get("schema").filter(|schema| !schema.is_null());
    let prompt = message["prompt"].as_str().unwrap_or_default();
    let prompt = match schema {
        Some(schema) => format!(
            "{prompt}\n\nWhen you are done, give as your final answer only a JSON value that matches this JSON Schema (no prose, no code fence):\n{schema}"
        ),
        None => prompt.to_string(),
    };
    let model = forced_model().or_else(|| message["model"].as_str().map(str::to_string));
    let outcome = crate::run_workflow_agent(
        label,
        &prompt,
        model.as_deref(),
        message["agentType"].as_str(),
        message["isolation"].as_str(),
    );
    drop(slot);
    let (value, transcript) = match outcome {
        Ok((report, transcript)) => (
            match schema {
                Some(schema) => conform(&report, schema, model.as_deref()),
                None => Ok(Value::String(report)),
            },
            Some(transcript),
        ),
        Err(error) => (Err(error), None),
    };
    let elapsed = started.elapsed();
    match value {
        Ok(value) => {
            update(&|agent| {
                agent.state = AgentState::Done;
                agent.elapsed = Some(elapsed);
                agent.transcript.clone_from(&transcript);
            });
            let line = json!({ "key": key, "result": value }).to_string();
            let _ = fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(dir.join("cache.jsonl"))
                .and_then(|mut file| writeln!(file, "{line}"));
            value
        }
        Err(error) => {
            log(id, &format!("agent « {label} » en échec : {error}"));
            update(&|agent| {
                agent.state = AgentState::Failed;
                agent.elapsed = Some(elapsed);
                agent.transcript.clone_from(&transcript);
                agent.error = Some(error.clone());
            });
            Value::Null
        }
    }
}

const CONFORM_SYSTEM: &str =
    "You convert a report into one JSON value that matches a given JSON Schema exactly. Reply with the JSON only: no prose, no code fence.";

/// Réponse d'un agent mise au format de `schema` : lue telle quelle, sinon
/// convertie par une requête sans outils.
fn conform(report: &str, schema: &Value, model: Option<&str>) -> Result<Value, String> {
    if let Some(value) = parse_json_answer(report).filter(|value| matches_schema(value, schema, 0))
    {
        return Ok(value);
    }
    let mut models: Vec<String> = model.map(str::to_string).into_iter().collect();
    if let Some(context) = crate::current_subagent_context() {
        models.push(context.model);
        models.extend(context.fallback_models);
    }
    models.retain(|model| !model.trim().is_empty());
    models.dedup();
    if models.is_empty() {
        return Err("la réponse de l'agent ne suit pas le schéma demandé".to_string());
    }
    let prompt = format!("JSON Schema:\n{schema}\n\nReport:\n{report}");
    let opinion = crate::advisors::ask(&models, CONFORM_SYSTEM, &prompt, 4_000)?;
    parse_json_answer(&opinion.text)
        .filter(|value| matches_schema(value, schema, 0))
        .ok_or_else(|| "la réponse de l'agent ne suit pas le schéma demandé".to_string())
}

/// Valeur JSON d'une réponse : entière, dans un bloc de code, ou entre la
/// première et la dernière accolade (ou crochet).
fn parse_json_answer(text: &str) -> Option<Value> {
    let text = text.trim();
    let fenced = text
        .strip_prefix("```")
        .and_then(|rest| rest.split_once('\n'))
        .map(|(_, rest)| rest.trim_end().trim_end_matches("```").trim());
    let sliced = text
        .find(['{', '['])
        .zip(text.rfind(['}', ']']))
        .filter(|(start, end)| start < end)
        .map(|(start, end)| &text[start..=end]);
    for candidate in [Some(text), fenced, sliced].into_iter().flatten() {
        if let Ok(value) = serde_json::from_str::<Value>(candidate) {
            return Some(value);
        }
        if let Some(value) = runtime::json_repair::repair_tool_input(candidate)
            .and_then(|fixed| serde_json::from_str::<Value>(&fixed).ok())
        {
            return Some(value);
        }
    }
    None
}

/// Contrôle simple d'un JSON Schema : type, champs requis, propriétés et
/// éléments.
fn matches_schema(value: &Value, schema: &Value, depth: usize) -> bool {
    let Some(schema) = schema.as_object() else {
        return true;
    };
    if depth > 8 {
        return true;
    }
    let typed = match schema.get("type").and_then(Value::as_str) {
        Some("object") => value.is_object(),
        Some("array") => value.is_array(),
        Some("string") => value.is_string(),
        Some("number") => value.is_number(),
        Some("integer") => value.is_i64() || value.is_u64(),
        Some("boolean") => value.is_boolean(),
        Some("null") => value.is_null(),
        _ => true,
    };
    if !typed {
        return false;
    }
    if let Some(object) = value.as_object() {
        let required = schema
            .get("required")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or_default();
        if required
            .iter()
            .filter_map(Value::as_str)
            .any(|key| !object.contains_key(key))
        {
            return false;
        }
        if let Some(properties) = schema.get("properties").and_then(Value::as_object) {
            for (key, property) in properties {
                if let Some(item) = object.get(key) {
                    if !matches_schema(item, property, depth + 1) {
                        return false;
                    }
                }
            }
        }
    }
    if let (Some(items), Some(item_schema)) = (value.as_array(), schema.get("items")) {
        return items
            .iter()
            .all(|item| matches_schema(item, item_schema, depth + 1));
    }
    true
}

fn duration_text(duration: Duration) -> String {
    let seconds = duration.as_secs();
    if seconds < 60 {
        format!("{seconds} s")
    } else if seconds < 3_600 {
        format!("{} min {:02} s", seconds / 60, seconds % 60)
    } else {
        format!("{} h {:02} min", seconds / 3_600, (seconds % 3_600) / 60)
    }
}

fn elapsed(run: &Run) -> Duration {
    run.ended.unwrap_or_else(Instant::now) - run.started
}

fn count(run: &Run, state: AgentState) -> usize {
    run.agents
        .iter()
        .filter(|agent| agent.state == state)
        .count()
}

/// Résumé écrit à la fin d'une exécution (`result.json`).
fn save_summary(run: &Run) {
    let agents: Vec<Value> = run
        .agents
        .iter()
        .map(|agent| {
            json!({
                "label": agent.label,
                "phase": agent.phase,
                "state": agent.state.label(),
                "seconds": agent.elapsed.map(|elapsed| elapsed.as_secs()),
                "transcript": agent.transcript,
                "error": agent.error,
            })
        })
        .collect();
    let finished_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs());
    let summary = json!({
        "id": run.id,
        "name": run.name,
        "state": run.state.label(),
        "finishedAt": finished_at,
        "seconds": elapsed(run).as_secs(),
        "result": run.result.as_deref().and_then(|text| serde_json::from_str::<Value>(text).ok()),
        "error": run.error,
        "agents": agents,
        "logs": run.logs,
    });
    let _ = fs::write(
        run.dir.join("result.json"),
        serde_json::to_string_pretty(&summary).unwrap_or_default(),
    );
}

/// Exécution finie dont le résultat attend d'être remis au modèle.
#[must_use]
pub fn has_finished() -> bool {
    lock()
        .iter()
        .any(|run| run.ended.is_some() && !run.delivered)
}

/// Exécution finie : la ligne pour l'utilisateur et le message pour le modèle.
#[derive(Debug, Clone)]
pub struct Finished {
    pub headline: String,
    pub for_model: String,
}

fn shortened(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_string();
    }
    let mut short: String = text.chars().take(limit).collect();
    short.push_str("\n... (shortened; the full result is in result.json)");
    short
}

/// Résultats des exécutions finies, à remettre une fois.
#[must_use]
pub fn take_finished() -> Vec<Finished> {
    let mut list = lock();
    let mut finished = Vec::new();
    for run in list.iter_mut() {
        if run.ended.is_none() || run.delivered {
            continue;
        }
        run.delivered = true;
        let total = run.agents.len();
        let failed = count(run, AgentState::Failed);
        let reused = count(run, AgentState::Reused);
        let duration = duration_text(elapsed(run));
        let headline = match run.state {
            RunState::Done => format!(
                "※ Workflow {} terminé · {total} agent(s) · {duration}",
                run.name
            ),
            _ => format!(
                "※ Workflow {} : {} · {}",
                run.name,
                run.state.label(),
                run.error
                    .as_deref()
                    .unwrap_or_default()
                    .lines()
                    .next()
                    .unwrap_or_default()
            ),
        };
        let outcome = match run.state {
            RunState::Done => "finished",
            RunState::Stopped => "was stopped",
            _ => "failed",
        };
        let mut message = format!(
            "[Claw] Workflow \"{}\" (run {}) {outcome} after {duration}: {total} agent call(s), {failed} failed, {reused} reused.",
            run.name, run.id
        );
        if let Some(result) = &run.result {
            let _ = write!(
                message,
                "\nResult (JSON):\n{}",
                shortened(result, MAX_RESULT_CHARS)
            );
        }
        if let Some(error) = &run.error {
            let _ = write!(message, "\nError: {}", shortened(error, 2_000));
        }
        let failures: Vec<String> = run
            .agents
            .iter()
            .filter_map(|agent| {
                agent
                    .error
                    .as_ref()
                    .map(|error| format!("- {}: {}", agent.label, shortened(error, 200)))
            })
            .take(5)
            .collect();
        if !failures.is_empty() {
            let _ = write!(message, "\nFailed agents:\n{}", failures.join("\n"));
        }
        let _ = write!(
            message,
            "\nRun folder (script, agent results, result.json): {}\nPresent the result to the user; resumeFromRunId \"{}\" reruns it reusing finished agents.",
            run.dir.display(),
            run.id
        );
        finished.push(Finished {
            headline,
            for_model: message,
        });
    }
    finished
}

/// Exécutions en cours (une session en arrière-plan reste en vie pour elles).
#[must_use]
pub fn running_count() -> usize {
    lock()
        .iter()
        .filter(|run| run.state == RunState::Running)
        .count()
}

/// Lignes de `/tasks` et du panneau Agents.
#[must_use]
pub fn lines() -> Vec<String> {
    lock()
        .iter()
        .map(|run| {
            let done = run
                .agents
                .iter()
                .filter(|agent| {
                    matches!(
                        agent.state,
                        AgentState::Done
                            | AgentState::Failed
                            | AgentState::Reused
                            | AgentState::Skipped
                    )
                })
                .count();
            let phase = run
                .current_phase
                .as_deref()
                .map(|phase| format!(" · {phase}"))
                .unwrap_or_default();
            format!(
                "  {}  {:<9} {}{phase} · {done}/{} agents · {}",
                run.id,
                run.state.label(),
                run.name,
                run.agents.len(),
                duration_text(elapsed(run))
            )
        })
        .collect()
}

fn find_run_id(reference: &str) -> Result<String, String> {
    let reference = reference.trim();
    let list = lock();
    list.iter()
        .rev()
        .find(|run| run.id == reference || run.id.starts_with(reference) || run.name == reference)
        .map(|run| run.id.clone())
        .ok_or_else(|| format!("aucune exécution « {reference} » (/workflows pour la liste)"))
}

fn stop_run(id: &str) -> Option<Arc<Mutex<Child>>> {
    with_run(id, |run| {
        run.stop.store(true, Ordering::SeqCst);
        run.delivered = true;
        if run.state == RunState::Running {
            run.state = RunState::Stopped;
        }
        run.child.clone()
    })
    .flatten()
}

/// Arrête une exécution : le script s'arrête, les agents en attente ne
/// partent pas (ceux en cours finissent leur tour).
pub fn stop(reference: &str) -> Result<String, String> {
    let id = find_run_id(reference)?;
    if let Some(child) = stop_run(&id) {
        let _ = child
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .kill();
    }
    Ok(format!("Workflow {id} arrêté"))
}

/// À la fin de Claw : les scripts en cours s'arrêtent.
pub fn stop_all() {
    let ids: Vec<String> = lock()
        .iter()
        .filter(|run| run.state == RunState::Running)
        .map(|run| run.id.clone())
        .collect();
    for id in ids {
        let _ = stop(&id);
    }
}

/// `/workflows save <id> [nom]` : le script devient `.claw/workflows/<nom>.js`
/// (relancé ensuite par `/<nom>`).
fn save(reference: &str, name: Option<&str>) -> Result<String, String> {
    let id = find_run_id(reference)?;
    let (dir, run_name) =
        with_run(&id, |run| (run.dir.clone(), run.name.clone())).ok_or("exécution introuvable")?;
    let script = fs::read_to_string(dir.join("script.js")).map_err(|error| error.to_string())?;
    let name: String = name
        .unwrap_or(&run_name)
        .trim()
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || character == '-' || character == '_' {
                character.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let name = name.trim_matches('-').to_string();
    if name.is_empty() {
        return Err("donnez un nom : /workflows save <id> <nom>".to_string());
    }
    let target = std::env::current_dir()
        .map_err(|error| error.to_string())?
        .join(".claw")
        .join("workflows")
        .join(format!("{name}.js"));
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    fs::write(&target, script).map_err(|error| error.to_string())?;
    Ok(format!(
        "Enregistré : {} · /{name} [arguments] le relance",
        target.display()
    ))
}

fn detail(id: &str) -> Option<String> {
    with_run(id, |run| {
        let mut text = format!(
            "Workflow {} · {} · {}\n  {}\n  Dossier        {}",
            run.name,
            run.id,
            run.state.label(),
            run.description,
            run.dir.display()
        );
        let _ = write!(text, "\n  Durée          {}", duration_text(elapsed(run)));
        let mut phases: Vec<Option<String>> = run.phases.iter().cloned().map(Some).collect();
        if run.agents.iter().any(|agent| agent.phase.is_none()) {
            phases.insert(0, None);
        }
        for phase in phases {
            let agents: Vec<&AgentCall> = run
                .agents
                .iter()
                .filter(|agent| agent.phase == phase)
                .collect();
            if agents.is_empty() && phase.is_none() {
                continue;
            }
            let _ = write!(text, "\n  {}", phase.as_deref().unwrap_or("(sans phase)"));
            if agents.is_empty() {
                text.push_str(" : aucun agent");
            }
            for agent in agents {
                let time = agent
                    .elapsed
                    .or_else(|| agent.started.map(|started| started.elapsed()))
                    .map(duration_text)
                    .unwrap_or_default();
                let _ = write!(
                    text,
                    "\n    {:<10} {}  {time}",
                    agent.state.label(),
                    agent.label
                );
            }
        }
        if !run.logs.is_empty() {
            text.push_str("\n  Journal");
            for line in run.logs.iter().rev().take(10).rev() {
                let _ = write!(text, "\n    {line}");
            }
        }
        if let Some(error) = &run.error {
            let _ = write!(text, "\n  Erreur         {}", shortened(error, 600));
        }
        if let Some(result) = &run.result {
            let _ = write!(text, "\n  Résultat       {}", shortened(result, 800));
        }
        text
    })
}

/// `/workflows` : exécutions de la session et workflows enregistrés ;
/// `/workflows <id>` le détail ; `stop <id>` ; `save <id> [nom]`.
#[must_use]
pub fn report(args: Option<&str>) -> String {
    let args = args.map(str::trim).unwrap_or_default();
    let (first, rest) = args.split_once(char::is_whitespace).unwrap_or((args, ""));
    let rest = rest.trim();
    match first {
        "stop" | "arreter" | "arrêter" if !rest.is_empty() => {
            stop(rest).unwrap_or_else(|error| error)
        }
        "save" | "enregistrer" if !rest.is_empty() => {
            let (reference, name) = rest
                .split_once(char::is_whitespace)
                .map_or((rest, None), |(reference, name)| (reference, Some(name)));
            save(reference, name).unwrap_or_else(|error| error)
        }
        "" => {
            let mut text = String::from("Workflows");
            let runs = lines();
            if runs.is_empty() {
                text.push_str("\n  Aucune exécution dans cette session.");
            } else {
                text.push('\n');
                text.push_str(&runs.join("\n"));
            }
            let saved = saved();
            text.push_str("\nEnregistrés (/nom [arguments] pour lancer)");
            for entry in saved {
                let _ = write!(
                    text,
                    "\n  /{:<18} {}  ({})",
                    entry.name, entry.description, entry.origin
                );
            }
            text.push_str(
                "\n/workflows <id> détail · /workflows stop <id> · /workflows save <id> [nom] · /workflow-authoring pour en écrire",
            );
            if let Err(error) = node() {
                let _ = write!(text, "\nAttention : {error}");
            }
            text
        }
        reference => match find_run_id(reference) {
            Ok(id) => detail(&id).unwrap_or_default(),
            Err(error) => error,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::{matches_schema, meta_description, parse_json_answer, split_meta, BUNDLED};
    use serde_json::json;

    #[test]
    fn meta_is_split_from_the_body() {
        let script = "// audit\nexport const meta = {\n  name: 'audit',\n  description: 'Vérifie { les routes }', // }\n  phases: [{ title: 'A' }],\n}\nconst x = await agent('liste')\nreturn x";
        let (meta, body) = split_meta(script).expect("meta");
        assert!(meta.starts_with('{') && meta.ends_with('}'));
        assert!(meta.contains("phases"));
        assert!(body.contains("const meta = {"));
        assert!(!body.contains("export"));
        assert!(body.ends_with("return x"));
        assert_eq!(meta_description(&meta), "Vérifie { les routes }");
        assert!(split_meta("const meta = { name: 'x' }").is_err());
        assert!(split_meta("export const meta = 3").is_err());
        assert!(split_meta("export const meta = { name: 'x'").is_err());
        for (_, script) in BUNDLED {
            assert!(split_meta(script).is_ok());
        }
    }

    #[test]
    fn answers_are_read_as_json_and_checked_against_the_schema() {
        let schema = json!({
            "type": "object",
            "required": ["files"],
            "properties": { "files": { "type": "array", "items": { "type": "string" } } }
        });
        let answer =
            parse_json_answer("Voici :\n```json\n{\"files\": [\"a.ts\"]}\n```").expect("json");
        assert!(matches_schema(&answer, &schema, 0));
        assert!(!matches_schema(&json!({ "files": [1] }), &schema, 0));
        assert!(!matches_schema(&json!({ "other": [] }), &schema, 0));
        assert!(!matches_schema(&json!([]), &schema, 0));
        assert_eq!(parse_json_answer("rien ici"), None);
    }
}
