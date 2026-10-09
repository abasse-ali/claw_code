//! Ressemblance d'un modèle gratuit avec les modèles phares de Claude (le
//! dernier Opus et le dernier Fable du catalogue `OpenRouter`), critère par
//! critère, sur ce qui se vérifie : capacités publiées (outils, raisonnement,
//! images, contexte, réponse maximale), orientation code et agents de la
//! description, rang en programmation, spécialités, mesures de Claw.
//!
//! C'est une ressemblance de profil, pas une égalité d'intelligence : les
//! capacités réelles se vérifient avec `/models bench`.

use api::{ModelCatalog, OpenRouterModel};

/// Familles de référence, dans l'ordre d'affichage.
const FAMILIES: &[&str] = &["opus", "fable"];

/// Ce qu'une description met en avant, par thème.
const ORIENTATION: &[(&str, &[&str])] = &[
    (
        "code",
        &["coding", "code", "software engineering", "programming"],
    ),
    ("agents", &["agentic", "agent"]),
    (
        "outils",
        &["tool use", "tool-use", "tool calling", "function calling"],
    ),
    (
        "tâches longues",
        &["long-horizon", "long-running", "multi-step", "long code"],
    ),
    ("raisonnement", &["reasoning"]),
];

/// Contexte et réponse minimaux considérés (échelle logarithmique).
const MIN_TOKENS: f64 = 4_096.0;
/// Au-delà de ce rang en programmation, le critère ne rapporte plus rien.
const RANK_SPAN: f64 = 50.0;

/// Un critère de comparaison : points obtenus sur le maximum, et le détail.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Criterion {
    pub label: &'static str,
    pub earned: f64,
    pub max: f64,
    pub detail: String,
}

/// Ressemblance d'un modèle avec une référence.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Likeness {
    pub reference: String,
    pub reference_name: String,
    pub percent: u32,
    pub criteria: Vec<Criterion>,
}

/// « Claude Opus 5.5 » à partir de « Anthropic: Claude Opus 5.5 ».
pub(crate) fn display_name(model: &OpenRouterModel) -> String {
    model
        .name
        .split_once(": ")
        .map_or_else(|| model.name.clone(), |(_, name)| name.to_string())
}

/// Version d'un identifiant `anthropic/claude-<famille>-5.5` : (5, 5).
fn version(id: &str, family: &str) -> Option<(u32, u32)> {
    let rest = id.strip_prefix(&format!("anthropic/claude-{family}-"))?;
    if rest.contains(':') {
        return None;
    }
    let mut parts = rest.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next().map_or(Some(0), |minor| minor.parse().ok())?;
    Some((major, minor))
}

/// Dernier Opus et dernier Fable du catalogue (ceux qui y figurent).
pub(crate) fn claude_references(catalog: &ModelCatalog) -> Vec<&OpenRouterModel> {
    FAMILIES
        .iter()
        .filter_map(|family| {
            catalog
                .models
                .iter()
                .filter_map(|model| version(&model.id, family).map(|version| (version, model)))
                .max_by_key(|(version, _)| *version)
                .map(|(_, model)| model)
        })
        .collect()
}

fn log_closeness(value: Option<u64>, reference: Option<u64>) -> f64 {
    #[allow(clippy::cast_precision_loss)]
    let scale = |tokens: u64| (tokens as f64 / MIN_TOKENS).max(1.0).log2();
    match (value, reference) {
        (Some(value), Some(reference)) if reference > 0 => {
            (scale(value) / scale(reference).max(1.0)).clamp(0.0, 1.0)
        }
        _ => 0.0,
    }
}

fn group(tokens: Option<u64>) -> String {
    let Some(tokens) = tokens else {
        return "inconnu".to_string();
    };
    let digits = tokens.to_string();
    let mut grouped = String::new();
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            grouped.push(' ');
        }
        grouped.push(digit);
    }
    grouped
}

fn themes(description: &str) -> Vec<&'static str> {
    let lower = description.to_ascii_lowercase();
    ORIENTATION
        .iter()
        .filter(|(_, words)| words.iter().any(|word| lower.contains(word)))
        .map(|(theme, _)| *theme)
        .collect()
}

fn yes_no(present: bool, earned_if: f64) -> (f64, String) {
    if present {
        (earned_if, "oui".to_string())
    } else {
        (0.0, "non".to_string())
    }
}

/// Mesure de Claw : tâches de raisonnement si elles ont été faites, sinon les
/// trois tâches de code. `None` s'il n'y a pas de mesure.
fn measured_ratio(result: Option<&crate::bench::BenchResult>) -> Option<(f64, String)> {
    let result = result?;
    if let Some(hard) = result.hard.as_ref().filter(|hard| hard.total > 0) {
        return Some((
            f64::from(hard.passed) / f64::from(hard.total),
            format!("raisonnement {}/{}", hard.passed, hard.total),
        ));
    }
    (result.total > 0).then(|| {
        (
            f64::from(result.passed) / f64::from(result.total),
            format!("{}/{} tâches de code", result.passed, result.total),
        )
    })
}

fn criterion(label: &'static str, earned: f64, max: f64, detail: String) -> Criterion {
    Criterion {
        label,
        earned,
        max,
        detail,
    }
}

/// Capacités publiées : outils, raisonnement, images, contexte, réponse.
fn capability_criteria(model: &OpenRouterModel, reference: &OpenRouterModel) -> Vec<Criterion> {
    let mut criteria = Vec::new();
    let (earned, detail) = yes_no(model.supports_tools, 15.0);
    criteria.push(criterion("Appels d'outils", earned, 15.0, detail));
    if reference.supports_reasoning {
        let (earned, detail) = yes_no(model.supports_reasoning, 10.0);
        criteria.push(criterion("Raisonnement", earned, 10.0, detail));
    }
    if reference.supports_images {
        let (earned, detail) = yes_no(model.supports_images, 10.0);
        criteria.push(criterion("Lecture d'images", earned, 10.0, detail));
    }
    criteria.push(criterion(
        "Contexte",
        10.0 * log_closeness(model.context_length, reference.context_length),
        10.0,
        format!(
            "{} jetons (Claude : {})",
            group(model.context_length),
            group(reference.context_length)
        ),
    ));
    criteria.push(criterion(
        "Réponse maximale",
        5.0 * log_closeness(model.max_completion_tokens, reference.max_completion_tokens),
        5.0,
        format!(
            "{} jetons (Claude : {})",
            group(model.max_completion_tokens),
            group(reference.max_completion_tokens)
        ),
    ));
    criteria
}

/// Thèmes de la description de Claude que celle du modèle met aussi en avant.
fn orientation_criterion(
    model: &OpenRouterModel,
    reference: &OpenRouterModel,
) -> Option<Criterion> {
    let wanted = themes(&reference.description);
    if wanted.is_empty() {
        return None;
    }
    let found = themes(&model.description);
    let shared: Vec<&str> = wanted
        .iter()
        .copied()
        .filter(|theme| found.contains(theme))
        .collect();
    #[allow(clippy::cast_precision_loss)]
    let share = shared.len() as f64 / wanted.len() as f64;
    let detail = if shared.is_empty() {
        format!("aucun des thèmes de Claude ({})", wanted.join(", "))
    } else {
        format!(
            "{} sur {} : {}",
            shared.len(),
            wanted.len(),
            shared.join(", ")
        )
    };
    Some(criterion(
        "Orienté code et agents",
        15.0 * share,
        15.0,
        detail,
    ))
}

/// Rang dans la catégorie programmation d'`OpenRouter`, comparé à celui de Claude.
fn rank_criterion(
    model: &OpenRouterModel,
    reference: &OpenRouterModel,
    catalog: &ModelCatalog,
) -> Criterion {
    let reference_rank = api::category_rank(&reference.id, "programming", catalog);
    let rank = api::category_rank(&model.id, "programming", catalog);
    #[allow(clippy::cast_precision_loss)]
    let share = rank.map_or(0.0, |rank| {
        (1.0 - (rank as f64 - 1.0) / RANK_SPAN).clamp(0.0, 1.0)
    });
    let claude_rank =
        reference_rank.map_or_else(|| "non classé".to_string(), |rank| format!("n°{rank}"));
    let detail = match rank {
        Some(rank) => format!("n°{rank} chez OpenRouter (Claude : {claude_rank})"),
        None => format!("non classé (Claude : {claude_rank})"),
    };
    criterion("Rang en programmation", 15.0 * share, 15.0, detail)
}

/// Part des spécialités de Claude que le modèle a aussi.
fn specialties_criterion(
    model: &OpenRouterModel,
    reference: &OpenRouterModel,
    catalog: &ModelCatalog,
) -> Option<Criterion> {
    let wanted: Vec<&str> = api::model_specialties(reference, catalog)
        .into_iter()
        .map(|specialty| specialty.key)
        .collect();
    if wanted.is_empty() {
        return None;
    }
    let mine: Vec<&str> = api::model_specialties(model, catalog)
        .into_iter()
        .map(|specialty| specialty.key)
        .collect();
    let shared = wanted.iter().filter(|key| mine.contains(key)).count();
    #[allow(clippy::cast_precision_loss)]
    let share = shared as f64 / wanted.len() as f64;
    Some(criterion(
        "Spécialités en commun",
        10.0 * share,
        10.0,
        format!("{shared} sur {}", wanted.len()),
    ))
}

/// Ressemblance de `model` avec `reference`, critère par critère.
pub(crate) fn likeness(
    model: &OpenRouterModel,
    reference: &OpenRouterModel,
    catalog: &ModelCatalog,
    measured: Option<&crate::bench::BenchResult>,
) -> Likeness {
    let mut criteria = capability_criteria(model, reference);
    criteria.extend(orientation_criterion(model, reference));
    criteria.push(rank_criterion(model, reference, catalog));
    criteria.extend(specialties_criterion(model, reference, catalog));
    let (ratio, detail) = measured_ratio(measured)
        .unwrap_or_else(|| (0.0, "pas encore mesuré (« Mesurer ce modèle »)".to_string()));
    criteria.push(criterion("Mesuré par Claw", 10.0 * ratio, 10.0, detail));

    let earned: f64 = criteria.iter().map(|criterion| criterion.earned).sum();
    let max: f64 = criteria.iter().map(|criterion| criterion.max).sum();
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let percent = if max > 0.0 {
        (earned * 100.0 / max).round() as u32
    } else {
        0
    };
    Likeness {
        reference: reference.id.clone(),
        reference_name: display_name(reference),
        percent,
        criteria,
    }
}

impl Likeness {
    /// Critères pleinement remplis (au moins les trois quarts des points).
    pub(crate) fn matches(&self) -> Vec<&'static str> {
        self.criteria
            .iter()
            .filter(|criterion| criterion.earned >= criterion.max * 0.75)
            .map(|criterion| criterion.label)
            .collect()
    }

    /// Critères où le modèle est loin de Claude (au plus le quart des points).
    pub(crate) fn gaps(&self) -> Vec<&'static str> {
        self.criteria
            .iter()
            .filter(|criterion| criterion.earned <= criterion.max * 0.25)
            .map(|criterion| criterion.label)
            .collect()
    }

    pub(crate) fn json(&self, with_criteria: bool) -> serde_json::Value {
        let mut value = serde_json::json!({
            "reference": self.reference,
            "reference_name": self.reference_name,
            "percent": self.percent,
            "matches": self.matches(),
            "gaps": self.gaps(),
        });
        if with_criteria {
            value["criteria"] = serde_json::json!(self
                .criteria
                .iter()
                .map(|criterion| serde_json::json!({
                    "label": criterion.label,
                    "earned": (criterion.earned * 10.0).round() / 10.0,
                    "max": criterion.max,
                    "detail": criterion.detail,
                }))
                .collect::<Vec<_>>());
        }
        value
    }
}

#[cfg(test)]
mod tests {
    use super::{claude_references, display_name, likeness, version};
    use api::{ModelCatalog, OpenRouterModel};
    use std::collections::BTreeMap;

    fn model(
        id: &str,
        name: &str,
        description: &str,
        context: u64,
        images: bool,
    ) -> OpenRouterModel {
        OpenRouterModel {
            id: id.to_string(),
            name: name.to_string(),
            description: description.to_string(),
            context_length: Some(context),
            max_completion_tokens: Some(context / 8),
            supports_tools: true,
            supports_reasoning: true,
            supports_images: images,
            prompt_price: if id.ends_with(":free") {
                0.0
            } else {
                0.000_005
            },
            completion_price: if id.ends_with(":free") {
                0.0
            } else {
                0.000_025
            },
            ..Default::default()
        }
    }

    fn catalog() -> ModelCatalog {
        let flagship =
            "Flagship model for demanding reasoning, coding, and long-horizon agentic work.";
        ModelCatalog {
            models: vec![
                model(
                    "anthropic/claude-opus-4.8",
                    "Anthropic: Claude Opus 4.8",
                    flagship,
                    1_000_000,
                    true,
                ),
                model(
                    "anthropic/claude-opus-5.5",
                    "Anthropic: Claude Opus 5.5",
                    flagship,
                    1_000_000,
                    true,
                ),
                model(
                    "anthropic/claude-opus-5.5:batch",
                    "Anthropic: Claude Opus 5.5 (batch)",
                    flagship,
                    1_000_000,
                    true,
                ),
                model(
                    "anthropic/claude-fable-5",
                    "Anthropic: Claude Fable 5",
                    flagship,
                    1_000_000,
                    true,
                ),
                model(
                    "anthropic/claude-fable-5.1",
                    "Anthropic: Claude Fable 5.1",
                    flagship,
                    1_000_000,
                    true,
                ),
                model(
                    "lab/proche:free",
                    "Lab: Proche",
                    "An open reasoning model for agentic coding and tool use.",
                    1_000_000,
                    true,
                ),
                model(
                    "lab/loin:free",
                    "Lab: Loin",
                    "A compact chat assistant.",
                    32_768,
                    false,
                ),
            ],
            category_rankings: BTreeMap::from([(
                "programming".to_string(),
                vec![
                    "anthropic/claude-opus-5.5".to_string(),
                    "lab/proche".to_string(),
                ],
            )]),
            ..ModelCatalog::default()
        }
    }

    #[test]
    fn picks_the_latest_opus_and_fable() {
        assert_eq!(version("anthropic/claude-opus-5.5", "opus"), Some((5, 5)));
        assert_eq!(version("anthropic/claude-opus-5", "opus"), Some((5, 0)));
        assert_eq!(version("anthropic/claude-opus-5.5:batch", "opus"), None);
        let catalog = catalog();
        let references: Vec<&str> = claude_references(&catalog)
            .iter()
            .map(|model| model.id.as_str())
            .collect();
        assert_eq!(
            references,
            ["anthropic/claude-opus-5.5", "anthropic/claude-fable-5.1"]
        );
        assert_eq!(
            display_name(claude_references(&catalog)[0]),
            "Claude Opus 5.5"
        );
        assert!(claude_references(&ModelCatalog::default()).is_empty());
    }

    #[test]
    fn scores_the_profile_criterion_by_criterion() {
        let catalog = catalog();
        let opus = claude_references(&catalog)[0];
        let near = catalog
            .models
            .iter()
            .find(|model| model.id == "lab/proche:free")
            .expect("proche");
        let far = catalog
            .models
            .iter()
            .find(|model| model.id == "lab/loin:free")
            .expect("loin");

        let close = likeness(near, opus, &catalog, None);
        let distant = likeness(far, opus, &catalog, None);
        assert!(
            close.percent > distant.percent,
            "{} <= {}",
            close.percent,
            distant.percent
        );
        assert!(close.percent < 100, "sans mesure, jamais 100 %");
        assert_eq!(close.reference_name, "Claude Opus 5.5");
        assert!(close.matches().contains(&"Lecture d'images"));
        assert!(close.matches().contains(&"Contexte"));
        let rank = close
            .criteria
            .iter()
            .find(|criterion| criterion.label == "Rang en programmation")
            .expect("rang");
        assert_eq!(rank.detail, "n°2 chez OpenRouter (Claude : n°1)");
        let orientation = close
            .criteria
            .iter()
            .find(|criterion| criterion.label == "Orienté code et agents")
            .expect("orientation");
        // « tâches longues » manque à sa description.
        assert_eq!(orientation.detail, "3 sur 4 : code, agents, raisonnement");
        assert!(distant.gaps().contains(&"Lecture d'images"));
        assert!(distant.gaps().contains(&"Rang en programmation"));

        // Une mesure de Claw réussie rapproche le modèle.
        let mut result = crate::bench::BenchResult::measured_only("lab/proche:free");
        result.passed = 3;
        result.total = 3;
        let measured = likeness(near, opus, &catalog, Some(&result));
        assert_eq!(measured.percent, close.percent + 10);
        let json = measured.json(true);
        assert_eq!(json["criteria"].as_array().map(Vec::len), Some(9));
    }
}
