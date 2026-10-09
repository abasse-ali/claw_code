//! `claw models bench --difficile` : mesure le **raisonnement** d'un modèle
//! sur six tâches à tests cachés (le modèle ne voit que la consigne et les
//! tests visibles) : cause d'un bug dans un autre fichier, cas limites d'une
//! spécification, correction sans toucher aux tests, réglage à faire suivre
//! dans plusieurs fichiers, code correct mais trop lent, petit analyseur.
//!
//! `--sans-aide` relance les mêmes tâches sans les aides de Claw
//! (vérification imposée, diagnostics, carte du projet…) pour comparer.

use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::bench::{self, BenchOptions, BenchResult, HardScore, Project, RunOutcome, TaskOutcome};

/// Requêtes estimées par tâche difficile.
const HARD_TASK_REQUESTS: u64 = 14;
/// Au-delà, les tests cachés sont considérés comme trop lents.
const CHECK_TIMEOUT: Duration = Duration::from_secs(120);

pub(crate) struct HardTask {
    pub id: &'static str,
    pub prompt: &'static str,
    pub files: &'static [(&'static str, &'static str)],
    /// Tests ajoutés après le travail du modèle.
    pub hidden: &'static [(&'static str, &'static str)],
    /// Fichiers que le modèle ne doit pas modifier (les tests visibles).
    pub frozen: &'static [&'static str],
}

macro_rules! task_file {
    ($task:literal, $file:literal) => {
        (
            $file,
            include_str!(concat!("bench_tasks/", $task, "/", $file)),
        )
    };
}

pub(crate) const HARD_TASKS: &[HardTask] = &[
    HardTask {
        id: "cause dans un autre fichier",
        prompt: include_str!("bench_tasks/cause_ailleurs/consigne.txt"),
        files: &[
            task_file!("cause_ailleurs", "devise.py"),
            task_file!("cause_ailleurs", "rapport.py"),
            task_file!("cause_ailleurs", "test_rapport.py"),
        ],
        hidden: &[task_file!("cause_ailleurs", "test_cache_devise.py")],
        frozen: &["test_rapport.py"],
    },
    HardTask {
        id: "cas limites d'une spécification",
        prompt: include_str!("bench_tasks/cas_limites/consigne.txt"),
        files: &[
            task_file!("cas_limites", "intervalles.py"),
            task_file!("cas_limites", "test_intervalles.py"),
        ],
        hidden: &[task_file!("cas_limites", "test_cache_intervalles.py")],
        frozen: &["test_intervalles.py"],
    },
    HardTask {
        id: "corriger sans toucher aux tests",
        prompt: include_str!("bench_tasks/sans_tricher/consigne.txt"),
        files: &[
            task_file!("sans_tricher", "panier.py"),
            task_file!("sans_tricher", "test_panier.py"),
        ],
        hidden: &[task_file!("sans_tricher", "test_cache_panier.py")],
        frozen: &["test_panier.py"],
    },
    HardTask {
        id: "réglage à faire suivre dans quatre fichiers",
        prompt: include_str!("bench_tasks/plusieurs_fichiers/consigne.txt"),
        files: &[
            task_file!("plusieurs_fichiers", "tarifs.py"),
            task_file!("plusieurs_fichiers", "commande.py"),
            task_file!("plusieurs_fichiers", "facture.py"),
            task_file!("plusieurs_fichiers", "export.py"),
            task_file!("plusieurs_fichiers", "test_commande.py"),
        ],
        hidden: &[task_file!("plusieurs_fichiers", "test_cache_decimales.py")],
        frozen: &["test_commande.py"],
    },
    HardTask {
        id: "correct mais trop lent",
        prompt: include_str!("bench_tasks/trop_lent/consigne.txt"),
        files: &[
            task_file!("trop_lent", "listes.py"),
            task_file!("trop_lent", "test_listes.py"),
        ],
        hidden: &[task_file!("trop_lent", "test_cache_listes.py")],
        frozen: &["test_listes.py"],
    },
    HardTask {
        id: "priorités, parenthèses, moins unaire",
        prompt: include_str!("bench_tasks/calculatrice/consigne.txt"),
        files: &[
            task_file!("calculatrice", "lexeur.py"),
            task_file!("calculatrice", "calcul.py"),
            task_file!("calculatrice", "test_calcul.py"),
        ],
        hidden: &[task_file!("calculatrice", "test_cache_calcul.py")],
        frozen: &["test_calcul.py"],
    },
];

fn normalized(text: &str) -> String {
    text.replace("\r\n", "\n")
}

/// Tests (visibles et cachés) avec une limite de temps : un code trop lent
/// échoue au lieu de bloquer la mesure.
fn run_tests(root: &Path, python: &Path) -> Result<(), String> {
    let mut child = Command::new(python)
        .args(["-m", "unittest", "discover", "-s", ".", "-p", "test_*.py"])
        .current_dir(root)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("python : {error}"))?;
    let stderr = child.stderr.take();
    let reader = std::thread::spawn(move || {
        let mut text = String::new();
        if let Some(mut stderr) = stderr {
            let _ = std::io::Read::read_to_string(&mut stderr, &mut text);
        }
        text
    });
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if started.elapsed() > CHECK_TIMEOUT => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(100)),
            Err(error) => return Err(format!("python : {error}")),
        }
    };
    let text = reader.join().unwrap_or_default();
    match status {
        None => Err(format!(
            "tests trop lents (plus de {} s)",
            CHECK_TIMEOUT.as_secs()
        )),
        Some(status) if status.success() => Ok(()),
        Some(_) => {
            let last = text
                .lines()
                .rev()
                .find(|line| !line.trim().is_empty())
                .unwrap_or_default()
                .trim()
                .to_string();
            Err(format!("tests cachés en échec ({last})"))
        }
    }
}

/// Vérifie le travail : tests visibles intacts, puis tous les tests passent.
pub(crate) fn check(task: &HardTask, root: &Path, python: &Path) -> Result<(), String> {
    for name in task.frozen {
        let original = task
            .files
            .iter()
            .find(|(file, _)| file == name)
            .map(|(_, content)| normalized(content));
        let current = fs::read_to_string(root.join(name))
            .ok()
            .map(|text| normalized(&text));
        if original != current {
            return Err(format!("a modifié {name} (interdit)"));
        }
    }
    for (name, content) in task.hidden {
        fs::write(root.join(name), content).map_err(|error| error.to_string())?;
    }
    run_tests(root, python)
}

/// Attente avant de réessayer une tâche que le modèle n'a pas pu traiter.
const RETRY_PAUSE: Duration = Duration::from_secs(30);

fn run_hard_task(
    task: &HardTask,
    model: &str,
    python: &Path,
    timeout: Duration,
    assisted: bool,
) -> TaskOutcome {
    let project = match Project::create("raisonnement", task.files) {
        Ok(project) => project,
        Err(error) => return TaskOutcome::Unavailable(error),
    };
    // Sans aide : le modèle seul, comme avant les aides au raisonnement.
    let envs: &[(&str, &str)] = if assisted {
        &[]
    } else {
        &[("CLAW_HARNESS", "0")]
    };
    let outcome: RunOutcome = bench::run_claw(
        &project.root,
        model,
        "workspace-write",
        task.prompt.trim(),
        timeout,
        envs,
    );
    match check(task, &project.root, python) {
        Ok(()) => TaskOutcome::Passed(outcome.seconds),
        Err(detail) => {
            if outcome.timed_out {
                return TaskOutcome::Failed(format!("délai de {} s dépassé", timeout.as_secs()));
            }
            match outcome.error.as_deref().map(bench::classify_error) {
                Some(failure) if failure.transient => TaskOutcome::Unavailable(failure.reason),
                Some(failure) => TaskOutcome::Failed(failure.reason),
                None => TaskOutcome::Failed(detail),
            }
        }
    }
}

/// Une tâche que le modèle n'a pas pu traiter (surcharge, flux coupé) est
/// réessayée une fois avant d'être déclarée non mesurée.
fn run_with_retry(
    task: &HardTask,
    model: &str,
    python: &Path,
    timeout: Duration,
    assisted: bool,
    progress: &mut dyn FnMut(&str),
) -> TaskOutcome {
    let outcome = run_hard_task(task, model, python, timeout, assisted);
    let TaskOutcome::Unavailable(cause) = &outcome else {
        return outcome;
    };
    progress(&format!(
        "  {model} · {} : ‖ {cause}, nouvel essai dans {} s",
        task.id,
        RETRY_PAUSE.as_secs()
    ));
    std::thread::sleep(RETRY_PAUSE);
    run_hard_task(task, model, python, timeout, assisted)
}

fn median(seconds: &[f64]) -> Option<f64> {
    let mut sorted = seconds.to_vec();
    if sorted.is_empty() {
        return None;
    }
    sorted.sort_by(f64::total_cmp);
    Some(sorted[sorted.len() / 2])
}

/// « 4/6 · 75 s » pour un score.
pub(crate) fn score_text(score: &HardScore) -> String {
    match median(&score.seconds) {
        Some(median) => format!("{}/{} · {median:.0} s", score.passed, score.total),
        None => format!("{}/{}", score.passed, score.total),
    }
}

/// Modèles à mesurer : ceux du filtre, sinon le modèle utilisé par défaut.
fn models_to_measure(options: &BenchOptions) -> Result<Vec<String>, String> {
    if options.filter.is_none() && !options.include_local {
        return Ok(vec![crate::default_model()]);
    }
    let mut models = bench::candidates(options)?;
    models.truncate(if options.max_explicit {
        options.task_models
    } else {
        1
    });
    Ok(models)
}

pub(crate) fn run(
    options: &BenchOptions,
    progress: &mut dyn FnMut(&str),
) -> Result<String, String> {
    let python = bench::find_python().ok_or_else(|| {
        "Python est introuvable : les tâches de raisonnement se vérifient avec Python".to_string()
    })?;
    let models = models_to_measure(options)?;
    let assisted = !options.unassisted;
    progress(&format!(
        "Raisonnement : {} tâches à tests cachés sur {} modèle(s), {} les aides de Claw.",
        HARD_TASKS.len(),
        models.len(),
        if assisted { "avec" } else { "sans" }
    ));
    progress(&format!(
        "Environ {} requêtes au plus par modèle ; plusieurs minutes par tâche.",
        HARD_TASKS.len() as u64 * HARD_TASK_REQUESTS
    ));

    let mut stored = bench::recent_results();
    let mut report = vec![
        String::new(),
        "Raisonnement (tâches à tests cachés)".to_string(),
    ];
    for model in &models {
        let timeout = Duration::from_secs(if bench::is_local_model(model) {
            1_800
        } else {
            600
        });
        let mut score = HardScore {
            passed: 0,
            total: 0,
            seconds: Vec::new(),
            failed: Vec::new(),
            tested_at: crate::models::unix_now(),
        };
        let mut unavailable = None;
        for task in HARD_TASKS {
            let outcome = run_with_retry(task, model, &python, timeout, assisted, progress);
            let line = match &outcome {
                TaskOutcome::Passed(seconds) => {
                    score.total += 1;
                    score.passed += 1;
                    score.seconds.push(*seconds);
                    format!("✓ {seconds:.0} s")
                }
                TaskOutcome::Failed(cause) => {
                    score.total += 1;
                    score.failed.push(format!("{} : {cause}", task.id));
                    format!("✗ {cause}")
                }
                TaskOutcome::Unavailable(cause) => {
                    unavailable = Some(cause.clone());
                    format!("‖ non mesurée ({cause}) : tâches suivantes passées")
                }
            };
            progress(&format!("  {model} · {} : {line}", task.id));
            if unavailable.is_some() {
                break;
            }
        }

        let entry = stored
            .entry(model.clone())
            .or_insert_with(|| BenchResult::measured_only(model));
        if score.total > 0 {
            if assisted {
                entry.hard = Some(score.clone());
            } else {
                entry.hard_unassisted = Some(score.clone());
            }
        }
        report.push(format!("  {model}"));
        if let Some(assisted_score) = &entry.hard {
            report.push(format!(
                "    avec les aides   {}",
                score_text(assisted_score)
            ));
        }
        if let Some(plain_score) = &entry.hard_unassisted {
            report.push(format!("    sans aide        {}", score_text(plain_score)));
        }
        for failure in &score.failed {
            report.push(format!("    ✗ {failure}"));
        }
        if let Some(cause) = unavailable {
            report.push(format!(
                "    ‖ mesure interrompue : {cause} ({} tâche(s) faite(s))",
                score.total
            ));
        }
    }
    let results: Vec<BenchResult> = stored.into_values().collect();
    bench::save_results(&results);
    report.push(String::new());
    report.push(if assisted {
        "Pour comparer sans les aides : /models bench --difficile --sans-aide [modèle]".to_string()
    } else {
        "Pour mesurer avec les aides : /models bench --difficile [modèle]".to_string()
    });
    Ok(report.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::{check, score_text, HARD_TASKS};
    use crate::bench::HardScore;

    #[test]
    fn every_hard_task_has_visible_and_hidden_tests() {
        assert_eq!(HARD_TASKS.len(), 6);
        for task in HARD_TASKS {
            assert!(!task.prompt.trim().is_empty(), "{}", task.id);
            assert!(!task.hidden.is_empty(), "{}", task.id);
            for frozen in task.frozen {
                assert!(
                    task.files.iter().any(|(name, _)| name == frozen),
                    "{} : {frozen} absent des fichiers",
                    task.id
                );
            }
            for (name, _) in task.hidden {
                assert!(name.starts_with("test_cache_"), "{}", task.id);
                assert!(
                    !task.files.iter().any(|(visible, _)| visible == name),
                    "{} : {name} visible",
                    task.id
                );
            }
        }
    }

    #[test]
    fn a_modified_test_file_fails_the_check_before_python_runs() {
        let task = &HARD_TASKS[2];
        let root =
            std::env::temp_dir().join(format!("claw-bench-hard-test-{}", std::process::id()));
        std::fs::create_dir_all(&root).expect("dossier");
        for (name, content) in task.files {
            std::fs::write(root.join(name), content).expect("fichier");
        }
        std::fs::write(root.join("test_panier.py"), "# tests supprimés\n").expect("fichier");
        let error = check(task, &root, std::path::Path::new("python-introuvable"))
            .expect_err("test modifié");
        assert!(error.contains("a modifié test_panier.py"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn formats_a_score() {
        let score = HardScore {
            passed: 4,
            total: 6,
            seconds: vec![60.0, 80.0, 75.0, 90.0],
            failed: Vec::new(),
            tested_at: 0,
        };
        assert_eq!(score_text(&score), "4/6 · 80 s");
    }
}
