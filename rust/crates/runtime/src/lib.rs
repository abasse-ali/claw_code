pub mod attachments;
mod bash;
pub mod bash_tasks;
mod bootstrap;
pub mod checkpoint;
pub mod claude_skills;
mod compact;
mod config;
mod conversation;
mod file_ops;
pub mod harness;
mod hooks;
pub mod interrupt;
mod json;
pub mod json_repair;
mod mcp;
mod mcp_client;
mod mcp_stdio;
mod oauth;
mod paths;
mod permissions;
mod prompt;
mod remote;
pub mod repo_map;
pub mod sandbox;
pub use bash::stop_all_containers;
mod session;
mod usage;

pub use bash::{execute_bash, BashCommandInput, BashCommandOutput};
pub use bootstrap::{BootstrapPhase, BootstrapPlan};
pub use compact::{
    compact_session, compact_session_with, estimate_session_tokens, format_compact_summary,
    get_compact_continuation_message, render_transcript_for_summary, should_compact,
    CompactionConfig, CompactionResult, MODEL_SUMMARY_PROMPT,
};
pub use config::{
    ConfigEntry, ConfigError, ConfigLoader, ConfigSource, HookCommandConfig, McpConfigCollection,
    McpManagedProxyServerConfig, McpOAuthConfig, McpRemoteServerConfig, McpSdkServerConfig,
    McpServerConfig, McpStdioServerConfig, McpTransport, McpWebSocketServerConfig, OAuthConfig,
    ResolvedPermissionMode, RuntimeConfig, RuntimeFeatureConfig, RuntimeHookConfig,
    RuntimePermissionRuleConfig, RuntimePluginConfig, RuntimeProviderConfig, ScopedMcpServerConfig,
    CLAW_SETTINGS_SCHEMA_NAME,
};
pub use conversation::{
    ApiClient, ApiRequest, AssistantEvent, AutoCompactionConfig, ConsultPurpose, Consultation,
    ConversationRuntime, RuntimeError, StaticToolExecutor, ToolError, ToolExecutor, TurnSummary,
    HOOK_CONTEXT_PREFIX, STOP_HOOK_PREFIX,
};
pub use file_ops::{
    edit_file, glob_search, grep_search, read_file, write_file, EditFileOutput, GlobSearchOutput,
    GrepSearchInput, GrepSearchOutput, ReadFileOutput, StructuredPatchHunk, TextFilePayload,
    WriteFileOutput,
};
pub use hooks::{set_session_info as set_hook_session_info, HookEvent, HookRunResult, HookRunner};
pub use lsp::{
    FileDiagnostics, LspContextEnrichment, LspError, LspManager, LspServerConfig, SymbolLocation,
    WorkspaceDiagnostics,
};
pub use mcp::{
    mcp_server_signature, mcp_tool_name, mcp_tool_prefix, normalize_name_for_mcp,
    scoped_mcp_config_hash, unwrap_ccr_proxy_url,
};
pub use mcp_client::{
    McpClientAuth, McpClientBootstrap, McpClientTransport, McpManagedProxyTransport,
    McpRemoteTransport, McpSdkTransport, McpStdioTransport,
};
pub use mcp_stdio::{
    mcp_server_log_path, spawn_mcp_stdio_process, JsonRpcError, JsonRpcId, JsonRpcRequest,
    JsonRpcResponse, ManagedMcpTool, McpDiscovery, McpInitializeClientInfo, McpInitializeParams,
    McpInitializeResult, McpInitializeServerInfo, McpListResourcesParams, McpListResourcesResult,
    McpListToolsParams, McpListToolsResult, McpReadResourceParams, McpReadResourceResult,
    McpResource, McpResourceContents, McpServerManager, McpServerManagerError, McpStdioProcess,
    McpTool, McpToolCallContent, McpToolCallParams, McpToolCallResult, UnsupportedMcpServer,
};
pub use oauth::{
    clear_oauth_credentials, code_challenge_s256, credentials_path, generate_pkce_pair,
    generate_state, load_oauth_credentials, loopback_redirect_uri, parse_oauth_callback_query,
    parse_oauth_callback_request_target, save_oauth_credentials, OAuthAuthorizationRequest,
    OAuthCallbackParams, OAuthRefreshRequest, OAuthTokenExchangeRequest, OAuthTokenSet,
    PkceChallengeMethod, PkceCodePair,
};
pub use paths::{find_executable, home_dir};
pub use permissions::{
    request_mode_switch, PermissionMode, PermissionOutcome, PermissionPolicy,
    PermissionPromptDecision, PermissionPrompter, PermissionRequest, PermissionRule,
    PermissionRules,
};
pub use prompt::{
    current_date_iso, load_system_prompt, prepend_bullets, ContextFile, ProjectContext,
    PromptBuildError, SystemPromptBuilder, FRONTIER_MODEL_NAME, SYSTEM_PROMPT_DYNAMIC_BOUNDARY,
};
pub use remote::{
    inherited_upstream_proxy_env, no_proxy_list, read_token, upstream_proxy_ws_url,
    RemoteSessionContext, UpstreamProxyBootstrap, UpstreamProxyState, DEFAULT_REMOTE_BASE_URL,
    DEFAULT_SESSION_TOKEN_PATH, DEFAULT_SYSTEM_CA_BUNDLE, NO_PROXY_HOSTS, UPSTREAM_PROXY_ENV_KEYS,
};
pub use session::{ContentBlock, ConversationMessage, MessageRole, Session, SessionError};
pub use usage::{
    format_usd, pricing_for_model, ModelPricing, TokenUsage, UsageCostEstimate, UsageTracker,
};

#[cfg(test)]
pub(crate) fn test_env_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
    LOCK.get_or_init(|| std::sync::Mutex::new(()))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}
