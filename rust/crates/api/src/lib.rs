mod client;
mod error;
mod openrouter;
mod providers;
mod sse;
mod types;

pub use openrouter::{
    fetch_free_quota, fetch_model_catalog, model_limits, rank_free_coding_models,
    register_model_limits, FreeModelQuota, ModelCatalog, ModelLimits, OpenRouterModel, RankedModel,
};

pub use client::{
    has_anthropic_credentials, oauth_token_is_expired, read_base_url, read_openrouter_base_url,
    read_xai_base_url, resolve_saved_oauth_token, resolve_startup_auth_source,
    summarize_transcript, MessageStream, OAuthTokenSet, ProviderClient,
};
pub use error::ApiError;
pub use providers::claw_provider::{AuthSource, ClawApiClient, ClawApiClient as ApiClient};
pub use providers::openai_compat::{OpenAiCompatClient, OpenAiCompatConfig};
pub use providers::{
    auto_compaction_threshold, context_window_for_model, detect_provider_kind,
    max_tokens_for_model, resolve_model_alias, ProviderKind,
};
pub use sse::{parse_frame, SseParser};
pub use types::{
    ContentBlockDelta, ContentBlockDeltaEvent, ContentBlockStartEvent, ContentBlockStopEvent,
    InputContentBlock, InputMessage, MessageDelta, MessageDeltaEvent, MessageRequest,
    MessageResponse, MessageStartEvent, MessageStopEvent, OutputContentBlock, StreamEvent,
    ToolChoice, ToolDefinition, ToolResultContentBlock, Usage,
};
