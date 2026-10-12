//! Raisonnement (« thinking ») des modèles compatibles `OpenAI` : niveau d'effort
//! demandé (`/think`, `"reasoning"` dans la config, `CLAW_REASONING`) et
//! séparation du raisonnement et de la réponse dans le flux.

use std::cell::Cell;
use std::collections::BTreeMap;
use std::sync::RwLock;

/// Niveaux rangés du plus faible au plus fort.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ReasoningEffort {
    /// Rien n'est demandé et le raisonnement éventuel n'est pas affiché.
    Off,
    Low,
    Medium,
    High,
    /// Le plus haut niveau que le modèle accepte : `OpenRouter` ramène un
    /// niveau non accepté au plus proche qui l'est.
    Max,
}

impl ReasoningEffort {
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        Some(match value.trim().to_ascii_lowercase().as_str() {
            "off" | "non" | "none" | "aucun" | "0" => Self::Off,
            "low" | "faible" | "bas" => Self::Low,
            "medium" | "moyen" | "normal" => Self::Medium,
            "high" | "fort" | "élevé" | "eleve" => Self::High,
            "max" | "maximum" | "maximal" | "xhigh" | "ultrathink" => Self::Max,
            _ => return None,
        })
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::Max => "max",
        }
    }

    /// Valeur du champ `reasoning_effort` des serveurs compatibles `OpenAI`
    /// autres qu'`OpenRouter`, qui ne connaissent pas « max ».
    #[must_use]
    pub const fn openai_value(self) -> &'static str {
        match self {
            Self::Max => "high",
            other => other.as_str(),
        }
    }

    /// Un cran de moins ; `None` sous « low ».
    #[must_use]
    pub const fn lower(self) -> Option<Self> {
        match self {
            Self::Max => Some(Self::High),
            Self::High => Some(Self::Medium),
            Self::Medium => Some(Self::Low),
            Self::Low | Self::Off => None,
        }
    }
}

static EFFORT: RwLock<Option<ReasoningEffort>> = RwLock::new(None);

/// Niveau de raisonnement de la session (`None` : réglage du modèle).
pub fn set_reasoning_effort(effort: Option<ReasoningEffort>) {
    *EFFORT
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = effort;
}

#[must_use]
pub fn reasoning_effort() -> Option<ReasoningEffort> {
    *EFFORT
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

thread_local! {
    /// Plafond du niveau des requêtes faites par ce fil.
    static THREAD_CAP: Cell<Option<ReasoningEffort>> = const { Cell::new(None) };
}

/// Plafonne le niveau de raisonnement des requêtes que ce fil fera ensuite
/// (`None` : pas de plafond). Sert aux consultations d'autres modèles
/// (relecture, conseil), dont la réponse doit tenir dans un petit budget de
/// sortie : au plus haut niveau, le raisonnement le prenait tout, et la
/// réponse arrivait vide.
pub fn cap_thread_reasoning(cap: Option<ReasoningEffort>) {
    THREAD_CAP.with(|current| current.set(cap));
}

/// Budget de raisonnement étendu des modèles Claude (API Anthropic).
#[must_use]
pub fn anthropic_thinking_budget() -> Option<u32> {
    match reasoning_effort()? {
        ReasoningEffort::Off => None,
        ReasoningEffort::Low => Some(2_048),
        ReasoningEffort::Medium => Some(8_192),
        ReasoningEffort::High | ReasoningEffort::Max => Some(24_576),
    }
}

/// Niveau demandé à un modèle compatible `OpenAI`. Sans réglage, un modèle
/// d'`OpenRouter` qui sait raisonner reçoit le plus haut niveau qui lui
/// convient (`strongest`, tiré du catalogue) : mieux vaut une réponse
/// réfléchie qu'une réponse rapide. Les autres serveurs ne reçoivent rien
/// sans réglage, certains refusant ce paramètre. Un modèle dont la réflexion
/// a déjà pris toute la place de la réponse reste un cran en dessous.
#[must_use]
pub fn effort_for_request(
    configured: Option<ReasoningEffort>,
    model: &str,
    openrouter: bool,
    strongest: Option<ReasoningEffort>,
) -> Option<ReasoningEffort> {
    if !effort_applies_to(model) {
        return None;
    }
    let effort = match configured {
        Some(ReasoningEffort::Off) => return None,
        Some(effort) => effort,
        None if openrouter => strongest?,
        None => return None,
    };
    let effort = match THREAD_CAP.with(Cell::get) {
        Some(ReasoningEffort::Off) => return None,
        Some(cap) if cap < effort => cap,
        _ => effort,
    };
    Some(match effort_cap(model) {
        Some(cap) if cap < effort => cap,
        _ => effort,
    })
}

// --- Réflexion qui prend toute la place ---------------------------------------------

/// Dernier niveau envoyé à chaque modèle, et plafond appris pour la session.
static SENT: RwLock<BTreeMap<String, ReasoningEffort>> = RwLock::new(BTreeMap::new());
static CAPS: RwLock<BTreeMap<String, ReasoningEffort>> = RwLock::new(BTreeMap::new());
static NOTICE: RwLock<Option<String>> = RwLock::new(None);

fn effort_cap(model: &str) -> Option<ReasoningEffort> {
    CAPS.read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get(model)
        .copied()
}

/// Retient le niveau demandé à ce modèle pour la requête en cours.
pub fn note_effort_sent(model: &str, effort: ReasoningEffort) {
    SENT.write()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .insert(model.to_string(), effort);
}

/// La réponse s'est arrêtée à la limite de sortie sans texte ni appel
/// d'outil : la réflexion a tout pris. Ce modèle reçoit désormais un cran de
/// moins (jusqu'à la fin de la session), et un avertissement attend d'être
/// affiché. Renvoie le nouveau niveau.
pub fn reasoning_exhausted(model: &str) -> Option<ReasoningEffort> {
    let sent = SENT
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get(model)
        .copied()?;
    let lower = sent.lower()?;
    CAPS.write()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .insert(model.to_string(), lower);
    *NOTICE
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(format!(
        "La réflexion de {model} a pris toute la place de sa réponse : niveau « {} » au lieu de « {} » pour la suite de la session",
        lower.as_str(),
        sent.as_str()
    ));
    Some(lower)
}

/// Avertissement en attente après un repli du niveau de raisonnement.
#[must_use]
pub fn take_reasoning_notice() -> Option<String> {
    NOTICE
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .take()
}

/// Les modèles Claude exigent qu'on leur renvoie leurs blocs de raisonnement
/// avec les appels d'outils, ce que les sessions ne conservent pas : on ne
/// leur demande donc pas de raisonnement étendu.
#[must_use]
pub fn effort_applies_to(model: &str) -> bool {
    let lower = model.to_ascii_lowercase();
    !(lower.contains("claude") || lower.contains("anthropic/"))
}

const OPEN: &str = "<think>";
const CLOSE: &str = "</think>";

/// Sépare le raisonnement écrit entre balises `<think>…</think>` (Qwen3,
/// `DeepSeek`-R1 servis par Ollama ou llama.cpp) du texte de la réponse, au fil du flux.
#[derive(Debug, Default)]
pub struct ThinkTagFilter {
    pending: String,
    inside: bool,
    seen_text: bool,
}

impl ThinkTagFilter {
    /// Renvoie (texte de réponse, raisonnement) pour ce morceau.
    pub fn push(&mut self, delta: &str) -> (String, String) {
        self.pending.push_str(delta);
        let mut text = String::new();
        let mut thinking = String::new();
        loop {
            let tag = if self.inside { CLOSE } else { OPEN };
            if let Some(position) = self.pending.find(tag) {
                let before: String = self.pending.drain(..position).collect();
                self.pending.drain(..tag.len());
                if self.inside {
                    thinking.push_str(&before);
                } else {
                    text.push_str(&before);
                }
                self.inside = !self.inside;
                continue;
            }
            // Garde un éventuel début de balise coupé en fin de morceau.
            let keep = (1..tag.len())
                .rev()
                .find(|length| self.pending.ends_with(&tag[..*length]))
                .unwrap_or(0);
            let cut = self.pending.len() - keep;
            let chunk: String = self.pending.drain(..cut).collect();
            if self.inside {
                thinking.push_str(&chunk);
            } else {
                text.push_str(&chunk);
            }
            break;
        }
        // Le texte qui suit `</think>` commence souvent par des sauts de ligne.
        if !self.seen_text {
            let trimmed = text.trim_start().to_string();
            if !trimmed.is_empty() {
                self.seen_text = true;
            }
            text = trimmed;
        }
        (text, thinking)
    }

    /// Fin du flux : ce qui restait en réserve.
    pub fn finish(&mut self) -> (String, String) {
        let rest = std::mem::take(&mut self.pending);
        if self.inside {
            (String::new(), rest)
        } else {
            (rest, String::new())
        }
    }
}

/// Retire les blocs `<think>` d'une réponse complète : (réponse, raisonnement).
#[must_use]
pub fn split_think_tags(text: &str) -> (String, String) {
    let mut filter = ThinkTagFilter::default();
    let (mut answer, mut thinking) = filter.push(text);
    let (rest_answer, rest_thinking) = filter.finish();
    answer.push_str(&rest_answer);
    thinking.push_str(&rest_thinking);
    (answer, thinking)
}

#[cfg(test)]
mod tests {
    use super::{split_think_tags, ReasoningEffort, ThinkTagFilter};

    #[test]
    fn parses_effort_levels_in_french_and_english() {
        assert_eq!(ReasoningEffort::parse("élevé"), Some(ReasoningEffort::High));
        assert_eq!(ReasoningEffort::parse("max"), Some(ReasoningEffort::Max));
        assert_eq!(
            ReasoningEffort::parse("ultrathink"),
            Some(ReasoningEffort::Max)
        );
        assert_eq!(ReasoningEffort::Max.openai_value(), "high");
        assert_eq!(
            ReasoningEffort::parse("medium"),
            Some(ReasoningEffort::Medium)
        );
        assert_eq!(ReasoningEffort::parse("off"), Some(ReasoningEffort::Off));
        assert_eq!(ReasoningEffort::parse("bof"), None);
        assert!(!super::effort_applies_to("anthropic/claude-sonnet-4.5"));
        assert!(super::effort_applies_to("deepseek/deepseek-r1:free"));
    }

    #[test]
    fn reasoning_models_think_hard_unless_told_otherwise() {
        use super::effort_for_request;
        let model = "deepseek/deepseek-r1:free";
        let max = Some(ReasoningEffort::Max);
        // Sans réglage : le plus haut niveau qui convient au modèle.
        assert_eq!(effort_for_request(None, model, true, max), max);
        assert_eq!(
            effort_for_request(None, model, true, Some(ReasoningEffort::High)),
            Some(ReasoningEffort::High)
        );
        // Modèle qui ne raisonne pas, ou autre serveur : rien n'est demandé.
        assert_eq!(effort_for_request(None, model, true, None), None);
        assert_eq!(
            effort_for_request(None, "groq:llama-3.3-70b", false, max),
            None
        );
        // Le réglage de l'utilisateur prime, y compris pour couper.
        assert_eq!(
            effort_for_request(Some(ReasoningEffort::Low), model, true, max),
            Some(ReasoningEffort::Low)
        );
        assert_eq!(
            effort_for_request(
                Some(ReasoningEffort::Medium),
                "groq:qwen-qwq-32b",
                false,
                None
            ),
            Some(ReasoningEffort::Medium)
        );
        assert_eq!(
            effort_for_request(Some(ReasoningEffort::Off), model, true, max),
            None
        );
        // Les modèles Claude gardent leur propre mécanisme.
        assert_eq!(
            effort_for_request(None, "anthropic/claude-sonnet-4.5", true, max),
            None
        );
    }

    #[test]
    fn a_thread_cap_lowers_the_effort_of_its_own_requests_only() {
        use super::{cap_thread_reasoning, effort_for_request};
        let model = "vendor/deep-thinker:free";
        let max = Some(ReasoningEffort::Max);
        cap_thread_reasoning(Some(ReasoningEffort::Medium));
        assert_eq!(
            effort_for_request(None, model, true, max),
            Some(ReasoningEffort::Medium)
        );
        // Un niveau plus bas, demandé par l'utilisateur, reste le sien.
        assert_eq!(
            effort_for_request(Some(ReasoningEffort::Low), model, true, max),
            Some(ReasoningEffort::Low)
        );
        // Un modèle qui ne raisonne pas ne reçoit toujours rien.
        assert_eq!(effort_for_request(None, model, true, None), None);
        // Un autre fil n'est pas plafonné.
        let elsewhere = std::thread::spawn(move || effort_for_request(None, model, true, max))
            .join()
            .expect("fil");
        assert_eq!(elsewhere, max);
        cap_thread_reasoning(Some(ReasoningEffort::Off));
        assert_eq!(effort_for_request(None, model, true, max), None);
        cap_thread_reasoning(None);
        assert_eq!(effort_for_request(None, model, true, max), max);
    }

    #[test]
    fn a_reasoning_that_fills_the_whole_reply_lowers_the_effort_one_notch() {
        use super::{
            effort_for_request, note_effort_sent, reasoning_exhausted, take_reasoning_notice,
        };
        let model = "vendor/long-thinker:free";
        let max = Some(ReasoningEffort::Max);
        // Rien d'envoyé à ce modèle : rien à abaisser.
        assert_eq!(reasoning_exhausted("vendor/unknown:free"), None);
        note_effort_sent(model, ReasoningEffort::Max);
        assert_eq!(reasoning_exhausted(model), Some(ReasoningEffort::High));
        let notice = take_reasoning_notice().expect("avertissement");
        assert!(notice.contains("« high » au lieu de « max »"), "{notice}");
        assert!(take_reasoning_notice().is_none());
        // La suite de la session reste un cran en dessous, réglage compris.
        assert_eq!(
            effort_for_request(None, model, true, max),
            Some(ReasoningEffort::High)
        );
        assert_eq!(
            effort_for_request(max, model, true, max),
            Some(ReasoningEffort::High)
        );
        assert_eq!(
            effort_for_request(Some(ReasoningEffort::Low), model, true, max),
            Some(ReasoningEffort::Low)
        );
        // Un autre modèle n'est pas concerné.
        assert_eq!(
            effort_for_request(None, "vendor/other:free", true, max),
            max
        );
    }

    #[test]
    fn separates_think_tags_across_chunks() {
        let mut filter = ThinkTagFilter::default();
        let mut answer = String::new();
        let mut thinking = String::new();
        for piece in [
            "<thi",
            "nk>Je compare ",
            "deux options.</th",
            "ink>\n\nLa seconde",
            " est meilleure.",
        ] {
            let (text, thought) = filter.push(piece);
            answer.push_str(&text);
            thinking.push_str(&thought);
        }
        let (rest, _) = filter.finish();
        answer.push_str(&rest);
        assert_eq!(thinking, "Je compare deux options.");
        assert_eq!(answer, "La seconde est meilleure.");
        assert_eq!(
            split_think_tags("pas de balise"),
            ("pas de balise".to_string(), String::new())
        );
    }
}
