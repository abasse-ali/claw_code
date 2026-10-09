//! Crédits `OpenRouter` en temps réel : solde, dépense du jour et requêtes
//! gratuites restantes, pour le terminal (`/credits`, ligne après chaque
//! réponse), l'interface web et la barre d'état de VS Code.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use api::AccountStatus;

/// Les crédits ne bougent qu'après une requête : inutile d'interroger
/// `OpenRouter` plus souvent.
const CACHE_FOR: Duration = Duration::from_secs(15);
const NETWORK_TIMEOUT: Duration = Duration::from_secs(10);

static CACHE: Mutex<Option<(Instant, Result<AccountStatus, String>)>> = Mutex::new(None);

/// Requêtes gratuites faites par ce processus depuis la dernière lecture des
/// chiffres d'`OpenRouter` : décomptées tout de suite à l'écran, puis
/// confirmées par la lecture suivante.
static FREE_SINCE_FETCH: AtomicU64 = AtomicU64::new(0);
/// Requêtes `OpenRouter` de ce processus (gratuites, payantes).
static SESSION_FREE: AtomicU64 = AtomicU64::new(0);
static SESSION_PAID: AtomicU64 = AtomicU64::new(0);
/// Change à chaque requête notée ou lecture faite : l'affichage se met à jour.
static GENERATION: AtomicU64 = AtomicU64::new(0);
/// Heure (millisecondes Unix) de la dernière lecture réussie chez `OpenRouter`.
static LAST_READ_MS: AtomicU64 = AtomicU64::new(0);

fn is_free_model(model: &str) -> bool {
    model.ends_with(":free") || model == "openrouter/free"
}

/// Une requête vient de partir vers `model` (une étape de l'agent).
pub fn note_request(model: &str) {
    if !is_openrouter_model(model) {
        return;
    }
    if is_free_model(model) {
        FREE_SINCE_FETCH.fetch_add(1, Ordering::SeqCst);
        SESSION_FREE.fetch_add(1, Ordering::SeqCst);
    } else {
        SESSION_PAID.fetch_add(1, Ordering::SeqCst);
    }
    GENERATION.fetch_add(1, Ordering::SeqCst);
}

/// Numéro de version de l'affichage (requête notée ou chiffres relus).
#[must_use]
pub fn generation() -> u64 {
    GENERATION.load(Ordering::SeqCst)
}

fn openrouter_key() -> Option<String> {
    std::env::var("OPENROUTER_API_KEY")
        .ok()
        .filter(|key| !key.trim().is_empty())
}

/// Le modèle passe par `OpenRouter` (`org/modele`, sans préfixe `ollama:`…).
#[must_use]
pub fn is_openrouter_model(model: &str) -> bool {
    model.contains('/') && api::split_compat_model(model).is_none()
}

/// État du compte (mis en cache 15 s ; `fresh` force la lecture). `None` sans
/// clé `OpenRouter`.
pub fn account_status(fresh: bool) -> Option<Result<AccountStatus, String>> {
    let key = openrouter_key()?;
    let mut cache = CACHE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some((at, status)) = cache.as_ref() {
        if !fresh && at.elapsed() < CACHE_FOR {
            return Some(status.clone());
        }
    }
    // Les requêtes notées pendant la lecture ne sont peut-être pas encore
    // comptées par OpenRouter : seules celles d'avant sont soldées.
    let noted_before = FREE_SINCE_FETCH.load(Ordering::SeqCst);
    let status = fetch(&key);
    if status.is_ok() {
        let _ = FREE_SINCE_FETCH.fetch_update(Ordering::SeqCst, Ordering::SeqCst, |count| {
            Some(count.saturating_sub(noted_before))
        });
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |elapsed| {
                u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX)
            });
        LAST_READ_MS.store(now, Ordering::SeqCst);
    }
    *cache = Some((Instant::now(), status.clone()));
    GENERATION.fetch_add(1, Ordering::SeqCst);
    Some(status)
}

/// État avec les requêtes gratuites faites depuis la dernière lecture déjà
/// décomptées (affichage en temps réel).
fn live(status: &AccountStatus) -> AccountStatus {
    let pending = FREE_SINCE_FETCH.load(Ordering::SeqCst);
    let mut live = status.clone();
    if let Some(free) = live.free_requests.as_mut() {
        free.remaining = free.remaining.saturating_sub(pending);
        free.used += pending;
    }
    live
}

fn fetch(key: &str) -> Result<AccountStatus, String> {
    let base_url = api::read_openrouter_base_url();
    let request = async {
        tokio::time::timeout(NETWORK_TIMEOUT, api::fetch_account_status(&base_url, key)).await
    };
    // Appelable depuis un fil qui exécute déjà un runtime tokio (serveur web).
    let outcome = std::thread::scope(|scope| {
        scope
            .spawn(|| {
                tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .map_err(|error| error.to_string())
                    .map(|runtime| runtime.block_on(request))
            })
            .join()
            .unwrap_or_else(|_| Err("lecture des crédits interrompue".to_string()))
    })?;
    outcome
        .map_err(|_| {
            format!(
                "pas de réponse d'OpenRouter après {} s",
                NETWORK_TIMEOUT.as_secs()
            )
        })?
        .map_err(|error| error.to_string())
}

/// Montant en dollars à la française : `8,25 $`, `0,0021 $`.
#[must_use]
pub fn dollars(amount: f64) -> String {
    let text = if amount.abs() > 0.0 && amount.abs() < 0.01 {
        format!("{amount:.4}")
    } else {
        format!("{amount:.2}")
    };
    format!("{} $", text.replace('.', ","))
}

/// Résumé d'une ligne : « 8,25 $ restants · 678/1000 requêtes gratuites ».
#[must_use]
pub fn short_line(status: &AccountStatus) -> String {
    let mut parts = Vec::new();
    if let Some(balance) = status.balance() {
        parts.push(format!("{} restants", dollars(balance)));
    }
    if let Some(free) = status.free_requests {
        parts.push(format!(
            "{}/{} requêtes gratuites",
            free.remaining, free.limit
        ));
    }
    if parts.is_empty() {
        "crédits indisponibles pour cette clé".to_string()
    } else {
        parts.join(" · ")
    }
}

/// Ligne courte à jour (requêtes en attente de confirmation déjà décomptées).
#[must_use]
pub fn current_line(fresh: bool) -> Option<String> {
    match account_status(fresh)? {
        Ok(status) => Some(short_line(&live(&status))),
        Err(_) => None,
    }
}

/// Forme compacte tirée du cache seulement, sans requête réseau, pour la
/// ligne d'état sous la saisie : « 8,25 $ · 678/1000 gratuites ».
#[must_use]
pub fn cached_line() -> Option<String> {
    let cache = CACHE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some((_, Ok(status))) = cache.as_ref() else {
        return None;
    };
    let status = live(status);
    let mut parts = Vec::new();
    if let Some(balance) = status.balance() {
        parts.push(dollars(balance));
    }
    if let Some(free) = status.free_requests {
        parts.push(format!("{}/{} gratuites", free.remaining, free.limit));
    }
    (!parts.is_empty()).then(|| parts.join(" · "))
}

/// Rapport de `/credits`.
#[must_use]
pub fn report(fresh: bool) -> String {
    let status = match account_status(fresh) {
        None => {
            return "Crédits OpenRouter : aucune clé (définissez OPENROUTER_API_KEY).".to_string()
        }
        Some(Err(error)) => return format!("Crédits OpenRouter indisponibles : {error}"),
        Some(Ok(status)) => live(&status),
    };
    let mut lines = vec!["Crédits OpenRouter".to_string()];
    if let (Some(balance), Some(total), Some(used)) =
        (status.balance(), status.credits_total, status.credits_used)
    {
        lines.push(format!("  Solde            {}", dollars(balance)));
        lines.push(format!(
            "  Utilisés         {} sur {} achetés",
            dollars(used),
            dollars(total)
        ));
    }
    if let Some(today) = status.key_usage_today {
        lines.push(format!("  Aujourd'hui      {} (cette clé)", dollars(today)));
    }
    if let Some(remaining) = status.key_limit_remaining {
        lines.push(format!(
            "  Plafond de clé   {} restants",
            dollars(remaining)
        ));
    }
    if let Some(free) = status.free_requests {
        lines.push(format!(
            "  Modèles gratuits {}/{} requêtes restantes aujourd'hui (remise à zéro à minuit UTC)",
            free.remaining, free.limit
        ));
    }
    let (free, paid) = session_requests();
    if free + paid > 0 {
        lines.push(format!(
            "  Cette session    {free} requête(s) gratuite(s), {paid} payante(s)"
        ));
    }
    lines.push("  Achat de crédits https://openrouter.ai/settings/credits".to_string());
    lines.join("\n")
}

fn session_requests() -> (u64, u64) {
    (
        SESSION_FREE.load(Ordering::SeqCst),
        SESSION_PAID.load(Ordering::SeqCst),
    )
}

/// Données pour l'interface web et la barre d'état de VS Code, avec les
/// requêtes faites depuis la dernière lecture déjà décomptées.
#[must_use]
pub fn json(fresh: bool) -> serde_json::Value {
    let (session_free, session_paid) = session_requests();
    match account_status(fresh) {
        None => serde_json::json!({ "available": false, "reason": "aucune clé OpenRouter" }),
        Some(Err(error)) => serde_json::json!({ "available": false, "reason": error }),
        Some(Ok(status)) => {
            let status = live(&status);
            serde_json::json!({
                "available": true,
                "balance": status.balance(),
                "balance_text": status.balance().map(dollars),
                "used": status.credits_used,
                "total": status.credits_total,
                "today": status.key_usage_today,
                "free": status.free_requests.map(|free| serde_json::json!({
                    "used": free.used,
                    "limit": free.limit,
                    "remaining": free.remaining,
                })),
                "session": { "free": session_free, "paid": session_paid },
                // Requêtes décomptées en attendant la confirmation d'OpenRouter.
                "pending": FREE_SINCE_FETCH.load(Ordering::SeqCst),
                // Dernière lecture des chiffres chez OpenRouter (ms Unix).
                "read_at": LAST_READ_MS.load(Ordering::SeqCst),
                "text": short_line(&status),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{dollars, is_openrouter_model, short_line};
    use api::{AccountStatus, FreeModelQuota};

    #[test]
    fn formats_amounts_and_the_one_line_summary() {
        assert_eq!(dollars(8.245_330_25), "8,25 $");
        assert_eq!(dollars(0.002_1), "0,0021 $");
        assert_eq!(dollars(0.0), "0,00 $");
        let status = AccountStatus {
            credits_total: Some(10.0),
            credits_used: Some(1.754_669_75),
            key_usage_today: Some(0.0),
            key_limit_remaining: None,
            free_requests: Some(FreeModelQuota {
                used: 322,
                limit: 1000,
                remaining: 678,
            }),
        };
        assert_eq!(
            short_line(&status),
            "8,25 $ restants · 678/1000 requêtes gratuites"
        );
    }

    #[test]
    fn recognizes_openrouter_models() {
        assert!(is_openrouter_model("cohere/north-mini-code:free"));
        assert!(is_openrouter_model("anthropic/claude-sonnet-4"));
        assert!(!is_openrouter_model("ollama:qwen3.5:4b"));
        assert!(!is_openrouter_model("claude-opus-4-6"));
    }
}
