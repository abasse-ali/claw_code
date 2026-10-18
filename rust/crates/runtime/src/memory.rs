//! Mémoire du projet au format de Claude Code, en plus des fichiers
//! d'instructions (`CLAUDE.md`, `CLAW.md`, `AGENTS.md`…) :
//!
//! - **imports** `@chemin` : relatifs au fichier qui importe, absolus ou
//!   `~/…` ; un espace s'échappe avec `\ ` ; ignorés dans le code (`` `@x` ``,
//!   blocs) ; quatre niveaux au plus. Un import de fichier de projet qui sort
//!   du dossier de travail (un fichier personnel, par exemple) attend votre
//!   accord (`/memory imports allow`), pour qu'un fichier commité par
//!   d'autres ne fasse pas lire au modèle des fichiers de votre machine ;
//! - **commentaires HTML** de bloc (`<!-- … -->`) retirés hors du code ;
//! - **règles** `.claude/rules/**/*.md` et `.claw/rules/**/*.md` (projet et
//!   `~/.claude/rules`, `~/.claw/rules`) : sans `paths`, chargées au démarrage ;
//!   avec `paths` (globs, accolades acceptées), données au modèle la première
//!   fois qu'il lit ou modifie un fichier correspondant ;
//! - **sous-dossiers** : leurs `CLAUDE.md`, `CLAW.md`, `AGENTS.md` sont donnés
//!   au modèle la première fois qu'il touche un fichier de ce sous-dossier ;
//! - **`claudeMdExcludes`** : globs des fichiers d'instructions à ignorer.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::prompt::ContextFile;

const MAX_IMPORT_DEPTH: usize = 4;
const MAX_BRACE_PATTERNS: usize = 1000;

/// Noms des fichiers d'instructions d'un dossier, dans l'ordre de lecture.
pub const INSTRUCTION_NAMES: &[&str] = &[
    "CLAW.md",
    "CLAW.local.md",
    ".claw/CLAW.md",
    ".claw/instructions.md",
    "CLAUDE.md",
    "CLAUDE.local.md",
    ".claude/CLAUDE.md",
    "AGENTS.md",
];

/// Règle limitée à des chemins (`paths` du frontmatter).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathRule {
    pub path: PathBuf,
    /// Dossier par rapport auquel les motifs s'appliquent (celui qui contient
    /// `.claude/rules`, ou le dossier de travail pour une règle personnelle).
    pub base: PathBuf,
    pub patterns: Vec<String>,
    pub content: String,
}

#[derive(Debug, Default)]
struct OnDemand {
    cwd: PathBuf,
    rules: Vec<PathRule>,
    delivered: BTreeSet<PathBuf>,
    skipped_external: Vec<PathBuf>,
}

static ON_DEMAND: Mutex<Option<OnDemand>> = Mutex::new(None);

fn with_state<T>(action: impl FnOnce(&mut OnDemand) -> T) -> T {
    let mut guard = ON_DEMAND
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    action(guard.get_or_insert_with(OnDemand::default))
}

/// Règles limitées à des chemins (pour `/memory`).
#[must_use]
pub fn path_rules() -> Vec<PathRule> {
    with_state(|state| state.rules.clone())
}

/// Imports externes ignorés faute d'accord (pour `/memory`).
#[must_use]
pub fn skipped_external_imports() -> Vec<PathBuf> {
    with_state(|state| state.skipped_external.clone())
}

// ------------------------------------------------------------ chargement

/// Complète les fichiers d'instructions trouvés : exclusions, imports,
/// règles ; prépare les instructions à donner à la demande.
#[must_use]
pub fn assemble(
    cwd: &Path,
    home: Option<&Path>,
    files: Vec<(ContextFile, bool)>,
) -> Vec<ContextFile> {
    let excludes = configured_excludes(cwd);
    let approved = external_imports_approved(cwd).unwrap_or(false);
    let mut skipped = Vec::new();
    let mut output = Vec::new();
    let mut seen = BTreeSet::new();
    let mut path_rules = Vec::new();

    // Règles personnelles d'abord, comme dans Claude Code.
    let mut entries: Vec<(ContextFile, bool)> = Vec::new();
    if let Some(home) = home {
        for dir in [
            home.join(".claude").join("rules"),
            home.join(".claw").join("rules"),
        ] {
            for (path, content) in rule_files(&dir) {
                let (patterns, body) = parse_rule(&content);
                if patterns.is_empty() {
                    entries.push((
                        ContextFile {
                            path,
                            content: body,
                        },
                        true,
                    ));
                } else {
                    path_rules.push(PathRule {
                        path,
                        base: cwd.to_path_buf(),
                        patterns,
                        content: body,
                    });
                }
            }
        }
    }
    entries.extend(files);
    // Règles du projet : `.claude/rules` et `.claw/rules` des dossiers parents.
    for dir in cwd.ancestors().collect::<Vec<_>>().into_iter().rev() {
        for rules_dir in [
            dir.join(".claude").join("rules"),
            dir.join(".claw").join("rules"),
        ] {
            for (path, content) in rule_files(&rules_dir) {
                let (patterns, body) = parse_rule(&content);
                if patterns.is_empty() {
                    entries.push((
                        ContextFile {
                            path,
                            content: body,
                        },
                        false,
                    ));
                } else {
                    path_rules.push(PathRule {
                        path,
                        base: dir.to_path_buf(),
                        patterns,
                        content: body,
                    });
                }
            }
        }
    }

    for (file, user_scope) in entries {
        if is_excluded(&file.path, &excludes) {
            continue;
        }
        expand_file(
            &file,
            user_scope,
            cwd,
            approved,
            0,
            &mut seen,
            &mut output,
            &mut skipped,
        );
    }
    with_state(|state| {
        if state.cwd != cwd {
            state.delivered.clear();
        }
        state.cwd = cwd.to_path_buf();
        state.rules = path_rules;
        state.skipped_external = skipped;
        for file in &output {
            state.delivered.insert(file.path.clone());
        }
    });
    output
}

#[allow(clippy::too_many_arguments)]
fn expand_file(
    file: &ContextFile,
    user_scope: bool,
    cwd: &Path,
    approved: bool,
    depth: usize,
    seen: &mut BTreeSet<PathBuf>,
    output: &mut Vec<ContextFile>,
    skipped: &mut Vec<PathBuf>,
) {
    let key = file
        .path
        .canonicalize()
        .unwrap_or_else(|_| file.path.clone());
    if !seen.insert(key) {
        return;
    }
    let content = strip_html_comments(&file.content);
    let targets = import_targets(&content);
    let path = file.path.clone();
    output.push(ContextFile {
        path: path.clone(),
        content,
    });
    if depth >= MAX_IMPORT_DEPTH {
        return;
    }
    let base = path
        .parent()
        .map_or_else(|| cwd.to_path_buf(), Path::to_path_buf);
    for target in targets {
        let Some(resolved) = resolve_import(&target, &base) else {
            continue;
        };
        if !resolved.is_file() {
            continue;
        }
        if !user_scope && !approved && !inside(&resolved, cwd) {
            if !skipped.contains(&resolved) {
                skipped.push(resolved);
            }
            continue;
        }
        let Ok(text) = fs::read_to_string(&resolved) else {
            continue;
        };
        expand_file(
            &ContextFile {
                path: resolved,
                content: text,
            },
            user_scope,
            cwd,
            approved,
            depth + 1,
            seen,
            output,
            skipped,
        );
    }
}

fn inside(path: &Path, root: &Path) -> bool {
    let path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    path.starts_with(root)
}

fn resolve_import(target: &str, base: &Path) -> Option<PathBuf> {
    let target = target.replace("\\ ", " ");
    if let Some(rest) = target.strip_prefix("~/") {
        return crate::home_dir().map(|home| home.join(rest));
    }
    let path = PathBuf::from(target.replace('/', std::path::MAIN_SEPARATOR_STR));
    Some(if path.is_absolute() {
        path
    } else {
        base.join(path)
    })
}

/// Chemins importés par `@chemin`, hors du code (extraits entre accents graves
/// et blocs de code).
#[must_use]
pub fn import_targets(content: &str) -> Vec<String> {
    let mut targets = Vec::new();
    let mut in_fence = false;
    for line in content.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        let chars: Vec<char> = line.chars().collect();
        let mut in_code = false;
        let mut index = 0;
        while index < chars.len() {
            match chars[index] {
                '`' => in_code = !in_code,
                '@' if !in_code
                    && (index == 0
                        || chars[index - 1].is_whitespace()
                        || matches!(chars[index - 1], '(' | '[')) =>
                {
                    let mut path = String::new();
                    let mut cursor = index + 1;
                    while cursor < chars.len() {
                        let character = chars[cursor];
                        if character == '\\' && chars.get(cursor + 1) == Some(&' ') {
                            path.push_str("\\ ");
                            cursor += 2;
                            continue;
                        }
                        if character.is_whitespace()
                            || matches!(character, ')' | ']' | ',' | ';' | '"' | '\'' | '`')
                        {
                            break;
                        }
                        path.push(character);
                        cursor += 1;
                    }
                    let path = path.trim_end_matches(['.', ':']).to_string();
                    if !path.is_empty() {
                        targets.push(path);
                    }
                    index = cursor;
                    continue;
                }
                _ => {}
            }
            index += 1;
        }
    }
    targets
}

/// Retire les commentaires HTML de bloc hors du code.
#[must_use]
pub fn strip_html_comments(content: &str) -> String {
    let mut output = String::new();
    let mut in_fence = false;
    let mut in_comment = false;
    for line in content.split_inclusive('\n') {
        let trimmed = line.trim_start();
        if in_comment {
            if let Some(end) = line.find("-->") {
                in_comment = false;
                let rest = &line[end + 3..];
                if !rest.trim().is_empty() {
                    output.push_str(rest);
                }
            }
            continue;
        }
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_fence = !in_fence;
            output.push_str(line);
            continue;
        }
        if !in_fence && trimmed.starts_with("<!--") {
            match trimmed.find("-->") {
                Some(end) => {
                    let rest = &trimmed[end + 3..];
                    if !rest.trim().is_empty() {
                        output.push_str(rest);
                    }
                }
                None => in_comment = true,
            }
            continue;
        }
        output.push_str(line);
    }
    output
}

// ------------------------------------------------------------ règles

/// Fichiers `.md` d'un dossier de règles, récursivement (liens symboliques
/// suivis, boucles évitées), triés par chemin.
fn rule_files(dir: &Path) -> Vec<(PathBuf, String)> {
    let mut found = Vec::new();
    let mut visited = BTreeSet::new();
    collect_rules(dir, &mut visited, &mut found, 0);
    found.sort_by(|left, right| left.0.cmp(&right.0));
    found
}

fn collect_rules(
    dir: &Path,
    visited: &mut BTreeSet<PathBuf>,
    found: &mut Vec<(PathBuf, String)>,
    depth: usize,
) {
    if depth > 8 {
        return;
    }
    let Ok(canonical) = dir.canonicalize() else {
        return;
    };
    if !visited.insert(canonical) {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if path.is_dir() {
            collect_rules(&path, visited, found, depth + 1);
        } else if path
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("md"))
        {
            if let Ok(content) = fs::read_to_string(&path) {
                if !content.trim().is_empty() {
                    found.push((path, content));
                }
            }
        }
    }
}

/// `paths` du frontmatter (liste YAML ou chaîne séparée par des virgules) et
/// corps de la règle.
#[must_use]
pub fn parse_rule(content: &str) -> (Vec<String>, String) {
    let content = content.trim_start_matches('\u{feff}');
    let Some(rest) = content
        .strip_prefix("---\n")
        .or_else(|| content.strip_prefix("---\r\n"))
    else {
        return (Vec::new(), content.to_string());
    };
    let Some(end) = rest.find("\n---") else {
        return (Vec::new(), content.to_string());
    };
    let frontmatter = &rest[..end];
    let body = rest[end + 4..].trim_start_matches(['\r', '\n']).to_string();
    let mut patterns = Vec::new();
    let mut in_paths = false;
    for line in frontmatter.lines() {
        let trimmed = line.trim();
        if let Some(value) = trimmed.strip_prefix("paths:") {
            let value = value.trim();
            in_paths = value.is_empty();
            if !value.is_empty() {
                patterns.extend(
                    value
                        .trim_start_matches('[')
                        .trim_end_matches(']')
                        .split(',')
                        .map(|item| item.trim().trim_matches(['"', '\'']).to_string())
                        .filter(|item| !item.is_empty()),
                );
            }
            continue;
        }
        if in_paths {
            match trimmed.strip_prefix("- ") {
                Some(item) => patterns.push(item.trim().trim_matches(['"', '\'']).to_string()),
                None if trimmed.is_empty() => {}
                None => in_paths = false,
            }
        }
    }
    (patterns, body)
}

/// Développe les accolades (`*.{ts,tsx}`), dans la limite du budget.
#[must_use]
pub fn expand_braces(pattern: &str) -> Vec<String> {
    let mut results = vec![pattern.to_string()];
    loop {
        let mut next = Vec::new();
        let mut expanded = false;
        for item in &results {
            let Some(open) = item.find('{') else {
                next.push(item.clone());
                continue;
            };
            let Some(close) = item[open..].find('}').map(|offset| open + offset) else {
                next.push(item.clone());
                continue;
            };
            expanded = true;
            for choice in item[open + 1..close].split(',') {
                next.push(format!("{}{choice}{}", &item[..open], &item[close + 1..]));
            }
        }
        if next.len() > MAX_BRACE_PATTERNS {
            return vec![pattern.to_string()];
        }
        results = next;
        if !expanded {
            return results;
        }
    }
}

fn pattern_matches(pattern: &str, relative: &str) -> bool {
    let options = glob::MatchOptions {
        case_sensitive: !cfg!(windows),
        require_literal_separator: true,
        require_literal_leading_dot: false,
    };
    expand_braces(pattern).iter().any(|expanded| {
        glob::Pattern::new(expanded).is_ok_and(|compiled| compiled.matches_with(relative, options))
    })
}

/// La règle s'applique à ce fichier (chemin absolu ou relatif au dossier de travail).
#[must_use]
pub fn rule_matches(rule: &PathRule, file: &Path, cwd: &Path) -> bool {
    let absolute = if file.is_absolute() {
        file.to_path_buf()
    } else {
        cwd.join(file)
    };
    let Ok(relative) = absolute.strip_prefix(&rule.base) else {
        return false;
    };
    let relative = relative.display().to_string().replace('\\', "/");
    rule.patterns
        .iter()
        .any(|pattern| pattern_matches(pattern, &relative))
}

// ------------------------------------------------------------ à la demande

/// Instructions à donner au modèle maintenant qu'il lit ou modifie `file` :
/// règles dont un motif correspond et fichiers d'instructions des
/// sous-dossiers traversés, chacun une seule fois.
#[must_use]
pub fn instructions_for(file: &str) -> Vec<String> {
    let cwd = std::env::current_dir().unwrap_or_default();
    with_state(|state| {
        let base = if state.cwd.as_os_str().is_empty() {
            cwd.clone()
        } else {
            state.cwd.clone()
        };
        let path = PathBuf::from(file);
        let absolute = if path.is_absolute() {
            path
        } else {
            base.join(path)
        };
        let mut notes = Vec::new();
        for rule in &state.rules {
            if state.delivered.contains(&rule.path) || !rule_matches(rule, &absolute, &base) {
                continue;
            }
            notes.push(format!(
                "[Claw] Project rule for files matching {} ({}):\n{}",
                rule.patterns.join(", "),
                rule.path.display(),
                rule.content.trim()
            ));
        }
        let matched: Vec<PathBuf> = state
            .rules
            .iter()
            .filter(|rule| {
                !state.delivered.contains(&rule.path) && rule_matches(rule, &absolute, &base)
            })
            .map(|rule| rule.path.clone())
            .collect();
        state.delivered.extend(matched);
        // Sous-dossiers entre le dossier de travail et le fichier.
        if let Ok(relative) = absolute.strip_prefix(&base) {
            let mut dir = base.clone();
            let components: Vec<_> = relative.components().collect();
            for component in components.iter().take(components.len().saturating_sub(1)) {
                dir.push(component);
                for name in INSTRUCTION_NAMES {
                    let candidate = dir.join(name);
                    if state.delivered.contains(&candidate) || !candidate.is_file() {
                        continue;
                    }
                    state.delivered.insert(candidate.clone());
                    if let Ok(text) = fs::read_to_string(&candidate) {
                        let text = strip_html_comments(&text);
                        if !text.trim().is_empty() {
                            notes.push(format!(
                                "[Claw] Instructions for {} ({}):\n{}",
                                dir.display(),
                                candidate.display(),
                                text.trim()
                            ));
                        }
                    }
                }
            }
        }
        notes
    })
}

// ------------------------------------------------------------ réglages

fn configured_excludes(cwd: &Path) -> Vec<String> {
    crate::ConfigLoader::default_for(cwd)
        .load()
        .ok()
        .and_then(|config| {
            config.get("claudeMdExcludes").and_then(|value| {
                value.as_array().map(|items| {
                    items
                        .iter()
                        .filter_map(|item| item.as_str().map(ToOwned::to_owned))
                        .collect()
                })
            })
        })
        .unwrap_or_default()
}

fn is_excluded(path: &Path, excludes: &[String]) -> bool {
    let text = path.display().to_string().replace('\\', "/");
    excludes.iter().any(|pattern| {
        let pattern = pattern.replace('\\', "/");
        expand_braces(&pattern).iter().any(|expanded| {
            glob::Pattern::new(expanded).is_ok_and(|compiled| {
                compiled.matches_with(
                    &text,
                    glob::MatchOptions {
                        case_sensitive: !cfg!(windows),
                        require_literal_separator: false,
                        require_literal_leading_dot: false,
                    },
                )
            })
        })
    })
}

fn approvals_path() -> Option<PathBuf> {
    let cwd = std::env::current_dir().ok()?;
    Some(
        crate::ConfigLoader::default_for(cwd)
            .config_home()
            .join("memory-imports.json"),
    )
}

/// Accord pour les imports externes de ce projet (`None` : pas encore décidé).
#[must_use]
pub fn external_imports_approved(cwd: &Path) -> Option<bool> {
    let text = fs::read_to_string(approvals_path()?).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    value.get(project_key(cwd))?.as_bool()
}

/// Enregistre l'accord (ou le refus) pour les imports externes de ce projet.
pub fn set_external_imports(cwd: &Path, allowed: bool) -> Result<PathBuf, String> {
    let path =
        approvals_path().ok_or_else(|| "dossier de configuration introuvable".to_string())?;
    let mut value = fs::read_to_string(&path)
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .filter(serde_json::Value::is_object)
        .unwrap_or_else(|| serde_json::json!({}));
    value[project_key(cwd)] = serde_json::Value::Bool(allowed);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let text = serde_json::to_string_pretty(&value).map_err(|error| error.to_string())?;
    fs::write(&path, format!("{text}\n")).map_err(|error| error.to_string())?;
    Ok(path)
}

fn project_key(cwd: &Path) -> String {
    crate::worktree::repo_root(cwd)
        .unwrap_or_else(|_| cwd.to_path_buf())
        .display()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::{
        assemble, expand_braces, import_targets, instructions_for, parse_rule, strip_html_comments,
    };
    use crate::prompt::ContextFile;
    use std::fs;
    use std::path::PathBuf;

    fn temp(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "claw-memory-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|elapsed| elapsed.as_nanos())
                .unwrap_or_default()
        ));
        fs::create_dir_all(&dir).expect("dossier");
        dir
    }

    #[test]
    fn imports_are_found_outside_code_only() {
        let content = "See @README for the overview and @package.json.\n- git @docs/git.md\n- design @Design\\ Docs/api.md\nMail me at a@b.c, or `@not/this`.\n```\n@inside/fence.md\n```\n(@paren.md)";
        assert_eq!(
            import_targets(content),
            [
                "README",
                "package.json",
                "docs/git.md",
                "Design\\ Docs/api.md",
                "paren.md"
            ]
        );
    }

    #[test]
    fn html_comments_are_stripped_outside_code() {
        let text = "Keep\n<!-- maintainer note\nspanning lines -->\nAlso keep\n```\n<!-- in code -->\n```\n<!-- one line --> after\n";
        assert_eq!(
            strip_html_comments(text),
            "Keep\nAlso keep\n```\n<!-- in code -->\n```\n after\n"
        );
    }

    #[test]
    fn rules_read_paths_and_expand_braces() {
        let (patterns, body) = parse_rule("---\npaths:\n  - \"src/**/*.{ts,tsx}\"\n  - lib/*.ts\n---\n\n# API\n- validate input\n");
        assert_eq!(patterns, ["src/**/*.{ts,tsx}", "lib/*.ts"]);
        assert_eq!(body, "# API\n- validate input\n");
        assert_eq!(parse_rule("---\npaths: a/*, b/*\n---\nx").0, ["a/*", "b/*"]);
        assert_eq!(expand_braces("src/*.{ts,tsx}"), ["src/*.ts", "src/*.tsx"]);
        assert_eq!(expand_braces("{a,b}/{c,d}").len(), 4);
    }

    #[test]
    fn memory_is_assembled_with_imports_rules_and_on_demand_instructions() {
        let _lock = crate::test_env_lock();
        let project = temp("projet");
        let outside = temp("dehors");
        fs::create_dir_all(project.join("docs")).expect("docs");
        fs::write(project.join("docs").join("git.md"), "Use rebase. @deep.md").expect("git");
        fs::write(project.join("docs").join("deep.md"), "Deep import").expect("deep");
        fs::write(outside.join("secret.md"), "PERSONAL").expect("secret");
        let claude = format!(
            "Main\n<!-- hidden block\n still hidden -->\n@docs/git.md\n@{}\n",
            outside
                .join("secret.md")
                .display()
                .to_string()
                .replace('\\', "/")
        );
        fs::write(project.join("CLAUDE.md"), &claude).expect("claude");
        fs::create_dir_all(project.join(".claude").join("rules").join("api")).expect("rules");
        fs::write(
            project.join(".claude").join("rules").join("style.md"),
            "Always use tabs",
        )
        .expect("style");
        fs::write(
            project
                .join(".claude")
                .join("rules")
                .join("api")
                .join("api.md"),
            "---\npaths:\n  - \"src/api/**/*.{ts,tsx}\"\n---\nValidate API input",
        )
        .expect("api");
        fs::create_dir_all(project.join("src").join("api")).expect("src");
        fs::write(project.join("src").join("CLAUDE.md"), "Src rules").expect("src claude");

        let files = assemble(
            &project,
            None,
            vec![(
                ContextFile {
                    path: project.join("CLAUDE.md"),
                    content: claude,
                },
                false,
            )],
        );
        let names: Vec<String> = files
            .iter()
            .map(|file| {
                file.path
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        assert_eq!(names, ["CLAUDE.md", "git.md", "deep.md", "style.md"]);
        assert!(!files[0].content.contains("hidden"));
        assert!(files.iter().all(|file| !file.content.contains("PERSONAL")));
        assert_eq!(super::skipped_external_imports().len(), 1);

        let previous = std::env::current_dir().expect("cwd");
        std::env::set_current_dir(&project).expect("cd");
        let notes = instructions_for("src/api/users.ts");
        assert_eq!(notes.len(), 2, "{notes:?}");
        assert!(notes.iter().any(|note| note.contains("Validate API input")));
        assert!(notes.iter().any(|note| note.contains("Src rules")));
        assert!(instructions_for("src/api/other.tsx").is_empty());
        assert!(instructions_for("README.md").is_empty());
        std::env::set_current_dir(previous).expect("cd");
        let _ = fs::remove_dir_all(project);
        let _ = fs::remove_dir_all(outside);
    }
}
