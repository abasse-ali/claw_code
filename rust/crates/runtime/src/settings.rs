//! Réglages généraux de Claude Code que Claw applique de la même façon :
//! `env`, `attribution` (et l'ancien `includeCoAuthoredBy`),
//! `includeGitInstructions`, `cleanupPeriodDays` et `companyAnnouncements`.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use crate::config::{ConfigLoader, ConfigSource, RuntimeConfig};
use crate::json::JsonValue;

/// Ligne ajoutée par défaut à la fin des messages de commit.
pub const DEFAULT_COMMIT_ATTRIBUTION: &str = "Assisted-by: Claw Code";
/// Ligne ajoutée par défaut à la fin des descriptions de pull request.
pub const DEFAULT_PR_ATTRIBUTION: &str = "Generated with Claw Code";

/// Texte d'attribution des commits et des pull requests ; vide = aucun.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attribution {
    pub commit: String,
    pub pr: String,
}

/// `attribution` : `false` retire tout ; `commit` et `pr` remplacent chacun le
/// texte par défaut (chaîne vide = rien). `includeCoAuthoredBy: false` retire
/// les deux tant qu'`attribution` ne fixe ni `commit` ni `pr`.
#[must_use]
pub fn attribution(config: &RuntimeConfig) -> Attribution {
    let attribution = config.get("attribution");
    if matches!(attribution, Some(JsonValue::Bool(false))) {
        return Attribution {
            commit: String::new(),
            pr: String::new(),
        };
    }
    let object = attribution.and_then(JsonValue::as_object);
    let commit = object
        .and_then(|object| object.get("commit"))
        .and_then(JsonValue::as_str);
    let pr = object
        .and_then(|object| object.get("pr"))
        .and_then(JsonValue::as_str);
    let legacy_off = commit.is_none()
        && pr.is_none()
        && config
            .get("includeCoAuthoredBy")
            .and_then(JsonValue::as_bool)
            == Some(false);
    let pick = |value: Option<&str>, default: &str| {
        if legacy_off {
            String::new()
        } else {
            value.unwrap_or(default).trim().to_string()
        }
    };
    Attribution {
        commit: pick(commit, DEFAULT_COMMIT_ATTRIBUTION),
        pr: pick(pr, DEFAULT_PR_ATTRIBUTION),
    }
}

/// Ajoute `attribution` à la fin d'un message de commit ou d'une description
/// de PR, sauf si elle est vide ou déjà présente.
#[must_use]
pub fn with_attribution(text: &str, attribution: &str) -> String {
    let text = text.trim_end();
    let attribution = attribution.trim();
    if attribution.is_empty() || text.contains(attribution) {
        return text.to_string();
    }
    format!("{text}\n\n{attribution}")
}

/// Commit fait par le modèle avec l'outil bash : si le nouveau commit ne se
/// termine pas par l'attribution réglée, une remarque le lui signale (les
/// petits modèles oublient la consigne du prompt).
#[derive(Debug)]
pub struct CommitWatch {
    cwd: PathBuf,
    head: Option<String>,
    attribution: String,
}

impl CommitWatch {
    /// À appeler avant la commande ; `None` si elle ne fait pas de commit ou
    /// si aucune attribution n'est réglée.
    #[must_use]
    pub fn start(command: &str, cwd: &Path) -> Option<Self> {
        if !runs_git_commit(command) {
            return None;
        }
        let config = ConfigLoader::default_for(cwd).load().ok()?;
        let attribution = attribution(&config).commit;
        if attribution.is_empty() {
            return None;
        }
        Some(Self {
            cwd: cwd.to_path_buf(),
            head: crate::worktree::git(cwd, &["rev-parse", "HEAD"]).ok(),
            attribution,
        })
    }

    /// À appeler après la commande : remarque si un commit sans attribution
    /// vient d'être créé.
    #[must_use]
    pub fn note(&self) -> Option<String> {
        let head = crate::worktree::git(&self.cwd, &["rev-parse", "--short", "HEAD"]).ok()?;
        let full = crate::worktree::git(&self.cwd, &["rev-parse", "HEAD"]).ok()?;
        if self.head.as_deref() == Some(full.as_str()) {
            return None;
        }
        let message = crate::worktree::git(&self.cwd, &["log", "-1", "--format=%B"]).ok()?;
        if message.contains(self.attribution.trim()) {
            return None;
        }
        Some(format!(
            "[Claw] Commit {head} does not end with the attribution the settings require. Unless the user's instructions say otherwise, amend it now, before any push (`git commit --amend` with the same message, a blank line, then):\n{}",
            self.attribution
        ))
    }
}

/// La commande lance `git commit` (`git -C dir commit`, `git -c k=v commit`…).
pub(crate) fn runs_git_commit(command: &str) -> bool {
    command.split(['&', ';', '|', '\n']).any(|part| {
        let words: Vec<&str> = part.split_whitespace().collect();
        words
            .iter()
            .position(|word| *word == "git")
            .is_some_and(|start| words[start..].contains(&"commit"))
    })
}

/// `includeGitInstructions` (vrai par défaut) : consignes de commit et de PR et
/// état git dans le prompt. `CLAW_DISABLE_GIT_INSTRUCTIONS` ou
/// `CLAUDE_CODE_DISABLE_GIT_INSTRUCTIONS` le coupent pour la session.
#[must_use]
pub fn include_git_instructions(config: &RuntimeConfig) -> bool {
    let disabled_by_env = [
        "CLAW_DISABLE_GIT_INSTRUCTIONS",
        "CLAUDE_CODE_DISABLE_GIT_INSTRUCTIONS",
    ]
    .iter()
    .any(|name| {
        std::env::var(name).is_ok_and(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
    });
    !disabled_by_env
        && config
            .get("includeGitInstructions")
            .and_then(JsonValue::as_bool)
            != Some(false)
}

/// Consignes de commit et de PR données au modèle.
#[must_use]
pub fn git_instructions_section(attribution: &Attribution) -> String {
    let mut lines = vec![
        "# Committing changes and pull requests".to_string(),
        "Only create commits, push or open pull requests when the user asks for it.".to_string(),
        "- Before committing, run `git status`, `git diff` (staged and unstaged) and `git log -n 5` to see what changed and follow the repository's commit message style.".to_string(),
        "- Stage the files you mean to commit by name rather than with `git add -A`, and never commit files that likely hold secrets (`.env`, credentials, keys).".to_string(),
        "- Never change the git config, never skip hooks (`--no-verify`) or signing, never force-push to the main branch and never use interactive flags (`-i`). If a pre-commit hook fails, fix the problem and create a new commit instead of amending.".to_string(),
        "- Pass multi-line messages through a file or a heredoc so the line breaks survive.".to_string(),
        "- For a pull request, push the branch, then use `gh pr create` with a title and a body that says what changed, why, and how it was verified.".to_string(),
    ];
    if attribution.commit.is_empty() {
        lines.push("- Do not add an attribution trailer to commit messages.".to_string());
    } else {
        lines.push(format!(
            "- End every commit message with these lines, after a blank line:\n{}",
            attribution.commit
        ));
    }
    if attribution.pr.is_empty() {
        lines.push("- Do not add an attribution line to pull request descriptions.".to_string());
    } else {
        lines.push(format!(
            "- End every pull request description with:\n{}",
            attribution.pr
        ));
    }
    lines.push(
        "The user's own instructions about attribution (CLAW.md, CLAUDE.md, rules) take precedence over these lines."
            .to_string(),
    );
    lines.join("\n")
}

/// `cleanupPeriodDays` : nombre de jours de conservation des sessions (au moins 1).
#[must_use]
pub fn cleanup_period_days(config: &RuntimeConfig) -> Option<u64> {
    config
        .get("cleanupPeriodDays")
        .and_then(JsonValue::as_i64)
        .and_then(|days| u64::try_from(days).ok())
        .filter(|days| *days >= 1)
}

/// Supprime les sessions (`sessions/`) et leurs points de restauration
/// (`checkpoints/`) de `claw_dir` modifiés il y a plus de `days` jours, sauf
/// `keep` (la session en cours). Renvoie le nombre de fichiers supprimés.
#[must_use]
pub fn sweep_old_sessions(claw_dir: &Path, days: u64, keep: Option<&str>) -> usize {
    let Some(limit) = SystemTime::now().checked_sub(Duration::from_secs(days * 86_400)) else {
        return 0;
    };
    let mut removed = 0;
    for folder in ["sessions", "checkpoints"] {
        let Ok(entries) = fs::read_dir(claw_dir.join(folder)) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
                continue;
            }
            if keep.is_some_and(|id| path.file_stem().and_then(|stem| stem.to_str()) == Some(id)) {
                continue;
            }
            let old = entry
                .metadata()
                .and_then(|metadata| metadata.modified())
                .is_ok_and(|modified| modified < limit);
            if old && fs::remove_file(&path).is_ok() {
                removed += 1;
            }
        }
    }
    removed
}

/// `companyAnnouncements` : une annonce prise au hasard pour la session.
#[must_use]
pub fn company_announcement(config: &RuntimeConfig) -> Option<String> {
    let announcements: Vec<&str> = config
        .get("companyAnnouncements")
        .and_then(JsonValue::as_array)?
        .iter()
        .filter_map(JsonValue::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .collect();
    if announcements.is_empty() {
        return None;
    }
    let seed = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.subsec_nanos() as usize);
    Some(announcements[seed % announcements.len()].to_string())
}

/// Variables qu'un fichier de projet (`.claw/settings.json`,
/// `.claw/settings.local.json`, `.claw.json`) ne peut pas fixer : elles
/// choisissent où Claw écrit ses fichiers, les programmes qu'il lance, ou
/// l'adresse et la clé des fournisseurs (un dépôt cloné ne doit pas pouvoir
/// envoyer vos clés vers un autre serveur).
fn project_may_set(name: &str) -> bool {
    const EXACT: &[&str] = &[
        "HOME",
        "USERPROFILE",
        "TMPDIR",
        "TMP",
        "TEMP",
        "APPDATA",
        "LOCALAPPDATA",
        "CLAW_CONFIG_HOME",
        "CLAUDE_CONFIG_DIR",
        "PATH",
        "SYSTEMROOT",
        "WINDIR",
        "COMSPEC",
        "PROGRAMDATA",
        "PATHEXT",
        "PSMODULEPATH",
        "SHELL",
        "BASH_ENV",
        "ENV",
        "LD_PRELOAD",
        "LD_LIBRARY_PATH",
        "NODE_OPTIONS",
        "HTTP_PROXY",
        "HTTPS_PROXY",
        "ALL_PROXY",
        "OLLAMA_HOST",
        "GIT_SSH_COMMAND",
        "GIT_ASKPASS",
        "SSH_ASKPASS",
        "CLAW_ENABLE_TELEMETRY",
        "CLAUDE_CODE_ENABLE_TELEMETRY",
        "CLAW_OTEL_METRIC_PREFIX",
    ];
    // `OTEL_*` et l'activation de la télémétrie : un dépôt ne choisit pas
    // où partent vos données d'usage.
    const PREFIXES: &[&str] = &["XDG_", "PROGRAMFILES", "DYLD_", "GIT_CONFIG", "OTEL_"];
    const SUFFIXES: &[&str] = &[
        "_API_KEY",
        "_AUTH_TOKEN",
        "_BASE_URL",
        "_API_BASE",
        "_TOKEN",
        "_SECRET",
    ];
    let upper = name.to_ascii_uppercase();
    !(EXACT.contains(&upper.as_str())
        || PREFIXES.iter().any(|prefix| upper.starts_with(prefix))
        || SUFFIXES.iter().any(|suffix| upper.ends_with(suffix)))
}

/// Ajoute à `env` les variables du réglage `env` d'un fichier, dans l'ordre de
/// priorité des fichiers (le dernier l'emporte). Les fichiers de Claude Code ne
/// sont pas lus ici : leurs variables visent Claude Code.
pub(crate) fn merge_env(
    env: &mut BTreeMap<String, String>,
    ignored: &mut Vec<String>,
    source: ConfigSource,
    value: &BTreeMap<String, JsonValue>,
) {
    let Some(variables) = value.get("env").and_then(JsonValue::as_object) else {
        return;
    };
    for (name, value) in variables {
        let text = match value {
            JsonValue::String(text) => text.clone(),
            JsonValue::Number(number) => number.to_string(),
            JsonValue::Bool(flag) => flag.to_string(),
            _ => continue,
        };
        if name.trim().is_empty() || name.contains('=') {
            continue;
        }
        if source != ConfigSource::User && !project_may_set(name) {
            if !ignored.contains(name) {
                ignored.push(name.clone());
            }
            continue;
        }
        env.insert(name.clone(), text);
    }
}

/// Applique le réglage `env` au processus : Claw et tout ce qu'il lance (bash,
/// hooks, serveurs MCP) voient ces variables. Une valeur du réglage remplace
/// celle du shell ; une valeur vide vaut « non définie » pour le choix du
/// fournisseur. À appeler au démarrage, avant le lancement d'autres threads.
pub fn apply_env(config: &RuntimeConfig) {
    for (name, value) in config.env_vars() {
        std::env::set_var(name, value);
    }
}

#[cfg(test)]
mod tests {
    use super::{
        attribution, include_git_instructions, merge_env, sweep_old_sessions, with_attribution,
        Attribution, DEFAULT_COMMIT_ATTRIBUTION, DEFAULT_PR_ATTRIBUTION,
    };
    use crate::config::{ConfigLoader, ConfigSource};
    use crate::json::JsonValue;
    use std::collections::BTreeMap;
    use std::fs;
    use std::time::{Duration, SystemTime};

    fn config_with(settings: &str) -> crate::config::RuntimeConfig {
        let root = std::env::temp_dir().join(format!(
            "claw-settings-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .expect("horloge")
                .as_nanos()
        ));
        let home = root.join("home").join(".claw");
        let cwd = root.join("project");
        fs::create_dir_all(&home).expect("home");
        fs::create_dir_all(cwd.join(".claw")).expect("projet");
        fs::write(home.join("settings.json"), settings).expect("réglages");
        let config = ConfigLoader::new(&cwd, &home).load().expect("config");
        let _ = fs::remove_dir_all(root);
        config
    }

    #[test]
    fn attribution_follows_the_claude_code_rules() {
        let default = attribution(&config_with("{}"));
        assert_eq!(default.commit, DEFAULT_COMMIT_ATTRIBUTION);
        assert_eq!(default.pr, DEFAULT_PR_ATTRIBUTION);

        let off = attribution(&config_with(r#"{"attribution": false}"#));
        assert!(off.commit.is_empty() && off.pr.is_empty());

        let legacy = attribution(&config_with(r#"{"includeCoAuthoredBy": false}"#));
        assert!(legacy.commit.is_empty() && legacy.pr.is_empty());

        let custom = attribution(&config_with(
            r#"{"includeCoAuthoredBy": false, "attribution": {"commit": "Co-Authored-By: AI <ai@example.com>"}}"#,
        ));
        assert_eq!(custom.commit, "Co-Authored-By: AI <ai@example.com>");
        assert_eq!(custom.pr, DEFAULT_PR_ATTRIBUTION);

        let no_pr = attribution(&config_with(r#"{"attribution": {"pr": ""}}"#));
        assert_eq!(no_pr.commit, DEFAULT_COMMIT_ATTRIBUTION);
        assert!(no_pr.pr.is_empty());

        assert_eq!(
            with_attribution("feat: notes\n", "Assisted-by: Claw Code"),
            "feat: notes\n\nAssisted-by: Claw Code"
        );
        assert_eq!(
            with_attribution(
                "feat: notes\n\nAssisted-by: Claw Code",
                "Assisted-by: Claw Code"
            ),
            "feat: notes\n\nAssisted-by: Claw Code"
        );
        assert_eq!(with_attribution("feat: notes", ""), "feat: notes");
        let section = super::git_instructions_section(&Attribution {
            commit: String::new(),
            pr: "Generated with Claw Code".to_string(),
        });
        assert!(section.contains("Do not add an attribution trailer"));
        assert!(section.contains("Generated with Claw Code"));
        assert!(!include_git_instructions(&config_with(
            r#"{"includeGitInstructions": false}"#
        )));
        assert!(super::runs_git_commit("git add a.py && git commit -m 'x'"));
        assert!(super::runs_git_commit(
            "git -C sub -c user.name=x commit -F msg"
        ));
        assert!(!super::runs_git_commit("git status; echo commit"));
        assert!(!super::runs_git_commit("git log --oneline"));
    }

    #[test]
    fn project_env_cannot_redirect_keys_or_storage() {
        let mut env = BTreeMap::new();
        let mut ignored = Vec::new();
        let value = |json: &str| match JsonValue::parse(json).expect("json") {
            JsonValue::Object(object) => object,
            _ => unreachable!(),
        };
        merge_env(
            &mut env,
            &mut ignored,
            ConfigSource::User,
            &value(r#"{"env": {"OPENROUTER_BASE_URL": "https://proxy.example.com", "A": "1"}}"#),
        );
        merge_env(
            &mut env,
            &mut ignored,
            ConfigSource::Project,
            &value(
                r#"{"env": {"OPENROUTER_BASE_URL": "https://evil.example.com", "OPENROUTER_API_KEY": "x", "HOME": "/tmp", "XDG_CONFIG_HOME": "/tmp", "A": "2", "RUST_LOG": "debug", "N": 3}}"#,
            ),
        );
        assert_eq!(env["OPENROUTER_BASE_URL"], "https://proxy.example.com");
        assert_eq!(env["A"], "2");
        assert_eq!(env["RUST_LOG"], "debug");
        assert_eq!(env["N"], "3");
        assert!(!env.contains_key("HOME") && !env.contains_key("OPENROUTER_API_KEY"));
        assert_eq!(
            ignored,
            vec![
                "HOME",
                "OPENROUTER_API_KEY",
                "OPENROUTER_BASE_URL",
                "XDG_CONFIG_HOME"
            ]
        );
        // Un dépôt ne règle pas la télémétrie.
        let mut telemetry_ignored = Vec::new();
        merge_env(
            &mut env,
            &mut telemetry_ignored,
            ConfigSource::Local,
            &value(
                r#"{"env": {"CLAW_ENABLE_TELEMETRY": "1", "OTEL_EXPORTER_OTLP_ENDPOINT": "https://evil.example.com"}}"#,
            ),
        );
        assert_eq!(telemetry_ignored.len(), 2);
    }

    #[test]
    fn sweep_removes_only_old_sessions() {
        let root = std::env::temp_dir().join(format!("claw-sweep-{}", std::process::id()));
        let sessions = root.join("sessions");
        fs::create_dir_all(&sessions).expect("sessions");
        fs::create_dir_all(root.join("checkpoints")).expect("checkpoints");
        let old_time = SystemTime::now() - Duration::from_secs(40 * 86_400);
        for (path, old) in [
            (sessions.join("old.json"), true),
            (sessions.join("current.json"), true),
            (sessions.join("recent.json"), false),
            (root.join("checkpoints").join("old.json"), true),
            (sessions.join("notes.txt"), true),
        ] {
            fs::write(&path, "{}").expect("fichier");
            if old {
                fs::File::options()
                    .write(true)
                    .open(&path)
                    .and_then(|file| file.set_modified(old_time))
                    .expect("date");
            }
        }
        assert_eq!(sweep_old_sessions(&root, 30, Some("current")), 2);
        assert!(!sessions.join("old.json").exists());
        assert!(sessions.join("current.json").exists());
        assert!(sessions.join("recent.json").exists());
        assert!(sessions.join("notes.txt").exists());
        let _ = fs::remove_dir_all(root);
    }
}
