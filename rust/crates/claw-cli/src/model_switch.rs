//! Changement de modèle en cours de session : jamais sans l'accord de
//! l'utilisateur quand il est là pour répondre.
//!
//! Quand le modèle de la session ne répond plus (panne du fournisseur, quota,
//! clé refusée) ou n'avance plus, Claw ne bascule pas de lui-même sur un autre
//! modèle : il demande. L'utilisateur fait réessayer le même modèle, en
//! choisit un autre, ou arrête le tour. Sans personne pour répondre (commande
//! ponctuelle dans un script, action GitHub, sous-agent) ou avec
//! `"modelSwitch": "auto"`, la chaîne de secours s'applique comme avant.

use std::io::IsTerminal;
use std::sync::Arc;
use std::time::{Duration, Instant};

use runtime::ConfigLoader;

/// Ce qui est demandé à l'utilisateur.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ModelQuestion {
    /// Modèle de la session.
    pub model: String,
    /// Raison lisible : « le fournisseur ne répond pas (erreur 504) »…
    pub reason: String,
    /// Le modèle répond mais l'agent n'avance plus : la question est de
    /// passer la main, pas de réessayer.
    pub stuck: bool,
    /// Autres modèles proposés, du plus recommandé au moins recommandé.
    pub suggestions: Vec<String>,
    /// Sans réponse dans ce délai, le même modèle reprend de lui-même :
    /// l'utilisateur n'a rien à faire. `None` : la question attend sa réponse.
    pub auto_retry: Option<Duration>,
    /// Numéro de l'essai automatique à venir, et nombre d'essais prévus.
    pub attempt: Option<(u32, u32)>,
    /// Faux quand le même essai échouerait de la même façon (clé refusée,
    /// modèle introuvable, quota du jour épuisé).
    pub retry_useful: bool,
}

/// Délais avant chaque nouvel essai automatique du même modèle : rapprochés
/// d'abord, puis toutes les deux minutes, une vingtaine de minutes en tout.
const AUTO_RETRY_DELAYS: [u64; 12] = [3, 10, 30, 60, 120, 120, 120, 120, 120, 120, 120, 120];
/// Sans réponse, l'agent qui n'avançait plus continue avec le même modèle.
pub(crate) const STUCK_AUTO_CONTINUE: Duration = Duration::from_secs(60);

/// Nombre d'essais automatiques avant que la question n'attende une réponse
/// (un par délai ci-dessus).
pub(crate) const AUTO_RETRIES: u32 = 12;

/// Délai avant l'essai automatique numéro `done + 1` ; `None` quand ils sont épuisés.
pub(crate) fn auto_retry_delay(done: u32) -> Option<Duration> {
    usize::try_from(done)
        .ok()
        .and_then(|index| AUTO_RETRY_DELAYS.get(index))
        .map(|seconds| Duration::from_secs(*seconds))
}

/// Réponse de l'utilisateur.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ModelDecision {
    /// Le même modèle reprend.
    Retry,
    /// Ce modèle prend la suite et devient celui de la session.
    Switch(String),
    /// Le tour s'arrête.
    Stop,
}

/// Pose la question dans l'interface en cours (terminal, page web).
pub(crate) trait ModelSwitchPrompter: Send + Sync {
    fn ask(&self, question: &ModelQuestion) -> ModelDecision;
}

pub(crate) type SharedModelPrompter = Arc<dyn ModelSwitchPrompter>;

/// Réglage `"modelSwitch"` (ou `CLAW_MODEL_SWITCH`) : `ask` par défaut,
/// `auto` pour laisser Claw basculer seul sur la chaîne de secours.
pub(crate) fn ask_before_switching() -> bool {
    let setting = std::env::var("CLAW_MODEL_SWITCH")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            let cwd = std::env::current_dir().ok()?;
            let config = ConfigLoader::default_for(&cwd).load().ok()?;
            config
                .get("modelSwitch")
                .and_then(|value| value.as_str().map(ToOwned::to_owned))
        });
    asks(setting.as_deref())
}

fn asks(setting: Option<&str>) -> bool {
    !matches!(
        setting
            .map(|value| value.trim().to_ascii_lowercase())
            .as_deref(),
        Some("auto" | "automatique" | "off" | "0" | "false" | "non")
    )
}

/// Modèles à proposer : les `preferred` d'abord, sans doublon ni `current`.
pub(crate) fn suggestions(current: &str, preferred: &[String], others: &[String]) -> Vec<String> {
    const LIMIT: usize = 6;
    let mut list: Vec<String> = Vec::new();
    for model in preferred.iter().chain(others) {
        if model != current && !model.trim().is_empty() && !list.contains(model) {
            list.push(model.clone());
        }
    }
    list.truncate(LIMIT);
    list
}

/// Question posée dans le terminal, quand quelqu'un est là pour y répondre.
pub(crate) struct TerminalModelPrompter;

impl TerminalModelPrompter {
    /// `None` hors d'un terminal interactif : personne ne répondrait.
    pub(crate) fn if_interactive() -> Option<SharedModelPrompter> {
        (std::io::stdin().is_terminal() && std::io::stdout().is_terminal())
            .then(|| Arc::new(Self) as SharedModelPrompter)
    }
}

impl ModelSwitchPrompter for TerminalModelPrompter {
    fn ask(&self, question: &ModelQuestion) -> ModelDecision {
        // Essai automatique : pas de menu, le terminal attend le délai (Échap
        // ou Ctrl+C arrête le tour) puis le même modèle reprend.
        if let Some(delay) = question.auto_retry {
            if question.stuck {
                return ModelDecision::Retry;
            }
            let deadline = Instant::now() + delay;
            while Instant::now() < deadline {
                if runtime::interrupt::interrupt_requested() {
                    return ModelDecision::Stop;
                }
                std::thread::sleep(Duration::from_millis(200));
            }
            return ModelDecision::Retry;
        }
        let title = if question.stuck {
            "L'agent n'avance plus"
        } else {
            "Le modèle ne répond plus"
        };
        let body = vec![format!("{} : {}", question.model, question.reason)];
        let mut options = vec![if question.stuck {
            "Continuer avec le même modèle".to_string()
        } else {
            "Réessayer avec le même modèle".to_string()
        }];
        options.extend(
            question
                .suggestions
                .iter()
                .map(|model| format!("Passer à {model}")),
        );
        options.push("Arrêter le tour (Échap)".to_string());
        // L'indicateur d'activité s'efface le temps de la question.
        crate::live::hide();
        let answer = crate::approval::ask(title, &body, "Que voulez-vous faire ?", &options);
        match answer {
            Ok(crate::approval::Answer::Pick(0)) => ModelDecision::Retry,
            Ok(crate::approval::Answer::Pick(index)) => question
                .suggestions
                .get(index - 1)
                .cloned()
                .map_or(ModelDecision::Stop, ModelDecision::Switch),
            _ => ModelDecision::Stop,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{asks, auto_retry_delay, suggestions, AUTO_RETRIES};
    use std::time::Duration;

    #[test]
    fn retries_alone_quickly_at_first_then_every_two_minutes() {
        assert_eq!(auto_retry_delay(0), Some(Duration::from_secs(3)));
        assert_eq!(auto_retry_delay(1), Some(Duration::from_secs(10)));
        assert_eq!(
            auto_retry_delay(AUTO_RETRIES - 1),
            Some(Duration::from_secs(120))
        );
        // Une fois les essais épuisés, la question attend une réponse.
        assert_eq!(auto_retry_delay(AUTO_RETRIES), None);
        let total: u64 = (0..AUTO_RETRIES)
            .filter_map(auto_retry_delay)
            .map(|delay| delay.as_secs())
            .sum();
        assert!((15 * 60..=25 * 60).contains(&total), "{total} s");
    }

    #[test]
    fn asks_by_default_and_lets_the_setting_turn_it_off() {
        assert!(asks(None));
        assert!(asks(Some("ask")));
        assert!(asks(Some("demander")));
        assert!(!asks(Some("auto")));
        assert!(!asks(Some(" Automatique ")));
    }

    #[test]
    fn suggests_other_models_once_each_in_order() {
        let list = |models: &[&str]| -> Vec<String> {
            models.iter().map(|model| (*model).to_string()).collect()
        };
        assert_eq!(
            suggestions(
                "a/lent:free",
                &list(&["b/fort:free", "a/lent:free"]),
                &list(&["c/autre:free", "b/fort:free", " ", "ollama:local"]),
            ),
            list(&["b/fort:free", "c/autre:free", "ollama:local"])
        );
        assert!(suggestions("seul", &[], &list(&["seul"])).is_empty());
    }
}
