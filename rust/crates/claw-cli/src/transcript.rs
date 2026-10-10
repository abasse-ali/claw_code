//! Affichage du terminal à la manière de Claude Code : réponses `●`, appels
//! d'outils `● Commande(cargo test)` suivis de leur résultat `  ⎿  …`, diffs
//! numérotés et encadrés (accueil, demandes d'autorisation).

use std::collections::VecDeque;
use std::env;
use std::fmt::Write as _;
use std::io::{self, Write};
use std::path::Path;
use std::sync::Mutex;
use std::time::Instant;

use serde_json::Value;

pub const ACCENT: &str = "\x1b[38;2;215;119;87m";
pub const DIM: &str = "\x1b[2m";
pub const RED: &str = "\x1b[38;5;203m";
pub const GREEN: &str = "\x1b[38;5;114m";
pub const BOLD: &str = "\x1b[1m";
pub const RESET: &str = "\x1b[0m";

const RESULT_PREFIX: &str = "  ⎿  ";
const RESULT_INDENT: &str = "     ";
/// Lignes de sortie montrées sous un outil (le reste : « … +N lignes »).
/// Taille des aperçus : compacte sous chaque outil, complète avec Ctrl+O.
struct Limits {
    lines: usize,
    diff: usize,
    names: usize,
    /// Montrer le contenu d'un fichier lu (pas seulement son nombre de lignes).
    read_content: bool,
}

const COMPACT: Limits = Limits {
    lines: 6,
    diff: 24,
    names: 5,
    read_content: false,
};
const FULL: Limits = Limits {
    lines: 2_000,
    diff: 2_000,
    names: 500,
    read_content: true,
};
const DIFF_CONTEXT: usize = 2;
const REMOVED_BG: &str = "\x1b[48;5;52m";
const ADDED_BG: &str = "\x1b[48;5;22m";

/// Largeur du terminal (80 hors terminal).
#[must_use]
pub fn terminal_width() -> usize {
    crossterm::terminal::size().map_or(80, |(columns, _)| usize::from(columns).max(20))
}

/// Largeur visible d'un texte (séquences ANSI ignorées).
#[must_use]
pub fn visible_width(text: &str) -> usize {
    strip_ansi(text).chars().count()
}

#[must_use]
pub fn strip_ansi(text: &str) -> String {
    let mut plain = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            if chars.peek() == Some(&'[') {
                chars.next();
                for next in chars.by_ref() {
                    if next.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
            continue;
        }
        plain.push(c);
    }
    plain
}

/// Coupe un texte sans couleurs à `limit` caractères (`…` à la fin).
#[must_use]
pub fn clip(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_string();
    }
    let kept: String = text.chars().take(limit.saturating_sub(1)).collect();
    format!("{kept}…")
}

/// Coupe le début d'un chemin trop long : `…/src/main.rs`.
#[must_use]
pub fn clip_start(text: &str, limit: usize) -> String {
    let count = text.chars().count();
    if count <= limit {
        return text.to_string();
    }
    let kept: String = text.chars().skip(count + 1 - limit).collect();
    format!("…{kept}")
}

/// Découpe un texte sans couleurs en lignes de `width` caractères au plus.
#[must_use]
pub fn wrap(text: &str, width: usize) -> Vec<String> {
    let width = width.max(8);
    let mut lines = Vec::new();
    for raw in text.lines() {
        let raw = raw.replace('\t', "    ");
        if raw.is_empty() {
            lines.push(String::new());
            continue;
        }
        let mut current = String::new();
        for word in raw.split(' ') {
            let needed =
                current.chars().count() + usize::from(!current.is_empty()) + word.chars().count();
            if needed > width && !current.is_empty() {
                lines.push(std::mem::take(&mut current));
            }
            if !current.is_empty() {
                current.push(' ');
            }
            current.push_str(word);
            while current.chars().count() > width {
                let head: String = current.chars().take(width).collect();
                current = current.chars().skip(width).collect();
                lines.push(head);
            }
        }
        lines.push(current);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

/// Encadré aux coins arrondis ; chaque ligne doit tenir dans `width - 4`.
#[must_use]
pub fn boxed(lines: &[String], width: usize, border: &str) -> Vec<String> {
    let inner = width.saturating_sub(4);
    let mut out = Vec::with_capacity(lines.len() + 2);
    out.push(format!(
        "{border}╭{}╮{RESET}",
        "─".repeat(width.saturating_sub(2))
    ));
    for line in lines {
        let pad = inner.saturating_sub(visible_width(line));
        out.push(format!(
            "{border}│{RESET} {line}{RESET}{} {border}│{RESET}",
            " ".repeat(pad)
        ));
    }
    out.push(format!(
        "{border}╰{}╯{RESET}",
        "─".repeat(width.saturating_sub(2))
    ));
    out
}

/// Chemin montré relatif au projet, sans le préfixe Windows `\\?\`.
#[must_use]
pub fn display_path(raw: &str) -> String {
    let cleaned = raw.trim_start_matches(r"\\?\");
    env::current_dir()
        .ok()
        .and_then(|cwd| {
            Path::new(cleaned)
                .strip_prefix(&cwd)
                .ok()
                .map(|relative| relative.display().to_string())
        })
        .unwrap_or_else(|| cleaned.to_string())
}

fn str_field<'a>(value: &'a Value, keys: &[&str]) -> Option<&'a str> {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(Value::as_str))
}

fn path_field(value: &Value) -> Option<String> {
    str_field(value, &["path", "file_path", "filePath", "notebook_path"]).map(display_path)
}

/// Nom affiché d'un outil.
#[must_use]
pub fn tool_label(name: &str) -> &str {
    match name {
        "bash" | "Bash" => "Commande",
        "read_file" | "Read" => "Lecture",
        "write_file" | "Write" => "Écriture",
        "edit_file" | "Edit" => "Modification",
        "glob_search" | "Glob" => "Fichiers",
        "grep_search" | "Grep" => "Recherche",
        "WebFetch" => "Page web",
        "WebSearch" => "Recherche web",
        "TodoWrite" => "Tâches",
        "Remember" => "Mémoire",
        "Consult" => "Second avis",
        "Agent" => "Sous-agent",
        "NotebookEdit" => "Notebook",
        "BashOutput" => "Sortie de tâche",
        "KillShell" => "Arrêt de tâche",
        "ExitPlanMode" => "Plan",
        "ToolSearch" => "Outils",
        other => other,
    }
}

/// Argument principal d'un appel, sur une ligne.
fn tool_argument(name: &str, input: &Value) -> Option<String> {
    let argument = match name {
        "bash" | "Bash" | "PowerShell" => str_field(input, &["command"]).map(ToOwned::to_owned),
        "read_file" | "Read" => path_field(input).map(|path| {
            let offset = input.get("offset").and_then(Value::as_u64);
            let limit = input.get("limit").and_then(Value::as_u64);
            match (offset, limit) {
                (Some(offset), Some(limit)) => {
                    format!("{path} · lignes {}-{}", offset + 1, offset + limit)
                }
                (Some(offset), None) => format!("{path} · depuis la ligne {}", offset + 1),
                (None, Some(limit)) => format!("{path} · {limit} lignes"),
                (None, None) => path,
            }
        }),
        "write_file" | "Write" | "edit_file" | "Edit" | "NotebookEdit" => path_field(input),
        "glob_search" | "Glob" | "grep_search" | "Grep" => {
            str_field(input, &["pattern"]).map(|pattern| {
                let pattern = if matches!(name, "grep_search" | "Grep") {
                    format!("\"{pattern}\"")
                } else {
                    pattern.to_string()
                };
                match str_field(input, &["path"]).filter(|path| !path.is_empty() && *path != ".") {
                    Some(path) => format!("{pattern} dans {}", display_path(path)),
                    None => pattern,
                }
            })
        }
        "WebFetch" => str_field(input, &["url"]).map(ToOwned::to_owned),
        "WebSearch" => str_field(input, &["query"]).map(ToOwned::to_owned),
        "Agent" => str_field(input, &["description", "subagent_type"]).map(ToOwned::to_owned),
        "Skill" => str_field(input, &["skill", "name"]).map(ToOwned::to_owned),
        "Remember" => str_field(input, &["note"]).map(ToOwned::to_owned),
        "Consult" => str_field(input, &["question"]).map(ToOwned::to_owned),
        "BashOutput" | "KillShell" => {
            str_field(input, &["bash_id", "shell_id", "id"]).map(ToOwned::to_owned)
        }
        "TodoWrite" | "ExitPlanMode" => None,
        _ => match input {
            Value::Object(map) if map.is_empty() => None,
            Value::Null => None,
            Value::String(text) => Some(text.clone()),
            other => Some(other.to_string()),
        },
    }?;
    let one_line = argument.split_whitespace().collect::<Vec<_>>().join(" ");
    Some(one_line)
}

/// `● Commande(cargo test)` : ligne affichée quand le modèle appelle un outil.
#[must_use]
pub fn tool_call_line(name: &str, input: &str) -> String {
    let parsed: Value =
        serde_json::from_str(input).unwrap_or_else(|_| Value::String(input.to_string()));
    let label = tool_label(name);
    let room = terminal_width()
        .saturating_sub(label.chars().count() + 6)
        .max(20);
    match tool_argument(name, &parsed) {
        Some(argument) if !argument.is_empty() => format!(
            "{GREEN}●{RESET} {BOLD}{label}{RESET}({})",
            clip(&argument, room)
        ),
        _ => format!("{GREEN}●{RESET} {BOLD}{label}{RESET}"),
    }
}

/// Libellé de l'indicateur pendant l'exécution d'un outil.
#[must_use]
pub fn running_label(name: &str) -> String {
    match name {
        "bash" | "Bash" | "PowerShell" => "Exécution…".to_string(),
        "read_file" | "Read" => "Lecture…".to_string(),
        "write_file" | "Write" | "edit_file" | "Edit" | "NotebookEdit" => {
            "Modification…".to_string()
        }
        "glob_search" | "Glob" | "grep_search" | "Grep" | "WebSearch" | "ToolSearch" => {
            "Recherche…".to_string()
        }
        "WebFetch" => "Lecture de la page…".to_string(),
        "Consult" => "Second avis demandé à d'autres modèles…".to_string(),
        "Agent" => "Sous-agent au travail…".to_string(),
        _ => format!("{}…", tool_label(name)),
    }
}

/// Lignes de résultat : la première après `  ⎿  `, les suivantes alignées.
fn result_block(lines: &[String]) -> String {
    let mut out = String::new();
    for (index, line) in lines.iter().enumerate() {
        if index > 0 {
            out.push('\n');
        }
        out.push_str(if index == 0 {
            RESULT_PREFIX
        } else {
            RESULT_INDENT
        });
        out.push_str(line);
    }
    if out.is_empty() {
        out.push_str(RESULT_PREFIX);
    }
    out
}

fn content_width() -> usize {
    terminal_width()
        .saturating_sub(RESULT_INDENT.len() + 1)
        .max(20)
}

/// Premières lignes d'une sortie, coupées à la largeur, puis « … +N lignes ».
fn preview(text: &str, max_lines: usize, color: &str) -> Vec<String> {
    let all: Vec<&str> = text.trim_end().lines().collect();
    let width = content_width();
    let mut lines: Vec<String> = all
        .iter()
        .take(max_lines)
        .map(|line| {
            let plain = strip_ansi(&line.replace('\t', "    "));
            if color.is_empty() {
                clip(&plain, width)
            } else {
                format!("{color}{}{RESET}", clip(&plain, width))
            }
        })
        .collect();
    if all.len() > max_lines {
        lines.push(more_lines(all.len() - max_lines));
    }
    lines
}

fn more_lines(count: usize) -> String {
    let hint = if interactive() {
        " (Ctrl+O pour tout voir)"
    } else {
        ""
    };
    format!(
        "{DIM}… +{count} ligne{}{hint}{RESET}",
        if count > 1 { "s" } else { "" }
    )
}

/// REPL interactif : Ctrl+O y montre le dernier tour en entier.
static INTERACTIVE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn set_interactive(enabled: bool) {
    INTERACTIVE.store(enabled, std::sync::atomic::Ordering::Relaxed);
}

fn interactive() -> bool {
    INTERACTIVE.load(std::sync::atomic::Ordering::Relaxed)
}

fn plural(count: u64, word: &str) -> String {
    format!("{count} {word}{}", if count > 1 { "s" } else { "" })
}

/// Résultat d'un outil, sous son appel.
#[must_use]
pub fn tool_result(name: &str, output: &str, is_error: bool) -> String {
    render_result(name, output, is_error, &COMPACT)
}

fn render_result(name: &str, output: &str, is_error: bool, limits: &Limits) -> String {
    if is_error {
        return error_result(output, limits);
    }
    let parsed: Value =
        serde_json::from_str(output).unwrap_or_else(|_| Value::String(output.to_string()));
    let lines = match name {
        "bash" | "Bash" | "PowerShell" => bash_result(&parsed, limits),
        "read_file" | "Read" => read_result(&parsed, limits),
        "write_file" | "Write" => write_result(&parsed, limits),
        "edit_file" | "Edit" => edit_result(&parsed, limits),
        "glob_search" | "Glob" => glob_result(&parsed, limits),
        "grep_search" | "Grep" => grep_result(&parsed, limits),
        "TodoWrite" => todo_result(&parsed),
        "Remember" => vec![
            if parsed.get("saved").and_then(Value::as_bool) == Some(true) {
                "Noté pour les prochaines sessions".to_string()
            } else {
                format!("{DIM}Déjà noté{RESET}")
            },
        ],
        "Consult" => consult_result(&parsed, limits),
        "WebFetch" if !limits.read_content => web_fetch_result(&parsed),
        _ => generic_result(&parsed, limits),
    };
    result_block(&lines)
}

fn error_result(output: &str, limits: &Limits) -> String {
    let text = output.trim();
    if text == runtime::interrupt::INTERRUPTED_MESSAGE {
        return result_block(&[format!("{RED}Interrompu{RESET}")]);
    }
    let max_lines = if limits.read_content { limits.lines } else { 4 };
    let mut lines = preview(text, max_lines, RED);
    if let Some(first) = lines.first_mut() {
        *first = format!("{RED}Erreur : {RESET}{first}");
    } else {
        lines.push(format!("{RED}Erreur{RESET}"));
    }
    result_block(&lines)
}

/// Sortie d'une commande sans les remarques que Claw y joint pour le modèle
/// (lancement ou passage en arrière-plan, délai dépassé).
fn without_claw_notes(text: &str) -> &str {
    if text.starts_with("[Claw] ") || text.starts_with("Started in the background as ") {
        return "";
    }
    text.find("\n[Claw] ")
        .map_or(text, |at| text[..at].trim_end())
}

fn bash_result(parsed: &Value, limits: &Limits) -> Vec<String> {
    let task = str_field(parsed, &["backgroundTaskId"]);
    let stdout = without_claw_notes(str_field(parsed, &["stdout"]).unwrap_or_default());
    let stderr = without_claw_notes(str_field(parsed, &["stderr"]).unwrap_or_default());
    let mut lines = Vec::new();
    if !stdout.trim().is_empty() {
        lines.extend(preview(stdout, limits.lines, ""));
    }
    if !stderr.trim().is_empty() {
        lines.extend(preview(stderr, limits.lines, RED));
    }
    match str_field(parsed, &["returnCodeInterpretation"]) {
        Some("interrupted") => lines.push(format!("{RED}Interrompue{RESET}")),
        Some("timeout") => lines.push(format!("{RED}Délai dépassé{RESET}")),
        Some(code) if code.starts_with("exit_code:") => lines.push(format!(
            "{RED}Code de sortie {}{RESET}",
            code.trim_start_matches("exit_code:")
        )),
        _ => {}
    }
    if let Some(task) = task {
        // Lancée en arrière-plan, ou passée en arrière-plan après une
        // première sortie (commande longue, serveur prêt, processus `&`).
        let label = if lines.is_empty() {
            "Lancée en arrière-plan"
        } else {
            "Continue en arrière-plan"
        };
        lines.push(format!("{label} {DIM}({task} · /bashes){RESET}"));
    } else if lines.is_empty() {
        lines.push(format!("{DIM}(Aucune sortie){RESET}"));
    }
    lines
}

fn read_result(parsed: &Value, limits: &Limits) -> Vec<String> {
    let file = parsed.get("file").unwrap_or(parsed);
    let num_lines = file.get("numLines").and_then(Value::as_u64).unwrap_or(0);
    let total = file
        .get("totalLines")
        .and_then(Value::as_u64)
        .unwrap_or(num_lines);
    let start = file.get("startLine").and_then(Value::as_u64).unwrap_or(1);
    let mut line = format!("Lu {}", plural(num_lines, "ligne"));
    if num_lines < total {
        let end = start + num_lines.saturating_sub(1);
        let _ = write!(line, " {DIM}(lignes {start}-{end} sur {total}){RESET}");
    }
    let mut lines = vec![line];
    if limits.read_content {
        let content = str_field(file, &["content"]).unwrap_or_default();
        let rows: Vec<&str> = content.lines().collect();
        let first = usize::try_from(start).unwrap_or(1);
        lines.extend(numbered(&rows, first, limits.lines, content_width()));
    }
    lines
}

/// Lignes numérotées (aperçu d'un fichier écrit) tenant dans `width` colonnes.
#[must_use]
pub fn numbered(
    lines: &[&str],
    first_number: usize,
    max_lines: usize,
    width: usize,
) -> Vec<String> {
    let width = width.saturating_sub(7).max(10);
    let mut out: Vec<String> = lines
        .iter()
        .take(max_lines)
        .enumerate()
        .map(|(index, line)| {
            format!(
                "{DIM}{:>5}{RESET} {}",
                first_number + index,
                clip(&line.replace('\t', "    "), width)
            )
        })
        .collect();
    if lines.len() > max_lines {
        out.push(more_lines(lines.len() - max_lines));
    }
    out
}

fn write_result(parsed: &Value, limits: &Limits) -> Vec<String> {
    let path = path_field(parsed).unwrap_or_else(|| "?".to_string());
    let content = str_field(parsed, &["content"]).unwrap_or_default();
    let lines: Vec<&str> = content.lines().collect();
    let verb = if str_field(parsed, &["type"]) == Some("create") {
        "Créé"
    } else {
        "Réécrit"
    };
    let mut out = vec![format!(
        "{verb} {BOLD}{path}{RESET} {DIM}({}){RESET}",
        plural(lines.len() as u64, "ligne")
    )];
    out.extend(numbered(&lines, 1, limits.lines, content_width()));
    out
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DiffKind {
    Same,
    Removed,
    Added,
}

/// Différence ligne à ligne (plus longue sous-suite commune).
fn diff_lines<'a>(old: &[&'a str], new: &[&'a str]) -> Vec<(DiffKind, &'a str)> {
    if old.len().saturating_mul(new.len()) > 250_000 {
        return old
            .iter()
            .map(|line| (DiffKind::Removed, *line))
            .chain(new.iter().map(|line| (DiffKind::Added, *line)))
            .collect();
    }
    let mut table = vec![vec![0u32; new.len() + 1]; old.len() + 1];
    for i in (0..old.len()).rev() {
        for j in (0..new.len()).rev() {
            table[i][j] = if old[i] == new[j] {
                table[i + 1][j + 1] + 1
            } else {
                table[i + 1][j].max(table[i][j + 1])
            };
        }
    }
    let (mut i, mut j) = (0, 0);
    let mut out = Vec::with_capacity(old.len() + new.len());
    while i < old.len() || j < new.len() {
        if i < old.len() && j < new.len() && old[i] == new[j] {
            out.push((DiffKind::Same, old[i]));
            i += 1;
            j += 1;
        } else if i < old.len() && (j == new.len() || table[i + 1][j] >= table[i][j + 1]) {
            // Suppressions d'abord, puis ajouts, comme dans un diff habituel.
            out.push((DiffKind::Removed, old[i]));
            i += 1;
        } else {
            out.push((DiffKind::Added, new[j]));
            j += 1;
        }
    }
    out
}

fn diff_row(
    number: Option<usize>,
    sign: char,
    text: &str,
    background: &str,
    width: usize,
) -> String {
    let number = number.map_or_else(|| "     ".to_string(), |number| format!("{number:>5}"));
    let width = width.saturating_sub(8).max(10);
    let body = clip(&text.replace('\t', "    "), width);
    if background.is_empty() {
        format!("{DIM}{number}{RESET}   {body}")
    } else {
        let pad = width.saturating_sub(body.chars().count());
        format!(
            "{DIM}{number}{RESET} {background}{sign} {body}{}{RESET}",
            " ".repeat(pad)
        )
    }
}

/// Diff numéroté d'un remplacement ; `first_line` : ligne du nouveau texte
/// dans le fichier (`None` si inconnue), `file` : contenu actuel pour le
/// contexte, `width` : largeur des lignes.
#[must_use]
pub fn render_diff(
    old_text: &str,
    new_text: &str,
    first_line: Option<usize>,
    file: Option<&str>,
    max_lines: usize,
    width: usize,
) -> Vec<String> {
    let old: Vec<&str> = old_text.lines().collect();
    let new: Vec<&str> = new_text.lines().collect();
    let file_lines: Vec<&str> = file.map(|file| file.lines().collect()).unwrap_or_default();
    let mut rows = Vec::new();
    if let Some(first) = first_line.filter(|_| !file_lines.is_empty()) {
        let start = first.saturating_sub(DIFF_CONTEXT).max(1);
        for number in start..first {
            if let Some(line) = file_lines.get(number - 1) {
                rows.push(diff_row(Some(number), ' ', line, "", width));
            }
        }
    }
    let (mut old_number, mut new_number) = (first_line, first_line);
    for (kind, line) in diff_lines(&old, &new) {
        match kind {
            DiffKind::Same => {
                rows.push(diff_row(new_number, ' ', line, "", width));
                old_number = old_number.map(|n| n + 1);
                new_number = new_number.map(|n| n + 1);
            }
            DiffKind::Removed => {
                rows.push(diff_row(old_number, '-', line, REMOVED_BG, width));
                old_number = old_number.map(|n| n + 1);
            }
            DiffKind::Added => {
                rows.push(diff_row(new_number, '+', line, ADDED_BG, width));
                new_number = new_number.map(|n| n + 1);
            }
        }
    }
    if let Some(next) = new_number.filter(|_| !file_lines.is_empty()) {
        for number in next..next + DIFF_CONTEXT {
            if let Some(line) = file_lines.get(number - 1) {
                rows.push(diff_row(Some(number), ' ', line, "", width));
            }
        }
    }
    if rows.len() > max_lines {
        let hidden = rows.len() - max_lines;
        rows.truncate(max_lines);
        rows.push(more_lines(hidden));
    }
    rows
}

fn count_changes(old_text: &str, new_text: &str) -> (usize, usize) {
    let old: Vec<&str> = old_text.lines().collect();
    let new: Vec<&str> = new_text.lines().collect();
    diff_lines(&old, &new)
        .iter()
        .fold((0, 0), |(added, removed), (kind, _)| match kind {
            DiffKind::Added => (added + 1, removed),
            DiffKind::Removed => (added, removed + 1),
            DiffKind::Same => (added, removed),
        })
}

/// Lignes entières touchées par un remplacement : `(première ligne, bloc qui
/// contient needle, même bloc avec replacement)`. `None` si needle est absent.
#[must_use]
pub fn expand_to_lines(
    file: &str,
    needle: &str,
    replacement: &str,
) -> Option<(usize, String, String)> {
    if needle.is_empty() {
        return None;
    }
    let start = file.find(needle)?;
    let end = start + needle.len();
    let line_start = file[..start].rfind('\n').map_or(0, |index| index + 1);
    let line_end = file[end..]
        .find('\n')
        .map_or(file.len(), |index| end + index);
    let first_line = file[..line_start].matches('\n').count() + 1;
    let with_replacement = format!(
        "{}{replacement}{}",
        &file[line_start..start],
        &file[end..line_end]
    );
    Some((
        first_line,
        file[line_start..line_end].to_string(),
        with_replacement,
    ))
}

fn edit_result(parsed: &Value, limits: &Limits) -> Vec<String> {
    let raw_path = str_field(parsed, &["filePath", "path"]).unwrap_or("?");
    let path = display_path(raw_path);
    let old_string = str_field(parsed, &["oldString"]).unwrap_or_default();
    let new_string = str_field(parsed, &["newString"]).unwrap_or_default();
    // Le fichier contient déjà le nouveau texte : les lignes complètes autour
    // de lui donnent un diff numéroté lisible.
    let file = std::fs::read_to_string(raw_path.trim_start_matches(r"\\?\")).ok();
    let expanded = file
        .as_deref()
        .and_then(|file| expand_to_lines(file, new_string, old_string));
    let (first_line, old_text, new_text) = match &expanded {
        Some((first, new_block, old_block)) => {
            (Some(*first), old_block.as_str(), new_block.as_str())
        }
        None => (None, old_string, new_string),
    };
    let (added, removed) = count_changes(old_text, new_text);
    let mut summary = format!("Modifié {BOLD}{path}{RESET}");
    let mut changes = Vec::new();
    if added > 0 {
        changes.push(plural(added as u64, "ajout"));
    }
    if removed > 0 {
        changes.push(plural(removed as u64, "suppression"));
    }
    if !changes.is_empty() {
        let _ = write!(summary, " {DIM}({}){RESET}", changes.join(", "));
    }
    if parsed.get("replaceAll").and_then(Value::as_bool) == Some(true) {
        let _ = write!(summary, " {DIM}· toutes les occurrences{RESET}");
    }
    let mut lines = vec![summary];
    if let Some(note) = str_field(parsed, &["matchNote"]) {
        lines.push(format!("{DIM}{note}{RESET}"));
    }
    lines.extend(render_diff(
        old_text,
        new_text,
        first_line,
        file.as_deref(),
        limits.diff,
        content_width(),
    ));
    lines
}

fn names_preview(parsed: &Value, max_lines: usize) -> Vec<String> {
    let names: Vec<String> = parsed
        .get("filenames")
        .and_then(Value::as_array)
        .map(|files| {
            files
                .iter()
                .filter_map(Value::as_str)
                .map(display_path)
                .collect()
        })
        .unwrap_or_default();
    preview(&names.join("\n"), max_lines, "")
}

fn glob_result(parsed: &Value, limits: &Limits) -> Vec<String> {
    let count = parsed.get("numFiles").and_then(Value::as_u64).unwrap_or(0);
    let mut lines = vec![if count == 0 {
        "Aucun fichier trouvé".to_string()
    } else {
        format!(
            "{} trouvé{}",
            plural(count, "fichier"),
            if count > 1 { "s" } else { "" }
        )
    }];
    lines.extend(names_preview(parsed, limits.names));
    lines
}

fn grep_result(parsed: &Value, limits: &Limits) -> Vec<String> {
    let matches = parsed.get("numMatches").and_then(Value::as_u64);
    let files = parsed.get("numFiles").and_then(Value::as_u64).unwrap_or(0);
    let content = str_field(parsed, &["content"]).unwrap_or_default();
    let summary = match matches {
        Some(0) | None if files == 0 && content.trim().is_empty() => {
            "Aucune correspondance".to_string()
        }
        Some(count) if count > 0 => format!(
            "{} dans {}",
            plural(count, "correspondance"),
            plural(files, "fichier")
        ),
        _ => format!("Trouvé dans {}", plural(files, "fichier")),
    };
    let mut lines = vec![summary];
    if content.trim().is_empty() {
        lines.extend(names_preview(parsed, limits.names));
    } else {
        lines.extend(preview(content, limits.names, ""));
    }
    lines
}

fn todo_result(parsed: &Value) -> Vec<String> {
    let todos = parsed
        .get("newTodos")
        .or_else(|| parsed.get("todos"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if todos.is_empty() {
        return vec![format!("{DIM}Liste de tâches vide{RESET}")];
    }
    todos
        .iter()
        .map(|todo| {
            let content = str_field(todo, &["content"]).unwrap_or("?");
            match str_field(todo, &["status"]) {
                Some("completed") => format!("{GREEN}✓{RESET} {DIM}\x1b[9m{content}{RESET}"),
                Some("in_progress") => format!("{ACCENT}■{RESET} {BOLD}{content}{RESET}"),
                _ => format!("□ {content}"),
            }
        })
        .collect()
}

/// Un avis par modèle : son nom, puis le début de sa réponse (tout avec Ctrl+O).
fn consult_result(parsed: &Value, limits: &Limits) -> Vec<String> {
    let mut lines = Vec::new();
    for opinion in parsed
        .get("opinions")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
    {
        let model = str_field(opinion, &["model"]).unwrap_or("?");
        let answer = str_field(opinion, &["answer"]).unwrap_or_default();
        lines.push(format!("{BOLD}{model}{RESET}"));
        let shown = if limits.read_content { limits.lines } else { 2 };
        lines.extend(preview(answer, shown, ""));
    }
    let unavailable = parsed
        .get("unavailable")
        .and_then(Value::as_array)
        .map_or(0, Vec::len);
    if unavailable > 0 {
        lines.push(format!("{DIM}{unavailable} modèle(s) sans réponse{RESET}"));
    }
    if lines.is_empty() {
        lines.push(format!("{DIM}Aucun avis{RESET}"));
    }
    lines
}

fn web_fetch_result(parsed: &Value) -> Vec<String> {
    let bytes = parsed.get("bytes").and_then(Value::as_u64);
    let code = parsed.get("code").and_then(Value::as_u64);
    let code_text = str_field(parsed, &["codeText"]).unwrap_or_default();
    match (bytes, code) {
        (Some(bytes), Some(code)) => {
            #[allow(clippy::cast_precision_loss)]
            let size = format!("{:.1}", bytes as f64 / 1024.0).replace('.', ",");
            vec![format!("Reçu {size} Ko {DIM}({code} {code_text}){RESET}")]
        }
        _ => generic_result(parsed, &COMPACT),
    }
}

fn generic_result(parsed: &Value, limits: &Limits) -> Vec<String> {
    let text = match parsed {
        Value::String(text) => text.clone(),
        Value::Null => String::new(),
        Value::Object(_) | Value::Array(_) => {
            serde_json::to_string_pretty(parsed).unwrap_or_else(|_| parsed.to_string())
        }
        other => other.to_string(),
    };
    if text.trim().is_empty() {
        return vec![format!("{DIM}Terminé{RESET}")];
    }
    preview(&text, limits.lines, "")
}

/// Le dernier tour en entier (Ctrl+O) : raisonnement et sorties complètes.
#[derive(Default)]
struct TurnRecord {
    thinking: String,
    tools: Vec<(String, String, String, bool)>,
}

static LAST_TURN: Mutex<Option<TurnRecord>> = Mutex::new(None);

fn last_turn() -> std::sync::MutexGuard<'static, Option<TurnRecord>> {
    LAST_TURN
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Nouveau tour : le précédent n'est plus montré par Ctrl+O.
pub fn begin_turn() {
    *last_turn() = Some(TurnRecord::default());
}

pub fn record_thinking(text: &str) {
    if let Some(record) = last_turn().as_mut() {
        record.thinking.push_str(text);
    }
}

pub fn record_result(name: &str, input: &str, output: &str, is_error: bool) {
    if let Some(record) = last_turn().as_mut() {
        record.tools.push((
            name.to_string(),
            input.to_string(),
            output.to_string(),
            is_error,
        ));
    }
}

/// Affichage de Ctrl+O.
#[must_use]
pub fn turn_details() -> String {
    let guard = last_turn();
    let Some(record) = guard
        .as_ref()
        .filter(|record| !record.thinking.trim().is_empty() || !record.tools.is_empty())
    else {
        return format!(
            "{DIM}  ⎿  Rien à déplier : le dernier tour n'a ni raisonnement ni outil.{RESET}"
        );
    };
    let mut out = format!("{DIM}── Dernier tour en entier (Ctrl+O) ──{RESET}\n");
    if !record.thinking.trim().is_empty() {
        let _ = write!(out, "\n{DIM}\x1b[3m∴ Réflexion{RESET}\n");
        for line in wrap(record.thinking.trim(), content_width()) {
            let _ = writeln!(out, "  {DIM}\x1b[3m{line}{RESET}");
        }
    }
    for (name, input, output, is_error) in &record.tools {
        let _ = write!(
            out,
            "\n{}\n{}\n",
            tool_call_line(name, input),
            render_result(name, output, *is_error, &FULL)
        );
    }
    out
}

/// Appels reçus du modèle mais pas encore affichés : chaque appel s'affiche
/// juste avant sa demande d'autorisation ou son exécution, pour que son
/// résultat apparaisse directement dessous.
static PENDING_CALLS: Mutex<VecDeque<(String, String)>> = Mutex::new(VecDeque::new());

fn pending_calls() -> std::sync::MutexGuard<'static, VecDeque<(String, String)>> {
    PENDING_CALLS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Le modèle vient d'appeler `name` : l'appel s'affichera à son exécution.
pub fn queue_call(name: &str, input: &str) {
    pending_calls().push_back((name.to_string(), input.to_string()));
}

/// Affiche l'appel en attente correspondant (rien s'il l'est déjà).
pub fn print_call(name: &str, input: &str) {
    let found = {
        let mut calls = pending_calls();
        let index = calls
            .iter()
            .position(|(queued, queued_input)| queued == name && queued_input == input)
            .or_else(|| calls.iter().position(|(queued, _)| queued == name));
        index.and_then(|index| calls.remove(index))
    };
    if found.is_some() {
        let mut out = crate::live::LiveStdout::new();
        let _ = writeln!(out, "\n{}", tool_call_line(name, input));
        let _ = out.flush();
    }
}

/// Oublie les appels en attente (réponse interrompue puis relancée).
pub fn discard_calls() {
    pending_calls().clear();
}

/// Appels jamais exécutés (refusés par le mode ou un hook) : affichés quand même.
pub fn flush_calls() {
    let calls: Vec<(String, String)> = pending_calls().drain(..).collect();
    if calls.is_empty() {
        return;
    }
    let mut out = crate::live::LiveStdout::new();
    for (name, input) in calls {
        let _ = writeln!(
            out,
            "\n{}\n{RESULT_PREFIX}{DIM}Non exécuté{RESET}",
            tool_call_line(&name, &input)
        );
    }
    let _ = out.flush();
}

/// Raisonnement du modèle : une ligne `∴ Réflexion (3 s)` une fois terminé,
/// ou le texte complet avec `CLAW_SHOW_THINKING=1`.
pub struct ThinkingDisplay {
    started: Option<Instant>,
    full: bool,
    prefixer: Prefixer,
}

impl ThinkingDisplay {
    #[must_use]
    pub fn new() -> Self {
        Self {
            started: None,
            full: env::var("CLAW_SHOW_THINKING").is_ok_and(|value| value.trim() == "1"),
            prefixer: Prefixer::new("∴", "\x1b[2;3m"),
        }
    }

    pub fn push(&mut self, out: &mut (impl Write + ?Sized), text: &str) -> io::Result<()> {
        if self.started.is_none() {
            self.started = Some(Instant::now());
        }
        if self.full {
            self.prefixer.write(out, text)?;
        }
        Ok(())
    }

    /// Fin du raisonnement (la réponse commence ou le bloc se termine).
    pub fn close(&mut self, out: &mut (impl Write + ?Sized)) -> io::Result<()> {
        let Some(started) = self.started.take() else {
            return Ok(());
        };
        if self.full {
            self.prefixer.finish_line(out)?;
            self.prefixer.reset();
        } else {
            let seconds = started.elapsed().as_secs().max(1);
            let hint = if interactive() {
                " · Ctrl+O pour la lire"
            } else {
                ""
            };
            writeln!(out, "\n{DIM}\x1b[3m∴ Réflexion ({seconds} s{hint}){RESET}")?;
        }
        out.flush()
    }
}

/// Coupe une ligne colorée en morceaux d'au plus `width` caractères visibles,
/// de préférence sur un espace.
fn wrap_ansi(line: &str, width: usize) -> Vec<String> {
    if width < 10 || visible_width(line) <= width {
        return vec![line.to_string()];
    }
    let mut pieces = Vec::new();
    let mut current = String::new();
    let mut visible = 0;
    // Dernier espace de la ligne en cours : (position dans `current`, caractères visibles avant).
    let mut last_space: Option<(usize, usize)> = None;
    let mut chars = line.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\x1b' {
            current.push(ch);
            if chars.peek() == Some(&'[') {
                for next in chars.by_ref() {
                    current.push(next);
                    if next.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
            continue;
        }
        if visible == width {
            if let Some((index, before)) = last_space.filter(|(_, before)| *before > width / 3) {
                let rest = current.split_off(index);
                pieces.push(std::mem::take(&mut current));
                current = rest.strip_prefix(' ').unwrap_or(&rest).to_string();
                visible -= before + 1;
            } else {
                pieces.push(std::mem::take(&mut current));
                visible = 0;
            }
            last_space = None;
        }
        if ch == ' ' {
            last_space = Some((current.len(), visible));
        }
        current.push(ch);
        visible += 1;
    }
    pieces.push(current);
    pieces
}

/// Préfixe le texte de l'assistant : `● ` sur la première ligne d'une réponse,
/// deux espaces sur les suivantes. Les lignes vides ne sont écrites qu'entre
/// deux lignes de texte (ni avant ni après la réponse).
pub struct Prefixer {
    marker: &'static str,
    style: &'static str,
    at_line_start: bool,
    first: bool,
    /// Lignes vides en attente de la ligne suivante.
    blank: Vec<String>,
}

impl Prefixer {
    #[must_use]
    pub fn assistant() -> Self {
        Self::new("●", "")
    }

    #[must_use]
    pub fn new(marker: &'static str, style: &'static str) -> Self {
        Self {
            marker,
            style,
            at_line_start: true,
            first: true,
            blank: Vec::new(),
        }
    }

    /// Le prochain texte commence une nouvelle réponse (nouveau `●`).
    pub fn reset(&mut self) {
        self.first = true;
        self.blank.clear();
    }

    pub fn write(&mut self, out: &mut (impl Write + ?Sized), text: &str) -> io::Result<()> {
        for piece in text.split_inclusive('\n') {
            let ends_line = piece.ends_with('\n');
            let body = piece.trim_end_matches('\n').trim_end_matches('\r');
            if self.at_line_start {
                if strip_ansi(body).trim().is_empty() {
                    // Une seule ligne vide entre deux blocs ; celles d'un bloc
                    // de code (fond coloré) sont gardées telles quelles.
                    let plain = !body.contains("48;");
                    let repeated = plain
                        && self
                            .blank
                            .last()
                            .is_some_and(|previous| !previous.contains("48;"));
                    if !self.first && ends_line && !repeated {
                        self.blank.push(body.to_string());
                    }
                    continue;
                }
                for blank in self.blank.drain(..) {
                    writeln!(out, "{blank}{RESET}")?;
                }
                if self.first {
                    write!(out, "\n{}{}{RESET} {}", self.style, self.marker, self.style)?;
                    self.first = false;
                } else {
                    write!(out, "  {}", self.style)?;
                }
                // Ligne plus large que le terminal : la suite reste en retrait.
                let segments = wrap_ansi(body, terminal_width().saturating_sub(3));
                for (index, segment) in segments.iter().enumerate() {
                    if index > 0 {
                        write!(out, "\n  {}", self.style)?;
                    }
                    write!(out, "{segment}")?;
                }
                if ends_line {
                    writeln!(out, "{RESET}")?;
                }
                self.at_line_start = ends_line;
                continue;
            }
            write!(out, "{body}")?;
            if ends_line {
                writeln!(out, "{RESET}")?;
            }
            self.at_line_start = ends_line;
        }
        out.flush()
    }

    /// Termine la ligne en cours (avant un appel d'outil ou la fin du tour).
    pub fn finish_line(&mut self, out: &mut (impl Write + ?Sized)) -> io::Result<()> {
        if !self.at_line_start {
            writeln!(out, "{RESET}")?;
            self.at_line_start = true;
        }
        out.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_call_lines_name_the_action_and_its_target() {
        let line = strip_ansi(&tool_call_line("bash", r#"{"command":"cargo test"}"#));
        assert_eq!(line, "● Commande(cargo test)");
        let line = strip_ansi(&tool_call_line("read_file", r#"{"path":"src/main.rs"}"#));
        assert_eq!(line, "● Lecture(src/main.rs)");
        let line = strip_ansi(&tool_call_line("TodoWrite", r#"{"todos":[]}"#));
        assert_eq!(line, "● Tâches");
        let line = strip_ansi(&tool_call_line(
            "grep_search",
            r#"{"pattern":"fn main","path":"src"}"#,
        ));
        assert_eq!(line, "● Recherche(\"fn main\" dans src)");
    }

    #[test]
    fn results_hang_under_the_call_and_stay_short() {
        let stdout = (0..20)
            .map(|n| format!("ligne {n}"))
            .collect::<Vec<_>>()
            .join("\n");
        let output = serde_json::json!({ "stdout": stdout, "stderr": "" }).to_string();
        let rendered = strip_ansi(&tool_result("bash", &output, false));
        let lines: Vec<&str> = rendered.lines().collect();
        assert_eq!(lines[0], "  ⎿  ligne 0");
        assert_eq!(lines[1], "     ligne 1");
        assert!(rendered.contains("… +14 lignes"));
        assert!(!rendered.contains("ligne 19"));

        let empty = serde_json::json!({ "stdout": "", "stderr": "" }).to_string();
        assert_eq!(
            strip_ansi(&tool_result("bash", &empty, false)),
            "  ⎿  (Aucune sortie)"
        );

        let failed = serde_json::json!({
            "stdout": "", "stderr": "introuvable", "returnCodeInterpretation": "exit_code:2"
        })
        .to_string();
        let rendered = strip_ansi(&tool_result("bash", &failed, false));
        assert!(rendered.contains("introuvable"));
        assert!(rendered.contains("Code de sortie 2"));

        let error = strip_ansi(&tool_result("bash", "permission refusée", true));
        assert_eq!(error, "  ⎿  Erreur : permission refusée");
    }

    #[test]
    fn read_results_count_lines_without_dumping_the_file() {
        let output = r#"{"file":{"filePath":"src/main.rs","content":"a\nb","numLines":2,"startLine":1,"totalLines":2}}"#;
        assert_eq!(
            strip_ansi(&tool_result("read_file", output, false)),
            "  ⎿  Lu 2 lignes"
        );
        let partial = r#"{"file":{"filePath":"x","content":"a","numLines":10,"startLine":5,"totalLines":90}}"#;
        assert_eq!(
            strip_ansi(&tool_result("read_file", partial, false)),
            "  ⎿  Lu 10 lignes (lignes 5-14 sur 90)"
        );
    }

    #[test]
    fn diffs_are_numbered_with_context_from_the_file() {
        let file = "un\ndeux\ntrois nouveau\nquatre\ncinq\n";
        let rows: Vec<String> = render_diff("trois", "trois nouveau", Some(3), Some(file), 24, 60)
            .iter()
            .map(|row| strip_ansi(row).trim_end().to_string())
            .collect();
        assert_eq!(
            rows,
            vec![
                "    1   un",
                "    2   deux",
                "    3 - trois",
                "    3 + trois nouveau",
                "    4   quatre",
                "    5   cinq",
            ]
        );
        assert_eq!(count_changes("a\nb\nc", "a\nB\nc\nd"), (2, 1));
        assert_eq!(
            expand_to_lines(file, "nouveau", "ancien"),
            Some((3, "trois nouveau".to_string(), "trois ancien".to_string()))
        );
        assert_eq!(expand_to_lines(file, "absent", "x"), None);
    }

    #[test]
    fn todo_lists_show_their_progress() {
        let output = serde_json::json!({ "newTodos": [
            { "content": "Lire", "activeForm": "Lecture", "status": "completed" },
            { "content": "Écrire", "activeForm": "Écriture", "status": "in_progress" },
            { "content": "Tester", "activeForm": "Tests", "status": "pending" },
        ]})
        .to_string();
        assert_eq!(
            strip_ansi(&tool_result("TodoWrite", &output, false)),
            "  ⎿  ✓ Lire\n     ■ Écrire\n     □ Tester"
        );
    }

    #[test]
    fn assistant_text_gets_a_bullet_then_an_indent() {
        let mut out = Vec::new();
        let mut prefixer = Prefixer::assistant();
        prefixer.write(&mut out, "\nBonjour,\n").unwrap();
        prefixer.write(&mut out, "\nvoici la ").unwrap();
        prefixer.write(&mut out, "suite.\n").unwrap();
        prefixer.finish_line(&mut out).unwrap();
        let text = strip_ansi(&String::from_utf8(out).unwrap());
        assert_eq!(text, "\n● Bonjour,\n\n  voici la suite.\n");

        // Lignes vides répétées : une seule.
        let mut out = Vec::new();
        let mut prefixer = Prefixer::assistant();
        prefixer.write(&mut out, "Bloc\n\n\n\nSuite\n").unwrap();
        let text = strip_ansi(&String::from_utf8(out).unwrap());
        assert_eq!(text, "\n● Bloc\n\n  Suite\n");

        // Ligne trop longue : coupée sur un espace, la suite en retrait.
        let long = "mot ".repeat(40);
        let pieces = wrap_ansi(long.trim_end(), 30);
        assert!(pieces.iter().all(|piece| visible_width(piece) <= 30));
        assert_eq!(pieces.concat().replace(' ', ""), long.replace(' ', ""));
        assert!(pieces[1].starts_with("mot"));

        // Blocs Markdown successifs : une ligne vide entre eux, aucune à la fin.
        let mut out = Vec::new();
        let mut prefixer = Prefixer::assistant();
        prefixer.write(&mut out, "Titre :\n\n").unwrap();
        prefixer.write(&mut out, "• un\n• deux\n\n").unwrap();
        prefixer.finish_line(&mut out).unwrap();
        let text = strip_ansi(&String::from_utf8(out).unwrap());
        assert_eq!(text, "\n● Titre :\n\n  • un\n  • deux\n");
    }

    #[test]
    fn ctrl_o_shows_the_whole_last_turn() {
        begin_turn();
        assert!(strip_ansi(&turn_details()).contains("Rien à déplier"));
        let stdout = (0..20)
            .map(|n| format!("ligne {n}"))
            .collect::<Vec<_>>()
            .join("\n");
        record_thinking("Je lis d'abord ");
        record_thinking("le fichier.");
        record_result(
            "bash",
            r#"{"command":"seq 20"}"#,
            &serde_json::json!({ "stdout": stdout, "stderr": "" }).to_string(),
            false,
        );
        let details = strip_ansi(&turn_details());
        assert!(details.contains("Je lis d'abord le fichier."));
        assert!(details.contains("● Commande(seq 20)"));
        assert!(details.contains("ligne 19"));
        assert!(!details.contains("+14 lignes"));
        begin_turn();
    }

    #[test]
    fn boxes_pad_lines_to_the_same_width() {
        let lines = boxed(&["abc".to_string(), format!("{BOLD}é{RESET}")], 10, "");
        let plain: Vec<String> = lines.iter().map(|line| strip_ansi(line)).collect();
        assert_eq!(plain[0], "╭────────╮");
        assert_eq!(plain[1], "│ abc    │");
        assert_eq!(plain[2], "│ é      │");
        assert_eq!(plain[3], "╰────────╯");
        assert_eq!(wrap("un deux trois", 7), vec!["un deux", "trois"]);
        assert_eq!(clip_start("abcdefgh", 5), "…efgh");
    }
}
