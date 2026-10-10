//! Raisonnement (« thinking ») des modèles compatibles `OpenAI` : niveau d'effort
//! demandé (`/think`, `"reasoning"` dans la config, `CLAW_REASONING`) et
//! séparation du raisonnement et de la réponse dans le flux.

use std::sync::RwLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReasoningEffort {
    /// Rien n'est demandé et le raisonnement éventuel n'est pas affiché.
    Off,
    Low,
    Medium,
    High,
}

impl ReasoningEffort {
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        Some(match value.trim().to_ascii_lowercase().as_str() {
            "off" | "non" | "none" | "aucun" | "0" => Self::Off,
            "low" | "faible" | "bas" => Self::Low,
            "medium" | "moyen" | "normal" => Self::Medium,
            "high" | "fort" | "élevé" | "eleve" | "max" | "ultrathink" => Self::High,
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

/// Budget de raisonnement étendu des modèles Claude (API Anthropic).
#[must_use]
pub fn anthropic_thinking_budget() -> Option<u32> {
    match reasoning_effort()? {
        ReasoningEffort::Off => None,
        ReasoningEffort::Low => Some(2_048),
        ReasoningEffort::Medium => Some(8_192),
        ReasoningEffort::High => Some(24_576),
    }
}

/// Niveau demandé à un modèle compatible `OpenAI`. Sans réglage, un modèle
/// d'`OpenRouter` qui sait raisonner reçoit le niveau le plus poussé : mieux
/// vaut une réponse réfléchie qu'une réponse rapide. Les autres serveurs ne
/// reçoivent rien sans réglage, certains refusant ce paramètre.
#[must_use]
pub fn effort_for_request(
    configured: Option<ReasoningEffort>,
    model: &str,
    openrouter: bool,
    model_reasons: bool,
) -> Option<ReasoningEffort> {
    if !effort_applies_to(model) {
        return None;
    }
    match configured {
        Some(ReasoningEffort::Off) => None,
        Some(effort) => Some(effort),
        None => (openrouter && model_reasons).then_some(ReasoningEffort::High),
    }
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
        // Sans réglage : réflexion poussée pour un modèle OpenRouter qui raisonne.
        assert_eq!(
            effort_for_request(None, model, true, true),
            Some(ReasoningEffort::High)
        );
        // Modèle qui ne raisonne pas, ou autre serveur : rien n'est demandé.
        assert_eq!(effort_for_request(None, model, true, false), None);
        assert_eq!(
            effort_for_request(None, "groq:llama-3.3-70b", false, true),
            None
        );
        // Le réglage de l'utilisateur prime, y compris pour couper.
        assert_eq!(
            effort_for_request(Some(ReasoningEffort::Low), model, true, true),
            Some(ReasoningEffort::Low)
        );
        assert_eq!(
            effort_for_request(
                Some(ReasoningEffort::Medium),
                "groq:qwen-qwq-32b",
                false,
                false
            ),
            Some(ReasoningEffort::Medium)
        );
        assert_eq!(
            effort_for_request(Some(ReasoningEffort::Off), model, true, true),
            None
        );
        // Les modèles Claude gardent leur propre mécanisme.
        assert_eq!(
            effort_for_request(None, "anthropic/claude-sonnet-4.5", true, true),
            None
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
