//! Compétences venues de Claude, en plus de celles des dossiers `skills/` du
//! projet et de l'utilisateur : celles que l'application Claude synchronise
//! (`~/.claude/skills/synced/<compte>/<nom>/SKILL.md`) et celles des plugins de
//! Claude Code activés. Une compétence écrite pour un environnement que Claw
//! n'offre pas (application de bureau Claude, connecteurs de claude.ai, outils
//! MCP non configurés) est gardée avec la raison, mais n'est pas proposée.
//!
//! `CLAW_NO_CLAUDE_CONFIG=1` désactive cet import.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde_json::Value;

/// D'où vient une compétence importée.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportedOrigin {
    /// Synchronisée par l'application Claude.
    ClaudeApp,
    /// Fournie par un plugin de Claude Code (son nom).
    ClaudePlugin(String),
}

/// Compétence importée de Claude.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportedSkill {
    /// Nom à donner à l'outil `Skill` (« plugin:nom » pour un plugin).
    pub name: String,
    pub description: Option<String>,
    pub skill_file: PathBuf,
    pub origin: ImportedOrigin,
    /// Le modèle peut la choisir de lui-même ; sinon seulement par `/nom`
    /// (`disable-model-invocation: true`).
    pub model_invocable: bool,
    /// Pourquoi Claw ne la propose pas, s'il y a une raison.
    pub unavailable: Option<String>,
}

/// Compétences synchronisées qui reposent sur des connecteurs ou des
/// fonctions de claude.ai que Claw n'a pas.
const CLAUDE_AI_ONLY: &[(&str, &str)] = &[
    ("docs", "repose sur le connecteur Claude Docs de claude.ai"),
    (
        "google-workspace",
        "repose sur les connecteurs Google de claude.ai",
    ),
    (
        "morning",
        "repose sur l'agenda, la messagerie et les artefacts de claude.ai",
    ),
    (
        "setup-writing-style",
        "lit vos messages envoyés par les connecteurs de claude.ai",
    ),
];

/// Environnements cités par le champ `compatibility` qui ne sont pas Claw.
const CLAUDE_ONLY_ENVIRONMENTS: &[&str] = &["desktop app", "cowork", "claude.ai"];

fn claude_config_disabled() -> bool {
    std::env::var("CLAW_NO_CLAUDE_CONFIG").is_ok_and(|value| value.trim() == "1")
}

/// Compétences importées de Claude pour le projet `cwd`, proposées ou non.
#[must_use]
pub fn imported_skills(cwd: &Path) -> Vec<ImportedSkill> {
    if claude_config_disabled() {
        return Vec::new();
    }
    let Some(home) = crate::home_dir() else {
        return Vec::new();
    };
    collect(&home.join(".claude"), cwd, &configured_mcp_servers(cwd))
}

/// Fichier `SKILL.md` d'une compétence importée et proposée, d'après son nom
/// (« plugin:nom », ou le seul « nom » s'il n'y a pas d'ambiguïté).
#[must_use]
pub fn find_imported_skill(cwd: &Path, name: &str) -> Option<PathBuf> {
    find_in(&imported_skills(cwd), name)
}

fn find_in(skills: &[ImportedSkill], name: &str) -> Option<PathBuf> {
    let available: Vec<&ImportedSkill> = skills
        .iter()
        .filter(|skill| skill.unavailable.is_none())
        .collect();
    if let Some(skill) = available
        .iter()
        .find(|skill| skill.name.eq_ignore_ascii_case(name))
    {
        return Some(skill.skill_file.clone());
    }
    let short: Vec<&&ImportedSkill> = available
        .iter()
        .filter(|skill| {
            skill
                .name
                .rsplit_once(':')
                .is_some_and(|(_, short)| short.eq_ignore_ascii_case(name))
        })
        .collect();
    match short.as_slice() {
        [only] => Some(only.skill_file.clone()),
        _ => None,
    }
}

/// Noms des serveurs MCP configurés dans Claw pour ce projet.
fn configured_mcp_servers(cwd: &Path) -> Vec<String> {
    crate::ConfigLoader::default_for(cwd)
        .load()
        .map(|config| config.mcp().servers().keys().cloned().collect())
        .unwrap_or_default()
}

fn collect(claude_home: &Path, cwd: &Path, mcp_servers: &[String]) -> Vec<ImportedSkill> {
    let mut skills = synced_skills(claude_home, mcp_servers);
    skills.extend(plugin_skills(claude_home, cwd, mcp_servers));
    skills
}

/// Dossiers d'un dossier, du plus récemment modifié au plus ancien.
fn subdirectories_newest_first(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut found: Vec<(std::time::SystemTime, PathBuf)> = entries
        .flatten()
        .filter(|entry| entry.path().is_dir())
        .filter(|entry| !entry.file_name().to_string_lossy().starts_with('.'))
        .map(|entry| {
            let modified = entry
                .metadata()
                .and_then(|metadata| metadata.modified())
                .unwrap_or(std::time::UNIX_EPOCH);
            (modified, entry.path())
        })
        .collect();
    found.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| left.1.cmp(&right.1)));
    found.into_iter().map(|(_, path)| path).collect()
}

/// Compétences synchronisées par l'application Claude : un dossier par compte,
/// le plus récent l'emporte quand un nom se répète.
fn synced_skills(claude_home: &Path, mcp_servers: &[String]) -> Vec<ImportedSkill> {
    let mut seen = BTreeSet::new();
    let mut skills = Vec::new();
    for account in subdirectories_newest_first(&claude_home.join("skills").join("synced")) {
        let mut dirs = subdirectories_newest_first(&account);
        dirs.sort();
        for dir in dirs {
            let Some(skill) = read_skill(&dir, None, ImportedOrigin::ClaudeApp, mcp_servers) else {
                continue;
            };
            if seen.insert(skill.name.to_ascii_lowercase()) {
                skills.push(skill);
            }
        }
    }
    skills.sort_by(|left, right| left.name.cmp(&right.name));
    skills
}

/// `enabledPlugins` des réglages de Claude Code (utilisateur, projet, local :
/// le dernier lu l'emporte).
fn enabled_plugins(claude_home: &Path, cwd: &Path) -> BTreeMap<String, bool> {
    let mut enabled = BTreeMap::new();
    let files = [
        claude_home.join("settings.json"),
        cwd.join(".claude").join("settings.json"),
        cwd.join(".claude").join("settings.local.json"),
    ];
    for file in files {
        let Some(value) = std::fs::read_to_string(&file)
            .ok()
            .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        else {
            continue;
        };
        if let Some(map) = value.get("enabledPlugins").and_then(Value::as_object) {
            for (plugin, on) in map {
                enabled.insert(plugin.clone(), on.as_bool().unwrap_or(false));
            }
        }
    }
    enabled
}

/// Dossiers d'installation des plugins (`installed_plugins.json`, dans son
/// ancien format - un objet par plugin - ou le nouveau - une liste).
fn installed_plugins(claude_home: &Path) -> Vec<(String, PathBuf)> {
    let Some(value) =
        std::fs::read_to_string(claude_home.join("plugins").join("installed_plugins.json"))
            .ok()
            .and_then(|text| serde_json::from_str::<Value>(&text).ok())
    else {
        return Vec::new();
    };
    let Some(plugins) = value.get("plugins").and_then(Value::as_object) else {
        return Vec::new();
    };
    let mut found = Vec::new();
    for (key, entry) in plugins {
        let entries = match entry {
            Value::Array(list) => list.iter().collect::<Vec<_>>(),
            other => vec![other],
        };
        for install in entries {
            if let Some(path) = install.get("installPath").and_then(Value::as_str) {
                found.push((key.clone(), PathBuf::from(path)));
            }
        }
    }
    found
}

/// Compétences des plugins de Claude Code installés et activés.
fn plugin_skills(claude_home: &Path, cwd: &Path, mcp_servers: &[String]) -> Vec<ImportedSkill> {
    let enabled = enabled_plugins(claude_home, cwd);
    let mut seen = BTreeSet::new();
    let mut skills = Vec::new();
    for (key, install_path) in installed_plugins(claude_home) {
        if !enabled.get(&key).copied().unwrap_or(false) {
            continue;
        }
        let plugin = key.split('@').next().unwrap_or(&key).to_string();
        let mut dirs = subdirectories_newest_first(&install_path.join("skills"));
        dirs.sort();
        for dir in dirs {
            let origin = ImportedOrigin::ClaudePlugin(plugin.clone());
            let Some(skill) = read_skill(&dir, Some(&plugin), origin, mcp_servers) else {
                continue;
            };
            if seen.insert(skill.name.to_ascii_lowercase()) {
                skills.push(skill);
            }
        }
    }
    skills.sort_by(|left, right| left.name.cmp(&right.name));
    skills
}

fn read_skill(
    dir: &Path,
    plugin: Option<&str>,
    origin: ImportedOrigin,
    mcp_servers: &[String],
) -> Option<ImportedSkill> {
    let skill_file = dir.join("SKILL.md");
    let contents = std::fs::read_to_string(&skill_file).ok()?;
    let fields = frontmatter(&contents);
    let short = fields
        .get("name")
        .cloned()
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| {
            dir.file_name()
                .map_or_else(String::new, |name| name.to_string_lossy().to_string())
        });
    let name = match plugin {
        Some(plugin) => format!("{plugin}:{short}"),
        None => short.clone(),
    };
    let unavailable = unavailable_reason(&short, &origin, &fields, &contents, mcp_servers);
    Some(ImportedSkill {
        name,
        description: fields.get("description").cloned(),
        skill_file,
        model_invocable: fields
            .get("disable-model-invocation")
            .is_none_or(|value| !value.eq_ignore_ascii_case("true")),
        origin,
        unavailable,
    })
}

fn unavailable_reason(
    name: &str,
    origin: &ImportedOrigin,
    fields: &BTreeMap<String, String>,
    contents: &str,
    mcp_servers: &[String],
) -> Option<String> {
    if let Some(compatibility) = fields.get("compatibility") {
        let lower = compatibility.to_ascii_lowercase();
        if CLAUDE_ONLY_ENVIRONMENTS
            .iter()
            .any(|environment| lower.contains(environment))
        {
            return Some("prévue pour l'application de bureau Claude".to_string());
        }
    }
    if *origin == ImportedOrigin::ClaudeApp {
        if let Some((_, reason)) = CLAUDE_AI_ONLY
            .iter()
            .find(|(only, _)| only.eq_ignore_ascii_case(name))
        {
            return Some((*reason).to_string());
        }
    }
    let normalize = |server: &str| server.to_ascii_lowercase().replace('-', "_");
    let configured: BTreeSet<String> = mcp_servers.iter().map(|server| normalize(server)).collect();
    let missing: Vec<String> = mcp_servers_used(contents)
        .into_iter()
        .filter(|server| !configured.contains(&normalize(server)))
        .collect();
    if !missing.is_empty() {
        return Some(format!(
            "utilise des outils MCP absents de Claw : {}",
            missing.join(", ")
        ));
    }
    None
}

/// Serveurs MCP dont la compétence appelle des outils (`mcp__serveur__outil`).
fn mcp_servers_used(contents: &str) -> BTreeSet<String> {
    let mut servers = BTreeSet::new();
    let mut rest = contents;
    while let Some(start) = rest.find("mcp__") {
        rest = &rest[start + 5..];
        let candidate: String = rest
            .chars()
            .take_while(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-'))
            .collect();
        if let Some((server, _)) = candidate.split_once("__") {
            if !server.is_empty() {
                servers.insert(server.to_string());
            }
        }
    }
    servers
}

/// Champs de l'en-tête YAML (`---` … `---`) : valeurs d'une ligne, entre
/// guillemets ou non, et blocs `>` / `|` sur les lignes indentées suivantes.
fn frontmatter(contents: &str) -> BTreeMap<String, String> {
    let mut fields = BTreeMap::new();
    let mut lines = contents.lines().peekable();
    if lines.next().map(str::trim) != Some("---") {
        return fields;
    }
    while let Some(line) = lines.next() {
        if line.trim() == "---" {
            break;
        }
        if line.starts_with([' ', '\t']) {
            continue;
        }
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let value = value.trim();
        let value = if value.starts_with(['>', '|']) {
            let mut block = Vec::new();
            while let Some(next) = lines.peek() {
                if next.trim() == "---"
                    || !(next.starts_with([' ', '\t']) || next.trim().is_empty())
                {
                    break;
                }
                block.push(next.trim().to_string());
                lines.next();
            }
            let separator = if value.starts_with('|') { "\n" } else { " " };
            block.join(separator).trim().to_string()
        } else {
            unquote(value)
        };
        fields.insert(key.trim().to_string(), value);
    }
    fields
}

fn unquote(value: &str) -> String {
    if value.len() >= 2 && value.starts_with('"') && value.ends_with('"') {
        let inner = &value[1..value.len() - 1];
        let mut out = String::with_capacity(inner.len());
        let mut chars = inner.chars();
        while let Some(ch) = chars.next() {
            if ch == '\\' {
                match chars.next() {
                    Some('n') => out.push('\n'),
                    Some('t') => out.push('\t'),
                    Some(other) => out.push(other),
                    None => {}
                }
            } else {
                out.push(ch);
            }
        }
        out
    } else if value.len() >= 2 && value.starts_with('\'') && value.ends_with('\'') {
        value[1..value.len() - 1].replace("''", "'")
    } else {
        value.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::{collect, find_in, frontmatter, mcp_servers_used, ImportedOrigin};
    use std::fs;
    use std::path::{Path, PathBuf};

    fn temp_dir(name: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_nanos());
        let dir = std::env::temp_dir().join(format!("claw-claude-skills-{name}-{nanos}"));
        fs::create_dir_all(&dir).expect("dossier");
        dir
    }

    fn skill(dir: &Path, name: &str, header: &str, body: &str) {
        fs::create_dir_all(dir.join(name)).expect("dossier");
        fs::write(
            dir.join(name).join("SKILL.md"),
            format!("---\nname: {name}\n{header}---\n\n{body}\n"),
        )
        .expect("compétence");
    }

    #[test]
    fn reads_quoted_and_block_frontmatter() {
        let fields = frontmatter(
            "---\nname: pdf\ndescription: \"Lire un \\\"PDF\\\"\"\nnote: 'l''outil'\nlong: >\n  première ligne\n  seconde ligne\ndisable-model-invocation: true\n---\ncorps\n",
        );
        assert_eq!(fields["description"], "Lire un \"PDF\"");
        assert_eq!(fields["note"], "l'outil");
        assert_eq!(fields["long"], "première ligne seconde ligne");
        assert_eq!(fields["disable-model-invocation"], "true");
        assert!(frontmatter("pas d'en-tête\n").is_empty());
    }

    #[test]
    fn finds_the_mcp_servers_a_skill_calls() {
        let used = mcp_servers_used(
            "Appelle mcp__Claude_Browser__navigate puis `mcp__remote-devices__computer_click`, pas mcp__ seul.",
        );
        assert_eq!(
            used.into_iter().collect::<Vec<_>>(),
            ["Claude_Browser", "remote-devices"]
        );
    }

    #[test]
    fn imports_synced_skills_and_sets_aside_those_claw_cannot_run() {
        let home = temp_dir("synced");
        let cwd = temp_dir("projet");
        let old = home.join("skills").join("synced").join("compte-ancien");
        let new = home.join("skills").join("synced").join("compte-recent");
        skill(&old, "pdf", "description: ancienne version\n", "");
        std::thread::sleep(std::time::Duration::from_millis(30));
        skill(
            &new,
            "pdf",
            "description: Travailler sur des PDF\n",
            "Lance scripts/fill.py",
        );
        skill(
            &new,
            "built-in-browser",
            "description: Navigateur\ncompatibility: \"Cowork in the Claude desktop app\"\n",
            "",
        );
        skill(&new, "docs", "description: Documents\n", "");
        skill(
            &new,
            "import-memory",
            "description: Mémoire\n",
            "Utilise mcp__memory__memory_write.",
        );
        skill(
            &new,
            "privee",
            "description: Seulement par /privee\ndisable-model-invocation: true\n",
            "",
        );
        // Le dossier le plus récent l'emporte pour un même nom.
        fs::write(
            new.join("pdf").join("SKILL.md"),
            "---\nname: pdf\ndescription: Travailler sur des PDF\n---\n",
        )
        .expect("maj");

        let skills = collect(&home, &cwd, &[]);
        let by_name = |name: &str| skills.iter().find(|skill| skill.name == name).expect(name);
        assert_eq!(skills.iter().filter(|skill| skill.name == "pdf").count(), 1);
        assert_eq!(
            by_name("pdf").description.as_deref(),
            Some("Travailler sur des PDF")
        );
        assert!(by_name("pdf").skill_file.starts_with(&new));
        assert_eq!(by_name("pdf").unavailable, None);
        assert_eq!(by_name("pdf").origin, ImportedOrigin::ClaudeApp);
        assert_eq!(
            by_name("built-in-browser").unavailable.as_deref(),
            Some("prévue pour l'application de bureau Claude")
        );
        assert!(by_name("docs")
            .unavailable
            .as_deref()
            .is_some_and(|reason| reason.contains("Claude Docs")));
        assert_eq!(
            by_name("import-memory").unavailable.as_deref(),
            Some("utilise des outils MCP absents de Claw : memory")
        );
        assert!(!by_name("privee").model_invocable && by_name("privee").unavailable.is_none());

        // Avec un serveur MCP « memory » configuré dans Claw, elle devient utilisable.
        let with_memory = collect(&home, &cwd, &["memory".to_string()]);
        assert!(with_memory
            .iter()
            .any(|skill| skill.name == "import-memory" && skill.unavailable.is_none()));

        assert_eq!(
            find_in(&skills, "PDF"),
            Some(new.join("pdf").join("SKILL.md"))
        );
        assert_eq!(find_in(&skills, "docs"), None);
        let _ = fs::remove_dir_all(home);
        let _ = fs::remove_dir_all(cwd);
    }

    #[test]
    fn imports_skills_of_enabled_claude_code_plugins_only() {
        let home = temp_dir("plugins");
        let cwd = temp_dir("projet-plugins");
        let installs = home.join("plugins").join("cache");
        let actif = installs.join("outils");
        let coupe = installs.join("coupe");
        let ancien = installs.join("ancien");
        skill(
            &actif.join("skills"),
            "revue",
            "description: Revue de code\n",
            "",
        );
        skill(
            &coupe.join("skills"),
            "rien",
            "description: Plugin désactivé\n",
            "",
        );
        skill(
            &ancien.join("skills"),
            "notes",
            "description: Ancien format\n",
            "",
        );
        fs::write(
            home.join("plugins").join("installed_plugins.json"),
            serde_json::json!({
                "version": 2,
                "plugins": {
                    "outils@labo": [{ "scope": "user", "installPath": actif.to_string_lossy() }],
                    "coupe@labo": [{ "scope": "user", "installPath": coupe.to_string_lossy() }],
                    "ancien@labo": { "installPath": ancien.to_string_lossy() },
                }
            })
            .to_string(),
        )
        .expect("plugins");
        fs::write(
            home.join("settings.json"),
            r#"{"enabledPlugins": {"outils@labo": true, "coupe@labo": true, "ancien@labo": true}}"#,
        )
        .expect("réglages");
        // Le projet désactive un plugin activé pour l'utilisateur.
        fs::create_dir_all(cwd.join(".claude")).expect("projet");
        fs::write(
            cwd.join(".claude").join("settings.json"),
            r#"{"enabledPlugins": {"coupe@labo": false}}"#,
        )
        .expect("réglages du projet");

        let skills = collect(&home, &cwd, &[]);
        let names: Vec<&str> = skills.iter().map(|skill| skill.name.as_str()).collect();
        assert_eq!(names, ["ancien:notes", "outils:revue"]);
        assert_eq!(
            skills[1].origin,
            ImportedOrigin::ClaudePlugin("outils".to_string())
        );
        // Le nom court suffit quand il n'y a pas d'ambiguïté.
        assert_eq!(
            find_in(&skills, "revue"),
            Some(actif.join("skills").join("revue").join("SKILL.md"))
        );
        assert_eq!(find_in(&skills, "outils:revue"), find_in(&skills, "revue"));
        let _ = fs::remove_dir_all(home);
        let _ = fs::remove_dir_all(cwd);
    }
}
