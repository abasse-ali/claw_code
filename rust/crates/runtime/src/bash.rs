use std::env;
use std::io;
use std::process::{Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::bash_tasks::Running;
use crate::sandbox::{
    build_linux_sandbox_command, resolve_sandbox_status_for_request, FilesystemIsolationMode,
    SandboxConfig, SandboxStatus,
};
use crate::ConfigLoader;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BashCommandInput {
    pub command: String,
    pub timeout: Option<u64>,
    pub description: Option<String>,
    #[serde(rename = "run_in_background")]
    pub run_in_background: Option<bool>,
    #[serde(rename = "dangerouslyDisableSandbox")]
    pub dangerously_disable_sandbox: Option<bool>,
    #[serde(rename = "namespaceRestrictions")]
    pub namespace_restrictions: Option<bool>,
    #[serde(rename = "isolateNetwork")]
    pub isolate_network: Option<bool>,
    #[serde(rename = "filesystemMode")]
    pub filesystem_mode: Option<FilesystemIsolationMode>,
    #[serde(rename = "allowedMounts")]
    pub allowed_mounts: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BashCommandOutput {
    pub stdout: String,
    pub stderr: String,
    #[serde(rename = "rawOutputPath")]
    pub raw_output_path: Option<String>,
    pub interrupted: bool,
    #[serde(rename = "isImage")]
    pub is_image: Option<bool>,
    #[serde(rename = "backgroundTaskId")]
    pub background_task_id: Option<String>,
    #[serde(rename = "backgroundedByUser")]
    pub backgrounded_by_user: Option<bool>,
    #[serde(rename = "assistantAutoBackgrounded")]
    pub assistant_auto_backgrounded: Option<bool>,
    #[serde(rename = "dangerouslyDisableSandbox")]
    pub dangerously_disable_sandbox: Option<bool>,
    #[serde(rename = "returnCodeInterpretation")]
    pub return_code_interpretation: Option<String>,
    #[serde(rename = "noOutputExpected")]
    pub no_output_expected: Option<bool>,
    #[serde(rename = "structuredContent")]
    pub structured_content: Option<Vec<serde_json::Value>>,
    #[serde(rename = "persistedOutputPath")]
    pub persisted_output_path: Option<String>,
    #[serde(rename = "persistedOutputSize")]
    pub persisted_output_size: Option<u64>,
    #[serde(rename = "sandboxStatus")]
    pub sandbox_status: Option<SandboxStatus>,
}

pub fn execute_bash(input: BashCommandInput) -> io::Result<BashCommandOutput> {
    if plugins::posix_shell_program().is_none() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            plugins::missing_shell_message(),
        ));
    }
    // Sous Windows, la commande est adaptée à Git Bash (chemins `D:\…`).
    let input = if cfg!(windows) {
        BashCommandInput {
            command: adapt_windows_command(&input.command),
            ..input
        }
    } else {
        input
    };
    let cwd = env::current_dir()?;
    let sandbox_status = sandbox_status_for_input(&input, &cwd);
    // Un serveur ou une surveillance de fichiers ne rend jamais la main : lancé
    // au premier plan, il bloquerait le tour. Sauf avis contraire du modèle
    // (`run_in_background: false`), il part d'office en arrière-plan.
    let auto_background = input.run_in_background.is_none() && starts_long_running(&input.command);
    let background = input.run_in_background == Some(true) || auto_background;

    // `programme | tail` : le code de sortie doit rester celui du programme.
    let script = with_faithful_exit_codes(&input.command).unwrap_or_else(|| input.command.clone());
    // Sous Windows, le PID du shell MSYS permet d'arrêter tout le groupe.
    let pid_file = cfg!(windows).then(shell_group_pid_file);
    let wrapped = pid_file.as_deref().map_or_else(
        || script.clone(),
        |path| with_group_pid_capture(&script, path),
    );
    #[allow(unused_mut)]
    let mut command = prepare_command(&wrapped, &cwd, &sandbox_status, true);
    // Sous Unix, un groupe de processus dédié permet de tuer aussi les descendants.
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let container = take_last_container();
    let running =
        crate::bash_tasks::launch(command, &input.command, pid_file, container, !background)?;
    let output = if background {
        started_in_background(&running.detach(), auto_background)
    } else {
        run_in_foreground(running, explicit_timeout(input.timeout), Waits::DEFAULT)
    };
    Ok(BashCommandOutput {
        dangerously_disable_sandbox: input.dangerously_disable_sandbox,
        sandbox_status: Some(sandbox_status),
        ..output
    })
}

/// Commande d'un autre outil (`PowerShell`), déjà préparée (programme,
/// arguments, dossier) : mêmes règles que `bash` pour l'arrière-plan, la
/// patience et le délai fixé par le modèle.
pub fn run_tool_command(
    command: Command,
    display: &str,
    timeout: Option<u64>,
    run_in_background: Option<bool>,
) -> io::Result<BashCommandOutput> {
    let auto_background = run_in_background.is_none() && starts_long_running(display);
    let background = run_in_background == Some(true) || auto_background;
    let running = crate::bash_tasks::launch(command, display, None, None, !background)?;
    Ok(if background {
        started_in_background(&running.detach(), auto_background)
    } else {
        run_in_foreground(running, explicit_timeout(timeout), Waits::DEFAULT)
    })
}

/// Patience accordée à une commande au premier plan quand le modèle ne fixe
/// pas de délai : au-delà, elle continue en arrière-plan (rien n'est arrêté).
pub const DEFAULT_TIMEOUT_MS: u64 = 5 * 60 * 1000;

/// Délai demandé par le modèle, en millisecondes. Une valeur sous 1 000 vient
/// d'un modèle qui pensait en secondes (`120` pour deux minutes) : elle est lue
/// en secondes, au lieu d'arrêter la commande au bout de 120 millisecondes.
/// Sans délai (ou 0), `DEFAULT_TIMEOUT_MS`.
#[must_use]
pub fn effective_timeout_ms(requested: Option<u64>) -> u64 {
    match requested {
        None | Some(0) => DEFAULT_TIMEOUT_MS,
        Some(value) if value < 1_000 => value.saturating_mul(1_000),
        Some(value) => value,
    }
}

/// Délai fixé par le modèle (0 compte comme aucun), en millisecondes.
fn explicit_timeout(requested: Option<u64>) -> Option<u64> {
    requested
        .filter(|value| *value > 0)
        .map(|value| effective_timeout_ms(Some(value)))
}

/// Attentes d'une commande au premier plan.
#[derive(Clone, Copy)]
struct Waits {
    /// Sans délai fixé : passage en arrière-plan au-delà.
    patience: Duration,
    /// Serveur annoncé prêt puis silencieux ce temps-là : il attend des
    /// requêtes et ne finira pas, il passe en arrière-plan.
    ready_idle: Duration,
}

impl Waits {
    const DEFAULT: Self = Self {
        patience: Duration::from_millis(DEFAULT_TIMEOUT_MS),
        ready_idle: Duration::from_secs(8),
    };
}

/// Le shell a fini mais un processus qu'il a lancé (`serveur &`) tient encore
/// sa sortie : on attend ce temps-là avant de le laisser en arrière-plan.
const LEFTOVER_GRACE: Duration = Duration::from_millis(1_500);
const POLL: Duration = Duration::from_millis(20);

/// Attend une commande au premier plan :
/// - finie : son résultat ;
/// - délai fixé par le modèle dépassé : arrêtée, avec la sortie déjà produite ;
/// - sans délai fixé, toujours en cours au bout de la patience, ou serveur
///   annoncé prêt puis silencieux : elle continue en arrière-plan ;
/// - shell fini mais sortie tenue par un processus lancé avec `&` : ce
///   processus continue en arrière-plan.
fn run_in_foreground(
    mut running: Running,
    explicit_timeout: Option<u64>,
    waits: Waits,
) -> BashCommandOutput {
    let limit = explicit_timeout.map_or(waits.patience, Duration::from_millis);
    let started = Instant::now();
    loop {
        if crate::interrupt::interrupt_requested() {
            running.kill();
            let (stdout, stderr) = running.output_so_far();
            return interrupted_output(stdout, stderr);
        }
        match running.try_wait() {
            Ok(Some(status)) => {
                // Les lecteurs reçoivent la fin de la sortie ; un processus
                // lancé avec `&` peut la garder ouverte.
                let grace = Instant::now() + LEFTOVER_GRACE;
                while running.open_pipes() > 0 && Instant::now() < grace {
                    std::thread::sleep(Duration::from_millis(10));
                }
                if running.open_pipes() == 0 {
                    running.finish();
                    let (stdout, stderr) = running.output_so_far();
                    return finished_output(stdout, stderr, status, None);
                }
                let (stdout, stderr, task_id) = running.into_background();
                return finished_output(stdout, stderr, status, Some(task_id));
            }
            Ok(None) => {}
            Err(error) => {
                running.kill();
                let (stdout, stderr) = running.output_so_far();
                let mut output = base_output(stdout, stderr);
                append_note(&mut output.stderr, &format!("[Claw] {error}"));
                output.return_code_interpretation = Some("exit_code:-1".to_string());
                return output;
            }
        }
        if started.elapsed() >= limit {
            if explicit_timeout.is_some() {
                running.kill();
                let (stdout, stderr) = running.output_so_far();
                return timed_out_output(stdout, stderr, limit);
            }
            let (stdout, stderr, task_id) = running.into_background();
            return handed_off_output(stdout, stderr, &task_id, Some(limit));
        }
        if explicit_timeout.is_none() && running.ready_and_idle(waits.ready_idle) {
            let (stdout, stderr, task_id) = running.into_background();
            return handed_off_output(stdout, stderr, &task_id, None);
        }
        std::thread::sleep(POLL);
    }
}

fn base_output(stdout: String, stderr: String) -> BashCommandOutput {
    BashCommandOutput {
        no_output_expected: Some(stdout.trim().is_empty() && stderr.trim().is_empty()),
        stdout,
        stderr,
        raw_output_path: None,
        interrupted: false,
        is_image: None,
        background_task_id: None,
        backgrounded_by_user: None,
        assistant_auto_backgrounded: None,
        dangerously_disable_sandbox: None,
        return_code_interpretation: None,
        structured_content: None,
        persisted_output_path: None,
        persisted_output_size: None,
        sandbox_status: None,
    }
}

/// Ajoute une remarque à la suite d'une sortie.
fn append_note(text: &mut String, note: &str) {
    let kept = text.trim_end().len();
    text.truncate(kept);
    if !text.is_empty() {
        text.push_str("\n\n");
    }
    text.push_str(note);
}

fn started_in_background(task_id: &str, auto_background: bool) -> BashCommandOutput {
    let started = if auto_background {
        format!(
            "[Claw] This command starts a server or a watcher, which never exits on its own: it was started in the background as {task_id} instead of blocking. Read its output with BashOutput (bash_id: \"{task_id}\"), for example to check that it is listening, and stop it with KillShell."
        )
    } else {
        format!(
            "Started in the background as {task_id}. Read its output with BashOutput (bash_id: \"{task_id}\") and stop it with KillShell."
        )
    };
    BashCommandOutput {
        background_task_id: Some(task_id.to_string()),
        backgrounded_by_user: Some(false),
        assistant_auto_backgrounded: Some(auto_background),
        no_output_expected: Some(true),
        ..base_output(started, String::new())
    }
}

/// Code de sortie lu par le harnais : rien pour un succès.
fn exit_interpretation(status: ExitStatus) -> Option<String> {
    match status.code() {
        Some(0) => None,
        Some(code) => Some(format!("exit_code:{code}")),
        // Tuée par un signal (Unix) : ce n'est pas un succès.
        None => Some("killed_by_signal".to_string()),
    }
}

fn finished_output(
    stdout: String,
    stderr: String,
    status: ExitStatus,
    leftover: Option<String>,
) -> BashCommandOutput {
    let mut output = base_output(stdout, stderr);
    output.return_code_interpretation = exit_interpretation(status);
    if let Some(task_id) = leftover {
        append_note(
            &mut output.stdout,
            &format!(
                "[Claw] The command finished (its result is above), but a process it started still holds its output and keeps running in the background as {task_id}: for example a server launched with `&`. If you started it on purpose, read its output with BashOutput (bash_id: \"{task_id}\") and stop it with KillShell when you are done; otherwise ignore it."
            ),
        );
        output.background_task_id = Some(task_id);
        output.assistant_auto_backgrounded = Some(true);
        output.no_output_expected = Some(false);
    }
    output
}

/// Toujours en cours : la commande continue en arrière-plan. `waited` : la
/// patience écoulée ; `None` pour un serveur qui s'est annoncé prêt.
fn handed_off_output(
    stdout: String,
    stderr: String,
    task_id: &str,
    waited: Option<Duration>,
) -> BashCommandOutput {
    let mut output = base_output(stdout, stderr);
    let note = match waited {
        Some(waited) => format!(
            "[Claw] Still running after {} s, so it continues in the background as {task_id} (nothing was stopped). Wait for its end with BashOutput {{\"bash_id\": \"{task_id}\", \"block\": true}}, or stop it with KillShell.",
            waited.as_secs()
        ),
        None => format!(
            "[Claw] This command announced a server or a watcher that is ready, then went quiet: it keeps running, so it continues in the background as {task_id}. If it is a server or a watcher, read its output with BashOutput (bash_id: \"{task_id}\") and stop it with KillShell when you are done. If it is a test or build run that ends by itself, wait for its end with BashOutput {{\"bash_id\": \"{task_id}\", \"block\": true}}."
        ),
    };
    append_note(&mut output.stdout, &note);
    output.background_task_id = Some(task_id.to_string());
    output.assistant_auto_backgrounded = Some(true);
    output.no_output_expected = Some(false);
    output
}

fn timed_out_output(stdout: String, stderr: String, limit: Duration) -> BashCommandOutput {
    let seconds = limit.as_secs();
    let mut output = base_output(stdout, stderr);
    append_note(
        &mut output.stderr,
        &format!(
            "La commande a dépassé le délai de {seconds} s : elle a été arrêtée.\n[Claw] The command did not finish within the {seconds} s timeout you set and was stopped (its output so far is above). For a server or a watcher, use run_in_background: true and follow it with BashOutput. For a long build, install or test run, run it again without a timeout: if it takes long, it continues in the background instead of being stopped."
        ),
    );
    output.interrupted = true;
    output.return_code_interpretation = Some("timeout".to_string());
    output.no_output_expected = Some(false);
    output
}

fn interrupted_output(stdout: String, stderr: String) -> BashCommandOutput {
    let mut output = base_output(stdout, stderr);
    append_note(&mut output.stderr, "Commande interrompue par l'utilisateur");
    output.interrupted = true;
    output.return_code_interpretation = Some("interrupted".to_string());
    output
}

/// Commandes qui démarrent un serveur ou une surveillance de fichiers, après
/// d'éventuels `VAR=valeur`, `npx`, `uv run`, `poetry run`… : elles ne rendent
/// jamais la main d'elles-mêmes.
const LONG_RUNNING: &[&str] = &[
    "npm run dev",
    "npm run start",
    "npm start",
    "npm run serve",
    "npm run watch",
    "npm run preview",
    "yarn dev",
    "yarn start",
    "yarn serve",
    "yarn watch",
    "pnpm dev",
    "pnpm start",
    "pnpm run dev",
    "pnpm run start",
    "bun dev",
    "bun run dev",
    "next dev",
    "next start",
    "nuxt dev",
    "ng serve",
    "vite",
    "webpack serve",
    "webpack-dev-server",
    "nodemon",
    "http-server",
    "live-server",
    "serve",
    "uvicorn",
    "gunicorn",
    "hypercorn",
    "daphne",
    "flask run",
    "fastapi dev",
    "fastapi run",
    "streamlit run",
    "jupyter notebook",
    "jupyter lab",
    "python manage.py runserver",
    "python3 manage.py runserver",
    "python -m http.server",
    "python3 -m http.server",
    "python -m uvicorn",
    "python3 -m uvicorn",
    "python -m flask run",
    "python3 -m flask run",
    "rails server",
    "rails s",
    "php artisan serve",
    "hugo server",
    "mkdocs serve",
    "jekyll serve",
    "cargo watch",
    "tail -f",
];

/// La commande démarre au premier plan du shell un serveur ou une
/// surveillance : elle ne finira pas d'elle-même. Le texte entre guillemets,
/// un document en ligne (`<<`) et les commentaires ne comptent pas, ni un
/// serveur que le shell détache lui-même (`serveur &` suivi d'autres
/// commandes) : la commande se termine alors seule.
#[must_use]
pub fn starts_long_running(command: &str) -> bool {
    if command.contains("<<") {
        return false;
    }
    shell_segments(&command.to_ascii_lowercase())
        .iter()
        .any(|(segment, detached)| !detached && segment_is_long_running(segment))
}

/// Commandes simples d'une ligne de shell, séparées par `;`, `&&`, `||`, `|`,
/// `&`, une fin de ligne ou des parenthèses. Le texte entre guillemets devient
/// `_` (un mot, sans son contenu), les commentaires sont retirés. Chaque
/// commande dit si le shell la détache lui-même (`&` simple après elle).
pub(crate) fn shell_segments(command: &str) -> Vec<(String, bool)> {
    shell_pieces(command)
        .into_iter()
        .map(|(segment, closed_by)| (segment, closed_by == Closed::Detached))
        .collect()
}

/// Ce qui ferme une commande simple dans une ligne de shell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Closed {
    /// `|` ou `|&` : sa sortie va à la commande suivante.
    Pipe,
    /// `&` simple : le shell la détache.
    Detached,
    /// `;`, `&&`, `||`, fin de ligne, parenthèse, fin de la commande.
    Sequence,
}

/// Les commandes simples d'une ligne de shell, chacune avec ce qui la ferme.
pub(crate) fn shell_pieces(command: &str) -> Vec<(String, Closed)> {
    let chars: Vec<char> = command.chars().collect();
    let mut segments = Vec::new();
    let mut current = String::new();
    let mut index = 0;
    let close = |current: &mut String, segments: &mut Vec<(String, Closed)>, closed: Closed| {
        let segment = current.trim().to_string();
        if !segment.is_empty() {
            segments.push((segment, closed));
        }
        current.clear();
    };
    while index < chars.len() {
        let ch = chars[index];
        match ch {
            '\'' | '"' => {
                // Jusqu'au guillemet fermant (`\"` ne ferme pas entre guillemets doubles).
                index += 1;
                while index < chars.len() && chars[index] != ch {
                    if ch == '"' && chars[index] == '\\' {
                        index += 1;
                    }
                    index += 1;
                }
                current.push('_');
            }
            '\\' => {
                // Caractère protégé : jamais un séparateur.
                index += 1;
                current.push('_');
            }
            '#' if current.is_empty() || current.ends_with(char::is_whitespace) => {
                while index + 1 < chars.len() && chars[index + 1] != '\n' {
                    index += 1;
                }
            }
            '&' if matches!(chars.get(index + 1), Some('>'))
                || matches!(current.chars().last(), Some('>' | '<')) =>
            {
                // `&>`, `2>&1`, `>&2` : une redirection.
                current.push(ch);
            }
            '&' if chars.get(index + 1) == Some(&'&') => {
                close(&mut current, &mut segments, Closed::Sequence);
                index += 1;
            }
            '&' => close(&mut current, &mut segments, Closed::Detached),
            '|' if chars.get(index + 1) == Some(&'|') => {
                close(&mut current, &mut segments, Closed::Sequence);
                index += 1;
            }
            '|' => {
                close(&mut current, &mut segments, Closed::Pipe);
                if chars.get(index + 1) == Some(&'&') {
                    index += 1;
                }
            }
            ';' | '\n' | '(' | ')' => close(&mut current, &mut segments, Closed::Sequence),
            _ => current.push(ch),
        }
        index += 1;
    }
    close(&mut current, &mut segments, Closed::Sequence);
    segments
}

/// Filtres qui ne font que couper ou recopier une sortie.
const OUTPUT_FILTERS: &[&str] = &["tail", "head", "tee", "cat", "less", "more"];
/// Outils de texte derrière lesquels un modèle range la sortie d'une
/// vérification (`pytest … | grep -E "passed|failed"`).
const TEXT_TOOLS: &[&str] = &[
    "grep", "egrep", "findstr", "sed", "awk", "sort", "uniq", "cut",
];

/// Premier mot d'une commande simple, après ses variables d'environnement.
fn program_of(segment: &str) -> &str {
    segment
        .split_whitespace()
        .find(|word| !word.contains('=') || word.starts_with('-'))
        .unwrap_or_default()
}

/// Opérateur d'une ligne de shell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Op {
    /// `|` ou `|&`.
    Pipe,
    /// `&&`.
    And,
    /// `||`.
    Or,
    /// `;` ou fin de ligne.
    Then,
    /// `(`.
    Open,
    /// `)`.
    Close,
    /// `&` simple : ce qui précède est détaché.
    Detached,
}

/// Opérateurs d'une ligne de shell, hors guillemets, commentaires et
/// substitutions (`$(…)`, `` `…` ``), avec leur place dans `command` (octets).
/// Même lecture que `shell_pieces`.
pub(crate) fn shell_operators(command: &str) -> Vec<(std::ops::Range<usize>, Op)> {
    let chars: Vec<(usize, char)> = command.char_indices().collect();
    let after = |index: usize| chars.get(index + 1).map_or(command.len(), |(at, _)| *at);
    let mut operators = Vec::new();
    let mut word_start = true;
    let mut index = 0;
    while index < chars.len() {
        let (at, ch) = chars[index];
        let next = chars.get(index + 1).map(|(_, next)| *next);
        let previous = index.checked_sub(1).map(|before| chars[before].1);
        match ch {
            '\'' | '"' => index = closing_quote(&chars, index),
            '\\' => index += 1,
            // Substitution : son contenu ne sépare pas les commandes d'ici.
            '$' if next == Some('(') => {
                let mut depth = 0;
                index += 1;
                while index < chars.len() {
                    match chars[index].1 {
                        '(' => depth += 1,
                        ')' => {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                        }
                        '\'' | '"' => index = closing_quote(&chars, index),
                        _ => {}
                    }
                    index += 1;
                }
            }
            '`' => {
                index += 1;
                while index < chars.len() && chars[index].1 != '`' {
                    if chars[index].1 == '\\' {
                        index += 1;
                    }
                    index += 1;
                }
            }
            '#' if word_start => {
                while index + 1 < chars.len() && chars[index + 1].1 != '\n' {
                    index += 1;
                }
            }
            '&' if next == Some('>') || matches!(previous, Some('>' | '<')) => {}
            '&' if next == Some('&') => {
                operators.push((at..after(index + 1), Op::And));
                index += 1;
            }
            '&' => operators.push((at..after(index), Op::Detached)),
            '|' if next == Some('|') => {
                operators.push((at..after(index + 1), Op::Or));
                index += 1;
            }
            '|' if next == Some('&') => {
                operators.push((at..after(index + 1), Op::Pipe));
                index += 1;
            }
            '|' => operators.push((at..after(index), Op::Pipe)),
            ';' | '\n' => operators.push((at..after(index), Op::Then)),
            '(' => operators.push((at..after(index), Op::Open)),
            ')' => operators.push((at..after(index), Op::Close)),
            _ => {}
        }
        word_start = ch.is_whitespace() || matches!(ch, ';' | '|' | '&' | '(' | ')');
        index += 1;
    }
    operators
}

/// Indice du guillemet qui ferme celui d'indice `open` (`\"` ne ferme pas
/// entre guillemets doubles), ou du dernier caractère.
fn closing_quote(chars: &[(usize, char)], open: usize) -> usize {
    let quote = chars[open].1;
    let mut index = open + 1;
    while index < chars.len() && chars[index].1 != quote {
        if quote == '"' && chars[index].1 == '\\' {
            index += 1;
        }
        index += 1;
    }
    index.min(chars.len().saturating_sub(1))
}

/// Mots qui ouvrent une construction du shell : un tube qui commence par l'un
/// d'eux n'est pas une commande simple, Claw ne le réécrit pas.
const SHELL_KEYWORDS: &[&str] = &[
    "{", "}", "if", "then", "else", "elif", "fi", "while", "until", "do", "done", "for", "case",
    "esac", "function", "select", "time", "coproc", "!", "[[",
];

/// Début d'un tube réécrit : `pipefail` dans un sous-shell, essayé d'abord à
/// part (un `sh` qui ne le connaît pas, comme dash, s'arrêterait sinon).
const PIPEFAIL_GROUP: &str = "( (set -o pipefail) 2>/dev/null && set -o pipefail; ";

/// Les tubes de la commande qui rangent la sortie d'un programme derrière des
/// filtres (`npm run build 2>&1 | tail -20`), réécrits pour que leur code de
/// sortie soit celui du programme : sans précaution c'est celui du filtre, et
/// une compilation en échec passe pour réussie. Chaque tube a son `pipefail`
/// dans un sous-shell : posé pour tout le script, il changerait le sens des
/// autres tubes (`git log | head -5 && …` s'arrêterait sur le code 141 de git).
///
/// - Une vérification suivie de filtres (`tail`, `head`, `grep`…) : le reste
///   de sa sortie est lu et jeté (`| { head -100` puis `cat >/dev/null; }`),
///   pour qu'elle finisse toujours et rende son vrai code. Coupée par `head`,
///   une vérification en Python ou un test Rust (qui ignorent SIGPIPE)
///   sortirait en échec alors que tout passe.
/// - Un autre programme suivi de filtres qui lisent tout (`tail`, `tee`) :
///   `pipefail` seul. Derrière `head`, rien ne change : il peut ne jamais
///   finir (`yes | head`), et son échec d'écriture n'en serait pas un.
///
/// `None` : rien à réécrire.
pub(crate) fn with_faithful_exit_codes(command: &str) -> Option<String> {
    if command.contains("<<") {
        return None;
    }
    let operators = shell_operators(command);
    let mut rewrites: Vec<(std::ops::Range<usize>, String)> = Vec::new();
    let mut index = 0;
    while index < operators.len() {
        if operators[index].1 != Op::Pipe {
            index += 1;
            continue;
        }
        // Un tube : ses `|` à la suite, de `index` à `last`.
        let mut last = index;
        while operators
            .get(last + 1)
            .is_some_and(|(_, op)| *op == Op::Pipe)
        {
            last += 1;
        }
        rewrites.extend(faithful_pipeline(command, &operators, index, last));
        index = last + 1;
    }
    if rewrites.is_empty() {
        return None;
    }
    let mut rewritten = command.to_string();
    for (range, text) in rewrites.into_iter().rev() {
        rewritten.replace_range(range, &text);
    }
    Some(rewritten)
}

/// Le tube dont les `|` sont les opérateurs `first` à `last`, réécrit, et la
/// place (octets) du texte qu'il remplace.
fn faithful_pipeline(
    command: &str,
    operators: &[(std::ops::Range<usize>, Op)],
    first: usize,
    last: usize,
) -> Option<(std::ops::Range<usize>, String)> {
    let before = first.checked_sub(1).map(|index| &operators[index]);
    let after = operators.get(last + 1);
    // Un tube détaché (`npm run dev | tee log &`) tourne en arrière-plan.
    if after.is_some_and(|(_, op)| *op == Op::Detached) {
        return None;
    }
    let first_pipe = &operators[first].0;
    let mut start = before.map_or(0, |(range, _)| range.end);
    let end = after.map_or(command.len(), |(range, _)| range.start);
    let lead = command[start..first_pipe.start].trim();
    // `(cd app && npm run build) 2>&1 | tail` : le premier maillon est un
    // sous-shell, le tube commence à sa parenthèse.
    let subshell = before.is_some_and(|(_, op)| *op == Op::Close);
    if subshell {
        if !(lead.is_empty() || starts_with_redirection(lead)) {
            return None;
        }
        start = matching_open(operators, first - 1)?;
    }
    let producer = command[start..first_pipe.start].to_ascii_lowercase();
    let program = program_of(producer.trim()).to_string();
    if !subshell && (program.is_empty() || SHELL_KEYWORDS.contains(&program.as_str())) {
        return None;
    }
    let filters: Vec<String> = (first..=last)
        .map(|index| {
            let stage_end = if index < last {
                operators[index + 1].0.start
            } else {
                end
            };
            program_of(command[operators[index].0.end..stage_end].trim()).to_ascii_lowercase()
        })
        .collect();
    let is_filter = |name: &String| OUTPUT_FILTERS.contains(&name.as_str());
    let is_text_tool = |name: &String| TEXT_TOOLS.contains(&name.as_str());
    let checking = crate::harness::is_check_command(&producer)
        && filters
            .iter()
            .all(|name| is_filter(name) || is_text_tool(name));
    let text = if checking {
        format!(
            "{PIPEFAIL_GROUP}{} {{ {}\ncat >/dev/null; }} )",
            command[start..first_pipe.end].trim_start(),
            command[first_pipe.end..end].trim()
        )
    } else if filters.iter().all(is_filter) && !filters.iter().any(|name| name == "head") {
        format!("{PIPEFAIL_GROUP}{}\n)", command[start..end].trim())
    } else {
        return None;
    };
    // Les blancs autour du tube restent ; après une parenthèse, un espace
    // évite `((`, qui ouvrirait un calcul.
    let whole = &command[start..end];
    let from = start + (whole.len() - whole.trim_start().len());
    let to = end - (whole.len() - whole.trim_end().len());
    let pad = if command[..from].ends_with('(') {
        " "
    } else {
        ""
    };
    Some((from..to, format!("{pad}{text}")))
}

/// `2>&1 | …`, `> log | …` : le texte commence par une redirection.
fn starts_with_redirection(text: &str) -> bool {
    let rest = text.trim_start_matches(|ch: char| ch.is_ascii_digit());
    rest.starts_with(['>', '<']) || rest.starts_with("&>")
}

/// Début (octet) de la parenthèse qui ouvre celle d'indice `close`.
fn matching_open(operators: &[(std::ops::Range<usize>, Op)], close: usize) -> Option<usize> {
    let mut depth = 0_usize;
    for index in (0..=close).rev() {
        match operators[index].1 {
            Op::Close => depth += 1,
            Op::Open => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(operators[index].0.start);
                }
            }
            _ => {}
        }
    }
    None
}

/// Le code de sortie de `command` est celui de ce qui finit à l'octet `from`
/// (une vérification) : après, son tube ne porte que des filtres de sa sortie
/// (rendus fidèles par `with_faithful_exit_codes`), et la suite n'est liée que
/// par `&&`. Un `|| echo`, un `; echo "exit=$?"`, un `&` ou un `| wc -l` le
/// masquent : la commande réussit même quand la vérification échoue.
pub(crate) fn exit_decided_at(command: &str, from: usize) -> bool {
    let operators = shell_operators(command);
    let following: Vec<&(std::ops::Range<usize>, Op)> = operators
        .iter()
        .filter(|(range, _)| range.start >= from)
        .collect();
    // Les `|` qui suivent la vérification sont ceux de son tube, jusqu'au
    // premier `&&` ; ceux des commandes suivantes ne changent rien.
    let mut own_pipeline = true;
    for (position, (range, op)) in following.iter().enumerate() {
        let masks = match op {
            Op::And => {
                own_pipeline = false;
                false
            }
            Op::Pipe if own_pipeline => {
                let stage_end = following
                    .get(position + 1)
                    .map_or(command.len(), |(next, _)| next.start);
                let program = program_of(command[range.end..stage_end].trim()).to_ascii_lowercase();
                !(OUTPUT_FILTERS.contains(&program.as_str())
                    || TEXT_TOOLS.contains(&program.as_str()))
            }
            // `;` ou fin de ligne : seulement s'il ne reste rien après.
            Op::Then => !command[range.end..].trim().is_empty(),
            Op::Or | Op::Detached => true,
            Op::Pipe | Op::Open | Op::Close => false,
        };
        if masks {
            return false;
        }
    }
    true
}

/// Une commande simple démarre-t-elle un serveur ou une surveillance ?
fn segment_is_long_running(segment: &str) -> bool {
    let mut words: Vec<&str> = segment
        .split_whitespace()
        .skip_while(|word| word.contains('=') && !word.starts_with('-'))
        .collect();
    let skip = match words.as_slice() {
        // `timeout 10 npm run dev` : un essai borné, voulu par le modèle.
        ["timeout", ..] => return false,
        ["npx" | "bunx" | "exec" | "nohup", ..] => 1,
        ["pnpm", "exec", ..] | ["poetry" | "uv" | "pipenv" | "pdm" | "hatch", "run", ..] => 2,
        _ => 0,
    };
    words.drain(..skip);
    // `vite build`, `next build` : une construction, qui se termine.
    if words.get(1) == Some(&"build")
        || words
            .iter()
            .any(|word| matches!(*word, "--help" | "-h" | "--version"))
    {
        return false;
    }
    if words.contains(&"--watch") || (words.first() == Some(&"tsc") && words.contains(&"-w")) {
        return true;
    }
    let joined = words.join(" ");
    LONG_RUNNING.iter().any(|pattern| {
        joined == *pattern
            || joined
                .strip_prefix(pattern)
                .is_some_and(|rest| rest.starts_with(' '))
    })
}

/// Sous Windows, `bash` est Git Bash : écrit hors guillemets, un chemin
/// `D:\dossier\fichier` y perd ses barres obliques inverses, et le `cd /d` de
/// cmd y est une erreur. La commande est réécrite comme elle était voulue :
/// `D:/dossier/fichier`, `cd` sans `/d`. Un document en ligne (`<<`) porte du
/// texte à garder tel quel : la commande n'est alors pas touchée.
fn adapt_windows_command(command: &str) -> String {
    if command.contains("<<") {
        return command.to_string();
    }
    let chars: Vec<char> = command.chars().collect();
    let mut out = String::with_capacity(command.len());
    let mut quote = None;
    let mut index = 0;
    while index < chars.len() {
        let ch = chars[index];
        if let Some(open) = quote {
            if ch == open {
                quote = None;
            } else if ch == '\\' && open == '"' {
                // `\"` ne ferme pas une chaîne entre guillemets doubles.
                if let Some(next) = chars.get(index + 1) {
                    out.push(ch);
                    out.push(*next);
                    index += 2;
                    continue;
                }
            }
            out.push(ch);
            index += 1;
            continue;
        }
        let word_start = out
            .chars()
            .next_back()
            .is_none_or(|last| last.is_whitespace() || "=;&|<>(".contains(last));
        if word_start {
            if let Some(path) = cd_drive_switch(&chars, index) {
                out.push_str("cd ");
                index = path;
                continue;
            }
            if is_drive_path(&chars, index) {
                index = push_drive_path(&chars, index, &mut out);
                continue;
            }
        }
        if ch == '"' || ch == '\'' {
            quote = Some(ch);
        } else if ch == '\\' {
            // Caractère protégé : il ne commence ni chaîne ni chemin.
            if let Some(next) = chars.get(index + 1) {
                out.push(ch);
                out.push(*next);
                index += 2;
                continue;
            }
        }
        out.push(ch);
        index += 1;
    }
    out
}

/// `cd /d chemin` à `index` : la position du chemin, s'il y en a bien un.
fn cd_drive_switch(chars: &[char], index: usize) -> Option<usize> {
    let blanks = |from: usize| {
        let count = chars[from..]
            .iter()
            .take_while(|ch| matches!(ch, ' ' | '\t'))
            .count();
        (count > 0).then_some(from + count)
    };
    if chars.get(index..index + 2)? != ['c', 'd'] {
        return None;
    }
    let switch = blanks(index + 2)?;
    if !matches!(chars.get(switch..switch + 2)?, ['/', 'd' | 'D']) {
        return None;
    }
    let path = blanks(switch + 2)?;
    chars
        .get(path)
        .is_some_and(|ch| !"\n;&|)".contains(*ch))
        .then_some(path)
}

fn is_drive_path(chars: &[char], index: usize) -> bool {
    matches!(
        chars.get(index..index + 3),
        Some([drive, ':', '\\']) if drive.is_ascii_alphabetic()
    )
}

/// Écrit avec des barres obliques le chemin `D:\…` qui commence à `index` et
/// renvoie la position qui le suit. `\ ` (espace protégé) et `\\` sont gardés.
fn push_drive_path(chars: &[char], mut index: usize, out: &mut String) -> usize {
    while let Some(ch) = chars
        .get(index)
        .copied()
        .filter(|ch| !(ch.is_whitespace() || ";&|<>()\"'".contains(*ch)))
    {
        index += 1;
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        match chars.get(index).copied() {
            Some(next) if next.is_whitespace() || next == '\\' => {
                out.push('\\');
                out.push(next);
                index += 1;
            }
            _ => out.push('/'),
        }
    }
    index
}

const WINDOWS_SHELL_HINT: &str = "[Claw] The `bash` tool runs Git Bash (POSIX syntax): outside quotes a backslash is an escape character, so `dir\\file` is read as `dirfile`. Write paths with forward slashes (`dir/file`); cmd syntax does not work here.";

/// Remarque jointe au résultat d'une commande en échec sous Windows quand elle
/// porte, hors guillemets, une barre oblique inverse devant une lettre ou un
/// chiffre : un chemin relatif écrit pour cmd (`dossier\fichier`).
pub(crate) fn windows_shell_hint(command: &str) -> Option<&'static str> {
    (cfg!(windows) && has_bare_backslash_path(&adapt_windows_command(command)))
        .then_some(WINDOWS_SHELL_HINT)
}

fn has_bare_backslash_path(command: &str) -> bool {
    if command.contains("<<") {
        return false;
    }
    let mut quote = None;
    let mut chars = command.chars();
    while let Some(ch) = chars.next() {
        if let Some(open) = quote {
            if ch == open {
                quote = None;
            } else if ch == '\\' && open == '"' {
                chars.next();
            }
        } else if ch == '"' || ch == '\'' {
            quote = Some(ch);
        } else if ch == '\\'
            && chars
                .next()
                .is_some_and(|next| next.is_ascii_alphanumeric())
        {
            return true;
        }
    }
    false
}

fn shell_group_pid_file() -> std::path::PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_nanos());
    env::temp_dir().join(format!("claw-bash-{}-{nanos}.pid", std::process::id()))
}

/// Préfixe la commande d'une ligne qui écrit le PID du shell dans `pid_file`.
fn with_group_pid_capture(command: &str, pid_file: &std::path::Path) -> String {
    let path = pid_file
        .to_string_lossy()
        .replace('\\', "/")
        .replace('\'', r"'\''");
    format!("printf '%s' \"$$\" > '{path}'\n{command}")
}

/// Tue tout le groupe de processus MSYS du shell (suivi par Cygwin même quand
/// la parenté Windows est rompue).
pub(crate) fn kill_shell_group(pid_file: &std::path::Path) {
    let Ok(pid) = std::fs::read_to_string(pid_file) else {
        return;
    };
    let pid = pid.trim();
    if pid.is_empty() || !pid.chars().all(|ch| ch.is_ascii_digit()) {
        return;
    }
    let _ = plugins::posix_shell_command(&format!(
        "kill -KILL -- -{pid} 2>/dev/null; kill -KILL {pid} 2>/dev/null; true"
    ))
    .stdin(Stdio::null())
    .stdout(Stdio::null())
    .stderr(Stdio::null())
    .status();
}

/// Tue la commande et tous ses descendants : sous Windows, tuer `bash.exe` ne
/// tue pas `sleep.exe` ou `cargo.exe` qu'il a lancés.
pub(crate) fn kill_process_tree(pid: Option<u32>) {
    let Some(pid) = pid else {
        return;
    };
    #[cfg(windows)]
    let mut killer = {
        let mut command = Command::new("taskkill");
        command.args(["/PID", &pid.to_string(), "/T", "/F"]);
        command
    };
    #[cfg(not(windows))]
    let mut killer = {
        let mut command = Command::new("kill");
        command.args(["-KILL", &format!("-{pid}")]);
        command
    };
    let _ = killer
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

fn sandbox_status_for_input(input: &BashCommandInput, cwd: &std::path::Path) -> SandboxStatus {
    let config = ConfigLoader::default_for(cwd).load().map_or_else(
        |_| SandboxConfig::default(),
        |runtime_config| runtime_config.sandbox().clone(),
    );
    let request = config.resolve_request(
        input.dangerously_disable_sandbox.map(|disabled| !disabled),
        input.namespace_restrictions,
        input.isolate_network,
        input.filesystem_mode,
        input.allowed_mounts.clone(),
    );
    let mut status = resolve_sandbox_status_for_request(&request, cwd);
    if input.dangerously_disable_sandbox != Some(true) {
        // Les options de la commande (isolateNetwork, allowedMounts) priment
        // sur la configuration, comme pour le bac à sable par namespaces.
        status.container = config.container().map(|mut container| {
            container.network_isolation = request.network_isolation;
            container.extra_mounts.clone_from(&status.allowed_mounts);
            container
        });
    }
    status
}

/// Conteneurs démarrés par ce processus et encore actifs (pour les arrêter).
static RUNNING_CONTAINERS: std::sync::Mutex<Vec<(String, crate::sandbox::ContainerSandbox)>> =
    std::sync::Mutex::new(Vec::new());
thread_local! {
    static LAST_CONTAINER: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
}

fn remember_container(name: &str, sandbox: &crate::sandbox::ContainerSandbox) {
    RUNNING_CONTAINERS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .push((name.to_string(), sandbox.clone()));
    LAST_CONTAINER.with(|last| *last.borrow_mut() = Some(name.to_string()));
}

/// Nom du dernier conteneur préparé sur ce fil (tâches de fond).
pub(crate) fn take_last_container() -> Option<String> {
    LAST_CONTAINER.with(|last| last.borrow_mut().take())
}

/// Arrête un conteneur démarré par Claw.
pub(crate) fn stop_container(name: &str) {
    let entry = {
        let mut running = RUNNING_CONTAINERS
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        running
            .iter()
            .position(|(running_name, _)| running_name == name)
            .map(|position| running.remove(position))
    };
    if let Some((name, sandbox)) = entry {
        sandbox.stop(&name);
    }
}

/// Arrête tous les conteneurs encore actifs (fin de Claw).
pub fn stop_all_containers() {
    let running = std::mem::take(
        &mut *RUNNING_CONTAINERS
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner),
    );
    for (name, sandbox) in running {
        sandbox.stop(&name);
    }
}

fn prepare_command(
    command: &str,
    cwd: &std::path::Path,
    sandbox_status: &SandboxStatus,
    create_dirs: bool,
) -> Command {
    if let Some(container) = &sandbox_status.container {
        let (args, name) = container.run_args(command, cwd);
        remember_container(&name, container);
        let mut prepared = Command::new(&container.program);
        prepared.args(args).current_dir(cwd);
        return prepared;
    }
    if create_dirs {
        prepare_sandbox_dirs(cwd);
    }

    if let Some(launcher) = build_linux_sandbox_command(command, cwd, sandbox_status) {
        let mut prepared = Command::new(launcher.program);
        prepared.args(launcher.args);
        prepared.current_dir(cwd);
        prepared.envs(launcher.env);
        return prepared;
    }

    let mut prepared = plugins::posix_shell_command(command);
    prepared.current_dir(cwd);
    if std::env::var_os("PYTHONIOENCODING").is_none() {
        prepared.env("PYTHONIOENCODING", "utf-8");
    }
    // Python écrit au fil de l'eau : la sortie d'un serveur ou d'un script
    // arrêté en route n'est pas perdue dans son tampon.
    if std::env::var_os("PYTHONUNBUFFERED").is_none() {
        prepared.env("PYTHONUNBUFFERED", "1");
    }
    if sandbox_status.filesystem_active {
        let (home, tmp) = crate::sandbox::sandbox_dirs(cwd);
        prepared.env("HOME", home);
        prepared.env("TMPDIR", tmp);
    }
    prepared
}

fn prepare_sandbox_dirs(cwd: &std::path::Path) {
    crate::sandbox::prepare_sandbox_dirs(cwd);
}

#[cfg(test)]
mod tests {
    use super::{execute_bash, BashCommandInput};
    use crate::sandbox::FilesystemIsolationMode;

    #[test]
    fn a_filter_at_the_end_of_a_pipe_does_not_hide_a_failure() {
        use super::{with_faithful_exit_codes, PIPEFAIL_GROUP};
        let drained = |before: &str, check: &str, filters: &str, after: &str| {
            Some(format!(
                "{before}{PIPEFAIL_GROUP}{check} {{ {filters}\ncat >/dev/null; }} ){after}"
            ))
        };
        let piped = |before: &str, pipeline: &str, after: &str| {
            Some(format!("{before}{PIPEFAIL_GROUP}{pipeline}\n){after}"))
        };
        // Une vérification derrière des filtres : elle finit toujours, et son
        // code de sortie est le sien.
        assert_eq!(
            with_faithful_exit_codes("cd frontend && npm run build 2>&1 | tail -20"),
            drained("cd frontend && ", "npm run build 2>&1 |", "tail -20", "")
        );
        assert_eq!(
            with_faithful_exit_codes("python -m pytest tests/ -v --tb=short 2>&1 | head -100"),
            drained(
                "",
                "python -m pytest tests/ -v --tb=short 2>&1 |",
                "head -100",
                ""
            )
        );
        assert_eq!(
            with_faithful_exit_codes(
                "cd backend && python -m pytest tests/ -v 2>&1 | grep -E \"(PASSED|FAILED)\" | head -5"
            ),
            drained(
                "cd backend && ",
                "python -m pytest tests/ -v 2>&1 |",
                "grep -E \"(PASSED|FAILED)\" | head -5",
                ""
            )
        );
        // Commentaire, `;` final, tube de stderr, sous-shell, substitution.
        assert_eq!(
            with_faithful_exit_codes("pytest -q | head -5 # le début"),
            drained("", "pytest -q |", "head -5 # le début", "")
        );
        assert_eq!(
            with_faithful_exit_codes("cargo test |& tail -3;"),
            drained("", "cargo test |&", "tail -3", ";")
        );
        assert_eq!(
            with_faithful_exit_codes("(cd frontend && npm run build) 2>&1 | tail -20"),
            drained("", "(cd frontend && npm run build) 2>&1 |", "tail -20", "")
        );
        assert_eq!(
            with_faithful_exit_codes("cd $(git rev-parse --show-toplevel) && cargo test | tail -5"),
            drained(
                "cd $(git rev-parse --show-toplevel) && ",
                "cargo test |",
                "tail -5",
                ""
            )
        );
        // Chaque tube a son pipefail : un filtre suivi de `&&` ne masque plus
        // l'échec, et `git log | head` n'arrête plus la suite.
        assert_eq!(
            with_faithful_exit_codes(
                "cd frontend && npm run build 2>&1 | tail -20 && echo \"Build OK\""
            ),
            drained(
                "cd frontend && ",
                "npm run build 2>&1 |",
                "tail -20",
                " && echo \"Build OK\""
            )
        );
        assert_eq!(
            with_faithful_exit_codes(
                "git log --oneline | head -5 && npm run build 2>&1 | head -40"
            ),
            drained(
                "git log --oneline | head -5 && ",
                "npm run build 2>&1 |",
                "head -40",
                ""
            )
        );
        // Un autre programme derrière un filtre qui lit tout : pipefail seul.
        assert_eq!(
            with_faithful_exit_codes("python script.py 2>&1 | tail -5"),
            piped("", "python script.py 2>&1 | tail -5", "")
        );
        assert_eq!(
            with_faithful_exit_codes("(echo a; exit 3) 2>&1 | tail -5"),
            piped("", "(echo a; exit 3) 2>&1 | tail -5", "")
        );
        // Laissé tel quel : le dernier programme décide (grep sans résultat,
        // comptage), il n'y a pas de tube, `head` peut couper un programme
        // qui ne finirait jamais, ou ce n'est pas une commande simple.
        for kept in [
            "grep -rn foo src | wc -l",
            "ls | grep test",
            "cat a.txt | python script.py",
            "npm run build",
            "npm run build || echo raté",
            "cat > a.txt <<EOF\nun | tail\nEOF",
            "yes | head -3",
            "python script.py | head -20",
            "npm run dev 2>&1 | tee dev.log &",
            "{ npm test | tail -5; }",
            "if npm test | tail -5; then echo ok; fi",
            "f() { npm test | tail -5; }",
        ] {
            assert_eq!(with_faithful_exit_codes(kept), None, "{kept}");
        }
    }

    #[test]
    fn a_rewritten_pipe_gives_the_exit_code_of_its_program() {
        let execute =
            |command: &str| execute_bash(foreground(command, Some(60_000))).expect("exécution");
        let run = |command: &str| execute(command).return_code_interpretation;
        // Le programme échoue derrière `tail` : son code de sortie est rendu.
        assert_eq!(
            run("(echo compilation; exit 3) 2>&1 | tail -5").as_deref(),
            Some("exit_code:3")
        );
        assert_eq!(run("echo tout va bien | tail -5"), None);
        assert_eq!(run("yes | head -3"), None);
        // Une vérification qui ignore SIGPIPE (comme Python ou un test Rust)
        // écrit bien plus que `head` n'en garde : elle finit, et réussit.
        let pytest = "pytest() { trap '' PIPE; for i in {1..8000}; do echo \"test_$i PASSED\" || return 1; done; }; pytest | head -5";
        assert_eq!(run(pytest), None);
        // La même en échec : son code de sortie est rendu, et `&&` ne lance
        // pas la suite.
        let failing = execute(
            "pytest() { echo \"test_1 FAILED\"; return 1; }; pytest | head -5 && echo \"Tests OK\"",
        );
        assert_eq!(
            failing.return_code_interpretation.as_deref(),
            Some("exit_code:1")
        );
        assert!(!failing.stdout.contains("Tests OK"), "{}", failing.stdout);
        // Un `head` plus haut dans la commande n'arrête plus la vérification.
        let later = execute("pytest() { echo verifie; }; yes | head -2 && pytest | head -5");
        assert_eq!(later.return_code_interpretation, None);
        assert!(later.stdout.contains("verifie"), "{}", later.stdout);
    }

    #[test]
    fn shell_segments_ignore_quotes_comments_and_redirections() {
        use super::shell_segments;
        let segments = |command: &str| {
            shell_segments(command)
                .into_iter()
                .map(|(segment, detached)| format!("{segment}{}", if detached { " &" } else { "" }))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            segments("cd app && npm run dev 2>&1 | tee log"),
            ["cd app", "npm run dev 2>&1", "tee log"]
        );
        assert_eq!(
            segments("echo \"a; b && c\" ; ls # x && y"),
            ["echo _", "ls"]
        );
        assert_eq!(
            segments("serve & sleep 1; find . -exec rm {} \\;"),
            ["serve &", "sleep 1", "find . -exec rm {} _"]
        );
    }

    #[test]
    fn executes_simple_command() {
        let output = execute_bash(BashCommandInput {
            command: String::from("printf 'hello'"),
            timeout: Some(1_000),
            description: None,
            run_in_background: Some(false),
            dangerously_disable_sandbox: Some(false),
            namespace_restrictions: Some(false),
            isolate_network: Some(false),
            filesystem_mode: Some(FilesystemIsolationMode::WorkspaceOnly),
            allowed_mounts: None,
        })
        .expect("la commande bash devrait s'exécuter");

        assert_eq!(output.stdout, "hello");
        assert!(!output.interrupted);
        assert!(output.sandbox_status.is_some());
    }

    #[test]
    fn a_timeout_written_in_seconds_is_read_in_seconds() {
        use super::{effective_timeout_ms, DEFAULT_TIMEOUT_MS};
        assert_eq!(effective_timeout_ms(None), DEFAULT_TIMEOUT_MS);
        assert_eq!(effective_timeout_ms(Some(0)), DEFAULT_TIMEOUT_MS);
        assert_eq!(effective_timeout_ms(Some(120)), 120_000);
        assert_eq!(effective_timeout_ms(Some(1_000)), 1_000);
        assert_eq!(effective_timeout_ms(Some(600_000)), 600_000);
    }

    #[test]
    fn servers_and_watchers_are_recognized_but_builds_are_not() {
        use super::starts_long_running;
        for command in [
            "npm run dev",
            "cd frontend && npm run dev",
            "PORT=3001 npm start",
            "uvicorn app.main:app --reload --port 8000",
            "python -m uvicorn app.main:app",
            "uv run fastapi dev main.py",
            "npx vite --port 5173",
            "flask run",
            "jest --watch",
            "tsc -w",
            "npm run dev 2>&1",
            "npm run dev 2>&1 | tee dev.log",
            "(cd web && npm run dev)",
            "nohup npm run dev",
            "npm install; npm run dev # serveur de dev",
        ] {
            assert!(starts_long_running(command), "{command}");
        }
        for command in [
            "npm run build",
            "npx vite build",
            "next build",
            "npm test",
            "cargo test",
            "python -m pytest",
            "grep -rn \"npm run dev\" README.md",
            "npm install && npm run lint",
            "uvicorn --help",
            "cd backend && pip install -r requirements.txt",
            // Texte qui parle d'un serveur sans le lancer.
            "git commit -m \"Front\nnpm run dev pour le voir\"",
            "echo \"Build ok; serve -s dist pour tester\"",
            "echo 'uvicorn app:app' > start.sh",
            "cat > README.md <<'EOF'\nnpm install\nnpm run dev\nEOF",
            "ls # npm run dev plus tard",
            // Serveur détaché par le shell lui-même : la commande se termine.
            "uvicorn app.main:app --port 8000 & PID=$!; sleep 3; curl -s localhost:8000/health; kill $PID",
            "python app.py &",
            "nohup npm run dev > dev.log 2>&1 &",
            // Essai borné voulu par le modèle.
            "timeout 10 npm run dev",
        ] {
            assert!(!starts_long_running(command), "{command}");
        }
    }

    /// Commande au premier plan, sans délai fixé ni avis sur l'arrière-plan.
    fn foreground(command: &str, timeout: Option<u64>) -> BashCommandInput {
        BashCommandInput {
            command: command.to_string(),
            timeout,
            description: None,
            run_in_background: None,
            dangerously_disable_sandbox: Some(true),
            namespace_restrictions: None,
            isolate_network: None,
            filesystem_mode: None,
            allowed_mounts: None,
        }
    }

    #[test]
    fn a_process_left_running_with_an_ampersand_continues_in_the_background() {
        let started = std::time::Instant::now();
        let output = execute_bash(foreground("echo début; sleep 30 &", None)).expect("exécution");
        assert!(
            started.elapsed() < std::time::Duration::from_secs(15),
            "{:?}",
            started.elapsed()
        );
        assert!(output.stdout.starts_with("début"), "{output:?}");
        assert_eq!(output.return_code_interpretation, None, "{output:?}");
        let task_id = output.background_task_id.expect("tâche de fond");
        assert!(output.stdout.contains(&task_id));
        let killed = crate::bash_tasks::kill(&task_id).expect("arrêt");
        assert!(!killed.running, "{killed:?}");
    }

    #[test]
    fn a_command_still_running_after_the_patience_continues_in_the_background() {
        let launch = |command: &str| {
            crate::bash_tasks::launch(
                plugins::posix_shell_command(command),
                command,
                None,
                None,
                true,
            )
            .expect("lancement")
        };
        let waits = super::Waits {
            patience: std::time::Duration::from_secs(1),
            ready_idle: std::time::Duration::from_secs(60),
        };
        // Rien n'est arrêté : la suite se lit avec BashOutput.
        let output =
            super::run_in_foreground(launch("echo avant; sleep 4; echo après"), None, waits);
        assert!(
            output.stdout.contains("Still running after 1 s"),
            "{output:?}"
        );
        assert!(!output.stdout.contains("après"), "{output:?}");
        let task_id = output.background_task_id.expect("tâche de fond");
        let rest = crate::bash_tasks::wait_for(&task_id, std::time::Duration::from_secs(60))
            .expect("attente");
        assert!(!rest.running, "{rest:?}");
        assert_eq!(rest.exit_code, Some(0));
        // Chaque ligne est rendue une fois, avant ou après le passage en
        // arrière-plan (selon le temps que le shell met à démarrer) : rien
        // n'est perdu ni répété.
        let whole = format!("{}\n{}", output.stdout, rest.new_output);
        assert_eq!(whole.matches("avant").count(), 1, "{whole}");
        assert_eq!(whole.matches("après").count(), 1, "{whole}");
        assert!(rest.new_output.contains("après"), "{rest:?}");

        // Un serveur qui s'annonce prêt puis attend : en arrière-plan sans
        // attendre la patience.
        let waits = super::Waits {
            patience: std::time::Duration::from_secs(60),
            ready_idle: std::time::Duration::from_millis(500),
        };
        let started = std::time::Instant::now();
        let output = super::run_in_foreground(
            launch("echo 'Server listening on http://localhost:3999'; sleep 30"),
            None,
            waits,
        );
        assert!(
            started.elapsed() < std::time::Duration::from_secs(20),
            "{:?}",
            started.elapsed()
        );
        assert!(
            output.stdout.contains("announced a server or a watcher"),
            "{output:?}"
        );
        let task_id = output.background_task_id.expect("tâche de fond");
        assert!(!crate::bash_tasks::kill(&task_id).expect("arrêt").running);
    }

    #[test]
    fn a_timeout_set_by_the_model_stops_the_command_and_keeps_its_output() {
        let started = std::time::Instant::now();
        // Délai large devant le démarrage du shell, même sur une machine chargée.
        let output =
            execute_bash(foreground("echo partiel; sleep 60", Some(5_000))).expect("exécution");
        assert!(
            started.elapsed() < std::time::Duration::from_secs(40),
            "{:?}",
            started.elapsed()
        );
        assert!(output.interrupted);
        assert_eq!(
            output.return_code_interpretation.as_deref(),
            Some("timeout")
        );
        assert!(output.stdout.contains("partiel"), "{output:?}");
        assert!(
            output.stderr.contains("a dépassé le délai de 5 s"),
            "{output:?}"
        );
        assert!(output.background_task_id.is_none());
    }

    #[test]
    fn a_large_output_does_not_block_the_command() {
        // Plus que ce que tient un tube : la sortie est lue pendant que la
        // commande tourne.
        let output = execute_bash(foreground(
            "i=0; while [ $i -lt 4000 ]; do echo \"ligne $i de remplissage pour le tube\"; i=$((i+1)); done; echo fin >&2",
            Some(120_000),
        ))
        .expect("exécution");
        assert_eq!(output.return_code_interpretation, None, "{}", output.stderr);
        assert_eq!(output.stdout.lines().count(), 4_000);
        assert_eq!(output.stderr.trim(), "fin");
    }

    #[test]
    fn disables_sandbox_when_requested() {
        let output = execute_bash(BashCommandInput {
            command: String::from("printf 'hello'"),
            timeout: Some(1_000),
            description: None,
            run_in_background: Some(false),
            dangerously_disable_sandbox: Some(true),
            namespace_restrictions: None,
            isolate_network: None,
            filesystem_mode: None,
            allowed_mounts: None,
        })
        .expect("la commande bash devrait s'exécuter");

        assert!(!output.sandbox_status.expect("statut du sandbox").enabled);
    }
}

#[cfg(test)]
mod interrupt_tests {
    #[test]
    #[ignore = "modifie l'indicateur global d'interruption : à lancer seul (cargo test -p runtime -- --ignored interrupt_kills)"]
    fn interrupt_kills_long_command() {
        crate::interrupt::clear_interrupt();
        std::thread::spawn(|| {
            std::thread::sleep(std::time::Duration::from_millis(500));
            crate::interrupt::request_interrupt();
            eprintln!("interruption demandée");
        });
        let started = std::time::Instant::now();
        let output = super::execute_bash(super::BashCommandInput {
            command: "sleep 5".to_string(),
            timeout: None,
            description: None,
            run_in_background: None,
            dangerously_disable_sandbox: None,
            namespace_restrictions: None,
            isolate_network: None,
            filesystem_mode: None,
            allowed_mounts: None,
        });
        eprintln!(
            "durée {:?} -> {:?}",
            started.elapsed(),
            output.map(|o| (o.interrupted, o.stderr))
        );
        crate::interrupt::clear_interrupt();
    }
}

#[cfg(test)]
mod group_capture_tests {
    #[test]
    fn prefixes_the_command_with_a_quoted_pid_capture() {
        let script = super::with_group_pid_capture(
            "cargo test",
            std::path::Path::new(r"C:\Users\O'Brien\Temp\claw.pid"),
        );
        assert_eq!(
            script,
            concat!(
                r#"printf '%s' "$$" > 'C:/Users/O'\''Brien/Temp/claw.pid'"#,
                "\ncargo test"
            )
        );
    }
}

#[cfg(test)]
mod windows_command_tests {
    use super::{adapt_windows_command, has_bare_backslash_path};

    #[test]
    fn drive_paths_get_forward_slashes_outside_quotes() {
        // La commande d'un modèle sous Windows : bash lisait `D:Mes_Projets…`.
        assert_eq!(
            adapt_windows_command(
                r#"cd D:\Mes_Projets\test_claw\backend && python -c "from app.main import app; print('OK')""#
            ),
            r#"cd D:/Mes_Projets/test_claw/backend && python -c "from app.main import app; print('OK')""#
        );
        assert_eq!(
            adapt_windows_command(r#"grep "a\.b" C:\src\x.txt > C:\out\r.txt"#),
            r#"grep "a\.b" C:/src/x.txt > C:/out/r.txt"#
        );
        assert_eq!(
            adapt_windows_command(r"OUT=C:\tools\bin ./run; ls (D:\a\b)"),
            "OUT=C:/tools/bin ./run; ls (D:/a/b)"
        );
        // Espace protégé gardé, séparateurs convertis.
        assert_eq!(
            adapt_windows_command(r"cat D:\Mes\ Docs\note.txt"),
            r"cat D:/Mes\ Docs/note.txt"
        );
    }

    #[test]
    fn cd_with_the_cmd_drive_switch_loses_it() {
        assert_eq!(
            adapt_windows_command(r"cd /d D:\Mes_Projets\test_claw\backend && python x.py"),
            "cd D:/Mes_Projets/test_claw/backend && python x.py"
        );
        assert_eq!(
            adapt_windows_command("ls && cd /D backend"),
            "ls && cd backend"
        );
        // `/d` seul est le lecteur D: de Git Bash.
        for kept in ["cd /d", "cd /d && ls", "cd /dev", "abcd /d x"] {
            assert_eq!(adapt_windows_command(kept), kept);
        }
    }

    #[test]
    fn quoted_text_escapes_and_here_documents_are_left_alone() {
        for kept in [
            r#"type "C:\Program Files\x\y.txt""#,
            r"echo 'D:\a\b' 'cd /d C:\x'",
            r#"echo "il dit \"cd /d C:\x\" ici""#,
            r"printf 'a\nb'; ls foo\ bar; grep foo\|bar x",
            r"python C:\\src\\run.py",
            r"echo \C:\a",
            "cat > note.txt <<EOF\nChemin : D:\\a\\b\nEOF",
            "curl http://hote/c:/x",
        ] {
            assert_eq!(adapt_windows_command(kept), kept);
        }
    }

    #[test]
    fn relative_backslash_paths_are_spotted_for_the_hint() {
        assert!(has_bare_backslash_path(r"python backend\app\main.py"));
        assert!(has_bare_backslash_path(r"cd backend && type app\main.py"));
        for fine in [
            r#"python "backend\app\main.py""#,
            r"echo 'a\b'",
            r"grep foo\|bar x; find . -name \*.py; ls foo\ bar",
            "python backend/app/main.py",
            "cat <<EOF\na\\b\nEOF",
        ] {
            assert!(!has_bare_backslash_path(fine), "{fine}");
        }
    }

    #[cfg(windows)]
    #[test]
    fn a_windows_path_in_a_command_runs_in_git_bash() {
        use super::{execute_bash, BashCommandInput};
        let dir = std::env::temp_dir().join(format!("claw-chemin-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("sous dossier")).expect("dossier d'essai");
        let windows_path = dir.to_string_lossy().replace('/', "\\");
        let run = |command: String| {
            execute_bash(BashCommandInput {
                command,
                timeout: Some(20_000),
                description: None,
                run_in_background: Some(false),
                dangerously_disable_sandbox: Some(true),
                namespace_restrictions: None,
                isolate_network: None,
                filesystem_mode: None,
                allowed_mounts: None,
            })
            .expect("la commande devrait se lancer")
        };
        let plain = run(format!("cd {windows_path} && ls"));
        assert_eq!(plain.stdout.trim(), "sous dossier", "{}", plain.stderr);
        let with_switch = run(format!("cd /d {windows_path} && ls"));
        assert_eq!(
            with_switch.stdout.trim(),
            "sous dossier",
            "{}",
            with_switch.stderr
        );
        std::fs::remove_dir_all(&dir).expect("nettoyage");
    }
}
