mod client;
mod error;
mod openrouter;
mod providers;
mod routing;
mod sse;
mod types;

pub use openrouter::{
    fetch_free_quota, fetch_model_catalog, model_limits, rank_free_coding_models,
    register_model_limits, FreeModelQuota, ModelCatalog, ModelLimits, OpenRouterModel, RankedModel,
};

pub use client::{
    has_anthropic_credentials, oauth_token_is_expired, read_base_url, read_openrouter_base_url,
    read_xai_base_url, resolve_saved_oauth_token, resolve_startup_auth_source, response_text,
    summarize_transcript, summary_request, MessageStream, OAuthTokenSet, ProviderClient,
};
pub use error::ApiError;
pub use providers::claw_provider::{AuthSource, ClawApiClient, ClawApiClient as ApiClient};
pub use providers::compat::{
    compat_providers, register_compat_providers, split_compat_model, CompatProvider,
};
pub use providers::openai_compat::{OpenAiCompatClient, OpenAiCompatConfig};
pub use providers::reasoning::{reasoning_effort, set_reasoning_effort, ReasoningEffort};
pub use providers::{
    auto_compaction_threshold, context_window_for_model, detect_provider_kind,
    max_tokens_for_model, resolve_model_alias, ProviderKind,
};
pub use routing::{
    mark_model_unavailable, model_available, reset_model_cooldowns, ClientFactory, ModelChain,
    OpenedStream,
};
pub use sse::{parse_frame, SseParser};
pub use types::{
    ContentBlockDelta, ContentBlockDeltaEvent, ContentBlockStartEvent, ContentBlockStopEvent,
    ImageSource, InputContentBlock, InputMessage, MessageDelta, MessageDeltaEvent, MessageRequest,
    MessageResponse, MessageStartEvent, MessageStopEvent, OutputContentBlock, StreamEvent,
    ToolChoice, ToolDefinition, ToolResultContentBlock, Usage,
};
