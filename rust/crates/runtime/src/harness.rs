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

use crate::edge_cases::feature_edge_cases;

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
    /// Déroulé ultracode (dossier préparé, vérificateur indépendant), quand il est demandé.
    Ultracode,
    /// Mode équipe : chaque étape confiée au modèle le plus fort pour elle, quand il est demandé.
    Team,
    /// Code des emplacements cités par une commande ou une vérification en échec.
    ErrorSites,
    /// Questions écrites en fin de réponse reposées avec le menu à choix.
    Questions,
    /// Méthode de débogage jointe à un signalement de problème, et preuves
    /// demandées avant de le dire résolu.
    ProblemGuide,
    /// Commandes conseillées à l'utilisateur sans avoir été lancées.
    TriedCommands,
    /// Réponse arrêtée sur des titres sans contenu : le travail annoncé est à faire.
    EmptyTitles,
    /// Contenu abrégé (« ... rest of the code unchanged ... ») refusé avant
    /// d'écraser un fichier.
    PlaceholderGuard,
    /// Réponse en texte coupée par la limite de sortie : la suite est demandée.
    CutReplies,
    /// Lignes d'un fichier de données réécrites : corriger le code qui les lit.
    DataGuard,
    /// Fichier montré après une modification : inutile de le relire.
    FileView,
    /// Essais lancés un par réponse, liste de tâches envoyée seule : à regrouper.
    BatchCalls,
    /// Compte rendu gardé quand la réponse à un contrôle n'en est qu'un accusé
    /// de réception.
    KeptAnswer,
    /// Fichier relu sans avoir changé : une seule copie gardée, et dit au modèle.
    RepeatedReads,
    /// Ce qu'un ingénieur vérifie d'office pour le genre de fonctionnalité
    /// demandé : joint à la demande, puis rappelé au contrôle des exigences.
    EdgeCases,
    /// Fichier que le modèle avait modifié, supprimé ou ramené à son état du
    /// début du tour par une commande : sa dernière version est gardée.
    LostEdits,
    /// Script Python muet qui définit `main()` sans l'appeler : dit au modèle.
    EntryPoint,
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
            Self::Ultracode => "CLAW_ULTRACODE",
            Self::Team => "CLAW_TEAM",
            Self::ErrorSites => "CLAW_ERROR_SITES",
            Self::Questions => "CLAW_QUESTIONS",
            Self::ProblemGuide => "CLAW_PROBLEM_GUIDE",
            Self::TriedCommands => "CLAW_TRIED_COMMANDS",
            Self::EmptyTitles => "CLAW_EMPTY_TITLES",
            Self::PlaceholderGuard => "CLAW_PLACEHOLDER_GUARD",
            Self::CutReplies => "CLAW_CUT_REPLIES",
            Self::DataGuard => "CLAW_DATA_GUARD",
            Self::FileView => "CLAW_FILE_VIEW",
            Self::BatchCalls => "CLAW_BATCH_CALLS",
            Self::KeptAnswer => "CLAW_KEPT_ANSWER",
            Self::RepeatedReads => "CLAW_REPEATED_READS",
            Self::EdgeCases => "CLAW_EDGE_CASES",
            Self::LostEdits => "CLAW_LOST_EDITS",
            Self::EntryPoint => "CLAW_ENTRY_POINT",
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
    verify_commands_for(setting, root, &[])
}

/// Commandes de vérification du projet, connaissant les fichiers modifiés
/// pendant le tour. Quand la racine ne porte aucun manifeste (projet rangé en
/// `backend/` et `frontend/`), la vérification est celle du dossier le plus
/// proche de chaque fichier modifié qui en porte un, lancée depuis ce dossier
/// (`cd frontend && npx tsc --noEmit`). Sans fichier connu, celle des
/// sous-dossiers directs de la racine.
#[must_use]
pub fn verify_commands_for(
    setting: &VerifySetting,
    root: &Path,
    changed: &[String],
) -> Vec<String> {
    match setting {
        VerifySetting::Off => return Vec::new(),
        VerifySetting::Commands(commands) => return commands.clone(),
        VerifySetting::Auto => {}
    }
    let at_root = detect_verify_commands(root);
    if changed.is_empty() && !at_root.is_empty() {
        return at_root;
    }
    let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let mut folders: Vec<std::path::PathBuf> = Vec::new();
    // Une racine qui a sa vérification ne la donne pas à un sous-projet
    // modifié (`frontend/` à côté d'une API Python) : chaque fichier est
    // vérifié par le dossier le plus proche qui en a une, la racine comprise.
    let mut root_checked = false;
    for file in changed {
        let file = Path::new(file);
        let file = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
        let candidates: Vec<&Path> = file
            .ancestors()
            .skip(1)
            .take_while(|dir| dir.starts_with(&root))
            .filter(|dir| !detect_verify_commands(dir).is_empty())
            .collect();
        // Le plus proche, hors d'un dossier de tests : `backend/tests/conftest.py`
        // ne fait pas de `tests/` le projet (lancés de là, les tests ne
        // trouvent plus `app`), le projet est `backend/`.
        let folder = candidates
            .iter()
            .find(|dir| !in_tests_dir(dir, &root))
            .or_else(|| candidates.first())
            .copied();
        if folder == Some(root.as_path()) {
            root_checked = true;
        } else if let Some(folder) = folder {
            if !folders.iter().any(|known| known == folder) {
                folders.push(folder.to_path_buf());
            }
        }
    }
    if changed.is_empty() {
        let mut found: Vec<_> = std::fs::read_dir(&root)
            .into_iter()
            .flatten()
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| {
                let name = path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned());
                path.is_dir()
                    && name.is_some_and(|name| !crate::file_ops::generated_dir(path, &name, &root))
                    && !detect_verify_commands(path).is_empty()
            })
            .collect();
        found.sort();
        folders = found;
    }
    let mut commands = if root_checked { at_root } else { Vec::new() };
    commands.extend(folders.iter().flat_map(|folder| {
        let relative = folder
            .strip_prefix(&root)
            .unwrap_or(folder)
            .to_string_lossy()
            .replace('\\', "/");
        let place = if relative.contains(' ') {
            format!("cd \"{relative}\"")
        } else {
            format!("cd {relative}")
        };
        detect_verify_commands(folder)
            .into_iter()
            .map(move |command| format!("{place} && {command}"))
    }));
    commands
}

/// `dir` est dans un dossier de tests du projet (`tests/`, `__tests__/`...).
fn in_tests_dir(dir: &Path, root: &Path) -> bool {
    dir.strip_prefix(root).is_ok_and(|relative| {
        relative.components().any(|part| {
            matches!(
                part.as_os_str()
                    .to_string_lossy()
                    .to_ascii_lowercase()
                    .as_str(),
                "tests" | "test" | "__tests__" | "spec" | "specs"
            )
        })
    })
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

/// Fichiers de test Python du projet : à la racine, dans `tests/` ou `test/`
/// et leurs sous-dossiers directs (au plus `MAX_TEST_FILES_READ`).
fn python_test_files(root: &Path) -> Vec<std::path::PathBuf> {
    let mut dirs = vec![root.to_path_buf()];
    for name in ["tests", "test"] {
        let dir = root.join(name);
        if let Ok(entries) = std::fs::read_dir(&dir) {
            dirs.extend(
                entries
                    .flatten()
                    .map(|entry| entry.path())
                    .filter(|path| path.is_dir()),
            );
        }
        dirs.push(dir);
    }
    dirs.iter()
        .filter_map(|dir| std::fs::read_dir(dir).ok())
        .flat_map(|entries| entries.flatten().map(|entry| entry.path()))
        .filter(|path| {
            path.file_name()
                .map(|name| name.to_string_lossy().to_ascii_lowercase())
                .and_then(|name| name.strip_suffix(".py").map(str::to_string))
                .is_some_and(|stem| stem.starts_with("test_") || stem.ends_with("_test"))
        })
        .take(MAX_TEST_FILES_READ)
        .collect()
}

/// Fichiers de test lus, au plus, pour savoir pour quel lanceur ils sont écrits.
const MAX_TEST_FILES_READ: usize = 40;

/// Les tests sont écrits pour pytest : un fichier l'importe, ou ses tests sont
/// de simples fonctions `test_...`, que `unittest` ne trouve pas.
fn tests_written_for_pytest(root: &Path) -> bool {
    python_test_files(root).iter().any(|path| {
        std::fs::read_to_string(path).is_ok_and(|text| {
            text.lines().any(|line| {
                line.starts_with("import pytest")
                    || line.starts_with("from pytest")
                    || line.starts_with("def test_")
                    || line.starts_with("async def test_")
            })
        })
    })
}

/// Délai accordé à `python -m pytest --version`.
const PYTEST_PROBE_TIMEOUT: Duration = Duration::from_secs(10);

/// pytest est installé pour ce Python : `python -m pytest --version` réussit.
/// Essayé une fois par processus (une seconde environ).
fn pytest_installed(python: &str) -> bool {
    static INSTALLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *INSTALLED.get_or_init(|| {
        let Some(executable) = crate::paths::find_executable(python) else {
            return false;
        };
        let Ok(mut child) = Command::new(executable)
            .args(["-m", "pytest", "--version"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        else {
            return false;
        };
        let started = Instant::now();
        loop {
            match child.try_wait() {
                Ok(Some(status)) => return status.success(),
                Ok(None) if started.elapsed() < PYTEST_PROBE_TIMEOUT => {
                    std::thread::sleep(Duration::from_millis(50));
                }
                _ => {
                    let _ = child.kill();
                    return false;
                }
            }
        }
    })
}

/// Le dossier, ou l'un de ses sous-dossiers directs, porte des fichiers de test.
fn holds_tests(dir: &Path) -> bool {
    has_test_files(dir)
        || std::fs::read_dir(dir).is_ok_and(|entries| {
            entries
                .flatten()
                .any(|entry| entry.path().is_dir() && has_test_files(&entry.path()))
        })
}

fn python_verify(root: &Path) -> Option<String> {
    let python = python_program();
    let in_test_dirs = ["tests", "test"]
        .iter()
        .any(|dir| holds_tests(&root.join(dir)));
    // Sans fichier de test, pytest ne trouve rien et sort en échec (code 5) :
    // le modèle serait renvoyé « corriger » un travail sain.
    if !in_test_dirs && !has_test_files(root) {
        return None;
    }
    // pytest réglé pour ce projet, ou nommé parmi ses dépendances.
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
            })
        || [
            "pyproject.toml",
            "requirements.txt",
            "requirements-dev.txt",
            "setup.cfg",
        ]
        .iter()
        .any(|name| {
            std::fs::read_to_string(root.join(name))
                .is_ok_and(|text| text.to_ascii_lowercase().contains("pytest"))
        });
    // Des tests écrits pour pytest, sans réglage : `unittest` n'y trouverait
    // aucun test (essai réel t4 : « aucun test n'a été trouvé ; le travail
    // n'est pas vérifié », pour douze tests pytest qui passaient).
    if pytest_configured || (tests_written_for_pytest(root) && pytest_installed(python)) {
        return Some(format!("{python} -m pytest -q -x"));
    }
    if has_test_files(root) {
        return Some(format!("{python} -m unittest discover -q"));
    }
    ["tests", "test"]
        .iter()
        .find(|dir| holds_tests(&root.join(dir)))
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
/// qui la contient : « cd app && cargo check --quiet »). Pour une vérification
/// lancée depuis un sous-dossier (`cd frontend && npx tsc --noEmit`), il
/// suffit que sa commande contienne la vérification et nomme ce dossier, quel
/// que soit le chemin qu'il a écrit pour s'y rendre.
#[must_use]
pub fn covers(model_command: &str, verify_command: &str) -> bool {
    let (ran, wanted) = (squeeze(model_command), squeeze(verify_command));
    if wanted.is_empty() {
        return false;
    }
    // Lancée, et son code de sortie est celui de la commande : après
    // `npx tsc --noEmit || echo "erreurs"`, rien n'est vérifié.
    let decided = |check: &str| {
        ran.match_indices(check)
            .any(|(at, _)| crate::bash::exit_decided_at(&ran, at + check.len()))
    };
    if decided(&wanted) {
        return true;
    }
    wanted
        .strip_prefix("cd ")
        .and_then(|rest| rest.split_once(" && "))
        .is_some_and(|(folder, check)| {
            let folder = folder.trim_matches('"');
            let name = folder.rsplit('/').next().unwrap_or(folder);
            ran.replace('\\', "/").contains(name) && decided(check)
        })
}

/// La dernière vérification de la commande décide de son code de sortie :
/// rien après elle ne masque son échec (`|| echo`, `; echo`, `| wc -l`).
#[must_use]
pub fn exit_reflects_check(command: &str) -> bool {
    let lower = command.to_ascii_lowercase();
    let operators = crate::bash::shell_operators(&lower);
    let mut start = 0;
    let mut check_end = None;
    for range in operators
        .iter()
        .map(|(range, _)| range.clone())
        .chain(std::iter::once(lower.len()..lower.len()))
    {
        if segment_is_check(lower[start..range.start].trim()) {
            check_end = Some(range.start);
        }
        start = range.end;
    }
    check_end.is_some_and(|end| crate::bash::exit_decided_at(&lower, end))
}

/// Issue d'une commande lancée par l'outil `bash`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandResult {
    pub passed: bool,
    pub timed_out: bool,
    /// Une partie tourne encore en arrière-plan (serveur, commande longue,
    /// processus lancé avec `&`) : son résultat n'est pas connu, elle ne
    /// vérifie rien.
    pub pending: bool,
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

/// La vérification a échoué parce que son outil n'est pas installé (pytest
/// absent de l'environnement, commande introuvable) : rien à corriger dans le
/// travail.
#[must_use]
pub fn check_tool_is_missing(output: &str) -> bool {
    let lower = output.to_ascii_lowercase();
    lower.contains("no module named pytest")
        || lower.contains("no module named 'pytest'")
        || (lower.contains("command not found") && lower.contains("(exit code 127)"))
}

/// La vérification n'a rien vérifié : elle n'a trouvé aucun test (pytest
/// sort alors avec le code 5, jest sans `--passWithNoTests` en échec).
#[must_use]
pub fn no_tests_ran(output: &str) -> bool {
    let lower = output.to_ascii_lowercase();
    [
        "no tests ran",
        "collected 0 items",
        "no tests found",
        "no tests were found",
    ]
    .iter()
    .any(|mark| lower.contains(mark))
}

/// Extensions des fichiers de code dont un changement par une commande se
/// vérifie (un journal ou une base de données écrits par un serveur ne
/// comptent pas).
const CODE_EXTENSIONS: &[&str] = &[
    "rs", "py", "js", "jsx", "mjs", "cjs", "ts", "tsx", "go", "java", "kt", "c", "h", "cc", "cpp",
    "hpp", "cs", "rb", "php", "swift", "vue", "svelte", "css", "scss", "html", "sh", "toml",
    "json", "yaml", "yml",
];

/// État des fichiers de code que `git status` signale, avec leur date et leur
/// taille : ce qu'une commande change ensuite (`sed -i`, `cat > f <<EOF`, un
/// script Python) se voit en comparant deux états.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProjectSnapshot(
    std::collections::BTreeMap<String, Option<(std::time::SystemTime, u64)>>,
);

/// L'état des fichiers de code du dépôt git de `root` ; `None` hors d'un
/// dépôt (ou sans git). Dépendances, caches et sorties de build sont écartés.
#[must_use]
pub fn project_snapshot(root: &Path) -> Option<ProjectSnapshot> {
    let output = Command::new("git")
        .args(["status", "--porcelain=v1", "-uall", "-z"])
        .current_dir(root)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let mut files = std::collections::BTreeMap::new();
    let mut entries = output.stdout.split(|byte| *byte == 0);
    while let Some(entry) = entries.next() {
        let entry = String::from_utf8_lossy(entry);
        let Some(path) = entry.get(3..).filter(|path| !path.is_empty()) else {
            continue;
        };
        // Un renommage porte ensuite l'ancien chemin.
        if entry.starts_with('R') || entry.starts_with('C') {
            entries.next();
        }
        let relative = Path::new(path);
        let code = relative
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| {
                CODE_EXTENSIONS.contains(&extension.to_ascii_lowercase().as_str())
            });
        if !code || crate::file_ops::under_generated_dir(root, relative) {
            continue;
        }
        let state = std::fs::metadata(root.join(relative))
            .ok()
            .and_then(|meta| Some((meta.modified().ok()?, meta.len())));
        files.insert(path.to_string(), state);
    }
    Some(ProjectSnapshot(files))
}

/// Fichiers de code ajoutés, modifiés, supprimés ou rétablis depuis `before`
/// (chemins sous `root`).
#[must_use]
pub fn changed_since(before: &ProjectSnapshot, root: &Path) -> Vec<String> {
    let Some(now) = project_snapshot(root) else {
        return Vec::new();
    };
    let mut changed: Vec<String> = now
        .0
        .iter()
        .filter(|(path, state)| before.0.get(*path) != Some(state))
        .map(|(path, _)| path.clone())
        .chain(
            before
                .0
                .keys()
                .filter(|path| !now.0.contains_key(*path))
                .cloned(),
        )
        .map(|path| root.join(path).to_string_lossy().into_owned())
        .collect();
    changed.sort();
    changed.dedup();
    changed
}

/// Lit le résultat JSON de l'outil `bash` (stdout, stderr, code de sortie).
#[must_use]
pub fn read_bash_output(output: &str) -> CommandResult {
    // Le JSON du résultat, suivi éventuellement de remarques de Claw.
    let Some(Ok(value)) = serde_json::Deserializer::from_str(output)
        .into_iter::<Value>()
        .next()
    else {
        return CommandResult {
            passed: false,
            timed_out: false,
            pending: false,
            excerpt: excerpt(output),
        };
    };
    let text = |key: &str| value.get(key).and_then(Value::as_str).unwrap_or_default();
    let status = text("returnCodeInterpretation");
    let pending = !text("backgroundTaskId").is_empty();
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
        pending,
        excerpt: excerpt(&combined),
    }
}

// --- Code cité par une erreur ---------------------------------------------------------

/// Emplacements montrés au plus pour une sortie en échec.
const ERROR_SITES: usize = 3;
/// Lignes montrées avant et après la ligne citée.
const ERROR_SITE_LINES: usize = 3;
const ERROR_SITES_MAX_CHARS: usize = 3_000;

fn location_pattern() -> &'static regex::Regex {
    static PATTERN: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    PATTERN.get_or_init(|| {
        regex::Regex::new(
            r#"File "(?P<py>[^"]+)", line (?P<pyline>\d+)|(?P<path>(?:[A-Za-z]:)?[\w.\-/\\]*[\w\-]\.[A-Za-z][A-Za-z0-9]{0,5})(?::(?P<line>\d+)|\((?P<tsline>\d+),\d+\))"#,
        )
        .expect("motif valide")
    })
}

/// Emplacements `fichier:ligne` qu'une sortie cite (trace Python, erreur Rust,
/// TypeScript `fichier(ligne,colonne)`, Node, Go, pytest…), sans doublon, les
/// plus utiles d'abord : dans l'ordre de la sortie (première erreur d'un
/// compilateur, cadre le plus profond d'une pile Node), sauf pour une trace
/// Python (« most recent call last »), lue depuis la fin, où est le cadre qui
/// lève l'erreur.
fn cited_sites(output: &str) -> Vec<(String, usize)> {
    let mut seen = std::collections::HashSet::new();
    let mut sites: Vec<(String, usize)> = Vec::new();
    for found in location_pattern().captures_iter(output) {
        let (path, line) = match (found.name("py"), found.name("pyline")) {
            (Some(path), Some(line)) => (path.as_str(), line.as_str()),
            _ => match (
                found.name("path"),
                found.name("line").or_else(|| found.name("tsline")),
            ) {
                (Some(path), Some(line)) => (path.as_str(), line.as_str()),
                _ => continue,
            },
        };
        // Adresse web (`https://cdn.exemple.com/lib.js:8080`) : pas un fichier.
        let before = found.get(0).map_or("", |whole| &output[..whole.start()]);
        if path.contains("://") || before.ends_with("://") || before.ends_with(":/") {
            continue;
        }
        let Ok(line) = line.parse::<usize>() else {
            continue;
        };
        let site = (path.to_string(), line);
        if line > 0 && seen.insert(site.clone()) {
            sites.push(site);
        }
    }
    if output.contains("most recent call last") {
        sites.reverse();
    }
    sites
}

/// Emplacements cités dont on cherche au plus le fichier : une sortie de
/// milliers de lignes `fichier:ligne` ne coûte pas des milliers d'accès disque.
const MAX_SITE_LOOKUPS: usize = 200;

/// Fichier du projet que désigne un chemin cité (relatif au dossier de
/// travail ou absolu dedans), hors dépendances et sorties de build.
fn project_file(
    root: &Path,
    cited: &str,
    command_dir: Option<&Path>,
) -> Option<(std::path::PathBuf, String)> {
    // `//hôte/…` (reste d'une adresse web) : un chemin réseau sous Windows, que
    // l'on ne va pas chercher sur le réseau.
    if cited.starts_with("//") || cited.starts_with(r"\\") {
        return None;
    }
    let cited = cited.trim_start_matches("./");
    let path = Path::new(cited);
    // Un chemin relatif part du dossier de travail, ou du dossier où la
    // commande s'est placée (`cd frontend && npm run build` cite `src/app.tsx`).
    let full = if path.is_absolute() {
        path.canonicalize().ok()?
    } else {
        std::iter::once(root)
            .chain(command_dir)
            .find_map(|base| base.join(path).canonicalize().ok())?
    };
    let relative = full.strip_prefix(root).ok()?.to_path_buf();
    // Dépendances, caches, sorties de build : une trace qui y passe n'y montre
    // rien d'utile à corriger (un dossier `build` de vrai code est gardé).
    if crate::file_ops::under_generated_dir(root, &relative) || !full.is_file() {
        return None;
    }
    let shown = relative.to_string_lossy().replace('\\', "/");
    Some((full, shown))
}

/// Dossier où une commande se place d'abord (`cd frontend && …`, `cd /d
/// D:\projet\backend && …`), s'il est dans le projet.
fn command_dir(command: &str, root: &Path) -> Option<std::path::PathBuf> {
    static PATTERN: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let pattern = PATTERN.get_or_init(|| {
        regex::Regex::new(r#"^\s*cd\s+(?:/[dD]\s+)?(?:"([^"]+)"|'([^']+)'|([^\s;&|]+))"#)
            .expect("motif valide")
    });
    let found = pattern.captures(command)?;
    let folder = found
        .get(1)
        .or_else(|| found.get(2))
        .or_else(|| found.get(3))?
        .as_str();
    let folder = Path::new(folder);
    let full = if folder.is_absolute() {
        folder.to_path_buf()
    } else {
        root.join(folder)
    };
    let full = full.canonicalize().ok()?;
    (full.starts_with(root) && full != root).then_some(full)
}

/// Le code autour des emplacements du projet qu'une sortie en échec cite
/// (`output` : le résultat JSON de l'outil `bash`, ou du texte brut ;
/// `command` : la commande lancée, pour les chemins cités depuis le dossier
/// où elle s'est placée). Le modèle voit les lignes en cause sans dépenser une
/// requête pour les relire.
#[must_use]
pub fn error_sites(output: &str, cwd: &Path, command: &str) -> Option<String> {
    let text = serde_json::from_str::<Value>(output).map_or_else(
        |_| output.to_string(),
        |value| {
            let field = |key: &str| value.get(key).and_then(Value::as_str).unwrap_or_default();
            format!("{}\n{}", field("stdout"), field("stderr"))
        },
    );
    let root = cwd.canonicalize().ok()?;
    let placed = command_dir(command, &root);
    let mut blocks: Vec<String> = Vec::new();
    let mut used = 0;
    for (cited, line) in cited_sites(&text).into_iter().take(MAX_SITE_LOOKUPS) {
        if blocks.len() >= ERROR_SITES {
            break;
        }
        let Some((file, shown)) = project_file(&root, &cited, placed.as_deref()) else {
            continue;
        };
        let Some(content) = crate::file_ops::read_any_text(&file) else {
            continue;
        };
        let lines: Vec<&str> = content.lines().collect();
        if line > lines.len() {
            continue;
        }
        let start = line.saturating_sub(ERROR_SITE_LINES + 1);
        let end = (line + ERROR_SITE_LINES).min(lines.len());
        let mut block = format!("{shown}:{line}\n");
        for (offset, code) in lines[start..end].iter().enumerate() {
            let number = start + offset + 1;
            let marker = if number == line { '>' } else { ' ' };
            let code: String = code.chars().take(200).collect();
            let _ = writeln!(block, "{marker}{number:>5} | {code}");
        }
        used += block.len();
        if used > ERROR_SITES_MAX_CHARS {
            break;
        }
        blocks.push(block);
    }
    (!blocks.is_empty()).then(|| {
        format!(
            "[Claw] Current code at the project locations this output points to (no need to read them again):\n{}",
            blocks.join("\n")
        )
    })
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

/// La commande est une vérification (tests, compilation, analyse). Jugée sur
/// le programme lancé et ses premiers arguments, commande simple par commande
/// simple : un mot comme « test » dans un chemin (`cd D:\test_claw\backend`)
/// ou dans un nom de fichier ne fait pas d'une commande une vérification.
#[must_use]
pub fn is_check_command(command: &str) -> bool {
    crate::bash::shell_segments(&command.to_ascii_lowercase())
        .iter()
        .any(|(segment, _)| segment_is_check(segment))
}

/// Mots d'un script ou d'une cible qui en font une vérification (`npm run
/// test:unit`, `make check`).
fn names_a_check(word: &str) -> bool {
    [
        "test", "check", "build", "lint", "tsc", "type", "compile", "verify", "vet", "clippy",
    ]
    .iter()
    .any(|check| word.contains(check))
}

fn segment_is_check(segment: &str) -> bool {
    let mut words = command_words(segment);
    // Lanceurs : la vérification est le programme qu'ils lancent.
    loop {
        let skip = match words.as_slice() {
            [first, ..]
                if matches!(first.as_str(), "npx" | "bunx" | "time" | "nice" | "winpty") =>
            {
                1
            }
            [first, second, ..]
                if (matches!(first.as_str(), "uv" | "poetry" | "pipenv" | "pdm" | "hatch")
                    && second == "run")
                    || (matches!(first.as_str(), "pnpm" | "bundle") && second == "exec") =>
            {
                2
            }
            _ => break,
        };
        words.drain(..skip);
    }
    let Some(program) = words.first() else {
        return false;
    };
    // `./gradlew`, `node_modules/.bin/tsc`, `pytest.exe` : le nom seul.
    let program = program.rsplit(['/', '\\']).next().unwrap_or(program);
    let program = [".exe", ".cmd", ".bat", ".sh"]
        .iter()
        .find_map(|suffix| program.strip_suffix(suffix))
        .unwrap_or(program);
    let argument = |index: usize| words.get(index).map_or("", String::as_str);
    match program {
        "pytest" | "py.test" | "tox" | "nox" | "tsc" | "eslint" | "jest" | "vitest" | "mocha"
        | "mypy" | "ruff" | "flake8" | "pylint" | "pyright" | "phpunit" | "rspec" | "ctest"
        | "playwright" | "cypress" => true,
        "python" => matches!(
            argument(1),
            "pytest"
                | "unittest"
                | "mypy"
                | "ruff"
                | "flake8"
                | "pylint"
                | "compileall"
                | "py_compile"
                | "tox"
        ),
        "cargo" => matches!(
            argument(1),
            "test" | "check" | "build" | "clippy" | "nextest" | "bench"
        ),
        "go" => matches!(argument(1), "test" | "build" | "vet"),
        "dotnet" => matches!(argument(1), "test" | "build"),
        "npm" | "pnpm" | "yarn" | "bun" => names_a_check(if argument(1) == "run" {
            argument(2)
        } else {
            argument(1)
        }),
        // `make` seul construit ; avec une cible, c'est elle qui décide.
        "make" => words.len() == 1 || words[1..].iter().any(|word| names_a_check(word)),
        "mvn" | "gradle" | "gradlew" | "rake" | "rails" | "php" | "deno" | "cmake" => {
            words[1..].iter().any(|word| names_a_check(word))
        }
        _ => false,
    }
}

pub(crate) fn fingerprint(text: &str) -> u64 {
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
        if let Some(command) = command
            .filter(|command| !failed && is_check_command(command) && exit_reflects_check(command))
        {
            let passes = self.passed_checks.entry(command.clone()).or_insert(0);
            *passes += 1;
            if *passes == 2 {
                return Some(Reminder {
                    to_model: format!(
                        "[Claw] `{command}` already succeeded and nothing has changed since: running it again proves nothing new. If something is still unverified, exercise the behaviour itself (start the server and request the page, call the function with a real input, open the produced file). Otherwise give your final answer now."
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
    !replaced_lines(tool_output).is_empty()
}

/// Les lignes qui existaient et que la modification a retirées ou changées,
/// dans l'ordre du fichier (résultat de `edit_file` ou `write_file` ; le
/// retour d'un hook peut suivre le JSON).
#[must_use]
pub fn replaced_lines(tool_output: &str) -> Vec<String> {
    let Some(value) = first_json(tool_output) else {
        return Vec::new();
    };
    let lines: Vec<&str> = value
        .get("structuredPatch")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|hunk| hunk.get("lines").and_then(Value::as_array))
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    let mut remaining: HashMap<&str, i64> = HashMap::new();
    for line in &lines {
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
    let mut replaced = Vec::new();
    for line in &lines {
        let Some(text) = line.strip_prefix('-') else {
            continue;
        };
        if let Some(count) = remaining.get_mut(text.trim()) {
            if *count > 0 {
                *count -= 1;
                replaced.push(text.to_string());
            }
        }
    }
    replaced
}

/// Texte joint au résultat de l'outil quand un test existant est réécrit
/// après une vérification en échec.
#[must_use]
pub fn test_guard_feedback(name: &str) -> String {
    format!(
        "[Claw] You changed existing lines of `{name}`, which is used to check the work, after a failing check. A check describes the expected behavior: making it pass by rewriting it proves nothing. Fix the code instead and restore the check, unless the user asked you to change it or the check itself is wrong; in that case say so explicitly in your final answer, with the reason. If the failing check exposed a real defect in the code, fix that defect: do not point the check at something else to get around it."
    )
}

// --- Données réécrites ----------------------------------------------------------------

/// Lignes d'origine citées au plus par la garde des données.
const DATA_GUARD_MAX_LINES: usize = 20;

/// Extensions des fichiers de données en lignes ou en colonnes.
const DATA_EXTENSIONS: [&str; 6] = ["csv", "tsv", "psv", "dat", "jsonl", "ndjson"];

/// `path` est un fichier de données, d'après son extension.
#[must_use]
pub fn is_data_file(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            DATA_EXTENSIONS
                .iter()
                .any(|known| extension.eq_ignore_ascii_case(known))
        })
}

/// Fichiers de données du dépôt Git au début du tour : date et taille, et
/// contenu d'origine de ceux qui n'étaient pas dans leur état du dernier
/// commit (les autres se relisent dans Git). Sert à voir ce qu'une commande du
/// modèle (`sed -i`, un script Python, `cat > f`) réécrit, comme `edit_file`.
#[derive(Debug, Default)]
pub struct DataSnapshot {
    files: std::collections::BTreeMap<String, (FileState, Option<String>)>,
}

type FileState = Option<(std::time::SystemTime, u64)>;

/// Fichiers de données suivis au plus, et taille au-delà de laquelle un
/// fichier n'est pas comparé ligne à ligne.
const MAX_DATA_FILES_WATCHED: usize = 200;
const MAX_DATA_FILE_BYTES: u64 = 2_000_000;

fn file_state(path: &Path) -> FileState {
    std::fs::metadata(path)
        .ok()
        .and_then(|meta| Some((meta.modified().ok()?, meta.len())))
}

fn git_output(root: &Path, args: &[&str]) -> Option<Vec<u8>> {
    let output = Command::new("git")
        .args(args)
        .current_dir(root)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    output.status.success().then_some(output.stdout)
}

impl DataSnapshot {
    /// Les fichiers de données de `root` (suivis par Git, ou nouveaux mais pas
    /// ignorés) ; `None` hors d'un dépôt.
    #[must_use]
    pub fn take(root: &Path) -> Option<Self> {
        let listed = git_output(
            root,
            &[
                "ls-files",
                "-z",
                "--cached",
                "--others",
                "--exclude-standard",
            ],
        )?;
        let changed = git_output(root, &["diff", "--name-only", "-z", "HEAD"]).unwrap_or_default();
        let committed = git_output(root, &["ls-files", "-z", "--cached"]).unwrap_or_default();
        let split = |bytes: &[u8]| {
            bytes
                .split(|byte| *byte == 0)
                .filter(|path| !path.is_empty())
                .map(|path| String::from_utf8_lossy(path).into_owned())
                .collect::<std::collections::BTreeSet<_>>()
        };
        let (changed, committed) = (split(&changed), split(&committed));
        let mut files = std::collections::BTreeMap::new();
        for path in split(&listed) {
            let full = root.join(&path);
            if files.len() >= MAX_DATA_FILES_WATCHED
                || !is_data_file(&full)
                || crate::file_ops::under_generated_dir(root, Path::new(&path))
            {
                continue;
            }
            let state = file_state(&full);
            // Pas dans son état du dernier commit : le contenu de départ est
            // gardé, Git ne le connaît pas.
            let original = (changed.contains(&path) || !committed.contains(&path))
                .then(|| {
                    state
                        .filter(|(_, size)| *size <= MAX_DATA_FILE_BYTES)
                        .and_then(|_| std::fs::read_to_string(&full).ok())
                })
                .flatten();
            files.insert(path, (state, original));
        }
        Some(Self { files })
    }

    /// Fichiers de données changés depuis l'instantané, avec les lignes
    /// d'origine remplacées ou retirées (chemin complet, lignes). Un fichier
    /// complété sans rien perdre ne compte pas. L'instantané est mis à jour.
    pub fn rewritten(&mut self, root: &Path) -> Vec<(String, Vec<String>)> {
        let mut found = Vec::new();
        for (path, (state, original)) in &mut self.files {
            let full = root.join(path.as_str());
            let now = file_state(&full);
            if now == *state {
                continue;
            }
            *state = now;
            let before = match original {
                Some(text) => text.clone(),
                None => match git_output(root, &["show", &format!("HEAD:{path}")]) {
                    Some(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
                    None => continue,
                },
            };
            if before.len() as u64 > MAX_DATA_FILE_BYTES {
                continue;
            }
            let after = std::fs::read_to_string(&full).unwrap_or_default();
            let removed = removed_lines(&before, &after);
            if !removed.is_empty() {
                found.push((full.to_string_lossy().into_owned(), removed));
            }
        }
        found
    }

    /// Le modèle a lui-même modifié ce fichier (`edit_file`, contrôlé à part) :
    /// sa nouvelle date n'est pas le fait d'une commande.
    pub fn refresh(&mut self, root: &Path, path: &str) {
        let full = Path::new(path);
        let relative = full.strip_prefix(root).unwrap_or(full);
        let key = relative.to_string_lossy().replace('\\', "/");
        if let Some((state, _)) = self.files.get_mut(&key) {
            *state = file_state(&root.join(&key));
        }
    }

    /// Après la vérification de Claw : ce qu'elle a écrit n'est pas du modèle.
    pub fn refresh_all(&mut self, root: &Path) {
        for (path, (state, _)) in &mut self.files {
            *state = file_state(&root.join(path.as_str()));
        }
    }
}

/// Lignes non vides de `before` absentes de `after` (avec leur nombre).
fn removed_lines(before: &str, after: &str) -> Vec<String> {
    let mut left: HashMap<&str, usize> = HashMap::new();
    for line in after.lines().map(str::trim).filter(|line| !line.is_empty()) {
        *left.entry(line).or_insert(0) += 1;
    }
    before
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter(|line| match left.get_mut(line) {
            Some(count) if *count > 0 => {
                *count -= 1;
                false
            }
            _ => true,
        })
        .map(str::to_string)
        .collect()
}

/// La même garde quand c'est une commande du modèle qui a réécrit le fichier.
#[must_use]
pub fn command_data_guard_feedback(path: &str, replaced: &[String], command: &str) -> String {
    data_guard_feedback(path, replaced).replacen(
        "You replaced or removed existing lines of",
        &format!(
            "Your command `{}` replaced or removed existing lines of",
            shortened(command, 120)
        ),
        1,
    )
}

/// Texte joint au résultat de l'outil quand des lignes existantes d'un fichier
/// de données sont remplacées : un petit modèle « répare » sinon les données de
/// l'utilisateur au lieu du code qui ne sait pas les lire.
#[must_use]
pub fn data_guard_feedback(path: &str, replaced: &[String]) -> String {
    let mut quoted: Vec<String> = replaced
        .iter()
        .take(DATA_GUARD_MAX_LINES)
        .map(|line| shortened(line, 200))
        .collect();
    if replaced.len() > DATA_GUARD_MAX_LINES {
        quoted.push(format!(
            "(and {} more lines)",
            replaced.len() - DATA_GUARD_MAX_LINES
        ));
    }
    format!(
        "[Claw] You replaced or removed existing lines of `{path}`, a data file. A program that fails on some data does not show that the data is wrong: values written in another valid format (decimal comma, thousands separator, another date format or encoding) are for the code to read, and the same source will produce them again. A user who says the file is broken, or asks you to repair it, is giving you a hypothesis to check, not asking for these edits. Keep this change only if the user asked for this exact edit of the data (rows to add, remove or correct) or if these lines are really invalid; otherwise put the original lines back with edit_file and fix the code that reads the file. Never rewrite a data file from memory, and say in your final answer exactly which lines you changed and why. If the user reported a problem, tell them plainly whether their file was really the cause.\nThe lines as they were before your change:\n{}",
        quoted.join("\n")
    )
}

/// Fichiers JSON de configuration connus : pas les données de l'application.
const JSON_CONFIG_NAMES: [&str; 23] = [
    "package.json",
    "package-lock.json",
    "composer.json",
    "jsconfig.json",
    "deno.json",
    "bower.json",
    "angular.json",
    "nx.json",
    "lerna.json",
    "vercel.json",
    "firebase.json",
    "renovate.json",
    "tslint.json",
    "global.json",
    "project.json",
    "manifest.json",
    "settings.json",
    "launch.json",
    "tasks.json",
    "extensions.json",
    "devcontainer.json",
    "app.json",
    "babel.config.json",
];

/// `path` (relatif au projet si possible) est un fichier JSON de données de
/// l'application, ce qu'elle enregistre, et non un fichier de configuration
/// connu (`package.json`, `tsconfig*.json`, `*.config.json`, dossiers cachés
/// comme `.vscode`, `node_modules`).
#[must_use]
pub fn is_json_data_file(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    let name = name.to_ascii_lowercase();
    let config = !Path::new(&name)
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("json"))
        || JSON_CONFIG_NAMES.contains(&name.as_str())
        || name.starts_with('.')
        || name.starts_with("tsconfig")
        || name.starts_with("appsettings")
        || name.ends_with(".config.json");
    let tooling = path.parent().is_some_and(|folder| {
        folder.components().any(|part| match part {
            std::path::Component::Normal(part) => part
                .to_str()
                .is_some_and(|part| part.starts_with('.') || part == "node_modules"),
            _ => false,
        })
    });
    !config && !tooling
}

/// Ce qu'une modification change dans un fichier JSON de données qui existait :
/// les clés ajoutées à au moins deux entrées déjà là (une migration faite à la
/// main ; une seule clé nouvelle est plutôt une entrée ajoutée) et le nombre de
/// valeurs existantes changées ou retirées. `None` si rien de cela, ou si l'un
/// des deux contenus n'est pas du JSON.
#[must_use]
pub fn json_data_change(before: &str, after: &str) -> Option<(Vec<String>, usize)> {
    let before: Value = serde_json::from_str(before).ok()?;
    let after: Value = serde_json::from_str(after).ok()?;
    let mut added = std::collections::BTreeMap::new();
    let mut changed = 0;
    compare_json(&before, &after, &mut added, &mut changed);
    let added: Vec<String> = added
        .into_iter()
        .filter(|(_, count)| *count >= 2)
        .map(|(key, _)| key)
        .collect();
    (!added.is_empty() || changed > 0).then_some((added, changed))
}

fn compare_json(
    before: &Value,
    after: &Value,
    added: &mut std::collections::BTreeMap<String, usize>,
    changed: &mut usize,
) {
    match (before, after) {
        (Value::Object(old), Value::Object(new)) => {
            for (key, value) in old {
                match new.get(key) {
                    Some(now) => compare_json(value, now, added, changed),
                    None => *changed += 1,
                }
            }
            if !old.is_empty() {
                for key in new.keys().filter(|key| !old.contains_key(*key)) {
                    *added.entry(key.clone()).or_default() += 1;
                }
            }
        }
        (Value::Array(old), Value::Array(new)) => {
            for (index, value) in old.iter().enumerate() {
                match new.get(index) {
                    Some(now) => compare_json(value, now, added, changed),
                    None => *changed += 1,
                }
            }
        }
        _ => {
            if before != after {
                *changed += 1;
            }
        }
    }
}

/// Remarque au modèle qui a changé un fichier JSON de données pour qu'il colle à
/// son nouveau code : les copies de l'utilisateur gardent l'ancienne forme.
#[must_use]
pub fn json_data_feedback(path: &str, added: &[String], changed: usize) -> String {
    let mut text = format!("[Claw] You changed `{path}`, the data your program saves:");
    if !added.is_empty() {
        let keys: Vec<String> = added.iter().take(5).map(|key| format!("`{key}`")).collect();
        let _ = write!(
            text,
            " you added {} to entries that were already there, so that they fit your new code.",
            keys.join(", ")
        );
    }
    if changed > 0 {
        let _ = write!(text, " Existing values changed or removed: {changed}.");
    }
    text.push_str(" The user's own copies of this file (their real data, backups, other machines) keep the old shape, and your code will meet them: make it read that shape too (give a missing field its default where the data is loaded or used, such as `entry.get(\"field\", [])`), and put the file back as it was with your editing tools. Keep this change only if the user asked for this edit of their data, and say so in your final answer. Then try your commands on the file as it was.");
    text
}

// --- Fichier montré après une modification --------------------------------------------

/// Début et fin de la vue d'un fichier jointe au résultat d'une modification.
pub const FILE_VIEW_PREFIX: &str = "[Claw] File view: ";
pub const FILE_VIEW_END: &str = "[Claw] End of file view.";
/// Mis à la place d'une vue quand une plus récente du même fichier la remplace.
pub const FILE_VIEW_REPLACED: &str =
    "[Claw] (An older view of this file was here; the file has changed since.)";
/// Mis à la place d'une vue quand le fichier a changé par un autre moyen
/// (commande, formateur, autre programme) ou quand la session est rouverte.
pub const FILE_VIEW_STALE: &str = "[Claw] (A view of this file was here, but the file may have changed since then by other means: read it again before you edit it.)";
/// En dessous, le fichier entier est montré ; au-dessus, les lignes autour du changement.
const VIEW_WHOLE_FILE_MAX_CHARS: usize = 4_000;
const VIEW_WHOLE_FILE_MAX_LINES: usize = 120;
const VIEW_CONTEXT_LINES: usize = 8;
const VIEW_REGION_MAX_LINES: usize = 60;
/// Caractères montrés au plus d'une ligne, et d'une vue autour d'un changement.
const VIEW_LINE_MAX_CHARS: usize = 300;
const VIEW_REGION_MAX_CHARS: usize = 4_000;

/// Une ligne trop longue (fichier minifié, base64) est coupée dans une vue.
fn clipped_line(line: &str) -> String {
    let count = line.chars().count();
    if count <= VIEW_LINE_MAX_CHARS {
        return line.to_string();
    }
    let kept: String = line.chars().take(VIEW_LINE_MAX_CHARS).collect();
    format!("{kept}… [line cut: {count} characters]")
}

/// Premier objet JSON d'un résultat d'outil (des remarques peuvent le suivre).
fn first_json(output: &str) -> Option<Value> {
    serde_json::Deserializer::from_str(output)
        .into_iter::<Value>()
        .next()?
        .ok()
}

/// Chemin tel que le modèle l'écrit : relatif au dossier de travail, barres obliques.
#[must_use]
pub fn shown_path(path: &str) -> String {
    let relative = std::env::current_dir()
        .ok()
        .and_then(|cwd| {
            Path::new(path)
                .strip_prefix(cwd)
                .ok()
                .map(Path::to_path_buf)
        })
        .unwrap_or_else(|| std::path::PathBuf::from(path));
    relative.to_string_lossy().replace('\\', "/")
}

/// Où Claw garde la dernière version d'un fichier que le modèle avait modifié
/// quand une commande l'a supprimé ou ramené en arrière : sous `.claw/kept/`
/// du projet, ou dans le dossier temporaire pour un fichier hors du projet.
#[must_use]
pub fn kept_copy_path(cwd: &Path, path: &Path) -> std::path::PathBuf {
    match path.strip_prefix(cwd) {
        Ok(inside) => cwd.join(".claw").join("kept").join(inside),
        Err(_) => std::env::temp_dir()
            .join("claw-kept")
            .join(path.file_name().unwrap_or(path.as_os_str())),
    }
}

/// Remarque au modèle : une commande a supprimé (`deleted`) un fichier qu'il
/// avait modifié pendant le tour, ou l'a ramené à son état du début du tour ;
/// `kept` : la copie de sa dernière version, si Claw a pu la garder.
#[must_use]
pub fn lost_edits_note(shown: &str, kept: Option<&str>, deleted: bool) -> String {
    let what = if deleted {
        format!("This command deleted `{shown}`, a file you had changed in this turn: your changes to it are gone from the project.")
    } else {
        format!("After this command, `{shown}` is back to its state from before your changes in this turn (as `git checkout`, `git restore` or `git stash` do): your changes to it are gone.")
    };
    match kept {
        Some(kept) => format!(
            "[Claw] {what} If that was not intended, Claw kept your last version: `cp \"{kept}\" \"{shown}\"` restores it in one command, while `git checkout` would bring back the version from before your changes. Then change the file with your editing tools, not by rewriting it through the shell."
        ),
        None => format!(
            "[Claw] {what} If that was not intended, write your last version back with your editing tools; `git checkout` would bring back the version from before your changes."
        ),
    }
}

/// Marque qui ouvre les vues de ce fichier (pour retirer les plus anciennes).
#[must_use]
pub fn file_view_marker(tool_output: &str) -> Option<String> {
    let value = first_json(tool_output)?;
    let path = value.get("filePath")?.as_str()?;
    Some(format!("{FILE_VIEW_PREFIX}`{}`", shown_path(path)))
}

/// Ce que le modèle doit savoir du fichier après une modification réussie,
/// pour ne pas le relire (une requête et le fichier une fois de plus dans le
/// contexte, à chaque fois) : le fichier tel qu'il est maintenant s'il est
/// petit, sinon les lignes autour de chaque changement. Après `write_file`, une
/// ligne suffit : le modèle vient d'écrire tout le contenu.
#[must_use]
pub fn view_after_edit(tool_name: &str, tool_output: &str) -> Option<String> {
    let value = first_json(tool_output)?;
    let path = value.get("filePath")?.as_str()?;
    let content = std::fs::read_to_string(path).ok()?;
    // Un fichier qui porte lui-même les marques d'une vue : on ne saurait plus
    // où la vue finit.
    if content.contains(FILE_VIEW_END) || content.contains(FILE_VIEW_PREFIX) {
        return None;
    }
    let shown = shown_path(path);
    let lines: Vec<&str> = content.lines().collect();
    let total = lines.len();
    let small = content.len() <= VIEW_WHOLE_FILE_MAX_CHARS && total <= VIEW_WHOLE_FILE_MAX_LINES;
    if tool_name == "write_file" {
        // L'outil a déjà une remarque (contenu abrégé, codage) : rien de
        // rassurant à y ajouter.
        if value.get("note").is_some_and(|note| !note.is_null()) {
            return None;
        }
        let as_sent = value
            .get("content")
            .and_then(Value::as_str)
            .is_some_and(|sent| {
                sent.lines()
                    .eq(content.trim_start_matches('\u{feff}').lines())
            });
        if as_sent {
            return Some(format!(
                "[Claw] `{shown}` is written exactly as you sent it ({total} lines): you do not need to read it again."
            ));
        }
        // Un formateur ou un hook l'a changé après l'écriture.
        if !small {
            return Some(format!(
                "[Claw] `{shown}` on disk now differs from what you sent ({total} lines; a formatter or a hook changed it): read it again before you edit it."
            ));
        }
    }
    if small {
        return Some(format!(
            "{FILE_VIEW_PREFIX}`{shown}` as it is now ({total} lines). You do not need to read it again.\n{}\n{FILE_VIEW_END}",
            lines.join("\n")
        ));
    }
    // Grand fichier : les lignes autour du changement, trouvé en comparant le
    // fichier d'avant (le correctif le porte en entier) à celui du disque.
    let before = original_content(tool_output)?;
    let old: Vec<&str> = before.lines().collect();
    let prefix = old.iter().zip(&lines).take_while(|(a, b)| a == b).count();
    let suffix = old[prefix..]
        .iter()
        .rev()
        .zip(lines[prefix..].iter().rev())
        .take_while(|(a, b)| a == b)
        .count();
    let changed_end = total - suffix;
    let from = prefix.saturating_sub(VIEW_CONTEXT_LINES);
    let wanted = (changed_end + VIEW_CONTEXT_LINES).min(total);
    let mut to = from;
    let mut text = String::new();
    while to < wanted && to - from < VIEW_REGION_MAX_LINES {
        let line = clipped_line(lines[to]);
        if text.len() + line.len() + 1 > VIEW_REGION_MAX_CHARS {
            break;
        }
        text.push_str(&line);
        text.push('\n');
        to += 1;
    }
    if to == from {
        return None;
    }
    let more = if to < wanted {
        format!(
            " (the changed part goes on to line {changed_end}: read those lines if you need them)"
        )
    } else {
        String::new()
    };
    Some(format!(
        "{FILE_VIEW_PREFIX}`{shown}` now has {total} lines. Lines {} to {to} as they are now, around your change{more}. You do not need to read the file again to continue from these lines.\n{}\n{FILE_VIEW_END}",
        from + 1,
        text.trim_end_matches('\n')
    ))
}

// --- Essais regroupés -----------------------------------------------------------------

/// Réponses d'un seul appel d'observation à la suite, avant la remarque. Elle
/// est redite toutes les `SINGLE_CALL_REPLIES` réponses tant que les appels
/// isolés continuent, `MAX_BATCH_NOTES` fois au plus par tour.
pub const SINGLE_CALL_REPLIES: u32 = 3;
pub const MAX_BATCH_NOTES: u32 = 3;

/// Au-delà, une commande n'entre pas dans l'exemple de la remarque.
const CHAINED_COMMAND_MAX_CHARS: usize = 200;

/// Des commandes lancées une par réponse, réunies en un seul appel : l'exemple
/// de la remarque. `None` si l'une d'elles ne s'y prête pas (plusieurs lignes,
/// trop longue, finie par un opérateur). Un même `cd dossier &&` en tête de
/// chacune n'est gardé qu'une fois.
#[must_use]
pub fn chained_commands(commands: &[&str]) -> Option<String> {
    let commands = commands
        .iter()
        .map(|command| command.trim())
        .collect::<Vec<_>>();
    let chainable = |command: &&str| {
        !command.is_empty()
            && !command.contains(['\n', '`'])
            && command.chars().count() <= CHAINED_COMMAND_MAX_CHARS
            && !command.ends_with(['&', ';', '|', '\\'])
    };
    if commands.len() < 2 || !commands.iter().all(chainable) {
        return None;
    }
    let prefix = commands[0]
        .find(" && ")
        .map(|at| &commands[0][..at + " && ".len()])
        .filter(|prefix| {
            prefix.starts_with("cd ")
                && commands
                    .iter()
                    .all(|command| command.len() > prefix.len() && command.starts_with(prefix))
        })
        .unwrap_or_default();
    let chained = commands
        .iter()
        .map(|command| &command[prefix.len()..])
        .collect::<Vec<_>>()
        .join("; ");
    Some(format!("{prefix}{chained}"))
}

/// Remarque jointe au résultat d'un appel isolé quand `streak` réponses de
/// suite n'ont fait qu'un appel d'observation : chaque réponse coûte une
/// requête, ce qui est déjà prévu se demande ensemble. `recent` : outil et
/// commande des derniers appels isolés qui étaient des commandes ; les
/// `SINGLE_CALL_REPLIES` dernières servent d'exemple quand elles s'enchaînent.
#[must_use]
pub fn batch_note(streak: u32, recent: &[(String, String)]) -> String {
    let last = recent
        .len()
        .checked_sub(SINGLE_CALL_REPLIES as usize)
        .map(|from| &recent[from..]);
    let example = last
        .filter(|last| last.iter().all(|(tool, _)| *tool == last[0].0))
        .and_then(|last| {
            let commands = last
                .iter()
                .map(|(_, command)| command.as_str())
                .collect::<Vec<_>>();
            Some((last[0].0.as_str(), chained_commands(&commands)?))
        });
    let how = match &example {
        Some((tool, chained)) => format!(
            "several tool calls in one reply, or the commands chained in one call. For example, your last {SINGLE_CALL_REPLIES} commands fit in one `{tool}` call: `{chained}`"
        ),
        None => "several tool calls in one reply, or the commands chained in one `bash` call (`cmd1; cmd2; cmd3`).".to_string(),
    };
    if streak <= SINGLE_CALL_REPLIES {
        format!(
            "[Claw] Your last {streak} replies each made a single call, and each reply costs one request of a limited daily quota. When you already know the next things you need (several files to read, several commands or inputs to try), ask for them together and read all the results at once: {how}"
        )
    } else {
        format!(
            "[Claw] {streak} replies in a row have now made a single call each: {streak} requests of a limited daily quota, one observation at a time. Decide now what you still have to check and send it all in your next reply: {how}"
        )
    }
}

// --- Budget de requêtes d'un tour --------------------------------------------------------

/// Requêtes au modèle dans un même tour avant que Claw lui fasse conclure
/// (réglage `turnRequestLimit`, variable `CLAW_TURN_REQUESTS` ; 0 : sans
/// limite). Les tours réels observés vont jusqu'à 35 requêtes ; un modèle qui
/// tournait en rond en a dépensé 250 en 25 minutes, sur un quota gratuit de
/// 1 000 par jour.
pub const TURN_REQUEST_LIMIT: usize = 100;

/// Message adressé au modèle quand le tour atteint son budget de requêtes.
#[must_use]
pub fn turn_limit_note(limit: usize) -> String {
    format!(
        "[Claw] This turn has used {limit} requests, the limit for one turn: the user's daily quota of requests is limited. Stop working now and do not call any tool (a call made in your next reply will not run). Give your final answer: what is done and verified, what is done but not verified, what remains to do, and the exact next step. The user can then ask you to continue."
    )
}

/// Résultat d'un appel d'outil demandé après ce message : il n'a pas tourné.
#[must_use]
pub fn turn_limit_refusal(limit: usize) -> String {
    format!(
        "[Claw] Not run: this turn reached its limit of {limit} requests. The user can ask you to continue."
    )
}

// --- Fichier relu sans avoir changé ------------------------------------------------------

/// Remarques par tour, au plus, pour un fichier relu à l'identique.
pub const MAX_REPEATED_READ_NOTES: u32 = 2;

/// Fichier lu, et longueur du JSON qui ouvre ce résultat de `read_file` (des
/// remarques ou un retour de hook peuvent le suivre).
#[must_use]
pub fn read_result(output: &str) -> Option<(String, usize)> {
    let mut values = serde_json::Deserializer::from_str(output).into_iter::<Value>();
    let result = values.next()?.ok()?;
    let path = result.get("file")?.get("filePath")?.as_str()?.to_string();
    Some((path, values.byte_offset()))
}

/// Ce qui reste, dans la conversation, d'une lecture que le modèle a refaite
/// à l'identique : la copie la plus récente est la seule gardée.
#[must_use]
pub fn repeated_read_stub(path: &str) -> String {
    format!(
        "[Claw] (An earlier read of `{}` was here. You read it again later and got the same content: that later copy is the one kept.)",
        shown_path(path)
    )
}

/// Remarque jointe à une lecture identique à une lecture du même tour : elle
/// n'a rien appris au modèle (et lui a coûté une requête si elle était seule
/// dans sa réponse).
#[must_use]
pub fn repeated_read_note(path: &str) -> String {
    format!(
        "[Claw] `{}` has not changed since you read it earlier in this turn: reading it again brought nothing new. Work from the reads you already have, and edit a file directly from them (after each edit, Claw shows you the file as it is). Read a file again only when something other than your own edits may have changed it, or after an edit failed. Each reply costs one request of a limited daily quota.",
        shown_path(path)
    )
}

// --- Liste de tâches envoyée seule -------------------------------------------------------

/// Remarques par tour, au plus, pour une liste de tâches envoyée seule.
pub const MAX_TODO_ALONE_NOTES: u32 = 3;

/// Remarque jointe au résultat de `TodoWrite` quand la réponse n'a fait que
/// mettre la liste à jour : une requête entière pour une coche.
pub const TODO_ALONE_NOTE: &str = "[Claw] This reply only updated the task list, and it cost a full request of a limited daily quota. From now on, send `TodoWrite` in the same reply as the next tool call of your work (the edit, the read or the command of the task you are starting), never alone.";

/// La liste que ce résultat de `TodoWrite` enregistre garde des tâches à
/// faire : du travail suit, la mise à jour pouvait partir avec lui.
#[must_use]
pub fn todos_left(output: &str) -> bool {
    first_json(output)
        .as_ref()
        .and_then(|result| result.get("newTodos"))
        .and_then(Value::as_array)
        .is_some_and(|todos| {
            todos
                .iter()
                .any(|todo| todo.get("status").and_then(Value::as_str) != Some("completed"))
        })
}

// --- Script muet -------------------------------------------------------------------------

/// Un script Python lancé par une commande qui n'a rien écrit : s'il définit
/// `main()` sans jamais l'appeler (plus de bloc `if __name__ == "__main__":`),
/// la commande ne fait rien. Rend le script et la remarque au modèle. Essai
/// réel t6 avec le petit modèle : le bloc perdu dans une réécriture, `python
/// conges.py jours ...` muet, et le modèle qui appelle `main()` par `python -c`
/// au lieu de réparer ; ses tests passaient, la commande ne marchait plus.
#[must_use]
pub fn uncalled_main_note(command: &str, output: &str, cwd: &Path) -> Option<(String, String)> {
    let value = first_json(output)?;
    let printed = |key: &str| {
        value
            .get(key)
            .and_then(Value::as_str)
            .is_some_and(|text| !text.trim().is_empty())
    };
    if printed("stdout") || printed("stderr") {
        return None;
    }
    let script = python_script(command)?;
    let source = std::fs::read_to_string(cwd.join(&script)).ok()?;
    uncalled_main(&source).then(|| {
        let note = format!(
            "[Claw] `{}` printed nothing: `{script}` defines `main()` but never calls it (no `if __name__ == \"__main__\":` block), so running it does nothing. Tests that call `main()` directly cannot see this. Put the block back at the end of the file (`if __name__ == \"__main__\":` then `sys.exit(main(sys.argv[1:]))`, with `import sys`) and run the command again.",
            shortened(command, 200)
        );
        (script, note)
    })
}

/// Le script `.py` qu'une commande lance avec Python, s'il y en a un.
fn python_script(command: &str) -> Option<String> {
    let words: Vec<&str> = command.split_whitespace().collect();
    let interpreter = words.iter().position(|word| {
        let name = word.rsplit(['/', '\\']).next().unwrap_or(word);
        matches!(name.trim_end_matches(".exe"), "python" | "python3" | "py")
    })?;
    words[interpreter + 1..]
        .iter()
        .map(|word| word.trim_matches(['"', '\'']))
        .find(|word| {
            !word.starts_with('-')
                && Path::new(word)
                    .extension()
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("py"))
        })
        .map(str::to_string)
}

/// Le source définit `main()` au premier niveau sans jamais l'appeler.
fn uncalled_main(source: &str) -> bool {
    let top_level = |line: &&str| !line.starts_with([' ', '\t']);
    let is_def = |line: &str| line.starts_with("def ") || line.starts_with("async def ");
    let defines = source
        .lines()
        .filter(top_level)
        .any(|line| line.starts_with("def main(") || line.starts_with("async def main("));
    let calls = source.contains("__name__")
        || source
            .lines()
            .filter(top_level)
            .any(|line| line.contains("main(") && !is_def(line));
    defines && !calls
}

// --- Python du Microsoft Store ---------------------------------------------------------

/// Le Python installé par le Microsoft Store a sa propre vue de `AppData` : il
/// ne voit pas ce que les autres programmes mettent dans le dossier temporaire
/// (`/tmp` de Git Bash), et ce qu'il y écrit leur est invisible. Un modèle y
/// perd plusieurs requêtes à chercher un fichier qu'il vient de copier.
#[must_use]
pub fn store_python_hint(output: &str) -> Option<String> {
    let texts: Vec<String> = match first_json(output) {
        Some(value) => ["stderr", "stdout"]
            .iter()
            .filter_map(|key| value.get(*key)?.as_str().map(str::to_string))
            .collect(),
        None => vec![output.to_string()],
    };
    let seen = texts.iter().flat_map(|text| text.lines()).any(|line| {
        let flat: String = line
            .to_lowercase()
            .chars()
            .filter(|ch| !matches!(ch, '\\' | '/'))
            .collect();
        flat.contains("windowsapps")
            && flat.contains("can't open file")
            && flat.contains("appdatalocaltemp")
    });
    seen.then(|| {
        "[Claw] This Python comes from the Microsoft Store: it has its own private view of AppData, so it cannot see files that other programs put in the temp folder (`/tmp` in Git Bash), and files it writes there are invisible to them. Work inside the project folder instead (a sub-folder you delete afterwards), not in /tmp.".to_string()
    })
}

// --- Exigences et escalade ------------------------------------------------------------

/// Message adressé au modèle qui dit avoir fini un travail conséquent : il
/// confronte son travail à chaque exigence avant de conclure (une fois par tour).
pub const CHECKLIST_PROMPT: &str = "[Claw] Before you finish, check your work against every requirement. List each part of the user's request (when their last message only asks you to continue, the request it continues): every feature, page, file, fix or explanation they asked for, plus the requirements of the docstrings, comments and specifications of the code you changed, and the edge cases these imply (empty or missing input, boundaries, invalid values, duplicates, ordering, large inputs). For each one, find the evidence that it is done: the file and the code that handle it, and the test or command output from this session that shows it works. An intention, a plan or a file you did not check is not evidence. Whatever is missing or unverified, do it now with your tools, in this reply: do not announce it, do not leave it for later, do not ask the user to do it. If the project has tests, add the missing test and run it; never describe tests, commands or results that do not exist, and never weaken or delete an existing test. Then end with your final answer to the user's original request, written as the report the system prompt describes under \"Final answer\" (the outcome first, what was wrong and why, what you changed, how it was verified, what is left for the user). Its verification part gives one short line per requirement with its evidence; it claims only what this check established and names plainly any part that is not done or not verified. The user reads only that last message and has not seen this check: it must stand on its own, cover everything they asked for, including any explanation or instructions they requested (such as how to install, run or test the result), be complete without padding, and not refer to this check.";

/// Demande citée au plus dans le contrôle des exigences.
const CHECKLIST_REQUEST_MAX_CHARS: usize = 4_000;

/// Ajouté aux contrôles quand une demande d'autorisation de ce tour est restée
/// sans réponse : le modèle ne peut plus lancer ce qui en demande une.
pub const AWAY_NOTE: &str = "The user is not answering permission requests right now: check with read-only tools only (read, search), and list in your answer what is left unverified and which calls wait for their approval.";

/// Fin d'un contrôle de fin de tour (exigences, preuves d'un problème) : les
/// cas limites du genre de fonctionnalité que nomme la demande, chacun à
/// prouver. Vide si la demande n'en nomme aucun. Le guide du début de tour ne
/// suffit pas : sur t3, il est arrivé au modèle, qui l'a laissé de côté.
#[must_use]
pub fn edge_case_check(request: &str, earlier: Option<&str>) -> String {
    if !enabled(Aid::EdgeCases) {
        return String::new();
    }
    // Le genre de fonctionnalité se lit dans la demande poursuivie aussi.
    let cases = feature_edge_cases(&format!("{request}\n{}", earlier.unwrap_or_default()));
    if cases.is_empty() {
        return String::new();
    }
    format!(
        "\n\nWhat experienced engineers always check for this kind of feature. A line that applies is part of what the user asked even though they did not write it: users never list these cases, they expect them (asking that the same person is not registered twice means \" zoé \" is Zoé). For each line that applies to this request, show the test or the command of this session that covers it, or add it now. The expected result of each try is in parentheses: a different result means your code is wrong, even if you chose that behavior on purpose (unless the user's request explicitly asks for that other result): fix the code and its tests, and say what you changed. Run the tries of a line together, in the project's tests when it has some or in one command (`python app.py a; echo $?; python app.py b; echo $?`), not one request per try:\n- {}",
        cases.join("\n- ")
    )
}

/// Guide joint à la demande quand elle nomme un genre de fonctionnalité : ses
/// cas limites, dès le départ (une correction de deux lignes n'atteint pas le
/// contrôle des exigences, et un travail bien fait d'emblée coûte moins de
/// requêtes qu'une reprise). `earlier` : la demande qu'un message court poursuit.
#[must_use]
pub fn edge_case_guide(request: &str, earlier: Option<&str>) -> Option<String> {
    if !enabled(Aid::EdgeCases) {
        return None;
    }
    let cases = feature_edge_cases(&format!("{request}\n{}", earlier.unwrap_or_default()));
    (!cases.is_empty()).then(|| {
        format!(
            "{GUIDE_PREFIX} What experienced engineers always check for this kind of work. A line that applies is part of the request even though the user did not write it. Keep each line that applies in mind from the start, and cover it with a test or a command; the expected result of each try is in parentheses. Run the tries of a line together, in the project's tests or in one command, not one request per try:\n- {}",
            cases.join("\n- ")
        )
    })
}

/// Contrôle des exigences, avec la demande à contrôler citée mot pour mot.
/// Sans elle, un petit modèle qui ne la voit plus (trente messages plus haut,
/// après un « reprendre ») contrôle ce qu'il croit avoir fait, pas ce qui est
/// demandé. `earlier` : la demande qu'un message court peut poursuivre.
#[must_use]
pub fn checklist_prompt(request: &str, earlier: Option<&str>, away: bool) -> String {
    let quoted = match earlier.map(str::trim).filter(|text| !text.is_empty()) {
        Some(earlier) => format!(
            "Their last message: \"{}\". If it only asks you to continue, check against the request it continues, word for word:\n<<<\n{}\n>>>\nOtherwise check against their last message.",
            shortened(request, CHECKLIST_REQUEST_MAX_CHARS),
            shortened(earlier, CHECKLIST_REQUEST_MAX_CHARS)
        ),
        None => format!(
            "The request to check against, word for word:\n<<<\n{}\n>>>",
            shortened(request, CHECKLIST_REQUEST_MAX_CHARS)
        ),
    };
    let mut prompt = format!(
        "{CHECKLIST_PROMPT}\n\n{quoted}\nGo through it line by line: one line of your check per sentence or bullet of this request, even for a part you believe is already done (then quote the file and line that show it). Read each sentence again as it is written, not as you remember it: say what the user must be able to observe (who can do what, when, and what is refused), and make sure your code does exactly that, not a close variant. An action that the request leaves to a person (\"the manager can then approve it\", \"only the owner can then confirm\") stays theirs to do: your code must not do it for them automatically, and the scenario that shows it is that person doing it, after the step that allows it."
    );
    prompt.push_str(&edge_case_check(request, earlier));
    if away {
        prompt.push(' ');
        prompt.push_str(AWAY_NOTE);
    }
    prompt
}

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

// --- Titres sans contenu --------------------------------------------------------------

/// Renvois au plus, par tour, pour une réponse arrêtée sur des titres vides.
pub const MAX_EMPTY_TITLE_ROUNDS: usize = 2;
/// Titres cités au plus dans la remarque.
const EMPTY_TITLES_SHOWN: usize = 6;

/// Niveau et texte d'un titre Markdown (`### Titre`) ; `None` pour une autre
/// ligne. Le texte peut être vide (`##` seul).
fn markdown_title(line: &str) -> Option<(usize, &str)> {
    let trimmed = line.trim_start();
    // Indenté de quatre espaces ou plus : du code, pas un titre.
    if line.len() - trimmed.len() > 3 {
        return None;
    }
    let level = trimmed.chars().take_while(|ch| *ch == '#').count();
    if !(1..=6).contains(&level) {
        return None;
    }
    let rest = &trimmed[level..];
    if !rest.is_empty() && !rest.starts_with([' ', '\t']) {
        return None;
    }
    Some((level, rest.trim().trim_end_matches('#').trim_end()))
}

/// Ligne de séparation (`---`, `***`, `___`).
fn is_rule(line: &str) -> bool {
    let marks: Vec<char> = line.chars().filter(|ch| !ch.is_whitespace()).collect();
    marks.len() >= 3
        && ['-', '*', '_']
            .iter()
            .any(|mark| marks.iter().all(|ch| ch == mark))
}

/// Titres de la réponse qui n'ont rien dessous (ni texte, ni liste, ni code)
/// avant le titre suivant de même niveau ou plus haut, hors blocs de code ;
/// et vrai si la réponse s'arrête sur un titre. Un titre suivi de ses
/// sous-titres n'est vide que si eux-mêmes le sont.
#[must_use]
pub fn empty_titles(answer: &str) -> (Vec<String>, bool) {
    // Dans l'ordre : un titre (niveau, texte), ou du contenu (`None`).
    let mut items: Vec<Option<(usize, &str)>> = Vec::new();
    let mut in_code = false;
    for line in answer.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_code = !in_code;
            items.push(None);
            continue;
        }
        if in_code {
            items.push(None);
            continue;
        }
        if trimmed.is_empty() || is_rule(trimmed) {
            continue;
        }
        match markdown_title(line) {
            Some((_, "")) => {}
            title => items.push(title),
        }
    }
    let empty = items
        .iter()
        .enumerate()
        .filter_map(|(index, item)| {
            let (level, text) = (*item)?;
            let has_content = items[index + 1..]
                .iter()
                .take_while(|next| !matches!(next, Some((next_level, _)) if *next_level <= level))
                .any(Option::is_none);
            (!has_content).then(|| text.to_string())
        })
        .collect();
    (empty, matches!(items.last(), Some(Some(_))))
}

/// Message adressé au modèle dont la réponse annonce un travail par des
/// titres sans contenu, sans avoir appelé d'outil.
#[must_use]
pub fn empty_titles_feedback(titles: &[String]) -> String {
    let shown: Vec<String> = titles
        .iter()
        .take(EMPTY_TITLES_SHOWN)
        .map(|title| format!("\"{title}\""))
        .collect();
    format!(
        "[Claw] Your reply has section titles with nothing under them: {}. A title does nothing by itself: you called no tool, so the work these titles announce does not exist yet. Do it now with your tools, starting with the first title (one tool call per file to create or change). Do not write these titles again before the work is done; if a title was only meant to introduce some text, write that text under it.",
        shown.join(", ")
    )
}

/// Lignes de la fin d'une réponse où chercher des questions à l'utilisateur.
const QUESTION_TAIL_LINES: usize = 12;
/// Questions à partir desquelles la réponse attend vraiment l'utilisateur :
/// une seule en fin de réponse est souvent une simple offre de politesse.
const QUESTIONS_TO_ASK: usize = 2;

/// La réponse se termine par des questions posées à l'utilisateur (validation
/// d'une proposition, choix entre des options) : au moins deux lignes de
/// question parmi les dernières lignes de texte, hors blocs de code et
/// tableaux.
#[must_use]
pub fn ends_with_questions(answer: &str) -> bool {
    let mut in_code = false;
    let mut prose: Vec<&str> = Vec::new();
    for line in answer.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_code = !in_code;
            continue;
        }
        if !in_code && !trimmed.is_empty() && !trimmed.starts_with('|') {
            prose.push(trimmed);
        }
    }
    let tail = &prose[prose.len().saturating_sub(QUESTION_TAIL_LINES)..];
    tail.iter().filter(|line| is_question_line(line)).count() >= QUESTIONS_TO_ASK
}

/// Une ligne de texte qui pose une question : un « ? » en fin de phrase (suivi
/// de rien, d'un blanc ou d'une marque de mise en forme), après de vrais mots.
/// Le « ? » d'une adresse web ou d'un morceau de code ne compte pas.
fn is_question_line(line: &str) -> bool {
    let mut in_code = false;
    let mut letters = 0;
    let mut chars = line.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '`' {
            in_code = !in_code;
        } else if !in_code && ch.is_alphabetic() {
            letters += 1;
        } else if !in_code && ch == '?' && letters >= 6 {
            let closes = chars
                .peek()
                .is_none_or(|next| next.is_whitespace() || "*_)\"»]".contains(*next));
            if closes {
                return true;
            }
        }
    }
    false
}

/// Message adressé au modèle qui a écrit ses questions dans sa réponse alors
/// que l'utilisateur peut y répondre d'un clic.
pub const QUESTIONS_FEEDBACK: &str = "[Claw] Your reply ends with questions for the user, written as text. The user has read your reply: do not repeat it. Call the AskUserQuestion tool now with those questions (at most 4, each with 2 to 4 short concrete options, the one you recommend first), so the user answers with one click. When the answers come back, continue the task with them.";

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

// --- Signalement d'un problème ---------------------------------------------------------

/// Préfixe du guide que Claw joint au message de l'utilisateur, pour le
/// modèle seulement (les interfaces ne le montrent pas).
pub const GUIDE_PREFIX: &str = "[Claw guide]";

/// Au-delà, un message n'est plus un signalement bref : ses mots ne suffisent
/// pas à le reconnaître (une demande de fonctionnalité parle aussi d'erreurs).
const REPORT_MAX_WORDS: usize = 40;
/// Message bref accompagné d'images : presque toujours « regarde ce que ça donne ».
const REPORT_WITH_IMAGE_MAX_WORDS: usize = 25;

/// Tournures d'un utilisateur qui dit que quelque chose ne va pas.
const PROBLEM_WORDS: &[&str] = &[
    "marche pas",
    "marche plus",
    "fonctionne pas",
    "fonctionne plus",
    "n'importe quoi",
    "nimporte quoi",
    "bug",
    "bogue",
    "erreur",
    "error",
    "cassé",
    "plante",
    "crash",
    "affiche pas",
    "affiche rien",
    "affiche plus",
    "charge pas",
    "toujours pas",
    "toujours le",
    "toujours la",
    "problème",
    "probleme",
    "pas normal",
    "incorrect",
    "faux",
    "mauvais",
    "hors ligne",
    "répond pas",
    "repond pas",
    "bloqué",
    "échoue",
    "echoue",
    "ne va pas",
    "pas bon",
    "pas ça",
    "pas ca",
    "doesn't work",
    "does not work",
    "doesnt work",
    "not working",
    "broken",
    "wrong",
    "fails",
    "failing",
    "failed",
    "still ",
    "stuck",
    "blank",
    "nothing happens",
];

/// Marques d'une sortie d'erreur collée dans un long message.
const ERROR_DUMP_MARKS: &[&str] = &[
    "traceback (most recent call last)",
    "error:",
    "erreur :",
    "exception",
    "npm err!",
    "panicked at",
    "segmentation fault",
];

/// Verbes par lesquels commence une demande de travail, pas un signalement
/// (« ajoute une page d'erreur », « fais une interface comme sur l'image »).
const REQUEST_VERBS: &[&str] = &[
    "ajoute",
    "ajouter",
    "crée",
    "créer",
    "cree",
    "creer",
    "implémente",
    "implemente",
    "génère",
    "genere",
    "écris",
    "ecris",
    "fais",
    "reproduis",
    "construis",
    "développe",
    "developpe",
    "add",
    "create",
    "implement",
    "build",
    "make",
    "write",
    "generate",
    "design",
];

/// Ce que dit le message de l'utilisateur, pour la méthode à y joindre.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Report {
    /// Une demande ordinaire.
    #[default]
    None,
    /// Il dit que quelque chose ne va pas, ou colle une sortie d'erreur.
    Problem,
    /// Des images sans plainte écrite (message bref, ou défaut vu par le
    /// lecteur d'images) : défaut à corriger, résultat accepté ou modèle à
    /// suivre, le modèle trie d'abord.
    Images,
}

/// Ce que dit le message de l'utilisateur : un message bref avec une
/// tournure de plainte, ou un long message qui porte une sortie d'erreur,
/// signale un problème ; des images envoyées avec un message bref, ou dans
/// lesquelles le lecteur d'images a vu un défaut (`defects`), sont à trier.
#[must_use]
pub fn read_report(text: &str, images: usize, defects: usize) -> Report {
    let lower = text.to_lowercase();
    let words: Vec<&str> = lower.split_whitespace().collect();
    let seen = if images > 0 && defects > 0 {
        Report::Images
    } else {
        Report::None
    };
    let Some(first) = words.first() else {
        // Des images sans un mot : « regarde ».
        return if images > 0 {
            Report::Images
        } else {
            Report::None
        };
    };
    if words.len() > REPORT_MAX_WORDS {
        return if ERROR_DUMP_MARKS.iter().any(|mark| lower.contains(mark)) {
            Report::Problem
        } else {
            seen
        };
    }
    let first = first.trim_matches(|ch: char| !ch.is_alphanumeric());
    if REQUEST_VERBS.contains(&first) {
        seen
    } else if PROBLEM_WORDS.iter().any(|word| lower.contains(word)) {
        Report::Problem
    } else if images > 0 && words.len() <= REPORT_WITH_IMAGE_MAX_WORDS {
        Report::Images
    } else {
        seen
    }
}

/// Méthode de débogage jointe au message d'un utilisateur qui signale un
/// problème : un petit modèle saute sinon à une cause supposée, ne regarde
/// qu'une image sur deux et annonce « résolu » sans avoir rien observé.
#[must_use]
pub fn problem_guide(images: usize, defects: &[(usize, String)]) -> String {
    let pictures = match images {
        0 => String::new(),
        1 => " Their message has 1 image: read all of it.".to_string(),
        count => format!(
            " Their message has {count} images: each one can show a different problem, so go through all {count}."
        ),
    };
    format!(
        "{GUIDE_PREFIX} The user is telling you that something is wrong.{pictures}{} Work as a debugger, in this order:\n{DEBUGGER_STEPS}",
        seen_defects(defects)
    )
}

/// La méthode du débogueur : symptômes, reproduction, cause prouvée,
/// disparition observée, et ce que la réponse en dit.
const DEBUGGER_STEPS: &str = "1. Symptoms: list every distinct problem in their message and in each image, one line each, quoting what is shown. Drop none of them, and start with the one that makes the result useless to them.\n2. Reproduce: observe each symptom yourself with your tools, the way the user gets it (same entry point, same kind of input), before you change anything. A request that answers 200 or tests that pass do not show that what the user sees is right: look at the result itself (the page as the browser shows it, with the ViewPage tool when you have it; the produced file or image; the text).\n3. Cause: find it from evidence (logs, outputs, the code path that produces the symptom). Do not fix a guess; when several causes are possible, run the check that tells them apart. A cause the user names themselves (\"the file is corrupted\", \"it is the cache\") is a hypothesis like any other: check it, and when it is wrong, fix the real cause, not what they pointed at. Data that a program fails to read is not wrong for that reason: when its values are valid in their own format, fix the code that reads them and leave the user's data as it is.\n4. Fix the cause, then observe again that each symptom is gone, and run the fix once on input the example does not contain (the other forms the same data or request can take: another format, an empty or missing value, a larger number), with a throwaway command or a test, never by editing the user's data.\nIn your answer, say for each symptom what you observed after the fix and, if the user named a cause themselves, tell them plainly whether it was the real one. If you could not observe one (for example what a browser displays), do not call it solved: say what you changed, what you could not check and exactly what the user should look at. Do not ask the user to investigate for you while one of your tools can still do it.";

/// Ce que le lecteur d'images a vu (« image 2: zone de comparaison vide »).
fn seen_defects(defects: &[(usize, String)]) -> String {
    if defects.is_empty() {
        return String::new();
    }
    let seen: Vec<String> = defects
        .iter()
        .map(|(number, line)| format!("image {number}: {line}"))
        .collect();
    format!(" The image reader saw a defect in {}.", seen.join("; "))
}

/// « 1 image », « 3 images ».
fn images_count(images: usize) -> String {
    if images == 1 {
        "1 image".to_string()
    } else {
        format!("{images} images")
    }
}

/// Méthode jointe à des images envoyées sans plainte écrite (message bref,
/// ou défaut vu par le lecteur d'images) : le modèle décide d'abord ce
/// qu'elles disent. Un constat (« c'est la même chose ») n'est pas une
/// plainte, et un petit modèle « corrige » sinon ce qui marche.
#[must_use]
pub fn triage_guide(images: usize, defects: &[(usize, String)]) -> String {
    let every = if images > 1 {
        format!(" Each one can show something different: go through all {images}.")
    } else {
        String::new()
    };
    format!(
        "{GUIDE_PREFIX} The user sent {} with their message.{every}{} First decide what they mean: say in one line what each image shows and whether it reports a problem, shows a result they accept, or is a design to follow. Only if it reports a problem, work as a debugger, in this order:\n{DEBUGGER_STEPS}",
        images_count(images),
        seen_defects(defects)
    )
}

/// Images jointes à une demande ordinaire (cahier des charges, maquette) :
/// chacune est lue, et une capture de l'application telle qu'elle tourne est
/// prise pour ce qu'elle est, l'état actuel, pas un modèle à copier.
#[must_use]
pub fn image_guide(images: usize) -> String {
    format!(
        "{GUIDE_PREFIX} This message has {}. Before you plan, write in one line what each one shows. If one shows the application as it runs today, list what looks wrong or unfinished in it: it is evidence of its current behaviour, not a design to copy.",
        images_count(images)
    )
}

/// Message renvoyé au modèle qui conclut après un signalement de problème :
/// chaque symptôme doit avoir sa cause observée et sa disparition observée.
pub const PROBLEM_EVIDENCE_PROMPT: &str = "[Claw] Before this answer goes to the user: they reported a problem. Go back to their message and to each image it carries, and list every symptom. For each one, name the observation you made in this turn, with your tools, that shows its cause, and the one that shows it is gone after your change. A guess (\"probably\", \"likely\") is not a cause; a passing test or a 200 response is not the symptom gone. If the user named a cause themselves, say whether your observations confirm it; if you changed their data rather than the code that reads it, justify it line by line or undo it and fix the code. Where an observation is missing, make it now: read the code that produces the symptom, run it the way the user does, read the logs and the result itself. If something truly cannot be observed with your tools, say so in your answer instead of calling it solved, and never hand the investigation to the user while one of your tools can still do it.";

/// Message renvoyé au modèle qui conclut après des images envoyées sans
/// plainte écrite, quand il a modifié des fichiers ou que le lecteur d'images
/// y a vu un défaut.
pub const IMAGES_EVIDENCE_PROMPT: &str = "[Claw] Before this answer goes to the user: look again at each image they sent. For each problem an image shows, name the observation you made in this turn, with your tools, that shows its cause, and the one that shows it is gone after your change; where one is missing, make it now. If an image shows no problem (a design to follow, a result they accept), say so in one line in your answer. Never call solved what you did not observe.";

// --- Outil appelé comme une commande ---------------------------------------------------

/// Outils de Claw qu'un modèle écrit parfois comme une commande du shell.
const TOOL_NAMES: &[&str] = &[
    "KillShell",
    "BashOutput",
    "read_file",
    "write_file",
    "edit_file",
    "glob_search",
    "grep_search",
    "TodoWrite",
    "AskUserQuestion",
    "WebFetch",
    "ViewPage",
    "WebSearch",
    "FindSymbol",
    "Remember",
    "Consult",
    "Skill",
    "Agent",
];

/// La commande commence par le nom d'un outil de Claw (`KillShell bash_4`) :
/// remarque qui renvoie le modèle vers l'appel d'outil.
#[must_use]
pub fn tool_named_in_command(command: &str) -> Option<String> {
    let lower = command.to_ascii_lowercase();
    let segments = crate::bash::shell_segments(&lower);
    let name = TOOL_NAMES.iter().find(|name| {
        let name = name.to_ascii_lowercase();
        segments
            .iter()
            .any(|(segment, _)| segment.split_whitespace().next() == Some(name.as_str()))
    })?;
    Some(format!(
        "[Claw] `{name}` is one of your tools, not a shell command: bash cannot run it. Call it as a tool (a tool call named {name} with its JSON arguments)."
    ))
}

// --- Commandes conseillées sans avoir été lancées --------------------------------------

/// Commandes montrées au plus dans la remarque.
const UNTRIED_SHOWN: usize = 6;
/// Blocs de code dont les lignes sont des commandes.
const SHELL_LANGUAGES: &[&str] = &[
    "bash",
    "sh",
    "shell",
    "zsh",
    "console",
    "terminal",
    "powershell",
    "pwsh",
    "ps1",
    "cmd",
    "bat",
];
/// Commandes qu'on ne demande pas d'essayer : navigation, affichage, exemples
/// de requêtes, installations du système, actions lourdes ou sans retour.
const NOT_DEMANDED: &[&str] = &[
    "cd",
    "ls",
    "dir",
    "cat",
    "echo",
    "export",
    "set",
    "source",
    "curl",
    "wget",
    "docker",
    "docker-compose",
    "podman",
    "git",
    "sudo",
    "rm",
    "mkdir",
    "cp",
    "mv",
    "code",
    "open",
    "start",
    "xdg-open",
    "apt",
    "apt-get",
    "brew",
    "winget",
    "choco",
    "scoop",
    "ssh",
    "scp",
    "kill",
    "taskkill",
];

/// Mots significatifs d'une commande simple : sans variables d'environnement
/// ni options, `python3` et `py` ramenés à `python`.
fn command_words(segment: &str) -> Vec<String> {
    segment
        .split_whitespace()
        .skip_while(|word| word.contains('=') && !word.starts_with('-'))
        .filter(|word| !word.starts_with('-') && *word != "_")
        .map(|word| match word {
            "python3" | "py" => "python".to_string(),
            other => other.to_string(),
        })
        .collect()
}

/// Commandes que la réponse dit à l'utilisateur de lancer (blocs de code
/// d'un shell) et que le modèle n'a lancées à aucun moment de la session :
/// leurs trois premiers mots significatifs ne commencent aucune commande
/// exécutée. Les exemples à compléter et les commandes de `NOT_DEMANDED` ne
/// comptent pas.
#[must_use]
pub fn untried_commands(answer: &str, executed: &[String]) -> Vec<String> {
    let ran: Vec<Vec<String>> = executed
        .iter()
        .flat_map(|command| crate::bash::shell_segments(&command.to_ascii_lowercase()))
        .map(|(segment, _)| command_words(&segment))
        .collect();
    let mut untried: Vec<String> = Vec::new();
    let mut in_shell = false;
    let mut in_code = false;
    let mut pending = String::new();
    for line in answer.lines() {
        let trimmed = line.trim();
        if let Some(language) = trimmed.strip_prefix("```") {
            in_shell = !in_code
                && SHELL_LANGUAGES.contains(&language.trim().to_ascii_lowercase().as_str());
            in_code = !in_code;
            pending.clear();
            continue;
        }
        if !in_shell || trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with("//")
        {
            continue;
        }
        // Commande sur plusieurs lignes (`\\` de bash, accent grave de PowerShell).
        if let Some(start) = trimmed
            .strip_suffix('\\')
            .or_else(|| trimmed.strip_suffix('`'))
        {
            pending.push_str(start);
            pending.push(' ');
            continue;
        }
        let shown = format!("{pending}{trimmed}");
        pending.clear();
        let shown = shown
            .trim_start_matches("$ ")
            .trim_start_matches("> ")
            .trim()
            .to_string();
        // Exemple à compléter par l'utilisateur : rien à essayer tel quel.
        let lower = shown.to_ascii_lowercase();
        if ["<", "votre", "your", "xxx", "..."]
            .iter()
            .any(|mark| lower.contains(mark))
        {
            continue;
        }
        for (segment, _) in crate::bash::shell_segments(&lower) {
            let words = command_words(&segment);
            let Some(program) = words.first() else {
                continue;
            };
            if NOT_DEMANDED.contains(&program.as_str()) {
                continue;
            }
            let key = &words[..words.len().min(3)];
            let tried = ran.iter().any(|ran| ran.starts_with(key));
            if !tried && !untried.contains(&segment) {
                untried.push(segment);
            }
        }
    }
    untried
}

/// Message renvoyé au modèle qui conseille des commandes jamais lancées.
#[must_use]
pub fn untried_feedback(commands: &[String]) -> String {
    let shown = commands
        .iter()
        .take(UNTRIED_SHOWN)
        .map(|command| format!("`{command}`"))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "[Claw] Your answer tells the user to run commands that you have not run in this session: {shown}. Instructions you never tried are often wrong (a script that does not exist, a port, a mode the configuration forbids). Check them now: read the files that define them (package.json scripts, configuration, entry point) and run each one that is safe to try here (a server starts in the background: then request it and read its output). Correct your answer from what you observe, and mark plainly as not tested any command you could not run. {FINAL_ANSWER_RULE}"
    )
}

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
    let value = first_json(tool_output)?;
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

/// Consignes du relecteur (un autre modèle que celui qui a écrit le code). Il
/// repart des phrases de la demande avant de lire le diff : dans un essai
/// réel, une exigence mal lue par l'agent (« mis de côté pour le premier de la
/// file » devenu « prêté d'office au premier ») avait passé la relecture, qui
/// jugeait le diff d'après la description que l'agent en donnait.
pub const REVIEW_SYSTEM: &str = "You are a strict, careful code reviewer. You are shown what a user asked for, the changes a coding agent made during this turn (unified diff) and the final answer the agent is about to give the user. The agent may have misread the request: judge the changes against the user's own words, never against the agent's description of them.\n\nStep 1. Before you look at the diff, go through the user's request sentence by sentence. For each sentence, write one short line: what the user must be able to observe once it is done exactly as written (who can do what, when, with which result, and what must be refused). Add one line for each case the request or the docstrings clearly imply without saying it (the opposite case, an invalid, duplicate or already existing input).\n\nStep 2. For each line of step 1, find the code in the diff that makes it true and follow it with a concrete example. A requirement that the code handles differently from the written words is a problem, even when the agent's variant looks reasonable.\n\nReport only real problems that must be fixed before the work can be accepted: a requirement of step 1 that is not met or is met differently, a bug, a claim in the agent's answer that the changes contradict or do not support (a feature, page, file, test or result it says exists but that is neither in the diff nor plausibly done in an earlier turn), a test that was weakened, skipped or deleted, a security issue. Do not comment on style, naming or anything optional. Be concrete and brief: one line per requirement in each step.\n\nFinish with a line containing only the word VERDICT, followed by either the single line LGTM, or at most six lines, one problem per line, in the form `file:line - problem - what to change`.";

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
    // Les cas limites du genre de fonctionnalité demandé : le relecteur les
    // confronte au code comme les phrases de la demande (essai réel t6 : une
    // période à l'envers et un solde négatif sont passés sans remarque).
    if enabled(Aid::EdgeCases) {
        let cases = feature_edge_cases(&format!("{request}\n{}", earlier.unwrap_or_default()));
        if !cases.is_empty() {
            let _ = write!(
                prompt,
                "What an experienced engineer always checks for this kind of feature. A point that applies is part of the request even though the user did not write it. In Step 1, add a line for each point that applies to this request; in Step 2, say whether the code handles it. The expected result of each try is in parentheses: code that gives another result is a defect to report, even when the agent chose it on purpose:\n- {}\n\n",
                cases.join("\n- ")
            );
        }
    }
    // La réponse de l'agent vient en dernier : elle décrit les changements à
    // sa façon, et le relecteur doit les avoir jugés d'après la demande.
    let answer = answer.trim();
    let _ = write!(
        prompt,
        "Changes made by the agent during this turn (earlier turns may have changed other files):\n{body}\n\nWhat the agent is about to tell the user (claims to check against the changes, not a description to trust).\nThe agent's final answer to the user:\n{}",
        if answer.is_empty() {
            "(none yet)".to_string()
        } else {
            shortened(answer, MAX_REVIEW_TEXT_CHARS)
        }
    );
    // Rappelé en dernier : dans les essais réels, des relecteurs répondaient
    // « VERDICT / LGTM » sans aucune analyse.
    prompt.push_str("\n\n");
    prompt.push_str(REVIEW_FORMAT_REMINDER);
    prompt
}

/// Fin de la demande au relecteur : la forme exigée, même quand tout est juste.
const REVIEW_FORMAT_REMINDER: &str = "Now write your review in the required form: Step 1 (one line per sentence of the request), Step 2 (the code that meets each line, or what differs), then the VERDICT line. Write Step 1 and Step 2 even if everything is correct: an approval without them is not accepted.";

/// Ce que le relecteur conclut : ce qui suit la dernière ligne `VERDICT` de
/// sa réponse. Sans elle (modèle qui n'a pas suivi la forme demandée), toute
/// la réponse.
fn review_verdict(answer: &str) -> &str {
    // Même longueur en octets : seules les lettres ASCII changent.
    match answer.to_ascii_uppercase().rfind("VERDICT") {
        Some(at) => answer[at + "VERDICT".len()..].trim_start_matches(|character: char| {
            character.is_whitespace() || matches!(character, ':' | '*' | '#' | '`')
        }),
        None => answer,
    }
}

/// Journal des relectures, pour savoir ce qu'un relecteur a répondu, surtout
/// quand il approuve : une ligne JSON par relecture dans le fichier que nomme
/// `CLAW_REVIEW_LOG`. Sans cette variable, rien n'est écrit.
pub fn log_review(model: &str, answer: &str) {
    let Ok(path) = std::env::var("CLAW_REVIEW_LOG") else {
        return;
    };
    let line = serde_json::json!({ "model": model, "answer": answer }).to_string() + "\n";
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = std::io::Write::write_all(&mut file, line.as_bytes());
    }
}

/// Le relecteur approuve sans avoir écrit l'analyse demandée (ni « Step 1 »
/// ni « Step 2 ») : rien ne dit qu'il a relu le travail.
#[must_use]
pub fn bare_approval(answer: &str) -> bool {
    let lower = answer.to_ascii_lowercase();
    review_findings(answer).is_none() && !lower.contains("step 1") && !lower.contains("step 2")
}

/// La ligne cite un emplacement du code (`services.py:43`, `src/a.rs:12-20`).
fn cites_a_location(line: &str) -> bool {
    line.split_whitespace().any(|word| {
        let word = word
            .trim_matches(|character: char| matches!(character, '`' | '*' | '(' | ')' | ',' | ';'));
        word.split_once(':').is_some_and(|(file, rest)| {
            file.contains('.') && rest.starts_with(|character: char| character.is_ascii_digit())
        })
    })
}

/// Remarques du relecteur, ou `None` s'il approuve (« LGTM »). Seul son
/// verdict repart à l'agent, pas le raisonnement qui le précède.
#[must_use]
pub fn review_findings(answer: &str) -> Option<String> {
    let marked = answer.to_ascii_uppercase().contains("VERDICT");
    let text = review_verdict(answer).trim();
    let head: String = text
        .chars()
        .take(24)
        .collect::<String>()
        .to_ascii_uppercase();
    // « NOT LGTM » est un refus.
    let lgtm = |text: &str| text.contains("LGTM") && !text.contains("NOT LGTM");
    let approved = if marked {
        // Un « LGTM » n'efface pas les problèmes que le verdict cite avec
        // lui (un relecteur réel en a mis un après sa liste).
        lgtm(&text.to_ascii_uppercase()) && !text.lines().any(cites_a_location)
    } else {
        // Sans ligne `VERDICT`, une approbation peut clore un raisonnement.
        let closing = text.lines().last().unwrap_or_default().trim();
        lgtm(&head)
            || closing
                .trim_matches(|character: char| !character.is_ascii_alphabetic())
                .eq_ignore_ascii_case("LGTM")
    };
    if text.is_empty() || approved {
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
mod error_site_tests {
    use super::{cited_sites, error_sites};

    #[test]
    fn a_python_traceback_is_read_from_the_frame_that_raises() {
        // Le bogue est dans le dernier cadre, pas dans manage.py.
        let output = "Traceback (most recent call last):\n  File \"manage.py\", line 22, in <module>\n  File \"shop/views.py\", line 40, in show\n  File \"shop/pricing.py\", line 12, in price\nKeyError: 'eur'\n";
        assert_eq!(
            cited_sites(output),
            [
                ("shop/pricing.py".to_string(), 12),
                ("shop/views.py".to_string(), 40),
                ("manage.py".to_string(), 22),
            ]
        );
    }

    #[test]
    fn locations_are_read_from_the_usual_error_formats() {
        let output = r#"  File "app/main.py", line 12, in <module>
error[E0425]: cannot find value `x` in this scope
 --> src/lib.rs:42:5
src/page.ts(7,3): error TS2304: Cannot find name 'y'.
    at load (C:\proj\web\app.js:30:11)
tests/test_total.py:9: AssertionError
./main.go:5:2: undefined: z
src/lib.rs:42:5: déjà cité
Listening on http://localhost:3000 and 127.0.0.1:8000
GET https://cdn.example.com/lib.js:8080 failed, see http://docs.example.com/err.html:3"#;
        let sites = cited_sites(output);
        assert_eq!(
            sites,
            [
                ("app/main.py".to_string(), 12),
                ("src/lib.rs".to_string(), 42),
                ("src/page.ts".to_string(), 7),
                (r"C:\proj\web\app.js".to_string(), 30),
                ("tests/test_total.py".to_string(), 9),
                ("./main.go".to_string(), 5),
            ]
        );
    }

    #[test]
    fn the_cited_project_code_is_shown_and_dependencies_are_skipped() {
        let root = std::env::temp_dir().join(format!(
            "claw-sites-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |elapsed| elapsed.as_nanos())
        ));
        std::fs::create_dir_all(root.join("app")).expect("dossier");
        std::fs::create_dir_all(root.join(".venv").join("lib")).expect("dépendances");
        let code = (1..=20)
            .map(|n| format!("ligne {n}\n"))
            .collect::<Vec<_>>()
            .concat();
        std::fs::write(root.join("app").join("main.py"), &code).expect("fichier");
        std::fs::write(root.join(".venv").join("lib").join("dep.py"), &code).expect("dépendance");
        let bash = serde_json::json!({
            "stdout": "",
            "stderr": "  File \".venv/lib/dep.py\", line 3, in f\n  File \"app/main.py\", line 10, in g\nNameError: name 'x' is not defined\n  File \"app/absent.py\", line 2\n",
            "returnCodeInterpretation": "exit_code:1",
        })
        .to_string();
        let sites = error_sites(&bash, &root, "python app/main.py").expect("emplacement du projet");
        assert!(sites.starts_with("[Claw] Current code at the project locations"));
        assert!(sites.contains("app/main.py:10\n"), "{sites}");
        assert!(sites.contains(">   10 | ligne 10"), "{sites}");
        assert!(
            sites.contains("     7 | ligne 7") && sites.contains("    13 | ligne 13"),
            "{sites}"
        );
        assert!(
            !sites.contains("dep.py"),
            "les dépendances ne sont pas montrées : {sites}"
        );
        // Rien d'utile : pas de note.
        assert_eq!(error_sites("tout va bien", &root, ""), None);
        // Une adresse web n'est pas un chemin réseau à aller chercher.
        assert_eq!(
            error_sites("GET https://cdn.example.com/lib.js:8080 failed", &root, ""),
            None
        );
        let _ = std::fs::remove_dir_all(root);
    }
}

#[cfg(test)]
mod guide_tests {
    use super::{
        checklist_prompt, empty_titles, empty_titles_feedback, image_guide, problem_guide,
        read_report, tool_named_in_command, triage_guide, untried_commands, untried_feedback,
        Report, GUIDE_PREFIX,
    };

    #[test]
    fn problem_reports_are_told_apart_from_requests() {
        // Messages d'une vraie session, avec ou sans captures d'écran.
        for (text, images) in [
            ("c'est n'importe quoi", 2),
            ("çaa reste hors ligne", 0),
            ("ça ne marche pas", 0),
            ("le bouton n'affiche rien", 0),
            ("corrige le bug du panier", 0),
            ("it still fails on empty input", 0),
            ("c'est toujours le même", 1),
        ] {
            assert_eq!(read_report(text, images, 0), Report::Problem, "{text}");
        }
        // Une sortie d'erreur collée dans un long message.
        let dump = format!(
            "{}\nTraceback (most recent call last):\n  File \"a.py\", line 3",
            "ligne de journal sans intérêt ".repeat(20)
        );
        assert_eq!(read_report(&dump, 0, 0), Report::Problem);
        // Des images sans plainte écrite : à trier, ce n'est pas forcément un
        // problème (« c'est la même chose » montrait un résultat accepté).
        for (text, images) in [
            ("voilà avec la premiere option", 2),
            (
                "c'est la même chose, http://localhost:3000 et http://localhost:8000",
                1,
            ),
            ("", 1),
        ] {
            assert_eq!(read_report(text, images, 0), Report::Images, "{text}");
        }
        for (text, images) in [
            ("comment le lancé ?", 0),
            ("ajoute une page d'erreur 404", 0),
            ("fais une interface comme sur l'image", 1),
            ("continue", 0),
            ("merci, c'est parfait", 0),
            ("", 0),
        ] {
            assert_eq!(read_report(text, images, 0), Report::None, "{text}");
        }
        // Une longue demande de travail qui parle d'erreurs n'est pas un signalement...
        let request = format!(
            "{} gère les erreurs de saisie",
            "décris le besoin ".repeat(30)
        );
        assert_eq!(read_report(&request, 1, 0), Report::None);
        // ... sauf si le lecteur d'images voit un défaut dans la capture jointe :
        // un cahier des charges envoyé avec une capture du résultat cassé.
        assert_eq!(read_report(&request, 1, 1), Report::Images);
        assert_eq!(
            read_report("fais une interface comme sur l'image", 1, 1),
            Report::Images
        );

        let guide = problem_guide(2, &[]);
        assert!(guide.starts_with(GUIDE_PREFIX));
        assert!(!guide.starts_with(super::CLAW_NOTE_PREFIX));
        assert!(guide.contains("go through all 2"));
        assert!(guide.contains("2. Reproduce: observe each symptom yourself"));
        assert!(!problem_guide(0, &[]).contains("Their message has"));
        let seen = [(2, "the comparison area is empty".to_string())];
        assert!(problem_guide(2, &seen)
            .contains("The image reader saw a defect in image 2: the comparison area is empty."));

        // Tri : la méthode n'est suivie que si les images signalent un problème.
        let triage = triage_guide(1, &seen);
        assert!(triage.starts_with(GUIDE_PREFIX));
        assert!(triage.contains("The user sent 1 image with their message."));
        assert!(triage.contains("First decide what they mean"));
        assert!(triage.contains("image 2: the comparison area is empty"));
        assert!(triage.contains("4. Fix the cause"));
        assert!(!triage.contains("The user is telling you that something is wrong"));
        let images = image_guide(3);
        assert!(images.starts_with(GUIDE_PREFIX));
        assert!(images.contains("This message has 3 images."));
        assert!(images.contains("not a design to copy"));
    }

    #[test]
    fn a_reply_made_of_empty_titles_is_told_from_a_report() {
        // La réponse 115 d'une vraie session : le travail annoncé, rien dessous.
        let announced = "Maintenant, page frontend dédiée pour l'URL :\n\n### 2. Frontend : page dédiée pour l'URL\n\n#### 2a. Composant URLInput\n\n#### 2b. Page /url\n\n#### 2c. Mise à jour de l'API frontend\n\n#### 2d. Lien depuis la page d'accueil";
        let (titles, ends_on_one) = empty_titles(announced);
        assert!(ends_on_one);
        assert_eq!(
            titles,
            [
                "2. Frontend : page dédiée pour l'URL",
                "2a. Composant URLInput",
                "2b. Page /url",
                "2c. Mise à jour de l'API frontend",
                "2d. Lien depuis la page d'accueil",
            ]
        );
        let feedback = empty_titles_feedback(&titles);
        assert!(feedback.starts_with(
            "[Claw] Your reply has section titles with nothing under them: \"2. Frontend"
        ));
        assert!(feedback.contains("you called no tool"));

        // Un rapport : un titre suivi de sous-titres remplis n'est pas vide.
        let report = "# Résultat\n## Ce qui a changé\n### backend\n- `app/main.py` : route ajoutée\n### frontend\nAucun changement.\n---\n## Vérification\n```bash\n# lancé\n## encore un commentaire\nnpm test\n```";
        assert_eq!(empty_titles(report), (Vec::<String>::new(), false));
        // Un seul titre vide au milieu d'un rapport : rien à signaler.
        let (titles, ends_on_one) = empty_titles("## Fait\n### a\n### b\ntexte");
        assert_eq!((titles.len(), ends_on_one), (1, false));
        // Des commentaires dans un bloc de code ne sont pas des titres.
        assert_eq!(
            empty_titles("Lancez :\n```powershell\n# Dans le terminal\n# Puis\n```")
                .0
                .len(),
            0
        );
        // `#include` ou `#1` ne sont pas des titres.
        assert_eq!(empty_titles("#include <stdio.h>\n#1 priorité").0.len(), 0);
    }

    #[test]
    fn the_checklist_quotes_the_request_to_check() {
        let prompt = checklist_prompt("ajoute la traduction par URL", None, false);
        assert!(prompt.contains("check your work against every requirement"));
        assert!(prompt.contains("word for word:\n<<<\najoute la traduction par URL\n>>>"));
        assert!(prompt.contains("one line of your check per sentence or bullet"));
        // Essais réels t4 : « mis de côté pour le premier de la file : lui
        // seul peut alors l'emprunter » devenait un prêt d'office.
        assert!(prompt.contains("Read each sentence again as it is written"));
        assert!(prompt.contains("must not do it for them automatically"));
        // Une traduction par URL : les pièges de la traduction et des appels
        // à un service extérieur (vraie demande de l'utilisateur, appli manga).
        assert!(prompt.contains("What experienced engineers always check"));
        assert!(prompt.contains("- OCR and translation:"));
        assert!(prompt.contains("- Calling an external service:"));
        // Une demande qui ne nomme aucun genre de fonctionnalité : rien.
        assert!(!checklist_prompt("corrige le bug", None, false)
            .contains("What experienced engineers always check"));
        // Essai réel t5 : une recherche, ses cas limites.
        let search = checklist_prompt(
            "Ajoute une recherche au carnet de recettes : un champ filtre la liste",
            None,
            false,
        );
        assert!(search.contains("What experienced engineers always check"));
        assert!(search.contains("without case and accents"));
        assert!(search.contains("`+` and `%20` both mean a space"));
        // Rejeu de t6 : le modèle gardait l'échange automatique des dates
        // (« 5 (échange auto) ») ; le résultat attendu de chaque essai le dément.
        assert!(search.contains("a different result means your code is wrong"));
        assert!(search.contains("\"oeufs\" (finds \"Œufs\")"));
        // Le même rejeu : un essai par requête, quarante requêtes.
        assert!(search.contains("not one request per try"));
        // Essai réel t7 : la ligne des doublons lue, puis écartée parce que
        // « the request doesn't explicitly ask for it ».
        assert!(search.contains("A line that applies is part of what the user asked"));
        assert!(!prompt.contains("not answering permission requests"));
        // « reprendre » : la demande poursuivie est citée.
        let prompt = checklist_prompt(
            "reprendre",
            Some("dans la langue traduite, c'est que le français ; et une page pour l'URL"),
            true,
        );
        assert!(prompt.contains("Their last message: \"reprendre\""));
        assert!(prompt.contains(
            "<<<\ndans la langue traduite, c'est que le français ; et une page pour l'URL\n>>>"
        ));
        assert!(prompt.contains("not answering permission requests"));
    }

    #[test]
    fn a_json_data_file_reshaped_for_new_code_is_told_apart_from_config() {
        use super::{is_json_data_file, json_data_change, json_data_feedback};
        use std::path::Path;
        assert!(is_json_data_file(Path::new("donnees.json")));
        assert!(is_json_data_file(Path::new("data/contacts.json")));
        for config in [
            "package.json",
            "tsconfig.build.json",
            ".vscode/settings.json",
            ".claw/settings.json",
            "node_modules/x/data.json",
            "jest.config.json",
            "notes.txt",
        ] {
            assert!(!is_json_data_file(Path::new(config)), "{config}");
        }
        // Essai réel t7 : `"inscrits": []` ajouté à chaque atelier.
        let before = r#"{"ateliers": {"Poterie": {"places": 2}, "Couture": {"places": 8}}}"#;
        let migrated = r#"{"ateliers": {"Poterie": {"places": 2, "inscrits": []}, "Couture": {"places": 8, "inscrits": []}}}"#;
        assert_eq!(
            json_data_change(before, migrated),
            Some((vec!["inscrits".to_string()], 0))
        );
        // Une entrée ajoutée (un atelier de plus) : pas une migration.
        let one_more = r#"{"ateliers": {"Poterie": {"places": 2}, "Couture": {"places": 8}, "Yoga": {"places": 5}}}"#;
        assert_eq!(json_data_change(before, one_more), None);
        // Une valeur changée.
        let changed = r#"{"ateliers": {"Poterie": {"places": 3}, "Couture": {"places": 8}}}"#;
        assert_eq!(json_data_change(before, changed), Some((Vec::new(), 1)));
        assert_eq!(json_data_change(before, "pas du json"), None);
        let note = json_data_feedback("donnees.json", &["inscrits".to_string()], 0);
        assert!(note.starts_with(
            "[Claw] You changed `donnees.json`, the data your program saves: you added `inscrits`"
        ));
        assert!(note.contains("make it read that shape too"));
    }

    #[test]
    fn a_silent_script_that_never_calls_main_is_pointed_out() {
        use super::{python_script, uncalled_main, uncalled_main_note};
        assert_eq!(
            python_script("cd x && python conges.py jours 1 2").as_deref(),
            Some("conges.py")
        );
        assert_eq!(python_script("python -m pytest -q"), None);
        assert_eq!(python_script("python -c \"print(1)\""), None);
        let lost = "import sys\n\ndef main(arguments):\n    print(arguments)\n    return 0\n";
        assert!(uncalled_main(lost));
        assert!(!uncalled_main(&format!(
            "{lost}\nif __name__ == \"__main__\":\n    sys.exit(main(sys.argv[1:]))\n"
        )));
        assert!(!uncalled_main(&format!("{lost}\nmain(sys.argv[1:])\n")));
        assert!(!uncalled_main("def total(x):\n    return x\n"));
        // Essai réel t6 : `python conges.py jours ...` muet, le bloc perdu.
        let folder = std::env::temp_dir().join(format!(
            "claw-muet-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |elapsed| elapsed.as_nanos())
        ));
        std::fs::create_dir_all(&folder).expect("dossier");
        std::fs::write(folder.join("conges.py"), lost).expect("script");
        let silent = r#"{"stdout":"","stderr":""}"#;
        let (script, note) = uncalled_main_note(
            "python conges.py jours 13/04/2026 17/04/2026",
            silent,
            &folder,
        )
        .expect("remarque");
        assert_eq!(script, "conges.py");
        assert!(note
            .starts_with("[Claw] `python conges.py jours 13/04/2026 17/04/2026` printed nothing"));
        assert!(note.contains("defines `main()` but never calls it"));
        // Une sortie, ou un script qui appelle `main()` : rien à dire.
        let printed = r#"{"stdout":"5","stderr":""}"#;
        assert!(uncalled_main_note("python conges.py jours", printed, &folder).is_none());
        std::fs::write(folder.join("conges.py"), format!("{lost}\nmain([])\n")).expect("script");
        assert!(uncalled_main_note("python conges.py jours", silent, &folder).is_none());
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn a_lost_change_is_named_with_the_command_that_restores_it() {
        let cwd = std::path::Path::new("projet");
        assert_eq!(
            super::kept_copy_path(cwd, &cwd.join("equipe").join("calendrier.py")),
            cwd.join(".claw")
                .join("kept")
                .join("equipe")
                .join("calendrier.py")
        );
        // Hors du projet : dans le dossier temporaire.
        let outside = std::path::Path::new("ailleurs").join("x.py");
        assert!(super::kept_copy_path(cwd, &outside).starts_with(std::env::temp_dir()));
        let deleted = super::lost_edits_note("conges.py", Some(".claw/kept/conges.py"), true);
        assert!(deleted.starts_with("[Claw] This command deleted `conges.py`"));
        assert!(deleted.contains("`cp \".claw/kept/conges.py\" \"conges.py\"`"));
        let reverted = super::lost_edits_note("conges.py", None, false);
        assert!(reverted.contains("is back to its state from before your changes"));
        assert!(!reverted.contains("cp "));
    }

    #[test]
    fn a_request_that_names_a_kind_of_feature_gets_its_edge_cases_from_the_start() {
        let guide = super::edge_case_guide(
            "Je pense que le fichier ventes.csv est corrompu, tu peux le réparer ?",
            None,
        )
        .expect("guide");
        assert!(guide.starts_with(super::GUIDE_PREFIX));
        assert!(guide.contains("non-breaking space"));
        // Essai réel t6 : cinq essais du guide, une requête chacun.
        assert!(guide.contains("not one request per try"));
        // « continue » : le genre se lit dans la demande poursuivie.
        assert!(super::edge_case_guide("continue", Some("ajoute un filtre par prix")).is_some());
        assert_eq!(super::edge_case_guide("explique ce module", None), None);
    }

    #[test]
    fn a_tool_written_as_a_shell_command_is_pointed_out() {
        let hint = tool_named_in_command("KillShell bash_4").expect("remarque");
        assert!(hint.contains("`KillShell` is one of your tools"));
        assert!(tool_named_in_command("cd app && bashoutput bash_2").is_some());
        for fine in [
            "ls",
            "grep -rn read_file src",
            "echo \"KillShell bash_4\"",
            "killall node",
        ] {
            assert!(tool_named_in_command(fine).is_none(), "{fine}");
        }
    }

    #[test]
    fn commands_given_to_the_user_but_never_run_are_found() {
        // La réponse « comment le lancer ? » d'une vraie session.
        let answer = "## Lancement\n\n```bash\ncd backend\n# Installer les dépendances\npip install -r requirements.txt\n\n# Démarrer le serveur (port 8000)\npython -m uvicorn app.main:app --reload --port 8000\n```\n\n```bash\ncd frontend\nnpm install\nnpm run dev\n# ou pour build production:\nnpm run build && npm start\n```\n\n```bash\ndocker-compose up --build\ncurl -X POST http://localhost:8000/api/v1/translate \\\n  -F \"file=@votre_image.png\"\npython3 -m pytest tests/ -v\n```\n\nVoici aussi du code :\n\n```ts\nnpm run inconnu\n```";
        let executed = vec![
            "cd backend && python -m pytest tests/ -v --tb=short 2>&1".to_string(),
            "cd backend && python -m uvicorn app.main:app --port 8000 --log-level info 2>&1 &\nsleep 3".to_string(),
            "cd backend && pip install pytest pytest-asyncio 2>&1".to_string(),
            "cd frontend && npm run build 2>&1".to_string(),
        ];
        assert_eq!(
            untried_commands(answer, &executed),
            [
                "pip install -r requirements.txt",
                "npm install",
                "npm run dev",
                "npm start",
            ]
        );
        // Tout a été lancé, ou rien n'est conseillé : rien à dire.
        assert!(untried_commands("```bash\nnpm run build\n```", &executed).is_empty());
        assert!(
            untried_commands("Lancez `npm run dev` puis ouvrez la page.", &executed).is_empty()
        );
        let feedback = untried_feedback(&["npm run dev".to_string(), "npm start".to_string()]);
        assert!(feedback.contains("`npm run dev`, `npm start`"));
    }
}

#[cfg(test)]
mod question_tests {
    use super::ends_with_questions;

    #[test]
    fn questions_closing_an_answer_are_recognized() {
        // La fin d'une vraie session : une validation demandée en texte.
        let asked = "## Architecture\n\n```\nbackend/\n```\n\n## Validation Requise\n\nAvant de continuer, confirme-moi :\n1. **Cette architecture te convient-elle ?** (Elle est déjà codée)\n2. **Veux-tu que je corrige les points d'attention ci-dessus** (police CJK, tests, streaming) ?\n3. **As-tu les clés API** (DeepL / OpenAI / Anthropic) pour tester ?\n\n> Veux-tu que je lance un test de build ou que je corrige d'abord les points critiques ?";
        assert!(ends_with_questions(asked));
        assert!(ends_with_questions(
            "Two ways to do it.\n\nWhich database do you prefer?\nShould I add tests as well?"
        ));
        for plain in [
            // Travail fini, une simple offre en conclusion.
            "C'est corrigé et les tests passent.\n\nVeux-tu que je regarde autre chose ?",
            "Terminé. Lancez `npm run dev` puis ouvrez http://localhost:3000/?page=2 et http://localhost:3000/?page=3.",
            // Des « ? » dans du code ou un tableau ne sont pas des questions.
            "Voici le code :\n\n```ts\nconst a = b ? c : d;\nconst e = f ? g : h;\n```\n\nTout est en place.",
            "| Question | Réponse |\n|---|---|\n| Pourquoi ce choix ? | Vitesse |\n| Et la mémoire ? | Stable |\n\nRien d'autre à signaler.",
            "Utilisez `a ? b : c` et `x ?? y` selon le cas.",
            "",
        ] {
            assert!(!ends_with_questions(plain), "{plain}");
        }
        // Des questions au début, suivies d'une longue réponse : pas une attente.
        let answered = format!(
            "Pourquoi ce bogue ? Que faire ?\n\n{}",
            "Une ligne d'explication.\n".repeat(14)
        );
        assert!(!ends_with_questions(&answered));
    }
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
        check_tool_is_missing, covers, detect_verify_commands, error_sites, exit_reflects_check,
        is_check_command, original_content, read_bash_output, review_findings, syntax_check,
        unified_diff, verify_commands, verify_commands_for, verify_feedback, StallGuard,
        VerifySetting,
    };
    use std::fs;
    use std::path::Path;

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
        // Des tests écrits pour pytest, sans réglage (essai réel t4).
        assert!(!super::tests_written_for_pytest(&python));
        let written = temp_dir("pytest-sans-reglage");
        fs::create_dir_all(written.join("tests")).expect("dossier");
        fs::write(
            written.join("tests").join("test_services.py"),
            "import pytest\n\nfrom bibliotheque import services\n",
        )
        .expect("fichier");
        assert!(super::tests_written_for_pytest(&written));
        let functions = temp_dir("fonctions-de-test");
        fs::write(
            functions.join("test_calcul.py"),
            "from calcul import total\n\ndef test_total():\n    assert total([]) == 0\n",
        )
        .expect("fichier");
        assert!(super::tests_written_for_pytest(&functions));
        // pytest installé : il est choisi ; sinon, `unittest` comme avant.
        let chosen = detect_verify_commands(&written);
        if super::pytest_installed(super::python_program()) {
            assert!(chosen[0].ends_with("-m pytest -q -x"), "{chosen:?}");
        } else {
            assert!(
                chosen[0].ends_with("-m unittest discover -s tests -q"),
                "{chosen:?}"
            );
        }
        let _ = fs::remove_dir_all(written);
        let _ = fs::remove_dir_all(functions);

        let pytest = temp_dir("pytest");
        fs::write(pytest.join("conftest.py"), "").expect("fichier");
        // Réglé pour pytest mais sans aucun test : rien à lancer, pytest
        // sortirait en échec sans rien vérifier.
        assert!(detect_verify_commands(&pytest).is_empty());
        fs::create_dir_all(pytest.join("tests").join("unit")).expect("dossier");
        fs::write(pytest.join("tests").join("unit").join("test_api.py"), "").expect("fichier");
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
    fn a_project_split_in_subfolders_is_verified_where_the_files_changed() {
        // La disposition d'un vrai projet : rien à la racine, `backend/` avec
        // pytest nommé dans pyproject.toml et ses tests dans `tests/`,
        // `frontend/` avec TypeScript installé.
        let root = temp_dir("monorepo");
        let backend = root.join("backend");
        let frontend = root.join("frontend");
        fs::create_dir_all(backend.join("tests")).expect("backend");
        fs::create_dir_all(backend.join("app")).expect("app");
        fs::write(
            backend.join("pyproject.toml"),
            "[project]\ndependencies = [\"pytest==8.2.0\"]\n",
        )
        .expect("pyproject");
        fs::write(backend.join("tests").join("conftest.py"), "").expect("conftest");
        fs::write(
            backend.join("tests").join("test_api.py"),
            "def test_ok():\n    pass\n",
        )
        .expect("test");
        fs::write(backend.join("app").join("main.py"), "x = 1\n").expect("main");
        fs::create_dir_all(frontend.join("node_modules").join(".bin")).expect("frontend");
        fs::create_dir_all(frontend.join("src")).expect("src");
        fs::write(
            frontend.join("package.json"),
            r#"{"scripts":{"build":"next build"}}"#,
        )
        .expect("package");
        fs::write(frontend.join("tsconfig.json"), "{}").expect("tsconfig");
        fs::write(frontend.join("node_modules").join(".bin").join("tsc"), "").expect("tsc");
        fs::write(frontend.join("src").join("page.tsx"), "export {}\n").expect("page");

        assert!(detect_verify_commands(&root).is_empty());
        let changed = |file: &Path| vec![file.to_string_lossy().into_owned()];
        // Un fichier du frontend modifié : sa vérification, depuis son dossier.
        assert_eq!(
            verify_commands_for(
                &VerifySetting::Auto,
                &root,
                &changed(&frontend.join("src").join("page.tsx"))
            ),
            vec!["cd frontend && npx tsc --noEmit"]
        );
        let backend_checks = verify_commands_for(
            &VerifySetting::Auto,
            &root,
            &changed(&backend.join("app").join("main.py")),
        );
        assert_eq!(backend_checks.len(), 1, "{backend_checks:?}");
        // Un test modifié : le projet reste `backend/`, une seule fois, même
        // avec un fichier du code à côté.
        let both = [
            backend
                .join("tests")
                .join("test_api.py")
                .to_string_lossy()
                .into_owned(),
            backend
                .join("app")
                .join("main.py")
                .to_string_lossy()
                .into_owned(),
        ];
        assert_eq!(
            verify_commands_for(&VerifySetting::Auto, &root, &both),
            backend_checks
        );
        assert!(
            backend_checks[0].starts_with("cd backend && "),
            "{backend_checks:?}"
        );
        assert!(
            backend_checks[0].ends_with("-m pytest -q -x"),
            "{backend_checks:?}"
        );
        // Sans fichier connu : les sous-dossiers qui portent un manifeste.
        assert_eq!(
            verify_commands_for(&VerifySetting::Auto, &root, &[]).len(),
            2
        );

        // Le modèle a lancé lui-même cette vérification, avec son propre chemin.
        assert!(covers(
            r"cd D:\Mes_Projets\test_claw\frontend && npx tsc --noEmit",
            "cd frontend && npx tsc --noEmit"
        ));
        assert!(!covers(
            "cd backend && npx tsc --noEmit",
            "cd frontend && npx tsc --noEmit"
        ));
        assert!(!covers(
            "cd frontend && npm run lint",
            "cd frontend && npx tsc --noEmit"
        ));

        // Une erreur cite un chemin depuis le dossier où la commande s'est placée.
        let sites = error_sites(
            "./src/page.tsx:1:8\nType error: something",
            &root,
            "cd frontend && npm run build 2>&1 | tail -20",
        )
        .expect("emplacement trouvé depuis frontend");
        assert!(sites.contains("frontend/src/page.tsx:1"), "{sites}");
        assert_eq!(
            error_sites("./src/page.tsx:1:8", &root, "npm run build"),
            None
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn data_rewritten_by_a_command_is_seen_against_its_original() {
        use super::DataSnapshot;
        let root = temp_dir("donnees-commande");
        let git = |args: &[&str]| {
            std::process::Command::new("git")
                .args(args)
                .current_dir(&root)
                .output()
                .is_ok_and(|output| output.status.success())
        };
        if !git(&["init", "-q"]) {
            let _ = fs::remove_dir_all(root);
            return;
        }
        fs::write(root.join("ventes.csv"), "date;montant\n2026-09-02;12,5\n").expect("csv");
        fs::write(root.join("brouillon.csv"), "a;b\n1;2\n").expect("csv");
        fs::write(root.join("main.py"), "x = 1\n").expect("main");
        git(&["add", "ventes.csv", "main.py"]);
        git(&[
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "commit",
            "-q",
            "-m",
            "départ",
        ]);
        let mut snapshot = DataSnapshot::take(&root).expect("dépôt git");
        assert!(snapshot.rewritten(&root).is_empty());
        // Essai réel t3 : le CSV « réparé » de mémoire, ici par une commande.
        std::thread::sleep(std::time::Duration::from_millis(20));
        fs::write(root.join("ventes.csv"), "date;montant\n2026-09-02;12.5\n").expect("csv");
        let found = snapshot.rewritten(&root);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].0.ends_with("ventes.csv"));
        assert_eq!(found[0].1, vec!["2026-09-02;12,5".to_string()]);
        // Déjà vu : rien de plus tant que le fichier ne change pas.
        assert!(snapshot.rewritten(&root).is_empty());
        // Un fichier nouveau (pas encore commité) : comparé à son contenu de départ.
        fs::write(root.join("brouillon.csv"), "a;b\n1;2\n3;4\n").expect("csv");
        assert!(
            snapshot.rewritten(&root).is_empty(),
            "une ligne ajoutée ne retire rien"
        );
        fs::write(root.join("brouillon.csv"), "a;b\n").expect("csv");
        assert_eq!(snapshot.rewritten(&root)[0].1, vec!["1;2".to_string()]);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn files_changed_by_a_command_are_seen_in_git() {
        use super::{changed_since, project_snapshot};
        let root = temp_dir("instantane-git");
        let git = |args: &[&str]| {
            std::process::Command::new("git")
                .args(args)
                .current_dir(&root)
                .output()
                .is_ok_and(|output| output.status.success())
        };
        if !git(&["init", "-q"]) {
            // Pas de git sur cette machine : rien à observer.
            let _ = fs::remove_dir_all(root);
            return;
        }
        fs::write(root.join("main.py"), "x = 1\n").expect("main");
        fs::write(root.join("notes.log"), "début\n").expect("journal");
        let before = project_snapshot(&root).expect("dépôt git");
        assert!(changed_since(&before, &root).is_empty());
        // Une commande change le code, un serveur écrit son journal.
        fs::write(root.join("main.py"), "x = 22\n").expect("main");
        fs::write(root.join("notes.log"), "suite\n").expect("journal");
        fs::create_dir_all(root.join("node_modules").join("pkg")).expect("dépendance");
        fs::write(root.join("node_modules").join("pkg").join("index.js"), "").expect("js");
        let changed = changed_since(&before, &root);
        assert_eq!(changed.len(), 1, "{changed:?}");
        assert!(changed[0].ends_with("main.py"), "{changed:?}");
        // Hors d'un dépôt : rien.
        let elsewhere = temp_dir("sans-git");
        assert_eq!(project_snapshot(&elsewhere.join("absent")), None);
        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(elsewhere);
    }

    #[test]
    fn a_changed_subproject_is_verified_even_when_the_root_has_a_check() {
        // Une API Python à la racine, l'interface dans `frontend/`.
        let root = temp_dir("racine-et-frontend");
        fs::write(root.join("pytest.ini"), "[pytest]\n").expect("pytest");
        fs::create_dir_all(root.join("tests")).expect("tests");
        fs::write(root.join("tests").join("test_api.py"), "").expect("test");
        fs::create_dir_all(root.join("app")).expect("app");
        fs::write(root.join("app").join("main.py"), "x = 1\n").expect("main");
        let frontend = root.join("frontend");
        fs::create_dir_all(frontend.join("node_modules").join(".bin")).expect("frontend");
        fs::create_dir_all(frontend.join("src")).expect("src");
        fs::write(frontend.join("tsconfig.json"), "{}").expect("tsconfig");
        fs::write(frontend.join("package.json"), "{}").expect("package");
        fs::write(frontend.join("node_modules").join(".bin").join("tsc"), "").expect("tsc");
        fs::write(frontend.join("src").join("App.tsx"), "export {}\n").expect("app");

        let path = |file: &Path| file.to_string_lossy().into_owned();
        let at_root = detect_verify_commands(&root);
        assert_eq!(at_root.len(), 1, "{at_root:?}");
        // Seul le frontend a changé : sa vérification, pas celle de la racine.
        assert_eq!(
            verify_commands_for(
                &VerifySetting::Auto,
                &root,
                &[path(&frontend.join("src").join("App.tsx"))]
            ),
            vec!["cd frontend && npx tsc --noEmit"]
        );
        // Seule l'API a changé : la vérification de la racine.
        assert_eq!(
            verify_commands_for(
                &VerifySetting::Auto,
                &root,
                &[path(&root.join("app").join("main.py"))]
            ),
            at_root
        );
        // Les deux : les deux vérifications.
        let both = verify_commands_for(
            &VerifySetting::Auto,
            &root,
            &[
                path(&root.join("app").join("main.py")),
                path(&frontend.join("src").join("App.tsx")),
            ],
        );
        assert_eq!(both.len(), 2, "{both:?}");
        assert_eq!(both[0], at_root[0]);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn a_check_counts_only_when_its_exit_code_is_the_command_s() {
        let wanted = "npx tsc --noEmit";
        for ran in [
            "npx tsc --noEmit",
            "cd frontend && npx tsc --noEmit",
            "npx tsc --noEmit 2>&1 | head -50",
            "npx tsc --noEmit && echo OK",
            "npx tsc --noEmit;",
            "npx tsc --noEmit && npm test",
        ] {
            assert!(covers(ran, wanted), "{ran}");
            assert!(exit_reflects_check(ran), "{ran}");
        }
        // Une vérification suivie d'une autre que `| wc -l` masque : la
        // première est faite, la commande ne dit rien de la seconde.
        assert!(covers("npx tsc --noEmit && npm test | wc -l", wanted));
        assert!(!exit_reflects_check("npx tsc --noEmit && npm test | wc -l"));
        for ran in [
            "npx tsc --noEmit || echo \"Erreurs de type\"",
            "npx tsc --noEmit && echo OK || echo KO",
            "npx tsc --noEmit; echo \"exit=$?\"",
            "npx tsc --noEmit 2>&1 | wc -l",
            "npx tsc --noEmit || true",
            "npx tsc --noEmit &",
        ] {
            assert!(!covers(ran, wanted), "{ran}");
            assert!(!exit_reflects_check(ran), "{ran}");
        }
        assert!(!covers(
            "cd frontend && npx tsc --noEmit || true",
            "cd frontend && npx tsc --noEmit"
        ));
        assert!(!exit_reflects_check("ls -la"));
    }

    #[test]
    fn a_check_is_told_by_its_program_not_by_a_word_in_a_path() {
        for check in [
            "cargo test",
            "cd rust && cargo clippy --all-targets",
            "npm test",
            "npm run test:unit -- --watch=false",
            "pnpm run typecheck",
            "npx tsc --noEmit",
            "python -m pytest tests/ -v",
            "python3 -m unittest discover",
            "pytest -q",
            "uv run pytest",
            "go vet ./...",
            "make",
            "make check",
            "./gradlew build",
            "node_modules/.bin/eslint src",
            "cd frontend && npm run build 2>&1 | tail -20",
        ] {
            assert!(is_check_command(check), "{check}");
        }
        for other in [
            // Le mot « test » est dans le chemin, pas dans la commande.
            r#"cd D:\Mes_Projets\test_claw\backend && python -c "from app.main import app; print('ok')""#,
            "cat tests/test_api.py",
            "ls build",
            "git status",
            "python build_docs.py",
            "npm run dev",
            "npm install",
            "curl http://localhost:8000/api/v1/health",
            "echo \"cargo test\"",
        ] {
            assert!(!is_check_command(other), "{other}");
        }
        assert!(check_tool_is_missing(
            "C:\\Python313\\python.exe: No module named pytest"
        ));
        assert!(!check_tool_is_missing("assert 1 == 2"));
    }

    #[test]
    fn a_changed_file_is_shown_so_that_it_is_not_read_again() {
        use super::{file_view_marker, view_after_edit, FILE_VIEW_END, FILE_VIEW_PREFIX};
        let dir = std::env::temp_dir().join(format!("claw-vue-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dossier");
        let output = |path: &std::path::Path, start: usize, count: usize| {
            serde_json::json!({
                "filePath": path.to_string_lossy(),
                "structuredPatch": [{ "oldStart": start, "oldLines": 1, "newStart": start, "newLines": count }]
            })
            .to_string()
        };
        // Petit fichier : montré en entier, tel qu'il est sur le disque.
        let small = dir.join("notes.py");
        std::fs::write(&small, "a = 1\nb = 2\n").expect("petit fichier");
        let view = view_after_edit("edit_file", &output(&small, 2, 1)).expect("vue");
        assert!(view.starts_with(FILE_VIEW_PREFIX), "{view}");
        assert!(view.contains("as it is now (2 lines)"), "{view}");
        assert!(view.contains("\na = 1\nb = 2\n"), "{view}");
        // Fichier en CRLF : montré sans retours chariot, comme read_file.
        std::fs::write(&small, "a = 1\r\nb = 2\r\n").expect("CRLF");
        let crlf = view_after_edit("edit_file", &output(&small, 2, 1)).expect("vue");
        assert!(
            crlf.contains("\na = 1\nb = 2\n") && !crlf.contains('\r'),
            "{crlf:?}"
        );
        std::fs::write(&small, "a = 1\nb = 2\n").expect("petit fichier");
        assert!(view.ends_with(FILE_VIEW_END), "{view}");
        let marker = file_view_marker(&output(&small, 2, 1)).expect("marque");
        assert!(view.starts_with(&marker), "{marker}");
        // Après `write_file`, une ligne suffit : le modèle vient de tout écrire.
        let sent = |content: &str, note: Option<&str>| {
            serde_json::json!({ "filePath": small.to_string_lossy(), "content": content, "note": note })
                .to_string()
        };
        let written = view_after_edit("write_file", &sent("a = 1\nb = 2\n", None)).expect("ligne");
        assert!(written.contains("is written exactly as you sent it (2 lines)"));
        assert!(!written.contains("a = 1"));
        // Changé après l'écriture (formateur, hook) : le fichier est montré.
        let changed = view_after_edit("write_file", &sent("a=1\nb=2\n", None)).expect("vue");
        assert!(
            changed.starts_with(FILE_VIEW_PREFIX) && changed.contains("a = 1"),
            "{changed}"
        );
        // L'outil a déjà une remarque (contenu abrégé) : rien de rassurant.
        assert!(
            view_after_edit("write_file", &sent("a = 1\nb = 2\n", Some("placeholder"))).is_none()
        );
        // Grand fichier : les lignes autour du changement, d'après le vrai
        // résultat de l'outil (un seul bloc de diff pour tout le fichier).
        let big = dir.join("page.tsx");
        let text: Vec<String> = (1..=400).map(|n| format!("ligne {n}")).collect();
        std::fs::write(&big, text.join("\n")).expect("grand fichier");
        let edited = crate::file_ops::edit_file(
            &big.to_string_lossy(),
            "ligne 300\n",
            "ligne 300 changée\nligne ajoutée\n",
            false,
        )
        .expect("modification");
        let result = serde_json::to_string(&edited).expect("JSON");
        let view = view_after_edit("edit_file", &result).expect("vue");
        assert!(view.contains("now has 401 lines"), "{view}");
        assert!(view.contains("Lines 292 to 309 as they are now"), "{view}");
        assert!(
            view.contains("ligne 300 changée\nligne ajoutée\nligne 301"),
            "{view}"
        );
        assert!(!view.contains("ligne 150\n"));
        assert!(view.ends_with(FILE_VIEW_END), "{view}");
        // Une ligne énorme est coupée, la vue reste bornée.
        let wide = dir.join("donnees.json");
        std::fs::write(&wide, format!("{{\"a\": \"{}\"}}\n", "x".repeat(50_000))).expect("large");
        let edited = crate::file_ops::edit_file(&wide.to_string_lossy(), "\"a\"", "\"b\"", false)
            .expect("modification");
        let view = view_after_edit("edit_file", &serde_json::to_string(&edited).expect("JSON"))
            .expect("vue");
        assert!(view.len() < 1_000, "{}", view.len());
        assert!(view.contains("line cut: "), "{view}");
        // Fichier disparu ou résultat sans chemin : rien.
        assert!(view_after_edit("edit_file", &output(&dir.join("absent.py"), 1, 1)).is_none());
        assert!(view_after_edit("edit_file", "pas du JSON").is_none());
        let _ = std::fs::remove_dir_all(&dir);
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

        // Partie en arrière-plan : pas d'échec, mais rien de vérifié.
        let background = read_bash_output(
            r#"{"stdout":"[Claw] started","stderr":"","backgroundTaskId":"bash_1","returnCodeInterpretation":null}"#,
        );
        assert!(background.passed && background.pending);
        assert!(!passed.pending);
        // Un message de hook à la suite du JSON ne change pas l'issue.
        let with_hook = read_bash_output("{\"stdout\":\"ok\",\"stderr\":\"\"}\n\nhook: formaté");
        assert!(with_hook.passed && !with_hook.pending);

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
        // Raisonnement en deux étapes, puis le verdict : lui seul compte.
        let reasoned = "Step 1.\n- returning a book sets it aside for the first in line\nStep 2.\n- services.py lends it at once: differs\n\nVERDICT\nservices.py:31 - the book is lent to the first in line instead of being set aside - keep it available for that member only";
        assert_eq!(
            review_findings(reasoned).as_deref(),
            Some("services.py:31 - the book is lent to the first in line instead of being set aside - keep it available for that member only")
        );
        assert_eq!(
            review_findings("Step 1.\n- one requirement\nStep 2.\n- met\n\nVERDICT\nLGTM"),
            None
        );
        assert_eq!(review_findings("Step 2.\n- met\n**Verdict:** LGTM"), None);
        assert_eq!(review_findings("Step 2.\n- met\nVERDICT"), None);
        // Verdict oublié : une approbation en dernière ligne en est une.
        assert_eq!(review_findings("Step 1.\n- one requirement\n\nLGTM."), None);
        // « NOT LGTM » n'approuve rien.
        assert_eq!(
            review_findings("VERDICT\nNOT LGTM: a.py:3 - the book is lent at once").as_deref(),
            Some("NOT LGTM: a.py:3 - the book is lent at once")
        );
        // Relecture rejouée sur t4c (dots) : des problèmes, puis « LGTM » à
        // la suite. Les problèmes l'emportent, sur la même ligne ou après.
        let listed = "Step 2.\n- emprunter lends a book still borrowed\n\nVERDICT\n`services.py:15-26 - borrowing is checked against the queue before the loan - check the loan first` `tests/test_services.py:93-99 - the test accepts it - fix the test` LGTM";
        assert!(review_findings(listed).is_some_and(|found| found.starts_with("services.py:15-26")));
        assert!(review_findings(
            "VERDICT\nservices.py:43 - the borrower can reserve his own book - refuse it\nLGTM"
        )
        .is_some());
        // Sans emplacement cité, un « LGTM » du verdict reste une approbation.
        assert_eq!(review_findings("**Verdict:** LGTM, nothing to fix"), None);
        // Approuver sans l'analyse demandée : un second relecteur est consulté.
        assert!(super::bare_approval("VERDICT\nLGTM"));
        assert!(super::bare_approval("LGTM"));
        assert!(!super::bare_approval(
            "Step 1.\n- one requirement\nStep 2.\n- met\n\nVERDICT\nLGTM"
        ));
        assert!(!super::bare_approval("VERDICT\na.py:3 - bug - fix"));
        // Dans la demande au relecteur, la réponse de l'agent vient après le diff.
        let prompt =
            super::review_prompt("ajoute x", None, "Fait.", &["--- a/x\n+++ b/x".to_string()]);
        assert!(prompt.starts_with("The user asked:\najoute x"));
        // La forme exigée est rappelée en dernier.
        assert!(prompt.ends_with("an approval without them is not accepted."));
        // Une demande qui nomme un genre de fonctionnalité : ses cas limites aussi.
        assert!(!prompt.contains("What an experienced engineer always checks"));
        let dated = super::review_prompt(
            "Ajoute le calcul des jours ouvrés entre deux dates",
            None,
            "Fait.",
            &["--- a/x\n+++ b/x".to_string()],
        );
        assert!(dated.contains("What an experienced engineer always checks"));
        assert!(dated.contains("code that gives another result is a defect to report"));
        assert!(
            dated
                .find("What an experienced engineer")
                .expect("cas limites")
                < dated.find("Changes made by the agent").expect("diff")
        );
        assert!(
            prompt.find("--- a/x").expect("diff")
                < prompt
                    .find("The agent's final answer to the user:\nFait.")
                    .expect("réponse")
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
        // Données : par extension, et la remarque nomme le fichier.
        assert!(super::is_data_file(std::path::Path::new("data/ventes.CSV")));
        assert!(super::is_data_file(std::path::Path::new("journal.jsonl")));
        assert!(!super::is_data_file(std::path::Path::new("rapport.py")));
        assert!(!super::is_data_file(std::path::Path::new("package.json")));
        let note = super::data_guard_feedback("data/ventes.csv", &["a;12,5".to_string()]);
        assert!(
            note.contains("`data/ventes.csv`") && note.ends_with("\na;12,5"),
            "{note}"
        );
        // Lignes retirées lues même quand un retour de hook suit le JSON.
        let patched = "{\"structuredPatch\":[{\"lines\":[\"-a;12,5\",\"-b;1\",\"+a;12.5\",\"+b;1\"]}]}\n\nRetour des hooks:\nformaté";
        assert_eq!(super::replaced_lines(patched), vec!["a;12,5".to_string()]);
        assert_eq!(
            super::original_content(patched).as_deref(),
            Some("a;12,5\nb;1")
        );
        // Python du Microsoft Store et dossier temporaire.
        let stderr = r"C:\Users\HP\AppData\Local\Microsoft\WindowsApps\python.exe: can't open file 'C:\\Users\\HP\\AppData\\Local\\Temp\\essai\\notes_test.py': [Errno 2] No such file or directory";
        assert!(super::store_python_hint(stderr).is_some_and(|hint| hint.contains("/tmp")));
        assert!(super::store_python_hint("python: can't open file 'x.py'").is_none());
        // Un autre Python, l'alias du Store seulement cité plus haut : rien.
        let other = "C:\\Users\\HP\\AppData\\Local\\Microsoft\\WindowsApps\\python.exe\nC:\\Python312\\python.exe: can't open file 'C:\\Users\\HP\\AppData\\Local\\Temp\\x.py'";
        assert!(super::store_python_hint(other).is_none());

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
    fn single_commands_are_shown_chained_in_one_call() {
        use super::{batch_note, chained_commands};
        assert_eq!(
            chained_commands(&["python a.py", " python b.py ", "python c.py"]).as_deref(),
            Some("python a.py; python b.py; python c.py")
        );
        // Le même `cd dossier &&` devant chacune : une fois suffit.
        assert_eq!(
            chained_commands(&["cd app && npm test", "cd app && npm run lint"]).as_deref(),
            Some("cd app && npm test; npm run lint")
        );
        // Un autre dossier, ou un `&&` qui n'est pas un `cd` : commandes entières.
        assert_eq!(
            chained_commands(&["cd app && ls", "cd lib && ls"]).as_deref(),
            Some("cd app && ls; cd lib && ls")
        );
        assert_eq!(
            chained_commands(&["make && ./a", "make && ./b"]).as_deref(),
            Some("make && ./a; make && ./b")
        );
        // Ce qui ne s'enchaîne pas : plusieurs lignes, arrière-plan, trop long, seule.
        assert_eq!(
            chained_commands(&["python - <<EOF\nprint(1)\nEOF", "ls"]),
            None
        );
        assert_eq!(chained_commands(&["npm run dev &", "ls"]), None);
        assert_eq!(chained_commands(&[&"x".repeat(201), "ls"]), None);
        assert_eq!(chained_commands(&["ls"]), None);

        let command = |tool: &str, text: &str| (tool.to_string(), text.to_string());
        let recent = [
            command("bash", "python a.py"),
            command("bash", "python b.py"),
            command("bash", "python c.py"),
        ];
        let first = batch_note(3, &recent);
        assert!(first.starts_with("[Claw] Your last 3 replies each made a single call"));
        assert!(first.ends_with("fit in one `bash` call: `python a.py; python b.py; python c.py`"));
        // Redite : plus ferme, et elle compte les requêtes dépensées.
        let again = batch_note(6, &recent);
        assert!(again.starts_with("[Claw] 6 replies in a row have now made a single call each"));
        assert!(again.contains("send it all in your next reply"));
        // Sans trois commandes du même outil à la suite : pas d'exemple inventé.
        let mixed = [
            command("bash", "python a.py"),
            command("PowerShell", "Get-ChildItem"),
            command("bash", "python c.py"),
        ];
        assert!(batch_note(3, &mixed).ends_with("(`cmd1; cmd2; cmd3`)."));
        assert!(batch_note(3, &recent[..2]).ends_with("(`cmd1; cmd2; cmd3`)."));
    }

    #[test]
    fn a_read_result_names_its_file_and_where_its_json_ends() {
        let json = r#"{"type":"text","file":{"filePath":"D:/projet/a.py","content":"x = 1\n","numLines":1,"startLine":1,"totalLines":1}}"#;
        let noted = format!("{json}\n\n[Claw] une remarque");
        assert_eq!(
            super::read_result(&noted),
            Some(("D:/projet/a.py".to_string(), json.len()))
        );
        // Une image, une erreur en texte : pas une lecture de texte.
        assert_eq!(super::read_result(r#"{"type":"image"}"#), None);
        assert_eq!(super::read_result("fichier introuvable"), None);
    }

    #[test]
    fn a_task_list_with_open_tasks_has_work_left() {
        let list = |statuses: &[&str]| {
            let todos = statuses
                .iter()
                .map(|status| serde_json::json!({ "content": "x", "status": status }))
                .collect::<Vec<_>>();
            format!(
                "{}\n\nRetour des hooks:\nok",
                serde_json::json!({ "oldTodos": [], "newTodos": todos })
            )
        };
        assert!(super::todos_left(&list(&["completed", "in_progress"])));
        assert!(super::todos_left(&list(&["pending"])));
        assert!(!super::todos_left(&list(&["completed", "completed"])));
        assert!(!super::todos_left(&list(&[])));
        assert!(!super::todos_left("liste illisible"));
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
