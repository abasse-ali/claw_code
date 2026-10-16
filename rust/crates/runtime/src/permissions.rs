use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};
use std::sync::Mutex;

use glob::{MatchOptions, Pattern};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum PermissionMode {
    ReadOnly,
    WorkspaceWrite,
    DangerFullAccess,
    Prompt,
    Allow,
    /// `dontAsk` de Claude Code : rien n'est demandé ; ce qui n'est pas
    /// autorisé d'avance (lecture, règles `allow`) est refusé.
    DontAsk,
    /// Mode auto de Claude Code : lectures et modifications du projet
    /// acceptées ; le reste (commandes, chemins protégés, écritures hors du
    /// projet) est jugé par un classificateur à la place de l'utilisateur.
    Auto,
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
            Self::DontAsk => "dont-ask",
            Self::Auto => "auto",
        }
    }
}

// ------------------------------------------------------------ mode auto
//
// Chemins dont l'écriture n'est jamais acceptée d'office en mode auto (état
// du dépôt, configuration des outils et de Claw) : elle passe par le
// classificateur.

const PROTECTED_DIRS: &[&str] = &[
    ".git",
    ".vscode",
    ".idea",
    ".husky",
    ".cargo",
    ".devcontainer",
    ".yarn",
    ".mvn",
    ".claude",
    ".claw",
];

const PROTECTED_FILES: &[&str] = &[
    ".gitconfig",
    ".gitmodules",
    ".gitattributes",
    ".bashrc",
    ".bash_profile",
    ".zshrc",
    ".zprofile",
    ".profile",
    ".mcp.json",
    ".claw.json",
];

/// Écriture dans un chemin protégé (`.git/`, `.claw/settings.json`…). Les
/// worktrees de Claw et le dossier de travail d'une session en arrière-plan
/// n'en font pas partie.
#[must_use]
pub fn is_protected_path(path: &str) -> bool {
    let normalized = path.replace('\\', "/");
    if normalized.contains("/.claw/worktrees/") || normalized.contains("/.claude/worktrees/") {
        return false;
    }
    if let Some(job_dir) = std::env::var_os("CLAW_JOB_DIR") {
        let tmp = format!("{}/tmp/", job_dir.to_string_lossy().replace('\\', "/"));
        if normalized.starts_with(&tmp) {
            return false;
        }
    }
    let parts: Vec<&str> = normalized
        .split('/')
        .filter(|part| !part.is_empty())
        .collect();
    let Some((file, dirs)) = parts.split_last() else {
        return false;
    };
    if PROTECTED_FILES.contains(file) {
        return true;
    }
    dirs.iter().any(|dir| PROTECTED_DIRS.contains(dir)) || normalized.contains(".config/git/")
}

static CLASSIFY_ALL_SHELL: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// `autoMode.classifyAllShell` : en mode auto, aucune règle `allow` ne
/// dispense une commande shell du classificateur.
pub fn set_classify_all_shell(enabled: bool) {
    CLASSIFY_ALL_SHELL.store(enabled, std::sync::atomic::Ordering::SeqCst);
}

/// Règle `allow` trop large pour le mode auto (exécution de code
/// arbitraire) : `Bash`, `Bash(*)`, interpréteurs avec joker, `npm run`…
fn grants_arbitrary_code(tool: &str, specifier: Option<&str>) -> bool {
    const INTERPRETERS: &[&str] = &[
        "python",
        "python3",
        "node",
        "deno",
        "bun",
        "ruby",
        "perl",
        "php",
        "bash",
        "sh",
        "zsh",
        "pwsh",
        "powershell",
        "cmd",
        "npx",
        "bunx",
        "uv run",
        "npm run",
        "yarn run",
        "pnpm run",
        "cargo run",
        "go run",
        "java",
        "dotnet run",
    ];
    if !matches!(tool, "bash" | "Bash" | "PowerShell") {
        return tool == "Agent";
    }
    if CLASSIFY_ALL_SHELL.load(std::sync::atomic::Ordering::SeqCst) {
        return true;
    }
    let Some(specifier) = specifier.map(str::trim) else {
        return true;
    };
    if specifier.is_empty() || specifier == "*" || specifier == ":*" {
        return true;
    }
    let head = specifier
        .trim_end_matches('*')
        .trim_end_matches(':')
        .trim()
        .to_ascii_lowercase();
    specifier.ends_with('*')
        && INTERPRETERS
            .iter()
            .any(|interpreter| head == *interpreter || head.starts_with(&format!("{interpreter} ")))
}

// ------------------------------------------------------------ dossiers autorisés
//
// En `workspace-write`, les écritures sont libres dans le dossier du projet,
// les dossiers ajoutés (`permissions.additionalDirectories`, `--add-dir`,
// `/add-dir`) et le dossier temporaire ; ailleurs, l'utilisateur confirme.

/// Outils accordés pour le tour en cours par le `allowed-tools` d'un skill :
/// retirés à la fin du tour. Les règles `deny` et `ask` restent prioritaires.
static TURN_GRANTS: Mutex<Vec<PermissionRule>> = Mutex::new(Vec::new());

/// Accorde ces règles (`Bash(git add *)`, `Read`…) jusqu'à la fin du tour.
pub fn grant_for_turn(rules: &[String]) {
    let mut grants = TURN_GRANTS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    grants.extend(rules.iter().filter_map(|rule| PermissionRule::parse(rule)));
}

/// Outils retirés pour le tour en cours par le `disallowed-tools` d'un skill.
static TURN_DENIALS: Mutex<Vec<PermissionRule>> = Mutex::new(Vec::new());

/// Refuse ces outils jusqu'à la fin du tour.
pub fn deny_for_turn(rules: &[String]) {
    TURN_DENIALS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .extend(rules.iter().filter_map(|rule| PermissionRule::parse(rule)));
}

/// Fin du tour : les outils accordés ou retirés par un skill ne le sont plus.
pub fn clear_turn_grants() {
    TURN_GRANTS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clear();
    TURN_DENIALS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clear();
}

fn turn_grants() -> Vec<PermissionRule> {
    TURN_GRANTS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
}

static CONFIGURED_DIRECTORIES: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());
static SESSION_DIRECTORIES: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

/// Dossiers de `permissions.additionalDirectories` (remplace les précédents).
pub fn set_configured_directories(directories: Vec<PathBuf>) {
    *CONFIGURED_DIRECTORIES
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = directories;
}

/// Ajoute un dossier pour la session (`--add-dir`, `/add-dir`) ; faux s'il y était.
pub fn add_session_directory(directory: PathBuf) -> bool {
    let mut directories = SESSION_DIRECTORIES
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if directories
        .iter()
        .any(|known| same_place(known, &directory))
    {
        return false;
    }
    directories.push(directory);
    true
}

/// Dossiers où l'écriture est libre en `workspace-write`, `cwd` compris.
#[must_use]
pub fn writable_directories(cwd: &Path) -> Vec<PathBuf> {
    let mut directories = vec![cwd.to_path_buf(), std::env::temp_dir()];
    for list in [&CONFIGURED_DIRECTORIES, &SESSION_DIRECTORIES] {
        directories.extend(
            list.lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .iter()
                .map(|directory| resolve_lexically(&directory.display().to_string(), cwd)),
        );
    }
    directories
}

/// Chemin absolu sans `.` ni `..` (sans toucher au disque).
fn resolve_lexically(path: &str, cwd: &Path) -> PathBuf {
    let raw = Path::new(path);
    let joined = if raw.is_absolute() {
        raw.to_path_buf()
    } else {
        cwd.join(raw)
    };
    let mut resolved = PathBuf::new();
    for component in joined.components() {
        match component {
            std::path::Component::ParentDir => {
                resolved.pop();
            }
            std::path::Component::CurDir => {}
            other => resolved.push(other.as_os_str()),
        }
    }
    resolved
}

/// Forme comparable d'un chemin : `/`, sans préfixe `\\?\`, sans casse sous Windows.
fn comparable(path: &Path) -> String {
    let text = path.display().to_string().replace('\\', "/");
    let text = text.strip_prefix("//?/").unwrap_or(&text);
    let text = text.trim_end_matches('/');
    if cfg!(windows) {
        text.to_lowercase()
    } else {
        text.to_string()
    }
}

fn same_place(left: &Path, right: &Path) -> bool {
    comparable(left) == comparable(right)
}

fn is_inside(path: &Path, root: &Path) -> bool {
    let path = comparable(path);
    let root = comparable(root);
    path == root || path.starts_with(&format!("{root}/"))
}

/// `path` (relatif à `cwd`) est dans un des dossiers où l'écriture est libre.
#[must_use]
pub fn is_writable_without_asking(path: &str, cwd: &Path) -> bool {
    let target = resolve_lexically(path, cwd);
    let canonical_target = target
        .parent()
        .and_then(|parent| parent.canonicalize().ok())
        .map(|parent| parent.join(target.file_name().unwrap_or_default()));
    writable_directories(cwd).iter().any(|root| {
        let canonical_root = root.canonicalize().ok();
        [Some(root.clone()), canonical_root]
            .into_iter()
            .flatten()
            .any(|root| {
                is_inside(&target, &root)
                    || canonical_target
                        .as_ref()
                        .is_some_and(|target| is_inside(target, &root))
            })
    })
}

/// Appel qui écrit un fichier hors des dossiers autorisés.
fn writes_outside_workspace(tool_name: &str, target: &ToolTarget, cwd: &Path) -> Option<String> {
    if !matches!(tool_name, "write_file" | "edit_file" | "NotebookEdit") {
        return None;
    }
    match target {
        ToolTarget::Path(path) if !is_writable_without_asking(path, cwd) => Some(path.clone()),
        _ => None,
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

// --- Demandes restées sans réponse ---------------------------------------------------

/// Début du résultat rendu au modèle pour un appel qui n'a pas tourné faute
/// de réponse à la demande d'autorisation : ce n'est ni un refus de
/// l'utilisateur ni un échec de l'outil.
pub const NOT_RUN_PREFIX: &str = "[Claw] Not run:";

/// Une demande d'autorisation du tour en cours est restée sans réponse.
static USER_AWAY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Décision d'une demande d'autorisation restée sans réponse (délai dépassé).
/// Le modèle doit comprendre que l'appel n'a pas tourné et qu'il n'est pas
/// bloqué : sans cela, un modèle a relancé quatre fois la même commande en la
/// croyant « suspendue », puis a rendu le travail à l'utilisateur.
#[must_use]
pub fn unanswered_permission(tool_name: &str) -> PermissionPromptDecision {
    PermissionPromptDecision::Deny {
        reason: format!(
            "{NOT_RUN_PREFIX} nobody answered the permission request for `{tool_name}`, so this call did not run. It is not hanging and nothing is wrong with it: the user seems to be away. Do not ask for this permission again and do not retry the call another way. Finish what needs no permission, then say in your final answer exactly which call is waiting for the user's approval."
        ),
    }
}

/// Demande suivante pendant que l'utilisateur ne répond pas : elle ne lui est
/// pas posée (chacune l'attendrait aussi longtemps que la première).
#[must_use]
pub(crate) fn not_asked(tool_name: &str) -> PermissionPromptDecision {
    PermissionPromptDecision::Deny {
        reason: format!(
            "{NOT_RUN_PREFIX} an earlier permission request of this turn is still unanswered (the user seems to be away), so the request for `{tool_name}` was not asked. Finish what needs no permission, then say in your final answer which calls are waiting for the user's approval."
        ),
    }
}

/// Le résultat dit qu'un appel n'a pas tourné faute de réponse.
#[must_use]
pub fn is_not_run(reason: &str) -> bool {
    reason.starts_with(NOT_RUN_PREFIX)
}

/// L'utilisateur n'a pas répondu à une demande d'autorisation de ce tour.
#[must_use]
pub fn user_away() -> bool {
    USER_AWAY.load(std::sync::atomic::Ordering::Relaxed)
}

/// Retenu au premier silence, oublié au début du tour suivant (l'utilisateur
/// vient d'écrire).
pub(crate) fn set_user_away(away: bool) {
    USER_AWAY.store(away, std::sync::atomic::Ordering::Relaxed);
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
            "read" => &["read_file", "glob_search", "grep_search", "FindSymbol"],
            "edit" => &["edit_file", "write_file", "NotebookEdit"],
            "write" => &["write_file"],
            "glob" => &["glob_search"],
            "grep" => &["grep_search"],
            "agent" | "task" => &["Agent"],
            // ViewPage charge aussi une page du web : une règle qui limite
            // WebFetch le limite aussi.
            "webfetch" => &["WebFetch", "ViewPage", "Screenshot"],
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

    /// Change le mode en cours de session (plan approuvé, `/permissions`).
    pub fn set_active_mode(&mut self, mode: PermissionMode) {
        self.active_mode = mode;
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
    #[allow(clippy::too_many_lines)]
    pub fn authorize(
        &mut self,
        tool_name: &str,
        input: &str,
        prompter: Option<&mut dyn PermissionPrompter>,
    ) -> PermissionOutcome {
        let current_mode = self.active_mode;
        let required_mode = self.required_mode_for(tool_name);
        let target = ToolTarget::from_input(tool_name, input);

        let denials = TURN_DENIALS
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        if let Some(rule) = self.first_match_any(&denials, tool_name, &target) {
            return PermissionOutcome::Deny {
                reason: format!(
                    "l'outil '{tool_name}' est retiré pendant ce skill (disallowed-tools : `{}`)",
                    rule.as_str()
                ),
            };
        }
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
        let outside = writes_outside_workspace(tool_name, &target, &self.cwd);
        if !must_ask && current_mode != PermissionMode::ReadOnly {
            let grants = turn_grants();
            if self.first_match_any(&grants, tool_name, &target).is_some() {
                return PermissionOutcome::Allow;
            }
        }
        if !must_ask {
            // Mode auto : modifications dans le projet acceptées, sauf
            // chemins protégés (classificateur).
            let protected = current_mode == PermissionMode::Auto
                && matches!(&target, ToolTarget::Path(path) if is_protected_path(path))
                && matches!(tool_name, "write_file" | "edit_file" | "NotebookEdit");
            if !protected
                && (matches!(
                    current_mode,
                    PermissionMode::Allow | PermissionMode::DangerFullAccess
                ) || required_mode <= PermissionMode::ReadOnly
                    || (matches!(
                        current_mode,
                        PermissionMode::WorkspaceWrite | PermissionMode::Auto
                    ) && required_mode <= PermissionMode::WorkspaceWrite
                        && outside.is_none())
                    || is_read_only_command(&target))
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

        // `dontAsk` : ce qui n'est pas autorisé d'avance est refusé sans demander.
        if current_mode == PermissionMode::DontAsk {
            return PermissionOutcome::Deny {
                reason: format!(
                    "mode dontAsk : l'outil '{tool_name}' n'est pas autorisé d'avance ; ajoutez une règle (/permissions allow {}) ou changez de mode",
                    suggest_rule(tool_name, &target).unwrap_or_else(|| tool_name.to_string())
                ),
            };
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
                reason: match outside {
                    Some(path) => format!(
                        "écriture hors du projet ({path}) : ajoutez le dossier avec /add-dir ou --add-dir, ou dans permissions.additionalDirectories"
                    ),
                    None => format!(
                        "l'outil '{tool_name}' nécessite une approbation pour passer de {} à {}",
                        current_mode.as_str(),
                        required_mode.as_str()
                    ),
                },
            },
        }
    }

    /// Appel déjà accordé par un hook (`permissionDecision: "allow"`) : seules
    /// les règles `deny` peuvent encore l'interdire.
    #[must_use]
    pub fn authorize_preapproved(&self, tool_name: &str, input: &str) -> PermissionOutcome {
        let target = ToolTarget::from_input(tool_name, input);
        match self.first_match_any(&self.rules.deny, tool_name, &target) {
            Some(rule) => PermissionOutcome::Deny {
                reason: format!(
                    "l'outil '{tool_name}' est interdit par la règle de permission `{}`",
                    rule.as_str()
                ),
            },
            None => PermissionOutcome::Allow,
        }
    }

    /// Un hook demande de confirmer (`permissionDecision: "ask"`) : après les
    /// règles `deny`, l'utilisateur est consulté même si le mode autoriserait
    /// l'appel ; sans invite (mode non interactif), l'appel est refusé.
    pub fn authorize_with_prompt(
        &mut self,
        tool_name: &str,
        input: &str,
        prompter: Option<&mut dyn PermissionPrompter>,
    ) -> PermissionOutcome {
        if let denied @ PermissionOutcome::Deny { .. } =
            self.authorize_preapproved(tool_name, input)
        {
            return denied;
        }
        let target = ToolTarget::from_input(tool_name, input);
        let request = PermissionRequest {
            tool_name: tool_name.to_string(),
            input: input.to_string(),
            current_mode: self.active_mode,
            required_mode: self.required_mode_for(tool_name),
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
                    "un hook demande de confirmer l'outil '{tool_name}', et personne ne peut répondre ici"
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
        let auto = self.active_mode == PermissionMode::Auto;
        let protected = auto && matches!(target, ToolTarget::Path(path) if is_protected_path(path));
        if protected {
            // Les règles `allow` n'accordent pas les chemins protégés.
            return false;
        }
        let rules = self
            .rules
            .allow
            .iter()
            .filter(|rule| rule.covers_tool(tool_name))
            // En mode auto, les règles qui accordent l'exécution de code
            // arbitraire sont suspendues : le classificateur juge.
            .filter(|rule| !(auto && grants_arbitrary_code(&rule.tool, rule.specifier.as_deref())))
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
            "WebFetch" | "ViewPage" => field("url").map_or(Self::Other, Self::Url),
            "Screenshot" => field("url")
                .map(Self::Url)
                .or_else(|| field("path").map(Self::Path))
                .unwrap_or(Self::Other),
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
        let mut chars = command.char_indices().peekable();

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

        while let Some((index, ch)) = chars.next() {
            match ch {
                '\\' if !in_single => {
                    current.push(ch);
                    if let Some((_, next)) = chars.next() {
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
                '$' if !in_single && matches!(chars.peek(), Some((_, '(' | '{'))) => {
                    has_substitution = true;
                    current.push(ch);
                }
                '>' if !in_single && !in_double => {
                    // `2>&1` (erreurs vers la sortie) et `> /dev/null` n'écrivent
                    // dans aucun fichier.
                    let rest = command[index + 1..].trim_start_matches('>').trim_start();
                    let harmless = rest.starts_with("/dev/null")
                        || rest
                            .strip_prefix('&')
                            .is_some_and(|fd| fd.starts_with(|digit: char| digit.is_ascii_digit()));
                    if !harmless {
                        redirects_output = true;
                    }
                    current.push(ch);
                }
                // `>&` et `&>` font partie d'une redirection, pas d'une séparation.
                '&' if !in_single
                    && !in_double
                    && (current.ends_with('>') || matches!(chars.peek(), Some((_, '>')))) =>
                {
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
        if let Some(home) = crate::home_dir() {
            pattern = format!("{}/{rest}", home.display()).replace('\\', "/");
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

// ------------------------------------------------------------ bascule de mode

static MODE_SWITCH: std::sync::Mutex<Option<PermissionMode>> = std::sync::Mutex::new(None);

/// Demande un changement de mode pour la suite du tour (par exemple après
/// l'approbation d'un plan) ; appliqué par la boucle de conversation.
pub fn request_mode_switch(mode: PermissionMode) {
    *MODE_SWITCH
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(mode);
}

pub(crate) fn take_mode_switch() -> Option<PermissionMode> {
    MODE_SWITCH
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .take()
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

    #[test]
    fn workspace_write_asks_before_writing_outside_the_project() {
        let project = std::env::current_dir()
            .expect("cwd")
            .join("projet-essai-ecriture");
        let elsewhere = if cfg!(windows) {
            "C:/Windows/claw-essai/hors.txt"
        } else {
            "/etc/claw-essai/hors.txt"
        };
        let mut policy = PermissionPolicy::new(PermissionMode::WorkspaceWrite)
            .with_tool_requirement("write_file", PermissionMode::WorkspaceWrite)
            .with_cwd(&project);
        assert_eq!(
            policy.authorize("write_file", &path_input("src/lib.rs"), None),
            PermissionOutcome::Allow
        );
        assert_eq!(
            policy.authorize(
                "write_file",
                &path_input("../projet-essai-ecriture/a.txt"),
                None
            ),
            PermissionOutcome::Allow
        );
        let temp = std::env::temp_dir().join("claw-essai.txt");
        assert_eq!(
            policy.authorize("write_file", &path_input(&temp.display().to_string()), None),
            PermissionOutcome::Allow
        );
        let PermissionOutcome::Deny { reason } =
            policy.authorize("write_file", &path_input(elsewhere), None)
        else {
            panic!("écriture hors du projet autorisée sans demander");
        };
        assert!(
            reason.contains("hors du projet") && reason.contains("/add-dir"),
            "{reason}"
        );
        let mut prompter = RecordingPrompter::allowing();
        assert_eq!(
            policy.authorize("write_file", &path_input(elsewhere), Some(&mut prompter)),
            PermissionOutcome::Allow
        );
        assert_eq!(prompter.seen.len(), 1);
        // Un dossier ajouté pour la session (`--add-dir`, `/add-dir`).
        let parent = Path::new(elsewhere).parent().expect("parent").to_path_buf();
        assert!(super::add_session_directory(parent.clone()));
        assert!(!super::add_session_directory(parent));
        assert_eq!(
            policy.authorize("write_file", &path_input(elsewhere), None),
            PermissionOutcome::Allow
        );
    }

    #[test]
    fn auto_mode_lets_project_edits_through_and_sends_the_rest_to_the_classifier() {
        let project = std::env::current_dir()
            .expect("cwd")
            .join("projet-essai-auto");
        let mut policy = PermissionPolicy::new(PermissionMode::Auto)
            .with_tool_requirement("read_file", PermissionMode::ReadOnly)
            .with_tool_requirement("edit_file", PermissionMode::WorkspaceWrite)
            .with_tool_requirement("bash", PermissionMode::DangerFullAccess)
            .with_rules(PermissionRules::from_strings(
                &["Bash(*)".to_string(), "Bash(cargo test:*)".to_string()],
                &[],
                &[],
            ))
            .with_cwd(&project);
        assert_eq!(
            policy.authorize("edit_file", &path_input("src/lib.rs"), None),
            PermissionOutcome::Allow
        );
        // Chemin protégé : le classificateur (ici l'invite) décide.
        let mut prompter = RecordingPrompter::denying();
        assert!(matches!(
            policy.authorize("edit_file", &path_input(".git/config"), Some(&mut prompter)),
            PermissionOutcome::Deny { .. }
        ));
        assert_eq!(prompter.seen.len(), 1);
        assert_eq!(prompter.seen[0].current_mode, PermissionMode::Auto);
        // `Bash(*)` est suspendue en mode auto ; une règle précise reste valable.
        let mut prompter = RecordingPrompter::allowing();
        assert_eq!(
            policy.authorize(
                "bash",
                &json!({"command": "npm publish"}).to_string(),
                Some(&mut prompter)
            ),
            PermissionOutcome::Allow
        );
        assert_eq!(prompter.seen.len(), 1);
        assert_eq!(
            policy.authorize(
                "bash",
                &json!({"command": "cargo test --workspace"}).to_string(),
                None
            ),
            PermissionOutcome::Allow
        );
        assert!(super::is_protected_path("C:/repo/.claw/settings.json"));
        assert!(super::is_protected_path("/repo/.bashrc"));
        assert!(!super::is_protected_path(
            "/repo/.claw/worktrees/bg-1/src/main.rs"
        ));
        assert!(!super::is_protected_path("/repo/src/main.rs"));
    }

    #[test]
    fn dont_ask_refuses_what_is_not_allowed_in_advance() {
        let mut policy = PermissionPolicy::new(PermissionMode::DontAsk)
            .with_tool_requirement("read_file", PermissionMode::ReadOnly)
            .with_tool_requirement("bash", PermissionMode::DangerFullAccess)
            .with_rules(PermissionRules::from_strings(
                &["Bash(cargo test:*)".to_string()],
                &[],
                &[],
            ));
        assert_eq!(
            policy.authorize("read_file", &path_input("src/lib.rs"), None),
            PermissionOutcome::Allow
        );
        assert_eq!(
            policy.authorize("bash", &bash("cargo test -p runtime"), None),
            PermissionOutcome::Allow
        );
        let mut prompter = RecordingPrompter::allowing();
        let PermissionOutcome::Deny { reason } = policy.authorize(
            "bash",
            &bash("curl https://exemple.com"),
            Some(&mut prompter),
        ) else {
            panic!("dontAsk a laissé passer une commande non autorisée");
        };
        assert!(prompter.seen.is_empty(), "dontAsk ne demande jamais");
        assert!(
            reason.contains("dontAsk") && reason.contains("/permissions allow"),
            "{reason}"
        );
        assert_eq!(PermissionMode::DontAsk.as_str(), "dont-ask");
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
    fn stderr_redirections_do_not_split_or_taint_a_command() {
        let parsed = super::ShellCommand::parse("python3 test_panier.py 2>&1");
        assert_eq!(parsed.segments.len(), 1);
        assert_eq!(parsed.segments[0].text, "python3 test_panier.py 2>&1");
        assert!(!parsed.segments[0].redirects_output);

        let quiet = super::ShellCommand::parse("ls missing 2>/dev/null && git status > /dev/null");
        assert_eq!(quiet.segments.len(), 2);
        assert!(quiet
            .segments
            .iter()
            .all(|segment| !segment.redirects_output));

        // Une vraie écriture dans un fichier reste repérée.
        let written = super::ShellCommand::parse("echo x > notes.txt; make &> build.log");
        assert_eq!(written.segments.len(), 2);
        assert!(written
            .segments
            .iter()
            .all(|segment| segment.redirects_output));

        // Plusieurs lignes entre guillemets : une seule commande.
        let script =
            super::ShellCommand::parse("python3 -c \"\nimport os\nprint(os.getcwd())\n\" 2>&1");
        assert_eq!(script.segments.len(), 1);
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
        // ViewPage charge aussi une page : la règle le concerne.
        policy.authorize(
            "ViewPage",
            &json!({ "url": "http://localhost:3000/" }).to_string(),
            Some(&mut prompter),
        );
        assert_eq!(prompter.seen.len(), 2);
        assert_eq!(prompter.seen[1].tool_name, "ViewPage");
        let mut policy = self::policy(
            PermissionMode::DangerFullAccess,
            &[],
            &["WebFetch(domain:intranet.local)"],
            &[],
        );
        assert!(matches!(
            policy.authorize(
                "ViewPage",
                &json!({ "url": "http://intranet.local/rapport" }).to_string(),
                None
            ),
            PermissionOutcome::Deny { .. }
        ));
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
