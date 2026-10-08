//! Bascule automatique entre modèles : si le modèle principal est limité (429),
//! indisponible ou injoignable (serveur local arrêté), la requête part sur le
//! modèle suivant de la chaîne. Un modèle en échec est mis de côté un moment
//! pour ne pas le réessayer à chaque tour.

use std::collections::BTreeMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::client::{MessageStream, ProviderClient};
use crate::error::ApiError;
use crate::types::{MessageRequest, MessageResponse, StreamEvent};

/// Clé spéciale : quota quotidien des modèles gratuits `OpenRouter` épuisé
/// (il est commun à tous les modèles `:free`).
const OPENROUTER_FREE_KEY: &str = "openrouter:*:free";

static COOLDOWN: Mutex<BTreeMap<String, Instant>> = Mutex::new(BTreeMap::new());

fn with_cooldown<T>(action: impl FnOnce(&mut BTreeMap<String, Instant>) -> T) -> T {
    action(
        &mut COOLDOWN
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner),
    )
}

fn is_openrouter_free(model: &str) -> bool {
    model.contains('/') && model.ends_with(":free")
}

/// Met un modèle de côté pendant `duration`.
pub fn mark_model_unavailable(model: &str, duration: Duration) {
    with_cooldown(|map| {
        map.insert(model.to_string(), Instant::now() + duration);
    });
}

/// Faux si le modèle est encore mis de côté.
#[must_use]
pub fn model_available(model: &str) -> bool {
    with_cooldown(|map| {
        let now = Instant::now();
        map.retain(|_, until| *until > now);
        !map.contains_key(model)
            && !(is_openrouter_free(model) && map.contains_key(OPENROUTER_FREE_KEY))
    })
}

/// Oublie les mises de côté (tests, ou changement explicite de modèle).
pub fn reset_model_cooldowns() {
    with_cooldown(BTreeMap::clear);
}

fn error_text(error: &ApiError) -> String {
    error.to_string().to_ascii_lowercase()
}

impl ApiError {
    /// Erreur propre au modèle ou au serveur, qu'un autre modèle peut contourner :
    /// quota, surcharge, indisponibilité, serveur injoignable, clé absente.
    #[must_use]
    pub fn is_fallback_worthy(&self) -> bool {
        match self {
            Self::Api { status, .. } => {
                matches!(status.as_u16(), 402 | 404 | 408 | 409 | 429 | 500..=599)
                    || error_text(self).contains("rate limit")
            }
            Self::RetriesExhausted { last_error, .. } => last_error.is_fallback_worthy(),
            Self::Http(error) => error.is_connect() || error.is_timeout() || error.is_request(),
            Self::MissingCredentials { .. } | Self::Auth(_) | Self::ExpiredOAuthToken => true,
            Self::InvalidApiKeyEnv(_)
            | Self::Io(_)
            | Self::Json(_)
            | Self::InvalidSseFrame(_)
            | Self::BackoffOverflow { .. } => false,
        }
    }
}

/// Durée de mise de côté selon l'erreur.
fn cooldown_for(error: &ApiError) -> Duration {
    let text = error_text(error);
    if text.contains("per-day") || text.contains("per day") || text.contains("daily") {
        Duration::from_secs(3_600)
    } else if text.contains("429") || text.contains("rate limit") {
        Duration::from_secs(90)
    } else {
        Duration::from_secs(300)
    }
}

fn record_failure(model: &str, error: &ApiError) {
    let duration = cooldown_for(error);
    mark_model_unavailable(model, duration);
    let text = error_text(error);
    if is_openrouter_free(model)
        && (text.contains("free-models-per-day") || text.contains("per-day"))
    {
        mark_model_unavailable(OPENROUTER_FREE_KEY, duration);
    }
}

pub type ClientFactory = Box<dyn Fn(&str) -> Result<ProviderClient, ApiError> + Send>;

/// Chaîne de modèles : le principal puis ses remplaçants.
pub struct ModelChain {
    models: Vec<String>,
    clients: BTreeMap<String, ProviderClient>,
    factory: ClientFactory,
}

/// Flux ouvert sur un modèle de la chaîne, avec son premier événement déjà lu.
pub struct OpenedStream {
    pub model: String,
    pub stream: MessageStream,
    pub first_event: Option<StreamEvent>,
}

impl std::fmt::Debug for ModelChain {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ModelChain")
            .field("models", &self.models)
            .finish_non_exhaustive()
    }
}

impl ModelChain {
    #[must_use]
    pub fn new(models: Vec<String>, factory: ClientFactory) -> Self {
        let mut unique: Vec<String> = Vec::new();
        for model in models {
            let model = model.trim().to_string();
            if !model.is_empty() && !unique.contains(&model) {
                unique.push(model);
            }
        }
        Self {
            models: unique,
            clients: BTreeMap::new(),
            factory,
        }
    }

    /// Chaîne d'un seul modèle avec le client par défaut.
    #[must_use]
    pub fn single(model: &str) -> Self {
        Self::new(
            vec![model.to_string()],
            Box::new(ProviderClient::from_model),
        )
    }

    #[must_use]
    pub fn models(&self) -> &[String] {
        &self.models
    }

    #[must_use]
    pub fn primary(&self) -> &str {
        self.models.first().map_or("", String::as_str)
    }

    /// Ordre d'essai : les modèles disponibles d'abord, puis ceux mis de côté
    /// (mieux vaut réessayer que d'échouer sans rien tenter).
    fn attempt_order(&self) -> Vec<String> {
        let (available, cooling): (Vec<_>, Vec<_>) = self
            .models
            .iter()
            .cloned()
            .partition(|model| model_available(model));
        available.into_iter().chain(cooling).collect()
    }

    fn client(&mut self, model: &str) -> Result<ProviderClient, ApiError> {
        if let Some(client) = self.clients.get(model) {
            return Ok(client.clone());
        }
        let client = (self.factory)(model)?;
        self.clients.insert(model.to_string(), client.clone());
        Ok(client)
    }

    /// Ouvre un flux sur le premier modèle qui répond. `build` prépare la
    /// requête pour un modèle donné ; `notify` reçoit les bascules.
    pub async fn open_stream(
        &mut self,
        build: &(dyn Fn(&str) -> MessageRequest + Send + Sync),
        notify: &mut (dyn FnMut(&str) + Send),
    ) -> Result<OpenedStream, ApiError> {
        let order = self.attempt_order();
        let mut last_error = None;
        for (index, model) in order.iter().enumerate() {
            let is_last = index + 1 == order.len();
            let attempt = async {
                let client = self.client(model)?;
                let mut stream = client.stream_message(&build(model)).await?;
                // Certaines erreurs (quota OpenRouter) arrivent dans le flux.
                let first_event = stream.next_event().await?;
                Ok::<_, ApiError>((stream, first_event))
            }
            .await;
            match attempt {
                Ok((stream, first_event)) => {
                    return Ok(OpenedStream {
                        model: model.clone(),
                        stream,
                        first_event,
                    })
                }
                Err(error) if !is_last && error.is_fallback_worthy() => {
                    record_failure(model, &error);
                    notify(&switch_notice(model, &error, &order[index + 1]));
                    last_error = Some(error);
                }
                Err(error) => {
                    if error.is_fallback_worthy() {
                        record_failure(model, &error);
                    }
                    return Err(error);
                }
            }
        }
        Err(last_error.unwrap_or_else(|| ApiError::Auth("aucun modèle configuré".to_string())))
    }

    /// Requête sans streaming, avec la même bascule.
    pub async fn send(
        &mut self,
        build: &(dyn Fn(&str) -> MessageRequest + Send + Sync),
        notify: &mut (dyn FnMut(&str) + Send),
    ) -> Result<(String, MessageResponse), ApiError> {
        let order = self.attempt_order();
        let mut last_error = None;
        for (index, model) in order.iter().enumerate() {
            let is_last = index + 1 == order.len();
            let attempt = async {
                let client = self.client(model)?;
                client.send_message(&build(model)).await
            }
            .await;
            match attempt {
                Ok(response) => return Ok((model.clone(), response)),
                Err(error) if !is_last && error.is_fallback_worthy() => {
                    record_failure(model, &error);
                    notify(&switch_notice(model, &error, &order[index + 1]));
                    last_error = Some(error);
                }
                Err(error) => return Err(error),
            }
        }
        Err(last_error.unwrap_or_else(|| ApiError::Auth("aucun modèle configuré".to_string())))
    }
}

fn switch_notice(model: &str, error: &ApiError, next: &str) -> String {
    let reason = match error {
        ApiError::Api { status, .. } if status.as_u16() == 429 => "quota atteint (429)".to_string(),
        ApiError::RetriesExhausted { last_error, .. } => short_reason(last_error),
        other => short_reason(other),
    };
    format!("{model} indisponible ({reason}) → bascule sur {next}")
}

fn short_reason(error: &ApiError) -> String {
    match error {
        ApiError::Api { status, .. } => format!("erreur {}", status.as_u16()),
        ApiError::Http(error) if error.is_connect() => "serveur injoignable".to_string(),
        ApiError::Http(error) if error.is_timeout() => "délai dépassé".to_string(),
        ApiError::MissingCredentials { .. } | ApiError::Auth(_) => "clé manquante".to_string(),
        other => other.to_string().chars().take(80).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::{mark_model_unavailable, model_available, reset_model_cooldowns, ModelChain};
    use std::time::Duration;

    #[test]
    fn cooldown_skips_models_and_daily_free_limit_covers_all_free_models() {
        reset_model_cooldowns();
        assert!(model_available("qwen/qwen3-coder:free"));
        mark_model_unavailable("qwen/qwen3-coder:free", Duration::from_secs(60));
        assert!(!model_available("qwen/qwen3-coder:free"));
        assert!(model_available("deepseek/deepseek-chat:free"));
        mark_model_unavailable(super::OPENROUTER_FREE_KEY, Duration::from_secs(60));
        assert!(!model_available("deepseek/deepseek-chat:free"));
        assert!(
            model_available("ollama:qwen3"),
            "les autres fournisseurs restent disponibles"
        );
        reset_model_cooldowns();
    }

    #[test]
    fn chain_deduplicates_and_orders_available_models_first() {
        reset_model_cooldowns();
        let chain = ModelChain::new(
            vec![
                "a/x:free".into(),
                "b/y:free".into(),
                "a/x:free".into(),
                " ".into(),
            ],
            Box::new(crate::client::ProviderClient::from_model),
        );
        assert_eq!(chain.models(), ["a/x:free", "b/y:free"]);
        mark_model_unavailable("a/x:free", Duration::from_secs(60));
        assert_eq!(chain.attempt_order(), ["b/y:free", "a/x:free"]);
        reset_model_cooldowns();
    }
}
