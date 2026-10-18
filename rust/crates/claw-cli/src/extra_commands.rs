//! Petites commandes de Claude Code : `/theme`, `/color`, `/terminal-setup`,
//! `/release-notes`, `/usage`, `/insights`, `/fewer-permission-prompts`
//! (statistiques des commandes) et `/subtask` (sous-agent en arrière-plan qui
//! reprend la conversation).

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use runtime::{ContentBlock, MessageRole, Session};
use serde_json::{json, Map, Value};

// --- Réglages personnels -------------------------------------------------------

/// Écrit `key` dans `~/.claw/settings.json` (fichier créé au besoin).
pub fn set_user_setting(key: &str, value: Value) -> Result<PathBuf, String> {
    let path = runtime::ConfigLoader::default_for(".")
        .config_home()
        .join("settings.json");
    let mut settings: Map<String, Value> = fs::read_to_string(&path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default();
    settings.insert(key.to_string(), value);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let text = serde_json::to_string_pretty(&Value::Object(settings))
        .map_err(|error| error.to_string())?;
    fs::write(&path, text).map_err(|error| error.to_string())?;
    Ok(path)
}

// --- /theme ------------------------------------------------------------------------

/// `/theme [nom]` : liste les thèmes, ou change et enregistre celui-ci.
#[must_use]
pub fn theme_command(arg: Option<&str>) -> String {
    let current = crate::render::theme_name();
    let Some(name) = arg.map(str::trim).filter(|name| !name.is_empty()) else {
        let mut lines = vec!["Thème".to_string()];
        for (theme, description) in crate::render::THEMES {
            let marker = if theme == current {
                "● actuel"
            } else {
                "○"
            };
            lines.push(format!("  {theme:<18} {marker:<9} {description}"));
        }
        lines.push(
            "  Changer          /theme <nom> (enregistré dans ~/.claw/settings.json)".to_string(),
        );
        return lines.join("\n");
    };
    if !crate::render::set_theme(name) {
        return format!(
            "Thème inconnu « {name} » : {}",
            crate::render::THEMES
                .iter()
                .map(|(theme, _)| *theme)
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    match set_user_setting("theme", json!(name)) {
        Ok(path) => format!("Thème : {name} (enregistré dans {})", path.display()),
        Err(error) => format!("Thème : {name} pour cette session (non enregistré : {error})"),
    }
}

// --- /color ------------------------------------------------------------------------

const COLORS: [(&str, &str); 8] = [
    ("red", "38;5;203"),
    ("blue", "38;5;75"),
    ("green", "38;5;114"),
    ("yellow", "38;5;220"),
    ("purple", "38;5;141"),
    ("orange", "38;5;208"),
    ("pink", "38;5;212"),
    ("cyan", "38;5;80"),
];

/// Changement de couleur demandé par `/color`.
pub enum ColorChange {
    Unchanged,
    Default,
    Color(String),
}

/// `/color [couleur|default]` : couleur des traits de la saisie pour la
/// session. Renvoie le message et le changement.
#[must_use]
pub fn color_command(arg: Option<&str>) -> (String, ColorChange) {
    let wanted = arg
        .map(|name| name.trim().to_lowercase())
        .filter(|name| !name.is_empty());
    let pick = match wanted.as_deref() {
        Some("default" | "defaut" | "défaut") => {
            return (
                "Couleur de la saisie : par défaut".to_string(),
                ColorChange::Default,
            );
        }
        Some(name) => COLORS.iter().find(|(color, _)| *color == name),
        None => {
            let seed = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |elapsed| elapsed.subsec_nanos() as usize);
            COLORS.get(seed % COLORS.len())
        }
    };
    match pick {
        Some((name, code)) => (
            format!("Couleur de la saisie : {name}"),
            ColorChange::Color(format!("\x1b[{code}m")),
        ),
        None => (
            format!(
                "Couleur inconnue : {}, ou default",
                COLORS
                    .iter()
                    .map(|(name, _)| *name)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            ColorChange::Unchanged,
        ),
    }
}

// --- /terminal-setup ----------------------------------------------------------------

/// Fichier des raccourcis de VS Code (ou Cursor) de l'utilisateur.
fn editor_keybindings(app: &str) -> Option<PathBuf> {
    let base = if cfg!(windows) {
        std::env::var_os("APPDATA").map(PathBuf::from)?
    } else if cfg!(target_os = "macos") {
        PathBuf::from(std::env::var_os("HOME")?)
            .join("Library")
            .join("Application Support")
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| Some(PathBuf::from(std::env::var_os("HOME")?).join(".config")))?
    };
    Some(base.join(app).join("User").join("keybindings.json"))
}

/// Retire les commentaires `//` (hors chaînes) d'un fichier JSONC.
fn strip_line_comments(text: &str) -> String {
    let mut out = String::new();
    for line in text.lines() {
        let mut in_string = false;
        let mut escaped = false;
        let mut cut = line.len();
        let bytes = line.as_bytes();
        for (index, byte) in bytes.iter().enumerate() {
            match byte {
                b'\\' if in_string => escaped = !escaped,
                b'"' if !escaped => in_string = !in_string,
                b'/' if !in_string && bytes.get(index + 1) == Some(&b'/') => {
                    cut = index;
                    break;
                }
                _ => escaped = false,
            }
            if *byte != b'\\' {
                escaped = false;
            }
        }
        out.push_str(&line[..cut]);
        out.push('\n');
    }
    out
}

/// `/terminal-setup` : dans VS Code ou Cursor, Maj+Entrée envoie une
/// séquence que Claw lit comme un saut de ligne (le terminal intégré envoie
/// sinon une simple Entrée).
#[must_use]
pub fn terminal_setup() -> String {
    let program = std::env::var("TERM_PROGRAM").unwrap_or_default();
    let app = match program.as_str() {
        "vscode" if std::env::var("CURSOR_TRACE_ID").is_ok() => "Cursor",
        "vscode" => "Code",
        _ => {
            return format!(
                "Terminal « {} » : Maj+Entrée fonctionne déjà dans Windows Terminal, iTerm2, WezTerm, kitty et Alacritty récents.\n  Partout                Ctrl+J ou Alt+Entrée insèrent aussi un saut de ligne\n  VS Code, Cursor        lancez /terminal-setup depuis leur terminal intégré",
                if program.is_empty() { "inconnu" } else { &program }
            );
        }
    };
    let Some(path) = editor_keybindings(app) else {
        return "Dossier de configuration de l'éditeur introuvable.".to_string();
    };
    let binding = json!({
        "key": "shift+enter",
        "command": "workbench.action.terminal.sendSequence",
        "args": { "text": "\u{1b}\r" },
        "when": "terminalFocus"
    });
    let existing = fs::read_to_string(&path).unwrap_or_else(|_| "[]".to_string());
    let Ok(Value::Array(mut bindings)) =
        serde_json::from_str::<Value>(&strip_line_comments(&existing))
    else {
        return format!(
            "{} n'est pas un tableau JSON lisible : ajoutez à la main\n  {}",
            path.display(),
            binding
        );
    };
    let present = bindings.iter().any(|entry| {
        entry.get("key").and_then(Value::as_str) == Some("shift+enter")
            && entry.get("command").and_then(Value::as_str)
                == Some("workbench.action.terminal.sendSequence")
    });
    if present {
        return format!("Maj+Entrée est déjà réglé dans {}", path.display());
    }
    bindings.push(binding);
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if path.exists() {
        let _ = fs::copy(&path, path.with_extension("json.bak"));
    }
    let text = serde_json::to_string_pretty(&Value::Array(bindings)).unwrap_or_default();
    match fs::write(&path, text) {
        Ok(()) => format!(
            "Maj+Entrée insère maintenant un saut de ligne dans le terminal de {app}\n  Fichier          {}\n  Copie            {}",
            path.display(),
            path.with_extension("json.bak").display()
        ),
        Err(error) => format!("Écriture impossible de {} : {error}", path.display()),
    }
}

// --- /release-notes ----------------------------------------------------------------

/// Notes de version livrées avec Claw.
#[must_use]
pub fn release_notes() -> String {
    format!(
        "Notes de version de Claw {}\n\n{}",
        env!("CARGO_PKG_VERSION"),
        include_str!("../../../docs/releases/0.1.0.md").trim()
    )
}

// --- Statistiques des conversations ------------------------------------------------

/// Outils appelés, erreurs, commandes shell d'une conversation.
#[derive(Debug, Default)]
pub struct Activity {
    pub user_messages: usize,
    pub assistant_messages: usize,
    pub tools: BTreeMap<String, usize>,
    pub tool_errors: usize,
    pub commands: BTreeMap<String, usize>,
}

/// Les deux premiers mots d'une commande (`cargo test`, `npm run`).
fn command_head(command: &str) -> String {
    let first_line = command.lines().next().unwrap_or_default();
    let segment = first_line.split(['&', ';', '|']).next().unwrap_or_default();
    segment
        .split_whitespace()
        .take(2)
        .collect::<Vec<_>>()
        .join(" ")
}

impl Activity {
    pub fn add(&mut self, session: &Session) {
        for message in &session.messages {
            match message.role {
                MessageRole::User => {
                    if message
                        .blocks
                        .iter()
                        .any(|block| matches!(block, ContentBlock::Text { .. }))
                    {
                        self.user_messages += 1;
                    }
                }
                MessageRole::Assistant => self.assistant_messages += 1,
                _ => {}
            }
            for block in &message.blocks {
                match block {
                    ContentBlock::ToolUse { name, input, .. } => {
                        *self.tools.entry(name.clone()).or_default() += 1;
                        if matches!(name.as_str(), "bash" | "PowerShell") {
                            let command = serde_json::from_str::<Value>(input)
                                .ok()
                                .and_then(|value| {
                                    value.get("command")?.as_str().map(str::to_string)
                                })
                                .unwrap_or_default();
                            let head = command_head(&command);
                            if !head.is_empty() {
                                *self.commands.entry(head).or_default() += 1;
                            }
                        }
                    }
                    ContentBlock::ToolResult { is_error: true, .. } => self.tool_errors += 1,
                    _ => {}
                }
            }
        }
    }

    /// Les `limit` entrées les plus fréquentes.
    #[must_use]
    pub fn top(map: &BTreeMap<String, usize>, limit: usize) -> Vec<(String, usize)> {
        let mut entries: Vec<(String, usize)> = map
            .iter()
            .map(|(key, count)| (key.clone(), *count))
            .collect();
        entries.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        entries.truncate(limit);
        entries
    }

    /// Résumé en quelques lignes (`/usage`).
    #[must_use]
    pub fn summary(&self) -> String {
        let calls: usize = self.tools.values().sum();
        let mut lines = vec![
            format!(
                "  Messages         {} de vous, {} de Claw",
                self.user_messages, self.assistant_messages
            ),
            format!("  Appels d'outils  {calls} ({} en échec)", self.tool_errors),
        ];
        let top = Self::top(&self.tools, 6);
        if !top.is_empty() {
            lines.push(format!(
                "  Outils           {}",
                top.iter()
                    .map(|(name, count)| format!("{name} {count}"))
                    .collect::<Vec<_>>()
                    .join(" · ")
            ));
        }
        lines.join("\n")
    }
}

/// Conversations enregistrées du projet (`.claw/sessions`).
fn project_sessions(cwd: &Path) -> Vec<(PathBuf, Session)> {
    let Ok(entries) = fs::read_dir(cwd.join(".claw").join("sessions")) else {
        return Vec::new();
    };
    entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension().and_then(|ext| ext.to_str()) == Some("json")
                && !path.to_string_lossy().ends_with(".tasks.json")
        })
        .filter_map(|path| {
            Session::load_from_path(&path)
                .ok()
                .map(|session| (path, session))
        })
        .collect()
}

/// Commandes shell qui ne demandent jamais d'autorisation dans Claw (lecture
/// seule) : inutile de les compter pour `/fewer-permission-prompts`.
const READ_ONLY_HEADS: &[&str] = &[
    "git status",
    "git log",
    "git diff",
    "git show",
    "git branch",
    "ls",
    "cat",
    "grep",
    "rg",
    "find",
    "pwd",
    "echo",
    "head",
    "tail",
    "wc",
    "tree",
    "which",
    "type",
];

/// `/fewer-permission-prompts` : commandes des conversations du projet, les
/// plus fréquentes d'abord, hors lecture seule.
#[must_use]
pub fn command_frequencies(cwd: &Path) -> Vec<(String, usize)> {
    let mut activity = Activity::default();
    for (_, session) in project_sessions(cwd) {
        activity.add(&session);
    }
    activity.commands.retain(|head, _| {
        !READ_ONLY_HEADS
            .iter()
            .any(|known| head == known || head.starts_with(&format!("{known} ")))
    });
    let mut tools: BTreeMap<String, usize> = activity
        .tools
        .into_iter()
        .filter(|(name, _)| name.starts_with("mcp__"))
        .collect();
    let mut top = Activity::top(&activity.commands, 25);
    top.extend(Activity::top(&std::mem::take(&mut tools), 10));
    top
}

// --- /insights ---------------------------------------------------------------------

fn escape_html(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn bars(title: &str, entries: &[(String, usize)]) -> String {
    let max = entries
        .iter()
        .map(|(_, count)| *count)
        .max()
        .unwrap_or(1)
        .max(1);
    let mut html = format!("<h2>{}</h2><table>", escape_html(title));
    for (name, count) in entries {
        let width = count * 100 / max;
        let _ = write!(
            html,
            "<tr><td>{}</td><td class=n>{count}</td><td><div class=bar style=\"width:{width}%\"></div></td></tr>",
            escape_html(name)
        );
    }
    html.push_str("</table>");
    html
}

/// `/insights` : rapport HTML sur les conversations du projet (outils,
/// commandes, échecs), écrit dans `.claw/insights.html`.
pub fn insights(cwd: &Path) -> Result<(PathBuf, String), String> {
    let sessions = project_sessions(cwd);
    if sessions.is_empty() {
        return Err("aucune conversation enregistrée dans ce projet (.claw/sessions)".to_string());
    }
    let mut activity = Activity::default();
    for (_, session) in &sessions {
        activity.add(session);
    }
    let calls: usize = activity.tools.values().sum();
    let error_rate = if calls == 0 {
        0
    } else {
        activity.tool_errors * 100 / calls
    };
    let jobs = crate::jobs::all()
        .into_iter()
        .filter(|(job, _)| Path::new(&job.origin).starts_with(cwd))
        .count();
    let mut advice = Vec::new();
    if error_rate >= 20 {
        advice.push("Beaucoup d'appels d'outils échouent : précisez les commandes de build et de test dans CLAW.md, ou réglez \"verify\".");
    }
    if activity.commands.keys().any(|head| {
        head.starts_with("cargo") || head.starts_with("npm") || head.starts_with("pytest")
    }) {
        advice.push("Les mêmes commandes de build ou de test reviennent : /fewer-permission-prompts propose des règles pour ne plus les approuver une à une.");
    }
    if sessions.len() >= 5 && jobs == 0 {
        advice.push("Pour des tâches indépendantes, claw --bg \"tâche\" les lance en arrière-plan, et claw agents les suit.");
    }
    if activity.tools.get("Agent").copied().unwrap_or(0) == 0 && calls > 50 {
        advice.push("Les sous-agents (outil Agent, /subtask) gardent la conversation principale courte sur les longues explorations.");
    }
    let advice_html = advice.iter().fold(String::new(), |mut html, line| {
        let _ = write!(html, "<li>{}</li>", escape_html(line));
        html
    });
    let html = format!(
        "<!doctype html><html lang=fr><head><meta charset=utf-8><title>Claw · bilan d'usage</title><style>body{{font-family:system-ui,sans-serif;max-width:860px;margin:2em auto;padding:0 1em;color:#222}}h1{{color:#d77757}}td{{padding:2px 8px}}td.n{{text-align:right;color:#666}}.bar{{background:#d77757;height:10px;border-radius:3px}}.k{{display:inline-block;margin:0 2em 1em 0}}.k b{{font-size:1.6em;display:block}}</style></head><body><h1>Bilan d'usage de Claw</h1><p>{}</p><div><span class=k><b>{}</b>conversations</span><span class=k><b>{}</b>messages de vous</span><span class=k><b>{calls}</b>appels d'outils</span><span class=k><b>{error_rate} %</b>en échec</span><span class=k><b>{jobs}</b>sessions en arrière-plan</span></div>{}{}<h2>Pistes</h2><ul>{}</ul></body></html>",
        escape_html(&cwd.display().to_string()),
        sessions.len(),
        activity.user_messages,
        bars("Outils les plus utilisés", &Activity::top(&activity.tools, 12)),
        bars("Commandes les plus lancées", &Activity::top(&activity.commands, 12)),
        if advice_html.is_empty() { "<li>Rien de particulier à signaler.</li>".to_string() } else { advice_html },
    );
    let path = cwd.join(".claw").join("insights.html");
    fs::write(&path, html).map_err(|error| error.to_string())?;
    let summary = format!(
        "Bilan d'usage\n  Conversations    {}\n{}\n  Rapport          {}",
        sessions.len(),
        activity.summary(),
        path.display()
    );
    Ok((path, summary))
}

// --- /subtask ----------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct Subtask {
    pub id: usize,
    pub task: String,
    pub result: Option<Result<String, String>>,
    pub delivered: bool,
}

fn subtasks() -> &'static Mutex<Vec<Subtask>> {
    static SUBTASKS: OnceLock<Mutex<Vec<Subtask>>> = OnceLock::new();
    SUBTASKS.get_or_init(|| Mutex::new(Vec::new()))
}

/// La conversation en texte (sans les sorties d'outils), pour la copie que
/// reçoit le sous-agent.
#[must_use]
pub fn transcript_text(session: &Session, limit: usize) -> String {
    let mut parts = Vec::new();
    for message in &session.messages {
        let who = match message.role {
            MessageRole::User => "User",
            MessageRole::Assistant => "Assistant",
            _ => continue,
        };
        let text = message
            .blocks
            .iter()
            .filter_map(|block| match block {
                ContentBlock::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n");
        if !text.trim().is_empty() {
            parts.push(format!("{who}: {}", text.trim()));
        }
    }
    let joined = parts.join("\n\n");
    let chars: Vec<char> = joined.chars().collect();
    let start = chars.len().saturating_sub(limit);
    chars[start..].iter().collect()
}

/// Lance un sous-agent qui part de la conversation et travaille sur `task`
/// pendant que vous continuez ; son résultat revient dans la conversation.
pub fn spawn_subtask(task: &str, transcript: &str, model: &str) -> usize {
    let id = {
        let mut list = subtasks()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let id = list.len() + 1;
        list.push(Subtask {
            id,
            task: task.to_string(),
            result: None,
            delivered: false,
        });
        id
    };
    let prompt = format!(
        "You are a forked sub-agent: you continue from the conversation below, with all its context, and work on one task while the user keeps working in the main conversation. Do the task fully, then report what you did and found in a few sentences.\n\n--- CONVERSATION SO FAR ---\n{transcript}\n--- END ---\n\nYOUR TASK: {task}"
    );
    let description = format!("Sous-tâche {id}");
    let model = model.to_string();
    let _ = std::thread::Builder::new()
        .name(format!("claw-subtask-{id}"))
        .spawn(move || {
            let output = tools::execute_tool(
                "Agent",
                &json!({
                    "description": description,
                    "prompt": prompt,
                    "subagent_type": "general-purpose",
                    "model": model,
                }),
            );
            let result = output.and_then(|text| {
                serde_json::from_str::<Value>(&text)
                    .ok()
                    .and_then(|value| value.get("result")?.as_str().map(str::to_string))
                    .ok_or_else(|| "le sous-agent n'a pas rendu de rapport".to_string())
            });
            let mut list = subtasks()
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if let Some(entry) = list.iter_mut().find(|entry| entry.id == id) {
                entry.result = Some(result);
            }
        });
    id
}

/// Une sous-tâche a fini et son résultat n'a pas encore été rendu.
#[must_use]
pub fn has_finished_subtask() -> bool {
    subtasks()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .iter()
        .any(|entry| entry.result.is_some() && !entry.delivered)
}

/// Résultats des sous-tâches finies, à rendre une fois.
pub fn take_finished_subtasks() -> Vec<Subtask> {
    let mut list = subtasks()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut done = Vec::new();
    for entry in list.iter_mut() {
        if entry.result.is_some() && !entry.delivered {
            entry.delivered = true;
            done.push(entry.clone());
        }
    }
    done
}

/// Sous-tâches de la session (`/tasks`).
#[must_use]
pub fn subtask_lines() -> Vec<String> {
    subtasks()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .iter()
        .map(|entry| {
            let state = match &entry.result {
                None => "en cours",
                Some(Ok(_)) => "terminée",
                Some(Err(_)) => "échec",
            };
            format!(
                "  sous-tâche {}  {state:<9} {}",
                entry.id,
                entry.task.chars().take(70).collect::<String>()
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{command_head, strip_line_comments};

    #[test]
    fn helpers_read_commands_and_jsonc() {
        assert_eq!(
            command_head("cargo test --workspace && echo ok"),
            "cargo test"
        );
        assert_eq!(command_head("ls"), "ls");
        let jsonc = "[\n  // un commentaire\n  {\"key\": \"a//b\"} // fin\n]";
        let parsed: serde_json::Value =
            serde_json::from_str(&strip_line_comments(jsonc)).expect("jsonc");
        assert_eq!(parsed[0]["key"], "a//b");
    }
}
