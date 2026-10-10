//! Bascule automatique entre modèles : si le modèle principal est limité (429),
//! indisponible ou injoignable (serveur local arrêté), la requête part sur le
//! modèle suivant de la chaîne. Un modèle en échec est mis de côté un moment
//! pour ne pas le réessayer à chaque tour.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::client::{MessageStream, ProviderClient};
use crate::error::ApiError;
use crate::types::{MessageRequest, MessageResponse, StreamEvent};

/// Clé spéciale : quota quotidien des modèles gratuits `OpenRouter` épuisé
/// (il est commun à tous les modèles `:free`).
const OPENROUTER_FREE_KEY: &str = "openrouter:*:free";

/// Mises de côté assez longues pour être gardées d'un lancement à l'autre
/// (quota du jour épuisé, modèle refusé à Claw).
const PERSISTED_COOLDOWN: Duration = Duration::from_secs(600);

/// Réessais sur place d'un modèle surchargé (erreur arrivée dans le flux).
const IN_PLACE_RETRIES: u32 = 2;

static COOLDOWN: Mutex<BTreeMap<String, Instant>> = Mutex::new(BTreeMap::new());
static COOLDOWN_STORE: Mutex<Option<PathBuf>> = Mutex::new(None);

fn now_epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

/// Secondes jusqu'à minuit à Los Angeles, où Google remet ses quotas du jour
/// à zéro : UTC-7 à l'heure d'été (du deuxième dimanche de mars au premier
/// dimanche de novembre, à 2 h), UTC-8 sinon. Soit 9 h à Paris.
fn secs_until_pacific_midnight(now: u64) -> u64 {
    const DAY: i64 = 86_400;
    let now = i64::try_from(now).unwrap_or(i64::MAX / 2);
    let year = year_of_day(now.div_euclid(DAY));
    let sunday_from = |day: i64| day + (7 - (day + 4).rem_euclid(7)) % 7;
    let summer_start = (sunday_from(days_from_civil(year, 3, 1)) + 7) * DAY + 10 * 3_600;
    let summer_end = sunday_from(days_from_civil(year, 11, 1)) * DAY + 9 * 3_600;
    let offset = if (summer_start..summer_end).contains(&now) {
        7
    } else {
        8
    };
    let local = now - offset * 3_600;
    u64::try_from(DAY - local.rem_euclid(DAY)).unwrap_or(0)
}

/// Jours écoulés depuis le 1er janvier 1970 à une date du calendrier.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let day_of_year = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

/// Année d'un jour compté depuis le 1er janvier 1970.
fn year_of_day(days: i64) -> i64 {
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    // Les mois sont comptés à partir de mars : janvier et février (10 et 11)
    // appartiennent à l'année suivante.
    let month_from_march = (5 * day_of_year + 2) / 153;
    year_of_era + era * 400 + i64::from(month_from_march >= 10)
}

/// Fichier où garder les longues mises de côté : celles encore valides sont
/// rechargées, les suivantes y sont enregistrées.
pub fn set_cooldown_store(path: PathBuf) {
    if let Some(entries) = std::fs::read_to_string(&path)
        .ok()
        .and_then(|text| serde_json::from_str::<BTreeMap<String, u64>>(&text).ok())
    {
        let now = now_epoch_secs();
        with_cooldown(|map| {
            for (model, until) in entries.into_iter().filter(|(_, until)| *until > now) {
                map.insert(model, Instant::now() + Duration::from_secs(until - now));
            }
        });
    }
    *COOLDOWN_STORE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(path);
}

fn persist_long_cooldowns() {
    let Some(path) = COOLDOWN_STORE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
    else {
        return;
    };
    let (now, epoch) = (Instant::now(), now_epoch_secs());
    let entries: BTreeMap<String, u64> = with_cooldown(|map| {
        map.iter()
            .map(|(model, until)| (model, until.saturating_duration_since(now)))
            .filter(|(_, left)| *left >= PERSISTED_COOLDOWN)
            .map(|(model, left)| (model.clone(), epoch + left.as_secs()))
            .collect()
    });
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(text) = serde_json::to_string_pretty(&entries) {
        let _ = std::fs::write(path, text);
    }
}

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
        !(map.contains_key(model)
            || is_openrouter_free(model) && map.contains_key(OPENROUTER_FREE_KEY))
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
            // 403 : modèle réservé à certaines applications (« only available on
            // agentic harnesses ») ou interdit à cette clé.
            Self::Api { status, .. } => {
                matches!(
                    status.as_u16(),
                    402 | 403 | 404 | 408 | 409 | 429 | 500..=599
                ) || error_text(self).contains("rate limit")
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

    /// Réponse arrivée coupée ou illisible (corps tronqué, JSON invalide).
    #[must_use]
    pub fn is_broken_response(&self) -> bool {
        match self {
            Self::Http(error) => error.is_body() || error.is_decode(),
            Self::Json(_) | Self::InvalidSseFrame(_) => true,
            _ => false,
        }
    }
}

fn api_status(error: &ApiError) -> Option<u16> {
    match error {
        ApiError::Api { status, .. } => Some(status.as_u16()),
        ApiError::RetriesExhausted { last_error, .. } => api_status(last_error),
        _ => None,
    }
}

/// Durée de mise de côté selon l'erreur : jusqu'à la remise à zéro annoncée
/// par le serveur, sinon selon le type d'erreur.
fn cooldown_for(error: &ApiError) -> Duration {
    let text = error_text(error);
    if let Some(reset) = error.rate_limit_reset() {
        reset.max(Duration::from_secs(30))
    } else if api_status(error) == Some(403) {
        Duration::from_secs(24 * 3_600)
    } else if text.contains("perday") {
        // Les quotas du jour de Google repartent à minuit, heure du Pacifique.
        Duration::from_secs(secs_until_pacific_midnight(now_epoch_secs()) + 60)
    } else if text.contains("per-day") || text.contains("per day") || text.contains("daily") {
        // Les quotas du jour d'OpenRouter repartent à minuit UTC.
        Duration::from_secs(86_400 - now_epoch_secs() % 86_400 + 60)
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
    if duration >= PERSISTED_COOLDOWN {
        persist_long_cooldowns();
    }
}

/// Erreur passagère arrivée dans le flux (surcharge du fournisseur) : le même
/// modèle peut répondre quelques secondes plus tard. Les erreurs HTTP sont déjà
/// réessayées par le client (`RetriesExhausted`).
fn retry_in_place_delay(error: &ApiError, attempt: u32) -> Option<Duration> {
    let ApiError::Api { status, .. } = error else {
        return None;
    };
    if !error.is_retryable() || attempt > IN_PLACE_RETRIES {
        return None;
    }
    match error.rate_limit_reset() {
        Some(reset) if reset <= Duration::from_secs(70) => Some(reset + Duration::from_millis(500)),
        // Quota du jour épuisé : le même modèle ne répondra pas avant demain.
        None if error.is_daily_quota() => None,
        None if status.as_u16() == 429 || status.is_server_error() => {
            Some(Duration::from_secs(2 * u64::from(attempt)))
        }
        // Remise à zéro lointaine, ou erreur qui n'annonce aucun délai.
        Some(_) | None => None,
    }
}

/// Changements de modèle, pour une même requête, après une panne du
/// fournisseur en cours de réponse.
pub const UPSTREAM_SWITCHES: u32 = 3;
/// Mise de côté du modèle dont le fournisseur a lâché en cours de réponse.
const UPSTREAM_COOLDOWN: Duration = Duration::from_secs(300);

/// Statut d'une panne du fournisseur lue dans le texte d'une erreur (délai
/// dépassé, surcharge, erreur interne, limite) : un autre modèle peut répondre
/// à la même requête. `None` pour une erreur de la requête elle-même (400,
/// 401…) ou sans statut.
#[must_use]
pub fn upstream_failure_status(error: &str) -> Option<u16> {
    const MARKER: &str = "l'api a retourné ";
    let after = &error[error.rfind(MARKER)? + MARKER.len()..];
    let status: u16 = after.get(..3)?.parse().ok()?;
    matches!(status, 408 | 429 | 500..=599).then_some(status)
}

/// Raison lisible d'un échec propre au modèle ou à son fournisseur, lue dans
/// le texte d'une erreur : un autre modèle pourrait répondre, ou le même un
/// peu plus tard. `None` pour une erreur de la requête elle-même (400…).
#[must_use]
pub fn model_failure_reason(error: &str) -> Option<String> {
    const MARKER: &str = "l'api a retourné ";
    if let Some(position) = error.rfind(MARKER) {
        let status: u16 = error[position + MARKER.len()..].get(..3)?.parse().ok()?;
        return Some(match status {
            401 => "clé refusée (erreur 401)".to_string(),
            402 => "crédits insuffisants (erreur 402)".to_string(),
            403 => "modèle refusé à cette clé (erreur 403)".to_string(),
            404 => not_found_reason(error).to_string(),
            429 => "quota ou limite de requêtes atteinte (erreur 429)".to_string(),
            408 | 409 | 500..=599 => format!("le fournisseur ne répond pas (erreur {status})"),
            _ => return None,
        });
    }
    if error.contains("identifiants") && error.contains("manquants") {
        Some("clé manquante".to_string())
    } else if error.contains("erreur d'authentification") {
        Some("clé refusée".to_string())
    } else if error.contains("erreur http :") {
        Some("serveur injoignable ou délai dépassé".to_string())
    } else {
        None
    }
}

/// Ce que veut dire une erreur 404. `OpenRouter` répond aussi 404 quand le
/// modèle existe mais qu'aucun de ses fournisseurs n'accepte la requête.
fn not_found_reason(error: &str) -> &'static str {
    let lower = error.to_ascii_lowercase();
    if !lower.contains("no endpoints found") {
        "modèle introuvable (erreur 404)"
    } else if lower.contains("image") {
        "ce modèle ne lit pas les images, et la conversation en contient (erreur 404)"
    } else if lower.contains("tool") {
        "ce modèle n'accepte pas les appels d'outils (erreur 404)"
    } else if lower.contains("data policy") {
        "vos réglages de confidentialité OpenRouter écartent ce modèle (erreur 404)"
    } else {
        "aucun fournisseur de ce modèle n'accepte cette requête (erreur 404)"
    }
}

/// L'échec lu dans le texte d'une erreur est passager : le même modèle peut
/// répondre un peu plus tard (panne ou surcharge du fournisseur, limite par
/// minute, réseau). Faux pour un quota du jour épuisé, une clé refusée ou un
/// modèle introuvable : réessayer n'y changerait rien.
#[must_use]
pub fn failure_is_transient(error: &str) -> bool {
    const MARKER: &str = "l'api a retourné ";
    let lower = error.to_ascii_lowercase();
    if let Some(position) = error.rfind(MARKER) {
        let status: Option<u16> = error[position + MARKER.len()..]
            .get(..3)
            .and_then(|code| code.parse().ok());
        return match status {
            Some(429) => {
                // « PerDay » : quota du jour de Google (…RequestsPerDayPerProject…).
                !(lower.contains("per-day")
                    || lower.contains("per day")
                    || lower.contains("perday")
                    || lower.contains("daily"))
            }
            Some(408 | 409 | 500..=599) => true,
            _ => false,
        };
    }
    error.contains("erreur http :")
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

    /// Le fournisseur de `failed` a lâché en cours de réponse : s'il reste un
    /// autre modèle disponible, `failed` est mis de côté quelques minutes (les
    /// étapes suivantes ne l'attendent plus) et ce modèle est renvoyé.
    #[must_use]
    pub fn switch_after_failure(&self, failed: &str) -> Option<String> {
        let next = self
            .models
            .iter()
            .find(|model| model.as_str() != failed && model_available(model))
            .cloned()?;
        mark_model_unavailable(failed, UPSTREAM_COOLDOWN);
        Some(next)
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
            let mut tries = 0;
            let attempt = loop {
                tries += 1;
                let attempt = async {
                    let client = self.client(model)?;
                    let mut stream = client.stream_message(&build(model)).await?;
                    // Certaines erreurs (quota, surcharge) arrivent dans le flux.
                    let first_event = stream.next_event().await?;
                    Ok::<_, ApiError>((stream, first_event))
                }
                .await;
                match &attempt {
                    Err(error) => match retry_in_place_delay(error, tries) {
                        Some(delay) => tokio::time::sleep(delay).await,
                        None => break attempt,
                    },
                    Ok(_) => break attempt,
                }
            };
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
                // Réponse coupée ou illisible : le modèle suivant peut répondre,
                // sans mettre celui-ci de côté pour autant.
                Err(error) if !is_last && error.is_broken_response() => {
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
    fn a_404_from_openrouter_is_named_for_what_it_is() {
        // OpenRouter répond 404 quand le modèle existe mais qu'aucun de ses
        // fournisseurs n'accepte la requête : ce n'est pas « modèle introuvable ».
        for (message, reason) in [
            (
                "No endpoints found that support image input",
                "ce modèle ne lit pas les images, et la conversation en contient (erreur 404)",
            ),
            (
                "No endpoints found that support tool use",
                "ce modèle n'accepte pas les appels d'outils (erreur 404)",
            ),
            (
                "No endpoints found matching your data policy",
                "vos réglages de confidentialité OpenRouter écartent ce modèle (erreur 404)",
            ),
            (
                "No endpoints found for acme/model",
                "aucun fournisseur de ce modèle n'accepte cette requête (erreur 404)",
            ),
            (
                "The model `acme/missing` does not exist",
                "modèle introuvable (erreur 404)",
            ),
        ] {
            assert_eq!(
                super::model_failure_reason(&format!("l'api a retourné 404 Not Found : {message}"))
                    .as_deref(),
                Some(reason)
            );
        }
    }

    #[test]
    fn a_failure_in_the_middle_of_an_answer_hands_over_to_the_next_model() {
        use super::{
            failure_is_transient as transient, model_failure_reason, upstream_failure_status,
        };
        assert_eq!(
            upstream_failure_status(
                "l'api a retourné 504 Gateway Timeout (504) : Upstream idle timeout exceeded"
            ),
            Some(504)
        );
        assert_eq!(
            upstream_failure_status(
                "l'api a échoué après 3 tentatives : l'api a retourné 503 Service Unavailable : overloaded"
            ),
            Some(503)
        );
        assert_eq!(
            upstream_failure_status("l'api a retourné 429 Too Many Requests : rate limit"),
            Some(429)
        );
        // Erreurs de la requête elle-même, ou sans statut : pas de changement de modèle.
        assert_eq!(
            upstream_failure_status("l'api a retourné 400 Bad Request : invalid tool schema"),
            None
        );
        // La même lecture, en clair, pour la question posée à l'utilisateur.
        assert_eq!(
            model_failure_reason(
                "l'api a retourné 504 Gateway Timeout (504) : Upstream idle timeout exceeded"
            )
            .as_deref(),
            Some("le fournisseur ne répond pas (erreur 504)")
        );
        assert_eq!(
            model_failure_reason("l'api a retourné 429 Too Many Requests : free-models-per-day")
                .as_deref(),
            Some("quota ou limite de requêtes atteinte (erreur 429)")
        );
        assert_eq!(
            model_failure_reason(
                "identifiants OpenRouter manquants ; définissez OPENROUTER_API_KEY avant d'appeler l'API OpenRouter"
            )
            .as_deref(),
            Some("clé manquante")
        );
        assert_eq!(
            model_failure_reason("l'api a retourné 400 Bad Request : invalid tool schema"),
            None
        );
        assert_eq!(model_failure_reason("erreur json : EOF"), None);
        // Passager (le même modèle peut réessayer) ou non.
        assert!(transient(
            "l'api a retourné 504 Gateway Timeout (504) : Upstream idle timeout exceeded"
        ));
        assert!(transient(
            "l'api a retourné 429 Too Many Requests : rate limit per minute"
        ));
        assert!(transient("erreur http : connection reset"));
        assert!(!transient(
            "l'api a retourné 429 Too Many Requests : Rate limit exceeded: free-models-per-day"
        ));
        assert!(!transient("l'api a retourné 403 Forbidden : not available"));
        assert!(!transient(
            "identifiants OpenRouter manquants ; définissez OPENROUTER_API_KEY avant d'appeler l'API OpenRouter"
        ));
        assert_eq!(
            upstream_failure_status("l'api a retourné 401 Unauthorized : User not found"),
            None
        );
        assert_eq!(upstream_failure_status("erreur json : EOF"), None);

        reset_model_cooldowns();
        let chain = ModelChain::new(
            vec!["a/lent:free".into(), "b/suivant:free".into()],
            Box::new(crate::client::ProviderClient::from_model),
        );
        assert_eq!(
            chain.switch_after_failure("a/lent:free").as_deref(),
            Some("b/suivant:free")
        );
        assert!(!model_available("a/lent:free"));
        assert_eq!(chain.attempt_order(), ["b/suivant:free", "a/lent:free"]);
        // Plus personne d'autre de disponible : le modèle n'est pas mis de côté.
        assert_eq!(chain.switch_after_failure("b/suivant:free"), None);
        assert!(model_available("b/suivant:free"));
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

    fn api_error(status: u16, body: &str) -> crate::error::ApiError {
        crate::error::ApiError::Api {
            status: reqwest::StatusCode::from_u16(status).expect("statut"),
            error_type: None,
            message: None,
            body: body.to_string(),
            retryable: matches!(status, 429 | 500..=599),
        }
    }

    #[test]
    fn restricted_and_rate_limited_models_are_set_aside_for_the_right_time() {
        let restricted = api_error(
            403,
            r#"{"error":{"message":"inkling:free is only available on agentic harnesses."}}"#,
        );
        assert!(restricted.is_fallback_worthy());
        assert_eq!(
            super::cooldown_for(&restricted),
            Duration::from_secs(24 * 3_600)
        );

        let reset_ms = (super::now_epoch_secs() + 40) * 1_000;
        let per_minute = api_error(
            429,
            &format!(
                r#"{{"error":{{"metadata":{{"headers":{{"X-RateLimit-Reset":"{reset_ms}"}}}}}}}}"#
            ),
        );
        let wait = super::cooldown_for(&per_minute);
        assert!(
            wait <= Duration::from_secs(40) && wait >= Duration::from_secs(30),
            "{wait:?}"
        );

        // Surcharge dans le flux : réessayée sur place, deux fois au plus.
        let overloaded = api_error(503, "{}");
        assert_eq!(
            super::retry_in_place_delay(&overloaded, 1),
            Some(Duration::from_secs(2))
        );
        assert_eq!(super::retry_in_place_delay(&overloaded, 3), None);
        // Déjà réessayée par le client HTTP, ou quota du jour : on bascule.
        let exhausted = crate::error::ApiError::RetriesExhausted {
            attempts: 3,
            last_error: Box::new(api_error(503, "{}")),
        };
        assert_eq!(super::retry_in_place_delay(&exhausted, 1), None);
        let tomorrow_ms = (super::now_epoch_secs() + 7_200) * 1_000;
        let per_day = api_error(
            429,
            &format!(
                r#"{{"error":{{"metadata":{{"headers":{{"X-RateLimit-Reset":"{tomorrow_ms}"}}}}}}}}"#
            ),
        );
        assert_eq!(super::retry_in_place_delay(&per_day, 1), None);
    }

    #[test]
    fn a_google_daily_quota_waits_for_midnight_in_california() {
        use super::{days_from_civil, secs_until_pacific_midnight};
        let at = |year: i64, month: i64, day: i64, hour: i64| {
            u64::try_from(days_from_civil(year, month, day) * 86_400 + hour * 3_600)
                .expect("date après 1970")
        };
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        // Été : midi UTC, 5 h à Los Angeles, minuit dans 19 h.
        assert_eq!(secs_until_pacific_midnight(at(2026, 7, 1, 12)), 19 * 3_600);
        // Hiver : midi UTC, 4 h à Los Angeles, minuit dans 20 h.
        assert_eq!(secs_until_pacific_midnight(at(2026, 1, 15, 12)), 20 * 3_600);
        // Passage à l'heure d'été le 8 mars 2026 à 10 h UTC.
        assert_eq!(
            secs_until_pacific_midnight(at(2026, 3, 8, 10) - 1),
            22 * 3_600 + 1
        );
        assert_eq!(secs_until_pacific_midnight(at(2026, 3, 8, 10)), 21 * 3_600);
        // Le quota du jour de Google, qui joint pourtant un court retryDelay :
        // ni réessai sur place, ni mise de côté de 47 s.
        let daily = api_error(
            429,
            r#"[{"error":{"code":429,"details":[{"@type":"type.googleapis.com/google.rpc.QuotaFailure","violations":[{"quotaId":"GenerateRequestsPerDayPerProjectPerModel-FreeTier"}]},{"@type":"type.googleapis.com/google.rpc.RetryInfo","retryDelay":"47s"}]}}]"#,
        );
        assert_eq!(super::retry_in_place_delay(&daily, 1), None);
        assert!(super::cooldown_for(&daily) > Duration::from_secs(60));
    }

    #[test]
    fn long_cooldowns_survive_a_restart() {
        reset_model_cooldowns();
        let store = std::env::temp_dir().join(format!(
            "claw-cooldowns-{}-{}.json",
            std::process::id(),
            super::now_epoch_secs()
        ));
        super::set_cooldown_store(store.clone());
        super::record_failure(
            "thinkingmachines/inkling:free",
            &api_error(
                403,
                r#"{"error":{"message":"only available on agentic harnesses"}}"#,
            ),
        );
        super::record_failure("court/modele:free", &api_error(503, "{}"));
        // Nouveau lancement : seule la longue mise de côté revient.
        reset_model_cooldowns();
        assert!(model_available("thinkingmachines/inkling:free"));
        super::set_cooldown_store(store.clone());
        assert!(!model_available("thinkingmachines/inkling:free"));
        assert!(model_available("court/modele:free"));
        *super::COOLDOWN_STORE
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
        reset_model_cooldowns();
        let _ = std::fs::remove_file(store);
    }
}
