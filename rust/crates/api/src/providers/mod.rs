use std::future::Future;
use std::pin::Pin;

use crate::error::ApiError;
use crate::types::{MessageRequest, MessageResponse};

pub mod claw_provider;
pub mod compat;
pub mod openai_compat;
pub mod reasoning;
pub mod text_tools;

pub type ProviderFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, ApiError>> + Send + 'a>>;

pub trait Provider {
    type Stream;

    fn send_message<'a>(
        &'a self,
        request: &'a MessageRequest,
    ) -> ProviderFuture<'a, MessageResponse>;

    fn stream_message<'a>(
        &'a self,
        request: &'a MessageRequest,
    ) -> ProviderFuture<'a, Self::Stream>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderKind {
    ClawApi,
    Xai,
    OpenAi,
    OpenRouter,
    /// Serveur compatible `OpenAI` désigné par un préfixe (`ollama:`, `groq:`…).
    Compat,
}

impl ProviderKind {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::ClawApi => "anthropic",
            Self::Xai => "xai",
            Self::OpenAi => "openai",
            Self::OpenRouter => "openrouter",
            Self::Compat => "compatible-openai",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "anthropic" | "claude" | "claw" => Some(Self::ClawApi),
            "xai" | "grok" => Some(Self::Xai),
            "openai" => Some(Self::OpenAi),
            "openrouter" => Some(Self::OpenRouter),
            _ => None,
        }
    }
}

// Fournisseur imposé via `CLAW_PROVIDER` (anthropic, openrouter, openai, xai).
// Utile pour un serveur local compatible OpenAI dont les modèles n'ont pas de `/`.
fn forced_provider_kind() -> Option<ProviderKind> {
    std::env::var("CLAW_PROVIDER")
        .ok()
        .and_then(|value| ProviderKind::parse(&value))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProviderMetadata {
    pub provider: ProviderKind,
    pub auth_env: &'static str,
    pub base_url_env: &'static str,
    pub default_base_url: &'static str,
}

const MODEL_REGISTRY: &[(&str, ProviderMetadata)] = &[
    (
        "opus",
        ProviderMetadata {
            provider: ProviderKind::ClawApi,
            auth_env: "ANTHROPIC_API_KEY",
            base_url_env: "ANTHROPIC_BASE_URL",
            default_base_url: claw_provider::DEFAULT_BASE_URL,
        },
    ),
    (
        "sonnet",
        ProviderMetadata {
            provider: ProviderKind::ClawApi,
            auth_env: "ANTHROPIC_API_KEY",
            base_url_env: "ANTHROPIC_BASE_URL",
            default_base_url: claw_provider::DEFAULT_BASE_URL,
        },
    ),
    (
        "haiku",
        ProviderMetadata {
            provider: ProviderKind::ClawApi,
            auth_env: "ANTHROPIC_API_KEY",
            base_url_env: "ANTHROPIC_BASE_URL",
            default_base_url: claw_provider::DEFAULT_BASE_URL,
        },
    ),
    (
        "claude-opus-4-6",
        ProviderMetadata {
            provider: ProviderKind::ClawApi,
            auth_env: "ANTHROPIC_API_KEY",
            base_url_env: "ANTHROPIC_BASE_URL",
            default_base_url: claw_provider::DEFAULT_BASE_URL,
        },
    ),
    (
        "claude-sonnet-4-6",
        ProviderMetadata {
            provider: ProviderKind::ClawApi,
            auth_env: "ANTHROPIC_API_KEY",
            base_url_env: "ANTHROPIC_BASE_URL",
            default_base_url: claw_provider::DEFAULT_BASE_URL,
        },
    ),
    (
        "claude-haiku-4-5-20251001",
        ProviderMetadata {
            provider: ProviderKind::ClawApi,
            auth_env: "ANTHROPIC_API_KEY",
            base_url_env: "ANTHROPIC_BASE_URL",
            default_base_url: claw_provider::DEFAULT_BASE_URL,
        },
    ),
    (
        "grok",
        ProviderMetadata {
            provider: ProviderKind::Xai,
            auth_env: "XAI_API_KEY",
            base_url_env: "XAI_BASE_URL",
            default_base_url: openai_compat::DEFAULT_XAI_BASE_URL,
        },
    ),
    (
        "grok-3",
        ProviderMetadata {
            provider: ProviderKind::Xai,
            auth_env: "XAI_API_KEY",
            base_url_env: "XAI_BASE_URL",
            default_base_url: openai_compat::DEFAULT_XAI_BASE_URL,
        },
    ),
    (
        "grok-mini",
        ProviderMetadata {
            provider: ProviderKind::Xai,
            auth_env: "XAI_API_KEY",
            base_url_env: "XAI_BASE_URL",
            default_base_url: openai_compat::DEFAULT_XAI_BASE_URL,
        },
    ),
    (
        "grok-3-mini",
        ProviderMetadata {
            provider: ProviderKind::Xai,
            auth_env: "XAI_API_KEY",
            base_url_env: "XAI_BASE_URL",
            default_base_url: openai_compat::DEFAULT_XAI_BASE_URL,
        },
    ),
    (
        "grok-2",
        ProviderMetadata {
            provider: ProviderKind::Xai,
            auth_env: "XAI_API_KEY",
            base_url_env: "XAI_BASE_URL",
            default_base_url: openai_compat::DEFAULT_XAI_BASE_URL,
        },
    ),
];

#[must_use]
pub fn resolve_model_alias(model: &str) -> String {
    let trimmed = model.trim();
    let lower = trimmed.to_ascii_lowercase();
    MODEL_REGISTRY
        .iter()
        .find_map(|(alias, metadata)| {
            (*alias == lower).then_some(match metadata.provider {
                ProviderKind::ClawApi => match *alias {
                    "opus" => "claude-opus-4-6",
                    "sonnet" => "claude-sonnet-4-6",
                    "haiku" => "claude-haiku-4-5-20251001",
                    _ => trimmed,
                },
                ProviderKind::Xai => match *alias {
                    "grok" | "grok-3" => "grok-3",
                    "grok-mini" | "grok-3-mini" => "grok-3-mini",
                    "grok-2" => "grok-2",
                    _ => trimmed,
                },
                ProviderKind::OpenAi | ProviderKind::OpenRouter | ProviderKind::Compat => trimmed,
            })
        })
        .map_or_else(|| trimmed.to_string(), ToOwned::to_owned)
}

#[must_use]
pub fn metadata_for_model(model: &str) -> Option<ProviderMetadata> {
    let canonical = resolve_model_alias(model);
    let lower = canonical.to_ascii_lowercase();
    if let Some((_, metadata)) = MODEL_REGISTRY.iter().find(|(alias, _)| *alias == lower) {
        return Some(*metadata);
    }
    if lower.starts_with("grok") {
        return Some(ProviderMetadata {
            provider: ProviderKind::Xai,
            auth_env: "XAI_API_KEY",
            base_url_env: "XAI_BASE_URL",
            default_base_url: openai_compat::DEFAULT_XAI_BASE_URL,
        });
    }
    None
}

/// Choisit le fournisseur d'un modèle, par ordre de priorité :
/// 1. `CLAW_PROVIDER` s'il est défini ;
/// 2. les alias connus (opus, sonnet, haiku, grok…) ;
/// 3. un identifiant `fournisseur/modèle` (ex. `qwen/qwen3-coder`) → `OpenRouter` ;
/// 4. sinon, le premier fournisseur dont les identifiants sont présents.
#[must_use]
pub fn detect_provider_kind(model: &str) -> ProviderKind {
    // `ollama:…`, `groq:…` : le préfixe désigne le serveur sans ambiguïté.
    if compat::split_compat_model(model).is_some() {
        return ProviderKind::Compat;
    }
    if let Some(kind) = forced_provider_kind() {
        return kind;
    }
    if let Some(metadata) = metadata_for_model(model) {
        return metadata.provider;
    }
    if is_openrouter_model_id(model) {
        return ProviderKind::OpenRouter;
    }
    if model.trim().to_ascii_lowercase().starts_with("claude-") {
        return ProviderKind::ClawApi;
    }
    if claw_provider::has_auth_from_env_or_saved().unwrap_or(false) {
        return ProviderKind::ClawApi;
    }
    if openai_compat::has_api_key("OPENROUTER_API_KEY") {
        return ProviderKind::OpenRouter;
    }
    if openai_compat::has_api_key("OPENAI_API_KEY") {
        return ProviderKind::OpenAi;
    }
    if openai_compat::has_api_key("XAI_API_KEY") {
        return ProviderKind::Xai;
    }
    ProviderKind::ClawApi
}

// Les identifiants OpenRouter ont la forme `fournisseur/modèle[:variante]`.
fn is_openrouter_model_id(model: &str) -> bool {
    let trimmed = model.trim();
    trimmed
        .split_once('/')
        .is_some_and(|(vendor, name)| !vendor.is_empty() && !name.is_empty())
}

/// Fenêtre de contexte d'un modèle : catalogue `OpenRouter` si connu, sinon
/// 200 000 pour Claude et 128 000 par défaut.
#[must_use]
pub fn context_window_for_model(model: &str) -> u64 {
    if let Some((provider, _)) = compat::split_compat_model(model) {
        return std::env::var("CLAW_CONTEXT_WINDOW")
            .ok()
            .and_then(|value| value.trim().parse::<u64>().ok())
            .filter(|value| *value > 0)
            .unwrap_or_else(|| provider.default_context_window());
    }
    let canonical = resolve_model_alias(model);
    if let Some(window) =
        crate::openrouter::model_limits(&canonical).and_then(|limits| limits.context_length)
    {
        return window;
    }
    if canonical.starts_with("claude-") || canonical.starts_with("anthropic/") {
        200_000
    } else {
        128_000
    }
}

const MAX_AUTO_COMPACTION_THRESHOLD: u64 = 180_000;

/// Seuil de compaction automatique (en jetons) : trois quarts de la fenêtre,
/// moins la sortie et les définitions d'outils, plafonné pour garder des
/// requêtes rapides. `CLAW_AUTO_COMPACT_TOKENS` le remplace.
#[must_use]
pub fn auto_compaction_threshold(model: &str) -> usize {
    if let Some(value) = std::env::var("CLAW_AUTO_COMPACT_TOKENS")
        .ok()
        .and_then(|value| value.trim().parse::<usize>().ok())
        .filter(|value| *value > 0)
    {
        return value;
    }
    let window = context_window_for_model(model);
    let reserve = u64::from(max_tokens_for_model(model)) + 8_000;
    let threshold = (window * 3 / 4)
        .saturating_sub(reserve)
        .clamp(8_000, MAX_AUTO_COMPACTION_THRESHOLD);
    usize::try_from(threshold).unwrap_or(usize::MAX)
}

/// Plafond de jetons de sortie demandé au modèle. `CLAW_MAX_TOKENS` le remplace.
#[must_use]
pub fn max_tokens_for_model(model: &str) -> u32 {
    if let Some(value) = std::env::var("CLAW_MAX_TOKENS")
        .ok()
        .and_then(|value| value.trim().parse::<u32>().ok())
        .filter(|value| *value > 0)
    {
        return value;
    }
    if let Some((provider, _)) = compat::split_compat_model(model) {
        let by_context = context_window_for_model(model) / 4;
        return u32::try_from(u64::from(provider.default_max_output_tokens()).min(by_context))
            .unwrap_or(8_192)
            .max(1_024);
    }
    let canonical = resolve_model_alias(model);
    if is_openrouter_model_id(&canonical) {
        // Les limites de sortie varient beaucoup selon les modèles OpenRouter, et
        // prompt + max_tokens ne doit pas dépasser la fenêtre de contexte : on suit
        // le catalogue (voir `register_model_limits`) quand il est connu.
        crate::openrouter::model_limits(&canonical).map_or(16_384, |limits| {
            let by_output = limits.max_completion_tokens.unwrap_or(16_384);
            let by_context = limits
                .context_length
                .map_or(u64::MAX, |context| context / 4);
            u32::try_from(by_output.min(by_context).clamp(1_024, 32_768)).unwrap_or(16_384)
        })
    } else if canonical.contains("opus") {
        32_000
    } else {
        64_000
    }
}

#[cfg(test)]
mod tests {
    use super::{detect_provider_kind, max_tokens_for_model, resolve_model_alias, ProviderKind};

    #[test]
    fn resolves_grok_aliases() {
        assert_eq!(resolve_model_alias("grok"), "grok-3");
        assert_eq!(resolve_model_alias("grok-mini"), "grok-3-mini");
        assert_eq!(resolve_model_alias("grok-2"), "grok-2");
    }

    #[test]
    fn detects_provider_from_model_name_first() {
        assert_eq!(detect_provider_kind("grok"), ProviderKind::Xai);
        assert_eq!(
            detect_provider_kind("claude-sonnet-4-6"),
            ProviderKind::ClawApi
        );
    }

    #[test]
    fn keeps_existing_max_token_heuristic() {
        assert_eq!(max_tokens_for_model("opus"), 32_000);
        assert_eq!(max_tokens_for_model("grok-3"), 64_000);
        assert_eq!(max_tokens_for_model("qwen/qwen3-coder"), 16_384);
    }

    #[test]
    fn openrouter_max_tokens_follow_catalog_limits() {
        let catalog = crate::openrouter::ModelCatalog {
            version: 0,
            category_rankings: std::collections::BTreeMap::new(),
            fetched_at: 0,
            programming_ranking: Vec::new(),
            models: vec![
                crate::openrouter::OpenRouterModel {
                    id: "small/out:free".to_string(),
                    name: String::new(),
                    description: String::new(),
                    context_length: Some(65_536),
                    max_completion_tokens: Some(8_192),
                    prompt_price: 0.0,
                    completion_price: 0.0,
                    supports_tools: true,
                    supports_images: false,
                    ..Default::default()
                },
                crate::openrouter::OpenRouterModel {
                    id: "huge/ctx:free".to_string(),
                    name: String::new(),
                    description: String::new(),
                    context_length: Some(1_000_000),
                    max_completion_tokens: None,
                    prompt_price: 0.0,
                    completion_price: 0.0,
                    supports_tools: true,
                    supports_images: false,
                    ..Default::default()
                },
            ],
        };
        crate::openrouter::register_model_limits(&catalog);
        assert_eq!(max_tokens_for_model("small/out:free"), 8_192);
        assert_eq!(max_tokens_for_model("huge/ctx:free"), 16_384);
        assert_eq!(max_tokens_for_model("unknown/model"), 16_384);

        assert_eq!(super::context_window_for_model("small/out:free"), 65_536);
        assert_eq!(
            super::context_window_for_model("claude-sonnet-4-6"),
            200_000
        );
        // 65 536 × ¾ − (8 192 + 8 000) = 32 960
        assert_eq!(super::auto_compaction_threshold("small/out:free"), 32_960);
        assert_eq!(super::auto_compaction_threshold("huge/ctx:free"), 180_000);
    }

    #[test]
    fn vendor_slash_model_ids_route_to_openrouter() {
        assert_eq!(
            detect_provider_kind("anthropic/claude-sonnet-4.5"),
            ProviderKind::OpenRouter
        );
        assert_eq!(
            detect_provider_kind("deepseek/deepseek-chat-v3.1:free"),
            ProviderKind::OpenRouter
        );
        assert_eq!(
            detect_provider_kind("openrouter/auto"),
            ProviderKind::OpenRouter
        );
        assert_eq!(resolve_model_alias("qwen/qwen3-coder"), "qwen/qwen3-coder");
        assert_eq!(resolve_model_alias("haiku"), "claude-haiku-4-5-20251001");
    }
}
