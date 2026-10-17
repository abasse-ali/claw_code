//! `apiKeyHelper` (comme dans Claude Code) : une commande fournit la clé d'un
//! fournisseur, pour des identifiants tournants (jeton de courte durée tiré
//! d'un coffre). Elle ne sert que si la variable de clé est absente ; sa
//! sortie est gardée 5 minutes (`CLAW_API_KEY_HELPER_TTL_MS`) et redemandée
//! après un refus 401 ou 403. Une clé n'est jamais donnée à un autre
//! fournisseur que celui pour lequel le helper est réglé.

use std::collections::BTreeMap;
use std::process::{Command, Stdio};
use std::sync::{Mutex, OnceLock, RwLock};
use std::time::{Duration, Instant};

/// Helpers réglés, par variable de clé (`OPENROUTER_API_KEY`…).
fn helpers() -> &'static RwLock<BTreeMap<String, String>> {
    static HELPERS: OnceLock<RwLock<BTreeMap<String, String>>> = OnceLock::new();
    HELPERS.get_or_init(|| RwLock::new(BTreeMap::new()))
}

/// Helper par défaut (réglage `apiKeyHelper`) et la variable de clé du
/// fournisseur de la session, à laquelle il s'applique.
fn default_helper() -> &'static RwLock<(Option<String>, Option<String>)> {
    static DEFAULT: OnceLock<RwLock<(Option<String>, Option<String>)>> = OnceLock::new();
    DEFAULT.get_or_init(|| RwLock::new((None, None)))
}

fn cache() -> &'static Mutex<BTreeMap<String, (String, Instant)>> {
    static CACHE: OnceLock<Mutex<BTreeMap<String, (String, Instant)>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(BTreeMap::new()))
}

/// Helper propre à une variable de clé (`providers[].apiKeyHelper`, ou
/// `apiKeyHelper` de Claude Code pour `ANTHROPIC_API_KEY`).
pub fn register(env_name: &str, command: &str) {
    let command = command.trim();
    if env_name.trim().is_empty() || command.is_empty() {
        return;
    }
    helpers()
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .insert(env_name.to_string(), command.to_string());
}

/// Réglage `apiKeyHelper` : la commande, appliquée au fournisseur du modèle
/// de la session.
pub fn set_default(command: Option<&str>) {
    default_helper()
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .0 = command
        .map(str::trim)
        .filter(|command| !command.is_empty())
        .map(str::to_string);
}

/// Variable de clé du fournisseur de la session (le helper par défaut ne
/// s'applique qu'à elle).
pub fn set_default_target(env_name: Option<&str>) {
    default_helper()
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .1 = env_name.map(str::to_string);
}

fn ttl() -> Duration {
    [
        "CLAW_API_KEY_HELPER_TTL_MS",
        "CLAUDE_CODE_API_KEY_HELPER_TTL_MS",
    ]
    .iter()
    .find_map(|name| std::env::var(name).ok()?.trim().parse::<u64>().ok())
    .map_or(Duration::from_secs(300), Duration::from_millis)
}

fn command_for(env_name: &str) -> Option<String> {
    if let Some(command) = helpers()
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get(env_name)
    {
        return Some(command.clone());
    }
    let guard = default_helper()
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    match (&guard.0, &guard.1) {
        (Some(command), Some(target)) if target == env_name => Some(command.clone()),
        _ => None,
    }
}

/// Lance la commande dans le shell du système (`cmd` sous Windows, `sh`
/// ailleurs) ; la première ligne non vide de sa sortie est la clé.
fn run(command: &str) -> Result<String, String> {
    let started = Instant::now();
    let mut process = if cfg!(windows) {
        let mut process = Command::new("cmd");
        process.arg("/C").arg(command);
        process
    } else {
        let mut process = Command::new("sh");
        process.arg("-c").arg(command);
        process
    };
    let output = process
        .stdin(Stdio::null())
        .output()
        .map_err(|error| format!("lancement impossible : {error}"))?;
    if started.elapsed() > Duration::from_secs(10) {
        eprintln!(
            "[apiKeyHelper] la commande a mis {} s à rendre la clé",
            started.elapsed().as_secs()
        );
    }
    if !output.status.success() {
        return Err(format!(
            "code {} : {}",
            output.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(str::to_string)
        .ok_or_else(|| "la commande n'a rien affiché".to_string())
}

/// Clé fournie par le helper de cette variable, s'il y en a un.
#[must_use]
pub fn key(env_name: &str) -> Option<String> {
    let command = command_for(env_name)?;
    if let Some((key, at)) = cache()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get(env_name)
    {
        if at.elapsed() < ttl() {
            return Some(key.clone());
        }
    }
    match run(&command) {
        Ok(key) => {
            cache()
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .insert(env_name.to_string(), (key.clone(), Instant::now()));
            Some(key)
        }
        Err(error) => {
            eprintln!("[apiKeyHelper] la commande pour {env_name} échoue ({error})");
            None
        }
    }
}

/// Après un refus 401 ou 403 : la prochaine requête redemande la clé.
pub fn invalidate() {
    cache()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clear();
}

#[cfg(test)]
mod tests {
    use super::{invalidate, key, register, set_default, set_default_target};

    #[test]
    fn helpers_give_keys_only_to_their_provider() {
        let command = if cfg!(windows) {
            "echo cle-essai"
        } else {
            "printf 'cle-essai\\n'"
        };
        register("CLAW_TEST_HELPER_KEY", command);
        assert_eq!(key("CLAW_TEST_HELPER_KEY").as_deref(), Some("cle-essai"));
        set_default(Some(command));
        set_default_target(Some("CLAW_TEST_DEFAULT_KEY"));
        assert_eq!(key("CLAW_TEST_DEFAULT_KEY").as_deref(), Some("cle-essai"));
        // Un autre fournisseur ne reçoit pas la clé du helper par défaut.
        assert_eq!(key("CLAW_TEST_OTHER_KEY"), None);
        invalidate();
        set_default(None);
        assert_eq!(key("CLAW_TEST_DEFAULT_KEY"), None);
    }
}
