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
        Some(result) if result.total == 0 => Some(0.95),
        Some(result) => Some(0.4 + 0.6 * f64::from(result.passed) / f64::from(result.total)),
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
        });
    let (power, reasons) = api::model_power(&model, catalog);
    Some(((power.max(0.0) + 5.0) * factor, reasons))
}

/// Modèles gratuits du plus efficace au moins efficace : ceux retenus pour le
/// code par le catalogue et ceux mesurés par `/models bench`, sans les
/// incompatibles avec Claw.
fn preferred_free_models(catalog: &ModelCatalog) -> Vec<String> {
    let measured = crate::bench::recent_results();
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
    scored.into_iter().map(|(_, id)| id).collect()
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
        lines.extend(render_ranking(&ranked, current_model));
    }
    lines.extend([
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
    ]);
    Ok(lines.join("\n"))
}

fn render_ranking(ranked: &[RankedModel], current_model: &str) -> Vec<String> {
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
        if entry.model.supports_images {
            details.push("lit les images".to_string());
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

/// Classement des modèles gratuits pour le code et quota, au format JSON de l'interface web.
pub fn free_coding_models_json() -> Result<serde_json::Value, String> {
    let (catalog, warning) = catalog(false)?;
    // Même ordre que le choix automatique : du plus efficace au moins efficace.
    let measured = crate::bench::recent_results();
    let models = preferred_free_models(&catalog)
        .into_iter()
        .filter_map(|id| catalog.models.iter().find(|model| model.id == id))
        .map(|model| {
            let (score, reasons) = efficiency(&model.id, Some(&catalog), measured.get(&model.id))
                .unwrap_or((0.0, Vec::new()));
            serde_json::json!({
                "id": model.id,
                "name": model.name,
                "context": format_context(model.context_length),
                "reasons": reasons,
                "score": score.round(),
                "measured": measured.get(&model.id).map(crate::bench::BenchResult::summary),
            })
        })
        .collect::<Vec<_>>();
    Ok(serde_json::json!({
        "models": models,
        "quota": free_quota_line(),
        "warning": warning,
    }))
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
            },
            score: 50.0,
            reasons: vec!["spécialisé code".to_string()],
        };
        let lines = render_ranking(&[entry], "pool/laguna:free");
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
