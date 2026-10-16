//! Skills au format de Claude Code (`SKILL.md`) : lecture du frontmatter et
//! rendu du contenu envoyé au modèle, que le skill soit invoqué par
//! l'utilisateur (`/nom arguments`) ou par le modèle (outil `Skill`).
//!
//! Rendu, dans l'ordre :
//! 1. les commandes injectées (`` !`commande` `` en début de ligne ou après un
//!    blanc, et les blocs ` ```! `) sont repérées dans le texte d'origine :
//!    un argument ne peut donc pas en créer ;
//! 2. les arguments sont substitués (`$ARGUMENTS`, `$ARGUMENTS[N]`, `$N` à
//!    partir de 0, les noms déclarés dans `arguments`), insérés tels quels ;
//!    sans aucun emplacement, ils sont ajoutés en `ARGUMENTS: …` ;
//! 3. les variables `${CLAUDE_SESSION_ID}`, `${CLAUDE_EFFORT}`,
//!    `${CLAUDE_SKILL_DIR}`, `${CLAUDE_PROJECT_DIR}` (et leurs formes
//!    `${CLAW_…}`) sont remplacées ;
//! 4. chaque commande injectée passe le contrôle des permissions (jamais de
//!    demande : refus = invocation annulée), puis s'exécute ; sa sortie
//!    remplace l'emplacement. Une commande en échec annule l'invocation.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

/// Texte qui remplace une commande injectée quand l'exécution est coupée.
pub const SHELL_DISABLED: &str = "[shell command execution disabled by policy]";

const COMMAND_TIMEOUT: Duration = Duration::from_secs(120);

/// Skill lu depuis son `SKILL.md` (ou un fichier de `commands/`). Les
/// booléens reprennent un à un les champs du frontmatter de Claude Code.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[allow(clippy::struct_excessive_bools)]
pub struct SkillFile {
    /// Nom de commande : `name` du frontmatter, sinon le nom du dossier.
    pub name: String,
    /// Nom du dossier (ou du fichier sans extension), qui invoque aussi le skill.
    pub dir_name: String,
    pub path: PathBuf,
    pub description: Option<String>,
    pub when_to_use: Option<String>,
    pub argument_hint: Option<String>,
    /// Arguments nommés (`arguments`), dans l'ordre des positions.
    pub arguments: Vec<String>,
    pub disable_model_invocation: bool,
    pub user_invocable: bool,
    /// Outils accordés sans demande pendant le tour (`allowed-tools`).
    pub allowed_tools: Vec<String>,
    /// Outils retirés pendant le tour (`disallowed-tools`).
    pub disallowed_tools: Vec<String>,
    pub model: Option<String>,
    pub effort: Option<String>,
    /// `context: fork` : exécuté par un sous-agent.
    pub fork: bool,
    /// Type de sous-agent pour `context: fork` (`agent`).
    pub agent: Option<String>,
    /// Motifs de fichiers qui limitent le chargement automatique (`paths`).
    pub paths: Vec<String>,
    /// `shell: powershell` pour les commandes injectées.
    pub powershell: bool,
    body: String,
}

impl SkillFile {
    /// Lit `path` ; `dir_name` est le nom du dossier du skill (ou du fichier).
    pub fn load(path: &Path, dir_name: &str) -> std::io::Result<Self> {
        let contents = std::fs::read_to_string(path)?;
        Ok(Self::parse(&contents, path, dir_name))
    }

    /// Analyse le contenu d'un `SKILL.md`.
    #[must_use]
    pub fn parse(contents: &str, path: &Path, dir_name: &str) -> Self {
        let contents = contents.trim_start_matches('\u{feff}');
        let (frontmatter, body) = split_frontmatter(contents);
        let field = |key: &str| frontmatter_value(frontmatter, key);
        let list = |key: &str| frontmatter_list(frontmatter, key);
        let flag = |key: &str, default: bool| {
            field(key).map_or(default, |value| parse_bool(&value).unwrap_or(default))
        };
        let description = field("description").or_else(|| {
            body.lines()
                .map(str::trim)
                .find(|line| !line.is_empty())
                .map(|line| line.trim_start_matches('#').trim().to_string())
        });
        let name = field("name")
            .filter(|name| !name.trim().is_empty())
            .unwrap_or_else(|| dir_name.to_string());
        Self {
            name,
            dir_name: dir_name.to_string(),
            path: path.to_path_buf(),
            description,
            when_to_use: field("when_to_use"),
            argument_hint: field("argument-hint"),
            arguments: list("arguments"),
            disable_model_invocation: flag("disable-model-invocation", false),
            user_invocable: flag("user-invocable", true),
            allowed_tools: list("allowed-tools"),
            disallowed_tools: list("disallowed-tools"),
            model: field("model").filter(|model| model != "inherit"),
            effort: field("effort"),
            fork: field("context").is_some_and(|value| value == "fork"),
            agent: field("agent"),
            paths: list("paths"),
            powershell: field("shell")
                .is_some_and(|value| value.eq_ignore_ascii_case("powershell")),
            body: body.to_string(),
        }
    }

    /// Dossier du skill (celui de `SKILL.md`).
    #[must_use]
    pub fn dir(&self) -> PathBuf {
        self.path
            .parent()
            .map_or_else(PathBuf::new, Path::to_path_buf)
    }

    /// Le skill répond à ce nom (nom de commande ou nom de dossier).
    #[must_use]
    pub fn answers_to(&self, name: &str) -> bool {
        let name = name.trim().trim_start_matches('/');
        self.name.eq_ignore_ascii_case(name) || self.dir_name.eq_ignore_ascii_case(name)
    }

    /// Texte de la liste des skills pour le modèle : description puis
    /// `when_to_use`, coupés à 1 536 caractères comme dans Claude Code.
    #[must_use]
    pub fn listing_text(&self) -> Option<String> {
        let text = [self.description.as_deref(), self.when_to_use.as_deref()]
            .into_iter()
            .flatten()
            .filter(|part| !part.trim().is_empty())
            .collect::<Vec<_>>()
            .join(" ");
        if text.trim().is_empty() {
            return None;
        }
        Some(if text.chars().count() > 1536 {
            format!("{}…", text.chars().take(1535).collect::<String>())
        } else {
            text
        })
    }

    /// Règles `allowed-tools` avec `${CLAUDE_SKILL_DIR}` et `${CLAUDE_PROJECT_DIR}` remplacés.
    #[must_use]
    pub fn granted_rules(&self, project_dir: &Path) -> Vec<String> {
        self.allowed_tools
            .iter()
            .map(|rule| replace_variables(rule, &self.dir(), project_dir, "", ""))
            .collect()
    }

    /// Corps du skill (sans frontmatter).
    #[must_use]
    pub fn body(&self) -> &str {
        &self.body
    }
}

/// Décision du contrôle de permission d'une commande injectée.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandCheck {
    Allow,
    Deny(String),
}

/// Ce que le rendu doit savoir de la session.
pub struct RenderContext<'a> {
    pub session_id: &'a str,
    pub effort: &'a str,
    pub project_dir: &'a Path,
    pub cwd: &'a Path,
    /// Commandes injectées non exécutées (skill synchronisé, réglage
    /// `disableSkillShellExecution`) : remplacées par [`SHELL_DISABLED`].
    pub shell_disabled: bool,
    /// Contrôle des permissions d'une commande injectée.
    pub check: &'a dyn Fn(&str) -> CommandCheck,
}

/// Morceau du modèle : texte, ou commande injectée.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Segment {
    Text(String),
    Inline(String),
    Block(String),
}

/// Rend le skill pour ces arguments. `Err` : invocation annulée (commande
/// refusée ou en échec), avec le message à montrer.
pub fn render(
    skill: &SkillFile,
    arguments: &str,
    context: &RenderContext<'_>,
) -> Result<String, String> {
    let values = split_arguments(arguments);
    let mut received = false;
    let mut segments = Vec::new();
    for segment in segments_of(skill.body()) {
        segments.push(match segment {
            Segment::Text(text) => Segment::Text(substitute_arguments(
                &text,
                arguments,
                &values,
                &skill.arguments,
                &mut received,
            )),
            Segment::Inline(command) => Segment::Inline(substitute_arguments(
                &command,
                arguments,
                &values,
                &skill.arguments,
                &mut received,
            )),
            Segment::Block(command) => Segment::Block(substitute_arguments(
                &command,
                arguments,
                &values,
                &skill.arguments,
                &mut received,
            )),
        });
    }
    let variables = |text: &str| {
        replace_variables(
            text,
            &skill.dir(),
            context.project_dir,
            context.session_id,
            context.effort,
        )
    };
    let mut rendered = String::new();
    for segment in segments {
        match segment {
            Segment::Text(text) => rendered.push_str(&variables(&text)),
            Segment::Inline(command) | Segment::Block(command) => {
                let command = variables(&command);
                let output = run_injected(&command, skill, context)?;
                rendered.push_str(output.trim_end());
            }
        }
    }
    if !received && !arguments.trim().is_empty() {
        rendered = format!("{}\n\nARGUMENTS: {}", rendered.trim_end(), arguments.trim());
    }
    Ok(rendered.trim().to_string())
}

fn run_injected(
    command: &str,
    skill: &SkillFile,
    context: &RenderContext<'_>,
) -> Result<String, String> {
    if context.shell_disabled {
        return Ok(SHELL_DISABLED.to_string());
    }
    let pattern = command.trim();
    if let CommandCheck::Deny(reason) = (context.check)(pattern) {
        return Err(format!(
            "Shell command permission check failed for pattern \"{pattern}\" ({reason}). Pré-approuvez-la avec allowed-tools dans le frontmatter du skill /{}.",
            skill.name
        ));
    }
    let mut process = if skill.powershell {
        let mut process = std::process::Command::new(if cfg!(windows) {
            "powershell.exe"
        } else {
            "pwsh"
        });
        process.args(["-NoProfile", "-NonInteractive", "-Command", pattern]);
        process
    } else {
        plugins::posix_shell_command(&format!("{pattern} 2>&1"))
    };
    process
        .current_dir(context.cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = process
        .spawn()
        .map_err(|error| format!("Shell command failed for pattern \"{pattern}\": {error}"))?;
    let stdout = child.stdout.take().map(read_all);
    let stderr = child.stderr.take().map(read_all);
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() < COMMAND_TIMEOUT => {
                std::thread::sleep(Duration::from_millis(20));
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!(
                    "Shell command failed for pattern \"{pattern}\": délai de {} s dépassé",
                    COMMAND_TIMEOUT.as_secs()
                ));
            }
        }
    };
    let output = stdout
        .and_then(|handle| handle.join().ok())
        .unwrap_or_default();
    let errors = stderr
        .and_then(|handle| handle.join().ok())
        .unwrap_or_default();
    let code = status.code().unwrap_or(-1);
    if code == 0 || (code == 1 && exit_one_is_normal(pattern)) {
        return Ok(output);
    }
    let detail = [output.trim(), errors.trim()]
        .into_iter()
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    Err(format!(
        "Shell command failed for pattern \"{pattern}\" (code {code})\n[stderr]\n{detail}"
    ))
}

fn read_all(mut pipe: impl std::io::Read + Send + 'static) -> std::thread::JoinHandle<String> {
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = pipe.read_to_end(&mut bytes);
        String::from_utf8_lossy(&bytes).into_owned()
    })
}

/// Commandes de recherche et de comparaison pour lesquelles le code 1 est un
/// résultat normal (rien trouvé, fichiers différents).
fn exit_one_is_normal(command: &str) -> bool {
    let first = command.split_whitespace().next().unwrap_or_default();
    matches!(
        first,
        "grep" | "egrep" | "fgrep" | "rg" | "diff" | "cmp" | "find" | "test" | "["
    ) || command.trim_start().starts_with("git diff")
}

/// Découpe le corps en texte et commandes injectées.
fn segments_of(body: &str) -> Vec<Segment> {
    let mut segments = Vec::new();
    let mut text = String::new();
    let mut lines = body.split_inclusive('\n').peekable();
    while let Some(line) = lines.next() {
        if line.trim_end() == "```!" {
            let mut block = String::new();
            for inner in lines.by_ref() {
                if inner.trim_end() == "```" {
                    break;
                }
                block.push_str(inner);
            }
            flush_text(&mut segments, &mut text);
            segments.push(Segment::Block(block.trim_end().to_string()));
            text.push('\n');
            continue;
        }
        inline_segments(line, &mut segments, &mut text);
    }
    flush_text(&mut segments, &mut text);
    segments
}

fn flush_text(segments: &mut Vec<Segment>, text: &mut String) {
    if !text.is_empty() {
        segments.push(Segment::Text(std::mem::take(text)));
    }
}

/// `` !`commande` `` reconnu en début de ligne ou après un blanc seulement.
fn inline_segments(line: &str, segments: &mut Vec<Segment>, text: &mut String) {
    let mut rest = line;
    loop {
        let Some(index) = rest.find("!`") else {
            text.push_str(rest);
            return;
        };
        let before = &rest[..index];
        let at_boundary = before.is_empty() && text.is_empty()
            || before.is_empty() && text.ends_with(char::is_whitespace)
            || before.ends_with(char::is_whitespace);
        let after = &rest[index + 2..];
        match (at_boundary, after.find('`')) {
            (true, Some(end)) if end > 0 => {
                text.push_str(before);
                flush_text(segments, text);
                segments.push(Segment::Inline(after[..end].to_string()));
                rest = &after[end + 1..];
            }
            _ => {
                text.push_str(&rest[..index + 2]);
                rest = after;
            }
        }
    }
}

/// Arguments séparés comme dans un shell (guillemets simples ou doubles).
#[must_use]
pub fn split_arguments(arguments: &str) -> Vec<String> {
    let mut values = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;
    let mut started = false;
    for character in arguments.chars() {
        match (quote, character) {
            (Some(open), close) if close == open => quote = None,
            (Some(_), other) => current.push(other),
            (None, '"' | '\'') => {
                quote = Some(character);
                started = true;
            }
            (None, blank) if blank.is_whitespace() => {
                if started {
                    values.push(std::mem::take(&mut current));
                    started = false;
                }
            }
            (None, other) => {
                current.push(other);
                started = true;
            }
        }
    }
    if started {
        values.push(current);
    }
    values
}

/// Substitue les emplacements d'arguments en une passe ; `received` passe à
/// vrai dès qu'un emplacement reçoit un argument.
fn substitute_arguments(
    template: &str,
    raw: &str,
    values: &[String],
    names: &[String],
    received: &mut bool,
) -> String {
    let chars: Vec<char> = template.chars().collect();
    let mut output = String::new();
    let mut index = 0;
    while index < chars.len() {
        let character = chars[index];
        // `\$1` : le `$` reste littéral (mais pas `\\$1`).
        if character == '\\'
            && chars.get(index + 1) == Some(&'$')
            && (index == 0 || chars[index - 1] != '\\')
        {
            if let Some((_, length)) = placeholder_at(&chars, index + 1, names) {
                output.extend(&chars[index + 1..index + 1 + length]);
                index += 1 + length;
                continue;
            }
        }
        if character == '$' {
            if let Some((placeholder, length)) = placeholder_at(&chars, index, names) {
                let replacement = match placeholder {
                    Placeholder::All => {
                        *received = true;
                        Some(raw.trim().to_string())
                    }
                    Placeholder::Index(position) => values.get(position).map(|value| {
                        *received = true;
                        value.clone()
                    }),
                    Placeholder::Named(position) => {
                        *received = true;
                        Some(values.get(position).cloned().unwrap_or_default())
                    }
                };
                match replacement {
                    Some(value) => output.push_str(&value),
                    None => output.extend(&chars[index..index + length]),
                }
                index += length;
                continue;
            }
        }
        output.push(character);
        index += 1;
    }
    output
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Placeholder {
    All,
    Index(usize),
    Named(usize),
}

fn placeholder_at(chars: &[char], start: usize, names: &[String]) -> Option<(Placeholder, usize)> {
    let rest: String = chars[start + 1..].iter().take(64).collect();
    if let Some(after) = rest.strip_prefix("ARGUMENTS") {
        if let Some(inner) = after.strip_prefix('[') {
            if let Some(end) = inner.find(']') {
                if let Ok(position) = inner[..end].parse::<usize>() {
                    return Some((
                        Placeholder::Index(position),
                        1 + "ARGUMENTS[".len() + end + 1,
                    ));
                }
            }
        }
        let next = after.chars().next();
        if !next.is_some_and(|character| character.is_alphanumeric() || character == '_') {
            return Some((Placeholder::All, 1 + "ARGUMENTS".len()));
        }
    }
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    if !digits.is_empty() {
        if let Ok(position) = digits.parse::<usize>() {
            return Some((Placeholder::Index(position), 1 + digits.len()));
        }
    }
    let word: String = rest
        .chars()
        .take_while(|character| {
            character.is_alphanumeric() || *character == '_' || *character == '-'
        })
        .collect();
    names
        .iter()
        .position(|name| *name == word)
        .map(|position| (Placeholder::Named(position), 1 + word.len()))
}

fn replace_variables(
    text: &str,
    skill_dir: &Path,
    project_dir: &Path,
    session_id: &str,
    effort: &str,
) -> String {
    let dir = skill_dir.display().to_string();
    let project = project_dir.display().to_string();
    let mut text = text.to_string();
    for (names, value) in [
        (["${CLAUDE_SKILL_DIR}", "${CLAW_SKILL_DIR}"], dir.as_str()),
        (
            ["${CLAUDE_PROJECT_DIR}", "${CLAW_PROJECT_DIR}"],
            project.as_str(),
        ),
        (["${CLAUDE_SESSION_ID}", "${CLAW_SESSION_ID}"], session_id),
        (["${CLAUDE_EFFORT}", "${CLAW_EFFORT}"], effort),
    ] {
        for name in names {
            if !value.is_empty() || name.contains("SKILL_DIR") || name.contains("PROJECT_DIR") {
                text = text.replace(name, value);
            }
        }
    }
    text
}

fn split_frontmatter(contents: &str) -> (&str, &str) {
    let Some(rest) = contents
        .strip_prefix("---\n")
        .or_else(|| contents.strip_prefix("---\r\n"))
    else {
        return ("", contents);
    };
    let mut offset = 0;
    for line in rest.split_inclusive('\n') {
        if line.trim_end() == "---" {
            return (&rest[..offset], &rest[offset + line.len()..]);
        }
        offset += line.len();
    }
    ("", contents)
}

fn unquote(value: &str) -> String {
    let value = value.trim();
    let quoted = (value.starts_with('"') && value.ends_with('"'))
        || (value.starts_with('\'') && value.ends_with('\''));
    if quoted && value.len() >= 2 {
        value[1..value.len() - 1].to_string()
    } else {
        value.to_string()
    }
}

/// Valeur scalaire d'une clé (une ligne, ou un bloc `|` / `>` indenté).
fn frontmatter_value(frontmatter: &str, key: &str) -> Option<String> {
    let lines: Vec<&str> = frontmatter.lines().collect();
    for (index, line) in lines.iter().enumerate() {
        let Some(value) = line
            .strip_prefix(key)
            .and_then(|rest| rest.trim_start().strip_prefix(':'))
        else {
            continue;
        };
        let value = value.trim();
        if matches!(value, "|" | ">" | "|-" | ">-") {
            let block: Vec<&str> = lines[index + 1..]
                .iter()
                .take_while(|next| {
                    next.starts_with(' ') || next.starts_with('\t') || next.trim().is_empty()
                })
                .map(|next| next.trim())
                .collect();
            let joined = block.join(if value.starts_with('|') { "\n" } else { " " });
            return Some(joined.trim().to_string()).filter(|text| !text.is_empty());
        }
        if value.is_empty() {
            return None;
        }
        return Some(unquote(value));
    }
    None
}

/// Liste : `a, b`, `a b`, `[a, b]` ou une liste YAML `- a`.
fn frontmatter_list(frontmatter: &str, key: &str) -> Vec<String> {
    let lines: Vec<&str> = frontmatter.lines().collect();
    for (index, line) in lines.iter().enumerate() {
        let Some(value) = line
            .strip_prefix(key)
            .and_then(|rest| rest.trim_start().strip_prefix(':'))
        else {
            continue;
        };
        let value = value.trim();
        if value.is_empty() {
            return lines[index + 1..]
                .iter()
                .take_while(|next| next.trim_start().starts_with("- "))
                .map(|next| unquote(next.trim_start().trim_start_matches("- ")))
                .filter(|item| !item.is_empty())
                .collect();
        }
        let value = value.trim_start_matches('[').trim_end_matches(']');
        return split_tool_like(value);
    }
    Vec::new()
}

/// Sépare sur les virgules et les espaces, sauf à l'intérieur de parenthèses
/// (`Bash(git add *) Read` donne deux éléments).
fn split_tool_like(value: &str) -> Vec<String> {
    let mut items = Vec::new();
    let mut current = String::new();
    let mut depth = 0_usize;
    for character in value.chars() {
        match character {
            '(' => {
                depth += 1;
                current.push(character);
            }
            ')' => {
                depth = depth.saturating_sub(1);
                current.push(character);
            }
            ',' | ' ' | '\t' if depth == 0 => {
                if !current.trim().is_empty() {
                    items.push(unquote(&current));
                }
                current.clear();
            }
            other => current.push(other),
        }
    }
    if !current.trim().is_empty() {
        items.push(unquote(&current));
    }
    items
}

fn parse_bool(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "true" | "yes" | "on" | "1" => Some(true),
        "false" | "no" | "off" | "0" => Some(false),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{render, split_arguments, CommandCheck, RenderContext, SkillFile, SHELL_DISABLED};
    use std::path::Path;

    fn skill(contents: &str) -> SkillFile {
        SkillFile::parse(contents, Path::new("/skills/demo/SKILL.md"), "demo")
    }

    fn context(check: &dyn Fn(&str) -> CommandCheck, disabled: bool) -> RenderContext<'_> {
        RenderContext {
            session_id: "session-42",
            effort: "high",
            project_dir: Path::new("/projet"),
            cwd: Path::new("."),
            shell_disabled: disabled,
            check,
        }
    }

    #[test]
    fn frontmatter_fields_are_read_like_claude_code() {
        let parsed = skill(
            "---\nname: deploy\ndescription: Deploy the app\nwhen_to_use: when the user says ship it\nargument-hint: [env]\narguments: [env, branch]\ndisable-model-invocation: yes\nuser-invocable: false\nallowed-tools: Bash(git add *) Bash(git commit *), Read\ndisallowed-tools:\n  - AskUserQuestion\nmodel: haiku\neffort: low\ncontext: fork\nagent: Explore\npaths: src/**, tests/**\nshell: powershell\n---\n\nDeploy $env\n",
        );
        assert_eq!(parsed.name, "deploy");
        assert!(parsed.answers_to("/demo") && parsed.answers_to("DEPLOY"));
        assert_eq!(parsed.arguments, ["env", "branch"]);
        assert!(parsed.disable_model_invocation && !parsed.user_invocable);
        assert_eq!(
            parsed.allowed_tools,
            ["Bash(git add *)", "Bash(git commit *)", "Read"]
        );
        assert_eq!(parsed.disallowed_tools, ["AskUserQuestion"]);
        assert_eq!(parsed.model.as_deref(), Some("haiku"));
        assert!(parsed.fork && parsed.powershell);
        assert_eq!(parsed.agent.as_deref(), Some("Explore"));
        assert_eq!(parsed.paths, ["src/**", "tests/**"]);
        assert_eq!(
            parsed.listing_text().as_deref(),
            Some("Deploy the app when the user says ship it")
        );
        // Sans frontmatter : nom du dossier, description = première ligne.
        let bare = skill("# Résume le diff\nFais-le.");
        assert_eq!(bare.name, "demo");
        assert_eq!(bare.description.as_deref(), Some("Résume le diff"));
        assert!(!bare.disable_model_invocation && bare.user_invocable);
    }

    #[test]
    fn arguments_follow_the_claude_code_rules() {
        let allow = |_: &str| CommandCheck::Allow;
        let ctx = context(&allow, true);
        let positional = skill("Migrate the $0 component from $1 to $ARGUMENTS[2]. Keep $3.");
        assert_eq!(
            render(&positional, "SearchBar \"Java Script\" TypeScript", &ctx).expect("rendu"),
            "Migrate the SearchBar component from Java Script to TypeScript. Keep $3."
        );
        let named = skill("---\narguments: [issue, branch]\n---\nFix $issue on $branch.");
        assert_eq!(render(&named, "123", &ctx).expect("rendu"), "Fix 123 on .");
        // Sans emplacement, les arguments sont ajoutés.
        let none = skill("Review the code.");
        assert_eq!(
            render(&none, "src/lib.rs", &ctx).expect("rendu"),
            "Review the code.\n\nARGUMENTS: src/lib.rs"
        );
        // Une valeur n'est pas réinterprétée ; `\$1` reste littéral.
        let literal = skill("Summarize $0 for \\$1.00 and $ARGUMENTS");
        assert_eq!(
            render(&literal, "\"$ARGUMENTS from yesterday\"", &ctx).expect("rendu"),
            "Summarize $ARGUMENTS from yesterday for $1.00 and \"$ARGUMENTS from yesterday\""
        );
        let variables =
            skill("Log to ${CLAUDE_SKILL_DIR}/log/${CLAUDE_SESSION_ID} at ${CLAUDE_EFFORT}");
        let rendered = render(&variables, "", &ctx).expect("rendu");
        assert!(rendered.contains("log/session-42 at high"), "{rendered}");
        assert!(rendered.contains("demo"), "{rendered}");
    }

    #[test]
    fn injected_commands_run_once_and_respect_permissions() {
        if cfg!(windows) && plugins::posix_shell_program().is_none() {
            return;
        }
        let allow = |_: &str| CommandCheck::Allow;
        let ctx = context(&allow, false);
        let dynamic = skill(
            "---\ndescription: d\n---\n## Branche\n!`echo main`\nKEY=!`echo pas-exécuté`\n```!\necho un\necho deux\n```\nFin $ARGUMENTS",
        );
        let rendered = render(&dynamic, "!`echo injecté`", &ctx).expect("rendu");
        assert!(rendered.contains("## Branche\nmain"), "{rendered}");
        assert!(rendered.contains("KEY=!`echo pas-exécuté`"), "{rendered}");
        assert!(rendered.contains("un\ndeux"), "{rendered}");
        // Un argument ne crée pas de commande injectée.
        assert!(rendered.ends_with("Fin !`echo injecté`"), "{rendered}");

        let deny = |command: &str| CommandCheck::Deny(format!("{command} refusée"));
        let refused = render(&dynamic, "", &context(&deny, false)).expect_err("refus");
        assert!(
            refused.contains("permission check failed for pattern \"echo main\""),
            "{refused}"
        );

        let failing = skill("!`exit 3`");
        assert!(render(&failing, "", &ctx)
            .expect_err("échec")
            .contains("Shell command failed"));
        let nothing_found = skill("Résultat : !`grep introuvable-xyz /dev/null`");
        assert!(render(&nothing_found, "", &ctx).is_ok());

        let disabled = render(&dynamic, "", &context(&allow, true)).expect("rendu");
        assert!(disabled.contains(SHELL_DISABLED), "{disabled}");
    }

    #[test]
    fn arguments_split_like_a_shell() {
        assert_eq!(
            split_arguments("a \"b c\" 'd e' f"),
            ["a", "b c", "d e", "f"]
        );
        assert_eq!(split_arguments("  "), Vec::<String>::new());
        assert_eq!(split_arguments("\"\""), [""]);
    }
}
