//! Commandes personnalisées : un fichier Markdown par commande dans
//! `.claw/commands/`, `.claude/commands/` ou `.codex/commands/` (projet, du plus
//! proche au plus lointain, puis `~/.claw`, `~/.claude`, `~/.codex`).
//!
//! `revue.md` devient `/revue` ; `front/composant.md` devient `/composant` et
//! `/front:composant`. Le contenu (sans l'en-tête YAML) est envoyé comme
//! demande, avec `$ARGUMENTS` (tous les arguments) et `$1`…`$9` remplacés ;
//! sans `$ARGUMENTS`, les arguments sont ajoutés à la fin. En-tête reconnu :
//! `description`, `argument-hint`, `model` (modèle du tour) et `allowed-tools`
//! (outils autorisés ; `Bash` permet aussi les lignes `` !`commande` ``, dont la
//! sortie est insérée dans la demande).

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

const COMMAND_DIRS: &[&str] = &[".claw", ".claude", ".codex"];
const MAX_DEPTH: usize = 3;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CustomCommand {
    /// Nom simple (`composant`).
    pub name: String,
    /// Sous-dossiers éventuels (`front`), pour la forme `/front:composant`.
    pub namespace: Option<String>,
    pub description: Option<String>,
    pub argument_hint: Option<String>,
    /// Modèle à utiliser pour ce tour (`model` dans l'en-tête).
    pub model: Option<String>,
    /// Outils autorisés pour ce tour (`allowed-tools`), vide = tous.
    pub allowed_tools: Vec<String>,
    pub path: PathBuf,
    /// « projet » ou « utilisateur ».
    pub scope: &'static str,
}

impl CustomCommand {
    /// Nom complet : `front:composant`, ou `composant` sans sous-dossier.
    #[must_use]
    pub fn qualified_name(&self) -> String {
        match &self.namespace {
            Some(namespace) => format!("{namespace}:{}", self.name),
            None => self.name.clone(),
        }
    }

    /// Demande à envoyer au modèle pour ces arguments.
    pub fn expand(&self, arguments: &str) -> Result<String, String> {
        let contents = fs::read_to_string(&self.path)
            .map_err(|error| format!("lecture de {} impossible : {error}", self.path.display()))?;
        let expanded = expand_template(strip_frontmatter(&contents), arguments);
        let cwd = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        Ok(run_inline_commands(&expanded, self.allows_bash(), &cwd))
    }

    /// `allowed-tools` autorise `Bash` (condition des lignes `` !`commande` ``).
    #[must_use]
    pub fn allows_bash(&self) -> bool {
        self.allowed_tools
            .iter()
            .any(|tool| tool.trim().to_ascii_lowercase().starts_with("bash"))
    }
}

/// Remplace chaque `` !`commande` `` par la sortie de la commande, exécutée
/// dans le projet (seulement si `allowed-tools` autorise Bash).
fn run_inline_commands(text: &str, allowed: bool, cwd: &Path) -> String {
    let mut output = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("!`") {
        let after = &rest[start + 2..];
        let Some(end) = after.find('`') else {
            break;
        };
        output.push_str(&rest[..start]);
        let command = &after[..end];
        if allowed {
            let result = plugins::posix_shell_command(command)
                .current_dir(cwd)
                .stdin(std::process::Stdio::null())
                .output();
            match result {
                Ok(result) => {
                    let stdout = String::from_utf8_lossy(&result.stdout)
                        .trim_end()
                        .to_string();
                    let stderr = String::from_utf8_lossy(&result.stderr)
                        .trim_end()
                        .to_string();
                    output.push_str(if stdout.is_empty() { &stderr } else { &stdout });
                }
                Err(error) => {
                    output.push_str(&format!("(commande `{command}` impossible : {error})"));
                }
            }
        } else {
            output.push_str(&format!(
                "!`{command}` (non exécutée : ajoutez Bash à allowed-tools dans l'en-tête de la commande)"
            ));
        }
        rest = &after[end + 1..];
    }
    output.push_str(rest);
    output
}

/// `Bash(git add:*), Read, Edit` → éléments séparés (virgules hors parenthèses).
fn split_tool_list(value: &str) -> Vec<String> {
    let mut items = Vec::new();
    let mut depth = 0_i32;
    let mut current = String::new();
    for c in value.trim().trim_matches(['[', ']']).chars() {
        match c {
            '(' => {
                depth += 1;
                current.push(c);
            }
            ')' => {
                depth -= 1;
                current.push(c);
            }
            ',' if depth == 0 => {
                items.push(std::mem::take(&mut current));
            }
            c => current.push(c),
        }
    }
    items.push(current);
    items
        .into_iter()
        .map(|item| item.trim().trim_matches(['"', '\'']).to_string())
        .filter(|item| !item.is_empty())
        .collect()
}

/// Une commande personnalisée invoquée et la demande qu'elle produit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CustomCommandInvocation {
    pub command: CustomCommand,
    pub arguments: String,
    pub prompt: String,
}

/// Dossiers de commandes, du plus prioritaire au moins prioritaire.
fn command_roots(cwd: &Path) -> Vec<(PathBuf, &'static str)> {
    let mut roots: Vec<(PathBuf, &'static str)> = Vec::new();
    let mut push = |path: PathBuf, scope: &'static str| {
        if path.is_dir() && !roots.iter().any(|(existing, _)| existing == &path) {
            roots.push((path, scope));
        }
    };
    let home = runtime::home_dir();
    for ancestor in cwd.ancestors() {
        // Le dossier personnel est traité comme portée « utilisateur ».
        if home.as_deref() == Some(ancestor) {
            continue;
        }
        for dir in COMMAND_DIRS {
            push(ancestor.join(dir).join("commands"), "projet");
        }
    }
    if let Some(config_home) = env::var_os("CLAW_CONFIG_HOME") {
        push(PathBuf::from(config_home).join("commands"), "utilisateur");
    }
    if let Some(home) = home {
        for dir in COMMAND_DIRS {
            push(home.join(dir).join("commands"), "utilisateur");
        }
    }
    if let Some(codex_home) = env::var_os("CODEX_HOME") {
        push(PathBuf::from(codex_home).join("commands"), "utilisateur");
    }
    roots
}

/// Toutes les commandes visibles depuis `cwd` ; une commande d'un dossier
/// prioritaire masque celles de même nom complet plus loin.
#[must_use]
pub fn discover_custom_commands(cwd: &Path) -> Vec<CustomCommand> {
    let mut commands: Vec<CustomCommand> = Vec::new();
    for (root, scope) in command_roots(cwd) {
        let mut found = Vec::new();
        collect_markdown(&root, &root, 0, scope, &mut found);
        found.sort_by_key(CustomCommand::qualified_name);
        for command in found {
            if !commands
                .iter()
                .any(|existing| existing.qualified_name() == command.qualified_name())
            {
                commands.push(command);
            }
        }
    }
    commands
}

fn collect_markdown(
    root: &Path,
    dir: &Path,
    depth: usize,
    scope: &'static str,
    found: &mut Vec<CustomCommand>,
) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            // Un dossier avec SKILL.md est une compétence, pas une commande.
            if depth < MAX_DEPTH && !path.join("SKILL.md").is_file() {
                collect_markdown(root, &path, depth + 1, scope, found);
            }
            continue;
        }
        let is_markdown = path
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("md"));
        let Some(name) = path.file_stem().and_then(|stem| stem.to_str()) else {
            continue;
        };
        if !is_markdown || !is_valid_name(name) {
            continue;
        }
        let namespace = path
            .parent()
            .and_then(|parent| parent.strip_prefix(root).ok())
            .map(|relative| {
                relative
                    .components()
                    .map(|component| component.as_os_str().to_string_lossy().to_string())
                    .collect::<Vec<_>>()
                    .join(":")
            })
            .filter(|namespace| !namespace.is_empty());
        let contents = fs::read_to_string(&path).unwrap_or_default();
        found.push(CustomCommand {
            name: name.to_string(),
            namespace,
            description: frontmatter_value(&contents, "description")
                .or_else(|| first_body_line(&contents)),
            argument_hint: frontmatter_value(&contents, "argument-hint"),
            model: frontmatter_value(&contents, "model").filter(|model| model != "inherit"),
            allowed_tools: frontmatter_value(&contents, "allowed-tools")
                .map(|value| split_tool_list(&value))
                .unwrap_or_default(),
            path,
            scope,
        });
    }
}

fn is_valid_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

/// Cherche la commande `name` (`composant` ou `front:composant`).
#[must_use]
pub fn find_custom_command(cwd: &Path, name: &str) -> Option<CustomCommand> {
    let commands = discover_custom_commands(cwd);
    let exact = |command: &&CustomCommand| {
        command.qualified_name() == name || (!name.contains(':') && command.name == name)
    };
    let loose = |command: &&CustomCommand| {
        command.qualified_name().eq_ignore_ascii_case(name)
            || (!name.contains(':') && command.name.eq_ignore_ascii_case(name))
    };
    commands
        .iter()
        .find(exact)
        .or_else(|| commands.iter().find(loose))
        .cloned()
}

/// `/nom arguments` → commande personnalisée et demande développée, ou `None`
/// si ce n'est pas une commande personnalisée connue.
pub fn resolve_custom_command(
    cwd: &Path,
    input: &str,
) -> Option<Result<CustomCommandInvocation, String>> {
    let rest = input.trim_start().strip_prefix('/')?;
    let (name, arguments) = rest
        .split_once(char::is_whitespace)
        .map_or((rest, ""), |(name, arguments)| (name, arguments.trim()));
    let command = find_custom_command(cwd, name)?;
    Some(
        command
            .expand(arguments)
            .map(|prompt| CustomCommandInvocation {
                command,
                arguments: arguments.to_string(),
                prompt,
            }),
    )
}

/// Section d'aide listant les commandes personnalisées.
#[must_use]
pub fn render_custom_commands_help(cwd: &Path) -> String {
    let commands = discover_custom_commands(cwd);
    let mut lines = vec!["Commandes personnalisées".to_string()];
    if commands.is_empty() {
        lines.push(
            "  Aucune. Créez .claw/commands/<nom>.md (texte de la demande, $ARGUMENTS, $1…$9) puis tapez /<nom>."
                .to_string(),
        );
        return lines.join("\n");
    }
    for command in &commands {
        let usage = match &command.argument_hint {
            Some(hint) => format!("/{} {hint}", command.qualified_name()),
            None => format!("/{}", command.qualified_name()),
        };
        let description = command.description.as_deref().unwrap_or("");
        lines.push(format!("  {usage:<28} {description} ({})", command.scope));
    }
    lines.join("\n")
}

/// Noms pour la complétion (`/nom` et `/dossier:nom`).
#[must_use]
pub fn custom_command_completions(cwd: &Path) -> Vec<String> {
    let mut names = Vec::new();
    for command in discover_custom_commands(cwd) {
        for name in [
            format!("/{}", command.name),
            format!("/{}", command.qualified_name()),
        ] {
            if !names.contains(&name) {
                names.push(name);
            }
        }
    }
    names
}

fn strip_frontmatter(contents: &str) -> &str {
    let trimmed = contents.trim_start_matches('\u{feff}');
    let Some(rest) = trimmed
        .strip_prefix("---\n")
        .or_else(|| trimmed.strip_prefix("---\r\n"))
    else {
        return trimmed;
    };
    let mut offset = 0;
    for line in rest.split_inclusive('\n') {
        offset += line.len();
        if line.trim() == "---" {
            return &rest[offset..];
        }
    }
    trimmed
}

fn frontmatter_value(contents: &str, key: &str) -> Option<String> {
    let mut lines = contents.trim_start_matches('\u{feff}').lines();
    if lines.next().map(str::trim) != Some("---") {
        return None;
    }
    let prefix = format!("{key}:");
    lines
        .take_while(|line| line.trim() != "---")
        .find_map(|line| line.trim().strip_prefix(&prefix).map(str::trim))
        .map(|value| {
            value
                .trim_matches(|c| c == '"' || c == '\'')
                .trim()
                .to_string()
        })
        .filter(|value| !value.is_empty())
}

fn first_body_line(contents: &str) -> Option<String> {
    strip_frontmatter(contents)
        .lines()
        .map(|line| line.trim().trim_start_matches('#').trim())
        .find(|line| !line.is_empty())
        .map(|line| line.chars().take(80).collect())
}

/// Remplace `$ARGUMENTS` et `$1`…`$9` ; ajoute les arguments à la fin s'ils
/// ne sont référencés nulle part.
#[must_use]
pub fn expand_template(template: &str, arguments: &str) -> String {
    let arguments = arguments.trim();
    let positional = split_arguments(arguments);
    let mut output = String::with_capacity(template.len() + arguments.len());
    let mut used = false;
    let mut rest = template;
    while let Some(index) = rest.find('$') {
        output.push_str(&rest[..index]);
        let after = &rest[index + 1..];
        if let Some(tail) = after.strip_prefix("ARGUMENTS") {
            output.push_str(arguments);
            used = true;
            rest = tail;
            continue;
        }
        let mut chars = after.chars();
        if let (Some(digit @ '1'..='9'), next) = (chars.next(), chars.next()) {
            if !next.is_some_and(|c| c.is_ascii_digit()) {
                let position = digit as usize - '1' as usize;
                output.push_str(positional.get(position).map_or("", String::as_str));
                used = true;
                rest = &after[1..];
                continue;
            }
        }
        output.push('$');
        rest = after;
    }
    output.push_str(rest);
    let mut output = output.trim().to_string();
    if !used && !arguments.is_empty() {
        output.push_str("\n\nARGUMENTS: ");
        output.push_str(arguments);
    }
    output
}

/// Découpe façon shell : espaces, avec "guillemets" ou 'apostrophes'.
fn split_arguments(arguments: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut quote = None;
    let mut started = false;
    for c in arguments.chars() {
        match (quote, c) {
            (Some(open), c) if c == open => quote = None,
            (Some(_), c) => current.push(c),
            (None, '"' | '\'') => {
                quote = Some(c);
                started = true;
            }
            (None, c) if c.is_whitespace() => {
                if started {
                    parts.push(std::mem::take(&mut current));
                    started = false;
                }
            }
            (None, c) => {
                current.push(c);
                started = true;
            }
        }
    }
    if started {
        parts.push(current);
    }
    parts
}

#[cfg(test)]
mod tests {
    use super::{
        discover_custom_commands, expand_template, render_custom_commands_help,
        resolve_custom_command, run_inline_commands, split_arguments, split_tool_list,
    };

    #[test]
    fn parses_tool_lists_and_runs_inline_commands_only_when_allowed() {
        assert_eq!(
            split_tool_list("Bash(git add:*), Bash(git status:*), Read"),
            vec!["Bash(git add:*)", "Bash(git status:*)", "Read"]
        );
        let cwd = std::env::temp_dir();
        assert_eq!(
            run_inline_commands("Statut : !`echo propre`.", true, &cwd),
            "Statut : propre."
        );
        assert!(
            run_inline_commands("Statut : !`echo propre`.", false, &cwd).contains("non exécutée")
        );
    }
    use std::fs;

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("claw-custom-{name}-{nanos}"));
        fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    #[test]
    fn expands_arguments_and_positionals() {
        assert_eq!(
            expand_template(
                "Corrige l'issue #$1 en priorité $2 ($ARGUMENTS)",
                "123 haute"
            ),
            "Corrige l'issue #123 en priorité haute (123 haute)"
        );
        assert_eq!(expand_template("Prix : $5 et $10", "a"), "Prix :  et $10");
        assert_eq!(
            expand_template("Fais une revue.", "src/lib.rs"),
            "Fais une revue.\n\nARGUMENTS: src/lib.rs"
        );
        assert_eq!(expand_template("Fais une revue.", ""), "Fais une revue.");
        assert_eq!(
            split_arguments(r#"un "deux trois" 'quatre' """#),
            vec!["un", "deux trois", "quatre", ""]
        );
    }

    #[test]
    fn discovers_project_commands_with_namespaces_and_frontmatter() {
        let workspace = temp_dir("workspace");
        let nested = workspace.join("app");
        let commands = workspace.join(".claw").join("commands");
        fs::create_dir_all(commands.join("front")).expect("commands dir");
        fs::create_dir_all(nested.join(".claude").join("commands")).expect("nested dir");
        fs::write(
            commands.join("revue.md"),
            "---\ndescription: Revue ciblée\nargument-hint: <fichier>\n---\nFais la revue de $ARGUMENTS.\n",
        )
        .expect("revue");
        fs::write(
            commands.join("front").join("composant.md"),
            "# Crée un composant $1\n",
        )
        .expect("composant");
        // Le dossier le plus proche l'emporte sur le même nom plus haut.
        fs::write(
            nested.join(".claude").join("commands").join("revue.md"),
            "Revue locale de $1",
        )
        .expect("override");

        // Seules les commandes du projet : celles de ~/.claw/commands peuvent exister.
        let found = discover_custom_commands(&workspace)
            .into_iter()
            .filter(|command| command.scope == "projet")
            .collect::<Vec<_>>();
        let names = found
            .iter()
            .map(super::CustomCommand::qualified_name)
            .collect::<Vec<_>>();
        assert_eq!(names, vec!["front:composant", "revue"]);
        assert_eq!(found[1].description.as_deref(), Some("Revue ciblée"));
        assert_eq!(
            found[0].description.as_deref(),
            Some("Crée un composant $1")
        );

        let invocation = resolve_custom_command(&workspace, "/revue src/main.rs")
            .expect("connue")
            .expect("lue");
        assert_eq!(invocation.prompt, "Fais la revue de src/main.rs.");
        let invocation = resolve_custom_command(&workspace, "/front:composant Bouton")
            .expect("connue")
            .expect("lue");
        assert_eq!(invocation.prompt, "# Crée un composant Bouton");
        assert!(resolve_custom_command(&workspace, "/composant Carte").is_some());
        assert!(resolve_custom_command(&workspace, "/inconnue").is_none());

        let local = resolve_custom_command(&nested, "/revue a.rs")
            .expect("connue")
            .expect("lue");
        assert_eq!(local.prompt, "Revue locale de a.rs");

        let help = render_custom_commands_help(&workspace);
        assert!(help.contains("/revue <fichier>"), "{help}");
        assert!(help.contains("(projet)"));
        let _ = fs::remove_dir_all(workspace);
    }
}
