//! `/models` : modèles gratuits d'`OpenRouter` les mieux placés en code, quota
//! gratuit du jour, et choix du modèle par défaut quand seule une clé
//! `OpenRouter` est disponible.

use std::env;
use std::fs;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::Duration;

use api::{ModelCatalog, RankedModel};
use runtime::{ConfigLoader, TokenUsage};

const CACHE_TTL_SECS: u64 = 24 * 3600;
const NETWORK_TIMEOUT: Duration = Duration::from_secs(15);
/// Routeur d'`OpenRouter` vers un modèle gratuit, utilisé si le catalogue est inaccessible.
pub const FREE_ROUTER_MODEL: &str = "openrouter/free";

fn cache_path() -> Option<PathBuf> {
    let cwd = env::current_dir().ok()?;
    Some(
        ConfigLoader::default_for(cwd)
            .config_home()
            .join("cache")
            .join("openrouter-models.json"),
    )
}

fn load_cached_catalog() -> Option<ModelCatalog> {
    let contents = fs::read_to_string(cache_path()?).ok()?;
    serde_json::from_str(&contents).ok()
}

fn save_catalog(catalog: &ModelCatalog) {
    let Some(path) = cache_path() else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(contents) = serde_json::to_string(catalog) {
        let _ = fs::write(path, contents);
    }
}

pub(crate) fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

fn block_on<F: std::future::Future>(future: F) -> Result<F::Output, String> {
    let runtime = tokio::runtime::Runtime::new().map_err(|error| error.to_string())?;
    runtime
        .block_on(async { tokio::time::timeout(NETWORK_TIMEOUT, future).await })
        .map_err(|_| {
            format!(
                "pas de réponse d'OpenRouter après {} s",
                NETWORK_TIMEOUT.as_secs()
            )
        })
}

/// Catalogue en cache s'il a moins de 24 h (sauf `refresh`), sinon téléchargé ;
/// en cas d'échec réseau, le cache périmé sert de secours.
pub(crate) fn catalog(refresh: bool) -> Result<(ModelCatalog, Option<String>), String> {
    let cached = load_cached_catalog();
    if !refresh {
        if let Some(catalog) = cached
            .as_ref()
            .filter(|catalog| catalog.version == api::CATALOG_VERSION)
            .filter(|catalog| unix_now().saturating_sub(catalog.fetched_at) < CACHE_TTL_SECS)
        {
            return Ok((catalog.clone(), None));
        }
    }
    let fetched = block_on(api::fetch_model_catalog(&api::read_openrouter_base_url()))
        .and_then(|result| result.map_err(|error| error.to_string()));
    match (fetched, cached) {
        (Ok(catalog), _) => {
            save_catalog(&catalog);
            api::register_model_limits(&catalog);
            Ok((catalog, None))
        }
        (Err(error), Some(stale)) => Ok((
            stale,
            Some(format!(
                "catalogue non actualisé ({error}), cache précédent utilisé"
            )),
        )),
        (Err(error), None) => Err(format!("catalogue OpenRouter inaccessible : {error}")),
    }
}

/// Enregistre une fois les limites des modèles du cache (pour `max_tokens`).
pub fn register_cached_limits() {
    static DONE: OnceLock<()> = OnceLock::new();
    DONE.get_or_init(|| {
        if let Some(catalog) = load_cached_catalog() {
            api::register_model_limits(&catalog);
        }
    });
}

/// Poids des mesures de `/models bench` : réussite aux tâches (×1 si toutes,
/// ×0,4 si aucune), compatible seulement sondé (×0,95), non mesuré (×0,9),
/// indisponible pour l'instant (×0,75) ; `None` si incompatible avec Claw.
pub(crate) fn measurement_factor(result: Option<&crate::bench::BenchResult>) -> Option<f64> {
    match result {
        None => Some(0.9),
        Some(result) if !result.compatible && !result.transient => None,
        Some(result) if !result.compatible => Some(0.75),
        Some(result) => {
            // Les tâches de raisonnement (tests cachés) comptent double.
            let (hard_passed, hard_total) = result
                .hard
                .as_ref()
                .map_or((0, 0), |hard| (hard.passed * 2, hard.total * 2));
            let (passed, total) = (result.passed + hard_passed, result.total + hard_total);
            if total == 0 {
                Some(0.95)
            } else {
                Some(0.4 + 0.6 * f64::from(passed) / f64::from(total))
            }
        }
    }
}

/// Efficacité d'un modèle pour le code : sa puissance (rang programmation,
/// taille, spécialisation) pondérée par ses mesures. Un modèle hors catalogue
/// (Ollama…) est jugé sur son nom.
pub(crate) fn efficiency(
    id: &str,
    catalog: Option<&ModelCatalog>,
    result: Option<&crate::bench::BenchResult>,
) -> Option<(f64, Vec<String>)> {
    let factor = measurement_factor(result)?;
    let fallback = ModelCatalog::default();
    let catalog = catalog.unwrap_or(&fallback);
    let model = catalog
        .models
        .iter()
        .find(|model| model.id == id)
        .cloned()
        .unwrap_or_else(|| api::OpenRouterModel {
            id: id.to_string(),
            name: id.to_string(),
            description: String::new(),
            context_length: None,
            max_completion_tokens: None,
            prompt_price: 0.0,
            completion_price: 0.0,
            supports_tools: true,
            supports_images: false,
            ..Default::default()
        });
    let (power, reasons) = api::model_power(&model, catalog);
    Some(((power.max(0.0) + 5.0) * factor, reasons))
}

/// Modèles gratuits du plus efficace au moins efficace : ceux retenus pour le
/// code par le catalogue et ceux mesurés par `/models bench`, sans les
/// incompatibles avec Claw.
fn preferred_free_models(catalog: &ModelCatalog) -> Vec<String> {
    scored_free_models(catalog, &crate::bench::recent_results())
        .into_iter()
        .map(|(_, id)| id)
        .collect()
}

/// Le même classement avec l'efficacité de chaque modèle.
fn scored_free_models(
    catalog: &ModelCatalog,
    measured: &std::collections::BTreeMap<String, crate::bench::BenchResult>,
) -> Vec<(f64, String)> {
    let retained: Vec<String> = api::rank_free_coding_models(catalog)
        .into_iter()
        .map(|entry| entry.model.id)
        .collect();
    let mut scored: Vec<(f64, String)> = catalog
        .models
        .iter()
        .filter(|model| {
            model.is_free() && model.supports_tools && !model.id.starts_with("openrouter/")
        })
        .filter(|model| retained.contains(&model.id) || measured.contains_key(&model.id))
        .filter_map(|model| {
            efficiency(&model.id, Some(catalog), measured.get(&model.id))
                .map(|(score, _)| (score, model.id.clone()))
        })
        .collect();
    scored.sort_by(|left, right| right.0.total_cmp(&left.0));
    scored
}

/// Modèle qui reprend la fin d'un tour quand l'agent n'avance plus
/// (`escalationModel`, `CLAW_ESCALATION_MODEL`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Escalation {
    /// Jamais : `"off"`, ou une session limitée à un seul modèle.
    Off,
    /// Le modèle gratuit le plus puissant du moment : sans réglage, ou `"free"`.
    StrongestFree,
    /// Le modèle que l'utilisateur a écrit lui-même.
    Model(String),
}

impl Escalation {
    /// `single_model` : la bascule entre modèles est coupée (`CLAW_FALLBACK=0`).
    fn parse(setting: Option<&str>, single_model: bool) -> Self {
        match setting.map(str::trim).filter(|value| !value.is_empty()) {
            None if single_model => Self::Off,
            None => Self::StrongestFree,
            Some(value) => match value.to_ascii_lowercase().as_str() {
                "off" | "none" | "non" | "aucun" | "false" | "0" => Self::Off,
                "free" | "gratuit" | "auto" => Self::StrongestFree,
                _ => Self::Model(api::resolve_model_alias(value)),
            },
        }
    }

    /// Réglage de la session : variable d'environnement, sinon configuration.
    #[must_use]
    pub fn configured() -> Self {
        let setting = env::var("CLAW_ESCALATION_MODEL")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .or_else(|| {
                let cwd = env::current_dir().ok()?;
                let config = ConfigLoader::default_for(&cwd).load().ok()?;
                config.escalation_model().map(ToOwned::to_owned)
            });
        let single_model = env::var("CLAW_FALLBACK").is_ok_and(|value| value.trim() == "0");
        Self::parse(setting.as_deref(), single_model)
    }

    /// Modèle qui prendrait le relais de `current` maintenant, s'il y en a un.
    #[must_use]
    pub fn target(&self, current: &str) -> Option<String> {
        match self {
            Self::Off => None,
            Self::StrongestFree => strongest_free_model(current),
            Self::Model(model) => Some(model.clone()).filter(|model| model != current),
        }
    }
}

/// Modèle gratuit le plus puissant, s'il l'est plus que `current` : lu dans le
/// catalogue en cache (aucune requête), parmi les modèles disponibles. Rien
/// sans clé `OpenRouter`, ni quand `current` est déjà le plus puissant.
fn strongest_free_model(current: &str) -> Option<String> {
    let has_key = env::var("OPENROUTER_API_KEY").is_ok_and(|key| !key.trim().is_empty());
    if !has_key {
        return None;
    }
    let catalog = load_cached_catalog()?;
    strongest_free_in(
        &catalog,
        &crate::bench::recent_results(),
        current,
        api::model_available,
    )
}

fn strongest_free_in(
    catalog: &ModelCatalog,
    measured: &std::collections::BTreeMap<String, crate::bench::BenchResult>,
    current: &str,
    available: impl Fn(&str) -> bool,
) -> Option<String> {
    let current_score =
        efficiency(current, Some(catalog), measured.get(current)).map_or(0.0, |(score, _)| score);
    scored_free_models(catalog, measured)
        .into_iter()
        .find(|(_, id)| id != current && available(id))
        .filter(|(score, _)| *score > current_score)
        .map(|(_, id)| id)
}

/// Ligne « Escalade » de `/models` : ce qui se passe quand l'agent n'avance plus.
fn escalation_line(current_model: &str) -> String {
    if !runtime::harness::enabled(runtime::harness::Aid::Escalation) {
        return "désactivée".to_string();
    }
    let escalation = Escalation::configured();
    match (&escalation, escalation.target(current_model)) {
        (Escalation::Off, _) => "désactivée".to_string(),
        (Escalation::StrongestFree, Some(model)) => {
            format!("{model} (le gratuit le plus puissant) reprend un tour bloqué")
        }
        (Escalation::StrongestFree, None) => {
            "aucune : le modèle actuel est déjà le gratuit le plus puissant disponible".to_string()
        }
        (Escalation::Model(_), Some(model)) => {
            format!("{model} (escalationModel) reprend un tour bloqué")
        }
        (Escalation::Model(_), None) => "aucune : c'est déjà le modèle de la session".to_string(),
    }
}

/// Modèles gratuits qui lisent les images, du plus puissant au moins
/// puissant, ceux qui sont disponibles d'abord : lus dans le catalogue en
/// cache (aucune requête). `current` et les routeurs d'`OpenRouter` sont écartés.
pub(crate) fn free_image_readers(current: &str) -> Vec<String> {
    load_cached_catalog().map_or_else(Vec::new, |catalog| {
        image_readers_in(&catalog, current, api::model_available)
    })
}

fn image_readers_in(
    catalog: &ModelCatalog,
    current: &str,
    available: impl Fn(&str) -> bool,
) -> Vec<String> {
    let mut readers: Vec<(bool, f64, String)> = catalog
        .models
        .iter()
        .filter(|model| {
            model.is_free()
                && model.supports_images
                && model.id != current
                && !model.id.starts_with("openrouter/")
        })
        .map(|model| {
            (
                available(&model.id),
                api::model_power(model, catalog).0,
                model.id.clone(),
            )
        })
        .collect();
    readers.sort_by(|left, right| {
        right
            .0
            .cmp(&left.0)
            .then(right.1.total_cmp(&left.1))
            .then(left.2.cmp(&right.2))
    });
    readers.into_iter().map(|(_, _, id)| id).collect()
}

/// `"model": "free"` : le modèle gratuit le plus puissant du moment.
#[must_use]
pub fn is_strongest_free_keyword(model: &str) -> bool {
    matches!(
        model.trim().to_ascii_lowercase().as_str(),
        "free" | "gratuit"
    )
}

/// Meilleur modèle gratuit pour le code (télécharge le catalogue si besoin),
/// en évitant ceux mis de côté (refusés à Claw, quota épuisé…).
pub fn best_free_coding_model() -> Option<String> {
    let (catalog, _) = catalog(false).ok()?;
    let preferred = preferred_free_models(&catalog);
    preferred
        .iter()
        .find(|id| api::model_available(id))
        .or_else(|| preferred.first())
        .cloned()
}

fn format_context(tokens: Option<u64>) -> String {
    match tokens {
        Some(tokens) if tokens >= 1_000_000 => {
            let millions = tokens / 100_000;
            if millions % 10 == 0 {
                format!("{}M", millions / 10)
            } else {
                format!("{}.{}M", millions / 10, millions % 10)
            }
        }
        Some(tokens) => format!("{}k", tokens / 1_024),
        None => "?".to_string(),
    }
}

fn format_age(seconds: u64) -> String {
    match seconds {
        0..=119 => "à l'instant".to_string(),
        120..=7_199 => format!("il y a {} min", seconds / 60),
        _ => format!("il y a {} h", seconds / 3_600),
    }
}

/// Meilleurs modèles gratuits du cache (sans réseau), pour la bascule automatique.
pub fn ranked_free_coding_models(limit: usize) -> Vec<String> {
    load_cached_catalog()
        .map(|catalog| {
            preferred_free_models(&catalog)
                .into_iter()
                .filter(|id| api::model_available(id))
                .take(limit)
                .collect()
        })
        .unwrap_or_default()
}

/// Enregistre les serveurs compatibles déclarés dans la config (`providers`).
pub fn register_configured_providers(config: &runtime::RuntimeConfig) {
    api::register_compat_providers(
        config
            .providers()
            .iter()
            .map(|provider| api::CompatProvider {
                name: provider.name.clone(),
                base_url: provider.base_url.clone(),
                api_key_env: provider.api_key_env.clone(),
                requires_key: provider.api_key_env.is_some() && !provider.local,
                local: provider.local,
                context_window: provider.context_window,
                max_output_tokens: provider.max_output_tokens,
                headers: provider
                    .headers
                    .iter()
                    .map(|(name, value)| (name.clone(), value.clone()))
                    .collect(),
                text_tools: provider.text_tools,
            })
            .collect(),
    );
}

/// Chaîne de modèles d'une session : le principal, puis `CLAW_FALLBACK_MODELS`
/// (liste séparée par des virgules) ou `fallbackModels` de la config. Sans liste
/// et pour un modèle gratuit d'`OpenRouter`, les meilleurs modèles gratuits du
/// classement servent de secours. `CLAW_FALLBACK=0` désactive la bascule.
pub fn model_chain(primary: &str, configured: &[String]) -> Vec<String> {
    let mut chain = vec![primary.to_string()];
    if env::var("CLAW_FALLBACK").is_ok_and(|value| value.trim() == "0") {
        return chain;
    }
    let from_env = env::var("CLAW_FALLBACK_MODELS")
        .ok()
        .map(|value| {
            value
                .split(',')
                .map(str::trim)
                .filter(|model| !model.is_empty())
                .map(ToOwned::to_owned)
                .collect::<Vec<_>>()
        })
        .filter(|models| !models.is_empty());
    let explicit = from_env.unwrap_or_else(|| configured.to_vec());
    if explicit.is_empty() {
        if primary.contains('/') && primary.ends_with(":free") {
            chain.extend(ranked_free_coding_models(4));
        }
    } else {
        // « free » : les meilleurs modèles gratuits d'OpenRouter pour le code, à
        // cette place de la liste (ex. ["free", "ollama:qwen2.5-coder:7b"]).
        for model in explicit {
            if model.eq_ignore_ascii_case("free") {
                chain.extend(ranked_free_coding_models(4));
            } else {
                chain.push(model);
            }
        }
    }
    let mut unique: Vec<String> = Vec::new();
    for model in chain {
        let model = api::resolve_model_alias(&model);
        if !unique.contains(&model) {
            unique.push(model);
        }
    }
    unique
}

/// Modèles proposés par un fournisseur compatible (`GET /models`).
fn provider_models(
    provider: &api::CompatProvider,
    timeout: Duration,
) -> Result<Vec<String>, String> {
    let client = api::OpenAiCompatClient::for_compat_provider(provider, "")
        .map_err(|error| error.to_string())?;
    let runtime = tokio::runtime::Runtime::new().map_err(|error| error.to_string())?;
    runtime
        .block_on(async { tokio::time::timeout(timeout, client.list_models()).await })
        .map_err(|_| "pas de réponse".to_string())?
        .map_err(|error| {
            let text = error.to_string();
            if text.contains("connect") || text.contains("error sending request") {
                "serveur arrêté".to_string()
            } else {
                text.chars().take(80).collect()
            }
        })
}

/// Premier modèle d'un serveur local en marche (Ollama, LM Studio, llama.cpp…),
/// pour démarrer sans aucune clé.
pub fn first_local_model() -> Option<String> {
    // Les tests ne doivent pas dépendre d'un Ollama lancé sur la machine.
    if cfg!(test) {
        return None;
    }
    api::compat_providers()
        .into_iter()
        .filter(|provider| provider.local)
        .find_map(|provider| {
            provider_models(&provider, Duration::from_millis(700))
                .ok()?
                .into_iter()
                .find(|model| !model.contains("embed"))
                .map(|model| format!("{}:{model}", provider.name))
        })
}

/// Modèles consultés pour la relecture, le diagnostic et le second avis :
/// ceux du réglage `advisorModels`, sinon les meilleurs modèles gratuits autres
/// que `current` (il faut alors une clé `OpenRouter`). Gardé deux minutes.
pub(crate) fn advisor_models(current: &str) -> Vec<String> {
    static CACHE: std::sync::Mutex<Option<(std::time::Instant, String, Vec<String>)>> =
        std::sync::Mutex::new(None);
    // Les tests ne consultent personne.
    if cfg!(test) {
        return Vec::new();
    }
    let mut cache = CACHE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some((at, model, models)) = cache.as_ref() {
        if model == current && at.elapsed() < Duration::from_secs(120) {
            return models.clone();
        }
    }
    let configured: Vec<String> = std::env::current_dir()
        .ok()
        .and_then(|cwd| runtime::ConfigLoader::default_for(&cwd).load().ok())
        .map(|config| {
            config
                .advisor_models()
                .iter()
                .map(|model| api::resolve_model_alias(model))
                .collect()
        })
        .unwrap_or_default();
    let has_key = std::env::var("OPENROUTER_API_KEY").is_ok_and(|key| !key.trim().is_empty());
    let models = if !configured.is_empty() {
        configured
    } else if has_key {
        catalog(false)
            .map(|(catalog, _)| {
                preferred_free_models(&catalog)
                    .into_iter()
                    .filter(|model| model != current)
                    .take(4)
                    .collect()
            })
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    *cache = Some((
        std::time::Instant::now(),
        current.to_string(),
        models.clone(),
    ));
    models
}

/// Serveurs locaux connus, leur adresse et leurs modèles (`claw doctor`).
pub(crate) fn local_servers() -> Vec<(String, String, Result<Vec<String>, String>)> {
    api::compat_providers()
        .into_iter()
        .filter(|provider| provider.local)
        .map(|provider| {
            let models = provider_models(&provider, Duration::from_millis(1500)).map(|models| {
                models
                    .into_iter()
                    .filter(|model| !model.contains("embed"))
                    .collect()
            });
            (provider.name.clone(), provider.base_url.clone(), models)
        })
        .collect()
}

/// Modèles des serveurs locaux en marche (`ollama:qwen3.5:4b`…), pour `/models bench --local`.
pub(crate) fn local_models() -> Vec<String> {
    api::compat_providers()
        .into_iter()
        .filter(|provider| provider.local)
        .flat_map(|provider| {
            provider_models(&provider, Duration::from_secs(2))
                .unwrap_or_default()
                .into_iter()
                .filter(|model| !model.contains("embed"))
                .map(move |model| format!("{}:{model}", provider.name))
        })
        .collect()
}

/// `/models local` : fournisseurs compatibles et modèles disponibles.
pub fn providers_report(current_model: &str) -> String {
    let mut lines = vec![
        "Fournisseurs compatibles OpenAI (préfixe nom:modèle)".to_string(),
        "  Utilisation      /model ollama:qwen2.5-coder:7b · /model groq:llama-3.3-70b-versatile"
            .to_string(),
        String::new(),
    ];
    for provider in api::compat_providers() {
        let kind = if provider.local { "local" } else { "en ligne" };
        if !provider.is_configured() {
            lines.push(format!(
                "  {:<10} {kind:<8} clé absente ({})",
                provider.name,
                provider.api_key_env.as_deref().unwrap_or("?")
            ));
            continue;
        }
        let timeout = if provider.local {
            Duration::from_secs(2)
        } else {
            Duration::from_secs(8)
        };
        match provider_models(&provider, timeout) {
            Ok(models) if models.is_empty() => lines.push(format!(
                "  {:<10} {kind:<8} {} · aucun modèle installé",
                provider.name, provider.base_url
            )),
            Ok(models) => {
                lines.push(format!(
                    "  {:<10} {kind:<8} {} · {} modèle(s)",
                    provider.name,
                    provider.base_url,
                    models.len()
                ));
                for model in models.iter().take(12) {
                    let id = format!("{}:{model}", provider.name);
                    let marker = if id == current_model {
                        "  ● actuel"
                    } else {
                        ""
                    };
                    lines.push(format!("      {id}{marker}"));
                }
                if models.len() > 12 {
                    lines.push(format!("      … et {} autre(s)", models.len() - 12));
                }
            }
            Err(reason) => lines.push(format!(
                "  {:<10} {kind:<8} {} · {reason}",
                provider.name, provider.base_url
            )),
        }
    }
    lines.extend([
        String::new(),
        "Serveur personnel : \"providers\": {\"monserveur\": {\"baseUrl\": \"http://hote:8000/v1\", \"apiKeyEnv\": \"MA_CLE\"}}"
            .to_string(),
        "Secours automatique : \"fallbackModels\": [\"groq:llama-3.3-70b-versatile\", \"ollama:qwen2.5-coder:7b\"]"
            .to_string(),
        "Ollama : lancez-le avec OLLAMA_CONTEXT_LENGTH=32768 (le contexte par défaut est trop petit pour un agent)."
            .to_string(),
    ]);
    lines.join("\n")
}

/// Modèles gratuits utilisables par l'agent (appels d'outils, pas un routeur,
/// pas mesurés incompatibles).
fn usable_free_models<'a>(
    catalog: &'a ModelCatalog,
    measured: &std::collections::BTreeMap<String, crate::bench::BenchResult>,
) -> Vec<&'a api::OpenRouterModel> {
    catalog
        .models
        .iter()
        .filter(|model| {
            model.is_free() && model.supports_tools && !model.id.starts_with("openrouter/")
        })
        .filter(|model| measurement_factor(measured.get(&model.id)).is_some())
        .collect()
}

/// Modèles gratuits rangés du plus proche au moins proche de `reference`.
fn closest_to<'a>(
    reference: &api::OpenRouterModel,
    models: &[&'a api::OpenRouterModel],
    catalog: &ModelCatalog,
    measured: &std::collections::BTreeMap<String, crate::bench::BenchResult>,
) -> Vec<(&'a api::OpenRouterModel, crate::likeness::Likeness)> {
    let mut ranked: Vec<_> = models
        .iter()
        .map(|model| {
            let likeness =
                crate::likeness::likeness(model, reference, catalog, measured.get(&model.id));
            (*model, likeness)
        })
        .collect();
    ranked.sort_by(|left, right| {
        right
            .1
            .percent
            .cmp(&left.1.percent)
            .then_with(|| left.0.id.cmp(&right.0.id))
    });
    ranked
}

/// Modèle payant proche de Claude, ou mieux classé que lui en programmation.
/// Proposé à titre d'information : il n'est utilisé que si l'utilisateur le
/// choisit lui-même.
struct PaidCandidate<'a> {
    model: &'a api::OpenRouterModel,
    likeness: crate::likeness::Likeness,
    rank: Option<usize>,
    reference_rank: Option<usize>,
}

impl PaidCandidate<'_> {
    /// Écarts tirés du catalogue publié : un payant n'est jamais mesuré par
    /// Claw (la mesure coûterait de l'argent), ce critère n'est donc pas un
    /// écart utile à afficher.
    fn published_gaps(&self) -> Vec<&'static str> {
        let mut gaps = self.likeness.gaps();
        gaps.retain(|label| *label != crate::likeness::MEASURED);
        gaps
    }

    /// Mieux classé que Claude dans la catégorie programmation d'`OpenRouter`
    /// (classement par usage, pas une mesure d'intelligence).
    fn better_ranked(&self) -> bool {
        matches!((self.rank, self.reference_rank), (Some(rank), Some(reference)) if rank < reference)
    }

    fn price(&self) -> String {
        let per_million = |price: f64| format!("{:.2} $", price * 1_000_000.0).replace('.', ",");
        format!(
            "{} / {} par million",
            per_million(self.model.prompt_price),
            per_million(self.model.completion_price)
        )
    }

    fn json(&self) -> serde_json::Value {
        let likeness = self.likeness.json(false);
        serde_json::json!({
            "id": self.model.id,
            "name": self.model.name,
            "context": format_context(self.model.context_length),
            "percent": self.likeness.percent,
            "matches": likeness["matches"],
            "gaps": self.published_gaps(),
            "rank": self.rank,
            "reference_rank": self.reference_rank,
            "better_ranked": self.better_ranked(),
            "prompt_price": price_per_million(self.model.prompt_price),
            "completion_price": price_per_million(self.model.completion_price),
        })
    }
}

/// Payants listés au plus par référence.
const PAID_CLOSE_LIMIT: usize = 8;
/// Ressemblance minimale d'un payant, à défaut d'être au moins aussi proche
/// que le meilleur gratuit.
const PAID_CLOSE_FLOOR: u32 = 60;

/// Modèles payants (appels d'outils, prix connu) au moins aussi proches de
/// `reference` que le meilleur gratuit, ou mieux classés qu'elle en
/// programmation ; sans les autres versions de sa propre famille.
fn paid_close_to<'a>(
    reference: &api::OpenRouterModel,
    catalog: &'a ModelCatalog,
    measured: &std::collections::BTreeMap<String, crate::bench::BenchResult>,
    best_free: u32,
) -> Vec<PaidCandidate<'a>> {
    let floor = best_free.max(PAID_CLOSE_FLOOR);
    let reference_rank = api::category_rank(&reference.id, "programming", catalog);
    let mut candidates: Vec<PaidCandidate<'a>> = catalog
        .models
        .iter()
        .filter(|model| {
            !model.is_free()
                && model.supports_tools
                && model.prompt_price >= 0.0
                && model.completion_price >= 0.0
                && !model.id.contains(':')
                && !model.id.starts_with("openrouter/")
                && model.id != reference.id
                && !crate::likeness::same_family(&model.id, &reference.id)
        })
        .map(|model| PaidCandidate {
            model,
            likeness: crate::likeness::likeness(model, reference, catalog, measured.get(&model.id)),
            rank: api::category_rank(&model.id, "programming", catalog),
            reference_rank,
        })
        .filter(|candidate| candidate.better_ranked() || candidate.likeness.percent >= floor)
        .collect();
    candidates.sort_by(|left, right| {
        right
            .likeness
            .percent
            .cmp(&left.likeness.percent)
            .then_with(|| {
                left.rank
                    .unwrap_or(usize::MAX)
                    .cmp(&right.rank.unwrap_or(usize::MAX))
            })
            .then_with(|| left.model.id.cmp(&right.model.id))
    });
    candidates.truncate(PAID_CLOSE_LIMIT);
    candidates
}

/// `/models claude` : modèles gratuits les plus proches du dernier Opus et du
/// dernier Fable, avec ce qui les rapproche et ce qui les en éloigne.
fn claude_likeness_report() -> Result<String, String> {
    let (catalog, warning) = catalog(false)?;
    let measured = crate::bench::recent_results();
    let references = crate::likeness::claude_references(&catalog);
    if references.is_empty() {
        return Ok(
            "Aucun modèle Opus ou Fable dans le catalogue OpenRouter : /models refresh le relit."
                .to_string(),
        );
    }
    let models = usable_free_models(&catalog, &measured);
    let mut lines = vec!["Modèles gratuits les plus proches de Claude".to_string()];
    if let Some(warning) = warning {
        lines.push(format!("  ▲ {warning}"));
    }
    let mut first_choice = None;
    for reference in &references {
        lines.push(String::new());
        lines.push(format!(
            "Proches de {} ({})",
            crate::likeness::display_name(reference),
            reference.id
        ));
        let ranked = closest_to(reference, &models, &catalog, &measured);
        let best_free = ranked.first().map_or(0, |(_, likeness)| likeness.percent);
        for (model, likeness) in ranked.into_iter().take(6) {
            first_choice.get_or_insert_with(|| model.id.clone());
            lines.push(format!("  {:>3} %  {}", likeness.percent, model.id));
            let matches = likeness.matches();
            let gaps = likeness.gaps();
            if !matches.is_empty() {
                lines.push(format!("         ✓ {}", matches.join(", ").to_lowercase()));
            }
            if !gaps.is_empty() {
                lines.push(format!("         ✗ {}", gaps.join(", ").to_lowercase()));
            }
        }
        let paid = paid_close_to(reference, &catalog, &measured, best_free);
        if !paid.is_empty() {
            lines.push("  Payants proches ou mieux classés (seulement si vous les choisissez : /model <id>)".to_string());
            for candidate in paid.iter().take(5) {
                let gaps = candidate.published_gaps();
                let detail = if candidate.better_ranked() {
                    format!(
                        "mieux classé en programmation (n°{} contre n°{})",
                        candidate.rank.unwrap_or_default(),
                        candidate.reference_rank.unwrap_or_default()
                    )
                } else if gaps.is_empty() {
                    "✓ aucun écart net".to_string()
                } else {
                    format!("✗ {}", gaps.join(", ").to_lowercase())
                };
                lines.push(format!(
                    "  {:>3} %  {}  · payant",
                    candidate.likeness.percent, candidate.model.id
                ));
                lines.push(format!("         {} · {detail}", candidate.price()));
            }
        }
    }
    lines.extend([
        String::new(),
        "Ressemblance de profil sur neuf critères vérifiables (outils, raisonnement, images,"
            .to_string(),
        "contexte, réponse maximale, orientation code et agents, rang en programmation,"
            .to_string(),
        "spécialités, mesures de Claw), pas une égalité d'intelligence : les capacités réelles"
            .to_string(),
        "se vérifient avec /models bench <modèle>.".to_string(),
    ]);
    if let Some(choice) = first_choice {
        lines.push(String::new());
        lines.push("Suite".to_string());
        lines.push(format!("  /model {choice}"));
        lines.push(
            "  Un tour bloqué est repris par le gratuit le plus puissant (voir /models, ligne Escalade)."
                .to_string(),
        );
    }
    Ok(lines.join("\n"))
}

/// Explication du classement et suite possible, au bas de `/models`.
fn models_report_footer() -> Vec<String> {
    vec![
        String::new(),
        "Classement par efficacité, le plus puissant en tête : rang dans la catégorie programmation"
            .to_string(),
        "d'OpenRouter, taille effective (paramètres actifs et totaux), spécialisation code, versions"
            .to_string(),
        "allégées pénalisées ; pondéré par /models bench (tâches réussies, compatibilité avec Claw)."
            .to_string(),
        "Seuls les modèles gratuits qui savent appeler des outils sont listés ; les incompatibles"
            .to_string(),
        "mesurés sont écartés.".to_string(),
        String::new(),
        "Suite".to_string(),
        "  /model <id>            utiliser ce modèle dans la session".to_string(),
        "  \"model\": \"<id>\"        dans .claw.json pour le garder".to_string(),
        "  /models refresh        recharger le catalogue".to_string(),
        "  /models local          modèles locaux (Ollama, LM Studio…) et autres fournisseurs"
            .to_string(),
        "  /models vision         modèles qui lisent les images (joindre : @capture.png)"
            .to_string(),
        "  /models claude         gratuits les plus proches de Claude Opus et Fable".to_string(),
        "  /models <recherche>    filtrer (ex. /models poolside)".to_string(),
        "  /models bench          mesurer les modèles gratuits compatibles avec Claw sur de vraies"
            .to_string(),
        "                         tâches (≈ 15 min ; --rapide, --local, --appliquer)".to_string(),
        String::new(),
        "Bon à savoir : chaque étape de l'agent (un appel au modèle) consomme une requête du"
            .to_string(),
        "quota gratuit. Selon le fournisseur, les requêtes vers un modèle gratuit peuvent être"
            .to_string(),
        "conservées : vérifiez sa politique de données avant d'y envoyer du code confidentiel."
            .to_string(),
    ]
}

/// Rapport de `/models [refresh|local|<recherche>]`.
pub fn models_report(args: Option<&str>, current_model: &str) -> Result<String, String> {
    let args = args.map(str::trim).filter(|value| !value.is_empty());
    if matches!(
        args,
        Some("local" | "locaux" | "fournisseurs" | "providers")
    ) {
        return Ok(providers_report(current_model));
    }
    if args.is_some_and(|args| args == "bench" || args.starts_with("bench ")) {
        return crate::bench::run(args.unwrap_or_default(), &mut |line| println!("{line}"));
    }
    if matches!(args, Some("claude" | "proches" | "opus" | "fable")) {
        return claude_likeness_report();
    }
    let refresh = args == Some("refresh");
    let filter = args.filter(|_| !refresh).map(str::to_ascii_lowercase);
    let (catalog, warning) = catalog(refresh)?;
    let free_with_tools = catalog
        .models
        .iter()
        .filter(|model| model.is_free() && model.supports_tools)
        .count();
    // Même ordre que le choix automatique : les modèles mesurés par
    // `/models bench` d'abord (y compris hors du classement), puis le classement.
    // Du plus efficace au moins efficace, avec les raisons de sa puissance.
    let measured = crate::bench::recent_results();
    let ranked = preferred_free_models(&catalog)
        .into_iter()
        .filter_map(|id| {
            let model = catalog.models.iter().find(|model| model.id == id)?;
            let (score, reasons) = efficiency(&id, Some(&catalog), measured.get(&id))?;
            Some(RankedModel {
                model: model.clone(),
                score,
                reasons,
            })
        })
        .filter(|entry| {
            filter.as_ref().is_none_or(|filter| {
                // `/models vision` : modèles qui lisent les images.
                (filter == "vision" && entry.model.supports_images)
                    || entry.model.id.to_ascii_lowercase().contains(filter)
                    || entry.model.name.to_ascii_lowercase().contains(filter)
            })
        })
        .collect::<Vec<_>>();

    let mut lines = vec![
        "Modèles gratuits recommandés pour le code (OpenRouter)".to_string(),
        format!(
            "  Catalogue        {} · {} modèle(s) gratuit(s) avec appel d'outils, {} retenu(s)",
            format_age(unix_now().saturating_sub(catalog.fetched_at)),
            free_with_tools,
            ranked.len()
        ),
    ];
    if let Some(quota) = free_quota_line() {
        lines.push(format!("  Quota gratuit    {quota}"));
    }
    lines.push(format!(
        "  Escalade         {}",
        escalation_line(current_model)
    ));
    if let Some(warning) = warning {
        lines.push(format!("  Attention        {warning}"));
    }
    lines.push(String::new());
    if ranked.is_empty() {
        lines.push(match &filter {
            Some(filter) => format!("  Aucun modèle retenu ne correspond à « {filter} »."),
            None => "  Aucun modèle gratuit n'a de signal de code fiable aujourd'hui.".to_string(),
        });
    } else {
        lines.extend(render_ranking(&ranked, current_model, &catalog));
    }
    lines.extend(models_report_footer());
    Ok(lines.join("\n"))
}

fn render_ranking(
    ranked: &[RankedModel],
    current_model: &str,
    catalog: &ModelCatalog,
) -> Vec<String> {
    let width = ranked
        .iter()
        .map(|entry| entry.model.id.len())
        .max()
        .unwrap_or(0)
        .max(6);
    let measured = crate::bench::recent_results();
    let mut lines = vec![format!("  #  {:<width$}  Contexte  Pourquoi", "Modèle")];
    for (index, entry) in ranked.iter().enumerate() {
        let marker = if entry.model.id == current_model {
            "  ● actuel"
        } else {
            ""
        };
        let mut details = entry.reasons.clone();
        // Ses autres spécialités (le classement est déjà celui du code).
        let others: Vec<&str> = api::model_specialties(&entry.model, catalog)
            .into_iter()
            .filter(|specialty| specialty.key != "programming")
            .map(|specialty| specialty.label)
            .collect();
        if !others.is_empty() {
            details.push(format!("aussi : {}", others.join(", ").to_lowercase()));
        }
        if let Some(result) = measured.get(&entry.model.id) {
            details.push(format!("mesuré : {}", result.summary()));
        }
        lines.push(format!(
            "  {:<2} {:<width$}  {:>8}  {}{marker}",
            index + 1,
            entry.model.id,
            format_context(entry.model.context_length),
            details.join(" · "),
        ));
    }
    lines
}

fn free_quota_line() -> Option<String> {
    let key = env::var("OPENROUTER_API_KEY")
        .ok()
        .filter(|key| !key.trim().is_empty())?;
    let quota = block_on(api::fetch_free_quota(
        &api::read_openrouter_base_url(),
        &key,
    ))
    .ok()?
    .ok()??;
    Some(format!(
        "{}/{} requêtes utilisées aujourd'hui ({} restantes, remise à zéro à minuit UTC)",
        quota.used, quota.limit, quota.remaining
    ))
}

/// Coût estimé d'une session pour un modèle `OpenRouter` connu du catalogue.
pub fn estimated_cost(model: &str, usage: TokenUsage) -> Option<String> {
    register_cached_limits();
    let limits = api::model_limits(model)?;
    if limits.prompt_price < 0.0 || limits.completion_price < 0.0 {
        return None;
    }
    if limits.prompt_price.abs() < f64::EPSILON && limits.completion_price.abs() < f64::EPSILON {
        return Some("0 $ (modèle gratuit)".to_string());
    }
    let cost = f64::from(usage.input_tokens) * limits.prompt_price
        + f64::from(usage.output_tokens) * limits.completion_price;
    Some(format!("{cost:.4} $ (tarifs OpenRouter)"))
}

/// Ligne « coût estimé » de `/cost`.
pub fn cost_line(model: &str, usage: TokenUsage) -> Option<String> {
    estimated_cost(model, usage).map(|cost| format!("  Coût estimé      {cost}"))
}

fn favorites_path() -> Option<PathBuf> {
    let cwd = env::current_dir().ok()?;
    Some(
        ConfigLoader::default_for(cwd)
            .config_home()
            .join("favorite-models.json"),
    )
}

/// Modèles favoris de l'utilisateur, épinglés en haut du menu des modèles
/// (`~/.claw/favorite-models.json`), dans l'ordre où ils ont été ajoutés.
pub fn favorite_models() -> Vec<String> {
    favorites_path()
        .and_then(|path| fs::read_to_string(path).ok())
        .and_then(|contents| serde_json::from_str(&contents).ok())
        .unwrap_or_default()
}

/// Liste des favoris après ajout ou retrait de `id` (sans doublon).
fn with_favorite(mut favorites: Vec<String>, id: &str, favorite: bool) -> Vec<String> {
    favorites.retain(|model| model != id);
    if favorite {
        favorites.push(id.to_string());
    }
    favorites
}

/// Ajoute ou retire un favori ; renvoie la nouvelle liste.
pub fn set_favorite_model(id: &str, favorite: bool) -> Result<Vec<String>, String> {
    let id = id.trim();
    if id.is_empty() || id.len() > 200 || id.chars().any(char::is_control) {
        return Err("identifiant de modèle invalide".to_string());
    }
    let path = favorites_path().ok_or("dossier de configuration introuvable")?;
    let favorites = with_favorite(favorite_models(), id, favorite);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let contents = serde_json::to_string_pretty(&favorites).map_err(|error| error.to_string())?;
    fs::write(path, contents).map_err(|error| format!("favoris non enregistrés : {error}"))?;
    Ok(favorites)
}

/// Prix d'un million de jetons, en dollars (`None` si inconnu).
fn price_per_million(price_per_token: f64) -> Option<f64> {
    (price_per_token >= 0.0).then_some(price_per_token * 1_000_000.0)
}

/// Fiche d'un modèle pour l'interface web : catalogue `OpenRouter` en cache
/// (aucune requête réseau), puissance estimée, mesures de `/models bench`,
/// disponibilité.
pub fn model_info_json(id: &str) -> serde_json::Value {
    let catalog = load_cached_catalog();
    let measured = crate::bench::recent_results();
    let result = measured.get(id);
    let entry = catalog
        .as_ref()
        .and_then(|catalog| catalog.models.iter().find(|model| model.id == id));
    let compat = api::split_compat_model(id);
    let base_id = id.trim_end_matches(":free");
    let rank = catalog.as_ref().and_then(|catalog| {
        catalog
            .programming_ranking
            .iter()
            .position(|ranked| ranked == id || ranked == base_id)
            .map(|position| position + 1)
    });
    // Un modèle hors catalogue (local…) n'a pas de puissance estimée fiable.
    let power = entry
        .and_then(|_| efficiency(id, catalog.as_ref(), result))
        .map(|(score, reasons)| (score.round(), reasons));
    let provider = match (&compat, id.split_once('/')) {
        (Some((provider, _)), _) if provider.local => {
            format!("{} (sur cette machine)", provider.name)
        }
        (Some((provider, _)), _) => provider.name.clone(),
        (None, Some((organization, _))) => format!("{organization} (via OpenRouter)"),
        (None, None) => "accès direct".to_string(),
    };
    let hard = |score: Option<&crate::bench::HardScore>| {
        score.map(|score| {
            serde_json::json!({
                "text": crate::bench_hard::score_text(score),
                "failed": score.failed,
                "tested_at": score.tested_at,
            })
        })
    };
    serde_json::json!({
        "id": id,
        "name": entry.map(|model| model.name.clone()),
        "description": entry.map(|model| model.description.clone()).filter(|text| !text.is_empty()),
        "provider": provider,
        "local": compat.as_ref().is_some_and(|(provider, _)| provider.local),
        "in_catalog": entry.is_some(),
        "context_length": entry
            .and_then(|model| model.context_length)
            .or_else(|| compat.as_ref().and_then(|(provider, _)| provider.context_window)),
        "max_output": entry.and_then(|model| model.max_completion_tokens).or_else(|| {
            compat
                .as_ref()
                .and_then(|(provider, _)| provider.max_output_tokens.map(u64::from))
        }),
        "free": entry.map(api::OpenRouterModel::is_free),
        "prompt_price": entry.and_then(|model| price_per_million(model.prompt_price)),
        "completion_price": entry.and_then(|model| price_per_million(model.completion_price)),
        "tools": entry.map(|model| model.supports_tools),
        "images": entry.map(|model| model.supports_images),
        "programming_rank": rank,
        "score": power.as_ref().map(|(score, _)| *score),
        "reasons": power.map(|(_, reasons)| reasons).unwrap_or_default(),
        "measured": result.map(crate::bench::BenchResult::summary),
        "measured_at": result.map(|result| result.tested_at),
        "incompatible": result.is_some_and(|result| !result.compatible && !result.transient),
        "hard": hard(result.and_then(|result| result.hard.as_ref())),
        "hard_unassisted": hard(result.and_then(|result| result.hard_unassisted.as_ref())),
        "available": api::model_available(id),
        "specialties": match (entry, catalog.as_ref()) {
            (Some(model), Some(catalog)) => specialties_json(&api::model_specialties(model, catalog)),
            _ => Vec::new(),
        },
        // Ressemblance avec Claude Opus et Fable, critère par critère.
        "likeness": match (entry, catalog.as_ref()) {
            (Some(model), Some(catalog)) if !model.id.starts_with("anthropic/") => {
                crate::likeness::claude_references(catalog)
                    .into_iter()
                    .map(|reference| crate::likeness::likeness(model, reference, catalog, result).json(true))
                    .collect::<Vec<_>>()
            }
            _ => Vec::new(),
        },
        "url": (compat.is_none() && id.contains('/')).then(|| format!("https://openrouter.ai/{id}")),
        "catalog_at": catalog.as_ref().map(|catalog| catalog.fetched_at),
    })
}

fn specialties_json(specialties: &[api::Specialty]) -> Vec<serde_json::Value> {
    specialties
        .iter()
        .map(|specialty| {
            serde_json::json!({
                "key": specialty.key,
                "label": specialty.label,
                "detail": specialty.detail,
            })
        })
        .collect()
}

/// Modèles payants les plus utilisés d'une catégorie d'`OpenRouter`, parmi
/// ceux qui savent appeler des outils (cinq au plus), avec leur prix.
fn paid_models_json(category: &str, catalog: &ModelCatalog) -> Vec<serde_json::Value> {
    let Some(ranking) = catalog.category_rankings.get(category) else {
        return Vec::new();
    };
    ranking
        .iter()
        .enumerate()
        .filter_map(|(position, id)| {
            let model = catalog.models.iter().find(|model| &model.id == id)?;
            if model.is_free() || !model.supports_tools {
                return None;
            }
            Some(serde_json::json!({
                "id": model.id,
                "name": model.name,
                "context": format_context(model.context_length),
                "rank": position + 1,
                "prompt_price": price_per_million(model.prompt_price)?,
                "completion_price": price_per_million(model.completion_price)?,
            }))
        })
        .take(5)
        .collect()
}

/// Modèles gratuits et quota au format JSON de l'interface web : tous les
/// modèles gratuits utilisables par l'agent (appels d'outils, pas un routeur,
/// pas mesurés incompatibles) avec leurs spécialités, et une catégorie par
/// spécialité. `refresh` relit le catalogue et les classements chez
/// `OpenRouter` au lieu du cache de 24 h (bouton « Rechercher »).
pub fn free_coding_models_json(refresh: bool) -> Result<serde_json::Value, String> {
    let (catalog, warning) = catalog(refresh)?;
    let mut view = models_view_json(&catalog, &crate::bench::recent_results());
    view["quota"] = serde_json::json!(free_quota_line());
    view["warning"] = serde_json::json!(warning);
    view["favorites"] = serde_json::json!(favorite_models());
    Ok(view)
}

/// Une catégorie par spécialité (autre que la programmation) : modèles
/// gratuits rangés par rang chez `OpenRouter` puis par efficacité, et les
/// payants les plus utilisés de la catégorie.
fn specialty_categories(
    usable: &[(&api::OpenRouterModel, f64, Vec<String>, Vec<api::Specialty>)],
    catalog: &ModelCatalog,
) -> Vec<serde_json::Value> {
    let mut categories = Vec::new();
    for (key, label) in api::SPECIALTIES
        .iter()
        .filter(|(key, _)| *key != "programming")
    {
        let mut members: Vec<(usize, usize, &str)> = usable
            .iter()
            .enumerate()
            .filter(|(_, entry)| entry.3.iter().any(|specialty| specialty.key == *key))
            .map(|(order, entry)| {
                let rank = api::category_rank(&entry.0.id, key, catalog).unwrap_or(usize::MAX);
                (rank, order, entry.0.id.as_str())
            })
            .collect();
        members.sort_unstable();
        let paid = paid_models_json(key, catalog);
        if members.is_empty() && paid.is_empty() {
            continue;
        }
        categories.push(serde_json::json!({
            "key": key,
            "label": label,
            "models": members.iter().map(|member| member.2).collect::<Vec<_>>(),
            "paid": paid,
        }));
    }

    categories
}

/// Catégorie « Proches de Claude » : modèles gratuits rangés du plus proche
/// au moins proche de chaque référence (Opus, Fable). `None` sans référence.
fn claude_category(
    closest: &[(
        &api::OpenRouterModel,
        Vec<(&api::OpenRouterModel, crate::likeness::Likeness)>,
    )],
    catalog: &ModelCatalog,
    measured: &std::collections::BTreeMap<String, crate::bench::BenchResult>,
) -> Option<serde_json::Value> {
    let (_, ranked) = closest.first()?;
    // En tête : les gratuits les plus proches de Claude, pour chaque référence.
    let by_reference: serde_json::Map<String, serde_json::Value> = closest
        .iter()
        .map(|(reference, ranked)| {
            (
                reference.id.clone(),
                serde_json::json!(ranked
                    .iter()
                    .map(|(model, _)| model.id.as_str())
                    .collect::<Vec<_>>()),
            )
        })
        .collect();
    // Payants proches ou mieux classés, par référence : jamais choisis sans
    // un clic (et une confirmation) de l'utilisateur.
    let paid_by_reference: serde_json::Map<String, serde_json::Value> = closest
        .iter()
        .map(|(reference, ranked)| {
            let best_free = ranked.first().map_or(0, |(_, likeness)| likeness.percent);
            let paid = paid_close_to(reference, catalog, measured, best_free);
            (
                reference.id.clone(),
                serde_json::json!(paid.iter().map(PaidCandidate::json).collect::<Vec<_>>()),
            )
        })
        .collect();
    Some(serde_json::json!({
        "key": "claude",
        "label": "Proches de Claude",
        "models": ranked.iter().map(|(model, _)| model.id.as_str()).collect::<Vec<_>>(),
        "by_reference": by_reference,
        "paid": [],
        "paid_by_reference": paid_by_reference,
    }))
}

/// Modèles gratuits utilisables, leurs spécialités et les catégories, pour un
/// catalogue et des mesures donnés (sans accès réseau).
fn models_view_json(
    catalog: &ModelCatalog,
    measured: &std::collections::BTreeMap<String, crate::bench::BenchResult>,
) -> serde_json::Value {
    // Du plus efficace au moins efficace, comme le choix automatique.
    let mut usable: Vec<(&api::OpenRouterModel, f64, Vec<String>, Vec<api::Specialty>)> = catalog
        .models
        .iter()
        .filter(|model| {
            model.is_free() && model.supports_tools && !model.id.starts_with("openrouter/")
        })
        .filter_map(|model| {
            let (score, reasons) = efficiency(&model.id, Some(catalog), measured.get(&model.id))?;
            Some((
                model,
                score,
                reasons,
                api::model_specialties(model, catalog),
            ))
        })
        .collect();
    usable.sort_by(|left, right| right.1.total_cmp(&left.1));

    let references = crate::likeness::claude_references(catalog);
    let candidates: Vec<&api::OpenRouterModel> = usable.iter().map(|entry| entry.0).collect();
    let closest: Vec<(
        &api::OpenRouterModel,
        Vec<(&api::OpenRouterModel, crate::likeness::Likeness)>,
    )> = references
        .iter()
        .map(|reference| {
            (
                *reference,
                closest_to(reference, &candidates, catalog, measured),
            )
        })
        .collect();
    let likeness_of = |id: &str| -> Vec<serde_json::Value> {
        closest
            .iter()
            .filter_map(|(_, ranked)| ranked.iter().find(|(model, _)| model.id == id))
            .map(|(_, likeness)| likeness.json(false))
            .collect()
    };

    let models = usable
        .iter()
        .map(|(model, score, reasons, specialties)| {
            serde_json::json!({
                "likeness": likeness_of(&model.id),
                "id": model.id,
                "name": model.name,
                "context": format_context(model.context_length),
                "reasons": reasons,
                "score": score.round(),
                "measured": measured.get(&model.id).map(crate::bench::BenchResult::summary),
                "available": api::model_available(&model.id),
                "specialties": specialties_json(specialties),
            })
        })
        .collect::<Vec<_>>();

    // Une catégorie par spécialité : « Programmation » garde le classement du
    // code, les autres rangent par rang chez OpenRouter puis par efficacité.
    let mut categories: Vec<serde_json::Value> = claude_category(&closest, catalog, measured)
        .into_iter()
        .collect();
    categories.extend([
        serde_json::json!({
            "key": "programming",
            "label": "Programmation",
            "models": preferred_free_models(catalog),
            "paid": paid_models_json("programming", catalog),
        }),
        serde_json::json!({
            "key": "all",
            "label": "Tous",
            "models": usable.iter().map(|entry| entry.0.id.as_str()).collect::<Vec<_>>(),
            "paid": [],
        }),
    ]);
    categories.extend(specialty_categories(&usable, catalog));

    serde_json::json!({
        "models": models,
        "categories": categories,
        // Modèles Claude servant de référence à la ressemblance.
        "references": references
            .iter()
            .map(|reference| serde_json::json!({
                "id": reference.id,
                "name": crate::likeness::display_name(reference),
            }))
            .collect::<Vec<_>>(),
        // Heure (secondes Unix) du catalogue utilisé pour ce classement.
        "catalog_at": catalog.fetched_at,
    })
}

#[cfg(test)]
mod tests {
    use super::{format_age, format_context, render_ranking};
    use api::{OpenRouterModel, RankedModel};

    #[test]
    fn formats_context_sizes_and_ages() {
        assert_eq!(format_context(Some(262_144)), "256k");
        assert_eq!(format_context(Some(1_000_000)), "1M");
        assert_eq!(format_context(Some(1_048_576)), "1M");
        assert_eq!(format_context(Some(1_500_000)), "1.5M");
        assert_eq!(format_context(None), "?");
        assert_eq!(format_age(30), "à l'instant");
        assert_eq!(format_age(600), "il y a 10 min");
        assert_eq!(format_age(7_200), "il y a 2 h");
    }

    #[test]
    fn model_info_describes_models_outside_the_catalog() {
        let local = super::model_info_json("ollama:modele-de-test");
        assert_eq!(local["local"], true);
        assert_eq!(local["in_catalog"], false);
        assert_eq!(local["provider"], "ollama (sur cette machine)");
        assert!(local["url"].is_null() && local["score"].is_null());

        let remote = super::model_info_json("acme/modele-inconnu-de-test:free");
        assert_eq!(remote["local"], false);
        assert_eq!(remote["provider"], "acme (via OpenRouter)");
        assert_eq!(
            remote["url"],
            "https://openrouter.ai/acme/modele-inconnu-de-test:free"
        );
        assert!(remote["programming_rank"].is_null());

        // Favoris : sans doublon, le dernier ajouté à la fin.
        let favorites = super::with_favorite(vec!["a".to_string(), "b".to_string()], "a", true);
        assert_eq!(favorites, ["b", "a"]);
        assert_eq!(super::with_favorite(favorites, "b", false), ["a"]);

        let per_million = super::price_per_million(0.000_002).expect("prix");
        assert!((per_million - 2.0).abs() < 1e-9);
        assert_eq!(super::price_per_million(-1.0), None);
    }

    #[test]
    fn image_readers_are_free_models_that_read_images() {
        let catalog = specialty_catalog();
        // Ni le routeur d'OpenRouter, ni un modèle payant, ni un modèle sans vision.
        let readers = super::image_readers_in(&catalog, "pool/coder:free", |_| true);
        assert_eq!(readers.len(), 2, "{readers:?}");
        assert!(readers.contains(&"lab/vue:free".to_string()));
        assert!(readers.contains(&"lab/muet:free".to_string()));
        // Le modèle de la session n'est pas son propre lecteur.
        assert_eq!(
            super::image_readers_in(&catalog, "lab/vue:free", |_| true),
            ["lab/muet:free"]
        );
        // Un modèle mis de côté (quota, panne) passe après ceux qui répondent.
        let first = readers[0].clone();
        let reordered = super::image_readers_in(&catalog, "pool/coder:free", |id| id != first);
        assert_eq!(reordered.last(), Some(&first));
    }

    /// Catalogue fictif : deux modèles gratuits utilisables, un sans outils, le
    /// routeur, et deux payants classés en droit.
    fn specialty_catalog() -> api::ModelCatalog {
        let model =
            |id: &str, description: &str, price: f64, tools: bool, images: bool| OpenRouterModel {
                id: id.to_string(),
                name: id.to_string(),
                description: description.to_string(),
                context_length: Some(262_144),
                prompt_price: price,
                completion_price: price * 4.0,
                supports_tools: tools,
                supports_images: images,
                ..Default::default()
            };
        api::ModelCatalog {
            version: api::CATALOG_VERSION,
            fetched_at: 42,
            models: vec![
                model(
                    "pool/coder:free",
                    "The latest coding agent model with 8B active parameters.",
                    0.0,
                    true,
                    false,
                ),
                model(
                    "lab/vue:free",
                    "A multimodal assistant with 12B active parameters.",
                    0.0,
                    true,
                    true,
                ),
                model(
                    "lab/muet:free",
                    "A vision model without tool calling.",
                    0.0,
                    false,
                    true,
                ),
                model("openrouter/free", "Router.", 0.0, true, true),
                model(
                    "big/droit",
                    "A model for legal work.",
                    0.000_002,
                    true,
                    false,
                ),
                model(
                    "big/sans-outils",
                    "A model for legal work.",
                    0.000_001,
                    false,
                    false,
                ),
            ],
            programming_ranking: Vec::new(),
            category_rankings: std::collections::BTreeMap::from([(
                "legal".to_string(),
                vec![
                    "big/sans-outils".to_string(),
                    "big/droit".to_string(),
                    "lab/vue".to_string(),
                ],
            )]),
        }
    }

    #[test]
    fn models_are_grouped_by_specialty_with_paid_ones_apart() {
        let catalog = specialty_catalog();
        let view = super::models_view_json(&catalog, &std::collections::BTreeMap::new());

        // Seuls les modèles gratuits qui appellent des outils (et pas le routeur).
        let ids: Vec<&str> = view["models"]
            .as_array()
            .expect("modèles")
            .iter()
            .map(|entry| entry["id"].as_str().expect("id"))
            .collect();
        assert_eq!(ids.len(), 2);
        assert!(ids.contains(&"pool/coder:free") && ids.contains(&"lab/vue:free"));

        let category = |key: &str| {
            view["categories"]
                .as_array()
                .expect("catégories")
                .iter()
                .find(|entry| entry["key"] == key)
                .cloned()
        };
        let keys: Vec<String> = view["categories"]
            .as_array()
            .expect("catégories")
            .iter()
            .map(|entry| entry["key"].as_str().expect("clé").to_string())
            .collect();
        assert_eq!(keys, ["programming", "all", "vision", "legal"]);
        assert_eq!(
            category("vision").expect("images")["models"],
            serde_json::json!(["lab/vue:free"])
        );
        // Droit : le modèle gratuit classé (par sa variante payante), puis le payant
        // qui appelle des outils, avec son prix par million de jetons.
        let legal = category("legal").expect("droit");
        assert_eq!(legal["label"], "Droit");
        assert_eq!(legal["models"], serde_json::json!(["lab/vue:free"]));
        assert_eq!(legal["paid"].as_array().expect("payants").len(), 1);
        assert_eq!(legal["paid"][0]["id"], "big/droit");
        assert_eq!(legal["paid"][0]["rank"], 2);
        assert!((legal["paid"][0]["prompt_price"].as_f64().expect("prix") - 2.0).abs() < 1e-9);
        assert_eq!(view["catalog_at"], 42);

        // Spécialités portées par chaque modèle.
        let vue = view["models"]
            .as_array()
            .expect("modèles")
            .iter()
            .find(|entry| entry["id"] == "lab/vue:free")
            .expect("modèle");
        let labels: Vec<&str> = vue["specialties"]
            .as_array()
            .expect("spécialités")
            .iter()
            .map(|entry| entry["label"].as_str().expect("nom"))
            .collect();
        assert_eq!(labels, ["Images", "Droit"]);
    }

    /// Catalogue fictif autour de Claude : la référence, une version plus
    /// ancienne, un gratuit proche, et des payants proches, classés ou lointains.
    fn claude_catalog() -> api::ModelCatalog {
        let flagship =
            "Flagship model for demanding reasoning, coding, and long-horizon agentic work.";
        let model = |id: &str, description: &str, context: u64, tools: bool| OpenRouterModel {
            id: id.to_string(),
            name: id.to_string(),
            description: description.to_string(),
            context_length: Some(context),
            max_completion_tokens: Some(context / 8),
            prompt_price: if id.ends_with(":free") {
                0.0
            } else {
                0.000_003
            },
            completion_price: if id.ends_with(":free") {
                0.0
            } else {
                0.000_015
            },
            supports_tools: tools,
            supports_reasoning: true,
            supports_images: context >= 1_000_000,
            ..Default::default()
        };
        api::ModelCatalog {
            version: api::CATALOG_VERSION,
            fetched_at: 42,
            models: vec![
                model("anthropic/claude-opus-5.5", flagship, 1_000_000, true),
                model("anthropic/claude-opus-4.8", flagship, 1_000_000, true),
                model("anthropic/claude-opus-5.5:batch", flagship, 1_000_000, true),
                model("lab/proche:free", flagship, 262_144, true),
                model("big/rival", flagship, 1_000_000, true),
                model("big/sans-outils", flagship, 1_000_000, false),
                model("big/classe", "A compact chat assistant.", 32_768, true),
                model("big/loin", "A compact chat assistant.", 32_768, true),
            ],
            programming_ranking: Vec::new(),
            category_rankings: std::collections::BTreeMap::from([(
                "programming".to_string(),
                vec![
                    "big/classe".to_string(),
                    "anthropic/claude-opus-5.5".to_string(),
                    "big/rival".to_string(),
                ],
            )]),
        }
    }

    #[test]
    fn paid_models_close_to_claude_are_listed_apart_and_never_picked() {
        let catalog = claude_catalog();
        let measured = std::collections::BTreeMap::new();
        let reference = catalog
            .models
            .iter()
            .find(|model| model.id == "anthropic/claude-opus-5.5")
            .expect("référence");

        let paid = super::paid_close_to(reference, &catalog, &measured, 0);
        let ids: Vec<&str> = paid
            .iter()
            .map(|candidate| candidate.model.id.as_str())
            .collect();
        // Le plus proche d'abord ; « big/classe » ressemble peu à Claude mais
        // le devance au classement en programmation.
        assert_eq!(ids, ["big/rival", "big/classe"]);
        assert!(!paid[0].better_ranked() && paid[1].better_ranked());
        assert!(paid[0].likeness.percent > paid[1].likeness.percent);
        assert_eq!(paid[0].price(), "3,00 $ / 15,00 $ par million");
        // Jamais mesuré : ce critère n'est pas présenté comme un écart.
        assert!(!paid[0]
            .published_gaps()
            .contains(&crate::likeness::MEASURED));

        // Un gratuit plus proche relève la barre : seul le mieux classé reste.
        let stricter = super::paid_close_to(reference, &catalog, &measured, 101);
        let ids: Vec<&str> = stricter
            .iter()
            .map(|candidate| candidate.model.id.as_str())
            .collect();
        assert_eq!(ids, ["big/classe"]);

        // Interface web : les payants sont à part, par référence, et la liste
        // proposée par défaut ne contient que des gratuits.
        let view = super::models_view_json(&catalog, &measured);
        let claude = view["categories"]
            .as_array()
            .expect("catégories")
            .iter()
            .find(|entry| entry["key"] == "claude")
            .expect("proches de Claude");
        assert_eq!(claude["models"], serde_json::json!(["lab/proche:free"]));
        assert_eq!(claude["paid"], serde_json::json!([]));
        let listed = &claude["paid_by_reference"]["anthropic/claude-opus-5.5"];
        assert_eq!(listed[0]["id"], "big/rival");
        assert_eq!(listed[0]["better_ranked"], false);
        assert!((listed[0]["prompt_price"].as_f64().expect("prix") - 3.0).abs() < 1e-9);
        let ranked = listed
            .as_array()
            .expect("payants")
            .iter()
            .find(|entry| entry["id"] == "big/classe")
            .expect("mieux classé");
        assert_eq!(ranked["better_ranked"], true);
        assert_eq!(ranked["rank"], 1);
        assert_eq!(ranked["reference_rank"], 2);
        assert!(view["models"]
            .as_array()
            .expect("modèles")
            .iter()
            .all(|entry| entry["id"].as_str().expect("id").ends_with(":free")));
    }

    #[test]
    fn escalation_setting_defaults_to_the_strongest_free_model() {
        use super::Escalation;
        assert_eq!(Escalation::parse(None, false), Escalation::StrongestFree);
        assert_eq!(
            Escalation::parse(Some("  "), false),
            Escalation::StrongestFree
        );
        assert_eq!(
            Escalation::parse(Some("free"), true),
            Escalation::StrongestFree
        );
        assert_eq!(
            Escalation::parse(Some("Gratuit"), false),
            Escalation::StrongestFree
        );
        // Une session limitée à un seul modèle ne change pas de modèle d'elle-même.
        assert_eq!(Escalation::parse(None, true), Escalation::Off);
        assert_eq!(Escalation::parse(Some("off"), false), Escalation::Off);
        // Un modèle écrit par l'utilisateur est respecté, sauf si c'est déjà le sien.
        let chosen = Escalation::parse(Some("lab/fort"), true);
        assert_eq!(chosen, Escalation::Model("lab/fort".to_string()));
        assert_eq!(chosen.target("lab/autre").as_deref(), Some("lab/fort"));
        assert_eq!(chosen.target("lab/fort"), None);
        assert_eq!(Escalation::Off.target("lab/autre"), None);
    }

    #[test]
    fn escalation_picks_the_strongest_available_free_model() {
        let model = |id: &str, description: &str, price: f64| OpenRouterModel {
            id: id.to_string(),
            name: id.to_string(),
            description: description.to_string(),
            context_length: Some(262_144),
            prompt_price: price,
            completion_price: price,
            supports_tools: true,
            ..Default::default()
        };
        let coder = "A coding agent model with";
        let catalog = api::ModelCatalog {
            version: api::CATALOG_VERSION,
            fetched_at: 42,
            models: vec![
                model(
                    "lab/petit-mini:free",
                    &format!("{coder} 4B parameters."),
                    0.0,
                ),
                model("lab/moyen:free", &format!("{coder} 30B parameters."), 0.0),
                model("lab/geant:free", &format!("{coder} 500B parameters."), 0.0),
                model("lab/casse:free", &format!("{coder} 900B parameters."), 0.0),
                model("big/payant", &format!("{coder} 900B parameters."), 0.000_01),
            ],
            programming_ranking: Vec::new(),
            category_rankings: std::collections::BTreeMap::from([(
                "programming".to_string(),
                vec!["big/payant".to_string()],
            )]),
        };
        let result = |id: &str, compatible: bool| {
            let value = serde_json::json!({
                "model": id, "tested_at": 1, "compatible": compatible, "passed": 3, "total": 3,
            });
            (
                id.to_string(),
                serde_json::from_value::<crate::bench::BenchResult>(value).expect("mesure"),
            )
        };
        let measured = std::collections::BTreeMap::from([
            result("lab/petit-mini:free", true),
            result("lab/moyen:free", true),
            result("lab/geant:free", true),
            // Incompatible avec Claw : jamais retenu, même s'il est le plus gros.
            result("lab/casse:free", false),
        ]);
        let ranking: Vec<String> = super::scored_free_models(&catalog, &measured)
            .into_iter()
            .map(|(_, id)| id)
            .collect();
        assert_eq!(
            ranking,
            ["lab/geant:free", "lab/moyen:free", "lab/petit-mini:free"]
        );

        let target = |current: &str, down: &str| {
            super::strongest_free_in(&catalog, &measured, current, |id| id != down)
        };
        // Le plus puissant reprend le tour, quel que soit le modèle de départ.
        assert_eq!(
            target("lab/petit-mini:free", "").as_deref(),
            Some("lab/geant:free")
        );
        assert_eq!(
            target("lab/moyen:free", "").as_deref(),
            Some("lab/geant:free")
        );
        assert_eq!(
            target("ollama:qwen3.5:4b", "").as_deref(),
            Some("lab/geant:free")
        );
        // Déjà au sommet : rien de plus fort à appeler.
        assert_eq!(target("lab/geant:free", ""), None);
        // Le plus puissant est indisponible : le suivant, s'il est plus fort.
        assert_eq!(
            target("lab/petit-mini:free", "lab/geant:free").as_deref(),
            Some("lab/moyen:free")
        );
        assert_eq!(target("lab/moyen:free", "lab/geant:free"), None);
        // Jamais un payant, même mieux classé.
        for current in ["lab/petit-mini:free", "lab/moyen:free", "lab/geant:free"] {
            assert_ne!(target(current, "").as_deref(), Some("big/payant"));
        }
    }

    #[test]
    fn ranking_marks_the_current_model() {
        let entry = RankedModel {
            model: OpenRouterModel {
                id: "pool/laguna:free".to_string(),
                name: "Laguna".to_string(),
                description: String::new(),
                context_length: Some(262_144),
                max_completion_tokens: None,
                prompt_price: 0.0,
                completion_price: 0.0,
                supports_tools: true,
                supports_images: false,
                ..Default::default()
            },
            score: 50.0,
            reasons: vec!["spécialisé code".to_string()],
        };
        let lines = render_ranking(&[entry], "pool/laguna:free", &api::ModelCatalog::default());
        assert!(lines[1].contains("pool/laguna:free"));
        assert!(lines[1].contains("256k"));
        assert!(lines[1].ends_with("spécialisé code  ● actuel"));
    }

    #[test]
    fn free_keyword_expands_in_place_in_the_fallback_chain() {
        let chain = super::model_chain(
            "qwen/qwen3-coder:free",
            &["free".to_string(), "ollama:qwen2.5-coder:7b".to_string()],
        );
        assert_eq!(
            chain.first().map(String::as_str),
            Some("qwen/qwen3-coder:free")
        );
        assert_eq!(
            chain.last().map(String::as_str),
            Some("ollama:qwen2.5-coder:7b")
        );
        assert!(!chain.iter().any(|model| model == "free"), "{chain:?}");
    }
}
