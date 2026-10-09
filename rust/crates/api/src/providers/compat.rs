//! Fournisseurs compatibles `OpenAI` désignés par un préfixe : `ollama:qwen2.5-coder:7b`,
//! `groq:llama-3.3-70b-versatile`, `monserveur:mon-modele`…
//!
//! Préréglages : serveurs locaux (Ollama, LM Studio, llama.cpp - sans clé) et
//! services en ligne (Groq, Mistral, `DeepSeek`, Gemini, Cerebras, Together,
//! Fireworks). D'autres serveurs se déclarent dans la config (`providers`).

use std::sync::RwLock;

/// Un serveur compatible `OpenAI` (`POST {base_url}/chat/completions`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompatProvider {
    pub name: String,
    pub base_url: String,
    /// Variable d'environnement contenant la clé (facultative pour un serveur local).
    pub api_key_env: Option<String>,
    pub requires_key: bool,
    /// Serveur sur la machine : pas de coût, données locales.
    pub local: bool,
    pub context_window: Option<u64>,
    pub max_output_tokens: Option<u32>,
    pub headers: Vec<(String, String)>,
    /// Appels d'outils décrits dans le prompt (`"toolMode": "text"`) plutôt que natifs.
    pub text_tools: bool,
}

struct Preset {
    name: &'static str,
    base_url: &'static str,
    base_url_env: &'static str,
    api_key_env: &'static str,
    local: bool,
}

const PRESETS: &[Preset] = &[
    Preset {
        name: "ollama",
        base_url: "http://localhost:11434/v1",
        base_url_env: "OLLAMA_BASE_URL",
        api_key_env: "OLLAMA_API_KEY",
        local: true,
    },
    Preset {
        name: "lmstudio",
        base_url: "http://localhost:1234/v1",
        base_url_env: "LMSTUDIO_BASE_URL",
        api_key_env: "LMSTUDIO_API_KEY",
        local: true,
    },
    Preset {
        name: "llamacpp",
        base_url: "http://localhost:8080/v1",
        base_url_env: "LLAMACPP_BASE_URL",
        api_key_env: "LLAMACPP_API_KEY",
        local: true,
    },
    Preset {
        name: "groq",
        base_url: "https://api.groq.com/openai/v1",
        base_url_env: "GROQ_BASE_URL",
        api_key_env: "GROQ_API_KEY",
        local: false,
    },
    Preset {
        name: "mistral",
        base_url: "https://api.mistral.ai/v1",
        base_url_env: "MISTRAL_BASE_URL",
        api_key_env: "MISTRAL_API_KEY",
        local: false,
    },
    Preset {
        name: "deepseek",
        base_url: "https://api.deepseek.com/v1",
        base_url_env: "DEEPSEEK_BASE_URL",
        api_key_env: "DEEPSEEK_API_KEY",
        local: false,
    },
    Preset {
        name: "gemini",
        base_url: "https://generativelanguage.googleapis.com/v1beta/openai",
        base_url_env: "GEMINI_BASE_URL",
        api_key_env: "GEMINI_API_KEY",
        local: false,
    },
    Preset {
        name: "cerebras",
        base_url: "https://api.cerebras.ai/v1",
        base_url_env: "CEREBRAS_BASE_URL",
        api_key_env: "CEREBRAS_API_KEY",
        local: false,
    },
    Preset {
        name: "together",
        base_url: "https://api.together.xyz/v1",
        base_url_env: "TOGETHER_BASE_URL",
        api_key_env: "TOGETHER_API_KEY",
        local: false,
    },
    Preset {
        name: "fireworks",
        base_url: "https://api.fireworks.ai/inference/v1",
        base_url_env: "FIREWORKS_BASE_URL",
        api_key_env: "FIREWORKS_API_KEY",
        local: false,
    },
];

static CUSTOM: RwLock<Vec<CompatProvider>> = RwLock::new(Vec::new());

/// Fournisseurs déclarés dans la config ; ils remplacent un préréglage de même nom.
pub fn register_compat_providers(providers: Vec<CompatProvider>) {
    *CUSTOM
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = providers;
}

fn env_non_empty(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn preset_provider(preset: &Preset) -> CompatProvider {
    let mut base_url =
        env_non_empty(preset.base_url_env).unwrap_or_else(|| preset.base_url.to_string());
    // `OLLAMA_HOST` (convention d'Ollama) : `hôte:port` ou URL complète.
    if preset.name == "ollama" && env_non_empty(preset.base_url_env).is_none() {
        if let Some(host) = env_non_empty("OLLAMA_HOST") {
            let host = host.trim_end_matches('/');
            base_url = if host.starts_with("http") {
                format!("{host}/v1")
            } else {
                format!("http://{host}/v1")
            };
        }
    }
    CompatProvider {
        name: preset.name.to_string(),
        base_url,
        api_key_env: Some(preset.api_key_env.to_string()),
        requires_key: !preset.local,
        local: preset.local,
        context_window: None,
        max_output_tokens: None,
        headers: Vec::new(),
        text_tools: false,
    }
}

/// Tous les fournisseurs connus : config d'abord, puis préréglages.
#[must_use]
pub fn compat_providers() -> Vec<CompatProvider> {
    let custom = CUSTOM
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    let mut providers = custom.clone();
    for preset in PRESETS {
        if !custom
            .iter()
            .any(|provider| provider.name.eq_ignore_ascii_case(preset.name))
        {
            providers.push(preset_provider(preset));
        }
    }
    providers
}

/// `fournisseur:modèle` → (fournisseur, modèle). Le préfixe ne contient jamais
/// de `/`, ce qui le distingue des identifiants `OpenRouter` (`qwen/qwen3-coder:free`).
#[must_use]
pub fn split_compat_model(model: &str) -> Option<(CompatProvider, String)> {
    let (prefix, name) = model.trim().split_once(':')?;
    if prefix.is_empty() || name.is_empty() || prefix.contains('/') {
        return None;
    }
    compat_providers()
        .into_iter()
        .find(|provider| provider.name.eq_ignore_ascii_case(prefix))
        .map(|provider| (provider, name.to_string()))
}

impl CompatProvider {
    /// Clé lue dans l'environnement, s'il y en a une.
    #[must_use]
    pub fn api_key(&self) -> Option<String> {
        self.api_key_env.as_deref().and_then(env_non_empty)
    }

    /// Utilisable maintenant : serveur local, ou clé présente.
    #[must_use]
    pub fn is_configured(&self) -> bool {
        !self.requires_key || self.api_key().is_some()
    }

    #[must_use]
    pub fn default_context_window(&self) -> u64 {
        // Les serveurs locaux tournent souvent avec un contexte réduit : 32 k
        // est un compromis prudent (réglable par `contextWindow`).
        self.context_window
            .unwrap_or(if self.local { 32_768 } else { 128_000 })
    }

    #[must_use]
    pub fn default_max_output_tokens(&self) -> u32 {
        self.max_output_tokens
            .unwrap_or(if self.local { 8_192 } else { 16_384 })
    }
}

#[cfg(test)]
mod tests {
    use super::{register_compat_providers, split_compat_model, CompatProvider};

    #[test]
    fn splits_prefixed_models_but_not_openrouter_ids() {
        let (provider, model) = split_compat_model("ollama:qwen2.5-coder:7b").expect("ollama");
        assert_eq!(provider.name, "ollama");
        assert_eq!(model, "qwen2.5-coder:7b");
        assert!(provider.local && !provider.requires_key);

        let (provider, model) = split_compat_model("groq:llama-3.3-70b-versatile").expect("groq");
        assert_eq!(provider.base_url, "https://api.groq.com/openai/v1");
        assert_eq!(model, "llama-3.3-70b-versatile");
        assert!(provider.requires_key);

        assert!(split_compat_model("qwen/qwen3-coder:free").is_none());
        assert!(split_compat_model("inconnu:modele").is_none());
        assert!(split_compat_model("claude-sonnet-4-6").is_none());
    }

    #[test]
    fn configured_providers_override_presets() {
        register_compat_providers(vec![CompatProvider {
            name: "ollama".to_string(),
            base_url: "http://gpu-box:11434/v1".to_string(),
            api_key_env: None,
            requires_key: false,
            local: true,
            context_window: Some(65_536),
            max_output_tokens: None,
            headers: Vec::new(),
            text_tools: false,
        }]);
        let (provider, _) = split_compat_model("ollama:qwen3").expect("ollama");
        assert_eq!(provider.base_url, "http://gpu-box:11434/v1");
        assert_eq!(provider.default_context_window(), 65_536);
        register_compat_providers(Vec::new());
    }
}
