use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use serde::Deserialize;
use serde_json::{json, Value};

use crate::error::ApiError;
use crate::types::{
    ContentBlockDelta, ContentBlockDeltaEvent, ContentBlockStartEvent, ContentBlockStopEvent,
    InputContentBlock, InputMessage, MessageDelta, MessageDeltaEvent, MessageRequest,
    MessageResponse, MessageStartEvent, MessageStopEvent, OutputContentBlock, StreamEvent,
    ToolChoice, ToolDefinition, ToolResultContentBlock, Usage,
};

use super::reasoning::{self, ReasoningEffort, ThinkTagFilter};
use super::text_tools::{self, TextToolFilter};
use super::{Provider, ProviderFuture};

pub const DEFAULT_XAI_BASE_URL: &str = "https://api.x.ai/v1";
pub const DEFAULT_OPENAI_BASE_URL: &str = "https://api.openai.com/v1";
pub const DEFAULT_OPENROUTER_BASE_URL: &str = "https://openrouter.ai/api/v1";
const DEFAULT_OPENROUTER_APP_TITLE: &str = "Claw Code";
const REQUEST_ID_HEADER: &str = "request-id";
const ALT_REQUEST_ID_HEADER: &str = "x-request-id";
// Surcharges passagères des fournisseurs (429 « temporarily rate-limited », 503) :
// quelques secondes d'attente avant de basculer sur un autre modèle.
const DEFAULT_INITIAL_BACKOFF: Duration = Duration::from_millis(800);
const DEFAULT_MAX_BACKOFF: Duration = Duration::from_secs(4);
/// Attente maximale de la remise à zéro d'une limite par minute.
const MAX_RATE_LIMIT_WAIT: Duration = Duration::from_secs(70);
const DEFAULT_MAX_RETRIES: u32 = 2;

/// Raisonnement d'un modèle `OpenRouter`, gardé pour être renvoyé à l'étape
/// suivante du même tour (le modèle reprend sa réflexion au lieu de repartir
/// de zéro après chaque outil). Il voyage dans le champ `signature` du bloc
/// de raisonnement : texte simple, ou `reasoning_details` tels que reçus.
const REASONING_PLAIN: &str = "or-plain";
const REASONING_DETAILS_PREFIX: &str = "or-details:";
/// Bloc du flux qui porte le raisonnement.
const THINKING_INDEX: u32 = 999;
/// Raisonnement renvoyé au plus, en caractères (la fin : la conclusion).
const MAX_PASSBACK_CHARS: usize = 8_000;

/// Ce bloc de raisonnement vient d'un modèle `OpenRouter` (et non d'un
/// raisonnement signé de Claude, à ne renvoyer qu'à l'API d'Anthropic).
#[must_use]
pub fn is_passback_reasoning(signature: &str) -> bool {
    signature == REASONING_PLAIN || signature.starts_with(REASONING_DETAILS_PREFIX)
}

/// Fusionne un fragment de `reasoning_details` reçu dans le flux : les textes
/// d'un même élément (même type, même index) se suivent, le reste est copié.
fn merge_reasoning_detail(details: &mut Vec<Value>, fragment: Value) {
    let key = |value: &Value| {
        (
            value
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            value.get("index").and_then(Value::as_u64),
        )
    };
    let fragment_key = key(&fragment);
    let Some(existing) = details
        .iter_mut()
        .find(|detail| key(detail) == fragment_key)
        .and_then(Value::as_object_mut)
    else {
        details.push(fragment);
        return;
    };
    let Value::Object(source) = fragment else {
        return;
    };
    for (name, value) in source {
        let appended = matches!(name.as_str(), "text" | "summary" | "data");
        match (existing.get_mut(&name), value) {
            (Some(Value::String(current)), Value::String(addition)) if appended => {
                current.push_str(&addition);
            }
            (Some(current), value) if current.is_null() || !appended => {
                if !value.is_null() {
                    *current = value;
                }
            }
            (None, value) => {
                existing.insert(name, value);
            }
            _ => {}
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OpenAiCompatConfig {
    pub provider_name: &'static str,
    pub api_key_env: &'static str,
    pub base_url_env: &'static str,
    pub default_base_url: &'static str,
}

const XAI_ENV_VARS: &[&str] = &["XAI_API_KEY"];
const OPENAI_ENV_VARS: &[&str] = &["OPENAI_API_KEY"];
const OPENROUTER_ENV_VARS: &[&str] = &["OPENROUTER_API_KEY"];

impl OpenAiCompatConfig {
    #[must_use]
    pub const fn openrouter() -> Self {
        Self {
            provider_name: "OpenRouter",
            api_key_env: "OPENROUTER_API_KEY",
            base_url_env: "OPENROUTER_BASE_URL",
            default_base_url: DEFAULT_OPENROUTER_BASE_URL,
        }
    }

    #[must_use]
    pub const fn xai() -> Self {
        Self {
            provider_name: "xAI",
            api_key_env: "XAI_API_KEY",
            base_url_env: "XAI_BASE_URL",
            default_base_url: DEFAULT_XAI_BASE_URL,
        }
    }

    #[must_use]
    pub const fn openai() -> Self {
        Self {
            provider_name: "OpenAI",
            api_key_env: "OPENAI_API_KEY",
            base_url_env: "OPENAI_BASE_URL",
            default_base_url: DEFAULT_OPENAI_BASE_URL,
        }
    }
    #[must_use]
    pub fn credential_env_vars(self) -> &'static [&'static str] {
        match self.provider_name {
            "xAI" => XAI_ENV_VARS,
            "OpenAI" => OPENAI_ENV_VARS,
            "OpenRouter" => OPENROUTER_ENV_VARS,
            _ => &[],
        }
    }

    // En-têtes propres au fournisseur. OpenRouter utilise `HTTP-Referer` et
    // `X-Title` pour attribuer les requêtes à une application (facultatif).
    fn extra_headers(self) -> Vec<(String, String)> {
        if self.provider_name != "OpenRouter" {
            return Vec::new();
        }
        let title = read_env_non_empty("OPENROUTER_APP_TITLE")
            .ok()
            .flatten()
            .unwrap_or_else(|| DEFAULT_OPENROUTER_APP_TITLE.to_string());
        let mut headers = vec![("X-Title".to_string(), title)];
        if let Some(url) = read_env_non_empty("OPENROUTER_APP_URL").ok().flatten() {
            headers.push(("HTTP-Referer".to_string(), url));
        }
        headers
    }
}

/// Serveurs qui ont refusé le champ `stream_options` pendant cette exécution.
fn stream_options_refusals() -> &'static Mutex<BTreeSet<String>> {
    static REFUSALS: OnceLock<Mutex<BTreeSet<String>>> = OnceLock::new();
    REFUSALS.get_or_init(|| Mutex::new(BTreeSet::new()))
}

/// Le serveur accepte `stream_options` (usage des jetons en fin de flux).
/// L'API de Mistral refuse tout champ inconnu de sa spécification, et celui-ci
/// n'y figure pas ; elle envoie l'usage dans le dernier morceau sans qu'on le
/// demande. Un serveur qui l'a déjà refusé est retenu.
fn accepts_stream_options(base_url: &str) -> bool {
    !base_url.contains("api.mistral.ai")
        && !stream_options_refusals()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .contains(base_url)
}

fn remember_stream_options_refusal(base_url: &str) {
    stream_options_refusals()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .insert(base_url.to_string());
}

/// L'erreur vient du champ `stream_options`, que ce serveur ne connaît pas.
fn is_stream_options_error(error: &ApiError) -> bool {
    matches!(error, ApiError::Api { status, .. } if matches!(status.as_u16(), 400 | 422))
        && error.to_string().contains("stream_options")
}

#[derive(Debug, Clone)]
pub struct OpenAiCompatClient {
    http: reqwest::Client,
    api_key: String,
    base_url: String,
    /// Nom du modèle envoyé au serveur (sans le préfixe `fournisseur:`).
    model_name: Option<String>,
    /// `OpenRouter`, `OpenAI`, `ollama`… (format du paramètre de raisonnement).
    provider_name: String,
    extra_headers: Vec<(String, String)>,
    max_retries: u32,
    initial_backoff: Duration,
    max_backoff: Duration,
}

impl OpenAiCompatClient {
    #[must_use]
    pub fn new(api_key: impl Into<String>, config: OpenAiCompatConfig) -> Self {
        let base_url = read_base_url(config);
        Self {
            http: super::http_client_for(&base_url),
            api_key: api_key.into(),
            base_url,
            model_name: None,
            provider_name: config.provider_name.to_string(),
            extra_headers: config.extra_headers(),
            max_retries: DEFAULT_MAX_RETRIES,
            initial_backoff: DEFAULT_INITIAL_BACKOFF,
            max_backoff: DEFAULT_MAX_BACKOFF,
        }
    }

    pub fn from_env(config: OpenAiCompatConfig) -> Result<Self, ApiError> {
        let Some(api_key) = read_env_non_empty(config.api_key_env)? else {
            return Err(ApiError::missing_credentials(
                config.provider_name,
                config.credential_env_vars(),
            ));
        };
        Ok(Self::new(api_key, config))
    }

    /// Client pour un fournisseur compatible (`ollama:`, `groq:`, serveur de la config).
    pub fn for_compat_provider(
        provider: &super::compat::CompatProvider,
        model_name: &str,
    ) -> Result<Self, ApiError> {
        let api_key = provider.api_key().unwrap_or_default();
        if provider.requires_key && api_key.is_empty() {
            return Err(ApiError::Auth(format!(
                "clé manquante pour le fournisseur {} : définissez {}",
                provider.name,
                provider
                    .api_key_env
                    .as_deref()
                    .unwrap_or("la variable de clé")
            )));
        }
        Ok(Self {
            http: super::http_client_for(&provider.base_url),
            api_key,
            base_url: provider.base_url.clone(),
            model_name: Some(model_name.to_string()),
            provider_name: provider.name.clone(),
            extra_headers: provider.headers.clone(),
            max_retries: DEFAULT_MAX_RETRIES,
            initial_backoff: DEFAULT_INITIAL_BACKOFF,
            max_backoff: DEFAULT_MAX_BACKOFF,
        })
    }

    /// Modèles proposés par le serveur (`GET /models`).
    pub async fn list_models(&self) -> Result<Vec<String>, ApiError> {
        let url = format!("{}/models", self.base_url.trim_end_matches('/'));
        let mut builder = self.http.get(&url);
        if !self.api_key.is_empty() {
            builder = builder.bearer_auth(&self.api_key);
        }
        let response = expect_success(builder.send().await?).await?;
        let payload = response.json::<Value>().await?;
        let mut models = payload
            .get("data")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| item.get("id").and_then(Value::as_str))
                    .map(ToOwned::to_owned)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        models.sort();
        Ok(models)
    }

    #[must_use]
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into();
        self
    }

    #[must_use]
    pub fn with_retry_policy(
        mut self,
        max_retries: u32,
        initial_backoff: Duration,
        max_backoff: Duration,
    ) -> Self {
        self.max_retries = max_retries;
        self.initial_backoff = initial_backoff;
        self.max_backoff = max_backoff;
        self
    }

    pub async fn send_message(
        &self,
        request: &MessageRequest,
    ) -> Result<MessageResponse, ApiError> {
        let request = MessageRequest {
            stream: false,
            ..request.clone()
        };
        let (response, text_mode) = self.send_negotiated(&request).await?;
        let request_id = request_id_from_headers(response.headers());
        let payload = parse_chat_completion(&response.bytes().await?)?;
        let known_tools: Vec<String> = request
            .tools
            .iter()
            .flatten()
            .map(|tool| tool.name.clone())
            .collect();
        let mut normalized = normalize_response(&request.model, payload, text_mode, &known_tools)?;
        if normalized.request_id.is_none() {
            normalized.request_id = request_id;
        }
        Ok(normalized)
    }

    pub async fn stream_message(
        &self,
        request: &MessageRequest,
    ) -> Result<MessageStream, ApiError> {
        let (response, text_mode) = self
            .send_negotiated(&request.clone().with_streaming())
            .await?;
        let state = StreamState::new(request.model.clone())
            .with_tools(request, text_mode)
            .with_reasoning_passback(self.provider_name == "OpenRouter");
        Ok(MessageStream {
            request_id: request_id_from_headers(response.headers()),
            response,
            parser: OpenAiSseParser::new(),
            pending: VecDeque::new(),
            done: false,
            state,
        })
    }

    /// Envoie la requête avec des outils natifs, ou en mode texte si le modèle
    /// l'exige ; un refus du paramètre `tools` fait basculer (et retenir) le mode texte.
    async fn send_negotiated(
        &self,
        request: &MessageRequest,
    ) -> Result<(reqwest::Response, bool), ApiError> {
        let has_tools = request
            .tools
            .as_ref()
            .is_some_and(|tools| !tools.is_empty());
        let text_mode = has_tools && text_tools::text_tools_enabled(&request.model);
        let first = match self.send_with_retry(request, text_mode).await {
            // Serveur strict : il ne connaît pas `stream_options`. On le
            // retient et la même requête repart sans ce champ.
            Err(error)
                if request.stream
                    && accepts_stream_options(&self.base_url)
                    && is_stream_options_error(&error) =>
            {
                remember_stream_options_refusal(&self.base_url);
                self.send_with_retry(request, text_mode).await
            }
            other => other,
        };
        match first {
            Ok(response) => Ok((response, text_mode)),
            Err(error)
                if has_tools
                    && !text_mode
                    && text_tools::is_tools_unsupported_error(&error.to_string()) =>
            {
                text_tools::enable_text_tools(&request.model);
                Ok((self.send_with_retry(request, true).await?, true))
            }
            Err(error) => Err(error),
        }
    }

    async fn send_with_retry(
        &self,
        request: &MessageRequest,
        text_mode: bool,
    ) -> Result<reqwest::Response, ApiError> {
        let mut attempts = 0;

        let last_error = loop {
            attempts += 1;
            let retryable_error = match self.send_raw_request(request, text_mode).await {
                Ok(response) => match expect_success(response).await {
                    Ok(response) => return Ok(response),
                    Err(error) if error.is_retryable() && attempts <= self.max_retries + 1 => error,
                    Err(error) => return Err(error),
                },
                Err(error) if error.is_retryable() && attempts <= self.max_retries + 1 => error,
                Err(error) => return Err(error),
            };

            if attempts > self.max_retries {
                break retryable_error;
            }

            // Limite par minute (20 requêtes pour les modèles gratuits d'OpenRouter) :
            // attendre sa remise à zéro plutôt que d'échouer. Au-delà (quota du
            // jour), on rend l'erreur pour basculer sur un autre modèle.
            let delay = match retryable_error.rate_limit_reset() {
                Some(reset) if reset <= MAX_RATE_LIMIT_WAIT => reset + Duration::from_millis(500),
                Some(_) => break retryable_error,
                None => self.backoff_for_attempt(attempts)?,
            };
            tokio::time::sleep(delay).await;
        };

        Err(ApiError::RetriesExhausted {
            attempts,
            last_error: Box::new(last_error),
        })
    }

    async fn send_raw_request(
        &self,
        request: &MessageRequest,
        text_mode: bool,
    ) -> Result<reqwest::Response, ApiError> {
        let request_url = chat_completions_endpoint(&self.base_url);
        let mut builder = self
            .http
            .post(&request_url)
            .header("content-type", "application/json");
        // Les serveurs locaux n'ont souvent pas de clé.
        if !self.api_key.is_empty() {
            builder = builder.bearer_auth(&self.api_key);
        }
        for (name, value) in &self.extra_headers {
            builder = builder.header(name.as_str(), value.as_str());
        }
        let mut payload = build_chat_completion_request_with(
            request,
            text_mode,
            self.provider_name == "OpenRouter",
        );
        if let Some(name) = &self.model_name {
            payload["model"] = Value::String(name.clone());
        }
        if !accepts_stream_options(&self.base_url) {
            if let Some(fields) = payload.as_object_mut() {
                fields.remove("stream_options");
            }
        }
        let openrouter = self.provider_name == "OpenRouter";
        let model_reasons = crate::openrouter::model_limits(&request.model)
            .is_some_and(|limits| limits.supports_reasoning);
        if let Some(effort) = reasoning::effort_for_request(
            reasoning::reasoning_effort(),
            &request.model,
            openrouter,
            model_reasons,
        ) {
            if openrouter {
                payload["reasoning"] = json!({ "effort": effort.as_str() });
            } else {
                payload["reasoning_effort"] = Value::String(effort.as_str().to_string());
            }
        }
        builder.json(&payload).send().await.map_err(ApiError::from)
    }

    fn backoff_for_attempt(&self, attempt: u32) -> Result<Duration, ApiError> {
        let Some(multiplier) = 1_u32.checked_shl(attempt.saturating_sub(1)) else {
            return Err(ApiError::BackoffOverflow {
                attempt,
                base_delay: self.initial_backoff,
            });
        };
        Ok(self
            .initial_backoff
            .checked_mul(multiplier)
            .map_or(self.max_backoff, |delay| delay.min(self.max_backoff)))
    }
}

impl Provider for OpenAiCompatClient {
    type Stream = MessageStream;

    fn send_message<'a>(
        &'a self,
        request: &'a MessageRequest,
    ) -> ProviderFuture<'a, MessageResponse> {
        Box::pin(async move { self.send_message(request).await })
    }

    fn stream_message<'a>(
        &'a self,
        request: &'a MessageRequest,
    ) -> ProviderFuture<'a, Self::Stream> {
        Box::pin(async move { self.stream_message(request).await })
    }
}

#[derive(Debug)]
pub struct MessageStream {
    request_id: Option<String>,
    response: reqwest::Response,
    parser: OpenAiSseParser,
    pending: VecDeque<StreamEvent>,
    done: bool,
    state: StreamState,
}

impl MessageStream {
    #[must_use]
    pub fn request_id(&self) -> Option<&str> {
        self.request_id.as_deref()
    }

    pub async fn next_event(&mut self) -> Result<Option<StreamEvent>, ApiError> {
        loop {
            if let Some(event) = self.pending.pop_front() {
                return Ok(Some(event));
            }

            if self.done {
                self.pending.extend(self.state.finish());
                if let Some(event) = self.pending.pop_front() {
                    return Ok(Some(event));
                }
                return Ok(None);
            }

            match self.response.chunk().await? {
                Some(chunk) => {
                    for parsed in self.parser.push(&chunk)? {
                        self.pending.extend(self.state.ingest_chunk(parsed)?);
                    }
                }
                None => {
                    self.done = true;
                }
            }
        }
    }
}

#[derive(Debug, Default)]
struct OpenAiSseParser {
    buffer: Vec<u8>,
}

impl OpenAiSseParser {
    fn new() -> Self {
        Self::default()
    }

    fn push(&mut self, chunk: &[u8]) -> Result<Vec<ChatCompletionChunk>, ApiError> {
        self.buffer.extend_from_slice(chunk);
        let mut events = Vec::new();

        while let Some(frame) = next_sse_frame(&mut self.buffer) {
            if let Some(event) = parse_sse_frame(&frame)? {
                events.push(event);
            }
        }

        Ok(events)
    }
}

// Chaque booléen suit une étape distincte du flux (message, texte, réflexion, fin).
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug)]
struct StreamState {
    model: String,
    message_started: bool,
    text_started: bool,
    text_finished: bool,
    finished: bool,
    stop_reason: Option<String>,
    usage: Option<Usage>,
    tool_calls: BTreeMap<u32, ToolCallState>,
    /// Appels d'outils écrits dans le texte (`<tool_call>`), extraits en fin de flux.
    text_mode: bool,
    text_filter: TextToolFilter,
    /// Raisonnement écrit entre `<think>` dans le texte.
    think_filter: ThinkTagFilter,
    /// Outils proposés au modèle (pour reconnaître un appel écrit en JSON brut).
    known_tools: Vec<String>,
    /// Outils natifs : début de réponse retenu tant qu'il peut être un appel
    /// écrit en JSON brut (`{"name": …}`) ; `None` une fois le texte libéré.
    held_text: Option<String>,
    /// Le raisonnement est gardé pour être renvoyé à l'étape suivante.
    keep_reasoning: bool,
    thinking_started: bool,
    reasoning_details: Vec<Value>,
}

impl StreamState {
    fn new(model: String) -> Self {
        Self {
            model,
            message_started: false,
            text_started: false,
            text_finished: false,
            finished: false,
            stop_reason: None,
            usage: None,
            tool_calls: BTreeMap::new(),
            text_mode: false,
            text_filter: TextToolFilter::default(),
            think_filter: ThinkTagFilter::default(),
            known_tools: Vec::new(),
            held_text: None,
            keep_reasoning: false,
            thinking_started: false,
            reasoning_details: Vec::new(),
        }
    }

    fn with_reasoning_passback(mut self, keep: bool) -> Self {
        self.keep_reasoning = keep;
        self
    }

    /// Outils de la requête ; en mode natif, la réponse est retenue tant
    /// qu'elle peut être un appel écrit en JSON.
    fn with_tools(mut self, request: &MessageRequest, text_mode: bool) -> Self {
        self.text_mode = text_mode;
        self.known_tools = request
            .tools
            .iter()
            .flatten()
            .map(|tool| tool.name.clone())
            .collect();
        if !text_mode && !self.known_tools.is_empty() {
            self.held_text = Some(String::new());
        }
        self
    }

    /// Raisonnement du modèle, affiché à part (sauf `/think off`).
    fn push_thinking(&mut self, text: String, events: &mut Vec<StreamEvent>) {
        if text.is_empty() || reasoning::reasoning_effort() == Some(ReasoningEffort::Off) {
            return;
        }
        if self.keep_reasoning && !self.thinking_started {
            self.thinking_started = true;
            events.push(StreamEvent::ContentBlockStart(ContentBlockStartEvent {
                index: THINKING_INDEX,
                content_block: OutputContentBlock::Thinking {
                    thinking: String::new(),
                    signature: None,
                },
            }));
        }
        events.push(StreamEvent::ContentBlockDelta(ContentBlockDeltaEvent {
            index: THINKING_INDEX,
            delta: ContentBlockDelta::ThinkingDelta { thinking: text },
        }));
    }

    /// Fin du raisonnement gardé : sa « signature » dit comment le renvoyer.
    fn finish_thinking(&mut self, events: &mut Vec<StreamEvent>) {
        if !self.thinking_started {
            return;
        }
        self.thinking_started = false;
        let signature = if self.reasoning_details.is_empty() {
            REASONING_PLAIN.to_string()
        } else {
            format!(
                "{REASONING_DETAILS_PREFIX}{}",
                Value::Array(std::mem::take(&mut self.reasoning_details))
            )
        };
        events.push(StreamEvent::ContentBlockDelta(ContentBlockDeltaEvent {
            index: THINKING_INDEX,
            delta: ContentBlockDelta::SignatureDelta { signature },
        }));
        events.push(StreamEvent::ContentBlockStop(ContentBlockStopEvent {
            index: THINKING_INDEX,
        }));
    }

    fn push_answer_text(&mut self, text: String, events: &mut Vec<StreamEvent>) {
        let visible = if self.text_mode {
            self.text_filter.push(&text)
        } else if let Some(held) = self.held_text.as_mut() {
            held.push_str(&text);
            let start = held.trim_start();
            if start.is_empty() || start.starts_with('{') || start.starts_with('`') {
                return;
            }
            self.held_text.take().unwrap_or_default()
        } else {
            text
        };
        self.push_text(visible, events);
    }

    /// Fin du flux en mode natif : la réponse retenue est un appel écrit en
    /// JSON (converti en appel d'outil) ou du texte ordinaire (affiché).
    fn finish_held_text(&mut self, events: &mut Vec<StreamEvent>) {
        let Some(held) = self.held_text.take() else {
            return;
        };
        let calls = if self.tool_calls.is_empty() {
            text_tools::salvage_json_tool_calls(&held, &self.known_tools)
        } else {
            None
        };
        match calls {
            Some(calls) => self.push_text_tool_calls(calls, events),
            None => self.push_text(held, events),
        }
    }

    fn push_text(&mut self, text: String, events: &mut Vec<StreamEvent>) {
        if text.is_empty() {
            return;
        }
        if !self.text_started {
            self.text_started = true;
            events.push(StreamEvent::ContentBlockStart(ContentBlockStartEvent {
                index: 0,
                content_block: OutputContentBlock::Text {
                    text: String::new(),
                },
            }));
        }
        events.push(StreamEvent::ContentBlockDelta(ContentBlockDeltaEvent {
            index: 0,
            delta: ContentBlockDelta::TextDelta { text },
        }));
    }

    /// Fin du flux en mode texte : texte restant puis appels extraits.
    fn finish_text_tools(&mut self, events: &mut Vec<StreamEvent>) {
        let (rest, raw) = self.text_filter.finish();
        self.push_text(rest, events);
        let (_, mut calls) = text_tools::extract_tool_calls(&raw);
        if calls.is_empty() {
            calls =
                text_tools::salvage_json_tool_calls(&raw, &self.known_tools).unwrap_or_default();
        }
        if !calls.is_empty() {
            self.push_text_tool_calls(calls, events);
        }
    }

    /// Appels d'outils trouvés dans le texte, ajoutés comme des appels natifs.
    fn push_text_tool_calls(&mut self, calls: Vec<(String, Value)>, events: &mut Vec<StreamEvent>) {
        if self.text_started && !self.text_finished {
            self.text_finished = true;
            events.push(StreamEvent::ContentBlockStop(ContentBlockStopEvent {
                index: 0,
            }));
        }
        for (offset, (name, input)) in calls.into_iter().enumerate() {
            let index = 1_000 + u32::try_from(offset).unwrap_or(0);
            events.push(StreamEvent::ContentBlockStart(ContentBlockStartEvent {
                index,
                content_block: OutputContentBlock::ToolUse {
                    id: format!("text_call_{offset}"),
                    name,
                    input: json!({}),
                },
            }));
            events.push(StreamEvent::ContentBlockDelta(ContentBlockDeltaEvent {
                index,
                delta: ContentBlockDelta::InputJsonDelta {
                    partial_json: input.to_string(),
                },
            }));
            events.push(StreamEvent::ContentBlockStop(ContentBlockStopEvent {
                index,
            }));
        }
        self.stop_reason = Some("tool_use".to_string());
    }

    fn ingest_chunk(&mut self, chunk: ChatCompletionChunk) -> Result<Vec<StreamEvent>, ApiError> {
        // OpenRouter signale les erreurs survenues en cours de flux dans un chunk
        // `{"error": {...}}` après l'en-tête 200 : on les remonte comme erreur d'API.
        if let Some(error) = chunk.error {
            return Err(error_in_body(&error));
        }

        let mut events = Vec::new();
        if !self.message_started {
            self.message_started = true;
            events.push(StreamEvent::MessageStart(MessageStartEvent {
                message: MessageResponse {
                    id: chunk.id.clone(),
                    kind: "message".to_string(),
                    role: "assistant".to_string(),
                    content: Vec::new(),
                    model: chunk.model.clone().unwrap_or_else(|| self.model.clone()),
                    stop_reason: None,
                    stop_sequence: None,
                    usage: Usage {
                        input_tokens: 0,
                        cache_creation_input_tokens: 0,
                        cache_read_input_tokens: 0,
                        output_tokens: 0,
                    },
                    request_id: None,
                },
            }));
        }

        if let Some(usage) = chunk.usage {
            self.usage = Some(Usage {
                input_tokens: usage.prompt_tokens,
                cache_creation_input_tokens: 0,
                cache_read_input_tokens: 0,
                output_tokens: usage.completion_tokens,
            });
        }

        for choice in chunk.choices {
            let has_reasoning_text =
                choice.delta.reasoning.is_some() || choice.delta.reasoning_content.is_some();
            for thought in [choice.delta.reasoning, choice.delta.reasoning_content]
                .into_iter()
                .flatten()
            {
                self.push_thinking(thought, &mut events);
            }
            for detail in choice.delta.reasoning_details {
                // Sans champ `reasoning`, le texte des détails sert à l'affichage.
                if !has_reasoning_text {
                    let text = detail
                        .get("text")
                        .or_else(|| detail.get("summary"))
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string();
                    self.push_thinking(text, &mut events);
                }
                if self.keep_reasoning {
                    merge_reasoning_detail(&mut self.reasoning_details, detail);
                }
            }
            if let Some(content) = choice.delta.content.filter(|value| !value.is_empty()) {
                let (text, thought) = self.think_filter.push(&content);
                self.push_thinking(thought, &mut events);
                self.push_answer_text(text, &mut events);
            }

            for tool_call in choice.delta.tool_calls {
                let state = self.tool_calls.entry(tool_call.index).or_default();
                state.apply(tool_call);
                let block_index = state.block_index();
                if !state.started {
                    if let Some(start_event) = state.start_event() {
                        state.started = true;
                        events.push(StreamEvent::ContentBlockStart(start_event));
                    } else {
                        continue;
                    }
                }
                if let Some(delta_event) = state.delta_event() {
                    events.push(StreamEvent::ContentBlockDelta(delta_event));
                }
                if choice.finish_reason.as_deref() == Some("tool_calls") && !state.stopped {
                    state.stopped = true;
                    events.push(StreamEvent::ContentBlockStop(ContentBlockStopEvent {
                        index: block_index,
                    }));
                }
            }

            if let Some(finish_reason) = choice.finish_reason {
                self.stop_reason = Some(normalize_finish_reason(&finish_reason));
                if finish_reason == "tool_calls" {
                    for state in self.tool_calls.values_mut() {
                        if state.started && !state.stopped {
                            state.stopped = true;
                            events.push(StreamEvent::ContentBlockStop(ContentBlockStopEvent {
                                index: state.block_index(),
                            }));
                        }
                    }
                }
            }
        }

        Ok(events)
    }

    fn finish(&mut self) -> Vec<StreamEvent> {
        if self.finished {
            return Vec::new();
        }
        self.finished = true;

        let mut events = Vec::new();
        let (rest_text, rest_thought) = self.think_filter.finish();
        self.push_thinking(rest_thought, &mut events);
        self.finish_thinking(&mut events);
        self.push_answer_text(rest_text, &mut events);
        if self.text_mode {
            self.finish_text_tools(&mut events);
        } else {
            self.finish_held_text(&mut events);
        }
        if self.text_started && !self.text_finished {
            self.text_finished = true;
            events.push(StreamEvent::ContentBlockStop(ContentBlockStopEvent {
                index: 0,
            }));
        }

        for state in self.tool_calls.values_mut() {
            if !state.started {
                if let Some(start_event) = state.start_event() {
                    state.started = true;
                    events.push(StreamEvent::ContentBlockStart(start_event));
                    if let Some(delta_event) = state.delta_event() {
                        events.push(StreamEvent::ContentBlockDelta(delta_event));
                    }
                }
            }
            if state.started && !state.stopped {
                state.stopped = true;
                events.push(StreamEvent::ContentBlockStop(ContentBlockStopEvent {
                    index: state.block_index(),
                }));
            }
        }

        if self.message_started {
            events.push(StreamEvent::MessageDelta(MessageDeltaEvent {
                delta: MessageDelta {
                    stop_reason: Some(
                        self.stop_reason
                            .clone()
                            .unwrap_or_else(|| "end_turn".to_string()),
                    ),
                    stop_sequence: None,
                },
                usage: self.usage.clone().unwrap_or(Usage {
                    input_tokens: 0,
                    cache_creation_input_tokens: 0,
                    cache_read_input_tokens: 0,
                    output_tokens: 0,
                }),
            }));
            events.push(StreamEvent::MessageStop(MessageStopEvent {}));
        }
        events
    }
}

#[derive(Debug, Default)]
struct ToolCallState {
    openai_index: u32,
    id: Option<String>,
    name: Option<String>,
    arguments: String,
    emitted_len: usize,
    started: bool,
    stopped: bool,
}

impl ToolCallState {
    fn apply(&mut self, tool_call: DeltaToolCall) {
        self.openai_index = tool_call.index;
        if let Some(id) = tool_call.id {
            self.id = Some(id);
        }
        if let Some(name) = tool_call.function.name {
            self.name = Some(name);
        }
        if let Some(arguments) = tool_call.function.arguments {
            self.arguments.push_str(&arguments);
        }
    }

    const fn block_index(&self) -> u32 {
        self.openai_index + 1
    }

    fn start_event(&self) -> Option<ContentBlockStartEvent> {
        let name = self.name.clone()?;
        let id = self
            .id
            .clone()
            .unwrap_or_else(|| format!("tool_call_{}", self.openai_index));
        Some(ContentBlockStartEvent {
            index: self.block_index(),
            content_block: OutputContentBlock::ToolUse {
                id,
                name,
                input: json!({}),
            },
        })
    }

    fn delta_event(&mut self) -> Option<ContentBlockDeltaEvent> {
        if self.emitted_len >= self.arguments.len() {
            return None;
        }
        let delta = self.arguments[self.emitted_len..].to_string();
        self.emitted_len = self.arguments.len();
        Some(ContentBlockDeltaEvent {
            index: self.block_index(),
            delta: ContentBlockDelta::InputJsonDelta {
                partial_json: delta,
            },
        })
    }
}

#[derive(Debug, Deserialize)]
struct ChatCompletionResponse {
    #[serde(default)]
    id: String,
    #[serde(default)]
    model: String,
    choices: Vec<ChatChoice>,
    #[serde(default)]
    usage: Option<OpenAiUsage>,
}

#[derive(Debug, Deserialize)]
struct ChatChoice {
    message: ChatMessage,
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ChatMessage {
    role: String,
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    tool_calls: Vec<ResponseToolCall>,
}

#[derive(Debug, Deserialize)]
struct ResponseToolCall {
    id: String,
    function: ResponseToolFunction,
}

#[derive(Debug, Deserialize)]
struct ResponseToolFunction {
    name: String,
    arguments: String,
}

#[derive(Debug, Deserialize)]
struct OpenAiUsage {
    #[serde(default)]
    prompt_tokens: u32,
    #[serde(default)]
    completion_tokens: u32,
}

#[derive(Debug, Deserialize)]
struct ChatCompletionChunk {
    #[serde(default)]
    id: String,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    choices: Vec<ChunkChoice>,
    #[serde(default)]
    usage: Option<OpenAiUsage>,
    #[serde(default)]
    error: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct ChunkChoice {
    delta: ChunkDelta,
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct ChunkDelta {
    #[serde(default)]
    content: Option<String>,
    /// Raisonnement (`OpenRouter`, Ollama…).
    #[serde(default)]
    reasoning: Option<String>,
    /// Raisonnement (`DeepSeek`, vLLM…).
    #[serde(default)]
    reasoning_content: Option<String>,
    /// Raisonnement structuré d'`OpenRouter` (texte, résumé, bloc chiffré).
    #[serde(default)]
    reasoning_details: Vec<Value>,
    #[serde(default)]
    tool_calls: Vec<DeltaToolCall>,
}

#[derive(Debug, Deserialize)]
struct DeltaToolCall {
    #[serde(default)]
    index: u32,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    function: DeltaFunction,
}

#[derive(Debug, Default, Deserialize)]
struct DeltaFunction {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    arguments: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ErrorEnvelope {
    error: ErrorBody,
}

#[derive(Debug, Deserialize)]
struct ErrorBody {
    #[serde(rename = "type")]
    error_type: Option<String>,
    message: Option<String>,
}

#[cfg(test)]
fn build_chat_completion_request(request: &MessageRequest) -> Value {
    build_chat_completion_request_with(request, false, false)
}

/// `passback` : renvoyer au modèle son raisonnement des étapes précédentes
/// du tour en cours (`OpenRouter` seulement ; d'autres serveurs le refusent).
fn build_chat_completion_request_with(
    request: &MessageRequest,
    text_mode: bool,
    passback: bool,
) -> Value {
    let text_mode = text_mode
        && request
            .tools
            .as_ref()
            .is_some_and(|tools| !tools.is_empty());
    let mut messages = Vec::new();
    let mut system = request.system.clone().unwrap_or_default();
    if text_mode {
        if !system.is_empty() {
            system.push_str("\n\n");
        }
        system.push_str(&text_tools::tool_protocol_prompt(
            request.tools.as_deref().unwrap_or_default(),
        ));
    }
    if !system.is_empty() {
        messages.push(json!({
            "role": "system",
            "content": system,
        }));
    }
    if text_mode {
        messages.extend(translate_messages_as_text(&request.messages));
    } else {
        // Le tour en cours commence à la dernière vraie demande de l'utilisateur
        // (un message `user` qui ne porte que des résultats d'outils n'en est pas une).
        let turn_start = request.messages.iter().rposition(|message| {
            message.role == "user"
                && message
                    .content
                    .iter()
                    .any(|block| matches!(block, InputContentBlock::Text { .. }))
        });
        for (index, message) in request.messages.iter().enumerate() {
            let with_reasoning = passback && turn_start.is_some_and(|start| index > start);
            messages.extend(translate_message(message, with_reasoning));
        }
    }

    let mut payload = json!({
        "model": request.model,
        "max_tokens": request.max_tokens,
        "messages": messages,
        "stream": request.stream,
    });
    if request.stream {
        // Sans cette option, la plupart des API compatibles OpenAI n'envoient pas
        // l'usage des jetons en streaming (et /cost resterait à zéro).
        payload["stream_options"] = json!({ "include_usage": true });
    }

    if !text_mode {
        if let Some(tools) = &request.tools {
            payload["tools"] =
                Value::Array(tools.iter().map(openai_tool_definition).collect::<Vec<_>>());
        }
        if let Some(tool_choice) = &request.tool_choice {
            payload["tool_choice"] = openai_tool_choice(tool_choice);
        }
    }

    payload
}

/// Historique en mode texte : appels et résultats d'outils écrits en clair,
/// messages `user`/`assistant` uniquement (pas de rôle `tool`).
fn translate_messages_as_text(messages: &[InputMessage]) -> Vec<Value> {
    let mut names = BTreeMap::new();
    let mut translated: Vec<Value> = Vec::new();
    for message in messages {
        let mut parts = Vec::new();
        let mut images = Vec::new();
        for block in &message.content {
            match block {
                InputContentBlock::Image { source } => images.push(image_part(source)),
                InputContentBlock::Thinking { .. } | InputContentBlock::RedactedThinking { .. } => {
                }
                InputContentBlock::Text { text } => parts.push(text.clone()),
                InputContentBlock::ToolUse { id, name, input } => {
                    names.insert(id.clone(), name.clone());
                    parts.push(text_tools::render_tool_call(name, input));
                }
                InputContentBlock::ToolResult {
                    tool_use_id,
                    content,
                    is_error,
                } => parts.push(text_tools::render_tool_result(
                    names.get(tool_use_id).map_or("tool", String::as_str),
                    &flatten_tool_result_content(content),
                    *is_error,
                )),
            }
        }
        if parts.is_empty() && images.is_empty() {
            continue;
        }
        let role = if message.role == "assistant" {
            "assistant"
        } else {
            "user"
        };
        let content = parts.join("\n\n");
        if !images.is_empty() && role == "user" {
            translated
                .push(json!({ "role": role, "content": content_with_images(&content, images) }));
            continue;
        }
        // Deux messages consécutifs du même rôle sont fusionnés.
        match translated.last_mut() {
            Some(last) if last["role"] == role => {
                let merged = format!(
                    "{}\n\n{content}",
                    last["content"].as_str().unwrap_or_default()
                );
                last["content"] = Value::String(merged);
            }
            _ => translated.push(json!({ "role": role, "content": content })),
        }
    }
    translated
}

fn translate_message(message: &InputMessage, with_reasoning: bool) -> Vec<Value> {
    if message.role == "assistant" {
        translate_assistant_message(message, with_reasoning)
    } else {
        translate_user_message(message)
    }
}

fn translate_assistant_message(message: &InputMessage, with_reasoning: bool) -> Vec<Value> {
    let mut text = String::new();
    let mut tool_calls = Vec::new();
    let mut reasoning_text = String::new();
    let mut reasoning_details = None;
    for block in &message.content {
        match block {
            InputContentBlock::Thinking {
                thinking,
                signature,
            } if with_reasoning && is_passback_reasoning(signature) => {
                match signature.strip_prefix(REASONING_DETAILS_PREFIX) {
                    Some(details) => {
                        reasoning_details = serde_json::from_str::<Value>(details).ok();
                    }
                    None => reasoning_text.push_str(thinking),
                }
            }
            InputContentBlock::Text { text: value } => text.push_str(value),
            InputContentBlock::ToolUse { id, name, input } => tool_calls.push(json!({
                "id": id,
                "type": "function",
                "function": {
                    "name": name,
                    "arguments": input.to_string(),
                }
            })),
            InputContentBlock::ToolResult { .. }
            | InputContentBlock::Image { .. }
            | InputContentBlock::Thinking { .. }
            | InputContentBlock::RedactedThinking { .. } => {}
        }
    }
    if text.is_empty() && tool_calls.is_empty() {
        return Vec::new();
    }
    let mut translated = json!({
        "role": "assistant",
        "content": (!text.is_empty()).then_some(text),
    });
    // Plusieurs fournisseurs rejettent un tableau `tool_calls` vide.
    if !tool_calls.is_empty() {
        translated["tool_calls"] = Value::Array(tool_calls);
    }
    if let Some(details) = reasoning_details {
        translated["reasoning_details"] = details;
    } else if !reasoning_text.is_empty() {
        let count = reasoning_text.chars().count();
        let kept: String = reasoning_text
            .chars()
            .skip(count.saturating_sub(MAX_PASSBACK_CHARS))
            .collect();
        translated["reasoning"] = Value::String(kept);
    }
    vec![translated]
}

fn translate_user_message(message: &InputMessage) -> Vec<Value> {
    // Les messages `tool` doivent suivre immédiatement le message assistant
    // qui contient les `tool_calls` : on les place avant le texte utilisateur.
    let tool_results = message.content.iter().filter_map(|block| match block {
        InputContentBlock::ToolResult {
            tool_use_id,
            content,
            is_error,
        } => {
            let output = flatten_tool_result_content(content);
            Some(json!({
                "role": "tool",
                "tool_call_id": tool_use_id,
                "content": if *is_error { format!("[error] {output}") } else { output },
            }))
        }
        InputContentBlock::Text { .. }
        | InputContentBlock::ToolUse { .. }
        | InputContentBlock::Image { .. }
        | InputContentBlock::Thinking { .. }
        | InputContentBlock::RedactedThinking { .. } => None,
    });
    // Plusieurs blocs de texte (demande + contexte des hooks) forment un seul
    // message : certains fournisseurs refusent deux messages `user` consécutifs.
    let text = message
        .content
        .iter()
        .filter_map(|block| match block {
            InputContentBlock::Text { text } => Some(text.as_str()),
            InputContentBlock::ToolResult { .. }
            | InputContentBlock::ToolUse { .. }
            | InputContentBlock::Image { .. }
            | InputContentBlock::Thinking { .. }
            | InputContentBlock::RedactedThinking { .. } => None,
        })
        .collect::<Vec<_>>()
        .join(
            "

",
        );
    let images = message
        .content
        .iter()
        .filter_map(|block| match block {
            InputContentBlock::Image { source } => Some(image_part(source)),
            _ => None,
        })
        .collect::<Vec<_>>();
    let texts = if images.is_empty() {
        (!text.is_empty()).then(|| {
            json!({
                "role": "user",
                "content": text,
            })
        })
    } else {
        Some(json!({
            "role": "user",
            "content": content_with_images(&text, images),
        }))
    };
    tool_results.chain(texts).collect()
}

fn image_part(source: &crate::types::ImageSource) -> Value {
    json!({ "type": "image_url", "image_url": { "url": source.data_url() } })
}

/// Contenu multimodal : le texte puis les images.
fn content_with_images(text: &str, images: Vec<Value>) -> Value {
    let mut parts = Vec::new();
    if !text.is_empty() {
        parts.push(json!({ "type": "text", "text": text }));
    }
    parts.extend(images);
    Value::Array(parts)
}

fn flatten_tool_result_content(content: &[ToolResultContentBlock]) -> String {
    content
        .iter()
        .map(|block| match block {
            ToolResultContentBlock::Text { text } => text.clone(),
            ToolResultContentBlock::Json { value } => value.to_string(),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn openai_tool_definition(tool: &ToolDefinition) -> Value {
    json!({
        "type": "function",
        "function": {
            "name": tool.name,
            "description": tool.description,
            "parameters": tool.input_schema,
        }
    })
}

fn openai_tool_choice(tool_choice: &ToolChoice) -> Value {
    match tool_choice {
        ToolChoice::Auto => Value::String("auto".to_string()),
        ToolChoice::Any => Value::String("required".to_string()),
        ToolChoice::Tool { name } => json!({
            "type": "function",
            "function": { "name": name },
        }),
    }
}

fn normalize_response(
    model: &str,
    response: ChatCompletionResponse,
    text_mode: bool,
    known_tools: &[String],
) -> Result<MessageResponse, ApiError> {
    let choice = response
        .choices
        .into_iter()
        .next()
        .ok_or(ApiError::InvalidSseFrame(
            "la réponse chat completion ne contient pas de choices",
        ))?;
    let mut content = Vec::new();
    let mut finish_reason = choice.finish_reason.clone();
    if let Some(text) = choice
        .message
        .content
        .map(|text| reasoning::split_think_tags(&text).0)
        .filter(|value| !value.is_empty())
    {
        let (visible, mut calls) = if text_mode {
            text_tools::extract_tool_calls(&text)
        } else {
            (text.clone(), Vec::new())
        };
        // Appel écrit en JSON brut au lieu d'un appel natif.
        if calls.is_empty() && choice.message.tool_calls.is_empty() {
            if let Some(salvaged) = text_tools::salvage_json_tool_calls(&text, known_tools) {
                calls = salvaged;
            }
        }
        if calls.is_empty() {
            if !visible.is_empty() {
                content.push(OutputContentBlock::Text { text: visible });
            }
        } else if text_mode && !visible.is_empty() {
            content.push(OutputContentBlock::Text { text: visible });
        }
        for (offset, (name, input)) in calls.into_iter().enumerate() {
            content.push(OutputContentBlock::ToolUse {
                id: format!("text_call_{offset}"),
                name,
                input,
            });
            finish_reason = Some("tool_calls".to_string());
        }
    }
    for tool_call in choice.message.tool_calls {
        content.push(OutputContentBlock::ToolUse {
            id: tool_call.id,
            name: tool_call.function.name,
            input: parse_tool_arguments(&tool_call.function.arguments),
        });
    }

    Ok(MessageResponse {
        id: response.id,
        kind: "message".to_string(),
        role: choice.message.role,
        content,
        model: response.model.if_empty_then(model.to_string()),
        stop_reason: finish_reason.map(|value| normalize_finish_reason(&value)),
        stop_sequence: None,
        usage: Usage {
            input_tokens: response
                .usage
                .as_ref()
                .map_or(0, |usage| usage.prompt_tokens),
            cache_creation_input_tokens: 0,
            cache_read_input_tokens: 0,
            output_tokens: response
                .usage
                .as_ref()
                .map_or(0, |usage| usage.completion_tokens),
        },
        request_id: None,
    })
}

fn parse_tool_arguments(arguments: &str) -> Value {
    serde_json::from_str(arguments).unwrap_or_else(|_| json!({ "raw": arguments }))
}

fn next_sse_frame(buffer: &mut Vec<u8>) -> Option<String> {
    let separator = buffer
        .windows(2)
        .position(|window| window == b"\n\n")
        .map(|position| (position, 2))
        .or_else(|| {
            buffer
                .windows(4)
                .position(|window| window == b"\r\n\r\n")
                .map(|position| (position, 4))
        })?;

    let (position, separator_len) = separator;
    let frame = buffer.drain(..position + separator_len).collect::<Vec<_>>();
    let frame_len = frame.len().saturating_sub(separator_len);
    Some(String::from_utf8_lossy(&frame[..frame_len]).into_owned())
}

fn parse_sse_frame(frame: &str) -> Result<Option<ChatCompletionChunk>, ApiError> {
    let trimmed = frame.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }

    let mut data_lines = Vec::new();
    for line in trimmed.lines() {
        if line.starts_with(':') {
            continue;
        }
        if let Some(data) = line.strip_prefix("data:") {
            data_lines.push(data.trim_start());
        }
    }
    if data_lines.is_empty() {
        return Ok(None);
    }
    let payload = data_lines.join("\n");
    if payload == "[DONE]" {
        return Ok(None);
    }
    serde_json::from_str(&payload)
        .map(Some)
        .map_err(ApiError::from)
}

fn read_env_non_empty(key: &str) -> Result<Option<String>, ApiError> {
    match std::env::var(key) {
        Ok(value) if !value.is_empty() => Ok(Some(value)),
        Ok(_) | Err(std::env::VarError::NotPresent) => Ok(None),
        Err(error) => Err(ApiError::from(error)),
    }
}

#[must_use]
pub fn has_api_key(key: &str) -> bool {
    read_env_non_empty(key)
        .ok()
        .and_then(std::convert::identity)
        .is_some()
}

#[must_use]
pub fn read_base_url(config: OpenAiCompatConfig) -> String {
    std::env::var(config.base_url_env).unwrap_or_else(|_| config.default_base_url.to_string())
}

fn chat_completions_endpoint(base_url: &str) -> String {
    let trimmed = base_url.trim_end_matches('/');
    if trimmed.ends_with("/chat/completions") {
        trimmed.to_string()
    } else {
        format!("{trimmed}/chat/completions")
    }
}

fn request_id_from_headers(headers: &reqwest::header::HeaderMap) -> Option<String> {
    headers
        .get(REQUEST_ID_HEADER)
        .or_else(|| headers.get(ALT_REQUEST_ID_HEADER))
        .and_then(|value| value.to_str().ok())
        .map(ToOwned::to_owned)
}

async fn expect_success(response: reqwest::Response) -> Result<reqwest::Response, ApiError> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }

    let body = response.text().await.unwrap_or_default();
    let parsed_error = serde_json::from_str::<ErrorEnvelope>(&body).ok();
    let retryable = is_retryable_status(status);

    Err(ApiError::Api {
        status,
        error_type: parsed_error
            .as_ref()
            .and_then(|error| error.error.error_type.clone()),
        message: parsed_error
            .as_ref()
            .and_then(|error| error.error.message.clone()),
        body,
        retryable,
    })
}

const fn is_retryable_status(status: reqwest::StatusCode) -> bool {
    matches!(status.as_u16(), 408 | 409 | 429 | 500 | 502 | 503 | 504)
}

/// Erreur renvoyée dans le corps d'une réponse « 200 » (`{"error": {...}}`) :
/// `OpenRouter` s'en sert quand le fournisseur du modèle est surchargé ou limité.
fn error_in_body(error: &Value) -> ApiError {
    // Code HTTP du fournisseur s'il est donné (503 surcharge, 429 limite).
    let status = error
        .get("code")
        .and_then(Value::as_u64)
        .and_then(|code| u16::try_from(code).ok())
        .and_then(|code| reqwest::StatusCode::from_u16(code).ok())
        .filter(|status| status.is_client_error() || status.is_server_error())
        .unwrap_or(reqwest::StatusCode::BAD_GATEWAY);
    ApiError::Api {
        status,
        error_type: error.get("code").map(|code| match code {
            Value::String(value) => value.clone(),
            other => other.to_string(),
        }),
        message: error
            .get("message")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
        body: json!({ "error": error }).to_string(),
        retryable: is_retryable_status(status),
    }
}

/// Réponse sans streaming : une erreur placée dans le corps est rendue comme
/// telle, au lieu d'un « corps illisible » qui ne dit rien.
fn parse_chat_completion(body: &[u8]) -> Result<ChatCompletionResponse, ApiError> {
    let value: Value = serde_json::from_slice(body)?;
    let answered = value
        .get("choices")
        .and_then(Value::as_array)
        .is_some_and(|choices| !choices.is_empty());
    match value.get("error") {
        Some(error) if !answered && !error.is_null() => Err(error_in_body(error)),
        _ => Ok(serde_json::from_value(value)?),
    }
}

fn normalize_finish_reason(value: &str) -> String {
    match value {
        "stop" => "end_turn",
        "tool_calls" => "tool_use",
        other => other,
    }
    .to_string()
}

trait StringExt {
    fn if_empty_then(self, fallback: String) -> String;
}

impl StringExt for String {
    fn if_empty_then(self, fallback: String) -> String {
        if self.is_empty() {
            fallback
        } else {
            self
        }
    }
}

#[cfg(test)]
mod passback_tests {
    use super::{
        build_chat_completion_request_with, is_passback_reasoning, ChatCompletionChunk, StreamState,
    };
    use crate::types::{
        ContentBlockDelta, InputContentBlock, InputMessage, MessageRequest, OutputContentBlock,
        StreamEvent, ToolResultContentBlock,
    };
    use serde_json::json;

    fn chunk(value: serde_json::Value) -> ChatCompletionChunk {
        serde_json::from_value(value).expect("fragment")
    }

    fn signature_of(events: &[StreamEvent]) -> Option<String> {
        events.iter().find_map(|event| match event {
            StreamEvent::ContentBlockDelta(delta) => match &delta.delta {
                ContentBlockDelta::SignatureDelta { signature } => Some(signature.clone()),
                _ => None,
            },
            _ => None,
        })
    }

    #[test]
    fn an_error_sent_with_status_200_is_an_api_error_another_model_can_replace() {
        let overloaded = br#"{"id":"gen-1","error":{"message":"Upstream error: Service temporarily overloaded","code":503,"metadata":{"error_type":"provider_overloaded"}}}"#;
        let error = super::parse_chat_completion(overloaded).expect_err("erreur dans le corps");
        match &error {
            crate::error::ApiError::Api {
                status, message, ..
            } => {
                assert_eq!(status.as_u16(), 503);
                assert!(message
                    .as_deref()
                    .is_some_and(|text| text.contains("overloaded")));
            }
            other => panic!("erreur inattendue : {other}"),
        }
        assert!(error.is_fallback_worthy());

        // Réponse normale, avec des champs inconnus et un `error` nul.
        let answer = br#"{"id":"gen-2","model":"x/y","error":null,"choices":[{"index":0,"finish_reason":"stop","message":{"role":"assistant","content":"OK","refusal":null}}],"usage":{"prompt_tokens":3,"completion_tokens":1}}"#;
        let parsed = super::parse_chat_completion(answer).expect("réponse");
        assert_eq!(parsed.choices.len(), 1);

        // Corps coupé : erreur de lecture, que la chaîne sait contourner.
        let cut = super::parse_chat_completion(br#"{"id":"gen-3","choi"#).expect_err("coupé");
        assert!(cut.is_broken_response() && !cut.is_fallback_worthy());
    }

    #[test]
    fn openrouter_reasoning_is_kept_with_how_to_send_it_back() {
        // Raisonnement en texte simple.
        let mut state = StreamState::new("x/y".to_string()).with_reasoning_passback(true);
        let mut events = state
            .ingest_chunk(chunk(json!({
                "id": "a", "model": "x/y",
                "choices": [{ "index": 0, "delta": { "reasoning": "Je lis " }, "finish_reason": null }]
            })))
            .expect("flux");
        assert!(events.iter().any(|event| matches!(
            event,
            StreamEvent::ContentBlockStart(start)
                if start.index == 999 && matches!(start.content_block, OutputContentBlock::Thinking { .. })
        )));
        events = state.finish();
        assert_eq!(signature_of(&events).as_deref(), Some("or-plain"));

        // Détails structurés : fusionnés et renvoyés tels quels.
        let mut state = StreamState::new("x/y".to_string()).with_reasoning_passback(true);
        for (text, signature) in [("Je lis ", None), ("le fichier.", Some("sig"))] {
            let mut detail = json!({ "type": "reasoning.text", "index": 0, "text": text });
            if let Some(signature) = signature {
                detail["signature"] = json!(signature);
            }
            state
                .ingest_chunk(chunk(json!({
                    "id": "a", "model": "x/y",
                    "choices": [{ "index": 0, "delta": { "reasoning": text, "reasoning_details": [detail] }, "finish_reason": null }]
                })))
                .expect("flux");
        }
        let signature = signature_of(&state.finish()).expect("signature");
        assert!(is_passback_reasoning(&signature));
        let details: serde_json::Value =
            serde_json::from_str(signature.trim_start_matches("or-details:")).expect("json");
        assert_eq!(details[0]["text"], "Je lis le fichier.");
        assert_eq!(details[0]["signature"], "sig");

        // Autres serveurs : rien n'est gardé (pas de bloc, pas de signature).
        let mut state = StreamState::new("ollama:q".to_string());
        let events = state
            .ingest_chunk(chunk(json!({
                "id": "a", "model": "q",
                "choices": [{ "index": 0, "delta": { "reasoning": "hm" }, "finish_reason": null }]
            })))
            .expect("flux");
        assert!(!events
            .iter()
            .any(|event| matches!(event, StreamEvent::ContentBlockStart(_))));
        assert_eq!(signature_of(&state.finish()), None);
        assert!(!is_passback_reasoning("EqQBCkgIAxgCIkD…"));
    }

    fn assistant(thinking: &str, signature: &str, tool_id: &str) -> InputMessage {
        InputMessage {
            role: "assistant".to_string(),
            content: vec![
                InputContentBlock::Thinking {
                    thinking: thinking.to_string(),
                    signature: signature.to_string(),
                },
                InputContentBlock::ToolUse {
                    id: tool_id.to_string(),
                    name: "read_file".to_string(),
                    input: json!({ "path": "a.rs" }),
                },
            ],
        }
    }

    fn tool_result(tool_id: &str) -> InputMessage {
        InputMessage {
            role: "user".to_string(),
            content: vec![InputContentBlock::ToolResult {
                tool_use_id: tool_id.to_string(),
                content: vec![ToolResultContentBlock::Text {
                    text: "contenu".to_string(),
                }],
                is_error: false,
            }],
        }
    }

    #[test]
    fn reasoning_goes_back_only_for_the_current_turn_and_only_when_asked() {
        let request = MessageRequest {
            model: "x/y".to_string(),
            max_tokens: 100,
            messages: vec![
                InputMessage::user_text("ancienne demande"),
                assistant("vieille réflexion", "or-plain", "t1"),
                tool_result("t1"),
                InputMessage::user_text("nouvelle demande"),
                assistant("réflexion en cours", "or-plain", "t2"),
                tool_result("t2"),
                assistant(
                    "",
                    r#"or-details:[{"type":"reasoning.text","text":"suite","index":0}]"#,
                    "t3",
                ),
                tool_result("t3"),
            ],
            system: None,
            tools: None,
            tool_choice: None,
            stream: false,
        };
        let payload = build_chat_completion_request_with(&request, false, true);
        let messages = payload["messages"].as_array().expect("messages");
        let assistants: Vec<&serde_json::Value> = messages
            .iter()
            .filter(|message| message["role"] == "assistant")
            .collect();
        // Tour précédent : rien. Tour en cours : texte, puis détails.
        assert!(assistants[0].get("reasoning").is_none());
        assert_eq!(assistants[1]["reasoning"], "réflexion en cours");
        assert_eq!(assistants[2]["reasoning_details"][0]["text"], "suite");

        // Serveur qui ne le prend pas en charge : aucun champ de raisonnement.
        let plain = build_chat_completion_request_with(&request, false, false);
        assert!(!plain.to_string().contains("reasoning"));
    }
}

#[cfg(test)]
mod tests {
    use super::{
        build_chat_completion_request, chat_completions_endpoint, normalize_finish_reason,
        openai_tool_choice, parse_sse_frame, parse_tool_arguments, OpenAiCompatClient,
        OpenAiCompatConfig, StreamState,
    };
    use crate::error::ApiError;
    use crate::types::{
        InputContentBlock, InputMessage, MessageRequest, ToolChoice, ToolDefinition,
        ToolResultContentBlock,
    };
    use serde_json::json;
    use std::sync::{Mutex, OnceLock};

    #[test]
    fn strict_servers_do_not_get_stream_options() {
        use super::{
            accepts_stream_options, is_stream_options_error, remember_stream_options_refusal,
        };
        // Mistral refuse tout champ absent de sa spécification.
        assert!(!accepts_stream_options("https://api.mistral.ai/v1"));
        assert!(accepts_stream_options("https://openrouter.ai/api/v1"));
        assert!(accepts_stream_options(
            "https://generativelanguage.googleapis.com/v1beta/openai"
        ));
        // Un serveur qui le refuse une fois est retenu.
        let strict = "https://strict.example.test/v1";
        assert!(accepts_stream_options(strict));
        remember_stream_options_refusal(strict);
        assert!(!accepts_stream_options(strict));

        let refusal = |status: u16, body: &str| crate::error::ApiError::Api {
            status: reqwest::StatusCode::from_u16(status).expect("statut"),
            error_type: None,
            message: None,
            body: body.to_string(),
            retryable: false,
        };
        assert!(is_stream_options_error(&refusal(
            422,
            r#"{"detail":[{"type":"extra_forbidden","loc":["body","stream_options"],"msg":"Extra inputs are not permitted"}]}"#
        )));
        assert!(!is_stream_options_error(&refusal(422, "invalid model")));
        assert!(!is_stream_options_error(&refusal(500, "stream_options")));
    }

    #[test]
    fn request_translation_uses_openai_compatible_shape() {
        let payload = build_chat_completion_request(&MessageRequest {
            model: "grok-3".to_string(),
            max_tokens: 64,
            messages: vec![InputMessage {
                role: "user".to_string(),
                content: vec![
                    InputContentBlock::Text {
                        text: "hello".to_string(),
                    },
                    InputContentBlock::ToolResult {
                        tool_use_id: "tool_1".to_string(),
                        content: vec![ToolResultContentBlock::Json {
                            value: json!({"ok": true}),
                        }],
                        is_error: false,
                    },
                ],
            }],
            system: Some("be helpful".to_string()),
            tools: Some(vec![ToolDefinition {
                name: "weather".to_string(),
                description: Some("Get weather".to_string()),
                input_schema: json!({"type": "object"}),
            }]),
            tool_choice: Some(ToolChoice::Auto),
            stream: false,
        });

        assert_eq!(payload["messages"][0]["role"], json!("system"));
        // Les résultats d'outils passent avant le texte utilisateur du même message.
        assert_eq!(payload["messages"][1]["role"], json!("tool"));
        assert!(payload["messages"][1].get("is_error").is_none());
        assert_eq!(payload["messages"][2]["role"], json!("user"));
        assert_eq!(payload["tools"][0]["type"], json!("function"));
        assert_eq!(payload["tool_choice"], json!("auto"));
        assert!(payload.get("stream_options").is_none());
    }

    #[test]
    fn assistant_without_tool_calls_omits_empty_array_and_errors_are_prefixed() {
        let payload = build_chat_completion_request(&MessageRequest {
            model: "qwen/qwen3-coder".to_string(),
            max_tokens: 64,
            messages: vec![
                InputMessage {
                    role: "assistant".to_string(),
                    content: vec![InputContentBlock::Text {
                        text: "done".to_string(),
                    }],
                },
                InputMessage {
                    role: "user".to_string(),
                    content: vec![InputContentBlock::ToolResult {
                        tool_use_id: "call_1".to_string(),
                        content: vec![ToolResultContentBlock::Text {
                            text: "boom".to_string(),
                        }],
                        is_error: true,
                    }],
                },
            ],
            system: None,
            tools: None,
            tool_choice: None,
            stream: true,
        });

        assert!(payload["messages"][0].get("tool_calls").is_none());
        assert_eq!(payload["messages"][1]["content"], json!("[error] boom"));
        assert_eq!(payload["stream_options"]["include_usage"], json!(true));
    }

    #[test]
    fn openrouter_config_targets_openrouter_and_sets_title_header() {
        let config = OpenAiCompatConfig::openrouter();
        assert_eq!(config.api_key_env, "OPENROUTER_API_KEY");
        assert_eq!(config.default_base_url, "https://openrouter.ai/api/v1");
        let headers = config.extra_headers();
        assert!(headers.iter().any(|(name, _)| name == "X-Title"));
        assert!(OpenAiCompatConfig::openai().extra_headers().is_empty());
    }

    #[test]
    fn tool_choice_translation_supports_required_function() {
        assert_eq!(openai_tool_choice(&ToolChoice::Any), json!("required"));
        assert_eq!(
            openai_tool_choice(&ToolChoice::Tool {
                name: "weather".to_string(),
            }),
            json!({"type": "function", "function": {"name": "weather"}})
        );
    }

    #[test]
    fn parses_tool_arguments_fallback() {
        assert_eq!(
            parse_tool_arguments("{\"city\":\"Paris\"}"),
            json!({"city": "Paris"})
        );
        assert_eq!(parse_tool_arguments("not-json"), json!({"raw": "not-json"}));
    }

    #[test]
    fn missing_xai_api_key_is_provider_specific() {
        let _lock = env_lock();
        std::env::remove_var("XAI_API_KEY");
        let error = OpenAiCompatClient::from_env(OpenAiCompatConfig::xai())
            .expect_err("une clé absente doit produire une erreur");
        assert!(matches!(
            error,
            ApiError::MissingCredentials {
                provider: "xAI",
                ..
            }
        ));
    }

    #[test]
    fn endpoint_builder_accepts_base_urls_and_full_endpoints() {
        assert_eq!(
            chat_completions_endpoint("https://api.x.ai/v1"),
            "https://api.x.ai/v1/chat/completions"
        );
        assert_eq!(
            chat_completions_endpoint("https://api.x.ai/v1/"),
            "https://api.x.ai/v1/chat/completions"
        );
        assert_eq!(
            chat_completions_endpoint("https://api.x.ai/v1/chat/completions"),
            "https://api.x.ai/v1/chat/completions"
        );
    }

    fn env_lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
            .lock()
            .expect("verrou d'environnement")
    }

    #[test]
    fn normalizes_stop_reasons() {
        assert_eq!(normalize_finish_reason("stop"), "end_turn");
        assert_eq!(normalize_finish_reason("tool_calls"), "tool_use");
    }

    #[test]
    fn mid_stream_error_chunk_becomes_api_error() {
        let chunk = parse_sse_frame(
            "data: {\"error\":{\"code\":\"server_error\",\"message\":\"Provider disconnected\"}}",
        )
        .expect("frame valide")
        .expect("chunk présent");
        let error = StreamState::new("x/y".to_string())
            .ingest_chunk(chunk)
            .expect_err("un chunk d'erreur doit échouer");
        assert!(error.to_string().contains("Provider disconnected"));

        // Surcharge du fournisseur : code conservé, erreur réessayable.
        let chunk = parse_sse_frame(
            "data: {\"error\":{\"code\":503,\"message\":\"Upstream error from Nvidia: Service temporarily overloaded\"}}",
        )
        .expect("frame valide")
        .expect("chunk présent");
        let error = StreamState::new("x/y".to_string())
            .ingest_chunk(chunk)
            .expect_err("surcharge");
        assert!(error.is_retryable(), "{error}");
        assert!(matches!(error, ApiError::Api { status, .. } if status.as_u16() == 503));

        // Limite par minute : délai de remise à zéro lu dans les en-têtes rapportés.
        let reset_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("horloge")
            .as_millis()
            + 12_000;
        let frame = format!(
            "data: {{\"error\":{{\"code\":429,\"message\":\"Rate limit exceeded: free-models-per-min.\",\"metadata\":{{\"headers\":{{\"X-RateLimit-Limit\":\"20\",\"X-RateLimit-Reset\":\"{reset_ms}\"}}}}}}}}"
        );
        let chunk = parse_sse_frame(&frame).expect("frame").expect("chunk");
        let error = StreamState::new("x/y".to_string())
            .ingest_chunk(chunk)
            .expect_err("limite");
        let wait = error.rate_limit_reset().expect("délai annoncé");
        assert!(wait.as_secs() >= 10 && wait.as_secs() <= 12, "{wait:?}");
    }

    /// Flux d'une réponse en texte découpée en morceaux, outils natifs proposés.
    fn stream_text(pieces: &[&str]) -> Vec<crate::types::StreamEvent> {
        let request = MessageRequest {
            model: "ollama:qwen".to_string(),
            max_tokens: 64,
            messages: vec![InputMessage::user_text("Corrige le bug")],
            system: None,
            tools: Some(vec![ToolDefinition {
                name: "read_file".to_string(),
                description: None,
                input_schema: json!({"type": "object"}),
            }]),
            tool_choice: None,
            stream: true,
        };
        let mut state = StreamState::new("ollama:qwen".to_string()).with_tools(&request, false);
        let mut events = Vec::new();
        for piece in pieces {
            let frame = format!(
                "data: {}",
                json!({"id": "c", "choices": [{"index": 0, "delta": {"content": piece}}]})
            );
            let chunk = parse_sse_frame(&frame).expect("frame").expect("chunk");
            events.extend(state.ingest_chunk(chunk).expect("morceau"));
        }
        events.extend(state.finish());
        events
    }

    fn texts_and_tools(events: &[crate::types::StreamEvent]) -> (String, Vec<String>) {
        use crate::types::{ContentBlockDelta, OutputContentBlock, StreamEvent};
        let mut text = String::new();
        let mut tools = Vec::new();
        for event in events {
            match event {
                StreamEvent::ContentBlockDelta(delta) => {
                    if let ContentBlockDelta::TextDelta { text: piece } = &delta.delta {
                        text.push_str(piece);
                    }
                }
                StreamEvent::ContentBlockStart(start) => {
                    if let OutputContentBlock::ToolUse { name, .. } = &start.content_block {
                        tools.push(name.clone());
                    }
                }
                _ => {}
            }
        }
        (text, tools)
    }

    #[test]
    fn raw_json_tool_call_in_text_becomes_a_tool_call() {
        let events = stream_text(&[
            "{\"name\": \"read_",
            "file\", \"arguments\": {\"path\": ",
            "\"stats.py\"}}",
        ]);
        let (text, tools) = texts_and_tools(&events);
        assert_eq!(text, "", "le JSON de l'appel ne doit pas s'afficher");
        assert_eq!(tools, ["read_file"]);

        // Texte ordinaire : affiché au fil de l'eau, sans appel.
        let first = stream_text(&["Le bug ", "est ligne 4."]);
        assert_eq!(
            texts_and_tools(&first),
            ("Le bug est ligne 4.".to_string(), Vec::new())
        );

        // JSON qui n'est pas un appel : affiché à la fin, tel quel.
        let other = stream_text(&["{\"total\": ", "3}"]);
        assert_eq!(
            texts_and_tools(&other),
            ("{\"total\": 3}".to_string(), Vec::new())
        );
    }
}
