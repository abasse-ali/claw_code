use std::collections::BTreeMap;
use std::fmt::{Display, Formatter};

use crate::compact::{
    compact_session, compact_session_with, estimate_message_tokens, estimate_session_tokens,
    render_transcript_for_summary, CompactionConfig, CompactionResult, MODEL_SUMMARY_PROMPT,
};
use crate::config::RuntimeFeatureConfig;
use crate::hooks::{HookEvent, HookRunResult, HookRunner};
use crate::interrupt::{clear_interrupt, interrupt_requested, INTERRUPTED_MESSAGE};
use crate::permissions::{
    PermissionOutcome, PermissionPolicy, PermissionPromptDecision, PermissionPrompter,
    PermissionRequest,
};
use crate::session::{ContentBlock, ConversationMessage, Session};
use crate::usage::{TokenUsage, UsageTracker};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiRequest {
    pub system_prompt: Vec<String>,
    pub messages: Vec<ConversationMessage>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssistantEvent {
    TextDelta(String),
    /// Bloc de raisonnement signé (modèles Claude), conservé dans la session.
    Thinking {
        thinking: String,
        signature: String,
        redacted: Option<String>,
    },
    ToolUse {
        id: String,
        name: String,
        input: String,
    },
    Usage(TokenUsage),
    MessageStop,
}

pub trait ApiClient {
    fn stream(&mut self, request: ApiRequest) -> Result<Vec<AssistantEvent>, RuntimeError>;

    /// Résume une transcription avec le modèle (sans outils, sans affichage).
    /// `None` : le runtime utilise son résumé local.
    fn summarize(&mut self, _system_prompt: &str, _transcript: &str) -> Option<String> {
        None
    }

    /// Informe l'interface d'un événement du runtime (compaction automatique…).
    fn notify(&mut self, _notice: &str) {}
}

/// Compaction automatique : dès que le contexte estimé dépasse `threshold_tokens`,
/// les anciens messages sont résumés avant l'appel suivant au modèle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AutoCompactionConfig {
    pub threshold_tokens: usize,
    pub preserve_recent_messages: usize,
}

pub trait ToolExecutor {
    fn execute(&mut self, tool_name: &str, input: &str) -> Result<String, ToolError>;

    /// Outil sans effet de bord, exécutable en même temps que d'autres.
    fn parallel_safe(&self, _tool_name: &str) -> bool {
        false
    }

    /// Exécute plusieurs appels sûrs ; par défaut l'un après l'autre.
    fn execute_batch(&mut self, calls: &[(String, String)]) -> Vec<Result<String, ToolError>> {
        calls
            .iter()
            .map(|(name, input)| self.execute(name, input))
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolError {
    message: String,
}

impl ToolError {
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl Display for ToolError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for ToolError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeError {
    message: String,
}

impl RuntimeError {
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl Display for RuntimeError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for RuntimeError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnSummary {
    pub assistant_messages: Vec<ConversationMessage>,
    pub tool_results: Vec<ConversationMessage>,
    pub iterations: usize,
    pub usage: TokenUsage,
}

pub struct ConversationRuntime<C, T> {
    session: Session,
    api_client: C,
    tool_executor: T,
    permission_policy: PermissionPolicy,
    system_prompt: Vec<String>,
    max_iterations: usize,
    usage_tracker: UsageTracker,
    hook_runner: HookRunner,
    auto_compaction: Option<AutoCompactionConfig>,
    checkpoint_turns: bool,
    subagent: bool,
}

/// Relances maximales par un hook `Stop` au cours d'un même tour.
const MAX_STOP_HOOK_CONTINUATIONS: usize = 5;

/// Préfixe du bloc de contexte ajouté au message utilisateur par les hooks.
pub const HOOK_CONTEXT_PREFIX: &str = "[Contexte ajouté par les hooks]";

/// Préfixe des relances demandées par un hook `Stop`.
pub const STOP_HOOK_PREFIX: &str = "[Hook Stop]";

/// Réponse sans texte ni appel d'outil (petits modèles qui ne font que raisonner).
const EMPTY_RESPONSE_ERROR: &str = "le modèle n'a renvoyé aucun contenu";

/// Relance unique après une réponse vide, au lieu d'échouer.
const EMPTY_RESPONSE_NUDGE: &str = "[Claw] Ta dernière réponse était vide. Continue la tâche avec les outils si elle n'est pas finie ; sinon, réponds à l'utilisateur en résumant ce que tu as fait et le résultat des vérifications.";

/// Demande l'avis des hooks `Notification` avant chaque demande d'autorisation.
struct NotifyingPrompter<'a> {
    hooks: &'a HookRunner,
    inner: &'a mut dyn PermissionPrompter,
}

impl PermissionPrompter for NotifyingPrompter<'_> {
    fn decide(&mut self, request: &PermissionRequest) -> PermissionPromptDecision {
        self.hooks.notify_in_background(
            "permission_prompt",
            &format!(
                "Claw demande l'autorisation d'utiliser {}",
                request.tool_name
            ),
        );
        self.inner.decide(request)
    }
}

impl<C, T> ConversationRuntime<C, T>
where
    C: ApiClient,
    T: ToolExecutor,
{
    #[must_use]
    pub fn new(
        session: Session,
        api_client: C,
        tool_executor: T,
        permission_policy: PermissionPolicy,
        system_prompt: Vec<String>,
    ) -> Self {
        Self::new_with_features(
            session,
            api_client,
            tool_executor,
            permission_policy,
            system_prompt,
            RuntimeFeatureConfig::default(),
        )
    }

    #[must_use]
    pub fn new_with_features(
        session: Session,
        api_client: C,
        tool_executor: T,
        permission_policy: PermissionPolicy,
        system_prompt: Vec<String>,
        feature_config: RuntimeFeatureConfig,
    ) -> Self {
        let usage_tracker = UsageTracker::from_session(&session);
        Self {
            session,
            api_client,
            tool_executor,
            permission_policy,
            system_prompt,
            max_iterations: usize::MAX,
            usage_tracker,
            hook_runner: HookRunner::from_feature_config(&feature_config),
            auto_compaction: None,
            checkpoint_turns: false,
            subagent: false,
        }
    }

    /// Runtime d'un sous-agent : pas de `UserPromptSubmit`, et `SubagentStop`
    /// remplace `Stop`.
    #[must_use]
    pub fn as_subagent(mut self) -> Self {
        self.subagent = true;
        self
    }

    #[must_use]
    pub fn hooks(&self) -> &HookRunner {
        &self.hook_runner
    }

    /// Signale à l'utilisateur les retours de hooks qui ne vont pas au modèle.
    fn notify_hook_feedback(&mut self, event: HookEvent, result: &HookRunResult) {
        for message in result.feedback() {
            self.api_client
                .notify(&format!("hook {} : {message}", event.as_str()));
        }
    }

    /// Ouvre un point de restauration à chaque tour (runtime principal) ; les
    /// écritures de fichiers sont toujours enregistrées dans le tour actif.
    #[must_use]
    pub fn with_checkpoint_turns(mut self, enabled: bool) -> Self {
        self.checkpoint_turns = enabled;
        self
    }

    /// Après `checkpoint::undo`, ramène la conversation avant le tour annulé si
    /// elle n'a pas été réécrite entre-temps (compaction). Renvoie vrai si fait.
    pub fn rewind_to(&mut self, report: &crate::checkpoint::UndoReport) -> bool {
        let matches = self
            .session
            .messages
            .get(report.message_index)
            .is_some_and(|message| {
                message.role == crate::session::MessageRole::User
                    && message.blocks.iter().any(|block| {
                        matches!(block, ContentBlock::Text { text } if text.starts_with(&report.request))
                    })
            });
        if matches {
            self.session.messages.truncate(report.message_index);
        }
        matches
    }

    #[must_use]
    pub fn with_max_iterations(mut self, max_iterations: usize) -> Self {
        self.max_iterations = max_iterations;
        self
    }

    #[must_use]
    pub fn with_auto_compaction(mut self, config: Option<AutoCompactionConfig>) -> Self {
        self.auto_compaction = config;
        self
    }

    /// Contexte estimé (prompt système compris), en jetons.
    #[must_use]
    pub fn estimated_context_tokens(&self) -> usize {
        let system = self
            .system_prompt
            .iter()
            .map(|section| section.len() / 4 + 1)
            .sum::<usize>();
        system + estimate_session_tokens(&self.session)
    }

    /// Compacte si le seuil est dépassé (ou toujours si `force`). Renvoie vrai
    /// si des messages ont été résumés.
    fn auto_compact(&mut self, force: bool) -> bool {
        let Some(config) = self.auto_compaction else {
            return false;
        };
        if !force && self.estimated_context_tokens() < config.threshold_tokens {
            return false;
        }
        // On garde les messages récents dans la limite d'un tiers du seuil (au
        // moins le dernier appel d'outil et son résultat) : ce sont souvent eux
        // qui pèsent le plus.
        let budget = config.threshold_tokens / 3;
        let mut kept_tokens = 0;
        let mut preserve = 0;
        for message in self.session.messages.iter().rev() {
            let tokens = estimate_message_tokens(message);
            if preserve >= 2 && kept_tokens + tokens > budget {
                break;
            }
            kept_tokens += tokens;
            preserve += 1;
            if preserve >= config.preserve_recent_messages {
                break;
            }
        }
        let compaction = CompactionConfig {
            preserve_recent_messages: preserve,
            max_estimated_tokens: 0,
        };
        let pre_compact = self.hook_runner.run_event(
            HookEvent::PreCompact,
            Some("auto"),
            serde_json::json!({ "trigger": "auto", "custom_instructions": "" }),
            &[],
        );
        self.notify_hook_feedback(HookEvent::PreCompact, &pre_compact);
        let before = self.estimated_context_tokens();
        let client = &mut self.api_client;
        let result = compact_session_with(&self.session, compaction, |messages| {
            client.summarize(
                MODEL_SUMMARY_PROMPT,
                &render_transcript_for_summary(messages),
            )
        });
        let summarized = result.removed_message_count;
        if summarized > 0 {
            self.session = result.compacted_session;
        }
        // Un résultat d'outil énorme peut à lui seul dépasser le seuil : on le tronque.
        let truncated = if self.estimated_context_tokens() > config.threshold_tokens {
            truncate_tool_outputs(&mut self.session, budget.saturating_mul(4).max(4_000))
        } else {
            0
        };
        if summarized == 0 && truncated == 0 {
            return false;
        }
        let after = self.estimated_context_tokens();
        let truncated_note = if truncated > 0 {
            format!(", {truncated} sortie(s) d'outil tronquée(s)")
        } else {
            String::new()
        };
        let notice = format!(
            "Contexte compacté automatiquement : {summarized} message(s) résumé(s){truncated_note}, ~{before} → ~{after} jetons"
        );
        self.api_client.notify(&notice);
        true
    }

    pub fn run_turn(
        &mut self,
        user_input: impl Into<String>,
        prompter: Option<&mut dyn PermissionPrompter>,
    ) -> Result<TurnSummary, RuntimeError> {
        self.run_turn_with_images(user_input, Vec::new(), prompter)
    }

    /// Comme `run_turn`, avec des images jointes à la demande.
    pub fn run_turn_with_images(
        &mut self,
        user_input: impl Into<String>,
        images: Vec<crate::attachments::ImageAttachment>,
        mut prompter: Option<&mut dyn PermissionPrompter>,
    ) -> Result<TurnSummary, RuntimeError> {
        clear_interrupt();
        let mut user_message = self.user_message_with_hooks(user_input.into())?;
        user_message
            .blocks
            .extend(images.into_iter().map(|image| ContentBlock::Image {
                media_type: image.media_type,
                data: image.data,
            }));
        self.session.messages.push(user_message);

        let mut assistant_messages = Vec::new();
        let mut tool_results = Vec::new();
        let mut iterations = 0;
        let mut stop_continuations = 0;
        let mut empty_nudged = false;

        loop {
            iterations += 1;
            if iterations > self.max_iterations {
                return Err(RuntimeError::new(
                    "la boucle de conversation a dépassé le nombre maximal d'itérations",
                ));
            }

            if interrupt_requested() {
                return Err(RuntimeError::new(INTERRUPTED_MESSAGE));
            }
            self.auto_compact(false);
            let request = ApiRequest {
                system_prompt: self.system_prompt.clone(),
                messages: self.session.messages.clone(),
            };
            let events = match self.api_client.stream(request) {
                Ok(events) => events,
                // Contexte trop long malgré l'estimation : on compacte et on réessaie une fois.
                Err(error)
                    if is_context_overflow(&error.to_string()) && self.auto_compact(true) =>
                {
                    self.api_client.stream(ApiRequest {
                        system_prompt: self.system_prompt.clone(),
                        messages: self.session.messages.clone(),
                    })?
                }
                Err(error) => return Err(error),
            };
            let (assistant_message, usage) = match build_assistant_message(events) {
                Ok(built) => built,
                Err(error) if error.to_string() == EMPTY_RESPONSE_ERROR && !empty_nudged => {
                    empty_nudged = true;
                    self.session
                        .messages
                        .push(ConversationMessage::user_text(EMPTY_RESPONSE_NUDGE));
                    continue;
                }
                Err(error) => return Err(error),
            };
            if let Some(usage) = usage {
                self.usage_tracker.record(usage);
            }
            let pending_tool_uses = assistant_message
                .blocks
                .iter()
                .filter_map(|block| match block {
                    ContentBlock::ToolUse { id, name, input } => {
                        Some((id.clone(), name.clone(), input.clone()))
                    }
                    _ => None,
                })
                .collect::<Vec<_>>();

            self.session.messages.push(assistant_message.clone());
            assistant_messages.push(assistant_message);

            if pending_tool_uses.is_empty() {
                if let Some(reason) = self.run_stop_hooks(stop_continuations, &assistant_messages) {
                    stop_continuations += 1;
                    self.session
                        .messages
                        .push(ConversationMessage::user_text(format!(
                            "{STOP_HOOK_PREFIX} {reason}"
                        )));
                    continue;
                }
                break;
            }

            if self.run_parallel_batch(&pending_tool_uses, &mut prompter, &mut tool_results) {
                continue;
            }
            for (tool_use_id, tool_name, input) in pending_tool_uses {
                // Après une interruption, chaque appel restant reçoit un résultat :
                // un appel d'outil sans résultat rendrait la conversation invalide.
                if interrupt_requested() {
                    let message = ConversationMessage::tool_result(
                        tool_use_id,
                        tool_name,
                        INTERRUPTED_MESSAGE.to_string(),
                        true,
                    );
                    self.session.messages.push(message.clone());
                    tool_results.push(message);
                    continue;
                }
                let result_message = self.execute_tool_call(
                    tool_use_id,
                    tool_name,
                    &input,
                    prompter
                        .as_mut()
                        .map(|prompt| &mut **prompt as &mut dyn PermissionPrompter),
                );
                self.session.messages.push(result_message.clone());
                tool_results.push(result_message);
                // Plan approuvé (ExitPlanMode) : la suite du tour peut modifier.
                if let Some(mode) = crate::permissions::take_mode_switch() {
                    self.permission_policy.set_active_mode(mode);
                }
            }
        }

        Ok(TurnSummary {
            assistant_messages,
            tool_results,
            iterations,
            usage: self.usage_tracker.cumulative_usage(),
        })
    }

    /// Hooks `UserPromptSubmit` (qui peuvent rejeter la demande) puis message
    /// utilisateur, complété par le contexte des hooks ; ouvre le point de
    /// restauration du tour.
    fn user_message_with_hooks(
        &mut self,
        user_input: String,
    ) -> Result<ConversationMessage, RuntimeError> {
        let mut context = Vec::new();
        if !self.subagent {
            context = crate::hooks::take_pending_context();
            let submitted = self.hook_runner.run_event(
                HookEvent::UserPromptSubmit,
                None,
                serde_json::json!({ "prompt": user_input }),
                &[("HOOK_PROMPT", user_input.clone())],
            );
            if submitted.is_denied() {
                return Err(RuntimeError::new(format!(
                    "demande bloquée par un hook UserPromptSubmit : {}",
                    submitted.reason("aucune raison donnée")
                )));
            }
            self.notify_hook_feedback(HookEvent::UserPromptSubmit, &submitted);
            context.extend(submitted.context().iter().cloned());
        }
        if self.checkpoint_turns {
            crate::checkpoint::begin_turn(self.session.messages.len(), &user_input);
        }
        let mut user_message = ConversationMessage::user_text(user_input);
        if !context.is_empty() {
            user_message.blocks.push(ContentBlock::Text {
                text: format!("{HOOK_CONTEXT_PREFIX}\n{}", context.join("\n\n")),
            });
        }
        Ok(user_message)
    }

    /// Plusieurs appels sans effet (sous-agents, lectures, recherches) : les
    /// autorisations et hooks restent séquentiels, l'exécution est simultanée.
    /// Renvoie faux si le lot ne s'y prête pas (traitement appel par appel).
    fn run_parallel_batch(
        &mut self,
        calls: &[(String, String, String)],
        prompter: &mut Option<&mut dyn PermissionPrompter>,
        tool_results: &mut Vec<ConversationMessage>,
    ) -> bool {
        if calls.len() < 2
            || interrupt_requested()
            || !calls
                .iter()
                .all(|(_, name, _)| self.tool_executor.parallel_safe(name))
        {
            return false;
        }
        // 1. Autorisations et hooks PreToolUse, dans l'ordre.
        let mut results: Vec<Option<ConversationMessage>> = vec![None; calls.len()];
        let mut ready = Vec::new();
        let mut pre_messages = Vec::new();
        for (index, (id, name, input)) in calls.iter().enumerate() {
            let mut notifying = prompter.as_mut().map(|prompt| NotifyingPrompter {
                hooks: &self.hook_runner,
                inner: &mut **prompt,
            });
            let outcome = self.permission_policy.authorize(
                name,
                input,
                notifying
                    .as_mut()
                    .map(|prompt| prompt as &mut dyn PermissionPrompter),
            );
            match outcome {
                PermissionOutcome::Deny { reason } => {
                    results[index] = Some(ConversationMessage::tool_result(
                        id.clone(),
                        name.clone(),
                        reason,
                        true,
                    ));
                }
                PermissionOutcome::Allow => {
                    let pre = self.hook_runner.run_pre_tool_use(name, input);
                    if pre.is_denied() {
                        let fallback = format!("hook PreToolUse : outil `{name}` refusé");
                        results[index] = Some(ConversationMessage::tool_result(
                            id.clone(),
                            name.clone(),
                            format_hook_message(&pre, &fallback),
                            true,
                        ));
                    } else {
                        ready.push(index);
                        pre_messages.push(pre.messages().to_vec());
                    }
                }
            }
        }
        // 2. Exécution simultanée des appels autorisés.
        let batch = ready
            .iter()
            .map(|index| (calls[*index].1.clone(), calls[*index].2.clone()))
            .collect::<Vec<_>>();
        let outputs = if batch.is_empty() {
            Vec::new()
        } else {
            self.tool_executor.execute_batch(&batch)
        };
        // 3. Hooks PostToolUse et résultats, dans l'ordre d'origine.
        for ((index, outcome), pre) in ready.iter().zip(outputs).zip(pre_messages) {
            let (id, name, input) = &calls[*index];
            let (mut output, mut is_error) = match outcome {
                Ok(output) => (output, false),
                Err(error) => (error.to_string(), true),
            };
            output = merge_hook_feedback(&pre, output, false);
            let post = self
                .hook_runner
                .run_post_tool_use(name, input, &output, is_error);
            if post.is_denied() {
                is_error = true;
            }
            output = merge_hook_feedback(post.messages(), output, post.is_denied());
            results[*index] = Some(ConversationMessage::tool_result(
                id.clone(),
                name.clone(),
                output,
                is_error,
            ));
        }
        for message in results.into_iter().flatten() {
            self.session.messages.push(message.clone());
            tool_results.push(message);
        }
        true
    }

    /// Autorisation, hooks `PreToolUse`/`PostToolUse`, point de restauration
    /// et exécution d'un appel d'outil ; renvoie le message de résultat.
    fn execute_tool_call(
        &mut self,
        tool_use_id: String,
        tool_name: String,
        input: &str,
        prompter: Option<&mut dyn PermissionPrompter>,
    ) -> ConversationMessage {
        let mut notifying = prompter.map(|prompt| NotifyingPrompter {
            hooks: &self.hook_runner,
            inner: prompt,
        });
        let permission_outcome = self.permission_policy.authorize(
            &tool_name,
            input,
            notifying
                .as_mut()
                .map(|prompt| prompt as &mut dyn PermissionPrompter),
        );
        match permission_outcome {
            PermissionOutcome::Allow => {
                let pre_hook_result = self.hook_runner.run_pre_tool_use(&tool_name, input);
                if pre_hook_result.is_denied() {
                    let deny_message = format!("hook PreToolUse : outil `{tool_name}` refusé");
                    ConversationMessage::tool_result(
                        tool_use_id,
                        tool_name,
                        format_hook_message(&pre_hook_result, &deny_message),
                        true,
                    )
                } else {
                    crate::checkpoint::record_before_write(&tool_name, input);
                    let (mut output, mut is_error) =
                        match self.tool_executor.execute(&tool_name, input) {
                            Ok(output) => (output, false),
                            Err(error) => (error.to_string(), true),
                        };
                    output = merge_hook_feedback(pre_hook_result.messages(), output, false);

                    let post_hook_result = self
                        .hook_runner
                        .run_post_tool_use(&tool_name, input, &output, is_error);
                    if post_hook_result.is_denied() {
                        is_error = true;
                    }
                    output = merge_hook_feedback(
                        post_hook_result.messages(),
                        output,
                        post_hook_result.is_denied(),
                    );

                    ConversationMessage::tool_result(tool_use_id, tool_name, output, is_error)
                }
            }
            PermissionOutcome::Deny { reason } => {
                ConversationMessage::tool_result(tool_use_id, tool_name, reason, true)
            }
        }
    }

    /// Hooks `Stop` (ou `SubagentStop`) en fin de tour : renvoie la raison si un
    /// hook demande à l'agent de continuer.
    fn run_stop_hooks(
        &mut self,
        continuations: usize,
        assistant_messages: &[ConversationMessage],
    ) -> Option<String> {
        let event = if self.subagent {
            HookEvent::SubagentStop
        } else {
            HookEvent::Stop
        };
        if !self.hook_runner.has(event) || interrupt_requested() {
            return None;
        }
        let last_message = assistant_messages
            .last()
            .map(|message| {
                message
                    .blocks
                    .iter()
                    .filter_map(|block| match block {
                        ContentBlock::Text { text } => Some(text.as_str()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_default();
        let result = self.hook_runner.run_event(
            event,
            None,
            serde_json::json!({
                "stop_hook_active": continuations > 0,
                "last_assistant_message": last_message,
            }),
            &[],
        );
        if !result.is_denied() {
            self.notify_hook_feedback(event, &result);
            return None;
        }
        let reason = result.reason("le hook demande de continuer");
        if continuations >= MAX_STOP_HOOK_CONTINUATIONS {
            self.api_client.notify(&format!(
                "hook {} : {MAX_STOP_HOOK_CONTINUATIONS} relances déjà faites, l'agent s'arrête ({reason})",
                event.as_str()
            ));
            return None;
        }
        self.api_client.notify(&format!(
            "hook {} : l'agent continue — {reason}",
            event.as_str()
        ));
        Some(reason)
    }

    /// Compaction manuelle (`/compact`), précédée des hooks `PreCompact`.
    #[must_use]
    pub fn compact(&self, config: CompactionConfig) -> CompactionResult {
        let _ = self.hook_runner.run_event(
            HookEvent::PreCompact,
            Some("manual"),
            serde_json::json!({ "trigger": "manual", "custom_instructions": "" }),
            &[],
        );
        compact_session(&self.session, config)
    }

    #[must_use]
    pub fn estimated_tokens(&self) -> usize {
        estimate_session_tokens(&self.session)
    }

    #[must_use]
    pub fn usage(&self) -> &UsageTracker {
        &self.usage_tracker
    }

    #[must_use]
    pub fn session(&self) -> &Session {
        &self.session
    }

    #[must_use]
    pub fn permission_policy(&self) -> &PermissionPolicy {
        &self.permission_policy
    }

    #[must_use]
    pub fn into_session(self) -> Session {
        self.session
    }
}

fn build_assistant_message(
    events: Vec<AssistantEvent>,
) -> Result<(ConversationMessage, Option<TokenUsage>), RuntimeError> {
    let mut text = String::new();
    let mut blocks = Vec::new();
    let mut finished = false;
    let mut usage = None;

    for event in events {
        match event {
            AssistantEvent::TextDelta(delta) => text.push_str(&delta),
            AssistantEvent::ToolUse { id, name, input } => {
                flush_text_block(&mut text, &mut blocks);
                // Arguments mal formés (petits modèles) : réparés si possible, sinon
                // laissés tels quels pour que l'outil renvoie une erreur explicite.
                let input = crate::json_repair::repair_tool_input(&input).unwrap_or(input);
                blocks.push(ContentBlock::ToolUse { id, name, input });
            }
            AssistantEvent::Thinking {
                thinking,
                signature,
                redacted,
            } => {
                flush_text_block(&mut text, &mut blocks);
                blocks.push(ContentBlock::Thinking {
                    thinking,
                    signature,
                    redacted,
                });
            }
            AssistantEvent::Usage(value) => usage = Some(value),
            AssistantEvent::MessageStop => {
                finished = true;
            }
        }
    }

    flush_text_block(&mut text, &mut blocks);

    if !finished {
        return Err(RuntimeError::new(
            "le flux du modèle s'est terminé sans fin de message (réponse incomplète)",
        ));
    }
    if blocks.is_empty() {
        return Err(RuntimeError::new(EMPTY_RESPONSE_ERROR));
    }

    Ok((
        ConversationMessage::assistant_with_usage(blocks, usage),
        usage,
    ))
}

fn flush_text_block(text: &mut String, blocks: &mut Vec<ContentBlock>) {
    if !text.is_empty() {
        blocks.push(ContentBlock::Text {
            text: std::mem::take(text),
        });
    }
}

/// Tronque les résultats d'outils plus longs que `max_chars` ; renvoie leur nombre.
fn truncate_tool_outputs(session: &mut Session, max_chars: usize) -> usize {
    let mut truncated = 0;
    for message in &mut session.messages {
        for block in &mut message.blocks {
            if let ContentBlock::ToolResult { output, .. } = block {
                if output.chars().count() > max_chars {
                    let kept = output.chars().take(max_chars).collect::<String>();
                    *output = format!(
                        "{kept}\n[… sortie tronquée lors de la compaction du contexte ; relancez l'outil avec une portée plus étroite si besoin]"
                    );
                    truncated += 1;
                }
            }
        }
    }
    truncated
}

/// Erreurs d'API signalant un dépassement de la fenêtre de contexte.
fn is_context_overflow(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    (lower.contains("context")
        && (lower.contains("length") || lower.contains("window") || lower.contains("too long")))
        || lower.contains("prompt is too long")
        || lower.contains("maximum context")
        || lower.contains("too many tokens")
}

fn format_hook_message(result: &HookRunResult, fallback: &str) -> String {
    if result.messages().is_empty() {
        fallback.to_string()
    } else {
        result.messages().join("\n")
    }
}

fn merge_hook_feedback(messages: &[String], output: String, denied: bool) -> String {
    if messages.is_empty() {
        return output;
    }

    let mut sections = Vec::new();
    if !output.trim().is_empty() {
        sections.push(output);
    }
    let label = if denied {
        "Retour des hooks (refusé)"
    } else {
        "Retour des hooks"
    };
    sections.push(format!("{label}:\n{}", messages.join("\n")));
    sections.join("\n\n")
}

type ToolHandler = Box<dyn FnMut(&str) -> Result<String, ToolError>>;

#[derive(Default)]
pub struct StaticToolExecutor {
    handlers: BTreeMap<String, ToolHandler>,
}

impl StaticToolExecutor {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn register(
        mut self,
        tool_name: impl Into<String>,
        handler: impl FnMut(&str) -> Result<String, ToolError> + 'static,
    ) -> Self {
        self.handlers.insert(tool_name.into(), Box::new(handler));
        self
    }
}

impl ToolExecutor for StaticToolExecutor {
    fn execute(&mut self, tool_name: &str, input: &str) -> Result<String, ToolError> {
        self.handlers
            .get_mut(tool_name)
            .ok_or_else(|| ToolError::new(format!("unknown tool: {tool_name}")))?(input)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ApiClient, ApiRequest, AssistantEvent, ConversationRuntime, RuntimeError,
        StaticToolExecutor,
    };
    use crate::compact::CompactionConfig;
    use crate::config::{RuntimeFeatureConfig, RuntimeHookConfig};
    use crate::permissions::{
        PermissionMode, PermissionPolicy, PermissionPromptDecision, PermissionPrompter,
        PermissionRequest,
    };
    use crate::prompt::{ProjectContext, SystemPromptBuilder};
    use crate::session::{ContentBlock, MessageRole, Session};
    use crate::usage::TokenUsage;
    use std::path::PathBuf;

    struct ScriptedApiClient {
        call_count: usize,
    }

    impl ApiClient for ScriptedApiClient {
        fn stream(&mut self, request: ApiRequest) -> Result<Vec<AssistantEvent>, RuntimeError> {
            self.call_count += 1;
            match self.call_count {
                1 => {
                    assert!(request
                        .messages
                        .iter()
                        .any(|message| message.role == MessageRole::User));
                    Ok(vec![
                        AssistantEvent::TextDelta("Let me calculate that.".to_string()),
                        AssistantEvent::ToolUse {
                            id: "tool-1".to_string(),
                            name: "add".to_string(),
                            input: "2,2".to_string(),
                        },
                        AssistantEvent::Usage(TokenUsage {
                            input_tokens: 20,
                            output_tokens: 6,
                            cache_creation_input_tokens: 1,
                            cache_read_input_tokens: 2,
                        }),
                        AssistantEvent::MessageStop,
                    ])
                }
                2 => {
                    let last_message = request
                        .messages
                        .last()
                        .expect("tool result should be present");
                    assert_eq!(last_message.role, MessageRole::Tool);
                    Ok(vec![
                        AssistantEvent::TextDelta("The answer is 4.".to_string()),
                        AssistantEvent::Usage(TokenUsage {
                            input_tokens: 24,
                            output_tokens: 4,
                            cache_creation_input_tokens: 1,
                            cache_read_input_tokens: 3,
                        }),
                        AssistantEvent::MessageStop,
                    ])
                }
                _ => Err(RuntimeError::new("unexpected extra API call")),
            }
        }
    }

    struct PromptAllowOnce;

    impl PermissionPrompter for PromptAllowOnce {
        fn decide(&mut self, request: &PermissionRequest) -> PermissionPromptDecision {
            assert_eq!(request.tool_name, "add");
            PermissionPromptDecision::Allow
        }
    }

    #[test]
    fn runs_user_to_tool_to_result_loop_end_to_end_and_tracks_usage() {
        let api_client = ScriptedApiClient { call_count: 0 };
        let tool_executor = StaticToolExecutor::new().register("add", |input| {
            let total = input
                .split(',')
                .map(|part| part.parse::<i32>().expect("input must be valid integer"))
                .sum::<i32>();
            Ok(total.to_string())
        });
        let permission_policy = PermissionPolicy::new(PermissionMode::WorkspaceWrite);
        let system_prompt = SystemPromptBuilder::new()
            .with_project_context(ProjectContext {
                cwd: PathBuf::from("/tmp/project"),
                current_date: "2026-03-31".to_string(),
                git_status: None,
                git_diff: None,
                instruction_files: Vec::new(),
            })
            .with_os("linux", "6.8")
            .build();
        let mut runtime = ConversationRuntime::new(
            Session::new(),
            api_client,
            tool_executor,
            permission_policy,
            system_prompt,
        );

        let summary = runtime
            .run_turn("what is 2 + 2?", Some(&mut PromptAllowOnce))
            .expect("conversation loop should succeed");

        assert_eq!(summary.iterations, 2);
        assert_eq!(summary.assistant_messages.len(), 2);
        assert_eq!(summary.tool_results.len(), 1);
        assert_eq!(runtime.session().messages.len(), 4);
        assert_eq!(summary.usage.output_tokens, 10);
        assert!(matches!(
            runtime.session().messages[1].blocks[1],
            ContentBlock::ToolUse { .. }
        ));
        assert!(matches!(
            runtime.session().messages[2].blocks[0],
            ContentBlock::ToolResult {
                is_error: false,
                ..
            }
        ));
    }

    #[test]
    fn records_denied_tool_results_when_prompt_rejects() {
        struct RejectPrompter;
        impl PermissionPrompter for RejectPrompter {
            fn decide(&mut self, _request: &PermissionRequest) -> PermissionPromptDecision {
                PermissionPromptDecision::Deny {
                    reason: "not now".to_string(),
                }
            }
        }

        struct SingleCallApiClient;
        impl ApiClient for SingleCallApiClient {
            fn stream(&mut self, request: ApiRequest) -> Result<Vec<AssistantEvent>, RuntimeError> {
                if request
                    .messages
                    .iter()
                    .any(|message| message.role == MessageRole::Tool)
                {
                    return Ok(vec![
                        AssistantEvent::TextDelta("I could not use the tool.".to_string()),
                        AssistantEvent::MessageStop,
                    ]);
                }
                Ok(vec![
                    AssistantEvent::ToolUse {
                        id: "tool-1".to_string(),
                        name: "blocked".to_string(),
                        input: "secret".to_string(),
                    },
                    AssistantEvent::MessageStop,
                ])
            }
        }

        let mut runtime = ConversationRuntime::new(
            Session::new(),
            SingleCallApiClient,
            StaticToolExecutor::new(),
            PermissionPolicy::new(PermissionMode::WorkspaceWrite),
            vec!["system".to_string()],
        );

        let summary = runtime
            .run_turn("use the tool", Some(&mut RejectPrompter))
            .expect("conversation should continue after denied tool");

        assert_eq!(summary.tool_results.len(), 1);
        assert!(matches!(
            &summary.tool_results[0].blocks[0],
            ContentBlock::ToolResult { is_error: true, output, .. } if output == "not now"
        ));
    }

    #[test]
    fn an_empty_reply_is_followed_up_once_instead_of_failing() {
        struct EmptyThenText {
            calls: usize,
        }
        impl ApiClient for EmptyThenText {
            fn stream(&mut self, request: ApiRequest) -> Result<Vec<AssistantEvent>, RuntimeError> {
                self.calls += 1;
                if self.calls == 1 {
                    // Petit modèle : uniquement du raisonnement, rien d'affichable.
                    return Ok(vec![AssistantEvent::MessageStop]);
                }
                let nudged = request.messages.iter().any(|message| {
                    message.blocks.iter().any(|block| {
                        matches!(block, ContentBlock::Text { text } if text == super::EMPTY_RESPONSE_NUDGE)
                    })
                });
                assert!(nudged, "la relance doit être envoyée au modèle");
                Ok(vec![
                    AssistantEvent::TextDelta("Terminé.".to_string()),
                    AssistantEvent::MessageStop,
                ])
            }
        }

        let mut runtime = ConversationRuntime::new(
            Session::new(),
            EmptyThenText { calls: 0 },
            StaticToolExecutor::new(),
            PermissionPolicy::new(PermissionMode::WorkspaceWrite),
            vec!["system".to_string()],
        );
        let summary = runtime
            .run_turn("corrige le bug", None)
            .expect("une réponse vide ne doit pas faire échouer le tour");
        assert_eq!(runtime.api_client.calls, 2);
        assert!(matches!(
            summary.assistant_messages.last().and_then(|message| message.blocks.first()),
            Some(ContentBlock::Text { text }) if text == "Terminé."
        ));

        // Deux réponses vides de suite : l'erreur est rendue.
        struct AlwaysEmpty;
        impl ApiClient for AlwaysEmpty {
            fn stream(
                &mut self,
                _request: ApiRequest,
            ) -> Result<Vec<AssistantEvent>, RuntimeError> {
                Ok(vec![AssistantEvent::MessageStop])
            }
        }
        let mut runtime = ConversationRuntime::new(
            Session::new(),
            AlwaysEmpty,
            StaticToolExecutor::new(),
            PermissionPolicy::new(PermissionMode::WorkspaceWrite),
            vec!["system".to_string()],
        );
        assert!(runtime.run_turn("corrige le bug", None).is_err());
    }

    struct RecordingTextApi {
        requests: Vec<ApiRequest>,
    }

    impl ApiClient for RecordingTextApi {
        fn stream(&mut self, request: ApiRequest) -> Result<Vec<AssistantEvent>, RuntimeError> {
            self.requests.push(request);
            Ok(vec![
                AssistantEvent::TextDelta(format!("réponse {}", self.requests.len())),
                AssistantEvent::MessageStop,
            ])
        }
    }

    #[test]
    fn user_prompt_hooks_add_context_and_stop_hooks_can_resume_the_agent() {
        let marker = std::env::temp_dir().join(format!(
            "claw-stop-hook-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        let marker = marker.display().to_string().replace('\\', "/");
        let hooks = RuntimeHookConfig::default()
            .with_commands(
                crate::hooks::HookEvent::UserPromptSubmit,
                vec!["printf \"branche: main\"".to_string()],
            )
            .with_commands(
                crate::hooks::HookEvent::Stop,
                vec![format!(
                    "if [ -f \"{marker}\" ]; then exit 0; fi; touch \"{marker}\"; printf \"lance les tests\" >&2; exit 2"
                )],
            );
        let mut runtime = ConversationRuntime::new_with_features(
            Session::new(),
            RecordingTextApi {
                requests: Vec::new(),
            },
            StaticToolExecutor::new(),
            PermissionPolicy::new(PermissionMode::DangerFullAccess),
            vec!["system".to_string()],
            RuntimeFeatureConfig::default().with_hooks(hooks),
        );

        let summary = runtime.run_turn("salut", None).expect("turn");
        let _ = std::fs::remove_file(&marker);

        assert_eq!(
            summary.assistant_messages.len(),
            2,
            "le hook Stop relance une fois"
        );
        let first_user = &runtime.session().messages[0];
        assert_eq!(first_user.blocks.len(), 2);
        assert!(matches!(&first_user.blocks[1],
            ContentBlock::Text { text } if text.starts_with(super::HOOK_CONTEXT_PREFIX) && text.contains("branche: main")));
        assert!(runtime.session().messages.iter().any(|message| message.role == MessageRole::User
            && matches!(&message.blocks[0], ContentBlock::Text { text } if text == "[Hook Stop] lance les tests")));
    }

    #[test]
    fn notification_hooks_run_when_permission_is_requested() {
        let file = std::env::temp_dir().join(format!(
            "claw-notification-{}-{}.json",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        let target = file.display().to_string().replace('\\', "/");
        let hooks = RuntimeHookConfig::default().with_commands(
            crate::hooks::HookEvent::Notification,
            vec![format!("cat > \"{target}\"")],
        );
        let mut runtime = ConversationRuntime::new_with_features(
            Session::new(),
            ScriptedApiClient { call_count: 0 },
            StaticToolExecutor::new().register("add", |_input| Ok("4".to_string())),
            PermissionPolicy::new(PermissionMode::WorkspaceWrite),
            vec!["system".to_string()],
            RuntimeFeatureConfig::default().with_hooks(hooks),
        );
        runtime
            .run_turn("what is 2 + 2?", Some(&mut PromptAllowOnce))
            .expect("turn");

        // Le hook tourne en arrière-plan : on attend son fichier.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        let payload = loop {
            if let Some(payload) = std::fs::read_to_string(&file)
                .ok()
                .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
            {
                break payload;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "hook Notification non exécuté"
            );
            std::thread::sleep(std::time::Duration::from_millis(50));
        };
        let _ = std::fs::remove_file(&file);
        assert_eq!(payload["hook_event_name"], "Notification");
        assert_eq!(payload["notification_type"], "permission_prompt");
        assert!(payload["message"]
            .as_str()
            .is_some_and(|text| text.contains("add")));
    }

    #[test]
    fn user_prompt_hooks_can_reject_a_request() {
        let hooks = RuntimeHookConfig::default().with_commands(
            crate::hooks::HookEvent::UserPromptSubmit,
            vec!["printf \"secret détecté\" >&2; exit 2".to_string()],
        );
        let mut runtime = ConversationRuntime::new_with_features(
            Session::new(),
            RecordingTextApi {
                requests: Vec::new(),
            },
            StaticToolExecutor::new(),
            PermissionPolicy::new(PermissionMode::DangerFullAccess),
            vec!["system".to_string()],
            RuntimeFeatureConfig::default().with_hooks(hooks),
        );
        let error = runtime
            .run_turn("voici ma clé sk-123", None)
            .expect_err("bloqué");
        assert!(error.to_string().contains("secret détecté"), "{error}");
        assert!(
            runtime.session().messages.is_empty(),
            "la demande n'est pas enregistrée"
        );
    }

    #[test]
    fn denies_tool_use_when_pre_tool_hook_blocks() {
        struct SingleCallApiClient;
        impl ApiClient for SingleCallApiClient {
            fn stream(&mut self, request: ApiRequest) -> Result<Vec<AssistantEvent>, RuntimeError> {
                if request
                    .messages
                    .iter()
                    .any(|message| message.role == MessageRole::Tool)
                {
                    return Ok(vec![
                        AssistantEvent::TextDelta("blocked".to_string()),
                        AssistantEvent::MessageStop,
                    ]);
                }
                Ok(vec![
                    AssistantEvent::ToolUse {
                        id: "tool-1".to_string(),
                        name: "blocked".to_string(),
                        input: r#"{"path":"secret.txt"}"#.to_string(),
                    },
                    AssistantEvent::MessageStop,
                ])
            }
        }

        let mut runtime = ConversationRuntime::new_with_features(
            Session::new(),
            SingleCallApiClient,
            StaticToolExecutor::new().register("blocked", |_input| {
                panic!("tool should not execute when hook denies")
            }),
            PermissionPolicy::new(PermissionMode::DangerFullAccess),
            vec!["system".to_string()],
            RuntimeFeatureConfig::default().with_hooks(RuntimeHookConfig::new(
                vec![shell_snippet("printf 'blocked by hook'; exit 2")],
                Vec::new(),
            )),
        );

        let summary = runtime
            .run_turn("use the tool", None)
            .expect("conversation should continue after hook denial");

        assert_eq!(summary.tool_results.len(), 1);
        let ContentBlock::ToolResult {
            is_error, output, ..
        } = &summary.tool_results[0].blocks[0]
        else {
            panic!("expected tool result block");
        };
        assert!(
            *is_error,
            "hook denial should produce an error result: {output}"
        );
        assert!(
            output.contains("denied tool") || output.contains("blocked by hook"),
            "unexpected hook denial output: {output:?}"
        );
    }

    #[test]
    fn appends_post_tool_hook_feedback_to_tool_result() {
        struct TwoCallApiClient {
            calls: usize,
        }

        impl ApiClient for TwoCallApiClient {
            fn stream(&mut self, request: ApiRequest) -> Result<Vec<AssistantEvent>, RuntimeError> {
                self.calls += 1;
                match self.calls {
                    1 => Ok(vec![
                        AssistantEvent::ToolUse {
                            id: "tool-1".to_string(),
                            name: "add".to_string(),
                            input: r#"{"lhs":2,"rhs":2}"#.to_string(),
                        },
                        AssistantEvent::MessageStop,
                    ]),
                    2 => {
                        assert!(request
                            .messages
                            .iter()
                            .any(|message| message.role == MessageRole::Tool));
                        Ok(vec![
                            AssistantEvent::TextDelta("done".to_string()),
                            AssistantEvent::MessageStop,
                        ])
                    }
                    _ => Err(RuntimeError::new("unexpected extra API call")),
                }
            }
        }

        let mut runtime = ConversationRuntime::new_with_features(
            Session::new(),
            TwoCallApiClient { calls: 0 },
            StaticToolExecutor::new().register("add", |_input| Ok("4".to_string())),
            PermissionPolicy::new(PermissionMode::DangerFullAccess),
            vec!["system".to_string()],
            RuntimeFeatureConfig::default().with_hooks(RuntimeHookConfig::new(
                vec![shell_snippet("printf 'pre hook ran'")],
                vec![shell_snippet("printf 'post hook ran'")],
            )),
        );

        let summary = runtime
            .run_turn("use add", None)
            .expect("tool loop succeeds");

        assert_eq!(summary.tool_results.len(), 1);
        let ContentBlock::ToolResult {
            is_error, output, ..
        } = &summary.tool_results[0].blocks[0]
        else {
            panic!("expected tool result block");
        };
        assert!(
            !*is_error,
            "post hook should preserve non-error result: {output:?}"
        );
        assert!(
            output.contains('4'),
            "tool output missing value: {output:?}"
        );
        assert!(
            output.contains("pre hook ran"),
            "tool output missing pre hook feedback: {output:?}"
        );
        assert!(
            output.contains("post hook ran"),
            "tool output missing post hook feedback: {output:?}"
        );
    }

    #[test]
    fn reconstructs_usage_tracker_from_restored_session() {
        struct SimpleApi;
        impl ApiClient for SimpleApi {
            fn stream(
                &mut self,
                _request: ApiRequest,
            ) -> Result<Vec<AssistantEvent>, RuntimeError> {
                Ok(vec![
                    AssistantEvent::TextDelta("done".to_string()),
                    AssistantEvent::MessageStop,
                ])
            }
        }

        let mut session = Session::new();
        session
            .messages
            .push(crate::session::ConversationMessage::assistant_with_usage(
                vec![ContentBlock::Text {
                    text: "earlier".to_string(),
                }],
                Some(TokenUsage {
                    input_tokens: 11,
                    output_tokens: 7,
                    cache_creation_input_tokens: 2,
                    cache_read_input_tokens: 1,
                }),
            ));

        let runtime = ConversationRuntime::new(
            session,
            SimpleApi,
            StaticToolExecutor::new(),
            PermissionPolicy::new(PermissionMode::DangerFullAccess),
            vec!["system".to_string()],
        );

        assert_eq!(runtime.usage().turns(), 1);
        assert_eq!(runtime.usage().cumulative_usage().total_tokens(), 21);
    }

    #[test]
    fn compacts_session_after_turns() {
        struct SimpleApi;
        impl ApiClient for SimpleApi {
            fn stream(
                &mut self,
                _request: ApiRequest,
            ) -> Result<Vec<AssistantEvent>, RuntimeError> {
                Ok(vec![
                    AssistantEvent::TextDelta("done".to_string()),
                    AssistantEvent::MessageStop,
                ])
            }
        }

        let mut runtime = ConversationRuntime::new(
            Session::new(),
            SimpleApi,
            StaticToolExecutor::new(),
            PermissionPolicy::new(PermissionMode::DangerFullAccess),
            vec!["system".to_string()],
        );
        runtime.run_turn("a", None).expect("turn a");
        runtime.run_turn("b", None).expect("turn b");
        runtime.run_turn("c", None).expect("turn c");

        let result = runtime.compact(CompactionConfig {
            preserve_recent_messages: 2,
            max_estimated_tokens: 1,
        });
        assert!(result.summary.contains("Conversation summary"));
        assert_eq!(
            result.compacted_session.messages[0].role,
            MessageRole::System
        );
    }

    /// Client factice : échoue d'abord par « contexte trop long » si demandé,
    /// puis répond ; résume avec un texte reconnaissable.
    struct CompactingApi {
        calls: usize,
        fail_first_with_overflow: bool,
        summaries: Vec<String>,
        notices: Vec<String>,
        message_counts: Vec<usize>,
    }

    impl ApiClient for CompactingApi {
        fn stream(&mut self, request: ApiRequest) -> Result<Vec<AssistantEvent>, RuntimeError> {
            self.calls += 1;
            self.message_counts.push(request.messages.len());
            if self.fail_first_with_overflow && self.calls == 1 {
                return Err(RuntimeError::new(
                    "l'api a retourné 400 : prompt is too long: 250000 tokens > 200000 maximum",
                ));
            }
            Ok(vec![
                AssistantEvent::TextDelta("ok".to_string()),
                AssistantEvent::MessageStop,
            ])
        }

        fn summarize(&mut self, system_prompt: &str, transcript: &str) -> Option<String> {
            assert!(system_prompt.contains("compacting"));
            self.summaries.push(transcript.to_string());
            Some("RÉSUMÉ-MODÈLE : l'utilisateur veut refactoriser le module api.".to_string())
        }

        fn notify(&mut self, notice: &str) {
            self.notices.push(notice.to_string());
        }
    }

    fn long_session(turns: usize) -> Session {
        let mut session = Session::new();
        for index in 0..turns {
            session
                .messages
                .push(crate::session::ConversationMessage::user_text(format!(
                    "demande {index} {}",
                    "x".repeat(400)
                )));
            session
                .messages
                .push(crate::session::ConversationMessage::assistant(vec![
                    ContentBlock::ToolUse {
                        id: format!("t{index}"),
                        name: "read_file".to_string(),
                        input: "{}".to_string(),
                    },
                ]));
            session
                .messages
                .push(crate::session::ConversationMessage::tool_result(
                    format!("t{index}"),
                    "read_file",
                    "y".repeat(400),
                    false,
                ));
        }
        session
    }

    fn compacting_runtime(
        session: Session,
        fail_first: bool,
    ) -> ConversationRuntime<CompactingApi, StaticToolExecutor> {
        ConversationRuntime::new(
            session,
            CompactingApi {
                calls: 0,
                fail_first_with_overflow: fail_first,
                summaries: Vec::new(),
                notices: Vec::new(),
                message_counts: Vec::new(),
            },
            StaticToolExecutor::new(),
            PermissionPolicy::new(PermissionMode::DangerFullAccess),
            vec!["system".to_string()],
        )
    }

    #[test]
    fn auto_compacts_with_a_model_summary_when_the_threshold_is_crossed() {
        let mut runtime = compacting_runtime(long_session(10), false).with_auto_compaction(Some(
            super::AutoCompactionConfig {
                threshold_tokens: 1_000,
                preserve_recent_messages: 4,
            },
        ));
        runtime
            .run_turn("continue", None)
            .expect("turn should succeed");

        let first = &runtime.session().messages[0];
        assert_eq!(first.role, MessageRole::System);
        assert!(
            matches!(&first.blocks[0], ContentBlock::Text { text } if text.contains("RÉSUMÉ-MODÈLE"))
        );
        // La partie conservée ne commence jamais par un résultat d'outil orphelin.
        assert_ne!(runtime.session().messages[1].role, MessageRole::Tool);
        assert!(runtime.api_client.summaries[0].contains("demande 0"));
        assert!(runtime.api_client.notices[0].starts_with("Contexte compacté automatiquement"));
        assert!(runtime.api_client.message_counts[0] < 31);
    }

    #[test]
    fn does_not_compact_below_the_threshold_or_when_disabled() {
        let mut runtime = compacting_runtime(long_session(2), false).with_auto_compaction(Some(
            super::AutoCompactionConfig {
                threshold_tokens: 1_000_000,
                preserve_recent_messages: 4,
            },
        ));
        runtime.run_turn("continue", None).expect("turn");
        assert!(runtime.api_client.summaries.is_empty());

        let mut disabled = compacting_runtime(long_session(10), false);
        disabled.run_turn("continue", None).expect("turn");
        assert!(disabled.api_client.summaries.is_empty());
    }

    #[test]
    fn retries_once_after_a_context_overflow_error() {
        let mut runtime = compacting_runtime(long_session(10), true).with_auto_compaction(Some(
            super::AutoCompactionConfig {
                threshold_tokens: usize::MAX,
                preserve_recent_messages: 4,
            },
        ));
        runtime
            .run_turn("continue", None)
            .expect("the retry after compaction should succeed");
        assert_eq!(runtime.api_client.calls, 2);
        assert_eq!(runtime.api_client.summaries.len(), 1);
        assert!(runtime.api_client.message_counts[1] < runtime.api_client.message_counts[0]);
    }

    #[cfg(windows)]
    fn shell_snippet(script: &str) -> String {
        script.replace('\'', "\"")
    }

    #[cfg(not(windows))]
    fn shell_snippet(script: &str) -> String {
        script.to_string()
    }
}
