//! `claw models bench --design` : quel modèle gratuit fait les plus belles
//! pages ? Chaque candidat construit la même page à partir du même brief,
//! avec la méthode de design de Claw mais sans revue (on mesure le modèle,
//! pas le juge) ; Claw la capture au téléphone et à l'ordinateur, puis le
//! juge qui voit la note sur les sept critères de la revue visuelle. Les
//! notes sont gardées avec les autres mesures, et `--appliquer` confie les
//! tours de design au meilleur (`designModel`).

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::bench::{self, BenchOptions, BenchResult, Project};

/// Le brief commun : un vrai petit site, assez riche pour départager.
const DESIGN_BRIEF: &str = "Crée la page d'accueil d'« Atelier Sève », un fleuriste de quartier à Lyon (bouquets de saison, ateliers de composition le samedi matin, livraison à vélo dans le 4e arrondissement) : index.html, avec ses styles dans styles.css et son script dans script.js. Sections : accueil, trois bouquets de saison avec leur prix, les ateliers (dates, places, réservation), deux avis de clients, contact avec adresse, téléphone et horaires. La page doit être belle et fonctionner au téléphone comme à l'ordinateur ; elle s'ouvre directement depuis le fichier, sans serveur.";
/// Requêtes du tour de chaque modèle : de quoi construire et se relire.
const DESIGN_TURN_REQUESTS: &str = "60";
/// Requêtes estimées par modèle : le tour, puis la note du juge.
const DESIGN_REQUESTS: u64 = 61;
const DEFAULT_DESIGN_MODELS: usize = 3;

/// Note de design d'un modèle.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DesignScore {
    /// Moyenne des sept critères, de 1 à 10 ; 0 quand la page manque.
    pub mean: f32,
    #[serde(default)]
    pub scores: Vec<(String, u8)>,
    /// Défauts mesurés par l'audit de `Screenshot`.
    #[serde(default)]
    pub problems: usize,
    #[serde(default)]
    pub seconds: f64,
    /// Le modèle qui a jugé.
    #[serde(default)]
    pub judge: String,
    /// Ce qui a empêché la mesure, s'il y a lieu.
    #[serde(default)]
    pub failure: Option<String>,
    #[serde(default)]
    pub tested_at: u64,
    /// Où la page construite est gardée, pour la regarder.
    #[serde(default)]
    pub page: Option<String>,
}

/// La page du projet : `index.html`, sinon la première page HTML.
fn built_page(root: &Path) -> Option<PathBuf> {
    let index = root.join("index.html");
    if index.is_file() {
        return Some(index);
    }
    let mut pages: Vec<PathBuf> = fs::read_dir(root)
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("html"))
        })
        .collect();
    pages.sort();
    pages.into_iter().next()
}

/// Remet le dossier de travail en place, même en cas d'erreur.
struct WorkingDir(Option<PathBuf>);

impl Drop for WorkingDir {
    fn drop(&mut self) {
        if let Some(previous) = self.0.take() {
            let _ = std::env::set_current_dir(previous);
        }
    }
}

/// Captures pleine page au téléphone et à l'ordinateur, et défauts mesurés.
/// `Screenshot` n'ouvre que les fichiers du dossier de travail : Claw s'y
/// place le temps de la capture.
fn capture(
    root: &Path,
    page: &Path,
) -> Result<(runtime::web_design::Audit, Vec<api::ImageSource>), String> {
    let _restore = WorkingDir(std::env::current_dir().ok());
    std::env::set_current_dir(root).map_err(|error| error.to_string())?;
    let name = page.file_name().map_or_else(
        || "index.html".to_string(),
        |name| name.to_string_lossy().into_owned(),
    );
    let input = json!({ "path": name, "viewports": ["phone", "desktop"], "full_page": true });
    let output = tools::execute_tool("Screenshot", &input)?;
    let images = tools::take_screenshots();
    if images.is_empty() {
        return Err("aucune capture".to_string());
    }
    let sources = images
        .into_iter()
        .map(|image| api::ImageSource::base64(image.media_type, image.data))
        .collect();
    Ok((runtime::web_design::read_audit(&output), sources))
}

/// Garde la page construite (sans la configuration du projet) pour la
/// comparer à l'œil : `design-bench/<modèle>/`.
fn keep_page(root: &Path, model: &str) -> Option<PathBuf> {
    let cwd = std::env::current_dir().ok()?;
    let slug: String = model
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '-' })
        .collect();
    let target = runtime::ConfigLoader::default_for(cwd)
        .config_home()
        .join("design-bench")
        .join(slug);
    let _ = fs::remove_dir_all(&target);
    copy_dir(root, &target, 0).ok()?;
    Some(target)
}

fn copy_dir(from: &Path, to: &Path, depth: usize) -> std::io::Result<()> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)?.filter_map(Result::ok) {
        let path = entry.path();
        let name = entry.file_name();
        if name == ".claw" {
            continue;
        }
        if path.is_dir() {
            if depth < 3 {
                copy_dir(&path, &to.join(&name), depth + 1)?;
            }
        } else {
            fs::copy(&path, to.join(&name))?;
        }
    }
    Ok(())
}

fn failed(reason: impl Into<String>, seconds: f64) -> DesignScore {
    DesignScore {
        mean: 0.0,
        scores: Vec::new(),
        problems: 0,
        seconds,
        judge: String::new(),
        failure: Some(reason.into()),
        tested_at: crate::models::unix_now(),
        page: None,
    }
}

/// Pourquoi aucune page n'a été écrite : les outils appelés et le début de
/// la réponse du modèle, pour ne pas laisser l'utilisateur deviner.
fn no_page(answer: Option<&str>, tools: &[String]) -> String {
    let mut reason = format!("aucune page écrite ({} appel(s) d'outil", tools.len());
    let answer: String = answer
        .unwrap_or_default()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if answer.is_empty() {
        reason.push_str(", aucune réponse)");
    } else {
        let short: String = answer.chars().take(140).collect();
        let cut = if answer.chars().count() > 140 {
            "…"
        } else {
            ""
        };
        let _ = write!(reason, ") ; réponse : « {short}{cut} »");
    }
    reason
}

/// Un modèle construit la page du brief, puis le juge la note.
fn measure(model: &str, timeout: Duration, progress: &mut dyn FnMut(&str)) -> DesignScore {
    let project = match Project::create("design", &[]) {
        Ok(project) => project,
        Err(error) => return failed(error, 0.0),
    };
    let outcome = bench::run_claw(
        &project.root,
        model,
        "workspace-write",
        DESIGN_BRIEF,
        timeout,
        &[
            // Le travail du modèle seul : ni revue du plan, ni revue des
            // captures, ni autre modèle pour le design.
            ("CLAW_DESIGN_REVIEW", "0"),
            ("CLAW_DESIGN_MODEL", "off"),
            ("CLAW_PROJECT_MODE", "0"),
            ("CLAW_PHOTOS", "0"),
            ("CLAW_TURN_REQUESTS", DESIGN_TURN_REQUESTS),
        ],
    );
    let Some(page) = built_page(&project.root) else {
        let reason = if outcome.timed_out {
            format!("délai de {} s dépassé sans page", timeout.as_secs())
        } else if let Some(error) = outcome.error.as_deref() {
            bench::classify_error(error).reason
        } else {
            no_page(outcome.result.as_deref(), &outcome.tools)
        };
        return failed(reason, outcome.seconds);
    };
    progress(&format!(
        "  {model} : page construite en {:.0} s, captures et note du juge…",
        outcome.seconds
    ));
    let kept = keep_page(&project.root, model);
    let (audit, images) = match capture(&project.root, &page) {
        Ok(captured) => captured,
        Err(error) => return failed(format!("capture impossible : {error}"), outcome.seconds),
    };
    let bar = runtime::web_design::design_bar();
    let prompt =
        runtime::web_design::critique_prompt(DESIGN_BRIEF, images.len(), &audit, bar, None);
    let (judge, text) = match crate::image_reader::critique(
        model,
        runtime::web_design::CRITIC_SYSTEM,
        &prompt,
        &images,
    ) {
        Ok(verdict) => verdict,
        Err(error) => return failed(format!("juge indisponible : {error}"), outcome.seconds),
    };
    let scores = runtime::web_design::critique_scores(&text);
    let Some(mean) = runtime::web_design::average_score(&scores) else {
        return failed("le juge n'a pas donné de notes", outcome.seconds);
    };
    DesignScore {
        mean,
        scores,
        problems: audit.problems.len() + audit.console.len(),
        seconds: outcome.seconds,
        judge,
        failure: None,
        tested_at: crate::models::unix_now(),
        page: kept.map(|path| path.display().to_string()),
    }
}

/// Modèles à mesurer : ceux du filtre, sinon les mieux classés pour le code
/// qui marchent avec Claw (une mesure récente d'incompatibilité les écarte).
fn models_to_measure(options: &BenchOptions) -> Result<Vec<String>, String> {
    let measured = bench::recent_results();
    let mut models: Vec<String> = bench::candidates(options)?
        .into_iter()
        .filter(|model| {
            measured
                .get(model)
                .is_none_or(|result| result.compatible || result.transient)
        })
        .collect();
    models.truncate(if options.max_explicit {
        options.task_models
    } else {
        DEFAULT_DESIGN_MODELS
    });
    if models.is_empty() {
        return Err("aucun modèle compatible à mesurer (voir /models bench)".to_string());
    }
    Ok(models)
}

/// Une note à la française : « 7,1 ».
fn decimal(value: f32) -> String {
    format!("{value:.1}").replace('.', ",")
}

/// Une ligne de résultat : « 7,1/10 · identity 6, finish 6 · 2 défauts · 540 s ».
fn score_line(score: &DesignScore, bar: u8) -> String {
    if let Some(failure) = &score.failure {
        return format!("non noté : {failure}");
    }
    let under: Vec<String> = score
        .scores
        .iter()
        .filter(|(_, value)| *value < bar)
        .map(|(criterion, value)| format!("{criterion} {value}"))
        .collect();
    let mut line = format!("{}/10", decimal(score.mean));
    if under.is_empty() {
        line.push_str(" · tout à la barre");
    } else {
        let _ = write!(line, " · sous la barre : {}", under.join(", "));
    }
    let _ = write!(
        line,
        " · {} défaut(s) mesuré(s) · {:.0} s · jugé par {}",
        score.problems, score.seconds, score.judge
    );
    line
}

/// Les modèles notés, du plus beau au moins beau.
fn ranked(scores: &[(String, DesignScore)]) -> Vec<&(String, DesignScore)> {
    let mut ranked: Vec<&(String, DesignScore)> = scores
        .iter()
        .filter(|(_, score)| score.failure.is_none() && score.mean > 0.0)
        .collect();
    ranked.sort_by(|(_, a), (_, b)| {
        b.mean
            .total_cmp(&a.mean)
            .then_with(|| a.problems.cmp(&b.problems))
            .then_with(|| a.seconds.total_cmp(&b.seconds))
    });
    ranked
}

/// `designModel` = le meilleur, dans les réglages de l'utilisateur.
fn apply(best: &str) -> Result<String, String> {
    let cwd = std::env::current_dir().map_err(|error| error.to_string())?;
    let path = runtime::ConfigLoader::default_for(cwd)
        .config_home()
        .join("settings.json");
    let mut settings = fs::read_to_string(&path)
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({}));
    settings["designModel"] = Value::String(best.to_string());
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    fs::write(
        &path,
        serde_json::to_string_pretty(&settings).map_err(|error| error.to_string())? + "\n",
    )
    .map_err(|error| error.to_string())?;
    Ok(format!(
        "Tours de design confiés à {best} (designModel, dans {}).",
        path.display()
    ))
}

pub(crate) fn run(
    options: &BenchOptions,
    progress: &mut dyn FnMut(&str),
) -> Result<String, String> {
    if !tools::page_viewer_available() {
        return Err(
            "aucun navigateur (Chrome, Edge, Chromium) : les pages se notent sur leurs captures"
                .to_string(),
        );
    }
    let models = models_to_measure(options)?;
    progress(&format!(
        "Design : {} modèle(s) construisent la même page (un fleuriste à Lyon), notée par un modèle qui voit sur les sept critères de la revue visuelle.",
        models.len()
    ));
    progress(&format!(
        "Environ {DESIGN_REQUESTS} requêtes au plus par modèle ; 10 à 20 minutes chacun."
    ));
    let bar = runtime::web_design::design_bar();
    let mut stored = bench::recent_results();
    let mut scores = Vec::new();
    for (index, model) in models.iter().enumerate() {
        progress(&format!(
            "  [{}/{}] {model} construit la page…",
            index + 1,
            models.len()
        ));
        let timeout = Duration::from_secs(if bench::is_local_model(model) {
            2_400
        } else {
            1_500
        });
        let score = measure(model, timeout, progress);
        progress(&format!("  {model} : {}", score_line(&score, bar)));
        stored
            .entry(model.clone())
            .or_insert_with(|| BenchResult::measured_only(model))
            .design = Some(score.clone());
        scores.push((model.clone(), score));
    }
    let results: Vec<BenchResult> = stored.into_values().collect();
    bench::save_results(&results);

    let mut report = vec![String::new(), format!("Design (barre {bar})")];
    let ranked = ranked(&scores);
    for (model, score) in &ranked {
        report.push(format!("  {model}"));
        report.push(format!("    {}", score_line(score, bar)));
        if let Some(page) = &score.page {
            report.push(format!("    page : {page}"));
        }
    }
    for (model, score) in scores.iter().filter(|(_, score)| score.failure.is_some()) {
        report.push(format!("  {model}"));
        report.push(format!("    {}", score_line(score, bar)));
    }
    report.push(String::new());
    match ranked.first() {
        Some((best, score)) if options.apply => {
            report.push(apply(best)?);
            report.push(format!(
                "Note de la page : {}/10. Les autres tours gardent le modèle de la session.",
                decimal(score.mean)
            ));
        }
        Some((best, score)) => report.push(format!(
            "Le plus beau : {best} ({}/10). Pour lui confier les tours de design : /models bench --design --appliquer {best}",
            decimal(score.mean)
        )),
        None => report.push("Aucune page notée : rien à appliquer.".to_string()),
    }
    report.push("Une note varie d'un essai à l'autre (modèles gratuits, juge) : un écart de moins d'un point ne départage pas vraiment.".to_string());
    Ok(report.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::{built_page, ranked, score_line, DesignScore, DESIGN_BRIEF};

    fn score(mean: f32, problems: usize) -> DesignScore {
        DesignScore {
            mean,
            scores: vec![("identity".to_string(), 6), ("phone".to_string(), 8)],
            problems,
            seconds: 300.0,
            judge: "juge".to_string(),
            failure: None,
            tested_at: 0,
            page: None,
        }
    }

    #[test]
    fn the_most_beautiful_page_comes_first() {
        let mut broken = score(0.0, 0);
        broken.failure = Some("aucune page écrite".to_string());
        let scores = vec![
            ("a".to_string(), score(6.1, 0)),
            ("b".to_string(), score(7.3, 2)),
            ("c".to_string(), broken),
            ("d".to_string(), score(7.3, 0)),
        ];
        let order: Vec<&str> = ranked(&scores)
            .iter()
            .map(|(model, _)| model.as_str())
            .collect();
        assert_eq!(order, ["d", "b", "a"]);
        let line = score_line(&scores[1].1, 8);
        assert!(
            line.starts_with("7,3/10 · sous la barre : identity 6 · 2 défaut(s)"),
            "{line}"
        );
        assert_eq!(score_line(&scores[2].1, 8), "non noté : aucune page écrite");
        assert_eq!(
            super::no_page(
                Some("Quel style\npréférez-vous ?"),
                &["read_file".to_string()]
            ),
            "aucune page écrite (1 appel(s) d'outil) ; réponse : « Quel style préférez-vous ? »"
        );
        assert_eq!(
            super::no_page(None, &[]),
            "aucune page écrite (0 appel(s) d'outil, aucune réponse)"
        );
        // Le brief demande bien une interface : la méthode de design s'y joint.
        assert!(runtime::web_design::design_guide(DESIGN_BRIEF, None).is_some());
    }

    #[test]
    fn the_built_page_is_found() {
        let dir = std::env::temp_dir().join(format!("claw-design-page-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dossier");
        assert_eq!(built_page(&dir), None);
        std::fs::write(dir.join("accueil.html"), "<p>x</p>").expect("page");
        assert_eq!(built_page(&dir), Some(dir.join("accueil.html")));
        std::fs::write(dir.join("index.html"), "<p>x</p>").expect("page");
        assert_eq!(built_page(&dir), Some(dir.join("index.html")));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
