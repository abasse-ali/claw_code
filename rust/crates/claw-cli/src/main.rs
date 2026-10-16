mod acp;
mod agent_view;
mod approval;
mod auto_mode;
mod bench;
mod bench_hard;
mod clipboard;
mod credits;
mod doctor;
mod extra_commands;
mod git_work;
mod github_setup;
mod image_reader;
mod init;
mod input;
mod jobs;
mod keybindings;
mod likeness;
mod live;
mod mcp_cli;
mod model_switch;
mod models;
mod questions;
mod render;
mod routines;
mod statusline;
mod stream_json;
mod transcript;
mod turn_keys;
mod user_env;
mod web;

use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fmt::Write as _;
use std::fs;
use std::io::{self, IsTerminal, Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use api::{
    detect_provider_kind, max_tokens_for_model, resolve_startup_auth_source, AuthSource,
    ClawApiClient, ContentBlockDelta, InputContentBlock, InputMessage, MessageRequest,
    MessageResponse, OutputContentBlock, ProviderClient, ProviderKind,
    StreamEvent as ApiStreamEvent, ToolChoice, ToolDefinition, ToolResultContentBlock,
};

use commands::{
    handle_agents_slash_command, handle_plugins_slash_command, handle_skills_slash_command,
    render_slash_command_help, resume_supported_slash_commands, slash_command_specs,
    suggest_slash_commands, ExpertCommand, SlashCommand,
};
use compat_harness::{extract_manifest, UpstreamPaths};
use init::initialize_repo;
use plugins::{PluginManager, PluginManagerConfig};
use render::{MarkdownStreamState, TerminalRenderer};
use runtime::{
    clear_oauth_credentials, generate_pkce_pair, generate_state, load_system_prompt,
    parse_oauth_callback_request_target, save_oauth_credentials, ApiClient, ApiRequest,
    AssistantEvent, CompactionConfig, ConfigLoader, ConfigSource, ContentBlock,
    ConversationMessage, ConversationRuntime, MessageRole, OAuthAuthorizationRequest, OAuthConfig,
    OAuthTokenExchangeRequest, PermissionMode, PermissionPolicy, PermissionRules, ProjectContext,
    ResolvedPermissionMode, RuntimeError, Session, TokenUsage, ToolError, ToolExecutor,
    UsageTracker,
};
use serde_json::json;
use tools::input_check as tool_input;
use tools::GlobalToolRegistry;

const DEFAULT_MODEL: &str = "claude-opus-4-6";

/// Modèle de départ : `CLAW_MODEL`, puis la clé `model` des fichiers de config
/// (`.claw.json`, `.claw/settings.json`…). Sinon, avec seulement une clé
/// `OpenRouter`, le meilleur modèle gratuit pour le code ; à défaut `DEFAULT_MODEL`.
fn default_model() -> String {
    if let Some(model) = env::var("CLAW_MODEL")
        .ok()
        .filter(|value| !value.trim().is_empty())
    {
        return resolve_model_alias(model.trim());
    }
    // Les tests ne dépendent ni de la config de l'utilisateur ni de ses clés.
    if cfg!(test) {
        return DEFAULT_MODEL.to_string();
    }
    if let Some(model) = env::current_dir()
        .ok()
        .and_then(|cwd| ConfigLoader::default_for(&cwd).load().ok())
        .and_then(|config| config.model().map(resolve_model_alias))
    {
        return model;
    }
    let has_openrouter_key = env::var("OPENROUTER_API_KEY").is_ok_and(|key| !key.trim().is_empty());
    let has_anthropic = api::has_anthropic_credentials();
    if has_openrouter_key && !has_anthropic {
        return models::best_free_coding_model()
            .unwrap_or_else(|| models::FREE_ROUTER_MODEL.to_string());
    }
    // Aucune clé : un modèle d'un serveur local en marche (Ollama, LM Studio…).
    if !has_anthropic {
        if let Some(model) = models::first_local_model() {
            return model;
        }
    }
    DEFAULT_MODEL.to_string()
}
const DEFAULT_DATE: &str = match option_env!("BUILD_DATE") {
    Some(date) => date,
    None => "inconnue",
};
const DEFAULT_OAUTH_CALLBACK_PORT: u16 = 4545;
const VERSION: &str = env!("CARGO_PKG_VERSION");
const BUILD_TARGET: Option<&str> = option_env!("TARGET");
const GIT_SHA: Option<&str> = option_env!("GIT_SHA");
const INTERNAL_PROGRESS_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(3);

type AllowedToolSet = BTreeSet<String>;

fn main() {
    ensure_home_env();
    // Clés définies dans le compte mais absentes de l'environnement reçu.
    user_env::adopt();
    apply_settings_env();
    runtime::set_classify_all_shell(auto_mode::rules().classify_all_shell);
    if let Some(theme) = env::current_dir()
        .ok()
        .and_then(|cwd| ConfigLoader::default_for(cwd).load().ok())
        .and_then(|config| {
            config
                .get("theme")
                .and_then(|value| value.as_str().map(str::to_string))
        })
    {
        render::set_theme(&theme);
    }
    runtime::telemetry::install(
        VERSION,
        Box::new(tools::http_post_json),
        Box::new(|model, usage| models::estimated_cost_usd(model, *usage)),
    );
    let result = run();
    // Ce que la télémétrie n'a pas encore envoyé part maintenant.
    runtime::telemetry::shutdown();
    // Les serveurs de dev et autres commandes de fond ne survivent pas à Claw.
    runtime::bash_tasks::kill_all();
    runtime::stop_all_containers();
    tools::shutdown_mcp();
    if let Err(error) = result {
        eprintln!("{}", render_cli_error(&error.to_string()));
        std::process::exit(1);
    }
}

/// `claw agents …` : options de la vue des agents.
fn parse_agent_view_args(
    args: &[String],
    model: Option<String>,
    permission_mode: PermissionMode,
) -> Result<CliAction, String> {
    let mut json = false;
    let mut all = false;
    let mut cwd = None;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--json" => json = true,
            "--all" => all = true,
            "--cwd" => {
                let value = args
                    .get(index + 1)
                    .ok_or_else(|| "valeur manquante pour --cwd".to_string())?;
                cwd = Some(PathBuf::from(value));
                index += 1;
            }
            flag if flag.starts_with("--cwd=") => cwd = Some(PathBuf::from(&flag[6..])),
            other => return Err(format!("option inconnue pour claw agents : {other}")),
        }
        index += 1;
    }
    let cwd = cwd.map(|path| {
        if path.is_absolute() {
            path
        } else {
            env::current_dir().map_or(path.clone(), |dir| dir.join(&path))
        }
    });
    Ok(CliAction::AgentView {
        json,
        all,
        cwd,
        model,
        permission_mode,
    })
}

fn job_target(rest: &[String], command: &str) -> Result<String, String> {
    rest.get(1)
        .cloned()
        .ok_or_else(|| format!("usage : claw {command} <identifiant ou nom de la session>"))
}

/// Texte de la dernière réponse du modèle dans une conversation.
fn last_assistant_of(session: &Session) -> String {
    session
        .messages
        .iter()
        .rev()
        .filter(|message| message.role == runtime::MessageRole::Assistant)
        .find_map(|message| {
            let text = message
                .blocks
                .iter()
                .filter_map(|block| match block {
                    ContentBlock::Text { text } => Some(text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n");
            (!text.trim().is_empty()).then_some(text)
        })
        .unwrap_or_default()
}

/// Première ligne utile d'une réponse, pour la ligne de la vue des agents.
fn summary_line(text: &str) -> String {
    text.lines()
        .map(|line| {
            line.trim()
                .trim_start_matches(['#', '*', '-', '>', ' '])
                .trim()
        })
        .find(|line| !line.is_empty())
        .map(|line| line.chars().take(200).collect())
        .unwrap_or_default()
}

/// Dernière ligne d'une réponse qui se termine par une question.
fn trailing_question(text: &str) -> Option<String> {
    let last = text.lines().map(str::trim).rfind(|line| !line.is_empty())?;
    last.trim_end_matches(['*', '_', ' '])
        .ends_with('?')
        .then(|| last.chars().take(300).collect())
}

/// Processus d'une session en arrière-plan : il traite la première demande,
/// puis chaque réponse déposée dans sa boîte, jusqu'à être libéré.
#[allow(clippy::too_many_lines)]
fn run_job_worker(id: &str) -> Result<(), Box<dyn std::error::Error>> {
    let job = jobs::load(id).ok_or("session en arrière-plan inconnue")?;
    let _heartbeat = jobs::Heartbeat::start(id);
    env::set_current_dir(&job.cwd)?;
    jobs::update_state(id, |state| state.pid = Some(std::process::id()));
    if let Some(command) = &job.command {
        run_exec_job(id, command);
        return Ok(());
    }
    let mode = parse_permission_mode_arg(&job.permission_mode)
        .unwrap_or_else(|_| default_permission_mode());
    let mut cli = LiveCli::new(job.model.clone(), true, None, mode)?;
    let initial = jobs::load_state(id);
    let existing = initial
        .session_path
        .clone()
        .or_else(|| job.session_path.clone())
        .filter(|path| Path::new(path).is_file());
    if let Some(path) = existing {
        let fresh = cli.session.path.clone();
        cli.load_session_quietly(Path::new(&path))?;
        if fresh != cli.session.path {
            let _ = fs::remove_file(fresh);
        }
    } else if job.worktree.is_some() {
        // Conversation rangée avec celles du dossier d'origine : elle reste
        // après la suppression du worktree (`claw --resume`).
        let sessions = Path::new(&job.origin).join(".claw").join("sessions");
        fs::create_dir_all(&sessions)?;
        if let Some(name) = cli.session.path.file_name() {
            let target = sessions.join(name);
            let _ = fs::remove_file(&cli.session.path);
            cli.session.path = target;
            cli.persist_session()?;
            session_bound(&cli.session.path);
        }
    }
    let session_path = cli.session.path.display().to_string();
    jobs::update_state(id, |state| state.session_path = Some(session_path.clone()));
    tools::set_question_asker(Some(jobs::question_asker(id)));
    runtime::telemetry::session_started("background");
    cli.session_hook_start("startup");
    let mut next = if initial.turns == 0 && !job.prompt.trim().is_empty() {
        Some(match &job.worktree {
            Some(tree) => format!("{}\n\n{}", job.prompt, jobs::isolation_note(tree)),
            None => job.prompt.clone(),
        })
    } else {
        jobs::take_message(id)
    };
    // Boucle au rythme choisi en cours : (demande, réveil de secours).
    let mut self_paced: Option<(String, bool)> = None;
    loop {
        let Some(prompt) = next.take() else {
            match jobs::wait_next_prompt(id) {
                Some(jobs::NextPrompt::Message(message)) => {
                    next = Some(message);
                    continue;
                }
                Some(jobs::NextPrompt::Task(task)) => {
                    println!("\n↻ Tâche planifiée {}", task.id);
                    if task.self_paced {
                        self_paced = Some((task.prompt.clone(), task.fallback));
                        let _ = runtime::cron::take_stop_requested();
                    }
                    next = Some(scheduled_prompt(&task));
                    continue;
                }
                None => break,
            }
        };
        let headline = summary_line(&prompt);
        jobs::update_state(id, |state| {
            state.state = jobs::WORKING.to_string();
            state.waiting_for = None;
            state.question = None;
            state.finished_at = None;
            state.detail = headline;
        });
        println!("\n> {}\n", prompt.trim());
        let result = cli.run_job_turn(&prompt, id);
        let answer = last_assistant_of(cli.runtime.session());
        let error = result.as_ref().err().map(ToString::to_string);
        jobs::update_state(id, |state| {
            state.turns += 1;
            state.finished_at = Some(jobs::now_ms());
            state.waiting_for = None;
            state.question = None;
            match &error {
                None => {
                    state.state = jobs::DONE.to_string();
                    state.detail = summary_line(&answer);
                    // Une réponse qui finit par une question attend l'utilisateur.
                    if let Some(question) = trailing_question(&answer) {
                        state.waiting_for = Some("input needed".to_string());
                        state.question = Some(question);
                    }
                }
                Some(error) => {
                    state.state = jobs::FAILED.to_string();
                    state.detail = summary_line(error);
                }
            }
        });
        if let Some(error) = error {
            eprintln!("{}", render_cli_error(&error));
        }
        if let Some((request, fallback)) = self_paced.take() {
            after_self_paced_pass(&request, fallback);
        }
        next = jobs::take_message(id);
    }
    cli.session_hook_end("exit");
    jobs::update_state(id, |state| state.pid = None);
    Ok(())
}

/// `claw --bg --exec 'commande'` : une commande shell en arrière-plan.
fn run_exec_job(id: &str, command: &str) {
    jobs::update_state(id, |state| {
        state.state = jobs::WORKING.to_string();
        state.detail = format!("$ {command}");
    });
    println!("$ {command}");
    let status = plugins::posix_shell_command(command)
        .stdin(std::process::Stdio::null())
        .status();
    let tail = transcript::strip_ansi(&jobs::log_tail(id, 4 * 1024));
    let last = tail
        .lines()
        .map(str::trim)
        .rfind(|line| !line.is_empty())
        .unwrap_or_default()
        .to_string();
    jobs::update_state(id, |state| {
        state.finished_at = Some(jobs::now_ms());
        state.pid = None;
        match status {
            Ok(status) if status.success() => {
                state.state = jobs::DONE.to_string();
                state.detail = if last.is_empty() {
                    "terminée (code 0)".to_string()
                } else {
                    last
                };
            }
            Ok(status) => {
                state.state = jobs::FAILED.to_string();
                state.detail = format!("code {} · {last}", status.code().unwrap_or(-1));
            }
            Err(error) => {
                state.state = jobs::FAILED.to_string();
                state.detail = format!("lancement impossible : {error}");
            }
        }
    });
}

enum Follow {
    /// L'utilisateur a rendu la main (Échap) : la session continue.
    Detached,
    /// La session ne travaille plus.
    Idle,
}

/// Nouvelles sorties de la session depuis `offset`.
fn read_new_output(id: &str, offset: &mut u64) -> String {
    use std::io::{Read as _, Seek as _};
    let Ok(mut file) = fs::File::open(jobs::log_path(id)) else {
        return String::new();
    };
    let length = file.metadata().map_or(0, |metadata| metadata.len());
    if length < *offset {
        *offset = 0;
    }
    if file.seek(io::SeekFrom::Start(*offset)).is_err() {
        return String::new();
    }
    let mut buffer = Vec::new();
    let _ = file.read_to_end(&mut buffer);
    *offset += u64::try_from(buffer.len()).unwrap_or(0);
    String::from_utf8_lossy(&buffer).into_owned()
}

/// Suit les sorties d'une session au travail. Une question posée en plein
/// tour (permission, `AskUserQuestion`) se répond ici ; Échap, ← ou Ctrl+C
/// rendent la main sans arrêter la session.
fn follow_job(id: &str, offset: &mut u64) -> io::Result<Follow> {
    use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
    let interactive = io::stdin().is_terminal() && io::stdout().is_terminal();
    let raw = |on: bool| {
        if interactive {
            let _ = if on {
                crossterm::terminal::enable_raw_mode()
            } else {
                crossterm::terminal::disable_raw_mode()
            };
        }
    };
    raw(true);
    let outcome = loop {
        let text = read_new_output(id, offset);
        if !text.is_empty() {
            let mut out = io::stdout();
            let text = if interactive {
                text.replace("\r\n", "\n").replace('\n', "\r\n")
            } else {
                text
            };
            let _ = write!(out, "{text}");
            let _ = out.flush();
        }
        let state = jobs::load_state(id);
        if state.state == jobs::WORKING && state.waiting_for.is_some() {
            raw(false);
            print!("{}répondre >{} ", transcript::ACCENT, transcript::RESET);
            let _ = io::stdout().flush();
            let mut answer = String::new();
            if io::stdin().read_line(&mut answer)? == 0 {
                break Follow::Detached;
            }
            if !answer.trim().is_empty() {
                jobs::send(id, answer.trim())?;
                for _ in 0..50 {
                    if jobs::load_state(id).waiting_for.is_none() {
                        break;
                    }
                    thread::sleep(Duration::from_millis(100));
                }
            }
            raw(true);
            continue;
        }
        if !jobs::alive(id) || state.state != jobs::WORKING {
            break Follow::Idle;
        }
        if interactive {
            if event::poll(Duration::from_millis(300))? {
                if let Event::Key(key) = event::read()? {
                    let ctrl_c = key.modifiers.contains(KeyModifiers::CONTROL)
                        && matches!(key.code, KeyCode::Char('c' | 'C'));
                    if key.kind == KeyEventKind::Press
                        && (matches!(key.code, KeyCode::Esc | KeyCode::Left) || ctrl_c)
                    {
                        break Follow::Detached;
                    }
                }
            }
        } else {
            thread::sleep(Duration::from_millis(300));
        }
    };
    raw(false);
    Ok(outcome)
}

/// `claw attach <id>` : les sorties de la session, puis sa conversation dans
/// ce terminal (le processus d'arrière-plan s'efface). `/exit` rend la main ;
/// la session reste dans la liste et reprend sur une réponse.
fn attach_job(reference: &str) -> Result<(), Box<dyn std::error::Error>> {
    let job = jobs::find(reference)?;
    let id = job.id.clone();
    if jobs::load_state(&id).attached {
        return Err(format!("{} est déjà ouverte dans un autre terminal", job.name).into());
    }
    let history = jobs::log_tail(&id, 48 * 1024);
    let mut offset = fs::metadata(jobs::log_path(&id)).map_or(0, |metadata| metadata.len());
    print!("{history}");
    println!(
        "\n{}── {} · {} · Échap rend la main sans arrêter la session ──{}",
        transcript::DIM,
        job.name,
        id,
        transcript::RESET
    );
    if jobs::alive(&id) {
        if let Follow::Detached = follow_job(&id, &mut offset)? {
            println!("\n  ⎿  {} continue en arrière-plan", job.name);
            return Ok(());
        }
    }
    if job.kind == "exec" {
        println!("\n  ⎿  commande terminée");
        return Ok(());
    }
    if jobs::alive(&id) {
        jobs::set_flag(&id, "release");
        let started = Instant::now();
        while jobs::alive(&id) && started.elapsed() < Duration::from_secs(20) {
            thread::sleep(Duration::from_millis(200));
        }
        if jobs::alive(&id) {
            return Err("la session ne s'est pas libérée : réessayez dans un instant".into());
        }
    }
    let state = jobs::load_state(&id);
    let session = state
        .session_path
        .clone()
        .or_else(|| job.session_path.clone())
        .filter(|path| Path::new(path).is_file())
        .ok_or("cette session n'a pas encore de conversation enregistrée")?;
    let previous = env::current_dir()?;
    env::set_current_dir(&job.cwd)?;
    jobs::update_state(&id, |state| {
        state.attached = true;
        state.pid = Some(std::process::id());
    });
    let heartbeat = jobs::Heartbeat::start(&id);
    *ATTACHED_JOB
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(id.clone());
    STOP_ATTACHED.store(false, std::sync::atomic::Ordering::SeqCst);
    install_interrupt_handler();
    let mode = parse_permission_mode_arg(&job.permission_mode)
        .unwrap_or_else(|_| default_permission_mode());
    let result = run_repl(job.model.clone(), None, mode, Some(PathBuf::from(&session)));
    drop(heartbeat);
    *ATTACHED_JOB
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
    let stopped = STOP_ATTACHED.swap(false, std::sync::atomic::Ordering::SeqCst);
    let answer = Session::load_from_path(Path::new(&session))
        .map(|session| last_assistant_of(&session))
        .unwrap_or_default();
    jobs::update_state(&id, |state| {
        state.attached = false;
        state.pid = None;
        state.state = if stopped { jobs::STOPPED } else { jobs::DONE }.to_string();
        state.waiting_for = None;
        state.question = None;
        state.finished_at = Some(jobs::now_ms());
        if !answer.trim().is_empty() {
            state.detail = summary_line(&answer);
        }
    });
    env::set_current_dir(previous)?;
    result
}

/// `claw logs <id> [-f]`.
fn show_job_logs(reference: &str, follow: bool) -> Result<(), Box<dyn std::error::Error>> {
    let job = jobs::find(reference)?;
    let text = jobs::log_tail(&job.id, 32 * 1024);
    if text.trim().is_empty() {
        println!("(aucune sortie pour l'instant)");
    } else if io::stdout().is_terminal() {
        print!("{text}");
    } else {
        print!("{}", transcript::strip_ansi(&text));
    }
    if follow && jobs::alive(&job.id) {
        let mut offset = fs::metadata(jobs::log_path(&job.id)).map_or(0, |metadata| metadata.len());
        let _ = follow_job(&job.id, &mut offset)?;
    }
    Ok(())
}

/// `claw agents` : la vue, puis la session ouverte, puis la vue à nouveau.
fn run_agent_view(
    model: String,
    permission_mode: PermissionMode,
    filter: Option<PathBuf>,
) -> Result<(), Box<dyn std::error::Error>> {
    let defaults = agent_view::Defaults {
        model,
        permission_mode: permission_mode.as_str().to_string(),
        cwd: env::current_dir()?,
        filter,
    };
    let mut selected = None;
    loop {
        match agent_view::run(defaults.clone(), selected.clone())? {
            agent_view::Outcome::Exit => return Ok(()),
            agent_view::Outcome::Attach(id) => {
                if let Err(error) = attach_job(&id) {
                    eprintln!("{}", render_cli_error(&error.to_string()));
                    thread::sleep(Duration::from_secs(2));
                }
                selected = Some(id);
            }
        }
    }
}

/// Demande par défaut d'un `/loop` sans demande.
const LOOP_MAINTENANCE_PROMPT: &str = "Maintenance pass: 1. continue any unfinished work from this conversation; 2. tend to the current branch's pull request if there is one (review comments, failed CI runs, merge conflicts); 3. when nothing else is pending, run a cleanup pass (bug hunt or simplification) limited to code this conversation touched. Do not start new initiatives outside that scope, and only push, delete or take other irreversible actions when they continue something the conversation already authorized. If nothing needs doing, say so in one line.";

/// `/loop` sans demande : `.claw/loop.md` ou `.claude/loop.md` du projet,
/// sinon de l'utilisateur (`~/.claw/loop.md`, `~/.claude/loop.md`), sinon la
/// passe d'entretien intégrée.
fn default_loop_prompt() -> String {
    let cwd = env::current_dir().unwrap_or_default();
    let config_home = ConfigLoader::default_for(&cwd).config_home().to_path_buf();
    let mut candidates = vec![
        cwd.join(".claw").join("loop.md"),
        cwd.join(".claude").join("loop.md"),
        config_home.join("loop.md"),
    ];
    if let Some(home) = config_home.parent() {
        candidates.push(home.join(".claude").join("loop.md"));
    }
    for path in candidates {
        if let Ok(text) = fs::read_to_string(&path) {
            if !text.trim().is_empty() {
                let mut end = text.len().min(25_000);
                while !text.is_char_boundary(end) {
                    end -= 1;
                }
                return text[..end].trim().to_string();
            }
        }
    }
    LOOP_MAINTENANCE_PROMPT.to_string()
}

/// Consigne d'une boucle au rythme choisi.
fn self_paced_note(request: &str) -> String {
    format!(
        "\n\n[Claw] This is a self-paced /loop. When this iteration is done, call ScheduleWakeup with delaySeconds between 60 and 3600 chosen from what you observed (short while something is still in progress, long when nothing is pending), prompt set to exactly {} and a one-line reason; or call ScheduleWakeup with stop: true when the task is complete.",
        serde_json::to_string(request).unwrap_or_default()
    )
}

/// Demande envoyée quand une tâche planifiée passe.
fn scheduled_prompt(task: &runtime::cron::ScheduledTask) -> String {
    let schedule = if task.self_paced {
        "self-paced /loop".to_string()
    } else if task.recurring {
        format!("cron {}, run {}", task.cron, task.runs)
    } else {
        "one-time reminder".to_string()
    };
    let mut prompt = format!(
        "[Claw] Scheduled task {} ({schedule}) is due: do this now. If it names a skill (/name), run it with the Skill tool.",
        task.id
    );
    if task.recurring && task.runs > 1 {
        prompt.push_str(
            " Report only what changed since the previous run; say so briefly if nothing did.",
        );
    }
    let _ = write!(prompt, "\n\n{}", task.prompt);
    if task.self_paced {
        prompt.push_str(&self_paced_note(&task.prompt));
    }
    prompt
}

/// Après un passage d'une boucle au rythme choisi : prochain réveil demandé
/// par le modèle, arrêt, ou réveil de secours dans 20 minutes (une seule
/// fois : sans nouveau réveil après celui-là, la boucle s'arrête).
fn after_self_paced_pass(request: &str, was_fallback: bool) {
    let dim = transcript::DIM;
    let reset = transcript::RESET;
    if runtime::cron::take_stop_requested() {
        println!("  ⎿  {dim}Boucle terminée par Claw{reset}");
        return;
    }
    if let Some(task) = runtime::cron::list()
        .into_iter()
        .find(|task| task.self_paced && task.prompt == request)
    {
        println!(
            "  ⎿  {dim}Prochain passage de la boucle à {} · /loop stop pour l'arrêter{reset}",
            runtime::cron::format_local(task.next_run)
        );
        return;
    }
    if was_fallback {
        println!("  ⎿  {dim}Boucle arrêtée : aucun nouveau passage demandé{reset}");
        return;
    }
    match runtime::cron::schedule_fallback(request) {
        Ok(task) => println!(
            "  ⎿  {dim}Aucun délai choisi : prochain passage à {} · /loop stop pour l'arrêter{reset}",
            runtime::cron::format_local(task.next_run)
        ),
        Err(error) => eprintln!("{}", render_cli_error(&error)),
    }
}

/// `/keybindings` : crée `~/.claw/keybindings.json` s'il n'existe aucun fichier
/// de raccourcis, puis montre les raccourcis en vigueur et les remarques.
fn keybindings_command() -> String {
    let mut lines = vec!["Raccourcis clavier".to_string()];
    if !keybindings::files().iter().any(|path| path.is_file()) {
        let path = keybindings::claw_file();
        let created = path
            .parent()
            .map_or(Ok(()), fs::create_dir_all)
            .and_then(|()| fs::write(&path, keybindings::TEMPLATE));
        match created {
            Ok(()) => lines.push(format!("  Créé             {}", path.display())),
            Err(error) => lines.push(format!(
                "  Création impossible de {} : {error}",
                path.display()
            )),
        }
    }
    let keymap = keybindings::Keymap::load();
    for path in &keymap.sources {
        lines.push(format!("  Fichier          {}", path.display()));
    }
    lines.push(
        "  Format           celui de Claude Code : https://code.claude.com/docs/en/keybindings"
            .to_string(),
    );
    lines.push(
        "  Prise en compte  à la saisie suivante, sans redémarrer ; \"null\" retire un raccourci"
            .to_string(),
    );
    lines.push("En vigueur".to_string());
    lines.push(keymap.report());
    if !keymap.warnings.is_empty() {
        lines.push("Remarques".to_string());
        lines.extend(keymap.warnings.iter().map(|warning| format!("  {warning}")));
    }
    lines.push(
        "Toujours actifs : Ctrl+C (effacer, quitter), Ctrl+D (quitter), Ctrl+A/E/K/U/W/Y, Alt+B/F/D"
            .to_string(),
    );
    lines.join("\n")
}

/// La saisie s'interrompt : une tâche planifiée est due, ou une sous-tâche a
/// fini.
pub(crate) fn idle_wakeup() -> bool {
    runtime::cron::has_due() || extra_commands::has_finished_subtask()
}

/// Session en arrière-plan ouverte dans ce terminal (`claw attach`), et
/// `/stop` demandé pendant qu'elle l'est.
static ATTACHED_JOB: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);
static STOP_ATTACHED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// `/sandbox` : état du bac à sable ; `docker`, `podman` ou `off` choisit le
/// conteneur pour la session.
fn sandbox_command(args: Option<&str>) -> String {
    match args.map(str::trim).filter(|value| !value.is_empty()) {
        Some(engine @ ("docker" | "podman")) => {
            env::set_var("CLAW_SANDBOX", engine);
            format!("Bac à sable : chaque commande shell s'exécute dans un conteneur {engine} (pour cette session ; \"sandbox\": {{\"backend\": \"{engine}\"}} dans .claw/settings.json pour le garder)")
        }
        Some("off" | "aucun" | "none") => {
            env::remove_var("CLAW_SANDBOX");
            "Bac à sable : plus de conteneur pour cette session (les réglages \"sandbox\" restent appliqués)".to_string()
        }
        Some(other) => format!("Option inconnue « {other} » : /sandbox [docker|podman|off]"),
        None => {
            let cwd = env::current_dir().unwrap_or_default();
            let config = ConfigLoader::default_for(&cwd).load().ok();
            let sandbox = config
                .as_ref()
                .map(|config| config.sandbox().clone())
                .unwrap_or_default();
            let container = sandbox.container().map_or_else(
                || "aucun".to_string(),
                |container| format!("{} ({})", container.program, container.image),
            );
            let status = runtime::sandbox::resolve_sandbox_status(&sandbox, &cwd);
            format!(
                "Bac à sable\n  Conteneur        {container}\n  Isolation Linux  {}\n  Changer          /sandbox docker|podman|off (session) · \"sandbox\" dans .claw/settings.json",
                if status.active { "active (namespaces)" } else { "inactive" }
            )
        }
    }
}

/// La conversation active change : ses tâches planifiées reviennent, et la
/// télémétrie la désigne par son identifiant.
fn session_bound(path: &Path) {
    runtime::cron::bind_session(Some(path));
    if let Some(id) = path.file_stem().and_then(|stem| stem.to_str()) {
        runtime::telemetry::set_session(id);
    }
}

/// Réglage `env` des fichiers de config (`~/.claw/settings.json`,
/// `.claw/settings.json`…), appliqué avant le démarrage des autres threads.
fn apply_settings_env() {
    let Ok(cwd) = env::current_dir() else {
        return;
    };
    if let Ok(config) = ConfigLoader::default_for(&cwd).load() {
        runtime::settings::apply_env(&config);
    }
}

/// Réglages `attribution` et `includeCoAuthoredBy` du projet courant.
fn current_attribution() -> runtime::settings::Attribution {
    let config = env::current_dir()
        .ok()
        .and_then(|cwd| ConfigLoader::default_for(cwd).load().ok())
        .unwrap_or_else(runtime::RuntimeConfig::empty);
    runtime::settings::attribution(&config)
}

/// La config utilisateur (`~/.claw`), les sessions et les skills se basent sur
/// `HOME`, rarement défini sous Windows hors Git Bash : on le déduit de
/// `USERPROFILE` avant que d'autres threads ne démarrent.
fn ensure_home_env() {
    if cfg!(windows) && env::var_os("HOME").is_none() {
        if let Some(profile) = env::var_os("USERPROFILE") {
            env::set_var("HOME", profile);
        }
    }
}

/// Pendant un tour, Ctrl+C interrompt le tour au lieu de quitter le programme ;
/// un second Ctrl+C dans les deux secondes quitte. Pendant la saisie, l'éditeur
/// de ligne (mode brut) reçoit Ctrl+C comme une touche : rien ne change.
fn install_interrupt_handler() {
    static INSTALLED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if INSTALLED.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return;
    }
    let spawned = thread::Builder::new()
        .name("claw-ctrl-c".to_string())
        .spawn(|| {
            let Ok(signal_runtime) = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            else {
                return;
            };
            signal_runtime.block_on(async {
                let mut last_press: Option<Instant> = None;
                while tokio::signal::ctrl_c().await.is_ok() {
                    let now = Instant::now();
                    let pressed_twice = last_press.is_some_and(|previous| {
                        now.duration_since(previous) < Duration::from_secs(2)
                    });
                    if pressed_twice && runtime::interrupt::interrupt_requested() {
                        eprintln!("\nArrêt de claw.");
                        tools::shutdown_mcp();
                        std::process::exit(130);
                    }
                    last_press = Some(now);
                    runtime::interrupt::request_interrupt();
                    live::suspend(|| {
                        eprintln!("\n■ Interruption du tour… (Ctrl+C à nouveau pour quitter claw)");
                    });
                }
            });
        });
    if let Err(error) = spawned {
        eprintln!("Ctrl+C ne pourra pas interrompre les tours : {error}");
    }
}

fn render_cli_error(problem: &str) -> String {
    let mut lines = vec!["Erreur".to_string()];
    for (index, line) in problem.lines().enumerate() {
        let label = if index == 0 {
            "  Problème         "
        } else {
            "                   "
        };
        lines.push(format!("{label}{line}"));
    }
    lines.push(format!("  Aide             {}", error_hint(problem)));
    lines.join("\n")
}

/// Code HTTP d'une erreur du fournisseur (« l'api a retourné 400 … »).
fn returned_status(problem: &str) -> Option<u16> {
    const MARKER: &str = "l'api a retourné ";
    let after = &problem[problem.rfind(MARKER)? + MARKER.len()..];
    after.get(..3)?.parse().ok()
}

/// Conseil adapté à l'erreur : chaque famille de cause dit quoi essayer (dans
/// le terminal comme dans la page).
fn error_hint(problem: &str) -> &'static str {
    let lower = problem.to_ascii_lowercase();
    let status = returned_status(problem);
    // Le code HTTP, pas un nombre du message : « 140129 tokens » contient
    // « 401 », « 142900 tokens » contient « 429 ».
    if status == Some(429)
        || lower.contains("rate limit")
        || lower.contains("too many requests")
        || lower.contains("resource_exhausted")
    {
        "limite de requêtes atteinte : les modèles gratuits sont limités par minute et par jour (quota : /models) ; réessayez plus tard ou changez de modèle (/model)"
    } else if lower.contains("data policy") {
        "autorisez les modèles gratuits dans vos paramètres de confidentialité OpenRouter (Settings > Privacy)"
    } else if lower.contains("no endpoints found") || lower.contains("tool use") {
        "ce modèle ne prend peut-être pas en charge l'appel d'outils : choisissez-en un avec /models"
    } else if status == Some(401)
        || lower.contains("user not found")
        || lower.contains("unauthorized")
    {
        "vérifiez la clé OPENROUTER_API_KEY (ou ANTHROPIC_API_KEY pour un modèle Claude direct)"
    } else if lower.contains("image") && (lower.contains("support") || lower.contains("modalit")) {
        "ce modèle ne lit pas les images : choisissez un modèle multimodal (/models vision)"
    } else if status == Some(402) {
        "crédits insuffisants : ce modèle est payant, ou le solde du compte est épuisé ; choisissez un modèle gratuit avec /models"
    } else if status == Some(403) {
        "ce modèle est refusé à votre clé : choisissez-en un autre avec /models"
    } else if status == Some(404) {
        "modèle introuvable chez ce fournisseur : vérifiez son identifiant avec /models"
    } else if status == Some(413)
        || lower.contains("context length")
        || lower.contains("too many tokens")
        || lower.contains("maximum number of tokens")
        || lower.contains("request too large")
    {
        "la conversation dépasse ce que ce modèle accepte : résumez-la avec /compact, ou repartez d'une session neuve"
    } else if api::upstream_failure_status(problem).is_some() {
        "le fournisseur du modèle a cessé de répondre (surcharge ou délai dépassé) et aucun autre modèle n'a pris le relais : renvoyez la demande, ou changez de modèle (/model)"
    } else if matches!(status, Some(400 | 422)) {
        "le fournisseur a refusé la requête telle qu'elle est écrite : essayez un autre modèle (/model), ou /compact si la conversation est longue ; si l'erreur revient avec tous les modèles, signalez-la"
    } else if lower.contains("erreur http") {
        "le serveur du modèle est injoignable : vérifiez la connexion, ou qu'Ollama est lancé pour un modèle local"
    } else if lower.contains("identifiants") && lower.contains("manquants") {
        "aucune clé API trouvée : définissez OPENROUTER_API_KEY (clé gratuite sur https://openrouter.ai/keys) puis relancez claw"
    } else {
        "renvoyez la demande ; si l'erreur revient, changez de modèle (/model) ou lancez `claw doctor`"
    }
}

/// `--add-dir <dossier>` et `--add-dir=<dossier>` retirés des arguments.
fn take_add_dir_flags(args: &mut Vec<String>) -> Vec<PathBuf> {
    let mut directories = Vec::new();
    let mut index = 0;
    while index < args.len() {
        if let Some(value) = args[index].strip_prefix("--add-dir=") {
            directories.push(PathBuf::from(value));
            args.remove(index);
        } else if args[index] == "--add-dir" && index + 1 < args.len() {
            args.remove(index);
            directories.push(PathBuf::from(args.remove(index)));
        } else {
            index += 1;
        }
    }
    directories
}

/// `/add-dir [dossier] [--save]` : sans dossier, la liste ; sinon le dossier
/// devient libre en écriture pour la session (et dans
/// `.claw/settings.local.json` avec `--save`).
fn run_add_dir_command(args: Option<&str>) -> String {
    let cwd = env::current_dir().unwrap_or_default();
    let args = args.map(str::trim).unwrap_or_default();
    if args.is_empty() {
        let lines: Vec<String> = runtime::writable_directories(&cwd)
            .iter()
            .map(|directory| format!("  {}", directory.display()))
            .collect();
        return format!(
            "Dossiers où l'écriture est libre (workspace-write)\n{}\n  Ajouter          /add-dir <dossier> [--save]",
            lines.join("\n")
        );
    }
    let save = args.split_whitespace().any(|word| word == "--save");
    let path = args.replace("--save", "");
    let path = path.trim().trim_matches('"');
    let directory = PathBuf::from(path);
    let directory = if directory.is_absolute() {
        directory
    } else {
        cwd.join(directory)
    };
    if !directory.is_dir() {
        return format!("dossier introuvable : {}", directory.display());
    }
    let added = runtime::add_session_directory(directory.clone());
    let mut report = format!(
        "Dossier ajouté\n  Dossier          {}\n  Effet            écriture libre en workspace-write{}",
        directory.display(),
        if added { "" } else { " (déjà présent)" }
    );
    if save {
        match add_directory_setting(&cwd, &directory.display().to_string()) {
            Ok(path) => {
                let _ = std::fmt::Write::write_fmt(
                    &mut report,
                    format_args!("\n  Enregistré dans  {}", path.display()),
                );
            }
            Err(error) => {
                let _ = std::fmt::Write::write_fmt(
                    &mut report,
                    format_args!("\n  Non enregistré   {error}"),
                );
            }
        }
    }
    report
}

/// Ajoute `directory` à `permissions.additionalDirectories` de `.claw/settings.local.json`.
fn add_directory_setting(cwd: &Path, directory: &str) -> Result<PathBuf, String> {
    edit_permission_rule(cwd, Some("additionalDirectories"), directory).map(|(path, _)| path)
}

/// Question d'un hook `prompt` : son modèle, sinon le modèle par défaut, avec
/// les modèles de second avis en secours (quota, panne).
fn evaluate_prompt_hook(system: &str, prompt: &str, model: Option<&str>) -> Result<String, String> {
    let primary = model.map_or_else(default_model, resolve_model_alias);
    let mut candidates = vec![primary.clone()];
    candidates.extend(
        models::advisor_models(&primary)
            .into_iter()
            .filter(|candidate| *candidate != primary),
    );
    tools::advisors::ask(&candidates, system, prompt, 2_000).map(|opinion| opinion.text)
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut args: Vec<String> = env::args().skip(1).collect();
    // `--add-dir <dossier>` (répétable) : écriture libre aussi dans ce dossier.
    for directory in take_add_dir_flags(&mut args) {
        runtime::add_session_directory(directory);
    }
    // `--worktree [nom]` : la session travaille dans un worktree git à part,
    // sur sa propre branche (sessions en parallèle sans conflit).
    let worktree = match git_work::take_worktree_flag(&mut args) {
        Some(name) => Some(git_work::enter(&name)?),
        None => None,
    };
    let result = run_action(&args);
    if let Some(session) = &worktree {
        git_work::leave(session);
    }
    result
}

#[allow(clippy::too_many_lines)]
fn run_action(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    // Hooks `http` et `prompt` : le runtime reçoit l'envoi HTTP et le modèle.
    runtime::set_http_hook_sender(Box::new(tools::send_hook_request));
    runtime::set_prompt_hook_evaluator(Box::new(evaluate_prompt_hook));
    // Serveurs compatibles déclarés dans la config (`providers`) : nécessaires
    // dès l'analyse des arguments pour reconnaître `nom:modèle`.
    let startup_config = env::current_dir()
        .ok()
        .and_then(|cwd| ConfigLoader::default_for(&cwd).load().ok());
    if let Some(config) = &startup_config {
        models::register_configured_providers(config);
    }
    // Modèles mis de côté longtemps (quota du jour épuisé, modèle refusé à
    // Claw) : retenus d'un lancement à l'autre pour ne pas les réessayer.
    if let Ok(cwd) = env::current_dir() {
        api::set_cooldown_store(
            ConfigLoader::default_for(&cwd)
                .config_home()
                .join("model-cooldowns.json"),
        );
    }
    init_reasoning_effort(startup_config.as_ref());
    init_ultracode(startup_config.as_ref());
    init_team(startup_config.as_ref());
    match parse_args(args)? {
        CliAction::DumpManifests => dump_manifests(),
        CliAction::BootstrapPlan => print_bootstrap_plan(),
        CliAction::Agents { args } => LiveCli::print_agents(args.as_deref())?,
        CliAction::Background {
            prompt,
            name,
            exec,
            model,
            permission_mode,
        } => {
            let job = jobs::dispatch(&jobs::Dispatch {
                prompt,
                name,
                command: exec,
                model: model.unwrap_or_else(default_model),
                permission_mode: permission_mode.as_str().to_string(),
                origin: env::current_dir()?,
                session_path: None,
                in_place: false,
            })?;
            println!("{}", jobs::launch_report(&job));
        }
        CliAction::AgentView {
            json,
            all,
            cwd,
            model,
            permission_mode,
        } => {
            if json {
                println!("{}", jobs::list_json(all, cwd.as_deref()));
            } else if io::stdin().is_terminal() && io::stdout().is_terminal() {
                runtime::telemetry::session_started("agents_view");
                run_agent_view(model.unwrap_or_else(default_model), permission_mode, cwd)?;
            } else {
                println!("{}", agent_view::plain_list(cwd.as_deref()));
            }
        }
        CliAction::Attach { target } => attach_job(&target)?,
        CliAction::AutoModeReport { defaults } => println!(
            "{}",
            if defaults {
                auto_mode::defaults_json()
            } else {
                auto_mode::config_json()
            }
        ),
        CliAction::JobLogs { target, follow } => show_job_logs(&target, follow)?,
        CliAction::JobStop { target } => println!("{}", jobs::stop(&target)?),
        CliAction::JobRemove { target } => println!("{}", jobs::remove(&target)?),
        CliAction::JobReply { target, text } => println!("{}", jobs::reply(&target, &text)?),
        CliAction::JobWorker { id } => run_job_worker(&id)?,
        CliAction::Skills { args } => LiveCli::print_skills(args.as_deref())?,
        CliAction::Serve {
            host,
            port,
            open_browser,
            model,
            allowed_tools,
            permission_mode,
        } => web::serve(web::ServeOptions {
            host,
            port,
            open_browser,
            model,
            permission_mode,
            allowed_tools,
        })?,
        CliAction::Models { args, model } => {
            let current = model.unwrap_or_else(default_model);
            println!("{}", models::models_report(args.as_deref(), &current)?);
        }
        CliAction::Credits => println!("{}", credits::report(true)),
        CliAction::ReviewReplay { model, raw, files } => review_replay(model, raw, &files)?,
        CliAction::ChecklistNote { request } => println!(
            "{}",
            runtime::harness::checklist_prompt(fs::read_to_string(request)?.trim(), None, false)
        ),
        CliAction::Doctor { model } => println!(
            "{}",
            doctor::report(&model.unwrap_or_else(default_model), &version_summary())
        ),
        CliAction::PrintSystemPrompt { cwd, date } => print_system_prompt(cwd, date),
        CliAction::Version => print_version(),
        CliAction::ResumeSession {
            session_path,
            commands,
        } => resume_session(&session_path, &commands),
        CliAction::Prompt {
            prompt,
            model,
            output_format,
            allowed_tools,
            permission_mode,
        } => {
            let prompt = prompt_from(prompt, io::stdin())?;
            install_interrupt_handler();
            let mut cli = LiveCli::new(model, true, allowed_tools, permission_mode)?;
            runtime::telemetry::session_started("fresh");
            cli.session_hook_start("startup");
            let result = cli.run_turn_with_output(&prompt, output_format);
            cli.session_hook_end("exit");
            result?;
        }
        CliAction::Login => run_login()?,
        CliAction::Logout => run_logout()?,
        CliAction::Init => run_init()?,
        CliAction::Acp {
            model,
            allowed_tools,
            permission_mode,
        } => acp::serve(model, permission_mode, allowed_tools),
        CliAction::Mcp { args } => match mcp_cli::run(&args) {
            Ok(report) => println!("{report}"),
            Err(error) => return Err(error.into()),
        },
        CliAction::Repl {
            model,
            allowed_tools,
            permission_mode,
        } => {
            install_interrupt_handler();
            run_repl(model, allowed_tools, permission_mode, None)?;
        }
        CliAction::StreamSession {
            model,
            allowed_tools,
            permission_mode,
            resume,
        } => {
            install_interrupt_handler();
            let mut cli = LiveCli::new(model, true, allowed_tools, permission_mode)?;
            if resume {
                if let Some(path) = latest_session()? {
                    let fresh = cli.session.path.clone();
                    cli.load_session_quietly(&path)?;
                    let _ = fs::remove_file(fresh);
                }
            }
            cli.session_hook_start("startup");
            let stdin = io::stdin();
            let mut line = String::new();
            loop {
                line.clear();
                if stdin.read_line(&mut line)? == 0 {
                    break;
                }
                match stream_json::user_text_from_line(&line) {
                    Ok(Some(text)) => cli.run_prompt_stream_json(&text)?,
                    Ok(None) => {}
                    Err(error) => stream_json::emit(
                        &json!({ "type": "system", "subtype": "error", "error": error }),
                    ),
                }
            }
            cli.session_hook_end("exit");
        }
        CliAction::ReplResume {
            model,
            allowed_tools,
            permission_mode,
            pick,
        } => {
            let target = if pick {
                pick_session()?
            } else {
                latest_session()?
            };
            install_interrupt_handler();
            run_repl(model, allowed_tools, permission_mode, target)?;
        }
        CliAction::Help => print_help(),
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum CliAction {
    DumpManifests,
    BootstrapPlan,
    Agents {
        args: Option<String>,
    },
    /// `claw --bg "demande"` (ou `--exec 'commande'`).
    Background {
        prompt: String,
        name: Option<String>,
        exec: Option<String>,
        model: Option<String>,
        permission_mode: PermissionMode,
    },
    /// `claw agents [--json] [--all] [--cwd CHEMIN]`.
    AgentView {
        json: bool,
        all: bool,
        cwd: Option<PathBuf>,
        model: Option<String>,
        permission_mode: PermissionMode,
    },
    Attach {
        target: String,
    },
    /// `claw auto-mode defaults|config`.
    AutoModeReport {
        defaults: bool,
    },
    JobLogs {
        target: String,
        follow: bool,
    },
    JobStop {
        target: String,
    },
    JobRemove {
        target: String,
    },
    JobReply {
        target: String,
        text: String,
    },
    /// Processus d'une session en arrière-plan (lancé par Claw lui-même).
    JobWorker {
        id: String,
    },
    Skills {
        args: Option<String>,
    },
    Models {
        args: Option<String>,
        model: Option<String>,
    },
    Credits,
    Doctor {
        model: Option<String>,
    },
    /// `claw review-replay DEMANDE DIFF RÉPONSE` (banc d'essais réels) : la
    /// relecture que Claw ferait de ce travail, sans outil. `raw` : les deux
    /// fichiers sont les consignes et la demande au relecteur, tels quels.
    ReviewReplay {
        model: Option<String>,
        raw: bool,
        files: Vec<PathBuf>,
    },
    /// `claw checklist-note DEMANDE` (banc d'essais réels) : le contrôle des
    /// exigences que Claw enverrait pour cette demande, sans requête.
    ChecklistNote {
        request: PathBuf,
    },
    Serve {
        host: String,
        port: u16,
        open_browser: bool,
        model: String,
        allowed_tools: Option<AllowedToolSet>,
        permission_mode: PermissionMode,
    },
    PrintSystemPrompt {
        cwd: PathBuf,
        date: String,
    },
    Version,
    ResumeSession {
        session_path: PathBuf,
        commands: Vec<String>,
    },
    Prompt {
        prompt: String,
        model: String,
        output_format: CliOutputFormat,
        allowed_tools: Option<AllowedToolSet>,
        permission_mode: PermissionMode,
    },
    Login,
    Logout,
    Init,
    /// `claw mcp add|list|remove|login…`
    Mcp {
        args: Vec<String>,
    },
    /// `claw acp` : agent de l'Agent Client Protocol (Zed, Neovim…).
    Acp {
        model: String,
        allowed_tools: Option<AllowedToolSet>,
        permission_mode: PermissionMode,
    },
    /// Tours lus en JSON sur l'entrée standard, événements JSON en sortie.
    StreamSession {
        model: String,
        allowed_tools: Option<AllowedToolSet>,
        permission_mode: PermissionMode,
        resume: bool,
    },
    /// REPL sur une session existante (`--continue` ou `--resume` sans chemin).
    ReplResume {
        model: String,
        allowed_tools: Option<AllowedToolSet>,
        permission_mode: PermissionMode,
        pick: bool,
    },
    Repl {
        model: String,
        allowed_tools: Option<AllowedToolSet>,
        permission_mode: PermissionMode,
    },
    // prompt-mode formatting is only supported for non-interactive runs
    Help,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CliOutputFormat {
    Text,
    Json,
    /// Un événement JSON par ligne (texte, outils, résultat).
    StreamJson,
}

impl CliOutputFormat {
    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "text" => Ok(Self::Text),
            "json" => Ok(Self::Json),
            "stream-json" => Ok(Self::StreamJson),
            other => Err(format!(
                "valeur non prise en charge pour --output-format : {other} (attendu : text, json ou stream-json)"
            )),
        }
    }
}

#[allow(clippy::too_many_lines)]
fn parse_args(args: &[String]) -> Result<CliAction, String> {
    // Résolu seulement si l'action en a besoin : le modèle par défaut peut
    // nécessiter le catalogue OpenRouter (réseau), inutile pour `--version`.
    let mut model: Option<String> = None;
    let mut output_format = CliOutputFormat::Text;
    let mut permission_mode = default_permission_mode();
    let mut wants_version = false;
    let mut continue_session = false;
    let mut stream_input = false;
    let mut background = false;
    let mut job_name: Option<String> = None;
    let mut exec_command: Option<String> = None;
    let mut allowed_tool_values = Vec::new();
    let mut rest = Vec::new();
    let mut index = 0;

    while index < args.len() {
        match args[index].as_str() {
            "--version" | "-V" => {
                wants_version = true;
                index += 1;
            }
            "--model" => {
                let value = args
                    .get(index + 1)
                    .ok_or_else(|| "valeur manquante pour --model".to_string())?;
                model = Some(resolve_model_alias(value));
                index += 2;
            }
            flag if flag.starts_with("--model=") => {
                model = Some(resolve_model_alias(&flag[8..]));
                index += 1;
            }
            "--output-format" => {
                let value = args
                    .get(index + 1)
                    .ok_or_else(|| "valeur manquante pour --output-format".to_string())?;
                output_format = CliOutputFormat::parse(value)?;
                index += 2;
            }
            "--input-format" => {
                match args.get(index + 1).map(String::as_str) {
                    Some("stream-json") => stream_input = true,
                    Some("text") => stream_input = false,
                    other => {
                        return Err(format!(
                            "valeur non prise en charge pour --input-format : {} (attendu : text ou stream-json)",
                            other.unwrap_or("")
                        ))
                    }
                }
                index += 2;
            }
            "--permission-mode" => {
                let value = args
                    .get(index + 1)
                    .ok_or_else(|| "valeur manquante pour --permission-mode".to_string())?;
                permission_mode = parse_permission_mode_arg(value)?;
                index += 2;
            }
            flag if flag.starts_with("--output-format=") => {
                output_format = CliOutputFormat::parse(&flag[16..])?;
                index += 1;
            }
            flag if flag.starts_with("--permission-mode=") => {
                permission_mode = parse_permission_mode_arg(&flag[18..])?;
                index += 1;
            }
            "--dangerously-skip-permissions" => {
                permission_mode = PermissionMode::DangerFullAccess;
                index += 1;
            }
            "--continue" | "-c" => {
                continue_session = true;
                index += 1;
            }
            "-p" => {
                if background {
                    return Err(
                        "--bg ne se combine pas avec -p : la demande est l'argument, par exemple claw --bg \"corrige le test\""
                            .to_string(),
                    );
                }
                // Compat Claw Code : -p "prompt" = prompt ponctuel
                let prompt = args[index + 1..].join(" ");
                if prompt.trim().is_empty() {
                    return Err("-p requiert une chaîne de prompt".to_string());
                }
                return Ok(CliAction::Prompt {
                    prompt,
                    model: model.unwrap_or_else(default_model),
                    output_format,
                    allowed_tools: normalize_allowed_tools(&allowed_tool_values)?,
                    permission_mode,
                });
            }
            "--bg" | "--background" => {
                background = true;
                index += 1;
            }
            "--name" => {
                let value = args
                    .get(index + 1)
                    .ok_or_else(|| "valeur manquante pour --name".to_string())?;
                job_name = Some(value.clone());
                index += 2;
            }
            "--exec" => {
                let value = args
                    .get(index + 1)
                    .ok_or_else(|| "valeur manquante pour --exec".to_string())?;
                exec_command = Some(value.clone());
                index += 2;
            }
            "--print" => {
                // Compat Claw Code : --print rend la sortie non interactive
                output_format = CliOutputFormat::Text;
                index += 1;
            }
            "--allowedTools" | "--allowed-tools" => {
                let value = args
                    .get(index + 1)
                    .ok_or_else(|| "valeur manquante pour --allowedTools".to_string())?;
                allowed_tool_values.push(value.clone());
                index += 2;
            }
            flag if flag.starts_with("--allowedTools=") => {
                allowed_tool_values.push(flag[15..].to_string());
                index += 1;
            }
            flag if flag.starts_with("--allowed-tools=") => {
                allowed_tool_values.push(flag[16..].to_string());
                index += 1;
            }
            other => {
                rest.push(other.to_string());
                index += 1;
            }
        }
    }

    if wants_version {
        return Ok(CliAction::Version);
    }
    if background {
        let prompt = rest.join(" ");
        if prompt.trim().is_empty() && exec_command.is_none() {
            return Err(
                "--bg attend une demande : claw --bg \"corrige le test qui échoue\" (ou --exec 'commande')"
                    .to_string(),
            );
        }
        return Ok(CliAction::Background {
            prompt,
            name: job_name,
            exec: exec_command,
            model,
            permission_mode,
        });
    }

    let allowed_tools = normalize_allowed_tools(&allowed_tool_values)?;

    if stream_input {
        return Ok(CliAction::StreamSession {
            model: model.unwrap_or_else(default_model),
            allowed_tools,
            permission_mode,
            resume: continue_session,
        });
    }
    if continue_session && rest.is_empty() {
        return Ok(CliAction::ReplResume {
            model: model.unwrap_or_else(default_model),
            allowed_tools,
            permission_mode,
            pick: false,
        });
    }
    if rest.is_empty() {
        return Ok(CliAction::Repl {
            model: model.unwrap_or_else(default_model),
            allowed_tools,
            permission_mode,
        });
    }
    if matches!(rest.first().map(String::as_str), Some("--help" | "-h")) {
        return Ok(CliAction::Help);
    }
    if rest.first().map(String::as_str) == Some("--resume") {
        // `claw --resume` seul : choisir une session dans la liste.
        if rest.len() == 1 {
            return Ok(CliAction::ReplResume {
                model: model.unwrap_or_else(default_model),
                allowed_tools,
                permission_mode,
                pick: true,
            });
        }
        return parse_resume_args(&rest[1..]);
    }

    match rest[0].as_str() {
        "dump-manifests" => Ok(CliAction::DumpManifests),
        "bootstrap-plan" => Ok(CliAction::BootstrapPlan),
        // `claw agents` : vue des sessions en arrière-plan ; `claw agents list`
        // garde la liste des sous-agents définis.
        "agents"
            if rest
                .get(1)
                .is_none_or(|arg| arg.starts_with("--") && arg != "--help") =>
        {
            parse_agent_view_args(&rest[1..], model, permission_mode)
        }
        "agents" => Ok(CliAction::Agents {
            args: join_optional_args(&rest[1..]),
        }),
        "auto-mode" => match rest.get(1).map(String::as_str) {
            Some("defaults") => Ok(CliAction::AutoModeReport { defaults: true }),
            Some("config") | None => Ok(CliAction::AutoModeReport { defaults: false }),
            Some(other) => Err(format!(
                "claw auto-mode {other} : utilisez claw auto-mode defaults ou claw auto-mode config"
            )),
        },
        "attach" => Ok(CliAction::Attach {
            target: job_target(&rest, "attach")?,
        }),
        "logs" => {
            let follow = rest[1..]
                .iter()
                .any(|arg| matches!(arg.as_str(), "-f" | "--follow"));
            let target = rest[1..]
                .iter()
                .find(|arg| !matches!(arg.as_str(), "-f" | "--follow"))
                .cloned()
                .ok_or_else(|| "usage : claw logs <identifiant ou nom> [-f]".to_string())?;
            Ok(CliAction::JobLogs { target, follow })
        }
        "stop" | "kill" => Ok(CliAction::JobStop {
            target: job_target(&rest, "stop")?,
        }),
        "rm" => Ok(CliAction::JobRemove {
            target: job_target(&rest, "rm")?,
        }),
        "reply" => {
            let target = job_target(&rest, "reply")?;
            let text = rest[2..].join(" ");
            if text.trim().is_empty() {
                return Err("usage : claw reply <identifiant ou nom> <réponse>".to_string());
            }
            Ok(CliAction::JobReply { target, text })
        }
        "__job-worker" => Ok(CliAction::JobWorker {
            id: job_target(&rest, "__job-worker")?,
        }),
        "skills" => Ok(CliAction::Skills {
            args: join_optional_args(&rest[1..]),
        }),
        "serve" => {
            let (host, port, open_browser) = parse_serve_args(&rest[1..])?;
            Ok(CliAction::Serve {
                host,
                port,
                open_browser,
                model: model.unwrap_or_else(default_model),
                allowed_tools,
                permission_mode,
            })
        }
        "models" => Ok(CliAction::Models {
            args: join_optional_args(&rest[1..]),
            model,
        }),
        "credits" | "solde" => Ok(CliAction::Credits),
        "checklist-note" => match &rest[1..] {
            [request] => Ok(CliAction::ChecklistNote {
                request: PathBuf::from(request),
            }),
            _ => Err("usage : claw checklist-note FICHIER_DE_LA_DEMANDE".to_string()),
        },
        "review-replay" => {
            let raw = rest.get(1).map(String::as_str) == Some("--raw");
            let files: Vec<PathBuf> = rest[1 + usize::from(raw)..]
                .iter()
                .map(PathBuf::from)
                .collect();
            if files.len() != if raw { 2 } else { 3 } {
                return Err(
                    "usage : claw review-replay DEMANDE DIFF RÉPONSE, ou claw review-replay --raw CONSIGNES DEMANDE"
                        .to_string(),
                );
            }
            Ok(CliAction::ReviewReplay { model, raw, files })
        }
        "doctor" | "diagnostic" => Ok(CliAction::Doctor { model }),
        "system-prompt" => parse_system_prompt_args(&rest[1..]),
        "login" => Ok(CliAction::Login),
        "logout" => Ok(CliAction::Logout),
        "init" => Ok(CliAction::Init),
        "mcp" => Ok(CliAction::Mcp {
            args: rest[1..].to_vec(),
        }),
        "acp" => Ok(CliAction::Acp {
            model: model.unwrap_or_else(default_model),
            allowed_tools,
            permission_mode,
        }),
        "prompt" => {
            let prompt = rest[1..].join(" ");
            if prompt.trim().is_empty() {
                return Err("la sous-commande prompt requiert une chaîne de prompt".to_string());
            }
            Ok(CliAction::Prompt {
                prompt,
                model: model.unwrap_or_else(default_model),
                output_format,
                allowed_tools,
                permission_mode,
            })
        }
        // `claw /review`, `claw /fix "cargo test"`… : prompt ponctuel spécialisé.
        other if other.starts_with('/') => match SlashCommand::parse(&rest.join(" ")) {
            Some(SlashCommand::Models { args }) => Ok(CliAction::Models { args, model }),
            Some(SlashCommand::Credits) => Ok(CliAction::Credits),
            Some(SlashCommand::Doctor) => Ok(CliAction::Doctor { model }),
            Some(SlashCommand::Expert { command, args }) => Ok(CliAction::Prompt {
                prompt: expert_command_prompt(command, args.as_deref())?,
                model: model.unwrap_or_else(default_model),
                output_format,
                allowed_tools,
                permission_mode,
            }),
            Some(SlashCommand::Unknown(_)) => {
                // Les arguments contenant des espaces gardent leurs guillemets ($1…$9).
                let input = rest
                    .iter()
                    .map(|arg| {
                        if arg.is_empty() || arg.contains(char::is_whitespace) {
                            format!("\"{arg}\"")
                        } else {
                            arg.clone()
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(" ");
                match custom_command_prompt(&input) {
                    Some(invocation) => {
                        let invocation = invocation?;
                        Ok(CliAction::Prompt {
                            prompt: invocation.prompt,
                            model: invocation
                                .command
                                .model
                                .map(|model| resolve_model_alias(&model))
                                .or(model)
                                .unwrap_or_else(default_model),
                            output_format,
                            allowed_tools: custom_allowed_tools(&invocation.command.allowed_tools)
                                .or(allowed_tools),
                            permission_mode,
                        })
                    }
                    None => parse_direct_slash_cli_action(&rest),
                }
            }
            _ => parse_direct_slash_cli_action(&rest),
        },
        _other => Ok(CliAction::Prompt {
            prompt: rest.join(" "),
            model: model.unwrap_or_else(default_model),
            output_format,
            allowed_tools,
            permission_mode,
        }),
    }
}

const DEFAULT_SERVE_PORT: u16 = 8787;

/// `claw serve [--port N] [--host ADRESSE] [--no-open]`
fn parse_serve_args(args: &[String]) -> Result<(String, u16, bool), String> {
    let mut host = "127.0.0.1".to_string();
    let mut port = DEFAULT_SERVE_PORT;
    let mut open_browser = true;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--port" => {
                let value = args
                    .get(index + 1)
                    .ok_or_else(|| "valeur manquante pour --port".to_string())?;
                port = value
                    .parse()
                    .map_err(|_| format!("port invalide : {value}"))?;
                index += 2;
            }
            "--host" => {
                host.clone_from(
                    args.get(index + 1)
                        .ok_or_else(|| "valeur manquante pour --host".to_string())?,
                );
                index += 2;
            }
            "--no-open" => {
                open_browser = false;
                index += 1;
            }
            // Affichage dans un panneau de VS Code (extension Claw).
            "--embed" => {
                env::set_var("CLAW_WEB_EMBED", "1");
                index += 1;
            }
            other => return Err(format!("option inconnue pour serve : {other}")),
        }
    }
    Ok((host, port, open_browser))
}

fn join_optional_args(args: &[String]) -> Option<String> {
    let joined = args.join(" ");
    let trimmed = joined.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

fn parse_direct_slash_cli_action(rest: &[String]) -> Result<CliAction, String> {
    let raw = rest.join(" ");
    match SlashCommand::parse(&raw) {
        Some(SlashCommand::Help) => Ok(CliAction::Help),
        Some(SlashCommand::Agents { args }) => Ok(CliAction::Agents { args }),
        Some(SlashCommand::Skills { args }) => Ok(CliAction::Skills { args }),
        Some(command) => Err(format_direct_slash_command_error(
            match &command {
                SlashCommand::Unknown(name) => format!("/{name}"),
                _ => rest[0].clone(),
            }
            .as_str(),
            matches!(command, SlashCommand::Unknown(_)),
        )),
        None => Err(format!("sous-commande inconnue : {}", rest[0])),
    }
}

fn format_direct_slash_command_error(command: &str, is_unknown: bool) -> String {
    let trimmed = command.trim().trim_start_matches('/');
    let mut lines = vec![
        "Commande slash indisponible hors du REPL".to_string(),
        format!("  Commande         /{trimmed}"),
    ];
    if is_unknown {
        append_slash_command_suggestions(&mut lines, trimmed);
    } else {
        lines
            .push("  Essayez          lancez `claw` pour utiliser les commandes slash".to_string());
        lines.push(
            "  Astuce           les commandes [resume] marchent aussi avec `claw --resume SESSION.json ...`"
                .to_string(),
        );
    }
    lines.join("\n")
}

/// Nom complet d'un modèle de session. `free` (ou `gratuit`) désigne le
/// modèle gratuit le plus puissant du moment, choisi au lancement.
fn resolve_model_alias(model: &str) -> String {
    if models::is_strongest_free_keyword(model) {
        // Les tests ne lisent ni le catalogue ni le réseau.
        if cfg!(test) {
            return models::FREE_ROUTER_MODEL.to_string();
        }
        return models::best_free_coding_model()
            .unwrap_or_else(|| models::FREE_ROUTER_MODEL.to_string());
    }
    api::resolve_model_alias(model)
}

fn normalize_allowed_tools(values: &[String]) -> Result<Option<AllowedToolSet>, String> {
    current_tool_registry()?.normalize_allowed_tools(values)
}

fn current_tool_registry() -> Result<GlobalToolRegistry, String> {
    let cwd = env::current_dir().map_err(|error| error.to_string())?;
    let loader = ConfigLoader::default_for(&cwd);
    let runtime_config = loader.load().map_err(|error| error.to_string())?;
    let plugin_manager = build_plugin_manager(&cwd, &loader, &runtime_config);
    let plugin_tools = plugin_manager
        .aggregated_tools()
        .map_err(|error| error.to_string())?;
    GlobalToolRegistry::with_plugin_tools(plugin_tools)
}

fn parse_permission_mode_arg(value: &str) -> Result<PermissionMode, String> {
    normalize_permission_mode(value)
        .ok_or_else(|| {
            format!(
                "mode de permission non pris en charge « {value} ». Utilisez prompt, read-only, workspace-write, dont-ask ou danger-full-access."
            )
        })
        .map(permission_mode_from_label)
}

fn permission_mode_from_label(mode: &str) -> PermissionMode {
    match mode {
        "prompt" => PermissionMode::Prompt,
        "read-only" => PermissionMode::ReadOnly,
        "workspace-write" => PermissionMode::WorkspaceWrite,
        "danger-full-access" => PermissionMode::DangerFullAccess,
        "dont-ask" => PermissionMode::DontAsk,
        "auto" => PermissionMode::Auto,
        other => panic!("label de mode de permission non pris en charge : {other}"),
    }
}

/// Mode de départ : `CLAW_PERMISSION_MODE`, puis `permissions.defaultMode` des
/// fichiers de config, sinon `prompt` (lecture libre, demande avant d'agir).
fn default_permission_mode() -> PermissionMode {
    if let Some(mode) = env::var("CLAW_PERMISSION_MODE")
        .ok()
        .as_deref()
        .and_then(normalize_permission_mode)
    {
        return permission_mode_from_label(mode);
    }
    env::current_dir()
        .ok()
        .and_then(|cwd| ConfigLoader::default_for(&cwd).load().ok())
        .and_then(|config| config.permission_mode())
        .map_or(PermissionMode::Prompt, |mode| match mode {
            ResolvedPermissionMode::Prompt => PermissionMode::Prompt,
            ResolvedPermissionMode::ReadOnly => PermissionMode::ReadOnly,
            ResolvedPermissionMode::WorkspaceWrite => PermissionMode::WorkspaceWrite,
            ResolvedPermissionMode::DangerFullAccess => PermissionMode::DangerFullAccess,
            ResolvedPermissionMode::DontAsk => PermissionMode::DontAsk,
            ResolvedPermissionMode::Auto => PermissionMode::Auto,
        })
}

/// Outils qui servent rarement : un petit modèle s'y égare (il lance `REPL`
/// au lieu de `bash`, interroge `Config`…). Ils restent disponibles avec
/// `--allowedTools`.
const SELDOM_USED_TOOLS: &[&str] = &[
    "REPL",
    "Config",
    "Sleep",
    "SendUserMessage",
    "StructuredOutput",
    "ToolSearch",
];

/// Le projet contient des notebooks Jupyter (cherchés une fois, sur trois niveaux).
fn project_has_notebooks() -> bool {
    static FOUND: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *FOUND.get_or_init(|| {
        let Ok(root) = env::current_dir() else {
            return false;
        };
        let mut pending = vec![(root, 0)];
        while let Some((dir, depth)) = pending.pop() {
            let Ok(entries) = fs::read_dir(&dir) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                let name = entry.file_name().to_string_lossy().to_string();
                if path.is_dir() {
                    if depth < 3
                        && !name.starts_with('.')
                        && name != "node_modules"
                        && name != "target"
                    {
                        pending.push((path, depth + 1));
                    }
                } else if path
                    .extension()
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("ipynb"))
                {
                    return true;
                }
            }
        }
        false
    })
}

/// Outils proposés au modèle. Sans liste explicite, seuls ceux qui servent
/// dans la situation : pas d'outil de notebook sans notebook, pas de suivi de
/// tâche de fond sans tâche de fond, `PowerShell` seulement faute de shell POSIX.
fn filter_tool_specs(
    tool_registry: &GlobalToolRegistry,
    allowed_tools: Option<&AllowedToolSet>,
) -> Vec<ToolDefinition> {
    let mut definitions = tool_registry.definitions(allowed_tools);
    // Les problèmes de l'éditeur ne se lisent que si un éditeur est relié.
    let editor = runtime::editor::connected().is_some()
        && runtime::harness::enabled(runtime::harness::Aid::EditorDiagnostics);
    definitions.retain(|definition| definition.name != "EditorDiagnostics" || editor);
    // Les questions à choix ne se posent que si quelqu'un peut y répondre.
    let questions = tools::question_asker_available();
    definitions.retain(|definition| definition.name != "AskUserQuestion" || questions);
    if allowed_tools.is_some() || !runtime::harness::enabled(runtime::harness::Aid::ToolDiet) {
        return definitions;
    }
    let background = !runtime::bash_tasks::list().is_empty();
    let posix_shell = plugins::posix_shell_program().is_some();
    definitions.retain(|definition| match definition.name.as_str() {
        name if SELDOM_USED_TOOLS.contains(&name) => false,
        "BashOutput" | "KillShell" => background,
        // Sans navigateur installé, l'outil ne pourrait rien montrer.
        "ViewPage" | "Screenshot" => tools::page_viewer_available(),
        "PowerShell" => !posix_shell,
        "NotebookEdit" => project_has_notebooks(),
        "Consult" => {
            runtime::harness::enabled(runtime::harness::Aid::Council)
                && !tools::advisors::configured_models().is_empty()
        }
        _ => true,
    });
    definitions
}

/// Rejoue la relecture par un autre modèle sur un travail enregistré : une
/// requête, aucun outil. Sert au banc d'essais réels à comparer deux
/// formulations de la relecture sur le même travail. `model` : le modèle qui
/// a fait le travail ; le relecteur est choisi comme pendant un tour.
fn review_replay(
    model: Option<String>,
    raw: bool,
    files: &[PathBuf],
) -> Result<(), Box<dyn std::error::Error>> {
    let read = |index: usize| fs::read_to_string(&files[index]);
    let (system, prompt) = if raw {
        (read(0)?, read(1)?)
    } else {
        (
            runtime::harness::REVIEW_SYSTEM.to_string(),
            runtime::harness::review_prompt(read(0)?.trim(), None, &read(2)?, &[read(1)?]),
        )
    };
    let worker = resolve_model_alias(&model.unwrap_or_else(default_model));
    tools::set_request_observer(credits::note_request);
    let reviewers = models::advisor_models(&worker);
    let started = Instant::now();
    let opinion = tools::advisors::ask_within(
        &reviewers,
        &system,
        &prompt,
        tools::advisors::REVIEW_MAX_TOKENS,
        tools::advisors::Patience::REVIEW,
    )?;
    println!(
        "{}",
        json!({
            "reviewer": opinion.model,
            "seconds": started.elapsed().as_secs(),
            "answer": opinion.text,
            "findings": runtime::harness::review_findings(&opinion.text),
        })
    );
    Ok(())
}

fn parse_system_prompt_args(args: &[String]) -> Result<CliAction, String> {
    let mut cwd = env::current_dir().map_err(|error| error.to_string())?;
    let mut date = runtime::current_date_iso();
    let mut index = 0;

    while index < args.len() {
        match args[index].as_str() {
            "--cwd" => {
                let value = args
                    .get(index + 1)
                    .ok_or_else(|| "valeur manquante pour --cwd".to_string())?;
                cwd = PathBuf::from(value);
                index += 2;
            }
            "--date" => {
                let value = args
                    .get(index + 1)
                    .ok_or_else(|| "valeur manquante pour --date".to_string())?;
                date.clone_from(value);
                index += 2;
            }
            other => return Err(format!("option system-prompt inconnue : {other}")),
        }
    }

    Ok(CliAction::PrintSystemPrompt { cwd, date })
}

fn parse_resume_args(args: &[String]) -> Result<CliAction, String> {
    let session_path = args
        .first()
        .ok_or_else(|| "chemin de session manquant pour --resume".to_string())
        .map(PathBuf::from)?;
    let commands = args[1..].to_vec();
    if commands
        .iter()
        .any(|command| !command.trim_start().starts_with('/'))
    {
        return Err("les arguments suivant --resume doivent être des commandes slash".to_string());
    }
    Ok(CliAction::ResumeSession {
        session_path,
        commands,
    })
}

fn dump_manifests() {
    let workspace_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let paths = UpstreamPaths::from_workspace_dir(&workspace_dir);
    match extract_manifest(&paths) {
        Ok(manifest) => {
            println!("commandes : {}", manifest.commands.entries().len());
            println!("outils : {}", manifest.tools.entries().len());
            println!("phases d'amorçage : {}", manifest.bootstrap.phases().len());
        }
        Err(error) => {
            eprintln!("échec de l'extraction des manifestes : {error}");
            std::process::exit(1);
        }
    }
}

fn print_bootstrap_plan() {
    for phase in runtime::BootstrapPlan::claw_default().phases() {
        println!("- {phase:?}");
    }
}

fn default_oauth_config() -> OAuthConfig {
    OAuthConfig {
        client_id: String::from("9d1c250a-e61b-44d9-88ed-5944d1962f5e"),
        authorize_url: String::from("https://platform.claw.dev/oauth/authorize"),
        token_url: String::from("https://platform.claw.dev/v1/oauth/token"),
        callback_port: None,
        manual_redirect_url: None,
        scopes: vec![
            String::from("user:profile"),
            String::from("user:inference"),
            String::from("user:sessions:claw_code"),
        ],
    }
}

fn run_login() -> Result<(), Box<dyn std::error::Error>> {
    let cwd = env::current_dir()?;
    let config = ConfigLoader::default_for(&cwd).load()?;
    let default_oauth = default_oauth_config();
    let oauth = config.oauth().unwrap_or(&default_oauth);
    let callback_port = oauth.callback_port.unwrap_or(DEFAULT_OAUTH_CALLBACK_PORT);
    let redirect_uri = runtime::loopback_redirect_uri(callback_port);
    let pkce = generate_pkce_pair()?;
    let state = generate_state()?;
    let authorize_url =
        OAuthAuthorizationRequest::from_config(oauth, redirect_uri.clone(), state.clone(), &pkce)
            .build_url();

    println!("Démarrage de la connexion OAuth Claw…");
    println!("Écoute du callback sur {redirect_uri}");
    if let Err(error) = open_browser(&authorize_url) {
        eprintln!("avertissement : échec de l'ouverture automatique du navigateur : {error}");
        println!("Ouvrez cette URL manuellement :\n{authorize_url}");
    }

    let callback = wait_for_oauth_callback(callback_port)?;
    if let Some(error) = callback.error {
        let description = callback
            .error_description
            .unwrap_or_else(|| "échec de l'autorisation".to_string());
        return Err(io::Error::other(format!("{error}: {description}")).into());
    }
    let code = callback.code.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "le callback n'a pas inclus de code",
        )
    })?;
    let returned_state = callback.state.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "le callback n'a pas inclus d'état",
        )
    })?;
    if returned_state != state {
        return Err(
            io::Error::new(io::ErrorKind::InvalidData, "incohérence de l'état oauth").into(),
        );
    }

    let client = ClawApiClient::from_auth(AuthSource::None).with_base_url(api::read_base_url());
    let exchange_request =
        OAuthTokenExchangeRequest::from_config(oauth, code, state, pkce.verifier, redirect_uri);
    let runtime = tokio::runtime::Runtime::new()?;
    let token_set = runtime.block_on(client.exchange_oauth_code(oauth, &exchange_request))?;
    save_oauth_credentials(&runtime::OAuthTokenSet {
        access_token: token_set.access_token,
        refresh_token: token_set.refresh_token,
        expires_at: token_set.expires_at,
        scopes: token_set.scopes,
    })?;
    println!("Connexion OAuth Claw terminée.");
    Ok(())
}

fn run_logout() -> Result<(), Box<dyn std::error::Error>> {
    clear_oauth_credentials()?;
    println!("Identifiants OAuth Claw effacés.");
    Ok(())
}

fn open_browser(url: &str) -> io::Result<()> {
    let commands = if cfg!(target_os = "macos") {
        vec![("open", vec![url])]
    } else if cfg!(target_os = "windows") {
        vec![("cmd", vec!["/C", "start", "", url])]
    } else {
        vec![("xdg-open", vec![url])]
    };
    for (program, args) in commands {
        match Command::new(program).args(args).spawn() {
            Ok(_) => return Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::NotFound,
        "aucune commande prise en charge pour ouvrir le navigateur n'a été trouvée",
    ))
}

fn wait_for_oauth_callback(
    port: u16,
) -> Result<runtime::OAuthCallbackParams, Box<dyn std::error::Error>> {
    let listener = TcpListener::bind(("127.0.0.1", port))?;
    let (mut stream, _) = listener.accept()?;
    let mut buffer = [0_u8; 4096];
    let bytes_read = stream.read(&mut buffer)?;
    let request = String::from_utf8_lossy(&buffer[..bytes_read]);
    let request_line = request.lines().next().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "ligne de requête de callback manquante",
        )
    })?;
    let target = request_line.split_whitespace().nth(1).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "cible de requête de callback manquante",
        )
    })?;
    let callback = parse_oauth_callback_request_target(target)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    let body = if callback.error.is_some() {
        "Échec de la connexion OAuth Claw. Vous pouvez fermer cette fenêtre."
    } else {
        "Connexion OAuth Claw réussie. Vous pouvez fermer cette fenêtre."
    };
    let response = format!(
        "HTTP/1.1 200 OK\r\ncontent-type: text/plain; charset=utf-8\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    stream.write_all(response.as_bytes())?;
    Ok(callback)
}

fn print_system_prompt(cwd: PathBuf, date: String) {
    let skills = commands::skills_prompt_section(&cwd);
    match load_system_prompt(cwd, date, env::consts::OS, "unknown") {
        Ok(mut sections) => {
            sections.extend(skills);
            println!("{}", sections.join("\n\n"));
        }
        Err(error) => {
            eprintln!("échec de la construction du system prompt : {error}");
            std::process::exit(1);
        }
    }
}

fn print_version() {
    println!("{}", render_version_report());
}

fn resume_session(session_path: &Path, commands: &[String]) {
    let session = match Session::load_from_path(session_path) {
        Ok(session) => session,
        Err(error) => {
            eprintln!("échec de la restauration de session : {error}");
            std::process::exit(1);
        }
    };

    if commands.is_empty() {
        println!(
            "Session restaurée depuis {} ({} messages).",
            session_path.display(),
            session.messages.len()
        );
        return;
    }

    let mut session = session;
    for raw_command in commands {
        let Some(command) = SlashCommand::parse(raw_command) else {
            eprintln!("commande non prise en charge en reprise : {raw_command}");
            std::process::exit(2);
        };
        match run_resume_command(session_path, &session, &command) {
            Ok(ResumeCommandOutcome {
                session: next_session,
                message,
            }) => {
                session = next_session;
                if let Some(message) = message {
                    println!("{message}");
                }
            }
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(2);
            }
        }
    }
}

#[derive(Debug, Clone)]
struct ResumeCommandOutcome {
    session: Session,
    message: Option<String>,
}

#[derive(Debug, Clone)]
struct StatusContext {
    cwd: PathBuf,
    session_path: Option<PathBuf>,
    loaded_config_files: usize,
    discovered_config_files: usize,
    memory_file_count: usize,
    project_root: Option<PathBuf>,
    git_branch: Option<String>,
}

#[derive(Debug, Clone, Copy)]
struct StatusUsage {
    message_count: usize,
    turns: u32,
    latest: TokenUsage,
    cumulative: TokenUsage,
    estimated_tokens: usize,
}

fn format_model_report(model: &str, message_count: usize, turns: u32) -> String {
    let provider = detect_provider_kind(model).label();
    format!(
        "Modèle
  Actuel           {model}
  Fournisseur      {provider}
  Session          {message_count} messages · {turns} tours

Alias
  opus             claude-opus-4-6
  sonnet           claude-sonnet-4-6
  haiku            claude-haiku-4-5-20251001
  vendeur/modèle   n'importe quel modèle OpenRouter (OPENROUTER_API_KEY), ex. qwen/qwen3-coder

Suite
  /models          Modèles gratuits recommandés pour le code
  /model <nom>     Change de modèle pour cette session"
    )
}

fn format_model_switch_report(previous: &str, next: &str, message_count: usize) -> String {
    format!(
        "Modèle changé
  Précédent        {previous}
  Actuel           {next}
  Conservés        {message_count} messages
  Astuce           la conversation en cours est conservée"
    )
}

fn format_permissions_report(mode: &str, rules: &PermissionRules) -> String {
    let modes = [
        (
            "prompt",
            "Lecture libre, demande avant d'éditer ou d'exécuter (défaut)",
        ),
        (
            "read-only",
            "Mode plan : lecture seule, l'agent explore et propose un plan",
        ),
        (
            "workspace-write",
            "Édite sans demander dans le projet, demande avant les commandes",
        ),
        (
            "dont-ask",
            "Ne demande jamais : seuls la lecture et les règles allow passent",
        ),
        (
            "auto",
            "Un classificateur juge les commandes à votre place ; le projet s'édite librement",
        ),
        ("danger-full-access", "Tout est autorisé sans demande"),
    ]
    .into_iter()
    .map(|(name, description)| {
        let marker = if name == mode {
            "● actuel"
        } else {
            "○ dispo"
        };
        format!("  {name:<18} {marker:<9} {description}")
    })
    .collect::<Vec<_>>()
    .join("\n");

    let effect = match mode {
        "prompt" => "Lecture libre ; demande avant chaque modification ou commande",
        "read-only" => "Seuls la lecture, la recherche et les commandes sans effet s'exécutent",
        "workspace-write" => "Les fichiers du projet se modifient sans demander",
        "dont-ask" => "Rien n'est demandé : ce qui n'est pas autorisé d'avance est refusé",
        "auto" => "Lecture et modifications du projet libres ; commandes, chemins protégés et écritures hors du projet jugés par le classificateur",
        "danger-full-access" => "Tous les outils s'exécutent sans demander",
        _ => "Mode de permission inconnu",
    };

    let render_rules = |rules: &[runtime::PermissionRule]| {
        if rules.is_empty() {
            "-".to_string()
        } else {
            rules
                .iter()
                .map(runtime::PermissionRule::as_str)
                .collect::<Vec<_>>()
                .join(", ")
        }
    };

    format!(
        "Permissions
  Mode actif       {mode}
  Effet            {effect}

Modes
{modes}

Règles (les règles deny priment toujours)
  allow            {}
  deny             {}
  ask              {}

Suite
  /permissions <mode>   Change de mode (alias : default, plan, acceptEdits, dontAsk, bypassPermissions)
  /permissions allow|deny|ask|remove <règle>   Ajoute ou retire une règle
  /add-dir <dossier>    Écriture libre aussi dans ce dossier
  Répondre « toujours » à une demande enregistre la règle dans .claw/settings.local.json{}",
        render_rules(&rules.allow),
        render_rules(&rules.deny),
        render_rules(&rules.ask),
        render_auto_denials(),
    )
}

/// Refus récents du mode auto, pour `/permissions`.
fn render_auto_denials() -> String {
    let denials = auto_mode::recent_denials();
    if denials.is_empty() {
        return String::new();
    }
    let mut lines = vec![
        String::new(),
        "Refusés récemment par le mode auto".to_string(),
    ];
    for denial in denials.iter().rev().take(10) {
        lines.push(format!(
            "  {}  [{}] {} : {}",
            denial.at, denial.rule, denial.action, denial.reason
        ));
    }
    lines.push(
        "  Pour en autoriser un : dites-le à Claw avec précision, ajoutez une règle (/permissions allow), ou une exception dans autoMode.allow (~/.claw/settings.json)"
            .to_string(),
    );
    lines.join("\n")
}

fn format_permissions_switch_report(previous: &str, next: &str) -> String {
    format!(
        "Permissions changées
  Mode précédent   {previous}
  Mode actif       {next}
  S'applique à     tous les prochains appels d'outils
  Astuce           /permissions affiche tous les modes"
    )
}

fn format_cost_report(usage: TokenUsage) -> String {
    format!(
        "Coût
  Jetons d'entrée  {}
  Jetons de sortie {}
  Cache (écriture) {}
  Cache (lecture)  {}
  Total            {}

Suite
  /status          Session et espace de travail
  /compact         Résume l'historique si la session devient longue",
        usage.input_tokens,
        usage.output_tokens,
        usage.cache_creation_input_tokens,
        usage.cache_read_input_tokens,
        usage.total_tokens(),
    )
}

/// « 12 300 » : milliers séparés par une espace.
fn group_digits(value: usize) -> String {
    let digits = value.to_string();
    let mut grouped = String::new();
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            grouped.push(' ');
        }
        grouped.push(digit);
    }
    grouped
}

/// Jetons estimés d'un texte (environ quatre caractères par jeton).
fn estimated_tokens(text: &str) -> usize {
    text.len() / 4 + 1
}

/// Titre d'une partie du prompt système : sa première ligne, sans `#`.
fn section_label(section: &str) -> String {
    let first = section
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("(vide)")
        .trim_start_matches('#')
        .trim();
    let label: String = first.chars().take(44).collect();
    if first.chars().count() > 44 {
        format!("{label}…")
    } else {
        label
    }
}

/// Rapport de `/context` : prompt système, outils et conversation, en jetons
/// estimés et en part de la fenêtre du modèle, avec les plus gros morceaux.
fn format_context_report(
    model: &str,
    system_prompt: &[String],
    tools: &[ToolDefinition],
    session: &Session,
) -> String {
    let window = usize::try_from(api::context_window_for_model(model)).unwrap_or(usize::MAX);
    let share = |tokens: usize| -> String {
        if window == 0 {
            return String::new();
        }
        #[allow(clippy::cast_precision_loss)]
        let percent = tokens as f64 * 100.0 / window as f64;
        format!("{percent:.1} %").replace('.', ",")
    };
    let mut sections: Vec<(String, usize)> = system_prompt
        .iter()
        .map(|section| (section_label(section), estimated_tokens(section)))
        .collect();
    let prompt_tokens: usize = sections.iter().map(|(_, tokens)| tokens).sum();
    let mut tool_sizes: Vec<(String, usize)> = tools
        .iter()
        .map(|tool| {
            let schema = serde_json::to_string(&tool.input_schema).unwrap_or_default();
            let text = format!(
                "{}{}{schema}",
                tool.name,
                tool.description.as_deref().unwrap_or("")
            );
            (tool.name.clone(), estimated_tokens(&text))
        })
        .collect();
    let tool_tokens: usize = tool_sizes.iter().map(|(_, tokens)| tokens).sum();
    let conversation_tokens = runtime::estimate_session_tokens(session);
    let total = prompt_tokens + tool_tokens + conversation_tokens;
    let threshold = api::auto_compaction_threshold(model);

    let row = |label: &str, tokens: usize, detail: String| {
        format!(
            "  {label:<16} ≈ {:>9} jetons  {:>7}{detail}",
            group_digits(tokens),
            share(tokens)
        )
    };
    let mut lines = vec![
        format!(
            "Contexte
  Modèle           {model} · fenêtre de {} jetons",
            group_digits(window)
        ),
        row(
            "Prompt système",
            prompt_tokens,
            format!("  · {} partie(s)", sections.len()),
        ),
        row(
            "Outils",
            tool_tokens,
            format!("  · {} outil(s)", tools.len()),
        ),
        row(
            "Conversation",
            conversation_tokens,
            format!("  · {} message(s)", session.messages.len()),
        ),
        row("Total", total, String::new()),
        format!(
            "  Compaction       automatique vers {} jetons{}",
            group_digits(threshold),
            if total >= threshold {
                " (atteinte)"
            } else {
                ""
            }
        ),
    ];
    sections.sort_by(|left, right| right.1.cmp(&left.1));
    tool_sizes.sort_by(|left, right| right.1.cmp(&left.1));
    lines.push(String::new());
    lines.push("Plus grosses parties du prompt".to_string());
    for (label, tokens) in sections.iter().take(5) {
        lines.push(format!("  ≈ {:>7}  {label}", group_digits(*tokens)));
    }
    lines.push("Plus gros outils".to_string());
    for (name, tokens) in tool_sizes.iter().take(5) {
        lines.push(format!("  ≈ {:>7}  {name}", group_digits(*tokens)));
    }
    lines.push(String::new());
    lines.push("Suite".to_string());
    lines.push("  /compact         Résume l'historique pour libérer de la place".to_string());
    lines.push("  /clear           Repart d'une conversation vide".to_string());
    lines.join("\n")
}

/// `/context` pour une session : outils réellement proposés au modèle.
fn context_report_for(
    model: &str,
    system_prompt: &[String],
    allowed_tools: Option<&AllowedToolSet>,
    session: &Session,
) -> String {
    let tools = build_runtime_plugin_state()
        .map(|(_, registry)| filter_tool_specs(&registry, allowed_tools))
        .unwrap_or_default();
    format_context_report(model, system_prompt, &tools, session)
}

fn format_resume_report(session_path: &str, message_count: usize, turns: u32) -> String {
    format!(
        "Session reprise
  Fichier          {session_path}
  Historique       {message_count} messages · {turns} tours
  Suite            /status · /diff · /export"
    )
}

fn format_compact_report(removed: usize, resulting_messages: usize, skipped: bool) -> String {
    if skipped {
        format!(
            "Compaction
  Résultat         ignorée
  Raison           rien à résumer : les derniers messages sont toujours gardés tels quels
  Messages gardés  {resulting_messages}"
        )
    } else {
        format!(
            "Compaction
  Résultat         compactée
  Messages résumés {removed}
  Messages gardés  {resulting_messages}
  Astuce           /status pour voir la session allégée"
        )
    }
}

fn parse_git_status_metadata(status: Option<&str>) -> (Option<PathBuf>, Option<String>) {
    let Some(status) = status else {
        return (None, None);
    };
    let branch = status.lines().next().and_then(|line| {
        line.strip_prefix("## ")
            .map(|line| {
                line.split(['.', ' '])
                    .next()
                    .unwrap_or_default()
                    .to_string()
            })
            .filter(|value| !value.is_empty())
    });
    let project_root = find_git_root().ok();
    (project_root, branch)
}

fn find_git_root() -> Result<PathBuf, Box<dyn std::error::Error>> {
    let output = std::process::Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(env::current_dir()?)
        .output()?;
    if !output.status.success() {
        return Err("pas un dépôt git".into());
    }
    let path = String::from_utf8(output.stdout)?.trim().to_string();
    if path.is_empty() {
        return Err("racine git vide".into());
    }
    Ok(PathBuf::from(path))
}

#[allow(clippy::too_many_lines)]
fn run_resume_command(
    session_path: &Path,
    session: &Session,
    command: &SlashCommand,
) -> Result<ResumeCommandOutcome, Box<dyn std::error::Error>> {
    match command {
        SlashCommand::Help => Ok(ResumeCommandOutcome {
            session: session.clone(),
            message: Some(render_repl_help()),
        }),
        // Sans modèle chargé, le résumé est celui de Claw : la consigne est ignorée.
        SlashCommand::Compact { .. } => {
            let result = runtime::compact_session(
                session,
                CompactionConfig {
                    max_estimated_tokens: 0,
                    ..CompactionConfig::default()
                },
            );
            let removed = result.removed_message_count;
            let kept = result.compacted_session.messages.len();
            let skipped = removed == 0;
            result.compacted_session.save_to_path(session_path)?;
            Ok(ResumeCommandOutcome {
                session: result.compacted_session,
                message: Some(format_compact_report(removed, kept, skipped)),
            })
        }
        SlashCommand::Clear { confirm } => {
            if !confirm {
                return Ok(ResumeCommandOutcome {
                    session: session.clone(),
                    message: Some(
                        "clear : confirmation requise ; relancez avec /clear --confirm".to_string(),
                    ),
                });
            }
            let cleared = Session::new();
            cleared.save_to_path(session_path)?;
            Ok(ResumeCommandOutcome {
                session: cleared,
                message: Some(format!(
                    "Fichier de session vidé : {}.",
                    session_path.display()
                )),
            })
        }
        SlashCommand::Status => {
            let tracker = UsageTracker::from_session(session);
            let usage = tracker.cumulative_usage();
            Ok(ResumeCommandOutcome {
                session: session.clone(),
                message: Some(format_status_report(
                    "restored-session",
                    StatusUsage {
                        message_count: session.messages.len(),
                        turns: tracker.turns(),
                        latest: tracker.current_turn_usage(),
                        cumulative: usage,
                        estimated_tokens: 0,
                    },
                    default_permission_mode().as_str(),
                    &status_context(Some(session_path))?,
                )),
            })
        }
        SlashCommand::Cost => {
            let usage = UsageTracker::from_session(session).cumulative_usage();
            Ok(ResumeCommandOutcome {
                session: session.clone(),
                message: Some(format_cost_report(usage)),
            })
        }
        SlashCommand::Config { section } => Ok(ResumeCommandOutcome {
            session: session.clone(),
            message: Some(render_config_report(section.as_deref())?),
        }),
        SlashCommand::Memory => Ok(ResumeCommandOutcome {
            session: session.clone(),
            message: Some(render_memory_report()?),
        }),
        SlashCommand::Init => Ok(ResumeCommandOutcome {
            session: session.clone(),
            message: Some(init_claw_md()?),
        }),
        SlashCommand::Diff => Ok(ResumeCommandOutcome {
            session: session.clone(),
            message: Some(render_diff_report()?),
        }),
        SlashCommand::Version => Ok(ResumeCommandOutcome {
            session: session.clone(),
            message: Some(render_version_report()),
        }),
        SlashCommand::Export { path } => {
            let export_path = resolve_export_path(path.as_deref(), session)?;
            fs::write(&export_path, render_export_text(session))?;
            Ok(ResumeCommandOutcome {
                session: session.clone(),
                message: Some(format!(
                    "Export\n  Résultat         transcription écrite\n  Fichier          {}\n  Messages         {}",
                    export_path.display(),
                    session.messages.len(),
                )),
            })
        }
        SlashCommand::Agents { args } => {
            let cwd = env::current_dir()?;
            Ok(ResumeCommandOutcome {
                session: session.clone(),
                message: Some(handle_agents_slash_command(args.as_deref(), &cwd)?),
            })
        }
        SlashCommand::Skills { args } => {
            let cwd = env::current_dir()?;
            Ok(ResumeCommandOutcome {
                session: session.clone(),
                message: Some(handle_skills_slash_command(args.as_deref(), &cwd)?),
            })
        }
        SlashCommand::Copy { index } => Ok(ResumeCommandOutcome {
            session: session.clone(),
            message: Some(copy_assistant_reply(session, index.as_deref())),
        }),
        SlashCommand::Rename { title } => {
            let id = session_path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .unwrap_or_default();
            Ok(ResumeCommandOutcome {
                session: session.clone(),
                message: Some(rename_report(id, title.as_deref())),
            })
        }
        SlashCommand::OutputStyle { args } => Ok(ResumeCommandOutcome {
            session: session.clone(),
            message: Some(apply_output_style_command(args.as_deref())?.0),
        }),
        SlashCommand::Schedule { args } => Ok(ResumeCommandOutcome {
            session: session.clone(),
            message: Some(run_schedule_command(args.as_deref(), &default_model())),
        }),
        SlashCommand::InstallGithubAction { args } => Ok(ResumeCommandOutcome {
            session: session.clone(),
            message: Some(run_install_github_action(args.as_deref(), &default_model())),
        }),
        SlashCommand::MemoryImports { allow } => Ok(ResumeCommandOutcome {
            session: session.clone(),
            message: Some(memory_imports_command(*allow)),
        }),
        SlashCommand::AddDir { args } => Ok(ResumeCommandOutcome {
            session: session.clone(),
            message: Some(run_add_dir_command(args.as_deref())),
        }),
        SlashCommand::Bughunter { .. }
        | SlashCommand::Btw { .. }
        | SlashCommand::Recap
        | SlashCommand::Goal { .. }
        | SlashCommand::Loop { .. }
        | SlashCommand::Branch { .. }
        | SlashCommand::Worktree { .. }
        | SlashCommand::CommitPushPr { .. }
        | SlashCommand::Commit
        | SlashCommand::Pr { .. }
        | SlashCommand::Issue { .. }
        | SlashCommand::Ultraplan { .. }
        | SlashCommand::Expert { .. }
        | SlashCommand::Mcp
        | SlashCommand::Undo { .. }
        | SlashCommand::Checkpoints
        | SlashCommand::Hooks
        | SlashCommand::Think { .. }
        | SlashCommand::Team { .. }
        | SlashCommand::Bashes { .. }
        | SlashCommand::Models { .. }
        | SlashCommand::Credits
        | SlashCommand::Doctor
        | SlashCommand::Keybindings
        | SlashCommand::Background { .. }
        | SlashCommand::Fork { .. }
        | SlashCommand::Plan { .. }
        | SlashCommand::Tasks
        | SlashCommand::Stop
        | SlashCommand::Cd { .. }
        | SlashCommand::Subtask { .. }
        | SlashCommand::Rewind
        | SlashCommand::Sandbox { .. }
        | SlashCommand::TerminalSetup
        | SlashCommand::Theme { .. }
        | SlashCommand::Color { .. }
        | SlashCommand::ReloadSkills
        | SlashCommand::ReloadPlugins
        | SlashCommand::ReleaseNotes
        | SlashCommand::Usage
        | SlashCommand::Insights
        | SlashCommand::Context
        | SlashCommand::Teleport { .. }
        | SlashCommand::DebugToolCall
        | SlashCommand::Resume { .. }
        | SlashCommand::Model { .. }
        | SlashCommand::Permissions { .. }
        | SlashCommand::Session { .. }
        | SlashCommand::Plugins { .. }
        | SlashCommand::Unknown(_) => {
            Err("commande non disponible avec --resume (lancez claw pour l'utiliser)".into())
        }
    }
}

#[allow(clippy::too_many_lines)]
fn run_repl(
    model: String,
    allowed_tools: Option<AllowedToolSet>,
    permission_mode: PermissionMode,
    resume: Option<PathBuf>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut cli = LiveCli::new(model, true, allowed_tools, permission_mode)?;
    let mut editor = input::LineEditor::new("> ", slash_command_completion_candidates());
    editor.set_command_help(slash_command_menu_entries());
    transcript::set_interactive(true);
    println!("{}\n", cli.startup_banner());
    // Crédits lus en arrière-plan : la ligne d'état les montre dès qu'ils arrivent.
    if credits::is_openrouter_model(&cli.model) {
        thread::spawn(|| {
            let _ = credits::account_status(false);
        });
    }
    runtime::telemetry::session_started(if resume.is_some() { "resume" } else { "fresh" });
    match resume {
        Some(path) => {
            // La session vide créée au démarrage est remplacée par la session reprise.
            let fresh = cli.session.path.clone();
            cli.resume_session(Some(path.display().to_string()))?;
            if fresh != cli.session.path {
                let _ = fs::remove_file(fresh);
            }
        }
        None => cli.session_hook_start("startup"),
    }

    loop {
        // Messages tapés pendant le tour précédent : traités avant de redemander.
        let outcome = if cli.typeahead.lines.is_empty() {
            if !cli.typeahead.partial.is_empty() {
                editor.set_initial_text(std::mem::take(&mut cli.typeahead.partial));
            }
            editor.set_status(cli.status_line());
            editor.read_line()?
        } else {
            let next = cli.typeahead.lines.remove(0);
            println!("\x1b[38;5;245m> {next}\x1b[0m");
            input::ReadOutcome::Submit(next)
        };
        match outcome {
            input::ReadOutcome::Submit(input) => {
                let trimmed = input.trim();
                if trimmed.is_empty() {
                    continue;
                }
                // « clear » ou « cls » seuls : efface l'écran (la conversation reste).
                if matches!(trimmed, "clear" | "cls") {
                    print!("\x1b[2J\x1b[3J\x1b[H");
                    println!("{}\n", cli.startup_banner());
                    continue;
                }
                if matches!(trimmed, "/exit" | "/quit" | "exit" | "quit") {
                    cli.persist_session()?;
                    cli.session_hook_end("exit");
                    break;
                }
                if let Some(command) = trimmed.strip_prefix('!').map(str::trim) {
                    editor.push_history(&input);
                    if !command.is_empty() {
                        cli.pending_context.push(run_user_shell_command(command));
                    }
                    continue;
                }
                if let Some(note) = trimmed
                    .strip_prefix('#')
                    .filter(|rest| !rest.starts_with('#'))
                {
                    let note = note.trim();
                    if !note.is_empty() {
                        match append_project_note(note).and_then(|path| {
                            cli.refresh_system_prompt()?;
                            Ok(path)
                        }) {
                            Ok(path) => println!("  ⎿  Noté dans {}", path.display()),
                            Err(error) => eprintln!("note impossible : {error}"),
                        }
                    }
                    continue;
                }
                if let Some(command) = SlashCommand::parse(trimmed) {
                    if matches!(command, SlashCommand::Unknown(_)) {
                        if let Some(outcome) = cli.invoke_skill(trimmed) {
                            editor.push_history(&input);
                            if let Err(error) = outcome {
                                eprintln!("{}", render_cli_error(&error.to_string()));
                            }
                            continue;
                        }
                        if let Some(invocation) = custom_command_prompt(trimmed) {
                            editor.push_history(&input);
                            match invocation {
                                Ok(invocation) => {
                                    let model = invocation.command.model.clone();
                                    let tools =
                                        custom_allowed_tools(&invocation.command.allowed_tools);
                                    if let Err(error) = cli.run_turn_with_overrides(
                                        &invocation.prompt,
                                        model,
                                        tools,
                                    ) {
                                        eprintln!("{}", render_cli_error(&error.to_string()));
                                    }
                                }
                                Err(error) => eprintln!("{error}"),
                            }
                            continue;
                        }
                        if let Some(prompt) = tools::mcp_prompt_invocation(trimmed) {
                            editor.push_history(&input);
                            match prompt {
                                Ok(prompt) => {
                                    if let Err(error) = cli.run_turn(&prompt) {
                                        eprintln!("{}", render_cli_error(&error.to_string()));
                                    }
                                }
                                Err(error) => eprintln!("{error}"),
                            }
                            continue;
                        }
                    }
                    if matches!(command, SlashCommand::Rewind) {
                        match cli.rewind_prompt() {
                            Ok(Some(request)) => editor.set_initial_text(request),
                            Ok(None) => {}
                            Err(error) => eprintln!("{}", render_cli_error(&error.to_string())),
                        }
                        continue;
                    }
                    if let SlashCommand::Color { color } = &command {
                        let (message, change) = extra_commands::color_command(color.as_deref());
                        match change {
                            extra_commands::ColorChange::Default => editor.set_rule_color(None),
                            extra_commands::ColorChange::Color(code) => {
                                editor.set_rule_color(Some(code));
                            }
                            extra_commands::ColorChange::Unchanged => {}
                        }
                        println!("  ⎿  {message}");
                        continue;
                    }
                    if matches!(command, SlashCommand::Stop) {
                        let attached = ATTACHED_JOB
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner)
                            .clone();
                        if let Some(id) = attached {
                            STOP_ATTACHED.store(true, std::sync::atomic::Ordering::SeqCst);
                            cli.persist_session()?;
                            cli.session_hook_end("stop");
                            println!("  ⎿  Session {id} arrêtée (conversation et worktree gardés)");
                            break;
                        }
                        println!("  ⎿  Aucune session en arrière-plan n'est ouverte ici : claw stop <id> en arrête une, /exit quitte");
                        continue;
                    }
                    if let SlashCommand::Background { prompt } = &command {
                        match cli.send_to_background(prompt.as_deref()) {
                            Ok(true) => {
                                cli.session_hook_end("background");
                                break;
                            }
                            Ok(false) => {}
                            Err(error) => eprintln!("{}", render_cli_error(&error.to_string())),
                        }
                        continue;
                    }
                    if cli.handle_repl_command(command)? {
                        cli.persist_session()?;
                    }
                    continue;
                }
                editor.push_history(&input);
                // Une requête en échec (quota gratuit, réseau…) ne doit pas fermer la session.
                if let Err(error) = cli.run_turn(&input) {
                    eprintln!("{}", render_cli_error(&error.to_string()));
                } else {
                    cli.continue_goal_after_turn();
                }
            }
            input::ReadOutcome::Cancel => {}
            // Maj+Tab : mode suivant, le texte en cours de saisie est conservé.
            input::ReadOutcome::CycleMode(text) => {
                if let Err(error) = cli.cycle_permission_mode() {
                    eprintln!("{}", render_cli_error(&error.to_string()));
                }
                if !text.is_empty() {
                    editor.set_initial_text(text);
                }
            }
            // Ctrl+O : le dernier tour en entier, puis retour à la saisie.
            input::ReadOutcome::ShowDetails(text) => {
                println!("{}", transcript::turn_details());
                if !text.is_empty() {
                    editor.set_initial_text(text);
                }
            }
            // Échap Échap : revenir avant un message précédent et le corriger.
            input::ReadOutcome::Rewind => match cli.rewind_prompt() {
                Ok(Some(request)) => editor.set_initial_text(request),
                Ok(None) => {}
                Err(error) => eprintln!("{}", render_cli_error(&error.to_string())),
            },
            input::ReadOutcome::Exit => {
                cli.persist_session()?;
                cli.session_hook_end("exit");
                break;
            }
            // Tâche planifiée échue ou sous-tâche finie : elle passe, puis
            // la saisie reprend.
            input::ReadOutcome::Scheduled(text) => {
                cli.deliver_subtasks();
                cli.run_due_tasks();
                if !text.is_empty() {
                    editor.set_initial_text(text);
                }
            }
        }
    }

    Ok(())
}

#[derive(Debug, Clone)]
struct SessionHandle {
    id: String,
    path: PathBuf,
}

#[derive(Debug, Clone)]
struct ManagedSessionSummary {
    id: String,
    path: PathBuf,
    modified_epoch_secs: u64,
    message_count: usize,
}

struct LiveCli {
    /// Sorties de `!commande` à transmettre avec la prochaine demande.
    pending_context: Vec<String>,
    /// Texte tapé pendant le dernier tour (lignes en file d'attente, saisie en cours).
    typeahead: turn_keys::TypeAhead,
    model: String,
    allowed_tools: Option<AllowedToolSet>,
    permission_mode: PermissionMode,
    system_prompt: Vec<String>,
    runtime: ConversationRuntime<DefaultRuntimeClient, CliToolExecutor>,
    session: SessionHandle,
    /// Objectif de `/goal` poursuivi d'un tour à l'autre.
    goal: Option<GoalState>,
    /// Le dernier tour s'est arrêté sur Échap ou Ctrl+C.
    last_turn_interrupted: bool,
}

/// Objectif de `/goal` : après chaque tour, un contrôle dit s'il est atteint ;
/// sinon Claw continue, jusqu'à une limite de tours.
#[derive(Debug, Clone, PartialEq, Eq)]
struct GoalState {
    condition: String,
    rounds: usize,
    last_reason: Option<String>,
    /// Contrôles successifs donnant la même raison (aucun progrès).
    stalls: usize,
    /// En pause (interruption, limite atteinte) : `/goal reprendre` relance.
    paused: bool,
}

impl GoalState {
    fn new(condition: &str) -> Self {
        Self {
            condition: condition.trim().to_string(),
            rounds: 0,
            last_reason: None,
            stalls: 0,
            paused: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum GoalVerdict {
    Met(String),
    NotMet(String),
}

impl LiveCli {
    fn new(
        model: String,
        enable_tools: bool,
        allowed_tools: Option<AllowedToolSet>,
        permission_mode: PermissionMode,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let system_prompt = build_system_prompt()?;
        let session = create_managed_session_handle()?;
        let runtime = build_runtime(
            Session::new(),
            model.clone(),
            system_prompt.clone(),
            enable_tools,
            true,
            allowed_tools.clone(),
            permission_mode,
            None,
        )?;
        let cli = Self {
            pending_context: Vec::new(),
            typeahead: turn_keys::TypeAhead::default(),
            model,
            allowed_tools,
            permission_mode,
            system_prompt,
            runtime,
            session,
            goal: None,
            last_turn_interrupted: false,
        };
        cli.persist_session()?;
        session_bound(&cli.session.path);
        Ok(cli)
    }

    /// Hooks `SessionStart` : leur sortie sera ajoutée au prochain tour.
    fn session_hook_start(&self, source: &str) {
        print_hook_feedback(
            runtime::HookEvent::SessionStart,
            &self.runtime.hooks().session_start(source),
        );
    }

    fn session_hook_end(&self, reason: &str) {
        print_hook_feedback(
            runtime::HookEvent::SessionEnd,
            &self.runtime.hooks().session_end(reason),
        );
    }

    /// Encadré d'accueil à la Claude Code, puis une astuce.
    fn startup_banner(&self) -> String {
        let color = io::stdout().is_terminal();
        let cwd = env::current_dir().ok();
        let cwd_display = cwd.as_ref().map_or_else(
            || "<inconnu>".to_string(),
            |path| path.display().to_string(),
        );
        let git_branch = status_context(Some(&self.session.path))
            .ok()
            .and_then(|context| context.git_branch);
        let has_claw_md = cwd
            .as_ref()
            .is_some_and(|path| path.join("CLAW.md").is_file());
        let width = transcript::terminal_width().saturating_sub(1).clamp(40, 76);
        let room = width.saturating_sub(16);
        let (accent, bold, dim, reset) = (
            transcript::ACCENT,
            transcript::BOLD,
            transcript::DIM,
            transcript::RESET,
        );
        let mut rows = vec![
            format!("{accent}✻{reset} Bienvenue dans {bold}Claw Code{reset} !"),
            String::new(),
            format!("  {dim}/help pour l'aide, /status pour l'état de la session{reset}"),
            String::new(),
            format!(
                "  {dim}dossier :{reset} {}",
                transcript::clip_start(&cwd_display, room)
            ),
        ];
        if let Some(branch) = git_branch {
            rows.push(format!(
                "  {dim}branche :{reset} {}",
                transcript::clip(&branch, room)
            ));
        }
        rows.push(format!(
            "  {dim}modèle  :{reset} {}",
            transcript::clip(&self.model, room)
        ));
        let mut lines = transcript::boxed(&rows, width, accent);
        if let Some(announcement) = cwd
            .as_ref()
            .and_then(|cwd| ConfigLoader::default_for(cwd).load().ok())
            .and_then(|config| runtime::settings::company_announcement(&config))
        {
            lines.push(format!(" {accent}※{reset} {announcement}"));
        }
        lines.push(String::new());
        if has_claw_md {
            lines.push(format!(
                " {dim}※ Astuce : Maj+Tab change de mode · Échap interrompt · Maj+Entrée ajoute une ligne{reset}"
            ));
        } else {
            lines.extend([
                " Pour bien commencer :".to_string(),
                String::new(),
                format!(" 1. Lancez {bold}/init{reset} pour créer CLAW.md, les consignes du projet"),
                " 2. Décrivez une tâche : Claw lit, modifie, lance les commandes et vérifie"
                    .to_string(),
                " 3. Soyez aussi précis qu'avec un collègue pour de meilleurs résultats"
                    .to_string(),
                format!(
                    " {dim}※ Maj+Tab change de mode · Échap interrompt · Maj+Entrée ajoute une ligne{reset}"
                ),
            ]);
        }
        let banner = lines.join("\n");
        if color {
            banner
        } else {
            transcript::strip_ansi(&banner)
        }
    }

    fn run_turn(&mut self, input: &str) -> Result<(), Box<dyn std::error::Error>> {
        let started = Instant::now();
        self.last_turn_interrupted = false;
        transcript::begin_turn();
        live::start("Réflexion…");
        let mut permission_prompter = CliPermissionPrompter::new(self.permission_mode);
        self.share_auto_context(input);
        let (mut text, images) = take_image_attachments(input);
        if !self.pending_context.is_empty() {
            text = format!("{}\n\n{text}", self.pending_context.join("\n\n"));
            self.pending_context.clear();
        }
        // Échap interrompt, le texte tapé pendant le tour est conservé.
        let keys = turn_keys::TurnKeyListener::start();
        self.runtime.autosave_to(&self.session.path);
        let result =
            self.runtime
                .run_turn_with_images(text, images, Some(&mut permission_prompter));
        let typed = keys.finish();
        live::stop();
        transcript::flush_calls();
        self.typeahead.lines.extend(typed.lines);
        if !typed.partial.is_empty() {
            self.typeahead.partial = typed.partial;
        }
        match result {
            Ok(_) => {
                self.print_turn_footer(started.elapsed());
                self.persist_session()?;
                self.sync_permission_mode()?;
                Ok(())
            }
            Err(error) if runtime::interrupt::is_interruption(&error.to_string()) => {
                // Un refus s'accroche sous l'appel refusé ; une interruption
                // (Échap pendant la réponse) se détache du texte.
                let (gap, message) = if permission_prompter.refused {
                    ("", "Refusé · dites à Claw comment continuer")
                } else {
                    ("\n", "Interrompu · que doit faire Claw maintenant ?")
                };
                println!(
                    "{gap}  ⎿  {}{message}{}\n",
                    transcript::RED,
                    transcript::RESET
                );
                self.last_turn_interrupted = true;
                self.persist_session()?;
                Ok(())
            }
            Err(error) => Err(Box::new(error)),
        }
    }

    /// `✻ Terminé en 12 s · OpenRouter : 8,25 $ restants · …` après chaque
    /// réponse (`CLAW_CREDITS=0` masque les crédits).
    fn print_turn_footer(&self, elapsed: Duration) {
        let mut parts = vec![format!("Terminé en {}", format_elapsed(elapsed))];
        if credits::is_openrouter_model(&self.model)
            && !env::var("CLAW_CREDITS").is_ok_and(|value| value.trim() == "0")
        {
            if let Some(line) = credits::current_line(true) {
                parts.push(format!("OpenRouter : {line}"));
            }
        }
        println!(
            "\n{}✻ {}{}\n",
            transcript::DIM,
            parts.join(" · "),
            transcript::RESET
        );
        // Quota gratuit bas : dit une fois par seuil (100, 20, 0).
        if let Some(warning) = credits::low_quota_warning() {
            println!("{}▲ {warning}{}\n", transcript::RED, transcript::RESET);
        }
    }

    /// Ligne d'état sous la saisie : celle de la commande `statusLine` des
    /// réglages s'il y en a une, sinon la ligne intégrée.
    fn status_line(&self) -> String {
        if let Some(command) = statusline::load_command() {
            if let Some(line) = statusline::run(&command, &self.status_input()) {
                return line;
            }
        }
        self.builtin_status_line()
    }

    /// Ce que reçoit la commande de ligne d'état.
    fn status_input(&self) -> statusline::StatusInput {
        let window = api::context_window_for_model(&self.model);
        let used = self.runtime.estimated_context_tokens();
        let cost = models::estimated_cost_usd(&self.model, self.runtime.usage().cumulative_usage());
        let output_style = env::current_dir()
            .ok()
            .and_then(|cwd| ConfigLoader::default_for(cwd).load().ok())
            .and_then(|config| {
                config
                    .get(commands::output_styles::SETTING_KEY)
                    .and_then(|value| value.as_str().map(ToOwned::to_owned))
            });
        statusline::StatusInput {
            session_id: self.session.id.clone(),
            transcript_path: self.session.path.display().to_string(),
            cwd: env::current_dir()
                .map(|cwd| cwd.display().to_string())
                .unwrap_or_default(),
            model: self.model.clone(),
            permission_mode: self.permission_mode.as_str().to_string(),
            output_style,
            context_used_percent: usize::try_from(window)
                .ok()
                .filter(|window| *window > 0)
                .map(|window| (used * 100 / window).min(100)),
            context_window: Some(window).filter(|window| *window > 0),
            total_cost_usd: cost,
            goal: self.goal.as_ref().map(|goal| goal.condition.clone()),
            version: VERSION.to_string(),
        }
    }

    fn builtin_status_line(&self) -> String {
        let mode = match self.permission_mode {
            PermissionMode::Prompt => "⏵ demander avant d'agir",
            PermissionMode::WorkspaceWrite => "⏵⏵ éditions acceptées",
            PermissionMode::ReadOnly => "‖ mode plan",
            PermissionMode::DangerFullAccess | PermissionMode::Allow => "▲ accès complet",
            PermissionMode::DontAsk => "⊘ sans demande (règles seules)",
            PermissionMode::Auto if auto_mode::paused() => {
                "⏵⏵ mode auto (en pause : Claw redemande)"
            }
            PermissionMode::Auto => "⏵⏵ mode auto",
        };
        let mut line = format!("  {mode} (Maj+Tab) · {}", self.model);
        let window = usize::try_from(api::context_window_for_model(&self.model)).unwrap_or(0);
        if window > 0 {
            let used = self.runtime.estimated_context_tokens();
            let _ = write!(line, " · contexte {} %", (used * 100 / window).min(100));
        }
        if credits::is_openrouter_model(&self.model) {
            if let Some(credits) = credits::cached_line() {
                line.push_str(" · ");
                line.push_str(&credits);
            }
        }
        let scheduled = runtime::cron::list().len();
        if scheduled > 0 {
            let plural = if scheduled > 1 { "s" } else { "" };
            let _ = write!(line, " · ↻ {scheduled} tâche{plural} planifiée{plural}");
        }
        let waiting = jobs::needing_input();
        if waiting > 0 {
            let _ = write!(
                line,
                " · ✻ {waiting} agent{} en attente (claw agents)",
                if waiting > 1 { "s" } else { "" }
            );
        }
        if let Some(goal) = &self.goal {
            let state = if goal.paused { "en pause" } else { "actif" };
            let _ = write!(
                line,
                " · ◎ objectif {state} : {}",
                transcript::clip(&goal.condition, 40)
            );
        }
        line
    }

    /// Échap Échap : liste des derniers messages ; celui choisi est annulé
    /// (conversation et fichiers) et revient dans la saisie pour être corrigé.
    fn rewind_prompt(&mut self) -> Result<Option<String>, Box<dyn std::error::Error>> {
        let turns = runtime::checkpoint::list();
        if turns.is_empty() {
            println!(
                "  ⎿  {}Aucun message précédent à reprendre.{}",
                transcript::DIM,
                transcript::RESET
            );
            return Ok(None);
        }
        let recent: Vec<String> = turns
            .iter()
            .rev()
            .take(9)
            .map(|turn| turn.request.clone())
            .collect();
        let options: Vec<String> = recent
            .iter()
            .enumerate()
            .map(|(index, request)| {
                let first_line = request.lines().next().unwrap_or_default();
                let preview = transcript::clip(first_line, 70);
                if index == 0 {
                    format!("{preview}  (dernier message)")
                } else {
                    format!("{preview}  (il y a {} messages)", index + 1)
                }
            })
            .collect();
        let body = vec![format!(
            "{}La conversation et les fichiers modifiés par Claw reviennent à l'état d'avant ce message.{}",
            transcript::DIM,
            transcript::RESET
        )];
        let answer = approval::ask(
            "Revenir en arrière",
            &body,
            "Reprendre à partir de quel message ?",
            &options,
        )?;
        let approval::Answer::Pick(index) = answer else {
            return Ok(None);
        };
        let report = undo_turns(&mut self.runtime, Some(&(index + 1).to_string()))?;
        println!("{}{report}{}", transcript::DIM, transcript::RESET);
        self.persist_session()?;
        Ok(recent.get(index).cloned())
    }

    /// Maj+Tab : demander → éditions acceptées → plan → demander.
    fn cycle_permission_mode(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let next = match self.permission_mode {
            PermissionMode::Prompt => PermissionMode::WorkspaceWrite,
            PermissionMode::WorkspaceWrite => PermissionMode::ReadOnly,
            PermissionMode::ReadOnly if !auto_mode::disabled() => PermissionMode::Auto,
            _ => PermissionMode::Prompt,
        };
        if next == PermissionMode::Auto {
            auto_mode::resume();
        }
        let session = self.runtime.session().clone();
        self.runtime = build_runtime(
            session,
            self.model.clone(),
            self.system_prompt.clone(),
            true,
            true,
            self.allowed_tools.clone(),
            next,
            None,
        )?;
        self.permission_mode = next;
        Ok(())
    }

    fn run_turn_with_output(
        &mut self,
        input: &str,
        output_format: CliOutputFormat,
    ) -> Result<(), Box<dyn std::error::Error>> {
        match output_format {
            CliOutputFormat::Text => self.run_turn(input),
            CliOutputFormat::Json => self.run_prompt_json(input),
            CliOutputFormat::StreamJson => self.run_prompt_stream_json(input),
        }
    }

    /// Charge une session existante sans rapport à l'écran (mode stream-json).
    fn load_session_quietly(&mut self, path: &Path) -> Result<(), Box<dyn std::error::Error>> {
        let handle = resolve_session_reference(&path.display().to_string())?;
        let session = Session::load_from_path(&handle.path)?;
        self.runtime = build_runtime(
            session,
            self.model.clone(),
            self.system_prompt.clone(),
            true,
            false,
            self.allowed_tools.clone(),
            self.permission_mode,
            None,
        )?;
        self.session = handle;
        session_bound(&self.session.path);
        Ok(())
    }

    /// Un tour en `--output-format stream-json` : événements au fil de l'eau
    /// puis une ligne `result`.
    fn run_prompt_stream_json(&mut self, input: &str) -> Result<(), Box<dyn std::error::Error>> {
        let started = std::time::Instant::now();
        let observer = Arc::new(stream_json::StreamJsonObserver::new(&self.session.id));
        let shared: SharedObserver = observer.clone();
        let mut runtime = build_runtime_with_ui(
            self.runtime.session().clone(),
            self.model.clone(),
            self.system_prompt.clone(),
            true,
            false,
            self.allowed_tools.clone(),
            self.permission_mode,
            None,
            Some(RuntimeUi {
                observer: shared,
                subagent_prompter: Arc::new(|| {
                    Box::new(stream_json::StreamJsonPrompter) as tools::SubagentPrompter
                }),
                model_prompter: None,
                question_asker: None,
            }),
        )?;
        stream_json::emit(&json!({
            "type": "system",
            "subtype": "init",
            "session_id": self.session.id,
            "model": self.model,
            "cwd": env::current_dir().unwrap_or_default().display().to_string(),
            "permission_mode": self.permission_mode.as_str(),
        }));
        let (text, images) = take_image_attachments(input);
        runtime.autosave_to(&self.session.path);
        let result =
            runtime.run_turn_with_images(text, images, Some(&mut stream_json::StreamJsonPrompter));
        observer.flush();
        let elapsed = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
        match result {
            Ok(summary) => {
                stream_json::emit(&json!({
                    "type": "result",
                    "subtype": "success",
                    "session_id": self.session.id,
                    "result": final_assistant_text(&summary),
                    "num_turns": summary.iterations,
                    "duration_ms": elapsed,
                    "model": self.model,
                    "usage": {
                        "input_tokens": summary.usage.input_tokens,
                        "output_tokens": summary.usage.output_tokens,
                    },
                    "cost": models::estimated_cost(&self.model, summary.usage),
                }));
            }
            Err(error) => {
                stream_json::emit(&json!({
                    "type": "result",
                    "subtype": "error",
                    "session_id": self.session.id,
                    "is_error": true,
                    "error": error.to_string(),
                    "duration_ms": elapsed,
                }));
            }
        }
        self.runtime = runtime;
        self.persist_session()?;
        Ok(())
    }

    fn run_prompt_json(&mut self, input: &str) -> Result<(), Box<dyn std::error::Error>> {
        let session = self.runtime.session().clone();
        let mut runtime = build_runtime(
            session,
            self.model.clone(),
            self.system_prompt.clone(),
            true,
            false,
            self.allowed_tools.clone(),
            self.permission_mode,
            None,
        )?;
        let mut permission_prompter = CliPermissionPrompter::new(self.permission_mode);
        let (text, images) = take_image_attachments(input);
        runtime.autosave_to(&self.session.path);
        start_collecting_notices();
        let result = runtime.run_turn_with_images(text, images, Some(&mut permission_prompter));
        let notices = take_notices();
        let summary = match result {
            Ok(summary) => summary,
            Err(error) => {
                // Un tour en échec rend quand même son JSON : l'erreur et ce
                // que Claw a signalé avant (essais après une limite...).
                println!(
                    "{}",
                    json!({ "error": error.to_string(), "model": self.model, "notices": notices })
                );
                return Err(Box::new(error));
            }
        };
        self.runtime = runtime;
        self.persist_session()?;
        println!(
            "{}",
            json!({
                "message": final_assistant_text(&summary),
                "model": self.model,
                "iterations": summary.iterations,
                "notices": notices,
                "tool_uses": collect_tool_uses(&summary),
                "tool_results": collect_tool_results(&summary),
                "usage": {
                    "input_tokens": summary.usage.input_tokens,
                    "output_tokens": summary.usage.output_tokens,
                    "cache_creation_input_tokens": summary.usage.cache_creation_input_tokens,
                    "cache_read_input_tokens": summary.usage.cache_read_input_tokens,
                }
            })
        );
        Ok(())
    }

    #[allow(clippy::too_many_lines)]
    fn handle_repl_command(
        &mut self,
        command: SlashCommand,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        Ok(match command {
            SlashCommand::Help => {
                println!("{}", render_repl_help());
                false
            }
            SlashCommand::Status => {
                self.print_status();
                false
            }
            SlashCommand::Bughunter { scope } => {
                self.run_bughunter(scope.as_deref())?;
                false
            }
            SlashCommand::Commit => {
                self.run_commit()?;
                true
            }
            SlashCommand::Pr { context } => {
                self.run_pr(context.as_deref())?;
                false
            }
            SlashCommand::Issue { context } => {
                self.run_issue(context.as_deref())?;
                false
            }
            SlashCommand::Ultraplan { task } => {
                self.run_ultraplan(task.as_deref())?;
                false
            }
            SlashCommand::Expert { command, args } => {
                match expert_command_prompt(command, args.as_deref()) {
                    Ok(prompt) => self.run_turn(&prompt)?,
                    Err(usage) => println!("{usage}"),
                }
                false
            }
            SlashCommand::Mcp => {
                let config = ConfigLoader::default_for(env::current_dir()?).load()?;
                println!("{}", tools::mcp_status_report(&config));
                false
            }
            SlashCommand::Undo { count } => {
                match undo_turns(&mut self.runtime, count.as_deref()) {
                    Ok(report) => {
                        self.persist_session()?;
                        println!("{report}");
                    }
                    Err(error) => println!("{error}"),
                }
                true
            }
            SlashCommand::Checkpoints => {
                println!("{}", render_checkpoints());
                false
            }
            SlashCommand::Hooks => {
                println!("{}", render_hooks_report(self.runtime.hooks().config()));
                false
            }
            SlashCommand::Think { level } => {
                // Niveaux de Claude Code (/effort) : xhigh, auto, status.
                let level = level.map(|level| level.trim().to_lowercase());
                match level.as_deref() {
                    Some("auto") => {
                        api::set_reasoning_effort(None);
                        println!("Raisonnement : auto (le réglage du modèle)");
                    }
                    Some("status") => println!("{}", apply_think_command(None)),
                    Some("xhigh") => println!("{}", apply_think_command(Some("max"))),
                    other => println!("{}", apply_think_command(other)),
                }
                false
            }
            SlashCommand::Team { args } => {
                println!(
                    "{}",
                    apply_team_command(args.as_deref(), self.model.as_str())
                );
                false
            }
            SlashCommand::Bashes { args } => {
                println!("{}", render_background_tasks(args.as_deref()));
                false
            }
            SlashCommand::Models { args } => {
                match models::models_report(args.as_deref(), &self.model) {
                    Ok(report) => println!("{report}"),
                    Err(error) => eprintln!("{}", render_cli_error(&error)),
                }
                false
            }
            SlashCommand::Credits => {
                println!("{}", credits::report(true));
                false
            }
            SlashCommand::Doctor => {
                println!("{}", doctor::report(&self.model, &version_summary()));
                false
            }
            SlashCommand::Keybindings => {
                println!("{}", keybindings_command());
                false
            }
            SlashCommand::Plan { task } => {
                self.set_permissions(Some("read-only".to_string()))?;
                if let Some(task) = task.filter(|task| !task.trim().is_empty()) {
                    self.run_turn(&task)?;
                    return Ok(true);
                }
                false
            }
            SlashCommand::Tasks => {
                Self::print_tasks();
                false
            }
            SlashCommand::Cd { path } => {
                if let Err(error) = self.change_directory(path.as_deref()) {
                    eprintln!("{}", render_cli_error(&error.to_string()));
                }
                false
            }
            SlashCommand::Subtask { task } => {
                match task.filter(|task| !task.trim().is_empty()) {
                    Some(task) => {
                        let transcript =
                            extra_commands::transcript_text(self.runtime.session(), 12_000);
                        let id = extra_commands::spawn_subtask(&task, &transcript, &self.model);
                        println!("  ⎿  Sous-tâche {id} lancée en arrière-plan : son résultat reviendra ici (/tasks pour la suivre)");
                    }
                    None => println!("Usage : /subtask <tâche>"),
                }
                false
            }
            SlashCommand::Sandbox { args } => {
                println!("{}", sandbox_command(args.as_deref()));
                false
            }
            SlashCommand::TerminalSetup => {
                println!("{}", extra_commands::terminal_setup());
                false
            }
            SlashCommand::Theme { name } => {
                println!("{}", extra_commands::theme_command(name.as_deref()));
                false
            }
            SlashCommand::ReloadSkills => {
                self.refresh_system_prompt()?;
                let count = env::current_dir()
                    .map(|cwd| commands::skill_registry::installed_skills(&cwd).len())
                    .unwrap_or(0);
                println!("  ⎿  Skills relus : {count} disponible(s)");
                false
            }
            SlashCommand::ReloadPlugins => {
                self.reload_runtime_features()?;
                self.refresh_system_prompt()?;
                println!("  ⎿  Plugins, hooks, serveurs MCP et réglages rechargés");
                false
            }
            SlashCommand::ReleaseNotes => {
                println!("{}", extra_commands::release_notes());
                false
            }
            SlashCommand::Usage => {
                self.print_cost();
                if credits::is_openrouter_model(&self.model) {
                    println!("{}", credits::report(false));
                }
                let mut activity = extra_commands::Activity::default();
                activity.add(self.runtime.session());
                println!("Activité de la session\n{}", activity.summary());
                false
            }
            SlashCommand::Insights => {
                match extra_commands::insights(&env::current_dir()?) {
                    Ok((_, summary)) => println!("{summary}"),
                    Err(error) => println!("  ⎿  {error}"),
                }
                false
            }
            SlashCommand::Fork { prompt } => {
                if let Err(error) = self.fork_to_background(prompt.as_deref()) {
                    eprintln!("{}", render_cli_error(&error.to_string()));
                }
                false
            }
            // Traitées par la boucle du terminal (saisie, sortie, session libérée).
            SlashCommand::Background { .. }
            | SlashCommand::Rewind
            | SlashCommand::Stop
            | SlashCommand::Color { .. } => false,
            SlashCommand::Teleport { target } => {
                self.run_teleport(target.as_deref())?;
                false
            }
            SlashCommand::DebugToolCall => {
                self.run_debug_tool_call()?;
                false
            }
            SlashCommand::Compact { instructions } => {
                self.compact(instructions.as_deref())?;
                false
            }
            SlashCommand::Btw { question } => {
                self.run_side_question(question.as_deref());
                false
            }
            SlashCommand::Recap => {
                self.run_recap();
                false
            }
            SlashCommand::Copy { index } => {
                println!(
                    "{}",
                    copy_assistant_reply(self.runtime.session(), index.as_deref())
                );
                false
            }
            SlashCommand::Rename { title } => {
                self.rename_session(title.as_deref());
                false
            }
            SlashCommand::Goal { args } => {
                self.handle_goal_command(args.as_deref())?;
                true
            }
            SlashCommand::Loop { args } => {
                self.run_loop(args.as_deref())?;
                true
            }
            SlashCommand::OutputStyle { args } => {
                match apply_output_style_command(args.as_deref()) {
                    Ok((report, changed)) => {
                        if changed {
                            self.refresh_system_prompt()?;
                        }
                        println!("{report}");
                    }
                    Err(error) => eprintln!("{}", render_cli_error(&error.to_string())),
                }
                false
            }
            SlashCommand::Schedule { args } => {
                println!("{}", run_schedule_command(args.as_deref(), &self.model));
                false
            }
            SlashCommand::AddDir { args } => {
                println!("{}", run_add_dir_command(args.as_deref()));
                false
            }
            SlashCommand::MemoryImports { allow } => {
                println!("{}", memory_imports_command(allow));
                if allow.is_some() {
                    self.refresh_system_prompt()?;
                }
                false
            }
            SlashCommand::InstallGithubAction { args } => {
                println!(
                    "{}",
                    run_install_github_action(args.as_deref(), &self.model)
                );
                false
            }
            SlashCommand::Model { model } => self.set_model(model)?,
            SlashCommand::Permissions { mode } => self.set_permissions(mode)?,
            SlashCommand::Clear { confirm } => self.clear_session(confirm)?,
            SlashCommand::Cost => {
                self.print_cost();
                false
            }
            SlashCommand::Context => {
                println!(
                    "{}",
                    context_report_for(
                        &self.model,
                        &self.system_prompt,
                        self.allowed_tools.as_ref(),
                        self.runtime.session(),
                    )
                );
                false
            }
            SlashCommand::Resume { session_path } => self.resume_session(session_path)?,
            SlashCommand::Config { section } => {
                Self::print_config(section.as_deref())?;
                false
            }
            SlashCommand::Memory => {
                Self::print_memory()?;
                false
            }
            SlashCommand::Init => {
                run_init()?;
                println!("Analyse du projet pour compléter CLAW.md…");
                let prompt = INIT_ANALYSIS_PROMPT.to_string();
                if let Err(error) = self.run_turn(&prompt) {
                    eprintln!("{}", render_cli_error(&error.to_string()));
                }
                self.refresh_system_prompt()?;
                true
            }
            SlashCommand::Diff => {
                Self::print_diff()?;
                false
            }
            SlashCommand::Version => {
                Self::print_version();
                false
            }
            SlashCommand::Export { path } => {
                self.export_session(path.as_deref())?;
                false
            }
            SlashCommand::Session { action, target } => {
                self.handle_session_command(action.as_deref(), target.as_deref())?
            }
            SlashCommand::Plugins { action, target } => {
                self.handle_plugins_command(action.as_deref(), target.as_deref())?
            }
            SlashCommand::Agents { args } => {
                Self::print_agents(args.as_deref())?;
                false
            }
            SlashCommand::Skills { args } => {
                Self::print_skills(args.as_deref())?;
                false
            }
            SlashCommand::Branch { action, target } => {
                println!(
                    "{}",
                    git_work::branch_command(
                        &env::current_dir()?,
                        action.as_deref(),
                        target.as_deref()
                    )
                );
                false
            }
            SlashCommand::Worktree {
                action,
                path,
                branch,
            } => {
                println!(
                    "{}",
                    git_work::worktree_command(
                        &env::current_dir()?,
                        action.as_deref(),
                        path.as_deref(),
                        branch.as_deref()
                    )
                );
                false
            }
            SlashCommand::CommitPushPr { context } => {
                if let Err(error) = self.run_commit_push_pr(context.as_deref()) {
                    eprintln!("{}", render_cli_error(&error.to_string()));
                }
                true
            }
            SlashCommand::Unknown(name) => {
                eprintln!("{}", render_unknown_repl_command(&name));
                false
            }
        })
    }

    fn persist_session(&self) -> Result<(), Box<dyn std::error::Error>> {
        self.runtime.session().save_to_path(&self.session.path)?;
        Ok(())
    }

    fn print_status(&self) {
        // Un fichier de réglages devenu invalide en cours de session ne doit
        // pas faire tomber le terminal : l'erreur est affichée.
        let context = match status_context(Some(&self.session.path)) {
            Ok(context) => context,
            Err(error) => {
                eprintln!("État indisponible : {error}");
                return;
            }
        };
        let cumulative = self.runtime.usage().cumulative_usage();
        let latest = self.runtime.usage().current_turn_usage();
        println!(
            "{}",
            format_status_report(
                &self.model,
                StatusUsage {
                    message_count: self.runtime.session().messages.len(),
                    turns: self.runtime.usage().turns(),
                    latest,
                    cumulative,
                    estimated_tokens: self.runtime.estimated_tokens(),
                },
                self.permission_mode.as_str(),
                &context,
            )
        );
    }

    fn set_model(&mut self, model: Option<String>) -> Result<bool, Box<dyn std::error::Error>> {
        let Some(model) = model else {
            println!(
                "{}",
                format_model_report(
                    &self.model,
                    self.runtime.session().messages.len(),
                    self.runtime.usage().turns(),
                )
            );
            return Ok(false);
        };

        let model = resolve_model_alias(&model);

        if model == self.model {
            println!(
                "{}",
                format_model_report(
                    &self.model,
                    self.runtime.session().messages.len(),
                    self.runtime.usage().turns(),
                )
            );
            return Ok(false);
        }

        let previous = self.model.clone();
        let session = self.runtime.session().clone();
        let message_count = session.messages.len();
        self.runtime = build_runtime(
            session,
            model.clone(),
            self.system_prompt.clone(),
            true,
            true,
            self.allowed_tools.clone(),
            self.permission_mode,
            None,
        )?;
        self.model.clone_from(&model);
        println!(
            "{}",
            format_model_switch_report(&previous, &model, message_count)
        );
        Ok(true)
    }

    /// Tour avec le modèle et les outils d'une commande personnalisée, puis
    /// retour au réglage de la session.
    fn run_turn_with_overrides(
        &mut self,
        prompt: &str,
        model: Option<String>,
        allowed_tools: Option<AllowedToolSet>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if model.is_none() && allowed_tools.is_none() {
            return self.run_turn(prompt);
        }
        let saved = (self.model.clone(), self.allowed_tools.clone());
        if let Some(model) = model {
            self.model = resolve_model_alias(&model);
        }
        if allowed_tools.is_some() {
            self.allowed_tools = allowed_tools;
        }
        self.reload_runtime_features()?;
        let result = self.run_turn(prompt);
        self.model = saved.0;
        self.allowed_tools = saved.1;
        self.reload_runtime_features()?;
        result
    }

    /// Prompt système reconstruit (après `#note` ou une modification de CLAW.md).
    fn refresh_system_prompt(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        self.system_prompt = build_system_prompt()?;
        self.reload_runtime_features()
    }

    /// Le mode a changé pendant le tour (plan approuvé avec `ExitPlanMode`) :
    /// la session continue dans ce mode, sans les consignes du mode plan.
    fn sync_permission_mode(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let active = self.runtime.permission_policy().active_mode();
        if active == self.permission_mode {
            return Ok(());
        }
        self.set_permissions(Some(active.as_str().to_string()))?;
        Ok(())
    }

    /// Ajoute ou retire une règle de `.claw/settings.local.json`, puis
    /// recharge les permissions de la session.
    fn edit_rule(
        &mut self,
        list: Option<&str>,
        rule: &str,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        if runtime::PermissionRule::parse(rule).is_none() {
            println!(
                "Règle illisible : {rule}
  Exemples         Bash(npm test:*) · Edit(src/**) · WebFetch(domain:docs.rs) · mcp__serveur"
            );
            return Ok(false);
        }
        let cwd = env::current_dir()?;
        let (path, changed) = edit_permission_rule(&cwd, list, rule)?;
        self.reload_runtime_features()?;
        let action = match (list, changed) {
            (Some(list), true) => format!("ajoutée à {list}"),
            (Some(list), false) => format!("déjà dans {list}"),
            (None, true) => "retirée".to_string(),
            (None, false) => {
                "absente de ce fichier (elle vient peut-être de .claw/settings.json ou de ~/.claw)"
                    .to_string()
            }
        };
        println!(
            "Permissions
  Règle            {rule}
  Résultat         {action}
  Fichier          {}",
            path.display()
        );
        Ok(false)
    }

    fn set_permissions(
        &mut self,
        mode: Option<String>,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        let Some(mode) = mode else {
            println!(
                "{}",
                format_permissions_report(
                    self.permission_mode.as_str(),
                    self.runtime.permission_policy().rules()
                )
            );
            return Ok(false);
        };
        // `/permissions allow|deny|ask|remove <règle>` : règles du projet.
        if let Some((list, rule)) = parse_rule_command(&mode) {
            return self.edit_rule(list, rule);
        }
        if matches!(mode.trim(), "rules" | "regles" | "règles" | "list") {
            println!(
                "{}",
                format_permissions_report(
                    self.permission_mode.as_str(),
                    self.runtime.permission_policy().rules()
                )
            );
            return Ok(false);
        }

        let normalized = normalize_permission_mode(&mode).ok_or_else(|| {
            format!(
                "mode de permission non pris en charge « {mode} ». Utilisez prompt, read-only (plan), workspace-write ou danger-full-access."
            )
        })?;

        if normalized == self.permission_mode.as_str() {
            println!(
                "{}",
                format_permissions_report(normalized, self.runtime.permission_policy().rules())
            );
            return Ok(false);
        }

        let previous = self.permission_mode.as_str().to_string();
        let session = self.runtime.session().clone();
        self.permission_mode = permission_mode_from_label(normalized);
        self.runtime = build_runtime(
            session,
            self.model.clone(),
            self.system_prompt.clone(),
            true,
            true,
            self.allowed_tools.clone(),
            self.permission_mode,
            None,
        )?;
        println!(
            "{}",
            format_permissions_switch_report(&previous, normalized)
        );
        Ok(true)
    }

    fn clear_session(&mut self, confirm: bool) -> Result<bool, Box<dyn std::error::Error>> {
        if !confirm {
            println!(
                "clear : confirmation requise ; tapez /clear --confirm pour repartir de zéro."
            );
            return Ok(false);
        }

        self.session_hook_end("clear");
        self.session = create_managed_session_handle()?;
        session_bound(&self.session.path);
        self.runtime = build_runtime(
            Session::new(),
            self.model.clone(),
            self.system_prompt.clone(),
            true,
            true,
            self.allowed_tools.clone(),
            self.permission_mode,
            None,
        )?;
        self.session_hook_start("clear");
        println!(
            "Session effacée\n  Mode             nouvelle session\n  Modèle conservé  {}\n  Permissions      {}\n  Session          {}",
            self.model,
            self.permission_mode.as_str(),
            self.session.id,
        );
        Ok(true)
    }

    fn print_cost(&self) {
        let cumulative = self.runtime.usage().cumulative_usage();
        println!("{}", format_cost_report(cumulative));
        if let Some(line) = models::cost_line(&self.model, cumulative) {
            println!("{line}");
        }
    }

    fn resume_session(
        &mut self,
        session_path: Option<String>,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        let Some(session_ref) = session_path else {
            println!("Usage : /resume <chemin ou identifiant de session>");
            return Ok(false);
        };

        let handle = resolve_session_reference(&session_ref)?;
        let session = Session::load_from_path(&handle.path)?;
        let message_count = session.messages.len();
        self.runtime = build_runtime(
            session,
            self.model.clone(),
            self.system_prompt.clone(),
            true,
            true,
            self.allowed_tools.clone(),
            self.permission_mode,
            None,
        )?;
        self.session = handle;
        session_bound(&self.session.path);
        self.session_hook_start("resume");
        println!(
            "{}",
            format_resume_report(
                &self.session.path.display().to_string(),
                message_count,
                self.runtime.usage().turns(),
            )
        );
        Ok(true)
    }

    fn print_config(section: Option<&str>) -> Result<(), Box<dyn std::error::Error>> {
        println!("{}", render_config_report(section)?);
        Ok(())
    }

    fn print_memory() -> Result<(), Box<dyn std::error::Error>> {
        println!("{}", render_memory_report()?);
        Ok(())
    }

    fn print_agents(args: Option<&str>) -> Result<(), Box<dyn std::error::Error>> {
        let cwd = env::current_dir()?;
        println!("{}", handle_agents_slash_command(args, &cwd)?);
        Ok(())
    }

    fn print_skills(args: Option<&str>) -> Result<(), Box<dyn std::error::Error>> {
        let cwd = env::current_dir()?;
        println!("{}", handle_skills_slash_command(args, &cwd)?);
        Ok(())
    }

    fn print_diff() -> Result<(), Box<dyn std::error::Error>> {
        println!("{}", render_diff_report()?);
        Ok(())
    }

    fn print_version() {
        println!("{}", render_version_report());
    }

    fn export_session(
        &self,
        requested_path: Option<&str>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let export_path = resolve_export_path(requested_path, self.runtime.session())?;
        fs::write(&export_path, render_export_text(self.runtime.session()))?;
        println!(
            "Export\n  Résultat         transcription écrite\n  Fichier          {}\n  Messages         {}",
            export_path.display(),
            self.runtime.session().messages.len(),
        );
        Ok(())
    }

    fn handle_session_command(
        &mut self,
        action: Option<&str>,
        target: Option<&str>,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        match action {
            None | Some("list") => {
                println!("{}", render_session_list(&self.session.id)?);
                Ok(false)
            }
            Some("switch") => {
                let Some(target) = target else {
                    println!("Usage : /session switch <identifiant de session>");
                    return Ok(false);
                };
                let handle = resolve_session_reference(target)?;
                let session = Session::load_from_path(&handle.path)?;
                let message_count = session.messages.len();
                self.runtime = build_runtime(
                    session,
                    self.model.clone(),
                    self.system_prompt.clone(),
                    true,
                    true,
                    self.allowed_tools.clone(),
                    self.permission_mode,
                    None,
                )?;
                self.session = handle;
                session_bound(&self.session.path);
                println!(
                    "Session changée\n  Session active   {}\n  Fichier          {}\n  Messages         {}",
                    self.session.id,
                    self.session.path.display(),
                    message_count,
                );
                Ok(true)
            }
            Some("rename" | "renommer") => {
                let Some((id, title)) = target.map(|target| {
                    target
                        .split_once(char::is_whitespace)
                        .unwrap_or((target, ""))
                }) else {
                    println!("Usage : /session rename <identifiant de session> <titre>  (sans titre : titre automatique)");
                    return Ok(false);
                };
                match rename_managed_session(id, title) {
                    Ok(Some(title)) => println!(
                        "Session renommée\n  Session          {id}\n  Titre            {title}"
                    ),
                    Ok(None) => println!("Titre automatique rétabli\n  Session          {id}"),
                    Err(error) => println!("{error}"),
                }
                Ok(false)
            }
            Some("delete" | "effacer") => {
                let Some(target) = target else {
                    println!("Usage : /session delete <identifiant de session>");
                    return Ok(false);
                };
                if target == self.session.id {
                    println!("La session en cours ne peut pas être effacée (/clear la vide).");
                    return Ok(false);
                }
                match delete_managed_session(target) {
                    Ok(()) => println!("Session effacée\n  Session          {target}"),
                    Err(error) => println!("{error}"),
                }
                Ok(false)
            }
            Some("clear" | "vider") => {
                let others = list_managed_sessions()?
                    .into_iter()
                    .filter(|session| session.id != self.session.id)
                    .count();
                if others == 0 {
                    println!("Aucune session précédente à effacer.");
                    return Ok(false);
                }
                // Irréversible : un « oui » explicite est demandé.
                if target != Some("oui") {
                    println!(
                        "{others} session(s) précédente(s) seront effacées, avec leurs points de restauration ; la session en cours est gardée.\nPour confirmer : /session clear oui"
                    );
                    return Ok(false);
                }
                match delete_other_sessions(&self.session.id) {
                    Ok(deleted) => println!("Sessions effacées\n  Nombre           {deleted}"),
                    Err(error) => println!("{error}"),
                }
                Ok(false)
            }
            Some(other) => {
                println!("Action /session inconnue « {other} ». Utilisez /session list, /session switch <identifiant>, /session rename <identifiant> <titre>, /session delete <identifiant> ou /session clear.");
                Ok(false)
            }
        }
    }

    fn handle_plugins_command(
        &mut self,
        action: Option<&str>,
        target: Option<&str>,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        let cwd = env::current_dir()?;
        let loader = ConfigLoader::default_for(&cwd);
        let runtime_config = loader.load()?;
        let mut manager = build_plugin_manager(&cwd, &loader, &runtime_config);
        let result = handle_plugins_slash_command(action, target, &mut manager)?;
        println!("{}", result.message);
        if result.reload_runtime {
            self.reload_runtime_features()?;
        }
        Ok(false)
    }

    fn reload_runtime_features(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        self.runtime = build_runtime(
            self.runtime.session().clone(),
            self.model.clone(),
            self.system_prompt.clone(),
            true,
            true,
            self.allowed_tools.clone(),
            self.permission_mode,
            None,
        )?;
        self.persist_session()
    }

    /// `/compact [consigne]` : le modèle résume l'historique (en suivant la
    /// consigne : ce qu'il faut garder), même sous le seuil automatique.
    fn compact(&mut self, instructions: Option<&str>) -> Result<(), Box<dyn std::error::Error>> {
        live::start("Compaction de la conversation…");
        let result = self.runtime.compact_with_model(
            CompactionConfig {
                max_estimated_tokens: 0,
                ..CompactionConfig::default()
            },
            instructions,
        );
        live::stop();
        let removed = result.removed_message_count;
        let kept = result.compacted_session.messages.len();
        let skipped = removed == 0;
        self.runtime = build_runtime(
            result.compacted_session,
            self.model.clone(),
            self.system_prompt.clone(),
            true,
            true,
            self.allowed_tools.clone(),
            self.permission_mode,
            None,
        )?;
        self.persist_session()?;
        println!("{}", format_compact_report(removed, kept, skipped));
        Ok(())
    }

    /// Réponse hors conversation (sans outils, non gardée), affichée sous un titre.
    fn print_side_answer(&self, title: &str, label: &str, prompt: &str) {
        live::start(label);
        let answer = self.run_internal_prompt_text(prompt, false);
        live::stop();
        match answer {
            Ok(text) if !text.trim().is_empty() => {
                println!(
                    "\n{}※ {title}{}\n{}\n{}  (hors conversation : rien n'est gardé){}\n",
                    transcript::ACCENT,
                    transcript::RESET,
                    TerminalRenderer::new().markdown_to_ansi(&text),
                    transcript::DIM,
                    transcript::RESET
                );
            }
            Ok(_) => println!("  ⎿  Réponse vide du modèle"),
            Err(error) if runtime::interrupt::is_interruption(&error.to_string()) => {
                println!("  ⎿  {}Interrompu{}", transcript::RED, transcript::RESET);
            }
            Err(error) => eprintln!("{}", render_cli_error(&error.to_string())),
        }
    }

    /// `/btw <question>` : question en aparté, sans interrompre ni allonger la
    /// conversation.
    fn run_side_question(&self, question: Option<&str>) {
        let Some(question) = question.map(str::trim).filter(|text| !text.is_empty()) else {
            println!("Usage : /btw <question>  (réponse en aparté, hors conversation)");
            return;
        };
        let prompt = format!(
            "[Side question from the user, asked in passing. Answer it briefly from what you already know of this conversation and project, in the user's language. Do not use tools, do not continue or change the current task.]\n\n{question}"
        );
        self.print_side_answer("Aparté", "Réponse en aparté…", &prompt);
    }

    /// `/recap` : bilan de la session (demandes, travail fait, reste à faire).
    fn run_recap(&self) {
        if assistant_replies(self.runtime.session()).is_empty() {
            println!("  ⎿  Rien à récapituler : la session vient de commencer");
            return;
        }
        let prompt = "[The user asks for a recap of this session. In their language and in at most 12 short bullet points, say: what was asked, what was done (files changed, commands run and their results), what is still open, and the next step. Do not use tools and do not continue the work.]";
        self.print_side_answer("Bilan de la session", "Bilan de la session…", prompt);
    }

    /// `/rename [titre]` : sans titre, le modèle en propose un d'après la
    /// conversation.
    fn rename_session(&self, title: Option<&str>) {
        if let Some(title) = title.map(str::trim).filter(|title| !title.is_empty()) {
            println!("{}", rename_report(&self.session.id, Some(title)));
            return;
        }
        if assistant_replies(self.runtime.session()).is_empty() {
            println!(
                "Usage : /rename <titre>  (sans titre : titre proposé d'après la conversation)"
            );
            return;
        }
        live::start("Choix d'un titre…");
        let proposed = self.run_internal_prompt_text(
            "[Give this conversation a short title: 3 to 7 words, in the user's language, no quotes, no final period. Reply with the title only.]",
            false,
        );
        live::stop();
        match proposed {
            Ok(text) => {
                let title = text
                    .lines()
                    .map(|line| line.trim().trim_matches(['"', '«', '»', '*', '#', ' ']))
                    .find(|line| !line.is_empty())
                    .unwrap_or_default()
                    .to_string();
                if title.is_empty() {
                    println!("  ⎿  Aucun titre proposé ; donnez-en un : /rename <titre>");
                } else {
                    println!("{}", rename_report(&self.session.id, Some(&title)));
                }
            }
            Err(error) => eprintln!("{}", render_cli_error(&error.to_string())),
        }
    }

    /// `/goal <condition>` fixe un objectif ; `/goal` montre où il en est,
    /// `/goal reprendre` relance un objectif en pause, `/goal clear` l'efface.
    fn handle_goal_command(
        &mut self,
        args: Option<&str>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let args = args.map(str::trim).filter(|args| !args.is_empty());
        let Some(args) = args else {
            println!("{}", render_goal_status(self.goal.as_ref()));
            return Ok(());
        };
        match fold_command_word(args).as_str() {
            "clear" | "stop" | "effacer" | "arreter" | "annuler" => {
                if self.goal.take().is_some() {
                    println!("  ⎿  Objectif effacé");
                } else {
                    println!("  ⎿  Aucun objectif en cours");
                }
                return Ok(());
            }
            "reprendre" | "resume" | "continuer" | "continue" => {
                let Some(goal) = self.goal.as_mut() else {
                    println!("  ⎿  Aucun objectif à reprendre ; fixez-en un : /goal <condition>");
                    return Ok(());
                };
                goal.paused = false;
                goal.stalls = 0;
                goal.rounds = 0;
                let prompt = goal_continue_prompt(&goal.condition, goal.last_reason.as_deref());
                println!(
                    "{}◎ Reprise de l'objectif : {}{}",
                    transcript::ACCENT,
                    goal.condition,
                    transcript::RESET
                );
                self.run_goal_turn(&prompt);
                return self.pursue_goal();
            }
            _ => {}
        }
        let goal = GoalState::new(args);
        println!(
            "{}◎ Objectif : {}{}\n{}  Claw continue tant qu'un contrôle ne le juge pas atteint (au plus {} tours) · Échap pour mettre en pause{}",
            transcript::ACCENT,
            goal.condition,
            transcript::RESET,
            transcript::DIM,
            goal_max_rounds(),
            transcript::RESET
        );
        let prompt = goal_start_prompt(&goal.condition);
        self.goal = Some(goal);
        self.run_goal_turn(&prompt);
        self.pursue_goal()
    }

    /// Tour poursuivant l'objectif : une erreur (quota, réseau) le met en pause
    /// au lieu de fermer la session.
    fn run_goal_turn(&mut self, prompt: &str) {
        if let Err(error) = self.run_turn(prompt) {
            eprintln!("{}", render_cli_error(&error.to_string()));
            self.last_turn_interrupted = true;
        }
    }

    /// Après un tour : l'objectif est contrôlé ; tant qu'il n'est pas atteint,
    /// Claw continue (jusqu'à la limite, une interruption ou l'absence de progrès).
    fn pursue_goal(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let max_rounds = goal_max_rounds();
        loop {
            let Some(goal) = self.goal.clone().filter(|goal| !goal.paused) else {
                return Ok(());
            };
            if self.last_turn_interrupted {
                self.pause_goal("interrompu");
                return Ok(());
            }
            live::start("Contrôle de l'objectif…");
            let verdict = self.run_internal_prompt_text(&goal_check_prompt(&goal.condition), false);
            live::stop();
            let verdict = match verdict {
                Ok(text) => parse_goal_verdict(&text),
                Err(error) => {
                    if !runtime::interrupt::is_interruption(&error.to_string()) {
                        eprintln!("{}", render_cli_error(&error.to_string()));
                    }
                    self.pause_goal("contrôle impossible");
                    return Ok(());
                }
            };
            let reason = match verdict {
                GoalVerdict::Met(reason) => {
                    println!(
                        "{}✓ Objectif atteint{} · {reason}\n",
                        transcript::ACCENT,
                        transcript::RESET
                    );
                    self.goal = None;
                    return Ok(());
                }
                GoalVerdict::NotMet(reason) => reason,
            };
            let Some(goal) = self.goal.as_mut() else {
                return Ok(());
            };
            goal.rounds += 1;
            if goal.last_reason.as_deref() == Some(reason.as_str()) {
                goal.stalls += 1;
            } else {
                goal.stalls = 0;
            }
            goal.last_reason = Some(reason.clone());
            let (rounds, stalls) = (goal.rounds, goal.stalls);
            let prompt = goal_continue_prompt(&goal.condition, Some(&reason));
            if rounds >= max_rounds {
                self.pause_goal(&format!("{max_rounds} tours sans l'atteindre"));
                return Ok(());
            }
            if stalls >= 2 {
                self.pause_goal("plus de progrès");
                return Ok(());
            }
            println!(
                "{}◎ Pas encore atteint ({rounds}/{max_rounds}) · {reason}{}",
                transcript::DIM,
                transcript::RESET
            );
            self.run_goal_turn(&prompt);
            self.persist_session()?;
        }
    }

    fn pause_goal(&mut self, why: &str) {
        if let Some(goal) = self.goal.as_mut() {
            goal.paused = true;
            let reason = goal
                .last_reason
                .as_deref()
                .map(|reason| format!("\n  Dernier contrôle  {reason}"))
                .unwrap_or_default();
            println!(
                "{}◎ Objectif en pause ({why}){reason}\n  Suite            /goal reprendre · /goal clear{}\n",
                transcript::DIM,
                transcript::RESET
            );
        }
    }

    /// Un message tapé pendant qu'un objectif est actif : le contrôle reprend
    /// après la réponse.
    fn continue_goal_after_turn(&mut self) {
        if self.goal.as_ref().is_some_and(|goal| !goal.paused) {
            if let Err(error) = self.pursue_goal() {
                eprintln!("{}", render_cli_error(&error.to_string()));
            }
        }
    }

    /// `/loop [intervalle] <demande>` : la demande relancée à intervalle
    /// régulier, jusqu'à Échap, Ctrl+C ou la limite de passages.
    /// Sous-tâches finies : le résultat est montré et rejoint la conversation.
    fn deliver_subtasks(&mut self) {
        for subtask in extra_commands::take_finished_subtasks() {
            match subtask.result {
                Some(Ok(report)) => {
                    println!(
                        "\n{}※ Sous-tâche {} terminée{} · {}\n{}\n",
                        transcript::ACCENT,
                        subtask.id,
                        transcript::RESET,
                        subtask.task,
                        TerminalRenderer::new().markdown_to_ansi(&report)
                    );
                    self.pending_context.push(format!(
                        "[Result of sub-task {} \"{}\", run by a forked sub-agent]\n{report}",
                        subtask.id, subtask.task
                    ));
                }
                Some(Err(error)) => println!(
                    "\n  ⎿  {}Sous-tâche {} en échec : {error}{}\n",
                    transcript::RED,
                    subtask.id,
                    transcript::RESET
                ),
                None => {}
            }
        }
    }

    /// `/cd <chemin>` : la session continue dans ce dossier (consignes,
    /// réglages, permissions et outils de ce dossier).
    fn change_directory(&mut self, path: Option<&str>) -> Result<(), Box<dyn std::error::Error>> {
        let Some(path) = path.map(str::trim).filter(|path| !path.is_empty()) else {
            println!(
                "Usage : /cd <chemin>  (dossier actuel : {})",
                env::current_dir()?.display()
            );
            return Ok(());
        };
        let target = env::current_dir()?.join(path);
        let target =
            fs::canonicalize(&target).map_err(|error| format!("{} : {error}", target.display()))?;
        // Sous Windows, `canonicalize` rend un chemin `\\?\C:\…` : on garde la
        // forme ordinaire (affichage, outils, git).
        let target = target
            .to_str()
            .and_then(|text| text.strip_prefix(r"\\?\"))
            .filter(|rest| !rest.starts_with("UNC"))
            .map_or_else(|| target.clone(), PathBuf::from);
        if !target.is_dir() {
            return Err(format!("{} n'est pas un dossier", target.display()).into());
        }
        env::set_current_dir(&target)?;
        self.refresh_system_prompt()?;
        self.reload_runtime_features()?;
        println!(
            "  ⎿  Dossier : {}\n     Consignes, réglages et permissions de ce dossier appliqués ; la conversation continue (/add-dir garde au contraire le dossier actuel)",
            transcript::display_path(&target.display().to_string())
        );
        Ok(())
    }

    /// `/tasks` : ce qui tourne en arrière-plan pour cette session.
    fn print_tasks() {
        let mut sections = vec![render_background_tasks(None)];
        let subtasks = extra_commands::subtask_lines();
        if !subtasks.is_empty() {
            sections.push(format!("Sous-tâches\n{}", subtasks.join("\n")));
        }
        let scheduled = runtime::cron::list();
        if !scheduled.is_empty() {
            sections.push(format!(
                "Tâches planifiées\n{}",
                scheduled
                    .iter()
                    .map(|task| format!("  {}", task.describe()))
                    .collect::<Vec<_>>()
                    .join("\n")
            ));
        }
        let cwd = env::current_dir().unwrap_or_default();
        let jobs: Vec<String> = jobs::all()
            .into_iter()
            .filter(|(job, _)| Path::new(&job.origin).starts_with(&cwd))
            .take(8)
            .map(|(job, state)| {
                format!(
                    "  {}  {:<8} {}",
                    job.id,
                    jobs::effective_state(&job, &state),
                    job.name
                )
            })
            .collect();
        if !jobs.is_empty() {
            sections.push(format!(
                "Sessions en arrière-plan (claw agents)\n{}",
                jobs.join("\n")
            ));
        }
        println!("{}", sections.join("\n\n"));
    }

    /// Tâches planifiées échues : chacune passe comme un tour.
    fn run_due_tasks(&mut self) {
        while let Some(task) = runtime::cron::take_due() {
            let kind = if task.self_paced {
                "boucle".to_string()
            } else if task.recurring {
                task.cron.clone()
            } else {
                "rappel".to_string()
            };
            println!(
                "{}↻ Tâche planifiée {} · {kind}{}",
                transcript::ACCENT,
                task.id,
                transcript::RESET
            );
            let _ = runtime::cron::take_stop_requested();
            if let Err(error) = self.run_turn(&scheduled_prompt(&task)) {
                eprintln!("{}", render_cli_error(&error.to_string()));
            }
            if task.self_paced {
                after_self_paced_pass(&task.prompt, task.fallback);
            }
            if self.last_turn_interrupted {
                break;
            }
        }
    }

    /// `/loop` : `/loop 5m demande` (tâche récurrente, la saisie reste libre),
    /// `/loop demande` (Claw choisit l'intervalle), `/loop` seul (`loop.md` ou
    /// la passe d'entretien), `/loop list`, `/loop stop [id]`.
    #[allow(clippy::too_many_lines)]
    fn run_loop(&mut self, args: Option<&str>) -> Result<(), Box<dyn std::error::Error>> {
        let args = args.map(str::trim).unwrap_or_default();
        let (first, remainder) = args.split_once(char::is_whitespace).unwrap_or((args, ""));
        let dim = transcript::DIM;
        let reset = transcript::RESET;
        match first.to_lowercase().as_str() {
            "list" | "liste" => {
                let tasks = runtime::cron::list();
                if tasks.is_empty() {
                    println!("Aucune tâche planifiée.");
                } else {
                    println!("Tâches planifiées");
                    for task in tasks {
                        println!("  {}", task.describe());
                    }
                }
                return Ok(());
            }
            "stop" | "arrêter" | "arreter" => {
                if remainder.trim().is_empty() {
                    let stopped = runtime::cron::stop_loops();
                    println!("  ⎿  {stopped} boucle(s) arrêtée(s)");
                } else {
                    match runtime::cron::delete(remainder) {
                        Ok(task) => println!("  ⎿  Tâche {} supprimée", task.id),
                        Err(error) => println!("  ⎿  {error}"),
                    }
                }
                return Ok(());
            }
            _ => {}
        }
        if runtime::cron::disabled() {
            println!("Les tâches planifiées sont désactivées (CLAW_DISABLE_CRON).");
            return Ok(());
        }
        let (interval, request) = match parse_interval(first) {
            Some(interval) => (Some(interval), remainder.trim()),
            None => (None, args),
        };
        let request = if request.is_empty() {
            default_loop_prompt()
        } else if request.starts_with('/') {
            // `/loop 5m /ma-commande` : une commande personnalisée devient sa demande.
            match custom_command_prompt(request) {
                Some(Ok(invocation)) => invocation.prompt,
                Some(Err(error)) => {
                    eprintln!("{error}");
                    return Ok(());
                }
                None => request.to_string(),
            }
        } else {
            request.to_string()
        };
        let Some(interval) = interval else {
            println!(
                "{}↻ Boucle au rythme choisi par Claw · /loop stop pour l'arrêter{reset}",
                transcript::ACCENT
            );
            let _ = runtime::cron::take_stop_requested();
            self.run_turn(&format!("{request}{}", self_paced_note(&request)))?;
            if self.last_turn_interrupted {
                let _ = runtime::cron::stop_loops();
                println!("  ⎿  Boucle arrêtée");
                return Ok(());
            }
            after_self_paced_pass(&request, false);
            return Ok(());
        };
        let (cron, minutes) = runtime::cron::interval_to_cron(interval);
        let max = loop_max_passes();
        println!(
            "{}↻ Boucle · toutes les {} ({cron}) · au plus {max} passages · /loop stop pour l'arrêter{reset}",
            transcript::ACCENT,
            format_interval(Duration::from_secs(minutes * 60)),
        );
        if minutes * 60 != interval.as_secs() {
            println!(
                "  ⎿  {dim}Intervalle arrondi à {} (pas d'une expression cron){reset}",
                format_interval(Duration::from_secs(minutes * 60))
            );
        }
        self.run_turn(&request)?;
        if self.last_turn_interrupted {
            println!("  ⎿  Boucle arrêtée");
            return Ok(());
        }
        if max <= 1 {
            return Ok(());
        }
        match runtime::cron::create_loop(&cron, &request, u32::try_from(max - 1).ok()) {
            Ok(task) => println!(
                "  ⎿  {dim}Boucle {} · prochain passage à {} · la saisie reste libre entre les passages{reset}",
                task.id,
                runtime::cron::format_local(task.next_run)
            ),
            Err(error) => eprintln!("{}", render_cli_error(&error)),
        }
        Ok(())
    }

    fn run_internal_prompt_text_with_progress(
        &self,
        prompt: &str,
        enable_tools: bool,
        progress: Option<InternalPromptProgressReporter>,
    ) -> Result<String, Box<dyn std::error::Error>> {
        let session = self.runtime.session().clone();
        let mut runtime = build_runtime(
            session,
            self.model.clone(),
            self.system_prompt.clone(),
            enable_tools,
            false,
            self.allowed_tools.clone(),
            self.permission_mode,
            progress,
        )?;
        let mut permission_prompter = CliPermissionPrompter::new(self.permission_mode);
        let summary = runtime.run_turn(prompt, Some(&mut permission_prompter))?;
        Ok(final_assistant_text(&summary).trim().to_string())
    }

    fn run_internal_prompt_text(
        &self,
        prompt: &str,
        enable_tools: bool,
    ) -> Result<String, Box<dyn std::error::Error>> {
        self.run_internal_prompt_text_with_progress(prompt, enable_tools, None)
    }

    fn run_bughunter(&self, scope: Option<&str>) -> Result<(), Box<dyn std::error::Error>> {
        let scope = scope.unwrap_or("the current repository");
        let prompt = format!(
            "You are /bughunter. Inspect {scope} and identify the most likely bugs or correctness issues. Prioritize concrete findings with file paths, severity, and suggested fixes. Use tools if needed."
        );
        println!("{}", self.run_internal_prompt_text(&prompt, true)?);
        Ok(())
    }

    fn run_ultraplan(&self, task: Option<&str>) -> Result<(), Box<dyn std::error::Error>> {
        let task = task.unwrap_or("the current repo work");
        let prompt = format!(
            "You are /ultraplan. Produce a deep multi-step execution plan for {task}. Include goals, risks, implementation sequence, verification steps, and rollback considerations. Use tools if needed."
        );
        let mut progress = InternalPromptProgressRun::start_ultraplan(task);
        match self.run_internal_prompt_text_with_progress(&prompt, true, Some(progress.reporter()))
        {
            Ok(plan) => {
                progress.finish_success();
                println!("{plan}");
                Ok(())
            }
            Err(error) => {
                progress.finish_failure(&error.to_string());
                Err(error)
            }
        }
    }

    #[allow(clippy::unused_self)]
    fn run_teleport(&self, target: Option<&str>) -> Result<(), Box<dyn std::error::Error>> {
        let Some(target) = target.map(str::trim).filter(|value| !value.is_empty()) else {
            println!("Usage : /teleport <symbole ou chemin>");
            return Ok(());
        };

        println!("{}", render_teleport_report(target)?);
        Ok(())
    }

    fn run_debug_tool_call(&self) -> Result<(), Box<dyn std::error::Error>> {
        println!("{}", render_last_tool_debug_report(self.runtime.session())?);
        Ok(())
    }

    fn run_commit(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let status = git_output(&["status", "--short"])?;
        if status.trim().is_empty() {
            println!("Commit\n  Résultat         ignoré\n  Raison           aucune modification dans le projet");
            return Ok(());
        }

        git_status_ok(&["add", "-A"])?;
        let staged_stat = git_output(&["diff", "--cached", "--stat"])?;
        let prompt = format!(
            "Generate a git commit message in plain text Lore format only. Base it on this staged diff summary:\n\n{}\n\nRecent conversation context:\n{}",
            truncate_for_prompt(&staged_stat, 8_000),
            recent_user_context(self.runtime.session(), 6)
        );
        let message = sanitize_generated_message(&self.run_internal_prompt_text(&prompt, false)?);
        if message.trim().is_empty() {
            return Err("le message de commit généré est vide".into());
        }
        let message = runtime::settings::with_attribution(&message, &current_attribution().commit);

        let path = write_temp_text_file("claw-commit-message.txt", &message)?;
        let output = Command::new("git")
            .args(["commit", "--file"])
            .arg(&path)
            .current_dir(env::current_dir()?)
            .output()?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            return Err(format!("échec de git commit : {stderr}").into());
        }

        println!(
            "Commit\n  Résultat         créé\n  Fichier message  {}\n\n{}",
            path.display(),
            message.trim()
        );
        Ok(())
    }

    /// Un tour d'une session en arrière-plan : pas de terminal ; permissions
    /// et questions attendent une réponse dans sa boîte.
    fn run_job_turn(
        &mut self,
        input: &str,
        job_id: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut prompter = jobs::JobPermissionPrompter {
            id: job_id.to_string(),
        };
        self.share_auto_context(input);
        let (text, images) = take_image_attachments(input);
        self.runtime.autosave_to(&self.session.path);
        let result = self
            .runtime
            .run_turn_with_images(text, images, Some(&mut prompter));
        transcript::flush_calls();
        self.persist_session()?;
        result
            .map(|_| ())
            .map_err(|error| Box::new(error) as Box<dyn std::error::Error>)
    }

    /// Mode auto : les dernières demandes de l'utilisateur (ses limites et
    /// ses accords), lues par le classificateur.
    fn share_auto_context(&self, input: &str) {
        let mut requests: Vec<String> = self
            .runtime
            .session()
            .messages
            .iter()
            .filter(|message| message.role == runtime::MessageRole::User)
            .filter_map(|message| {
                message.blocks.iter().find_map(|block| match block {
                    ContentBlock::Text { text }
                        if !text.trim().is_empty() && !text.starts_with("[Claw]") =>
                    {
                        Some(text.clone())
                    }
                    _ => None,
                })
            })
            .collect();
        requests.push(input.to_string());
        let start = requests.len().saturating_sub(8);
        auto_mode::set_context(requests[start..].to_vec(), &self.model);
    }

    /// Première demande de la conversation (nom de la session).
    fn first_user_text(&self) -> Option<String> {
        self.runtime
            .session()
            .messages
            .iter()
            .filter(|message| message.role == runtime::MessageRole::User)
            .find_map(|message| {
                message.blocks.iter().find_map(|block| match block {
                    ContentBlock::Text { text } if !text.trim().is_empty() => Some(text.clone()),
                    _ => None,
                })
            })
    }

    /// `/bg [demande]` : la conversation continue dans une session en
    /// arrière-plan, au même endroit ; ce terminal se libère. `true` : quitter.
    fn send_to_background(
        &mut self,
        prompt: Option<&str>,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        let prompt = prompt.unwrap_or_default().trim().to_string();
        if self.runtime.session().messages.is_empty() && prompt.is_empty() {
            println!(
                "  ⎿  Envoyez d'abord un message (ou /bg <demande>) : une session vide n'a rien à poursuivre."
            );
            return Ok(false);
        }
        self.persist_session()?;
        let name =
            jobs::name_from_prompt(&self.first_user_text().unwrap_or_else(|| prompt.clone()));
        let job = jobs::dispatch(&jobs::Dispatch {
            prompt,
            name: Some(name),
            command: None,
            model: self.model.clone(),
            permission_mode: self.permission_mode.as_str().to_string(),
            origin: env::current_dir()?,
            session_path: Some(self.session.path.clone()),
            in_place: true,
        })?;
        println!("{}", jobs::launch_report(&job));
        Ok(true)
    }

    /// `/fork [demande]` : une copie de la conversation continue en
    /// arrière-plan (dans son propre worktree) ; cette session reste ici.
    fn fork_to_background(
        &mut self,
        prompt: Option<&str>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.persist_session()?;
        let copy = sessions_dir()?.join(format!("{}.json", generate_session_id()));
        fs::copy(&self.session.path, &copy)?;
        let base = jobs::name_from_prompt(
            &self
                .first_user_text()
                .unwrap_or_else(|| prompt.unwrap_or("copie").to_string()),
        );
        let job = jobs::dispatch(&jobs::Dispatch {
            prompt: prompt.unwrap_or_default().trim().to_string(),
            name: Some(format!("{base} (copie)")),
            command: None,
            model: self.model.clone(),
            permission_mode: self.permission_mode.as_str().to_string(),
            origin: env::current_dir()?,
            session_path: Some(copy),
            in_place: false,
        })?;
        println!("{}", jobs::launch_report(&job));
        if prompt.is_none_or(|text| text.trim().is_empty()) {
            println!(
                "  ⎿  La copie attend sa première demande : claw reply {} <demande>, ou Espace sur sa ligne dans claw agents",
                job.id
            );
        }
        Ok(())
    }

    /// `/nom-du-skill arguments` : skill installé (`.claw/skills`,
    /// `.claude/skills`, `~/.claude/skills`…) ; `None` si aucun skill ne porte
    /// ce nom.
    fn invoke_skill(&mut self, input: &str) -> Option<Result<(), Box<dyn std::error::Error>>> {
        let line = input.trim().strip_prefix('/')?;
        let (name, arguments) = line.split_once(char::is_whitespace).unwrap_or((line, ""));
        let cwd = env::current_dir().ok()?;
        let installed = commands::skill_registry::find_installed_skill(&cwd, name)?;
        Some(self.run_skill(&installed, arguments.trim(), &cwd))
    }

    fn run_skill(
        &mut self,
        installed: &commands::skill_registry::InstalledSkill,
        arguments: &str,
        cwd: &Path,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let skill = &installed.file;
        if !installed.user_invocable() {
            println!(
                "  ⎿  /{} est réservé à Claude (user-invocable: false ou skillOverrides) : demandez la tâche en langage courant.",
                skill.name
            );
            return Ok(());
        }
        let source = skill
            .path
            .strip_prefix(cwd)
            .unwrap_or(&skill.path)
            .display()
            .to_string();
        let model = skill
            .model
            .as_deref()
            .map(|model| format!(" · modèle {model}"))
            .unwrap_or_default();
        eprintln!("↳ /{} ({source}){model}", skill.name);
        runtime::deny_for_turn(&skill.disallowed_tools);
        let rendered = match tools::render_skill_for_session(skill, arguments) {
            Ok(rendered) => rendered,
            Err(error) => {
                runtime::clear_turn_grants();
                println!("  ⎿  {}{error}{}", transcript::RED, transcript::RESET);
                return Ok(());
            }
        };
        if skill.fork {
            let agent = skill
                .agent
                .clone()
                .unwrap_or_else(|| "general-purpose".to_string());
            let output = tools::execute_tool(
                "Agent",
                &json!({
                    "description": format!("Skill {}", skill.name),
                    "prompt": rendered,
                    "subagent_type": agent,
                    "model": skill.model,
                }),
            );
            runtime::clear_turn_grants();
            let report = output.map_err(|error| error.clone()).and_then(|text| {
                serde_json::from_str::<serde_json::Value>(&text)
                    .ok()
                    .and_then(|value| value.get("result")?.as_str().map(ToOwned::to_owned))
                    .ok_or_else(|| "le sous-agent n'a pas rendu de rapport".to_string())
            });
            match report {
                Ok(report) => {
                    println!(
                        "\n{}※ /{} (sous-agent {agent}){}\n{}\n",
                        transcript::ACCENT,
                        skill.name,
                        transcript::RESET,
                        TerminalRenderer::new().markdown_to_ansi(&report)
                    );
                    self.pending_context.push(format!(
                        "[Result of the /{} skill, run by a {agent} sub-agent]\n{report}",
                        skill.name
                    ));
                }
                Err(error) => eprintln!("{}", render_cli_error(&error)),
            }
            return Ok(());
        }
        let previous_effort = api::reasoning_effort();
        let effort = skill.effort.as_deref().and_then(parse_skill_effort);
        if effort.is_some() {
            api::set_reasoning_effort(effort);
        }
        let result = self.run_turn_with_overrides(&rendered, skill.model.clone(), None);
        if effort.is_some() {
            api::set_reasoning_effort(previous_effort);
        }
        runtime::clear_turn_grants();
        result
    }

    /// `/commit-push-pr [contexte]` : sur la branche principale, une branche
    /// est créée d'abord ; puis commit (message généré), poussée et pull
    /// request (avec `gh`, sinon le lien pour l'ouvrir).
    fn run_commit_push_pr(
        &mut self,
        context: Option<&str>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let cwd = env::current_dir()?;
        let base = git_work::default_branch(&cwd);
        let mut branch = git_work::current_branch(&cwd)?;
        if branch == base || branch == "HEAD" {
            let hint = context.map_or_else(
                || {
                    let session = self.runtime.session();
                    let last = recent_user_context(session, 1);
                    last.trim_start_matches("1. ").to_string()
                },
                ToOwned::to_owned,
            );
            let name = git_work::branch_name_for(&hint);
            runtime::worktree::git(&cwd, &["switch", "-c", &name])?;
            println!("Branche\n  Créée            {name} (depuis {base})");
            branch = name;
        }
        self.run_commit()?;
        let ahead =
            runtime::worktree::git(&cwd, &["rev-list", "--count", &format!("{base}..HEAD")])
                .ok()
                .and_then(|count| count.trim().parse::<u32>().ok())
                .unwrap_or(1);
        if ahead == 0 {
            println!("Pull request\n  Résultat         ignorée\n  Raison           {branch} n'a aucun commit de plus que {base}");
            return Ok(());
        }
        live::start(&format!("git push origin {branch}…"));
        let pushed = runtime::worktree::git(&cwd, &["push", "-u", "origin", &branch]);
        live::stop();
        if let Err(error) = pushed {
            return Err(format!("échec de git push : {error}").into());
        }
        println!("Poussée\n  Branche          origin/{branch}");

        let log = git_output(&["log", "--oneline", &format!("{base}..HEAD")]).unwrap_or_default();
        let stat = git_output(&["diff", "--stat", &format!("{base}...HEAD")]).unwrap_or_default();
        let prompt = format!(
            "Generate a pull request title and body for these commits. Output plain text in this format exactly:\nTITLE: <title>\nBODY:\n<body markdown: what changes and why, how it was verified>\n\nContext hint: {}\n\nCommits:\n{}\n\nDiff summary:\n{}\n\nRecent requests:\n{}",
            context.unwrap_or("none"),
            truncate_for_prompt(&log, 4_000),
            truncate_for_prompt(&stat, 8_000),
            recent_user_context(self.runtime.session(), 4)
        );
        live::start("Rédaction de la pull request…");
        let draft = self.run_internal_prompt_text(&prompt, false);
        live::stop();
        let draft = sanitize_generated_message(&draft?);
        let (title, body) = parse_titled_body(&draft).unwrap_or_else(|| {
            (
                log.lines().next().unwrap_or(branch.as_str()).to_string(),
                draft.clone(),
            )
        });
        let body = runtime::settings::with_attribution(&body, &current_attribution().pr);
        if command_exists("gh") {
            let body_path = write_temp_text_file("claw-pr-body.md", &body)?;
            let output = Command::new("gh")
                .args([
                    "pr",
                    "create",
                    "--base",
                    &base,
                    "--head",
                    &branch,
                    "--title",
                    &title,
                    "--body-file",
                ])
                .arg(&body_path)
                .current_dir(&cwd)
                .output()?;
            if output.status.success() {
                let url = String::from_utf8_lossy(&output.stdout).trim().to_string();
                println!("Pull request\n  Résultat         créée\n  Titre            {title}\n  URL              {url}");
                return Ok(());
            }
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            println!("  ⎿  gh pr create a échoué : {stderr}");
        }
        let link = git_work::compare_url(&cwd, &base, &branch).unwrap_or_else(|| {
            format!("(dépôt hors GitHub : ouvrez la PR de {branch} vers {base})")
        });
        println!("Pull request à ouvrir\n  Lien             {link}\n  Titre            {title}\n\n{body}");
        Ok(())
    }

    fn run_pr(&self, context: Option<&str>) -> Result<(), Box<dyn std::error::Error>> {
        let staged = git_output(&["diff", "--stat"])?;
        let prompt = format!(
            "Generate a pull request title and body from this conversation and diff summary. Output plain text in this format exactly:\nTITLE: <title>\nBODY:\n<body markdown>\n\nContext hint: {}\n\nDiff summary:\n{}",
            context.unwrap_or("none"),
            truncate_for_prompt(&staged, 10_000)
        );
        let draft = sanitize_generated_message(&self.run_internal_prompt_text(&prompt, false)?);
        let (title, body) = parse_titled_body(&draft)
            .ok_or_else(|| "titre ou corps de PR généré illisible".to_string())?;
        let body = runtime::settings::with_attribution(&body, &current_attribution().pr);

        if command_exists("gh") {
            let body_path = write_temp_text_file("claw-pr-body.md", &body)?;
            let output = Command::new("gh")
                .args(["pr", "create", "--title", &title, "--body-file"])
                .arg(&body_path)
                .current_dir(env::current_dir()?)
                .output()?;
            if output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
                println!(
                    "PR\n  Résultat         créée\n  Titre            {title}\n  URL              {}",
                    if stdout.is_empty() { "<inconnue>" } else { &stdout }
                );
                return Ok(());
            }
        }

        println!("Brouillon de PR\n  Titre            {title}\n\n{body}");
        Ok(())
    }

    fn run_issue(&self, context: Option<&str>) -> Result<(), Box<dyn std::error::Error>> {
        let prompt = format!(
            "Generate a GitHub issue title and body from this conversation. Output plain text in this format exactly:\nTITLE: <title>\nBODY:\n<body markdown>\n\nContext hint: {}\n\nConversation context:\n{}",
            context.unwrap_or("none"),
            truncate_for_prompt(&recent_user_context(self.runtime.session(), 10), 10_000)
        );
        let draft = sanitize_generated_message(&self.run_internal_prompt_text(&prompt, false)?);
        let (title, body) = parse_titled_body(&draft)
            .ok_or_else(|| "titre ou corps d'issue généré illisible".to_string())?;

        if command_exists("gh") {
            let body_path = write_temp_text_file("claw-issue-body.md", &body)?;
            let output = Command::new("gh")
                .args(["issue", "create", "--title", &title, "--body-file"])
                .arg(&body_path)
                .current_dir(env::current_dir()?)
                .output()?;
            if output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
                println!(
                    "Issue\n  Résultat         créée\n  Titre            {title}\n  URL              {}",
                    if stdout.is_empty() { "<inconnue>" } else { &stdout }
                );
                return Ok(());
            }
        }

        println!("Brouillon d'issue\n  Titre            {title}\n\n{body}");
        Ok(())
    }
}

fn sessions_dir() -> Result<PathBuf, Box<dyn std::error::Error>> {
    let cwd = env::current_dir()?;
    let path = cwd.join(".claw").join("sessions");
    fs::create_dir_all(&path)?;
    Ok(path)
}

/// `cleanupPeriodDays` : sessions et points de restauration du projet plus
/// anciens que ce nombre de jours supprimés au début d'une session. Sans ce
/// réglage, Claw garde tout.
fn sweep_old_sessions(current: &str) {
    let Ok(cwd) = env::current_dir() else {
        return;
    };
    let Some(days) = ConfigLoader::default_for(&cwd)
        .load()
        .ok()
        .and_then(|config| runtime::settings::cleanup_period_days(&config))
    else {
        return;
    };
    let _removed = runtime::settings::sweep_old_sessions(&cwd.join(".claw"), days, Some(current));
}

fn create_managed_session_handle() -> Result<SessionHandle, Box<dyn std::error::Error>> {
    let id = generate_session_id();
    let path = sessions_dir()?.join(format!("{id}.json"));
    let handle = SessionHandle { id, path };
    activate_checkpoints(&handle);
    sweep_old_sessions(&handle.id);
    Ok(handle)
}

/// Points de restauration de la session (`.claw/checkpoints/<id>.json`) et
/// identifiant transmis aux hooks.
fn activate_checkpoints(handle: &SessionHandle) {
    runtime::set_hook_session_info(&handle.id, &handle.path);
    if let Some(claw_dir) = handle.path.parent().and_then(Path::parent) {
        runtime::checkpoint::activate(
            claw_dir
                .join("checkpoints")
                .join(format!("{}.json", handle.id)),
        );
    }
}

/// `/undo [n]` : restaure les fichiers des n derniers tours et, si possible,
/// ramène la conversation avant le premier d'entre eux.
fn undo_turns<C: ApiClient, T: ToolExecutor>(
    runtime: &mut ConversationRuntime<C, T>,
    count: Option<&str>,
) -> Result<String, String> {
    let count = match count.map(str::trim).filter(|value| !value.is_empty()) {
        Some(value) => value
            .parse::<usize>()
            .ok()
            .filter(|count| *count > 0)
            .ok_or_else(|| format!("Usage : /undo [nombre de tours] (reçu « {value} »)"))?,
        None => 1,
    };
    let report = runtime::checkpoint::undo(count)?;
    let rewound = runtime.rewind_to(&report);
    let cwd = env::current_dir().unwrap_or_default();
    let show = |path: &PathBuf| {
        path.strip_prefix(&cwd)
            .unwrap_or(path)
            .display()
            .to_string()
    };
    let mut lines = vec![format!("Annulation de {} tour(s)", report.turns)];
    if report.restored.is_empty() && report.deleted.is_empty() {
        lines.push(
            "  Fichiers         aucun fichier modifié par l'agent dans ces tours".to_string(),
        );
    }
    for path in &report.restored {
        lines.push(format!("  Restauré         {}", show(path)));
    }
    for path in &report.deleted {
        lines.push(format!(
            "  Supprimé         {} (créé par l'agent)",
            show(path)
        ));
    }
    for path in &report.skipped {
        lines.push(format!(
            "  Ignoré           {} (fichier binaire)",
            show(path)
        ));
    }
    lines.push(if rewound {
        "  Conversation     ramenée avant ce tour".to_string()
    } else {
        "  Conversation     inchangée (elle a été compactée depuis)".to_string()
    });
    let mut request_lines = report.request.lines();
    let first_line = request_lines.next().unwrap_or_default();
    let more = if request_lines.next().is_some() {
        " …"
    } else {
        ""
    };
    lines.push(format!("  Votre demande    {first_line}{more}"));
    lines.push(
        "  Remarque         les modifications faites par des commandes shell ne sont pas annulées"
            .to_string(),
    );
    Ok(lines.join("\n"))
}

/// Retours de hooks à montrer (avertissements, `systemMessage`…).
fn print_hook_feedback(event: runtime::HookEvent, result: &runtime::HookRunResult) {
    for message in result.feedback() {
        eprintln!("hook {} : {message}", event.as_str());
    }
}

fn render_hooks_report(config: &runtime::RuntimeHookConfig) -> String {
    let mut lines = vec!["Hooks".to_string()];
    for event in runtime::HookEvent::ALL {
        let entries = config.entries(event);
        if entries.is_empty() {
            continue;
        }
        lines.push(format!(
            "  {:<19} {} hook(s) · blocage : {}",
            event.as_str(),
            entries.len(),
            event.blocking_effect()
        ));
        for entry in entries {
            let matcher = entry
                .matcher
                .as_deref()
                .filter(|matcher| !matcher.is_empty())
                .map(|matcher| format!("[{matcher}] "))
                .unwrap_or_default();
            let timeout = entry
                .timeout_secs
                .map(|seconds| format!(" ({seconds} s max)"))
                .unwrap_or_default();
            let kind = match &entry.kind {
                runtime::HookKind::Command => "",
                runtime::HookKind::Http { .. } => "http ",
                runtime::HookKind::Prompt { .. } => "prompt ",
            };
            let background = if entry.run_async { " (async)" } else { "" };
            lines.push(format!(
                "    {matcher}{kind}{}{timeout}{background}",
                entry.command
            ));
        }
    }
    if lines.len() == 1 {
        lines.push("  Aucun hook configuré.".to_string());
        lines.push(
            "  Exemple (.claw/settings.json) : {\"hooks\": {\"Stop\": [{\"hooks\": [{\"type\": \"command\", \"command\": \"cargo test -q\"}]}]}}"
                .to_string(),
        );
        lines.push(format!(
            "  Événements : {}",
            runtime::HookEvent::ALL
                .iter()
                .map(|event| event.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    lines.join("\n")
}

fn render_checkpoints() -> String {
    let turns = runtime::checkpoint::list();
    if turns.is_empty() {
        return "Points de restauration\n  Aucun tour enregistré dans cette session.".to_string();
    }
    let cwd = env::current_dir().unwrap_or_default();
    let mut lines = vec!["Points de restauration (du plus récent au plus ancien)".to_string()];
    for (index, turn) in turns.iter().rev().enumerate() {
        let files = if turn.files.is_empty() {
            "aucun fichier".to_string()
        } else {
            turn.files
                .iter()
                .map(|file| {
                    file.path
                        .strip_prefix(&cwd)
                        .unwrap_or(&file.path)
                        .display()
                        .to_string()
                })
                .collect::<Vec<_>>()
                .join(", ")
        };
        lines.push(format!(
            "  /undo {:<3} {} → {}",
            index + 1,
            turn.request.lines().next().unwrap_or_default(),
            files
        ));
    }
    lines.join("\n")
}

fn generate_session_id() -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default();
    format!("session-{millis}")
}

fn resolve_session_reference(reference: &str) -> Result<SessionHandle, Box<dyn std::error::Error>> {
    let handle = resolve_session_path(reference)?;
    activate_checkpoints(&handle);
    Ok(handle)
}

fn resolve_session_path(reference: &str) -> Result<SessionHandle, Box<dyn std::error::Error>> {
    let direct = PathBuf::from(reference);
    let path = if direct.exists() {
        direct
    } else {
        sessions_dir()?.join(format!("{reference}.json"))
    };
    if !path.exists() {
        return Err(format!("session introuvable : {reference}").into());
    }
    let id = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or(reference)
        .to_string();
    Ok(SessionHandle { id, path })
}

fn list_managed_sessions() -> Result<Vec<ManagedSessionSummary>, Box<dyn std::error::Error>> {
    let mut sessions = Vec::new();
    for entry in fs::read_dir(sessions_dir()?)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        let metadata = entry.metadata()?;
        let modified_epoch_secs = metadata
            .modified()
            .ok()
            .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
            .map(|duration| duration.as_secs())
            .unwrap_or_default();
        let message_count = Session::load_from_path(&path)
            .map(|session| session.messages.len())
            .unwrap_or_default();
        let id = path
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("unknown")
            .to_string();
        sessions.push(ManagedSessionSummary {
            id,
            path,
            modified_epoch_secs,
            message_count,
        });
    }
    sessions.sort_by(|left, right| right.modified_epoch_secs.cmp(&left.modified_epoch_secs));
    Ok(sessions)
}

/// Titres donnés aux sessions par l'utilisateur : `.claw/session-titles.json`
/// (hors du dossier des sessions, où chaque fichier `.json` est une session).
fn session_titles_path() -> Result<PathBuf, String> {
    let dir = sessions_dir().map_err(|error| error.to_string())?;
    dir.parent()
        .map(|claw_dir| claw_dir.join("session-titles.json"))
        .ok_or_else(|| "dossier .claw introuvable".to_string())
}

fn session_titles() -> BTreeMap<String, String> {
    session_titles_path()
        .ok()
        .and_then(|path| fs::read_to_string(path).ok())
        .and_then(|contents| serde_json::from_str(&contents).ok())
        .unwrap_or_default()
}

fn save_session_titles(titles: &BTreeMap<String, String>) -> Result<(), String> {
    let path = session_titles_path()?;
    let contents = serde_json::to_string_pretty(titles).map_err(|error| error.to_string())?;
    fs::write(path, contents).map_err(|error| format!("titre non enregistré : {error}"))
}

/// Titre sur une ligne, espaces resserrés, 80 caractères au plus.
fn clean_session_title(title: &str) -> String {
    title
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(80)
        .collect()
}

/// Donne un titre à une session (un titre vide rend le titre automatique) ;
/// renvoie le titre enregistré.
fn rename_managed_session(id: &str, title: &str) -> Result<Option<String>, String> {
    if !is_session_id(id) {
        return Err(format!("identifiant de session invalide : {id}"));
    }
    let dir = sessions_dir().map_err(|error| error.to_string())?;
    if !dir.join(format!("{id}.json")).is_file() {
        return Err(format!("session introuvable : {id}"));
    }
    let title = clean_session_title(title);
    let mut titles = session_titles();
    if title.is_empty() {
        titles.remove(id);
    } else {
        titles.insert(id.to_string(), title.clone());
    }
    save_session_titles(&titles)?;
    Ok(Some(title).filter(|title| !title.is_empty()))
}

/// `/rename <titre>` ; sans titre, la session reprend son titre automatique.
fn rename_report(id: &str, title: Option<&str>) -> String {
    match rename_managed_session(id, title.unwrap_or_default()) {
        Ok(Some(title)) => format!("  ⎿  Session renommée : {title}"),
        Ok(None) => "  ⎿  Titre automatique rétabli (tiré du premier message)".to_string(),
        Err(error) => format!("renommage impossible : {error}"),
    }
}

/// Textes des réponses de Claw, de la plus ancienne à la plus récente.
fn assistant_replies(session: &Session) -> Vec<String> {
    session
        .messages
        .iter()
        .filter(|message| message.role == MessageRole::Assistant)
        .map(|message| {
            message
                .blocks
                .iter()
                .filter_map(|block| match block {
                    ContentBlock::Text { text } => Some(text.as_str()),
                    _ => None,
                })
                .collect::<String>()
        })
        .filter(|text| !text.trim().is_empty())
        .collect()
}

/// `/copy [n]` : la n-ième réponse en partant de la fin (1 = la dernière)
/// dans le presse-papiers.
fn copy_assistant_reply(session: &Session, index: Option<&str>) -> String {
    let wanted = match index.map(str::trim).filter(|value| !value.is_empty()) {
        None => 1,
        Some(value) => match value.parse::<usize>() {
            Ok(number) if number >= 1 => number,
            _ => {
                return "Usage : /copy [n] (1 = dernière réponse, 2 = l'avant-dernière…)"
                    .to_string()
            }
        },
    };
    let replies = assistant_replies(session);
    let Some(text) = replies.iter().rev().nth(wanted - 1) else {
        return if replies.is_empty() {
            "  ⎿  Aucune réponse à copier pour l'instant".to_string()
        } else {
            format!(
                "  ⎿  Seulement {} réponse(s) dans cette session",
                replies.len()
            )
        };
    };
    let size = text.chars().count();
    if clipboard::copy_text(text.trim()) {
        format!("  ⎿  Réponse copiée dans le presse-papiers ({size} caractères)")
    } else {
        // Sans outil système, la copie passe par le terminal (OSC 52).
        format!(
            "  ⎿  Réponse envoyée au presse-papiers du terminal ({size} caractères) ; \
si rien n'est copié, votre terminal ne gère pas OSC 52"
        )
    }
}

/// `/output-style [nom|new <nom>]` : rapport, et `true` quand le style actif
/// a changé (le prompt système est alors reconstruit).
fn apply_output_style_command(
    args: Option<&str>,
) -> Result<(String, bool), Box<dyn std::error::Error>> {
    use commands::output_styles;
    let cwd = env::current_dir()?;
    let loader = ConfigLoader::default_for(&cwd);
    let config_home = loader.config_home().to_path_buf();
    let active = loader.load().ok().and_then(|config| {
        config
            .get(output_styles::SETTING_KEY)
            .and_then(|value| value.as_str().map(ToOwned::to_owned))
    });
    let styles = output_styles::discover_styles(&cwd, &config_home);
    let args = args.map(str::trim).filter(|args| !args.is_empty());
    let Some(args) = args.filter(|args| !matches!(*args, "list" | "liste")) else {
        return Ok((
            output_styles::render_styles_report(&styles, active.as_deref()),
            false,
        ));
    };
    if let Some(name) = args
        .strip_prefix("new ")
        .or_else(|| args.strip_prefix("nouveau "))
    {
        let path = output_styles::create_style_file(&cwd, name)?;
        return Ok((
            format!(
                "Style créé\n  Fichier          {}\n  Suite            décrivez le style dans ce fichier, puis /output-style {}",
                path.display(),
                name.trim()
            ),
            false,
        ));
    }
    let Some(style) = output_styles::find_style(&styles, args) else {
        return Ok((
            format!(
                "Style inconnu : {args}\n\n{}",
                output_styles::render_styles_report(&styles, active.as_deref())
            ),
            false,
        ));
    };
    let path = output_styles::persist_style(&cwd, &style.name)?;
    Ok((
        format!(
            "Style de réponse\n  Actif            {} · {}\n  Enregistré dans  {}",
            style.name,
            style.description,
            path.display()
        ),
        true,
    ))
}

/// Action GitHub qui lance Claw dans les routines.
fn routine_action() -> String {
    if let Some(action) = env::var("CLAW_ROUTINE_ACTION")
        .ok()
        .filter(|value| !value.trim().is_empty())
    {
        return action.trim().to_string();
    }
    env::current_dir()
        .ok()
        .and_then(|cwd| ConfigLoader::default_for(cwd).load().ok())
        .and_then(|config| {
            config
                .get("routineAction")
                .and_then(|value| value.as_str().map(|value| value.trim().to_string()))
        })
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| routines::DEFAULT_ACTION.to_string())
}

/// `/schedule` : routines du dépôt, des workflows GitHub Actions qui lancent
/// Claw selon un horaire avec un modèle gratuit.
fn run_schedule_command(args: Option<&str>, model: &str) -> String {
    let action = match routines::parse_command(args) {
        Ok(action) => action,
        Err(error) => return error,
    };
    if action == routines::Action::Help {
        return routines::usage();
    }
    let Ok(repo) = find_git_root() else {
        return "Routines : lancez /schedule dans un dépôt git (une routine est un workflow GitHub Actions du dépôt)."
            .to_string();
    };
    // Git rend des `/` même sous Windows : chemins affichés d'un seul tenant.
    let repo = PathBuf::from(
        repo.display()
            .to_string()
            .replace('/', std::path::MAIN_SEPARATOR_STR),
    );
    match action {
        routines::Action::Help => routines::usage(),
        routines::Action::List => routines::render_list(&routines::list_routines(&repo)),
        routines::Action::Create { schedule, prompt } => {
            let model = routines::routine_model(model);
            match routines::create_routine(&repo, &schedule, &prompt, &model, &routine_action()) {
                Ok((name, workflow, prompt_path)) => format!(
                    "Routine créée\n  Nom              {name}\n  Horaire          {} (cron « {} »)\n  Modèle           {}\n  Workflow         {}\n  Demande          {}\n\nPour l'activer :\n  1. git add .github && git commit -m \"Routine {name}\" && git push\n  2. gh secret set OPENROUTER_API_KEY   (clé gratuite : https://openrouter.ai/keys)\n  3. Settings › Actions › General › cochez « Allow GitHub Actions to create and approve pull requests »\n  Essai immédiat : gh workflow run claw-routine-{name}.yml",
                    schedule.label,
                    schedule.cron,
                    if model.is_empty() { "meilleur modèle gratuit (choisi par l'action)" } else { model.as_str() },
                    workflow.display(),
                    prompt_path.display(),
                ),
                Err(error) => format!("routine impossible : {error}"),
            }
        }
        routines::Action::Delete { name } => match routines::delete_routine(&repo, &name) {
            Ok(removed) => {
                let files = removed
                    .iter()
                    .map(|path| format!("  Supprimé         {}", path.display()))
                    .collect::<Vec<_>>()
                    .join("\n");
                format!("Routine supprimée\n{files}\n  Suite            committez et poussez pour l'arrêter chez GitHub")
            }
            Err(error) => error,
        },
    }
}

/// `/memory imports [allow|deny]` : imports `@chemin` de CLAUDE.md qui sortent
/// du projet (fichiers personnels par exemple), chargés seulement avec accord.
fn memory_imports_command(allow: Option<bool>) -> String {
    let cwd = env::current_dir().unwrap_or_default();
    if let Some(allow) = allow {
        return match runtime::memory::set_external_imports(&cwd, allow) {
            Ok(path) => format!(
                "Imports externes
  Décision         {}
  Enregistrée dans {}",
                if allow {
                    "autorisés pour ce projet"
                } else {
                    "refusés pour ce projet"
                },
                path.display()
            ),
            Err(error) => format!("imports externes : {error}"),
        };
    }
    let _ = ProjectContext::discover(&cwd, runtime::current_date_iso());
    let state = match runtime::memory::external_imports_approved(&cwd) {
        Some(true) => "autorisés",
        Some(false) => "refusés",
        None => "pas encore décidé (non chargés)",
    };
    let skipped = runtime::memory::skipped_external_imports();
    let mut lines = vec![format!(
        "Imports externes
  État             {state}"
    )];
    for path in skipped {
        lines.push(format!("  En attente       {}", path.display()));
    }
    lines.push("  Changer          /memory imports allow · /memory imports deny".to_string());
    lines.join(
        "
",
    )
}

/// Champ `effort` d'un skill.
fn parse_skill_effort(value: &str) -> Option<api::ReasoningEffort> {
    match value.trim().to_ascii_lowercase().as_str() {
        "low" => Some(api::ReasoningEffort::Low),
        "medium" => Some(api::ReasoningEffort::Medium),
        "high" => Some(api::ReasoningEffort::High),
        "xhigh" | "max" => Some(api::ReasoningEffort::Max),
        _ => None,
    }
}

/// `/install-github-action [assistant|review|all] [--force]` : workflows
/// `@claw` et revue des pull requests, avec un modèle gratuit.
fn run_install_github_action(args: Option<&str>, model: &str) -> String {
    let request = match github_setup::parse_request(args) {
        Ok(request) => request,
        Err(error) => return error,
    };
    let Ok(repo) = find_git_root() else {
        return "Lancez /install-github-action dans un dépôt git hébergé sur GitHub.".to_string();
    };
    let repo = PathBuf::from(
        repo.display()
            .to_string()
            .replace('/', std::path::MAIN_SEPARATOR_STR),
    );
    let model = routines::routine_model(model);
    match github_setup::install(&repo, &request, &routine_action(), &model) {
        Ok(outcomes) => github_setup::render_report(&outcomes, &model),
        Err(error) => format!("installation impossible : {error}"),
    }
}

/// Mot de commande sans accents ni majuscules (`Arrêter` → `arreter`).
fn fold_command_word(word: &str) -> String {
    word.trim()
        .chars()
        .map(|character| match character {
            'à' | 'â' | 'À' | 'Â' => 'a',
            'é' | 'è' | 'ê' | 'É' | 'È' | 'Ê' => 'e',
            'î' | 'ï' => 'i',
            'ô' => 'o',
            'û' | 'ù' => 'u',
            other => other.to_ascii_lowercase(),
        })
        .collect()
}

/// Tours au plus pour atteindre un objectif (`CLAW_GOAL_MAX_TURNS`, 12 par défaut).
fn goal_max_rounds() -> usize {
    env::var("CLAW_GOAL_MAX_TURNS")
        .ok()
        .and_then(|value| value.trim().parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(12)
        .min(200)
}

fn goal_start_prompt(condition: &str) -> String {
    format!(
        "[Goal set with /goal. Keep working until this condition is met, verifying it yourself (run the tests, read the files) before you stop: {condition}]\n\nWork toward this goal: {condition}"
    )
}

fn goal_continue_prompt(condition: &str, reason: Option<&str>) -> String {
    let reason = reason
        .map(|reason| format!(" A check found it is not met yet: {reason}."))
        .unwrap_or_default();
    format!(
        "[Goal still open: {condition}.{reason} Continue working toward it; do not repeat what already failed, try another approach if needed, and verify the result before stopping.]"
    )
}

fn goal_check_prompt(condition: &str) -> String {
    format!(
        "[Goal check: do not continue the work and do not use tools. Goal: {condition}\nJudge only from the evidence in this conversation (tool results, test output, files written). On the first line answer exactly MET or NOT MET. On the second line give a one-sentence reason in the user's language; when not met, say what is missing. Without clear evidence, answer NOT MET.]"
    )
}

/// Réponse du contrôle : `MET` / `NOT MET` sur la première ligne, la raison ensuite.
fn parse_goal_verdict(text: &str) -> GoalVerdict {
    let mut lines = text
        .lines()
        .map(|line| line.trim().trim_matches(['*', '#', '`', '_', ' ']))
        .filter(|line| !line.is_empty());
    let first = lines.next().unwrap_or_default();
    let upper = first.to_uppercase().replace(['_', '-'], " ");
    let upper = upper
        .strip_prefix("VERDICT")
        .map_or(upper.as_str(), |rest| rest.trim_start_matches([':', ' ']));
    let rest_of_first = first
        .split_once([':', '\u{2014}', '.'])
        .map(|(_, rest)| rest.trim())
        .unwrap_or_default();
    let reason = lines
        .next()
        .map_or(rest_of_first, str::trim)
        .trim_start_matches(['-', '•', ' '])
        .trim_start_matches("Reason:")
        .trim_start_matches("Raison :")
        .trim();
    let reason: String = if reason.is_empty() {
        "aucune raison donnée".to_string()
    } else {
        reason.chars().take(240).collect()
    };
    let not_met = ["NOT MET", "NON ATTEINT", "PAS ATTEINT", "UNMET"]
        .iter()
        .any(|marker| upper.starts_with(marker));
    let met = ["MET", "ATTEINT", "YES", "OUI"]
        .iter()
        .any(|marker| upper.starts_with(marker));
    if !not_met && met {
        GoalVerdict::Met(reason)
    } else {
        GoalVerdict::NotMet(reason)
    }
}

fn render_goal_status(goal: Option<&GoalState>) -> String {
    let Some(goal) = goal else {
        return "Objectif\n  Aucun objectif en cours\n\nUsage\n  /goal <condition>   Claw continue jusqu'à ce que la condition soit remplie\n  /goal reprendre     relance un objectif en pause\n  /goal clear         efface l'objectif".to_string();
    };
    let state = if goal.paused { "en pause" } else { "actif" };
    let mut report = format!(
        "Objectif\n  Condition        {}\n  État             {state}\n  Tours            {}/{}",
        goal.condition,
        goal.rounds,
        goal_max_rounds()
    );
    if let Some(reason) = &goal.last_reason {
        let _ = write!(report, "\n  Dernier contrôle {reason}");
    }
    report.push_str("\n  Suite            /goal reprendre · /goal clear");
    report
}

/// Passages au plus d'une boucle (`CLAW_LOOP_MAX`, 50 par défaut).
fn loop_max_passes() -> usize {
    env::var("CLAW_LOOP_MAX")
        .ok()
        .and_then(|value| value.trim().parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(50)
}

const LOOP_USAGE: &str = "Usage : /loop [intervalle] <demande ou /commande>
  intervalle       30s, 5m, 1h… (10m par défaut, 30s au moins)
  Exemple          /loop 5m vérifie si le déploiement est terminé
  Arrêt            Échap ou Ctrl+C, ou après CLAW_LOOP_MAX passages (50)";

/// `[intervalle] <demande>` : intervalle facultatif en tête (`30s`, `5m`, `1h`).
fn parse_loop_args(args: Option<&str>) -> Result<(Duration, String), String> {
    let args = args.map(str::trim).unwrap_or_default();
    if args.is_empty() {
        return Err(LOOP_USAGE.to_string());
    }
    let (first, rest) = args.split_once(char::is_whitespace).unwrap_or((args, ""));
    let (interval, request) = match parse_interval(first) {
        Some(interval) => (interval, rest.trim()),
        None => (Duration::from_secs(600), args),
    };
    if request.is_empty() {
        return Err(LOOP_USAGE.to_string());
    }
    if interval < Duration::from_secs(30) {
        return Err(format!(
            "intervalle trop court : 30s au moins\n{LOOP_USAGE}"
        ));
    }
    Ok((interval, request.to_string()))
}

fn parse_interval(token: &str) -> Option<Duration> {
    let token = token.trim().to_lowercase();
    let split = token.find(|character: char| !character.is_ascii_digit())?;
    let (number, unit) = token.split_at(split);
    let number: u64 = number.parse().ok()?;
    let seconds = match unit {
        "s" | "sec" => number,
        "m" | "min" | "mn" => number.checked_mul(60)?,
        "h" => number.checked_mul(3600)?,
        "j" | "d" => number.checked_mul(86_400)?,
        _ => return None,
    };
    Some(Duration::from_secs(seconds))
}

fn format_interval(interval: Duration) -> String {
    let seconds = interval.as_secs();
    if seconds.is_multiple_of(3600) {
        format!("{} h", seconds / 3600)
    } else if seconds.is_multiple_of(60) {
        format!("{} min", seconds / 60)
    } else {
        format!("{seconds} s")
    }
}

fn loop_prompt(request: &str, pass: usize) -> String {
    if pass == 1 {
        request.to_string()
    } else {
        format!(
            "[/loop, pass {pass}: the same request again. Report only what changed since the previous pass; say so briefly if nothing did.]\n\n{request}"
        )
    }
}

/// Minuscule d'un caractère, un pour un : les positions restent alignées
/// avec le texte d'origine.
fn lower_char(ch: char) -> char {
    ch.to_lowercase().next().unwrap_or(ch)
}

/// Extrait d'une ligne autour de la première occurrence de `query`, cherchée
/// sans tenir compte de la casse.
fn search_snippet(text: &str, query: &str) -> Option<String> {
    let query: Vec<char> = query.chars().map(lower_char).collect();
    if query.is_empty() {
        return None;
    }
    let chars: Vec<char> = text.chars().collect();
    let lowered: Vec<char> = chars.iter().copied().map(lower_char).collect();
    let at = lowered
        .windows(query.len())
        .position(|window| window == query.as_slice())?;
    let start = at.saturating_sub(30);
    let end = (at + query.len() + 50).min(chars.len());
    let body: String = chars[start..end]
        .iter()
        .map(|ch| if ch.is_whitespace() { ' ' } else { *ch })
        .collect();
    Some(format!(
        "{}{}{}",
        if start > 0 { "…" } else { "" },
        body.trim(),
        if end < chars.len() { "…" } else { "" }
    ))
}

/// Session trouvée par une recherche, avec l'extrait du message s'il y en a un.
type FoundSession = (ManagedSessionSummary, Option<String>);

/// Sessions du projet qui contiennent `query` : dans leur titre, leur
/// identifiant ou le texte d'un message (alors avec l'extrait trouvé).
fn search_managed_sessions(query: &str) -> Result<Vec<FoundSession>, Box<dyn std::error::Error>> {
    let query = query.trim();
    let titles = session_titles();
    let mut found = Vec::new();
    for session in list_managed_sessions()?.into_iter().take(300) {
        let title = titles.get(&session.id).map_or("", String::as_str);
        if search_snippet(title, query).is_some() || search_snippet(&session.id, query).is_some() {
            found.push((session, None));
            continue;
        }
        let snippet = Session::load_from_path(&session.path)
            .ok()
            .and_then(|saved| {
                saved.messages.iter().find_map(|message| {
                    message.blocks.iter().find_map(|block| match block {
                        ContentBlock::Text { text } => search_snippet(text, query),
                        _ => None,
                    })
                })
            });
        if snippet.is_some() {
            found.push((session, snippet));
        }
    }
    Ok(found)
}

/// Identifiant utilisable sans risque comme nom de fichier de session.
fn is_session_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 80
        && id
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_'))
}

/// Efface une session enregistrée du projet et ses points de restauration.
fn delete_managed_session(id: &str) -> Result<(), String> {
    if !is_session_id(id) {
        return Err(format!("identifiant de session invalide : {id}"));
    }
    let dir = sessions_dir().map_err(|error| error.to_string())?;
    let path = dir.join(format!("{id}.json"));
    if !path.is_file() {
        return Err(format!("session introuvable : {id}"));
    }
    fs::remove_file(&path).map_err(|error| format!("effacement impossible : {error}"))?;
    if let Some(claw_dir) = dir.parent() {
        let _ = fs::remove_file(claw_dir.join("checkpoints").join(format!("{id}.json")));
    }
    let mut titles = session_titles();
    if titles.remove(id).is_some() {
        let _ = save_session_titles(&titles);
    }
    Ok(())
}

/// Efface toutes les sessions enregistrées du projet sauf `keep` (celle en
/// cours) ; renvoie leur nombre.
fn delete_other_sessions(keep: &str) -> Result<usize, String> {
    let sessions = list_managed_sessions().map_err(|error| error.to_string())?;
    let mut deleted = 0;
    for session in sessions.iter().filter(|session| session.id != keep) {
        delete_managed_session(&session.id)?;
        deleted += 1;
    }
    Ok(deleted)
}

/// Dernière session non vide du projet (`claw --continue`).
fn latest_session() -> Result<Option<PathBuf>, Box<dyn std::error::Error>> {
    let latest = list_managed_sessions()?
        .into_iter()
        .find(|session| session.message_count > 0)
        .map(|session| session.path);
    if latest.is_none() {
        eprintln!("Aucune session à reprendre dans ce projet : nouvelle session.");
    }
    Ok(latest)
}

/// Première demande d'une session, pour la reconnaître dans la liste.
fn session_preview(path: &Path) -> String {
    Session::load_from_path(path)
        .ok()
        .and_then(|session| {
            session.messages.iter().find_map(|message| {
                (message.role == MessageRole::User).then(|| {
                    message.blocks.iter().find_map(|block| match block {
                        ContentBlock::Text { text } => {
                            Some(text.lines().next().unwrap_or("").to_string())
                        }
                        _ => None,
                    })
                })?
            })
        })
        .map(|text| {
            let short: String = text.chars().take(70).collect();
            if text.chars().count() > 70 {
                format!("{short}…")
            } else {
                short
            }
        })
        .unwrap_or_default()
}

/// `claw --resume` sans chemin : liste numérotée des sessions du projet.
fn pick_session() -> Result<Option<PathBuf>, Box<dyn std::error::Error>> {
    let sessions = list_managed_sessions()?
        .into_iter()
        .filter(|session| session.message_count > 0)
        .take(15)
        .collect::<Vec<_>>();
    if sessions.is_empty() {
        eprintln!("Aucune session à reprendre dans ce projet : nouvelle session.");
        return Ok(None);
    }
    println!("Sessions récentes");
    let titles = session_titles();
    for (index, session) in sessions.iter().enumerate() {
        println!(
            "  {:>2}. {:<14} {:>3} msgs  {}",
            index + 1,
            format_relative_timestamp(session.modified_epoch_secs),
            session.message_count,
            titles
                .get(&session.id)
                .cloned()
                .unwrap_or_else(|| session_preview(&session.path))
        );
    }
    print!("Numéro de la session (Entrée pour une nouvelle) : ");
    io::stdout().flush()?;
    let mut answer = String::new();
    io::stdin().read_line(&mut answer)?;
    Ok(answer
        .trim()
        .parse::<usize>()
        .ok()
        .and_then(|number| number.checked_sub(1))
        .and_then(|index| sessions.get(index))
        .map(|session| session.path.clone()))
}

fn format_relative_timestamp(epoch_secs: u64) -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(epoch_secs);
    let elapsed = now.saturating_sub(epoch_secs);
    match elapsed {
        0..=59 => format!("il y a {elapsed} s"),
        60..=3_599 => format!("il y a {} min", elapsed / 60),
        3_600..=86_399 => format!("il y a {} h", elapsed / 3_600),
        _ => format!("il y a {} j", elapsed / 86_400),
    }
}

fn render_session_list(active_session_id: &str) -> Result<String, Box<dyn std::error::Error>> {
    let sessions = list_managed_sessions()?;
    let mut lines = vec![
        "Sessions".to_string(),
        format!("  Dossier           {}", sessions_dir()?.display()),
    ];
    if sessions.is_empty() {
        lines.push("  Aucune session enregistrée pour l'instant.".to_string());
        return Ok(lines.join("\n"));
    }
    let titles = session_titles();
    for session in sessions {
        let marker = if session.id == active_session_id {
            "● active"
        } else {
            "○ sauvée"
        };
        lines.push(format!(
            "  {id:<20} {marker:<10} {msgs:>3} msgs · modifiée {modified}",
            id = session.id,
            msgs = session.message_count,
            modified = format_relative_timestamp(session.modified_epoch_secs),
        ));
        // Titre donné par l'utilisateur, sinon début de la première demande.
        let label = titles
            .get(&session.id)
            .cloned()
            .unwrap_or_else(|| session_preview(&session.path));
        if !label.is_empty() {
            lines.push(format!("    « {label} »"));
        }
        lines.push(format!("    {}", session.path.display()));
    }
    Ok(lines.join("\n"))
}

/// `/nom arguments` → demande développée d'une commande personnalisée
/// (`.claw/commands/nom.md`…), ou `None` si aucune ne porte ce nom.
fn custom_command_prompt(input: &str) -> Option<Result<commands::CustomCommandInvocation, String>> {
    let cwd = env::current_dir().ok()?;
    let invocation = commands::resolve_custom_command(&cwd, input)?;
    Some(invocation.inspect(|invocation| {
        let source = invocation
            .command
            .path
            .strip_prefix(&cwd)
            .unwrap_or(&invocation.command.path)
            .display()
            .to_string();
        let model = invocation
            .command
            .model
            .as_deref()
            .map(|model| format!(" · modèle {model}"))
            .unwrap_or_default();
        eprintln!(
            "↳ /{} ({source}){model}",
            invocation.command.qualified_name()
        );
    }))
}

/// Outils de `allowed-tools` (Claude Code) → noms d'outils de Claw.
fn custom_allowed_tools(tools: &[String]) -> Option<AllowedToolSet> {
    if tools.is_empty() {
        return None;
    }
    let names = tools
        .iter()
        .map(|tool| tool.split('(').next().unwrap_or(tool).trim().to_string())
        .collect::<Vec<_>>();
    match normalize_allowed_tools(&[names.join(",")]) {
        Ok(set) => set,
        Err(error) => {
            eprintln!("▲ allowed-tools ignoré : {error}");
            None
        }
    }
}

/// Note ajoutée à la mémoire du projet (`#note` dans le REPL) : fin de CLAW.md.
fn append_project_note(note: &str) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let path = env::current_dir()?.join("CLAW.md");
    let mut contents = fs::read_to_string(&path).unwrap_or_default();
    if contents.trim().is_empty() {
        contents = "# CLAW.md\n\nConsignes du projet pour l'agent.\n".to_string();
    }
    if !contents.contains("\n## Notes") {
        if !contents.ends_with('\n') {
            contents.push('\n');
        }
        contents.push_str("\n## Notes\n");
    }
    if !contents.ends_with('\n') {
        contents.push('\n');
    }
    let _ = writeln!(contents, "- {note}");
    fs::write(&path, contents)?;
    Ok(path)
}

/// `!commande` dans le REPL : exécutée directement, sortie affichée et
/// transmise au modèle avec la demande suivante.
fn run_user_shell_command(command: &str) -> String {
    let output = plugins::posix_shell_command(command)
        .current_dir(env::current_dir().unwrap_or_default())
        .stdin(std::process::Stdio::null())
        .output();
    match output {
        Ok(output) => {
            let mut text = String::from_utf8_lossy(&output.stdout).to_string();
            let stderr = String::from_utf8_lossy(&output.stderr);
            if !stderr.trim().is_empty() {
                text.push_str(&stderr);
            }
            let code = output.status.code().unwrap_or(-1);
            print!("{text}");
            if !text.ends_with('\n') && !text.is_empty() {
                println!();
            }
            if code != 0 {
                println!("(code de sortie {code})");
            }
            let shown: String = text.chars().take(20_000).collect();
            format!(
                "The user ran this shell command themselves (exit code {code}):\n$ {command}\n```\n{}\n```",
                shown.trim_end()
            )
        }
        Err(error) => {
            eprintln!("commande impossible : {error}");
            format!("The user tried to run `{command}`, which failed to start: {error}")
        }
    }
}

const INIT_ANALYSIS_PROMPT: &str = "/init - Analyze this repository and write (or improve) CLAW.md at its root: the file of instructions that every future session will read first. Explore the codebase with read and search tools, then include: a one-paragraph overview of the project; the exact commands to build, run, test (including a single test) and lint; the architecture (main modules and how they fit together, key entry points); conventions you observe (style, naming, error handling, test layout); and pitfalls a new contributor should know. Keep it concise and factual (no generic advice), prefer bullet lists, keep any existing useful content, and do not invent commands you have not seen in the project files. Write the file in the language the project already uses for its documentation.";

fn render_repl_help() -> String {
    [
        "REPL interactif".to_string(),
        "  Pour commencer       décrivez une tâche en langage courant, ou utilisez les commandes ci-dessous."
            .to_string(),
        "  Essentiel            /help · /status · /model · /permissions · /compact".to_string(),
        "  Travail continu      /goal <condition> · /loop 5m <demande> · /schedule (routines GitHub)".to_string(),
        "  Sans interrompre     /btw <question> (aparté) · /recap (bilan) · /copy (dernière réponse)".to_string(),
        "  Quitter              exit, /exit ou /quit".to_string(),
        "  Mode                 Maj+Tab : demander → éditions acceptées → plan".to_string(),
        "  Effacer l'écran      clear (la conversation est gardée)".to_string(),
        "  Commandes            « / » ouvre le menu : ↑↓ choisit, Tab complète, Entrée lance".to_string(),
        "  Détails              Ctrl+O montre le dernier tour en entier (sorties, raisonnement)".to_string(),
        "  Revenir en arrière   Échap Échap (saisie vide) : reprendre avant un message précédent".to_string(),
        "  Image                Alt+V joint l'image du presse-papiers".to_string(),
        "  Diagnostic           /doctor vérifie clés, modèles, Ollama, Git, shell".to_string(),
        "  Mode vim             /vim active l'édition modale".to_string(),
        "  Historique           Haut/Bas rappelle les demandes précédentes".to_string(),
        "  Complétion           Tab parcourt les commandes correspondantes".to_string(),
        "  Annuler la saisie    Ctrl+C efface la ligne (ou quitte si elle est vide)".to_string(),
        "  Interrompre          Ctrl+C pendant un tour arrête l'agent ; /undo annule le dernier tour".to_string(),
        "  Multiligne           Maj+Entrée ou Ctrl+J insère un saut de ligne".to_string(),
        String::new(),
        render_slash_command_help(),
        String::new(),
        commands::render_custom_commands_help(&env::current_dir().unwrap_or_default()),
    ]
    .join(
        "
",
    )
}

fn append_slash_command_suggestions(lines: &mut Vec<String>, name: &str) {
    let suggestions = suggest_slash_commands(name, 3);
    if suggestions.is_empty() {
        lines.push("  Essayez          /help liste toutes les commandes".to_string());
        return;
    }

    lines.push("  Essayez          /help liste toutes les commandes".to_string());
    lines.push("Suggestions".to_string());
    lines.extend(
        suggestions
            .into_iter()
            .map(|suggestion| format!("  {suggestion}")),
    );
}

fn render_unknown_repl_command(name: &str) -> String {
    let mut lines = vec![
        "Commande slash inconnue".to_string(),
        format!("  Commande         /{name}"),
    ];
    append_repl_command_suggestions(&mut lines, name);
    lines.join("\n")
}

fn append_repl_command_suggestions(lines: &mut Vec<String>, name: &str) {
    let suggestions = suggest_repl_commands(name);
    if suggestions.is_empty() {
        lines.push("  Essayez          /help liste toutes les commandes".to_string());
        return;
    }

    lines.push("  Essayez          /help liste toutes les commandes".to_string());
    lines.push("Suggestions".to_string());
    lines.extend(
        suggestions
            .into_iter()
            .map(|suggestion| format!("  {suggestion}")),
    );
}

fn status_context(
    session_path: Option<&Path>,
) -> Result<StatusContext, Box<dyn std::error::Error>> {
    let cwd = env::current_dir()?;
    let loader = ConfigLoader::default_for(&cwd);
    let discovered_config_files = loader.discover().len();
    let runtime_config = loader.load()?;
    let project_context = ProjectContext::discover_with_git(&cwd, runtime::current_date_iso())?;
    let (project_root, git_branch) =
        parse_git_status_metadata(project_context.git_status.as_deref());
    Ok(StatusContext {
        cwd,
        session_path: session_path.map(Path::to_path_buf),
        loaded_config_files: runtime_config.loaded_entries().len(),
        discovered_config_files,
        memory_file_count: project_context.instruction_files.len(),
        project_root,
        git_branch,
    })
}

fn format_status_report(
    model: &str,
    usage: StatusUsage,
    permission_mode: &str,
    context: &StatusContext,
) -> String {
    [
        format!(
            "Session
  Modèle           {model}
  Permissions      {permission_mode}
  Activité         {} messages · {} tours
  Jetons           estimés {} · dernier tour {} · total {}",
            usage.message_count,
            usage.turns,
            usage.estimated_tokens,
            usage.latest.total_tokens(),
            usage.cumulative.total_tokens(),
        ),
        format!(
            "Consommation
  Entrée (cumul)   {}
  Sortie (cumul)   {}
  Cache (écriture) {}
  Cache (lecture)  {}",
            usage.cumulative.input_tokens,
            usage.cumulative.output_tokens,
            usage.cumulative.cache_creation_input_tokens,
            usage.cumulative.cache_read_input_tokens,
        ),
        format!(
            "Espace de travail
  Dossier          {}
  Racine du projet {}
  Branche git      {}
  Fichier session  {}
  Configuration    {}/{} fichiers chargés
  Instructions     {} fichier(s) CLAW.md

Suite
  /help            Toutes les commandes
  /session list    Sessions enregistrées
  /diff            Modifications en cours",
            context.cwd.display(),
            context
                .project_root
                .as_ref()
                .map_or_else(|| "inconnue".to_string(), |path| path.display().to_string()),
            context.git_branch.as_deref().unwrap_or("inconnue"),
            context.session_path.as_ref().map_or_else(
                || "live-repl".to_string(),
                |path| path.display().to_string()
            ),
            context.loaded_config_files,
            context.discovered_config_files,
            context.memory_file_count,
        ),
    ]
    .join(
        "

",
    )
}

fn render_config_report(section: Option<&str>) -> Result<String, Box<dyn std::error::Error>> {
    let cwd = env::current_dir()?;
    let loader = ConfigLoader::default_for(&cwd);
    let discovered = loader.discover();
    let runtime_config = loader.load()?;

    let mut lines = vec![
        format!(
            "Configuration
  Dossier           {}
  Fichiers chargés  {}
  Clés fusionnées   {}",
            cwd.display(),
            runtime_config.loaded_entries().len(),
            runtime_config.merged().len()
        ),
        "Fichiers recherchés".to_string(),
    ];
    for entry in discovered {
        let source = match (entry.source, entry.foreign) {
            (ConfigSource::User, false) => "utilisateur",
            (ConfigSource::Project, false) => "projet",
            (ConfigSource::Local, false) => "local",
            (ConfigSource::User, true) => "utilisateur (Claude Code)",
            (ConfigSource::Project, true) => "projet (Claude Code)",
            (ConfigSource::Local, true) => "local (Claude Code)",
        };
        let status = if runtime_config
            .loaded_entries()
            .iter()
            .any(|loaded_entry| loaded_entry.path == entry.path)
        {
            "chargé"
        } else {
            "absent"
        };
        lines.push(format!(
            "  {source:<25} {status:<7} {}",
            entry.path.display()
        ));
    }

    if let Some(section) = section {
        lines.push(format!("Section fusionnée : {section}"));
        let value = match section {
            "env" => runtime_config.get("env"),
            "hooks" => runtime_config.get("hooks"),
            "model" => runtime_config.get("model"),
            "plugins" => runtime_config
                .get("plugins")
                .or_else(|| runtime_config.get("enabledPlugins")),
            other => {
                lines.push(format!(
                    "  Section « {other} » non prise en charge. Utilisez env, hooks, model ou plugins."
                ));
                return Ok(lines.join(
                    "
",
                ));
            }
        };
        lines.push(format!(
            "  {}",
            match value {
                Some(value) => value.render(),
                None => "<non défini>".to_string(),
            }
        ));
        if section == "env" && !runtime_config.ignored_env().is_empty() {
            lines.push(format!(
                "  Ignorées          {} (un fichier de projet ne peut pas les fixer : ~/.claw/settings.json ou le shell)",
                runtime_config.ignored_env().join(", ")
            ));
        }
        return Ok(lines.join(
            "
",
        ));
    }

    lines.push("JSON fusionné".to_string());
    lines.push(format!("  {}", runtime_config.as_json().render()));
    Ok(lines.join(
        "
",
    ))
}

fn render_memory_report() -> Result<String, Box<dyn std::error::Error>> {
    let cwd = env::current_dir()?;
    let project_context = ProjectContext::discover(&cwd, runtime::current_date_iso())?;
    let mut lines = vec![format!(
        "Mémoire (instructions du projet)
  Dossier           {}
  Fichiers trouvés  {}",
        cwd.display(),
        project_context.instruction_files.len()
    )];
    if project_context.instruction_files.is_empty() {
        lines.push("Fichiers".to_string());
        lines.push(
            "  Aucun fichier CLAW.md dans ce dossier ni ses parents (/init en crée un)."
                .to_string(),
        );
    } else {
        lines.push("Fichiers".to_string());
        for (index, file) in project_context.instruction_files.iter().enumerate() {
            let preview = file.content.lines().next().unwrap_or("").trim();
            let preview = if preview.is_empty() {
                "<vide>"
            } else {
                preview
            };
            lines.push(format!("  {}. {}", index + 1, file.path.display(),));
            lines.push(format!(
                "     lignes={} aperçu={}",
                file.content.lines().count(),
                preview
            ));
        }
    }
    let rules = runtime::memory::path_rules();
    if !rules.is_empty() {
        lines.push(
            "Règles limitées à des chemins (données au premier fichier correspondant)".to_string(),
        );
        for rule in rules {
            lines.push(format!(
                "  {} · {}",
                rule.patterns.join(", "),
                rule.path.display()
            ));
        }
    }
    let skipped = runtime::memory::skipped_external_imports();
    if !skipped.is_empty() {
        lines.push("Imports externes en attente (hors du projet, non chargés)".to_string());
        for path in skipped {
            lines.push(format!("  {}", path.display()));
        }
        lines.push(
            "  Autoriser        /memory imports allow · refuser : /memory imports deny".to_string(),
        );
    }
    Ok(lines.join(
        "
",
    ))
}

fn init_claw_md() -> Result<String, Box<dyn std::error::Error>> {
    let cwd = env::current_dir()?;
    Ok(initialize_repo(&cwd)?.render())
}

fn run_init() -> Result<(), Box<dyn std::error::Error>> {
    println!("{}", init_claw_md()?);
    Ok(())
}

/// Accepte aussi les noms de modes de Claude Code (default, plan, acceptEdits,
/// bypassPermissions) et les ramène aux libellés de Claw.
fn normalize_permission_mode(mode: &str) -> Option<&'static str> {
    match mode.trim() {
        "prompt" | "default" | "ask" => Some("prompt"),
        "read-only" | "plan" => Some("read-only"),
        "workspace-write" | "acceptEdits" | "accept-edits" => Some("workspace-write"),
        "danger-full-access" | "bypassPermissions" => Some("danger-full-access"),
        // `dontAsk` refuse ce qui n'est pas autorisé d'avance (et non l'inverse).
        "dont-ask" | "dontAsk" => Some("dont-ask"),
        "auto" => Some("auto"),
        _ => None,
    }
}

fn render_diff_report() -> Result<String, Box<dyn std::error::Error>> {
    let output = std::process::Command::new("git")
        .args(["diff", "--", ":(exclude).omx"])
        .current_dir(env::current_dir()?)
        .output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(format!("échec de git diff : {stderr}").into());
    }
    let diff = String::from_utf8(output.stdout)?;
    if diff.trim().is_empty() {
        return Ok(
            "Diff\n  Résultat         arbre de travail propre\n  Détail           aucune modification en cours"
                .to_string(),
        );
    }
    Ok(format!("Diff\n\n{}", diff.trim_end()))
}

fn render_teleport_report(target: &str) -> Result<String, Box<dyn std::error::Error>> {
    let cwd = env::current_dir()?;

    let file_list = Command::new("rg")
        .args(["--files"])
        .current_dir(&cwd)
        .output()?;
    let file_matches = if file_list.status.success() {
        String::from_utf8(file_list.stdout)?
            .lines()
            .filter(|line| line.contains(target))
            .take(10)
            .map(ToOwned::to_owned)
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };

    let content_output = Command::new("rg")
        .args(["-n", "-S", "--color", "never", target, "."])
        .current_dir(&cwd)
        .output()?;

    let mut lines = vec![format!("Teleport\n  Cible            {target}")];
    if !file_matches.is_empty() {
        lines.push(String::new());
        lines.push("Fichiers correspondants".to_string());
        lines.extend(file_matches.into_iter().map(|path| format!("  {path}")));
    }

    if content_output.status.success() {
        let matches = String::from_utf8(content_output.stdout)?;
        if !matches.trim().is_empty() {
            lines.push(String::new());
            lines.push("Contenu correspondant".to_string());
            lines.push(truncate_for_prompt(&matches, 4_000));
        }
    }

    if lines.len() == 1 {
        lines.push("  Résultat         aucune correspondance".to_string());
    }

    Ok(lines.join("\n"))
}

fn render_last_tool_debug_report(session: &Session) -> Result<String, Box<dyn std::error::Error>> {
    let last_tool_use = session
        .messages
        .iter()
        .rev()
        .find_map(|message| {
            message.blocks.iter().rev().find_map(|block| match block {
                ContentBlock::ToolUse { id, name, input } => {
                    Some((id.clone(), name.clone(), input.clone()))
                }
                _ => None,
            })
        })
        .ok_or_else(|| "aucun appel d'outil dans cette session".to_string())?;

    let tool_result = session.messages.iter().rev().find_map(|message| {
        message.blocks.iter().rev().find_map(|block| match block {
            ContentBlock::ToolResult {
                tool_use_id,
                tool_name,
                output,
                is_error,
            } if tool_use_id == &last_tool_use.0 => {
                Some((tool_name.clone(), output.clone(), *is_error))
            }
            _ => None,
        })
    });

    let mut lines = vec![
        "Dernier appel d'outil".to_string(),
        format!("  Identifiant      {}", last_tool_use.0),
        format!("  Outil            {}", last_tool_use.1),
        "  Entrée".to_string(),
        indent_block(&last_tool_use.2, 4),
    ];

    match tool_result {
        Some((tool_name, output, is_error)) => {
            lines.push("  Résultat".to_string());
            lines.push(format!("    outil          {tool_name}"));
            lines.push(format!(
                "    statut         {}",
                if is_error { "erreur" } else { "ok" }
            ));
            lines.push(indent_block(&output, 4));
        }
        None => lines.push("  Résultat         absent".to_string()),
    }

    Ok(lines.join("\n"))
}

fn indent_block(value: &str, spaces: usize) -> String {
    let indent = " ".repeat(spaces);
    value
        .lines()
        .map(|line| format!("{indent}{line}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn git_output(args: &[&str]) -> Result<String, Box<dyn std::error::Error>> {
    let output = Command::new("git")
        .args(args)
        .current_dir(env::current_dir()?)
        .output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(format!("échec de git {} : {stderr}", args.join(" ")).into());
    }
    Ok(String::from_utf8(output.stdout)?)
}

fn git_status_ok(args: &[&str]) -> Result<(), Box<dyn std::error::Error>> {
    let output = Command::new("git")
        .args(args)
        .current_dir(env::current_dir()?)
        .output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(format!("échec de git {} : {stderr}", args.join(" ")).into());
    }
    Ok(())
}

fn command_exists(name: &str) -> bool {
    Command::new(if cfg!(windows) { "where" } else { "which" })
        .arg(name)
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

fn write_temp_text_file(
    filename: &str,
    contents: &str,
) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let path = env::temp_dir().join(filename);
    fs::write(&path, contents)?;
    Ok(path)
}

fn recent_user_context(session: &Session, limit: usize) -> String {
    let requests = session
        .messages
        .iter()
        .filter(|message| message.role == MessageRole::User)
        .filter_map(|message| {
            message.blocks.iter().find_map(|block| match block {
                ContentBlock::Text { text } => Some(text.trim().to_string()),
                _ => None,
            })
        })
        .rev()
        .take(limit)
        .collect::<Vec<_>>();

    if requests.is_empty() {
        "<no prior user messages>".to_string()
    } else {
        requests
            .into_iter()
            .rev()
            .enumerate()
            .map(|(index, text)| format!("{}. {}", index + 1, text))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

fn truncate_for_prompt(value: &str, limit: usize) -> String {
    if value.chars().count() <= limit {
        value.trim().to_string()
    } else {
        let truncated = value.chars().take(limit).collect::<String>();
        format!("{}\n…[truncated]", truncated.trim_end())
    }
}

fn sanitize_generated_message(value: &str) -> String {
    value.trim().trim_matches('`').trim().replace("\r\n", "\n")
}

fn parse_titled_body(value: &str) -> Option<(String, String)> {
    let normalized = sanitize_generated_message(value);
    let title = normalized
        .lines()
        .find_map(|line| line.strip_prefix("TITLE:").map(str::trim))?;
    let body_start = normalized.find("BODY:")?;
    let body = normalized[body_start + "BODY:".len()..].trim();
    Some((title.to_string(), body.to_string()))
}

/// « 0.1.0 · commit 8d78bcc » (diagnostic).
fn version_summary() -> String {
    format!("{VERSION} · commit {}", GIT_SHA.unwrap_or("inconnu"))
}

fn render_version_report() -> String {
    let git_sha = GIT_SHA.unwrap_or("inconnu");
    let target = BUILD_TARGET.unwrap_or("inconnue");
    format!(
        "Claw Code\n  Version          {VERSION}\n  Commit git       {git_sha}\n  Cible            {target}\n  Date de build    {DEFAULT_DATE}\n\nAide\n  Terminal         claw --help\n  REPL             /help"
    )
}

fn render_export_text(session: &Session) -> String {
    let mut lines = vec!["# Export de conversation".to_string(), String::new()];
    for (index, message) in session.messages.iter().enumerate() {
        let role = match message.role {
            MessageRole::System => "system",
            MessageRole::User => "user",
            MessageRole::Assistant => "assistant",
            MessageRole::Tool => "tool",
        };
        lines.push(format!("## {}. {role}", index + 1));
        for block in &message.blocks {
            match block {
                ContentBlock::Text { text } => lines.push(text.clone()),
                ContentBlock::ToolUse { id, name, input } => {
                    lines.push(format!("[tool_use id={id} name={name}] {input}"));
                }
                ContentBlock::ToolResult {
                    tool_use_id,
                    tool_name,
                    output,
                    is_error,
                } => {
                    lines.push(format!(
                        "[tool_result id={tool_use_id} name={tool_name} error={is_error}] {output}"
                    ));
                }
                ContentBlock::Image { media_type, .. } => {
                    lines.push(format!("[image {media_type}]"));
                }
                ContentBlock::Thinking { thinking, .. } => {
                    lines.push(format!("[raisonnement] {thinking}"));
                }
            }
        }
        lines.push(String::new());
    }
    lines.join("\n")
}

fn default_export_filename(session: &Session) -> String {
    let stem = session
        .messages
        .iter()
        .find_map(|message| match message.role {
            MessageRole::User => message.blocks.iter().find_map(|block| match block {
                ContentBlock::Text { text } => Some(text.as_str()),
                _ => None,
            }),
            _ => None,
        })
        .map_or("conversation", |text| {
            text.lines().next().unwrap_or("conversation")
        })
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .split('-')
        .filter(|part| !part.is_empty())
        .take(8)
        .collect::<Vec<_>>()
        .join("-");
    let fallback = if stem.is_empty() {
        "conversation"
    } else {
        &stem
    };
    format!("{fallback}.txt")
}

fn resolve_export_path(
    requested_path: Option<&str>,
    session: &Session,
) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let cwd = env::current_dir()?;
    let file_name =
        requested_path.map_or_else(|| default_export_filename(session), ToOwned::to_owned);
    let final_name = if Path::new(&file_name)
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("txt"))
    {
        file_name
    } else {
        format!("{file_name}.txt")
    };
    Ok(cwd.join(final_name))
}

fn build_system_prompt() -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let cwd = env::current_dir()?;
    let mut prompt = load_system_prompt(
        cwd.clone(),
        runtime::current_date_iso(),
        env::consts::OS,
        "unknown",
    )?;
    if let Some(skills) = commands::skills_prompt_section(&cwd) {
        prompt.push(skills);
    }
    // Style de réponse choisi avec /output-style (clé `outputStyle`).
    let loader = ConfigLoader::default_for(&cwd);
    let configured = loader.load().ok().and_then(|config| {
        config
            .get(commands::output_styles::SETTING_KEY)
            .and_then(|value| value.as_str().map(ToOwned::to_owned))
    });
    if let Some(style) = commands::output_styles::configured_section(
        &cwd,
        loader.config_home(),
        configured.as_deref(),
    ) {
        prompt.push(style);
    }
    Ok(prompt)
}

fn build_runtime_plugin_state(
) -> Result<(runtime::RuntimeFeatureConfig, GlobalToolRegistry), Box<dyn std::error::Error>> {
    let cwd = env::current_dir()?;
    let loader = ConfigLoader::default_for(&cwd);
    let runtime_config = loader.load()?;
    let plugin_manager = build_plugin_manager(&cwd, &loader, &runtime_config);
    // Les serveurs MCP sont connectés une fois puis réutilisés (cache par config).
    let tool_registry = GlobalToolRegistry::with_plugin_tools(plugin_manager.aggregated_tools()?)?
        .with_mcp_tools(tools::ensure_mcp_started(&runtime_config));
    let plugin_hooks = plugin_manager.aggregated_hooks().unwrap_or_default();
    let hooks = runtime_config
        .feature_config()
        .hooks()
        .merged(&runtime::RuntimeHookConfig::new(
            plugin_hooks.pre_tool_use,
            plugin_hooks.post_tool_use,
        ));
    Ok((
        runtime_config.feature_config().clone().with_hooks(hooks),
        tool_registry,
    ))
}

fn build_plugin_manager(
    cwd: &Path,
    loader: &ConfigLoader,
    runtime_config: &runtime::RuntimeConfig,
) -> PluginManager {
    let plugin_settings = runtime_config.plugins();
    let mut plugin_config = PluginManagerConfig::new(loader.config_home().to_path_buf());
    plugin_config.enabled_plugins = plugin_settings.enabled_plugins().clone();
    plugin_config.external_dirs = plugin_settings
        .external_directories()
        .iter()
        .map(|path| resolve_plugin_path(cwd, loader.config_home(), path))
        .collect();
    plugin_config.install_root = plugin_settings
        .install_root()
        .map(|path| resolve_plugin_path(cwd, loader.config_home(), path));
    plugin_config.registry_path = plugin_settings
        .registry_path()
        .map(|path| resolve_plugin_path(cwd, loader.config_home(), path));
    plugin_config.bundled_root = plugin_settings
        .bundled_root()
        .map(|path| resolve_plugin_path(cwd, loader.config_home(), path));
    PluginManager::new(plugin_config)
}

fn resolve_plugin_path(cwd: &Path, config_home: &Path, value: &str) -> PathBuf {
    let path = PathBuf::from(value);
    if path.is_absolute() {
        path
    } else if value.starts_with('.') {
        cwd.join(path)
    } else {
        config_home.join(path)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct InternalPromptProgressState {
    command_label: &'static str,
    task_label: String,
    step: usize,
    phase: String,
    detail: Option<String>,
    saw_final_text: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InternalPromptProgressEvent {
    Started,
    Update,
    Heartbeat,
    Complete,
    Failed,
}

#[derive(Debug)]
struct InternalPromptProgressShared {
    state: Mutex<InternalPromptProgressState>,
    output_lock: Mutex<()>,
    started_at: Instant,
}

#[derive(Debug, Clone)]
struct InternalPromptProgressReporter {
    shared: Arc<InternalPromptProgressShared>,
}

#[derive(Debug)]
struct InternalPromptProgressRun {
    reporter: InternalPromptProgressReporter,
    heartbeat_stop: Option<mpsc::Sender<()>>,
    heartbeat_handle: Option<thread::JoinHandle<()>>,
}

impl InternalPromptProgressReporter {
    fn ultraplan(task: &str) -> Self {
        Self {
            shared: Arc::new(InternalPromptProgressShared {
                state: Mutex::new(InternalPromptProgressState {
                    command_label: "Ultraplan",
                    task_label: task.to_string(),
                    step: 0,
                    phase: "planification démarrée".to_string(),
                    detail: Some(format!("task: {task}")),
                    saw_final_text: false,
                }),
                output_lock: Mutex::new(()),
                started_at: Instant::now(),
            }),
        }
    }

    fn emit(&self, event: InternalPromptProgressEvent, error: Option<&str>) {
        let snapshot = self.snapshot();
        let line = format_internal_prompt_progress_line(event, &snapshot, self.elapsed(), error);
        self.write_line(&line);
    }

    fn mark_model_phase(&self) {
        let snapshot = {
            let mut state = self
                .shared
                .state
                .lock()
                .expect("internal prompt progress state poisoned");
            state.step += 1;
            state.phase = if state.step == 1 {
                "analyse de la demande".to_string()
            } else {
                "examen des résultats".to_string()
            };
            state.detail = Some(format!("tâche : {}", state.task_label));
            state.clone()
        };
        self.write_line(&format_internal_prompt_progress_line(
            InternalPromptProgressEvent::Update,
            &snapshot,
            self.elapsed(),
            None,
        ));
    }

    fn mark_tool_phase(&self, name: &str, input: &str) {
        let detail = describe_tool_progress(name, input);
        let snapshot = {
            let mut state = self
                .shared
                .state
                .lock()
                .expect("internal prompt progress state poisoned");
            state.step += 1;
            state.phase = format!("outil {name}");
            state.detail = Some(detail);
            state.clone()
        };
        self.write_line(&format_internal_prompt_progress_line(
            InternalPromptProgressEvent::Update,
            &snapshot,
            self.elapsed(),
            None,
        ));
    }

    fn mark_text_phase(&self, text: &str) {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return;
        }
        let detail = truncate_for_summary(first_visible_line(trimmed), 120);
        let snapshot = {
            let mut state = self
                .shared
                .state
                .lock()
                .expect("internal prompt progress state poisoned");
            if state.saw_final_text {
                return;
            }
            state.saw_final_text = true;
            state.step += 1;
            state.phase = "rédaction du plan final".to_string();
            state.detail = (!detail.is_empty()).then_some(detail);
            state.clone()
        };
        self.write_line(&format_internal_prompt_progress_line(
            InternalPromptProgressEvent::Update,
            &snapshot,
            self.elapsed(),
            None,
        ));
    }

    fn emit_heartbeat(&self) {
        let snapshot = self.snapshot();
        self.write_line(&format_internal_prompt_progress_line(
            InternalPromptProgressEvent::Heartbeat,
            &snapshot,
            self.elapsed(),
            None,
        ));
    }

    fn snapshot(&self) -> InternalPromptProgressState {
        self.shared
            .state
            .lock()
            .expect("internal prompt progress state poisoned")
            .clone()
    }

    fn elapsed(&self) -> Duration {
        self.shared.started_at.elapsed()
    }

    fn write_line(&self, line: &str) {
        let _guard = self
            .shared
            .output_lock
            .lock()
            .expect("internal prompt progress output lock poisoned");
        let mut stdout = io::stdout();
        let _ = writeln!(stdout, "{line}");
        let _ = stdout.flush();
    }
}

impl InternalPromptProgressRun {
    fn start_ultraplan(task: &str) -> Self {
        let reporter = InternalPromptProgressReporter::ultraplan(task);
        reporter.emit(InternalPromptProgressEvent::Started, None);

        let (heartbeat_stop, heartbeat_rx) = mpsc::channel();
        let heartbeat_reporter = reporter.clone();
        let heartbeat_handle = thread::spawn(move || loop {
            match heartbeat_rx.recv_timeout(INTERNAL_PROGRESS_HEARTBEAT_INTERVAL) {
                Ok(()) | Err(RecvTimeoutError::Disconnected) => break,
                Err(RecvTimeoutError::Timeout) => heartbeat_reporter.emit_heartbeat(),
            }
        });

        Self {
            reporter,
            heartbeat_stop: Some(heartbeat_stop),
            heartbeat_handle: Some(heartbeat_handle),
        }
    }

    fn reporter(&self) -> InternalPromptProgressReporter {
        self.reporter.clone()
    }

    fn finish_success(&mut self) {
        self.stop_heartbeat();
        self.reporter
            .emit(InternalPromptProgressEvent::Complete, None);
    }

    fn finish_failure(&mut self, error: &str) {
        self.stop_heartbeat();
        self.reporter
            .emit(InternalPromptProgressEvent::Failed, Some(error));
    }

    fn stop_heartbeat(&mut self) {
        if let Some(sender) = self.heartbeat_stop.take() {
            let _ = sender.send(());
        }
        if let Some(handle) = self.heartbeat_handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for InternalPromptProgressRun {
    fn drop(&mut self) {
        self.stop_heartbeat();
    }
}

fn format_internal_prompt_progress_line(
    event: InternalPromptProgressEvent,
    snapshot: &InternalPromptProgressState,
    elapsed: Duration,
    error: Option<&str>,
) -> String {
    let elapsed_seconds = elapsed.as_secs();
    let step_label = if snapshot.step == 0 {
        "étape en attente".to_string()
    } else {
        format!("étape {}", snapshot.step)
    };
    let mut status_bits = vec![step_label, format!("phase {}", snapshot.phase)];
    if let Some(detail) = snapshot
        .detail
        .as_deref()
        .filter(|detail| !detail.is_empty())
    {
        status_bits.push(detail.to_string());
    }
    let status = status_bits.join(" · ");
    match event {
        InternalPromptProgressEvent::Started => {
            format!(
                "● {} · planification démarrée · {status}",
                snapshot.command_label
            )
        }
        InternalPromptProgressEvent::Update => {
            format!("… {} · {status}", snapshot.command_label)
        }
        InternalPromptProgressEvent::Heartbeat => format!(
            "… {} · toujours en cours · {elapsed_seconds} s · {status}",
            snapshot.command_label
        ),
        InternalPromptProgressEvent::Complete => format!(
            "✓ {} · terminé · {elapsed_seconds} s · {} étape(s)",
            snapshot.command_label, snapshot.step
        ),
        InternalPromptProgressEvent::Failed => format!(
            "✘ {} · échec · {elapsed_seconds} s · {}",
            snapshot.command_label,
            error.unwrap_or("erreur inconnue")
        ),
    }
}

fn describe_tool_progress(name: &str, input: &str) -> String {
    let parsed: serde_json::Value =
        serde_json::from_str(input).unwrap_or(serde_json::Value::String(input.to_string()));
    match name {
        "bash" | "Bash" => {
            let command = parsed
                .get("command")
                .and_then(|value| value.as_str())
                .unwrap_or_default();
            if command.is_empty() {
                "commande shell".to_string()
            } else {
                format!("commande {}", truncate_for_summary(command.trim(), 100))
            }
        }
        "read_file" | "Read" => format!("lecture de {}", extract_tool_path(&parsed)),
        "write_file" | "Write" => format!("écriture de {}", extract_tool_path(&parsed)),
        "edit_file" | "Edit" => format!("modification de {}", extract_tool_path(&parsed)),
        "glob_search" | "Glob" => {
            let pattern = parsed
                .get("pattern")
                .and_then(|value| value.as_str())
                .unwrap_or("?");
            let scope = parsed
                .get("path")
                .and_then(|value| value.as_str())
                .unwrap_or(".");
            format!("fichiers `{pattern}` dans {scope}")
        }
        "grep_search" | "Grep" => {
            let pattern = parsed
                .get("pattern")
                .and_then(|value| value.as_str())
                .unwrap_or("?");
            let scope = parsed
                .get("path")
                .and_then(|value| value.as_str())
                .unwrap_or(".");
            format!("recherche `{pattern}` dans {scope}")
        }
        "web_search" | "WebSearch" => parsed
            .get("query")
            .and_then(|value| value.as_str())
            .map_or_else(
                || "recherche web".to_string(),
                |query| format!("recherche web {}", truncate_for_summary(query, 100)),
            ),
        _ => {
            let summary = summarize_tool_payload(input);
            if summary.is_empty() {
                format!("outil {name}")
            } else {
                format!("{name}: {summary}")
            }
        }
    }
}

#[allow(clippy::needless_pass_by_value)]
#[allow(clippy::too_many_arguments)]
fn build_runtime(
    session: Session,
    model: String,
    system_prompt: Vec<String>,
    enable_tools: bool,
    emit_output: bool,
    allowed_tools: Option<AllowedToolSet>,
    permission_mode: PermissionMode,
    progress_reporter: Option<InternalPromptProgressReporter>,
) -> Result<ConversationRuntime<DefaultRuntimeClient, CliToolExecutor>, Box<dyn std::error::Error>>
{
    build_runtime_with_ui(
        session,
        model,
        system_prompt,
        enable_tools,
        emit_output,
        allowed_tools,
        permission_mode,
        progress_reporter,
        None,
    )
}

/// Comme [`build_runtime`], avec une interface optionnelle (web) qui reçoit les
/// événements à la place du terminal et répond aux demandes des sous-agents.
#[allow(clippy::needless_pass_by_value)]
#[allow(clippy::too_many_arguments)]
fn build_runtime_with_ui(
    session: Session,
    model: String,
    system_prompt: Vec<String>,
    enable_tools: bool,
    emit_output: bool,
    allowed_tools: Option<AllowedToolSet>,
    permission_mode: PermissionMode,
    progress_reporter: Option<InternalPromptProgressReporter>,
    ui: Option<RuntimeUi>,
) -> Result<ConversationRuntime<DefaultRuntimeClient, CliToolExecutor>, Box<dyn std::error::Error>>
{
    // Dans le terminal, la question se pose si quelqu'un est là pour répondre.
    let model_prompter = match &ui {
        Some(ui) => ui.model_prompter.clone(),
        None if emit_output => model_switch::TerminalModelPrompter::if_interactive(),
        None => None,
    };
    // Les questions de l'agent se posent là où quelqu'un peut répondre.
    tools::set_question_asker(match &ui {
        Some(ui) => ui.question_asker.clone(),
        None if emit_output => questions::terminal_asker(),
        None => None,
    });
    let emit_output = emit_output && ui.is_none();
    let observer = ui.as_ref().map(|ui| Arc::clone(&ui.observer));
    let (feature_config, tool_registry) = build_runtime_plugin_state()?;
    // Mode plan : le modèle de planification (`plannerModel`), s'il est réglé,
    // réfléchit à la place du modèle de la session.
    let planner = feature_config
        .planner_model()
        .filter(|_| {
            permission_mode == PermissionMode::ReadOnly
                && runtime::harness::enabled(runtime::harness::Aid::Planner)
        })
        .map(resolve_model_alias)
        .filter(|planner| *planner != model);
    let planner_notice = planner
        .as_ref()
        .map(|planner| format!("Mode plan : la réflexion est confiée à {planner} (plannerModel)"));
    let model = planner.unwrap_or(model);
    ACTIVE_MODEL
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone_from(&model);
    runtime::telemetry::set_model(&model);
    tools::advisors::set_resolver(|| models::advisor_models(&active_model()));
    // Sous-agents et modèles consultés comptent aussi dans les crédits du jour.
    tools::set_request_observer(credits::note_request);
    // Un modèle qui ne lit pas les images peut les regarder à nouveau.
    tools::set_image_looker(image_reader::look);
    let session_fallbacks = env::current_dir()
        .ok()
        .and_then(|cwd| ConfigLoader::default_for(&cwd).load().ok())
        .map(|config| config.fallback_models().to_vec())
        .unwrap_or_default();
    models::register_cached_limits();
    let rules = feature_config.permission_rules();
    runtime::set_configured_directories(
        rules
            .additional_directories()
            .iter()
            .map(PathBuf::from)
            .collect(),
    );
    let rules = PermissionRules::from_strings(rules.allow(), rules.deny(), rules.ask());
    let policy = permission_policy(permission_mode, &tool_registry).with_rules(rules.clone());
    // Les sous-agents héritent du modèle, du mode et des règles de la session,
    // et demandent leurs approbations par la même invite.
    tools::configure_subagents(
        tools::SubagentContext {
            model: model.clone(),
            permission_mode,
            rules,
            hooks: feature_config.hooks().clone(),
            fallback_models: models::model_chain(&model, &session_fallbacks),
        },
        Some(ui.as_ref().map_or_else(
            || Box::new(CliPermissionPrompter::new(permission_mode)) as tools::SubagentPrompter,
            |ui| (ui.subagent_prompter)(),
        )),
    );
    let mut system_prompt = system_prompt;
    if permission_mode == PermissionMode::ReadOnly {
        system_prompt.push(PLAN_MODE_PROMPT.to_string());
    }
    let auto_compaction = auto_compaction_config(&model);
    Ok(ConversationRuntime::new_with_features(
        session,
        DefaultRuntimeClient::new(
            &model,
            enable_tools,
            emit_output,
            allowed_tools.clone(),
            tool_registry.clone(),
            progress_reporter,
        )?
        .with_observer(observer.clone())
        .with_start_notice(planner_notice)
        .with_model_prompter(model_prompter),
        CliToolExecutor::new(allowed_tools.clone(), emit_output, tool_registry.clone())
            .with_observer(observer),
        policy,
        system_prompt,
        feature_config,
    )
    .with_auto_compaction(auto_compaction)
    .with_checkpoint_turns(true))
}

/// Compaction automatique selon la fenêtre de contexte du modèle ;
/// `CLAW_AUTO_COMPACT=0` la désactive.
fn auto_compaction_config(model: &str) -> Option<runtime::AutoCompactionConfig> {
    let disabled = env::var("CLAW_AUTO_COMPACT")
        .is_ok_and(|value| matches!(value.trim(), "0" | "false" | "off" | "non"));
    (!disabled).then(|| runtime::AutoCompactionConfig {
        threshold_tokens: api::auto_compaction_threshold(model),
        preserve_recent_messages: 6,
    })
}

/// Prompt d'une commande spécialisée, ou message d'usage si un argument requis manque.
#[allow(clippy::too_many_lines)]
fn expert_command_prompt(command: ExpertCommand, args: Option<&str>) -> Result<String, String> {
    // Les consignes sont en anglais pour le modèle ; la réponse doit rester
    // dans la langue de l'utilisateur.
    const LANGUAGE_NOTE: &str = "\n\nWrite your response in the user's language: the configured response language if any, otherwise the language of the user's previous messages, otherwise the language of the arguments above.";
    let args = args.map(str::trim).filter(|value| !value.is_empty());
    let role = |name: &str| tools::builtin_agent_instructions(name).unwrap_or_default();
    let prompt = match command {
        ExpertCommand::Review => {
            let target = args.unwrap_or(
                "the current uncommitted changes (`git diff` and `git diff --staged`), or the latest commit if the working tree is clean",
            );
            format!(
                "/review - Act as a code reviewer for {target}.\n\n{}\n\nDo not modify any file. End by asking which findings I want you to fix.",
                role("reviewer")
            )
        }
        ExpertCommand::Audit => {
            let scope = args.unwrap_or("this whole project");
            format!(
                "/audit - Audit {scope}.\n\n{}\n\nDo not modify any file.",
                role("auditor")
            )
        }
        ExpertCommand::Archi => {
            let Some(topic) = args else {
                return Err("Usage : /archi <besoin ou sujet>  (ex. /archi découper le monolithe en services)".to_string());
            };
            format!(
                "/archi - Design the architecture for: {topic}\n\n{}\n\nIf essential requirements are missing, ask me focused questions before designing. Do not modify files unless I ask you to.",
                role("architect")
            )
        }
        ExpertCommand::Adr => {
            let Some(decision) = args else {
                return Err(
                    "Usage : /adr <décision>  (ex. /adr utiliser PostgreSQL plutôt que MongoDB)"
                        .to_string(),
                );
            };
            format!(
                "/adr - Write an Architecture Decision Record about: {decision}\n\nBase it on our conversation and on the codebase. Store it in docs/adr/ (create the directory if needed) as NNNN-short-kebab-title.md, where NNNN is the next number after the existing records. Use the sections: Title, Status (Proposed), Date, Context, Decision, Alternatives considered (with their trade-offs), Consequences (positive and negative). Then create the file and show me its path."
            )
        }
        ExpertCommand::Avis => {
            let Some(question) = args else {
                return Err(
                    "Usage : /avis <question>  (ex. /avis faut-il un cache Redis ou en mémoire ?)"
                        .to_string(),
                );
            };
            format!(
                "/avis - Get independent second opinions on this question: {question}\n\nFirst gather what is needed to answer well (read the relevant code). Then call the Consult tool with the question and, in `context`, everything the other models need: code excerpts, constraints, what was already tried. Finally compare the answers: where they agree, where they differ and who is right given the code, and give your own recommendation with its main risk."
            )
        }
        ExpertCommand::Fix => {
            let command = args.map_or_else(
                || "Detect the test command of this project.".to_string(),
                |command| format!("Test command to use: `{command}`."),
            );
            format!("/fix - {command}\n\n{}", role("tester"))
        }
        ExpertCommand::SecurityReview => {
            let target = args.unwrap_or(
                "the current uncommitted changes (`git diff` and `git diff --staged`), or the latest commit if the working tree is clean",
            );
            format!("/security-review - Perform a security review of {target}.\n\n{SECURITY_REVIEW_INSTRUCTIONS}")
        }
        ExpertCommand::CodeReview => {
            let target = args.map_or_else(
                || "the current uncommitted changes (`git diff` and `git diff --staged`), or the latest commit if the working tree is clean".to_string(),
                |target| {
                    let number = target.trim_start_matches('#');
                    if !number.is_empty() && number.chars().all(|c| c.is_ascii_digit()) {
                        format!("pull request #{number} (`gh pr diff {number}`)")
                    } else {
                        target.to_string()
                    }
                },
            );
            format!(
                "/code-review - Multi-agent code review of {target}.\n\n{CODE_REVIEW_INSTRUCTIONS}"
            )
        }
        ExpertCommand::Simplify => {
            let target = args.unwrap_or(
                "the current uncommitted changes (`git diff HEAD`), or the latest commit if the working tree is clean",
            );
            format!(
                "/simplify - Review {target} for cleanup opportunities and apply the fixes.\n\nIn ONE reply, start four Agent sub-agents in parallel (subagent_type general-purpose), each reviewing the same changes with one focus and reporting concrete suggestions with file:line:\n1. Reuse: logic that duplicates an existing helper, utility or library function in this codebase.\n2. Simplification: needless complexity, dead code, redundant branches, over-long functions, unclear names.\n3. Efficiency: wasted work, repeated I/O or allocations, quadratic loops, missing early exits.\n4. Abstraction level: code at the wrong layer, leaky or premature abstractions, misplaced responsibilities.\nThis is not a bug hunt (that is /code-review): do not change behavior. Then merge their findings, apply the changes worth making, keep the behavior identical, run the project's build and tests, and summarize what you changed and what you deliberately left."
            )
        }
        ExpertCommand::Verify => {
            let what = args.unwrap_or(
                "the most recent change in this conversation (or the uncommitted changes)",
            );
            format!(
                "/verify - Confirm that {what} does what it should by building the project's application, running it and observing the result, rather than relying on tests or type checks alone.\n\nFind how the project is built and started (README, CLAW.md, manifests). Build it, then exercise the changed behavior the way a user would: run the CLI with representative inputs, start the server and request the affected endpoints, or open the page (ViewPage, Screenshot) and check what it shows. Use edge cases where they matter. Stop servers you started. Report what you ran, what you observed, and whether it matches the expected behavior, with the evidence. Do not fix anything unless I ask: if something is wrong, say what and where."
            )
        }
        ExpertCommand::Statusline => {
            let wanted = args.unwrap_or(
                "a compact line inspired by my shell prompt: current directory, git branch, model, context use",
            );
            let os = std::env::consts::OS;
            format!(
                "/statusline - Configure Claw's status line: {wanted}.\n\nHow it works: before each prompt, Claw runs the command set in the `statusLine` setting, writes a JSON object on its stdin and shows the first lines it prints (ANSI colors allowed). The JSON has: hook_event_name, session_id, transcript_path, cwd, model.id, model.display_name, workspace.current_dir, workspace.project_dir, version, permission_mode, output_style.name, context_window.used_percentage, context_window.context_window_size, cost.total_cost_usd, goal.\n\nSteps: 1. Write a small script for this system ({os}: PowerShell or Python on Windows, sh or Python elsewhere) in the Claw config folder (`~/.claw/statusline.<ext>`) that reads that JSON and prints the line; it must be fast and never fail (missing fields: print nothing for them). 2. Test it by piping a sample JSON into it. 3. Set `\"statusLine\": {{\"type\": \"command\", \"command\": \"<the command>\"}}` in `~/.claw/settings.json`, keeping every other setting. 4. Show me the line it prints."
            )
        }
        ExpertCommand::FewerPrompts => {
            let cwd = env::current_dir().map_err(|error| error.to_string())?;
            let frequencies = extra_commands::command_frequencies(&cwd);
            if frequencies.is_empty() {
                return Err("Aucune commande dans les conversations enregistrées de ce projet (.claw/sessions) : rien à proposer pour l'instant.".to_string());
            }
            let table = frequencies
                .iter()
                .map(|(name, count)| format!("- {name} ({count})"))
                .collect::<Vec<_>>()
                .join("\n");
            format!(
                "/fewer-permission-prompts - Reduce permission prompts in this project. These shell commands (first words) and MCP tools come up most in the project's saved conversations; read-only commands already run without a prompt and are not listed:\n{table}\n\nPropose `permissions.allow` rules (format `Bash(cargo test:*)` for a command prefix, or the MCP tool name such as `mcp__server__tool`) ONLY for the ones that are safe and routine: running tests, builds, linters, formatters, type checkers, listing or inspecting packages, read-only MCP tools. Never for commands that delete, push, deploy, publish, install system-wide, change git history or touch credentials. Show the list ranked by how many prompts it saves, with a one-line reason each, ask me to confirm, then add the approved rules to `.claw/settings.json` (create it if needed and keep the existing rules)."
            )
        }
        ExpertCommand::PrComments => {
            let number = args.map(|value| value.trim_start_matches('#').to_string());
            let target = number.as_deref().map_or_else(
                || "the pull request of the current branch".to_string(),
                |number| format!("pull request #{number}"),
            );
            let view = number.map_or_else(
                || "gh pr view --json number,title,url,reviews,comments".to_string(),
                |number| format!("gh pr view {number} --json number,title,url,reviews,comments"),
            );
            format!(
                "/pr-comments - Collect the review comments of {target} with the GitHub CLI: `{view}`, then the inline review comments with `gh api repos/OWNER/REPO/pulls/NUMBER/comments` (`gh repo view --json nameWithOwner` gives OWNER/REPO). If `gh` is missing or not logged in, say how to fix it (install it, then `gh auth login`) and stop.\n\nThen summarize: group the comments by file and by thread, give each reviewer's point briefly with file:line, say which ones look already addressed by the current code, and for each open point propose a concrete change. Do not modify any file; end by asking which points I want you to address."
            )
        }
    };
    Ok(format!("{prompt}{LANGUAGE_NOTE}"))
}

/// Consignes de `/security-review`, reprises de la revue de sécurité de Claude Code.
const SECURITY_REVIEW_INSTRUCTIONS: &str = "Look only for security vulnerabilities that these changes introduce or touch and that an attacker could realistically exploit:
- injection: SQL, shell commands, templates, LDAP, path traversal, unsafe file names;
- authentication and authorization: missing checks, privilege escalation, insecure direct object references, session handling;
- secrets and sensitive data: keys or tokens in code or logs, data exposed in responses or errors;
- unsafe deserialization, `eval`-like execution, server-side request forgery, XML external entities;
- cross-site scripting and unsafe HTML, open redirects;
- weak or misused cryptography (hard-coded keys, disabled certificate checks, predictable randomness for secrets);
- CI/CD: GitHub Actions expressions with untrusted event data inside `run:` scripts, overly broad token permissions.

Do not report: denial of service or resource exhaustion, missing rate limiting, generic hardening advice without a concrete exploit, issues only in tests or documentation, outdated dependencies without a reachable known vulnerability, memory-safety issues in memory-safe code, theoretical race conditions.

Method: understand the context first (frameworks, existing validation and sanitizers). For each candidate, trace the untrusted input from its source to the dangerous sink, then try to disprove the finding: is the input really attacker-controlled, is it validated upstream, is the code reachable? Keep only findings you rate at least 8/10 in confidence.

Report each finding with: title, severity (high, medium or low), confidence, file:line, a concrete exploit scenario (input and impact) and the fix. If nothing passes the bar, say that no high-confidence vulnerability was found and list what you checked. Do not modify any file; end by asking which findings I want you to fix.";

/// Consignes de `/code-review` : relecteurs en parallèle puis vérification,
/// l'équivalent local et gratuit de la revue multi-agents de Claude Code.
const CODE_REVIEW_INSTRUCTIONS: &str = "1. Scope: get the diff (the target above; `git diff` and `git diff --staged` by default; `gh pr diff N` for a pull request) and list the changed files. If there is nothing to review, say so and stop.
2. Parallel review: in a single message, call the Agent tool four times with subagent_type \"reviewer\" so the four reviews run at the same time. Sub-agents do not see this conversation: give each one the command that shows the diff and the list of changed files, and one focus: (a) correctness bugs and regressions; (b) security (injection, secrets, authorization, unsafe input); (c) error handling, resource leaks, concurrency and performance; (d) tests and maintainability (changed behavior without tests, duplication, dead code). Ask each for at most 8 verified findings, each with severity, file:line, problem, failure scenario and fix, and no style nitpicks.
3. Verification: merge duplicate findings. Check every finding of medium severity or above against the code yourself (for many findings, verify them in parallel with Agent calls of subagent_type \"Verification\"), and classify it CONFIRMED, PLAUSIBLE or REJECTED with the reason. Drop the rejected ones.
4. Report: confirmed then plausible findings, most severe first, each with file:line, failure scenario and suggested fix; then one line with the verdict (ready to merge, or needs changes) and how many findings were rejected during verification. Do not modify any file; end by asking which findings I want you to fix.";

const PLAN_MODE_PROMPT: &str = "# Plan mode
- The session is in read-only plan mode: file edits and commands with side effects are denied.
- Explore the codebase with read and search tools (and read-only commands such as `git status`, `git log`, `ls`).
- Then write a concrete plan: the files to change, the approach, the risks, and how the result will be verified (tests, build).
- Submit it with the ExitPlanMode tool (argument `plan`). If the user approves, the session switches to edit mode and you implement the plan right away in the same turn; otherwise revise it.
- Do not attempt workarounds to modify files before approval.";

struct CliPermissionPrompter {
    current_mode: PermissionMode,
    /// L'utilisateur a refusé (Échap, « Non ») : le tour s'arrête pour sa consigne.
    refused: bool,
}

impl CliPermissionPrompter {
    fn new(current_mode: PermissionMode) -> Self {
        Self {
            current_mode,
            refused: false,
        }
    }

    /// « Oui, toujours » : règle enregistrée pour le projet (ou la session).
    fn allow_always(rule: &str) -> runtime::PermissionPromptDecision {
        let note = if rule_is_session_only(rule) {
            format!("Règle « {rule} » active pour cette session")
        } else {
            match env::current_dir()
                .map_err(|error| error.to_string())
                .and_then(|cwd| persist_allow_rule(&cwd, rule))
            {
                Ok(path) => format!(
                    "Règle « {rule} » enregistrée dans {}",
                    transcript::display_path(&path.display().to_string())
                ),
                Err(error) => format!(
                    "Règle « {rule} » active pour cette session (non enregistrée : {error})"
                ),
            }
        };
        println!("  ⎿  {}{note}{}", transcript::DIM, transcript::RESET);
        runtime::PermissionPromptDecision::AllowAlways {
            rule: rule.to_string(),
        }
    }

    /// Encadré avec choix aux flèches (terminal interactif).
    fn decide_with_menu(
        &mut self,
        request: &runtime::PermissionRequest,
    ) -> runtime::PermissionPromptDecision {
        let plan = request.tool_name == "ExitPlanMode";
        if plan {
            print_proposed_plan(&request.input);
        }
        let (title, body, question) = permission_box_content(&request.tool_name, &request.input);
        let rule = request.suggested_rule.clone().filter(|_| !plan);
        let mut options = vec!["Oui".to_string()];
        if let Some(rule) = &rule {
            options.push(format!(
                "Oui, et ne plus demander pour {rule} {}",
                if rule_is_session_only(rule) {
                    "pendant cette session"
                } else {
                    "dans ce projet"
                }
            ));
        }
        options.push(if plan {
            "Non, continuer à planifier (Échap)".to_string()
        } else {
            "Non, et dire à Claw quoi faire autrement (Échap)".to_string()
        });
        let answer = match approval::ask(&title, &body, &question, &options) {
            Ok(answer) => answer,
            Err(error) => {
                return runtime::PermissionPromptDecision::Deny {
                    reason: format!("échec de la demande d'approbation : {error}"),
                }
            }
        };
        match (answer, &rule) {
            (approval::Answer::Pick(0), _) => runtime::PermissionPromptDecision::Allow,
            (approval::Answer::Pick(1), Some(rule)) => Self::allow_always(rule),
            _ => {
                // Comme dans Claude Code : le tour s'arrête et l'utilisateur dit
                // quoi faire à la place.
                self.refused = true;
                runtime::interrupt::request_interrupt();
                runtime::PermissionPromptDecision::Deny {
                    reason: format!(
                        "l'utilisateur a refusé l'outil '{}' ; attendez sa consigne avant de continuer",
                        request.tool_name
                    ),
                }
            }
        }
    }

    /// Question en mode texte (entrée non interactive) : o, t, n ou une consigne.
    fn decide_with_text(
        &mut self,
        request: &runtime::PermissionRequest,
    ) -> runtime::PermissionPromptDecision {
        let always = request.suggested_rule.as_deref().map(|rule| {
            let scope = if rule_is_session_only(rule) {
                "pour cette session"
            } else {
                "enregistré dans .claw/settings.local.json"
            };
            format!("  [t] toujours  `{rule}` ({scope})")
        });
        println!();
        println!("Permission requise · {}", request.tool_name);
        println!(
            "  {}",
            describe_permission_target(&request.tool_name, &request.input)
        );
        println!("  Mode actuel      {}", self.current_mode.as_str());
        println!("  [o] oui, une fois");
        if let Some(always) = &always {
            println!("{always}");
        }
        println!("  [n] non          (ou tapez une consigne pour l'agent)");
        // L'écoute du clavier (Échap) rend la main le temps de la réponse.
        let _keys = turn_keys::pause();
        print!("Votre choix : ");
        let _ = io::stdout().flush();

        let mut response = String::new();
        if let Err(error) = io::stdin().read_line(&mut response) {
            return runtime::PermissionPromptDecision::Deny {
                reason: format!("échec de la demande d'approbation : {error}"),
            };
        }
        match parse_permission_answer(&response) {
            PermissionAnswer::Once => runtime::PermissionPromptDecision::Allow,
            PermissionAnswer::Always => match &request.suggested_rule {
                Some(rule) => Self::allow_always(rule),
                None => runtime::PermissionPromptDecision::Allow,
            },
            PermissionAnswer::Deny => runtime::PermissionPromptDecision::Deny {
                reason: format!(
                    "l'utilisateur a refusé l'outil '{}' ; demandez-lui comment poursuivre",
                    request.tool_name
                ),
            },
            PermissionAnswer::Feedback(feedback) => runtime::PermissionPromptDecision::Deny {
                reason: format!(
                    "l'utilisateur a refusé l'outil '{}' avec cette consigne : {feedback}",
                    request.tool_name
                ),
            },
        }
    }
}

impl runtime::PermissionPrompter for CliPermissionPrompter {
    fn decide(
        &mut self,
        request: &runtime::PermissionRequest,
    ) -> runtime::PermissionPromptDecision {
        let gate = auto_mode::gate(request);
        if let Some(notice) = &gate.notice {
            live::hide();
            transcript::print_call(&request.tool_name, &request.input);
            println!("  ⎿  {}{notice}{}", transcript::RED, transcript::RESET);
        }
        if let Some(decision) = gate.decision {
            return decision;
        }
        live::hide();
        transcript::print_call(&request.tool_name, &request.input);
        let decision = if approval::interactive() {
            self.decide_with_menu(request)
        } else {
            self.decide_with_text(request)
        };
        if request.current_mode == PermissionMode::Auto
            && !matches!(decision, runtime::PermissionPromptDecision::Deny { .. })
        {
            auto_mode::resume();
        }
        decision
    }
}

/// Plan proposé (`ExitPlanMode`), affiché en entier avant la question.
fn print_proposed_plan(input: &str) {
    let plan = serde_json::from_str::<serde_json::Value>(input)
        .ok()
        .and_then(|value| {
            value
                .get("plan")
                .and_then(serde_json::Value::as_str)
                .map(ToOwned::to_owned)
        })
        .filter(|plan| !plan.trim().is_empty());
    if let Some(plan) = plan {
        let rendered = TerminalRenderer::new().markdown_to_ansi(&plan);
        let mut out = io::stdout();
        let mut answer = transcript::Prefixer::assistant();
        let _ = answer.write(&mut out, &rendered);
        let _ = answer.finish_line(&mut out);
        println!();
    }
}

/// Titre, corps (lignes prêtes à encadrer) et question d'une autorisation.
#[allow(clippy::too_many_lines)]
fn permission_box_content(tool_name: &str, input: &str) -> (String, Vec<String>, String) {
    let value = serde_json::from_str::<serde_json::Value>(input).unwrap_or_default();
    let field = |key: &str| value.get(key).and_then(serde_json::Value::as_str);
    let width = approval::box_width().saturating_sub(6);
    let wrap = |text: &str| transcript::wrap(text, width);
    let dim = |line: String| format!("{}{line}{}", transcript::DIM, transcript::RESET);
    let bold = |line: String| format!("{}{line}{}", transcript::BOLD, transcript::RESET);
    let proceed = "Voulez-vous continuer ?".to_string();
    match tool_name {
        "bash" | "Bash" | "PowerShell" => {
            let mut body: Vec<String> = wrap(field("command").unwrap_or_default())
                .into_iter()
                .map(bold)
                .collect();
            if let Some(description) = field("description").filter(|text| !text.trim().is_empty()) {
                body.extend(wrap(description).into_iter().map(dim));
            }
            let title = if tool_name == "PowerShell" {
                "Commande PowerShell"
            } else {
                "Commande shell"
            };
            (title.to_string(), body, proceed)
        }
        "edit_file" => {
            let raw = field("path").unwrap_or("?");
            let old_string = field("old_string").unwrap_or_default();
            let new_string = field("new_string").unwrap_or_default();
            // Lignes complètes du fichier actuel, numérotées.
            let expanded = fs::read_to_string(raw)
                .ok()
                .and_then(|file| transcript::expand_to_lines(&file, old_string, new_string));
            let (first, old_text, new_text) = match &expanded {
                Some((first, old_block, new_block)) => {
                    (Some(*first), old_block.as_str(), new_block.as_str())
                }
                None => (None, old_string, new_string),
            };
            let mut body = vec![bold(transcript::display_path(raw))];
            body.extend(transcript::render_diff(
                old_text, new_text, first, None, 14, width,
            ));
            (
                "Modifier un fichier".to_string(),
                body,
                "Appliquer cette modification ?".to_string(),
            )
        }
        "write_file" => {
            let raw = field("path").unwrap_or("?");
            let content = field("content").unwrap_or_default();
            let lines: Vec<&str> = content.lines().collect();
            let exists = Path::new(raw).exists();
            let mut body = vec![format!(
                "{} {}",
                bold(transcript::display_path(raw)),
                dim(format!("({} lignes)", lines.len()))
            )];
            body.extend(transcript::numbered(&lines, 1, 10, width));
            if exists {
                (
                    "Réécrire un fichier".to_string(),
                    body,
                    "Remplacer ce fichier ?".to_string(),
                )
            } else {
                (
                    "Créer un fichier".to_string(),
                    body,
                    "Créer ce fichier ?".to_string(),
                )
            }
        }
        "WebFetch" => {
            let mut body: Vec<String> = wrap(field("url").unwrap_or("?"))
                .into_iter()
                .map(bold)
                .collect();
            if let Some(prompt) = field("prompt") {
                body.extend(wrap(prompt).into_iter().take(4).map(dim));
            }
            ("Lire une page web".to_string(), body, proceed)
        }
        "Agent" => {
            let mut body: Vec<String> = wrap(field("description").unwrap_or("sous-agent"))
                .into_iter()
                .map(bold)
                .collect();
            if let Some(prompt) = field("prompt") {
                body.extend(wrap(prompt).into_iter().take(6).map(dim));
            }
            ("Lancer un sous-agent".to_string(), body, proceed)
        }
        "ExitPlanMode" => (
            "Plan prêt".to_string(),
            vec![dim(
                "Approuver passe en mode « éditions acceptées » pour l'appliquer.".to_string(),
            )],
            "Approuver ce plan ?".to_string(),
        ),
        _ => {
            let summary = describe_permission_target(tool_name, input);
            let body = wrap(&summary).into_iter().take(10).collect();
            (format!("Outil {tool_name}"), body, proceed)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PermissionAnswer {
    Once,
    Always,
    Deny,
    Feedback(String),
}

fn parse_permission_answer(raw: &str) -> PermissionAnswer {
    let trimmed = raw.trim();
    match trimmed.to_lowercase().as_str() {
        "o" | "oui" | "y" | "yes" => PermissionAnswer::Once,
        "t" | "toujours" | "a" | "always" => PermissionAnswer::Always,
        "" | "n" | "non" | "no" => PermissionAnswer::Deny,
        _ => PermissionAnswer::Feedback(trimmed.to_string()),
    }
}

/// Autoriser toutes les éditions est réservé à la session en cours ; les autres
/// règles (commandes, domaines…) sont enregistrées pour le projet.
fn rule_is_session_only(rule: &str) -> bool {
    rule == "Edit"
}

fn describe_permission_target(tool_name: &str, input: &str) -> String {
    let value = serde_json::from_str::<serde_json::Value>(input).unwrap_or_default();
    let field = |key: &str| value.get(key).and_then(serde_json::Value::as_str);
    let described = match tool_name {
        "bash" | "PowerShell" => {
            field("command").map(|command| format!("Commande         {command}"))
        }
        "read_file" | "write_file" | "edit_file" => {
            field("path").map(|path| format!("Fichier          {path}"))
        }
        "NotebookEdit" => field("notebook_path").map(|path| format!("Notebook         {path}")),
        "WebFetch" | "ViewPage" => field("url").map(|url| format!("URL              {url}")),
        "Screenshot" => field("url")
            .or_else(|| field("path"))
            .map(|page| format!("Page             {page}")),
        "Agent" => {
            field("description").map(|description| format!("Sous-agent       {description}"))
        }
        "ExitPlanMode" => field("plan").map(|plan| {
            format!("Plan proposé (approuver = passer en mode workspace-write)\n\n{plan}\n")
        }),
        "KillShell" => field("shell_id").map(|id| format!("Tâche            {id}")),
        _ => None,
    };
    described.unwrap_or_else(|| {
        let compact = input.split_whitespace().collect::<Vec<_>>().join(" ");
        let preview = compact.chars().take(200).collect::<String>();
        let ellipsis = if compact.chars().count() > 200 {
            "…"
        } else {
            ""
        };
        format!("Entrée           {preview}{ellipsis}")
    })
}

/// `allow <règle>`, `deny <règle>`, `ask <règle>` ou `remove <règle>` :
/// la liste visée (aucune pour un retrait) et la règle.
fn parse_rule_command(args: &str) -> Option<(Option<&'static str>, &str)> {
    let (verb, rule) = args.trim().split_once(char::is_whitespace)?;
    let rule = rule.trim();
    if rule.is_empty() {
        return None;
    }
    let list = match verb {
        "allow" | "autoriser" => Some("allow"),
        "deny" | "interdire" => Some("deny"),
        "ask" | "demander" => Some("ask"),
        "remove" | "rm" | "retirer" => None,
        _ => return None,
    };
    Some((list, rule))
}

/// Ajoute `rule` à `permissions.allow` de `.claw/settings.local.json` (créé si besoin).
fn persist_allow_rule(cwd: &Path, rule: &str) -> Result<PathBuf, String> {
    edit_permission_rule(cwd, Some("allow"), rule).map(|(path, _)| path)
}

/// Ajoute `rule` à la liste `list` (`allow`, `deny`, `ask`) de
/// `.claw/settings.local.json`, ou la retire de toutes (`list` absent).
/// Renvoie le fichier et vrai s'il a changé.
fn edit_permission_rule(
    cwd: &Path,
    list: Option<&str>,
    rule: &str,
) -> Result<(PathBuf, bool), String> {
    let path = cwd.join(".claw").join("settings.local.json");
    let mut root = match fs::read_to_string(&path) {
        Ok(contents) if !contents.trim().is_empty() => {
            serde_json::from_str::<serde_json::Value>(&contents)
                .map_err(|error| format!("{} illisible : {error}", path.display()))?
        }
        Ok(_) => json!({}),
        Err(error) if error.kind() == io::ErrorKind::NotFound => json!({}),
        Err(error) => return Err(error.to_string()),
    };
    if !root.is_object() {
        return Err(format!("{} n'est pas un objet JSON", path.display()));
    }
    let permissions = &mut root["permissions"];
    if !permissions.is_object() {
        *permissions = json!({});
    }
    let mut changed = false;
    if let Some(list) = list {
        let rules = &mut permissions[list];
        if !rules.is_array() {
            *rules = json!([]);
        }
        if let Some(rules) = rules.as_array_mut() {
            if !rules.iter().any(|existing| existing.as_str() == Some(rule)) {
                rules.push(json!(rule));
                changed = true;
            }
        }
    } else {
        // `get_mut` et non `permissions[list]` : l'indexation créerait une
        // liste `null`, que la lecture de la config refuse ensuite.
        for list in ["allow", "deny", "ask", "additionalDirectories"] {
            if let Some(rules) = permissions
                .get_mut(list)
                .and_then(serde_json::Value::as_array_mut)
            {
                let before = rules.len();
                rules.retain(|existing| existing.as_str() != Some(rule));
                changed |= rules.len() != before;
            }
        }
        if !changed {
            return Ok((path, false));
        }
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let rendered = serde_json::to_string_pretty(&root).map_err(|error| error.to_string())?;
    fs::write(&path, format!("{rendered}\n")).map_err(|error| error.to_string())?;
    Ok((path, changed))
}

/// Reçoit les événements d'un tour pour une interface autre que le terminal
/// (l'interface web de `claw serve`).
trait TurnObserver: Send + Sync {
    fn text_delta(&self, text: &str);
    /// Raisonnement du modèle (affiché à part de la réponse).
    fn thinking_delta(&self, _text: &str) {}
    fn tool_call(&self, name: &str, input: &str);
    fn tool_result(&self, name: &str, output: &str, is_error: bool);
    fn notice(&self, text: &str);
    /// Le modèle vient de répondre sans appeler d'outil.
    fn answer_drafted(&self) {}
    /// Un contrôle de Claw le renvoie au travail : cette réponse est provisoire.
    fn answer_superseded(&self) {}
    /// La réponse rendue provisoire reste la réponse : l'interface la rouvre.
    fn answer_kept(&self) {}
    /// La réponse en cours est abandonnée (flux coupé) et va être redemandée :
    /// les appels d'outils qu'elle annonçait ne seront pas exécutés.
    fn reply_discarded(&self) {}
    /// La réponse qui vient continue la précédente, coupée par la limite de
    /// sortie : l'interface l'y raccorde.
    fn reply_continues(&self) {}
}

type SharedObserver = Arc<dyn TurnObserver>;

/// Interface d'un runtime piloté hors du terminal : observateur des événements
/// et invite d'approbation pour les sous-agents.
#[derive(Clone)]
struct RuntimeUi {
    observer: SharedObserver,
    subagent_prompter: Arc<dyn Fn() -> tools::SubagentPrompter + Send + Sync>,
    /// Pose la question « réessayer, changer de modèle ou arrêter » ; `None`
    /// quand personne n'est là pour répondre (script, protocole d'éditeur).
    model_prompter: Option<model_switch::SharedModelPrompter>,
    /// Pose les questions de l'agent (outil `AskUserQuestion`) ; `None` :
    /// l'outil n'est pas proposé au modèle.
    question_asker: Option<tools::SharedQuestionAsker>,
}

struct DefaultRuntimeClient {
    runtime: tokio::runtime::Runtime,
    /// Modèle principal puis remplaçants (bascule sur quota ou panne).
    chain: api::ModelChain,
    /// Chaîne des résumés : `smallModel` d'abord s'il est configuré.
    summary_chain: Option<api::ModelChain>,
    enable_tools: bool,
    emit_output: bool,
    allowed_tools: Option<AllowedToolSet>,
    tool_registry: GlobalToolRegistry,
    progress_reporter: Option<InternalPromptProgressReporter>,
    observer: Option<SharedObserver>,
    /// Avis à donner avant la première requête (modèle de planification).
    start_notice: Option<String>,
    /// Modèle plus fort qui reprend un tour bloqué : par défaut le gratuit le
    /// plus puissant du moment, sinon `escalationModel`.
    escalation: models::Escalation,
    /// Chaîne habituelle, mise de côté pendant une escalade.
    saved_chain: Option<api::ModelChain>,
    /// Présent : Claw ne change jamais de modèle sans le demander.
    model_prompter: Option<model_switch::SharedModelPrompter>,
    /// Modèles de secours configurés, proposés quand la question se pose.
    candidates: Vec<String>,
    /// Mode équipe : rôle annoncé pour la prochaine requête.
    team_step: Option<runtime::team::Step>,
    /// Chaîne de la session, mise de côté pendant qu'un autre membre de
    /// l'équipe travaille ; elle revient à la fin du tour.
    team_base: Option<api::ModelChain>,
    /// Modèle de la requête précédente du tour : le raisonnement n'est
    /// renvoyé qu'au modèle qui l'a écrit.
    team_last: Option<String>,
    /// Modèle qui a écrit le code pendant ce tour : le relecteur en prend un autre.
    team_coder: Option<String>,
    /// La dernière réponse s'est arrêtée sur la limite de sortie au milieu de
    /// son texte : le runtime en demandera la suite.
    reply_cut: bool,
}

impl DefaultRuntimeClient {
    fn new(
        model: &str,
        enable_tools: bool,
        emit_output: bool,
        allowed_tools: Option<AllowedToolSet>,
        tool_registry: GlobalToolRegistry,
        progress_reporter: Option<InternalPromptProgressReporter>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let config = env::current_dir()
            .ok()
            .and_then(|cwd| ConfigLoader::default_for(&cwd).load().ok());
        let fallback_models = config
            .as_ref()
            .map(|config| config.fallback_models().to_vec())
            .unwrap_or_default();
        let chain_models = models::model_chain(model, &fallback_models);
        // Sans remplaçant, une clé absente doit être signalée dès le démarrage.
        if chain_models.len() == 1 {
            build_provider_client(model)?;
        }
        let small_model = env::var("CLAW_SMALL_MODEL")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .or_else(|| {
                config
                    .as_ref()
                    .and_then(|config| config.small_model().map(ToOwned::to_owned))
            });
        let summary_chain = small_model.map(|small| {
            let mut models = vec![resolve_model_alias(&small)];
            models.extend(chain_models.iter().cloned());
            api::ModelChain::new(models, cli_client_factory())
        });
        Ok(Self {
            runtime: tokio::runtime::Runtime::new()?,
            chain: api::ModelChain::new(chain_models, cli_client_factory()),
            summary_chain,
            enable_tools,
            emit_output,
            allowed_tools,
            tool_registry,
            progress_reporter,
            observer: None,
            start_notice: None,
            escalation: models::Escalation::configured(),
            saved_chain: None,
            model_prompter: None,
            candidates: Vec::new(),
            team_step: None,
            team_base: None,
            team_last: None,
            team_coder: None,
            reply_cut: false,
        })
    }

    fn with_observer(mut self, observer: Option<SharedObserver>) -> Self {
        self.observer = observer;
        self
    }

    fn with_start_notice(mut self, notice: Option<String>) -> Self {
        self.start_notice = notice;
        self
    }

    /// Avec quelqu'un pour répondre (et sauf `"modelSwitch": "auto"`), la
    /// session ne garde que son modèle : les secours deviennent des
    /// propositions, faites à l'utilisateur quand le modèle ne répond plus.
    fn with_model_prompter(mut self, prompter: Option<model_switch::SharedModelPrompter>) -> Self {
        let Some(prompter) = prompter.filter(|_| model_switch::ask_before_switching()) else {
            return self;
        };
        let models = self.chain.models().to_vec();
        if let Some((primary, others)) = models.split_first() {
            self.candidates = others.to_vec();
            self.chain = api::ModelChain::new(vec![primary.clone()], cli_client_factory());
            self.model_prompter = Some(prompter);
        }
        self
    }

    /// Avis du premier de ces modèles qui répond (relecture, diagnostic).
    fn consult_models(
        &mut self,
        purpose: runtime::ConsultPurpose,
        system: &str,
        prompt: &str,
        models: &[String],
    ) -> Option<runtime::Consultation> {
        if models.is_empty() {
            return None;
        }
        if self.emit_output {
            live::show(match purpose {
                runtime::ConsultPurpose::Review => "Relecture par un autre modèle…",
                runtime::ConsultPurpose::Design => "Relecture du plan par un autre modèle…",
                runtime::ConsultPurpose::Unblock => "Diagnostic par un autre modèle…",
            });
        }
        // La relecture clôt le tour : elle a le temps et la place de finir.
        let opinion = match purpose {
            runtime::ConsultPurpose::Review | runtime::ConsultPurpose::Design => {
                tools::advisors::ask_within(
                    models,
                    system,
                    prompt,
                    tools::advisors::REVIEW_MAX_TOKENS,
                    tools::advisors::Patience::REVIEW,
                )
            }
            runtime::ConsultPurpose::Unblock => tools::advisors::ask(models, system, prompt, 3_000),
        };
        match opinion {
            Ok(opinion) => Some(runtime::Consultation {
                model: opinion.model,
                text: opinion.text,
            }),
            Err(error) => {
                // Aucun autre modèle n'a répondu (relecteurs gratuits saturés) :
                // le modèle de la session relit, dans une conversation neuve,
                // plutôt que de laisser le travail sans relecture. Essai réel t4 :
                // sans relecture, le livre mis de côté redevenait un prêt d'office.
                if purpose != runtime::ConsultPurpose::Unblock {
                    let own = self.chain.primary().to_string();
                    if !own.is_empty() && !models.contains(&own) {
                        if let Ok(opinion) = tools::advisors::ask_within(
                            std::slice::from_ref(&own),
                            system,
                            prompt,
                            tools::advisors::REVIEW_MAX_TOKENS,
                            tools::advisors::Patience::REVIEW,
                        ) {
                            self.notify(&format!(
                                "Aucun autre modèle n'a répondu ({error}) : le modèle de la session relit, dans une conversation neuve"
                            ));
                            return Some(runtime::Consultation {
                                model: opinion.model,
                                text: opinion.text,
                            });
                        }
                    }
                }
                self.notify(&format!("Aucun autre modèle n'a répondu ({error})"));
                None
            }
        }
    }

    /// Modèles proposés à la place de `current` : le gratuit le plus puissant
    /// d'abord, puis les secours configurés.
    fn model_suggestions(&self, current: &str) -> Vec<String> {
        let strongest: Vec<String> = models::Escalation::StrongestFree
            .target(current)
            .into_iter()
            .collect();
        model_switch::suggestions(current, &strongest, &self.candidates)
    }

    /// Le modèle choisi par l'utilisateur prend la suite et devient celui de la session.
    fn switch_to(&mut self, next: &str) {
        let previous = self.chain.primary().to_string();
        self.candidates.retain(|model| model != next);
        if !previous.is_empty() && !self.candidates.contains(&previous) {
            self.candidates.insert(0, previous);
        }
        self.chain = api::ModelChain::new(vec![next.to_string()], cli_client_factory());
        // Le choix de l'utilisateur devient le modèle de la session.
        self.team_base = None;
        ACTIVE_MODEL
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone_from(&next.to_string());
    }

    /// Mode équipe : confie l'étape au modèle le plus fort pour son rôle, et
    /// renvoie la consigne de relais à ajouter au prompt système. Pendant une
    /// escalade, le modèle fort garde la main.
    fn apply_team_step(
        &mut self,
        step: runtime::team::Step,
        messages: &[ConversationMessage],
    ) -> Option<String> {
        if self.saved_chain.is_some() {
            return None;
        }
        let base_chain = self.team_base.as_ref().unwrap_or(&self.chain);
        let base = base_chain.primary().to_string();
        let base_models = base_chain.models().to_vec();
        let needs = models::TeamNeeds {
            images: images_in_current_request(messages),
            context_tokens: conversation_tokens(messages),
            avoid: None,
        };
        let (target, why) = models::team_pick(step.role, &needs).map_or_else(
            || (base.clone(), "modèle de la session".to_string()),
            |pick| (pick.model, pick.why),
        );
        if target != self.chain.primary() {
            if target == base {
                if let Some(chain) = self.team_base.take() {
                    self.chain = chain;
                }
            } else {
                let mut chain_models = vec![target.clone()];
                // Seul, sans personne pour décider : la chaîne de la session
                // sert de secours. Sinon Claw demande, comme toujours.
                if self.model_prompter.is_none() {
                    chain_models.extend(base_models.into_iter().filter(|model| *model != target));
                }
                let chain = api::ModelChain::new(chain_models, cli_client_factory());
                let previous = std::mem::replace(&mut self.chain, chain);
                if self.team_base.is_none() {
                    self.team_base = Some(previous);
                }
            }
            self.notify(&format!(
                "Équipe : « {} » confié à {target} ({why})",
                step.role.label()
            ));
        }
        if step.role == runtime::team::Role::Code {
            self.team_coder = Some(target.clone());
        }
        Some(runtime::team::role_section(step.role, &target))
    }

    /// Un appel d'outil sans arguments complets dans une réponse qui ne s'est
    /// pas terminée sur un appel d'outil : elle a été coupée. Par la limite de
    /// sortie, l'appel est marqué et le modèle apprendra quoi faire ; sinon
    /// c'est une erreur, et la réponse est redemandée.
    fn check_cut_calls(
        tool_registry: &GlobalToolRegistry,
        events: &mut [AssistantEvent],
        stop_reason: Option<&str>,
    ) -> Result<(), RuntimeError> {
        let definitions = std::cell::OnceCell::new();
        let required_for = |name: &str| {
            definitions
                .get_or_init(|| tool_registry.definitions(None))
                .iter()
                .find(|definition| definition.name == name)
                .map(|definition| tool_input::required_arguments(&definition.input_schema))
                .unwrap_or_default()
        };
        tool_input::check_cut_calls(events, stop_reason, required_for, true)
            .map_err(RuntimeError::new)?;
        // Noms d'arguments réduits par un serveur qui refuse les schémas tels
        // quels : l'outil reçoit les siens.
        tool_input::restore_names_in(events, |name| {
            definitions
                .get_or_init(|| tool_registry.definitions(None))
                .iter()
                .find(|definition| definition.name == name)
                .map(|definition| definition.input_schema.clone())
        });
        Ok(())
    }
}

// Anthropic garde son flux d'authentification (clé API ou OAuth sauvegardé) ;
// les autres fournisseurs (OpenRouter, OpenAI, xAI) lisent leur clé dans l'environnement.
fn build_provider_client(model: &str) -> Result<ProviderClient, Box<dyn std::error::Error>> {
    Ok(match detect_provider_kind(model) {
        ProviderKind::ClawApi => ProviderClient::ClawApi(
            ClawApiClient::from_auth(resolve_cli_auth_source()?)
                .with_base_url(api::read_base_url()),
        ),
        _ => ProviderClient::from_model(model)?,
    })
}

fn cli_client_factory() -> api::ClientFactory {
    Box::new(|model| {
        build_provider_client(model).map_err(|error| api::ApiError::Auth(error.to_string()))
    })
}

fn resolve_cli_auth_source() -> Result<AuthSource, Box<dyn std::error::Error>> {
    Ok(resolve_startup_auth_source(|| {
        let cwd = env::current_dir().map_err(api::ApiError::from)?;
        let config = ConfigLoader::default_for(&cwd).load().map_err(|error| {
            api::ApiError::Auth(format!("configuration OAuth illisible : {error}"))
        })?;
        Ok(config.oauth().cloned())
    })?)
}

impl ApiClient for DefaultRuntimeClient {
    fn summarize(&mut self, system_prompt: &str, transcript: &str) -> Option<String> {
        if self.emit_output {
            // Seconde demande : le texte rendu n'était pas un résumé.
            let line = if transcript.ends_with(runtime::SUMMARY_RETRY_NOTE) {
                "\n⟳ Le texte rendu n'était pas un résumé : nouvelle demande au modèle…"
            } else {
                "\n⟳ Contexte presque plein : résumé de la conversation en cours…"
            };
            live::suspend(|| {
                println!("{line}");
            });
        }
        let emit = self.emit_output;
        let observer = self.observer.clone();
        let mut notify = move |text: &str| notify_fallback(emit, observer.as_ref(), text);
        let chain = match self.summary_chain.as_mut() {
            Some(chain) => chain,
            None => &mut self.chain,
        };
        let build = |model: &str| api::summary_request(model, system_prompt, transcript);
        self.runtime
            .block_on(runtime::interrupt::interruptible(
                chain.send(&build, &mut notify),
            ))
            .ok()
            .map(|(model, response)| {
                credits::note_request(&model);
                runtime::typography::cleaned(&api::response_text(&response)).into_owned()
            })
            .filter(|summary| !summary.trim().is_empty())
    }

    fn notify(&mut self, notice: &str) {
        if self.emit_output {
            live::suspend(|| println!("⟳ {notice}"));
        }
        if let Some(observer) = &self.observer {
            observer.notice(notice);
        }
        collect_notice(notice);
    }

    fn answer_drafted(&mut self) {
        if let Some(observer) = &self.observer {
            observer.answer_drafted();
        }
    }

    fn reply_cut_by_output_limit(&mut self) -> bool {
        std::mem::take(&mut self.reply_cut)
    }

    fn can_ask_user(&self) -> bool {
        self.enable_tools && tools::question_asker_available()
    }

    /// Mode projet : le quota gratuit du jour, pour un modèle gratuit d'`OpenRouter`.
    fn free_requests_left(&mut self) -> Option<u64> {
        let model = self.chain.primary().to_string();
        if !(credits::is_openrouter_model(&model) && model.ends_with(":free")) {
            return None;
        }
        credits::free_requests_left()
    }

    /// Images de la demande décrites dès le début du tour, pour un modèle qui
    /// ne les lit pas (elles l'auraient été à la première requête) : le
    /// défaut qu'elles montrent décide de la méthode jointe à la demande.
    fn image_defects(
        &mut self,
        images: &[runtime::attachments::ImageAttachment],
    ) -> Vec<(usize, String)> {
        let primary = self.chain.primary().to_string();
        if !image_reader::model_cannot_read_images(&primary) {
            return Vec::new();
        }
        let sources: Vec<api::ImageSource> = images
            .iter()
            .map(|image| api::ImageSource::base64(image.media_type.clone(), image.data.clone()))
            .collect();
        image_reader::describe(&primary, &sources, &mut |notice| self.notify(notice));
        sources
            .iter()
            .enumerate()
            .filter_map(|(index, source)| {
                image_reader::defect(source).map(|line| (index + 1, line))
            })
            .collect()
    }

    fn answer_superseded(&mut self) {
        if let Some(observer) = &self.observer {
            observer.answer_superseded();
        }
    }

    fn answer_kept(&mut self) {
        if let Some(observer) = &self.observer {
            observer.answer_kept();
        }
    }

    fn consult(
        &mut self,
        purpose: runtime::ConsultPurpose,
        system: &str,
        prompt: &str,
    ) -> Option<runtime::Consultation> {
        let models = tools::advisors::configured_models();
        self.consult_models(purpose, system, prompt, &models)
    }

    fn consult_again(
        &mut self,
        purpose: runtime::ConsultPurpose,
        system: &str,
        prompt: &str,
        excluded: &str,
    ) -> Option<runtime::Consultation> {
        let models = tools::advisors::configured_models()
            .into_iter()
            .filter(|model| model != excluded)
            .collect::<Vec<_>>();
        self.consult_models(purpose, system, prompt, &models)
    }

    fn escalate(&mut self) -> Option<String> {
        if self.saved_chain.is_some() {
            return None;
        }
        // Quelqu'un est là pour décider : Claw propose, il ne change pas seul.
        if let Some(prompter) = self.model_prompter.clone() {
            let current = self.chain.primary().to_string();
            let suggestions = self.model_suggestions(&current);
            if suggestions.is_empty() {
                return None;
            }
            let question = model_switch::ModelQuestion {
                model: current,
                reason: "deux essais n'ont pas suffi".to_string(),
                stuck: true,
                suggestions,
                // Sans réponse, le même modèle continue.
                auto_retry: Some(model_switch::STUCK_AUTO_CONTINUE),
                attempt: None,
                retry_useful: true,
            };
            return match prompter.ask(&question) {
                model_switch::ModelDecision::Switch(next) => {
                    self.switch_to(&next);
                    Some(next)
                }
                model_switch::ModelDecision::Retry => None,
                model_switch::ModelDecision::Stop => {
                    runtime::interrupt::request_interrupt();
                    None
                }
            };
        }
        // Choisi au moment du blocage : le classement et les disponibilités changent.
        let model = self.escalation.target(self.chain.primary())?;
        // Le modèle fort d'abord ; s'il est indisponible, la chaîne habituelle.
        let mut models = vec![model.clone()];
        models.extend(self.chain.models().iter().cloned());
        let stronger = api::ModelChain::new(models, cli_client_factory());
        self.saved_chain = Some(std::mem::replace(&mut self.chain, stronger));
        Some(model)
    }

    fn restore_model(&mut self) {
        if let Some(chain) = self.saved_chain.take() {
            self.chain = chain;
        }
        // Mode équipe : le modèle de la session reprend à la fin du tour.
        if let Some(chain) = self.team_base.take() {
            self.chain = chain;
        }
        self.team_step = None;
        self.team_last = None;
        self.team_coder = None;
    }

    fn plan_step(&mut self, step: runtime::team::Step) {
        self.team_step = Some(step);
    }

    fn team_model(&mut self, role: runtime::team::Role) -> Option<String> {
        let avoid = self
            .team_coder
            .as_deref()
            .filter(|_| role == runtime::team::Role::Review);
        let needs = models::TeamNeeds {
            avoid,
            ..models::TeamNeeds::default()
        };
        models::team_pick(role, &needs).map(|pick| pick.model)
    }

    #[allow(clippy::too_many_lines)]
    fn stream(&mut self, request: ApiRequest) -> Result<Vec<AssistantEvent>, RuntimeError> {
        if let Some(progress_reporter) = &self.progress_reporter {
            progress_reporter.mark_model_phase();
        }
        if let Some(notice) = self.start_notice.take() {
            self.notify(&notice);
        }
        // Mode équipe : le membre le plus fort pour cette étape, et son rôle.
        let role_note = self
            .team_step
            .take()
            .and_then(|step| self.apply_team_step(step, &request.messages));
        let messages = convert_messages(&request.messages);
        let mut system =
            (!request.system_prompt.is_empty()).then(|| request.system_prompt.join("\n\n"));
        if let Some(note) = role_note {
            system = Some(match system {
                Some(prompt) => format!("{prompt}\n\n{note}"),
                None => note,
            });
        }
        let tools = self
            .enable_tools
            .then(|| filter_tool_specs(&self.tool_registry, self.allowed_tools.as_ref()));
        let enable_tools = self.enable_tools;
        let primary = self.chain.primary().to_string();
        // Le raisonnement d'un modèle ne va pas à un autre (changement de
        // membre de l'équipe, escalade) : il n'est renvoyé qu'à son auteur.
        let passback = runtime::harness::enabled(runtime::harness::Aid::ReasoningPassback)
            && self.team_last.as_deref().is_none_or(|last| last == primary);
        self.team_last = Some(primary.clone());
        image_reader::prepare(&primary, &messages, &mut |notice| self.notify(notice));
        // `LookAtImage` sert au modèle qui reçoit des images en descriptions.
        let tools = tools.map(|mut list| {
            if !image_reader::lookable(&primary, &messages) {
                list.retain(|definition| definition.name != "LookAtImage");
            }
            list
        });
        let build = move |model: &str| MessageRequest {
            model: model.to_string(),
            max_tokens: max_tokens_for_model(model),
            messages: messages_for_model(model, &messages, passback),
            system: system.clone(),
            tools: tools.clone(),
            tool_choice: enable_tools.then_some(ToolChoice::Auto),
            stream: true,
        };
        let emit = self.emit_output;
        let observer = self.observer.clone();
        let mut notify = move |text: &str| notify_fallback(emit, observer.as_ref(), text);
        if emit {
            live::show("Réflexion…");
        }

        // Une connexion coupée pendant la réponse ne fait pas échouer le tour :
        // la même requête est relancée. Quand c'est le fournisseur du modèle qui
        // lâche en cours de réponse (504, surcharge), elle repart sur un autre modèle.
        let mut retries_left = BROKEN_STREAM_RETRIES;
        let mut switches_left = api::UPSTREAM_SWITCHES;
        // Essais automatiques du même modèle déjà faits pour cette requête.
        let mut auto_retries = 0_u32;
        let answering = std::cell::RefCell::new(String::new());
        // Réponse en texte arrêtée par la limite de sortie (voir
        // `reply_cut_by_output_limit`).
        let cut = std::cell::Cell::new(false);
        // Suite d'une réponse coupée : l'affichage la raccorde à ce qui est
        // déjà écrit, et un bloc de code resté ouvert le reste pour la suite.
        let continued = runtime::cut_reply_continued(&request.messages);
        let open_fence = continued.as_deref().and_then(render::open_code_fence);
        loop {
            answering.borrow_mut().clear();
            cut.set(false);
            if let (Some(_), Some(observer)) = (&continued, &self.observer) {
                observer.reply_continues();
            }
            let result: Result<Vec<AssistantEvent>, RuntimeError> = self.runtime.block_on(async {
                let opened =
                    runtime::interrupt::interruptible(self.chain.open_stream(&build, &mut notify))
                        .await?;
                answering.borrow_mut().clone_from(&opened.model);
                // Chaque étape de l'agent compte : l'affichage des crédits suit en direct.
                credits::note_request(&opened.model);
                let mut stream = opened.stream;
                let mut first_event = opened.first_event;
                let mut stdout = live::LiveStdout::new();
                let mut sink = io::sink();
                let out: &mut dyn Write = if emit { &mut stdout } else { &mut sink };
                let renderer = TerminalRenderer::new();
                let mut markdown_stream = MarkdownStreamState::default();
                if let Some(fence) = &open_fence {
                    // Rouvert sans être réaffiché : la suite est du code.
                    let _ = markdown_stream.push(&renderer, &format!("{fence}\n"));
                }
                // Réponse préfixée `●`, raisonnement résumé en une ligne `∴`.
                let mut answer = transcript::Prefixer::assistant();
                let mut reasoning = transcript::ThinkingDisplay::new();
                let mut events = Vec::new();
                // Indexé par bloc : les modèles compatibles OpenAI émettent souvent
                // plusieurs appels d'outils en parallèle dont les deltas s'entrelacent.
                let mut pending_tools = PendingTools::new();
                let mut saw_stop = false;
                let mut stop_reason: Option<String> = None;
                let mut signed_thinking = SignedThinking::new();
                // Ni tiret cadratin ni emoji dans ce que Claw écrit ; la réflexion
                // signée, elle, est renvoyée telle quelle au fournisseur.
                let mut text_dashes = runtime::typography::OutputFilter::default();
                let mut thinking_dashes = runtime::typography::OutputFilter::default();

                loop {
                    let event = match first_event.take() {
                        Some(event) => event,
                        None => match runtime::interrupt::interruptible(stream.next_event()).await?
                        {
                            Some(event) => event,
                            None => break,
                        },
                    };
                    match event {
                        ApiStreamEvent::MessageStart(start) => {
                            for block in start.message.content {
                                push_output_block(
                                    block,
                                    0,
                                    out,
                                    &mut events,
                                    &mut pending_tools,
                                    true,
                                )?;
                            }
                        }
                        ApiStreamEvent::ContentBlockStart(start) => {
                            start_signed_thinking(
                                &start.content_block,
                                start.index,
                                &mut signed_thinking,
                            );
                            push_output_block(
                                start.content_block,
                                start.index,
                                out,
                                &mut events,
                                &mut pending_tools,
                                true,
                            )?;
                        }
                        ApiStreamEvent::ContentBlockDelta(delta) => match delta.delta {
                            ContentBlockDelta::TextDelta { text } => {
                                let text = text_dashes.push(&text);
                                if !text.is_empty() {
                                    reasoning
                                        .close(out)
                                        .map_err(|error| RuntimeError::new(error.to_string()))?;
                                    if let Some(progress_reporter) = &self.progress_reporter {
                                        progress_reporter.mark_text_phase(&text);
                                    }
                                    // Un bloc complet : la ligne vide qui le sépare du
                                    // suivant est retirée par le rendu, on la remet.
                                    if let Some(rendered) = markdown_stream.push(&renderer, &text) {
                                        answer.write(out, &format!("{rendered}\n\n")).map_err(
                                            |error| RuntimeError::new(error.to_string()),
                                        )?;
                                    }
                                    if let Some(observer) = &self.observer {
                                        observer.text_delta(&text);
                                    }
                                    events.push(AssistantEvent::TextDelta(text));
                                }
                            }
                            ContentBlockDelta::InputJsonDelta { partial_json } => {
                                if let Some((_, _, input)) = pending_tools.get_mut(&delta.index) {
                                    input.push_str(&partial_json);
                                }
                            }
                            ContentBlockDelta::ThinkingDelta { thinking } => {
                                if let Some(entry) = signed_thinking.get_mut(&delta.index) {
                                    entry.0.push_str(&thinking);
                                }
                                let thinking = thinking_dashes.push(&thinking);
                                if emit {
                                    transcript::record_thinking(&thinking);
                                }
                                reasoning
                                    .push(out, &thinking)
                                    .map_err(|error| RuntimeError::new(error.to_string()))?;
                                if let Some(observer) = &self.observer {
                                    observer.thinking_delta(&thinking);
                                }
                            }
                            ContentBlockDelta::SignatureDelta { signature } => {
                                if let Some(entry) = signed_thinking.get_mut(&delta.index) {
                                    entry.1.push_str(&signature);
                                }
                            }
                        },
                        ApiStreamEvent::ContentBlockStop(stop) => {
                            finish_signed_thinking(stop.index, &mut signed_thinking, &mut events);
                            reasoning
                                .close(out)
                                .map_err(|error| RuntimeError::new(error.to_string()))?;
                            if let Some(rendered) = markdown_stream.flush(&renderer) {
                                answer
                                    .write(out, &rendered)
                                    .map_err(|error| RuntimeError::new(error.to_string()))?;
                            }
                            if let Some((id, name, input)) = pending_tools.remove(&stop.index) {
                                // « Bash », « Read »… (noms de Claude Code) : nom du registre.
                                let name =
                                    self.tool_registry.resolve_tool_name(&name).unwrap_or(name);
                                if let Some(progress_reporter) = &self.progress_reporter {
                                    progress_reporter.mark_tool_phase(&name, &input);
                                }
                                // L'appel s'affiche juste avant son exécution (ou sa
                                // demande d'autorisation), son résultat dessous.
                                answer
                                    .finish_line(out)
                                    .map_err(|error| RuntimeError::new(error.to_string()))?;
                                answer.reset();
                                if emit {
                                    transcript::queue_call(&name, &input);
                                }
                                if let Some(observer) = &self.observer {
                                    observer.tool_call(&name, &input);
                                }
                                events.push(AssistantEvent::ToolUse { id, name, input });
                            }
                        }
                        ApiStreamEvent::MessageDelta(delta) => {
                            if delta.delta.stop_reason.is_some() {
                                stop_reason.clone_from(&delta.delta.stop_reason);
                            }
                            events.push(AssistantEvent::Usage(TokenUsage {
                                input_tokens: delta.usage.input_tokens,
                                output_tokens: delta.usage.output_tokens,
                                cache_creation_input_tokens: 0,
                                cache_read_input_tokens: 0,
                            }));
                        }
                        ApiStreamEvent::MessageStop(_) => {
                            saw_stop = true;
                            if let Some(rendered) = markdown_stream.flush(&renderer) {
                                answer
                                    .write(out, &rendered)
                                    .map_err(|error| RuntimeError::new(error.to_string()))?;
                            }
                            events.push(AssistantEvent::MessageStop);
                        }
                    }
                }
                reasoning
                    .close(out)
                    .map_err(|error| RuntimeError::new(error.to_string()))?;
                answer
                    .finish_line(out)
                    .map_err(|error| RuntimeError::new(error.to_string()))?;
                Self::check_cut_calls(&self.tool_registry, &mut events, stop_reason.as_deref())?;
                cut.set(tool_input::reply_cut_by_output_limit(
                    stop_reason.as_deref(),
                    &events,
                ));

                if !saw_stop
                    && events.iter().any(|event| {
                        matches!(event, AssistantEvent::TextDelta(text) if !text.is_empty())
                            || matches!(event, AssistantEvent::ToolUse { .. })
                    })
                {
                    events.push(AssistantEvent::MessageStop);
                }

                if events
                    .iter()
                    .any(|event| matches!(event, AssistantEvent::MessageStop))
                {
                    return Ok(events);
                }

                let build_complete = |model: &str| MessageRequest {
                    stream: false,
                    ..build(model)
                };
                let (model, response) = runtime::interrupt::interruptible(
                    self.chain.send(&build_complete, &mut notify),
                )
                .await?;
                credits::note_request(&model);
                let stop_reason = response.stop_reason.clone();
                let mut events = response_to_events(response, out)?
                    .into_iter()
                    .map(|event| match event {
                        AssistantEvent::ToolUse { id, name, input } => AssistantEvent::ToolUse {
                            id,
                            name: self.tool_registry.resolve_tool_name(&name).unwrap_or(name),
                            input,
                        },
                        other => other,
                    })
                    .collect::<Vec<_>>();
                Self::check_cut_calls(&self.tool_registry, &mut events, stop_reason.as_deref())?;
                cut.set(tool_input::reply_cut_by_output_limit(
                    stop_reason.as_deref(),
                    &events,
                ));
                if let Some(observer) = &self.observer {
                    for event in &events {
                        match event {
                            AssistantEvent::TextDelta(text) => observer.text_delta(text),
                            AssistantEvent::ToolUse { name, input, .. } => {
                                observer.tool_call(name, input);
                            }
                            AssistantEvent::Thinking { thinking, .. } => {
                                observer.thinking_delta(thinking);
                            }
                            AssistantEvent::Usage(_) | AssistantEvent::MessageStop => {}
                        }
                    }
                }
                Ok(events)
            });
            // Réflexion qui a pris toute la place de la réponse : le niveau de
            // ce modèle vient de baisser d'un cran, l'utilisateur le voit.
            if let Some(notice) = api::take_reasoning_notice() {
                notify(&notice);
            }
            match result {
                Err(error) if retries_left > 0 && is_broken_stream(&error.to_string()) => {
                    retries_left -= 1;
                    if emit {
                        transcript::discard_calls();
                    }
                    if let Some(observer) = &self.observer {
                        observer.reply_discarded();
                    }
                    notify(
                        if error.to_string().contains(tool_input::UNFINISHED_REPLY) {
                            "Réponse du modèle coupée avant la fin d'un appel d'outil : nouvelle tentative"
                        } else {
                            "Connexion coupée pendant la réponse : nouvelle tentative"
                        },
                    );
                    std::thread::sleep(Duration::from_millis(1_500));
                    if emit {
                        live::show("Réflexion…");
                    }
                }
                // Quelqu'un est là pour décider : le même modèle reprend, et
                // Claw demande avant d'en changer.
                Err(error) if self.model_prompter.is_some() => {
                    let Some(reason) = api::model_failure_reason(&error.to_string())
                        .or_else(|| tool_input::unfinished_reason(&error.to_string()))
                    else {
                        return Err(error);
                    };
                    let Some(prompter) = self.model_prompter.clone() else {
                        return Err(error);
                    };
                    let model = self.chain.primary().to_string();
                    if emit {
                        transcript::discard_calls();
                    }
                    if let Some(observer) = &self.observer {
                        observer.reply_discarded();
                    }
                    // Échec passager : le même modèle réessaie de lui-même après un
                    // délai, sans rien demander. L'utilisateur peut en profiter
                    // pour changer de modèle ou arrêter, mais n'a rien à faire.
                    let auto_retry = api::failure_is_transient(&error.to_string())
                        .then(|| model_switch::auto_retry_delay(auto_retries))
                        .flatten();
                    if let Some(delay) = auto_retry {
                        auto_retries += 1;
                        notify(&format!(
                            "{model} : {reason}. Nouvel essai automatique dans {} s (essai {auto_retries} sur {})",
                            delay.as_secs(),
                            model_switch::AUTO_RETRIES
                        ));
                    }
                    let question = model_switch::ModelQuestion {
                        suggestions: self.model_suggestions(&model),
                        model: model.clone(),
                        reason,
                        stuck: false,
                        auto_retry,
                        attempt: auto_retry.map(|_| (auto_retries, model_switch::AUTO_RETRIES)),
                        retry_useful: api::failure_is_transient(&error.to_string())
                            || tool_input::unfinished_reason(&error.to_string()).is_some(),
                    };
                    match prompter.ask(&question) {
                        model_switch::ModelDecision::Retry => {}
                        model_switch::ModelDecision::Switch(next) => {
                            self.switch_to(&next);
                            auto_retries = 0;
                            notify(&format!("{next} prend la suite, à votre demande"));
                        }
                        model_switch::ModelDecision::Stop => {
                            return Err(RuntimeError::new(runtime::interrupt::INTERRUPTED_MESSAGE));
                        }
                    }
                    if emit {
                        live::show("Réflexion…");
                    }
                }
                Err(error) => {
                    let failed = answering.borrow().clone();
                    let Some(status) = api::upstream_failure_status(&error.to_string())
                        .filter(|_| switches_left > 0 && !failed.is_empty())
                    else {
                        // Personne pour décider (script, action GitHub, banc
                        // d'essais) et rien d'autre à essayer : un échec
                        // passager du fournisseur (modèle gratuit limité en
                        // amont, surcharge) se réessaie après une pause au
                        // lieu d'arrêter le tour.
                        let delay =
                            model_switch::unattended_retry_delay(auto_retries).filter(|_| {
                                self.model_prompter.is_none()
                                    && api::failure_is_transient(&error.to_string())
                            });
                        let Some(delay) = delay else {
                            return Err(error);
                        };
                        auto_retries += 1;
                        let model = self.chain.primary().to_string();
                        let reason = api::model_failure_reason(&error.to_string())
                            .unwrap_or_else(|| "échec passager du fournisseur".to_string());
                        notify(&format!(
                            "{model} : {reason}. Nouvel essai automatique dans {} s (essai {auto_retries} sur {})",
                            delay.as_secs(),
                            model_switch::UNATTENDED_RETRIES
                        ));
                        if !model_switch::pause(delay) {
                            return Err(RuntimeError::new(runtime::interrupt::INTERRUPTED_MESSAGE));
                        }
                        if emit {
                            live::show("Réflexion…");
                        }
                        continue;
                    };
                    if emit {
                        transcript::discard_calls();
                    }
                    if let Some(observer) = &self.observer {
                        observer.reply_discarded();
                    }
                    if let Some(next) = self.chain.switch_after_failure(&failed) {
                        switches_left -= 1;
                        notify(&format!(
                            "{failed} a cessé de répondre (erreur {status}) → la requête repart sur {next}"
                        ));
                    } else {
                        // Aucun autre modèle : une seule nouvelle tentative, après une pause.
                        switches_left = 0;
                        notify(&format!(
                            "{failed} a cessé de répondre (erreur {status}) : nouvelle tentative"
                        ));
                        std::thread::sleep(Duration::from_secs(3));
                    }
                    if emit {
                        live::show("Réflexion…");
                    }
                }
                Ok(events) => {
                    self.reply_cut = cut.get();
                    return Ok(events);
                }
            }
        }
    }
}

/// Messages envoyés à `model`. Le raisonnement gardé d'un modèle `OpenRouter`
/// (non signé par Anthropic) n'est renvoyé qu'aux modèles compatibles `OpenAI` :
/// l'API d'Anthropic le refuserait. Il est retiré aussi quand l'aide est coupée.
/// La demande en cours (depuis le dernier message de l'utilisateur) contient des images.
fn images_in_current_request(messages: &[ConversationMessage]) -> bool {
    let start = messages
        .iter()
        .rposition(|message| {
            message.role == MessageRole::User
                && message
                    .blocks
                    .iter()
                    .any(|block| matches!(block, ContentBlock::Text { .. }))
        })
        .unwrap_or(0);
    messages[start..].iter().any(|message| {
        message
            .blocks
            .iter()
            .any(|block| matches!(block, ContentBlock::Image { .. }))
    })
}

/// Taille de la conversation en jetons, à quatre caractères par jeton.
fn conversation_tokens(messages: &[ConversationMessage]) -> u64 {
    let characters: usize = messages
        .iter()
        .flat_map(|message| message.blocks.iter())
        .map(|block| match block {
            ContentBlock::Text { text } => text.len(),
            ContentBlock::ToolUse { input, .. } => input.len(),
            ContentBlock::ToolResult { output, .. } => output.len(),
            ContentBlock::Thinking { thinking, .. } => thinking.len(),
            ContentBlock::Image { .. } => 0,
        })
        .sum();
    u64::try_from(characters / 4).unwrap_or(u64::MAX)
}

fn messages_for_model(model: &str, messages: &[InputMessage], passback: bool) -> Vec<InputMessage> {
    let anthropic = matches!(detect_provider_kind(model), ProviderKind::ClawApi);
    let blind = image_reader::model_cannot_read_images(model);
    // Le raisonnement renvoyé sert à la suite d'appels d'outils en cours. Celui
    // des tours précédents ne sert plus : dans une vraie session, il occupait
    // un sixième du contexte.
    let kept_from = if passback && !anthropic {
        messages
            .iter()
            .rposition(|message| {
                message.role == "user"
                    && message
                        .content
                        .iter()
                        .any(|block| matches!(block, InputContentBlock::Text { .. }))
            })
            .unwrap_or(0)
    } else {
        messages.len()
    };
    messages
        .iter()
        .enumerate()
        .filter_map(|(index, message)| {
            // Images du message, numérotées quand il y en a plusieurs.
            let image_total = message
                .content
                .iter()
                .filter(|block| matches!(block, InputContentBlock::Image { .. }))
                .count();
            let mut image_number = 0;
            let content = message
                .content
                .iter()
                .filter(|block| {
                    index >= kept_from
                        || !matches!(block, InputContentBlock::Thinking { signature, .. }
                            if api::is_passback_reasoning(signature))
                })
                .map(|block| match block {
                    // Sinon OpenRouter refuse toute la requête (erreur 404) : le
                    // modèle reçoit la description de l'image à sa place.
                    InputContentBlock::Image { source } if blind => {
                        image_number += 1;
                        InputContentBlock::Text {
                            text: image_reader::replacement(source, image_number, image_total),
                        }
                    }
                    other => other.clone(),
                })
                .collect::<Vec<_>>();
            (!content.is_empty()).then(|| InputMessage {
                role: message.role.clone(),
                content,
            })
        })
        .collect()
}

/// Modèle de la session en cours : les modèles consultés sont « les autres ».
static ACTIVE_MODEL: Mutex<String> = Mutex::new(String::new());

fn active_model() -> String {
    ACTIVE_MODEL
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
}

/// Relances après une connexion coupée en cours de réponse.
const BROKEN_STREAM_RETRIES: u32 = 2;
/// La réponse s'est interrompue côté réseau (et non par l'utilisateur ou par
/// un refus du fournisseur) : la même requête peut être relancée.
fn is_broken_stream(error: &str) -> bool {
    if runtime::interrupt::is_interruption(error) {
        return false;
    }
    let lower = error.to_ascii_lowercase();
    [
        "error decoding response body",
        "connection reset",
        "connection closed",
        "forcibly closed",
        "unexpected eof",
        "incomplete message",
        "broken pipe",
        // Réponse close avant la fin d'un appel d'outil (`tool_input`).
        tool_input::UNFINISHED_REPLY,
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

/// Durée lisible : `8 s`, `2 min 05 s`.
fn format_elapsed(elapsed: Duration) -> String {
    let seconds = elapsed.as_secs();
    if seconds < 60 {
        format!("{} s", seconds.max(1))
    } else {
        format!("{} min {:02} s", seconds / 60, seconds % 60)
    }
}

/// `/bashes [kill <id>]` : commandes lancées en arrière-plan.
fn render_background_tasks(args: Option<&str>) -> String {
    if let Some(id) = args
        .map(str::trim)
        .and_then(|args| args.strip_prefix("kill"))
        .map(str::trim)
        .filter(|id| !id.is_empty())
    {
        return match runtime::bash_tasks::kill(id) {
            Ok(task) => format!("Tâche {} arrêtée ({})", task.id, task.command),
            Err(error) => error,
        };
    }
    let tasks = runtime::bash_tasks::list();
    let mut lines = vec!["Tâches de fond".to_string()];
    if tasks.is_empty() {
        lines.push("  Aucune commande lancée en arrière-plan.".to_string());
    }
    for task in &tasks {
        let state = if task.running {
            "en cours".to_string()
        } else {
            format!("terminée (code {})", task.exit_code.unwrap_or(-1))
        };
        lines.push(format!(
            "  {:<8} {:<22} {} s  {}",
            task.id, state, task.elapsed_secs, task.command
        ));
    }
    if !tasks.is_empty() {
        lines.push("  Arrêter          /bashes kill <id>".to_string());
    }
    let agents = tools::recent_agent_runs(8);
    if !agents.is_empty() {
        lines.push(String::new());
        lines.push("Sous-agents récents".to_string());
        for agent in agents {
            let state = match agent.status.as_str() {
                "running" => "en cours",
                "completed" => "terminé",
                "failed" => "en échec",
                other => other,
            };
            lines.push(format!(
                "  {:<10} {:<28} {}",
                state,
                transcript::clip(&agent.name, 28),
                transcript::clip(&agent.description, 50)
            ));
            if agent.status == "running" || agent.status == "completed" {
                lines.push(format!("             rapport : {}", agent.output_file));
            }
        }
    }
    lines.join("\n")
}

/// `/think [niveau]` : affiche ou change le niveau de raisonnement.
fn apply_think_command(level: Option<&str>) -> String {
    let current = |effort: Option<api::ReasoningEffort>| {
        effort.map_or("le plus haut que chaque modèle d'OpenRouter accepte (high pour ceux qui raisonnent par budget), réglage du modèle ailleurs", |effort| match effort {
            api::ReasoningEffort::Off => "off (raisonnement masqué)",
            api::ReasoningEffort::Low => "low",
            api::ReasoningEffort::Medium => "medium",
            api::ReasoningEffort::High => "high",
            api::ReasoningEffort::Max => "max (le plus haut niveau que le modèle accepte)",
        })
    };
    let ultracode = if runtime::workflow::ultracode() {
        "activé : chaque demande conséquente est étudiée par un agent au contexte vierge avant le travail, puis contrôlée par un vérificateur indépendant (plus de requêtes, plus lent)"
    } else {
        "désactivé (/think ultracode pour la session, ou le mot « ultracode » dans un message pour ce seul message)"
    };
    match level.map(str::trim).filter(|value| !value.is_empty()) {
        None => format!(
            "Raisonnement\n  Niveau           {}\n  Ultracode        {ultracode}\n  Changer          /think off|low|medium|high|max, /think ultracode [off] (ou \"reasoning\" et \"ultracode\" dans .claw/settings.json)\n  Remarque         s'applique aux modèles qui raisonnent (DeepSeek-R1, Qwen3, GLM, o-series…) et aux modèles Claude via l'API Anthropic",
            current(api::reasoning_effort())
        ),
        Some(value) if value.split_whitespace().next().is_some_and(|word| word.eq_ignore_ascii_case("ultracode")) => {
            apply_ultracode_command(value.split_whitespace().nth(1))
        }
        Some(value) => match api::ReasoningEffort::parse(value) {
            Some(effort) => {
                api::set_reasoning_effort(Some(effort));
                format!("Raisonnement : {}", current(Some(effort)))
            }
            None => format!("Niveau inconnu « {value} » : off, low, medium, high ou max."),
        },
    }
}

/// Niveau de raisonnement au démarrage : `CLAW_REASONING`, sinon la config.
/// `/think ultracode [on|off]` : déroulé ultracode pour la session. L'activer
/// pousse aussi le raisonnement au plus haut niveau que le modèle accepte.
fn apply_ultracode_command(switch: Option<&str>) -> String {
    let switch = switch.map(str::to_lowercase);
    match switch.as_deref() {
        None | Some("on" | "oui" | "activer" | "1") => {
            if !runtime::harness::enabled(runtime::harness::Aid::Ultracode) {
                return "Ultracode est coupé par CLAW_ULTRACODE=0 ou CLAW_HARNESS=0.".to_string();
            }
            runtime::workflow::set_ultracode(true);
            api::set_reasoning_effort(Some(api::ReasoningEffort::Max));
            "Ultracode activé pour la session\n  Avant le travail  un agent au contexte vierge étudie la demande et le code et prépare un dossier : exigences, code concerné, risques, plan vérifiable\n  Après le travail  un vérificateur indépendant contrôle chaque exigence, lance les tests et renvoie ses objections à l'agent\n  Raisonnement      max (le plus haut niveau que le modèle accepte)\n  Coût              une à trois dizaines de requêtes de plus par demande conséquente\n  Couper            /think ultracode off".to_string()
        }
        Some("off" | "non" | "désactiver" | "desactiver" | "0") => {
            runtime::workflow::set_ultracode(false);
            "Ultracode désactivé : retour au travail courant (le niveau de raisonnement ne change pas).".to_string()
        }
        Some(other) => format!(
            "« {other} » : /think ultracode pour l'activer, /think ultracode off pour le couper."
        ),
    }
}

/// `/equipe [off]` : mode équipe pour la session, et qui ferait quoi.
fn apply_team_command(args: Option<&str>, current_model: &str) -> String {
    let switch = args.map(str::trim).map(str::to_lowercase);
    match switch.as_deref() {
        None | Some("" | "on" | "oui" | "activer" | "1") => {
            if !runtime::harness::enabled(runtime::harness::Aid::Team) {
                return "Le mode équipe est coupé par CLAW_TEAM=0 ou CLAW_HARNESS=0.".to_string();
            }
            runtime::team::set_team(true);
            models::team_report(current_model)
        }
        Some("off" | "non" | "désactiver" | "desactiver" | "0") => {
            runtime::team::set_team(false);
            format!("Mode équipe désactivé : {current_model} fait tout le travail.")
        }
        Some("statut" | "etat" | "état" | "status") => models::team_report(current_model),
        Some(other) => format!("« {other} » : /equipe pour l'activer, /equipe off pour le couper."),
    }
}

/// `claw prompt -` : la demande lue sur l'entrée standard. Un moteur y passe une longue
/// conversation : hors de la ligne de commande de Claw, un `pkill -f <texte>` lancé par
/// l'agent ne le trouve pas par le texte de la conversation.
fn prompt_from(prompt: String, mut input: impl io::Read) -> Result<String, String> {
    if prompt.trim() != "-" {
        return Ok(prompt);
    }
    let mut text = String::new();
    input
        .read_to_string(&mut text)
        .map_err(|error| format!("lecture de la demande sur l'entrée standard : {error}"))?;
    if text.trim().is_empty() {
        return Err("la demande lue sur l'entrée standard est vide".to_string());
    }
    Ok(text.trim_end_matches(['\n', '\r']).to_string())
}

/// Mode équipe au démarrage : `CLAW_TEAM`, sinon `"team"` dans la config.
fn init_team(config: Option<&runtime::RuntimeConfig>) {
    let from_env = env::var("CLAW_TEAM").ok().and_then(|value| {
        match value.trim().to_ascii_lowercase().as_str() {
            "1" | "true" | "on" | "oui" | "yes" => Some(true),
            "0" | "false" | "off" | "non" | "no" => Some(false),
            _ => None,
        }
    });
    if let Some(on) = from_env.or_else(|| config.and_then(runtime::RuntimeConfig::team)) {
        runtime::team::set_team(on);
    }
}

/// Ultracode au démarrage : `CLAW_ULTRACODE`, sinon `"ultracode"` dans la config.
fn init_ultracode(config: Option<&runtime::RuntimeConfig>) {
    let from_env = env::var("CLAW_ULTRACODE").ok().and_then(|value| {
        match value.trim().to_ascii_lowercase().as_str() {
            "1" | "true" | "on" | "oui" | "yes" => Some(true),
            "0" | "false" | "off" | "non" | "no" => Some(false),
            _ => None,
        }
    });
    if let Some(on) = from_env.or_else(|| config.and_then(runtime::RuntimeConfig::ultracode)) {
        runtime::workflow::set_ultracode(on);
    }
}

fn init_reasoning_effort(config: Option<&runtime::RuntimeConfig>) {
    let configured = env::var("CLAW_REASONING")
        .ok()
        .or_else(|| config.and_then(|config| config.reasoning().map(ToOwned::to_owned)));
    if let Some(effort) = configured.as_deref().and_then(api::ReasoningEffort::parse) {
        api::set_reasoning_effort(Some(effort));
    }
}

/// Pièces jointes de la demande : images (`@capture.png`, chemin glissé-déposé),
/// encodées, et fichiers ou dossiers `@chemin`, dont le contenu est ajouté au texte.
fn take_image_attachments(input: &str) -> (String, Vec<runtime::attachments::ImageAttachment>) {
    let cwd = env::current_dir().unwrap_or_default();
    let files = runtime::attachments::file_mention_blocks(input, &cwd);
    let (mut text, images, errors) = runtime::attachments::extract_image_attachments(input, &cwd);
    if !files.is_empty() {
        eprintln!("+ {} fichier(s) joint(s)", files.len());
        text = format!("{text}\n\n{}", files.join("\n\n"));
    }
    for error in errors {
        eprintln!("▲ image ignorée : {error}");
    }
    if !images.is_empty() {
        eprintln!("+ {} image(s) jointe(s)", images.len());
    }
    (text, images)
}

/// Signale une bascule de modèle (terminal et interface web).
fn notify_fallback(emit: bool, observer: Option<&SharedObserver>, text: &str) {
    if emit {
        live::suspend(|| eprintln!("\n▲ {text}"));
    }
    if let Some(observer) = observer {
        observer.notice(text);
    }
    collect_notice(text);
}

/// Notices de Claw pendant un tour sans affichage (`--output-format json`) :
/// rendues avec le résultat. Sans elles, rien ne disait par exemple si la
/// relecture par un autre modèle avait eu lieu, approuvé ou échoué.
static JSON_NOTICES: Mutex<Option<Vec<String>>> = Mutex::new(None);

fn start_collecting_notices() {
    *JSON_NOTICES
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(Vec::new());
}

/// Notices gardées depuis `start_collecting_notices` ; la collecte s'arrête.
fn take_notices() -> Vec<String> {
    JSON_NOTICES
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .take()
        .unwrap_or_default()
}

fn collect_notice(text: &str) {
    if let Some(notices) = JSON_NOTICES
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .as_mut()
    {
        notices.push(text.to_string());
    }
}

fn final_assistant_text(summary: &runtime::TurnSummary) -> String {
    summary
        .assistant_messages
        .last()
        .map(|message| {
            message
                .blocks
                .iter()
                .filter_map(|block| match block {
                    ContentBlock::Text { text } => Some(text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("")
        })
        .unwrap_or_default()
}

fn collect_tool_uses(summary: &runtime::TurnSummary) -> Vec<serde_json::Value> {
    summary
        .assistant_messages
        .iter()
        .flat_map(|message| message.blocks.iter())
        .filter_map(|block| match block {
            ContentBlock::ToolUse { id, name, input } => Some(json!({
                "id": id,
                "name": name,
                "input": input,
            })),
            _ => None,
        })
        .collect()
}

fn collect_tool_results(summary: &runtime::TurnSummary) -> Vec<serde_json::Value> {
    summary
        .tool_results
        .iter()
        .flat_map(|message| message.blocks.iter())
        .filter_map(|block| match block {
            ContentBlock::ToolResult {
                tool_use_id,
                tool_name,
                output,
                is_error,
            } => Some(json!({
                "tool_use_id": tool_use_id,
                "tool_name": tool_name,
                "output": output,
                "is_error": is_error,
            })),
            _ => None,
        })
        .collect()
}

/// Menu qui s'ouvre sous la saisie quand on tape « / » : nom et description.
fn slash_command_menu_entries() -> Vec<(String, String)> {
    let mut entries: Vec<(String, String)> = slash_command_specs()
        .iter()
        .map(|spec| (format!("/{}", spec.name), spec.summary.to_string()))
        .collect();
    entries.extend([
        (
            "/vim".to_string(),
            "Active ou coupe l'édition modale vim".to_string(),
        ),
        ("/exit".to_string(), "Quitte Claw".to_string()),
    ]);
    if let Ok(cwd) = env::current_dir() {
        for (name, description) in commands::skill_registry::user_invocable_names(&cwd) {
            let summary = if description.is_empty() {
                "Skill".to_string()
            } else {
                format!("Skill · {description}")
            };
            entries.push((format!("/{name}"), summary));
        }
        for command in commands::discover_custom_commands(&cwd) {
            let summary = command
                .description
                .clone()
                .unwrap_or_else(|| "Commande personnalisée".to_string());
            entries.push((format!("/{}", command.qualified_name()), summary));
        }
    }
    for (name, description) in tools::mcp_prompt_commands() {
        entries.push((
            name,
            description.unwrap_or_else(|| "Prompt MCP".to_string()),
        ));
    }
    entries.sort_by(|left, right| left.0.cmp(&right.0));
    entries.dedup_by(|left, right| left.0 == right.0);
    entries
}

fn slash_command_completion_candidates() -> Vec<String> {
    let mut candidates = slash_command_specs()
        .iter()
        .flat_map(|spec| {
            std::iter::once(spec.name)
                .chain(spec.aliases.iter().copied())
                .map(|name| format!("/{name}"))
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    candidates.extend([
        String::from("/vim"),
        String::from("/exit"),
        String::from("/quit"),
    ]);
    if let Ok(cwd) = env::current_dir() {
        candidates.extend(commands::custom_command_completions(&cwd));
        candidates.extend(
            commands::skill_registry::user_invocable_names(&cwd)
                .into_iter()
                .map(|(name, _)| format!("/{name}")),
        );
    }
    candidates.extend(
        tools::mcp_prompt_commands()
            .into_iter()
            .map(|(name, _)| name),
    );
    candidates.sort();
    candidates.dedup();
    candidates
}

fn suggest_repl_commands(name: &str) -> Vec<String> {
    let normalized = name.trim().trim_start_matches('/').to_ascii_lowercase();
    if normalized.is_empty() {
        return Vec::new();
    }

    let mut ranked = slash_command_completion_candidates()
        .into_iter()
        .filter_map(|candidate| {
            let raw = candidate.trim_start_matches('/').to_ascii_lowercase();
            let distance = edit_distance(&normalized, &raw);
            let prefix_match = raw.starts_with(&normalized) || normalized.starts_with(&raw);
            let near_match = distance <= 2;
            (prefix_match || near_match).then_some((distance, candidate))
        })
        .collect::<Vec<_>>();
    ranked.sort();
    ranked.dedup_by(|left, right| left.1 == right.1);
    ranked
        .into_iter()
        .map(|(_, candidate)| candidate)
        .take(3)
        .collect()
}

fn edit_distance(left: &str, right: &str) -> usize {
    if left == right {
        return 0;
    }
    if left.is_empty() {
        return right.chars().count();
    }
    if right.is_empty() {
        return left.chars().count();
    }

    let right_chars = right.chars().collect::<Vec<_>>();
    let mut previous = (0..=right_chars.len()).collect::<Vec<_>>();
    let mut current = vec![0; right_chars.len() + 1];

    for (left_index, left_char) in left.chars().enumerate() {
        current[0] = left_index + 1;
        for (right_index, right_char) in right_chars.iter().enumerate() {
            let substitution_cost = usize::from(left_char != *right_char);
            current[right_index + 1] = (previous[right_index + 1] + 1)
                .min(current[right_index] + 1)
                .min(previous[right_index] + substitution_cost);
        }
        std::mem::swap(&mut previous, &mut current);
    }

    previous[right_chars.len()]
}

fn extract_tool_path(parsed: &serde_json::Value) -> String {
    let raw = parsed
        .get("file_path")
        .or_else(|| parsed.get("filePath"))
        .or_else(|| parsed.get("path"))
        .and_then(|value| value.as_str())
        .unwrap_or("?");
    // Chemin affiché relatif au projet, sans le préfixe Windows `\\?\`.
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

fn first_visible_line(text: &str) -> &str {
    text.lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or(text)
}

fn summarize_tool_payload(payload: &str) -> String {
    let compact = match serde_json::from_str::<serde_json::Value>(payload) {
        Ok(value) => value.to_string(),
        Err(_) => payload.trim().to_string(),
    };
    truncate_for_summary(&compact, 96)
}

fn truncate_for_summary(value: &str, limit: usize) -> String {
    let mut chars = value.chars();
    let truncated = chars.by_ref().take(limit).collect::<String>();
    if chars.next().is_some() {
        format!("{truncated}…")
    } else {
        truncated
    }
}

/// Appels d'outils en cours de réception, par index de bloc : (id, nom, entrée JSON).
type PendingTools = std::collections::BTreeMap<u32, (String, String, String)>;

/// Raisonnements signés en cours de réception, par index de bloc :
/// (texte, signature, contenu chiffré éventuel).
type SignedThinking = std::collections::BTreeMap<u32, (String, String, Option<String>)>;

/// Début d'un bloc de raisonnement Claude (à renvoyer ensuite à l'API).
fn start_signed_thinking(block: &OutputContentBlock, index: u32, signed: &mut SignedThinking) {
    match block {
        OutputContentBlock::Thinking {
            thinking,
            signature,
        } => {
            signed.insert(
                index,
                (
                    thinking.clone(),
                    signature.clone().unwrap_or_default(),
                    None,
                ),
            );
        }
        OutputContentBlock::RedactedThinking { data } => {
            let data = data
                .as_str()
                .map_or_else(|| data.to_string(), ToOwned::to_owned);
            signed.insert(index, (String::new(), String::new(), Some(data)));
        }
        _ => {}
    }
}

/// Fin d'un bloc : un raisonnement signé devient un événement conservé.
fn finish_signed_thinking(
    index: u32,
    signed: &mut SignedThinking,
    events: &mut Vec<AssistantEvent>,
) {
    if let Some((thinking, signature, redacted)) = signed.remove(&index) {
        if !signature.is_empty() || redacted.is_some() {
            events.push(AssistantEvent::Thinking {
                thinking,
                signature,
                redacted,
            });
        }
    }
}

fn push_output_block(
    block: OutputContentBlock,
    block_index: u32,
    out: &mut (impl Write + ?Sized),
    events: &mut Vec<AssistantEvent>,
    pending_tools: &mut PendingTools,
    streaming_tool_input: bool,
) -> Result<(), RuntimeError> {
    match block {
        OutputContentBlock::Text { text } => {
            let text = runtime::typography::cleaned(&text).into_owned();
            if !text.is_empty() {
                let rendered = TerminalRenderer::new().markdown_to_ansi(&text);
                let mut answer = transcript::Prefixer::assistant();
                answer
                    .write(out, &rendered)
                    .and_then(|()| answer.finish_line(out))
                    .map_err(|error| RuntimeError::new(error.to_string()))?;
                events.push(AssistantEvent::TextDelta(text));
            }
        }
        OutputContentBlock::ToolUse { id, name, input } => {
            // During streaming, the initial content_block_start has an empty input ({}).
            // The real input arrives via input_json_delta events. In
            // non-streaming responses, preserve a legitimate empty object.
            let initial_input = if streaming_tool_input
                && input.is_object()
                && input.as_object().is_some_and(serde_json::Map::is_empty)
            {
                String::new()
            } else {
                input.to_string()
            };
            pending_tools.insert(block_index, (id, name, initial_input));
        }
        // Réponse complète (sans flux) : le raisonnement signé est conservé.
        OutputContentBlock::Thinking {
            thinking,
            signature,
        } if !streaming_tool_input => {
            if let Some(signature) = signature.filter(|signature| !signature.is_empty()) {
                events.push(AssistantEvent::Thinking {
                    thinking,
                    signature,
                    redacted: None,
                });
            }
        }
        OutputContentBlock::RedactedThinking { data } if !streaming_tool_input => {
            events.push(AssistantEvent::Thinking {
                thinking: String::new(),
                signature: String::new(),
                redacted: Some(
                    data.as_str()
                        .map_or_else(|| data.to_string(), ToOwned::to_owned),
                ),
            });
        }
        OutputContentBlock::Thinking { .. } | OutputContentBlock::RedactedThinking { .. } => {}
    }
    Ok(())
}

fn response_to_events(
    response: MessageResponse,
    out: &mut (impl Write + ?Sized),
) -> Result<Vec<AssistantEvent>, RuntimeError> {
    let mut events = Vec::new();
    let mut pending_tools = PendingTools::new();

    for block in response.content {
        push_output_block(block, 0, out, &mut events, &mut pending_tools, false)?;
        if let Some((id, name, input)) = pending_tools.remove(&0) {
            events.push(AssistantEvent::ToolUse { id, name, input });
        }
    }

    events.push(AssistantEvent::Usage(TokenUsage {
        input_tokens: response.usage.input_tokens,
        output_tokens: response.usage.output_tokens,
        cache_creation_input_tokens: response.usage.cache_creation_input_tokens,
        cache_read_input_tokens: response.usage.cache_read_input_tokens,
    }));
    events.push(AssistantEvent::MessageStop);
    Ok(events)
}

struct CliToolExecutor {
    emit_output: bool,
    allowed_tools: Option<AllowedToolSet>,
    tool_registry: GlobalToolRegistry,
    observer: Option<SharedObserver>,
}

impl CliToolExecutor {
    fn new(
        allowed_tools: Option<AllowedToolSet>,
        emit_output: bool,
        tool_registry: GlobalToolRegistry,
    ) -> Self {
        Self {
            emit_output,
            allowed_tools,
            tool_registry,
            observer: None,
        }
    }

    fn with_observer(mut self, observer: Option<SharedObserver>) -> Self {
        self.observer = observer;
        self
    }
}

impl CliToolExecutor {
    fn check_allowed(&self, tool_name: &str) -> Result<(), ToolError> {
        if self
            .allowed_tools
            .as_ref()
            .is_some_and(|allowed| !allowed.contains(tool_name))
        {
            return Err(ToolError::new(format!(
                "l'outil `{tool_name}` n'est pas activé par l'option --allowedTools actuelle"
            )));
        }
        Ok(())
    }

    fn run_tool(
        registry: &GlobalToolRegistry,
        tool_name: &str,
        input: &str,
    ) -> Result<String, String> {
        let value = serde_json::from_str(input).map_err(|error| {
            format!(
                "invalid tool input JSON: {error} - send the call again with a valid JSON object"
            )
        })?;
        registry.execute(tool_name, &value)
    }
}

impl ToolExecutor for CliToolExecutor {
    fn parallel_safe(&self, tool_name: &str) -> bool {
        tools::PARALLEL_SAFE_TOOLS.contains(&tool_name)
    }

    fn execute_batch(&mut self, calls: &[(String, String)]) -> Vec<Result<String, ToolError>> {
        if self.emit_output {
            live::show(&format!("{} outils en parallèle…", calls.len()));
        }
        let outcomes = std::thread::scope(|scope| {
            let handles = calls
                .iter()
                .map(|(name, input)| {
                    let registry = &self.tool_registry;
                    let allowed = self.check_allowed(name);
                    scope.spawn(move || match allowed {
                        Ok(()) => Self::run_tool(registry, name, input),
                        Err(error) => Err(error.to_string()),
                    })
                })
                .collect::<Vec<_>>();
            handles
                .into_iter()
                .map(|handle| {
                    handle
                        .join()
                        .unwrap_or_else(|_| Err("tool thread panicked".to_string()))
                })
                .collect::<Vec<_>>()
        });
        calls
            .iter()
            .zip(outcomes)
            .map(|((name, input), result)| {
                if self.emit_output {
                    transcript::print_call(name, input);
                }
                self.report(name, input, result)
            })
            .collect()
    }

    fn announce(&mut self, tool_name: &str, input: &str) {
        if self.emit_output {
            transcript::queue_call(tool_name, input);
        }
        if let Some(observer) = &self.observer {
            observer.tool_call(tool_name, input);
        }
    }

    /// Appel refusé ou interrompu avant son exécution : sans ce résultat, la
    /// page web et les éditeurs l'afficheraient « en cours » indéfiniment.
    fn skipped(&mut self, tool_name: &str, _input: &str, reason: &str) {
        if let Some(observer) = &self.observer {
            observer.tool_result(tool_name, reason, true);
        }
    }

    /// Arguments obligatoires absents, JSON invalide ou appel coupé : l'erreur
    /// va au modèle sans qu'une autorisation soit demandée pour un appel vide.
    fn invalid_input(&mut self, tool_name: &str, input: &str) -> Option<String> {
        let definitions = self.tool_registry.definitions(None);
        // Un outil qui n'existe pas : dit tel quel, sans demander d'autorisation.
        let Some(definition) = definitions
            .iter()
            .find(|definition| definition.name == tool_name)
        else {
            let known: Vec<String> = definitions
                .iter()
                .map(|definition| definition.name.clone())
                .collect();
            return Some(tools::unknown_tool_message(tool_name, &known));
        };
        let known: Vec<String> = definition
            .input_schema
            .get("properties")
            .and_then(serde_json::Value::as_object)
            .map(|properties| properties.keys().cloned().collect())
            .unwrap_or_default();
        tool_input::problem(
            tool_name,
            input,
            &tool_input::required_arguments(&definition.input_schema),
            &known,
        )
    }

    fn argument_note(&mut self, tool_name: &str, input: &str) -> Option<String> {
        tool_input::take_argument_note(tool_name, input)
    }

    /// Captures d'écran de l'appel (outil `Screenshot`), jointes à son résultat.
    fn take_images(&mut self) -> Vec<runtime::attachments::ImageAttachment> {
        tools::take_screenshots()
    }

    fn execute(&mut self, tool_name: &str, input: &str) -> Result<String, ToolError> {
        if self.emit_output {
            transcript::print_call(tool_name, input);
        }
        if let Err(error) = self.check_allowed(tool_name) {
            return self.report(tool_name, input, Err(error.to_string()));
        }
        if self.emit_output {
            live::show(&transcript::running_label(tool_name));
        }
        let result = Self::run_tool(&self.tool_registry, tool_name, input);
        self.report(tool_name, input, result)
    }
}

impl CliToolExecutor {
    /// Affiche le résultat (terminal, observateur) et le convertit.
    fn report(
        &mut self,
        tool_name: &str,
        input: &str,
        result: Result<String, String>,
    ) -> Result<String, ToolError> {
        if let Some(observer) = &self.observer {
            match &result {
                Ok(output) => observer.tool_result(tool_name, output, false),
                Err(error) => observer.tool_result(tool_name, error, true),
            }
        }
        if self.emit_output {
            let (text, is_error) = match &result {
                Ok(output) => (output.as_str(), false),
                Err(error) => (error.as_str(), true),
            };
            transcript::record_result(tool_name, input, text, is_error);
            let mut out = live::LiveStdout::new();
            writeln!(
                out,
                "{}",
                transcript::tool_result(tool_name, text, is_error)
            )
            .and_then(|()| out.flush())
            .map_err(|error| ToolError::new(error.to_string()))?;
        }
        result.map_err(ToolError::new)
    }
}

fn permission_policy(mode: PermissionMode, tool_registry: &GlobalToolRegistry) -> PermissionPolicy {
    tool_registry.permission_specs(None).into_iter().fold(
        PermissionPolicy::new(mode),
        |policy, (name, required_permission)| {
            policy.with_tool_requirement(name, required_permission)
        },
    )
}

fn convert_messages(messages: &[ConversationMessage]) -> Vec<InputMessage> {
    messages
        .iter()
        .filter_map(|message| {
            let role = match message.role {
                MessageRole::System | MessageRole::User | MessageRole::Tool => "user",
                MessageRole::Assistant => "assistant",
            };
            let content = message
                .blocks
                .iter()
                .map(|block| match block {
                    ContentBlock::Text { text } => InputContentBlock::Text { text: text.clone() },
                    ContentBlock::ToolUse { id, name, input } => InputContentBlock::ToolUse {
                        id: id.clone(),
                        name: name.clone(),
                        input: serde_json::from_str(input)
                            .unwrap_or_else(|_| serde_json::json!({ "raw": input })),
                    },
                    ContentBlock::ToolResult {
                        tool_use_id,
                        output,
                        is_error,
                        ..
                    } => InputContentBlock::ToolResult {
                        tool_use_id: tool_use_id.clone(),
                        content: vec![ToolResultContentBlock::Text {
                            text: output.clone(),
                        }],
                        is_error: *is_error,
                    },
                    ContentBlock::Image { media_type, data } => InputContentBlock::Image {
                        source: api::ImageSource::base64(media_type.clone(), data.clone()),
                    },
                    ContentBlock::Thinking {
                        thinking,
                        signature,
                        redacted,
                    } => match redacted {
                        Some(data) => InputContentBlock::RedactedThinking { data: data.clone() },
                        None => InputContentBlock::Thinking {
                            thinking: thinking.clone(),
                            signature: signature.clone(),
                        },
                    },
                })
                .collect::<Vec<_>>();
            (!content.is_empty()).then(|| InputMessage {
                role: role.to_string(),
                content,
            })
        })
        .collect()
}

#[allow(clippy::too_many_lines)]
fn print_help_to(out: &mut impl Write) -> io::Result<()> {
    const LINES: &[&str] = &[
        "  Agent de code et d'architecture pour le dossier courant (OpenRouter, Anthropic…).",
        "",
        "Pour commencer",
        "  claw                                  Lance le REPL interactif",
        "  claw \"résume ce dépôt\"                Exécute une demande puis quitte",
        "  claw prompt \"explique src/main.rs\"    Demande ponctuelle explicite",
        "  claw /review                          Commande spécialisée ou personnalisée ponctuelle",
        "  claw serve                            Interface web locale",
        "  claw --resume SESSION.json /status    Inspecte une session enregistrée",
        "",
        "Dans le REPL",
        "  /help                                 Toutes les commandes slash",
        "  /status                               Session et espace de travail",
        "  /model <nom>                          Change de modèle en cours de session",
        "  /permissions <mode>                   Règle l'accès aux outils",
        "  /bg [demande], /fork [demande]        Passe la session (ou une copie) en arrière-plan",
        "  Ctrl+C                                Interrompt le tour en cours (/undo l'annule)",
        "  Tab                                   Complète les commandes slash",
        "  /vim                                  Active l'édition modale",
        "  Maj+Entrée / Ctrl+J                   Insère un saut de ligne",
        "",
        "Sous-commandes",
        "  claw serve [--port N] [--no-open]     Interface web locale (navigateur)",
        "  claw mcp add|list|remove|login        Serveurs MCP (stdio, http, sse, OAuth)",
        "  claw acp                              Agent pour les éditeurs (Zed, Neovim… via ACP)",
        "  claw models [refresh|recherche]       Modèles gratuits recommandés pour le code",
        "  claw credits                          Crédits OpenRouter : solde, requêtes gratuites restantes",
        "  claw --bg \"tâche\" [--name NOM]       Lance une session en arrière-plan (--exec 'cmd' : commande shell)",
        "  claw agents [--json] [--all]          Sessions en arrière-plan (vue, ou JSON pour les scripts)",
        "  claw attach|logs|reply|stop|rm ID     Ouvre, lit, répond, arrête ou supprime une session",
        "  claw agents list                      Liste les sous-agents configurés",
        "  claw skills                           Liste les compétences installées",
        "  claw init                             Crée CLAW.md et les fichiers locaux",
        "  claw login                            Connexion OAuth",
        "  claw logout                           Supprime les identifiants OAuth enregistrés",
        "  claw system-prompt [--cwd CHEMIN] [--date AAAA-MM-JJ]",
        "  claw dump-manifests                   (développement) lit les sources TS d'origine",
        "  claw bootstrap-plan                   (développement) affiche les phases de démarrage",
        "",
        "Options",
        "  --model MODÈLE                        Modèle à utiliser (ex. qwen/qwen3-coder:free)",
        "  --output-format FORMAT                Sortie non interactive : text, json ou stream-json",
        "  --input-format stream-json            Tours lus en JSON sur l'entrée standard (scripts, éditeurs)",
        "  --continue, -c                        Reprend la dernière session du projet",
        "  --permission-mode MODE                prompt (défaut), read-only (plan), workspace-write, auto ou danger-full-access",
        "  --dangerously-skip-permissions        Désactive toutes les vérifications de permission",
        "  --allowedTools OUTILS                 Limite les outils (répétable ; alias séparés par des virgules)",
        "  --version, -V                         Version et informations de build",
    ];
    writeln!(out, "Claw Code CLI v{VERSION}")?;
    for line in LINES {
        writeln!(out, "{line}")?;
    }
    writeln!(out)?;
    writeln!(out, "Référence des commandes slash")?;
    writeln!(out, "{}", render_slash_command_help())?;
    writeln!(out)?;
    let resume_commands = resume_supported_slash_commands()
        .into_iter()
        .map(|spec| match spec.argument_hint {
            Some(argument_hint) => format!("/{} {}", spec.name, argument_hint),
            None => format!("/{}", spec.name),
        })
        .collect::<Vec<_>>()
        .join(", ");
    writeln!(
        out,
        "Commandes utilisables avec --resume : {resume_commands}"
    )?;
    writeln!(out, "Exemples")?;
    for example in [
        "  claw --model qwen/qwen3-coder:free \"résume ce dépôt\"",
        "  claw --output-format json prompt \"explique src/main.rs\"",
        "  claw --allowedTools read,glob \"résume Cargo.toml\"",
        "  claw --resume session.json /status /diff /export notes.txt",
        "  claw /fix \"cargo test\"",
        "  claw serve --port 8787",
    ] {
        writeln!(out, "{example}")?;
    }
    Ok(())
}

fn print_help() {
    let _ = print_help_to(&mut io::stdout());
}

#[cfg(test)]
mod tests {
    use super::{
        describe_permission_target, expert_command_prompt, parse_permission_answer,
        persist_allow_rule, ExpertCommand, PermissionAnswer,
    };

    #[test]
    fn notices_of_a_json_turn_are_kept_for_its_result() {
        // Essais réels : la sortie JSON ne disait pas si la relecture avait
        // eu lieu, approuvé ou échoué.
        super::start_collecting_notices();
        super::notify_fallback(false, None, "Relecture par relecteur : rien à corriger");
        assert_eq!(
            super::take_notices(),
            vec!["Relecture par relecteur : rien à corriger".to_string()]
        );
        // Hors d'un tour en JSON, rien n'est gardé.
        super::notify_fallback(false, None, "affichée ailleurs");
        assert!(super::take_notices().is_empty());
    }

    #[test]
    fn kept_reasoning_goes_back_only_with_the_current_turn() {
        use api::{InputContentBlock, InputMessage};
        let thinking = |text: &str| InputContentBlock::Thinking {
            thinking: text.to_string(),
            signature: "or-plain".to_string(),
        };
        let message = |role: &str, content: Vec<InputContentBlock>| InputMessage {
            role: role.to_string(),
            content,
        };
        let text = |text: &str| InputContentBlock::Text {
            text: text.to_string(),
        };
        let messages = vec![
            message("user", vec![text("première demande")]),
            message("assistant", vec![thinking("ancien"), text("fait")]),
            message("user", vec![text("deuxième demande")]),
            message(
                "assistant",
                vec![
                    thinking("en cours"),
                    InputContentBlock::ToolUse {
                        id: "appel-1".to_string(),
                        name: "read_file".to_string(),
                        input: serde_json::json!({ "path": "a.py" }),
                    },
                ],
            ),
            message(
                "user",
                vec![InputContentBlock::ToolResult {
                    tool_use_id: "appel-1".to_string(),
                    content: Vec::new(),
                    is_error: false,
                }],
            ),
        ];
        let thoughts = |sent: &[InputMessage]| {
            sent.iter()
                .flat_map(|message| &message.content)
                .filter_map(|block| match block {
                    InputContentBlock::Thinking { thinking, .. } => Some(thinking.clone()),
                    _ => None,
                })
                .collect::<Vec<_>>()
        };
        // Le raisonnement du tour en cours accompagne ses appels d'outils ;
        // celui des tours finis ne repart plus à chaque requête.
        let sent = super::messages_for_model("qwen/qwen3-coder:free", &messages, true);
        assert_eq!(thoughts(&sent), ["en cours"]);
        assert_eq!(sent.len(), messages.len());
        // Aide coupée : aucun raisonnement renvoyé.
        let plain = super::messages_for_model("qwen/qwen3-coder:free", &messages, false);
        assert!(thoughts(&plain).is_empty());
    }

    #[test]
    fn images_are_not_sent_to_a_model_that_cannot_read_them() {
        use api::{InputContentBlock, InputMessage};
        let model = |id: &str, images: bool| api::OpenRouterModel {
            id: id.to_string(),
            supports_images: images,
            ..Default::default()
        };
        api::register_model_limits(&api::ModelCatalog {
            models: vec![
                model("essai/texte-seul:free", false),
                model("essai/vision:free", true),
            ],
            ..Default::default()
        });
        let messages = vec![InputMessage {
            role: "user".to_string(),
            content: vec![
                InputContentBlock::Text {
                    text: "regarde".to_string(),
                },
                InputContentBlock::Image {
                    source: api::ImageSource::base64("image/png".to_string(), "AAAA".to_string()),
                },
            ],
        }];
        let images = |sent: &[InputMessage]| {
            sent.iter()
                .flat_map(|message| &message.content)
                .filter(|block| matches!(block, InputContentBlock::Image { .. }))
                .count()
        };
        // OpenRouter refuse (404) toute requête avec une image pour un modèle
        // qui ne lit que du texte : l'image est remplacée par sa description,
        // ou par une mention quand personne ne l'a décrite.
        let blind = super::messages_for_model("essai/texte-seul:free", &messages, true);
        assert_eq!(images(&blind), 0);
        assert!(matches!(
            &blind[0].content[1],
            InputContentBlock::Text { text } if text.starts_with("[Image not sent")
        ));
        // Modèle qui lit les images, ou inconnu du catalogue : rien ne change.
        assert_eq!(
            images(&super::messages_for_model(
                "essai/vision:free",
                &messages,
                true
            )),
            1
        );
        assert_eq!(
            images(&super::messages_for_model("ollama:llava", &messages, true)),
            1
        );
    }
    use super::{
        describe_tool_progress, filter_tool_specs, format_compact_report, format_cost_report,
        format_internal_prompt_progress_line, format_model_report, format_model_switch_report,
        format_permissions_report, format_permissions_switch_report, format_resume_report,
        format_status_report, normalize_permission_mode, parse_args, parse_git_status_metadata,
        permission_policy, print_help_to, push_output_block, render_config_report,
        render_memory_report, render_repl_help, render_unknown_repl_command, resolve_model_alias,
        response_to_events, resume_supported_slash_commands, slash_command_completion_candidates,
        status_context, CliAction, CliOutputFormat, InternalPromptProgressEvent,
        InternalPromptProgressState, PendingTools, SlashCommand, StatusUsage, DEFAULT_MODEL,
    };
    use api::{MessageResponse, OutputContentBlock, Usage};
    use plugins::{PluginTool, PluginToolDefinition, PluginToolPermission};
    use runtime::{
        AssistantEvent, ContentBlock, ConversationMessage, MessageRole, PermissionMode,
        PermissionRules,
    };
    use serde_json::json;
    use std::fs;
    use std::path::PathBuf;
    use std::time::Duration;
    use tools::GlobalToolRegistry;

    fn registry_with_plugin_tool() -> GlobalToolRegistry {
        GlobalToolRegistry::with_plugin_tools(vec![PluginTool::new(
            "plugin-demo@external",
            "plugin-demo",
            PluginToolDefinition {
                name: "plugin_echo".to_string(),
                description: Some("Echo plugin payload".to_string()),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "message": { "type": "string" }
                    },
                    "required": ["message"],
                    "additionalProperties": false
                }),
            },
            "echo".to_string(),
            Vec::new(),
            PluginToolPermission::WorkspaceWrite,
            None,
        )])
        .expect("plugin tool registry should build")
    }

    #[test]
    fn defaults_to_repl_when_no_args() {
        assert_eq!(
            parse_args(&[]).expect("args should parse"),
            CliAction::Repl {
                model: DEFAULT_MODEL.to_string(),
                allowed_tools: None,
                permission_mode: PermissionMode::Prompt,
            }
        );
    }

    #[test]
    fn parses_prompt_subcommand() {
        let args = vec![
            "prompt".to_string(),
            "hello".to_string(),
            "world".to_string(),
        ];
        assert_eq!(
            parse_args(&args).expect("args should parse"),
            CliAction::Prompt {
                prompt: "hello world".to_string(),
                model: DEFAULT_MODEL.to_string(),
                output_format: CliOutputFormat::Text,
                allowed_tools: None,
                permission_mode: PermissionMode::Prompt,
            }
        );
    }

    #[test]
    fn a_dash_prompt_is_read_from_standard_input() {
        assert_eq!(
            super::prompt_from(
                "-".to_string(),
                "Mets-le en ligne\n\n[Claw] Earlier conversation\n".as_bytes()
            ),
            Ok("Mets-le en ligne\n\n[Claw] Earlier conversation".to_string())
        );
        assert_eq!(
            super::prompt_from("bonjour".to_string(), "ignoré".as_bytes()),
            Ok("bonjour".to_string())
        );
        assert!(super::prompt_from("-".to_string(), " \n".as_bytes()).is_err());
    }

    #[test]
    fn parses_bare_prompt_and_json_output_flag() {
        let args = vec![
            "--output-format=json".to_string(),
            "--model".to_string(),
            "custom-opus".to_string(),
            "explain".to_string(),
            "this".to_string(),
        ];
        assert_eq!(
            parse_args(&args).expect("args should parse"),
            CliAction::Prompt {
                prompt: "explain this".to_string(),
                model: "custom-opus".to_string(),
                output_format: CliOutputFormat::Json,
                allowed_tools: None,
                permission_mode: PermissionMode::Prompt,
            }
        );
    }

    #[test]
    fn resolves_model_aliases_in_args() {
        let args = vec![
            "--model".to_string(),
            "opus".to_string(),
            "explain".to_string(),
            "this".to_string(),
        ];
        assert_eq!(
            parse_args(&args).expect("args should parse"),
            CliAction::Prompt {
                prompt: "explain this".to_string(),
                model: "claude-opus-4-6".to_string(),
                output_format: CliOutputFormat::Text,
                allowed_tools: None,
                permission_mode: PermissionMode::Prompt,
            }
        );
    }

    #[test]
    fn resolves_known_model_aliases() {
        assert_eq!(resolve_model_alias("opus"), "claude-opus-4-6");
        assert_eq!(resolve_model_alias("sonnet"), "claude-sonnet-4-6");
        assert_eq!(resolve_model_alias("haiku"), "claude-haiku-4-5-20251001");
        assert_eq!(resolve_model_alias("qwen/qwen3-coder"), "qwen/qwen3-coder");
        assert_eq!(resolve_model_alias("custom-opus"), "custom-opus");
        // « free » : le gratuit le plus puissant du moment (le routeur gratuit dans les tests).
        assert_eq!(
            resolve_model_alias("free"),
            crate::models::FREE_ROUTER_MODEL
        );
        assert_eq!(
            resolve_model_alias(" Gratuit "),
            crate::models::FREE_ROUTER_MODEL
        );
    }

    #[test]
    fn parses_version_flags_without_initializing_prompt_mode() {
        assert_eq!(
            parse_args(&["--version".to_string()]).expect("args should parse"),
            CliAction::Version
        );
        assert_eq!(
            parse_args(&["-V".to_string()]).expect("args should parse"),
            CliAction::Version
        );
    }

    #[test]
    fn parses_permission_mode_flag() {
        let args = vec!["--permission-mode=read-only".to_string()];
        assert_eq!(
            parse_args(&args).expect("args should parse"),
            CliAction::Repl {
                model: DEFAULT_MODEL.to_string(),
                allowed_tools: None,
                permission_mode: PermissionMode::ReadOnly,
            }
        );
    }

    #[test]
    fn parses_allowed_tools_flags_with_aliases_and_lists() {
        let args = vec![
            "--allowedTools".to_string(),
            "read,glob".to_string(),
            "--allowed-tools=write_file".to_string(),
        ];
        assert_eq!(
            parse_args(&args).expect("args should parse"),
            CliAction::Repl {
                model: DEFAULT_MODEL.to_string(),
                allowed_tools: Some(
                    ["glob_search", "read_file", "write_file"]
                        .into_iter()
                        .map(str::to_string)
                        .collect()
                ),
                permission_mode: PermissionMode::Prompt,
            }
        );
    }

    #[test]
    fn rejects_unknown_allowed_tools() {
        let error = parse_args(&["--allowedTools".to_string(), "teleport".to_string()])
            .expect_err("tool should be rejected");
        assert!(error.contains("unsupported tool in --allowedTools: teleport"));
    }

    #[test]
    fn parses_system_prompt_options() {
        let args = vec![
            "system-prompt".to_string(),
            "--cwd".to_string(),
            "/tmp/project".to_string(),
            "--date".to_string(),
            "2026-04-01".to_string(),
        ];
        assert_eq!(
            parse_args(&args).expect("args should parse"),
            CliAction::PrintSystemPrompt {
                cwd: PathBuf::from("/tmp/project"),
                date: "2026-04-01".to_string(),
            }
        );
    }

    #[test]
    fn parses_login_and_logout_subcommands() {
        assert_eq!(
            parse_args(&["login".to_string()]).expect("login should parse"),
            CliAction::Login
        );
        assert_eq!(
            parse_args(&["logout".to_string()]).expect("logout should parse"),
            CliAction::Logout
        );
        assert_eq!(
            parse_args(&["init".to_string()]).expect("init should parse"),
            CliAction::Init
        );
        assert!(matches!(
            parse_args(&["agents".to_string()]).expect("agents should parse"),
            CliAction::AgentView { json: false, .. }
        ));
        assert!(matches!(
            parse_args(&[
                "agents".to_string(),
                "--json".to_string(),
                "--all".to_string()
            ])
            .expect("agents --json should parse"),
            CliAction::AgentView {
                json: true,
                all: true,
                ..
            }
        ));
        assert_eq!(
            parse_args(&["agents".to_string(), "list".to_string()]).expect("agents list"),
            CliAction::Agents {
                args: Some("list".to_string())
            }
        );
        assert!(matches!(
            parse_args(&[
                "--bg".to_string(),
                "--name".to_string(),
                "fix".to_string(),
                "corrige".to_string(),
                "le test".to_string()
            ])
            .expect("--bg should parse"),
            CliAction::Background { ref prompt, ref name, .. }
                if prompt == "corrige le test" && name.as_deref() == Some("fix")
        ));
        assert!(parse_args(&["--bg".to_string(), "-p".to_string(), "x".to_string()]).is_err());
        assert_eq!(
            parse_args(&["logs".to_string(), "-f".to_string(), "7c5d".to_string()]).expect("logs"),
            CliAction::JobLogs {
                target: "7c5d".to_string(),
                follow: true
            }
        );
        assert_eq!(
            parse_args(&["reply".to_string(), "7c5d".to_string(), "oui".to_string()])
                .expect("reply"),
            CliAction::JobReply {
                target: "7c5d".to_string(),
                text: "oui".to_string()
            }
        );
        assert_eq!(
            parse_args(&["skills".to_string()]).expect("skills should parse"),
            CliAction::Skills { args: None }
        );
        assert_eq!(
            parse_args(&["agents".to_string(), "--help".to_string()])
                .expect("agents help should parse"),
            CliAction::Agents {
                args: Some("--help".to_string())
            }
        );
    }

    #[test]
    fn parses_direct_agents_and_skills_slash_commands() {
        assert_eq!(
            parse_args(&["/agents".to_string()]).expect("/agents should parse"),
            CliAction::Agents { args: None }
        );
        assert_eq!(
            parse_args(&["/skills".to_string()]).expect("/skills should parse"),
            CliAction::Skills { args: None }
        );
        assert_eq!(
            parse_args(&["/skills".to_string(), "help".to_string()])
                .expect("/skills help should parse"),
            CliAction::Skills {
                args: Some("help".to_string())
            }
        );
        let error = parse_args(&["/status".to_string()])
            .expect_err("/status should remain REPL-only when invoked directly");
        assert!(error.contains("Commande slash indisponible hors du REPL"));
        assert!(error.contains("/status"));
    }

    #[test]
    fn parses_resume_flag_with_slash_command() {
        let args = vec![
            "--resume".to_string(),
            "session.json".to_string(),
            "/compact".to_string(),
        ];
        assert_eq!(
            parse_args(&args).expect("args should parse"),
            CliAction::ResumeSession {
                session_path: PathBuf::from("session.json"),
                commands: vec!["/compact".to_string()],
            }
        );
    }

    #[test]
    fn parses_resume_flag_with_multiple_slash_commands() {
        let args = vec![
            "--resume".to_string(),
            "session.json".to_string(),
            "/status".to_string(),
            "/compact".to_string(),
            "/cost".to_string(),
        ];
        assert_eq!(
            parse_args(&args).expect("args should parse"),
            CliAction::ResumeSession {
                session_path: PathBuf::from("session.json"),
                commands: vec![
                    "/status".to_string(),
                    "/compact".to_string(),
                    "/cost".to_string(),
                ],
            }
        );
    }

    #[test]
    fn filtered_tool_specs_respect_allowlist() {
        let allowed = ["read_file", "grep_search"]
            .into_iter()
            .map(str::to_string)
            .collect();
        let filtered = filter_tool_specs(&GlobalToolRegistry::builtin(), Some(&allowed));
        let names = filtered
            .into_iter()
            .map(|spec| spec.name)
            .collect::<Vec<_>>();
        assert_eq!(names, vec!["read_file", "grep_search"]);
    }

    #[test]
    fn filtered_tool_specs_include_plugin_tools() {
        let filtered = filter_tool_specs(&registry_with_plugin_tool(), None);
        let names = filtered
            .into_iter()
            .map(|definition| definition.name)
            .collect::<Vec<_>>();
        assert!(names.contains(&"bash".to_string()));
        assert!(names.contains(&"plugin_echo".to_string()));
    }

    #[test]
    fn permission_policy_uses_plugin_tool_permissions() {
        let policy = permission_policy(PermissionMode::ReadOnly, &registry_with_plugin_tool());
        let required = policy.required_mode_for("plugin_echo");
        assert_eq!(required, PermissionMode::WorkspaceWrite);
    }

    #[test]
    fn shared_help_uses_resume_annotation_copy() {
        let help = commands::render_slash_command_help();
        assert!(help.contains("Commandes slash"));
        assert!(help.contains("Tab complète les commandes dans le REPL."));
        assert!(help.contains("disponible via claw --resume SESSION.json"));
    }

    #[test]
    fn repl_help_includes_shared_commands_and_exit() {
        let help = render_repl_help();
        assert!(help.contains("REPL interactif"));
        assert!(help.contains("/help"));
        assert!(help.contains("/status"));
        assert!(help.contains("/model [modèle]"));
        assert!(help.contains("/permissions [prompt|read-only|workspace-write|danger-full-access|allow|deny|ask|remove <règle>]"));
        assert!(help.contains("/clear [--confirm]"));
        assert!(help.contains("/cost"));
        assert!(help.contains("/resume <chemin-session>"));
        assert!(help.contains("/config [env|hooks|model|plugins]"));
        assert!(help.contains("/memory"));
        assert!(help.contains("/init"));
        assert!(help.contains("/diff"));
        assert!(help.contains("/version"));
        assert!(help.contains("/export [fichier]"));
        assert!(help.contains("/session [list|switch <id>|rename <id> <titre>|delete <id>|clear]"));
        assert!(help.contains("/plugin"));
        assert!(help.contains("/plugins, /marketplace"));
        assert!(help.contains("/agents"));
        assert!(help.contains("/skills"));
        assert!(help.contains("/exit"));
        assert!(help.contains("Tab parcourt les commandes correspondantes"));
    }

    #[test]
    fn completion_candidates_include_repl_only_exit_commands() {
        let candidates = slash_command_completion_candidates();
        assert!(candidates.contains(&"/help".to_string()));
        assert!(candidates.contains(&"/vim".to_string()));
        assert!(candidates.contains(&"/exit".to_string()));
        assert!(candidates.contains(&"/quit".to_string()));
    }

    #[test]
    fn unknown_repl_command_suggestions_include_repl_shortcuts() {
        let rendered = render_unknown_repl_command("exi");
        assert!(rendered.contains("Commande slash inconnue"));
        assert!(rendered.contains("/exit"));
        assert!(rendered.contains("/help"));
    }

    #[test]
    fn resume_supported_command_list_matches_expected_surface() {
        let names = resume_supported_slash_commands()
            .into_iter()
            .map(|spec| spec.name)
            .collect::<Vec<_>>();
        assert_eq!(
            names,
            vec![
                "help",
                "status",
                "compact",
                "clear",
                "cost",
                "config",
                "memory",
                "init",
                "diff",
                "version",
                "add-dir",
                "install-github-action",
                "export",
                "copy",
                "rename",
                "output-style",
                "schedule",
                "agents",
                "skills",
            ]
        );
    }

    #[test]
    fn resume_report_uses_sectioned_layout() {
        let report = format_resume_report("session.json", 14, 6);
        assert!(report.contains("Session reprise"));
        assert!(report.contains("Fichier          session.json"));
        assert!(report.contains("Historique       14 messages · 6 tours"));
        assert!(report.contains("/status · /diff · /export"));
    }

    #[test]
    fn compact_report_uses_structured_output() {
        let compacted = format_compact_report(8, 5, false);
        assert!(compacted.contains("Compaction"));
        assert!(compacted.contains("Résultat         compactée"));
        assert!(compacted.contains("Messages résumés 8"));
        assert!(compacted.contains("/status pour voir"));
        let skipped = format_compact_report(0, 3, true);
        assert!(skipped.contains("Résultat         ignorée"));
    }

    #[test]
    fn cost_report_uses_sectioned_layout() {
        let report = format_cost_report(runtime::TokenUsage {
            input_tokens: 20,
            output_tokens: 8,
            cache_creation_input_tokens: 3,
            cache_read_input_tokens: 1,
        });
        assert!(report.contains("Coût"));
        assert!(report.contains("Jetons d'entrée  20"));
        assert!(report.contains("Jetons de sortie 8"));
        assert!(report.contains("Cache (écriture) 3"));
        assert!(report.contains("Cache (lecture)  1"));
        assert!(report.contains("Total            32"));
        assert!(report.contains("/compact"));
    }

    #[test]
    fn permissions_report_uses_sectioned_layout() {
        let rules = PermissionRules::from_strings(
            &["Bash(cargo test:*)".to_string()],
            &["Read(.env)".to_string()],
            &[],
        );
        let report = format_permissions_report("workspace-write", &rules);
        assert!(report.contains("Permissions"));
        assert!(report.contains("Mode actif       workspace-write"));
        assert!(
            report.contains("Effet            Les fichiers du projet se modifient sans demander")
        );
        assert!(report.contains("Modes"));
        assert!(report.contains("prompt             ○ dispo"));
        assert!(report.contains("workspace-write    ● actuel"));
        assert!(report.contains("allow            Bash(cargo test:*)"));
        assert!(report.contains("deny             Read(.env)"));
        assert!(report.contains("ask              -"));
    }

    #[test]
    fn expert_commands_build_role_prompts_and_require_topics() {
        let review = expert_command_prompt(ExpertCommand::Review, None).expect("review prompt");
        assert!(review.contains("git diff"));
        assert!(review.contains("severity"));
        let scoped = expert_command_prompt(ExpertCommand::Review, Some("src/api")).expect("scoped");
        assert!(scoped.contains("for src/api."));

        let fix =
            expert_command_prompt(ExpertCommand::Fix, Some("cargo test -p api")).expect("fix");
        assert!(fix.contains("`cargo test -p api`"));
        assert!(fix.contains("root cause"));
        assert!(fix.contains("user's language"));

        assert!(expert_command_prompt(ExpertCommand::Archi, None)
            .expect_err("topic required")
            .starts_with("Usage : /archi"));
        assert!(expert_command_prompt(ExpertCommand::Adr, Some("  ")).is_err());
        let adr =
            expert_command_prompt(ExpertCommand::Adr, Some("utiliser PostgreSQL")).expect("adr");
        assert!(adr.contains("docs/adr/"));
    }

    #[test]
    fn review_commands_name_their_scope_and_method() {
        let security =
            expert_command_prompt(ExpertCommand::SecurityReview, None).expect("security");
        assert!(security.starts_with(
            "/security-review - Perform a security review of the current uncommitted changes"
        ));
        assert!(security.contains("8/10") && security.contains("Do not report"));
        assert!(security.contains("user's language"));

        let multi = expert_command_prompt(ExpertCommand::CodeReview, Some("#42")).expect("multi");
        assert!(
            multi.contains("pull request #42 (`gh pr diff 42`)"),
            "{multi}"
        );
        assert!(multi.contains("subagent_type \"reviewer\"") && multi.contains("CONFIRMED"));
        let branch =
            expert_command_prompt(ExpertCommand::CodeReview, Some("main..HEAD")).expect("branche");
        assert!(branch.contains("review of main..HEAD."));

        let comments =
            expert_command_prompt(ExpertCommand::PrComments, Some("#7")).expect("comments");
        assert!(comments.contains("pull request #7") && comments.contains("`gh pr view 7 --json"));
        let current = expert_command_prompt(ExpertCommand::PrComments, None).expect("courante");
        assert!(current.contains("the pull request of the current branch"));
        assert!(current.contains("`gh pr view --json"));
    }

    #[test]
    fn models_subcommand_keeps_explicit_model_and_defers_default() {
        assert_eq!(
            parse_args(&["models".to_string(), "poolside".to_string()]).expect("models"),
            CliAction::Models {
                args: Some("poolside".to_string()),
                model: None
            }
        );
        assert_eq!(
            parse_args(&[
                "--model".to_string(),
                "qwen/qwen3-coder:free".to_string(),
                "/models".to_string()
            ])
            .expect("/models"),
            CliAction::Models {
                args: None,
                model: Some("qwen/qwen3-coder:free".to_string())
            }
        );
    }

    #[test]
    fn an_upstream_failure_that_reaches_the_user_comes_with_a_hint() {
        assert!(super::error_hint(
            "l'api a retourné 504 Gateway Timeout (504) : Upstream idle timeout exceeded"
        )
        .contains("a cessé de répondre"));
    }

    #[test]
    fn error_hints_cover_free_model_limits() {
        assert!(super::error_hint("l'api a retourné 429 Too Many Requests").contains("quota"));
        assert!(
            super::error_hint("No endpoints found matching your data policy")
                .contains("confidentialité")
        );
        assert!(super::error_hint("autre chose").contains("claw doctor"));
        // Chaque famille d'erreur du fournisseur dit quoi essayer.
        for (problem, advice) in [
            ("l'api a retourné 402 Payment Required : credits", "modèle gratuit"),
            ("l'api a retourné 403 Forbidden : not allowed", "refusé à votre clé"),
            ("l'api a retourné 404 Not Found : model x", "introuvable"),
            ("l'api a retourné 413 Payload Too Large : Request too large for model", "/compact"),
            (
                "l'api a retourné 400 Bad Request : The input token count exceeds the maximum number of tokens allowed",
                "/compact",
            ),
            ("l'api a retourné 400 Bad Request : invalid tool schema", "refusé la requête"),
            ("erreur http : connection refused", "injoignable"),
        ] {
            assert!(super::error_hint(problem).contains(advice), "{problem}");
        }
        // Un nombre du message n'est pas un code d'erreur.
        let context = "l'api a retourné 400 Bad Request : {\"error\":{\"message\":\"This endpoint's maximum context length is 131072 tokens. However, you requested about 140129 tokens (140129 of text input)\",\"code\":400}}";
        assert!(super::error_hint(context).contains("/compact"), "{context}");
        let context = context.replace("140129", "142900");
        assert!(
            super::error_hint(&context).contains("/compact"),
            "{context}"
        );
    }

    #[test]
    fn direct_expert_slash_commands_become_one_shot_prompts() {
        let action = parse_args(&["/review".to_string(), "src/api".to_string()])
            .expect("direct /review should parse");
        let CliAction::Prompt { prompt, .. } = action else {
            panic!("expected a prompt action, got {action:?}");
        };
        assert!(prompt.starts_with("/review - Act as a code reviewer for src/api."));
        assert!(parse_args(&["/archi".to_string()])
            .expect_err("topic required")
            .contains("Usage : /archi"));
    }

    #[test]
    fn parses_permission_answers_in_french_and_english() {
        assert_eq!(parse_permission_answer("o\n"), PermissionAnswer::Once);
        assert_eq!(parse_permission_answer("Yes"), PermissionAnswer::Once);
        assert_eq!(parse_permission_answer("t"), PermissionAnswer::Always);
        assert_eq!(parse_permission_answer("always"), PermissionAnswer::Always);
        assert_eq!(parse_permission_answer(""), PermissionAnswer::Deny);
        assert_eq!(parse_permission_answer("Non"), PermissionAnswer::Deny);
        assert_eq!(
            parse_permission_answer("lance plutôt cargo check\n"),
            PermissionAnswer::Feedback("lance plutôt cargo check".to_string())
        );
    }

    #[test]
    fn describes_permission_targets_by_tool() {
        assert!(
            describe_permission_target("bash", r#"{"command":"cargo test"}"#)
                .ends_with("cargo test")
        );
        assert!(
            describe_permission_target("edit_file", r#"{"path":"src/lib.rs"}"#)
                .ends_with("src/lib.rs")
        );
        assert!(describe_permission_target("Unknown", "{}").starts_with("Entrée"));
    }

    #[test]
    fn persists_allow_rules_without_duplicates_or_clobbering() {
        let root = std::env::temp_dir().join(format!(
            "claw-persist-rule-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        fs::create_dir_all(root.join(".claw")).expect("claw dir");
        fs::write(
            root.join(".claw").join("settings.local.json"),
            r#"{"model":"qwen/qwen3-coder","permissions":{"deny":["Read(.env)"]}}"#,
        )
        .expect("seed settings");

        persist_allow_rule(&root, "Bash(cargo test:*)").expect("first write");
        let path = persist_allow_rule(&root, "Bash(cargo test:*)").expect("second write");

        let saved: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(path).expect("read back")).expect("json");
        assert_eq!(saved["model"], "qwen/qwen3-coder");
        assert_eq!(saved["permissions"]["deny"], json!(["Read(.env)"]));
        assert_eq!(saved["permissions"]["allow"], json!(["Bash(cargo test:*)"]));

        // `/permissions ask|remove <règle>`.
        let (_, added) =
            super::edit_permission_rule(&root, Some("ask"), "Bash(git push:*)").expect("ask");
        assert!(added);
        let (path, removed) =
            super::edit_permission_rule(&root, None, "Read(.env)").expect("retrait");
        assert!(removed);
        let saved: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).expect("relu")).expect("json");
        assert_eq!(saved["permissions"]["ask"], json!(["Bash(git push:*)"]));
        assert_eq!(saved["permissions"]["deny"], json!([]));
        assert!(
            !super::edit_permission_rule(&root, None, "Read(.env)")
                .expect("absente")
                .1
        );
        // Retrait dans un fichier où des listes manquent : aucune liste `null` créée.
        fs::write(
            root.join(".claw").join("settings.local.json"),
            r#"{"permissions":{"allow":["Bash(git status)"]}}"#,
        )
        .expect("réglages partiels");
        let (path, removed) =
            super::edit_permission_rule(&root, None, "Bash(git status)").expect("retrait");
        assert!(removed);
        let saved: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).expect("relu")).expect("json");
        assert_eq!(saved["permissions"], json!({ "allow": [] }));
        assert_eq!(
            super::parse_rule_command("deny  Bash(rm:*)"),
            Some((Some("deny"), "Bash(rm:*)"))
        );
        assert_eq!(
            super::parse_rule_command("remove WebFetch"),
            Some((None, "WebFetch"))
        );
        assert_eq!(super::parse_rule_command("workspace-write"), None);
        assert_eq!(super::parse_rule_command("allow"), None);

        fs::write(root.join(".claw").join("settings.local.json"), "{ broken").expect("corrupt");
        assert!(persist_allow_rule(&root, "Bash(ls:*)").is_err());

        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn permissions_switch_report_is_structured() {
        let report = format_permissions_switch_report("read-only", "workspace-write");
        assert!(report.contains("Permissions changées"));
        assert!(report.contains("Mode précédent   read-only"));
        assert!(report.contains("Mode actif       workspace-write"));
        assert!(report.contains("S'applique à     tous les prochains appels d'outils"));
    }

    #[test]
    fn init_help_mentions_direct_subcommand() {
        let mut help = Vec::new();
        print_help_to(&mut help).expect("help should render");
        let help = String::from_utf8(help).expect("help should be utf8");
        assert!(help.contains("claw init"));
        assert!(help.contains("claw agents"));
        assert!(help.contains("claw skills"));
        assert!(help.contains("claw serve"));
        assert!(help.contains("claw models"));
        assert!(help.contains("claw skills"));
    }

    #[test]
    fn model_report_uses_sectioned_layout() {
        let report = format_model_report("sonnet", 12, 4);
        assert!(report.contains("Modèle"));
        assert!(report.contains("Actuel           sonnet"));
        assert!(report.contains("Session          12 messages · 4 tours"));
        assert!(report.contains("Alias"));
        assert!(report.contains("/model <nom>     Change de modèle pour cette session"));
    }

    #[test]
    fn model_switch_report_preserves_context_summary() {
        let report = format_model_switch_report("sonnet", "opus", 9);
        assert!(report.contains("Modèle changé"));
        assert!(report.contains("Précédent        sonnet"));
        assert!(report.contains("Actuel           opus"));
        assert!(report.contains("Conservés        9 messages"));
    }

    #[test]
    fn status_line_reports_model_and_token_totals() {
        let status = format_status_report(
            "sonnet",
            StatusUsage {
                message_count: 7,
                turns: 3,
                latest: runtime::TokenUsage {
                    input_tokens: 5,
                    output_tokens: 4,
                    cache_creation_input_tokens: 1,
                    cache_read_input_tokens: 0,
                },
                cumulative: runtime::TokenUsage {
                    input_tokens: 20,
                    output_tokens: 8,
                    cache_creation_input_tokens: 2,
                    cache_read_input_tokens: 1,
                },
                estimated_tokens: 128,
            },
            "workspace-write",
            &super::StatusContext {
                cwd: PathBuf::from("/tmp/project"),
                session_path: Some(PathBuf::from("session.json")),
                loaded_config_files: 2,
                discovered_config_files: 3,
                memory_file_count: 4,
                project_root: Some(PathBuf::from("/tmp")),
                git_branch: Some("main".to_string()),
            },
        );
        assert!(status.contains("Session"));
        assert!(status.contains("Modèle           sonnet"));
        assert!(status.contains("Permissions      workspace-write"));
        assert!(status.contains("Activité         7 messages · 3 tours"));
        assert!(status.contains("Jetons           estimés 128 · dernier tour 10 · total 31"));
        assert!(status.contains("Dossier          /tmp/project"));
        assert!(status.contains("Racine du projet /tmp"));
        assert!(status.contains("Branche git      main"));
        assert!(status.contains("Fichier session  session.json"));
        assert!(status.contains("Configuration    2/3 fichiers chargés"));
        assert!(status.contains("Instructions     4 fichier(s) CLAW.md"));
        assert!(status.contains("/session list"));
    }

    #[test]
    fn config_report_supports_section_views() {
        let report = render_config_report(Some("env")).expect("config report should render");
        assert!(report.contains("Section fusionnée : env"));
        let plugins_report =
            render_config_report(Some("plugins")).expect("plugins config report should render");
        assert!(plugins_report.contains("Section fusionnée : plugins"));
    }

    #[test]
    fn memory_report_uses_sectioned_layout() {
        let report = render_memory_report().expect("memory report should render");
        assert!(report.contains("Mémoire"));
        assert!(report.contains("Dossier"));
        assert!(report.contains("Fichiers trouvés"));
        assert!(report.contains("Fichiers"));
    }

    #[test]
    fn config_report_uses_sectioned_layout() {
        let report = render_config_report(None).expect("config report should render");
        assert!(report.contains("Configuration"));
        assert!(report.contains("Fichiers recherchés"));
        assert!(report.contains("JSON fusionné"));
    }

    #[test]
    fn parses_git_status_metadata() {
        let (root, branch) = parse_git_status_metadata(Some(
            "## rcc/cli...origin/rcc/cli
 M src/main.rs",
        ));
        assert_eq!(branch.as_deref(), Some("rcc/cli"));
        let _ = root;
    }

    #[test]
    fn status_context_reads_real_workspace_metadata() {
        let context = status_context(None).expect("status context should load");
        assert!(context.cwd.is_absolute());
        // 5 fichiers Claw, plus 4 de Claude Code sauf si CLAW_NO_CLAUDE_CONFIG=1.
        assert!(matches!(context.discovered_config_files, 5 | 9));
        assert!(context.loaded_config_files <= context.discovered_config_files);
    }

    #[test]
    fn normalizes_supported_permission_modes() {
        assert_eq!(normalize_permission_mode("read-only"), Some("read-only"));
        assert_eq!(
            normalize_permission_mode("workspace-write"),
            Some("workspace-write")
        );
        assert_eq!(
            normalize_permission_mode("danger-full-access"),
            Some("danger-full-access")
        );
        assert_eq!(normalize_permission_mode("default"), Some("prompt"));
        assert_eq!(normalize_permission_mode("plan"), Some("read-only"));
        assert_eq!(
            normalize_permission_mode("acceptEdits"),
            Some("workspace-write")
        );
        assert_eq!(
            normalize_permission_mode("bypassPermissions"),
            Some("danger-full-access")
        );
        // `dontAsk` refuse ce qui n'est pas autorisé d'avance : jamais l'accès complet.
        assert_eq!(normalize_permission_mode("dontAsk"), Some("dont-ask"));
        assert_eq!(
            super::permission_mode_from_label("dont-ask"),
            PermissionMode::DontAsk
        );
        assert_eq!(normalize_permission_mode("unknown"), None);
    }

    #[test]
    fn clear_command_requires_explicit_confirmation_flag() {
        assert_eq!(
            SlashCommand::parse("/clear"),
            Some(SlashCommand::Clear { confirm: false })
        );
        assert_eq!(
            SlashCommand::parse("/clear --confirm"),
            Some(SlashCommand::Clear { confirm: true })
        );
    }

    #[test]
    fn parses_resume_and_config_slash_commands() {
        assert_eq!(
            SlashCommand::parse("/resume saved-session.json"),
            Some(SlashCommand::Resume {
                session_path: Some("saved-session.json".to_string())
            })
        );
        assert_eq!(
            SlashCommand::parse("/clear --confirm"),
            Some(SlashCommand::Clear { confirm: true })
        );
        assert_eq!(
            SlashCommand::parse("/config"),
            Some(SlashCommand::Config { section: None })
        );
        assert_eq!(
            SlashCommand::parse("/config env"),
            Some(SlashCommand::Config {
                section: Some("env".to_string())
            })
        );
        assert_eq!(SlashCommand::parse("/memory"), Some(SlashCommand::Memory));
        assert_eq!(SlashCommand::parse("/init"), Some(SlashCommand::Init));
    }

    #[test]
    fn init_template_mentions_detected_rust_workspace() {
        let rendered = crate::init::render_init_claw_md(std::path::Path::new("."));
        assert!(rendered.contains("# CLAW.md"));
        assert!(rendered.contains("cargo clippy --workspace --all-targets -- -D warnings"));
    }

    #[test]
    fn converts_tool_roundtrip_messages() {
        let messages = vec![
            ConversationMessage::user_text("hello"),
            ConversationMessage::assistant(vec![ContentBlock::ToolUse {
                id: "tool-1".to_string(),
                name: "bash".to_string(),
                input: "{\"command\":\"pwd\"}".to_string(),
            }]),
            ConversationMessage {
                role: MessageRole::Tool,
                blocks: vec![ContentBlock::ToolResult {
                    tool_use_id: "tool-1".to_string(),
                    tool_name: "bash".to_string(),
                    output: "ok".to_string(),
                    is_error: false,
                }],
                usage: None,
            },
        ];

        let converted = super::convert_messages(&messages);
        assert_eq!(converted.len(), 3);
        assert_eq!(converted[1].role, "assistant");
        assert_eq!(converted[2].role, "user");
    }
    #[test]
    fn repl_help_mentions_history_completion_and_multiline() {
        let help = render_repl_help();
        assert!(help.contains("Haut/Bas"));
        assert!(help.contains("Tab parcourt"));
        assert!(help.contains("Maj+Entrée ou Ctrl+J"));
    }

    #[test]
    fn ultraplan_progress_lines_include_phase_step_and_elapsed_status() {
        let snapshot = InternalPromptProgressState {
            command_label: "Ultraplan",
            task_label: "ship plugin progress".to_string(),
            step: 3,
            phase: "outil read_file".to_string(),
            detail: Some("reading rust/crates/claw-cli/src/main.rs".to_string()),
            saw_final_text: false,
        };

        let started = format_internal_prompt_progress_line(
            InternalPromptProgressEvent::Started,
            &snapshot,
            Duration::from_secs(0),
            None,
        );
        let heartbeat = format_internal_prompt_progress_line(
            InternalPromptProgressEvent::Heartbeat,
            &snapshot,
            Duration::from_secs(9),
            None,
        );
        let completed = format_internal_prompt_progress_line(
            InternalPromptProgressEvent::Complete,
            &snapshot,
            Duration::from_secs(12),
            None,
        );
        let failed = format_internal_prompt_progress_line(
            InternalPromptProgressEvent::Failed,
            &snapshot,
            Duration::from_secs(12),
            Some("network timeout"),
        );

        assert!(started.contains("planification démarrée"));
        assert!(started.contains("étape 3"));
        assert!(heartbeat.contains("toujours en cours"));
        assert!(heartbeat.contains("9 s"));
        assert!(heartbeat.contains("phase outil read_file"));
        assert!(completed.contains("terminé"));
        assert!(completed.contains("3 étape(s)"));
        assert!(failed.contains("échec"));
        assert!(failed.contains("network timeout"));
    }

    #[test]
    fn describe_tool_progress_summarizes_known_tools() {
        assert_eq!(
            describe_tool_progress("read_file", r#"{"path":"src/main.rs"}"#),
            "lecture de src/main.rs"
        );
        assert!(
            describe_tool_progress("bash", r#"{"command":"cargo test -p claw-cli"}"#)
                .contains("cargo test -p claw-cli")
        );
        assert_eq!(
            describe_tool_progress("grep_search", r#"{"pattern":"ultraplan","path":"rust"}"#),
            "recherche `ultraplan` dans rust"
        );
    }

    #[test]
    fn push_output_block_renders_markdown_text() {
        let mut out = Vec::new();
        let mut events = Vec::new();
        let mut pending_tools = PendingTools::new();

        push_output_block(
            OutputContentBlock::Text {
                text: "# Heading".to_string(),
            },
            0,
            &mut out,
            &mut events,
            &mut pending_tools,
            false,
        )
        .expect("text block should render");

        let rendered = String::from_utf8(out).expect("utf8");
        assert!(rendered.contains("Heading"));
        assert!(rendered.contains('\u{1b}'));
    }

    #[test]
    fn push_output_block_skips_empty_object_prefix_for_tool_streams() {
        let mut out = Vec::new();
        let mut events = Vec::new();
        let mut pending_tools = PendingTools::new();

        push_output_block(
            OutputContentBlock::ToolUse {
                id: "tool-1".to_string(),
                name: "read_file".to_string(),
                input: json!({}),
            },
            1,
            &mut out,
            &mut events,
            &mut pending_tools,
            true,
        )
        .expect("tool block should accumulate");

        assert!(events.is_empty());
        assert_eq!(
            pending_tools.get(&1),
            Some(&("tool-1".to_string(), "read_file".to_string(), String::new()))
        );
    }

    #[test]
    fn push_output_block_tracks_parallel_tool_calls_separately() {
        let mut out = Vec::new();
        let mut events = Vec::new();
        let mut pending_tools = PendingTools::new();

        for (index, id, name) in [(1, "call_1", "read_file"), (2, "call_2", "grep_search")] {
            push_output_block(
                OutputContentBlock::ToolUse {
                    id: id.to_string(),
                    name: name.to_string(),
                    input: json!({}),
                },
                index,
                &mut out,
                &mut events,
                &mut pending_tools,
                true,
            )
            .expect("tool block should accumulate");
        }

        assert_eq!(pending_tools.len(), 2);
        assert_eq!(pending_tools[&1].1, "read_file");
        assert_eq!(pending_tools[&2].1, "grep_search");
    }

    #[test]
    fn response_to_events_preserves_empty_object_json_input_outside_streaming() {
        let mut out = Vec::new();
        let events = response_to_events(
            MessageResponse {
                id: "msg-1".to_string(),
                kind: "message".to_string(),
                model: "claude-opus-4-6".to_string(),
                role: "assistant".to_string(),
                content: vec![OutputContentBlock::ToolUse {
                    id: "tool-1".to_string(),
                    name: "read_file".to_string(),
                    input: json!({}),
                }],
                stop_reason: Some("tool_use".to_string()),
                stop_sequence: None,
                usage: Usage {
                    input_tokens: 1,
                    output_tokens: 1,
                    cache_creation_input_tokens: 0,
                    cache_read_input_tokens: 0,
                },
                request_id: None,
            },
            &mut out,
        )
        .expect("response conversion should succeed");

        assert!(matches!(
            &events[0],
            AssistantEvent::ToolUse { name, input, .. }
                if name == "read_file" && input == "{}"
        ));
    }

    #[test]
    fn response_to_events_preserves_non_empty_json_input_outside_streaming() {
        let mut out = Vec::new();
        let events = response_to_events(
            MessageResponse {
                id: "msg-2".to_string(),
                kind: "message".to_string(),
                model: "claude-opus-4-6".to_string(),
                role: "assistant".to_string(),
                content: vec![OutputContentBlock::ToolUse {
                    id: "tool-2".to_string(),
                    name: "read_file".to_string(),
                    input: json!({ "path": "rust/Cargo.toml" }),
                }],
                stop_reason: Some("tool_use".to_string()),
                stop_sequence: None,
                usage: Usage {
                    input_tokens: 1,
                    output_tokens: 1,
                    cache_creation_input_tokens: 0,
                    cache_read_input_tokens: 0,
                },
                request_id: None,
            },
            &mut out,
        )
        .expect("response conversion should succeed");

        assert!(matches!(
            &events[0],
            AssistantEvent::ToolUse { name, input, .. }
                if name == "read_file" && input == "{\"path\":\"rust/Cargo.toml\"}"
        ));
    }

    #[test]
    fn response_to_events_keeps_signed_thinking_without_printing_it() {
        let mut out = Vec::new();
        let events = response_to_events(
            MessageResponse {
                id: "msg-3".to_string(),
                kind: "message".to_string(),
                model: "claude-opus-4-6".to_string(),
                role: "assistant".to_string(),
                content: vec![
                    OutputContentBlock::Thinking {
                        thinking: "step 1".to_string(),
                        signature: Some("sig_123".to_string()),
                    },
                    OutputContentBlock::Text {
                        text: "Final answer".to_string(),
                    },
                ],
                stop_reason: Some("end_turn".to_string()),
                stop_sequence: None,
                usage: Usage {
                    input_tokens: 1,
                    output_tokens: 1,
                    cache_creation_input_tokens: 0,
                    cache_read_input_tokens: 0,
                },
                request_id: None,
            },
            &mut out,
        )
        .expect("response conversion should succeed");

        // Le raisonnement signé est conservé (à renvoyer à l'API), pas affiché.
        assert!(matches!(
            &events[0],
            AssistantEvent::Thinking { thinking, signature, redacted: None }
                if thinking == "step 1" && signature == "sig_123"
        ));
        assert!(matches!(
            &events[1],
            AssistantEvent::TextDelta(text) if text == "Final answer"
        ));
        assert!(!String::from_utf8(out).expect("utf8").contains("step 1"));
    }

    #[test]
    fn only_plain_session_ids_can_name_a_file_to_delete() {
        assert!(super::is_session_id("session-1791500000000"));
        assert!(super::is_session_id("ma_session-2"));
        for unsafe_id in [
            "",
            "../secret",
            "a/b",
            "a\\b",
            "session 1",
            "session.json",
            "C:x",
        ] {
            assert!(!super::is_session_id(unsafe_id), "{unsafe_id}");
        }
        assert!(!super::is_session_id(&"a".repeat(81)));
        // Un identifiant refusé est rejeté avant tout accès au disque.
        assert!(super::delete_managed_session("../secret").is_err());
        assert!(super::rename_managed_session("../secret", "x").is_err());
    }

    #[test]
    fn the_team_command_turns_the_mode_on_and_off() {
        struct Off;
        impl Drop for Off {
            fn drop(&mut self) {
                runtime::team::set_team(false);
            }
        }
        let _off = Off;
        let on = super::apply_team_command(None, "vendor/base:free");
        assert!(
            on.starts_with("Mode équipe\n  État             activé"),
            "{on}"
        );
        assert!(
            on.contains("vendor/base:free (modèle de la session)"),
            "{on}"
        );
        assert!(runtime::team::team());
        let off = super::apply_team_command(Some("off"), "vendor/base:free");
        assert_eq!(
            off,
            "Mode équipe désactivé : vendor/base:free fait tout le travail."
        );
        assert!(!runtime::team::team());
        assert!(super::apply_team_command(Some("peut-être"), "x").contains("/equipe off"));
    }

    #[test]
    fn think_ultracode_turns_the_workflow_on_and_off_for_the_session() {
        struct Restore(Option<api::ReasoningEffort>);
        impl Drop for Restore {
            fn drop(&mut self) {
                runtime::workflow::set_ultracode(false);
                api::set_reasoning_effort(self.0);
            }
        }
        let _restore = Restore(api::reasoning_effort());
        let on = super::apply_think_command(Some("ultracode"));
        assert!(on.starts_with("Ultracode activé pour la session"), "{on}");
        assert!(runtime::workflow::ultracode());
        assert_eq!(api::reasoning_effort(), Some(api::ReasoningEffort::Max));
        assert!(super::apply_think_command(None).contains("Ultracode        activé"));
        let off = super::apply_think_command(Some("ULTRACODE off"));
        assert!(off.starts_with("Ultracode désactivé"), "{off}");
        assert!(!runtime::workflow::ultracode());
        assert!(super::apply_think_command(Some("ultracode peut-être"))
            .contains("/think ultracode off"));
        // « ultrathink » reste un niveau de raisonnement, pas le déroulé.
        assert_eq!(
            super::apply_think_command(Some("ultrathink")),
            "Raisonnement : max (le plus haut niveau que le modèle accepte)"
        );
        assert!(!runtime::workflow::ultracode());
    }

    #[test]
    fn goal_checks_read_met_or_not_met_and_the_reason() {
        use super::{parse_goal_verdict, GoalVerdict};
        assert_eq!(
            parse_goal_verdict("MET\nLes 42 tests passent."),
            GoalVerdict::Met("Les 42 tests passent.".to_string())
        );
        assert_eq!(
            parse_goal_verdict("**NOT MET**\n- Deux tests échouent encore"),
            GoalVerdict::NotMet("Deux tests échouent encore".to_string())
        );
        assert_eq!(
            parse_goal_verdict("NOT_MET: le build casse"),
            GoalVerdict::NotMet("le build casse".to_string())
        );
        assert_eq!(
            parse_goal_verdict("Verdict: MET\nTout est vert"),
            GoalVerdict::Met("Tout est vert".to_string())
        );
        // Sans verdict lisible, l'objectif n'est pas tenu pour atteint.
        assert!(matches!(
            parse_goal_verdict("Je pense que oui, à peu près."),
            GoalVerdict::NotMet(_)
        ));
        assert!(matches!(parse_goal_verdict(""), GoalVerdict::NotMet(_)));
    }

    #[test]
    fn goal_status_and_prompts_name_the_condition() {
        let mut goal = super::GoalState::new("  tous les tests passent ");
        assert_eq!(goal.condition, "tous les tests passent");
        assert!(super::render_goal_status(None).contains("Aucun objectif"));
        goal.paused = true;
        goal.last_reason = Some("un test échoue".to_string());
        let status = super::render_goal_status(Some(&goal));
        assert!(
            status.contains("en pause") && status.contains("un test échoue"),
            "{status}"
        );
        assert!(super::goal_check_prompt(&goal.condition).contains("NOT MET"));
        assert!(
            super::goal_continue_prompt(&goal.condition, Some("un test échoue"))
                .contains("not met yet: un test échoue")
        );
        assert_eq!(super::fold_command_word(" Arrêter "), "arreter");
    }

    #[test]
    fn loop_takes_an_optional_interval_before_the_request() {
        use std::time::Duration;
        assert_eq!(
            super::parse_loop_args(Some("5m vérifie le déploiement")),
            Ok((
                Duration::from_secs(300),
                "vérifie le déploiement".to_string()
            ))
        );
        assert_eq!(
            super::parse_loop_args(Some("vérifie la CI")),
            Ok((Duration::from_secs(600), "vérifie la CI".to_string()))
        );
        assert_eq!(
            super::parse_loop_args(Some("2h /rapport")),
            Ok((Duration::from_secs(7200), "/rapport".to_string()))
        );
        assert!(super::parse_loop_args(Some("10s trop vite"))
            .unwrap_err()
            .contains("30s au moins"));
        assert!(super::parse_loop_args(Some("5m")).is_err());
        assert!(super::parse_loop_args(None).is_err());
        assert_eq!(super::format_interval(Duration::from_secs(300)), "5 min");
        assert_eq!(super::format_interval(Duration::from_secs(3600)), "1 h");
        assert_eq!(super::format_interval(Duration::from_secs(45)), "45 s");
        assert_eq!(super::loop_prompt("vérifie", 1), "vérifie");
        assert!(super::loop_prompt("vérifie", 2).contains("pass 2"));
    }

    #[test]
    fn copy_finds_the_replies_of_claw_from_the_end() {
        use runtime::{ContentBlock, ConversationMessage, MessageRole, Session};
        let message = |role: MessageRole, text: &str| ConversationMessage {
            role,
            blocks: vec![ContentBlock::Text {
                text: text.to_string(),
            }],
            usage: None,
        };
        let mut session = Session::new();
        assert!(super::copy_assistant_reply(&session, None).contains("Aucune réponse"));
        session.messages = vec![
            message(MessageRole::User, "bonjour"),
            message(MessageRole::Assistant, "première réponse"),
            message(MessageRole::User, "encore"),
            message(MessageRole::Assistant, "   "),
            message(MessageRole::Assistant, "seconde réponse"),
        ];
        assert_eq!(
            super::assistant_replies(&session),
            vec![
                "première réponse".to_string(),
                "seconde réponse".to_string()
            ]
        );
        assert!(super::copy_assistant_reply(&session, Some("3")).contains("Seulement 2"));
        assert!(super::copy_assistant_reply(&session, Some("zéro")).starts_with("Usage"));
    }

    #[test]
    fn session_titles_are_cleaned_and_text_is_found_ignoring_case() {
        assert_eq!(
            super::clean_session_title("  Cache   des\ttarifs \n v2 "),
            "Cache des tarifs v2"
        );
        assert_eq!(
            super::clean_session_title(&"é".repeat(120)).chars().count(),
            80
        );

        let text = "Première ligne.\nLe module de FACTURATION dépend de tarifs.py et de devise.py, puis du rapport mensuel.";
        let snippet = super::search_snippet(text, "facturation").expect("trouvé");
        assert!(
            snippet.contains("module de FACTURATION dépend"),
            "{snippet}"
        );
        assert!(
            !snippet.contains('\n') && snippet.ends_with('…'),
            "{snippet}"
        );
        assert_eq!(super::search_snippet(text, "introuvable"), None);
        assert_eq!(super::search_snippet(text, ""), None);
        assert_eq!(
            super::search_snippet("Élan", "élan").as_deref(),
            Some("Élan")
        );
    }
}
