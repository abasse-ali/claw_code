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

fn unix_now() -> u64 {
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
fn catalog(refresh: bool) -> Result<(ModelCatalog, Option<String>), String> {
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

/// Meilleur modèle gratuit pour le code (télécharge le catalogue si besoin).
pub fn best_free_coding_model() -> Option<String> {
    let (catalog, _) = catalog(false).ok()?;
    api::rank_free_coding_models(&catalog)
        .into_iter()
        .next()
        .map(|entry| entry.model.id)
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

/// Rapport de `/models [refresh|<recherche>]`.
pub fn models_report(args: Option<&str>, current_model: &str) -> Result<String, String> {
    let args = args.map(str::trim).filter(|value| !value.is_empty());
    let refresh = args == Some("refresh");
    let filter = args.filter(|_| !refresh).map(str::to_ascii_lowercase);
    let (catalog, warning) = catalog(refresh)?;
    let free_with_tools = catalog
        .models
        .iter()
        .filter(|model| model.is_free() && model.supports_tools)
        .count();
    let ranked = api::rank_free_coding_models(&catalog)
        .into_iter()
        .filter(|entry| {
            filter.as_ref().is_none_or(|filter| {
                entry.model.id.to_ascii_lowercase().contains(filter)
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
        "Classement : rang dans la catégorie programmation d'OpenRouter, spécialisation et"
            .to_string(),
        "benchmarks de code cités par le modèle, puis taille. Seuls les modèles gratuits qui"
            .to_string(),
        "savent appeler des outils sont listés (indispensable pour un agent).".to_string(),
        String::new(),
        "Suite".to_string(),
        "  /model <id>            utiliser ce modèle dans la session".to_string(),
        "  \"model\": \"<id>\"        dans .claw.json pour le garder".to_string(),
        "  /models refresh        recharger le catalogue".to_string(),
        "  /models <recherche>    filtrer (ex. /models poolside)".to_string(),
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
    let mut lines = vec![format!("  #  {:<width$}  Contexte  Pourquoi", "Modèle")];
    for (index, entry) in ranked.iter().enumerate() {
        let marker = if entry.model.id == current_model {
            "  ● actuel"
        } else {
            ""
        };
        lines.push(format!(
            "  {:<2} {:<width$}  {:>8}  {}{marker}",
            index + 1,
            entry.model.id,
            format_context(entry.model.context_length),
            entry.reasons.join(" · "),
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
    let models = api::rank_free_coding_models(&catalog)
        .into_iter()
        .map(|entry| {
            serde_json::json!({
                "id": entry.model.id,
                "name": entry.model.name,
                "context": format_context(entry.model.context_length),
                "reasons": entry.reasons,
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
            },
            score: 50.0,
            reasons: vec!["spécialisé code".to_string()],
        };
        let lines = render_ranking(&[entry], "pool/laguna:free");
        assert!(lines[1].contains("pool/laguna:free"));
        assert!(lines[1].contains("256k"));
        assert!(lines[1].ends_with("spécialisé code  ● actuel"));
    }
}
