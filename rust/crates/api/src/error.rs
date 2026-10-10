use std::env::VarError;
use std::fmt::{Display, Formatter};
use std::time::Duration;

#[derive(Debug)]
pub enum ApiError {
    MissingCredentials {
        provider: &'static str,
        env_vars: &'static [&'static str],
    },
    ExpiredOAuthToken,
    Auth(String),
    InvalidApiKeyEnv(VarError),
    Http(reqwest::Error),
    Io(std::io::Error),
    Json(serde_json::Error),
    Api {
        status: reqwest::StatusCode,
        error_type: Option<String>,
        message: Option<String>,
        body: String,
        retryable: bool,
    },
    RetriesExhausted {
        attempts: u32,
        last_error: Box<ApiError>,
    },
    InvalidSseFrame(&'static str),
    BackoffOverflow {
        attempt: u32,
        base_delay: Duration,
    },
}

impl ApiError {
    #[must_use]
    pub const fn missing_credentials(
        provider: &'static str,
        env_vars: &'static [&'static str],
    ) -> Self {
        Self::MissingCredentials { provider, env_vars }
    }

    #[must_use]
    pub fn is_retryable(&self) -> bool {
        match self {
            Self::Http(error) => error.is_connect() || error.is_timeout() || error.is_request(),
            Self::Api { retryable, .. } => *retryable,
            Self::RetriesExhausted { last_error, .. } => last_error.is_retryable(),
            Self::MissingCredentials { .. }
            | Self::ExpiredOAuthToken
            | Self::Auth(_)
            | Self::InvalidApiKeyEnv(_)
            | Self::Io(_)
            | Self::Json(_)
            | Self::InvalidSseFrame(_)
            | Self::BackoffOverflow { .. } => false,
        }
    }

    /// Délai avant la remise à zéro d'une limite de débit, annoncée par le
    /// serveur (`X-RateLimit-Reset` en millisecondes dans le corps d'erreur
    /// d'`OpenRouter`, ou `Retry-After` en secondes).
    #[must_use]
    pub fn rate_limit_reset(&self) -> Option<Duration> {
        let body = match self {
            Self::Api { body, .. } => body,
            Self::RetriesExhausted { last_error, .. } => return last_error.rate_limit_reset(),
            _ => return None,
        };
        let value: serde_json::Value = serde_json::from_str(body).ok()?;
        if let Some(headers) = value.pointer("/error/metadata/headers") {
            let header = |name: &str| {
                headers.as_object()?.iter().find_map(|(key, value)| {
                    key.eq_ignore_ascii_case(name).then(|| match value {
                        serde_json::Value::String(text) => text.trim().parse::<u64>().ok(),
                        other => other.as_u64(),
                    })?
                })
            };
            if let Some(reset_ms) = header("X-RateLimit-Reset") {
                let now_ms = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |elapsed| {
                        u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX)
                    });
                return Some(Duration::from_millis(reset_ms.saturating_sub(now_ms)));
            }
            if let Some(seconds) = header("Retry-After") {
                return Some(Duration::from_secs(seconds));
            }
        }
        google_retry_delay(&value)
    }
}

/// Délai qu'annonce Google (API Gemini) dans une erreur 429 : `retryDelay`
/// (« 37s ») dans les détails de l'erreur, sinon « retry in 12.5s » dans son
/// message.
fn google_retry_delay(value: &serde_json::Value) -> Option<Duration> {
    fn find(value: &serde_json::Value) -> Option<&str> {
        match value {
            serde_json::Value::Object(fields) => fields
                .get("retryDelay")
                .and_then(serde_json::Value::as_str)
                .or_else(|| fields.values().find_map(find)),
            serde_json::Value::Array(items) => items.iter().find_map(find),
            _ => None,
        }
    }
    fn seconds(text: &str) -> Option<Duration> {
        let number: String = text
            .trim()
            .chars()
            .take_while(|c| c.is_ascii_digit() || *c == '.')
            .collect();
        let seconds: f64 = number.parse().ok()?;
        (seconds.is_finite() && seconds >= 0.0).then(|| Duration::from_secs_f64(seconds))
    }
    if let Some(delay) = find(value).and_then(seconds) {
        return Some(delay);
    }
    let text = value.to_string().to_ascii_lowercase();
    let at = text.find("retry in ")?;
    seconds(&text[at + "retry in ".len()..])
}

impl Display for ApiError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingCredentials { provider, env_vars } => write!(
                f,
                "identifiants {provider} manquants ; définissez {} avant d'appeler l'API {provider}",
                env_vars.join(" ou ")
            ),
            Self::ExpiredOAuthToken => {
                write!(
                    f,
                    "le jeton OAuth sauvegardé est expiré et aucun jeton de rafraîchissement n'est disponible"
                )
            }
            Self::Auth(message) => write!(f, "erreur d'authentification : {message}"),
            Self::InvalidApiKeyEnv(error) => {
                write!(f, "échec de lecture de la variable d'environnement d'identifiants : {error}")
            }
            Self::Http(error) => write!(f, "erreur http : {error}"),
            Self::Io(error) => write!(f, "erreur d'E/S : {error}"),
            Self::Json(error) => write!(f, "erreur json : {error}"),
            Self::Api {
                status,
                error_type,
                message,
                body,
                ..
            } => match (error_type, message) {
                (Some(error_type), Some(message)) => {
                    write!(f, "l'api a retourné {status} ({error_type}) : {message}")
                }
                _ => write!(f, "l'api a retourné {status} : {body}"),
            },
            Self::RetriesExhausted {
                attempts,
                last_error,
            } => write!(f, "l'api a échoué après {attempts} tentatives : {last_error}"),
            Self::InvalidSseFrame(message) => write!(f, "trame sse invalide : {message}"),
            Self::BackoffOverflow {
                attempt,
                base_delay,
            } => write!(
                f,
                "dépassement du délai de réessai à la tentative {attempt} avec délai de base {base_delay:?}"
            ),
        }
    }
}

impl std::error::Error for ApiError {}

impl From<reqwest::Error> for ApiError {
    fn from(value: reqwest::Error) -> Self {
        Self::Http(value)
    }
}

impl From<std::io::Error> for ApiError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<serde_json::Error> for ApiError {
    fn from(value: serde_json::Error) -> Self {
        Self::Json(value)
    }
}

impl From<VarError> for ApiError {
    fn from(value: VarError) -> Self {
        Self::InvalidApiKeyEnv(value)
    }
}

#[cfg(test)]
mod tests {
    use super::ApiError;
    use std::time::Duration;

    fn rate_limited(body: &str) -> ApiError {
        ApiError::Api {
            status: reqwest::StatusCode::TOO_MANY_REQUESTS,
            error_type: None,
            message: None,
            body: body.to_string(),
            retryable: true,
        }
    }

    #[test]
    fn google_announces_its_wait_in_the_error_body() {
        // Réponse de l'API Gemini (compatible OpenAI) : une liste d'erreurs.
        let gemini = r#"[{"error":{"code":429,"message":"You exceeded your current quota.","status":"RESOURCE_EXHAUSTED","details":[{"@type":"type.googleapis.com/google.rpc.QuotaFailure"},{"@type":"type.googleapis.com/google.rpc.RetryInfo","retryDelay":"37s"}]}}]"#;
        assert_eq!(
            rate_limited(gemini).rate_limit_reset(),
            Some(Duration::from_secs(37))
        );
        let in_message =
            r#"{"error":{"code":429,"message":"Quota exceeded. Please retry in 12.5s."}}"#;
        assert_eq!(
            rate_limited(in_message).rate_limit_reset(),
            Some(Duration::from_millis(12_500))
        );
        assert_eq!(
            rate_limited(r#"{"error":{"code":429}}"#).rate_limit_reset(),
            None
        );
    }
}
