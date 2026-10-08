use crate::error::ApiError;
use crate::providers::claw_provider::{self, AuthSource, ClawApiClient};
use crate::providers::openai_compat::{self, OpenAiCompatClient, OpenAiCompatConfig};
use crate::providers::{self, Provider, ProviderKind};
use crate::types::{MessageRequest, MessageResponse, StreamEvent};

async fn send_via_provider<P: Provider>(
    provider: &P,
    request: &MessageRequest,
) -> Result<MessageResponse, ApiError> {
    provider.send_message(request).await
}

async fn stream_via_provider<P: Provider>(
    provider: &P,
    request: &MessageRequest,
) -> Result<P::Stream, ApiError> {
    provider.stream_message(request).await
}

#[derive(Debug, Clone)]
pub enum ProviderClient {
    ClawApi(ClawApiClient),
    Xai(OpenAiCompatClient),
    OpenAi(OpenAiCompatClient),
    OpenRouter(OpenAiCompatClient),
    Compat(OpenAiCompatClient),
}

impl ProviderClient {
    pub fn from_model(model: &str) -> Result<Self, ApiError> {
        Self::from_model_with_default_auth(model, None)
    }

    pub fn from_model_with_default_auth(
        model: &str,
        default_auth: Option<AuthSource>,
    ) -> Result<Self, ApiError> {
        let resolved_model = providers::resolve_model_alias(model);
        match providers::detect_provider_kind(&resolved_model) {
            ProviderKind::ClawApi => Ok(Self::ClawApi(match default_auth {
                Some(auth) => ClawApiClient::from_auth(auth),
                None => ClawApiClient::from_env()?,
            })),
            ProviderKind::Xai => Ok(Self::Xai(OpenAiCompatClient::from_env(
                OpenAiCompatConfig::xai(),
            )?)),
            ProviderKind::OpenAi => Ok(Self::OpenAi(OpenAiCompatClient::from_env(
                OpenAiCompatConfig::openai(),
            )?)),
            ProviderKind::OpenRouter => Ok(Self::OpenRouter(OpenAiCompatClient::from_env(
                OpenAiCompatConfig::openrouter(),
            )?)),
            ProviderKind::Compat => {
                let (provider, name) = providers::compat::split_compat_model(&resolved_model)
                    .ok_or_else(|| ApiError::Auth(format!("fournisseur inconnu pour {model}")))?;
                Ok(Self::Compat(OpenAiCompatClient::for_compat_provider(
                    &provider, &name,
                )?))
            }
        }
    }

    #[must_use]
    pub const fn provider_kind(&self) -> ProviderKind {
        match self {
            Self::ClawApi(_) => ProviderKind::ClawApi,
            Self::Xai(_) => ProviderKind::Xai,
            Self::OpenAi(_) => ProviderKind::OpenAi,
            Self::OpenRouter(_) => ProviderKind::OpenRouter,
            Self::Compat(_) => ProviderKind::Compat,
        }
    }

    pub async fn send_message(
        &self,
        request: &MessageRequest,
    ) -> Result<MessageResponse, ApiError> {
        match self {
            Self::ClawApi(client) => send_via_provider(client, request).await,
            Self::Xai(client)
            | Self::OpenAi(client)
            | Self::OpenRouter(client)
            | Self::Compat(client) => send_via_provider(client, request).await,
        }
    }

    pub async fn stream_message(
        &self,
        request: &MessageRequest,
    ) -> Result<MessageStream, ApiError> {
        match self {
            Self::ClawApi(client) => stream_via_provider(client, request)
                .await
                .map(MessageStream::ClawApi),
            Self::Xai(client)
            | Self::OpenAi(client)
            | Self::OpenRouter(client)
            | Self::Compat(client) => stream_via_provider(client, request)
                .await
                .map(MessageStream::OpenAiCompat),
        }
    }
}

#[derive(Debug)]
pub enum MessageStream {
    ClawApi(claw_provider::MessageStream),
    OpenAiCompat(openai_compat::MessageStream),
}

impl MessageStream {
    #[must_use]
    pub fn request_id(&self) -> Option<&str> {
        match self {
            Self::ClawApi(stream) => stream.request_id(),
            Self::OpenAiCompat(stream) => stream.request_id(),
        }
    }

    pub async fn next_event(&mut self) -> Result<Option<StreamEvent>, ApiError> {
        match self {
            Self::ClawApi(stream) => stream.next_event().await,
            Self::OpenAiCompat(stream) => stream.next_event().await,
        }
    }
}

pub use claw_provider::{
    oauth_token_is_expired, resolve_saved_oauth_token, resolve_startup_auth_source, OAuthTokenSet,
};
#[must_use]
pub fn read_base_url() -> String {
    claw_provider::read_base_url()
}

#[must_use]
pub fn read_xai_base_url() -> String {
    openai_compat::read_base_url(OpenAiCompatConfig::xai())
}

#[must_use]
pub fn read_openrouter_base_url() -> String {
    openai_compat::read_base_url(OpenAiCompatConfig::openrouter())
}

/// Requête de résumé d'une conversation, sans outils (compaction du contexte).
#[must_use]
pub fn summary_request(model: &str, system_prompt: &str, transcript: &str) -> MessageRequest {
    MessageRequest {
        model: model.to_string(),
        max_tokens: providers::max_tokens_for_model(model).min(8_192),
        messages: vec![crate::types::InputMessage::user_text(format!(
            "Conversation to summarize:\n\n{transcript}"
        ))],
        system: Some(system_prompt.to_string()),
        tools: None,
        tool_choice: None,
        stream: false,
    }
}

/// Texte d'une réponse complète (blocs de texte mis bout à bout).
#[must_use]
pub fn response_text(response: &MessageResponse) -> String {
    response
        .content
        .iter()
        .filter_map(|block| match block {
            crate::types::OutputContentBlock::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("")
}

/// Fait résumer `transcript` par le modèle, sans outils (compaction du contexte).
pub async fn summarize_transcript(
    client: &ProviderClient,
    model: &str,
    system_prompt: &str,
    transcript: &str,
) -> Result<String, ApiError> {
    let response = client
        .send_message(&summary_request(model, system_prompt, transcript))
        .await?;
    Ok(response_text(&response))
}

/// Vrai si une clé Anthropic ou une connexion OAuth sauvegardée est disponible.
#[must_use]
pub fn has_anthropic_credentials() -> bool {
    claw_provider::has_auth_from_env_or_saved().unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use crate::providers::{detect_provider_kind, resolve_model_alias, ProviderKind};

    #[test]
    fn resolves_existing_and_grok_aliases() {
        assert_eq!(resolve_model_alias("opus"), "claude-opus-4-6");
        assert_eq!(resolve_model_alias("grok"), "grok-3");
        assert_eq!(resolve_model_alias("grok-mini"), "grok-3-mini");
    }

    #[test]
    fn provider_detection_prefers_model_family() {
        assert_eq!(detect_provider_kind("grok-3"), ProviderKind::Xai);
        assert_eq!(
            detect_provider_kind("claude-sonnet-4-6"),
            ProviderKind::ClawApi
        );
    }
}
