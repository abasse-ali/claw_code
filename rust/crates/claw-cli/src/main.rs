mod init;
mod input;
mod models;
mod render;
mod web;

use std::collections::BTreeSet;
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
use render::{MarkdownStreamState, Spinner, TerminalRenderer};
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
    if let Some(model) = env::current_dir()
        .ok()
        .and_then(|cwd| ConfigLoader::default_for(&cwd).load().ok())
        .and_then(|config| config.model().map(resolve_model_alias))
    {
        return model;
    }
    let has_openrouter_key = env::var("OPENROUTER_API_KEY").is_ok_and(|key| !key.trim().is_empty());
    if has_openrouter_key && !api::has_anthropic_credentials() {
        return models::best_free_coding_model()
            .unwrap_or_else(|| models::FREE_ROUTER_MODEL.to_string());
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
    let result = run();
    tools::shutdown_mcp();
    if let Err(error) = result {
        eprintln!("{}", render_cli_error(&error.to_string()));
        std::process::exit(1);
    }
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
                    eprintln!("\n⏹ Interruption du tour… (Ctrl+C à nouveau pour quitter claw)");
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

/// Conseil adapté aux erreurs fréquentes avec `OpenRouter` et les modèles gratuits.
fn error_hint(problem: &str) -> &'static str {
    let lower = problem.to_ascii_lowercase();
    if lower.contains("429") || lower.contains("rate limit") {
        "limite de requêtes atteinte : les modèles gratuits sont limités par minute et par jour (quota : /models) ; réessayez plus tard ou changez de modèle (/model)"
    } else if lower.contains("data policy") {
        "autorisez les modèles gratuits dans vos paramètres de confidentialité OpenRouter (Settings > Privacy)"
    } else if lower.contains("no endpoints found") || lower.contains("tool use") {
        "ce modèle ne prend peut-être pas en charge l'appel d'outils : choisissez-en un avec /models"
    } else if lower.contains("401") || lower.contains("user not found") {
        "vérifiez la clé OPENROUTER_API_KEY (ou ANTHROPIC_API_KEY pour un modèle Claude direct)"
    } else if lower.contains("identifiants") && lower.contains("manquants") {
        "aucune clé API trouvée : définissez OPENROUTER_API_KEY (clé gratuite sur https://openrouter.ai/keys) puis relancez claw"
    } else {
        "claw --help"
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().skip(1).collect();
    match parse_args(&args)? {
        CliAction::DumpManifests => dump_manifests(),
        CliAction::BootstrapPlan => print_bootstrap_plan(),
        CliAction::Agents { args } => LiveCli::print_agents(args.as_deref())?,
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
            install_interrupt_handler();
            let mut cli = LiveCli::new(model, true, allowed_tools, permission_mode)?;
            cli.session_hook_start("startup");
            let result = cli.run_turn_with_output(&prompt, output_format);
            cli.session_hook_end("exit");
            result?;
        }
        CliAction::Login => run_login()?,
        CliAction::Logout => run_logout()?,
        CliAction::Init => run_init()?,
        CliAction::Repl {
            model,
            allowed_tools,
            permission_mode,
        } => {
            install_interrupt_handler();
            run_repl(model, allowed_tools, permission_mode)?;
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
    Skills {
        args: Option<String>,
    },
    Models {
        args: Option<String>,
        model: Option<String>,
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
}

impl CliOutputFormat {
    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "text" => Ok(Self::Text),
            "json" => Ok(Self::Json),
            other => Err(format!(
                "valeur non prise en charge pour --output-format : {other} (attendu : text ou json)"
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
            "-p" => {
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

    let allowed_tools = normalize_allowed_tools(&allowed_tool_values)?;

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
        return parse_resume_args(&rest[1..]);
    }

    match rest[0].as_str() {
        "dump-manifests" => Ok(CliAction::DumpManifests),
        "bootstrap-plan" => Ok(CliAction::BootstrapPlan),
        "agents" => Ok(CliAction::Agents {
            args: join_optional_args(&rest[1..]),
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
        "system-prompt" => parse_system_prompt_args(&rest[1..]),
        "login" => Ok(CliAction::Login),
        "logout" => Ok(CliAction::Logout),
        "init" => Ok(CliAction::Init),
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
                    Some(prompt) => Ok(CliAction::Prompt {
                        prompt: prompt?,
                        model: model.unwrap_or_else(default_model),
                        output_format,
                        allowed_tools,
                        permission_mode,
                    }),
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

fn resolve_model_alias(model: &str) -> String {
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
                "mode de permission non pris en charge « {value} ». Utilisez prompt, read-only, workspace-write ou danger-full-access."
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
        })
}

fn filter_tool_specs(
    tool_registry: &GlobalToolRegistry,
    allowed_tools: Option<&AllowedToolSet>,
) -> Vec<ToolDefinition> {
    tool_registry.definitions(allowed_tools)
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
    match load_system_prompt(cwd, date, env::consts::OS, "unknown") {
        Ok(sections) => println!("{}", sections.join("\n\n")),
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
            "Édite sans demander, demande avant les commandes",
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
        "danger-full-access" => "Tous les outils s'exécutent sans demander",
        _ => "Mode de permission inconnu",
    };

    let render_rules = |rules: &[runtime::PermissionRule]| {
        if rules.is_empty() {
            "—".to_string()
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
  /permissions <mode>   Change de mode (alias : default, plan, acceptEdits, bypassPermissions)
  Répondre « toujours » à une demande enregistre la règle dans .claw/settings.local.json",
        render_rules(&rules.allow),
        render_rules(&rules.deny),
        render_rules(&rules.ask),
    )
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
  Raison           la session est sous le seuil de compaction
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
        SlashCommand::Compact => {
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
        SlashCommand::Bughunter { .. }
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
        | SlashCommand::Models { .. }
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

fn run_repl(
    model: String,
    allowed_tools: Option<AllowedToolSet>,
    permission_mode: PermissionMode,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut cli = LiveCli::new(model, true, allowed_tools, permission_mode)?;
    let mut editor = input::LineEditor::new("> ", slash_command_completion_candidates());
    println!("{}", cli.startup_banner());
    cli.session_hook_start("startup");

    loop {
        match editor.read_line()? {
            input::ReadOutcome::Submit(input) => {
                let trimmed = input.trim();
                if trimmed.is_empty() {
                    continue;
                }
                if matches!(trimmed, "/exit" | "/quit") {
                    cli.persist_session()?;
                    cli.session_hook_end("exit");
                    break;
                }
                if let Some(command) = SlashCommand::parse(trimmed) {
                    if matches!(command, SlashCommand::Unknown(_)) {
                        if let Some(prompt) = custom_command_prompt(trimmed) {
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
                    if cli.handle_repl_command(command)? {
                        cli.persist_session()?;
                    }
                    continue;
                }
                editor.push_history(&input);
                // Une requête en échec (quota gratuit, réseau…) ne doit pas fermer la session.
                if let Err(error) = cli.run_turn(&input) {
                    eprintln!("{}", render_cli_error(&error.to_string()));
                }
            }
            input::ReadOutcome::Cancel => {}
            input::ReadOutcome::Exit => {
                cli.persist_session()?;
                cli.session_hook_end("exit");
                break;
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
    model: String,
    allowed_tools: Option<AllowedToolSet>,
    permission_mode: PermissionMode,
    system_prompt: Vec<String>,
    runtime: ConversationRuntime<DefaultRuntimeClient, CliToolExecutor>,
    session: SessionHandle,
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
            model,
            allowed_tools,
            permission_mode,
            system_prompt,
            runtime,
            session,
        };
        cli.persist_session()?;
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

    fn startup_banner(&self) -> String {
        let color = io::stdout().is_terminal();
        let cwd = env::current_dir().ok();
        let cwd_display = cwd.as_ref().map_or_else(
            || "<inconnu>".to_string(),
            |path| path.display().to_string(),
        );
        let workspace_name = cwd
            .as_ref()
            .and_then(|path| path.file_name())
            .and_then(|name| name.to_str())
            .unwrap_or("workspace");
        let git_branch = status_context(Some(&self.session.path))
            .ok()
            .and_then(|context| context.git_branch);
        let workspace_summary = git_branch.as_deref().map_or_else(
            || workspace_name.to_string(),
            |branch| format!("{workspace_name} · {branch}"),
        );
        let has_claw_md = cwd
            .as_ref()
            .is_some_and(|path| path.join("CLAW.md").is_file());
        let mut lines = vec![
            format!(
                "{} {}",
                if color {
                    "\x1b[1;38;5;45m🦞 Claw Code\x1b[0m"
                } else {
                    "Claw Code"
                },
                if color {
                    "\x1b[2m· prêt\x1b[0m"
                } else {
                    "· prêt"
                }
            ),
            format!("  Projet           {workspace_summary}"),
            format!("  Dossier          {cwd_display}"),
            format!("  Modèle           {}", self.model),
            format!("  Permissions      {}", self.permission_mode.as_str()),
            format!("  Session          {}", self.session.id),
            format!(
                "  Pour commencer   {}",
                if has_claw_md {
                    "décrivez une tâche · /help · /status"
                } else {
                    "/init · /help · /status"
                }
            ),
            "  Édition          Tab complète les commandes · /vim mode modal · Ctrl+C interrompt un tour"
                .to_string(),
            "  Multiligne       Maj+Entrée ou Ctrl+J insère un saut de ligne".to_string(),
        ];
        if !has_claw_md {
            lines.push(
                "  Premier lancement /init crée CLAW.md, .claw.json et les fichiers de session"
                    .to_string(),
            );
        }
        lines.join("\n")
    }

    fn run_turn(&mut self, input: &str) -> Result<(), Box<dyn std::error::Error>> {
        let mut spinner = Spinner::new();
        let mut stdout = io::stdout();
        spinner.tick(
            "🦀 Réflexion…",
            TerminalRenderer::new().color_theme(),
            &mut stdout,
        )?;
        let mut permission_prompter = CliPermissionPrompter::new(self.permission_mode);
        let result = self.runtime.run_turn(input, Some(&mut permission_prompter));
        match result {
            Ok(_) => {
                spinner.finish(
                    "✨ Terminé",
                    TerminalRenderer::new().color_theme(),
                    &mut stdout,
                )?;
                println!();
                self.persist_session()?;
                Ok(())
            }
            Err(error) if runtime::interrupt::is_interruption(&error.to_string()) => {
                spinner.fail(
                    "⏹ Tour interrompu",
                    TerminalRenderer::new().color_theme(),
                    &mut stdout,
                )?;
                println!();
                self.persist_session()?;
                Ok(())
            }
            Err(error) => {
                spinner.fail(
                    "❌ Échec de la requête",
                    TerminalRenderer::new().color_theme(),
                    &mut stdout,
                )?;
                Err(Box::new(error))
            }
        }
    }

    fn run_turn_with_output(
        &mut self,
        input: &str,
        output_format: CliOutputFormat,
    ) -> Result<(), Box<dyn std::error::Error>> {
        match output_format {
            CliOutputFormat::Text => self.run_turn(input),
            CliOutputFormat::Json => self.run_prompt_json(input),
        }
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
        let summary = runtime.run_turn(input, Some(&mut permission_prompter))?;
        self.runtime = runtime;
        self.persist_session()?;
        println!(
            "{}",
            json!({
                "message": final_assistant_text(&summary),
                "model": self.model,
                "iterations": summary.iterations,
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
            SlashCommand::Models { args } => {
                match models::models_report(args.as_deref(), &self.model) {
                    Ok(report) => println!("{report}"),
                    Err(error) => eprintln!("{}", render_cli_error(&error)),
                }
                false
            }
            SlashCommand::Teleport { target } => {
                self.run_teleport(target.as_deref())?;
                false
            }
            SlashCommand::DebugToolCall => {
                self.run_debug_tool_call()?;
                false
            }
            SlashCommand::Compact => {
                self.compact()?;
                false
            }
            SlashCommand::Model { model } => self.set_model(model)?,
            SlashCommand::Permissions { mode } => self.set_permissions(mode)?,
            SlashCommand::Clear { confirm } => self.clear_session(confirm)?,
            SlashCommand::Cost => {
                self.print_cost();
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
                false
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
            SlashCommand::Branch { .. } => {
                eprintln!(
                    "{}",
                    render_mode_unavailable("branch", "commandes git branch")
                );
                false
            }
            SlashCommand::Worktree { .. } => {
                eprintln!(
                    "{}",
                    render_mode_unavailable("worktree", "commandes git worktree")
                );
                false
            }
            SlashCommand::CommitPushPr { .. } => {
                eprintln!(
                    "{}",
                    render_mode_unavailable("commit-push-pr", "automatisation commit + push + PR")
                );
                false
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
                &status_context(Some(&self.session.path)).expect("status context should load"),
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
                println!(
                    "Session changée\n  Session active   {}\n  Fichier          {}\n  Messages         {}",
                    self.session.id,
                    self.session.path.display(),
                    message_count,
                );
                Ok(true)
            }
            Some(other) => {
                println!("Action /session inconnue « {other} ». Utilisez /session list ou /session switch <identifiant>.");
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

    fn compact(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let result = self.runtime.compact(CompactionConfig::default());
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

fn create_managed_session_handle() -> Result<SessionHandle, Box<dyn std::error::Error>> {
    let id = generate_session_id();
    let path = sessions_dir()?.join(format!("{id}.json"));
    let handle = SessionHandle { id, path };
    activate_checkpoints(&handle);
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
    lines.push(format!("  Votre demande    {}", report.request));
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
            "  {:<17} {} commande(s) · code 2 : {}",
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
            lines.push(format!("    {matcher}{}{timeout}", entry.command));
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
        lines.push(format!("    {}", session.path.display()));
    }
    Ok(lines.join("\n"))
}

/// `/nom arguments` → demande développée d'une commande personnalisée
/// (`.claw/commands/nom.md`…), ou `None` si aucune ne porte ce nom.
fn custom_command_prompt(input: &str) -> Option<Result<String, String>> {
    let cwd = env::current_dir().ok()?;
    let invocation = commands::resolve_custom_command(&cwd, input)?;
    Some(invocation.map(|invocation| {
        let source = invocation
            .command
            .path
            .strip_prefix(&cwd)
            .unwrap_or(&invocation.command.path)
            .display()
            .to_string();
        eprintln!("↳ /{} ({source})", invocation.command.qualified_name());
        invocation.prompt
    }))
}

fn render_repl_help() -> String {
    [
        "REPL interactif".to_string(),
        "  Pour commencer       décrivez une tâche en langage courant, ou utilisez les commandes ci-dessous."
            .to_string(),
        "  Essentiel            /help · /status · /model · /permissions · /compact".to_string(),
        "  Quitter              /exit ou /quit".to_string(),
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

fn render_mode_unavailable(command: &str, label: &str) -> String {
    [
        "Commande indisponible dans ce mode".to_string(),
        format!("  Commande         /{command}"),
        format!("  Fonction         {label}"),
        "  Astuce           /help liste les commandes disponibles".to_string(),
    ]
    .join("\n")
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
        let source = match entry.source {
            ConfigSource::User => "utilisateur",
            ConfigSource::Project => "projet",
            ConfigSource::Local => "local",
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
            "  {source:<11} {status:<7} {}",
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
        "danger-full-access" | "bypassPermissions" | "dontAsk" => Some("danger-full-access"),
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
    Ok(load_system_prompt(
        env::current_dir()?,
        runtime::current_date_iso(),
        env::consts::OS,
        "unknown",
    )?)
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
                    phase: "planning started".to_string(),
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
                "analyzing request".to_string()
            } else {
                "reviewing findings".to_string()
            };
            state.detail = Some(format!("task: {}", state.task_label));
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
            state.phase = format!("running {name}");
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
            state.phase = "drafting final plan".to_string();
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
                "🧭 {} · planification démarrée · {status}",
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
            "✔ {} · terminé · {elapsed_seconds} s · {} étape(s)",
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
                "running shell command".to_string()
            } else {
                format!("command {}", truncate_for_summary(command.trim(), 100))
            }
        }
        "read_file" | "Read" => format!("reading {}", extract_tool_path(&parsed)),
        "write_file" | "Write" => format!("writing {}", extract_tool_path(&parsed)),
        "edit_file" | "Edit" => format!("editing {}", extract_tool_path(&parsed)),
        "glob_search" | "Glob" => {
            let pattern = parsed
                .get("pattern")
                .and_then(|value| value.as_str())
                .unwrap_or("?");
            let scope = parsed
                .get("path")
                .and_then(|value| value.as_str())
                .unwrap_or(".");
            format!("glob `{pattern}` in {scope}")
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
            format!("grep `{pattern}` in {scope}")
        }
        "web_search" | "WebSearch" => parsed
            .get("query")
            .and_then(|value| value.as_str())
            .map_or_else(
                || "running web search".to_string(),
                |query| format!("query {}", truncate_for_summary(query, 100)),
            ),
        _ => {
            let summary = summarize_tool_payload(input);
            if summary.is_empty() {
                format!("running {name}")
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
    let emit_output = emit_output && ui.is_none();
    let observer = ui.as_ref().map(|ui| Arc::clone(&ui.observer));
    let (feature_config, tool_registry) = build_runtime_plugin_state()?;
    models::register_cached_limits();
    let rules = feature_config.permission_rules();
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
            model,
            enable_tools,
            emit_output,
            allowed_tools.clone(),
            tool_registry.clone(),
            progress_reporter,
        )?
        .with_observer(observer.clone()),
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
                "/review — Act as a code reviewer for {target}.\n\n{}\n\nDo not modify any file. End by asking which findings I want you to fix.",
                role("reviewer")
            )
        }
        ExpertCommand::Audit => {
            let scope = args.unwrap_or("this whole project");
            format!(
                "/audit — Audit {scope}.\n\n{}\n\nDo not modify any file.",
                role("auditor")
            )
        }
        ExpertCommand::Archi => {
            let Some(topic) = args else {
                return Err("Usage : /archi <besoin ou sujet>  (ex. /archi découper le monolithe en services)".to_string());
            };
            format!(
                "/archi — Design the architecture for: {topic}\n\n{}\n\nIf essential requirements are missing, ask me focused questions before designing. Do not modify files unless I ask you to.",
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
                "/adr — Write an Architecture Decision Record about: {decision}\n\nBase it on our conversation and on the codebase. Store it in docs/adr/ (create the directory if needed) as NNNN-short-kebab-title.md, where NNNN is the next number after the existing records. Use the sections: Title, Status (Proposed), Date, Context, Decision, Alternatives considered (with their trade-offs), Consequences (positive and negative). Then create the file and show me its path."
            )
        }
        ExpertCommand::Fix => {
            let command = args.map_or_else(
                || "Detect the test command of this project.".to_string(),
                |command| format!("Test command to use: `{command}`."),
            );
            format!("/fix — {command}\n\n{}", role("tester"))
        }
    };
    Ok(format!("{prompt}{LANGUAGE_NOTE}"))
}

const PLAN_MODE_PROMPT: &str = "# Plan mode
- The session is in read-only plan mode: file edits and commands with side effects are denied.
- Explore the codebase with read and search tools (and read-only commands such as `git status`, `git log`, `ls`).
- Then present a concrete plan: the files to change, the approach, the risks, and how the result will be verified (tests, build).
- Do not attempt workarounds to modify files. The user will switch modes (`/permissions prompt` or `/permissions workspace-write`) once the plan is approved.";

struct CliPermissionPrompter {
    current_mode: PermissionMode,
}

impl CliPermissionPrompter {
    fn new(current_mode: PermissionMode) -> Self {
        Self { current_mode }
    }
}

impl runtime::PermissionPrompter for CliPermissionPrompter {
    fn decide(
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
                Some(rule) => {
                    if !rule_is_session_only(rule) {
                        match env::current_dir()
                            .map_err(|error| error.to_string())
                            .and_then(|cwd| persist_allow_rule(&cwd, rule))
                        {
                            Ok(path) => println!("  Règle `{rule}` ajoutée à {}", path.display()),
                            Err(error) => println!(
                                "  Règle `{rule}` active pour cette session (non enregistrée : {error})"
                            ),
                        }
                    }
                    runtime::PermissionPromptDecision::AllowAlways { rule: rule.clone() }
                }
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
        "WebFetch" => field("url").map(|url| format!("URL              {url}")),
        "Agent" => {
            field("description").map(|description| format!("Sous-agent       {description}"))
        }
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

/// Ajoute `rule` à `permissions.allow` de `.claw/settings.local.json` (créé si besoin).
fn persist_allow_rule(cwd: &Path, rule: &str) -> Result<PathBuf, String> {
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
    let allow = &mut permissions["allow"];
    if !allow.is_array() {
        *allow = json!([]);
    }
    if let Some(rules) = allow.as_array_mut() {
        if !rules.iter().any(|existing| existing.as_str() == Some(rule)) {
            rules.push(json!(rule));
        }
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let rendered = serde_json::to_string_pretty(&root).map_err(|error| error.to_string())?;
    fs::write(&path, format!("{rendered}\n")).map_err(|error| error.to_string())?;
    Ok(path)
}

/// Reçoit les événements d'un tour pour une interface autre que le terminal
/// (l'interface web de `claw serve`).
trait TurnObserver: Send + Sync {
    fn text_delta(&self, text: &str);
    fn tool_call(&self, name: &str, input: &str);
    fn tool_result(&self, name: &str, output: &str, is_error: bool);
    fn notice(&self, text: &str);
}

type SharedObserver = Arc<dyn TurnObserver>;

/// Interface d'un runtime piloté hors du terminal : observateur des événements
/// et invite d'approbation pour les sous-agents.
#[derive(Clone)]
struct RuntimeUi {
    observer: SharedObserver,
    subagent_prompter: Arc<dyn Fn() -> tools::SubagentPrompter + Send + Sync>,
}

struct DefaultRuntimeClient {
    runtime: tokio::runtime::Runtime,
    client: ProviderClient,
    model: String,
    enable_tools: bool,
    emit_output: bool,
    allowed_tools: Option<AllowedToolSet>,
    tool_registry: GlobalToolRegistry,
    progress_reporter: Option<InternalPromptProgressReporter>,
    observer: Option<SharedObserver>,
}

impl DefaultRuntimeClient {
    fn new(
        model: String,
        enable_tools: bool,
        emit_output: bool,
        allowed_tools: Option<AllowedToolSet>,
        tool_registry: GlobalToolRegistry,
        progress_reporter: Option<InternalPromptProgressReporter>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        Ok(Self {
            runtime: tokio::runtime::Runtime::new()?,
            client: build_provider_client(&model)?,
            model,
            enable_tools,
            emit_output,
            allowed_tools,
            tool_registry,
            progress_reporter,
            observer: None,
        })
    }

    fn with_observer(mut self, observer: Option<SharedObserver>) -> Self {
        self.observer = observer;
        self
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
            println!("\n⟳ Contexte presque plein : résumé de la conversation en cours…");
        }
        self.runtime
            .block_on(runtime::interrupt::interruptible(
                api::summarize_transcript(&self.client, &self.model, system_prompt, transcript),
            ))
            .ok()
            .filter(|summary| !summary.trim().is_empty())
    }

    fn notify(&mut self, notice: &str) {
        if self.emit_output {
            println!("⟳ {notice}");
        }
        if let Some(observer) = &self.observer {
            observer.notice(notice);
        }
    }

    #[allow(clippy::too_many_lines)]
    fn stream(&mut self, request: ApiRequest) -> Result<Vec<AssistantEvent>, RuntimeError> {
        if let Some(progress_reporter) = &self.progress_reporter {
            progress_reporter.mark_model_phase();
        }
        let message_request = MessageRequest {
            model: self.model.clone(),
            max_tokens: max_tokens_for_model(&self.model),
            messages: convert_messages(&request.messages),
            system: (!request.system_prompt.is_empty()).then(|| request.system_prompt.join("\n\n")),
            tools: self
                .enable_tools
                .then(|| filter_tool_specs(&self.tool_registry, self.allowed_tools.as_ref())),
            tool_choice: self.enable_tools.then_some(ToolChoice::Auto),
            stream: true,
        };

        self.runtime.block_on(async {
            let mut stream =
                runtime::interrupt::interruptible(self.client.stream_message(&message_request))
                    .await?;
            let mut stdout = io::stdout();
            let mut sink = io::sink();
            let out: &mut dyn Write = if self.emit_output {
                &mut stdout
            } else {
                &mut sink
            };
            let renderer = TerminalRenderer::new();
            let mut markdown_stream = MarkdownStreamState::default();
            let mut events = Vec::new();
            // Indexé par bloc : les modèles compatibles OpenAI émettent souvent
            // plusieurs appels d'outils en parallèle dont les deltas s'entrelacent.
            let mut pending_tools = PendingTools::new();
            let mut saw_stop = false;

            while let Some(event) = runtime::interrupt::interruptible(stream.next_event()).await? {
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
                            if !text.is_empty() {
                                if let Some(progress_reporter) = &self.progress_reporter {
                                    progress_reporter.mark_text_phase(&text);
                                }
                                if let Some(rendered) = markdown_stream.push(&renderer, &text) {
                                    write!(out, "{rendered}")
                                        .and_then(|()| out.flush())
                                        .map_err(|error| RuntimeError::new(error.to_string()))?;
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
                        ContentBlockDelta::ThinkingDelta { .. }
                        | ContentBlockDelta::SignatureDelta { .. } => {}
                    },
                    ApiStreamEvent::ContentBlockStop(stop) => {
                        if let Some(rendered) = markdown_stream.flush(&renderer) {
                            write!(out, "{rendered}")
                                .and_then(|()| out.flush())
                                .map_err(|error| RuntimeError::new(error.to_string()))?;
                        }
                        if let Some((id, name, input)) = pending_tools.remove(&stop.index) {
                            if let Some(progress_reporter) = &self.progress_reporter {
                                progress_reporter.mark_tool_phase(&name, &input);
                            }
                            // Display tool call now that input is fully accumulated
                            writeln!(out, "\n{}", format_tool_call_start(&name, &input))
                                .and_then(|()| out.flush())
                                .map_err(|error| RuntimeError::new(error.to_string()))?;
                            if let Some(observer) = &self.observer {
                                observer.tool_call(&name, &input);
                            }
                            events.push(AssistantEvent::ToolUse { id, name, input });
                        }
                    }
                    ApiStreamEvent::MessageDelta(delta) => {
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
                            write!(out, "{rendered}")
                                .and_then(|()| out.flush())
                                .map_err(|error| RuntimeError::new(error.to_string()))?;
                        }
                        events.push(AssistantEvent::MessageStop);
                    }
                }
            }

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

            let response =
                runtime::interrupt::interruptible(self.client.send_message(&MessageRequest {
                    stream: false,
                    ..message_request.clone()
                }))
                .await?;
            let events = response_to_events(response, out)?;
            if let Some(observer) = &self.observer {
                for event in &events {
                    match event {
                        AssistantEvent::TextDelta(text) => observer.text_delta(text),
                        AssistantEvent::ToolUse { name, input, .. } => {
                            observer.tool_call(name, input);
                        }
                        AssistantEvent::Usage(_) | AssistantEvent::MessageStop => {}
                    }
                }
            }
            Ok(events)
        })
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
    }
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

fn format_tool_call_start(name: &str, input: &str) -> String {
    let parsed: serde_json::Value =
        serde_json::from_str(input).unwrap_or(serde_json::Value::String(input.to_string()));

    let detail = match name {
        "bash" | "Bash" => format_bash_call(&parsed),
        "read_file" | "Read" => {
            let path = extract_tool_path(&parsed);
            format!("\x1b[2m📄 Lecture de {path}…\x1b[0m")
        }
        "write_file" | "Write" => {
            let path = extract_tool_path(&parsed);
            let lines = parsed
                .get("content")
                .and_then(|value| value.as_str())
                .map_or(0, |content| content.lines().count());
            format!("\x1b[1;32m✏️ Écriture de {path}\x1b[0m \x1b[2m({lines} lignes)\x1b[0m")
        }
        "edit_file" | "Edit" => {
            let path = extract_tool_path(&parsed);
            let old_value = parsed
                .get("old_string")
                .or_else(|| parsed.get("oldString"))
                .and_then(|value| value.as_str())
                .unwrap_or_default();
            let new_value = parsed
                .get("new_string")
                .or_else(|| parsed.get("newString"))
                .and_then(|value| value.as_str())
                .unwrap_or_default();
            format!(
                "\x1b[1;33m📝 Modification de {path}\x1b[0m{}",
                format_patch_preview(old_value, new_value)
                    .map(|preview| format!("\n{preview}"))
                    .unwrap_or_default()
            )
        }
        "glob_search" | "Glob" => format_search_start("🔎 Glob", &parsed),
        "grep_search" | "Grep" => format_search_start("🔎 Grep", &parsed),
        "web_search" | "WebSearch" => parsed
            .get("query")
            .and_then(|value| value.as_str())
            .unwrap_or("?")
            .to_string(),
        _ => summarize_tool_payload(input),
    };

    let border = "─".repeat(name.len() + 8);
    format!(
        "\x1b[38;5;245m╭─ \x1b[1;36m{name}\x1b[0;38;5;245m ─╮\x1b[0m\n\x1b[38;5;245m│\x1b[0m {detail}\n\x1b[38;5;245m╰{border}╯\x1b[0m"
    )
}

fn format_tool_result(name: &str, output: &str, is_error: bool) -> String {
    let icon = if is_error {
        "\x1b[1;31m✗\x1b[0m"
    } else {
        "\x1b[1;32m✓\x1b[0m"
    };
    if is_error {
        let summary = truncate_for_summary(output.trim(), 160);
        return if summary.is_empty() {
            format!("{icon} \x1b[38;5;245m{name}\x1b[0m")
        } else {
            format!("{icon} \x1b[38;5;245m{name}\x1b[0m\n\x1b[38;5;203m{summary}\x1b[0m")
        };
    }

    let parsed: serde_json::Value =
        serde_json::from_str(output).unwrap_or(serde_json::Value::String(output.to_string()));
    match name {
        "bash" | "Bash" => format_bash_result(icon, &parsed),
        "read_file" | "Read" => format_read_result(icon, &parsed),
        "write_file" | "Write" => format_write_result(icon, &parsed),
        "edit_file" | "Edit" => format_edit_result(icon, &parsed),
        "glob_search" | "Glob" => format_glob_result(icon, &parsed),
        "grep_search" | "Grep" => format_grep_result(icon, &parsed),
        _ => format_generic_tool_result(icon, name, &parsed),
    }
}

const DISPLAY_TRUNCATION_NOTICE: &str =
    "\x1b[2m… sortie tronquée à l'affichage ; le résultat complet est conservé dans la session.\x1b[0m";
const READ_DISPLAY_MAX_LINES: usize = 80;
const READ_DISPLAY_MAX_CHARS: usize = 6_000;
const TOOL_OUTPUT_DISPLAY_MAX_LINES: usize = 60;
const TOOL_OUTPUT_DISPLAY_MAX_CHARS: usize = 4_000;

fn extract_tool_path(parsed: &serde_json::Value) -> String {
    parsed
        .get("file_path")
        .or_else(|| parsed.get("filePath"))
        .or_else(|| parsed.get("path"))
        .and_then(|value| value.as_str())
        .unwrap_or("?")
        .to_string()
}

fn format_search_start(label: &str, parsed: &serde_json::Value) -> String {
    let pattern = parsed
        .get("pattern")
        .and_then(|value| value.as_str())
        .unwrap_or("?");
    let scope = parsed
        .get("path")
        .and_then(|value| value.as_str())
        .unwrap_or(".");
    format!("{label} {pattern}\n\x1b[2mdans {scope}\x1b[0m")
}

fn format_patch_preview(old_value: &str, new_value: &str) -> Option<String> {
    if old_value.is_empty() && new_value.is_empty() {
        return None;
    }
    Some(format!(
        "\x1b[38;5;203m- {}\x1b[0m\n\x1b[38;5;70m+ {}\x1b[0m",
        truncate_for_summary(first_visible_line(old_value), 72),
        truncate_for_summary(first_visible_line(new_value), 72)
    ))
}

fn format_bash_call(parsed: &serde_json::Value) -> String {
    let command = parsed
        .get("command")
        .and_then(|value| value.as_str())
        .unwrap_or_default();
    if command.is_empty() {
        String::new()
    } else {
        format!(
            "\x1b[48;5;236;38;5;255m $ {} \x1b[0m",
            truncate_for_summary(command, 160)
        )
    }
}

fn first_visible_line(text: &str) -> &str {
    text.lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or(text)
}

fn format_bash_result(icon: &str, parsed: &serde_json::Value) -> String {
    let mut lines = vec![format!("{icon} \x1b[38;5;245mbash\x1b[0m")];
    if let Some(task_id) = parsed
        .get("backgroundTaskId")
        .and_then(|value| value.as_str())
    {
        write!(&mut lines[0], " en arrière-plan ({task_id})").expect("write to string");
    } else if let Some(status) = parsed
        .get("returnCodeInterpretation")
        .and_then(|value| value.as_str())
        .filter(|status| !status.is_empty())
    {
        write!(&mut lines[0], " {status}").expect("write to string");
    }

    if let Some(stdout) = parsed.get("stdout").and_then(|value| value.as_str()) {
        if !stdout.trim().is_empty() {
            lines.push(truncate_output_for_display(
                stdout,
                TOOL_OUTPUT_DISPLAY_MAX_LINES,
                TOOL_OUTPUT_DISPLAY_MAX_CHARS,
            ));
        }
    }
    if let Some(stderr) = parsed.get("stderr").and_then(|value| value.as_str()) {
        if !stderr.trim().is_empty() {
            lines.push(format!(
                "\x1b[38;5;203m{}\x1b[0m",
                truncate_output_for_display(
                    stderr,
                    TOOL_OUTPUT_DISPLAY_MAX_LINES,
                    TOOL_OUTPUT_DISPLAY_MAX_CHARS,
                )
            ));
        }
    }

    lines.join("\n\n")
}

fn format_read_result(icon: &str, parsed: &serde_json::Value) -> String {
    let file = parsed.get("file").unwrap_or(parsed);
    let path = extract_tool_path(file);
    let start_line = file
        .get("startLine")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(1);
    let num_lines = file
        .get("numLines")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);
    let total_lines = file
        .get("totalLines")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(num_lines);
    let content = file
        .get("content")
        .and_then(|value| value.as_str())
        .unwrap_or_default();
    let end_line = start_line.saturating_add(num_lines.saturating_sub(1));

    format!(
        "{icon} \x1b[2m📄 Lu {path} (lignes {}-{} sur {})\x1b[0m\n{}",
        start_line,
        end_line.max(start_line),
        total_lines,
        truncate_output_for_display(content, READ_DISPLAY_MAX_LINES, READ_DISPLAY_MAX_CHARS)
    )
}

fn format_write_result(icon: &str, parsed: &serde_json::Value) -> String {
    let path = extract_tool_path(parsed);
    let kind = parsed
        .get("type")
        .and_then(|value| value.as_str())
        .unwrap_or("write");
    let line_count = parsed
        .get("content")
        .and_then(|value| value.as_str())
        .map_or(0, |content| content.lines().count());
    format!(
        "{icon} \x1b[1;32m✏️ {} {path}\x1b[0m \x1b[2m({line_count} lignes)\x1b[0m",
        if kind == "create" {
            "Créé"
        } else {
            "Mis à jour"
        },
    )
}

fn format_structured_patch_preview(parsed: &serde_json::Value) -> Option<String> {
    let hunks = parsed.get("structuredPatch")?.as_array()?;
    let mut preview = Vec::new();
    for hunk in hunks.iter().take(2) {
        let lines = hunk.get("lines")?.as_array()?;
        for line in lines.iter().filter_map(|value| value.as_str()).take(6) {
            match line.chars().next() {
                Some('+') => preview.push(format!("\x1b[38;5;70m{line}\x1b[0m")),
                Some('-') => preview.push(format!("\x1b[38;5;203m{line}\x1b[0m")),
                _ => preview.push(line.to_string()),
            }
        }
    }
    if preview.is_empty() {
        None
    } else {
        Some(preview.join("\n"))
    }
}

fn format_edit_result(icon: &str, parsed: &serde_json::Value) -> String {
    let path = extract_tool_path(parsed);
    let suffix = if parsed
        .get("replaceAll")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
    {
        " (toutes les occurrences)"
    } else {
        ""
    };
    let preview = format_structured_patch_preview(parsed).or_else(|| {
        let old_value = parsed
            .get("oldString")
            .and_then(|value| value.as_str())
            .unwrap_or_default();
        let new_value = parsed
            .get("newString")
            .and_then(|value| value.as_str())
            .unwrap_or_default();
        format_patch_preview(old_value, new_value)
    });

    match preview {
        Some(preview) => format!("{icon} \x1b[1;33m📝 Modifié {path}{suffix}\x1b[0m\n{preview}"),
        None => format!("{icon} \x1b[1;33m📝 Modifié {path}{suffix}\x1b[0m"),
    }
}

fn format_glob_result(icon: &str, parsed: &serde_json::Value) -> String {
    let num_files = parsed
        .get("numFiles")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);
    let filenames = parsed
        .get("filenames")
        .and_then(|value| value.as_array())
        .map(|files| {
            files
                .iter()
                .filter_map(|value| value.as_str())
                .take(8)
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default();
    if filenames.is_empty() {
        format!("{icon} \x1b[38;5;245mglob_search\x1b[0m {num_files} fichier(s) trouvé(s)")
    } else {
        format!(
            "{icon} \x1b[38;5;245mglob_search\x1b[0m {num_files} fichier(s) trouvé(s)\n{filenames}"
        )
    }
}

fn format_grep_result(icon: &str, parsed: &serde_json::Value) -> String {
    let num_matches = parsed
        .get("numMatches")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);
    let num_files = parsed
        .get("numFiles")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);
    let content = parsed
        .get("content")
        .and_then(|value| value.as_str())
        .unwrap_or_default();
    let filenames = parsed
        .get("filenames")
        .and_then(|value| value.as_array())
        .map(|files| {
            files
                .iter()
                .filter_map(|value| value.as_str())
                .take(8)
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default();
    let summary = format!(
        "{icon} \x1b[38;5;245mgrep_search\x1b[0m {num_matches} correspondance(s) dans {num_files} fichier(s)"
    );
    if !content.trim().is_empty() {
        format!(
            "{summary}\n{}",
            truncate_output_for_display(
                content,
                TOOL_OUTPUT_DISPLAY_MAX_LINES,
                TOOL_OUTPUT_DISPLAY_MAX_CHARS,
            )
        )
    } else if !filenames.is_empty() {
        format!("{summary}\n{filenames}")
    } else {
        summary
    }
}

fn format_generic_tool_result(icon: &str, name: &str, parsed: &serde_json::Value) -> String {
    let rendered_output = match parsed {
        serde_json::Value::String(text) => text.clone(),
        serde_json::Value::Null => String::new(),
        serde_json::Value::Object(_) | serde_json::Value::Array(_) => {
            serde_json::to_string_pretty(parsed).unwrap_or_else(|_| parsed.to_string())
        }
        _ => parsed.to_string(),
    };
    let preview = truncate_output_for_display(
        &rendered_output,
        TOOL_OUTPUT_DISPLAY_MAX_LINES,
        TOOL_OUTPUT_DISPLAY_MAX_CHARS,
    );

    if preview.is_empty() {
        format!("{icon} \x1b[38;5;245m{name}\x1b[0m")
    } else if preview.contains('\n') {
        format!("{icon} \x1b[38;5;245m{name}\x1b[0m\n{preview}")
    } else {
        format!("{icon} \x1b[38;5;245m{name}:\x1b[0m {preview}")
    }
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

fn truncate_output_for_display(content: &str, max_lines: usize, max_chars: usize) -> String {
    let original = content.trim_end_matches('\n');
    if original.is_empty() {
        return String::new();
    }

    let mut preview_lines = Vec::new();
    let mut used_chars = 0usize;
    let mut truncated = false;

    for (index, line) in original.lines().enumerate() {
        if index >= max_lines {
            truncated = true;
            break;
        }

        let newline_cost = usize::from(!preview_lines.is_empty());
        let available = max_chars.saturating_sub(used_chars + newline_cost);
        if available == 0 {
            truncated = true;
            break;
        }

        let line_chars = line.chars().count();
        if line_chars > available {
            preview_lines.push(line.chars().take(available).collect::<String>());
            truncated = true;
            break;
        }

        preview_lines.push(line.to_string());
        used_chars += newline_cost + line_chars;
    }

    let mut preview = preview_lines.join("\n");
    if truncated {
        if !preview.is_empty() {
            preview.push('\n');
        }
        preview.push_str(DISPLAY_TRUNCATION_NOTICE);
    }
    preview
}

/// Appels d'outils en cours de réception, par index de bloc : (id, nom, entrée JSON).
type PendingTools = std::collections::BTreeMap<u32, (String, String, String)>;

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
            if !text.is_empty() {
                let rendered = TerminalRenderer::new().markdown_to_ansi(&text);
                write!(out, "{rendered}")
                    .and_then(|()| out.flush())
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
    renderer: TerminalRenderer,
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
            renderer: TerminalRenderer::new(),
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

impl ToolExecutor for CliToolExecutor {
    fn execute(&mut self, tool_name: &str, input: &str) -> Result<String, ToolError> {
        if self
            .allowed_tools
            .as_ref()
            .is_some_and(|allowed| !allowed.contains(tool_name))
        {
            return Err(ToolError::new(format!(
                "l'outil `{tool_name}` n'est pas activé par l'option --allowedTools actuelle"
            )));
        }
        let value = serde_json::from_str(input)
            .map_err(|error| ToolError::new(format!("invalid tool input JSON: {error}")))?;
        let result = self.tool_registry.execute(tool_name, &value);
        if let Some(observer) = &self.observer {
            match &result {
                Ok(output) => observer.tool_result(tool_name, output, false),
                Err(error) => observer.tool_result(tool_name, error, true),
            }
        }
        match result {
            Ok(output) => {
                if self.emit_output {
                    let markdown = format_tool_result(tool_name, &output, false);
                    self.renderer
                        .stream_markdown(&markdown, &mut io::stdout())
                        .map_err(|error| ToolError::new(error.to_string()))?;
                }
                Ok(output)
            }
            Err(error) => {
                if self.emit_output {
                    let markdown = format_tool_result(tool_name, &error, true);
                    self.renderer
                        .stream_markdown(&markdown, &mut io::stdout())
                        .map_err(|stream_error| ToolError::new(stream_error.to_string()))?;
                }
                Err(ToolError::new(error))
            }
        }
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
        "  Ctrl+C                                Interrompt le tour en cours (/undo l'annule)",
        "  Tab                                   Complète les commandes slash",
        "  /vim                                  Active l'édition modale",
        "  Maj+Entrée / Ctrl+J                   Insère un saut de ligne",
        "",
        "Sous-commandes",
        "  claw serve [--port N] [--no-open]     Interface web locale (navigateur)",
        "  claw models [refresh|recherche]       Modèles gratuits recommandés pour le code",
        "  claw agents                           Liste les agents configurés",
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
        "  --output-format FORMAT                Sortie non interactive : text ou json",
        "  --permission-mode MODE                prompt (défaut), read-only (plan), workspace-write ou danger-full-access",
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
    use super::{
        describe_tool_progress, filter_tool_specs, format_compact_report, format_cost_report,
        format_internal_prompt_progress_line, format_model_report, format_model_switch_report,
        format_permissions_report, format_permissions_switch_report, format_resume_report,
        format_status_report, format_tool_call_start, format_tool_result,
        normalize_permission_mode, parse_args, parse_git_status_metadata, permission_policy,
        print_help_to, push_output_block, render_config_report, render_memory_report,
        render_repl_help, render_unknown_repl_command, resolve_model_alias, response_to_events,
        resume_supported_slash_commands, slash_command_completion_candidates, status_context,
        CliAction, CliOutputFormat, InternalPromptProgressEvent, InternalPromptProgressState,
        PendingTools, SlashCommand, StatusUsage, DEFAULT_MODEL,
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
        assert_eq!(
            parse_args(&["agents".to_string()]).expect("agents should parse"),
            CliAction::Agents { args: None }
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
        assert!(help.contains("/permissions [prompt|read-only|workspace-write|danger-full-access]"));
        assert!(help.contains("/clear [--confirm]"));
        assert!(help.contains("/cost"));
        assert!(help.contains("/resume <chemin-session>"));
        assert!(help.contains("/config [env|hooks|model|plugins]"));
        assert!(help.contains("/memory"));
        assert!(help.contains("/init"));
        assert!(help.contains("/diff"));
        assert!(help.contains("/version"));
        assert!(help.contains("/export [fichier]"));
        assert!(help.contains("/session [list|switch <id-session>]"));
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
                "help", "status", "compact", "clear", "cost", "config", "memory", "init", "diff",
                "version", "export", "agents", "skills",
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
        assert!(report.contains("ask              —"));
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
    fn error_hints_cover_free_model_limits() {
        assert!(super::error_hint("l'api a retourné 429 Too Many Requests").contains("quota"));
        assert!(
            super::error_hint("No endpoints found matching your data policy")
                .contains("confidentialité")
        );
        assert_eq!(super::error_hint("autre chose"), "claw --help");
    }

    #[test]
    fn direct_expert_slash_commands_become_one_shot_prompts() {
        let action = parse_args(&["/review".to_string(), "src/api".to_string()])
            .expect("direct /review should parse");
        let CliAction::Prompt { prompt, .. } = action else {
            panic!("expected a prompt action, got {action:?}");
        };
        assert!(prompt.starts_with("/review — Act as a code reviewer for src/api."));
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
        assert_eq!(context.discovered_config_files, 5);
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
    fn tool_rendering_helpers_compact_output() {
        let start = format_tool_call_start("read_file", r#"{"path":"src/main.rs"}"#);
        assert!(start.contains("read_file"));
        assert!(start.contains("src/main.rs"));

        let done = format_tool_result(
            "read_file",
            r#"{"file":{"filePath":"src/main.rs","content":"hello","numLines":1,"startLine":1,"totalLines":1}}"#,
            false,
        );
        assert!(done.contains("📄 Lu src/main.rs"));
        assert!(done.contains("hello"));
    }

    #[test]
    fn tool_rendering_truncates_large_read_output_for_display_only() {
        let content = (0..200)
            .map(|index| format!("line {index:03}"))
            .collect::<Vec<_>>()
            .join("\n");
        let output = json!({
            "file": {
                "filePath": "src/main.rs",
                "content": content,
                "numLines": 200,
                "startLine": 1,
                "totalLines": 200
            }
        })
        .to_string();

        let rendered = format_tool_result("read_file", &output, false);

        assert!(rendered.contains("line 000"));
        assert!(rendered.contains("line 079"));
        assert!(!rendered.contains("line 199"));
        assert!(rendered.contains("résultat complet est conservé dans la session"));
        assert!(output.contains("line 199"));
    }

    #[test]
    fn tool_rendering_truncates_large_bash_output_for_display_only() {
        let stdout = (0..120)
            .map(|index| format!("stdout {index:03}"))
            .collect::<Vec<_>>()
            .join("\n");
        let output = json!({
            "stdout": stdout,
            "stderr": "",
            "returnCodeInterpretation": "completed successfully"
        })
        .to_string();

        let rendered = format_tool_result("bash", &output, false);

        assert!(rendered.contains("stdout 000"));
        assert!(rendered.contains("stdout 059"));
        assert!(!rendered.contains("stdout 119"));
        assert!(rendered.contains("résultat complet est conservé dans la session"));
        assert!(output.contains("stdout 119"));
    }

    #[test]
    fn tool_rendering_truncates_generic_long_output_for_display_only() {
        let items = (0..120)
            .map(|index| format!("payload {index:03}"))
            .collect::<Vec<_>>();
        let output = json!({
            "summary": "plugin payload",
            "items": items,
        })
        .to_string();

        let rendered = format_tool_result("plugin_echo", &output, false);

        assert!(rendered.contains("plugin_echo"));
        assert!(rendered.contains("payload 000"));
        assert!(rendered.contains("payload 040"));
        assert!(!rendered.contains("payload 080"));
        assert!(!rendered.contains("payload 119"));
        assert!(rendered.contains("résultat complet est conservé dans la session"));
        assert!(output.contains("payload 119"));
    }

    #[test]
    fn tool_rendering_truncates_raw_generic_output_for_display_only() {
        let output = (0..120)
            .map(|index| format!("raw {index:03}"))
            .collect::<Vec<_>>()
            .join("\n");

        let rendered = format_tool_result("plugin_echo", &output, false);

        assert!(rendered.contains("plugin_echo"));
        assert!(rendered.contains("raw 000"));
        assert!(rendered.contains("raw 059"));
        assert!(!rendered.contains("raw 119"));
        assert!(rendered.contains("résultat complet est conservé dans la session"));
        assert!(output.contains("raw 119"));
    }

    #[test]
    fn ultraplan_progress_lines_include_phase_step_and_elapsed_status() {
        let snapshot = InternalPromptProgressState {
            command_label: "Ultraplan",
            task_label: "ship plugin progress".to_string(),
            step: 3,
            phase: "running read_file".to_string(),
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
        assert!(heartbeat.contains("phase running read_file"));
        assert!(completed.contains("terminé"));
        assert!(completed.contains("3 étape(s)"));
        assert!(failed.contains("échec"));
        assert!(failed.contains("network timeout"));
    }

    #[test]
    fn describe_tool_progress_summarizes_known_tools() {
        assert_eq!(
            describe_tool_progress("read_file", r#"{"path":"src/main.rs"}"#),
            "reading src/main.rs"
        );
        assert!(
            describe_tool_progress("bash", r#"{"command":"cargo test -p claw-cli"}"#)
                .contains("cargo test -p claw-cli")
        );
        assert_eq!(
            describe_tool_progress("grep_search", r#"{"pattern":"ultraplan","path":"rust"}"#),
            "grep `ultraplan` in rust"
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
    fn response_to_events_ignores_thinking_blocks() {
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

        assert!(matches!(
            &events[0],
            AssistantEvent::TextDelta(text) if text == "Final answer"
        ));
        assert!(!String::from_utf8(out).expect("utf8").contains("étape 1"));
    }
}
