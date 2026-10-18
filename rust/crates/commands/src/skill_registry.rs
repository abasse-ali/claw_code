//! Skills installés, lus en entier (frontmatter compris) pour être invoqués :
//! dossiers `skills/` du projet (du plus proche au plus lointain) et de
//! l'utilisateur, puis skills importés de Claude (application, plugins).
//! Le premier skill trouvé pour un nom masque les suivants.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use runtime::skills::SkillFile;

use super::{discover_skill_roots, DefinitionSource, SkillOrigin};

/// Réglage `skillOverrides` : visibilité d'un skill sans toucher à son fichier.
pub const OVERRIDES_KEY: &str = "skillOverrides";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Visibility {
    /// Nom et description listés au modèle, présent dans le menu `/`.
    On,
    /// Seul le nom est listé au modèle.
    NameOnly,
    /// Caché au modèle, invocable par l'utilisateur.
    UserOnly,
    /// Caché partout.
    Off,
}

impl Visibility {
    fn parse(value: &str) -> Self {
        match value.trim() {
            "name-only" => Self::NameOnly,
            "user-invocable-only" | "user-only" => Self::UserOnly,
            "off" => Self::Off,
            _ => Self::On,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledSkill {
    pub file: SkillFile,
    /// Origine affichée (« projet .claude », « utilisateur », « application Claude »…).
    pub source: &'static str,
    /// Synchronisé depuis claude.ai : ses commandes injectées ne s'exécutent pas.
    pub synced: bool,
    pub visibility: Visibility,
}

impl InstalledSkill {
    /// Le modèle peut l'invoquer (outil `Skill`).
    #[must_use]
    pub fn model_invocable(&self) -> bool {
        !self.file.disable_model_invocation
            && matches!(self.visibility, Visibility::On | Visibility::NameOnly)
    }

    /// L'utilisateur peut l'invoquer (`/nom`).
    #[must_use]
    pub fn user_invocable(&self) -> bool {
        self.file.user_invocable && self.visibility != Visibility::Off
    }
}

/// Valeurs de `skillOverrides` des réglages fusionnés.
#[must_use]
pub fn overrides(cwd: &Path) -> BTreeMap<String, Visibility> {
    runtime::ConfigLoader::default_for(cwd)
        .load()
        .ok()
        .and_then(|config| {
            config.get(OVERRIDES_KEY).and_then(|value| {
                value.as_object().map(|entries| {
                    entries
                        .iter()
                        .filter_map(|(name, value)| {
                            value
                                .as_str()
                                .map(|value| (name.to_ascii_lowercase(), Visibility::parse(value)))
                        })
                        .collect()
                })
            })
        })
        .unwrap_or_default()
}

/// Tous les skills installés, sans doublon de nom.
#[must_use]
pub fn installed_skills(cwd: &Path) -> Vec<InstalledSkill> {
    let overrides = overrides(cwd);
    let mut seen = BTreeSet::new();
    let mut skills = Vec::new();
    let mut push =
        |file: SkillFile, source: &'static str, synced: bool, skills: &mut Vec<InstalledSkill>| {
            let key = file.name.to_ascii_lowercase();
            if !seen.insert(key.clone()) {
                return;
            }
            let visibility = overrides
                .get(&key)
                .or_else(|| overrides.get(&file.dir_name.to_ascii_lowercase()))
                .copied()
                .unwrap_or(Visibility::On);
            skills.push(InstalledSkill {
                file,
                source,
                synced,
                visibility,
            });
        };
    for root in discover_skill_roots(cwd) {
        if root.origin != SkillOrigin::SkillsDir {
            continue;
        }
        let Ok(entries) = std::fs::read_dir(&root.path) else {
            continue;
        };
        let mut dirs: Vec<_> = entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.join("SKILL.md").is_file())
            .collect();
        dirs.sort();
        for dir in dirs {
            let dir_name = dir
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            // `synced` est réservé aux skills téléchargés de claude.ai.
            if dir_name.eq_ignore_ascii_case("synced") {
                continue;
            }
            if let Ok(file) = SkillFile::load(&dir.join("SKILL.md"), &dir_name) {
                push(file, source_label(root.source), false, &mut skills);
            }
        }
    }
    for imported in runtime::claude_skills::imported_skills(cwd) {
        if imported.unavailable.is_some() {
            continue;
        }
        let synced = imported.origin == runtime::claude_skills::ImportedOrigin::ClaudeApp;
        if let Ok(mut file) = SkillFile::load(&imported.skill_file, &imported.name) {
            file.name.clone_from(&imported.name);
            push(
                file,
                if synced {
                    "application Claude"
                } else {
                    "plugin Claude Code"
                },
                synced,
                &mut skills,
            );
        }
    }
    skills
}

/// Skill installé qui répond à ce nom (nom de commande ou de dossier).
#[must_use]
pub fn find_installed_skill(cwd: &Path, name: &str) -> Option<InstalledSkill> {
    installed_skills(cwd)
        .into_iter()
        .find(|skill| skill.file.answers_to(name))
}

/// Noms `/nom` des skills que l'utilisateur peut invoquer (complétion, menu).
#[must_use]
pub fn user_invocable_names(cwd: &Path) -> Vec<(String, String)> {
    installed_skills(cwd)
        .into_iter()
        .filter(InstalledSkill::user_invocable)
        .map(|skill| {
            let description = skill.file.description.clone().unwrap_or_default();
            (skill.file.name, description)
        })
        .collect()
}

fn source_label(source: DefinitionSource) -> &'static str {
    match source {
        DefinitionSource::ProjectClaw => "projet .claw",
        DefinitionSource::ProjectClaude => "projet .claude",
        DefinitionSource::ProjectCodex => "projet .codex",
        DefinitionSource::UserClaw => "utilisateur ~/.claw",
        DefinitionSource::UserClaude => "utilisateur ~/.claude",
        DefinitionSource::UserCodex | DefinitionSource::UserCodexHome => "utilisateur codex",
        DefinitionSource::ClaudeApp => "application Claude",
        DefinitionSource::ClaudePlugin => "plugin Claude Code",
    }
}

#[cfg(test)]
mod tests {
    use super::{find_installed_skill, installed_skills, Visibility};
    use std::fs;

    #[test]
    fn project_skills_are_found_by_name_or_folder_with_their_overrides() {
        let root = std::env::temp_dir().join(format!(
            "claw-skill-registry-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|elapsed| elapsed.as_nanos())
                .unwrap_or_default()
        ));
        let skills = root.join(".claude").join("skills");
        fs::create_dir_all(skills.join("deploy-staging")).expect("dossier");
        fs::write(
            skills.join("deploy-staging").join("SKILL.md"),
            "---\nname: deploy\ndescription: Deploy\ndisable-model-invocation: true\n---\nDeploy $ARGUMENTS",
        )
        .expect("skill");
        fs::create_dir_all(skills.join("context")).expect("dossier");
        fs::write(
            skills.join("context").join("SKILL.md"),
            "---\ndescription: Legacy context\n---\nContext",
        )
        .expect("skill");
        fs::create_dir_all(root.join(".claude")).expect("claude");
        fs::write(
            root.join(".claude").join("settings.json"),
            r#"{"skillOverrides":{"context":"name-only"}}"#,
        )
        .expect("réglages");

        let deploy = find_installed_skill(&root, "/deploy").expect("par nom");
        assert_eq!(deploy.file.dir_name, "deploy-staging");
        assert!(!deploy.model_invocable() && deploy.user_invocable());
        assert!(find_installed_skill(&root, "deploy-staging").is_some());
        let context = installed_skills(&root)
            .into_iter()
            .find(|skill| skill.file.name == "context")
            .expect("context");
        assert_eq!(context.visibility, Visibility::NameOnly);
        assert!(context.model_invocable());
        fs::remove_dir_all(root).expect("nettoyage");
    }
}
