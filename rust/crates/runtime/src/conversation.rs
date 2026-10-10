use std::collections::BTreeMap;
use std::fmt::{Display, Formatter};

use crate::compact::{
    compact_session, compact_session_with, estimate_message_tokens, estimate_session_tokens,
    render_transcript_for_summary, CompactionConfig, CompactionResult, MODEL_SUMMARY_PROMPT,
};
use crate::config::RuntimeFeatureConfig;
use crate::harness::{self, Aid, StallGuard, VerifySetting};
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

    /// Demande l'avis d'un autre modèle (relecture des modifications, conseil
    /// quand l'agent n'avance plus). `None` : aucun autre modèle disponible.
    fn consult(
        &mut self,
        _purpose: ConsultPurpose,
        _system: &str,
        _prompt: &str,
    ) -> Option<Consultation> {
        None
    }

    /// Confie la suite du tour à un modèle plus fort (`escalationModel`) ;
    /// renvoie son nom, ou `None` s'il n'y en a pas.
    fn escalate(&mut self) -> Option<String> {
        None
    }

    /// Fin du tour : le modèle habituel reprend après une escalade.
    fn restore_model(&mut self) {}

    /// Le modèle vient de répondre sans appeler d'outil : c'est sa réponse
    /// finale, sauf si un contrôle de Claw le renvoie au travail.
    fn answer_drafted(&mut self) {}

    /// Un contrôle de Claw (vérification, exigences, relecture, hook) renvoie
    /// le modèle au travail : la réponse qu'il venait de donner est provisoire,
    /// celle qui suivra la remplace.
    fn answer_superseded(&mut self) {}

    /// Quelqu'un peut répondre aux questions de l'agent (menu à choix de
    /// l'outil `AskUserQuestion`) : faux dans un script ou pour un sous-agent.
    fn can_ask_user(&self) -> bool {
        false
    }

    /// Mode équipe : rôle de la prochaine requête (le client choisit le modèle
    /// le plus fort pour ce rôle).
    fn plan_step(&mut self, _step: crate::team::Step) {}

    /// Mode équipe : modèle d'un sous-agent lancé par Claw dans ce rôle.
    fn team_model(&mut self, _role: crate::team::Role) -> Option<String> {
        None
    }
}

/// Pourquoi le runtime consulte un autre modèle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConsultPurpose {
    /// Relire les modifications du tour.
    Review,
    /// Débloquer un agent qui échoue plusieurs fois de suite.
    Unblock,
}

/// Réponse d'un modèle consulté.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Consultation {
    pub model: String,
    pub text: String,
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

    /// Annonce un appel lancé par Claw lui-même (vérification imposée), pour
    /// que l'interface l'affiche comme un appel du modèle.
    fn announce(&mut self, _tool_name: &str, _input: &str) {}

    /// L'appel n'a pas été exécuté (autorisation refusée, hook, interruption) :
    /// l'interface, qui l'a déjà affiché, peut le clore. `reason` est le
    /// résultat rendu au modèle.
    fn skipped(&mut self, _tool_name: &str, _input: &str, _reason: &str) {}

    /// Ce qui rend l'appel inexécutable (argument obligatoire absent, JSON
    /// invalide), dit au modèle. Rien n'est alors demandé à l'utilisateur :
    /// on n'autorise pas un appel vide.
    fn invalid_input(&mut self, _tool_name: &str, _input: &str) -> Option<String> {
        None
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
    verify: VerifySetting,
    review: bool,
    /// Langue des réponses réglée par l'utilisateur, rappelée dans les notes de Claw.
    response_language: Option<String>,
}

/// Aides au raisonnement, le temps d'un tour.
// Des états indépendants les uns des autres, pas une machine à états.
#[allow(clippy::struct_excessive_bools)]
#[derive(Default)]
struct TurnAids {
    /// Des fichiers ont été modifiés depuis la dernière vérification.
    dirty: bool,
    /// Commandes réussies lancées par le modèle depuis sa dernière modification.
    verified_commands: Vec<String>,
    /// Commandes parties en arrière-plan depuis la dernière modification, par
    /// identifiant de tâche : leur fin, lue avec `BashOutput`, comptera.
    background_commands: std::collections::BTreeMap<String, String>,
    verify_rounds: usize,
    /// Vérification refusée, impossible ou trop longue : on n'insiste pas.
    verify_disabled: bool,
    /// La dernière vérification a échoué (le travail n'est pas validé).
    verify_failing: bool,
    stall: StallGuard,
    /// Contenu des fichiers avant leur première modification du tour.
    originals: std::collections::BTreeMap<String, String>,
    reviewed: bool,
    /// Rappels déjà adressés au modèle pendant ce tour.
    reminders: u32,
    /// Un autre modèle a déjà été appelé à l'aide pendant ce tour.
    advised: bool,
    /// Une vérification (tests, compilation) a échoué pendant ce tour.
    check_failed: bool,
    /// Tests réécrits après un échec, déjà signalés.
    guarded_tests: std::collections::BTreeSet<String>,
    /// Un modèle plus fort a repris le tour.
    escalated: bool,
    /// Le modèle a déjà confronté son travail aux exigences pendant ce tour.
    checklisted: bool,
    /// Des fichiers ont été modifiés depuis la dernière question à l'éditeur.
    editor_dirty: bool,
    /// Renvois au modèle pour des erreurs signalées par l'éditeur.
    editor_rounds: usize,
    /// Appels d'outils en échec depuis le dernier qui a réussi.
    failure_streak: u32,
    /// Un appel a été refusé (autorisation, hook) : l'agent ne doit pas insister.
    refused: bool,
    /// L'agent a déjà été renvoyé chercher une autre voie pendant ce tour.
    persisted: bool,
    /// L'agent a posé ses questions avec le menu à choix pendant ce tour, ou
    /// Claw le lui a déjà demandé.
    asked: bool,
    /// Déroulé ultracode pour ce tour (session ou mot-clé du message).
    ultracode: bool,
    /// Dossier préparé par l'agent d'exploration (ultracode).
    brief: Option<String>,
    /// Des fichiers ont été modifiés depuis la dernière vérification indépendante.
    workflow_dirty: bool,
    /// Une vérification indépendante a déjà eu lieu pendant ce tour.
    workflow_checked: bool,
    workflow_runs: usize,
    /// Mode équipe pour ce tour, et sujet spécialisé de la demande.
    team: bool,
    topic: Option<&'static str>,
    /// La dernière commande de vérification (tests, compilation) a échoué.
    check_failing: bool,
    /// Une remarque d'un contrôle de Claw attend sa correction.
    feedback_pending: bool,
}

/// Mots en dessous desquels un message de l'utilisateur peut n'être qu'une
/// relance (« reprendre », « continue », « vas-y ») d'une demande antérieure.
const CONTINUATION_MAX_WORDS: usize = 8;
const MAX_EARLIER_REQUEST_CHARS: usize = 3_000;

/// Demande qu'un message court poursuit peut-être : le dernier message de
/// l'utilisateur assez long pour en être une (hors notes de Claw et hooks).
/// `None` si le message du tour est assez long pour se suffire.
fn earlier_request(messages: &[ConversationMessage], current: &str) -> Option<String> {
    if current.split_whitespace().count() >= CONTINUATION_MAX_WORDS {
        return None;
    }
    messages
        .iter()
        .rev()
        .filter(|message| message.role == crate::session::MessageRole::User)
        .find_map(|message| {
            let Some(ContentBlock::Text { text }) = message.blocks.first() else {
                return None;
            };
            let text = text.trim();
            let substantive = !text.starts_with(harness::CLAW_NOTE_PREFIX)
                && !text.starts_with(STOP_HOOK_PREFIX)
                && text.split_whitespace().count() >= CONTINUATION_MAX_WORDS;
            substantive.then(|| text.chars().take(MAX_EARLIER_REQUEST_CHARS).collect())
        })
}

/// Texte d'un message : ses blocs de texte, à la suite.
fn message_text(message: &ConversationMessage) -> String {
    message
        .blocks
        .iter()
        .filter_map(|block| match block {
            ContentBlock::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Sortie et statut d'un message de résultat d'outil.
fn tool_result_parts(message: &ConversationMessage) -> Option<(&str, bool)> {
    message.blocks.iter().find_map(|block| match block {
        ContentBlock::ToolResult {
            output, is_error, ..
        } => Some((output.as_str(), *is_error)),
        _ => None,
    })
}

/// Retire du résultat gardé dans la session ce que le modèle sait déjà (son
/// propre texte, le diff, l'état du bac à sable) : l'interface a reçu le
/// résultat complet à l'exécution.
fn lean_result(tool_name: &str, message: &mut ConversationMessage) {
    if !harness::enabled(Aid::LeanResults) {
        return;
    }
    for block in &mut message.blocks {
        if let ContentBlock::ToolResult {
            output,
            is_error: false,
            ..
        } = block
        {
            if let Some(lean) = crate::lean::lean_tool_output(tool_name, output) {
                *output = lean;
            }
        }
    }
}

/// Issue connue d'une commande du modèle.
struct CommandOutcome {
    command: String,
    passed: bool,
    /// Ce qu'elle a écrit (résultat de `bash`, ou texte lu par `BashOutput`).
    output: String,
}

/// Issue de la commande que ce résultat d'outil fait connaître, s'il y en a
/// une : celle de `bash` ou `PowerShell` au premier plan, ou la fin, lue avec
/// `BashOutput`, d'une commande passée en arrière-plan pendant ce tour. Une
/// commande qui part en arrière-plan (serveur, commande encore en cours) n'a
/// pas encore d'issue : elle est retenue, et ne vérifie rien d'ici là
/// (`npx tsc && npm run dev` n'a pas montré le résultat de tsc).
fn command_outcome(
    tool_name: &str,
    input: &str,
    output: &str,
    is_error: bool,
    aids: &mut TurnAids,
) -> Option<CommandOutcome> {
    if is_error {
        return None;
    }
    if matches!(tool_name, "bash" | "PowerShell") {
        let command = json_field(input, "command")?;
        let result = harness::read_bash_output(output);
        if result.passed && result.pending {
            // Seule une commande dont le shell tourne encore aura une issue ;
            // un processus laissé par `&` n'en a pas.
            if let Some(task) = json_field(output, "backgroundTaskId")
                .filter(|task| crate::bash_tasks::shell_running(task))
            {
                aids.background_commands.insert(task, command);
            }
            return None;
        }
        return Some(CommandOutcome {
            command,
            passed: result.passed,
            output: output.to_string(),
        });
    }
    if tool_name != "BashOutput" {
        return None;
    }
    let value = serde_json::Deserializer::from_str(output)
        .into_iter::<serde_json::Value>()
        .next()?
        .ok()?;
    let exited = value.get("status").and_then(serde_json::Value::as_str) == Some("exited");
    let killed = value.get("killed").and_then(serde_json::Value::as_bool) == Some(true);
    if !exited || killed {
        return None;
    }
    let task = value.get("bash_id").and_then(serde_json::Value::as_str)?;
    let command = aids.background_commands.remove(task)?;
    Some(CommandOutcome {
        command,
        passed: value.get("exitCode").and_then(serde_json::Value::as_i64) == Some(0),
        output: value
            .get("newOutput")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_string(),
    })
}

/// Ajoute une note de Claw à la suite d'un résultat d'outil (lue par le modèle).
fn append_tool_output(message: &mut ConversationMessage, note: &str) {
    for block in &mut message.blocks {
        if let ContentBlock::ToolResult { output, .. } = block {
            output.push_str("\n\n");
            output.push_str(note);
            return;
        }
    }
}

/// Champ texte du JSON qui ouvre `text` (un résultat d'outil peut être suivi
/// d'un message de hook).
fn json_field(text: &str, key: &str) -> Option<String> {
    serde_json::Deserializer::from_str(text)
        .into_iter::<serde_json::Value>()
        .next()?
        .ok()?
        .get(key)?
        .as_str()
        .map(ToOwned::to_owned)
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
const EMPTY_RESPONSE_NUDGE: &str = "[Claw] Your last reply was empty: no text and no tool call. A tool call written only in your reasoning is never executed. If the task is not finished, continue it now by calling the tools; otherwise give the user your final answer, in their language: what you did and what the verification showed.";

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

    // La configuration est donnée entière par l'appelant, qui n'en garde rien.
    #[allow(clippy::needless_pass_by_value)]
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
            verify: feature_config.verify().clone(),
            review: feature_config.review_enabled(),
            response_language: feature_config.response_language().map(ToOwned::to_owned),
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

    /// Vérification imposée après modification (réglage `"verify"`).
    #[must_use]
    pub fn with_verify(mut self, verify: VerifySetting) -> Self {
        self.verify = verify;
        self
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
        prompter: Option<&mut dyn PermissionPrompter>,
    ) -> Result<TurnSummary, RuntimeError> {
        let result = self.run_turn_inner(user_input.into(), images, prompter);
        // Après une escalade, le tour suivant repart avec le modèle habituel.
        self.api_client.restore_model();
        result
    }

    #[allow(clippy::too_many_lines)]
    fn run_turn_inner(
        &mut self,
        user_input: String,
        images: Vec<crate::attachments::ImageAttachment>,
        mut prompter: Option<&mut dyn PermissionPrompter>,
    ) -> Result<TurnSummary, RuntimeError> {
        clear_interrupt();
        let request_text = user_input.clone();
        // « reprendre » : la relecture doit connaître la demande poursuivie.
        let earlier = earlier_request(&self.session.messages, &request_text);
        let mut user_message = self.user_message_with_hooks(user_input)?;
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
        let team = !self.subagent && crate::team::team();
        let mut aids = TurnAids {
            ultracode: !self.subagent && crate::workflow::active_for(&request_text),
            team,
            topic: team.then(|| crate::team::topic_of(&request_text)).flatten(),
            ..TurnAids::default()
        };
        // Ultracode : un agent au contexte vierge étudie la demande avant le
        // travail ; à la fin, un vérificateur indépendant remplace le contrôle
        // des exigences que le modèle faisait sur son propre travail.
        aids.checklisted = aids.ultracode;
        if aids.ultracode {
            self.explore_stage(&request_text, earlier.as_deref(), &mut aids, &mut prompter);
        }

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
            // Mode équipe : le rôle de cette étape, d'après l'état du travail.
            if aids.team {
                let troubled = aids.verify_failing
                    || aids.check_failing
                    || aids.feedback_pending
                    || aids.failure_streak >= harness::PERSISTENCE_FAILURES;
                self.api_client.plan_step(crate::team::step_for(
                    aids.topic,
                    troubled,
                    !aids.originals.is_empty(),
                ));
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
                self.api_client.answer_drafted();
                // Le modèle dit avoir fini : Claw vérifie avant de le croire
                // (pas d'abandon sur un échec, vérification, erreurs vues par
                // l'éditeur relié, exigences, relecture par un autre modèle).
                // Un contrôle qui le renvoie au travail rend sa réponse provisoire.
                let answer = assistant_messages
                    .last()
                    .map(message_text)
                    .unwrap_or_default();
                // Questions écrites en fin de réponse : à poser avec le menu à choix.
                let mut asking = false;
                let feedback = self
                    .persistence_gate(&mut aids)
                    .or_else(|| self.verification_gate(&mut aids, &mut prompter))
                    .or_else(|| self.editor_gate(&mut aids))
                    .or_else(|| {
                        self.workflow_gate(
                            &mut aids,
                            &request_text,
                            earlier.as_deref(),
                            &answer,
                            &mut prompter,
                        )
                    })
                    .or_else(|| self.requirements_gate(&mut aids))
                    .or_else(|| {
                        self.review_gate(&mut aids, &request_text, earlier.as_deref(), &answer)
                    })
                    .or_else(|| {
                        asking = self.questions_gate(&mut aids, &answer);
                        asking.then(|| harness::QUESTIONS_FEEDBACK.to_string())
                    })
                    .or_else(|| {
                        let reason =
                            self.run_stop_hooks(stop_continuations, &assistant_messages)?;
                        stop_continuations += 1;
                        Some(format!("{STOP_HOOK_PREFIX} {reason}"))
                    });
                let Some(mut feedback) = feedback else {
                    break;
                };
                // La note de Claw est en anglais : la réponse, elle, reste dans la
                // langue de l'utilisateur. Le texte d'un hook est laissé tel quel.
                if !feedback.starts_with(STOP_HOOK_PREFIX) {
                    feedback.push_str(&harness::language_note(self.response_language.as_deref()));
                }
                // La réponse qui précède des questions reste la réponse : elle
                // n'est ni provisoire ni le signe d'un travail à reprendre.
                if !asking {
                    self.api_client.answer_superseded();
                    aids.feedback_pending = true;
                }
                self.session
                    .messages
                    .push(ConversationMessage::user_text(feedback));
                continue;
            }

            if self.run_parallel_batch(
                &pending_tool_uses,
                &mut prompter,
                &mut tool_results,
                &mut aids,
            ) {
                continue;
            }
            for (tool_use_id, tool_name, input) in pending_tool_uses {
                // Après une interruption, chaque appel restant reçoit un résultat :
                // un appel d'outil sans résultat rendrait la conversation invalide.
                if interrupt_requested() {
                    self.tool_executor
                        .skipped(&tool_name, &input, INTERRUPTED_MESSAGE);
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
                let (mut result_message, refused) = self.execute_tool_call(
                    tool_use_id,
                    tool_name.clone(),
                    &input,
                    prompter
                        .as_mut()
                        .map(|prompt| &mut **prompt as &mut dyn PermissionPrompter),
                );
                aids.refused |= refused;
                self.after_tool(&tool_name, &input, &mut result_message, &mut aids);
                lean_result(&tool_name, &mut result_message);
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

    /// Arguments, autorisation puis hook `PreToolUse` d'un appel d'un lot : les
    /// messages du hook s'il peut être exécuté, sinon la raison rendue au
    /// modèle, et vrai si c'est un refus (autorisation ou hook).
    fn admit_tool_call(
        &mut self,
        name: &str,
        input: &str,
        prompter: &mut Option<&mut dyn PermissionPrompter>,
    ) -> Result<Vec<String>, (String, bool)> {
        if let Some(problem) = self.tool_executor.invalid_input(name, input) {
            return Err((problem, false));
        }
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
        if let PermissionOutcome::Deny { reason } = outcome {
            return Err((reason, true));
        }
        let pre = self.hook_runner.run_pre_tool_use(name, input);
        if pre.is_denied() {
            let fallback = format!("hook PreToolUse : outil `{name}` refusé");
            return Err((format_hook_message(&pre, &fallback), true));
        }
        Ok(pre.messages().to_vec())
    }

    /// Plusieurs appels sans effet (sous-agents, lectures, recherches) : les
    /// autorisations et hooks restent séquentiels, l'exécution est simultanée.
    /// Renvoie faux si le lot ne s'y prête pas (traitement appel par appel).
    fn run_parallel_batch(
        &mut self,
        calls: &[(String, String, String)],
        prompter: &mut Option<&mut dyn PermissionPrompter>,
        tool_results: &mut Vec<ConversationMessage>,
        aids: &mut TurnAids,
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
            match self.admit_tool_call(name, input, prompter) {
                Ok(messages) => {
                    ready.push(index);
                    pre_messages.push(messages);
                }
                Err((reason, refused)) => {
                    aids.refused |= refused;
                    self.tool_executor.skipped(name, input, &reason);
                    results[index] = Some(ConversationMessage::tool_result(
                        id.clone(),
                        name.clone(),
                        reason,
                        true,
                    ));
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
        for (mut message, (_, name, input)) in results
            .into_iter()
            .zip(calls)
            .filter_map(|(message, call)| message.map(|message| (message, call)))
        {
            self.after_tool(name, input, &mut message, aids);
            lean_result(name, &mut message);
            self.session.messages.push(message.clone());
            tool_results.push(message);
        }
        true
    }

    /// Après un appel d'outil : contrôle de syntaxe du fichier écrit, suivi
    /// des modifications à vérifier, détection de blocage. Les remarques sont
    /// jointes au résultat que lira le modèle.
    fn after_tool(
        &mut self,
        tool_name: &str,
        input: &str,
        message: &mut ConversationMessage,
        aids: &mut TurnAids,
    ) {
        let Some((output, is_error)) = tool_result_parts(message) else {
            return;
        };
        let output = output.to_string();
        let mut notes = Vec::new();
        // Questions déjà posées avec le menu : rien à redemander en fin de tour.
        aids.asked |= tool_name == "AskUserQuestion";
        // Issue d'une commande du modèle : celle de `bash`, ou la fin, lue avec
        // `BashOutput`, d'une commande passée en arrière-plan pendant ce tour.
        let command_outcome = command_outcome(tool_name, input, &output, is_error, aids);
        // Échecs à la suite (outil en erreur, commande qui échoue) : un agent
        // qui s'arrête là est renvoyé chercher une autre voie.
        let command_failed = command_outcome
            .as_ref()
            .is_some_and(|outcome| !outcome.passed);
        aids.failure_streak = if is_error || command_failed {
            aids.failure_streak + 1
        } else {
            0
        };

        if matches!(tool_name, "edit_file" | "write_file" | "NotebookEdit") && !is_error {
            aids.dirty = true;
            aids.editor_dirty = true;
            aids.workflow_dirty = true;
            aids.feedback_pending = false;
            aids.verified_commands.clear();
            // Une commande encore en cours a été lancée sur l'ancien code.
            aids.background_commands.clear();
            let path = json_field(&output, "filePath").or_else(|| json_field(input, "path"));
            if let Some(path) = &path {
                if !aids.originals.contains_key(path) {
                    aids.originals.insert(
                        path.clone(),
                        harness::original_content(&output).unwrap_or_default(),
                    );
                }
            }
            if let Some(path) = path.as_deref().filter(|_| aids.check_failed) {
                if let Some(name) = self.rewritten_check(path, &output, aids) {
                    self.api_client.notify(&format!(
                        "Attention : {name} sert à vérifier le travail et a été modifié après un échec : relisez ce changement"
                    ));
                    notes.push(harness::test_guard_feedback(&name));
                }
            }
            if let Some(path) = path.filter(|_| harness::enabled(Aid::Diagnostics)) {
                if let Some(error) = harness::syntax_check(std::path::Path::new(&path)) {
                    let name = std::path::Path::new(&path)
                        .file_name()
                        .map_or_else(|| path.clone(), |name| name.to_string_lossy().to_string());
                    self.api_client.notify(&format!(
                        "Erreur de syntaxe dans {name} : signalée au modèle"
                    ));
                    notes.push(harness::syntax_feedback(&path, &error));
                }
            }
        }
        if let Some(outcome) = command_outcome {
            if outcome.passed {
                if harness::is_check_command(&outcome.command) {
                    aids.check_failing = false;
                }
                aids.verified_commands.push(outcome.command);
            } else {
                if harness::is_check_command(&outcome.command) {
                    aids.check_failed = true;
                    aids.check_failing = true;
                }
                // Le code des emplacements que l'erreur cite : une relecture de moins.
                if harness::enabled(Aid::ErrorSites) {
                    if let Ok(cwd) = std::env::current_dir() {
                        notes.extend(harness::error_sites(&outcome.output, &cwd));
                    }
                }
                // Sous Windows : un chemin `dossier\fichier` écrit pour cmd.
                if tool_name == "bash" && harness::enabled(Aid::PathHelp) {
                    let hint = crate::bash::windows_shell_hint(&outcome.command);
                    notes.extend(hint.map(str::to_string));
                }
            }
        }
        if harness::enabled(Aid::StallGuard) {
            if let Some(reminder) = aids.stall.observe(tool_name, input, &output, is_error) {
                self.api_client.notify(&reminder.notice);
                notes.push(reminder.to_model);
                aids.reminders += 1;
                // Deuxième rappel du tour : l'agent s'enlise, un autre modèle l'aide.
                if aids.reminders == 2 && is_error {
                    notes.extend(self.advice_when_stuck(aids, Some(&output)));
                    notes.extend(self.escalate_when_stuck(aids));
                }
            }
        }
        if !notes.is_empty() {
            append_tool_output(message, &notes.join("\n\n"));
        }
    }

    /// Nom du fichier si cette modification réécrit, après un échec, des lignes
    /// d'un fichier qui sert à vérifier le travail (test, ou script nommé par
    /// la commande de vérification). Signalé une fois par fichier.
    fn rewritten_check(&self, path: &str, output: &str, aids: &mut TurnAids) -> Option<String> {
        if !harness::enabled(Aid::TestGuard) || aids.guarded_tests.contains(path) {
            return None;
        }
        // Ajouter des tests est normal ; seules les lignes retirées comptent.
        if !harness::removes_existing_lines(output) {
            return None;
        }
        let cwd = std::env::current_dir().ok()?;
        let file = std::path::Path::new(path);
        let name = file.file_name()?.to_string_lossy().to_string();
        let checks_the_work = harness::is_test_file(file, &cwd)
            || harness::verify_commands(&self.verify, &cwd)
                .iter()
                .any(|command| command.contains(&name));
        if !checks_the_work {
            return None;
        }
        aids.guarded_tests.insert(path.to_string());
        Some(name)
    }

    /// Vérification imposée : quand le modèle a modifié des fichiers et dit
    /// avoir fini, Claw lance la commande de vérification du projet. En cas
    /// d'échec, la sortie est renvoyée au modèle et le tour continue (au plus
    /// `MAX_VERIFY_ROUNDS` corrections). Renvoie ce message, ou `None` si le
    /// tour peut se terminer.
    fn verification_gate(
        &mut self,
        aids: &mut TurnAids,
        prompter: &mut Option<&mut dyn PermissionPrompter>,
    ) -> Option<String> {
        if self.subagent
            || !aids.dirty
            || aids.verify_disabled
            || interrupt_requested()
            || !harness::enabled(Aid::Verify)
        {
            return None;
        }
        let cwd = std::env::current_dir().ok()?;
        let commands = harness::verify_commands(&self.verify, &cwd);
        aids.dirty = false;
        for command in commands {
            if aids
                .verified_commands
                .iter()
                .any(|ran| harness::covers(ran, &command))
            {
                continue;
            }
            self.api_client.notify(&format!(
                "Vérification automatique après modification : {command}"
            ));
            // Au premier plan et arrêtée au-delà du délai : son résultat
            // décide de la suite du tour.
            let input = serde_json::json!({
                "command": command,
                "timeout": harness::VERIFY_TIMEOUT_MS,
                "run_in_background": false,
                "description": "Vérification automatique de Claw",
            })
            .to_string();
            self.tool_executor.announce("bash", &input);
            let (message, _) = self.execute_tool_call(
                format!("claw-verify-{}", aids.verify_rounds),
                "bash".to_string(),
                &input,
                prompter
                    .as_mut()
                    .map(|prompt| &mut **prompt as &mut dyn PermissionPrompter),
            );
            let (output, is_error) = tool_result_parts(&message)?;
            if is_error {
                // Refusée par l'utilisateur ou impossible à lancer : on le dit,
                // pour ne pas laisser croire que le travail a été vérifié.
                aids.verify_disabled = true;
                self.api_client.notify(&format!(
                    "Vérification non lancée ({command} refusée ou impossible) : le travail n'est pas vérifié"
                ));
                return None;
            }
            let result = harness::read_bash_output(output);
            if result.timed_out {
                aids.verify_disabled = true;
                self.api_client.notify(&format!(
                    "Vérification trop longue ({command}) : choisissez une commande plus rapide avec \"verify\" dans .claw/settings.json"
                ));
                return None;
            }
            if !result.passed {
                aids.verify_rounds += 1;
                aids.verify_failing = true;
                aids.check_failed = true;
                if aids.verify_rounds > harness::MAX_VERIFY_ROUNDS {
                    self.api_client.notify(&format!(
                        "La vérification échoue encore après {} corrections ({command}) : le travail n'est pas validé",
                        harness::MAX_VERIFY_ROUNDS
                    ));
                    return None;
                }
                // Même sans nouvelle modification, la vérification sera relancée.
                aids.dirty = true;
                aids.verified_commands.clear();
                let mut feedback = harness::verify_feedback(&command, &result);
                if harness::enabled(Aid::ErrorSites) {
                    if let Some(sites) = harness::error_sites(output, &cwd) {
                        feedback.push_str("\n\n");
                        feedback.push_str(&sites);
                    }
                }
                // Deuxième échec : un autre modèle donne son diagnostic.
                if aids.verify_rounds == 2 {
                    if let Some(advice) = self.advice_when_stuck(aids, Some(&result.excerpt)) {
                        feedback.push_str("\n\n");
                        feedback.push_str(&advice);
                    }
                    // Et un modèle plus fort reprend la suite, s'il est réglé.
                    if let Some(takeover) = self.escalate_when_stuck(aids) {
                        feedback.push_str("\n\n");
                        feedback.push_str(&takeover);
                    }
                }
                return Some(feedback);
            }
        }
        if aids.verify_rounds > 0 {
            self.api_client
                .notify("Vérification réussie après correction");
        }
        aids.verify_failing = false;
        None
    }

    /// Fin de la session vue par un conseiller : la demande et les derniers
    /// échanges (appels d'outils et sorties), les plus récents en dernier.
    fn transcript_tail(&self, max_chars: usize) -> String {
        let mut parts = Vec::new();
        for message in self.session.messages.iter().rev().take(14) {
            for block in message.blocks.iter().rev() {
                let text = match block {
                    ContentBlock::Text { text } => {
                        let role = if message.role == crate::session::MessageRole::Assistant {
                            "agent"
                        } else {
                            "user"
                        };
                        format!("[{role}] {}", text.chars().take(1_500).collect::<String>())
                    }
                    ContentBlock::ToolUse { name, input, .. } => {
                        format!(
                            "[call {name}] {}",
                            input.chars().take(600).collect::<String>()
                        )
                    }
                    ContentBlock::ToolResult {
                        tool_name,
                        output,
                        is_error,
                        ..
                    } => format!(
                        "[{} of {tool_name}] {}",
                        if *is_error { "error" } else { "result" },
                        output.chars().take(1_500).collect::<String>()
                    ),
                    _ => continue,
                };
                parts.push(text);
            }
        }
        let mut kept = Vec::new();
        let mut used = 0;
        for part in parts {
            used += part.chars().count();
            if used > max_chars {
                break;
            }
            kept.push(part);
        }
        kept.reverse();
        kept.join("\n\n")
    }

    /// L'agent n'avance plus : un autre modèle lit la fin de la session et
    /// propose une cause et une prochaine étape (une fois par tour).
    fn advice_when_stuck(&mut self, aids: &mut TurnAids, latest: Option<&str>) -> Option<String> {
        if aids.advised || self.subagent || !harness::enabled(Aid::Council) {
            return None;
        }
        aids.advised = true;
        let mut tail = self.transcript_tail(8_000);
        if let Some(latest) = latest {
            tail.push_str("\n\n[latest failure]\n");
            tail.push_str(&latest.chars().take(2_000).collect::<String>());
        }
        self.api_client
            .notify("L'agent n'avance plus : Claw demande un diagnostic à un autre modèle");
        let consultation = self.api_client.consult(
            ConsultPurpose::Unblock,
            harness::UNBLOCK_SYSTEM,
            &harness::unblock_prompt(&tail),
        )?;
        if consultation.text.trim().is_empty() {
            return None;
        }
        self.api_client.notify(&format!(
            "Diagnostic de {} transmis à l'agent",
            consultation.model
        ));
        Some(harness::unblock_feedback(&consultation.text))
    }

    /// Escalade : un modèle plus fort (`escalationModel`) reprend la fin du tour
    /// quand l'agent n'avance plus. Une fois par tour.
    fn escalate_when_stuck(&mut self, aids: &mut TurnAids) -> Option<String> {
        if aids.escalated || self.subagent || !harness::enabled(Aid::Escalation) {
            return None;
        }
        aids.escalated = true;
        let model = self.api_client.escalate()?;
        self.api_client.notify(&format!(
            "L'agent n'avance plus : {model} reprend la fin du tour (escalationModel)"
        ));
        Some(harness::escalation_feedback(&model))
    }

    /// Diffs des fichiers modifiés pendant le tour (chemins relatifs au
    /// dossier de travail) et nombre de lignes changées.
    fn turn_diffs(aids: &TurnAids) -> (Vec<String>, usize) {
        let mut diffs = Vec::new();
        let mut changed_lines = 0;
        for (path, original) in &aids.originals {
            let Ok(current) = std::fs::read_to_string(path) else {
                continue;
            };
            let shown = std::env::current_dir()
                .ok()
                .and_then(|cwd| {
                    std::path::Path::new(path)
                        .strip_prefix(cwd)
                        .ok()
                        .map(|relative| relative.to_string_lossy().replace('\\', "/"))
                })
                .unwrap_or_else(|| path.clone());
            let (diff, changed) = harness::unified_diff(&shown, original, &current);
            if changed > 0 {
                changed_lines += changed;
                diffs.push(diff);
            }
        }
        (diffs, changed_lines)
    }

    /// Erreurs vues par l'éditeur : quand un éditeur est relié (VS Code) et que
    /// le tour a modifié des fichiers, Claw lui demande les erreurs qu'il y
    /// signale (types, compilation, lint) et les renvoie au modèle. Au plus
    /// `MAX_EDITOR_ROUNDS` renvois par tour ; sans éditeur, rien ne se passe.
    fn editor_gate(&mut self, aids: &mut TurnAids) -> Option<String> {
        if !aids.editor_dirty
            || aids.editor_rounds >= harness::MAX_EDITOR_ROUNDS
            || self.subagent
            || interrupt_requested()
            || !harness::enabled(Aid::EditorDiagnostics)
        {
            return None;
        }
        let editor = crate::editor::connected()?;
        aids.editor_dirty = false;
        let paths: Vec<String> = aids
            .originals
            .keys()
            .filter(|path| std::path::Path::new(path).is_file())
            .cloned()
            .collect();
        if paths.is_empty() {
            return None;
        }
        let files = crate::editor::check(&paths, harness::EDITOR_CHECK_TIMEOUT)?;
        let errors = crate::editor::at_least(files, crate::editor::Severity::Error);
        if errors.is_empty() {
            if aids.editor_rounds > 0 {
                self.api_client.notify(&format!(
                    "{editor} ne signale plus d'erreur dans les fichiers modifiés"
                ));
            }
            return None;
        }
        aids.editor_rounds += 1;
        self.api_client.notify(&format!(
            "{editor} signale {} erreur(s) dans les fichiers modifiés : transmises à l'agent",
            crate::editor::count(&errors)
        ));
        Some(harness::editor_feedback(&editor, &errors))
    }

    /// Contrôle des exigences : quand le tour a modifié assez de code et que
    /// la vérification passe, le modèle confronte son travail à chaque
    /// exigence de la demande et du code (cas limites compris) avant de
    /// conclure. Une fois par tour.
    fn requirements_gate(&mut self, aids: &mut TurnAids) -> Option<String> {
        if aids.checklisted
            || aids.verify_failing
            || aids.originals.is_empty()
            || self.subagent
            || interrupt_requested()
            || !harness::enabled(Aid::Checklist)
        {
            return None;
        }
        let (diffs, changed_lines) = Self::turn_diffs(aids);
        if diffs.is_empty()
            || (changed_lines < harness::REVIEW_MIN_CHANGED_LINES && diffs.len() < 2)
        {
            return None;
        }
        aids.checklisted = true;
        self.api_client
            .notify("Claw fait confronter le travail à chaque exigence avant de conclure");
        Some(harness::CHECKLIST_PROMPT.to_string())
    }

    /// Une étape du déroulé ultracode : un sous-agent lancé par Claw avec
    /// l'outil `Agent` (même affichage et mêmes autorisations qu'un appel du
    /// modèle). Renvoie son rapport, ou la raison de l'échec.
    fn run_workflow_agent(
        &mut self,
        id: String,
        subagent_type: &str,
        description: &str,
        prompt: &str,
        prompter: &mut Option<&mut dyn PermissionPrompter>,
    ) -> Result<String, String> {
        let role = if subagent_type == "Verification" {
            crate::team::Role::Review
        } else {
            crate::team::Role::Plan
        };
        let model = if crate::team::team() {
            self.api_client.team_model(role)
        } else {
            None
        };
        let input =
            crate::workflow::agent_input(subagent_type, description, prompt, model.as_deref());
        self.tool_executor.announce("Agent", &input);
        let (message, _) = self.execute_tool_call(
            id,
            "Agent".to_string(),
            &input,
            prompter
                .as_mut()
                .map(|prompt| &mut **prompt as &mut dyn PermissionPrompter),
        );
        let (output, is_error) =
            tool_result_parts(&message).ok_or_else(|| "aucun résultat".to_string())?;
        if is_error {
            return Err(output.chars().take(300).collect());
        }
        crate::workflow::agent_report(output).ok_or_else(|| "rapport vide".to_string())
    }

    /// Ultracode, avant le travail : un agent d'exploration, au contexte
    /// vierge et en lecture seule, étudie la demande et le code. Son dossier
    /// (exigences, code concerné, risques, plan vérifiable) est joint à la
    /// demande. Une relance (« reprendre ») garde le dossier déjà joint.
    fn explore_stage(
        &mut self,
        request: &str,
        earlier: Option<&str>,
        aids: &mut TurnAids,
        prompter: &mut Option<&mut dyn PermissionPrompter>,
    ) {
        if !crate::workflow::substantive(request) || interrupt_requested() {
            return;
        }
        self.api_client.notify(
            "Ultracode : un agent au contexte vierge étudie la demande et le code avant le travail",
        );
        let prompt = crate::workflow::explore_prompt(request, earlier);
        match self.run_workflow_agent(
            "claw-ultracode-explore".to_string(),
            "Explore",
            "Ultracode : comprendre la demande",
            &prompt,
            prompter,
        ) {
            Ok(report) => {
                if let Some(message) = self.session.messages.last_mut() {
                    message.blocks.push(ContentBlock::Text {
                        text: crate::workflow::brief_block(&report),
                    });
                }
                aids.brief = Some(report);
                self.api_client
                    .notify("Ultracode : dossier préparé, transmis à l'agent");
            }
            Err(reason) => self.api_client.notify(&format!(
                "Ultracode : l'étude préalable n'a pas abouti ({reason}) : le travail commence sans dossier"
            )),
        }
    }

    /// Ultracode, quand l'agent dit avoir fini : un vérificateur indépendant,
    /// au contexte vierge, qui n'a pas écrit le code, contrôle chaque exigence
    /// (lecture du code, tests, cas limites, affirmations de la réponse) et
    /// rend un verdict. Ses objections repartent à l'agent ; après une
    /// correction, il vérifie de nouveau (`MAX_VERIFY_RUNS` fois au plus).
    /// Sans modification de fichier, il contre-vérifie une fois la réponse.
    fn workflow_gate(
        &mut self,
        aids: &mut TurnAids,
        request: &str,
        earlier: Option<&str>,
        answer: &str,
        prompter: &mut Option<&mut dyn PermissionPrompter>,
    ) -> Option<String> {
        if !aids.ultracode
            || aids.verify_failing
            || self.subagent
            || interrupt_requested()
            || aids.workflow_runs >= crate::workflow::MAX_VERIFY_RUNS
        {
            return None;
        }
        let changed = !aids.originals.is_empty();
        let due = if changed {
            aids.workflow_dirty
        } else {
            !aids.workflow_checked
                && crate::workflow::substantive(request)
                && answer.chars().count() >= crate::workflow::FACT_CHECK_MIN_CHARS
        };
        if !due {
            return None;
        }
        aids.workflow_runs += 1;
        aids.workflow_dirty = false;
        aids.workflow_checked = true;
        let prompt = if changed {
            let (diffs, _) = Self::turn_diffs(aids);
            crate::workflow::verify_prompt(request, earlier, aids.brief.as_deref(), &diffs, answer)
        } else {
            crate::workflow::fact_check_prompt(request, earlier, aids.brief.as_deref(), answer)
        };
        self.api_client.notify(if changed {
            "Ultracode : un vérificateur indépendant contrôle chaque exigence et lance les tests"
        } else {
            "Ultracode : un vérificateur indépendant contre-vérifie la réponse dans le code"
        });
        let report = match self.run_workflow_agent(
            format!("claw-ultracode-verify-{}", aids.workflow_runs),
            "Verification",
            "Ultracode : vérifier le travail",
            &prompt,
            prompter,
        ) {
            Ok(report) => report,
            Err(reason) => {
                self.api_client.notify(&format!(
                    "Ultracode : la vérification indépendante n'a pas abouti ({reason}) : l'agent confronte lui-même son travail aux exigences"
                ));
                // Sans vérificateur, le contrôle des exigences par le modèle
                // reprend sa place (il suit dans la chaîne des contrôles).
                aids.checklisted = false;
                return None;
            }
        };
        match crate::workflow::verdict(&report) {
            crate::workflow::Verdict::Pass => {
                self.api_client
                    .notify("Ultracode : la vérification indépendante n'a rien trouvé à redire");
                None
            }
            crate::workflow::Verdict::Unclear => {
                self.api_client.notify(
                    "Ultracode : le vérificateur n'a pas rendu de verdict lisible : l'agent confronte lui-même son travail aux exigences",
                );
                aids.checklisted = false;
                None
            }
            crate::workflow::Verdict::Fail
                if aids.workflow_runs >= crate::workflow::MAX_VERIFY_RUNS =>
            {
                self.api_client.notify(
                    "Ultracode : le vérificateur signale encore des problèmes après plusieurs corrections : le travail n'est pas validé",
                );
                None
            }
            crate::workflow::Verdict::Fail => {
                self.api_client.notify(
                    "Ultracode : le vérificateur signale des problèmes, transmis à l'agent",
                );
                Some(crate::workflow::verify_feedback(&report, changed))
            }
        }
    }

    /// Questions à l'utilisateur : la réponse finale se termine par des
    /// questions écrites en texte alors que quelqu'un peut y répondre d'un
    /// clic. Le modèle est renvoyé une fois les poser avec `AskUserQuestion`,
    /// puis le tour continue avec les réponses, comme dans Claude Code.
    fn questions_gate(&mut self, aids: &mut TurnAids, answer: &str) -> bool {
        if aids.asked
            || self.subagent
            || interrupt_requested()
            || !harness::enabled(Aid::Questions)
            || !self.api_client.can_ask_user()
            || !harness::ends_with_questions(answer)
        {
            return false;
        }
        aids.asked = true;
        self.api_client.notify(
            "L'agent a écrit ses questions dans sa réponse : Claw lui demande de les poser avec le menu à choix",
        );
        true
    }

    /// Persévérance : le modèle veut s'arrêter juste après plusieurs échecs
    /// d'outils qui ne sont pas des refus ; il est renvoyé une fois chercher
    /// une autre voie au lieu de rendre le travail à l'utilisateur.
    fn persistence_gate(&mut self, aids: &mut TurnAids) -> Option<String> {
        if aids.persisted
            || aids.refused
            || aids.failure_streak < harness::PERSISTENCE_FAILURES
            || self.subagent
            || interrupt_requested()
            || !harness::enabled(Aid::Persistence)
        {
            return None;
        }
        aids.persisted = true;
        self.api_client.notify(
            "Les derniers essais ont échoué : Claw demande à l'agent de chercher une autre voie avant de conclure",
        );
        Some(harness::persistence_feedback(aids.failure_streak))
    }

    /// Relecture imposée : quand le tour a modifié assez de code et que la
    /// vérification passe, un autre modèle relit le diff et la réponse que
    /// l'agent s'apprête à donner. Ses remarques sont renvoyées au modèle
    /// (une relecture par tour). `None` : rien à corriger.
    fn review_gate(
        &mut self,
        aids: &mut TurnAids,
        request: &str,
        earlier: Option<&str>,
        answer: &str,
    ) -> Option<String> {
        if aids.reviewed
            || aids.verify_failing
            || aids.originals.is_empty()
            || self.subagent
            || !self.review
            || interrupt_requested()
            || !harness::enabled(Aid::Review)
        {
            return None;
        }
        aids.reviewed = true;
        let (diffs, changed_lines) = Self::turn_diffs(aids);
        if diffs.is_empty()
            || (changed_lines < harness::REVIEW_MIN_CHANGED_LINES && diffs.len() < 2)
        {
            return None;
        }
        self.api_client
            .notify("Relecture des modifications par un autre modèle…");
        let consultation = self.api_client.consult(
            ConsultPurpose::Review,
            harness::REVIEW_SYSTEM,
            &harness::review_prompt(request, earlier, answer, &diffs),
        )?;
        match harness::review_findings(&consultation.text) {
            None => {
                self.api_client.notify(&format!(
                    "Relecture par {} : rien à corriger",
                    consultation.model
                ));
                None
            }
            Some(findings) => {
                self.api_client.notify(&format!(
                    "Relecture par {} : {} remarque(s) transmise(s) à l'agent",
                    consultation.model,
                    findings.lines().count()
                ));
                // Les corrections seront vérifiées à leur tour.
                Some(harness::review_feedback(&findings))
            }
        }
    }

    /// Autorisation, hooks `PreToolUse`/`PostToolUse`, point de restauration
    /// et exécution d'un appel d'outil ; renvoie le message de résultat, et
    /// vrai si l'appel a été refusé (autorisation ou hook).
    fn execute_tool_call(
        &mut self,
        tool_use_id: String,
        tool_name: String,
        input: &str,
        prompter: Option<&mut dyn PermissionPrompter>,
    ) -> (ConversationMessage, bool) {
        if let Some(problem) = self.tool_executor.invalid_input(&tool_name, input) {
            self.tool_executor.skipped(&tool_name, input, &problem);
            let message = ConversationMessage::tool_result(tool_use_id, tool_name, problem, true);
            return (message, false);
        }
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
                    let reason = format_hook_message(&pre_hook_result, &deny_message);
                    self.tool_executor.skipped(&tool_name, input, &reason);
                    let message =
                        ConversationMessage::tool_result(tool_use_id, tool_name, reason, true);
                    (message, true)
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

                    let message =
                        ConversationMessage::tool_result(tool_use_id, tool_name, output, is_error);
                    (message, false)
                }
            }
            PermissionOutcome::Deny { reason } => {
                self.tool_executor.skipped(&tool_name, input, &reason);
                let message =
                    ConversationMessage::tool_result(tool_use_id, tool_name, reason, true);
                (message, true)
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
            .map(message_text)
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
            "hook {} : l'agent continue - {reason}",
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
    // Un raisonnement seul, sans texte ni appel d'outil, ne fait ni ne dit
    // rien : le tour se serait arrêté en silence.
    let says_or_does = blocks.iter().any(|block| match block {
        ContentBlock::Text { text } => !text.trim().is_empty(),
        ContentBlock::ToolUse { .. } => true,
        _ => false,
    });
    if !says_or_does {
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
                ..ProjectContext::default()
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
    fn a_refused_tool_call_is_reported_to_the_interface_as_skipped() {
        use std::sync::{Arc, Mutex};

        struct RejectPrompter;
        impl PermissionPrompter for RejectPrompter {
            fn decide(&mut self, _request: &PermissionRequest) -> PermissionPromptDecision {
                PermissionPromptDecision::Deny {
                    reason: "not now".to_string(),
                }
            }
        }

        struct OneCall;
        impl ApiClient for OneCall {
            fn stream(&mut self, request: ApiRequest) -> Result<Vec<AssistantEvent>, RuntimeError> {
                let answered = request
                    .messages
                    .iter()
                    .any(|message| message.role == MessageRole::Tool);
                Ok(if answered {
                    vec![
                        AssistantEvent::TextDelta("Entendu.".to_string()),
                        AssistantEvent::MessageStop,
                    ]
                } else {
                    vec![
                        AssistantEvent::ToolUse {
                            id: "tool-1".to_string(),
                            name: "blocked".to_string(),
                            input: "secret".to_string(),
                        },
                        AssistantEvent::MessageStop,
                    ]
                })
            }
        }

        /// Ce que l'interface apprend de chaque appel.
        struct Recorder(Arc<Mutex<Vec<String>>>);
        impl super::ToolExecutor for Recorder {
            fn execute(
                &mut self,
                tool_name: &str,
                _input: &str,
            ) -> Result<String, super::ToolError> {
                self.0
                    .lock()
                    .expect("journal")
                    .push(format!("exécuté {tool_name}"));
                Ok(String::new())
            }

            fn skipped(&mut self, tool_name: &str, input: &str, reason: &str) {
                self.0
                    .lock()
                    .expect("journal")
                    .push(format!("écarté {tool_name} ({input}) : {reason}"));
            }
        }

        let seen = Arc::new(Mutex::new(Vec::new()));
        let mut runtime = ConversationRuntime::new(
            Session::new(),
            OneCall,
            Recorder(Arc::clone(&seen)),
            PermissionPolicy::new(PermissionMode::WorkspaceWrite),
            vec!["system".to_string()],
        );
        runtime
            .run_turn("use the tool", Some(&mut RejectPrompter))
            .expect("le tour continue après un refus");

        // Sans ce signal, la page afficherait l'appel « en cours » pour toujours.
        assert_eq!(
            *seen.lock().expect("journal"),
            ["écarté blocked (secret) : not now"]
        );
    }

    #[test]
    fn a_call_with_unusable_arguments_fails_without_asking_for_permission() {
        struct NeverAsked;
        impl PermissionPrompter for NeverAsked {
            fn decide(&mut self, request: &PermissionRequest) -> PermissionPromptDecision {
                panic!(
                    "aucune autorisation ne se demande pour un appel vide : {}",
                    request.tool_name
                );
            }
        }

        struct EmptyCall;
        impl ApiClient for EmptyCall {
            fn stream(&mut self, request: ApiRequest) -> Result<Vec<AssistantEvent>, RuntimeError> {
                let answered = request
                    .messages
                    .iter()
                    .any(|message| message.role == MessageRole::Tool);
                Ok(if answered {
                    vec![
                        AssistantEvent::TextDelta("Entendu.".to_string()),
                        AssistantEvent::MessageStop,
                    ]
                } else {
                    vec![
                        AssistantEvent::ToolUse {
                            id: "tool-1".to_string(),
                            name: "edit_file".to_string(),
                            input: "{}".to_string(),
                        },
                        AssistantEvent::MessageStop,
                    ]
                })
            }
        }

        /// Outil dont les arguments vides sont refusés avant toute exécution.
        struct Strict(Vec<String>);
        impl super::ToolExecutor for Strict {
            fn execute(
                &mut self,
                tool_name: &str,
                _input: &str,
            ) -> Result<String, super::ToolError> {
                self.0.push(format!("exécuté {tool_name}"));
                Ok(String::new())
            }

            fn skipped(&mut self, tool_name: &str, _input: &str, reason: &str) {
                self.0.push(format!("écarté {tool_name} : {reason}"));
            }

            fn invalid_input(&mut self, _tool_name: &str, input: &str) -> Option<String> {
                (input == "{}").then(|| "arguments absents".to_string())
            }
        }

        let mut runtime = ConversationRuntime::new(
            Session::new(),
            EmptyCall,
            Strict(Vec::new()),
            PermissionPolicy::new(PermissionMode::Prompt)
                .with_tool_requirement("edit_file", PermissionMode::WorkspaceWrite),
            vec!["system".to_string()],
        );
        let summary = runtime
            .run_turn("modifie", Some(&mut NeverAsked))
            .expect("le tour continue après un appel vide");

        assert!(matches!(
            &summary.tool_results[0].blocks[0],
            ContentBlock::ToolResult { is_error: true, output, .. } if output.starts_with("arguments absents")
        ));
        assert_eq!(
            runtime.tool_executor.0,
            ["écarté edit_file : arguments absents"]
        );
    }

    // Le second faux modèle est déclaré là où le scénario s'en sert.
    #[allow(clippy::items_after_statements)]
    #[test]
    fn an_empty_reply_is_followed_up_once_instead_of_failing() {
        struct EmptyThenText {
            calls: usize,
        }
        impl ApiClient for EmptyThenText {
            fn stream(&mut self, request: ApiRequest) -> Result<Vec<AssistantEvent>, RuntimeError> {
                self.calls += 1;
                if self.calls == 1 {
                    // Petit modèle : uniquement du raisonnement, rien d'affichable
                    // (ni texte, ni appel d'outil qui serait exécuté).
                    return Ok(vec![
                        AssistantEvent::Thinking {
                            thinking: "<invoke name=\"edit_file\">…</invoke>".to_string(),
                            signature: String::new(),
                            redacted: None,
                        },
                        AssistantEvent::TextDelta("\n  ".to_string()),
                        AssistantEvent::MessageStop,
                    ]);
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

    // --- Aides au raisonnement ------------------------------------------------

    #[derive(Default)]
    struct Observed {
        /// Messages reçus à chaque appel du modèle (texte de débogage).
        requests: Vec<String>,
        notices: Vec<String>,
        announced: Vec<String>,
        /// Demandes faites à un autre modèle (relecture, conseil).
        consults: Vec<(super::ConsultPurpose, String)>,
        /// Escalades demandées, et retours au modèle habituel.
        escalations: usize,
        restored: usize,
        /// Réponses données sans appel d'outil, et celles qu'un contrôle a
        /// rendues provisoires.
        drafted: usize,
        superseded: usize,
        /// Rôles annoncés au client en mode équipe, dans l'ordre.
        team_steps: Vec<String>,
    }

    /// Modèle scripté : une réponse par appel.
    struct StepsApiClient {
        steps: std::collections::VecDeque<Vec<AssistantEvent>>,
        observed: std::rc::Rc<std::cell::RefCell<Observed>>,
        /// Réponses de l'autre modèle consulté, dans l'ordre.
        advisor: std::collections::VecDeque<&'static str>,
        /// Modèle plus fort disponible pour une escalade.
        escalation: Option<&'static str>,
    }

    impl ApiClient for StepsApiClient {
        fn stream(&mut self, request: ApiRequest) -> Result<Vec<AssistantEvent>, RuntimeError> {
            self.observed
                .borrow_mut()
                .requests
                .push(format!("{:?}", request.messages));
            self.steps
                .pop_front()
                .ok_or_else(|| RuntimeError::new("plus de réponse scriptée"))
        }

        fn notify(&mut self, notice: &str) {
            self.observed.borrow_mut().notices.push(notice.to_string());
        }

        fn consult(
            &mut self,
            purpose: super::ConsultPurpose,
            _system: &str,
            prompt: &str,
        ) -> Option<super::Consultation> {
            self.observed
                .borrow_mut()
                .consults
                .push((purpose, prompt.to_string()));
            self.advisor.pop_front().map(|text| super::Consultation {
                model: "relecteur".to_string(),
                text: text.to_string(),
            })
        }

        fn escalate(&mut self) -> Option<String> {
            self.observed.borrow_mut().escalations += 1;
            self.escalation.map(ToOwned::to_owned)
        }

        fn restore_model(&mut self) {
            self.observed.borrow_mut().restored += 1;
        }

        fn answer_drafted(&mut self) {
            self.observed.borrow_mut().drafted += 1;
        }

        fn answer_superseded(&mut self) {
            self.observed.borrow_mut().superseded += 1;
        }

        fn can_ask_user(&self) -> bool {
            CAN_ASK.with(std::cell::Cell::get)
        }

        fn plan_step(&mut self, step: crate::team::Step) {
            self.observed
                .borrow_mut()
                .team_steps
                .push(step.role.key().to_string());
        }
    }

    fn tool_step(name: &str, input: &str) -> Vec<AssistantEvent> {
        vec![
            AssistantEvent::ToolUse {
                id: format!("appel-{name}"),
                name: name.to_string(),
                input: input.to_string(),
            },
            AssistantEvent::MessageStop,
        ]
    }

    fn text_step(text: &str) -> Vec<AssistantEvent> {
        vec![
            AssistantEvent::TextDelta(text.to_string()),
            AssistantEvent::MessageStop,
        ]
    }

    const WRITTEN: &str = r#"{"filePath":"notes.txt","content":"x"}"#;
    const PASSING: &str = r#"{"stdout":"ok","stderr":""}"#;
    const FAILING: &str =
        r#"{"stdout":"","stderr":"assertion failed","returnCodeInterpretation":"exit_code:1"}"#;

    /// Runtime d'essai, ce que le modèle a vu, et les commandes lancées.
    type Aided = (
        ConversationRuntime<StepsApiClient, StaticToolExecutor>,
        std::rc::Rc<std::cell::RefCell<Observed>>,
        std::rc::Rc<std::cell::RefCell<Vec<String>>>,
    );

    /// Runtime dont l'outil `bash` renvoie les sorties données, dans l'ordre.
    fn aided_runtime(steps: Vec<Vec<AssistantEvent>>, bash_outputs: Vec<&'static str>) -> Aided {
        aided_runtime_with(steps, bash_outputs, Vec::new(), WRITTEN.to_string())
    }

    /// Variante : réponses de l'autre modèle consulté et sortie de `write_file`.
    fn aided_runtime_with(
        steps: Vec<Vec<AssistantEvent>>,
        bash_outputs: Vec<&'static str>,
        advisor: Vec<&'static str>,
        written: String,
    ) -> Aided {
        let observed = std::rc::Rc::new(std::cell::RefCell::new(Observed::default()));
        let bash_calls = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let recorded = std::rc::Rc::clone(&bash_calls);
        let mut outputs = std::collections::VecDeque::from(bash_outputs);
        let executor = StaticToolExecutor::new()
            .register("write_file", move |_| Ok(written.clone()))
            .register("probe", |_| Err(super::ToolError::new("introuvable")))
            .register("BashOutput", |_| {
                Ok(TASK_REPLY.with(|reply| reply.borrow().clone()))
            })
            .register("AskUserQuestion", |_| {
                Ok(r#"{"answers":[{"question":"Continuer ?","answer":"Oui"}]}"#.to_string())
            })
            .register("bash", move |input| {
                recorded.borrow_mut().push(input.to_string());
                Ok(outputs.pop_front().unwrap_or(PASSING).to_string())
            });
        let runtime = ConversationRuntime::new(
            Session::new(),
            StepsApiClient {
                steps: steps.into(),
                observed: std::rc::Rc::clone(&observed),
                advisor: advisor.into(),
                escalation: None,
            },
            executor,
            PermissionPolicy::new(PermissionMode::DangerFullAccess),
            vec!["système".to_string()],
        )
        .with_verify(crate::harness::VerifySetting::Commands(vec![
            "verifier".to_string()
        ]));
        (runtime, observed, bash_calls)
    }

    #[test]
    fn a_failed_verification_sends_the_model_back_to_work() {
        let (mut runtime, observed, bash_calls) = aided_runtime(
            vec![
                tool_step("write_file", r#"{"path":"notes.txt","content":"x"}"#),
                text_step("C'est fait."),
                text_step("Corrigé."),
            ],
            vec![FAILING, PASSING],
        );
        let summary = runtime.run_turn("écris le fichier", None).expect("tour");

        // Deux vérifications : la première échoue, la seconde suit la correction.
        assert_eq!(bash_calls.borrow().len(), 2);
        assert!(bash_calls.borrow()[0].contains(r#""command":"verifier""#));
        assert_eq!(summary.iterations, 3);
        let observed = observed.borrow();
        assert!(observed.requests[2].contains("Automatic verification after your changes failed"));
        assert!(observed.requests[2].contains("assertion failed"));
        assert!(observed
            .notices
            .iter()
            .any(|notice| notice == "Vérification automatique après modification : verifier"));
        assert!(observed
            .notices
            .iter()
            .any(|notice| notice == "Vérification réussie après correction"));
    }

    thread_local! {
        /// Quelqu'un peut répondre aux questions de l'agent.
        static CAN_ASK: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    }

    const ASKING: &str = "Voici l'architecture proposée.\n\nCette architecture te convient-elle ?\nVeux-tu que j'ajoute des tests ?";

    #[test]
    fn questions_written_in_the_answer_are_asked_again_with_the_menu() {
        // Quelqu'un peut répondre : le modèle est renvoyé poser ses questions
        // avec l'outil, puis continue avec les réponses.
        CAN_ASK.with(|flag| flag.set(true));
        let (mut runtime, observed, _) = aided_runtime(
            vec![
                text_step(ASKING),
                tool_step(
                    "AskUserQuestion",
                    r#"{"questions":[{"question":"Continuer ?","options":[{"label":"Oui"},{"label":"Non"}]}]}"#,
                ),
                text_step("Très bien, je commence."),
            ],
            vec![],
        );
        let summary = runtime
            .run_turn("propose puis attends", None)
            .expect("tour");
        assert_eq!(summary.iterations, 3);
        {
            let observed = observed.borrow();
            assert!(observed.requests[1].contains("Call the AskUserQuestion tool now"));
            // La proposition reste la réponse : elle n'est pas repliée.
            assert_eq!(observed.superseded, 0);
            assert!(observed
                .notices
                .iter()
                .any(|notice| notice.contains("menu à choix")));
        }

        // Une seule relance par tour, même si le modèle récrit ses questions.
        let (mut runtime, _, _) = aided_runtime(vec![text_step(ASKING), text_step(ASKING)], vec![]);
        let summary = runtime
            .run_turn("propose puis attends", None)
            .expect("tour");
        assert_eq!(summary.iterations, 2);

        // Personne pour répondre (script, sous-agent) : rien n'est demandé.
        CAN_ASK.with(|flag| flag.set(false));
        let (mut runtime, observed, _) = aided_runtime(vec![text_step(ASKING)], vec![]);
        let summary = runtime
            .run_turn("propose puis attends", None)
            .expect("tour");
        assert_eq!(summary.iterations, 1);
        assert!(observed.borrow().notices.is_empty());
    }

    thread_local! {
        /// Réponse de `BashOutput` dans `aided_runtime`.
        static TASK_REPLY: std::cell::RefCell<String> = const { std::cell::RefCell::new(String::new()) };
    }

    #[test]
    fn a_command_sent_to_the_background_verifies_nothing_until_its_end_is_read() {
        // `verifier && npm run dev` part en arrière-plan : le résultat de la
        // vérification n'a pas été montré, Claw la lance donc lui-même.
        let (mut runtime, _, bash_calls) = aided_runtime(
            vec![
                tool_step("write_file", r#"{"path":"notes.txt","content":"x"}"#),
                tool_step("bash", r#"{"command":"verifier && npm run dev"}"#),
                text_step("C'est fait."),
            ],
            vec![
                r#"{"stdout":"[Claw] This command starts a server","stderr":"","backgroundTaskId":"bash_inconnue"}"#,
                PASSING,
            ],
        );
        runtime.run_turn("écris le fichier", None).expect("tour");
        {
            let calls = bash_calls.borrow();
            assert_eq!(calls.len(), 2, "{calls:?}");
            assert!(calls[1].contains(r#""command":"verifier""#), "{calls:?}");
            assert!(
                calls[1].contains(r#""run_in_background":false"#),
                "{calls:?}"
            );
        }

        // La vérification, trop longue, continue en arrière-plan ; le modèle
        // lit sa fin avec BashOutput : elle compte, Claw ne la relance pas.
        let task =
            crate::bash_tasks::spawn(plugins::posix_shell_command("sleep 20"), "verifier", None)
                .expect("tâche de fond");
        let handed_off: &'static str = Box::leak(
            format!(
                r#"{{"stdout":"[Claw] Still running after 300 s","stderr":"","backgroundTaskId":"{task}"}}"#
            )
            .into_boxed_str(),
        );
        TASK_REPLY.with(|reply| {
            *reply.borrow_mut() = format!(
                r#"{{"bash_id":"{task}","status":"exited","exitCode":0,"newOutput":"ok","killed":false}}"#
            );
        });
        let (mut runtime, _, bash_calls) = aided_runtime(
            vec![
                tool_step("write_file", r#"{"path":"notes.txt","content":"x"}"#),
                tool_step("bash", r#"{"command":"verifier"}"#),
                tool_step("BashOutput", r#"{"bash_id":"x","block":true}"#),
                text_step("C'est fait et vérifié."),
            ],
            vec![handed_off],
        );
        runtime.run_turn("écris le fichier", None).expect("tour");
        assert_eq!(bash_calls.borrow().len(), 1, "{:?}", bash_calls.borrow());

        // Elle se termine en échec : c'est un échec de vérification, comme au
        // premier plan, et Claw relance la sienne après la correction.
        TASK_REPLY.with(|reply| {
            *reply.borrow_mut() = format!(
                r#"{{"bash_id":"{task}","status":"exited","exitCode":1,"newOutput":"assertion failed","killed":false}}"#
            );
        });
        let (mut runtime, _, bash_calls) = aided_runtime(
            vec![
                tool_step("write_file", r#"{"path":"notes.txt","content":"x"}"#),
                tool_step("bash", r#"{"command":"verifier"}"#),
                tool_step("BashOutput", r#"{"bash_id":"x","block":true}"#),
                text_step("C'est fait."),
            ],
            vec![handed_off, PASSING],
        );
        runtime.run_turn("écris le fichier", None).expect("tour");
        assert_eq!(bash_calls.borrow().len(), 2, "{:?}", bash_calls.borrow());
        crate::bash_tasks::kill(&task).expect("arrêt");
    }

    #[test]
    fn no_verification_without_changes_or_when_the_model_already_ran_it() {
        // Aucun fichier modifié : rien à vérifier.
        let (mut runtime, _, bash_calls) = aided_runtime(vec![text_step("Bonjour.")], vec![]);
        runtime.run_turn("salut", None).expect("tour");
        assert!(bash_calls.borrow().is_empty());

        // Le modèle a lancé lui-même la vérification après sa modification.
        let (mut runtime, observed, bash_calls) = aided_runtime(
            vec![
                tool_step("write_file", r#"{"path":"notes.txt","content":"x"}"#),
                tool_step("bash", r#"{"command":"verifier"}"#),
                text_step("Fait et vérifié."),
            ],
            vec![PASSING],
        );
        runtime.run_turn("écris le fichier", None).expect("tour");
        assert_eq!(bash_calls.borrow().len(), 1);
        assert!(observed.borrow().notices.is_empty());
    }

    #[test]
    fn verification_gives_up_after_a_few_corrections_and_says_so() {
        let (mut runtime, observed, bash_calls) = aided_runtime(
            vec![
                tool_step("write_file", r#"{"path":"notes.txt","content":"x"}"#),
                text_step("Fait."),
                text_step("Essai 1."),
                text_step("Essai 2."),
                text_step("Essai 3."),
            ],
            vec![FAILING, FAILING, FAILING, FAILING],
        );
        runtime.run_turn("écris le fichier", None).expect("tour");
        assert_eq!(bash_calls.borrow().len(), 4);
        assert!(observed
            .borrow()
            .notices
            .iter()
            .any(|notice| notice.contains("le travail n'est pas validé")));
    }

    #[test]
    fn a_model_going_in_circles_gets_one_reminder() {
        let (mut runtime, observed, _) = aided_runtime(
            vec![
                tool_step("probe", "{}"),
                tool_step("probe", "{}"),
                tool_step("probe", "{}"),
                text_step("Je change d'approche."),
                text_step("La sonde n'existe pas ici : rien d'autre à essayer."),
            ],
            vec![],
        );
        runtime.run_turn("cherche", None).expect("tour");
        let observed = observed.borrow();
        // Le rappel accompagne le troisième résultat, pas les deux premiers.
        assert!(!observed.requests[2].contains("exact `probe` call"));
        assert!(observed.requests[3].contains("exact `probe` call 3 times"));
        assert!(observed
            .notices
            .iter()
            .any(|notice| notice.contains("3 appels identiques à probe")));
        assert!(observed.announced.is_empty());
        // S'arrêter sur ces échecs sans rien changer : renvoyé chercher une autre voie.
        assert!(observed.requests[4].contains("Your last 3 tool calls failed"));
    }

    const COMMAND_FAILED: &str = r#"{"stdout":"","stderr":"npm: command not found","returnCodeInterpretation":"exit_code:127"}"#;

    #[test]
    fn an_agent_stopping_on_failures_is_sent_back_once_to_try_another_way() {
        let (mut runtime, observed, bash_calls) = aided_runtime(
            vec![
                tool_step("bash", r#"{"command":"npm run dev"}"#),
                tool_step("bash", r#"{"command":"npm start"}"#),
                text_step("Lancez la commande dans votre propre terminal."),
                tool_step("bash", r#"{"command":"ls node_modules/.bin"}"#),
                tool_step("probe", "{}"),
                text_step("npm est absent de cette machine : installez Node.js."),
            ],
            vec![COMMAND_FAILED, COMMAND_FAILED, COMMAND_FAILED],
        );
        let summary = runtime.run_turn("lance le serveur", None).expect("tour");
        assert_eq!(bash_calls.borrow().len(), 3);
        assert_eq!(summary.iterations, 6);
        let observed = observed.borrow();
        // Deux commandes en échec puis l'agent rend la main : il est renvoyé.
        let note = &observed.requests[3];
        assert!(note.contains("[Claw] Your last 2 tool calls failed and you are about to stop"));
        assert!(note.contains("Do not hand the task back to the user"));
        assert!(note.contains("end with your final answer"));
        assert!(observed
            .notices
            .iter()
            .any(|notice| notice.starts_with("Les derniers essais ont échoué")));
        // Une fois par tour : le second arrêt sur des échecs termine le tour
        // (chaque requête reprend toute la conversation).
        let last = observed.requests.last().expect("requêtes");
        assert_eq!(
            last.matches("tool calls failed and you are about to stop")
                .count(),
            1
        );
        assert_eq!((observed.drafted, observed.superseded), (2, 1));
    }

    #[test]
    fn a_success_or_a_refusal_ends_the_failure_streak() {
        // Un échec puis une commande qui réussit : rien à redire.
        let (mut runtime, observed, _) = aided_runtime(
            vec![
                tool_step("probe", "{}"),
                tool_step("bash", r#"{"command":"ls"}"#),
                text_step("Voici le contenu du dossier."),
            ],
            vec![PASSING],
        );
        let summary = runtime.run_turn("liste", None).expect("tour");
        assert_eq!(summary.iterations, 3);
        assert!(!observed
            .borrow()
            .requests
            .iter()
            .any(|request| request.contains("tool calls failed")));

        // L'utilisateur a refusé un appel : l'agent ne doit pas insister.
        let (mut runtime, observed, _) = aided_runtime(
            vec![
                tool_step("write_file", r#"{"path":"notes.txt","content":"x"}"#),
                tool_step("probe", "{}"),
                text_step("Je n'ai pas le droit d'écrire ce fichier."),
            ],
            vec![],
        );
        runtime.permission_policy = PermissionPolicy::new(PermissionMode::ReadOnly)
            .with_tool_requirement("write_file", PermissionMode::WorkspaceWrite);
        let summary = runtime.run_turn("écris le fichier", None).expect("tour");
        assert_eq!(summary.iterations, 3);
        assert!(!observed
            .borrow()
            .requests
            .iter()
            .any(|request| request.contains("tool calls failed")));
    }

    #[test]
    fn a_short_follow_up_points_the_review_to_the_request_it_continues() {
        use super::{earlier_request, ConversationMessage};
        let request = "Ajoute une page frontend pour traduire une image depuis une URL";
        let messages = vec![
            ConversationMessage::user_text(request),
            ConversationMessage::assistant(vec![ContentBlock::Text {
                text: "Je commence.".to_string(),
            }]),
            ConversationMessage::user_text(
                "[Claw] Your last reply was empty: no text and no tool call, continue the task now.",
            ),
        ];
        // « reprendre » poursuit la demande précédente, pas la note de Claw.
        assert_eq!(
            earlier_request(&messages, "reprendre").as_deref(),
            Some(request)
        );
        // Une vraie demande se suffit.
        assert!(earlier_request(
            &messages,
            "Ajoute aussi un bouton pour effacer l'historique des traductions"
        )
        .is_none());
        assert!(earlier_request(&[], "continue").is_none());
    }

    #[test]
    fn the_review_sees_the_answer_and_the_continued_request() {
        let (path, output) = written_file("reprise");
        let (mut runtime, observed, _) = aided_runtime_with(
            vec![
                text_step("Je m'y mets."),
                tool_step("write_file", r#"{"path":"total.py","content":"x"}"#),
                text_step("Fait : l'onglet Depuis une URL est en place."),
                text_step("Chaque exigence est couverte."),
            ],
            vec![PASSING],
            vec!["LGTM"],
            output,
        );
        runtime
            .run_turn(
                "Ajoute une page frontend pour traduire une image depuis une URL",
                None,
            )
            .expect("premier tour");
        runtime.run_turn("reprendre", None).expect("tour");
        let observed = observed.borrow();
        assert_eq!(observed.consults.len(), 1);
        let prompt = &observed.consults[0].1;
        assert!(prompt.contains("The user asked:\nreprendre"));
        assert!(prompt.contains("which the message above may continue:\nAjoute une page frontend"));
        assert!(
            prompt.contains("The agent's final answer to the user:\nChaque exigence est couverte.")
        );
        assert!(prompt.contains("+def total(prix):"));
        let _ = std::fs::remove_file(path);
    }

    /// Runtime d'essai, ce que le modèle a vu, et les arguments de chaque
    /// appel de l'outil `Agent`.
    type Ultracoded = (
        ConversationRuntime<StepsApiClient, StaticToolExecutor>,
        std::rc::Rc<std::cell::RefCell<Observed>>,
        std::rc::Rc<std::cell::RefCell<Vec<serde_json::Value>>>,
    );

    /// Runtime d'essai dont l'outil `Agent` renvoie les rapports donnés, dans
    /// l'ordre, et note les arguments de chaque appel.
    fn ultracode_runtime(
        steps: Vec<Vec<AssistantEvent>>,
        reports: Vec<&'static str>,
        written: String,
    ) -> Ultracoded {
        let (mut runtime, observed, _) = aided_runtime_with(steps, vec![], vec![], written);
        let agents = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let recorded = std::rc::Rc::clone(&agents);
        let mut reports = std::collections::VecDeque::from(reports);
        runtime.tool_executor =
            std::mem::take(&mut runtime.tool_executor).register("Agent", move |input| {
                recorded
                    .borrow_mut()
                    .push(serde_json::from_str(input).expect("arguments JSON"));
                Ok(serde_json::json!({
                    "agentId": "a1",
                    "status": "completed",
                    "result": reports.pop_front().unwrap_or("VERDICT: PASS"),
                })
                .to_string())
            });
        (runtime, observed, agents)
    }

    #[test]
    fn ultracode_prepares_a_brief_then_an_independent_verifier_sends_problems_back() {
        let (path, output) = written_file("ultracode");
        let (mut runtime, observed, agents) = ultracode_runtime(
            vec![
                tool_step("write_file", r#"{"path":"total.py","content":"x"}"#),
                text_step("Fait : total.py additionne les prix."),
                tool_step("write_file", r#"{"path":"total.py","content":"y"}"#),
                text_step("Corrigé : la liste vide renvoie 0."),
            ],
            vec![
                "R1 écrire total.py qui additionne les prix",
                "R1 FAIL - total.py:2 - la liste vide n'est pas traitée\nVERDICT: FAIL",
                "R1 PASS - total.py:2 - vérifié avec une liste vide\nVERDICT: PASS",
            ],
            output,
        );
        let summary = runtime
            .run_turn("ultracode: écris total.py qui additionne les prix", None)
            .expect("tour");
        assert_eq!(summary.iterations, 4);
        let agents = agents.borrow();
        assert_eq!(agents.len(), 3, "étude, vérification, revérification");
        // Étude préalable : agent en lecture seule, avec la demande.
        assert_eq!(agents[0]["subagent_type"], "Explore");
        assert!(agents[0]["prompt"]
            .as_str()
            .is_some_and(|prompt| prompt.contains("écris total.py qui additionne les prix")));
        // Vérification : le dossier, le diff du tour et la réponse de l'agent.
        assert_eq!(agents[1]["subagent_type"], "Verification");
        let verify = agents[1]["prompt"].as_str().expect("mission");
        assert!(verify.contains("R1 écrire total.py qui additionne les prix"));
        assert!(verify.contains("+def total(prix):"));
        assert!(verify.contains("Fait : total.py additionne les prix."));
        let observed = observed.borrow();
        // Le modèle travaille avec le dossier sous les yeux…
        assert!(observed.requests[0].contains("[Ultracode] Preparation brief"));
        assert!(observed.requests[0].contains("R1 écrire total.py"));
        // … reçoit les objections du vérificateur…
        assert!(observed.requests[2].contains("[Claw] Ultracode: an independent verifier"));
        assert!(observed.requests[2].contains("la liste vide n'est pas traitée"));
        // … et le vérificateur remplace son propre contrôle des exigences.
        assert!(!observed
            .requests
            .iter()
            .any(|request| request.contains("check your work against every requirement")));
        assert!(observed
            .notices
            .iter()
            .any(|notice| notice
                == "Ultracode : la vérification indépendante n'a rien trouvé à redire"));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn without_ultracode_or_for_a_short_follow_up_no_agent_is_started() {
        let (mut runtime, _, agents) = ultracode_runtime(
            vec![text_step("Bonjour, que puis-je faire ?")],
            vec![],
            WRITTEN.to_string(),
        );
        runtime
            .run_turn("écris total.py qui additionne les prix", None)
            .expect("tour");
        assert!(agents.borrow().is_empty());

        // Le mot-clé seul dans une relance : rien à étudier ni à contre-vérifier.
        let (mut runtime, _, agents) =
            ultracode_runtime(vec![text_step("Je reprends.")], vec![], WRITTEN.to_string());
        runtime.run_turn("ultracode vas-y", None).expect("tour");
        assert!(agents.borrow().is_empty());
    }

    #[test]
    fn an_analysis_answer_is_fact_checked_once_in_ultracode() {
        let long_answer = "Le module de paiement calcule les taxes dans taxes.py. ".repeat(14);
        let (mut runtime, observed, agents) = ultracode_runtime(
            vec![
                text_step(&long_answer),
                text_step("Réponse corrigée : le calcul est dans facture.py."),
            ],
            vec![
                "R1 expliquer le calcul des taxes",
                "FAIL - taxes.py n'existe pas, le calcul est dans facture.py:40\nVERDICT: FAIL",
            ],
            WRITTEN.to_string(),
        );
        let summary = runtime
            .run_turn(
                "ultracode: explique comment le module de paiement calcule les taxes",
                None,
            )
            .expect("tour");
        assert_eq!(summary.iterations, 2);
        let agents = agents.borrow();
        assert_eq!(agents.len(), 2, "étude puis une seule contre-vérification");
        assert!(agents[1]["prompt"]
            .as_str()
            .is_some_and(|prompt| prompt.contains("without changing any file")));
        assert!(observed.borrow().requests[1].contains("Correct your answer"));
    }

    #[test]
    fn without_a_readable_verdict_the_model_checks_its_own_work_again() {
        let (path, output) = written_file("sans-verdict");
        let (mut runtime, observed, agents) = ultracode_runtime(
            vec![
                tool_step("write_file", r#"{"path":"total.py","content":"x"}"#),
                text_step("Fait."),
                text_step("Chaque exigence est couverte."),
            ],
            vec!["R1 écrire total.py", "Tout me semble correct."],
            output,
        );
        runtime
            .run_turn("ultracode: écris total.py qui additionne les prix", None)
            .expect("tour");
        assert_eq!(agents.borrow().len(), 2);
        let observed = observed.borrow();
        assert!(observed.requests[2].contains("check your work against every requirement"));
        assert!(observed
            .notices
            .iter()
            .any(|notice| notice.contains("pas rendu de verdict lisible")));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn in_team_mode_each_step_is_announced_with_the_role_the_work_needs() {
        struct Off;
        impl Drop for Off {
            fn drop(&mut self) {
                crate::team::set_team(false);
            }
        }
        let _off = Off;
        crate::team::set_team(true);
        let (mut runtime, observed, _) = aided_runtime(
            vec![
                tool_step("write_file", r#"{"path":"notes.txt","content":"x"}"#),
                text_step("Fait."),
                text_step("Corrigé."),
            ],
            vec![FAILING, PASSING],
        );
        runtime
            .run_turn("écris le fichier de notes", None)
            .expect("tour");
        // Comprendre, puis écrire le code, puis déboguer après la vérification en échec.
        assert_eq!(observed.borrow().team_steps, ["plan", "code", "debug"]);

        // Une demande hors programmation va au spécialiste du sujet.
        let (mut runtime, observed, _) =
            aided_runtime(vec![text_step("Hello, how are you?")], vec![]);
        runtime
            .run_turn("Traduis en anglais : bonjour, comment vas-tu ?", None)
            .expect("tour");
        assert_eq!(observed.borrow().team_steps, ["translation"]);

        // Mode coupé : aucun rôle annoncé.
        crate::team::set_team(false);
        let (mut runtime, observed, _) = aided_runtime(vec![text_step("Bonjour.")], vec![]);
        runtime.run_turn("salut", None).expect("tour");
        assert!(observed.borrow().team_steps.is_empty());
    }

    #[test]
    fn the_session_setting_turns_ultracode_on_until_it_is_turned_off() {
        struct Off;
        impl Drop for Off {
            fn drop(&mut self) {
                crate::workflow::set_ultracode(false);
            }
        }
        let _off = Off;
        crate::workflow::set_ultracode(true);
        let (mut runtime, _, agents) = ultracode_runtime(
            vec![text_step("Voici le plan.")],
            vec!["R1 un plan"],
            WRITTEN.to_string(),
        );
        runtime
            .run_turn("propose un plan pour la page de connexion", None)
            .expect("tour");
        assert_eq!(agents.borrow().len(), 1, "étude préalable sans le mot-clé");
        crate::workflow::set_ultracode(false);
        let (mut runtime, _, agents) = ultracode_runtime(
            vec![text_step("Voici le plan.")],
            vec![],
            WRITTEN.to_string(),
        );
        runtime
            .run_turn("propose un plan pour la page de connexion", None)
            .expect("tour");
        assert!(agents.borrow().is_empty());
    }

    /// Fichier réel de six lignes, « créé » par l'outil `write_file` du test.
    fn written_file(name: &str) -> (std::path::PathBuf, String) {
        let path = std::env::temp_dir().join(format!(
            "claw-revue-{name}-{}.py",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |elapsed| elapsed.as_nanos())
        ));
        let content = "def total(prix):\n    somme = 0\n    for p in prix:\n        somme += p\n    return somme\n\n";
        std::fs::write(&path, content).expect("fichier");
        let lines: Vec<String> = content.lines().map(|line| format!("+{line}")).collect();
        let output = serde_json::json!({
            "filePath": path.to_string_lossy(),
            "structuredPatch": [{ "oldStart": 1, "oldLines": 0, "newStart": 1, "newLines": lines.len(), "lines": lines }],
        })
        .to_string();
        (path, output)
    }

    #[test]
    fn another_model_reviews_the_changes_once() {
        let (path, output) = written_file("remarques");
        let (mut runtime, observed, _) = aided_runtime_with(
            vec![
                tool_step("write_file", r#"{"path":"total.py","content":"x"}"#),
                text_step("C'est fait."),
                text_step("Exigences vérifiées."),
                text_step("Corrigé."),
            ],
            vec![PASSING],
            vec!["total.py:2 - liste vide non gérée - renvoyer 0"],
            output,
        );
        runtime.run_turn("écris total.py", None).expect("tour");
        let observed = observed.borrow();
        // Une seule relecture, avec la demande et le diff.
        assert_eq!(observed.consults.len(), 1);
        assert_eq!(observed.consults[0].0, super::ConsultPurpose::Review);
        assert!(observed.consults[0].1.contains("écris total.py"));
        assert!(observed.consults[0].1.contains("+def total(prix):"));
        // Le modèle confronte d'abord son travail aux exigences…
        assert!(observed.requests[2].contains("check your work against every requirement"));
        // … puis les remarques du relecteur lui reviennent.
        assert!(observed.requests[3].contains("Another model reviewed your changes"));
        assert!(observed.requests[3].contains("liste vide non gérée"));
        assert!(observed.notices.iter().any(
            |notice| notice == "Relecture par relecteur : 1 remarque(s) transmise(s) à l'agent"
        ));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn an_approved_review_ends_the_turn_and_small_edits_are_not_reviewed() {
        let (path, output) = written_file("approuve");
        let (mut runtime, observed, _) = aided_runtime_with(
            vec![
                tool_step("write_file", r#"{"path":"total.py","content":"x"}"#),
                text_step("C'est fait."),
                text_step("Exigences vérifiées."),
            ],
            vec![PASSING],
            vec!["LGTM"],
            output,
        );
        let summary = runtime.run_turn("écris total.py", None).expect("tour");
        assert_eq!(summary.iterations, 3);
        assert!(observed
            .borrow()
            .notices
            .iter()
            .any(|notice| notice == "Relecture par relecteur : rien à corriger"));
        let _ = std::fs::remove_file(path);

        // Une modification d'une ligne : pas de relecture.
        let small =
            std::env::temp_dir().join(format!("claw-revue-petit-{}.py", std::process::id()));
        std::fs::write(&small, "x = 1\n").expect("fichier");
        let output = serde_json::json!({
            "filePath": small.to_string_lossy(),
            "structuredPatch": [{ "lines": ["-x = 0", "+x = 1"] }],
        })
        .to_string();
        let (mut runtime, observed, _) = aided_runtime_with(
            vec![
                tool_step("write_file", r#"{"path":"petit.py","content":"x"}"#),
                text_step("Fait."),
            ],
            vec![PASSING],
            vec!["jamais demandé"],
            output,
        );
        runtime.run_turn("change x", None).expect("tour");
        assert!(observed.borrow().consults.is_empty());
        let _ = std::fs::remove_file(small);
    }

    #[test]
    fn a_second_failed_verification_brings_advice_from_another_model() {
        let (mut runtime, observed, _) = aided_runtime_with(
            vec![
                tool_step("write_file", r#"{"path":"notes.txt","content":"x"}"#),
                text_step("Fait."),
                text_step("Essai 1."),
                text_step("Essai 2."),
            ],
            vec![FAILING, FAILING, PASSING],
            vec!["La cause est dans devise.py, pas dans rapport.py."],
            WRITTEN.to_string(),
        );
        runtime.run_turn("corrige", None).expect("tour");
        let observed = observed.borrow();
        assert_eq!(observed.consults.len(), 1);
        assert_eq!(observed.consults[0].0, super::ConsultPurpose::Unblock);
        assert!(observed.consults[0].1.contains("assertion failed"));
        // Le conseil accompagne le deuxième échec, pas le premier.
        assert!(!observed.requests[2].contains("Advice from another model"));
        assert!(observed.requests[3].contains("Advice from another model"));
        assert!(observed.requests[3].contains("La cause est dans devise.py"));
    }

    #[test]
    fn substantial_work_is_checked_against_every_requirement_once() {
        let (path, output) = written_file("exigences");
        let (mut runtime, observed, _) = aided_runtime_with(
            vec![
                tool_step("write_file", r#"{"path":"total.py","content":"x"}"#),
                text_step("C'est fait."),
                text_step("Chaque exigence est couverte."),
            ],
            vec![PASSING],
            vec!["LGTM"],
            output,
        );
        let summary = runtime.run_turn("écris total.py", None).expect("tour");
        let observed = observed.borrow();
        // Une seule demande de contrôle, après la vérification réussie.
        assert_eq!(summary.iterations, 3);
        assert!(observed.requests[2].contains("check your work against every requirement"));
        assert!(observed.requests[2].contains("edge cases"));
        // Le modèle n'invente pas de tests, et sa dernière réponse est pour l'utilisateur.
        assert!(observed.requests[2]
            .contains("never describe tests, commands or results that do not exist"));
        assert!(observed.requests[2]
            .contains("end with your final answer to the user's original request"));
        assert!(observed.requests[2].contains("how to install, run or test the result"));
        // Deux réponses sans outil : la première est rendue provisoire par le contrôle.
        assert_eq!((observed.drafted, observed.superseded), (2, 1));
        // La note de Claw est en anglais, mais rappelle de répondre dans la langue de l'utilisateur.
        assert!(observed.requests[2]
            .contains("in the language of their request, not in the language of this note"));
        assert_eq!(
            observed
                .notices
                .iter()
                .filter(|notice| notice.starts_with("Claw fait confronter"))
                .count(),
            1
        );
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn errors_seen_by_the_connected_editor_go_back_to_the_model() {
        use crate::editor::{tests::fake_editor, tests::problem, FileDiagnostics, Severity};
        let (path, output) = written_file("editeur");
        let shown = path.to_string_lossy().to_string();
        let asked = fake_editor(vec![FileDiagnostics {
            path: "total.py".to_string(),
            diagnostics: vec![
                problem(4, Severity::Error, "\"p\" is not defined"),
                problem(1, Severity::Warning, "missing docstring"),
            ],
        }]);
        let connection = crate::editor::connect("VS Code");
        let (mut runtime, observed, _) = aided_runtime_with(
            vec![
                tool_step("write_file", r#"{"path":"total.py","content":"x"}"#),
                text_step("C'est fait."),
                text_step("Corrigé."),
                text_step("Exigences vérifiées."),
            ],
            vec![PASSING],
            vec!["LGTM"],
            output,
        );
        let summary = runtime.run_turn("écris total.py", None).expect("tour");
        drop(connection);

        let observed = observed.borrow();
        assert_eq!(summary.iterations, 4);
        // Après la vérification réussie, l'éditeur est interrogé sur le fichier modifié.
        assert_eq!(*asked.lock().expect("demandes"), [[shown]]);
        let feedback = &observed.requests[2];
        assert!(feedback.contains("VS Code reports 1 error(s) in the files you changed"));
        assert!(feedback.contains("total.py:4:5: error:"));
        assert!(feedback.contains("is not defined"));
        // Les avertissements ne renvoient pas le modèle au travail.
        assert!(!feedback.contains("missing docstring"));
        assert!(observed.notices.iter().any(|notice| notice
            == "VS Code signale 1 erreur(s) dans les fichiers modifiés : transmises à l'agent"));
        // Sans nouvelle modification, l'éditeur n'est pas interrogé une seconde fois.
        assert!(!observed.requests[3].contains("VS Code reports 1 error(s)\n"));
        assert!(observed.requests[3].contains("check your work against every requirement"));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn a_stronger_model_takes_over_after_the_second_failed_verification() {
        let (mut runtime, observed, _) = aided_runtime_with(
            vec![
                tool_step("write_file", r#"{"path":"notes.txt","content":"x"}"#),
                text_step("Fait."),
                text_step("Essai 1."),
                text_step("Corrigé par le modèle fort."),
            ],
            vec![FAILING, FAILING, PASSING],
            Vec::new(),
            WRITTEN.to_string(),
        );
        runtime.api_client.escalation = Some("fort/modele");
        runtime.run_turn("corrige", None).expect("tour");
        {
            let observed = observed.borrow();
            // Une seule escalade, au deuxième échec ; le modèle habituel revient ensuite.
            assert_eq!(observed.escalations, 1);
            assert!(!observed.requests[2].contains("You are fort/modele"));
            assert!(observed.requests[3].contains("You are fort/modele"));
            assert!(observed.notices.iter().any(|notice| notice
                == "L'agent n'avance plus : fort/modele reprend la fin du tour (escalationModel)"));
            assert_eq!(observed.restored, 1);
        }

        // Sans modèle plus fort réglé : ni note au modèle ni avis.
        let (mut runtime, observed, _) = aided_runtime_with(
            vec![
                tool_step("write_file", r#"{"path":"notes.txt","content":"x"}"#),
                text_step("Fait."),
                text_step("Essai 1."),
                text_step("Essai 2."),
            ],
            vec![FAILING, FAILING, PASSING],
            Vec::new(),
            WRITTEN.to_string(),
        );
        runtime.run_turn("corrige", None).expect("tour");
        let observed = observed.borrow();
        assert_eq!(observed.escalations, 1);
        assert!(!observed.requests[3].contains("stronger model"));
        assert!(!observed
            .notices
            .iter()
            .any(|notice| notice.contains("reprend la fin du tour")));
    }

    #[test]
    fn rewriting_a_test_after_a_failed_check_is_flagged_once() {
        let cwd = std::env::current_dir().expect("dossier");
        let patched = |added: &str| {
            serde_json::json!({
                "filePath": cwd.join("test_total.py").to_string_lossy(),
                "structuredPatch": [{ "lines": ["-    assert total([1, 2]) == 3", added] }],
            })
            .to_string()
        };
        let rewritten = patched("+    assert True");
        let flagged = |notices: &[String]| {
            notices
                .iter()
                .filter(|notice| notice.starts_with("Attention : test_total.py"))
                .count()
        };
        let (mut runtime, observed, _) = aided_runtime_with(
            vec![
                tool_step("bash", r#"{"command":"python -m unittest"}"#),
                tool_step("write_file", r#"{"path":"test_total.py","content":"x"}"#),
                tool_step("write_file", r#"{"path":"test_total.py","content":"y"}"#),
                text_step("Fait."),
            ],
            vec![FAILING],
            Vec::new(),
            rewritten.clone(),
        );
        runtime.run_turn("corrige total", None).expect("tour");
        assert_eq!(flagged(&observed.borrow().notices), 1);
        assert!(observed.borrow().requests[2].contains("rewriting it proves nothing"));

        // Sans échec auparavant, modifier un test est un travail ordinaire.
        let (mut runtime, observed, _) = aided_runtime_with(
            vec![
                tool_step("write_file", r#"{"path":"test_total.py","content":"x"}"#),
                text_step("Fait."),
            ],
            vec![PASSING],
            Vec::new(),
            rewritten,
        );
        runtime.run_turn("mets à jour le test", None).expect("tour");
        assert_eq!(flagged(&observed.borrow().notices), 0);

        // Après un échec, ajouter un test sans toucher aux autres reste permis.
        let (mut runtime, observed, _) = aided_runtime_with(
            vec![
                tool_step("bash", r#"{"command":"python -m unittest"}"#),
                tool_step("write_file", r#"{"path":"test_total.py","content":"x"}"#),
                text_step("Fait."),
            ],
            vec![FAILING],
            Vec::new(),
            patched("+    assert total([1, 2]) == 3"),
        );
        runtime.run_turn("ajoute un test", None).expect("tour");
        assert_eq!(flagged(&observed.borrow().notices), 0);
    }
}
