//! `claw models bench` : cherche les modèles gratuits les plus performants pour
//! le code **et** compatibles avec Claw.
//!
//! 1. Sonde : un vrai appel d'outil (`read_file`) dont la réponse doit citer un
//!    code secret — détecte les modèles refusés à Claw, surchargés, qui
//!    n'appellent pas les outils ou répondent faux (environ 2 requêtes).
//! 2. Tâches : les meilleurs modèles compatibles corrigent un bug, implémentent
//!    une fonction et renomment dans plusieurs fichiers ; les tests du projet
//!    vérifient le travail (Python requis).
//!
//! Chaque essai lance `claw prompt` dans un projet temporaire, comme un vrai
//! usage. Les résultats sont gardés (`model-bench.json`) et orientent le choix
//! automatique du modèle gratuit.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Mesures plus anciennes ignorées : les modèles gratuits changent vite.
const RESULTS_TTL_SECS: u64 = 14 * 24 * 3600;
const DEFAULT_TASK_MODELS: usize = 4;
/// Requêtes estimées : sonde, puis chaque tâche.
const PROBE_REQUESTS: u64 = 3;
const TASK_REQUESTS: u64 = 12;

// Un booléen par option de la ligne de commande.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BenchOptions {
    /// Nombre de modèles compatibles qui passent les tâches.
    pub task_models: usize,
    /// Sonde seulement (`--rapide`).
    pub quick: bool,
    /// Inclure les modèles locaux (Ollama, LM Studio…).
    pub include_local: bool,
    /// Régler le meilleur comme modèle par défaut (`--appliquer`).
    pub apply: bool,
    /// Ne tester que les modèles dont l'identifiant contient ce texte.
    pub filter: Option<String>,
    /// Tâches de raisonnement à tests cachés (`--difficile`).
    pub hard: bool,
    /// Sans les aides de Claw, pour comparer (`--sans-aide`).
    pub unassisted: bool,
    /// `--max` a été donné (les tâches difficiles mesurent un modèle par défaut).
    pub max_explicit: bool,
}

impl Default for BenchOptions {
    fn default() -> Self {
        Self {
            task_models: DEFAULT_TASK_MODELS,
            quick: false,
            include_local: false,
            apply: false,
            filter: None,
            hard: false,
            unassisted: false,
            max_explicit: false,
        }
    }
}

pub const USAGE: &str =
    "Usage : /models bench [--rapide] [--local] [--max N] [--appliquer] [--difficile [--sans-aide]] [filtre]";

/// Options de `bench …` (le premier mot `bench` est facultatif).
pub fn parse_options(args: &str) -> Result<BenchOptions, String> {
    let mut options = BenchOptions::default();
    let mut words = args.split_whitespace().peekable();
    if words.peek() == Some(&"bench") {
        words.next();
    }
    while let Some(word) = words.next() {
        match word {
            "--rapide" | "--quick" => options.quick = true,
            "--local" | "--locaux" => options.include_local = true,
            "--appliquer" | "--apply" => options.apply = true,
            "--difficile" | "--raisonnement" | "--hard" => options.hard = true,
            "--sans-aide" | "--baseline" => {
                options.hard = true;
                options.unassisted = true;
            }
            "--max" => {
                options.max_explicit = true;
                options.task_models = words
                    .next()
                    .and_then(|value| value.parse().ok())
                    .filter(|count| *count > 0)
                    .ok_or_else(|| format!("--max attend un nombre\n{USAGE}"))?;
            }
            other if other.starts_with("--") => {
                return Err(format!("option inconnue {other}\n{USAGE}"));
            }
            other => options.filter = Some(other.to_ascii_lowercase()),
        }
    }
    Ok(options)
}

/// Résultat mesuré d'un modèle.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BenchResult {
    pub model: String,
    pub tested_at: u64,
    pub compatible: bool,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub probe_seconds: f64,
    #[serde(default)]
    pub passed: u32,
    #[serde(default)]
    pub total: u32,
    #[serde(default)]
    pub task_seconds: Vec<f64>,
    /// Échec de disponibilité (surcharge, limite du fournisseur) et non
    /// d'incompatibilité : le modèle n'est pas écarté du choix automatique.
    #[serde(default)]
    pub transient: bool,
    /// Tâches non mesurées faute de disponibilité du modèle.
    #[serde(default)]
    pub skipped: u32,
    /// Raisonnement : tâches à tests cachés, avec les aides de Claw.
    #[serde(default)]
    pub hard: Option<HardScore>,
    /// Les mêmes tâches sans les aides (`--sans-aide`).
    #[serde(default)]
    pub hard_unassisted: Option<HardScore>,
}

/// Score aux tâches de raisonnement (`/models bench --difficile`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HardScore {
    pub passed: u32,
    pub total: u32,
    #[serde(default)]
    pub seconds: Vec<f64>,
    /// Tâches ratées et leur cause.
    #[serde(default)]
    pub failed: Vec<String>,
    #[serde(default)]
    pub tested_at: u64,
}

impl BenchResult {
    /// Modèle mesuré seulement sur les tâches de raisonnement.
    #[must_use]
    pub fn measured_only(model: &str) -> Self {
        Self {
            model: model.to_string(),
            tested_at: super::models::unix_now(),
            compatible: true,
            reason: None,
            probe_seconds: 0.0,
            passed: 0,
            total: 0,
            task_seconds: Vec::new(),
            transient: false,
            skipped: 0,
            hard: None,
            hard_unassisted: None,
        }
    }

    /// Part des tâches réussies ; un modèle seulement sondé compte pour ½
    /// (compatible mais sans preuve), entre ceux qui réussissent et ceux qui échouent.
    #[must_use]
    pub fn success_ratio(&self) -> f64 {
        if !self.compatible {
            return if self.transient { -0.5 } else { -1.0 };
        }
        if self.total == 0 {
            0.5
        } else {
            f64::from(self.passed) / f64::from(self.total)
        }
    }

    #[must_use]
    pub fn median_seconds(&self) -> Option<f64> {
        let mut seconds = self.task_seconds.clone();
        if seconds.is_empty() {
            return None;
        }
        seconds.sort_by(f64::total_cmp);
        Some(seconds[seconds.len() / 2])
    }

    /// Résumé court pour `/models` : « 3/3 tâches · 40 s » ou la raison du refus.
    #[must_use]
    pub fn summary(&self) -> String {
        let reason = self.reason.as_deref().unwrap_or("incompatible");
        if !self.compatible {
            return if self.transient {
                format!("‖ indisponible pour l'instant ({reason})")
            } else {
                format!("✗ {reason}")
            };
        }
        let measured = match (self.total, self.median_seconds()) {
            (0, _) => "✓ compatible (sonde)".to_string(),
            (total, Some(median)) => format!("{}/{total} tâches · {median:.0} s", self.passed),
            (total, None) => format!("{}/{total} tâches", self.passed),
        };
        let measured = match &self.hard {
            Some(hard) => format!("{measured} · raisonnement {}/{}", hard.passed, hard.total),
            None => measured,
        };
        if self.skipped > 0 {
            format!("{measured} · {} non mesurée(s) : {reason}", self.skipped)
        } else {
            measured
        }
    }
}

/// Ordre de préférence : réussite, puis rapidité.
fn compare_results(a: &BenchResult, b: &BenchResult) -> std::cmp::Ordering {
    b.success_ratio()
        .total_cmp(&a.success_ratio())
        .then_with(|| {
            a.median_seconds()
                .unwrap_or(f64::MAX)
                .total_cmp(&b.median_seconds().unwrap_or(f64::MAX))
        })
        .then_with(|| a.probe_seconds.total_cmp(&b.probe_seconds))
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct BenchStore {
    results: Vec<BenchResult>,
}

fn store_path() -> Option<PathBuf> {
    let cwd = std::env::current_dir().ok()?;
    Some(
        runtime::ConfigLoader::default_for(cwd)
            .config_home()
            .join("model-bench.json"),
    )
}

/// Mesures récentes, par modèle.
#[must_use]
pub fn recent_results() -> BTreeMap<String, BenchResult> {
    // Les tests ne dépendent pas des mesures faites sur la machine.
    if cfg!(test) {
        return BTreeMap::new();
    }
    let Some(store) = store_path()
        .and_then(|path| fs::read_to_string(path).ok())
        .and_then(|text| serde_json::from_str::<BenchStore>(&text).ok())
    else {
        return BTreeMap::new();
    };
    let now = super::models::unix_now();
    store
        .results
        .into_iter()
        .filter(|result| now.saturating_sub(result.tested_at) < RESULTS_TTL_SECS)
        .map(|result| (result.model.clone(), result))
        .collect()
}

pub(crate) fn save_results(new_results: &[BenchResult]) {
    let Some(path) = store_path() else {
        return;
    };
    let mut merged = recent_results();
    for result in new_results {
        merged.insert(result.model.clone(), result.clone());
    }
    let store = BenchStore {
        results: merged.into_values().collect(),
    };
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(text) = serde_json::to_string_pretty(&store) {
        let _ = fs::write(path, text);
    }
}

/// Du plus efficace (le plus puissant parmi ceux qui marchent avec Claw) au
/// moins efficace ; les incompatibles en dernier, puis réussite et rapidité.
fn rank_by_efficiency(results: &mut [BenchResult]) {
    let catalog = super::models::catalog(false)
        .ok()
        .map(|(catalog, _)| catalog);
    let score = |result: &BenchResult| {
        super::models::efficiency(&result.model, catalog.as_ref(), Some(result))
            .map_or(f64::MIN, |(score, _)| score)
    };
    results.sort_by(|a, b| {
        score(b)
            .total_cmp(&score(a))
            .then_with(|| compare_results(a, b))
    });
}

/// `org/modele:free` : un seul « : », celui de `:free` (pas `groq:…`).
fn is_openrouter_free(model: &str) -> bool {
    model.contains('/') && model.ends_with(":free") && model.matches(':').count() == 1
}

// --- Exécution de claw dans un projet temporaire ---------------------------------

#[derive(Debug, Default)]
pub(crate) struct RunOutcome {
    pub result: Option<String>,
    pub error: Option<String>,
    pub tools: Vec<String>,
    pub seconds: f64,
    pub timed_out: bool,
}

/// Analyse la sortie `stream-json` de `claw prompt`.
fn parse_stream(lines: &[String]) -> (Option<String>, Option<String>, Vec<String>) {
    let mut result = None;
    let mut error = None;
    let mut tools = Vec::new();
    for line in lines {
        let Ok(event) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        match event.get("type").and_then(Value::as_str) {
            Some("assistant") => {
                if let Some(blocks) = event.pointer("/message/content").and_then(Value::as_array) {
                    tools.extend(
                        blocks
                            .iter()
                            .filter(|block| {
                                block.get("type").and_then(Value::as_str) == Some("tool_use")
                            })
                            .filter_map(|block| block.get("name").and_then(Value::as_str))
                            .map(ToOwned::to_owned),
                    );
                }
            }
            Some("result") => {
                if event.get("subtype").and_then(Value::as_str) == Some("error") {
                    error = event
                        .get("error")
                        .and_then(Value::as_str)
                        .map(ToOwned::to_owned);
                } else {
                    result = Some(
                        event
                            .get("result")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string(),
                    );
                }
            }
            _ => {}
        }
    }
    (result, error, tools)
}

pub(crate) fn run_claw(
    project: &Path,
    model: &str,
    mode: &str,
    prompt: &str,
    timeout: Duration,
    envs: &[(&str, &str)],
) -> RunOutcome {
    let started = Instant::now();
    let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("claw"));
    let spawned = Command::new(exe)
        .args([
            "--model",
            model,
            "--permission-mode",
            mode,
            "--output-format",
            "stream-json",
            "prompt",
            prompt,
        ])
        .current_dir(project)
        // Mesurer ce modèle, pas ses remplaçants.
        .env("CLAW_FALLBACK", "0")
        .envs(envs.iter().copied())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn();
    let mut child = match spawned {
        Ok(child) => child,
        Err(error) => {
            return RunOutcome {
                error: Some(format!("lancement de claw impossible : {error}")),
                ..RunOutcome::default()
            }
        }
    };
    let stdout = child.stdout.take();
    let reader = std::thread::spawn(move || {
        stdout.map_or_else(Vec::new, |stdout| {
            BufReader::new(stdout)
                .lines()
                .map_while(Result::ok)
                .collect::<Vec<_>>()
        })
    });
    let mut timed_out = false;
    loop {
        match child.try_wait() {
            Ok(Some(_)) | Err(_) => break,
            Ok(None) if started.elapsed() > timeout => {
                timed_out = true;
                let _ = child.kill();
                let _ = child.wait();
                break;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(200)),
        }
    }
    let lines = reader.join().unwrap_or_default();
    let (result, error, tools) = parse_stream(&lines);
    RunOutcome {
        result,
        error,
        tools,
        seconds: started.elapsed().as_secs_f64(),
        timed_out,
    }
}

/// Cause d'un échec : lisible, et passagère (disponibilité) ou durable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Failure {
    pub reason: String,
    pub transient: bool,
}

impl Failure {
    fn lasting(reason: impl Into<String>) -> Self {
        Self {
            reason: reason.into(),
            transient: false,
        }
    }

    fn passing(reason: impl Into<String>) -> Self {
        Self {
            reason: reason.into(),
            transient: true,
        }
    }
}

/// Raison d'un échec à partir du message d'erreur de claw.
pub(crate) fn classify_error(error: &str) -> Failure {
    let lower = error.to_ascii_lowercase();
    if lower.contains("only available on")
        || lower.contains(" 403 ")
        || lower.contains("403 forbidden")
    {
        Failure::lasting("refusé à Claw (réservé à certaines applications)")
    } else if lower.contains("per-day") || lower.contains("per day") {
        Failure::passing("quota du jour épuisé")
    } else if lower.contains("429")
        || lower.contains("rate limit")
        || lower.contains("rate-limited")
    {
        Failure::passing("limité par le fournisseur (429)")
    } else if lower.contains("overloaded")
        || lower.contains(" 503")
        || lower.contains(" 502")
        || lower.contains(" 500")
    {
        Failure::passing("surchargé")
    } else if lower.contains(" 404") || lower.contains("no endpoints") {
        Failure::lasting("indisponible (404)")
    } else if lower.contains("error decoding response body")
        || lower.contains("unexpected eof")
        || lower.contains("incomplete message")
    {
        Failure::passing("connexion coupée pendant la réponse")
    } else if lower.contains("connect") || lower.contains("error sending request") {
        Failure::passing("serveur injoignable")
    } else {
        Failure::lasting(error.chars().take(90).collect::<String>())
    }
}

pub(crate) struct Project {
    pub root: PathBuf,
}

impl Project {
    pub(crate) fn create(label: &str, files: &[(&str, &str)]) -> Result<Self, String> {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_nanos());
        let root = std::env::temp_dir().join(format!("claw-bench-{label}-{nanos}"));
        fs::create_dir_all(root.join(".claw")).map_err(|error| error.to_string())?;
        for (name, content) in files {
            fs::write(root.join(name), content).map_err(|error| error.to_string())?;
        }
        // Comme dans un vrai projet : l'agent lance les tests sans demande.
        let allow: Vec<String> = ["Bash", "PowerShell"]
            .iter()
            .flat_map(|tool| {
                ["python", "python3", "py"].map(|program| format!("{tool}({program}:*)"))
            })
            .collect();
        fs::write(
            root.join(".claw").join("settings.json"),
            serde_json::json!({ "permissions": { "allow": allow } }).to_string(),
        )
        .map_err(|error| error.to_string())?;
        Ok(Self { root })
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        if std::env::var_os("CLAW_BENCH_KEEP").is_none() {
            let _ = fs::remove_dir_all(&self.root);
        }
    }
}

// --- Sonde et tâches --------------------------------------------------------------

/// Résultat de la sonde : compatible, ou la cause de l'échec.
fn probe(model: &str, timeout: Duration) -> (Result<(), Failure>, f64) {
    let secret = format!(
        "CLAW-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(4242, |elapsed| elapsed.subsec_nanos() % 900_000 + 100_000)
    );
    let project = match Project::create(
        "sonde",
        &[("sonde.txt", &format!("Code secret : {secret}\n"))],
    ) {
        Ok(project) => project,
        Err(error) => return (Err(Failure::passing(error)), 0.0),
    };
    let outcome = run_claw(
        &project.root,
        model,
        "read-only",
        "Lis le fichier sonde.txt avec l'outil read_file, puis réponds uniquement avec le code secret qu'il contient.",
        timeout,
        &[],
    );
    let verdict = if outcome.timed_out {
        Err(Failure::lasting(format!(
            "trop lent (> {} s)",
            timeout.as_secs()
        )))
    } else if let Some(error) = &outcome.error {
        Err(classify_error(error))
    } else if outcome.tools.is_empty() {
        Err(Failure::lasting("n'appelle pas les outils"))
    } else if outcome
        .result
        .as_deref()
        .is_some_and(|result| result.contains(&secret))
    {
        Ok(())
    } else {
        Err(Failure::lasting("réponse incorrecte après l'appel d'outil"))
    };
    (verdict, outcome.seconds)
}

struct Task {
    id: &'static str,
    prompt: &'static str,
    files: &'static [(&'static str, &'static str)],
    check: fn(&Path, &Path) -> Result<(), String>,
}

fn unittest(root: &Path, python: &Path) -> Result<(), String> {
    let output = Command::new(python)
        .args(["-m", "unittest", "discover", "-s", ".", "-p", "test_*.py"])
        .current_dir(root)
        .stdin(Stdio::null())
        .output()
        .map_err(|error| format!("python : {error}"))?;
    if output.status.success() {
        Ok(())
    } else {
        let text = String::from_utf8_lossy(&output.stderr);
        let last = text
            .lines()
            .rev()
            .find(|line| !line.trim().is_empty())
            .unwrap_or_default()
            .trim();
        Err(format!("tests en échec ({last})"))
    }
}

fn check_rename(root: &Path, python: &Path) -> Result<(), String> {
    unittest(root, python)?;
    let sources: String = ["panier.py", "facture.py"]
        .iter()
        .filter_map(|name| fs::read_to_string(root.join(name)).ok())
        .collect();
    if sources.contains("calc(") {
        return Err("l'ancien nom est encore utilisé".to_string());
    }
    Ok(())
}

const TASKS: &[Task] = &[
    Task {
        id: "corriger un bug",
        prompt: "Les tests échouent. Trouve et corrige le bug dans stats.py, sans modifier les tests.",
        files: &[
            ("stats.py", "def moyenne(valeurs):\n    \"\"\"Moyenne des valeurs ; 0 pour une liste vide.\"\"\"\n    total = 0\n    for i in range(len(valeurs) - 1):\n        total += valeurs[i]\n    return total / len(valeurs)\n"),
            ("test_stats.py", "import unittest\nfrom stats import moyenne\n\n\nclass Tests(unittest.TestCase):\n    def test_moyenne(self):\n        self.assertEqual(moyenne([2, 4, 6]), 4)\n\n    def test_vide(self):\n        self.assertEqual(moyenne([]), 0)\n\n\nif __name__ == '__main__':\n    unittest.main()\n"),
        ],
        check: unittest,
    },
    Task {
        id: "implémenter une fonction",
        prompt: "Implémente la fonction slugify de texte.py selon sa docstring, pour que les tests passent.",
        files: &[
            ("texte.py", "def slugify(texte):\n    \"\"\"Convertit un titre en identifiant d'URL : minuscules, accents retirés,\n    tout ce qui n'est pas lettre ou chiffre remplacé par un seul tiret, sans\n    tiret au début ni à la fin. Ex. « Élan, été ! » -> « elan-ete ».\"\"\"\n    raise NotImplementedError\n"),
            ("test_texte.py", "import unittest\nfrom texte import slugify\n\n\nclass Tests(unittest.TestCase):\n    def test_simple(self):\n        self.assertEqual(slugify('Bonjour le monde'), 'bonjour-le-monde')\n\n    def test_accents_ponctuation(self):\n        self.assertEqual(slugify('  Élan, été !  '), 'elan-ete')\n\n    def test_chiffres(self):\n        self.assertEqual(slugify('Version 2.0 -- finale'), 'version-2-0-finale')\n\n\nif __name__ == '__main__':\n    unittest.main()\n"),
        ],
        check: unittest,
    },
    Task {
        id: "renommer dans plusieurs fichiers",
        prompt: "Renomme la fonction calc en calculer_total partout dans le projet (définition et appels) ; les tests doivent passer.",
        files: &[
            ("panier.py", "def calc(prix, quantites):\n    return sum(p * q for p, q in zip(prix, quantites))\n"),
            ("facture.py", "from panier import calc\n\n\ndef montant_ttc(prix, quantites, tva=0.2):\n    return round(calc(prix, quantites) * (1 + tva), 2)\n"),
            ("test_facture.py", "import unittest\nfrom facture import montant_ttc\nfrom panier import calculer_total\n\n\nclass Tests(unittest.TestCase):\n    def test_total(self):\n        self.assertEqual(calculer_total([2, 3], [1, 2]), 8)\n\n    def test_ttc(self):\n        self.assertEqual(montant_ttc([10], [1]), 12.0)\n\n\nif __name__ == '__main__':\n    unittest.main()\n"),
        ],
        check: check_rename,
    },
];

/// Issue d'une tâche : réussie, ratée par le modèle, ou non mesurée (le
/// modèle n'a pas pu travailler : surcharge, limite du fournisseur).
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum TaskOutcome {
    Passed(f64),
    Failed(String),
    Unavailable(String),
}

fn run_task(task: &Task, model: &str, python: &Path, timeout: Duration) -> TaskOutcome {
    let project = match Project::create("tache", task.files) {
        Ok(project) => project,
        Err(error) => return TaskOutcome::Unavailable(error),
    };
    let outcome = run_claw(
        &project.root,
        model,
        "workspace-write",
        task.prompt,
        timeout,
        &[],
    );
    // Une erreur en cours de route n'empêche pas de vérifier le travail fait.
    match (task.check)(&project.root, python) {
        Ok(()) => TaskOutcome::Passed(outcome.seconds),
        Err(detail) => {
            if outcome.timed_out {
                return TaskOutcome::Failed(format!("délai de {} s dépassé", timeout.as_secs()));
            }
            match outcome.error.as_deref().map(classify_error) {
                Some(failure) if failure.transient => TaskOutcome::Unavailable(failure.reason),
                Some(failure) => TaskOutcome::Failed(failure.reason),
                None => TaskOutcome::Failed(detail),
            }
        }
    }
}

// --- Candidats et rapport -----------------------------------------------------------

pub(crate) fn is_local_model(model: &str) -> bool {
    api::compat_providers()
        .iter()
        .any(|provider| provider.local && model.starts_with(&format!("{}:", provider.name)))
}

pub(crate) fn candidates(options: &BenchOptions) -> Result<Vec<String>, String> {
    let mut models = Vec::new();
    let has_key = std::env::var("OPENROUTER_API_KEY").is_ok_and(|key| !key.trim().is_empty());
    if has_key {
        let (catalog, _) = super::models::catalog(false)?;
        // Les mieux classés d'abord, puis les autres modèles gratuits avec outils.
        let mut ids: Vec<String> = api::rank_free_coding_models(&catalog)
            .into_iter()
            .map(|entry| entry.model.id)
            .collect();
        let mut others: Vec<String> = catalog
            .models
            .iter()
            .filter(|model| model.is_free() && model.supports_tools && !ids.contains(&model.id))
            .map(|model| model.id.clone())
            .collect();
        others.sort();
        ids.extend(others);
        models.extend(ids);
    }
    if options.include_local {
        models.extend(super::models::local_models());
    }
    if let Some(filter) = &options.filter {
        // Un identifiant complet désigne ce modèle et lui seul (« Mesurer ce modèle »).
        let exact = models
            .iter()
            .any(|model| model.eq_ignore_ascii_case(filter));
        models.retain(|model| {
            if exact {
                model.eq_ignore_ascii_case(filter)
            } else {
                model.to_ascii_lowercase().contains(filter)
            }
        });
    }
    if models.is_empty() {
        return Err(if let Some(filter) = &options.filter {
            format!("aucun modèle gratuit ne correspond à « {filter} » (voir /models)")
        } else if has_key || options.include_local {
            "aucun modèle à tester".to_string()
        } else {
            "aucune clé OpenRouter : définissez OPENROUTER_API_KEY, ou ajoutez --local pour les modèles locaux".to_string()
        });
    }
    Ok(models)
}

pub(crate) fn find_python() -> Option<PathBuf> {
    ["python3", "python", "py"]
        .iter()
        .find_map(|name| runtime::find_executable(name))
}

/// Lance la recherche ; `progress` reçoit l'avancement ligne par ligne.
#[allow(clippy::too_many_lines)]
pub fn run(args: &str, progress: &mut dyn FnMut(&str)) -> Result<String, String> {
    let options = parse_options(args)?;
    if options.hard {
        return crate::bench_hard::run(&options, progress);
    }
    let models = candidates(&options)?;
    let python = find_python();
    let run_tasks = !options.quick && python.is_some();

    let online = models.iter().filter(|model| !is_local_model(model)).count();
    let estimate = online as u64 * PROBE_REQUESTS
        + if run_tasks {
            (options.task_models.min(online) * TASKS.len()) as u64 * TASK_REQUESTS
        } else {
            0
        };
    progress(&format!(
        "Recherche des modèles gratuits pour le code compatibles avec Claw : {} modèle(s) à sonder{}",
        models.len(),
        if run_tasks {
            format!(", puis {} tâches de code sur les {} meilleurs", TASKS.len(), options.task_models)
        } else {
            String::new()
        }
    ));
    if online > 0 {
        progress(&format!(
            "Environ {estimate} requêtes OpenRouter au plus (limite : 20 par minute, Claw attend si besoin)."
        ));
    }
    if !options.quick && python.is_none() {
        progress("Python est introuvable : seule la sonde de compatibilité est lancée (les tâches se vérifient avec Python).");
    }

    // 1. Sonde de compatibilité.
    let mut results: Vec<BenchResult> = Vec::new();
    for (index, model) in models.iter().enumerate() {
        let local = is_local_model(model);
        if !local && !api::model_available(model) {
            progress(&format!(
                "  [{}/{}] {model} : mis de côté récemment (refus ou quota), ignoré",
                index + 1,
                models.len()
            ));
            continue;
        }
        let timeout = Duration::from_secs(if local { 600 } else { 150 });
        let (verdict, seconds) = probe(model, timeout);
        let failure = verdict.err();
        progress(&format!(
            "  [{}/{}] {model} : {}",
            index + 1,
            models.len(),
            match &failure {
                None => format!("✓ compatible ({seconds:.0} s)"),
                Some(failure) if failure.transient => {
                    format!("‖ indisponible pour l'instant ({})", failure.reason)
                }
                Some(failure) => format!("✗ {}", failure.reason),
            }
        ));
        results.push(BenchResult {
            model: model.clone(),
            tested_at: super::models::unix_now(),
            compatible: failure.is_none(),
            transient: failure.as_ref().is_some_and(|failure| failure.transient),
            reason: failure.map(|failure| failure.reason),
            probe_seconds: seconds,
            passed: 0,
            total: 0,
            task_seconds: Vec::new(),
            skipped: 0,
            hard: None,
            hard_unassisted: None,
        });
    }

    // 2. Tâches de code sur les meilleurs modèles compatibles (dans l'ordre du
    // classement). Un modèle indisponible le temps des tâches cède sa place au
    // suivant, pour mesurer autant de modèles que demandé.
    if let (true, Some(python)) = (run_tasks, python.as_deref()) {
        let compatible: Vec<usize> = results
            .iter()
            .enumerate()
            .filter(|(_, result)| result.compatible)
            .map(|(index, _)| index)
            .collect();
        if !compatible.is_empty() {
            progress(&format!(
                "Tâches de code (jusqu'à {} modèle(s)) :",
                options.task_models.min(compatible.len())
            ));
        }
        let mut measured_models = 0;
        for index in compatible {
            if measured_models >= options.task_models {
                break;
            }
            let model = results[index].model.clone();
            let timeout = Duration::from_secs(if is_local_model(&model) { 1_500 } else { 600 });
            for (position, task) in TASKS.iter().enumerate() {
                let outcome = run_task(task, &model, python, timeout);
                let result = &mut results[index];
                let line = match &outcome {
                    TaskOutcome::Passed(seconds) => {
                        result.total += 1;
                        result.passed += 1;
                        result.task_seconds.push(*seconds);
                        format!("✓ {seconds:.0} s")
                    }
                    TaskOutcome::Failed(cause) => {
                        result.total += 1;
                        format!("✗ {cause}")
                    }
                    TaskOutcome::Unavailable(cause) => {
                        result.skipped = u32::try_from(TASKS.len() - position).unwrap_or(0);
                        result.reason = Some(cause.clone());
                        format!("‖ non mesurée ({cause}) : tâches suivantes passées")
                    }
                };
                progress(&format!("  {model} · {} : {line}", task.id));
                if matches!(outcome, TaskOutcome::Unavailable(_)) {
                    break;
                }
            }
            if results[index].total > 0 {
                measured_models += 1;
            }
        }
    }

    save_results(&results);
    let mut ranked = results.clone();
    rank_by_efficiency(&mut ranked);
    let mut report = render_report(&ranked);
    if options.apply {
        report.push('\n');
        report.push_str(&apply_best(&ranked)?);
    } else if let Some(best) = best_choice(&ranked) {
        let _ = write!(
            report,
            "\n\nMeilleur choix : {model}. Pour l'utiliser par défaut : /models bench --appliquer\n(ou \"model\": \"{model}\" dans ~/.claw/settings.json).",
            model = best.model
        );
    } else {
        report.push_str(
            "\n\nAucun modèle n'a réussi de tâche : la configuration actuelle est conservée.",
        );
    }
    Ok(report)
}

fn render_report(ranked: &[BenchResult]) -> String {
    let width = ranked
        .iter()
        .map(|result| result.model.len())
        .max()
        .unwrap_or(6)
        .max(6);
    let mut lines = vec![
        String::new(),
        "Résultats, du plus efficace au moins efficace (gardés 14 jours ; le choix automatique les suit)"
            .to_string(),
        format!("  #  {:<width$}  Résultat", "Modèle"),
    ];
    for (index, result) in ranked.iter().enumerate() {
        lines.push(format!(
            "  {:<2} {:<width$}  {}",
            index + 1,
            result.model,
            result.summary()
        ));
    }
    lines.join("\n")
}

/// Règle le meilleur modèle mesuré comme modèle par défaut (réglages utilisateur),
/// avec en secours les autres modèles gratuits (`free`) et les modèles locaux déjà prévus.
/// Premier modèle recommandable : compatible et, s'il a passé les tâches, au
/// moins une réussie.
fn best_choice(ranked: &[BenchResult]) -> Option<&BenchResult> {
    ranked
        .iter()
        .find(|result| result.compatible && (result.total == 0 || result.passed > 0))
}

fn apply_best(ranked: &[BenchResult]) -> Result<String, String> {
    let best =
        best_choice(ranked).ok_or_else(|| "aucun modèle compatible à appliquer".to_string())?;
    let cwd = std::env::current_dir().map_err(|error| error.to_string())?;
    let path = runtime::ConfigLoader::default_for(cwd)
        .config_home()
        .join("settings.json");
    let mut settings = fs::read_to_string(&path)
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .filter(Value::is_object)
        .unwrap_or_else(|| serde_json::json!({}));
    update_settings(&mut settings, &best.model);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    fs::write(
        &path,
        serde_json::to_string_pretty(&settings).map_err(|error| error.to_string())? + "\n",
    )
    .map_err(|error| error.to_string())?;
    Ok(format!(
        "Modèle par défaut : {} (dans {}).\nSecours : {}",
        best.model,
        path.display(),
        settings["fallbackModels"]
    ))
}

/// `model` = le meilleur ; `fallbackModels` = « free » puis les modèles non
/// `OpenRouter` déjà présents (locaux, autres fournisseurs).
fn update_settings(settings: &mut Value, best: &str) {
    let kept: Vec<Value> = settings
        .get("fallbackModels")
        .and_then(Value::as_array)
        .map(|models| {
            models
                .iter()
                .filter_map(Value::as_str)
                .filter(|model| *model != "free" && *model != best && !is_openrouter_free(model))
                .map(|model| Value::String(model.to_string()))
                .collect()
        })
        .unwrap_or_default();
    let mut fallbacks = vec![Value::String("free".to_string())];
    fallbacks.extend(kept);
    settings["model"] = Value::String(best.to_string());
    settings["fallbackModels"] = Value::Array(fallbacks);
}

#[cfg(test)]
mod tests {
    use super::{
        classify_error, compare_results, parse_options, parse_stream, update_settings,
        BenchOptions, BenchResult,
    };
    use serde_json::json;

    fn result(
        model: &str,
        compatible: bool,
        passed: u32,
        total: u32,
        seconds: &[f64],
    ) -> BenchResult {
        BenchResult {
            model: model.to_string(),
            tested_at: 0,
            compatible,
            reason: (!compatible).then(|| "refusé".to_string()),
            probe_seconds: 5.0,
            passed,
            total,
            task_seconds: seconds.to_vec(),
            transient: false,
            skipped: 0,
            hard: None,
            hard_unassisted: None,
        }
    }

    #[test]
    fn parses_bench_options() {
        assert_eq!(
            parse_options("bench").expect("défaut"),
            BenchOptions::default()
        );
        let options =
            parse_options("bench --rapide --local --max 2 --appliquer Cohere").expect("options");
        assert!(options.quick && options.include_local && options.apply);
        assert_eq!(options.task_models, 2);
        assert_eq!(options.filter.as_deref(), Some("cohere"));
        assert!(!options.hard && options.max_explicit);
        let hard = parse_options("bench --difficile cohere").expect("difficile");
        assert!(hard.hard && !hard.unassisted && !hard.max_explicit);
        let baseline = parse_options("bench --sans-aide").expect("sans aide");
        assert!(baseline.hard && baseline.unassisted);
        assert!(parse_options("bench --max zero").is_err());
        assert!(parse_options("bench --inconnue").is_err());
    }

    #[test]
    fn ranks_by_success_then_speed_and_puts_incompatible_last() {
        let mut results = [
            result("a/lent:free", true, 3, 3, &[90.0, 80.0, 100.0]),
            result("b/refuse:free", false, 0, 0, &[]),
            result("c/rapide:free", true, 3, 3, &[10.0, 20.0, 15.0]),
            result("d/sonde:free", true, 0, 0, &[]),
            result("e/moyen:free", true, 2, 3, &[30.0, 40.0]),
            BenchResult {
                transient: true,
                reason: Some("surchargé".to_string()),
                ..result("f/surcharge:free", false, 0, 0, &[])
            },
        ];
        results.sort_by(compare_results);
        let order: Vec<&str> = results.iter().map(|result| result.model.as_str()).collect();
        assert_eq!(
            order,
            [
                "c/rapide:free",
                "a/lent:free",
                "e/moyen:free",
                "d/sonde:free",
                "f/surcharge:free",
                "b/refuse:free"
            ]
        );
        assert_eq!(results[0].summary(), "3/3 tâches · 15 s");
        assert_eq!(
            results[4].summary(),
            "‖ indisponible pour l'instant (surchargé)"
        );
        assert_eq!(results[5].summary(), "✗ refusé");
    }

    #[test]
    fn reads_tools_results_and_errors_from_stream_json() {
        let lines = vec![
            json!({"type": "system", "subtype": "init"}).to_string(),
            json!({"type": "assistant", "message": {"content": [{"type": "tool_use", "name": "read_file"}, {"type": "text", "text": "…"}]}}).to_string(),
            json!({"type": "result", "subtype": "success", "result": "CLAW-123456"}).to_string(),
        ];
        let (result, error, tools) = parse_stream(&lines);
        assert_eq!(result.as_deref(), Some("CLAW-123456"));
        assert!(error.is_none());
        assert_eq!(tools, ["read_file"]);

        let failed = vec![json!({"type": "result", "subtype": "error", "error": "l'api a retourné 403 Forbidden : only available on agentic harnesses"}).to_string()];
        let (_, error, _) = parse_stream(&failed);
        let refused = classify_error(&error.expect("erreur"));
        assert_eq!(
            refused.reason,
            "refusé à Claw (réservé à certaines applications)"
        );
        assert!(!refused.transient, "un refus est durable");
        let overloaded = classify_error(
            "l'api a retourné 503 Service Unavailable : Service temporarily overloaded",
        );
        assert_eq!(overloaded.reason, "surchargé");
        assert!(overloaded.transient, "une surcharge est passagère");
        let limited = classify_error("l'api a échoué après 3 tentatives : 429 Too Many Requests");
        assert_eq!(limited.reason, "limité par le fournisseur (429)");
        assert!(limited.transient);
    }

    #[test]
    fn applying_keeps_local_fallbacks_and_other_settings() {
        let mut settings = json!({
            "language": "français",
            "model": "ancien/modele:free",
            "fallbackModels": ["free", "ollama:qwen3.5:4b", "x/autre:free"]
        });
        update_settings(&mut settings, "cohere/north-mini-code:free");
        assert_eq!(settings["model"], "cohere/north-mini-code:free");
        assert_eq!(
            settings["fallbackModels"],
            json!(["free", "ollama:qwen3.5:4b"])
        );
        assert_eq!(settings["language"], "français");
    }
}
