//! `claw doctor` (ou `/doctor`) : vérifie que tout est prêt pour travailler
//! (clés, modèles, serveur local, outils, terminal) et dit quoi corriger.

use std::env;
use std::fs;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};

use runtime::ConfigLoader;

use crate::transcript::{DIM, GREEN, RED, RESET};

const YELLOW: &str = "\x1b[38;5;179m";

enum Level {
    Ok,
    Warn,
    Fail,
    Info,
}

struct Check {
    level: Level,
    label: &'static str,
    detail: String,
}

impl Check {
    fn new(level: Level, label: &'static str, detail: impl Into<String>) -> Self {
        Self {
            level,
            label,
            detail: detail.into(),
        }
    }
}

/// Rapport complet ; `version` : version et commit du binaire.
#[must_use]
pub fn report(model: &str, version: &str) -> String {
    let mut checks = vec![
        Check::new(Level::Ok, "Version", version),
        executable_check(),
    ];
    checks.extend(config_checks(model));
    checks.push(openrouter_check(model));
    checks.push(anthropic_check());
    checks.extend(local_server_checks());
    checks.push(tool_check("Git", "git", true));
    checks.push(shell_check());
    checks.push(sandbox_check());
    checks.push(terminal_check());
    checks.push(sessions_check());

    let color = std::io::stdout().is_terminal();
    let paint = |code: &'static str| if color { code } else { "" };
    let mut lines = vec!["Diagnostic de Claw".to_string()];
    for check in &checks {
        let (mark, code) = match check.level {
            Level::Ok => ("✓", GREEN),
            Level::Warn => ("▲", YELLOW),
            Level::Fail => ("✗", RED),
            Level::Info => ("·", DIM),
        };
        lines.push(format!(
            "  {}{mark}{} {:<14} {}",
            paint(code),
            paint(RESET),
            check.label,
            check.detail
        ));
    }
    let failures = checks
        .iter()
        .filter(|check| matches!(check.level, Level::Fail))
        .count();
    let warnings = checks
        .iter()
        .filter(|check| matches!(check.level, Level::Warn))
        .count();
    lines.push(String::new());
    lines.push(match (failures, warnings) {
        (0, 0) => "Tout est prêt.".to_string(),
        (0, _) => format!("Prêt, avec {warnings} point(s) à surveiller (▲)."),
        _ => {
            format!("{failures} problème(s) à corriger (✗), {warnings} point(s) à surveiller (▲).")
        }
    });
    lines.join("\n")
}

fn same_file(left: &Path, right: &Path) -> bool {
    match (fs::canonicalize(left), fs::canonicalize(right)) {
        (Ok(left), Ok(right)) => left == right,
        _ => false,
    }
}

/// Le `claw` trouvé sur le PATH doit être celui-ci (une ancienne copie, un
/// alias ou une fonction du profil du shell peuvent le masquer).
fn executable_check() -> Check {
    let Ok(current) = env::current_exe() else {
        return Check::new(Level::Warn, "Exécutable", "emplacement inconnu");
    };
    let shown = current.display().to_string();
    match runtime::find_executable("claw") {
        Some(found) if same_file(&found, &current) => Check::new(Level::Ok, "Exécutable", shown),
        Some(found) => Check::new(
            Level::Warn,
            "Exécutable",
            format!(
                "{shown} ; mais « claw » sur le PATH désigne {} (ancienne copie ?)",
                found.display()
            ),
        ),
        None => Check::new(
            Level::Warn,
            "Exécutable",
            format!("{shown} (pas sur le PATH : « claw » ne marchera pas partout)"),
        ),
    }
}

fn config_checks(model: &str) -> Vec<Check> {
    let cwd = env::current_dir().unwrap_or_default();
    let (config_check, fallbacks) = match ConfigLoader::default_for(&cwd).load() {
        Ok(config) => {
            let files = config
                .loaded_entries()
                .iter()
                .map(|entry| shorten(&entry.path))
                .collect::<Vec<_>>();
            let detail = if files.is_empty() {
                "aucun fichier (réglages par défaut)".to_string()
            } else {
                files.join(", ")
            };
            (
                Check::new(Level::Ok, "Configuration", detail),
                config.fallback_models().to_vec(),
            )
        }
        Err(error) => (
            Check::new(Level::Fail, "Configuration", format!("illisible : {error}")),
            Vec::new(),
        ),
    };
    let chain = crate::models::model_chain(model, &fallbacks);
    let model_detail = match chain.split_first() {
        Some((first, rest)) if !rest.is_empty() => {
            format!("{first} (secours : {})", rest.join(", "))
        }
        Some((first, _)) => format!("{first} (aucun modèle de secours)"),
        None => model.to_string(),
    };
    vec![config_check, Check::new(Level::Ok, "Modèle", model_detail)]
}

fn openrouter_check(model: &str) -> Check {
    let needed = crate::credits::is_openrouter_model(model);
    match crate::credits::account_status(true) {
        None if needed => Check::new(
            Level::Fail,
            "OpenRouter",
            "clé absente : définissez OPENROUTER_API_KEY (variable d'environnement utilisateur)",
        ),
        None => Check::new(Level::Info, "OpenRouter", "pas de clé (facultatif ici)"),
        Some(Ok(_)) => Check::new(
            Level::Ok,
            "OpenRouter",
            crate::credits::current_line(false).unwrap_or_default(),
        ),
        Some(Err(error)) => Check::new(
            if needed { Level::Fail } else { Level::Warn },
            "OpenRouter",
            format!("clé refusée ou service injoignable : {error}"),
        ),
    }
}

fn anthropic_check() -> Check {
    let has_key = ["ANTHROPIC_API_KEY", "ANTHROPIC_AUTH_TOKEN"]
        .iter()
        .any(|name| env::var(name).is_ok_and(|value| !value.trim().is_empty()));
    if has_key {
        Check::new(Level::Ok, "Anthropic", "clé présente")
    } else {
        Check::new(Level::Info, "Anthropic", "pas de clé (facultatif)")
    }
}

fn local_server_checks() -> Vec<Check> {
    crate::models::local_servers()
        .into_iter()
        .map(|(name, base_url, models)| {
            let label = match name.as_str() {
                "ollama" => "Ollama",
                "lmstudio" => "LM Studio",
                "llamacpp" => "llama.cpp",
                _ => "Serveur local",
            };
            match models {
                Ok(models) if !models.is_empty() => Check::new(
                    Level::Ok,
                    label,
                    format!(
                        "{} modèle(s) : {} ({base_url})",
                        models.len(),
                        models.join(", ")
                    ),
                ),
                Ok(_) => Check::new(
                    Level::Warn,
                    label,
                    format!("en marche mais sans modèle ({base_url})"),
                ),
                Err(error) => Check::new(
                    Level::Info,
                    label,
                    format!("{error} (facultatif : pour travailler hors ligne)"),
                ),
            }
        })
        .collect()
}

fn tool_check(label: &'static str, program: &str, important: bool) -> Check {
    match runtime::find_executable(program) {
        Some(path) => Check::new(Level::Ok, label, path.display().to_string()),
        None if important => Check::new(Level::Warn, label, format!("{program} introuvable")),
        None => Check::new(Level::Info, label, format!("{program} absent (facultatif)")),
    }
}

/// Les commandes de l'agent passent par un shell POSIX (Git Bash sous Windows).
fn shell_check() -> Check {
    match plugins::posix_shell_program() {
        Some(path) => Check::new(Level::Ok, "Shell", path.display().to_string()),
        None if cfg!(windows) => Check::new(
            Level::Fail,
            "Shell",
            "aucun bash trouvé : installez Git pour Windows (il fournit Git Bash)",
        ),
        None => Check::new(Level::Fail, "Shell", "sh introuvable"),
    }
}

fn sandbox_check() -> Check {
    let engines = ["docker", "podman"]
        .into_iter()
        .filter(|engine| runtime::find_executable(engine).is_some())
        .collect::<Vec<_>>();
    if engines.is_empty() {
        Check::new(
            Level::Info,
            "Bac à sable",
            "ni docker ni podman (facultatif : commandes isolées en conteneur)",
        )
    } else {
        Check::new(Level::Ok, "Bac à sable", engines.join(", "))
    }
}

fn terminal_check() -> Check {
    if !std::io::stdout().is_terminal() {
        return Check::new(
            Level::Info,
            "Terminal",
            "sortie redirigée (pas de terminal)",
        );
    }
    let columns = crossterm::terminal::size().map_or(0, |(columns, _)| columns);
    if columns < 80 {
        Check::new(
            Level::Warn,
            "Terminal",
            format!("{columns} colonnes : agrandissez la fenêtre (80 au moins)"),
        )
    } else {
        Check::new(Level::Ok, "Terminal", format!("{columns} colonnes"))
    }
}

fn sessions_check() -> Check {
    let dir: PathBuf = env::current_dir()
        .unwrap_or_default()
        .join(".claw")
        .join("sessions");
    let probe = dir.join(".doctor");
    let writable = fs::create_dir_all(&dir).is_ok() && fs::write(&probe, b"ok").is_ok();
    let _ = fs::remove_file(&probe);
    if writable {
        Check::new(Level::Ok, "Sessions", shorten(&dir))
    } else {
        Check::new(
            Level::Fail,
            "Sessions",
            format!("{} : écriture impossible", dir.display()),
        )
    }
}

/// Chemin court : relatif au dossier courant, ou `~/…` dans le dossier personnel.
fn shorten(path: &Path) -> String {
    if let Some(relative) = env::current_dir()
        .ok()
        .and_then(|cwd| path.strip_prefix(cwd).ok().map(Path::to_path_buf))
    {
        return relative.display().to_string();
    }
    if let Some(relative) =
        runtime::home_dir().and_then(|home| path.strip_prefix(home).ok().map(Path::to_path_buf))
    {
        return format!("~{}{}", std::path::MAIN_SEPARATOR, relative.display());
    }
    path.display().to_string()
}
