use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use glob::{MatchOptions, Pattern};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum PermissionMode {
    ReadOnly,
    WorkspaceWrite,
    DangerFullAccess,
    Prompt,
    Allow,
}

impl PermissionMode {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ReadOnly => "read-only",
            Self::WorkspaceWrite => "workspace-write",
            Self::DangerFullAccess => "danger-full-access",
            Self::Prompt => "prompt",
            Self::Allow => "allow",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionRequest {
    pub tool_name: String,
    pub input: String,
    pub current_mode: PermissionMode,
    pub required_mode: PermissionMode,
    /// Règle proposée si l'utilisateur choisit « toujours » (ex. `Bash(cargo test:*)`).
    pub suggested_rule: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PermissionPromptDecision {
    Allow,
    /// Autorise et ajoute la règle aux règles `allow` de la session.
    AllowAlways {
        rule: String,
    },
    Deny {
        reason: String,
    },
}

pub trait PermissionPrompter {
    fn decide(&mut self, request: &PermissionRequest) -> PermissionPromptDecision;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PermissionOutcome {
    Allow,
    Deny { reason: String },
}

/// Règle de permission au format de Claude Code : `Outil` ou `Outil(spécificateur)`.
///
/// - `Bash(cargo test:*)` : commandes commençant par `cargo test` ; `Bash(git status)` : exacte.
/// - `Edit(src/**)`, `Read(.env)` : chemins (motifs glob relatifs au répertoire de travail).
/// - `WebFetch(domain:docs.rs)` : domaine (et sous-domaines).
/// - `Read` couvre aussi la recherche (glob/grep), `Edit` toutes les écritures de fichiers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionRule {
    raw: String,
    tool: String,
    specifier: Option<String>,
}

impl PermissionRule {
    #[must_use]
    pub fn parse(raw: &str) -> Option<Self> {
        let raw = raw.trim();
        if raw.is_empty() {
            return None;
        }
        let (tool, specifier) = match raw.split_once('(') {
            Some((tool, rest)) => (tool.trim(), Some(rest.strip_suffix(')')?.trim())),
            None => (raw, None),
        };
        if tool.is_empty() {
            return None;
        }
        Some(Self {
            raw: raw.to_string(),
            tool: tool.to_string(),
            specifier: specifier
                .filter(|value| !value.is_empty() && *value != "*")
                .map(ToOwned::to_owned),
        })
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.raw
    }

    fn covers_tool(&self, tool_name: &str) -> bool {
        let covered: &[&str] = match self.tool.to_ascii_lowercase().as_str() {
            "bash" => &["bash"],
            "powershell" => &["PowerShell"],
            "read" => &["read_file", "glob_search", "grep_search"],
            "edit" => &["edit_file", "write_file", "NotebookEdit"],
            "write" => &["write_file"],
            "glob" => &["glob_search"],
            "grep" => &["grep_search"],
            "agent" | "task" => &["Agent"],
            _ => {
                return self.tool.eq_ignore_ascii_case(tool_name)
                    || (self.tool.starts_with("mcp__")
                        && tool_name.starts_with(&format!("{}__", self.tool)));
            }
        };
        covered.contains(&tool_name)
    }

    /// Vrai si la règle couvre l'appel. Pour une commande shell, `segment` est
    /// l'une des sous-commandes (séparées par `&&`, `;`, `|`…).
    fn matches_target(&self, target: &ToolTarget, segment: Option<&str>, cwd: &Path) -> bool {
        let Some(specifier) = &self.specifier else {
            return true;
        };
        match target {
            ToolTarget::Command(_) => {
                segment.is_some_and(|segment| command_pattern_matches(specifier, segment))
            }
            ToolTarget::Path(path) => path_pattern_matches(specifier, path, cwd),
            ToolTarget::Url(url) => url_pattern_matches(specifier, url),
            ToolTarget::Other => false,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PermissionRules {
    pub allow: Vec<PermissionRule>,
    pub deny: Vec<PermissionRule>,
    pub ask: Vec<PermissionRule>,
}

impl PermissionRules {
    /// Construit les règles à partir de chaînes ; les règles mal formées sont ignorées.
    #[must_use]
    pub fn from_strings(allow: &[String], deny: &[String], ask: &[String]) -> Self {
        let parse = |rules: &[String]| {
            rules
                .iter()
                .filter_map(|rule| PermissionRule::parse(rule))
                .collect()
        };
        Self {
            allow: parse(allow),
            deny: parse(deny),
            ask: parse(ask),
        }
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.allow.is_empty() && self.deny.is_empty() && self.ask.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionPolicy {
    active_mode: PermissionMode,
    tool_requirements: BTreeMap<String, PermissionMode>,
    rules: PermissionRules,
    cwd: PathBuf,
}

impl PermissionPolicy {
    #[must_use]
    pub fn new(active_mode: PermissionMode) -> Self {
        Self {
            active_mode,
            tool_requirements: BTreeMap::new(),
            rules: PermissionRules::default(),
            cwd: std::env::current_dir().unwrap_or_default(),
        }
    }

    #[must_use]
    pub fn with_tool_requirement(
        mut self,
        tool_name: impl Into<String>,
        required_mode: PermissionMode,
    ) -> Self {
        self.tool_requirements
            .insert(tool_name.into(), required_mode);
        self
    }

    #[must_use]
    pub fn with_rules(mut self, rules: PermissionRules) -> Self {
        self.rules = rules;
        self
    }

    #[must_use]
    pub fn with_cwd(mut self, cwd: impl Into<PathBuf>) -> Self {
        self.cwd = cwd.into();
        self
    }

    #[must_use]
    pub fn active_mode(&self) -> PermissionMode {
        self.active_mode
    }

    #[must_use]
    pub fn rules(&self) -> &PermissionRules {
        &self.rules
    }

    #[must_use]
    pub fn required_mode_for(&self, tool_name: &str) -> PermissionMode {
        self.tool_requirements
            .get(tool_name)
            .copied()
            .unwrap_or(PermissionMode::DangerFullAccess)
    }

    /// Ordre d'évaluation : règles `deny` (toujours prioritaires), règles `ask`,
    /// mode actif, règles `allow` et commandes en lecture seule, puis demande.
    pub fn authorize(
        &mut self,
        tool_name: &str,
        input: &str,
        prompter: Option<&mut dyn PermissionPrompter>,
    ) -> PermissionOutcome {
        let current_mode = self.active_mode;
        let required_mode = self.required_mode_for(tool_name);
        let target = ToolTarget::from_input(tool_name, input);

        if let Some(rule) = self.first_match_any(&self.rules.deny, tool_name, &target) {
            return PermissionOutcome::Deny {
                reason: format!(
                    "l'outil '{tool_name}' est interdit par la règle de permission `{}`",
                    rule.as_str()
                ),
            };
        }

        let must_ask = self
            .first_match_any(&self.rules.ask, tool_name, &target)
            .is_some();
        if !must_ask {
            if matches!(
                current_mode,
                PermissionMode::Allow | PermissionMode::DangerFullAccess
            ) || required_mode <= PermissionMode::ReadOnly
                || (current_mode == PermissionMode::WorkspaceWrite
                    && required_mode <= PermissionMode::WorkspaceWrite)
                || is_read_only_command(&target)
            {
                return PermissionOutcome::Allow;
            }
            // En lecture seule (mode plan), les règles `allow` ne s'appliquent pas :
            // elles pourraient autoriser des modifications.
            if current_mode == PermissionMode::ReadOnly {
                return PermissionOutcome::Deny {
                    reason: format!(
                        "l'outil '{tool_name}' nécessite la permission {} ; le mode actuel est {} \
                         (lecture seule : explorez et proposez un plan sans modifier)",
                        required_mode.as_str(),
                        current_mode.as_str()
                    ),
                };
            }
            if self.all_segments_allowed(tool_name, &target) {
                return PermissionOutcome::Allow;
            }
        }

        let request = PermissionRequest {
            tool_name: tool_name.to_string(),
            input: input.to_string(),
            current_mode,
            required_mode,
            suggested_rule: suggest_rule(tool_name, &target),
        };
        match prompter {
            Some(prompter) => match prompter.decide(&request) {
                PermissionPromptDecision::Allow => PermissionOutcome::Allow,
                PermissionPromptDecision::AllowAlways { rule } => {
                    if let Some(rule) = PermissionRule::parse(&rule) {
                        self.rules.allow.push(rule);
                    }
                    PermissionOutcome::Allow
                }
                PermissionPromptDecision::Deny { reason } => PermissionOutcome::Deny { reason },
            },
            None => PermissionOutcome::Deny {
                reason: format!(
                    "l'outil '{tool_name}' nécessite une approbation pour passer de {} à {}",
                    current_mode.as_str(),
                    required_mode.as_str()
                ),
            },
        }
    }

    /// Première règle qui couvre l'appel (pour une commande : n'importe quelle sous-commande).
    fn first_match_any<'a>(
        &self,
        rules: &'a [PermissionRule],
        tool_name: &str,
        target: &ToolTarget,
    ) -> Option<&'a PermissionRule> {
        rules
            .iter()
            .filter(|rule| rule.covers_tool(tool_name))
            .find(|rule| match target {
                ToolTarget::Command(command) => {
                    rule.specifier.is_none()
                        || command.segments.iter().any(|segment| {
                            rule.matches_target(target, Some(&segment.text), &self.cwd)
                        })
                }
                _ => rule.matches_target(target, None, &self.cwd),
            })
    }

    /// Une commande n'est autorisée que si chacune de ses sous-commandes l'est
    /// (par une règle ou parce qu'elle est en lecture seule), et jamais
    /// lorsqu'elle contient une substitution (`$(…)`, backticks).
    fn all_segments_allowed(&self, tool_name: &str, target: &ToolTarget) -> bool {
        let rules = self
            .rules
            .allow
            .iter()
            .filter(|rule| rule.covers_tool(tool_name))
            .collect::<Vec<_>>();
        if rules.is_empty() {
            return false;
        }
        match target {
            ToolTarget::Command(command) => {
                if rules.iter().any(|rule| rule.specifier.is_none()) {
                    return true;
                }
                !command.has_substitution
                    && !command.segments.is_empty()
                    && command.segments.iter().all(|segment| {
                        is_read_only_segment(segment)
                            || rules.iter().any(|rule| {
                                rule.matches_target(target, Some(&segment.text), &self.cwd)
                            })
                    })
            }
            _ => rules
                .iter()
                .any(|rule| rule.matches_target(target, None, &self.cwd)),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ToolTarget {
    Command(ShellCommand),
    Path(String),
    Url(String),
    Other,
}

impl ToolTarget {
    fn from_input(tool_name: &str, input: &str) -> Self {
        let value = serde_json::from_str::<Value>(input).unwrap_or(Value::Null);
        let field = |key: &str| {
            value
                .get(key)
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
        };
        match tool_name {
            "bash" | "PowerShell" => field("command").map_or(Self::Other, |command| {
                Self::Command(ShellCommand::parse(&command))
            }),
            "read_file" | "write_file" | "edit_file" => {
                field("path").map_or(Self::Other, Self::Path)
            }
            "NotebookEdit" => field("notebook_path").map_or(Self::Other, Self::Path),
            "glob_search" | "grep_search" => {
                Self::Path(field("path").unwrap_or_else(|| ".".to_string()))
            }
            "WebFetch" => field("url").map_or(Self::Other, Self::Url),
            _ => Self::Other,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ShellCommand {
    segments: Vec<CommandSegment>,
    has_substitution: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CommandSegment {
    text: String,
    redirects_output: bool,
}

impl ShellCommand {
    /// Découpe une ligne de commande en sous-commandes sur `;`, `&`, `|` et les
    /// retours à la ligne situés hors guillemets, et repère les substitutions
    /// et les redirections de sortie.
    fn parse(command: &str) -> Self {
        let mut segments = Vec::new();
        let mut current = String::new();
        let mut redirects_output = false;
        let mut has_substitution = false;
        let mut in_single = false;
        let mut in_double = false;
        let mut chars = command.chars().peekable();

        let mut push_segment = |current: &mut String, redirects_output: &mut bool| {
            let text = current.split_whitespace().collect::<Vec<_>>().join(" ");
            if !text.is_empty() {
                segments.push(CommandSegment {
                    text,
                    redirects_output: *redirects_output,
                });
            }
            current.clear();
            *redirects_output = false;
        };

        while let Some(ch) = chars.next() {
            match ch {
                '\\' if !in_single => {
                    current.push(ch);
                    if let Some(next) = chars.next() {
                        current.push(next);
                    }
                }
                '\'' if !in_double => {
                    in_single = !in_single;
                    current.push(ch);
                }
                '"' if !in_single => {
                    in_double = !in_double;
                    current.push(ch);
                }
                '`' if !in_single => {
                    has_substitution = true;
                    current.push(ch);
                }
                '$' if !in_single && matches!(chars.peek(), Some('(' | '{')) => {
                    has_substitution = true;
                    current.push(ch);
                }
                '>' if !in_single && !in_double => {
                    redirects_output = true;
                    current.push(ch);
                }
                ';' | '&' | '|' | '\n' if !in_single && !in_double => {
                    push_segment(&mut current, &mut redirects_output);
                }
                _ => current.push(ch),
            }
        }
        push_segment(&mut current, &mut redirects_output);

        Self {
            segments,
            has_substitution,
        }
    }
}

/// Commandes sans effet de bord autorisées sans confirmation (hors mode plan
/// compris), à condition de ne contenir ni substitution ni redirection.
const READ_ONLY_COMMANDS: &[&str] = &[
    "pwd",
    "ls",
    "dir",
    "cat",
    "head",
    "tail",
    "wc",
    "which",
    "where",
    "echo",
    "date",
    "git status",
    "git diff",
    "git log",
    "git show",
    "git branch --show-current",
    "git rev-parse",
    "git ls-files",
    "git remote -v",
    "cargo --version",
    "rustc --version",
    "node --version",
    "npm --version",
    "python --version",
    "python3 --version",
    "Get-ChildItem",
    "Get-Content",
    "Get-Location",
];

fn is_read_only_command(target: &ToolTarget) -> bool {
    let ToolTarget::Command(command) = target else {
        return false;
    };
    !command.has_substitution
        && !command.segments.is_empty()
        && command.segments.iter().all(is_read_only_segment)
}

fn is_read_only_segment(segment: &CommandSegment) -> bool {
    !segment.redirects_output
        && !segment.text.contains("--output")
        && READ_ONLY_COMMANDS
            .iter()
            .any(|safe| command_pattern_matches(&format!("{safe}:*"), &segment.text))
}

fn command_pattern_matches(pattern: &str, segment: &str) -> bool {
    let normalize = |value: &str| value.split_whitespace().collect::<Vec<_>>().join(" ");
    let segment = normalize(segment);
    if let Some(prefix) = pattern
        .strip_suffix(":*")
        .or_else(|| pattern.strip_suffix(" *"))
    {
        let prefix = normalize(prefix);
        return segment == prefix || segment.starts_with(&format!("{prefix} "));
    }
    if let Some(prefix) = pattern.strip_suffix('*') {
        return segment.starts_with(&normalize(prefix));
    }
    segment == normalize(pattern)
}

fn path_pattern_matches(pattern: &str, path: &str, cwd: &Path) -> bool {
    let options = MatchOptions {
        case_sensitive: !cfg!(windows),
        require_literal_separator: true,
        require_literal_leading_dot: false,
    };
    let absolute = normalize_path(&cwd.join(path));
    let cwd = normalize_path(cwd);
    let relative = absolute
        .strip_prefix(&format!("{cwd}/"))
        .map(ToOwned::to_owned);

    let mut pattern = pattern.replace('\\', "/");
    if let Some(rest) = pattern.strip_prefix("~/") {
        if let Some(home) = std::env::var_os("HOME") {
            pattern = format!("{}/{rest}", PathBuf::from(home).display()).replace('\\', "/");
        }
    }
    let pattern = pattern.trim_start_matches("./").to_string();
    let is_absolute = Path::new(&pattern).is_absolute() || pattern.starts_with('/');
    let subject = if is_absolute {
        absolute
    } else if let Some(relative) = relative {
        relative
    } else {
        return false;
    };

    // Comme dans un .gitignore : un motif sans `/` vaut à toute profondeur, et
    // un motif sans joker couvre aussi tout ce qui se trouve sous ce chemin.
    let pattern = if pattern.ends_with('/') {
        format!("{pattern}**")
    } else {
        pattern
    };
    let candidates = if is_absolute || pattern.contains('/') {
        vec![pattern.clone()]
    } else {
        vec![pattern.clone(), format!("**/{pattern}")]
    };
    candidates.iter().any(|candidate| {
        let literal_prefix = !candidate.contains(['*', '?', '[']);
        if literal_prefix
            && (eq_path(&subject, candidate)
                || starts_with_path(&subject, &format!("{candidate}/")))
        {
            return true;
        }
        Pattern::new(candidate).is_ok_and(|compiled| compiled.matches_with(&subject, options))
    })
}

fn eq_path(left: &str, right: &str) -> bool {
    if cfg!(windows) {
        left.eq_ignore_ascii_case(right)
    } else {
        left == right
    }
}

fn starts_with_path(path: &str, prefix: &str) -> bool {
    if cfg!(windows) {
        path.to_ascii_lowercase()
            .starts_with(&prefix.to_ascii_lowercase())
    } else {
        path.starts_with(prefix)
    }
}

/// Normalisation lexicale (sans accès disque) : résout `.` et `..`, et utilise `/`.
fn normalize_path(path: &Path) -> String {
    let mut parts: Vec<String> = Vec::new();
    let mut prefix = String::new();
    for component in path.components() {
        match component {
            Component::Prefix(value) => prefix = value.as_os_str().to_string_lossy().into_owned(),
            Component::RootDir => prefix.push('/'),
            Component::CurDir => {}
            Component::ParentDir => {
                parts.pop();
            }
            Component::Normal(value) => parts.push(value.to_string_lossy().into_owned()),
        }
    }
    format!("{prefix}{}", parts.join("/"))
}

fn url_pattern_matches(pattern: &str, url: &str) -> bool {
    if let Some(domain) = pattern.strip_prefix("domain:") {
        let domain = domain.trim().to_ascii_lowercase();
        let host = url_host(url);
        return host == domain || host.ends_with(&format!(".{domain}"));
    }
    url.starts_with(pattern)
}

fn url_host(url: &str) -> String {
    let without_scheme = url.split_once("://").map_or(url, |(_, rest)| rest);
    let authority = without_scheme
        .split(['/', '?', '#'])
        .next()
        .unwrap_or_default();
    let host = authority.rsplit('@').next().unwrap_or_default();
    host.split(':')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase()
}

fn suggest_rule(tool_name: &str, target: &ToolTarget) -> Option<String> {
    match target {
        ToolTarget::Command(command) => {
            let [segment] = command.segments.as_slice() else {
                return None;
            };
            if command.has_substitution {
                return None;
            }
            let mut words = segment.text.split_whitespace();
            let first = words.next()?;
            let prefix = match words.next() {
                Some(second)
                    if second
                        .chars()
                        .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
                        && !second.starts_with('-') =>
                {
                    format!("{first} {second}")
                }
                _ => first.to_string(),
            };
            let tool = if tool_name == "PowerShell" {
                "PowerShell"
            } else {
                "Bash"
            };
            Some(format!("{tool}({prefix}:*)"))
        }
        ToolTarget::Path(_) if matches!(tool_name, "edit_file" | "write_file" | "NotebookEdit") => {
            Some("Edit".to_string())
        }
        ToolTarget::Url(url) => Some(format!("WebFetch(domain:{})", url_host(url))),
        _ => Some(tool_name.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        PermissionMode, PermissionOutcome, PermissionPolicy, PermissionPromptDecision,
        PermissionPrompter, PermissionRequest, PermissionRule, PermissionRules,
    };
    use serde_json::json;
    use std::path::Path;

    struct RecordingPrompter {
        seen: Vec<PermissionRequest>,
        decision: PermissionPromptDecision,
    }

    impl RecordingPrompter {
        fn allowing() -> Self {
            Self {
                seen: Vec::new(),
                decision: PermissionPromptDecision::Allow,
            }
        }

        fn denying() -> Self {
            Self {
                seen: Vec::new(),
                decision: PermissionPromptDecision::Deny {
                    reason: "pas maintenant".to_string(),
                },
            }
        }
    }

    impl PermissionPrompter for RecordingPrompter {
        fn decide(&mut self, request: &PermissionRequest) -> PermissionPromptDecision {
            self.seen.push(request.clone());
            self.decision.clone()
        }
    }

    fn bash(command: &str) -> String {
        json!({ "command": command }).to_string()
    }

    fn path_input(path: &str) -> String {
        json!({ "path": path }).to_string()
    }

    fn policy(
        mode: PermissionMode,
        allow: &[&str],
        deny: &[&str],
        ask: &[&str],
    ) -> PermissionPolicy {
        let owned = |rules: &[&str]| {
            rules
                .iter()
                .map(|rule| (*rule).to_string())
                .collect::<Vec<_>>()
        };
        PermissionPolicy::new(mode)
            .with_tool_requirement("read_file", PermissionMode::ReadOnly)
            .with_tool_requirement("glob_search", PermissionMode::ReadOnly)
            .with_tool_requirement("write_file", PermissionMode::WorkspaceWrite)
            .with_tool_requirement("edit_file", PermissionMode::WorkspaceWrite)
            .with_tool_requirement("bash", PermissionMode::DangerFullAccess)
            .with_tool_requirement("WebFetch", PermissionMode::DangerFullAccess)
            .with_rules(PermissionRules::from_strings(
                &owned(allow),
                &owned(deny),
                &owned(ask),
            ))
            .with_cwd(if cfg!(windows) {
                r"C:\work\repo"
            } else {
                "/work/repo"
            })
    }

    #[test]
    fn allows_tools_when_active_mode_meets_requirement() {
        let mut policy = PermissionPolicy::new(PermissionMode::WorkspaceWrite)
            .with_tool_requirement("read_file", PermissionMode::ReadOnly)
            .with_tool_requirement("write_file", PermissionMode::WorkspaceWrite);

        assert_eq!(
            policy.authorize("read_file", "{}", None),
            PermissionOutcome::Allow
        );
        assert_eq!(
            policy.authorize("write_file", "{}", None),
            PermissionOutcome::Allow
        );
    }

    #[test]
    fn denies_read_only_escalations_without_prompt() {
        let mut policy = PermissionPolicy::new(PermissionMode::ReadOnly)
            .with_tool_requirement("write_file", PermissionMode::WorkspaceWrite)
            .with_tool_requirement("bash", PermissionMode::DangerFullAccess);

        assert!(matches!(
            policy.authorize("write_file", "{}", None),
            PermissionOutcome::Deny { reason } if reason.contains("nécessite la permission workspace-write")
        ));
        assert!(matches!(
            policy.authorize("bash", "{}", None),
            PermissionOutcome::Deny { reason } if reason.contains("nécessite la permission danger-full-access")
        ));
    }

    #[test]
    fn prompts_for_workspace_write_to_danger_full_access_escalation() {
        let mut policy = PermissionPolicy::new(PermissionMode::WorkspaceWrite)
            .with_tool_requirement("bash", PermissionMode::DangerFullAccess);
        let mut prompter = RecordingPrompter::allowing();

        let outcome = policy.authorize("bash", "echo hi", Some(&mut prompter));

        assert_eq!(outcome, PermissionOutcome::Allow);
        assert_eq!(prompter.seen.len(), 1);
        assert_eq!(prompter.seen[0].tool_name, "bash");
        assert_eq!(
            prompter.seen[0].current_mode,
            PermissionMode::WorkspaceWrite
        );
        assert_eq!(
            prompter.seen[0].required_mode,
            PermissionMode::DangerFullAccess
        );
    }

    #[test]
    fn honors_prompt_rejection_reason() {
        let mut policy = PermissionPolicy::new(PermissionMode::WorkspaceWrite)
            .with_tool_requirement("bash", PermissionMode::DangerFullAccess);
        let mut prompter = RecordingPrompter::denying();

        assert!(matches!(
            policy.authorize("bash", "echo hi", Some(&mut prompter)),
            PermissionOutcome::Deny { reason } if reason == "pas maintenant"
        ));
    }

    #[test]
    fn prompt_mode_allows_reads_and_asks_for_edits_and_commands() {
        let mut policy = policy(PermissionMode::Prompt, &[], &[], &[]);
        let mut prompter = RecordingPrompter::allowing();

        assert_eq!(
            policy.authorize("read_file", &path_input("src/lib.rs"), Some(&mut prompter)),
            PermissionOutcome::Allow
        );
        assert!(prompter.seen.is_empty());

        policy.authorize("edit_file", &path_input("src/lib.rs"), Some(&mut prompter));
        policy.authorize("bash", &bash("cargo build"), Some(&mut prompter));
        assert_eq!(prompter.seen.len(), 2);
        assert_eq!(prompter.seen[0].suggested_rule.as_deref(), Some("Edit"));
        assert_eq!(
            prompter.seen[1].suggested_rule.as_deref(),
            Some("Bash(cargo build:*)")
        );
    }

    #[test]
    fn read_only_commands_skip_the_prompt_even_in_plan_mode() {
        for mode in [PermissionMode::Prompt, PermissionMode::ReadOnly] {
            let mut policy = policy(mode, &[], &[], &[]);
            assert_eq!(
                policy.authorize("bash", &bash("git status && git diff --stat"), None),
                PermissionOutcome::Allow
            );
            assert!(matches!(
                policy.authorize("bash", &bash("echo secret > out.txt"), None),
                PermissionOutcome::Deny { .. }
            ));
            assert!(matches!(
                policy.authorize("bash", &bash("cat $(echo /etc/passwd)"), None),
                PermissionOutcome::Deny { .. }
            ));
        }
    }

    #[test]
    fn allow_rules_require_every_compound_segment_to_match() {
        let mut policy = policy(PermissionMode::Prompt, &["Bash(cargo test:*)"], &[], &[]);

        assert_eq!(
            policy.authorize("bash", &bash("cargo test --workspace"), None),
            PermissionOutcome::Allow
        );
        assert_eq!(
            policy.authorize("bash", &bash("cargo test && git status"), None),
            PermissionOutcome::Allow
        );
        assert!(matches!(
            policy.authorize("bash", &bash("cargo test && rm -rf target"), None),
            PermissionOutcome::Deny { .. }
        ));
        assert!(matches!(
            policy.authorize("bash", &bash("cargo testing"), None),
            PermissionOutcome::Deny { .. }
        ));
        assert!(matches!(
            policy.authorize("bash", &bash("cargo test `rm -rf /`"), None),
            PermissionOutcome::Deny { .. }
        ));
    }

    #[test]
    fn deny_rules_win_over_modes_and_allow_rules() {
        let mut policy = policy(
            PermissionMode::DangerFullAccess,
            &["Bash"],
            &["Bash(git push:*)", "Read(.env)", "Edit(secrets/**)"],
            &[],
        );

        assert!(matches!(
            policy.authorize("bash", &bash("git add . && git push --force"), None),
            PermissionOutcome::Deny { reason } if reason.contains("Bash(git push:*)")
        ));
        assert!(matches!(
            policy.authorize("read_file", &path_input("config/.env"), None),
            PermissionOutcome::Deny { .. }
        ));
        assert!(matches!(
            policy.authorize("write_file", &path_input("./secrets/key.pem"), None),
            PermissionOutcome::Deny { .. }
        ));
        assert!(matches!(
            policy.authorize("write_file", &path_input("src/../secrets/key.pem"), None),
            PermissionOutcome::Deny { .. }
        ));
        assert_eq!(
            policy.authorize("write_file", &path_input("src/main.rs"), None),
            PermissionOutcome::Allow
        );
    }

    #[test]
    fn ask_rules_prompt_even_when_the_mode_would_allow() {
        let mut policy = policy(PermissionMode::DangerFullAccess, &[], &[], &["WebFetch"]);
        let mut prompter = RecordingPrompter::allowing();

        policy.authorize(
            "WebFetch",
            &json!({ "url": "https://docs.rs/serde", "prompt": "x" }).to_string(),
            Some(&mut prompter),
        );
        assert_eq!(
            prompter.seen[0].suggested_rule.as_deref(),
            Some("WebFetch(domain:docs.rs)")
        );
    }

    #[test]
    fn allow_always_adds_a_session_rule() {
        let mut policy = policy(PermissionMode::Prompt, &[], &[], &[]);
        let mut prompter = RecordingPrompter {
            seen: Vec::new(),
            decision: PermissionPromptDecision::AllowAlways {
                rule: "Bash(npm run:*)".to_string(),
            },
        };

        policy.authorize("bash", &bash("npm run lint"), Some(&mut prompter));
        assert_eq!(
            policy.authorize("bash", &bash("npm run test"), None),
            PermissionOutcome::Allow
        );
        assert_eq!(prompter.seen.len(), 1);
    }

    #[test]
    fn path_rules_follow_glob_and_directory_semantics() {
        let cwd = Path::new(if cfg!(windows) {
            r"C:\work\repo"
        } else {
            "/work/repo"
        });
        let matches = |pattern: &str, path: &str| super::path_pattern_matches(pattern, path, cwd);

        assert!(matches("src/**", "src/a/b.rs"));
        assert!(matches("src", "src/a/b.rs"));
        assert!(matches("*.rs", "deep/nested/file.rs"));
        assert!(matches("src/*.rs", "src/lib.rs"));
        assert!(!matches("src/*.rs", "src/a/lib.rs"));
        assert!(!matches("src/**", "other/file.rs"));
        assert!(!matches("src/**", "../outside/src/x"));
    }

    #[test]
    fn parses_rules_and_rejects_malformed_ones() {
        let rule = PermissionRule::parse("Bash(cargo test:*)").expect("valid");
        assert_eq!(rule.as_str(), "Bash(cargo test:*)");
        assert!(PermissionRule::parse("Bash(unclosed").is_none());
        assert!(PermissionRule::parse("  ").is_none());
        assert!(PermissionRule::parse("Edit(*)").is_some_and(|rule| rule.specifier.is_none()));
    }
}
