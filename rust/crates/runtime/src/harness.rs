//! Aides au raisonnement que Claw **impose** au lieu de seulement les demander
//! au modèle : vérification après modification, contrôle de syntaxe immédiat,
//! détection de blocage. Un grand modèle suit ces consignes de lui-même ; un
//! petit modèle les oublie, alors c'est le programme qui les applique.
//!
//! Chaque aide se coupe par une variable d'environnement (`CLAW_VERIFY=0`…) ;
//! `CLAW_HARNESS=0` les coupe toutes (mesure « sans aide » du banc d'essai).

use std::collections::HashMap;
use std::fmt::Write as _;
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::Value;

/// Une aide au raisonnement, activée par défaut.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Aid {
    /// Vérification imposée après des modifications de fichiers.
    Verify,
    /// Contrôle de syntaxe juste après chaque modification.
    Diagnostics,
    /// Détection d'un modèle qui tourne en rond.
    StallGuard,
    /// Carte du projet donnée au début de la session.
    RepoMap,
    /// Raisonnement du modèle renvoyé à l'étape suivante.
    ReasoningPassback,
    /// Relecture automatique des modifications.
    Review,
    /// Plan préparé par un modèle plus fort.
    Planner,
    /// Leçons retenues d'une session à l'autre.
    Lessons,
    /// Second avis demandé à d'autres modèles.
    Council,
    /// Chemins inventés retrouvés, erreurs « fichier introuvable » explicites.
    PathHelp,
    /// Liste d'outils réduite à ceux qui servent (moins de choix, moins d'égarement).
    ToolDiet,
    /// Signalement d'un test réécrit après un échec (faire passer n'est pas corriger).
    TestGuard,
    /// Un modèle plus fort reprend le tour quand l'agent n'avance plus.
    Escalation,
    /// Avant de conclure, le modèle confronte son travail à chaque exigence.
    Checklist,
    /// Erreurs vues par l'éditeur relié (VS Code) dans les fichiers modifiés.
    EditorDiagnostics,
    /// Résultats d'outils gardés sans ce que le modèle sait déjà (son texte, le diff).
    LeanResults,
    /// Un agent qui s'arrête sur des échecs cherche d'abord une autre voie.
    Persistence,
}

impl Aid {
    fn variable(self) -> &'static str {
        match self {
            Self::Verify => "CLAW_VERIFY",
            Self::Diagnostics => "CLAW_DIAGNOSTICS",
            Self::StallGuard => "CLAW_STALL_GUARD",
            Self::RepoMap => "CLAW_REPO_MAP",
            Self::ReasoningPassback => "CLAW_REASONING_PASSBACK",
            Self::Review => "CLAW_REVIEW",
            Self::Planner => "CLAW_PLANNER",
            Self::Lessons => "CLAW_LESSONS",
            Self::Council => "CLAW_COUNCIL",
            Self::PathHelp => "CLAW_PATH_HELP",
            Self::ToolDiet => "CLAW_TOOL_DIET",
            Self::TestGuard => "CLAW_TEST_GUARD",
            Self::Escalation => "CLAW_ESCALATION",
            Self::Checklist => "CLAW_CHECKLIST",
            Self::EditorDiagnostics => "CLAW_EDITOR_DIAGNOSTICS",
            Self::LeanResults => "CLAW_LEAN_RESULTS",
            Self::Persistence => "CLAW_PERSISTENCE",
        }
    }
}

fn switched_off(variable: &str) -> bool {
    std::env::var(variable).is_ok_and(|value| {
        matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "0" | "false" | "off" | "non" | "no"
        )
    })
}

/// L'aide est active (ni `CLAW_HARNESS=0`, ni sa propre variable à 0).
#[must_use]
pub fn enabled(aid: Aid) -> bool {
    !switched_off("CLAW_HARNESS") && !switched_off(aid.variable())
}

// --- Vérification imposée ---------------------------------------------------------

/// Réglage `"verify"` : automatique, coupé, ou commandes choisies.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum VerifySetting {
    /// Commande déduite du projet (Cargo.toml, tests Python…).
    #[default]
    Auto,
    Off,
    Commands(Vec<String>),
}

/// Délai d'une vérification, en millisecondes.
pub const VERIFY_TIMEOUT_MS: u64 = 300_000;
/// Corrections demandées au plus après une vérification en échec.
pub const MAX_VERIFY_ROUNDS: usize = 3;

/// Commandes de vérification du projet situé dans `root`.
#[must_use]
pub fn verify_commands(setting: &VerifySetting, root: &Path) -> Vec<String> {
    match setting {
        VerifySetting::Off => Vec::new(),
        VerifySetting::Commands(commands) => commands.clone(),
        VerifySetting::Auto => detect_verify_commands(root),
    }
}

fn python_program() -> &'static str {
    if cfg!(windows) || crate::paths::find_executable("python3").is_none() {
        "python"
    } else {
        "python3"
    }
}

fn has_test_files(dir: &Path) -> bool {
    std::fs::read_dir(dir).is_ok_and(|entries| {
        entries.flatten().any(|entry| {
            let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
            name.strip_suffix(".py")
                .is_some_and(|stem| stem.starts_with("test_") || stem.ends_with("_test"))
        })
    })
}

fn python_verify(root: &Path) -> Option<String> {
    let python = python_program();
    let pytest_configured = ["pytest.ini", "conftest.py"]
        .iter()
        .any(|name| root.join(name).is_file())
        || ["pyproject.toml", "setup.cfg", "tox.ini"]
            .iter()
            .any(|name| {
                std::fs::read_to_string(root.join(name)).is_ok_and(|text| {
                    text.contains("[tool.pytest")
                        || text.contains("[pytest]")
                        || text.contains("[tool:pytest]")
                })
            });
    if pytest_configured {
        return Some(format!("{python} -m pytest -q -x"));
    }
    if has_test_files(root) {
        return Some(format!("{python} -m unittest discover -q"));
    }
    ["tests", "test"]
        .iter()
        .find(|dir| has_test_files(&root.join(dir)))
        .map(|dir| format!("{python} -m unittest discover -s {dir} -q"))
}

fn node_verify(root: &Path) -> Option<String> {
    let package: Value =
        serde_json::from_str(&std::fs::read_to_string(root.join("package.json")).ok()?).ok()?;
    let has_script = |name: &str| {
        package
            .get("scripts")
            .and_then(|scripts| scripts.get(name))
            .and_then(Value::as_str)
            .is_some_and(|script| !script.trim().is_empty())
    };
    if has_script("typecheck") {
        return Some("npm run --silent typecheck".to_string());
    }
    let tsc = root.join("node_modules").join(".bin");
    let installed = tsc.join("tsc").is_file() || tsc.join("tsc.cmd").is_file();
    (root.join("tsconfig.json").is_file() && installed).then(|| "npx tsc --noEmit".to_string())
}

/// Commande de vérification rapide et sûre déduite du projet : compilation
/// (Rust, Go, TypeScript) ou tests (Python). Les tests Node ne sont pas lancés
/// d'office : beaucoup restent en mode « surveillance » et ne rendent pas la main.
#[must_use]
pub fn detect_verify_commands(root: &Path) -> Vec<String> {
    if root.join("Cargo.toml").is_file() {
        return vec!["cargo check --quiet".to_string()];
    }
    if root.join("go.mod").is_file() {
        return vec!["go build ./...".to_string()];
    }
    if root.join("package.json").is_file() {
        if let Some(command) = node_verify(root) {
            return vec![command];
        }
    }
    python_verify(root).into_iter().collect()
}

fn squeeze(command: &str) -> String {
    command.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Le modèle a déjà lancé cette vérification lui-même (commande identique, ou
/// qui la contient : « cd app && cargo check --quiet »).
#[must_use]
pub fn covers(model_command: &str, verify_command: &str) -> bool {
    let (ran, wanted) = (squeeze(model_command), squeeze(verify_command));
    !wanted.is_empty() && ran.contains(&wanted)
}

/// Issue d'une commande lancée par l'outil `bash`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandResult {
    pub passed: bool,
    pub timed_out: bool,
    /// Début et fin de la sortie (là où sont les erreurs).
    pub excerpt: String,
}

fn excerpt(text: &str) -> String {
    const HEAD: usize = 40;
    const TAIL: usize = 40;
    const MAX_CHARS: usize = 6_000;
    let lines: Vec<&str> = text.trim().lines().collect();
    let joined = if lines.len() > HEAD + TAIL {
        format!(
            "{}\n… ({} lines omitted) …\n{}",
            lines[..HEAD].join("\n"),
            lines.len() - HEAD - TAIL,
            lines[lines.len() - TAIL..].join("\n")
        )
    } else {
        lines.join("\n")
    };
    if joined.chars().count() > MAX_CHARS {
        let head: String = joined.chars().take(MAX_CHARS / 2).collect();
        let tail: String = joined
            .chars()
            .rev()
            .take(MAX_CHARS / 2)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        format!("{head}\n… (output shortened) …\n{tail}")
    } else {
        joined
    }
}

/// Lit le résultat JSON de l'outil `bash` (stdout, stderr, code de sortie).
#[must_use]
pub fn read_bash_output(output: &str) -> CommandResult {
    let Ok(value) = serde_json::from_str::<Value>(output) else {
        return CommandResult {
            passed: false,
            timed_out: false,
            excerpt: excerpt(output),
        };
    };
    let text = |key: &str| value.get(key).and_then(Value::as_str).unwrap_or_default();
    let status = text("returnCodeInterpretation");
    let mut combined = text("stdout").trim().to_string();
    let stderr = text("stderr").trim();
    if !stderr.is_empty() {
        if !combined.is_empty() {
            combined.push('\n');
        }
        combined.push_str(stderr);
    }
    if status.starts_with("exit_code:") {
        let _ = write!(
            combined,
            "\n(exit code {})",
            status.trim_start_matches("exit_code:")
        );
    }
    CommandResult {
        passed: status.is_empty(),
        timed_out: status == "timeout",
        excerpt: excerpt(&combined),
    }
}

/// Message renvoyé au modèle quand la vérification échoue.
#[must_use]
pub fn verify_feedback(command: &str, result: &CommandResult) -> String {
    format!(
        "[Claw] Automatic verification after your changes failed.\n$ {command}\n{}\n\nFind the root cause and fix it. Never weaken, skip or delete a test to make it pass. When you are done, the verification runs again automatically. {FINAL_ANSWER_RULE}",
        result.excerpt
    )
}

// --- Détection de blocage ---------------------------------------------------------

/// Repère un modèle qui tourne en rond pendant un tour et prépare le rappel à
/// lui adresser (joint au résultat de l'outil).
#[derive(Debug, Default)]
pub struct StallGuard {
    /// Par appel (outil, entrée) : nombre de fois et dernier résultat.
    calls: HashMap<(String, String), (u32, u64)>,
    consecutive_failures: u32,
    /// Échecs d'édition « texte introuvable », par fichier.
    edit_misses: HashMap<String, u32>,
    /// Vérifications réussies depuis la dernière modification, par commande.
    passed_checks: HashMap<String, u32>,
    tool_calls: u32,
}

/// Rappel adressé au modèle, et sa description pour l'utilisateur.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reminder {
    pub to_model: String,
    pub notice: String,
}

const REPEAT_LIMIT: u32 = 3;
const FAILURE_LIMIT: u32 = 4;
/// Appels d'outils dans un tour avant de demander au modèle de faire le point.
const TOOL_BUDGET: u32 = 40;

/// La commande est une vérification (tests, compilation, analyse).
#[must_use]
pub fn is_check_command(command: &str) -> bool {
    let lower = command.to_ascii_lowercase();
    [
        "test", "check", "build", "lint", "tsc", "compile", "clippy", "vet", "mypy", "ruff",
    ]
    .iter()
    .any(|word| lower.contains(word))
}

fn fingerprint(text: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut hasher);
    hasher.finish()
}

fn is_failure(tool_name: &str, output: &str, is_error: bool) -> bool {
    if is_error {
        return true;
    }
    matches!(tool_name, "bash" | "PowerShell") && {
        let result = read_bash_output(output);
        !result.passed
    }
}

impl StallGuard {
    /// Observe un résultat d'outil ; renvoie un rappel si le modèle s'enlise.
    pub fn observe(
        &mut self,
        tool_name: &str,
        input: &str,
        output: &str,
        is_error: bool,
    ) -> Option<Reminder> {
        let failed = is_failure(tool_name, output, is_error);
        if failed {
            self.consecutive_failures += 1;
        } else {
            self.consecutive_failures = 0;
        }
        self.tool_calls += 1;
        let command = matches!(tool_name, "bash" | "PowerShell")
            .then(|| serde_json::from_str::<Value>(input).ok())
            .flatten()
            .and_then(|value| value.get("command").and_then(Value::as_str).map(squeeze));
        let wrote = matches!(tool_name, "edit_file" | "write_file" | "NotebookEdit") && !is_error;
        // Un fichier modifié, ou une commande qui n'est pas une vérification
        // (elle a pu changer l'état du projet) : les vérifications sont à refaire.
        if wrote
            || command
                .as_deref()
                .is_some_and(|command| !is_check_command(command))
        {
            self.passed_checks.clear();
        }

        // Le même appel, encore, avec le même résultat.
        let entry = self
            .calls
            .entry((tool_name.to_string(), input.to_string()))
            .or_insert((0, 0));
        let same_result = entry.1 == fingerprint(output);
        entry.0 = if same_result || entry.0 == 0 {
            entry.0 + 1
        } else {
            1
        };
        entry.1 = fingerprint(output);
        if entry.0 == REPEAT_LIMIT {
            return Some(Reminder {
                to_model: format!(
                    "[Claw] You have now made this exact `{tool_name}` call {REPEAT_LIMIT} times and got the same result each time. Repeating it will not change anything. Step back: re-read the output above, question the assumption behind this call, and take a different approach (read the relevant code, search for the definition, or test a smaller hypothesis). If you are blocked, tell the user what is missing instead of retrying."
                ),
                notice: format!(
                    "Claw recadre le modèle : {REPEAT_LIMIT} appels identiques à {tool_name} sans effet"
                ),
            });
        }

        // Une modification dont le texte à remplacer est introuvable, deux fois.
        if matches!(tool_name, "edit_file" | "Edit") && is_error {
            let path = serde_json::from_str::<Value>(input)
                .ok()
                .and_then(|value| {
                    value
                        .get("path")
                        .and_then(Value::as_str)
                        .map(ToOwned::to_owned)
                })
                .unwrap_or_default();
            // Sans chemin, ce n'est pas un texte introuvable mais un appel
            // incomplet, que son message d'erreur explique déjà.
            let misses = self.edit_misses.entry(path.clone()).or_insert(0);
            *misses += u32::from(!path.is_empty());
            if *misses == 2 && !path.is_empty() {
                return Some(Reminder {
                    to_model: format!(
                        "[Claw] This is the second failed edit of `{path}`. Do not guess the text again: read the file with read_file and copy the exact lines you want to replace (indentation included), or replace a smaller, unique fragment."
                    ),
                    notice: format!("Claw recadre le modèle : deux modifications de {path} ratées"),
                });
            }
        }

        if self.consecutive_failures == FAILURE_LIMIT {
            return Some(Reminder {
                to_model: format!(
                    "[Claw] {FAILURE_LIMIT} tool calls in a row have failed. Stop acting and diagnose first: state what you expected, what actually happened, and the most likely cause. Confirm that cause with a read or a search before changing anything else. If the approach itself is wrong, choose another one."
                ),
                notice: format!("Claw recadre le modèle : {FAILURE_LIMIT} échecs d'affilée"),
            });
        }

        // La même vérification réussit de nouveau alors que rien n'a changé :
        // le travail est fait, inutile de continuer à vérifier.
        if let Some(command) = command.filter(|command| !failed && is_check_command(command)) {
            let passes = self.passed_checks.entry(command.clone()).or_insert(0);
            *passes += 1;
            if *passes == 2 {
                return Some(Reminder {
                    to_model: format!(
                        "[Claw] `{command}` already succeeded and nothing has changed since. Checking again adds nothing. If the task is complete, stop using tools and give your final answer now."
                    ),
                    notice: "Claw recadre le modèle : vérification déjà réussie, inutile de la relancer"
                        .to_string(),
                });
            }
        }

        if self.tool_calls == TOOL_BUDGET {
            return Some(Reminder {
                to_model: format!(
                    "[Claw] This turn has already used {TOOL_BUDGET} tool calls. Pause and take stock: what is done, what remains. If the task is complete, answer now. Otherwise name the single remaining obstacle and deal with it directly."
                ),
                notice: format!("Claw recadre le modèle : {TOOL_BUDGET} appels d'outils dans ce tour"),
            });
        }
        None
    }
}

// --- Contrôle de syntaxe après modification ---------------------------------------

const SYNTAX_TIMEOUT: Duration = Duration::from_secs(8);

/// Lance un outil de contrôle ; `None` s'il est absent, trop lent ou réussit.
fn checker_errors(program: &str, args: &[&str], cwd: Option<&Path>) -> Option<String> {
    let executable = crate::paths::find_executable(program)?;
    let mut command = Command::new(executable);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    if let Some(cwd) = cwd {
        command.current_dir(cwd);
    }
    let mut child = command.spawn().ok()?;
    let stderr = child.stderr.take();
    let reader = std::thread::spawn(move || {
        let mut text = String::new();
        if let Some(mut stderr) = stderr {
            let _ = stderr.read_to_string(&mut text);
        }
        text
    });
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() > SYNTAX_TIMEOUT => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(_) => return None,
        }
    };
    let text = reader.join().unwrap_or_default();
    (!status.success() && !text.trim().is_empty()).then_some(text)
}

fn last_lines(text: &str, count: usize) -> String {
    let lines: Vec<&str> = text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect();
    lines[lines.len().saturating_sub(count)..].join("\n")
}

/// Erreur de syntaxe du fichier qui vient d'être écrit, si un contrôle rapide
/// existe pour ce langage (JSON intégré ; Python, JavaScript, Rust, Go, shell
/// avec l'outil du langage s'il est installé). `None` : rien à signaler.
#[must_use]
pub fn syntax_check(path: &Path) -> Option<String> {
    let extension = path.extension()?.to_string_lossy().to_ascii_lowercase();
    let file = path.to_string_lossy().to_string();
    let parent = path.parent();
    match extension.as_str() {
        "json" => {
            let text = std::fs::read_to_string(path).ok()?;
            serde_json::from_str::<Value>(&text)
                .err()
                .map(|error| error.to_string())
        }
        "py" => checker_errors(python_program(), &["-m", "py_compile", &file], parent)
            .map(|text| last_lines(&text, 5)),
        "js" | "mjs" | "cjs" => {
            checker_errors("node", &["--check", &file], parent).map(|text| last_lines(&text, 6))
        }
        // rustfmt échoue aussi pour un simple écart de mise en forme, mais
        // n'écrit sur la sortie d'erreur que pour une erreur d'analyse.
        "rs" => checker_errors("rustfmt", &["--check", "--edition", "2021", &file], parent)
            .filter(|text| text.contains("error"))
            .map(|text| last_lines(&text, 12)),
        "go" => {
            checker_errors("gofmt", &["-e", "-l", &file], parent).map(|text| last_lines(&text, 6))
        }
        "sh" | "bash" => {
            checker_errors("sh", &["-n", &file], parent).map(|text| last_lines(&text, 5))
        }
        _ => None,
    }
}

/// Texte joint au résultat de l'outil quand le fichier écrit ne s'analyse pas.
#[must_use]
pub fn syntax_feedback(path: &str, error: &str) -> String {
    format!(
        "[Claw] Syntax check failed for `{path}` right after this change:\n{error}\nFix it now, before doing anything else."
    )
}

// --- Tests réécrits après un échec ----------------------------------------------------

/// `path` (dans le projet `root`) est un fichier de tests, d'après son nom ou
/// son dossier.
#[must_use]
pub fn is_test_file(path: &Path, root: &Path) -> bool {
    let relative = path.strip_prefix(root).unwrap_or(path);
    let normalized = relative
        .to_string_lossy()
        .replace('\\', "/")
        .to_ascii_lowercase();
    let mut parts = normalized.rsplit('/');
    let name = parts.next().unwrap_or_default();
    let stem = name.split('.').next().unwrap_or_default();
    name.starts_with("test_")
        || stem.ends_with("_test")
        || stem.ends_with("_tests")
        || name.contains(".test.")
        || name.contains(".spec.")
        || (relative.is_relative()
            && parts.any(|dir| matches!(dir, "tests" | "test" | "__tests__" | "spec")))
}

/// La modification retire ou change des lignes qui existaient dans le fichier
/// (lu dans le résultat de `edit_file` ou `write_file`) ; ajouter des lignes
/// ne compte pas.
#[must_use]
pub fn removes_existing_lines(tool_output: &str) -> bool {
    let Ok(value) = serde_json::from_str::<Value>(tool_output) else {
        return false;
    };
    let mut remaining: HashMap<&str, i64> = HashMap::new();
    let lines = value
        .get("structuredPatch")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|hunk| hunk.get("lines").and_then(Value::as_array))
        .flatten()
        .filter_map(Value::as_str);
    for line in lines {
        let (sign, text) = line.split_at(line.len().min(1));
        let text = text.trim();
        if text.is_empty() {
            continue;
        }
        match sign {
            "-" => *remaining.entry(text).or_insert(0) += 1,
            "+" => *remaining.entry(text).or_insert(0) -= 1,
            _ => {}
        }
    }
    remaining.values().any(|count| *count > 0)
}

/// Texte joint au résultat de l'outil quand un test existant est réécrit
/// après une vérification en échec.
#[must_use]
pub fn test_guard_feedback(name: &str) -> String {
    format!(
        "[Claw] You changed existing lines of `{name}`, which is used to check the work, after a failing check. A check describes the expected behavior: making it pass by rewriting it proves nothing. Fix the code instead and restore the check, unless the user asked you to change it or the check itself is wrong; in that case say so explicitly in your final answer, with the reason."
    )
}

// --- Exigences et escalade ------------------------------------------------------------

/// Message adressé au modèle qui dit avoir fini un travail conséquent : il
/// confronte son travail à chaque exigence avant de conclure (une fois par tour).
pub const CHECKLIST_PROMPT: &str = "[Claw] Before you finish, check your work against every requirement. List each part of the user's request (when their last message only asks you to continue, the request it continues): every feature, page, file, fix or explanation they asked for, plus the requirements of the docstrings, comments and specifications of the code you changed, and the edge cases these imply (empty or missing input, boundaries, invalid values, duplicates, ordering, large inputs). For each one, find the evidence that it is done: the file and the code that handle it, and the test or command output from this session that shows it works. An intention, a plan or a file you did not check is not evidence. Whatever is missing or unverified, do it now with your tools, in this reply: do not announce it, do not leave it for later, do not ask the user to do it. If the project has tests, add the missing test and run it; never describe tests, commands or results that do not exist, and never weaken or delete an existing test. Then end with your final answer to the user's original request, written as the report the system prompt describes under \"Final answer\" (the outcome first, what was wrong and why, what you changed, how it was verified, what is left for the user). Its verification part gives one short line per requirement with its evidence; it claims only what this check established and names plainly any part that is not done or not verified. The user reads only that last message and has not seen this check: it must stand on its own, cover everything they asked for, including any explanation or instructions they requested (such as how to install, run or test the result), be complete without padding, and not refer to this check.";

/// Fin des messages que Claw renvoie au modèle après un contrôle : la réponse
/// qui suivra est celle que l'utilisateur lira, elle doit répondre à sa demande.
pub const FINAL_ANSWER_RULE: &str = "When this is dealt with, end with your final answer to the user's original request. The user reads only that last message and has not seen this note: it must stand on its own, written as the report the system prompt describes under \"Final answer\" (the outcome first, what was wrong and why, what you changed, how it was verified, what is left for the user), cover everything they asked for, including any explanation or instructions they requested (such as how to install, run or test the result), claim only what the files and your command outputs show, be complete without padding, and not refer to this note.";

// --- Persévérance ----------------------------------------------------------------------

/// Échecs d'outils consécutifs à partir desquels un agent qui s'arrête est
/// renvoyé chercher une autre voie (une fois par tour).
pub const PERSISTENCE_FAILURES: u32 = 2;

/// Message adressé au modèle qui s'arrête juste après des échecs d'outils :
/// ne pas rendre la main à l'utilisateur tant qu'un outil peut encore servir.
#[must_use]
pub fn persistence_feedback(failures: u32) -> String {
    format!(
        "[Claw] Your last {failures} tool calls failed and you are about to stop. Do not hand the task back to the user while one of your tools can still do it. Say in one sentence what these failures tell you, then try a different way: fix the command, use another tool, take a smaller step or change approach. Only if nothing you can run would work, explain precisely what blocks you and what exactly the user must do. {FINAL_ANSWER_RULE}"
    )
}

/// Rappel de langue ajouté aux notes de Claw : elles sont en anglais, et un
/// modèle répond volontiers dans la langue de la dernière consigne lue.
#[must_use]
pub fn language_note(language: Option<&str>) -> String {
    match language.map(str::trim).filter(|language| !language.is_empty()) {
        Some(language) => format!(
            " Write your reply to the user in {language}, as the system prompt requires: this note is in English, your answer must not be."
        ),
        None => " Write your reply to the user in the language of their request, not in the language of this note.".to_string(),
    }
}

/// Préfixe des messages que Claw adresse au modèle à la place de
/// l'utilisateur (contrôles, rappels) : ils ne sont pas des demandes de sa part.
pub const CLAW_NOTE_PREFIX: &str = "[Claw]";

/// Note adressée au modèle plus fort qui reprend le tour.
#[must_use]
pub fn escalation_feedback(model: &str) -> String {
    format!(
        "[Claw] You are {model}, a stronger model taking over this turn because the previous model was stuck. Re-read the user's request and the latest failure above, find the root cause yourself (do not trust the previous model's assumptions), fix it, and verify before answering."
    )
}

// --- Erreurs vues par l'éditeur -------------------------------------------------------

/// Renvois au modèle, par tour, pour des erreurs que l'éditeur signale.
pub const MAX_EDITOR_ROUNDS: usize = 2;
/// Attente de la réponse de l'éditeur (il laisse ses analyses se stabiliser).
pub const EDITOR_CHECK_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);
const MAX_EDITOR_PROBLEMS: usize = 30;

/// Message adressé au modèle : les erreurs que l'éditeur signale dans les
/// fichiers qu'il vient de modifier.
#[must_use]
pub fn editor_feedback(editor: &str, files: &[crate::editor::FileDiagnostics]) -> String {
    format!(
        "[Claw] {editor} reports {} error(s) in the files you changed during this turn:\n\n{}\n\nFix the ones your changes caused or left behind. If one of them was already there before your changes and is unrelated to the request, say so instead of fixing it. {FINAL_ANSWER_RULE}",
        crate::editor::count(files),
        crate::editor::render(files, MAX_EDITOR_PROBLEMS)
    )
}

// --- Relecture et conseil par un autre modèle ----------------------------------------

/// En dessous, une modification est trop petite pour mériter une relecture.
pub const REVIEW_MIN_CHANGED_LINES: usize = 5;
const MAX_REVIEW_DIFF_CHARS: usize = 24_000;
const DIFF_CONTEXT: usize = 3;

/// Contenu d'un fichier avant sa première modification du tour, lu dans le
/// résultat de `edit_file` ou `write_file` (lignes « - » du correctif complet).
#[must_use]
pub fn original_content(tool_output: &str) -> Option<String> {
    let value: Value = serde_json::from_str(tool_output).ok()?;
    let hunks = value.get("structuredPatch")?.as_array()?;
    let mut lines = Vec::new();
    for hunk in hunks {
        for line in hunk.get("lines")?.as_array()? {
            if let Some(text) = line.as_str().and_then(|line| line.strip_prefix('-')) {
                lines.push(text.to_string());
            }
        }
    }
    Some(lines.join("\n"))
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Change {
    Same,
    Removed,
    Added,
}

fn line_changes<'a>(old: &[&'a str], new: &[&'a str]) -> Vec<(Change, &'a str)> {
    let mut table = vec![vec![0u32; new.len() + 1]; old.len() + 1];
    for i in (0..old.len()).rev() {
        for j in (0..new.len()).rev() {
            table[i][j] = if old[i] == new[j] {
                table[i + 1][j + 1] + 1
            } else {
                table[i + 1][j].max(table[i][j + 1])
            };
        }
    }
    let (mut i, mut j) = (0, 0);
    let mut changes = Vec::with_capacity(old.len() + new.len());
    while i < old.len() || j < new.len() {
        if i < old.len() && j < new.len() && old[i] == new[j] {
            changes.push((Change::Same, old[i]));
            i += 1;
            j += 1;
        } else if i < old.len() && (j == new.len() || table[i + 1][j] >= table[i][j + 1]) {
            changes.push((Change::Removed, old[i]));
            i += 1;
        } else {
            changes.push((Change::Added, new[j]));
            j += 1;
        }
    }
    changes
}

/// Diff unifié d'un fichier (trois lignes de contexte) et nombre de lignes
/// ajoutées ou retirées. Vide si rien n'a changé.
#[must_use]
pub fn unified_diff(path: &str, old: &str, new: &str) -> (String, usize) {
    let old_lines: Vec<&str> = old.lines().collect();
    let new_lines: Vec<&str> = new.lines().collect();
    if old_lines == new_lines {
        return (String::new(), 0);
    }
    let header = format!("--- a/{path}\n+++ b/{path}\n");
    if old_lines.len().saturating_mul(new_lines.len()) > 4_000_000 {
        let head = new_lines
            .iter()
            .take(80)
            .copied()
            .collect::<Vec<_>>()
            .join("\n");
        return (
            format!("{header}(file too large for a line diff; it now starts with)\n{head}\n"),
            old_lines.len().max(new_lines.len()),
        );
    }
    let changes = line_changes(&old_lines, &new_lines);
    let changed_lines = changes
        .iter()
        .filter(|(kind, _)| *kind != Change::Same)
        .count();
    // Numéros de ligne (ancien, nouveau) avant chaque changement.
    let mut positions = Vec::with_capacity(changes.len());
    let (mut old_number, mut new_number) = (1usize, 1usize);
    for (kind, _) in &changes {
        positions.push((old_number, new_number));
        match kind {
            Change::Same => {
                old_number += 1;
                new_number += 1;
            }
            Change::Removed => old_number += 1,
            Change::Added => new_number += 1,
        }
    }
    let mut out = header;
    let mut index = 0;
    while index < changes.len() {
        if changes[index].0 == Change::Same {
            index += 1;
            continue;
        }
        // Un bloc : les changements séparés par moins de deux contextes.
        let start = index.saturating_sub(DIFF_CONTEXT);
        let mut end = index;
        let mut cursor = index;
        while cursor < changes.len() && cursor <= end + 2 * DIFF_CONTEXT {
            if changes[cursor].0 != Change::Same {
                end = cursor;
            }
            cursor += 1;
        }
        let stop = (end + DIFF_CONTEXT + 1).min(changes.len());
        let block = &changes[start..stop];
        let old_count = block
            .iter()
            .filter(|(kind, _)| *kind != Change::Added)
            .count();
        let new_count = block
            .iter()
            .filter(|(kind, _)| *kind != Change::Removed)
            .count();
        let _ = writeln!(
            out,
            "@@ -{},{old_count} +{},{new_count} @@",
            positions[start].0, positions[start].1
        );
        for (kind, line) in block {
            out.push(match kind {
                Change::Same => ' ',
                Change::Removed => '-',
                Change::Added => '+',
            });
            out.push_str(line);
            out.push('\n');
        }
        index = stop;
    }
    (out, changed_lines)
}

/// Consignes du relecteur (un autre modèle que celui qui a écrit le code).
pub const REVIEW_SYSTEM: &str = "You are a strict, careful code reviewer. You are shown what a user asked for, the changes a coding agent made during this turn (unified diff) and the final answer the agent is about to give the user. Report only real problems that must be fixed before the work can be accepted: a bug, an unhandled edge case that the request or the docstrings require, a part of the request that was not done, a claim in the agent's answer that the changes contradict or do not support (a feature, page, file, test or result it says exists but that is neither in the diff nor plausibly done in an earlier turn), a test that was weakened, skipped or deleted, a security issue. Do not comment on style, naming or anything optional. Be concrete and brief. If the changes are acceptable, answer with exactly: LGTM. Otherwise answer with at most six lines, one problem per line, in the form `file:line - problem - what to change`.";

/// Taille gardée de la réponse de l'agent et de la demande antérieure
/// montrées au relecteur.
const MAX_REVIEW_TEXT_CHARS: usize = 4_000;

fn shortened(text: &str, limit: usize) -> String {
    let text = text.trim();
    if text.chars().count() <= limit {
        return text.to_string();
    }
    text.chars().take(limit).collect::<String>() + "\n… (shortened)"
}

/// Demande de relecture : la demande de l'utilisateur (avec celle qu'un
/// message court comme « reprendre » poursuit), la réponse que l'agent
/// s'apprête à donner, puis les diffs du tour.
#[must_use]
pub fn review_prompt(
    request: &str,
    earlier: Option<&str>,
    answer: &str,
    diffs: &[String],
) -> String {
    let mut body = diffs.join("\n");
    if body.chars().count() > MAX_REVIEW_DIFF_CHARS {
        body =
            body.chars().take(MAX_REVIEW_DIFF_CHARS).collect::<String>() + "\n… (diff shortened)";
    }
    let mut prompt = format!("The user asked:\n{}\n\n", request.trim());
    if let Some(earlier) = earlier.map(str::trim).filter(|text| !text.is_empty()) {
        let _ = write!(
            prompt,
            "Earlier request in this conversation, which the message above may continue:\n{}\n\n",
            shortened(earlier, MAX_REVIEW_TEXT_CHARS)
        );
    }
    let answer = answer.trim();
    let _ = write!(
        prompt,
        "The agent's final answer to the user:\n{}\n\nChanges made by the agent during this turn (earlier turns may have changed other files):\n{body}",
        if answer.is_empty() {
            "(none yet)".to_string()
        } else {
            shortened(answer, MAX_REVIEW_TEXT_CHARS)
        }
    );
    prompt
}

/// Remarques du relecteur, ou `None` s'il approuve (« LGTM »).
#[must_use]
pub fn review_findings(answer: &str) -> Option<String> {
    let text = answer.trim();
    let head: String = text
        .chars()
        .take(24)
        .collect::<String>()
        .to_ascii_uppercase();
    if text.is_empty() || head.contains("LGTM") {
        return None;
    }
    Some(text.lines().take(12).collect::<Vec<_>>().join("\n"))
}

/// Message renvoyé au modèle avec les remarques du relecteur.
#[must_use]
pub fn review_feedback(findings: &str) -> String {
    format!(
        "[Claw] Another model reviewed your changes and raised these points:\n{findings}\n\nCheck each point against the code and the user's request. Fix the ones that are real problems; ignore a point that is wrong and say why in one sentence. Do not rewrite what already works. {FINAL_ANSWER_RULE}"
    )
}

/// Consignes du conseiller appelé quand l'agent n'avance plus.
pub const UNBLOCK_SYSTEM: &str = "You are a senior engineer helping a coding agent that is stuck. You are shown the end of its session: the user's request, its recent tool calls and their outputs. Work out the most likely root cause of the failure and the single best next step. Be concrete: name files, functions or commands. Do not repeat what the agent already tried. Answer in at most 150 words.";

#[must_use]
pub fn unblock_prompt(transcript_tail: &str) -> String {
    format!("End of the agent's session (most recent last):\n\n{transcript_tail}")
}

/// Message renvoyé au modèle avec le conseil d'un autre modèle.
#[must_use]
pub fn unblock_feedback(advice: &str) -> String {
    format!(
        "[Claw] Advice from another model that read your recent attempts (a hypothesis to check, not an order):\n{}",
        advice.trim()
    )
}

#[cfg(test)]
mod language_tests {
    use super::language_note;

    #[test]
    fn notes_of_claw_remind_the_response_language() {
        let configured = language_note(Some(" français "));
        assert!(configured.contains("in français"));
        assert!(configured.contains("this note is in English, your answer must not be"));
        // Sans réglage : la langue de la demande de l'utilisateur.
        for missing in [None, Some("  ")] {
            assert!(language_note(missing).contains("in the language of their request"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        covers, detect_verify_commands, original_content, read_bash_output, review_findings,
        syntax_check, unified_diff, verify_commands, verify_feedback, StallGuard, VerifySetting,
    };
    use std::fs;

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_nanos());
        let dir = std::env::temp_dir().join(format!("claw-harness-{name}-{nanos}"));
        fs::create_dir_all(&dir).expect("dossier");
        dir
    }

    #[test]
    fn detects_the_verification_command_of_a_project() {
        let rust = temp_dir("rust");
        fs::write(rust.join("Cargo.toml"), "[package]\nname = \"x\"\n").expect("fichier");
        assert_eq!(detect_verify_commands(&rust), vec!["cargo check --quiet"]);

        let python = temp_dir("python");
        fs::write(python.join("test_stats.py"), "import unittest\n").expect("fichier");
        let commands = detect_verify_commands(&python);
        assert_eq!(commands.len(), 1);
        assert!(commands[0].ends_with("-m unittest discover -q"));

        let pytest = temp_dir("pytest");
        fs::write(pytest.join("conftest.py"), "").expect("fichier");
        assert!(detect_verify_commands(&pytest)[0].ends_with("-m pytest -q -x"));

        let node = temp_dir("node");
        fs::write(
            node.join("package.json"),
            r#"{"scripts":{"test":"vitest","typecheck":"tsc --noEmit"}}"#,
        )
        .expect("fichier");
        assert_eq!(
            detect_verify_commands(&node),
            vec!["npm run --silent typecheck"]
        );

        let empty = temp_dir("vide");
        assert!(detect_verify_commands(&empty).is_empty());
        assert!(verify_commands(&VerifySetting::Off, &rust).is_empty());
        assert_eq!(
            verify_commands(
                &VerifySetting::Commands(vec!["make test".to_string()]),
                &rust
            ),
            vec!["make test"]
        );
        for dir in [rust, python, pytest, node, empty] {
            let _ = fs::remove_dir_all(dir);
        }
    }

    #[test]
    fn reads_command_results_and_builds_the_feedback() {
        let passed = read_bash_output(r#"{"stdout":"ok","stderr":""}"#);
        assert!(passed.passed && !passed.timed_out);

        let failed = read_bash_output(
            r#"{"stdout":"","stderr":"error[E0308]: mismatched types","returnCodeInterpretation":"exit_code:101"}"#,
        );
        assert!(!failed.passed);
        assert!(failed.excerpt.contains("mismatched types"));
        assert!(failed.excerpt.contains("exit code 101"));
        let feedback = verify_feedback("cargo check --quiet", &failed);
        assert!(feedback.contains("$ cargo check --quiet"));
        assert!(feedback.contains("Never weaken"));

        let slow =
            read_bash_output(r#"{"stdout":"","stderr":"","returnCodeInterpretation":"timeout"}"#);
        assert!(slow.timed_out && !slow.passed);

        assert!(covers(
            "cd app && cargo  check --quiet",
            "cargo check --quiet"
        ));
        assert!(!covers("cargo fmt", "cargo check --quiet"));
    }

    #[test]
    fn long_outputs_keep_their_head_and_tail() {
        let stdout = (0..200)
            .map(|n| format!("ligne {n}"))
            .collect::<Vec<_>>()
            .join("\n");
        let output = serde_json::json!({
            "stdout": stdout, "stderr": "", "returnCodeInterpretation": "exit_code:1"
        })
        .to_string();
        let result = read_bash_output(&output);
        assert!(result.excerpt.contains("ligne 0"));
        assert!(result.excerpt.contains("ligne 199"));
        assert!(result.excerpt.contains("lines omitted"));
        assert!(!result.excerpt.contains("ligne 100\n"));
    }

    #[test]
    fn the_stall_guard_speaks_once_per_dead_end() {
        let mut guard = StallGuard::default();
        let input = r#"{"command":"cargo test"}"#;
        let failing = r#"{"stdout":"","stderr":"boom","returnCodeInterpretation":"exit_code:1"}"#;
        assert!(guard.observe("bash", input, failing, false).is_none());
        assert!(guard.observe("bash", input, failing, false).is_none());
        let reminder = guard
            .observe("bash", input, failing, false)
            .expect("troisième appel identique");
        assert!(reminder.to_model.contains("exact `bash` call 3 times"));
        assert!(reminder.notice.contains("3 appels identiques"));
        // Quatrième échec d'affilée : un autre rappel, pas le même.
        let other = guard
            .observe("bash", r#"{"command":"cargo build"}"#, failing, false)
            .expect("quatre échecs");
        assert!(other.to_model.contains("4 tool calls in a row have failed"));

        // Un résultat différent remet le compteur de répétition à zéro.
        let mut guard = StallGuard::default();
        assert!(guard.observe("read_file", "{}", "a", false).is_none());
        assert!(guard.observe("read_file", "{}", "b", false).is_none());
        assert!(guard.observe("read_file", "{}", "c", false).is_none());

        // Une vérification réussie relancée sans rien changer : il faut conclure.
        let mut guard = StallGuard::default();
        let tests = r#"{"command":"python -m unittest"}"#;
        let passing = r#"{"stdout":"OK","stderr":""}"#;
        assert!(guard.observe("bash", tests, passing, false).is_none());
        let reminder = guard
            .observe(
                "bash",
                r#"{"command":"python  -m unittest"}"#,
                passing,
                false,
            )
            .expect("deuxième réussite sans changement");
        assert!(reminder
            .to_model
            .contains("already succeeded and nothing has changed"));
        // Après une modification, relancer les tests est normal.
        assert!(guard.observe("edit_file", "{}", "{}", false).is_none());
        assert!(guard.observe("bash", tests, passing, false).is_none());
        // Une commande ordinaire répétée n'est pas une vérification.
        let mut guard = StallGuard::default();
        let status = r#"{"command":"git status"}"#;
        assert!(guard.observe("bash", status, "{}", false).is_none());
        assert!(guard.observe("bash", status, "{}", false).is_none());

        // Deux modifications ratées du même fichier.
        let mut guard = StallGuard::default();
        let edit = r#"{"path":"src/lib.rs","old_string":"x","new_string":"y"}"#;
        assert!(guard
            .observe("edit_file", edit, "old_string not found", true)
            .is_none());
        let reminder = guard
            .observe(
                "edit_file",
                r#"{"path":"src/lib.rs","old_string":"z","new_string":"y"}"#,
                "old_string not found",
                true,
            )
            .expect("deuxième modification ratée");
        assert!(reminder
            .to_model
            .contains("second failed edit of `src/lib.rs`"));
    }

    #[test]
    fn builds_a_unified_diff_with_context() {
        let old = "un\ndeux\ntrois\nquatre\ncinq\nsix\nsept\nhuit\nneuf\ndix\n";
        let new = "un\ndeux\ntrois\nquatre bis\ncinq\nsix\nsept\nhuit\nneuf\ndix\nonze\n";
        let (diff, changed) = unified_diff("src/a.txt", old, new);
        assert_eq!(changed, 3);
        assert!(diff.starts_with("--- a/src/a.txt\n+++ b/src/a.txt\n@@ -1,7 +1,7 @@\n"));
        assert!(diff.contains("-quatre\n+quatre bis\n"));
        assert!(diff.contains("@@ -8,3 +8,4 @@\n huit\n neuf\n dix\n+onze\n"));
        assert_eq!(unified_diff("a", "x\n", "x\n"), (String::new(), 0));
        // Fichier créé : tout est ajouté.
        let (created, lines) = unified_diff("b.py", "", "a = 1\nb = 2\n");
        assert_eq!(lines, 2);
        assert!(created.contains("@@ -1,0 +1,2 @@\n+a = 1\n+b = 2\n"));
    }

    #[test]
    fn reads_the_original_file_and_the_review_verdict() {
        let output = r#"{"filePath":"a.py","structuredPatch":[{"oldStart":1,"oldLines":2,"newStart":1,"newLines":1,"lines":["-x = 1","-y = 2","+x = 3"]}]}"#;
        assert_eq!(original_content(output).as_deref(), Some("x = 1\ny = 2"));
        assert_eq!(
            original_content(r#"{"structuredPatch":[{"lines":["+nouveau"]}]}"#).as_deref(),
            Some("")
        );
        assert_eq!(original_content("pas du json"), None);

        assert_eq!(review_findings("LGTM"), None);
        assert_eq!(review_findings("  lgtm. Nothing to fix."), None);
        assert_eq!(review_findings(""), None);
        assert_eq!(
            review_findings("a.py:3 - division by zero - guard the empty list").as_deref(),
            Some("a.py:3 - division by zero - guard the empty list")
        );
    }

    #[test]
    fn recognizes_test_files_inside_the_project_only() {
        let root = std::path::Path::new("/home/moi/test/projet");
        for path in [
            "test_commande.py",
            "src/outils_test.go",
            "web/bouton.test.tsx",
            "web/bouton.spec.ts",
            "tests/integration.rs",
            "src/__tests__/panier.js",
        ] {
            assert!(super::is_test_file(&root.join(path), root), "{path}");
        }
        // Un dossier « test » au-dessus du projet ne compte pas.
        for path in ["commande.py", "src/contest.py", "docs/attestation.md"] {
            assert!(!super::is_test_file(&root.join(path), root), "{path}");
        }
        assert!(super::test_guard_feedback("test_x.py").contains("`test_x.py`"));

        // Le correctif liste tout l'ancien fichier : seul un vrai retrait compte.
        let patch = |lines: &[&str]| {
            serde_json::json!({ "structuredPatch": [{ "lines": lines }] }).to_string()
        };
        assert!(super::removes_existing_lines(&patch(&[
            "-def test_total():",
            "-    assert total([1, 2]) == 3",
            "+def test_total():",
            "+    assert True",
        ])));
        assert!(!super::removes_existing_lines(&patch(&[
            "-def test_total():",
            "-    assert total([1, 2]) == 3",
            "+def test_total():",
            "+    assert total([1, 2]) == 3",
            "+",
            "+def test_vide():",
            "+    assert total([]) == 0",
        ])));
        assert!(!super::removes_existing_lines(&patch(&[
            "+nouveau fichier"
        ])));
        assert!(!super::removes_existing_lines("pas du JSON"));
    }

    #[test]
    fn json_syntax_is_checked_without_any_tool() {
        let dir = temp_dir("syntaxe");
        let good = dir.join("bon.json");
        let bad = dir.join("mauvais.json");
        fs::write(&good, r#"{"a": 1}"#).expect("fichier");
        fs::write(&bad, "{\"a\": 1,\n}").expect("fichier");
        assert_eq!(syntax_check(&good), None);
        assert!(syntax_check(&bad).expect("erreur").contains("line 2"));
        assert_eq!(syntax_check(&dir.join("notes.txt")), None);
        let _ = fs::remove_dir_all(dir);
    }
}
