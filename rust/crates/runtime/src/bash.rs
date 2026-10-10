use std::env;
use std::io;
use std::process::{Command, Stdio};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::process::Command as TokioCommand;
use tokio::runtime::Builder;
use tokio::time::timeout;

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
    // au premier plan, il bloquerait le tour. Il part d'office en arrière-plan.
    let auto_background =
        !input.run_in_background.unwrap_or(false) && starts_long_running(&input.command);

    if input.run_in_background.unwrap_or(false) || auto_background {
        // Sous Windows, le PID du shell MSYS permet d'arrêter tout le groupe.
        let pid_file = cfg!(windows).then(shell_group_pid_file);
        let wrapped = pid_file.as_deref().map_or_else(
            || input.command.clone(),
            |path| with_group_pid_capture(&input.command, path),
        );
        #[allow(unused_mut)]
        let mut command = prepare_command(&wrapped, &cwd, &sandbox_status, false);
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
        let container = take_last_container();
        let task_id = crate::bash_tasks::spawn(command, &input.command, pid_file)?;
        if let Some(name) = container {
            crate::bash_tasks::set_container(&task_id, name);
        }

        let started = if auto_background {
            format!(
                "[Claw] This command starts a server or a watcher, which never exits on its own: it was started in the background as {task_id} instead of blocking. Read its output with BashOutput (bash_id: \"{task_id}\"), for example to check that it is listening, and stop it with KillShell."
            )
        } else {
            format!(
                "Started in the background as {task_id}. Read its output with BashOutput (bash_id: \"{task_id}\") and stop it with KillShell."
            )
        };
        return Ok(BashCommandOutput {
            stdout: started,
            stderr: String::new(),
            raw_output_path: None,
            interrupted: false,
            is_image: None,
            background_task_id: Some(task_id),
            backgrounded_by_user: Some(false),
            assistant_auto_backgrounded: Some(auto_background),
            dangerously_disable_sandbox: input.dangerously_disable_sandbox,
            return_code_interpretation: None,
            no_output_expected: Some(true),
            structured_content: None,
            persisted_output_path: None,
            persisted_output_size: None,
            sandbox_status: Some(sandbox_status),
        });
    }

    let runtime = Builder::new_current_thread().enable_all().build()?;
    let result = runtime.block_on(execute_bash_async(input, sandbox_status, cwd));
    // Ne pas attendre les lectures de tubes encore ouvertes par un processus
    // abandonné (délai dépassé ou interruption).
    runtime.shutdown_background();
    result
}

/// Délai d'une commande au premier plan quand le modèle n'en donne pas.
pub const DEFAULT_TIMEOUT_MS: u64 = 5 * 60 * 1000;

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

/// La commande démarre un serveur ou une surveillance (ou se termine par un
/// `&` qui la détache tout en gardant sa sortie ouverte) : elle ne finira pas.
fn starts_long_running(command: &str) -> bool {
    let lower = command.to_ascii_lowercase();
    let trimmed = lower.trim_end();
    if trimmed.ends_with('&') && !trimmed.ends_with("&&") {
        return true;
    }
    lower
        .split([';', '|', '&', '\n'])
        .map(str::trim)
        .filter(|segment| !segment.is_empty())
        .any(|segment| {
            let mut words: Vec<&str> = segment
                .split_whitespace()
                .skip_while(|word| word.contains('=') && !word.starts_with('-'))
                .collect();
            let skip = match words.as_slice() {
                ["npx" | "bunx", ..] => 1,
                ["pnpm", "exec", ..]
                | ["poetry" | "uv" | "pipenv" | "pdm" | "hatch", "run", ..] => 2,
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
            if words.contains(&"--watch")
                || (words.first() == Some(&"tsc") && words.contains(&"-w"))
            {
                return true;
            }
            let joined = words.join(" ");
            LONG_RUNNING.iter().any(|pattern| {
                joined == *pattern
                    || joined
                        .strip_prefix(pattern)
                        .is_some_and(|rest| rest.starts_with(' '))
            })
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

#[allow(clippy::too_many_lines)]
async fn execute_bash_async(
    input: BashCommandInput,
    sandbox_status: SandboxStatus,
    cwd: std::path::PathBuf,
) -> io::Result<BashCommandOutput> {
    // Sous Windows, le bash de Git (MSYS) peut détacher ses sous-processus de
    // l'arbre Windows (fork + exec) : il note son PID MSYS pour qu'on puisse
    // tuer tout son groupe de processus en cas d'interruption.
    let group_pid_file = cfg!(windows).then(shell_group_pid_file);
    let command_text = group_pid_file.as_deref().map_or_else(
        || input.command.clone(),
        |pid_file| with_group_pid_capture(&input.command, pid_file),
    );
    let mut command = prepare_tokio_command(&command_text, &cwd, &sandbox_status, true);
    let container_name = take_last_container();
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    // Sous Unix, un groupe de processus dédié permet de tuer aussi les descendants.
    #[cfg(unix)]
    command.process_group(0);
    let timeout_ms = input.timeout.unwrap_or(DEFAULT_TIMEOUT_MS);
    let child = command.spawn()?;
    let pid = child.id();

    let output_result = {
        let run = timeout(Duration::from_millis(timeout_ms), child.wait_with_output());
        let outcome = tokio::select! {
            outcome = run => Some(outcome),
            () = crate::interrupt::wait_for_interrupt() => None,
        };
        if !matches!(outcome, Some(Ok(_))) {
            if let Some(pid_file) = &group_pid_file {
                kill_shell_group(pid_file);
            }
            kill_process_tree(pid);
        }
        if let Some(name) = &container_name {
            stop_container(name);
        }
        if let Some(pid_file) = &group_pid_file {
            let _ = std::fs::remove_file(pid_file);
        }
        match outcome {
            None => {
                return Ok(BashCommandOutput {
                    stdout: String::new(),
                    stderr: "Commande interrompue par l'utilisateur".to_string(),
                    raw_output_path: None,
                    interrupted: true,
                    is_image: None,
                    background_task_id: None,
                    backgrounded_by_user: None,
                    assistant_auto_backgrounded: None,
                    dangerously_disable_sandbox: input.dangerously_disable_sandbox,
                    return_code_interpretation: Some(String::from("interrupted")),
                    no_output_expected: Some(true),
                    structured_content: None,
                    persisted_output_path: None,
                    persisted_output_size: None,
                    sandbox_status: Some(sandbox_status),
                });
            }
            Some(Ok(result)) => (result?, false),
            Some(Err(_)) => {
                return Ok(BashCommandOutput {
                    stdout: String::new(),
                    stderr: format!(
                        "La commande a dépassé le délai de {} s : elle a été arrêtée.\n[Claw] The command did not finish within {} s and was stopped. If it starts a server or a watcher, run it with run_in_background: true and follow it with BashOutput. If it is a long build or test run, run it again with a larger timeout (in milliseconds).",
                        timeout_ms / 1000,
                        timeout_ms / 1000
                    ),
                    raw_output_path: None,
                    interrupted: true,
                    is_image: None,
                    background_task_id: None,
                    backgrounded_by_user: None,
                    assistant_auto_backgrounded: None,
                    dangerously_disable_sandbox: input.dangerously_disable_sandbox,
                    return_code_interpretation: Some(String::from("timeout")),
                    no_output_expected: Some(true),
                    structured_content: None,
                    persisted_output_path: None,
                    persisted_output_size: None,
                    sandbox_status: Some(sandbox_status),
                });
            }
        }
    };

    let (output, interrupted) = output_result;
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    let no_output_expected = Some(stdout.trim().is_empty() && stderr.trim().is_empty());
    let return_code_interpretation = output.status.code().and_then(|code| {
        if code == 0 {
            None
        } else {
            Some(format!("exit_code:{code}"))
        }
    });

    Ok(BashCommandOutput {
        stdout,
        stderr,
        raw_output_path: None,
        interrupted,
        is_image: None,
        background_task_id: None,
        backgrounded_by_user: None,
        assistant_auto_backgrounded: None,
        dangerously_disable_sandbox: input.dangerously_disable_sandbox,
        return_code_interpretation,
        no_output_expected,
        structured_content: None,
        persisted_output_path: None,
        persisted_output_size: None,
        sandbox_status: Some(sandbox_status),
    })
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
    if sandbox_status.filesystem_active {
        let (home, tmp) = crate::sandbox::sandbox_dirs(cwd);
        prepared.env("HOME", home);
        prepared.env("TMPDIR", tmp);
    }
    prepared
}

fn prepare_tokio_command(
    command: &str,
    cwd: &std::path::Path,
    sandbox_status: &SandboxStatus,
    create_dirs: bool,
) -> TokioCommand {
    if let Some(container) = &sandbox_status.container {
        let (args, name) = container.run_args(command, cwd);
        remember_container(&name, container);
        let mut prepared = TokioCommand::new(&container.program);
        prepared.args(args).current_dir(cwd);
        return prepared;
    }
    if create_dirs {
        prepare_sandbox_dirs(cwd);
    }

    if let Some(launcher) = build_linux_sandbox_command(command, cwd, sandbox_status) {
        let mut prepared = TokioCommand::new(launcher.program);
        prepared.args(launcher.args);
        prepared.current_dir(cwd);
        prepared.envs(launcher.env);
        return prepared;
    }

    let mut prepared = TokioCommand::from(plugins::posix_shell_command(command));
    prepared.current_dir(cwd);
    if std::env::var_os("PYTHONIOENCODING").is_none() {
        prepared.env("PYTHONIOENCODING", "utf-8");
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
            "python app.py &",
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
        ] {
            assert!(!starts_long_running(command), "{command}");
        }
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
