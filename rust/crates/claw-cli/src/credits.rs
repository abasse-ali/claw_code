//! Crédits `OpenRouter` en temps réel : solde, dépense du jour et requêtes
//! gratuites restantes, pour le terminal (`/credits`, ligne après chaque
//! réponse), l'interface web et la barre d'état de VS Code.

use std::collections::BTreeMap;
use std::io::Write as _;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
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
    log_request(model);
    GENERATION.fetch_add(1, Ordering::SeqCst);
}

// --- Journal des requêtes du jour ---------------------------------------------------

/// En dessous, le quota gratuit du jour est signalé comme bas.
pub const LOW_FREE_REQUESTS: u64 = 100;
/// Jours de journal gardés.
const USAGE_KEPT_DAYS: u64 = 14;
/// Les vieux journaux sont retirés une fois par lancement.
static USAGE_CLEANED: OnceLock<()> = OnceLock::new();

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

fn usage_dir() -> Option<PathBuf> {
    let cwd = std::env::current_dir().ok()?;
    Some(
        runtime::ConfigLoader::default_for(cwd)
            .config_home()
            .join("usage"),
    )
}

/// Journal d'un jour UTC (le quota gratuit d'`OpenRouter` repart à minuit UTC).
fn usage_path(day: u64) -> Option<PathBuf> {
    Some(usage_dir()?.join(format!("jour-{day}.jsonl")))
}

/// Ajoute la requête au journal du jour, commun à tous les Claw de la machine
/// (terminal, interface web, sous-agents, mesures) : une ligne par requête.
fn log_request(model: &str) {
    if cfg!(test) {
        return;
    }
    let now = now_secs();
    let Some(path) = usage_path(now / 86_400) else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    USAGE_CLEANED.get_or_init(|| remove_old_usage(now / 86_400));
    let line = format!("{}\n", serde_json::json!({ "t": now, "m": model }));
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = file.write_all(line.as_bytes());
    }
}

fn remove_old_usage(today: u64) {
    let Some(entries) = usage_dir().and_then(|dir| std::fs::read_dir(dir).ok()) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        let day = name
            .strip_prefix("jour-")
            .and_then(|rest| rest.strip_suffix(".jsonl"))
            .and_then(|day| day.parse::<u64>().ok());
        if day.is_some_and(|day| day + USAGE_KEPT_DAYS < today) {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

/// Requêtes de Claw aujourd'hui (UTC), lues dans le journal.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DayUsage {
    /// (modèle, requêtes), du plus utilisé au moins utilisé.
    pub by_model: Vec<(String, u64)>,
    /// Heures (secondes Unix) des requêtes vers des modèles gratuits.
    free_times: Vec<u64>,
}

impl DayUsage {
    fn parse(journal: &str) -> Self {
        let mut counts: BTreeMap<String, u64> = BTreeMap::new();
        let mut free_times = Vec::new();
        for line in journal.lines() {
            let Ok(entry) = serde_json::from_str::<serde_json::Value>(line) else {
                continue;
            };
            let Some(model) = entry.get("m").and_then(serde_json::Value::as_str) else {
                continue;
            };
            *counts.entry(model.to_string()).or_default() += 1;
            if is_free_model(model) {
                free_times.extend(entry.get("t").and_then(serde_json::Value::as_u64));
            }
        }
        let mut by_model: Vec<(String, u64)> = counts.into_iter().collect();
        by_model.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
        Self {
            by_model,
            free_times,
        }
    }

    #[must_use]
    pub fn total(&self) -> u64 {
        self.by_model.iter().map(|(_, requests)| requests).sum()
    }

    /// Rythme récent des requêtes gratuites, par heure, mesuré sur la dernière
    /// heure ; `None` avec trop peu de recul (moins de 10 minutes ou de 5 requêtes).
    #[must_use]
    pub fn free_per_hour(&self, now: u64) -> Option<f64> {
        let first = *self.free_times.iter().min()?;
        let window = now.saturating_sub(first).min(3_600);
        let recent = self
            .free_times
            .iter()
            .filter(|time| now.saturating_sub(**time) <= window)
            .count();
        if window < 600 || recent < 5 {
            return None;
        }
        #[allow(clippy::cast_precision_loss)]
        Some(recent as f64 * 3_600.0 / window as f64)
    }
}

fn day_usage() -> DayUsage {
    usage_path(now_secs() / 86_400)
        .and_then(|path| std::fs::read_to_string(path).ok())
        .map(|journal| DayUsage::parse(&journal))
        .unwrap_or_default()
}

/// Heure (secondes Unix) à laquelle le quota gratuit sera épuisé à ce rythme,
/// si c'est avant sa remise à zéro de minuit UTC.
fn exhausted_at(remaining: u64, per_hour: f64, now: u64) -> Option<u64> {
    if per_hour <= 0.0 {
        return None;
    }
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss
    )]
    let seconds = (remaining as f64 / per_hour * 3_600.0) as u64;
    let at = now.saturating_add(seconds);
    let reset = (now / 86_400 + 1) * 86_400;
    (at < reset).then_some(at)
}

/// « 2 h 10 » ou « 25 min ».
fn duration_text(seconds: u64) -> String {
    let minutes = seconds / 60;
    if minutes >= 60 {
        format!("{} h {:02}", minutes / 60, minutes % 60)
    } else {
        format!("{} min", minutes.max(1))
    }
}

/// Phrase sur le rythme : épuisement prévu, ou quota suffisant jusqu'à minuit UTC.
fn pace_sentence(remaining: u64, per_hour: f64, now: u64) -> String {
    match exhausted_at(remaining, per_hour, now) {
        Some(at) => format!(
            "à ce rythme (≈ {per_hour:.0} requêtes/h), quota gratuit épuisé dans environ {}",
            duration_text(at.saturating_sub(now))
        ),
        None => format!(
            "à ce rythme (≈ {per_hour:.0} requêtes/h), le quota gratuit tient jusqu'à sa remise à zéro"
        ),
    }
}

/// Avertissement quand le quota gratuit passe sous 100, 20 puis 0 requêtes :
/// rendu une fois par seuil (terminal, après une réponse). Sans requête réseau.
#[must_use]
pub fn low_quota_warning() -> Option<String> {
    /// Dernier seuil déjà signalé (`u64::MAX` : aucun).
    static WARNED: AtomicU64 = AtomicU64::new(u64::MAX);
    let remaining = {
        let cache = CACHE
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let Some((_, Ok(status))) = cache.as_ref() else {
            return None;
        };
        live(status).free_requests?.remaining
    };
    let threshold = low_quota_threshold(remaining);
    let Some(threshold) = threshold else {
        // Quota remonté (nouveau jour) : les seuils pourront resservir.
        WARNED.store(u64::MAX, Ordering::SeqCst);
        return None;
    };
    if WARNED.swap(threshold, Ordering::SeqCst) <= threshold {
        return None;
    }
    let mut message = if remaining == 0 {
        "Quota gratuit du jour épuisé (remise à zéro à minuit UTC)".to_string()
    } else {
        format!("Il reste {remaining} requêtes gratuites aujourd'hui")
    };
    let now = now_secs();
    if let Some(per_hour) = day_usage().free_per_hour(now).filter(|_| remaining > 0) {
        message.push_str(" ; ");
        message.push_str(&pace_sentence(remaining, per_hour, now));
    }
    Some(message)
}

/// Seuil d'alerte atteint par ce nombre de requêtes restantes : 100, 20 ou 0.
fn low_quota_threshold(remaining: u64) -> Option<u64> {
    [0, 20, LOW_FREE_REQUESTS]
        .into_iter()
        .find(|threshold| remaining <= *threshold && (remaining < LOW_FREE_REQUESTS))
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

/// Requêtes gratuites qu'il reste aujourd'hui d'après `OpenRouter` (lecture
/// gardée 15 s), celles de ce processus déjà décomptées. `None` sans clé ou
/// sans réponse.
#[must_use]
pub fn free_requests_left() -> Option<u64> {
    let status = account_status(false)?.ok()?;
    Some(live(&status).free_requests?.remaining)
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
    let usage = day_usage();
    let now = now_secs();
    if let (Some(quota), Some(per_hour)) = (status.free_requests, usage.free_per_hour(now)) {
        lines.push(format!(
            "  Rythme           {}",
            pace_sentence(quota.remaining, per_hour, now)
        ));
    }
    if !usage.by_model.is_empty() {
        lines.push(format!(
            "  Par modèle       {} requête(s) de Claw aujourd'hui sur cette machine",
            usage.total()
        ));
        for (model, requests) in usage.by_model.iter().take(8) {
            lines.push(format!("    {requests:>5}  {model}"));
        }
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
            let usage = day_usage();
            let now = now_secs();
            let remaining = status.free_requests.map(|free| free.remaining);
            let pace = usage.free_per_hour(now).map(|per_hour| {
                serde_json::json!({
                    "per_hour": per_hour.round(),
                    // Heure d'épuisement (secondes Unix) si elle précède la remise à zéro.
                    "exhausted_at": remaining.and_then(|left| exhausted_at(left, per_hour, now)),
                })
            });
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
                // Requêtes de Claw aujourd'hui sur cette machine, par modèle.
                "by_model": usage.by_model.iter().map(|(model, requests)| serde_json::json!({
                    "model": model,
                    "requests": requests,
                    "free": is_free_model(model),
                })).collect::<Vec<_>>(),
                "counted": usage.total(),
                "pace": pace,
                "low": remaining.is_some_and(|left| left < LOW_FREE_REQUESTS),
                "text": short_line(&status),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        dollars, duration_text, exhausted_at, is_openrouter_model, low_quota_threshold,
        pace_sentence, short_line, DayUsage,
    };
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
    fn the_journal_gives_the_breakdown_and_the_pace() {
        let now = 1_791_540_000;
        // 30 requêtes gratuites sur la dernière demi-heure, 2 payantes, une ligne abîmée.
        let mut lines: Vec<String> = (0..30)
            .map(|index| {
                let model = if index < 24 {
                    "a/petit:free"
                } else {
                    "b/grand:free"
                };
                format!("{{\"t\":{},\"m\":\"{model}\"}}", now - 1_800 + index * 60)
            })
            .collect();
        let paid = format!("{{\"t\":{now},\"m\":\"c/payant\"}}");
        lines.extend([paid.clone(), paid, "ligne abîmée".to_string()]);
        let usage = DayUsage::parse(&lines.join("\n"));
        assert_eq!(
            usage.by_model,
            vec![
                ("a/petit:free".to_string(), 24),
                ("b/grand:free".to_string(), 6),
                ("c/payant".to_string(), 2)
            ]
        );
        assert_eq!(usage.total(), 32);
        // 30 requêtes en 30 minutes : 60 par heure.
        let per_hour = usage.free_per_hour(now).expect("rythme");
        assert!((per_hour - 60.0).abs() < 0.01, "{per_hour}");
        // Trop peu de recul : pas d'estimation.
        assert_eq!(
            DayUsage::parse("{\"t\":10,\"m\":\"a/x:free\"}").free_per_hour(20),
            None
        );
        assert_eq!(DayUsage::default().free_per_hour(now), None);
    }

    #[test]
    fn estimates_when_the_free_quota_runs_out() {
        // Midi UTC : 120 requêtes à 60/h tiennent 2 h, avant minuit.
        let noon = 20_000 * 86_400 + 12 * 3_600;
        assert_eq!(exhausted_at(120, 60.0, noon), Some(noon + 7_200));
        // 900 requêtes à 60/h : 15 h, au-delà de la remise à zéro.
        assert_eq!(exhausted_at(900, 60.0, noon), None);
        assert_eq!(exhausted_at(10, 0.0, noon), None);
        assert_eq!(duration_text(7_800), "2 h 10");
        assert_eq!(duration_text(1_500), "25 min");
        assert!(pace_sentence(120, 60.0, noon).contains("épuisé dans environ 2 h 00"));
        assert!(pace_sentence(900, 60.0, noon).contains("tient jusqu'à sa remise à zéro"));
        // Seuils d'alerte : 100, 20, 0.
        assert_eq!(low_quota_threshold(623), None);
        assert_eq!(low_quota_threshold(100), None);
        assert_eq!(low_quota_threshold(99), Some(100));
        assert_eq!(low_quota_threshold(20), Some(20));
        assert_eq!(low_quota_threshold(0), Some(0));
    }

    #[test]
    fn recognizes_openrouter_models() {
        assert!(is_openrouter_model("cohere/north-mini-code:free"));
        assert!(is_openrouter_model("anthropic/claude-sonnet-4"));
        assert!(!is_openrouter_model("ollama:qwen3.5:4b"));
        assert!(!is_openrouter_model("claude-opus-4-6"));
    }
}
