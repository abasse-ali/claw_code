use std::collections::BTreeMap;
use std::fmt::{Display, Formatter};

use crate::compact::{
    compact_session, compact_session_with, estimate_message_tokens, estimate_session_tokens,
    render_transcript_for_summary, summary_is_usable, CompactionConfig, CompactionResult,
    MODEL_SUMMARY_PROMPT, SUMMARY_RETRY_NOTE,
};
use crate::config::RuntimeFeatureConfig;
use crate::harness::{self, Aid, StallGuard, VerifySetting};
use crate::hooks::{HookEvent, HookPermissionDecision, HookRunResult, HookRunner};
use crate::interrupt::{clear_interrupt, interrupt_requested, INTERRUPTED_MESSAGE};
use crate::permissions::{
    PermissionOutcome, PermissionPolicy, PermissionPromptDecision, PermissionPrompter,
    PermissionRequest,
};
use crate::project;
use crate::session::{ContentBlock, ConversationMessage, Session};
use crate::usage::{TokenUsage, UsageTracker};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiRequest {
    pub system_prompt: Vec<String>,
    pub messages: Vec<ConversationMessage>,
    /// Réponse en texte seul : les outils restent décrits, mais le modèle ne
    /// peut pas les appeler (annonce de ce qu'il fait à l'utilisateur).
    pub text_only: bool,
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

    /// La même consultation, à un autre modèle que `excluded` (second avis).
    fn consult_again(
        &mut self,
        _purpose: ConsultPurpose,
        _system: &str,
        _prompt: &str,
        _excluded: &str,
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

    /// Tour de design : le confie au modèle réglé pour le design
    /// (`designModel`) jusqu'à la fin du tour ; renvoie son nom, ou `None`
    /// s'il n'y en a pas ou que c'est déjà lui.
    fn design_model(&mut self) -> Option<String> {
        None
    }

    /// Le modèle vient de répondre sans appeler d'outil : c'est sa réponse
    /// finale, sauf si un contrôle de Claw le renvoie au travail.
    fn answer_drafted(&mut self) {}

    /// Un contrôle de Claw (vérification, exigences, relecture, hook) renvoie
    /// le modèle au travail : la réponse qu'il venait de donner est provisoire,
    /// celle qui suivra la remplace.
    fn answer_superseded(&mut self) {}

    /// La réponse rendue provisoire par un contrôle reste la réponse : celle
    /// qui a suivi n'en était qu'un accusé de réception. L'interface rouvre
    /// la réponse qu'elle avait repliée.
    fn answer_kept(&mut self) {}

    /// La dernière réponse s'est arrêtée sur la limite de sortie du modèle au
    /// milieu de son texte, sans appel d'outil : le runtime en demande la suite.
    fn reply_cut_by_output_limit(&mut self) -> bool {
        false
    }

    /// Quelqu'un peut répondre aux questions de l'agent (menu à choix de
    /// l'outil `AskUserQuestion`) : faux dans un script ou pour un sous-agent.
    fn can_ask_user(&self) -> bool {
        false
    }

    /// Requêtes gratuites qu'il reste aujourd'hui pour le modèle de la session
    /// (`None` : inconnu, ou modèle qui n'est pas gratuit).
    fn free_requests_left(&mut self) -> Option<u64> {
        None
    }

    /// Défauts que montrent les images jointes à la demande, d'après le
    /// lecteur d'images : (numéro de l'image, défaut en une ligne). Le client
    /// peut les faire décrire à ce moment. Vide quand le modèle lit lui-même
    /// les images : rien n'est décrit.
    fn image_defects(
        &mut self,
        _images: &[crate::attachments::ImageAttachment],
    ) -> Vec<(usize, String)> {
        Vec::new()
    }

    /// Le modèle du tour lit lui-même les images : la capture d'une
    /// vérification de la page peut lui être jointe (sans description à
    /// attendre).
    fn reads_images(&mut self) -> bool {
        false
    }

    /// Critique de design : les captures d'une interface jugées par un modèle
    /// qui lit les images. `None` : aucun modèle disponible.
    fn critique_design(
        &mut self,
        _system: &str,
        _prompt: &str,
        _images: &[crate::attachments::ImageAttachment],
    ) -> Option<Consultation> {
        None
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
    /// Mode projet : relire le cahier des charges et le plan avant le code.
    Design,
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

/// Appel d'outil lancé dans un fil à part : son résultat et ses images.
pub type Background = std::thread::JoinHandle<(
    Result<String, String>,
    Vec<crate::attachments::ImageAttachment>,
)>;

/// Vérification de la page lancée après une modification, qui tourne pendant
/// la requête suivante du modèle (`spawn_background`) : la page à nommer, et
/// les défauts de la vérification d'avant, pour dire ce qui a changé.
struct PendingCheck {
    handle: Background,
    target: String,
    before: Option<Vec<String>>,
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

    /// Lance un outil dans un fil à part, sans rien demander à l'utilisateur
    /// (une vérification de Claw, en lecture seule) : son résultat et ses
    /// images se reprennent plus tard. `None` quand l'exécuteur ne sait pas :
    /// l'outil tourne alors tout de suite, comme avant.
    fn spawn_background(&mut self, _tool_name: &str, _input: &str) -> Option<Background> {
        None
    }

    /// Annonce un appel lancé par Claw lui-même (vérification imposée), pour
    /// que l'interface l'affiche comme un appel du modèle.
    fn announce(&mut self, _tool_name: &str, _input: &str) {}

    /// Images produites par le dernier appel (captures d'écran), à joindre à
    /// son résultat ; par défaut aucune.
    fn take_images(&mut self) -> Vec<crate::attachments::ImageAttachment> {
        Vec::new()
    }

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

    /// Ce que la mise en forme des arguments de cet appel a changé ou ignoré
    /// (nom d'un autre agent traduit, nombre écrit en texte), à dire au modèle
    /// avec le résultat : il ne refera pas la même faute.
    fn argument_note(&mut self, _tool_name: &str, _input: &str) -> Option<String> {
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
    /// Vues de fichiers encore affichées dans la conversation (marque de la
    /// vue, fichier, taille et date au moment de la vue) : retirées dès que
    /// le fichier change par un autre moyen qu'une modification du modèle.
    file_views: BTreeMap<String, FileStamp>,
    /// Fichier où la session s'enregistre à chaque étape d'un tour.
    autosave: Option<std::path::PathBuf>,
    /// Requêtes au modèle par tour avant que Claw lui fasse conclure (0 : sans limite).
    turn_request_limit: usize,
    /// Heure (secondes Unix) à laquelle le modèle conclut, quoi qu'il reste
    /// (`CLAW_DEADLINE`, posée par le moteur de Claw Web avant la fin de son job).
    deadline: Option<u64>,
    /// Dossier suivi par le mode projet ; `None` : le dossier de travail.
    project_root: Option<std::path::PathBuf>,
    /// Mode projet : la sonde de l'environnement, au premier tour (une fois
    /// par session).
    environment: EnvironmentProbe,
    /// Captures d'écran des appels de la réponse en cours, jointes à son
    /// dernier résultat d'outil.
    tool_images: Vec<crate::attachments::ImageAttachment>,
    /// Dernière page regardée par le modèle (`Screenshot`, `ViewPage`) :
    /// celle que reprend la revue visuelle.
    page_target: Option<String>,
    /// Dernière capture demandée par le modèle (entrée normalisée, résultat),
    /// tant qu'aucun fichier n'a pu changer depuis : la même demande la reprend.
    last_capture: Option<(String, String)>,
    /// Arrêt demandé par un hook (`"continue": false`) : le tour s'arrête
    /// après les appels en cours.
    hook_stop: Option<String>,
    /// Le tour répond à une question d'information : les outils qui écrivent
    /// des fichiers sont refusés.
    question_turn: bool,
}

/// Issue de la démonstration finale du mode projet.
enum DemoOutcome {
    Passed,
    /// Pas lancée (refus, demande d'autorisation sans réponse).
    NotRun,
    /// Remarque de l'échec, pour le modèle.
    Failed(String),
}

/// Sonde de l'environnement du mode projet : à faire au premier tour de la
/// session, faite (ce qu'elle a trouvé, gardé pour le relecteur
/// d'architecture), ou coupée (essais).
#[derive(Debug, Clone, PartialEq, Eq)]
enum EnvironmentProbe {
    Pending,
    Done(String),
    Off,
}

/// Budget de requêtes d'un tour : variable `CLAW_TURN_REQUESTS`, sinon réglage
/// `turnRequestLimit`, sinon `harness::TURN_REQUEST_LIMIT`.
fn turn_request_limit(config: &RuntimeFeatureConfig) -> usize {
    std::env::var("CLAW_TURN_REQUESTS")
        .ok()
        .and_then(|value| value.trim().parse().ok())
        .or(config.turn_request_limit())
        .unwrap_or(harness::TURN_REQUEST_LIMIT)
}

/// Fichier, taille et date de modification au moment d'une vue.
type FileStamp = (std::path::PathBuf, u64, Option<std::time::SystemTime>);

fn file_stamp(path: &std::path::Path) -> Option<FileStamp> {
    let metadata = std::fs::metadata(path).ok()?;
    Some((path.to_path_buf(), metadata.len(), metadata.modified().ok()))
}

/// Aides au raisonnement, le temps d'un tour.
// Des états indépendants les uns des autres, pas une machine à états.
#[allow(clippy::struct_excessive_bools)]
#[derive(Default)]
struct TurnAids {
    /// Des fichiers ont été modifiés depuis la dernière vérification.
    dirty: bool,
    /// Un fichier d'interface (HTML, CSS, composant) a changé depuis la
    /// dernière revue visuelle.
    interface_dirty: bool,
    /// Revues visuelles faites pendant ce tour.
    interface_rounds: usize,
    /// Modifications de l'interface depuis le dernier regard sur la page (une
    /// capture du modèle, une revue, une mesure de Claw).
    blind_edits: u32,
    /// Vérifications de la page faites par Claw pendant ce tour, après les
    /// modifications de l'interface.
    blind_measures: u32,
    /// Défauts relevés par la vérification précédente : la suivante dit ce
    /// que la modification a cassé ou réparé.
    last_checked: Option<Vec<String>>,
    /// Une réponse « vérifié » sans regard sur la page a déjà été renvoyée.
    claim_checked: bool,
    /// Recherches dans les styles à la suite, sans modification ni capture.
    style_searches: u32,
    /// Remarques déjà faites pendant ce tour vers `inspect` de `Screenshot`.
    style_search_notes: u32,
    /// Fichiers d'interface changés par une commande, déjà revus.
    interface_commands: std::collections::BTreeSet<String>,
    /// La méthode de design est jointe à la demande de ce tour.
    design_guided: bool,
    /// Le message du tour demande des corrections, pas une création : la
    /// revue renvoie ce qui est mesuré, sans note de design à atteindre.
    repair: bool,
    /// Le plan de design de ce tour a été relu avant le code.
    plan_reviewed: bool,
    /// Le modèle a écrit l'interface sans plan : il lui a été demandé.
    plan_requested: bool,
    /// Wireframe du plan mal aligné : signalé une fois par tour.
    wireframe_flagged: bool,
    /// Un autre modèle a repris après deux réponses vides de suite.
    empty_escalated: bool,
    /// Le modèle a été prévenu que sa page appelle des fichiers qui manquent.
    assets_reminded: bool,
    /// Le modèle a été prévenu que ses styles n'ont pas le socle de la méthode.
    foundation_noted: bool,
    /// Notes des revues visuelles du tour, dans l'ordre.
    graded: Vec<crate::web_design::Graded>,
    /// Règles des défauts mesurés sur la page avant le premier changement du
    /// tour (`web_design::problem_key`) : les revues nomment ceux que le tour
    /// a introduits.
    baseline: Option<Vec<String>>,
    /// Pièges du code déjà signalés pendant ce tour (fichier et constat).
    pitfalls_seen: std::collections::HashSet<String>,
    /// Langages dont les règles ont été données pendant ce tour.
    code_languages: std::collections::BTreeSet<crate::code_guide::Language>,
    /// Scripts écrits pendant ce tour (chemins).
    scripts_written: std::collections::BTreeSet<String>,
    /// Fichiers SVG écrits pendant ce tour : relus, ils ne comptent pas comme
    /// des SVG récupérés.
    svg_written: std::collections::BTreeSet<String>,
    /// Commandes lancées par le modèle pendant ce tour.
    commands_run: Vec<String>,
    /// Le modèle a été renvoyé essayer ses scripts.
    scripts_asked: bool,
    /// Le piège des guillemets de `PowerShell` passé par bash a été dit.
    quoting_noted: bool,
    /// Les premiers outils du tour ont été vus : l'annonce de départ n'est
    /// demandée qu'une fois.
    announce_checked: bool,
    /// Étapes d'outils d'affilée sans un mot pour l'utilisateur.
    silent_steps: u32,
    /// Annonce à demander à la requête suivante (en texte seul).
    announce_due: Option<String>,
    /// Le message est une question sur le travail : la réponse en texte seul
    /// est la réponse du tour (pas de reprise du travail).
    work_question: bool,
    /// Poursuites accordées au tour de design resté sous la barre.
    continuations: usize,
    /// Revues notées au moment de la dernière poursuite.
    graded_at_continuation: usize,
    /// Notes de fonctionnement des revues du tour (`function_score`).
    function_scores: Vec<u8>,
    /// Poursuites accordées au fonctionnement sous la barre.
    function_extensions: usize,
    function_explained: bool,
    /// Vérification de la page en cours, lancée en arrière-plan.
    pending_check: Option<PendingCheck>,
    /// Le design a stagné une fois : le juge a été prié de changer la structure.
    restructured: bool,
    restructure_asked: bool,
    /// Revues visuelles ajoutées au tour par les poursuites.
    extra_rounds: usize,
    /// L'arrêt sous la barre a été expliqué à l'utilisateur.
    continuation_explained: bool,
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
    /// Services tiers qui reçoivent les données des visiteurs : signalés une fois par tour.
    services_checked: bool,
    /// Questions montrées sans attendre (appli web) : le tour finit avec ce
    /// lot d'outils, la réponse viendra au message suivant.
    questions_deferred: bool,
    /// Ce que dit le message de l'utilisateur (problème signalé, images à
    /// trier) : méthode jointe à sa demande, preuves demandées avant de conclure.
    report: harness::Report,
    /// Le lecteur d'images a vu un défaut dans une image de la demande.
    saw_defects: bool,
    /// Réponses renvoyées pendant ce tour pour des titres sans contenu.
    empty_title_rounds: usize,
    /// L'utilisateur a appris qu'une demande d'autorisation est restée sans réponse.
    away_noticed: bool,
    /// Les preuves ont déjà été demandées pendant ce tour.
    evidence_asked: bool,
    /// Les commandes conseillées sans avoir été lancées ont déjà été signalées.
    commands_checked: bool,
    /// La réponse sur une page, écrite pour un développeur, a déjà été renvoyée.
    /// Réponses renvoyées pour être dites en mots simples. Une seconde fois
    /// seulement quand l'agent a modifié des fichiers depuis (une autre garde
    /// l'a renvoyé au travail) : sa nouvelle réponse se relit. Vu le 26 octobre
    /// 2026 (session #32, passage 5) : réécrite, puis refaite après la relecture
    /// et le tri des photos, la réponse finale citait app.js et localStorage.
    plain_asked: u8,
    /// Fichiers modifiés depuis la dernière demande de mots simples.
    edited_since_plain: bool,
    /// Règles inventées sur la page déjà cherchées dans ce tour.
    rules_checked: bool,
    /// La demande fait voir aux uns ce que les autres entrent : les revues
    /// vérifient sur un deuxième appareil.
    shared_request: bool,
    /// Code d'accès de la page déjà cherché dans la réponse.
    code_told: bool,
    /// Données que la demande veut partagées, gardées dans chaque navigateur :
    /// déjà signalé.
    shared_checked: bool,
    /// Clés d'un compte à créer laissées dans le code : déjà signalées.
    keys_checked: bool,
    /// Photos enregistrées par l'outil `Photo` pendant le tour.
    photos: std::collections::BTreeSet<String>,
    /// Photos jamais utilisées par la page : déjà signalées.
    photos_checked: bool,
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
    /// État des fichiers de code du dépôt au début du tour, puis après chaque
    /// vérification : ce qu'une commande change ensuite se vérifie aussi.
    snapshot: Option<harness::ProjectSnapshot>,
    /// Fichiers changés par une commande pendant le tour.
    command_changes: Vec<String>,
    /// Fichiers de données dont des lignes ont été réécrites, déjà signalés.
    guarded_data: std::collections::BTreeSet<String>,
    /// Contenu des fichiers au début du tour (`None` : absents), relevé avant
    /// leur première modification par les outils du modèle.
    turn_start_files: std::collections::BTreeMap<std::path::PathBuf, Option<String>>,
    /// Dernière version écrite par les outils du modèle : une commande qui la
    /// supprime ou la ramène en arrière lui fait perdre son travail.
    edited_files: std::collections::BTreeMap<std::path::PathBuf, String>,
    /// Scripts muets déjà signalés (`main()` jamais appelée).
    silent_scripts: std::collections::BTreeSet<String>,
    /// Fichiers de données du dépôt au début du tour, pour voir ce qu'une
    /// commande du modèle en réécrit.
    data_snapshot: Option<harness::DataSnapshot>,
    /// Appels d'outils de la réponse en cours.
    calls_in_reply: usize,
    /// Réponses d'un seul appel d'observation, à la suite.
    single_call_streak: u32,
    /// Outil et commande des derniers appels isolés qui étaient des commandes.
    single_commands: Vec<(String, String)>,
    /// Remarques de regroupement des essais déjà faites pendant ce tour.
    batch_notes: u32,
    /// Remarques déjà faites pendant ce tour pour une liste de tâches envoyée seule.
    todo_alone_notes: u32,
    /// Tâches pas finies de la dernière liste écrite pendant ce tour.
    open_tasks: Vec<(String, String)>,
    /// Renvois déjà faits pendant ce tour pour une liste laissée ouverte.
    open_task_rounds: u32,
    /// Paliers de requêtes déjà ajoutés pour finir la liste de tâches, et
    /// l'avancée au dernier palier (modifications, tâches ouvertes).
    task_extensions: u32,
    changes_at_extension: usize,
    open_at_extension: Option<usize>,
    /// Le refus d'un palier (rien n'avance) déjà dit.
    task_extension_explained: bool,
    /// Renvois déjà faits pendant ce tour pour un travail annoncé et pas fait.
    announced_work_rounds: u32,
    /// Données de test laissées dans une page : déjà renvoyé faire le ménage.
    test_data_sent_back: bool,
    /// Lectures de fichiers de ce tour (empreinte du résultat).
    reads_seen: std::collections::BTreeSet<u64>,
    /// Fichiers lus alors qu'une vue à jour les montrait, déjà signalés.
    viewed_reads: std::collections::BTreeSet<String>,
    /// Remarques déjà faites pendant ce tour pour un fichier relu à l'identique.
    repeated_read_notes: u32,
    /// Lectures entières de longs fichiers pendant ce tour, par fichier.
    whole_reads: std::collections::HashMap<String, u32>,
    /// Mode projet : ce que Claw suit pendant le tour.
    project: Option<project::Turn>,
    /// Requêtes au modèle faites pendant le tour.
    requests: usize,
    /// Modifications et commandes du tour : une vérification de jalon en échec
    /// ne se refait qu'après un changement.
    project_changes: usize,
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
            substantive.then(|| crate::compact::clip_request(text, MAX_EARLIER_REQUEST_CHARS))
        })
        // Demande passée dans un résumé de compaction : Claw l'y a notée mot
        // pour mot.
        .or_else(|| {
            crate::compact::recorded_requests(messages)
                .into_iter()
                .rev()
                .find(|text| text.split_whitespace().count() >= CONTINUATION_MAX_WORDS)
                .map(|text| crate::compact::clip_request(&text, MAX_EARLIER_REQUEST_CHARS))
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

/// Clé d'une demande de capture : ses champs triés, sans les espaces.
fn capture_key(input: &str) -> Option<String> {
    serde_json::from_str::<std::collections::BTreeMap<String, serde_json::Value>>(input)
        .ok()
        .and_then(|fields| serde_json::to_string(&fields).ok())
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

/// Remplace par `placeholder` les vues de fichier qui commencent par `marker`
/// dans les résultats déjà enregistrés : une seule vue à jour par fichier dans
/// la conversation, pour que le modèle ne cite pas un texte périmé.
fn replace_file_views(session: &mut Session, marker: &str, placeholder: &str) {
    for message in &mut session.messages {
        for block in &mut message.blocks {
            // Vues jointes à un résultat d'outil, ou au premier message d'un
            // petit projet.
            let output = match block {
                ContentBlock::ToolResult { output, .. } => output,
                ContentBlock::Text { text } => text,
                _ => continue,
            };
            let mut from = 0;
            while let Some(start) = output[from..].find(marker).map(|at| from + at) {
                let Some(length) = output[start..].find(harness::FILE_VIEW_END) else {
                    break;
                };
                let end = start + length + harness::FILE_VIEW_END.len();
                output.replace_range(start..end, placeholder);
                from = start + placeholder.len();
            }
        }
    }
}

/// Allège les résultats d'outils déjà enregistrés dans la session, comme
/// `lean_result` le fait pour chaque nouveau résultat.
fn lean_stored_results(session: &mut Session) {
    if !harness::enabled(Aid::LeanResults) {
        return;
    }
    for message in &mut session.messages {
        let tool_name = message.blocks.iter().find_map(|block| match block {
            ContentBlock::ToolResult { tool_name, .. } => Some(tool_name.clone()),
            _ => None,
        });
        if let Some(tool_name) = tool_name {
            lean_result(&tool_name, message);
        }
    }
}

/// Contenu d'un fichier avant sa première modification du tour par les outils
/// du modèle (garde des modifications perdues).
fn remember_turn_start(tool_name: &str, input: &str, aids: &mut TurnAids) {
    if !crate::file_tools::writes_files(tool_name)
        || tool_name == "NotebookEdit"
        || !(harness::enabled(Aid::LostEdits)
            || harness::enabled(Aid::DataGuard)
            || harness::enabled(Aid::TestGuard))
    {
        return;
    }
    for written in crate::file_tools::written_paths(tool_name, input) {
        let Some(path) = edited_path(&serde_json::json!({ "path": written }).to_string()) else {
            continue;
        };
        aids.turn_start_files
            .entry(path)
            .or_insert_with_key(|path| std::fs::read_to_string(path).ok());
    }
}

/// Chemin absolu, sans `.` ni `..`, du fichier que modifie un appel
/// `edit_file` ou `write_file`.
fn edited_path(input: &str) -> Option<std::path::PathBuf> {
    let path = json_field(input, "path")?;
    let joined = std::env::current_dir().ok()?.join(path);
    let mut clean = std::path::PathBuf::new();
    for part in joined.components() {
        match part {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                clean.pop();
            }
            other => clean.push(other),
        }
    }
    Some(clean)
}

/// Résultat d'`apply_patch` ou de `delete_file` : pour chaque fichier changé,
/// une entrée `{"path"}` et sa part du résultat, de la forme de celui d'`edit_file`.
fn changed_file_results(output: &str) -> Vec<(String, String)> {
    let mut values = serde_json::Deserializer::from_str(output).into_iter::<serde_json::Value>();
    let Some(Ok(value)) = values.next() else {
        return Vec::new();
    };
    value
        .get("files")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|file| {
            let path = file.get("filePath")?.as_str()?;
            Some((
                serde_json::json!({ "path": path }).to_string(),
                file.to_string(),
            ))
        })
        .collect()
}

/// Même texte, aux fins de ligne près (Git peut les changer en restaurant).
fn same_text(left: &str, right: &str) -> bool {
    left.lines().eq(right.lines())
}

/// Nombre d'occurrences de chaque ligne non vide d'un texte, sans les blancs
/// autour.
fn line_counts(text: &str) -> std::collections::HashMap<&str, usize> {
    let mut counts = std::collections::HashMap::new();
    for line in text.lines().map(str::trim).filter(|line| !line.is_empty()) {
        *counts.entry(line).or_insert(0) += 1;
    }
    counts
}

/// Remarque pour une réponse qui n'a fait qu'un appel, chaque réponse étant
/// une requête : la liste de tâches envoyée seule alors que du travail suit,
/// ou une observation isolée de plus (lecture, recherche, commande) après
/// plusieurs à la suite. Une commande au code de sortie non nul reste une
/// observation (un refus essayé exprès) ; seuls un outil en erreur ou une
/// réponse d'un autre genre interrompent la série.
fn lone_call_note(
    tool_name: &str,
    input: &str,
    output: &str,
    is_error: bool,
    aids: &mut TurnAids,
) -> Option<String> {
    let alone = aids.calls_in_reply == 1 && !is_error && harness::enabled(Aid::BatchCalls);
    let observation = matches!(
        tool_name,
        "read_file"
            | "grep_search"
            | "glob_search"
            | "list_directory"
            | "read_many_files"
            | "file_outline"
            | "git"
            | "bash"
            | "PowerShell"
    );
    if !alone || !observation {
        aids.single_call_streak = 0;
        aids.single_commands.clear();
        if alone
            && tool_name == "TodoWrite"
            && aids.todo_alone_notes < harness::MAX_TODO_ALONE_NOTES
            && harness::todos_left(output)
        {
            aids.todo_alone_notes += 1;
            return Some(harness::TODO_ALONE_NOTE.to_string());
        }
        return None;
    }
    aids.single_call_streak += 1;
    match json_field(input, "command").filter(|_| matches!(tool_name, "bash" | "PowerShell")) {
        Some(command) => aids.single_commands.push((tool_name.to_string(), command)),
        None => aids.single_commands.clear(),
    }
    let kept = harness::SINGLE_CALL_REPLIES as usize;
    let extra = aids.single_commands.len().saturating_sub(kept);
    aids.single_commands.drain(..extra);
    if !aids
        .single_call_streak
        .is_multiple_of(harness::SINGLE_CALL_REPLIES)
        || aids.batch_notes >= harness::MAX_BATCH_NOTES
    {
        return None;
    }
    aids.batch_notes += 1;
    Some(harness::batch_note(
        aids.single_call_streak,
        &aids.single_commands,
    ))
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
    // Les tests lancés par `run_tests` : une vérification comme `cargo test` dans bash.
    if tool_name == "run_tests" {
        let value = serde_json::Deserializer::from_str(output)
            .into_iter::<serde_json::Value>()
            .next()?
            .ok()?;
        if value
            .get("note")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|note| note.contains("background"))
        {
            return None;
        }
        return Some(CommandOutcome {
            command: value.get("command")?.as_str()?.to_string(),
            passed: value.get("succeeded").and_then(serde_json::Value::as_bool) == Some(true),
            output: value
                .get("output")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_string(),
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

/// Captures d'écran d'outils gardées dans la conversation.
const MAX_TOOL_IMAGES: usize = 4;
const OLD_SCREENSHOTS_NOTE: &str = "[Claw] The screenshots of this call were removed from the conversation to save room: call Screenshot again to see the page as it is now.";

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
/// Le résultat de cet outil apporte des SVG récupérés, pas dessinés par le
/// modèle : un outil d'icônes ou d'ornements, une page lue sur le web, un
/// fichier d'un paquet, un SVG du projet qu'il n'a pas écrit, la sortie d'un
/// téléchargement ou d'un gestionnaire de paquets (pas sa propre page servie
/// en local).
fn fetches_svg(tool_name: &str, input: &str, aids: &TurnAids) -> bool {
    match tool_name {
        "WebFetch" | "http_request" | "Ornament" | "Icon" => true,
        "read_file" => json_field(input, "path").is_some_and(|path| {
            let lower = path.to_ascii_lowercase().replace('\\', "/");
            lower.contains("node_modules/")
                || lower.contains("vendor/")
                || (lower.ends_with(".svg") && !aids.svg_written.contains(&path))
        }),
        "bash" | "PowerShell" => json_field(input, "command").is_some_and(|command| {
            let local = ["localhost", "127.0.0.1", "0.0.0.0"]
                .iter()
                .any(|host| command.contains(host));
            !local
                && [
                    "curl ",
                    "wget ",
                    "npm ",
                    "npx ",
                    "pnpm ",
                    "yarn ",
                    "node_modules",
                    "pip ",
                    "git clone",
                    "Invoke-WebRequest",
                ]
                .iter()
                .any(|word| command.contains(word))
        }),
        _ => false,
    }
}

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

/// Relance d'une réponse en texte arrêtée par la limite de sortie du modèle.
/// Vu le 24 octobre 2026 (session #30, Nemotron 3 Ultra) : une appli entière
/// préparée pour un seul fichier, la réponse coupée avant l'appel d'outil, et
/// « continue » relançait le même fichier trop long.
const CUT_REPLY_NUDGE: &str = "[Claw] Your reply was cut off by the output limit. Continue exactly where you stopped, without repeating anything you already wrote. If you were about to write a file, it does not fit in one reply: split it into several smaller files (the page, its stylesheet, its script), or Write a first part and add the rest with Edit, one call per part.";

/// Un compte rendu d'au moins cette taille est gardé quand la réponse au
/// contrôle qui l'a suivi fait moins du tiers de sa longueur.
const KEPT_ANSWER_MIN_CHARS: usize = 400;
const KEPT_ANSWER_RATIO: usize = 3;

/// Suites demandées au plus pour une même réponse coupée.
const MAX_CUT_CONTINUATIONS: usize = 2;

/// Résultat noté, dans la session enregistrée en cours de tour, pour un appel
/// dont l'exécution n'est pas finie : il n'y reste que si Claw est arrêté avant.
/// Instructions liées au fichier qu'un outil lit ou modifie (règles avec
/// `paths`, `CLAUDE.md` du sous-dossier), chacune donnée une seule fois.
fn path_instruction_notes(tool_name: &str, input: &str) -> Vec<String> {
    if !matches!(
        tool_name,
        "read_file" | "write_file" | "edit_file" | "MultiEdit" | "delete_file" | "NotebookEdit"
    ) {
        return Vec::new();
    }
    serde_json::from_str::<serde_json::Value>(input)
        .ok()
        .and_then(|value| {
            value
                .get("path")
                .or_else(|| value.get("notebook_path"))
                .and_then(serde_json::Value::as_str)
                .map(ToOwned::to_owned)
        })
        .map(|path| crate::memory::instructions_for(&path))
        .unwrap_or_default()
}

/// Résultat des appels restants quand un hook arrête l'agent (`"continue": false`).
const HOOK_STOP_MESSAGE: &str =
    "[Claw] Not run: a hook stopped the agent (\"continue\": false) before this call.";

const INTERRUPTED_CALL_RESULT: &str = "[Claw] Claw was stopped while this call was running: its result was not recorded, and the call may have run only in part. Check the current state (files, processes) before relying on it or running it again.";

/// Une erreur d'API en une ligne lisible : sans le corps JSON que le
/// fournisseur y joint.
fn short_error(error: &str) -> String {
    const MAX_CHARS: usize = 160;
    let line = error.lines().next().unwrap_or_default();
    let line = [" : [", " : {", ": [", ": {"]
        .iter()
        .filter_map(|start| line.find(start))
        .min()
        .map_or(line, |end| &line[..end])
        .trim();
    if line.chars().count() > MAX_CHARS {
        let cut: String = line.chars().take(MAX_CHARS).collect();
        format!("{cut}…")
    } else {
        line.to_string()
    }
}

/// Le dernier message est la relance d'une réponse coupée, juste après elle.
fn after_cut_nudge(messages: &[ConversationMessage]) -> bool {
    let [.., cut, nudge] = messages else {
        return false;
    };
    cut.role == crate::session::MessageRole::Assistant
        && matches!(
            nudge.blocks.first(),
            Some(ContentBlock::Text { text }) if text == CUT_REPLY_NUDGE
        )
}

/// Ces messages demandent la suite d'une réponse coupée par la limite de
/// sortie : le texte déjà écrit, pour que l'affichage y raccorde la suite.
#[must_use]
pub fn cut_reply_continued(messages: &[ConversationMessage]) -> Option<String> {
    after_cut_nudge(messages).then(|| message_text(&messages[messages.len() - 2]))
}

/// Demande l'avis des hooks `Notification` avant chaque demande d'autorisation.
/// Autorisation d'un appel selon la décision des hooks `PreToolUse`.
fn authorize_after_hook(
    policy: &mut PermissionPolicy,
    decision: Option<HookPermissionDecision>,
    tool_name: &str,
    input: &str,
    prompter: Option<&mut dyn PermissionPrompter>,
) -> PermissionOutcome {
    let outcome = match decision {
        Some(HookPermissionDecision::Allow) => policy.authorize_preapproved(tool_name, input),
        Some(HookPermissionDecision::Ask) => {
            policy.authorize_with_prompt(tool_name, input, prompter)
        }
        None => policy.authorize(tool_name, input, prompter),
    };
    crate::telemetry::tool_decision(
        tool_name,
        input,
        matches!(outcome, PermissionOutcome::Allow),
    );
    outcome
}

struct NotifyingPrompter<'a> {
    hooks: &'a HookRunner,
    inner: &'a mut dyn PermissionPrompter,
}

impl PermissionPrompter for NotifyingPrompter<'_> {
    fn decide(&mut self, request: &PermissionRequest) -> PermissionPromptDecision {
        // Hooks `PermissionRequest` : ils peuvent répondre à la place de
        // l'utilisateur (autoriser ou refuser).
        let hooked = self
            .hooks
            .run_permission_request(&request.tool_name, &request.input);
        if hooked.is_denied() {
            return PermissionPromptDecision::Deny {
                reason: hooked.reason("refusé par un hook PermissionRequest"),
            };
        }
        if hooked.permission_decision() == Some(HookPermissionDecision::Allow) {
            return PermissionPromptDecision::Allow;
        }
        // Une demande de ce tour est restée sans réponse : l'utilisateur est
        // absent, et chaque nouvelle demande l'attendrait encore longtemps.
        if crate::permissions::user_away() {
            return crate::permissions::not_asked(&request.tool_name);
        }
        self.hooks.notify_in_background(
            "permission_prompt",
            &format!(
                "Claw demande l'autorisation d'utiliser {}",
                request.tool_name
            ),
        );
        crate::telemetry::note_asked();
        let decision = self.inner.decide(request);
        if matches!(&decision, PermissionPromptDecision::Deny { reason } if crate::permissions::is_not_run(reason))
        {
            crate::permissions::set_user_away(true);
        }
        decision
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
        let mut session = session;
        // Session enregistrée avant l'allègement des résultats (ou pendant
        // qu'il était coupé) : ses anciens résultats d'outils repartiraient en
        // entier avec chaque requête.
        lean_stored_results(&mut session);
        // Les fichiers ont pu changer depuis l'enregistrement de la session :
        // les vues qu'elle garde ne sont plus « le fichier tel qu'il est ».
        replace_file_views(
            &mut session,
            harness::FILE_VIEW_PREFIX,
            harness::FILE_VIEW_STALE,
        );
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
            file_views: BTreeMap::new(),
            autosave: None,
            turn_request_limit: turn_request_limit(&feature_config),
            deadline: harness::deadline(),
            project_root: None,
            environment: EnvironmentProbe::Pending,
            tool_images: Vec::new(),
            page_target: None,
            last_capture: None,
            hook_stop: None,
            question_turn: false,
        }
    }

    /// Requêtes au modèle par tour avant que Claw lui fasse conclure (0 :
    /// sans limite).
    #[must_use]
    pub fn with_turn_request_limit(mut self, limit: usize) -> Self {
        self.turn_request_limit = limit;
        self
    }

    /// Heure (secondes Unix) à laquelle le modèle doit conclure.
    #[must_use]
    pub fn with_deadline(mut self, deadline: Option<u64>) -> Self {
        self.deadline = deadline;
        self
    }

    /// Enregistre la session dans `path` à chaque étape des tours suivants :
    /// un tour interrompu (fenêtre fermée, plantage, coupure) garde sa trace
    /// et peut être repris.
    pub fn autosave_to(&mut self, path: impl Into<std::path::PathBuf>) {
        self.autosave = Some(path.into());
    }

    /// Enregistrement en cours de tour. `pending` : appels dont le résultat
    /// n'est pas encore connu ; ils sont notés comme interrompus, pour que la
    /// session enregistrée reste valide si Claw s'arrête pendant leur
    /// exécution. Un échec d'écriture n'arrête pas le tour : l'interface
    /// enregistre de nouveau à la fin et le signale.
    fn autosave(&self, pending: &[(String, String, String)]) {
        let Some(path) = &self.autosave else {
            return;
        };
        if pending.is_empty() {
            let _ = self.session.save_to_path(path);
            return;
        }
        let mut session = self.session.clone();
        session.messages.extend(pending.iter().map(|(id, name, _)| {
            ConversationMessage::tool_result(
                id.clone(),
                name.clone(),
                INTERRUPTED_CALL_RESULT.to_string(),
                true,
            )
        }));
        let _ = session.save_to_path(path);
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

    /// Retient l'arrêt demandé par un hook (le premier compte).
    fn note_hook_stop(&mut self, result: &HookRunResult) {
        if self.hook_stop.is_none() {
            self.hook_stop = result.stop_reason().map(ToOwned::to_owned);
        }
    }

    /// Fin du tour demandée par un hook : l'utilisateur est prévenu.
    fn finish_on_hook_stop(&mut self) -> bool {
        let Some(reason) = self.hook_stop.take() else {
            return false;
        };
        self.api_client
            .notify(&format!("Un hook arrête l'agent : {reason}"));
        true
    }

    /// Questions montrées sans attendre (appli web) : le tour s'arrête sur
    /// elles, l'utilisateur y répond d'un toucher au message suivant.
    fn finish_on_deferred_questions(&mut self, aids: &TurnAids) -> bool {
        if aids.questions_deferred {
            self.api_client.notify(
                "Questions posées à l'utilisateur : Claw attend ses réponses, au message suivant",
            );
        }
        aids.questions_deferred
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

    /// Dossier que suit le mode projet, à la place du dossier de travail.
    #[must_use]
    pub fn with_project_root(mut self, root: impl Into<std::path::PathBuf>) -> Self {
        self.project_root = Some(root.into());
        self
    }

    /// Sonde de l'environnement du mode projet (versions, pièges connus).
    #[must_use]
    pub fn with_environment_probe(mut self, enabled: bool) -> Self {
        self.environment = if enabled {
            EnvironmentProbe::Pending
        } else {
            EnvironmentProbe::Off
        };
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
        let mut asked = false;
        let result = compact_session_with(&self.session, compaction, |messages| {
            let transcript = render_transcript_for_summary(messages);
            let first = client.summarize(MODEL_SUMMARY_PROMPT, &transcript)?;
            asked = true;
            if summary_is_usable(&first, messages) {
                return Some(first);
            }
            // Le modèle a continué la conversation au lieu de la résumer : une
            // seconde demande, plus nette ; à défaut, le résumé de Claw.
            client.summarize(
                MODEL_SUMMARY_PROMPT,
                &format!("{transcript}\n\n{SUMMARY_RETRY_NOTE}"),
            )
        });
        let summarized = result.removed_message_count;
        // Le texte du modèle n'était pas un résumé : celui de Claw l'a
        // remplacé, l'utilisateur le sait.
        let by_claw = if asked && summarized > 0 && !result.model_summary {
            " par Claw (le texte du modèle n'était pas un résumé)"
        } else {
            ""
        };
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
            "Contexte compacté automatiquement : {summarized} message(s) résumé(s){by_claw}{truncated_note}, ~{before} → ~{after} jetons"
        );
        self.api_client.notify(&notice);
        true
    }

    /// Un contrôle de Claw n'a rien fait changer et le modèle lui répond en
    /// quelques lignes (« la relecture confirme... ») : ce n'est pas un compte
    /// rendu, or l'utilisateur ne lit que le dernier message. Le compte rendu
    /// précédent (`draft`) est remis devant cette réponse. Vrai si c'est fait.
    fn keep_draft_answer(
        &mut self,
        draft: &str,
        assistant_messages: &mut [ConversationMessage],
    ) -> bool {
        let Some(reply) = assistant_messages.last().map(message_text) else {
            return false;
        };
        let draft = draft.trim();
        let (draft_size, reply_size) = (draft.chars().count(), reply.trim().chars().count());
        if !harness::enabled(Aid::KeptAnswer)
            || draft_size < KEPT_ANSWER_MIN_CHARS
            || reply_size * KEPT_ANSWER_RATIO >= draft_size
        {
            return false;
        }
        for message in [
            self.session.messages.last_mut(),
            assistant_messages.last_mut(),
        ]
        .into_iter()
        .flatten()
        {
            let text = message.blocks.iter_mut().find_map(|block| match block {
                ContentBlock::Text { text } => Some(text),
                _ => None,
            });
            if let Some(text) = text {
                *text = format!("{draft}\n\n{}", text.trim_start());
            }
        }
        true
    }

    /// Demande d'annonce quand la réponse qui appelle des outils n'a rien dit
    /// à l'utilisateur : aux premiers outils du tour, puis après
    /// [`harness::SILENT_STEPS_BEFORE_UPDATE`] étapes muettes d'affilée.
    /// Jamais pour un sous-agent (personne ne le lit). L'annonce elle-même se
    /// voit : aucun avis de plus.
    fn announce_note(
        &mut self,
        aids: &mut TurnAids,
        assistant_messages: &[ConversationMessage],
    ) -> Option<String> {
        if self.subagent || !announcements_on() || !harness::enabled(Aid::Announce) {
            return None;
        }
        let said = assistant_messages
            .last()
            .map(message_text)
            .unwrap_or_default();
        // Quelques mots suffisent ; rien, ou un mot isolé, ne dit rien du plan.
        if said.split_whitespace().count() >= 4 {
            aids.announce_checked = true;
            aids.silent_steps = 0;
            return None;
        }
        aids.silent_steps += 1;
        let note = if !aids.announce_checked {
            harness::ANNOUNCE_NOTE.to_string()
        } else if aids.silent_steps >= harness::SILENT_STEPS_BEFORE_UPDATE {
            harness::update_note(aids.silent_steps)
        } else {
            return None;
        };
        aids.announce_checked = true;
        aids.silent_steps = 0;
        Some(format!(
            "{note}{}",
            harness::language_note(self.response_language.as_deref())
        ))
    }

    /// Plus aucun modèle ne répond après un contrôle de Claw : les notes de
    /// Claw envoyées depuis la réponse sont retirées (la session finit sur la
    /// réponse, comme un tour ordinaire) et la réponse est rouverte.
    fn keep_answer_after_error(&mut self, error: &str) {
        while self.session.messages.last().is_some_and(|message| {
            message.role == crate::session::MessageRole::User
                && message
                    .blocks
                    .iter()
                    .all(|block| matches!(block, ContentBlock::Text { .. }))
        }) {
            self.session.messages.pop();
        }
        self.api_client.answer_kept();
        self.api_client.notify(&format!(
            "Le modèle n'a pas pu reprendre après un contrôle de Claw ({}) : la réponse déjà donnée reste celle du tour",
            short_error(error)
        ));
    }

    /// La suite d'une réponse coupée par la limite de sortie complète cette
    /// réponse : la relance de Claw est retirée et les deux parties ne font
    /// plus qu'un message, celui que l'utilisateur et les contrôles lisent.
    fn join_cut_reply(
        &mut self,
        continuation: ConversationMessage,
        assistant_messages: &mut Vec<ConversationMessage>,
    ) -> ConversationMessage {
        if !after_cut_nudge(&self.session.messages) {
            return continuation;
        }
        self.session.messages.pop();
        let Some(mut joined) = self.session.messages.pop() else {
            return continuation;
        };
        assistant_messages.pop();
        for block in continuation.blocks {
            match block {
                // Le raisonnement de la suite n'est pas gardé : il ne porte
                // que sur la fin d'une réponse déjà commencée.
                ContentBlock::Thinking { .. } => {}
                // Le texte rejoint le dernier texte de la partie coupée, même
                // quand un raisonnement le suit (modèles d'OpenRouter) : une
                // seule réponse, sans saut de ligne ajouté à la coupure.
                ContentBlock::Text { text: more } => {
                    let last_text = joined
                        .blocks
                        .iter_mut()
                        .rev()
                        .find_map(|block| match block {
                            ContentBlock::Text { text } => Some(text),
                            _ => None,
                        });
                    match last_text {
                        Some(text) => text.push_str(&more),
                        None => joined.blocks.push(ContentBlock::Text { text: more }),
                    }
                }
                other => joined.blocks.push(other),
            }
        }
        // Les jetons des deux requêtes restent comptés.
        joined.usage = match (joined.usage, continuation.usage) {
            (Some(cut), Some(more)) => Some(TokenUsage {
                input_tokens: cut.input_tokens.saturating_add(more.input_tokens),
                output_tokens: cut.output_tokens.saturating_add(more.output_tokens),
                cache_creation_input_tokens: cut
                    .cache_creation_input_tokens
                    .saturating_add(more.cache_creation_input_tokens),
                cache_read_input_tokens: cut
                    .cache_read_input_tokens
                    .saturating_add(more.cache_read_input_tokens),
            }),
            (cut, more) => more.or(cut),
        };
        joined
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
        let user_input = user_input.into();
        let started = std::time::Instant::now();
        if !self.subagent {
            crate::telemetry::user_prompt(&user_input);
        }
        let result = self.run_turn_inner(user_input, images, prompter);
        if !self.subagent {
            crate::telemetry::active_time(started.elapsed());
        }
        // Après une escalade, le tour suivant repart avec le modèle habituel.
        self.api_client.restore_model();
        // Fin du tour, réussi ou non : ce qui a été fait est enregistré.
        self.autosave(&[]);
        // Les outils accordés par un skill ne valent que pour ce tour.
        if !self.subagent {
            crate::permissions::clear_turn_grants();
        }
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
        // L'utilisateur vient d'écrire : il est là, ses autorisations lui
        // seront de nouveau demandées.
        if !self.subagent {
            crate::permissions::set_user_away(false);
        }
        // L'utilisateur a pu modifier des fichiers depuis le tour précédent.
        self.drop_stale_file_views();
        let request_text = user_input.clone();
        // « reprendre » : la relecture doit connaître la demande poursuivie.
        let earlier = earlier_request(&self.session.messages, &request_text);
        let mut user_message = self.user_message_with_hooks(user_input)?;
        let image_count = images.len();
        let guided = !self.subagent && harness::enabled(Aid::ProblemGuide);
        // Défauts que le lecteur d'images voit dans les captures jointes (un
        // modèle qui lit lui-même les images n'en fait rien décrire).
        let defects = if guided && image_count > 0 {
            self.api_client.image_defects(&images)
        } else {
            Vec::new()
        };
        user_message
            .blocks
            .extend(images.into_iter().map(|image| ContentBlock::Image {
                media_type: image.media_type,
                data: image.data,
            }));
        // Selon ce que dit l'utilisateur, une méthode jointe à sa demande, pour
        // le modèle : débogage d'un problème signalé (symptômes de chaque
        // image, reproduire, cause prouvée, disparition observée), tri
        // d'images envoyées sans plainte écrite, lecture de chaque image.
        let report = if guided {
            harness::read_report(&request_text, image_count, defects.len())
        } else {
            harness::Report::None
        };
        let guide = match report {
            harness::Report::Problem => Some(harness::problem_guide(image_count, &defects)),
            harness::Report::Images => Some(harness::triage_guide(image_count, &defects)),
            harness::Report::None if guided && image_count > 0 => {
                Some(harness::image_guide(image_count))
            }
            harness::Report::None => None,
        };
        if let Some(text) = guide {
            user_message.blocks.push(ContentBlock::Text { text });
        }
        // Liste de tâches d'un tour précédent restée ouverte : la liste de ce
        // tour se refait à partir du nouveau message. Une relance courte
        // (« continue ») poursuit l'ancienne liste : rien à dire.
        if !self.subagent && earlier.is_none() && harness::enabled(Aid::OpenTasks) {
            let earlier_open = self
                .session
                .messages
                .iter()
                .rev()
                .flat_map(|message| message.blocks.iter().rev())
                .find_map(|block| match block {
                    ContentBlock::ToolUse { name, input, .. } if name == "TodoWrite" => {
                        Some(harness::open_tasks_in_input(input))
                    }
                    _ => None,
                })
                .unwrap_or_default();
            if !earlier_open.is_empty() {
                user_message.blocks.push(ContentBlock::Text {
                    text: harness::fresh_tasks_note(&earlier_open),
                });
            }
        }
        // Question d'information (« comment marche le formulaire ? ») : la
        // réponse est le travail du tour. Ni méthode de travail, ni mode
        // projet, ni Ultracode, et les fichiers restent tels quels. Vu le
        // 22 octobre 2026 : à une simple question, le modèle reprenait le
        // jalon suivant du projet sans y répondre.
        let question = !self.subagent
            && report == harness::Report::None
            && harness::enabled(Aid::WorkQuestions)
            && {
                let words = project::own_words(&request_text);
                harness::asks_a_question(words) && !harness::asks_about_the_work(words)
            };
        self.question_turn = question;
        if question {
            user_message.blocks.push(ContentBlock::Text {
                text: format!(
                    "{}{}",
                    harness::QUESTION_NOTE,
                    harness::language_note(self.response_language.as_deref())
                ),
            });
            self.api_client
                .notify("Question : Claw y répond sans modifier les fichiers");
        }
        // Ce qu'un ingénieur vérifie d'office pour ce genre de fonctionnalité.
        if !self.subagent && !question {
            if let Some(text) = harness::edge_case_guide(&request_text, earlier.as_deref()) {
                user_message.blocks.push(ContentBlock::Text { text });
            }
        }
        // Interface web : la méthode d'un studio de design (plan avant le
        // code, marques d'une page générée, plancher de qualité, vérification).
        let mut design_guided = false;
        if !self.subagent
            && !question
            && harness::enabled(Aid::DesignGuide)
            && report != harness::Report::Problem
        {
            if let Some(text) = crate::web_design::design_guide(&request_text, earlier.as_deref()) {
                user_message.blocks.push(ContentBlock::Text { text });
                design_guided = true;
                // Le modèle réglé pour le design (`designModel`) fait le tour ;
                // le modèle habituel reprend à la fin.
                if let Some(model) = self.api_client.design_model() {
                    self.api_client.notify(&format!(
                        "Tour de design confié à {model} (designModel, mesuré par /models bench --design)"
                    ));
                }
            }
        }
        // Code : la méthode d'un ingénieur soigneux et les règles des langages
        // que la demande nomme (pas pour une interface, qui a la sienne).
        let mut code_languages = std::collections::BTreeSet::new();
        if !self.subagent && !question && !design_guided && harness::enabled(Aid::CodeGuide) {
            if let Some((text, languages)) =
                crate::code_guide::code_guide(&request_text, earlier.as_deref())
            {
                user_message.blocks.push(ContentBlock::Text { text });
                code_languages.extend(languages);
            }
        }
        // Petit projet : tous ses fichiers en vues, au premier tour de la session.
        if !self.subagent && self.session.messages.is_empty() && harness::enabled(Aid::SmallProject)
        {
            if let Some(text) = self.small_project_views() {
                user_message.blocks.push(ContentBlock::Text { text });
            }
        }
        // Mode projet : où en est le projet, et l'étape à faire.
        let project_turn = if question {
            None
        } else {
            self.start_project_turn(&request_text, earlier.as_deref(), &mut user_message)
        };
        self.session.messages.push(user_message);

        let mut assistant_messages = Vec::new();
        let mut tool_results = Vec::new();
        let mut iterations = 0;
        let mut stop_continuations = 0;
        let mut empty_nudged = false;
        let mut cut_continuations = 0;
        // Le budget de requêtes du tour est atteint : le modèle conclut.
        let mut wrapping_up = false;
        // La conclusion est demandée par l'heure (`CLAW_DEADLINE`), pas par le budget.
        let mut out_of_time = false;
        // Réponse qu'un contrôle vient de rendre provisoire, tant que le
        // modèle n'a rien changé depuis.
        let mut superseded_draft: Option<String> = None;
        let team = !self.subagent && crate::team::team();
        let mut aids = TurnAids {
            ultracode: !self.subagent && !question && crate::workflow::active_for(&request_text),
            team,
            topic: team.then(|| crate::team::topic_of(&request_text)).flatten(),
            report,
            saw_defects: !defects.is_empty(),
            snapshot: (!self.subagent
                && harness::enabled(Aid::Verify)
                && self.verify != harness::VerifySetting::Off)
                .then(|| std::env::current_dir().ok())
                .flatten()
                .and_then(|cwd| harness::project_snapshot(&cwd)),
            data_snapshot: (!self.subagent && harness::enabled(Aid::DataGuard))
                .then(|| std::env::current_dir().ok())
                .flatten()
                .and_then(|cwd| harness::DataSnapshot::take(&cwd)),
            project: project_turn,
            design_guided,
            // Les mots de l'utilisateur seulement : Claw Web y ajoute la
            // conversation et ses consignes (« created from projet-vierge »),
            // qui faisaient passer le passage 5 de la session #31 pour une
            // création.
            repair: crate::web_design::is_repair_request(crate::project::own_words(&request_text)),
            code_languages,
            ..TurnAids::default()
        };
        // Tour de design : plan, code, captures et revues dépassent le budget
        // ordinaire ; il grandit, sauf si l'utilisateur l'a réglé lui-même.
        let mut turn_limit = crate::web_design::turn_request_budget(
            design_guided,
            self.turn_request_limit,
            harness::TURN_REQUEST_LIMIT,
            std::env::var_os("CLAW_TURN_REQUESTS").is_some(),
        );
        // Mode projet : un tour long est normal, le plan en jalons fait le point.
        if aids.project.is_some() {
            aids.stall.skip_budget();
        }
        // Ultracode : un agent au contexte vierge étudie la demande avant le
        // travail ; à la fin, un vérificateur indépendant remplace le contrôle
        // des exigences que le modèle faisait sur son propre travail.
        // Question sur le travail (« tu fais quoi ? ») : la première requête
        // part sans outils, et sa réponse termine le tour.
        if !self.subagent
            && harness::enabled(Aid::WorkQuestions)
            && harness::asks_about_the_work(crate::project::own_words(&request_text))
        {
            aids.work_question = true;
            aids.announce_due = Some(format!(
                "{}{}",
                harness::WORK_QUESTION_NOTE,
                harness::language_note(self.response_language.as_deref())
            ));
        }
        aids.checklisted = aids.ultracode;
        if aids.ultracode {
            self.explore_stage(&request_text, earlier.as_deref(), &mut aids, &mut prompter);
        }
        // Tour qui touche à une page déjà là : son état avant le premier
        // changement, pour que les revues nomment ce que le tour casse.
        let interface_turn = aids.design_guided
            || crate::web_design::is_interface_request(&request_text)
            || earlier
                .as_deref()
                .is_some_and(crate::web_design::is_interface_request);
        if interface_turn && !aids.work_question {
            aids.shared_request = crate::web_design::shared_between_people(
                &crate::project::user_words_with_history(&request_text),
            );
            self.baseline_audit(&mut aids, &mut prompter);
        }

        loop {
            iterations += 1;
            aids.requests = iterations;
            if iterations > self.max_iterations {
                return Err(RuntimeError::new(
                    "la boucle de conversation a dépassé le nombre maximal d'itérations",
                ));
            }

            if interrupt_requested() {
                return Err(RuntimeError::new(INTERRUPTED_MESSAGE));
            }
            // Le temps du passage s'achève (Claw Web) : le modèle conclut avant que
            // le job ne soit coupé, quelles que soient les requêtes qui restent.
            if !wrapping_up && !self.subagent && harness::deadline_reached(self.deadline) {
                wrapping_up = true;
                out_of_time = true;
                self.api_client.notify(
                    "Le temps de ce passage s'achève : Claw demande à l'agent de conclure. Écrivez « continue » pour poursuivre",
                );
                let mut note =
                    format!("{} {}", harness::DEADLINE_NOTE, harness::FINAL_ANSWER_SHAPE);
                if aids.project.is_some() {
                    note.push_str(project::WRAP_UP_NOTE);
                }
                note.push_str(&self.wrap_up_measure(&mut aids, &mut prompter));
                note.push_str(&harness::language_note(self.response_language.as_deref()));
                self.session
                    .messages
                    .push(ConversationMessage::user_text(note));
            }
            // Tour de design resté sous la barre, dont la note monte : la
            // limite recule, au lieu d'attendre un « continue ».
            if !wrapping_up
                && turn_limit > 0
                && iterations > turn_limit
                && (self.continue_function(&mut aids, "limite du tour")
                    || (aids.design_guided && self.continue_design(&mut aids, "limite du tour")))
            {
                turn_limit += crate::web_design::CONTINUATION_REQUESTS;
            }
            // Liste de tâches pas finie et travail qui avance : la limite
            // recule d'un palier, au lieu de s'arrêter à 0/9 (demandé le 25
            // octobre 2026 : « Faut l'interdire de s'arrêter s'il n'a pas fini
            // les tâches »).
            if !wrapping_up
                && turn_limit > 0
                && iterations > turn_limit
                && self.continue_tasks(&mut aids)
            {
                turn_limit += harness::TASK_EXTENSION_REQUESTS;
            }
            // Budget de requêtes du tour atteint : le modèle conclut et rend
            // la main, au lieu de dépenser le quota du jour en un seul tour.
            let limit = turn_limit;
            if !wrapping_up && limit > 0 && iterations > limit {
                wrapping_up = true;
                let plural = if limit > 1 { "s" } else { "" };
                self.api_client.notify(&format!(
                    "Ce tour a utilisé {limit} requête{plural}, la limite d'un tour (réglage `turnRequestLimit`) : Claw demande à l'agent de conclure. Écrivez « continue » pour poursuivre"
                ));
                let mut note = harness::turn_limit_note(limit);
                if aids.project.is_some() {
                    note.push_str(project::WRAP_UP_NOTE);
                }
                note.push_str(&self.wrap_up_measure(&mut aids, &mut prompter));
                note.push_str(&harness::language_note(self.response_language.as_deref()));
                self.session
                    .messages
                    .push(ConversationMessage::user_text(note));
            }
            // Revue visuelle anticipée : le budget du tour s'épuise et
            // l'interface, modifiée, n'a jamais été revue. Le juge parle tant
            // qu'il reste de quoi corriger ; à la limite, la conclusion part
            // sans aucun contrôle.
            // Sans page à capturer (un module `TypeScript`, un script), rien
            // n'est annoncé : vu dans un essai réel, l'annonce revenait à
            // chaque modification d'un `.ts`, sans aucune revue.
            if !wrapping_up
                && limit > 0
                && aids.interface_dirty
                && aids.interface_rounds == 0
                && iterations >= crate::web_design::early_review_at(limit)
                && self.page_to_review(&aids).is_some()
            {
                self.api_client.notify(&format!(
                    "Le tour a utilisé {} requêtes sur {limit} sans revue visuelle : revue maintenant, pour que les corrections tiennent dans le budget",
                    iterations - 1
                ));
                if let Some(feedback) =
                    self.interface_gate(&mut aids, &request_text, earlier.as_deref(), &mut prompter)
                {
                    self.session
                        .messages
                        .push(ConversationMessage::user_text(feedback));
                }
            }
            // L'interface vient de changer : Claw regarde la page avant la
            // requête suivante.
            if let Some((check, _)) = self.finish_visual_check(&mut aids) {
                self.session.messages.push(check);
            }
            if !wrapping_up {
                if let Some(check) = self.visual_check(&mut aids, &mut prompter) {
                    self.session.messages.push(check);
                }
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
            // Le modèle travaille sans un mot pour l'utilisateur : cette requête
            // part sans outils, il ne peut que dire où il en est.
            let announcing = match aids.announce_due.take() {
                Some(note) if !wrapping_up => {
                    self.session
                        .messages
                        .push(ConversationMessage::user_text(note));
                    true
                }
                _ => false,
            };
            self.auto_compact(false);
            self.autosave(&[]);
            // Budget du tour atteint : la conclusion part sans outils. Vu le 25
            // octobre 2026 (session #30) : avec ses outils, le modèle a continué
            // à corriger, chaque appel a été refusé et le tour a fini sans un
            // mot pour l'utilisateur après 280 requêtes.
            let text_only = announcing || wrapping_up;
            let request = ApiRequest {
                system_prompt: self.system_prompt.clone(),
                messages: self.session.messages.clone(),
                text_only,
            };
            let request_started = std::time::Instant::now();
            let events = match self.api_client.stream(request) {
                Ok(events) => events,
                // Contexte trop long malgré l'estimation : on compacte et on réessaie une fois.
                Err(error)
                    if is_context_overflow(&error.to_string()) && self.auto_compact(true) =>
                {
                    self.api_client.stream(ApiRequest {
                        system_prompt: self.system_prompt.clone(),
                        messages: self.session.messages.clone(),
                        text_only,
                    })?
                }
                // L'annonce ne passe pas (fournisseur qui refuse une requête
                // sans outils) : le travail reprend sans elle.
                Err(_) if announcing && !interrupt_requested() => {
                    self.session.messages.pop();
                    continue;
                }
                Err(error) => {
                    crate::telemetry::api_error(&error.to_string(), request_started.elapsed());
                    // Un contrôle de Claw vient de renvoyer au travail un modèle
                    // qui avait fini, et plus aucun modèle ne répond (quota du
                    // jour épuisé, vu dans un essai réel avec Gemini) : la
                    // réponse déjà donnée reste celle du tour, au lieu de le perdre.
                    if superseded_draft.is_some() && !interrupt_requested() {
                        self.keep_answer_after_error(&error.to_string());
                        self.run_stop_hooks(stop_continuations, true, &assistant_messages);
                        break;
                    }
                    return Err(error);
                }
            };
            let (assistant_message, usage) = match build_assistant_message(events) {
                // Une réponse avec du contenu : la relance d'une réponse vide
                // vaut de nouveau. Vu dans un essai réel : deux réponses vides
                // séparées par des dizaines d'autres suffisaient à perdre le tour.
                Ok(built) => {
                    empty_nudged = false;
                    built
                }
                // Annonce restée vide : le travail reprend sans elle.
                Err(error) if announcing && error.to_string() == EMPTY_RESPONSE_ERROR => {
                    self.session.messages.pop();
                    continue;
                }
                // Suite d'une réponse coupée restée vide (souvent tout le
                // budget pris par le raisonnement) : la même demande repart une
                // fois, puis la réponse coupée est rendue telle quelle.
                Err(error)
                    if error.to_string() == EMPTY_RESPONSE_ERROR
                        && after_cut_nudge(&self.session.messages) =>
                {
                    if !empty_nudged {
                        empty_nudged = true;
                        continue;
                    }
                    self.api_client.notify(
                        "La suite de la réponse coupée n'est pas venue : elle est rendue telle quelle",
                    );
                    (ConversationMessage::assistant(Vec::new()), None)
                }
                Err(error) if error.to_string() == EMPTY_RESPONSE_ERROR && !empty_nudged => {
                    empty_nudged = true;
                    self.session
                        .messages
                        .push(ConversationMessage::user_text(EMPTY_RESPONSE_NUDGE));
                    continue;
                }
                // Vide encore après la relance : un autre modèle reprend la fin
                // du tour au lieu de le perdre. Vu dans un essai réel : dots-3,
                // 594 s de raisonnement puis rien, et le site laissé à mi-chemin.
                // Une escalade à part de celle d'un agent bloqué : déjà faite
                // plus tôt dans le tour, elle ne doit pas laisser le tour se perdre.
                Err(error)
                    if error.to_string() == EMPTY_RESPONSE_ERROR
                        && !aids.empty_escalated
                        && !self.subagent
                        && harness::enabled(Aid::Escalation) =>
                {
                    aids.empty_escalated = true;
                    let Some(model) = self.api_client.escalate() else {
                        return Err(error);
                    };
                    self.api_client.notify(&format!(
                        "Le modèle n'a rien répondu deux fois : {model} reprend la fin du tour"
                    ));
                    self.session.messages.push(ConversationMessage::user_text(
                        harness::escalation_feedback(&model),
                    ));
                    continue;
                }
                Err(error) => return Err(error),
            };
            if let Some(usage) = usage {
                self.usage_tracker.record(usage);
                crate::telemetry::api_request(&usage, request_started.elapsed(), self.subagent);
            }
            // Suite d'une réponse coupée : elle complète le message coupé.
            let assistant_message = self.join_cut_reply(assistant_message, &mut assistant_messages);
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

            // L'annonce est faite (texte seul) : ce n'est pas la réponse du
            // tour, le travail reprend avec les outils.
            if announcing && pending_tool_uses.is_empty() && !aids.work_question {
                self.session.messages.push(ConversationMessage::user_text(
                    harness::GO_ON_NOTE.to_string(),
                ));
                continue;
            }

            if pending_tool_uses.is_empty() {
                aids.single_call_streak = 0;
                // Réponse en texte arrêtée par la limite de sortie : Claw en
                // demande la suite (deux fois au plus pour une même réponse,
                // jamais au-delà des étapes permises) au lieu de rendre un
                // texte tronqué.
                let cut = self.api_client.reply_cut_by_output_limit()
                    && harness::enabled(Aid::CutReplies);
                if cut
                    && cut_continuations < MAX_CUT_CONTINUATIONS
                    && iterations < self.max_iterations
                    && !wrapping_up
                {
                    cut_continuations += 1;
                    self.api_client.notify(
                        "Réponse coupée par la limite de sortie du modèle : Claw demande la suite",
                    );
                    self.session
                        .messages
                        .push(ConversationMessage::user_text(CUT_REPLY_NUDGE));
                    continue;
                }
                cut_continuations = 0;
                // Accusé de réception d'un contrôle : le compte rendu
                // précédent reste la réponse.
                let kept = superseded_draft
                    .take()
                    .is_some_and(|draft| self.keep_draft_answer(&draft, &mut assistant_messages));
                if kept {
                    self.api_client.answer_kept();
                }
                self.api_client.answer_drafted();
                if cut {
                    self.api_client.notify(
                        "Réponse encore coupée par la limite de sortie du modèle : elle est rendue telle quelle",
                    );
                }
                // Vérification encore en route (lancée après la dernière
                // modification) : elle finit avant tout contrôle de la conclusion.
                let pending_check = self.finish_visual_check(&mut aids);
                // Budget du tour atteint : la conclusion part telle quelle,
                // aucun contrôle ne renvoie le modèle au travail.
                if wrapping_up {
                    self.run_stop_hooks(stop_continuations, true, &assistant_messages);
                    break;
                }
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
                // Ce qu'elle a trouvé de nouveau passe avant la conclusion.
                let checked = pending_check
                    .filter(|(_, new)| *new > 0)
                    .map(|(message, _)| message_text(&message));
                let feedback = checked
                    .or_else(|| self.empty_titles_gate(&mut aids, &answer))
                    .or_else(|| self.announced_work_gate(&mut aids, &answer))
                    .or_else(|| self.persistence_gate(&mut aids))
                    .or_else(|| self.open_tasks_gate(&mut aids, &answer))
                    .or_else(|| self.test_data_gate(&mut aids))
                    .or_else(|| self.verification_gate(&mut aids, &mut prompter))
                    .or_else(|| self.editor_gate(&mut aids))
                    .or_else(|| self.run_scripts_gate(&mut aids))
                    .or_else(|| {
                        self.interface_gate(
                            &mut aids,
                            &request_text,
                            earlier.as_deref(),
                            &mut prompter,
                        )
                    })
                    .or_else(|| self.unchecked_claim_gate(&mut aids, &answer, &mut prompter))
                    .or_else(|| {
                        self.workflow_gate(
                            &mut aids,
                            &request_text,
                            earlier.as_deref(),
                            &answer,
                            &mut prompter,
                        )
                    })
                    .or_else(|| self.evidence_gate(&mut aids, &request_text, earlier.as_deref()))
                    .or_else(|| self.project_gate(&mut aids, &answer, &mut prompter))
                    .or_else(|| {
                        self.requirements_gate(&mut aids, &request_text, earlier.as_deref())
                    })
                    .or_else(|| self.data_service_gate(&mut aids, &request_text))
                    .or_else(|| self.shared_data_gate(&mut aids, &request_text))
                    .or_else(|| self.invented_rules_gate(&mut aids, &request_text))
                    .or_else(|| self.access_code_gate(&mut aids, &answer))
                    // Avant la question sur les clés : une question posée dans
                    // Claw Web clôt le passage, sans les contrôles qui suivent.
                    .or_else(|| self.plain_answer_gate(&mut aids, &answer))
                    .or_else(|| self.account_setup_gate(&mut aids))
                    .or_else(|| {
                        self.review_gate(&mut aids, &request_text, earlier.as_deref(), &answer)
                    })
                    .or_else(|| self.milestone_validation_gate(&mut aids, &answer))
                    .or_else(|| self.instructions_gate(&mut aids, &answer))
                    .or_else(|| self.unused_photos_gate(&mut aids))
                    .or_else(|| {
                        asking = self.questions_gate(&mut aids, &answer);
                        asking.then(|| harness::QUESTIONS_FEEDBACK.to_string())
                    })
                    .or_else(|| {
                        let reason =
                            self.run_stop_hooks(stop_continuations, false, &assistant_messages)?;
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
                    superseded_draft = Some(answer);
                    // Le travail reprend après la réponse : s'il s'y remet sans un
                    // mot, le modèle redit ce qu'il fait (vu le 20 octobre 2026,
                    // session 15 : des tests relancés en silence après sa réponse).
                    aids.announce_checked = false;
                    aids.silent_steps = 0;
                }
                self.session
                    .messages
                    .push(ConversationMessage::user_text(feedback));
                continue;
            }

            // Une réponse qui appelle des outils est finie : la prochaine
            // réponse coupée a de nouveau droit à ses suites.
            cut_continuations = 0;
            aids.calls_in_reply = pending_tool_uses.len();
            // Le modèle retravaille : sa prochaine réponse sera un nouveau
            // compte rendu, l'ancien n'est plus à garder.
            superseded_draft = None;
            // Encore des outils alors que le budget du tour est atteint : rien
            // ne tourne, chaque appel reçoit son résultat et le tour s'arrête.
            if wrapping_up {
                let reason = if out_of_time {
                    harness::DEADLINE_REFUSAL.to_string()
                } else {
                    harness::turn_limit_refusal(turn_limit)
                };
                for (tool_use_id, tool_name, input) in pending_tool_uses {
                    self.tool_executor.skipped(&tool_name, &input, &reason);
                    let message = ConversationMessage::tool_result(
                        tool_use_id,
                        tool_name,
                        reason.clone(),
                        true,
                    );
                    self.session.messages.push(message.clone());
                    tool_results.push(message);
                }
                self.api_client.notify(
                    "L'agent n'a pas conclu : le tour s'arrête là, sans autre appel d'outil",
                );
                break;
            }
            // Plan de design relu avant le code : à la première écriture d'un
            // fichier d'interface qui suit le plan, une fois par tour.
            let mut plan_note = self.plan_review(
                &pending_tool_uses,
                &assistant_messages,
                &request_text,
                earlier.as_deref(),
                &mut aids,
            );
            // Outils lancés sans un mot pour l'utilisateur : la requête suivante
            // lui fait dire ce qu'il fait (au départ, puis de temps en temps).
            if let Some(note) = self.announce_note(&mut aids, &assistant_messages) {
                aids.announce_due = Some(note);
            }
            self.autosave(&pending_tool_uses);
            if self.run_parallel_batch(
                &pending_tool_uses,
                &mut prompter,
                &mut tool_results,
                &mut aids,
            ) {
                if let Some(note) = plan_note.take() {
                    self.session
                        .messages
                        .push(ConversationMessage::user_text(note));
                }
                if self.finish_on_hook_stop() || self.finish_on_deferred_questions(&aids) {
                    break;
                }
                continue;
            }
            for (tool_use_id, tool_name, input) in pending_tool_uses {
                // Après une interruption, chaque appel restant reçoit un résultat :
                // un appel d'outil sans résultat rendrait la conversation invalide.
                if interrupt_requested() || self.hook_stop.is_some() {
                    let reason = if self.hook_stop.is_some() {
                        HOOK_STOP_MESSAGE
                    } else {
                        INTERRUPTED_MESSAGE
                    };
                    self.tool_executor.skipped(&tool_name, &input, reason);
                    let message = ConversationMessage::tool_result(
                        tool_use_id,
                        tool_name,
                        reason.to_string(),
                        true,
                    );
                    self.session.messages.push(message.clone());
                    tool_results.push(message);
                    continue;
                }
                remember_turn_start(&tool_name, &input, &mut aids);
                let (mut result_message, refused) = self.execute_tool_call(
                    tool_use_id,
                    tool_name.clone(),
                    &input,
                    prompter
                        .as_mut()
                        .map(|prompt| &mut **prompt as &mut dyn PermissionPrompter),
                );
                aids.refused |= refused;
                self.after_tool(&tool_name, &input, &mut result_message, refused, &mut aids);
                if let Some(note) = plan_note.take() {
                    append_tool_output(&mut result_message, &note);
                }
                lean_result(&tool_name, &mut result_message);
                self.session.messages.push(result_message.clone());
                tool_results.push(result_message);
                // Plan approuvé (ExitPlanMode) : la suite du tour peut modifier.
                if let Some(mode) = crate::permissions::take_mode_switch() {
                    self.permission_policy.set_active_mode(mode);
                }
            }
            self.attach_tool_images(&mut tool_results);
            if self.finish_on_hook_stop() || self.finish_on_deferred_questions(&aids) {
                break;
            }
            // Mode projet : un jalon marqué fait se vérifie tout de suite.
            if let Some(note) = self.project_after_tools(&mut aids, &mut prompter) {
                if let Some(last) = self.session.messages.last_mut() {
                    append_tool_output(last, &note);
                }
                if let Some(last) = tool_results.last_mut() {
                    append_tool_output(last, &note);
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

    /// Arguments, hook `PreToolUse` puis autorisation d'un appel d'un lot :
    /// les messages du hook et les arguments à utiliser (remplacés par un hook
    /// avec `updatedInput`) s'il peut être exécuté, sinon la raison rendue au
    /// modèle, et vrai si c'est un refus (hook ou autorisation).
    fn admit_tool_call(
        &mut self,
        name: &str,
        input: &str,
        prompter: &mut Option<&mut dyn PermissionPrompter>,
    ) -> Result<(Vec<String>, Option<String>), (String, bool)> {
        if let Some(problem) = self.tool_executor.invalid_input(name, input) {
            return Err((problem, false));
        }
        let pre = self.hook_runner.run_pre_tool_use(name, input);
        if pre.is_denied() {
            let fallback = format!("hook PreToolUse : outil `{name}` refusé");
            return Err((format_hook_message(&pre, &fallback), true));
        }
        self.note_hook_stop(&pre);
        let updated = pre.updated_input().map(ToOwned::to_owned);
        let effective = updated.as_deref().unwrap_or(input);
        let mut notifying = prompter.as_mut().map(|prompt| NotifyingPrompter {
            hooks: &self.hook_runner,
            inner: &mut **prompt,
        });
        let outcome = authorize_after_hook(
            &mut self.permission_policy,
            pre.permission_decision(),
            name,
            effective,
            notifying
                .as_mut()
                .map(|prompt| prompt as &mut dyn PermissionPrompter),
        );
        if let PermissionOutcome::Deny { reason } = outcome {
            return Err((reason, true));
        }
        Ok((pre.messages().to_vec(), updated))
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
        let mut refused_calls = vec![false; calls.len()];
        let mut ready = Vec::new();
        let mut pre_messages = Vec::new();
        let mut inputs: Vec<String> = calls.iter().map(|(_, _, input)| input.clone()).collect();
        for (index, (id, name, input)) in calls.iter().enumerate() {
            match self.admit_tool_call(name, input, prompter) {
                Ok((messages, updated)) => {
                    if let Some(updated) = updated {
                        inputs[index] = updated;
                    }
                    ready.push(index);
                    pre_messages.push(messages);
                }
                Err((reason, refused)) => {
                    aids.refused |= refused;
                    refused_calls[index] = refused;
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
            .map(|index| (calls[*index].1.clone(), inputs[*index].clone()))
            .collect::<Vec<_>>();
        let outputs = if batch.is_empty() {
            Vec::new()
        } else {
            self.tool_executor.execute_batch(&batch)
        };
        // Les images des appels du lot (photos de `Photo`) suivent leurs résultats.
        self.tool_images.extend(self.tool_executor.take_images());
        // 3. Hooks PostToolUse et résultats, dans l'ordre d'origine.
        for ((index, outcome), pre) in ready.iter().zip(outputs).zip(pre_messages) {
            let (id, name, _) = &calls[*index];
            let input = &inputs[*index];
            let (mut output, mut is_error) = match outcome {
                Ok(output) => (output, false),
                Err(error) => (error.to_string(), true),
            };
            output = merge_hook_feedback(&pre, output, false);
            let post = self
                .hook_runner
                .run_post_tool_use(name, input, &output, is_error);
            self.note_hook_stop(&post);
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
        for ((mut message, refused), (_, name, input)) in results
            .into_iter()
            .zip(refused_calls)
            .zip(calls)
            .filter_map(|((message, refused), call)| {
                message.map(|message| ((message, refused), call))
            })
        {
            self.after_tool(name, input, &mut message, refused, aids);
            lean_result(name, &mut message);
            self.session.messages.push(message.clone());
            tool_results.push(message);
        }
        true
    }

    /// Après un appel d'outil : contrôle de syntaxe du fichier écrit, suivi
    /// des modifications à vérifier, détection de blocage. Les remarques sont
    /// jointes au résultat que lira le modèle. `refused` : l'appel n'a pas
    /// tourné (refus, demande d'autorisation sans réponse, hook).
    #[allow(clippy::too_many_lines)]
    fn after_tool(
        &mut self,
        tool_name: &str,
        input: &str,
        message: &mut ConversationMessage,
        refused: bool,
        aids: &mut TurnAids,
    ) {
        let Some((output, is_error)) = tool_result_parts(message) else {
            return;
        };
        crate::telemetry::tool_result(tool_name, input, !is_error && !refused);
        let output = output.to_string();
        // Ni l'échec d'un outil ni le résultat d'une commande : rien ne dit
        // que le modèle tourne en rond.
        if refused {
            self.note_not_run(&output, aids);
            return;
        }
        if crate::file_tools::writes_files(tool_name) || matches!(tool_name, "bash" | "PowerShell")
        {
            aids.project_changes += 1;
        }
        let mut notes = Vec::new();
        notes.extend(self.tool_executor.argument_note(tool_name, input));
        // SVG récupérés (téléchargés, lus dans un paquet, donnés par un outil) :
        // ce ne sont pas des dessins à la main.
        if crate::file_tools::writes_files(tool_name) {
            if let Some(path) = json_field(input, "path").filter(|path| {
                std::path::Path::new(path)
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("svg"))
            }) {
                aids.svg_written.insert(path);
            }
        } else if !is_error && fetches_svg(tool_name, input, aids) {
            crate::icon_set::learn_fetched(&output);
        }
        // Photos enregistrées : la fin du tour vérifie que la page s'en sert.
        if tool_name == "Photo" && !is_error {
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(&output) {
                for photo in value["photos"].as_array().into_iter().flatten() {
                    if let Some(path) = photo["path"].as_str() {
                        aids.photos.insert(path.to_string());
                    }
                }
            }
        }
        // Questions déjà posées avec le menu : rien à redemander en fin de tour.
        aids.asked |= tool_name == "AskUserQuestion";
        aids.questions_deferred |=
            tool_name == "AskUserQuestion" && output.contains(harness::QUESTIONS_DEFERRED);
        // Page que le modèle a regardée lui-même : la revue visuelle la reprend.
        if matches!(tool_name, "Screenshot" | "ViewPage" | "Browser") && !is_error {
            if let Some(target) = crate::web_design::page_target(input) {
                self.page_target = Some(target);
            }
            aids.blind_edits = 0;
        }
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
        // Un seul appel par réponse (observations à la suite, liste de tâches
        // seule) : ce qui est déjà prévu se regroupe.
        notes.extend(lone_call_note(tool_name, input, &output, is_error, aids));
        if tool_name == "TodoWrite" && !is_error {
            if let Some(open) = harness::open_todos(&output) {
                aids.open_tasks = open;
            }
        }
        notes.extend(self.style_search_note(tool_name, input, aids));
        if tool_name == "read_file" && !is_error {
            notes.extend(self.repeated_read_note(&output, aids));
            notes.extend(self.viewed_read_note(&output, aids));
        }
        // Règles `.claude/rules` liées à ce chemin et instructions du
        // sous-dossier, données la première fois que le fichier est touché.
        if !is_error {
            notes.extend(path_instruction_notes(tool_name, input));
        }
        if matches!(
            tool_name,
            "edit_file" | "write_file" | "MultiEdit" | "NotebookEdit"
        ) && !is_error
        {
            self.after_file_change(tool_name, input, &output, aids, &mut notes);
        } else if crate::file_tools::changes_listed_files(tool_name) && !is_error {
            // Chaque fichier changé, comme après un edit_file sur lui.
            self.drop_stale_file_views();
            for (file_input, file_output) in changed_file_results(&output) {
                self.after_file_change("edit_file", &file_input, &file_output, aids, &mut notes);
            }
        } else if !matches!(
            tool_name,
            "read_file"
                | "grep_search"
                | "glob_search"
                | "list_directory"
                | "read_many_files"
                | "file_outline"
                | "git"
                | "http_request"
        ) {
            // Une commande ou un autre outil a pu changer des fichiers.
            self.drop_stale_file_views();
        }
        if matches!(tool_name, "bash" | "PowerShell") {
            self.edits_lost_by_command(aids, &mut notes);
        }
        if matches!(tool_name, "bash" | "PowerShell") && !is_error {
            self.data_rewritten_by_command(input, aids, &mut notes);
        }
        // Du PowerShell passé par bash entre guillemets doubles : bash a
        // remplacé ses variables avant que PowerShell ne le lise.
        if tool_name == "bash" && harness::enabled(Aid::Pitfalls) && !aids.quoting_noted {
            if let Some(note) = json_field(input, "command")
                .and_then(|command| crate::code_checks::bash_powershell_quoting(&command))
            {
                aids.quoting_noted = true;
                notes.push(note.to_string());
            }
        }
        if let Some(outcome) = command_outcome {
            aids.commands_run.push(outcome.command.clone());
            if outcome.passed {
                // Un script muet qui définit `main()` sans l'appeler.
                if harness::enabled(Aid::EntryPoint) {
                    if let Ok(cwd) = std::env::current_dir() {
                        if let Some((script, note)) =
                            harness::uncalled_main_note(&outcome.command, &outcome.output, &cwd)
                        {
                            if aids.silent_scripts.insert(script) {
                                notes.push(note);
                            }
                        }
                    }
                }
                if harness::is_check_command(&outcome.command)
                    && harness::exit_reflects_check(&outcome.command)
                {
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
                        notes.extend(harness::error_sites(
                            &outcome.output,
                            &cwd,
                            &outcome.command,
                        ));
                    }
                }
                // Sous Windows : un chemin `dossier\fichier` écrit pour cmd.
                if tool_name == "bash" && harness::enabled(Aid::PathHelp) {
                    let hint = crate::bash::windows_shell_hint(&outcome.command);
                    notes.extend(hint.map(str::to_string));
                }
                // Un outil de Claw écrit comme une commande (`KillShell bash_4`).
                notes.extend(harness::tool_named_in_command(&outcome.command));
                // Python du Microsoft Store : il ne voit pas `/tmp` de Git Bash.
                if harness::enabled(Aid::PathHelp) {
                    notes.extend(harness::store_python_hint(&outcome.output));
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

    /// Une commande a supprimé un fichier que le modèle avait modifié pendant le
    /// tour, ou l'a ramené à son état du début du tour : ses modifications sont
    /// perdues. Essai réel t6 avec le petit modèle : `sed ... && rm conges.py`,
    /// puis `git checkout conges.py`, deux fois, et tout son travail à refaire.
    /// Claw garde la dernière version et le dit, sans la remettre à sa place
    /// (une suppression peut être voulue).
    fn edits_lost_by_command(&mut self, aids: &mut TurnAids, notes: &mut Vec<String>) {
        if aids.edited_files.is_empty() {
            return;
        }
        let Ok(cwd) = std::env::current_dir() else {
            return;
        };
        let mut lost = Vec::new();
        for (path, last) in &mut aids.edited_files {
            let start = aids.turn_start_files.get(path).cloned().flatten();
            match std::fs::read_to_string(path) {
                // Illisible mais présent, ou créé pendant le tour puis retiré
                // (un fichier de travail) : rien à dire.
                Err(_) if path.exists() || start.is_none() => {}
                Err(_) => lost.push((path.clone(), last.clone(), true)),
                Ok(now) if same_text(&now, last) => {}
                Ok(now) if start.as_deref().is_some_and(|start| same_text(&now, start)) => {
                    lost.push((path.clone(), last.clone(), false));
                }
                // Changé autrement (formateur, `sed -i`) : cette version compte.
                Ok(now) => *last = now,
            }
        }
        for (path, last, deleted) in lost {
            aids.edited_files.remove(&path);
            let kept = harness::kept_copy_path(&cwd, &path);
            let saved = kept
                .parent()
                .is_some_and(|folder| std::fs::create_dir_all(folder).is_ok())
                && std::fs::write(&kept, &last).is_ok();
            let shown = harness::shown_path(&path.to_string_lossy());
            let kept_shown = saved.then(|| harness::shown_path(&kept.to_string_lossy()));
            let event = if deleted {
                format!("a supprimé {shown}, que le modèle avait modifié pendant ce tour")
            } else {
                format!("a ramené {shown} à son état du début du tour, effaçant les modifications du modèle")
            };
            let copy = kept_shown.as_deref().map_or_else(
                || "Claw n'a pas pu en garder une copie".to_string(),
                |kept| format!("sa dernière version est gardée dans {kept}"),
            );
            self.api_client
                .notify(&format!("Attention : une commande {event} ; {copy}"));
            notes.push(harness::lost_edits_note(
                &shown,
                kept_shown.as_deref(),
                deleted,
            ));
        }
    }

    /// Une commande du modèle a réécrit des lignes d'un fichier de données qui
    /// existait au début du tour (essai réel t3 : le CSV « réparé » de mémoire) :
    /// même garde que pour `edit_file`, une fois par fichier.
    fn data_rewritten_by_command(
        &mut self,
        input: &str,
        aids: &mut TurnAids,
        notes: &mut Vec<String>,
    ) {
        let (Some(snapshot), Ok(cwd)) = (aids.data_snapshot.as_mut(), std::env::current_dir())
        else {
            return;
        };
        let command = json_field(input, "command").unwrap_or_default();
        for (path, replaced) in snapshot.rewritten(&cwd) {
            if !aids.guarded_data.insert(path.clone()) {
                continue;
            }
            let shown = harness::shown_path(&path);
            self.api_client.notify(&format!(
                "Attention : une commande a modifié ou retiré des lignes de {shown}, un fichier de données : relisez ce changement"
            ));
            notes.push(harness::command_data_guard_feedback(
                &shown, &replaced, &command,
            ));
        }
    }

    /// Suites d'une modification de fichier réussie : état du tour, garde des
    /// tests et des données réécrits, fichier montré tel qu'il est, syntaxe.
    fn after_file_change(
        &mut self,
        tool_name: &str,
        input: &str,
        output: &str,
        aids: &mut TurnAids,
        notes: &mut Vec<String>,
    ) {
        // La version que le modèle vient d'écrire : une commande peut la perdre.
        if harness::enabled(Aid::LostEdits) {
            if let Some(path) = edited_path(input) {
                if let Ok(text) = std::fs::read_to_string(&path) {
                    aids.edited_files.insert(path, text);
                }
            }
        }
        aids.dirty = true;
        aids.editor_dirty = true;
        aids.workflow_dirty = true;
        aids.edited_since_plain = true;
        if json_field(input, "path").is_some_and(|path| crate::web_design::is_interface_file(&path))
        {
            aids.interface_dirty = true;
            aids.blind_edits += 1;
        }
        aids.feedback_pending = false;
        aids.verified_commands.clear();
        // Une commande encore en cours a été lancée sur l'ancien code.
        aids.background_commands.clear();
        let path = json_field(output, "filePath").or_else(|| json_field(input, "path"));
        // Une modification du modèle, contrôlée ci-dessous : pas celle d'une commande.
        if let (Some(path), Some(snapshot), Ok(cwd)) =
            (&path, aids.data_snapshot.as_mut(), std::env::current_dir())
        {
            snapshot.refresh(&cwd, path);
        }
        if let Some(path) = &path {
            if !aids.originals.contains_key(path) {
                aids.originals.insert(
                    path.clone(),
                    harness::original_content(output).unwrap_or_default(),
                );
            }
        }
        if let Some(path) = path.as_deref().filter(|_| aids.check_failed) {
            if let Some(name) = self.rewritten_check(path, input, output, aids) {
                self.api_client.notify(&format!(
                    "Attention : {name} sert à vérifier le travail et a été modifié après un échec : relisez ce changement"
                ));
                notes.push(harness::test_guard_feedback(&name));
            }
        }
        if let Some(note) = self.json_data_reshaped(input, aids) {
            notes.push(note);
        }
        if let Some((shown, replaced)) = path
            .as_deref()
            .and_then(|path| Self::rewritten_data(path, output, aids))
        {
            self.api_client.notify(&format!(
                "Attention : des lignes de {shown}, un fichier de données, ont été modifiées ou retirées : relisez ce changement"
            ));
            notes.push(harness::data_guard_feedback(&shown, &replaced));
        }
        // Le fichier tel qu'il est maintenant : une relecture de moins, et
        // une seule vue à jour par fichier dans la conversation.
        if matches!(tool_name, "edit_file" | "write_file" | "MultiEdit")
            && harness::enabled(Aid::FileView)
        {
            if let Some(marker) = harness::file_view_marker(output) {
                replace_file_views(&mut self.session, &marker, harness::FILE_VIEW_REPLACED);
                self.file_views.remove(&marker);
                if let Some(view) = harness::view_after_edit(tool_name, output) {
                    let stamp = path
                        .as_deref()
                        .and_then(|path| file_stamp(std::path::Path::new(path)));
                    if let Some(stamp) =
                        stamp.filter(|_| view.starts_with(harness::FILE_VIEW_PREFIX))
                    {
                        self.file_views.insert(marker, stamp);
                    }
                    notes.push(view);
                }
            }
        }
        if let Some(path) = path.filter(|_| harness::enabled(Aid::Diagnostics)) {
            let name = std::path::Path::new(&path)
                .file_name()
                .map_or_else(|| path.clone(), |name| name.to_string_lossy().to_string());
            if let Some(error) = harness::syntax_check(std::path::Path::new(&path)) {
                self.api_client.notify(&format!(
                    "Erreur de syntaxe dans {name} : signalée au modèle"
                ));
                notes.push(harness::syntax_feedback(&path, &error));
            } else if harness::enabled(Aid::Pitfalls) {
                // Les pièges du code, chacun dit une fois par tour : une
                // modification ailleurs dans le fichier ne les répète pas.
                let fresh: Vec<String> = crate::code_checks::pitfalls(std::path::Path::new(&path))
                    .into_iter()
                    .filter(|finding| aids.pitfalls_seen.insert(format!("{path}\u{1}{finding}")))
                    .collect();
                if !fresh.is_empty() {
                    self.api_client.notify(&format!(
                        "Pièges relevés dans {name} : {}, signalés au modèle",
                        fresh.len()
                    ));
                    notes.push(crate::code_checks::pitfalls_feedback(&path, &fresh));
                }
            }
            self.interface_file_notes(&path, aids, notes);
            self.code_file_notes(&path, aids, notes);
        }
    }

    /// Un fichier de code vient d'être écrit : les règles de son langage la
    /// première fois dans le tour, et le script retenu pour être essayé.
    fn code_file_notes(&mut self, path: &str, aids: &mut TurnAids, notes: &mut Vec<String>) {
        let file = std::path::Path::new(path);
        let Some(language) = crate::code_guide::Language::of(file) else {
            return;
        };
        let name = file.file_name().map_or_else(
            || path.to_string(),
            |name| name.to_string_lossy().to_string(),
        );
        // Le JavaScript d'une page suit la méthode de design.
        let page_script = aids.design_guided
            && language == crate::code_guide::Language::JavaScript
            && crate::web_design::is_interface_file(path);
        if harness::enabled(Aid::CodeGuide)
            && !self.subagent
            && !page_script
            && aids.code_languages.insert(language)
        {
            notes.push(crate::code_guide::rules_note(language, &name));
        }
        let text = std::fs::read_to_string(file).unwrap_or_default();
        if !page_script && crate::code_guide::is_runnable_script(file, &text) {
            aids.scripts_written.insert(path.to_string());
        }
    }

    /// Contrôles d'un fichier d'interface qui vient d'être écrit, dits tout de
    /// suite, avant que la page ne soit regardée : aucun emoji (du SVG
    /// seulement), et, dans un tour de design, le socle de la méthode présent
    /// dans les feuilles de style (une fois par tour, toutes feuilles lues).
    fn interface_file_notes(&mut self, path: &str, aids: &mut TurnAids, notes: &mut Vec<String>) {
        let svg_file = std::path::Path::new(path)
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("svg"));
        if !(crate::web_design::is_interface_file(path) || svg_file)
            || !harness::enabled(Aid::DesignGuide)
        {
            return;
        }
        let file = std::path::Path::new(path);
        let name = file.file_name().map_or_else(
            || path.to_string(),
            |name| name.to_string_lossy().to_string(),
        );
        let text = std::fs::read_to_string(file).unwrap_or_default();
        if let Some(found) = crate::web_design::emoji_in_source(&text) {
            self.api_client.notify(&format!(
                "Emoji dans {name} : à remplacer par du SVG, signalé au modèle"
            ));
            notes.push(crate::web_design::emoji_feedback(path, &found));
        }
        // Une icône ne se dessine jamais à la main : l'outil `Icon` la donne.
        let drawn = crate::icon_set::drawn_icons(&text);
        if !drawn.is_empty() {
            self.api_client.notify(&format!(
                "Icône(s) dessinée(s) à la main dans {name} : {}, signalée(s) au modèle",
                drawn.len()
            ));
            notes.push(crate::icon_set::drawn_icons_feedback(path, &drawn));
        }
        let is_stylesheet = file
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("css") || ext.eq_ignore_ascii_case("scss"));
        if is_stylesheet && aids.design_guided && !aids.foundation_noted {
            let root = std::env::current_dir().unwrap_or_default();
            let gaps = crate::web_design::foundation_gaps(&crate::web_design::project_css(&root));
            if gaps.len() >= 2 {
                aids.foundation_noted = true;
                self.api_client.notify(&format!(
                    "Socle de la méthode absent des styles ({} pièce(s) manquante(s)) : signalé au modèle",
                    gaps.len()
                ));
                notes.push(crate::web_design::foundation_feedback(&gaps));
            }
        }
    }

    /// Lecture identique (même fichier, mêmes lignes, même contenu) à une
    /// lecture encore dans la conversation : l'ancienne copie laisse la place
    /// à la nouvelle. Refaite dans le même tour, elle n'a rien appris au
    /// modèle pour le prix d'une requête : une remarque le lui dit.
    /// Trois recherches à la suite dans les styles d'une page : remarque qui
    /// renvoie vers `inspect` de `Screenshot`, qui nomme les règles d'un
    /// élément (fichier, ligne) et mesure ses espacements. Vu le 26 octobre
    /// 2026 (session #31) : six recherches de suite (`\.section\s*\{`,
    /// `section.*padding`…) puis deux lectures pour trouver l'espacement d'une
    /// section.
    fn style_search_note(
        &self,
        tool_name: &str,
        input: &str,
        aids: &mut TurnAids,
    ) -> Option<String> {
        if harness::is_style_search(tool_name, input) {
            aids.style_searches += 1;
        } else if !matches!(tool_name, "read_file" | "TodoWrite") {
            aids.style_searches = 0;
        }
        if aids.style_searches < harness::STYLE_SEARCHES_BEFORE_NOTE
            || aids.style_search_notes >= harness::MAX_STYLE_SEARCH_NOTES
            || self.subagent
            || self.page_to_review(aids).is_none()
        {
            return None;
        }
        aids.style_searches = 0;
        aids.style_search_notes += 1;
        Some(harness::STYLE_SEARCH_NOTE.to_string())
    }

    fn repeated_read_note(&mut self, output: &str, aids: &mut TurnAids) -> Option<String> {
        if !harness::enabled(Aid::RepeatedReads) {
            return None;
        }
        let (path, length) = harness::read_result(output)?;
        let read = &output[..length];
        let stub = harness::repeated_read_stub(&path);
        let span = harness::read_span(read);
        let mut repeated = false;
        for message in &mut self.session.messages {
            for block in &mut message.blocks {
                if let ContentBlock::ToolResult {
                    tool_name,
                    output: earlier,
                    is_error: false,
                    ..
                } = block
                {
                    if tool_name != "read_file" {
                        continue;
                    }
                    if earlier.starts_with(read) {
                        earlier.replace_range(..length, &stub);
                        repeated = true;
                        continue;
                    }
                    // Le même fichier relu après un changement (en entier, ou les
                    // mêmes lignes) : seule la lecture la plus récente reste. Vu
                    // dans une vraie session : les copies périmées de `base.html`
                    // s'accumulaient, 62 000 jetons par requête en moyenne.
                    let Some((new_path, start, lines, whole)) = span.as_ref() else {
                        continue;
                    };
                    let Some((old_path, old_start, old_lines, _)) = harness::read_span(earlier)
                    else {
                        continue;
                    };
                    let Some((_, old_length)) = harness::read_result(earlier) else {
                        continue;
                    };
                    if old_path == *new_path
                        && (*whole || (old_start == *start && old_lines == *lines))
                    {
                        earlier.replace_range(..old_length, &harness::superseded_read_stub(&path));
                    }
                }
            }
        }
        // Long fichier relu en entier plusieurs fois dans le tour : lire les
        // lignes utiles suffit. Une fois par fichier et par tour.
        let mut long_note = None;
        if let Some((_, _, lines, true)) = span.as_ref() {
            if read.len() > harness::LONG_READ_CHARS {
                let count = aids.whole_reads.entry(path.clone()).or_insert(0);
                *count += 1;
                if *count == harness::LONG_READS_BEFORE_NOTE {
                    long_note = Some(harness::long_reread_note(&path, *lines, *count));
                }
            }
        }
        // Une lecture d'un tour précédent refaite à l'identique n'est pas une
        // faute : le fichier avait pu changer entre-temps.
        let same_turn = !aids.reads_seen.insert(harness::fingerprint(read));
        if repeated && same_turn && aids.repeated_read_notes < harness::MAX_REPEATED_READ_NOTES {
            aids.repeated_read_notes += 1;
            return Some(harness::repeated_read_note(&path));
        }
        long_note
    }

    /// Le modèle lit un fichier qu'une vue encore à jour montre déjà en entier
    /// (essai réel t6 : le petit modèle relisait les fichiers du petit projet
    /// qu'il avait sous les yeux). Une fois par fichier, dans la limite des
    /// remarques de relecture du tour.
    fn viewed_read_note(&mut self, output: &str, aids: &mut TurnAids) -> Option<String> {
        if !harness::enabled(Aid::RepeatedReads)
            || aids.repeated_read_notes >= harness::MAX_REPEATED_READ_NOTES
        {
            return None;
        }
        let (path, _) = harness::read_result(output)?;
        let marker = format!(
            "{}`{}`",
            harness::FILE_VIEW_PREFIX,
            harness::shown_path(&path)
        );
        let current = self.file_views.get(&marker)?;
        if file_stamp(std::path::Path::new(&path)).as_ref() != Some(current) {
            return None;
        }
        if !aids.viewed_reads.insert(marker) {
            return None;
        }
        aids.repeated_read_notes += 1;
        Some(harness::viewed_read_note(&path))
    }

    /// Un appel n'a pas tourné faute de réponse à la demande d'autorisation :
    /// l'utilisateur apprend, une fois par tour, que les suivantes ne lui
    /// seront pas posées.
    fn note_not_run(&mut self, output: &str, aids: &mut TurnAids) {
        if crate::permissions::is_not_run(output) && !aids.away_noticed {
            aids.away_noticed = true;
            self.api_client.notify(
                "Demande d'autorisation restée sans réponse : les suivantes de ce tour sont refusées sans vous être posées ; l'agent termine ce qui n'en demande pas",
            );
        }
    }

    /// Chemin (tel que le modèle l'écrit) et lignes d'origine quand cette
    /// modification change ou retire des lignes d'un fichier de données qui
    /// existait au début du tour. Signalé une fois ; un fichier remis dans son
    /// état de départ réarme la garde.
    fn rewritten_data(
        path: &str,
        output: &str,
        aids: &mut TurnAids,
    ) -> Option<(String, Vec<String>)> {
        let file = std::path::Path::new(path);
        if !harness::enabled(Aid::DataGuard) || !harness::is_data_file(file) {
            return None;
        }
        // Créé pendant ce tour, ou vide au départ : ce sont les données de l'agent.
        let original = aids.originals.get(path)?.clone();
        if original.trim().is_empty() {
            return None;
        }
        if std::fs::read_to_string(file).is_ok_and(|now| now.lines().eq(original.lines())) {
            aids.guarded_data.remove(path);
            return None;
        }
        let replaced = harness::replaced_lines(output);
        if replaced.is_empty() || !aids.guarded_data.insert(path.to_string()) {
            return None;
        }
        Some((harness::shown_path(path), replaced))
    }

    /// Le modèle a changé un fichier JSON de données qui existait au début du
    /// tour pour qu'il colle à son nouveau code. Essai réel t7, deux fois :
    /// `"inscrits": []` ajouté à chaque atelier de `donnees.json`, puis un code
    /// qui plante sur le fichier de l'utilisateur, resté à l'ancienne forme.
    fn json_data_reshaped(&mut self, input: &str, aids: &mut TurnAids) -> Option<String> {
        if !harness::enabled(Aid::DataGuard) {
            return None;
        }
        let path = edited_path(input)?;
        let cwd = std::env::current_dir().ok()?;
        if !harness::is_json_data_file(path.strip_prefix(&cwd).unwrap_or(&path)) {
            return None;
        }
        let before = aids.turn_start_files.get(&path)?.clone()?;
        let after = std::fs::read_to_string(&path).ok()?;
        let (added, changed) = harness::json_data_change(&before, &after)?;
        let shown = harness::shown_path(&path.to_string_lossy());
        if !aids.guarded_data.insert(shown.clone()) {
            return None;
        }
        self.api_client.notify(&format!(
            "Attention : {shown}, les données que le programme enregistre, a été modifié pour coller au nouveau code : relisez ce changement"
        ));
        Some(harness::json_data_feedback(&shown, &added, changed))
    }

    /// Le guide d'un petit projet, ses vues enregistrées comme celles qui
    /// suivent une modification (remplacées quand le fichier change).
    fn small_project_views(&mut self) -> Option<String> {
        let cwd = std::env::current_dir().ok()?;
        let files = harness::small_project_files(&cwd)?;
        for (path, _) in &files {
            if let Some(stamp) = file_stamp(&cwd.join(path)) {
                self.file_views
                    .insert(format!("{}`{path}`", harness::FILE_VIEW_PREFIX), stamp);
            }
        }
        Some(harness::small_project_guide(&files))
    }

    /// Les vues dont le fichier a changé depuis (commande, formateur, `git
    /// checkout`, l'utilisateur entre deux tours) ne sont plus à jour : elles
    /// sont retirées de la conversation, le modèle relira le fichier.
    fn drop_stale_file_views(&mut self) {
        let stale: Vec<String> = self
            .file_views
            .iter()
            .filter(|(_, stamp)| file_stamp(&stamp.0).as_ref() != Some(*stamp))
            .map(|(marker, _)| marker.clone())
            .collect();
        for marker in stale {
            replace_file_views(&mut self.session, &marker, harness::FILE_VIEW_STALE);
            self.file_views.remove(&marker);
        }
    }

    /// Nom du fichier si cette modification réécrit, après un échec, des lignes
    /// d'un fichier qui sert à vérifier le travail (test, ou script nommé par
    /// la commande de vérification). Signalé une fois par fichier.
    fn rewritten_check(
        &self,
        path: &str,
        input: &str,
        output: &str,
        aids: &mut TurnAids,
    ) -> Option<String> {
        if !harness::enabled(Aid::TestGuard) || aids.guarded_tests.contains(path) {
            return None;
        }
        // Ajouter des tests est normal ; seules les lignes retirées comptent.
        if !harness::removes_existing_lines(output) {
            return None;
        }
        // Et seulement celles qui existaient au début du tour : corriger un
        // test que le modèle vient d'écrire n'est pas réécrire une vérification
        // (essais réels du 13 octobre : la garde se déclenchait sur ses propres
        // tests neufs, et lui disait de les « restaurer »).
        let edited = edited_path(input);
        match edited
            .as_ref()
            .and_then(|file| aids.turn_start_files.get(file))
        {
            // Absent au début du tour et là maintenant : créé par le modèle.
            Some(None) if edited.as_ref().is_some_and(|file| file.exists()) => return None,
            // Une ligne d'origine est retirée quand le fichier en compte moins
            // qu'au début du tour : retirer une copie que le modèle avait
            // ajoutée (`assert statut == 200` dans un test neuf) ne compte pas.
            Some(Some(start)) => {
                let now = edited
                    .as_ref()
                    .and_then(|file| std::fs::read_to_string(file).ok())
                    .unwrap_or_default();
                let (before, after) = (line_counts(start), line_counts(&now));
                if !harness::replaced_lines(output).iter().any(|line| {
                    let line = line.trim();
                    after.get(line).copied().unwrap_or(0) < before.get(line).copied().unwrap_or(0)
                }) {
                    return None;
                }
            }
            _ => {}
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

    /// Aucune vérification n'est connue pour ce projet : l'utilisateur
    /// l'apprend une fois par lancement, au lieu de croire son travail vérifié.
    fn say_once_that_nothing_verifies(&mut self) {
        static SAID: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
        if !SAID.swap(true, std::sync::atomic::Ordering::Relaxed) {
            self.api_client.notify(
                "Aucune vérification automatique connue pour ce projet : les modifications ne sont pas vérifiées par Claw. Réglez \"verify\" dans .claw/settings.json (par exemple [\"npm test\"])",
            );
        }
    }

    /// La vérification n'a rien dit du travail : son outil n'est pas installé
    /// (ce n'est pas un défaut du travail, le modèle n'a rien à « corriger »),
    /// ou elle a dépassé son délai. Claw le dit et n'insiste plus de ce tour.
    fn verification_says_nothing(
        &mut self,
        aids: &mut TurnAids,
        command: &str,
        result: &harness::CommandResult,
    ) -> bool {
        let notice = if !result.passed && harness::check_tool_is_missing(&result.excerpt) {
            format!(
                "Vérification impossible ({command}) : l'outil de test n'est pas installé ; le travail n'est pas vérifié"
            )
        } else if !result.passed && harness::no_tests_ran(&result.excerpt) {
            format!(
                "Vérification sans objet ({command}) : aucun test n'a été trouvé ; le travail n'est pas vérifié"
            )
        } else if result.timed_out {
            format!(
                "Vérification trop longue ({command}) : choisissez une commande plus rapide avec \"verify\" dans .claw/settings.json"
            )
        } else {
            return false;
        };
        aids.verify_disabled = true;
        self.api_client.notify(&notice);
        true
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
            || aids.verify_disabled
            || interrupt_requested()
            || !harness::enabled(Aid::Verify)
        {
            return None;
        }
        let by_commands = Self::changed_by_commands(aids);
        if !aids.dirty && !by_commands {
            return None;
        }
        let feedback = self.run_verification(aids, prompter);
        // Ce que la vérification elle-même a écrit n'est pas un changement du
        // modèle : l'état de référence est repris après elle.
        if aids.snapshot.is_some() {
            aids.snapshot = std::env::current_dir()
                .ok()
                .and_then(|cwd| harness::project_snapshot(&cwd));
        }
        if let (Some(snapshot), Ok(cwd)) = (aids.data_snapshot.as_mut(), std::env::current_dir()) {
            snapshot.refresh_all(&cwd);
        }
        feedback
    }

    /// Revue du plan de design avant le code : un autre modèle juge la
    /// direction (identité, hiérarchie, typographie, couleur, composition)
    /// quand le modèle s'apprête à écrire un fichier d'interface après l'avoir
    /// posée. Changer un plan ne coûte rien ; changer une page faite coûte une
    /// réécriture que le budget du tour ne permet pas toujours. Une fois par
    /// tour, une requête de plus ; rien quand la méthode n'a pas été jointe.
    fn plan_review(
        &mut self,
        pending: &[(String, String, String)],
        assistant_messages: &[ConversationMessage],
        request: &str,
        earlier: Option<&str>,
        aids: &mut TurnAids,
    ) -> Option<String> {
        // Une correction, ou une page qui existait déjà sans demande de refonte :
        // pas de plan à écrire. Vu le 26 octobre 2026 (session #32) : deux
        // balises <script> ajoutées pour partager les réservations, et l'agent
        // renvoyé écrire un plan de design complet.
        let existing_page = aids.baseline.is_some()
            && !crate::web_design::asks_new_design(crate::project::own_words(request));
        if !aids.design_guided
            || aids.plan_reviewed
            || aids.repair
            || existing_page
            || self.subagent
            || interrupt_requested()
            || !harness::enabled(Aid::DesignReview)
        {
            return None;
        }
        let writes_interface = pending.iter().any(|(_, name, input)| {
            crate::file_tools::writes_files(name)
                && crate::file_tools::written_paths(name, input)
                    .iter()
                    .any(|path| crate::web_design::is_interface_file(path))
        });
        if !writes_interface {
            return None;
        }
        // Le plan écrit dans la réponse, ou posé dans le raisonnement : beaucoup
        // de modèles le font là.
        let plan = assistant_messages
            .iter()
            .flat_map(|message| message.blocks.iter())
            .filter_map(|block| match block {
                ContentBlock::Text { text } => Some(text.as_str()),
                ContentBlock::Thinking { thinking, .. } => Some(thinking.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n");
        if !crate::web_design::looks_like_design_plan(&plan) {
            // Pas de plan du tout : le modèle l'écrit avant d'aller plus loin,
            // et le juge le relira à l'écriture suivante. Vu dans un essai
            // réel : le modèle est passé droit au code, sans palette.
            if aids.plan_requested {
                return None;
            }
            aids.plan_requested = true;
            self.api_client
                .notify("Pas de plan de design avant le code : demandé à l'agent");
            return Some(crate::web_design::PLAN_REQUEST.to_string());
        }
        // Un wireframe dessiné à la main, aux bords décalés : à redessiner avec l'outil avant
        // que le juge relise le plan (vu le 21 octobre 2026 : bords et textes qui débordent).
        if !aids.wireframe_flagged {
            let problems = crate::ascii_art::misaligned(&plan, 6);
            if !problems.is_empty() {
                aids.wireframe_flagged = true;
                self.api_client.notify(
                    "Wireframe du plan mal aligné : redessin demandé avec l'outil Wireframe",
                );
                return Some(crate::ascii_art::feedback(&problems));
            }
        }
        aids.plan_reviewed = true;
        if self
            .api_client
            .free_requests_left()
            .is_some_and(|left| left < crate::web_design::MIN_FREE_REQUESTS)
        {
            return None;
        }
        // Une relance courte (« continue ») se juge avec la demande qu'elle poursuit.
        let brief = match earlier {
            Some(earlier) if request.split_whitespace().count() <= 12 => {
                format!("{earlier}\n{request}")
            }
            _ => request.to_string(),
        };
        self.api_client
            .notify("Revue du plan de design par un autre modèle, avant le code…");
        let consultation = self.api_client.consult(
            ConsultPurpose::Design,
            crate::web_design::PLAN_CRITIC_SYSTEM,
            &crate::web_design::plan_critique_prompt(
                &brief,
                &plan,
                crate::web_design::design_bar(),
            ),
        )?;
        if let Some(changes) = crate::web_design::plan_changes(&consultation.text) {
            self.api_client.notify(&format!(
                "Revue du plan par {} : {} remarque(s) transmise(s) à l'agent",
                consultation.model,
                changes.lines().count()
            ));
            Some(crate::web_design::plan_feedback(
                &consultation.model,
                &changes,
            ))
        } else {
            self.api_client.notify(&format!(
                "Revue du plan par {} : rien à redire",
                consultation.model
            ));
            None
        }
    }

    /// La page que la revue visuelle capturerait : celle que l'utilisateur a
    /// nommée, sinon une page HTML parmi les fichiers modifiés. Aucune pour
    /// un module `TypeScript` ou un script sans page.
    fn page_to_review(&self, aids: &TurnAids) -> Option<String> {
        if let Some(target) = &self.page_target {
            return Some(target.clone());
        }
        let cwd = std::env::current_dir().ok()?;
        let changed: Vec<String> = aids
            .originals
            .keys()
            .chain(&aids.command_changes)
            .cloned()
            .collect();
        crate::web_design::page_from_files(&cwd, &changed)
    }

    /// État de la page au début du tour : ses défauts mesurés (sans juge ni
    /// captures pour le modèle), gardés pour que chaque revue du tour nomme
    /// ceux que le tour a introduits.
    fn baseline_audit(
        &mut self,
        aids: &mut TurnAids,
        prompter: &mut Option<&mut dyn PermissionPrompter>,
    ) {
        if self.subagent || interrupt_requested() || !harness::enabled(Aid::DesignReview) {
            return;
        }
        let Some(target) = self.page_to_review(aids) else {
            return;
        };
        let cwd = std::env::current_dir().unwrap_or_default();
        if !crate::web_design::missing_local_assets(&cwd, &target).is_empty() {
            return;
        }
        self.api_client
            .notify("État de la page avant ce tour : Claw la mesure, pour repérer ce que le tour casserait…");
        let input = crate::web_design::review_screenshot_input(&target, aids.shared_request);
        self.tool_executor.announce("Screenshot", &input);
        let (message, _) = self.execute_tool_call(
            "claw-baseline".to_string(),
            "Screenshot".to_string(),
            &input,
            prompter
                .as_mut()
                .map(|prompt| &mut **prompt as &mut dyn PermissionPrompter),
        );
        // Les captures ne servent qu'à mesurer : ni juge, ni conversation.
        drop(std::mem::take(&mut self.tool_images));
        let Some((output, false)) = tool_result_parts(&message) else {
            return;
        };
        let audit = crate::web_design::read_audit(output);
        aids.baseline = Some(
            audit
                .problems
                .iter()
                .map(|problem| crate::web_design::problem_key(problem))
                .collect(),
        );
        // Point de départ de la vérification qui suit chaque modification :
        // sans lui, ce que la première modification casse passait pour
        // « restant ». Vu le 26 octobre 2026 (session #31) : « 0 défaut
        // mesuré » avant le tour, puis « 0 nouveau, 2 restants ». Toutes les
        // tailles d'écran : la mesure du début essaie les boutons à
        // l'ordinateur, la vérification au téléphone ; un bouton mort déjà là
        // ne doit pas passer pour cassé par la modification.
        aids.last_checked = Some(
            audit
                .problems
                .iter()
                .chain(&audit.console)
                .cloned()
                .collect(),
        );
        self.api_client.notify(&format!(
            "État de la page avant ce tour : {} défaut(s) mesuré(s)",
            audit.problems.len() + audit.console.len()
        ));
        // Ce que la mesure a vu va à l'agent avec la demande : il part des
        // défauts réels, pas d'une supposition.
        let shared = aids.shared_request && self.kept_locally(aids);
        if let Some(brief) = crate::web_design::start_brief(&target, &audit, shared) {
            if let Some(message) = self.session.messages.last_mut() {
                message.blocks.push(ContentBlock::Text { text: brief });
            }
            self.api_client
                .notify("État de la page avant ce tour : transmis à l'agent avec la demande");
        }
    }

    /// Mesure de la page par Claw, sans juge ni captures pour le modèle : la
    /// page et la liste de ce qui est à reprendre (régressions du tour en
    /// tête), `None` dans la liste si rien ne l'est. Le compte des
    /// modifications à l'aveugle repart de zéro.
    fn measure_now(
        &mut self,
        aids: &mut TurnAids,
        prompter: &mut Option<&mut dyn PermissionPrompter>,
    ) -> Option<(String, Option<String>)> {
        let (target, audit, _) = self.render_page(aids, prompter, false)?;
        let broken = aids.baseline.as_ref().map_or_else(Vec::new, |baseline| {
            crate::web_design::regressions(baseline, &audit)
        });
        Some((target, crate::web_design::measured_list(&audit, &broken)))
    }

    /// La page ouverte par Claw dans le navigateur, mesurée et parcourue :
    /// téléphone et ordinateur, ou téléphone seul (`quick`, la vérification
    /// après chaque modification). Rend la page, les mesures et les captures.
    fn render_page(
        &mut self,
        aids: &mut TurnAids,
        prompter: &mut Option<&mut dyn PermissionPrompter>,
        quick: bool,
    ) -> Option<(
        String,
        crate::web_design::Audit,
        Vec<crate::attachments::ImageAttachment>,
    )> {
        if self.subagent || interrupt_requested() || !harness::enabled(Aid::DesignReview) {
            return None;
        }
        let target = self.page_to_review(aids)?;
        let cwd = std::env::current_dir().ok()?;
        if !crate::web_design::missing_local_assets(&cwd, &target).is_empty() {
            return None;
        }
        let input = if quick {
            crate::web_design::quick_screenshot_input(&target)
        } else {
            crate::web_design::screenshot_input(&target)
        };
        self.tool_executor.announce("Screenshot", &input);
        let (message, _) = self.execute_tool_call(
            format!("claw-measure-{}", aids.blind_measures + 1),
            "Screenshot".to_string(),
            &input,
            prompter
                .as_mut()
                .map(|prompt| &mut **prompt as &mut dyn PermissionPrompter),
        );
        // Les captures ne vont pas dans la conversation d'elles-mêmes.
        let images = std::mem::take(&mut self.tool_images);
        let (output, is_error) = tool_result_parts(&message)?;
        if is_error {
            return None;
        }
        aids.blind_edits = 0;
        let audit = crate::web_design::read_audit(output);
        aids.last_checked = Some(
            audit
                .problems
                .iter()
                .chain(&audit.console)
                .cloned()
                .collect(),
        );
        Some((target, audit, images))
    }

    /// Après chaque réponse du modèle qui a modifié l'interface, Claw ouvre
    /// la page dans un navigateur de téléphone, la mesure et la parcourt, et
    /// dit au modèle ce que sa modification a cassé, réparé, et ce qui reste.
    /// Un modèle qui lit les images reçoit aussi la capture. Demandé le 25
    /// octobre 2026 : « À chaque modification il doit vérifier visuellement
    /// la page ».
    fn visual_check(
        &mut self,
        aids: &mut TurnAids,
        prompter: &mut Option<&mut dyn PermissionPrompter>,
    ) -> Option<ConversationMessage> {
        if aids.blind_edits == 0 || aids.blind_measures >= harness::MAX_VISUAL_CHECKS {
            return None;
        }
        // Rien à regarder (pas de page HTML), ou une page qui appelle des
        // fichiers pas encore écrits : on attend sans compter d'essai.
        let target = self.page_to_review(aids)?;
        let cwd = std::env::current_dir().ok()?;
        if !crate::web_design::missing_local_assets(&cwd, &target).is_empty() {
            return None;
        }
        aids.blind_measures += 1;
        let before = aids.last_checked.clone();
        // En arrière-plan quand l'exécuteur le permet : la page se mesure
        // pendant que le modèle prépare sa réponse suivante, et le résultat
        // arrive avec elle. Vu le 26 octobre 2026 (session #32, passage 5) : 33
        // vérifications de 25 s chacune, le modèle attendant chaque fois.
        let input = crate::web_design::quick_screenshot_input(&target);
        if let Some(handle) = self.tool_executor.spawn_background("Screenshot", &input) {
            aids.blind_edits = 0;
            aids.pending_check = Some(PendingCheck {
                handle,
                target,
                before,
            });
            return None;
        }
        let Some((target, audit, images)) = self.render_page(aids, prompter, true) else {
            // La page ne s'ouvre pas en fichier (une appli qui veut son
            // serveur) : pas d'autre essai pendant ce tour.
            aids.blind_measures = harness::MAX_VISUAL_CHECKS;
            return None;
        };
        Some(
            self.check_message(&target, before.as_deref(), &audit, images)
                .0,
        )
    }

    /// Ce que la vérification dit au modèle : ce que sa modification a cassé,
    /// réparé, et ce qui reste (avec la capture pour un modèle qui la lit) ;
    /// et le nombre de défauts nouveaux.
    fn check_message(
        &mut self,
        target: &str,
        before: Option<&[String]>,
        audit: &crate::web_design::Audit,
        images: Vec<crate::attachments::ImageAttachment>,
    ) -> (ConversationMessage, usize) {
        let (new, fixed, open) = crate::web_design::check_delta(before, audit);
        self.api_client.notify(&format!(
            "Claw vérifie la page après la modification : {} défaut(s) nouveau(x), {fixed} réparé(s), {} restant(s)",
            new.len(),
            open.len()
        ));
        let image = images
            .into_iter()
            .next()
            .filter(|_| self.api_client.reads_images());
        let text =
            crate::web_design::visual_check_feedback(target, &new, fixed, &open, image.is_some());
        let mut message = ConversationMessage::user_text(text);
        if let Some(image) = image {
            message.blocks.push(ContentBlock::Image {
                media_type: image.media_type,
                data: image.data,
            });
        }
        (message, new.len())
    }

    /// La vérification lancée en arrière-plan, attendue (le plus souvent déjà
    /// finie : elle a tourné pendant la requête du modèle) : son message et le
    /// nombre de défauts nouveaux. Une page qui ne s'ouvre pas arrête les
    /// vérifications du tour, comme avant.
    fn finish_visual_check(&mut self, aids: &mut TurnAids) -> Option<(ConversationMessage, usize)> {
        let pending = aids.pending_check.take()?;
        let (result, images) = pending
            .handle
            .join()
            .unwrap_or_else(|_| (Err("la vérification s'est arrêtée".to_string()), Vec::new()));
        let Ok(output) = result else {
            aids.blind_measures = harness::MAX_VISUAL_CHECKS;
            return None;
        };
        let audit = crate::web_design::read_audit(&output);
        aids.last_checked = Some(
            audit
                .problems
                .iter()
                .chain(&audit.console)
                .cloned()
                .collect(),
        );
        Some(self.check_message(&pending.target, pending.before.as_deref(), &audit, images))
    }

    /// Le tour se termine (limite de requêtes ou de temps) après des
    /// modifications de l'interface que rien n'a regardées : Claw mesure la
    /// page, et la conclusion dit ce qui reste au lieu de le croire réglé. Vu
    /// le 25 octobre 2026 (session #31) : conclusion à la limite du tour, sans
    /// aucun contrôle, qui donnait pour vérifiés des boutons jamais essayés.
    fn wrap_up_measure(
        &mut self,
        aids: &mut TurnAids,
        prompter: &mut Option<&mut dyn PermissionPrompter>,
    ) -> String {
        if aids.blind_edits == 0 {
            return String::new();
        }
        self.api_client
            .notify("Claw mesure la page avant la conclusion, pour qu'elle dise ce qui reste");
        match self.measure_now(aids, prompter) {
            Some((_, Some(measured))) => format!(
                "\n\n[Claw] Claw measured the page just now (phone and desktop, every button tapped, every form sent). Still wrong:\n{measured}\nIn your conclusion, list these plainly as not done yet, and call verified only what this measurement does not contradict."
            ),
            _ => String::new(),
        }
    }

    /// La réponse dit « vérifié » alors que l'interface a changé depuis le
    /// dernier regard sur la page : Claw la mesure, et le modèle reprend ce
    /// qui ne va pas avant de répondre, une fois par tour. Vu le 25 octobre
    /// 2026 (session #31) : « Persistance localStorage … survivant au
    /// rechargement » et « Toutes interactions » donnés pour vérifiés, alors
    /// qu'aucun bouton n'avait été essayé de tout le tour.
    fn unchecked_claim_gate(
        &mut self,
        aids: &mut TurnAids,
        answer: &str,
        prompter: &mut Option<&mut dyn PermissionPrompter>,
    ) -> Option<String> {
        if aids.claim_checked || aids.blind_edits == 0 || self.subagent || interrupt_requested() {
            return None;
        }
        let sentence = harness::verification_claim(answer)?;
        aids.claim_checked = true;
        let edits = aids.blind_edits;
        self.api_client.notify(
            "La réponse dit « vérifié » sans regard sur la page depuis les dernières modifications : Claw la mesure",
        );
        let measured = self
            .measure_now(aids, prompter)
            .and_then(|(_, measured)| measured);
        Some(harness::unchecked_claim_feedback(
            &sentence,
            edits,
            measured.as_deref(),
        ))
    }

    /// Revue visuelle : le modèle a modifié l'interface et dit avoir fini.
    /// Claw regarde la page comme un utilisateur (captures pleine page au
    /// téléphone et à l'ordinateur, défauts mesurés, essai du menu) et la fait
    /// juger par un modèle qui voit ; ce qui est à reprendre repart au modèle,
    /// quelques fois par tour au plus (`CLAW_DESIGN_REVIEW_ROUNDS`).
    #[allow(clippy::too_many_lines)]
    fn interface_gate(
        &mut self,
        aids: &mut TurnAids,
        request: &str,
        earlier: Option<&str>,
        prompter: &mut Option<&mut dyn PermissionPrompter>,
    ) -> Option<String> {
        if self.subagent || interrupt_requested() || !harness::enabled(Aid::DesignReview) {
            return None;
        }
        let by_commands: Vec<String> = aids
            .command_changes
            .iter()
            .filter(|path| {
                crate::web_design::is_interface_file(path)
                    && !aids.interface_commands.contains(*path)
            })
            .cloned()
            .collect();
        if !aids.interface_dirty && by_commands.is_empty() {
            return None;
        }
        aids.interface_dirty = false;
        aids.interface_commands.extend(by_commands);
        let mut rounds = crate::web_design::max_review_rounds() + aids.extra_rounds;
        if aids.interface_rounds >= rounds {
            // Revues du tour épuisées : une de plus si la note monte encore.
            if rounds == 0
                || !(self.continue_function(aids, "revues du tour")
                    || self.continue_design(aids, "revues du tour"))
            {
                return None;
            }
            aids.extra_rounds += 1;
            rounds += 1;
        }
        if self
            .api_client
            .free_requests_left()
            .is_some_and(|left| left < crate::web_design::MIN_FREE_REQUESTS)
        {
            self.api_client
                .notify("Revue visuelle sautée : il reste peu de requêtes gratuites aujourd'hui");
            return None;
        }
        let cwd = std::env::current_dir().ok()?;
        let target = self.page_to_review(aids)?;
        // La page appelle des feuilles de style ou des scripts pas encore
        // écrits : la revue attend qu'ils existent, sans perdre son tour.
        let missing = crate::web_design::missing_local_assets(&cwd, &target);
        if !missing.is_empty() {
            if aids.assets_reminded {
                return None;
            }
            aids.assets_reminded = true;
            self.api_client.notify(&format!(
                "Revue visuelle reportée : la page appelle {}, pas encore écrit(s)",
                missing.join(", ")
            ));
            return Some(crate::web_design::missing_assets_feedback(
                &target, &missing,
            ));
        }
        aids.interface_rounds += 1;
        self.api_client.notify(&format!(
            "Revue visuelle de l'interface ({}/{rounds}) : captures au téléphone et à l'ordinateur…",
            aids.interface_rounds
        ));
        let shared = aids.shared_request
            || crate::web_design::shared_between_people(&crate::project::user_words_with_history(
                request,
            ));
        let input = crate::web_design::review_screenshot_input(&target, shared);
        self.tool_executor.announce("Screenshot", &input);
        let (message, _) = self.execute_tool_call(
            format!("claw-design-{}", aids.interface_rounds),
            "Screenshot".to_string(),
            &input,
            prompter
                .as_mut()
                .map(|prompt| &mut **prompt as &mut dyn PermissionPrompter),
        );
        // Les captures vont au juge, pas dans la conversation.
        let images = std::mem::take(&mut self.tool_images);
        aids.blind_edits = 0;
        let (output, is_error) = tool_result_parts(&message)?;
        if is_error {
            self.api_client.notify(&format!(
                "Revue visuelle impossible : {}",
                output.lines().next().unwrap_or_default()
            ));
            return None;
        }
        let audit = crate::web_design::read_audit(output);
        // Une relance courte (« plus coloré ») se juge avec la demande qu'elle poursuit.
        let brief = match earlier {
            Some(earlier) if request.split_whitespace().count() <= 12 => {
                format!(
                    "{earlier}
{request}"
                )
            }
            _ => request.to_string(),
        };
        let bar = crate::web_design::design_bar();
        // Une demande de corrections se juge sur ce qui est mesuré (défauts,
        // régressions) : pas de note de design à atteindre. Vu le 26 octobre
        // 2026 (session #31, passage 4) : 89 minutes après une première
        // réponse à 17 minutes, pour une note restée à 4 sur 8.
        let critique = if images.is_empty() || aids.repair {
            None
        } else {
            self.api_client
                .notify("Revue visuelle : un modèle qui voit note les captures…");
            self.api_client.critique_design(
                crate::web_design::CRITIC_SYSTEM,
                &{
                    let mut prompt = crate::web_design::critique_prompt(
                        &brief,
                        images.len(),
                        &audit,
                        bar,
                        aids.graded.last().map(|graded| graded.scores.as_slice()),
                    );
                    if aids.restructured && !aids.restructure_asked {
                        aids.restructure_asked = true;
                        prompt.push_str(&crate::web_design::restructure_note(
                            &crate::web_design::score_trail(&aids.graded),
                            bar,
                        ));
                    }
                    prompt
                },
                &images,
            )
        };
        // Les notes du juge, pour l'utilisateur : « 6,4/10, barre 8 ».
        let scores = critique
            .as_ref()
            .map(|consultation| crate::web_design::critique_scores(&consultation.text))
            .unwrap_or_default();
        let reached = crate::web_design::design_reached(&scores, bar);
        let graded = crate::web_design::average_score(&scores).map_or_else(String::new, |mean| {
            let under: Vec<&str> = scores
                .iter()
                .filter(|(_, score)| *score < bar && !reached)
                .map(|(criterion, _)| criterion.as_str())
                .collect();
            aids.graded.push(crate::web_design::Graded {
                mean,
                under: under.iter().map(ToString::to_string).collect(),
                scores: scores.clone(),
            });
            if under.is_empty() {
                format!(", note {mean:.1}/10 (barre {bar})")
            } else {
                format!(
                    ", note {mean:.1}/10 (barre {bar}, sous la barre : {})",
                    under.join(", ")
                )
            }
        });
        let verdict = critique.as_ref().and_then(|consultation| {
            crate::web_design::critique_changes(&consultation.text, bar)
                .map(|lines| (consultation.model.clone(), lines))
        });
        let feedback = crate::web_design::review_feedback(
            &target,
            &audit,
            verdict
                .as_ref()
                .map(|(model, lines)| (model.as_str(), lines.as_str())),
            aids.interface_rounds,
            rounds,
            bar,
        );
        // Ce que le tour a cassé passe en tête : à réparer avant le reste.
        let broken = aids.baseline.as_ref().map_or_else(Vec::new, |baseline| {
            crate::web_design::regressions(baseline, &audit)
        });
        let feedback = match feedback {
            Some(text) if !broken.is_empty() => Some(format!(
                "{}\n\n{text}",
                crate::web_design::regression_note(&broken)
            )),
            other => other,
        };
        if !broken.is_empty() {
            self.api_client.notify(&format!(
                "Revue visuelle : {} défaut(s) absent(s) au début du tour, introduit(s) par ce tour : à réparer d'abord",
                broken.len()
            ));
        }
        let function = crate::web_design::function_score(&audit);
        aids.function_scores.push(function.score);
        let graded = format!(
            "{graded} · fonctionnement {}/10{}",
            function.score,
            if function.reasons.is_empty() {
                String::new()
            } else {
                format!(" ({})", function.reasons.join(", "))
            }
        );
        if feedback.is_some() {
            let judged = verdict.as_ref().map_or_else(String::new, |(model, lines)| {
                format!(
                    ", {} remarque(s) de {model}",
                    lines
                        .lines()
                        .filter(|line| !line.starts_with("Scores"))
                        .count()
                )
            });
            self.api_client.notify(&format!(
                "Revue visuelle : {} défaut(s) mesuré(s), {} remarque(s) de design{judged}{graded} : renvoyé(s) à l'agent",
                audit.problems.len() + audit.console.len(),
                audit.notes.len()
            ));
        } else {
            self.api_client.notify(&format!(
                "Revue visuelle : rien à reprendre sur la page{graded}"
            ));
        }
        feedback
    }

    /// Le tour de design touche une limite (`at` : ses requêtes, ou ses
    /// revues) sous la barre : il continue si la note monte, dans la limite
    /// des poursuites (`CLAW_DESIGN_CONTINUE`) et du quota du jour. Sinon,
    /// l'utilisateur lit pourquoi il s'arrête, une fois.
    /// À la limite du tour, des tâches de la liste restent ouvertes : un palier
    /// de requêtes de plus si le travail a avancé depuis le palier précédent
    /// (un fichier changé, une tâche finie), dans la limite des paliers et du
    /// quota gratuit du jour. Sinon le tour conclut, et la raison est dite.
    fn continue_tasks(&mut self, aids: &mut TurnAids) -> bool {
        if self.subagent
            || interrupt_requested()
            || !harness::enabled(Aid::OpenTasks)
            || aids.open_tasks.is_empty()
        {
            return false;
        }
        let open = aids.open_tasks.len();
        let moved = aids.project_changes > aids.changes_at_extension
            || aids.open_at_extension.is_some_and(|before| open < before);
        let free_left = self.api_client.free_requests_left();
        let refusal = if aids.task_extensions >= harness::MAX_TASK_EXTENSIONS {
            Some(format!(
                "{} paliers déjà ajoutés",
                harness::MAX_TASK_EXTENSIONS
            ))
        } else if !moved {
            Some("rien n'a avancé depuis le dernier palier".to_string())
        } else if free_left.is_some_and(|left| left < harness::TASK_EXTENSION_REQUESTS as u64 * 2) {
            Some("le quota gratuit du jour est presque épuisé".to_string())
        } else {
            None
        };
        if let Some(reason) = refusal {
            if !aids.task_extension_explained {
                aids.task_extension_explained = true;
                self.api_client.notify(&format!(
                    "Tâches pas finies ({open} restante{}) mais le tour conclut : {reason}. Écrivez « continue » pour reprendre",
                    if open > 1 { "s" } else { "" }
                ));
            }
            return false;
        }
        aids.task_extensions += 1;
        aids.changes_at_extension = aids.project_changes;
        aids.open_at_extension = Some(open);
        self.api_client.notify(&format!(
            "Tâches pas finies ({open} restante{}) : le tour continue, {} requêtes de plus ({}/{})",
            if open > 1 { "s" } else { "" },
            harness::TASK_EXTENSION_REQUESTS,
            aids.task_extensions,
            harness::MAX_TASK_EXTENSIONS
        ));
        true
    }

    fn continue_design(&mut self, aids: &mut TurnAids, at: &str) -> bool {
        // Des corrections ne poursuivent pas une note de design.
        if aids.repair
            || self.subagent
            || interrupt_requested()
            || !harness::enabled(Aid::DesignReview)
        {
            return false;
        }
        let max = crate::web_design::max_continuations();
        let free_left = self.api_client.free_requests_left();
        let decision = crate::web_design::continuation(
            &aids.graded,
            aids.graded_at_continuation,
            aids.continuations,
            max,
            free_left,
        );
        let bar = crate::web_design::design_bar();
        let trail = crate::web_design::score_trail(&aids.graded);
        if decision == crate::web_design::Continuation::Continue {
            aids.continuations += 1;
            aids.graded_at_continuation = aids.graded.len();
            let under = aids
                .graded
                .last()
                .map(|graded| graded.under.join(", "))
                .unwrap_or_default();
            self.api_client.notify(&format!(
                "Poursuite du design ({}/{max}, {at}) : note {trail}/10, sous la barre {bar} ({under}) ; le tour continue",
                aids.continuations
            ));
            return true;
        }
        // La note stagne sous la barre : une poursuite de plus, une seule, où le
        // juge ne donne que des changements de structure.
        if decision == crate::web_design::Continuation::Stalled
            && !aids.restructured
            && self
                .api_client
                .free_requests_left()
                .is_none_or(|left| left >= crate::web_design::CONTINUATION_RESERVE)
        {
            aids.restructured = true;
            aids.graded_at_continuation = aids.graded.len();
            self.api_client.notify(&format!(
                "Design bloqué sous la barre {bar} (note {trail}/10) : Claw demande au juge des changements de structure, et le tour continue"
            ));
            return true;
        }
        if let Some(reason) = crate::web_design::stop_reason(decision) {
            if !aids.continuation_explained {
                aids.continuation_explained = true;
                self.api_client.notify(&format!(
                    "Design arrêté sous la barre {bar} (note {trail}/10) : {reason}. Écrivez « continue » pour reprendre"
                ));
            }
        }
        false
    }

    /// Le fonctionnement mesuré reste sous la barre : le tour continue tant
    /// que la note monte (ou à la première note), deux fois au plus, dans le
    /// quota du jour ; les corrections comptent aussi. Sinon l'utilisateur lit
    /// où il s'arrête, une fois.
    fn continue_function(&mut self, aids: &mut TurnAids, at: &str) -> bool {
        if self.subagent || interrupt_requested() || !harness::enabled(Aid::DesignReview) {
            return false;
        }
        let bar = crate::web_design::design_bar();
        let Some(&last) = aids.function_scores.last() else {
            return false;
        };
        if last >= bar {
            return false;
        }
        let best_before = aids.function_scores[..aids.function_scores.len() - 1]
            .iter()
            .max()
            .copied();
        let rising = best_before.is_none_or(|best| last > best);
        let quota = self
            .api_client
            .free_requests_left()
            .is_none_or(|left| left >= crate::web_design::CONTINUATION_RESERVE);
        let trail = aids
            .function_scores
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(" puis ");
        if rising && quota && aids.function_extensions < crate::web_design::max_continuations() {
            aids.function_extensions += 1;
            self.api_client.notify(&format!(
                "Fonctionnement {trail}/10, sous la barre {bar} ({at}) : le tour continue pour le réparer"
            ));
            return true;
        }
        if !aids.function_explained {
            aids.function_explained = true;
            let reason = if !quota {
                "il reste peu de requêtes gratuites aujourd'hui"
            } else if rising {
                "les poursuites permises ont servi"
            } else {
                "la dernière revue ne l'a pas fait monter"
            };
            self.api_client.notify(&format!(
                "Fonctionnement arrêté sous la barre {bar} ({trail}/10) : {reason}. Écrivez « continue » pour reprendre"
            ));
        }
        false
    }

    /// Des fichiers de code ont changé par une commande (`sed -i`, `cat > f
    /// <<EOF`, un script) depuis le début du tour ou la dernière
    /// vérification : ils se vérifient comme après `edit_file`. Sans cela, le
    /// modèle qui contourne un `edit_file` raté finit sans vérification.
    fn changed_by_commands(aids: &mut TurnAids) -> bool {
        let (Some(before), Ok(cwd)) = (aids.snapshot.as_ref(), std::env::current_dir()) else {
            return false;
        };
        let changed = harness::changed_since(before, &cwd);
        if changed.is_empty() {
            return false;
        }
        aids.command_changes.extend(changed);
        aids.command_changes.sort();
        aids.command_changes.dedup();
        true
    }

    /// Lance la vérification du projet (voir `verification_gate`).
    fn run_verification(
        &mut self,
        aids: &mut TurnAids,
        prompter: &mut Option<&mut dyn PermissionPrompter>,
    ) -> Option<String> {
        let cwd = std::env::current_dir().ok()?;
        // Les fichiers modifiés du tour disent où vérifier quand le projet est
        // rangé en sous-dossiers (`backend/`, `frontend/`).
        let changed: Vec<String> = aids
            .originals
            .keys()
            .chain(&aids.command_changes)
            .cloned()
            .collect();
        let commands = harness::verify_commands_for(&self.verify, &cwd, &changed);
        aids.dirty = false;
        if commands.is_empty() && self.verify == harness::VerifySetting::Auto {
            self.say_once_that_nothing_verifies();
        }
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
                // Refusée par l'utilisateur, sans réponse ou impossible à
                // lancer : on le dit, pour ne pas laisser croire que le
                // travail a été vérifié.
                aids.verify_disabled = true;
                self.api_client.notify(&if crate::permissions::is_not_run(output) {
                    format!("Vérification non lancée ({command}) : elle attend votre autorisation, restée sans réponse ; le travail n'est pas vérifié")
                } else {
                    format!("Vérification non lancée ({command} refusée ou impossible) : le travail n'est pas vérifié")
                });
                return None;
            }
            let result = harness::read_bash_output(output);
            if self.verification_says_nothing(aids, &command, &result) {
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
                    if let Some(sites) = harness::error_sites(output, &cwd, &command) {
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
    fn requirements_gate(
        &mut self,
        aids: &mut TurnAids,
        request: &str,
        earlier: Option<&str>,
    ) -> Option<String> {
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
        let mut prompt =
            harness::checklist_prompt(request, earlier, crate::permissions::user_away());
        if let Some(reference) = Self::project_reference(aids) {
            prompt.push_str("\n\n");
            prompt.push_str(&reference);
        }
        Some(prompt)
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

    /// Signalement d'un problème : avant que la réponse parte, le modèle doit
    /// nommer, pour chaque symptôme, l'observation qui montre sa cause et celle
    /// qui montre sa disparition (une fois par tour). Remplace, pour ce tour,
    /// le contrôle général des exigences ; le vérificateur ultracode s'en
    /// charge quand il est actif.
    fn evidence_gate(
        &mut self,
        aids: &mut TurnAids,
        request: &str,
        earlier: Option<&str>,
    ) -> Option<String> {
        let due = match aids.report {
            harness::Report::Problem => true,
            // Des images sans plainte écrite : les preuves ne sont demandées
            // que si le modèle y a vu un problème (il a modifié des fichiers)
            // ou si le lecteur d'images en a vu un.
            harness::Report::Images => aids.saw_defects || !aids.originals.is_empty(),
            harness::Report::None => false,
        };
        if !due
            || aids.evidence_asked
            || aids.ultracode
            || aids.verify_failing
            || self.subagent
            || interrupt_requested()
        {
            return None;
        }
        aids.evidence_asked = true;
        aids.checklisted = true;
        self.api_client.notify(
            "Problème signalé : Claw demande à l'agent ce qu'il a observé de sa cause et de sa disparition avant de conclure",
        );
        let prompt = if aids.report == harness::Report::Problem {
            harness::PROBLEM_EVIDENCE_PROMPT
        } else {
            harness::IMAGES_EVIDENCE_PROMPT
        };
        let away = if crate::permissions::user_away() {
            format!(" {}", harness::AWAY_NOTE)
        } else {
            String::new()
        };
        // Les cas limites du genre de fonctionnalité en cause, à prouver aussi.
        let edges = harness::edge_case_check(request, earlier);
        Some(format!(
            "{prompt}{away}{edges}\n\n{}",
            harness::FINAL_ANSWER_RULE
        ))
    }

    /// Scripts écrits dans le tour et jamais lancés : le modèle est renvoyé
    /// les essayer, une fois, avant de répondre. Un code jamais lancé n'est
    /// pas fini ; un script qui ferait quelque chose d'irréversible n'est
    /// pas lancé, et la réponse le dit.
    fn run_scripts_gate(&mut self, aids: &mut TurnAids) -> Option<String> {
        if self.subagent
            || aids.scripts_asked
            || aids.scripts_written.is_empty()
            || interrupt_requested()
            || !harness::enabled(Aid::RunScripts)
            || self.permission_policy.active_mode() == crate::permissions::PermissionMode::ReadOnly
        {
            return None;
        }
        let commands: Vec<String> = aids
            .commands_run
            .iter()
            .map(|command| command.to_lowercase())
            .collect();
        let unrun: Vec<(String, crate::code_guide::Language)> = aids
            .scripts_written
            .iter()
            .filter_map(|path| {
                let file = std::path::Path::new(path);
                let name = file.file_name()?.to_string_lossy().to_string();
                let language = crate::code_guide::Language::of(file)?;
                // Un fichier supprimé depuis, ou déjà lancé, n'est plus à essayer.
                let lowered = name.to_lowercase();
                let run = commands.iter().any(|command| command.contains(&lowered));
                (file.exists() && !run).then_some((name, language))
            })
            .collect();
        if unrun.is_empty() {
            return None;
        }
        aids.scripts_asked = true;
        let names: Vec<&str> = unrun.iter().map(|(name, _)| name.as_str()).collect();
        self.api_client.notify(&format!(
            "Script(s) écrit(s) mais jamais lancé(s) : {} : Claw demande à l'agent de les essayer",
            names.join(", ")
        ));
        Some(crate::code_guide::run_scripts_feedback(&unrun))
    }

    /// Commandes conseillées sans avoir été lancées : la réponse dit à
    /// l'utilisateur de lancer des commandes (blocs de code d'un shell) que le
    /// modèle n'a lancées à aucun moment de la session. Il est renvoyé une
    /// fois les essayer, ou les dire non testées.
    fn instructions_gate(&mut self, aids: &mut TurnAids, answer: &str) -> Option<String> {
        if aids.commands_checked
            || self.subagent
            || interrupt_requested()
            || crate::permissions::user_away()
            || !harness::enabled(Aid::TriedCommands)
        {
            return None;
        }
        let executed: Vec<String> = self
            .session
            .messages
            .iter()
            .flat_map(|message| &message.blocks)
            .filter_map(|block| match block {
                ContentBlock::ToolUse { name, input, .. }
                    if matches!(name.as_str(), "bash" | "PowerShell") =>
                {
                    json_field(input, "command")
                }
                _ => None,
            })
            .collect();
        let untried = harness::untried_commands(answer, &executed);
        if untried.is_empty() {
            return None;
        }
        aids.commands_checked = true;
        self.api_client.notify(
            "La réponse conseille des commandes jamais lancées : Claw demande à l'agent de les essayer",
        );
        Some(harness::untried_feedback(&untried))
    }

    /// Données partagées gardées dans chaque téléphone : la demande fait entrer
    /// des choses par les uns (réserver, s'inscrire) et voir par d'autres, et
    /// le tour a mis ce qu'entrent les visiteurs dans `localStorage`, sans rien
    /// qui l'envoie ailleurs. Le modèle rend les données partagées ou le dit
    /// en tête de sa réponse ; une fois par tour.
    fn shared_data_gate(&mut self, aids: &mut TurnAids, request: &str) -> Option<String> {
        if aids.shared_checked
            || self.subagent
            || interrupt_requested()
            || !harness::enabled(Aid::DesignReview)
            || self.page_to_review(aids).is_none()
            || !crate::web_design::shared_between_people(&crate::project::user_words_with_history(
                request,
            ))
        {
            return None;
        }
        // Tout le site, pas seulement ce que le tour a touché : vu le 27
        // octobre 2026 (session #32, passage 6), « je ne vois aucune
        // réservation de mes clientes sur mon téléphone », le stockage local
        // était déjà là avant le tour, et l'agent a répondu « limitation
        // architecturale » sans proposer d'envoyer les réservations.
        if !self.kept_locally(aids) {
            return None;
        }
        aids.shared_checked = true;
        self.api_client.notify(
            "Ce que les visiteurs entrent reste dans leur propre téléphone, alors que la demande le veut partagé : Claw demande à l'agent de le rendre partagé ou de le dire",
        );
        Some(crate::web_design::SHARED_DATA_NOTE.to_string())
    }

    /// Le site garde ce que les visiteurs entrent dans leur propre navigateur,
    /// sans rien en envoyer : le dossier de la page revue (le projet), et les
    /// fichiers du tour.
    fn kept_locally(&self, aids: &TurnAids) -> bool {
        let Some(target) = self.page_to_review(aids) else {
            return false;
        };
        let Some(root) = target
            .strip_prefix("path:")
            .and_then(|page| {
                std::path::Path::new(page)
                    .parent()
                    .map(std::path::Path::to_path_buf)
            })
            .filter(|folder| !folder.as_os_str().is_empty())
            .or_else(|| std::env::current_dir().ok())
        else {
            return false;
        };
        let mut sources = crate::web_design::page_scripts(&root);
        for path in aids.originals.keys() {
            if let Ok(text) = std::fs::read_to_string(path) {
                sources.push(text);
            }
        }
        let sources: Vec<&str> = sources.iter().map(String::as_str).collect();
        crate::web_design::kept_on_each_device(&sources)
    }

    /// Ce que les visiteurs entrent (noms, téléphones), envoyé par le tour à
    /// un service d'un tiers que l'utilisateur n'a ni nommé ni accepté. Vu le
    /// 27 octobre 2026 (session #32, passage 7) : l'agent a trouvé `MantleDB`
    /// par une recherche et y envoyait les réservations des clientes, que
    /// lirait quiconque devine l'adresse, sans rien demander à la boulangère.
    fn data_service_gate(&mut self, aids: &mut TurnAids, request: &str) -> Option<String> {
        if aids.services_checked || aids.asked || self.subagent || interrupt_requested() {
            return None;
        }
        let words = crate::project::user_words_with_history(request).to_lowercase();
        let mut hosts: Vec<String> = Vec::new();
        for (path, before) in &aids.originals {
            let Ok(after) = std::fs::read_to_string(path) else {
                continue;
            };
            for host in crate::web_design::new_data_services(before, &after) {
                if !hosts.contains(&host) && !crate::web_design::named_by_user(&host, &words) {
                    hosts.push(host);
                }
            }
        }
        if hosts.is_empty() {
            return None;
        }
        aids.services_checked = true;
        self.api_client.notify(
            "Le code envoie ce que les visiteurs entrent à un service tiers que l'utilisateur n'a pas choisi : Claw demande à l'agent de lui poser la question d'abord",
        );
        Some(crate::web_design::data_service_note(&hosts))
    }

    /// Règles et promesses de la page que l'utilisateur n'a pas données (un
    /// délai d'annulation, une confirmation par SMS) : retirées ou demandées.
    /// Une fois par tour.
    fn invented_rules_gate(&mut self, aids: &mut TurnAids, request: &str) -> Option<String> {
        if aids.rules_checked
            || self.subagent
            || interrupt_requested()
            || !harness::enabled(Aid::DesignReview)
            || self.page_to_review(aids).is_none()
        {
            return None;
        }
        aids.rules_checked = true;
        let cwd = std::env::current_dir().ok()?;
        let page = crate::web_design::shown_text(&cwd);
        let user = crate::project::user_words_with_history(request);
        let sentences = crate::web_design::invented_rules(&page, &user);
        if sentences.is_empty() {
            return None;
        }
        self.api_client.notify(&format!(
            "{} règle(s) ou promesse(s) sur la page que l'utilisateur n'a pas données : Claw demande à l'agent de les retirer ou de les lui demander",
            sentences.len()
        ));
        Some(crate::web_design::invented_rules_note(&sentences))
    }

    /// Code d'accès écrit dans le script de la page, absent de la réponse :
    /// l'utilisateur ne pourrait pas entrer. Une fois par tour.
    fn access_code_gate(&mut self, aids: &mut TurnAids, answer: &str) -> Option<String> {
        if aids.code_told
            || self.subagent
            || interrupt_requested()
            || !harness::enabled(Aid::DesignReview)
            || self.page_to_review(aids).is_none()
        {
            return None;
        }
        aids.code_told = true;
        let cwd = std::env::current_dir().ok()?;
        let mut codes: Vec<String> = Vec::new();
        for text in crate::web_design::page_texts(&cwd) {
            for code in crate::code_checks::access_codes(&text) {
                if !codes.contains(&code) {
                    codes.push(code);
                }
            }
        }
        if codes.is_empty() || codes.iter().any(|code| answer.contains(code.as_str())) {
            return None;
        }
        self.api_client.notify(
            "La page s'ouvre avec un code que la réponse ne donne pas : Claw demande à l'agent de le dire à l'utilisateur",
        );
        Some(crate::web_design::access_code_note(&codes))
    }

    /// Clés à remplacer (`VOTRE_API_KEY`) introduites par le tour dans un
    /// fichier modifié, sans que l'utilisateur ait été consulté : le modèle lui
    /// demande d'abord quelle solution il accepte. Une fois par tour.
    fn account_setup_gate(&mut self, aids: &mut TurnAids) -> Option<String> {
        if aids.keys_checked || aids.asked || self.subagent || interrupt_requested() {
            return None;
        }
        // Un fichier touché par le tour qui attend encore ses clés, même
        // laissées par un tour précédent : tant qu'elles manquent, ce qui en
        // dépend ne marche pas, et l'utilisateur doit le savoir.
        let sample = aids.originals.keys().find_map(|path| {
            let current = std::fs::read_to_string(path).ok()?;
            crate::web_design::placeholder_credential(&current)
        })?;
        aids.keys_checked = true;
        self.api_client.notify(
            "Le code attend les clés d'un compte que l'utilisateur doit créer : Claw demande à l'agent de lui poser la question d'abord",
        );
        Some(crate::web_design::account_setup_note(&sample))
    }

    /// Photos enregistrées pendant le tour et que rien n'utilise : du poids
    /// pour le site, et souvent une photo hors sujet. Vu le 26 octobre 2026
    /// (session #32) : 19 photos (11 Mo) dans images/, aucune dans la page,
    /// dont « baguette-1.jpg » qui montrait un pot d'ajvar.
    fn unused_photos_gate(&mut self, aids: &mut TurnAids) -> Option<String> {
        if aids.photos_checked || self.subagent || interrupt_requested() {
            return None;
        }
        let cwd = std::env::current_dir().ok()?;
        // Celles d'un tour précédent aussi, quand ce tour touche à la page :
        // l'outil les note dans images/CREDITS.md. Vu le 26 octobre 2026
        // (session #32) : 11 Mo de photos du premier passage, jamais utilisées,
        // toujours publiés trois passages plus tard.
        let mut photos = aids.photos.clone();
        if self.page_to_review(aids).is_some() {
            photos.extend(crate::web_design::credited_photos(&cwd));
        }
        if photos.is_empty() {
            return None;
        }
        let pages = crate::web_design::page_texts(&cwd);
        let unused: Vec<&String> = photos
            .iter()
            .filter(|path| {
                cwd.join(path.as_str()).is_file() || std::path::Path::new(path.as_str()).is_file()
            })
            .filter(|path| {
                let name = std::path::Path::new(path.as_str())
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default();
                !pages.iter().any(|text| text.contains(&name))
            })
            .collect();
        if unused.is_empty() {
            return None;
        }
        aids.photos_checked = true;
        self.api_client.notify(&format!(
            "{} photo(s) enregistrée(s) et jamais utilisée(s) : Claw demande à l'agent de les retirer ou de s'en servir",
            unused.len()
        ));
        Some(format!(
            "[Claw] The page uses none of these photos you saved: {}. Each one weighs on the site the user publishes. Delete the ones you do not use (with a shell command, and their lines in images/CREDITS.md), or put the right one in the page where it shows what it should; then end with your final answer.",
            unused
                .iter()
                .take(12)
                .map(|path| format!("`{path}`"))
                .collect::<Vec<_>>()
                .join(", ")
        ))
    }

    /// Réponse sur une page écrite pour un développeur : le relecteur de Claw
    /// nommé, des bouts de code, des noms de fichiers. L'utilisateur d'un site
    /// ne lit pas le code : le modèle réécrit sa réponse une fois, en mots
    /// simples, sur le plan de `FINAL_ANSWER_SHAPE`.
    fn plain_answer_gate(&mut self, aids: &mut TurnAids, answer: &str) -> Option<String> {
        if aids.plain_asked >= 2
            || (aids.plain_asked == 1 && !aids.edited_since_plain)
            || self.subagent
            || interrupt_requested()
            || !harness::enabled(Aid::DesignReview)
            || self.page_to_review(aids).is_none()
        {
            return None;
        }
        let signs = harness::developer_signs(answer);
        if signs.is_empty() {
            return None;
        }
        aids.plain_asked += 1;
        aids.edited_since_plain = false;
        self.api_client.notify(
            "La réponse parle de code et de fichiers : Claw demande à l'agent de la réécrire en mots simples",
        );
        Some(harness::plain_answer_feedback(&signs))
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

    /// Titres sans contenu : la réponse annonce un travail par des titres
    /// (`### 2a. Composant URLInput`) sans rien dessous et sans avoir appelé
    /// d'outil. Le modèle est renvoyé le faire, deux fois au plus par tour,
    /// avant tout autre contrôle.
    fn empty_titles_gate(&mut self, aids: &mut TurnAids, answer: &str) -> Option<String> {
        if aids.empty_title_rounds >= harness::MAX_EMPTY_TITLE_ROUNDS
            || interrupt_requested()
            || !harness::enabled(Aid::EmptyTitles)
        {
            return None;
        }
        let (titles, ends_on_one) = harness::empty_titles(answer);
        if !ends_on_one && titles.len() < 2 {
            return None;
        }
        aids.empty_title_rounds += 1;
        self.api_client.notify(
            "La réponse s'arrête sur des titres sans contenu : Claw renvoie l'agent faire le travail annoncé",
        );
        Some(harness::empty_titles_feedback(&titles))
    }

    /// Travail annoncé et pas fait : la réponse finale dit ce que le modèle
    /// va faire alors qu'aucun fichier n'a été écrit pendant le tour, ni par
    /// ses outils ni par une commande. Il est renvoyé le faire, deux fois au
    /// plus par tour. Vu le 24 octobre 2026 dans Claw Web (session #30,
    /// Nemotron 3 Ultra) : polices et icônes choisies, puis « Maintenant je
    /// construis la PWA complète. » comme réponse finale, et aucun fichier.
    fn announced_work_gate(&mut self, aids: &mut TurnAids, answer: &str) -> Option<String> {
        if aids.announced_work_rounds >= harness::MAX_ANNOUNCED_WORK_ROUNDS
            || !aids.originals.is_empty()
            || !aids.command_changes.is_empty()
            || aids.questions_deferred
            || aids.work_question
            || self.subagent
            || interrupt_requested()
            || !harness::enabled(Aid::AnnouncedWork)
        {
            return None;
        }
        let sentence = harness::announced_work(answer)?;
        aids.announced_work_rounds += 1;
        self.api_client.notify(
            "La réponse annonce le travail sans l'avoir fait : Claw renvoie l'agent le faire",
        );
        Some(harness::announced_work_feedback(&sentence))
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

    /// Liste de tâches laissée ouverte : le modèle conclut alors que la
    /// liste écrite pendant le tour (`TodoWrite`) garde des tâches pas faites,
    /// que l'utilisateur voit inachevées sous la réponse. Il est renvoyé les
    /// faire, ou dire pourquoi et mettre la liste à jour, deux fois au plus
    /// par tour. Une réponse qui pose ses questions attend l'utilisateur, et
    /// la liste avec elle. Vu le 24 octobre 2026 dans Claw Web : réponse
    /// finale sous « Relancer tous les tests » en cours et « Commiter les
    /// changements » à faire.
    fn open_tasks_gate(&mut self, aids: &mut TurnAids, answer: &str) -> Option<String> {
        if aids.open_tasks.is_empty()
            || aids.open_task_rounds >= harness::MAX_OPEN_TASK_ROUNDS
            || aids.questions_deferred
            || self.subagent
            || interrupt_requested()
            || !harness::enabled(Aid::OpenTasks)
            || harness::ends_with_questions(answer)
        {
            return None;
        }
        aids.open_task_rounds += 1;
        self.api_client.notify(
            "La liste de tâches de l'agent n'est pas finie : Claw le renvoie faire les tâches restantes avant de conclure",
        );
        Some(harness::open_tasks_feedback(&aids.open_tasks))
    }

    /// Données de test laissées dans une page : un fichier modifié pendant le
    /// tour écrit encore des données d'essai dans le stockage du navigateur
    /// quand la page se charge. Le modèle est renvoyé les retirer, une fois.
    fn test_data_gate(&mut self, aids: &mut TurnAids) -> Option<String> {
        if aids.test_data_sent_back
            || self.subagent
            || interrupt_requested()
            || !harness::enabled(Aid::Pitfalls)
        {
            return None;
        }
        let left: Vec<String> = aids
            .edited_files
            .keys()
            .filter(|path| {
                std::fs::read_to_string(path)
                    .is_ok_and(|text| !crate::code_checks::injected_test_data(&text).is_empty())
            })
            .map(|path| path.display().to_string())
            .collect();
        if left.is_empty() {
            return None;
        }
        aids.test_data_sent_back = true;
        self.api_client.notify(
            "Des données de test restent dans le projet : Claw renvoie l'agent les retirer avant de conclure",
        );
        Some(format!(
            "[Claw] Before your final answer: {} still write(s) test or debug data into the browser storage as the page loads. Each time the user opens the app, it would replace their own data. Remove it now (the Screenshot tool's `storage` shows a page with sample data without touching the project), check the page still works, then answer.",
            left.join(", ")
        ))
    }

    /// Dossier que suit le mode projet.
    fn project_root(&self) -> Option<std::path::PathBuf> {
        self.project_root
            .clone()
            .or_else(|| std::env::current_dir().ok())
    }

    /// Quelqu'un peut répondre maintenant aux questions de l'agent.
    fn can_ask_now(&self) -> bool {
        self.api_client.can_ask_user() && !crate::permissions::user_away()
    }

    /// Mode projet au début du tour : ouvert par une demande de création dans
    /// un dossier vide ou par « mode projet », arrêté par « sans mode projet »,
    /// gardé ensuite pour ce dossier (`.claw/project.json`). Le guide de
    /// l'étape à faire est joint à la demande.
    fn start_project_turn(
        &mut self,
        request: &str,
        earlier: Option<&str>,
        user_message: &mut ConversationMessage,
    ) -> Option<project::Turn> {
        if self.subagent || !harness::enabled(Aid::ProjectMode) {
            return None;
        }
        let root = self.project_root()?;
        let saved = project::ProjectState::exists(&root);
        let mut state = project::ProjectState::load(&root);
        let words = project::own_words(request);
        let wish = project::read_wish(words);
        if wish == project::Wish::Stop {
            if state.active {
                state.active = false;
                let _ = state.save(&root);
                self.api_client.notify("Mode projet arrêté pour ce dossier");
            }
            return None;
        }
        // Mode projet choisi d'office (option de l'appli web) : il s'ouvre au
        // premier tour, sans attendre une demande de création.
        let starting = !state.active
            && (wish == project::Wish::Start
                || harness::project_mode_forced()
                || (project::creation_request(words) && project::empty_folder(&root)));
        // Projet déjà mené en mode projet (cahier des charges et plan en jalons à la racine)
        // dont l'état de Claw a disparu : clone neuf du moteur de l'appli web, `.claw`
        // effacé. Le mode reprend ; un arrêt enregistré (« sans mode projet ») reste.
        let resumed = !state.active
            && !starting
            && !saved
            && !matches!(
                project::Project::read(&root).phase(),
                project::Phase::Spec | project::Phase::Plan
            );
        if !state.active && !starting && !resumed {
            return None;
        }
        if resumed {
            state.active = true;
            // Le plan a été relu par un autre modèle quand il a été écrit ; la
            // relecture revient si des exigences ou des jalons changent.
            state.design_reviewed = Some(project::Project::read(&root).design_fingerprint());
            let _ = state.save(&root);
            self.api_client
                .notify("Mode projet repris : SPEC.md et PLAN.md sont à la racine du projet");
        }
        if starting {
            state.active = true;
            if let Err(error) = state.save(&root) {
                self.api_client.notify(&format!(
                    "Mode projet : son état n'a pas pu être enregistré ({error}) ; il ne vaut que pour ce tour"
                ));
            }
            self.api_client.notify(
                "Mode projet : Claw guide la création pas à pas (cahier des charges, plan en jalons vérifiés, tests d'acceptation, démonstration finale) ; « sans mode projet » l'arrête pour ce dossier",
            );
        }
        let project = project::Project::read(&root);
        let can_ask = self.can_ask_now();
        let mut guide = if starting && project.phase() == project::Phase::Spec {
            project::activation_guide(can_ask)
        } else {
            project::phase_guide(&project, can_ask)
        };
        // Quota gratuit du jour trop court pour les jalons restants : le modèle
        // le sait, et finit proprement plutôt que de s'arrêter au milieu.
        let remaining = match project.phase() {
            project::Phase::Spec | project::Phase::Plan => 1,
            project::Phase::Build(_) => project
                .milestones()
                .iter()
                .filter(|milestone| !milestone.done)
                .count(),
            project::Phase::Finish | project::Phase::Done => 0,
        };
        if remaining > 0 {
            if let Some(left) = self.api_client.free_requests_left() {
                let cost = project::milestone_cost(&project.state);
                if let Some((note, notice)) = project::budget_note(left, cost, remaining) {
                    guide.push('\n');
                    guide.push_str(&note);
                    self.api_client.notify(&notice);
                }
            }
        }
        // Au premier tour de la session : les outils de la machine et leurs pièges.
        if self.environment == EnvironmentProbe::Pending {
            let note = project::environment_note(&project::probe_environment());
            guide.push('\n');
            guide.push_str(&note);
            self.environment = EnvironmentProbe::Done(note);
        }
        user_message.blocks.push(ContentBlock::Text { text: guide });
        let mut turn = project::Turn::start(&project);
        turn.request = match earlier {
            Some(earlier) => {
                format!("{request}\n\nEarlier request that this message continues:\n{earlier}")
            }
            None => request.to_string(),
        };
        Some(turn)
    }

    /// Mode projet : le cahier des charges et le plan sont écrits et lisibles.
    /// Avant le code, un autre modèle les relit, une fois par contenu (de
    /// nouveau si des exigences ou des jalons changent) ; ses remarques vont au
    /// modèle. Sans relecteur, elle sera tentée au tour suivant.
    fn design_review(&mut self, turn: &mut project::Turn) -> Option<String> {
        if turn.design_review_tried
            || !self.review
            || interrupt_requested()
            || !harness::enabled(Aid::Review)
        {
            return None;
        }
        let current = project::Project::read(&turn.root);
        let plan = current.plan.as_ref()?;
        if !matches!(current.phase(), project::Phase::Build(_))
            || !project::plan_problems(plan, current.spec.as_ref()).is_empty()
        {
            return None;
        }
        let fingerprint = current.design_fingerprint();
        if current.state.design_reviewed.as_deref() == Some(fingerprint.as_str()) {
            return None;
        }
        turn.design_review_tried = true;
        self.api_client.notify(
            "Mode projet : un autre modèle relit le cahier des charges et le plan avant le code…",
        );
        let environment = match &self.environment {
            EnvironmentProbe::Done(note) => Some(note.as_str()),
            EnvironmentProbe::Pending | EnvironmentProbe::Off => None,
        };
        let prompt = project::design_review_prompt(&turn.request, &current, environment);
        let consultation = self.api_client.consult(
            ConsultPurpose::Design,
            project::DESIGN_REVIEW_SYSTEM,
            &prompt,
        )?;
        harness::log_review(&consultation.model, &consultation.text);
        let mut state = current.state.clone();
        state.design_reviewed = Some(fingerprint);
        let _ = state.save(&turn.root);
        match harness::review_findings(&consultation.text) {
            None => {
                self.api_client.notify(&format!(
                    "Relecture du plan par {} : rien à redire",
                    consultation.model
                ));
                None
            }
            Some(findings) => {
                self.api_client.notify(&format!(
                    "Relecture du plan par {} : {} remarque(s) transmise(s) à l'agent",
                    consultation.model,
                    findings.lines().count()
                ));
                Some(project::design_review_feedback(
                    &consultation.model,
                    &findings,
                ))
            }
        }
    }

    /// Après les appels d'une réponse, en mode projet : cahier des charges et
    /// plan relus, jalon marqué fait vérifié tout de suite. Remarques au modèle.
    fn project_after_tools(
        &mut self,
        aids: &mut TurnAids,
        prompter: &mut Option<&mut dyn PermissionPrompter>,
    ) -> Option<String> {
        let mut turn = aids.project.take()?;
        let (mut notes, _) = self.project_checks(&mut turn, aids, prompter, false);
        notes.extend(self.design_review(&mut turn));
        aids.project = Some(turn);
        (!notes.is_empty()).then(|| notes.join("\n\n"))
    }

    /// Ce qui empêche Claw de suivre les fichiers du projet, une remarque par
    /// problème nouveau. Le journal des décisions s'écrit juste après le plan :
    /// il est réclamé une fois un jalon entamé, ou quand l'agent dit avoir fini.
    fn project_structure_notes(
        &mut self,
        current: &project::Project,
        turn: &mut project::Turn,
        at_end: bool,
    ) -> Vec<String> {
        let mut problems = Vec::new();
        if let Some(spec) = &current.spec {
            problems.push((project::SPEC_FILE, project::spec_problems(spec)));
        }
        if let Some(plan) = &current.plan {
            problems.push((
                project::PLAN_FILE,
                project::plan_problems(plan, current.spec.as_ref()),
            ));
        }
        if at_end || current.milestones().iter().any(|milestone| milestone.done) {
            problems.push((
                project::DECISIONS_FILE,
                project::decisions_problems(current),
            ));
        }
        let mut notes = Vec::new();
        for (file, found) in problems {
            if found.is_empty() {
                continue;
            }
            let note = project::structure_note(file, &found);
            if turn.noted.insert(project::note_key(&note)) {
                self.api_client.notify(&format!(
                    "Mode projet : {file} est à compléter, signalé à l'agent"
                ));
                notes.push(note);
            }
        }
        notes
    }

    /// Contrôles du mode projet : forme de `SPEC.md` et de `PLAN.md` (une
    /// remarque par problème nouveau), vérification des jalons marqués faits
    /// pendant le tour (refaite après un échec si le projet a changé depuis).
    /// En fin de tour (`at_end`), un jalon toujours marqué fait après un échec
    /// est rappelé. Renvoie les remarques, et vrai si l'une demande une correction.
    fn project_checks(
        &mut self,
        turn: &mut project::Turn,
        aids: &mut TurnAids,
        prompter: &mut Option<&mut dyn PermissionPrompter>,
        at_end: bool,
    ) -> (Vec<String>, bool) {
        let current = project::Project::read(&turn.root);
        let mut notes = self.project_structure_notes(&current, turn, at_end);
        let mut to_fix = !notes.is_empty();
        for milestone in current.milestones() {
            if !milestone.done || turn.done_at_start.contains(&milestone.id) {
                continue;
            }
            let key = project::check_key(milestone);
            // Après trop d'échecs, plus de nouvel essai à chaque modification du projet
            // (seulement si la commande de vérification change).
            let failures = turn.failures.get(&milestone.id).copied().unwrap_or(0);
            let due = match turn.checked.get(&milestone.id) {
                None => true,
                Some((known, passed)) => {
                    *known != key
                        || (!passed
                            && failures < project::MAX_MILESTONE_CHECKS
                            && turn
                                .failed_at
                                .get(&milestone.id)
                                .is_some_and(|at| aids.project_changes > *at))
                }
            };
            if !due {
                let failing = turn
                    .checked
                    .get(&milestone.id)
                    .is_some_and(|(_, passed)| !passed);
                if at_end && failing {
                    notes.push(project::still_failing_note(&milestone.id));
                    to_fix = true;
                }
                continue;
            }
            if milestone.verify.is_empty() {
                turn.checked.insert(milestone.id.clone(), (key, false));
                turn.failed_at
                    .insert(milestone.id.clone(), aids.project_changes);
                notes.push(project::milestone_without_check_note(&milestone.id));
                to_fix = true;
                continue;
            }
            match self.run_milestone_check(aids, milestone, prompter) {
                // Pas lancée (refus, demande sans réponse) : déjà dit à l'utilisateur.
                None => {
                    turn.checked.insert(milestone.id.clone(), (key, true));
                }
                Some(Err(note)) => {
                    turn.checked.insert(milestone.id.clone(), (key, false));
                    turn.failed_at
                        .insert(milestone.id.clone(), aids.project_changes);
                    let failures = turn.failures.entry(milestone.id.clone()).or_insert(0);
                    *failures += 1;
                    if *failures >= project::MAX_MILESTONE_CHECKS {
                        self.api_client.notify(&format!(
                            "Le jalon {} échoue encore à sa vérification ({} essais) : Claw ne la relance plus dans ce tour, l'agent doit le remettre à faire ou dire ce qui bloque",
                            milestone.id, failures
                        ));
                        notes.push(format!(
                            "{note}\n\n{}",
                            project::milestone_blocked_note(&milestone.id, *failures)
                        ));
                    } else {
                        self.api_client.notify(&format!(
                            "Le jalon {} est marqué fait mais sa vérification échoue : renvoyé à l'agent",
                            milestone.id
                        ));
                        notes.push(note);
                    }
                    to_fix = true;
                }
                Some(Ok(())) => {
                    turn.checked.insert(milestone.id.clone(), (key, true));
                    turn.failed_at.remove(&milestone.id);
                    if !turn.completed.contains(&milestone.id) {
                        turn.completed.push(milestone.id.clone());
                    }
                    // Ce qu'a coûté le jalon dans ce tour, pour le budget des suivants.
                    let spent = aids.requests.saturating_sub(turn.requests_at_milestone);
                    turn.requests_at_milestone = aids.requests;
                    let mut state = project::ProjectState::load(&turn.root);
                    state.milestone_requests.insert(
                        milestone.id.clone(),
                        u64::try_from(spent).unwrap_or(u64::MAX),
                    );
                    let _ = state.save(&turn.root);
                    self.api_client.notify(&format!(
                        "Jalon {} vérifié : sa commande de vérification réussit ({spent} requête{} dans ce tour)",
                        milestone.id,
                        if spent > 1 { "s" } else { "" }
                    ));
                    let untested = project::untested(&turn.root, &milestone.items);
                    notes.push(project::milestone_checked_note(
                        &milestone.id,
                        &milestone.verify,
                        &untested,
                    ));
                }
            }
        }
        (notes, to_fix)
    }

    /// Lance les commandes `Verify` d'un jalon ; celles que le modèle a déjà
    /// lancées avec succès depuis sa dernière modification comptent. `None` :
    /// elles n'ont pas pu tourner (refus, demande d'autorisation sans réponse).
    fn run_milestone_check(
        &mut self,
        aids: &mut TurnAids,
        milestone: &project::Milestone,
        prompter: &mut Option<&mut dyn PermissionPrompter>,
    ) -> Option<Result<(), String>> {
        for (index, command) in milestone.verify.iter().enumerate() {
            if aids
                .verified_commands
                .iter()
                .any(|ran| harness::covers(ran, command))
            {
                continue;
            }
            self.api_client.notify(&format!(
                "Mode projet : vérification du jalon {} : {command}",
                milestone.id
            ));
            let input = serde_json::json!({
                "command": command,
                "timeout": harness::VERIFY_TIMEOUT_MS,
                "run_in_background": false,
                "description": format!("Vérification du jalon {} par Claw", milestone.id),
            })
            .to_string();
            self.tool_executor.announce("bash", &input);
            let (message, _) = self.execute_tool_call(
                format!("claw-milestone-{}-{index}", milestone.id),
                "bash".to_string(),
                &input,
                prompter
                    .as_mut()
                    .map(|prompt| &mut **prompt as &mut dyn PermissionPrompter),
            );
            let (output, is_error) = tool_result_parts(&message)?;
            if is_error {
                self.api_client.notify(&format!(
                    "Vérification du jalon {} non lancée ({command}) : le jalon n'est pas vérifié",
                    milestone.id
                ));
                return None;
            }
            let result = harness::read_bash_output(output);
            if !result.passed || result.pending {
                return Some(Err(project::milestone_failed_note(
                    &milestone.id,
                    command,
                    &result.excerpt,
                    result.timed_out || result.pending,
                )));
            }
            aids.verified_commands.push(command.clone());
        }
        Some(Ok(()))
    }

    /// Mode projet, quand l'agent dit avoir fini : jalons vérifiés et forme
    /// des fichiers du projet, cahier des charges d'abord, tests d'acceptation ;
    /// sans personne pour répondre, le plan est suivi jusqu'au bout.
    fn project_gate(
        &mut self,
        aids: &mut TurnAids,
        answer: &str,
        prompter: &mut Option<&mut dyn PermissionPrompter>,
    ) -> Option<String> {
        if aids.project.is_none() || self.subagent || interrupt_requested() || aids.verify_failing {
            return None;
        }
        let mut turn = aids.project.take()?;
        let feedback = self.project_gate_for(&mut turn, aids, answer, prompter);
        aids.project = Some(turn);
        feedback.map(|feedback| format!("{feedback}\n\n{}", harness::FINAL_ANSWER_RULE))
    }

    fn project_gate_for(
        &mut self,
        turn: &mut project::Turn,
        aids: &mut TurnAids,
        answer: &str,
        prompter: &mut Option<&mut dyn PermissionPrompter>,
    ) -> Option<String> {
        let (notes, to_fix) = self.project_checks(turn, aids, prompter, true);
        if to_fix {
            turn.gate_rounds += 1;
            if turn.gate_rounds > project::MAX_GATE_ROUNDS {
                self.api_client.notify(
                    "Mode projet : des contrôles échouent encore après plusieurs renvois : le travail n'est pas validé",
                );
                return None;
            }
            return Some(notes.join("\n\n"));
        }
        let current = project::Project::read(&turn.root);
        // Jalons terminés dont des lignes n'ont toujours aucun test (une fois).
        let untested: Vec<String> = current
            .milestones()
            .iter()
            .filter(|milestone| turn.completed.contains(&milestone.id))
            .flat_map(|milestone| project::untested(&turn.root, &milestone.items))
            .collect();
        if !untested.is_empty() && turn.asked.insert("tests".to_string()) {
            self.api_client.notify(
                "Mode projet : des exigences de jalons faits n'ont aucun test : Claw demande à l'agent de les écrire",
            );
            return Some(project::untested_feedback(&untested));
        }
        let can_ask = self.can_ask_now();
        match current.phase() {
            project::Phase::Spec => {
                let changed = !aids.originals.is_empty();
                if (changed || !can_ask) && turn.asked.insert("spec".to_string()) {
                    self.api_client.notify(
                        "Mode projet : Claw demande à l'agent d'écrire d'abord le cahier des charges (SPEC.md)",
                    );
                    return Some(project::spec_first_feedback(changed));
                }
                None
            }
            project::Phase::Plan if can_ask => {
                let confirm = !turn.spec_at_start
                    && !aids.asked
                    && !harness::ends_with_questions(answer)
                    && turn.asked.insert("confirm-spec".to_string());
                confirm.then(|| {
                    self.api_client.notify(
                        "Mode projet : Claw demande à l'agent de faire valider le cahier des charges",
                    );
                    project::confirm_spec_feedback()
                })
            }
            project::Phase::Plan => turn.asked.insert("plan".to_string()).then(|| {
                self.api_client.notify(
                    "Mode projet : cahier des charges écrit : Claw demande à l'agent d'écrire le plan",
                );
                project::plan_next_feedback()
            }),
            project::Phase::Build(index) if !can_ask => {
                let id = current.milestones()[index].id.clone();
                turn.asked.insert(format!("continue-{id}")).then(|| {
                    self.api_client.notify(&format!(
                        "Mode projet : jalons restants : Claw renvoie l'agent au jalon {id}"
                    ));
                    project::continue_feedback(&current, index)
                })
            }
            project::Phase::Finish => self.final_demo(turn, &current, prompter),
            project::Phase::Build(_) | project::Phase::Done => None,
        }
    }

    /// Mode projet, tous les jalons faits : Claw copie le projet dans un dossier
    /// vide et y lance les lignes `Install` puis `Run` de la démonstration, comme
    /// le fera l'utilisateur. Réussie, elle est notée pour ce plan et le README
    /// doit montrer les mêmes commandes ; en échec, le modèle corrige (trois
    /// fois au plus).
    fn final_demo(
        &mut self,
        turn: &mut project::Turn,
        current: &project::Project,
        prompter: &mut Option<&mut dyn PermissionPrompter>,
    ) -> Option<String> {
        let plan = current.plan.as_ref()?;
        let Some(demo) = plan.demo.as_ref().filter(|demo| !demo.run.is_empty()) else {
            return turn.asked.insert("demo".to_string()).then(|| {
                self.api_client.notify(
                    "Mode projet : tous les jalons sont faits mais le plan ne dit pas comment montrer le projet : demandé à l'agent",
                );
                project::demo_missing_feedback()
            });
        };
        if turn.demo_rounds >= project::MAX_DEMO_ROUNDS {
            return None;
        }
        let copy = match project::clean_copy(&turn.root) {
            Ok(copied) => copied,
            Err(reason) => {
                turn.demo_rounds = project::MAX_DEMO_ROUNDS;
                self.api_client.notify(&format!(
                    "Mode projet : démonstration finale impossible, le projet n'a pas pu être copié ({reason})"
                ));
                return None;
            }
        };
        let left_out = if copy.left_out.is_empty() {
            String::new()
        } else {
            format!(
                " ; laissées de côté comme données du programme : {}",
                copy.left_out.join(", ")
            )
        };
        self.api_client.notify(&format!(
            "Mode projet : démonstration finale dans une copie vide du projet ({} fichiers{left_out})",
            copy.files
        ));
        let outcome = self.run_demo(&copy, demo, prompter);
        let _ = std::fs::remove_dir_all(&copy.folder);
        match outcome {
            DemoOutcome::NotRun => {
                turn.demo_rounds = project::MAX_DEMO_ROUNDS;
                None
            }
            DemoOutcome::Failed(feedback) => {
                turn.demo_rounds += 1;
                self.api_client.notify(if turn.demo_rounds >= project::MAX_DEMO_ROUNDS {
                    "La démonstration finale échoue encore : renvoyée une dernière fois à l'agent, le projet n'est pas validé"
                } else {
                    "La démonstration finale échoue dans une copie vide du projet : renvoyée à l'agent"
                });
                Some(feedback)
            }
            DemoOutcome::Passed => {
                let mut state = current.state.clone();
                state.demo_passed = Some(plan.fingerprint());
                let _ = state.save(&turn.root);
                self.api_client
                    .notify("Démonstration finale réussie dans une copie vide du projet");
                (!project::readme_shows(&turn.root, &demo.run)
                    && turn.asked.insert("readme".to_string()))
                .then(|| {
                    self.api_client.notify(
                        "Le README ne montre pas les commandes de la démonstration : demandé à l'agent",
                    );
                    project::readme_feedback(&demo.run)
                })
            }
        }
    }

    /// Lignes `Install` puis `Run` de la démonstration, lancées dans la copie
    /// vide du projet.
    fn run_demo(
        &mut self,
        copy: &project::CleanCopy,
        demo: &project::Demo,
        prompter: &mut Option<&mut dyn PermissionPrompter>,
    ) -> DemoOutcome {
        let place = copy.folder.to_string_lossy().replace('\\', "/");
        // Une seule session, comme l'utilisateur qui tape les lignes à la suite.
        let input = serde_json::json!({
            "command": project::demo_script(&place, demo),
            "timeout": harness::VERIFY_TIMEOUT_MS + project::DEMO_RUN_TIMEOUT_MS,
            "run_in_background": false,
            "description": "Démonstration finale par Claw",
        })
        .to_string();
        self.tool_executor.announce("bash", &input);
        let (message, _) = self.execute_tool_call(
            "claw-demo".to_string(),
            "bash".to_string(),
            &input,
            prompter
                .as_mut()
                .map(|prompt| &mut **prompt as &mut dyn PermissionPrompter),
        );
        let Some((output, is_error)) = tool_result_parts(&message) else {
            return DemoOutcome::NotRun;
        };
        if is_error {
            self.api_client
                .notify("Démonstration finale non lancée : le projet n'est pas démontré");
            return DemoOutcome::NotRun;
        }
        let result = harness::read_bash_output(output);
        if result.passed && !result.pending {
            return DemoOutcome::Passed;
        }
        let shown = demo
            .install
            .iter()
            .chain(&demo.run)
            .map(|command| project::without_comment(command))
            .collect::<Vec<_>>()
            .join(" && ");
        DemoOutcome::Failed(project::demo_failed_feedback(
            copy,
            &shown,
            &result.excerpt,
            result.timed_out || result.pending,
        ))
    }

    /// Mode projet, quand quelqu'un peut répondre : un jalon terminé pendant le
    /// tour est à faire essayer à l'utilisateur, qui dit s'il convient (une fois).
    fn milestone_validation_gate(&mut self, aids: &mut TurnAids, answer: &str) -> Option<String> {
        if self.subagent
            || interrupt_requested()
            || aids.asked
            || !self.can_ask_now()
            || harness::ends_with_questions(answer)
        {
            return None;
        }
        let turn = aids.project.as_mut()?;
        if turn.completed.is_empty() || turn.validation_asked {
            return None;
        }
        turn.validation_asked = true;
        let current = project::Project::read(&turn.root);
        let next = current
            .milestones()
            .iter()
            .find(|milestone| !milestone.done)
            .map(|milestone| milestone.id.clone());
        let feedback = project::validation_feedback(&turn.completed, next.as_deref());
        self.api_client.notify(
            "Mode projet : jalon terminé : Claw demande à l'agent de le faire essayer à l'utilisateur",
        );
        Some(feedback)
    }

    /// Les lignes du cahier des charges des jalons faits, pour le contrôle des
    /// exigences et la relecture.
    fn project_reference(aids: &TurnAids) -> Option<String> {
        let turn = aids.project.as_ref()?;
        project::reference_for_checks(&project::Project::read(&turn.root))
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
        let reference = Self::project_reference(aids);
        let prompt =
            harness::review_prompt_with(request, earlier, answer, &diffs, reference.as_deref());
        let mut consultation =
            self.api_client
                .consult(ConsultPurpose::Review, harness::REVIEW_SYSTEM, &prompt)?;
        harness::log_review(&consultation.model, &consultation.text);
        // Une approbation sans aucune analyse ne prouve pas que le travail a
        // été relu (rejoué sur un vrai travail : « LGTM » en 8 s, quand un
        // autre relecteur y trouvait un bogue) : un second relecteur la remplace.
        if harness::bare_approval(&consultation.text) {
            self.api_client.notify(&format!(
                "Relecture par {} : approbation sans analyse, un autre modèle relit",
                consultation.model
            ));
            if let Some(second) = self.api_client.consult_again(
                ConsultPurpose::Review,
                harness::REVIEW_SYSTEM,
                &prompt,
                &consultation.model,
            ) {
                harness::log_review(&second.model, &second.text);
                if harness::bare_approval(&second.text) {
                    self.api_client.notify(&format!(
                        "Relecture par {} : approbation sans analyse elle aussi ; le travail n'a pas vraiment été relu",
                        second.model
                    ));
                }
                consultation = second;
            }
        }
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
    /// Joint au dernier résultat d'outil de la réponse les captures prises par
    /// ses appels : les résultats d'outils restent d'un seul tenant (des images
    /// placées entre deux résultats feraient refuser la requête par une API de
    /// type `OpenAI`, qui veut les résultats juste après les appels).
    fn attach_tool_images(&mut self, tool_results: &mut [ConversationMessage]) {
        if self.tool_images.is_empty() {
            return;
        }
        let images = std::mem::take(&mut self.tool_images);
        self.forget_old_tool_images(images.len());
        let blocks: Vec<ContentBlock> = images
            .into_iter()
            .map(|image| ContentBlock::Image {
                media_type: image.media_type,
                data: image.data,
            })
            .collect();
        if let Some(last) = self
            .session
            .messages
            .last_mut()
            .filter(|message| message.role == crate::session::MessageRole::Tool)
        {
            last.blocks.extend(blocks.iter().cloned());
        }
        if let Some(last) = tool_results.last_mut() {
            last.blocks.extend(blocks);
        }
    }

    /// Chaque image renvoyée au modèle compte à chaque requête : au-delà des
    /// `MAX_TOOL_IMAGES` captures les plus récentes (`coming` comprises), les
    /// plus anciennes sont retirées et leur résultat le dit au modèle.
    fn forget_old_tool_images(&mut self, coming: usize) {
        let mut kept = coming;
        for message in self.session.messages.iter_mut().rev() {
            if message.role != crate::session::MessageRole::Tool {
                continue;
            }
            let images = message
                .blocks
                .iter()
                .filter(|block| matches!(block, ContentBlock::Image { .. }))
                .count();
            if images == 0 {
                continue;
            }
            if kept + images <= MAX_TOOL_IMAGES {
                kept += images;
                continue;
            }
            message
                .blocks
                .retain(|block| !matches!(block, ContentBlock::Image { .. }));
            append_tool_output(message, OLD_SCREENSHOTS_NOTE);
            kept = MAX_TOOL_IMAGES;
        }
    }

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
        // Tour de question : les fichiers restent tels quels.
        if self.question_turn && crate::file_tools::writes_files(&tool_name) {
            let problem = harness::QUESTION_WRITE_REFUSED.to_string();
            self.tool_executor.skipped(&tool_name, input, &problem);
            let message = ConversationMessage::tool_result(tool_use_id, tool_name, problem, true);
            return (message, false);
        }
        // `PreToolUse` d'abord, comme dans Claude Code : un hook peut refuser,
        // accorder (`allow`), faire confirmer (`ask`) ou corriger les arguments.
        let pre_hook_result = self.hook_runner.run_pre_tool_use(&tool_name, input);
        if pre_hook_result.is_denied() {
            let deny_message = format!("hook PreToolUse : outil `{tool_name}` refusé");
            let reason = format_hook_message(&pre_hook_result, &deny_message);
            self.tool_executor.skipped(&tool_name, input, &reason);
            let message = ConversationMessage::tool_result(tool_use_id, tool_name, reason, true);
            return (message, true);
        }
        self.note_hook_stop(&pre_hook_result);
        let updated = pre_hook_result.updated_input().map(ToOwned::to_owned);
        let input = updated.as_deref().unwrap_or(input);
        let mut notifying = prompter.map(|prompt| NotifyingPrompter {
            hooks: &self.hook_runner,
            inner: prompt,
        });
        let permission_outcome = authorize_after_hook(
            &mut self.permission_policy,
            pre_hook_result.permission_decision(),
            &tool_name,
            input,
            notifying
                .as_mut()
                .map(|prompt| prompt as &mut dyn PermissionPrompter),
        );
        match permission_outcome {
            PermissionOutcome::Allow => {
                // La même capture, demandée par le modèle sans qu'aucun fichier
                // ait pu changer depuis : son résultat, sans refaire le rendu.
                // Vu le 26 octobre 2026 (session #31, passage 4) : 74 captures
                // du modèle, souvent deux de suite sans rien changer entre elles.
                let model_capture = tool_name == "Screenshot"
                    && !tool_use_id.starts_with("claw-")
                    && !self.api_client.reads_images();
                let capture_key = model_capture.then(|| capture_key(input)).flatten();
                if let (Some(key), Some((last, output))) = (&capture_key, &self.last_capture) {
                    if key == last {
                        let output = format!("{}\n{output}", harness::SAME_CAPTURE_NOTE);
                        let message =
                            ConversationMessage::tool_result(tool_use_id, tool_name, output, false);
                        return (message, false);
                    }
                }
                crate::checkpoint::record_before_write(&tool_name, input);
                let (mut output, mut is_error) = match self.tool_executor.execute(&tool_name, input)
                {
                    Ok(output) => (output, false),
                    Err(error) => (error.to_string(), true),
                };
                if !harness::leaves_files_alone(&tool_name) {
                    self.last_capture = None;
                }
                if let (Some(key), false) = (capture_key, is_error) {
                    self.last_capture = Some((key, output.clone()));
                }
                self.tool_images.extend(self.tool_executor.take_images());
                output = merge_hook_feedback(pre_hook_result.messages(), output, false);

                let post_hook_result = self
                    .hook_runner
                    .run_post_tool_use(&tool_name, input, &output, is_error);
                self.note_hook_stop(&post_hook_result);
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
        final_stop: bool,
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
        if final_stop {
            self.api_client.notify(&format!(
                "hook {} : limite de requêtes du tour atteinte, l'agent s'arrête ({reason})",
                event.as_str()
            ));
            return None;
        }
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

    /// Compaction manuelle (`/compact [consigne]`) : la conversation est
    /// résumée par le modèle, qui suit la consigne de l'utilisateur (ce qu'il
    /// faut garder) ; à défaut de résumé utilisable, celui de Claw le remplace.
    /// Les messages récents restent tels quels. La session est remplacée et le
    /// résultat renvoyé.
    pub fn compact_with_model(
        &mut self,
        config: CompactionConfig,
        instructions: Option<&str>,
    ) -> CompactionResult {
        let instructions = instructions.map(str::trim).filter(|text| !text.is_empty());
        let pre_compact = self.hook_runner.run_event(
            HookEvent::PreCompact,
            Some("manual"),
            serde_json::json!({
                "trigger": "manual",
                "custom_instructions": instructions.unwrap_or_default(),
            }),
            &[],
        );
        self.notify_hook_feedback(HookEvent::PreCompact, &pre_compact);
        let system = match instructions {
            Some(focus) => format!(
                "{MODEL_SUMMARY_PROMPT}\n\nThe user asked for this compaction with these instructions; follow them when choosing what to keep: {focus}"
            ),
            None => MODEL_SUMMARY_PROMPT.to_string(),
        };
        let client = &mut self.api_client;
        let result = compact_session_with(&self.session, config, |messages| {
            let transcript = render_transcript_for_summary(messages);
            let first = client.summarize(&system, &transcript)?;
            if summary_is_usable(&first, messages) {
                return Some(first);
            }
            client.summarize(&system, &format!("{transcript}\n\n{SUMMARY_RETRY_NOTE}"))
        });
        if result.removed_message_count > 0 {
            self.session = result.compacted_session.clone();
        }
        result
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

/// Erreurs d'API signalant un dépassement de la fenêtre de contexte, dans
/// les formulations des différents fournisseurs : `OpenRouter` et `OpenAI`
/// (« maximum context length »), Anthropic (« prompt is too long »), Google
/// (« exceeds the maximum number of tokens »), Groq (413, « Request too
/// large »), Cerebras (« reduce the length of the messages »).
fn is_context_overflow(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    (lower.contains("context")
        && (lower.contains("length") || lower.contains("window") || lower.contains("too long")))
        || [
            "prompt is too long",
            "maximum context",
            "too many tokens",
            "maximum number of tokens",
            "request too large",
            "reduce the length",
            "token limit",
            "input is too long",
            "l'api a retourné 413",
        ]
        .iter()
        .any(|mark| lower.contains(mark))
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
    /// Outils lancés en arrière-plan (`spawn_background`), à résultat fixe.
    background: BTreeMap<String, String>,
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

    /// `tool_name` tourne en arrière-plan quand Claw le demande, et rend `output`.
    #[must_use]
    pub fn in_background(
        mut self,
        tool_name: impl Into<String>,
        output: impl Into<String>,
    ) -> Self {
        self.background.insert(tool_name.into(), output.into());
        self
    }
}

impl ToolExecutor for StaticToolExecutor {
    fn execute(&mut self, tool_name: &str, input: &str) -> Result<String, ToolError> {
        self.handlers
            .get_mut(tool_name)
            .ok_or_else(|| ToolError::new(format!("unknown tool: {tool_name}")))?(input)
    }

    fn spawn_background(&mut self, tool_name: &str, _input: &str) -> Option<Background> {
        let output = self.background.get(tool_name)?.clone();
        Some(std::thread::spawn(move || (Ok(output), Vec::new())))
    }
}

#[cfg(test)]
thread_local! {
    /// Annonces dans les tests du runtime : coupées, car les modèles scriptés
    /// se mettent au travail sans un mot ; les tests de l'annonce les rallument
    /// (chaque test a son propre fil).
    static TEST_ANNOUNCEMENTS: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Annonces demandées au modèle muet (toujours, hors des tests du runtime).
fn announcements_on() -> bool {
    #[cfg(test)]
    {
        TEST_ANNOUNCEMENTS.with(std::cell::Cell::get)
    }
    #[cfg(not(test))]
    {
        true
    }
}

#[cfg(test)]
// Chaque test prend d'abord le verrou commun ([`crate::test_serial`]), avant
// les types qu'il déclare.
#[allow(clippy::items_after_statements)]
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
        let _serial = crate::test_serial();
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
        let _serial = crate::test_serial();
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
        // Le refus d'abord ; la demande d'annonce de Claw peut suivre.
        assert!(matches!(
            &summary.tool_results[0].blocks[0],
            ContentBlock::ToolResult { is_error: true, output, .. } if output.starts_with("not now")
        ));
    }

    #[test]
    fn a_refused_tool_call_is_reported_to_the_interface_as_skipped() {
        let _serial = crate::test_serial();
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
    fn screenshots_follow_the_last_tool_result_and_only_the_latest_are_kept() {
        let _serial = crate::test_serial();
        /// Trois réponses à captures, la première avec un second appel après la capture.
        struct Script(usize);
        impl ApiClient for Script {
            fn stream(
                &mut self,
                _request: ApiRequest,
            ) -> Result<Vec<AssistantEvent>, RuntimeError> {
                self.0 += 1;
                let call = |id: &str, name: &str| AssistantEvent::ToolUse {
                    id: id.to_string(),
                    name: name.to_string(),
                    input: "{}".to_string(),
                };
                Ok(match self.0 {
                    1 => vec![
                        call("shot-1", "shot"),
                        call("other-1", "other"),
                        AssistantEvent::MessageStop,
                    ],
                    2 => vec![call("shot-2", "shot"), AssistantEvent::MessageStop],
                    3 => vec![call("shot-3", "shot"), AssistantEvent::MessageStop],
                    _ => vec![
                        AssistantEvent::TextDelta("Fait.".to_string()),
                        AssistantEvent::MessageStop,
                    ],
                })
            }
        }

        /// Chaque capture rend deux images (téléphone et ordinateur).
        struct Camera(Vec<crate::attachments::ImageAttachment>);
        impl super::ToolExecutor for Camera {
            fn execute(
                &mut self,
                tool_name: &str,
                _input: &str,
            ) -> Result<String, super::ToolError> {
                if tool_name == "shot" {
                    self.0 = ["phone", "desktop"]
                        .iter()
                        .map(|screen| crate::attachments::ImageAttachment {
                            media_type: "image/jpeg".to_string(),
                            data: (*screen).to_string(),
                            path: std::path::PathBuf::new(),
                        })
                        .collect();
                }
                Ok(format!("{tool_name} fait"))
            }

            fn take_images(&mut self) -> Vec<crate::attachments::ImageAttachment> {
                std::mem::take(&mut self.0)
            }
        }

        let mut runtime = ConversationRuntime::new(
            Session::new(),
            Script(0),
            Camera(Vec::new()),
            PermissionPolicy::new(PermissionMode::DangerFullAccess),
            vec!["system".to_string()],
        );
        runtime.run_turn("regarde la page", None).expect("tour");
        let results: Vec<(String, usize, String)> = runtime
            .session()
            .messages
            .iter()
            .filter(|message| message.role == MessageRole::Tool)
            .map(|message| {
                let images = message
                    .blocks
                    .iter()
                    .filter(|block| matches!(block, ContentBlock::Image { .. }))
                    .count();
                let (id, output) = message
                    .blocks
                    .iter()
                    .find_map(|block| match block {
                        ContentBlock::ToolResult {
                            tool_use_id,
                            output,
                            ..
                        } => Some((tool_use_id.clone(), output.clone())),
                        _ => None,
                    })
                    .expect("résultat");
                (id, images, output)
            })
            .collect();
        let shape: Vec<(&str, usize)> = results
            .iter()
            .map(|(id, images, _)| (id.as_str(), *images))
            .collect();
        // Les captures de la première réponse suivent son dernier résultat, puis
        // celles de la première réponse laissent la place aux deux suivantes.
        assert_eq!(
            shape,
            [("shot-1", 0), ("other-1", 0), ("shot-2", 2), ("shot-3", 2)]
        );
        assert!(results[1]
            .2
            .contains("screenshots of this call were removed"));
        assert!(!results[2].2.contains("removed"));
    }

    #[test]
    fn a_call_with_unusable_arguments_fails_without_asking_for_permission() {
        let _serial = crate::test_serial();
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
        let _serial = crate::test_serial();
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
            .run_turn("ajoute un titre", None)
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
        let _serial = crate::test_serial();
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
        let _serial = crate::test_serial();
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
        let _serial = crate::test_serial();
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
        let _serial = crate::test_serial();
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

    /// Client qui demande deux appels d'outil, puis répond par du texte ;
    /// compte ses requêtes.
    struct TwoToolsApiClient {
        requests: std::rc::Rc<std::cell::Cell<usize>>,
    }

    impl ApiClient for TwoToolsApiClient {
        fn stream(&mut self, _request: ApiRequest) -> Result<Vec<AssistantEvent>, RuntimeError> {
            self.requests.set(self.requests.get() + 1);
            if self.requests.get() > 1 {
                return Ok(vec![
                    AssistantEvent::TextDelta("fini".to_string()),
                    AssistantEvent::MessageStop,
                ]);
            }
            Ok(vec![
                AssistantEvent::ToolUse {
                    id: "tool-1".to_string(),
                    name: "echo".to_string(),
                    input: r#"{"text":"original"}"#.to_string(),
                },
                AssistantEvent::ToolUse {
                    id: "tool-2".to_string(),
                    name: "echo".to_string(),
                    input: r#"{"text":"second"}"#.to_string(),
                },
                AssistantEvent::MessageStop,
            ])
        }
    }

    fn tool_output(message: &crate::session::ConversationMessage) -> &str {
        match &message.blocks[0] {
            ContentBlock::ToolResult { output, .. } => output.as_str(),
            _ => panic!("résultat d'outil attendu"),
        }
    }

    #[test]
    fn pre_tool_hooks_can_allow_and_rewrite_a_call() {
        let _serial = crate::test_serial();
        let requests = std::rc::Rc::new(std::cell::Cell::new(0));
        let hook = r#"printf '{"hookSpecificOutput":{"permissionDecision":"allow","updatedInput":{"text":"corrigé"}}}'"#;
        let mut runtime = ConversationRuntime::new_with_features(
            Session::new(),
            TwoToolsApiClient {
                requests: requests.clone(),
            },
            StaticToolExecutor::new().register("echo", |input| Ok(input.to_string())),
            // Mode `prompt` sans invite : sans le hook, l'appel serait refusé.
            PermissionPolicy::new(PermissionMode::Prompt),
            vec!["system".to_string()],
            RuntimeFeatureConfig::default()
                .with_hooks(RuntimeHookConfig::new(vec![hook.to_string()], Vec::new())),
        );
        let summary = runtime.run_turn("fais-le", None).expect("tour");
        assert_eq!(summary.tool_results.len(), 2);
        for result in &summary.tool_results {
            assert!(
                tool_output(result).contains("corrigé"),
                "{}",
                tool_output(result)
            );
        }
        assert_eq!(requests.get(), 2);
    }

    #[test]
    fn a_hook_with_continue_false_stops_the_turn() {
        let _serial = crate::test_serial();
        let requests = std::rc::Rc::new(std::cell::Cell::new(0));
        let hook = r#"printf '{"continue":false,"stopReason":"arrêt demandé"}'"#;
        let mut runtime = ConversationRuntime::new_with_features(
            Session::new(),
            TwoToolsApiClient {
                requests: requests.clone(),
            },
            StaticToolExecutor::new().register("echo", |input| Ok(input.to_string())),
            PermissionPolicy::new(PermissionMode::DangerFullAccess),
            vec!["system".to_string()],
            RuntimeFeatureConfig::default()
                .with_hooks(RuntimeHookConfig::new(Vec::new(), vec![hook.to_string()])),
        );
        let summary = runtime.run_turn("fais-le", None).expect("tour");
        assert_eq!(summary.tool_results.len(), 2);
        assert!(tool_output(&summary.tool_results[0]).contains("original"));
        assert!(
            tool_output(&summary.tool_results[1]).contains("a hook stopped the agent"),
            "{}",
            tool_output(&summary.tool_results[1])
        );
        assert_eq!(requests.get(), 1, "le modèle n'est plus rappelé");
    }

    #[test]
    fn appends_post_tool_hook_feedback_to_tool_result() {
        let _serial = crate::test_serial();
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
        let _serial = crate::test_serial();
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
        let _serial = crate::test_serial();
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
        /// Réponses aux demandes de résumé, avant `MODEL_SUMMARY`.
        summary_replies: std::collections::VecDeque<&'static str>,
        notices: Vec<String>,
        message_counts: Vec<usize>,
    }

    /// Résumé rendu par le modèle factice : il porte les rubriques demandées.
    const MODEL_SUMMARY: &str = "RÉSUMÉ-MODÈLE\n1. Demandes : refactoriser le module api.\n2. Décisions : aucune.\n6. Prochaine étape : continuer.";

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
            Some(
                self.summary_replies
                    .pop_front()
                    .unwrap_or(MODEL_SUMMARY)
                    .to_string(),
            )
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
                summary_replies: std::collections::VecDeque::new(),
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
        let _serial = crate::test_serial();
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
    fn a_summary_that_continues_the_conversation_is_asked_again_then_replaced() {
        let _serial = crate::test_serial();
        let config = Some(super::AutoCompactionConfig {
            threshold_tokens: 1_000,
            preserve_recent_messages: 4,
        });
        // Le modèle continue la conversation au lieu de la résumer : Claw
        // redemande une fois, plus nettement.
        let mut runtime = compacting_runtime(long_session(10), false).with_auto_compaction(config);
        runtime
            .api_client
            .summary_replies
            .push_back("L'installation est en cours. Une fois terminée : uvicorn app.main:app");
        runtime
            .run_turn("continue", None)
            .expect("turn should succeed");
        assert_eq!(runtime.api_client.summaries.len(), 2);
        assert!(runtime.api_client.summaries[0].ends_with("6. Next step.]"));
        assert!(runtime.api_client.summaries[1].ends_with(crate::compact::SUMMARY_RETRY_NOTE));
        assert!(matches!(
            &runtime.session().messages[0].blocks[0],
            ContentBlock::Text { text } if text.contains("RÉSUMÉ-MODÈLE")
                && !text.contains("installation")
                && text.contains("- demande 0 ")
        ));

        // Deux fois de suite : le résumé de Claw, avec les demandes mot pour mot.
        let mut runtime = compacting_runtime(long_session(10), false).with_auto_compaction(config);
        runtime
            .api_client
            .summary_replies
            .extend(["Je continue le travail.", "Voici la suite."]);
        runtime
            .run_turn("continue", None)
            .expect("turn should succeed");
        assert_eq!(runtime.api_client.summaries.len(), 2);
        assert!(matches!(
            &runtime.session().messages[0].blocks[0],
            ContentBlock::Text { text } if text.contains("Conversation summary")
                && text.contains("[Recorded by Claw, exact]")
                && text.contains("- demande 0 ")
                && !text.contains("Je continue")
        ));
    }

    #[test]
    fn results_stored_before_leaning_are_leaned_when_the_session_is_opened() {
        let _serial = crate::test_serial();
        // Session enregistrée avant l'allègement : l'ancien et le nouveau
        // texte, et le fichier entier, repartaient avec chaque requête.
        let fat = serde_json::json!({
            "filePath": "page.tsx",
            "oldString": "a",
            "newString": "b",
            "originalFile": "x".repeat(5_000),
            "structuredPatch": [{
                "oldStart": 3, "oldLines": 1, "newStart": 3, "newLines": 1,
                "lines": ["-a", "+b"]
            }]
        })
        .to_string();
        let mut session = Session::new();
        session
            .messages
            .push(crate::session::ConversationMessage::user_text("change a"));
        session
            .messages
            .push(crate::session::ConversationMessage::assistant(vec![
                ContentBlock::ToolUse {
                    id: "t1".to_string(),
                    name: "edit_file".to_string(),
                    input: "{}".to_string(),
                },
            ]));
        session
            .messages
            .push(crate::session::ConversationMessage::tool_result(
                "t1",
                "edit_file",
                fat,
                false,
            ));
        let runtime = compacting_runtime(session, false);
        let ContentBlock::ToolResult { output, .. } = &runtime.session().messages[2].blocks[0]
        else {
            panic!("résultat d'outil attendu");
        };
        assert!(output.len() < 400, "{output}");
        assert!(output.contains("page.tsx"));
        assert!(output.contains("\"newStart\": 3"), "{output}");
        assert!(!output.contains("originalFile"));
    }

    #[test]
    fn does_not_compact_below_the_threshold_or_when_disabled() {
        let _serial = crate::test_serial();
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
        let _serial = crate::test_serial();
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
        /// Rang des requêtes parties en texte seul (outils coupés).
        text_only: Vec<usize>,
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
        /// Comptes rendus gardés après un simple accusé de réception.
        kept: usize,
        /// Rôles annoncés au client en mode équipe, dans l'ordre.
        team_steps: Vec<String>,
    }

    /// Rallume les annonces le temps d'un test.
    struct AnnouncementsOn;

    impl AnnouncementsOn {
        fn new() -> Self {
            super::TEST_ANNOUNCEMENTS.with(|on| on.set(true));
            Self
        }
    }

    impl Drop for AnnouncementsOn {
        fn drop(&mut self) {
            super::TEST_ANNOUNCEMENTS.with(|on| on.set(false));
        }
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
            let mut observed = self.observed.borrow_mut();
            if request.text_only {
                let rank = observed.requests.len();
                observed.text_only.push(rank);
            }
            observed.requests.push(format!("{:?}", request.messages));
            drop(observed);
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

        fn consult_again(
            &mut self,
            purpose: super::ConsultPurpose,
            system: &str,
            prompt: &str,
            _excluded: &str,
        ) -> Option<super::Consultation> {
            self.consult(purpose, system, prompt)
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

        fn answer_kept(&mut self) {
            self.observed.borrow_mut().kept += 1;
        }

        fn reply_cut_by_output_limit(&mut self) -> bool {
            CUT_REPLIES.with(|cut| {
                let left = cut.get();
                cut.set(left.saturating_sub(1));
                left > 0
            })
        }

        fn can_ask_user(&self) -> bool {
            CAN_ASK.with(std::cell::Cell::get)
        }

        fn free_requests_left(&mut self) -> Option<u64> {
            FREE_LEFT.with(std::cell::Cell::get)
        }

        fn image_defects(
            &mut self,
            _images: &[crate::attachments::ImageAttachment],
        ) -> Vec<(usize, String)> {
            DEFECTS.with(|defects| defects.borrow().clone())
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
                Ok(if ASK_LATER.with(std::cell::Cell::get) {
                    crate::harness::QUESTIONS_DEFERRED.to_string()
                } else {
                    r#"{"answers":[{"question":"Continuer ?","answer":"Oui"}]}"#.to_string()
                })
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
    fn a_design_plan_is_reviewed_once_before_the_first_interface_file() {
        let _serial = crate::test_serial();
        let plan_and_write = |path: &str| {
            vec![
                AssistantEvent::TextDelta(
                    "Plan : #F5E6D3 sable, #8B4A3A ocre, #6B8E7A sauge. Typo : Fraunces et Karla."
                        .to_string(),
                ),
                AssistantEvent::ToolUse {
                    id: format!("appel-{path}"),
                    name: "write_file".to_string(),
                    input: format!(r#"{{"path":"{path}","content":"<p>x</p>"}}"#),
                },
                AssistantEvent::MessageStop,
            ]
        };
        let (mut runtime, observed, _) = aided_runtime_with(
            vec![
                plan_and_write("site-essai.html"),
                plan_and_write("site-essai.css"),
            ],
            Vec::new(),
            vec!["1. [identity] centered hero -> a split hero with a pressed-flower SVG"],
            WRITTEN.to_string(),
        );
        // Le tour s'arrête faute de réponses scriptées : seul compte ce qui précède.
        let _ = runtime.run_turn("Crée un site vitrine pour un fleuriste", None);
        let observed = observed.borrow();
        let reviews: Vec<&String> = observed
            .consults
            .iter()
            .filter(|(purpose, _)| *purpose == super::ConsultPurpose::Design)
            .map(|(_, prompt)| prompt)
            .collect();
        assert_eq!(reviews.len(), 1, "une revue du plan par tour");
        assert!(reviews[0].contains("#8B4A3A") && reviews[0].contains("fleuriste"));
        assert!(
            observed.requests[1].contains("An art director (relecteur) reviewed your design plan")
                && observed.requests[1].contains("pressed-flower"),
            "la critique part avec le résultat de l'écriture"
        );
        assert!(observed
            .notices
            .iter()
            .any(|notice| notice
                == "Revue du plan par relecteur : 1 remarque(s) transmise(s) à l'agent"));
        drop(observed);

        // Sans plan (pas de palette) : aucune revue, mais le plan est demandé,
        // une fois, avec le résultat de la première écriture.
        let write = |path: &str| {
            tool_step(
                "write_file",
                &format!(r#"{{"path":"{path}","content":"<p>x</p>"}}"#),
            )
        };
        let (mut runtime, observed, _) = aided_runtime_with(
            vec![write("site-essai.html"), write("site-essai.css")],
            Vec::new(),
            vec!["1. [identity] x -> y"],
            WRITTEN.to_string(),
        );
        let _ = runtime.run_turn("Crée un site vitrine pour un fleuriste", None);
        let observed = observed.borrow();
        assert!(!observed
            .consults
            .iter()
            .any(|(purpose, _)| *purpose == super::ConsultPurpose::Design));
        assert!(observed.requests[1].contains("without the design plan the method asks for"));
        // L'historique de la requête suivante ne la porte qu'une fois.
        assert_eq!(
            observed.requests[2]
                .matches("without the design plan")
                .count(),
            1
        );
    }

    #[test]
    fn a_script_gets_its_language_rules_and_must_be_run_before_the_answer() {
        let _serial = crate::test_serial();
        let (mut runtime, observed, _) = aided_runtime(Vec::new(), Vec::new());
        let dir = std::env::temp_dir().join(format!("claw-run-scripts-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dossier");
        let script = dir.join("trier.py");
        std::fs::write(&script, "import sys\n\ndef main():\n    return 0\n\nif __name__ == '__main__':\n    sys.exit(main())\n").expect("script");
        let module = dir.join("outils.py");
        std::fs::write(&module, "def double(x):\n    return 2 * x\n").expect("module");
        let path = script.to_string_lossy().to_string();
        let mut aids = super::TurnAids::default();
        let mut notes = Vec::new();
        runtime.code_file_notes(&path, &mut aids, &mut notes);
        runtime.code_file_notes(&module.to_string_lossy(), &mut aids, &mut notes);
        // Les règles de Python une fois, le script retenu, pas le module.
        assert_eq!(notes.len(), 1, "{notes:?}");
        assert!(
            notes[0].contains("trier.py is Python code")
                && notes[0].contains("python trier.py --help")
        );
        assert_eq!(aids.scripts_written.len(), 1);
        // Jamais lancé : le modèle est renvoyé l'essayer, une fois.
        let feedback = runtime.run_scripts_gate(&mut aids).expect("à essayer");
        assert!(
            feedback.contains("You wrote trier.py in this turn but never ran it"),
            "{feedback}"
        );
        assert!(runtime.run_scripts_gate(&mut aids).is_none());
        assert!(observed
            .borrow()
            .notices
            .iter()
            .any(|notice| notice.contains("jamais lancé(s) : trier.py")));
        // Lancé : rien à redire.
        let mut aids = super::TurnAids {
            scripts_written: std::iter::once(path).collect(),
            commands_run: vec!["python Trier.py --help".to_string()],
            ..super::TurnAids::default()
        };
        assert!(runtime.run_scripts_gate(&mut aids).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_icon_drawn_by_hand_is_reported_when_the_file_is_written() {
        let _serial = crate::test_serial();
        let (mut runtime, observed, _) = aided_runtime(Vec::new(), Vec::new());
        let dir = std::env::temp_dir().join(format!("claw-drawn-icon-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dossier");
        let page = dir.join("index.html");
        std::fs::write(
            &page,
            "<button>\n<svg class=\"icon-phone\" viewBox=\"0 0 24 24\"><path d=\"M2 3c1 5 6 10 11 11l2-2\"/></svg>\n</button>",
        )
        .expect("page");
        let path = page.to_string_lossy().to_string();
        let mut aids = super::TurnAids::default();
        let mut notes = Vec::new();
        runtime.interface_file_notes(&path, &mut aids, &mut notes);
        assert_eq!(notes.len(), 1, "{notes:?}");
        assert!(
            notes[0].contains("draws 1 icon(s) by hand")
                && notes[0].contains("line 2")
                && notes[0].contains("[\"phone\"]"),
            "{}",
            notes[0]
        );
        assert!(observed.borrow().notices.iter().any(|notice| notice
            == "Icône(s) dessinée(s) à la main dans index.html : 1, signalée(s) au modèle"));
        // La même icône prise dans le jeu ne dit rien.
        let phone = crate::icon_set::LUCIDE
            .lines()
            .find(|line| line.starts_with("phone\t"))
            .and_then(|line| line.splitn(3, '\t').nth(2))
            .expect("phone");
        std::fs::write(
            &page,
            format!("<button><svg viewBox=\"0 0 24 24\">{phone}</svg></button>"),
        )
        .expect("page");
        let mut notes = Vec::new();
        runtime.interface_file_notes(&path, &mut aids, &mut notes);
        assert!(notes.is_empty(), "{notes:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_design_turn_under_the_bar_continues_while_its_score_rises() {
        let _serial = crate::test_serial();
        let (mut runtime, observed, _) = aided_runtime(Vec::new(), Vec::new());
        let graded = |mean: f32| crate::web_design::Graded {
            mean,
            under: vec!["identity".to_string()],
            scores: Vec::new(),
        };
        let mut aids = super::TurnAids {
            design_guided: true,
            ..super::TurnAids::default()
        };
        // Pas encore de note : la limite reste la limite.
        assert!(!runtime.continue_design(&mut aids, "limite du tour"));
        aids.graded.push(graded(6.0));
        assert!(runtime.continue_design(&mut aids, "limite du tour"));
        // Aucune revue depuis : pas de deuxième poursuite sur la même note.
        assert!(!runtime.continue_design(&mut aids, "limite du tour"));
        aids.graded.push(graded(6.1));
        // La note stagne : une poursuite de plus, une seule, pour changer la
        // structure (session #32, passage 5 : 6,4 puis 6,6 puis 6,6).
        assert!(runtime.continue_design(&mut aids, "revues du tour"));
        assert!(aids.restructured);
        assert!(!runtime.continue_design(&mut aids, "revues du tour"));
        aids.graded.push(graded(6.1));
        assert!(!runtime.continue_design(&mut aids, "revues du tour"));
        assert!(!runtime.continue_design(&mut aids, "revues du tour"));
        let observed = observed.borrow();
        assert!(observed.notices.iter().any(|notice| notice
            == "Poursuite du design (1/2, limite du tour) : note 6,0/10, sous la barre 8 (identity) ; le tour continue"));
        assert!(observed.notices.iter().any(|notice| notice.starts_with(
            "Design bloqué sous la barre 8 (note 6,0 puis 6,1/10) : Claw demande au juge des changements de structure"
        )));
        let stops: Vec<&String> = observed
            .notices
            .iter()
            .filter(|notice| notice.starts_with("Design arrêté sous la barre"))
            .collect();
        assert_eq!(stops.len(), 1, "l'arrêt est expliqué une fois : {stops:?}");
        assert!(stops[0].contains("6,0 puis 6,1 puis 6,1") && stops[0].contains("pas fait monter"));
        assert_eq!(aids.continuations, 1);
    }

    #[test]
    fn a_page_that_works_badly_keeps_the_turn_going_while_it_gets_better() {
        let _serial = crate::test_serial();
        let (mut runtime, observed, _) = aided_runtime(Vec::new(), Vec::new());
        let mut aids = super::TurnAids::default();
        // Pas encore de note : rien à poursuivre.
        assert!(!runtime.continue_function(&mut aids, "revues du tour"));
        aids.function_scores.push(4);
        assert!(runtime.continue_function(&mut aids, "revues du tour"));
        aids.function_scores.push(6);
        assert!(runtime.continue_function(&mut aids, "revues du tour"));
        // Les poursuites permises ont servi.
        aids.function_scores.push(7);
        assert!(!runtime.continue_function(&mut aids, "revues du tour"));
        // À la barre : rien à poursuivre.
        let mut good = super::TurnAids::default();
        good.function_scores.push(9);
        assert!(!runtime.continue_function(&mut good, "revues du tour"));
        // Note qui ne monte pas : arrêt expliqué une fois.
        let mut flat = super::TurnAids::default();
        flat.function_scores.extend([5, 5]);
        assert!(!runtime.continue_function(&mut flat, "limite du tour"));
        let observed = observed.borrow();
        assert!(observed.notices.iter().any(|notice| notice
            == "Fonctionnement 4/10, sous la barre 8 (revues du tour) : le tour continue pour le réparer"));
        assert!(observed.notices.iter().any(|notice| notice.starts_with(
            "Fonctionnement arrêté sous la barre 8 (5 puis 5/10) : la dernière revue ne l'a pas fait monter"
        )));
    }

    #[test]
    fn a_model_that_answers_nothing_twice_hands_the_turn_to_another() {
        let _serial = crate::test_serial();
        let empty = || {
            vec![
                AssistantEvent::TextDelta("\n ".to_string()),
                AssistantEvent::MessageStop,
            ]
        };
        let (mut runtime, observed, _) = aided_runtime(
            vec![empty(), empty(), text_step("Voici l'explication.")],
            Vec::new(),
        );
        runtime.api_client.escalation = Some("fort/modele");
        let summary = runtime
            .run_turn("explique le module", None)
            .expect("un autre modèle reprend le tour");
        let observed = observed.borrow();
        assert_eq!(observed.escalations, 1);
        assert!(observed.notices.iter().any(|notice| notice
            == "Le modèle n'a rien répondu deux fois : fort/modele reprend la fin du tour"));
        assert!(summary.assistant_messages.iter().any(|message| message
            .blocks
            .iter()
            .any(|block| matches!(block, ContentBlock::Text { text } if text == "Voici l'explication."))));

        // Sans autre modèle, l'erreur est rendue comme avant.
        let (mut runtime, _, _) = aided_runtime(vec![empty(), empty()], Vec::new());
        let error = runtime
            .run_turn("explique le module", None)
            .expect_err("aucun autre modèle");
        assert_eq!(error.to_string(), super::EMPTY_RESPONSE_ERROR);

        // Essai réel : deux réponses vides séparées par du travail ne sont pas
        // « vide encore après la relance » ; chacune est relancée.
        let (mut runtime, observed, _) = aided_runtime(
            vec![
                empty(),
                tool_step("probe", "{}"),
                empty(),
                text_step("Voici l'explication."),
            ],
            Vec::new(),
        );
        runtime
            .run_turn("explique le module", None)
            .expect("chaque vide est relancé");
        assert_eq!(observed.borrow().escalations, 0);
    }

    #[test]
    fn a_failed_verification_sends_the_model_back_to_work() {
        let _serial = crate::test_serial();
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
        /// Défauts que le lecteur d'images voit dans les images de la demande.
        static DEFECTS: std::cell::RefCell<Vec<(usize, String)>> =
            const { std::cell::RefCell::new(Vec::new()) };
        /// Réponses en texte suivantes arrêtées par la limite de sortie.
        static CUT_REPLIES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
        /// Requêtes gratuites restantes annoncées par le client.
        static FREE_LEFT: std::cell::Cell<Option<u64>> = const { std::cell::Cell::new(None) };
    }

    #[test]
    fn a_short_reply_to_a_check_keeps_the_report_it_follows() {
        let _serial = crate::test_serial();
        // Essai réel du 12 octobre : après un contrôle sans reproche, la
        // réponse finale n'était plus que « La revue confirme... ».
        let report = format!(
            "**Résultat**\nTout passe.\n\n**Cause**\n{}\n\n**Vérification**\npytest : 3 passed.",
            "La liste par défaut était partagée entre les stocks. ".repeat(10)
        );
        let (mut runtime, observed, _) = aided_runtime(
            vec![
                text_step(&report),
                text_step("La vérification confirme que tout est correct."),
            ],
            vec![],
        );
        let summary = runtime.run_turn("les tests échouent", None).expect("tour");
        let last = super::message_text(summary.assistant_messages.last().expect("réponse"));
        assert!(last.starts_with("**Résultat**"), "{last}");
        assert!(
            last.ends_with("pytest : 3 passed.\n\nLa vérification confirme que tout est correct."),
            "{last}"
        );
        assert_eq!(observed.borrow().kept, 1);
        // Le dernier message de la session porte le compte rendu lui aussi.
        let stored = super::message_text(runtime.session().messages.last().expect("message"));
        assert_eq!(stored, last);

        // Une réponse de taille comparable remplace la précédente, comme avant.
        let second = format!(
            "**Résultat**\nTout passe, vérifié.\n\n{}",
            "Détail. ".repeat(60)
        );
        let (mut runtime, observed, _) =
            aided_runtime(vec![text_step(&report), text_step(&second)], vec![]);
        let summary = runtime.run_turn("les tests échouent", None).expect("tour");
        let last = super::message_text(summary.assistant_messages.last().expect("réponse"));
        assert_eq!(last.trim(), second.trim());
        assert_eq!(observed.borrow().kept, 0);
    }

    #[test]
    fn a_reply_cut_by_the_output_limit_is_continued_and_joined() {
        let _serial = crate::test_serial();
        CUT_REPLIES.with(|cut| cut.set(1));
        let (mut runtime, observed, _) = aided_runtime(
            vec![
                text_step("Voici le plan : d'abord la base, ensuite l'inter"),
                text_step("face, enfin les tests."),
            ],
            vec![],
        );
        let summary = runtime.run_turn("explique le plan", None).expect("tour");
        CUT_REPLIES.with(|cut| cut.set(0));
        let observed = observed.borrow();
        // La suite est demandée une fois, et l'utilisateur le sait.
        assert_eq!(observed.requests.len(), 2);
        assert!(observed.requests[1].contains("cut off by the output limit"));
        assert!(observed
            .notices
            .iter()
            .any(|notice| notice.contains("Claw demande la suite")));
        assert_eq!(observed.drafted, 1);
        // Une seule réponse, entière, sans la relance de Claw.
        assert_eq!(summary.assistant_messages.len(), 1);
        let messages = &runtime.session().messages;
        assert_eq!(messages.len(), 2);
        assert_eq!(
            super::message_text(&messages[1]),
            "Voici le plan : d'abord la base, ensuite l'interface, enfin les tests."
        );
    }

    #[test]
    fn a_continuation_joins_the_text_even_after_a_reasoning_block() {
        let _serial = crate::test_serial();
        use crate::session::ConversationMessage;
        // Modèles d'OpenRouter : le raisonnement arrive après le texte.
        CUT_REPLIES.with(|cut| cut.set(1));
        let (mut runtime, _observed, _) = aided_runtime(
            vec![
                vec![
                    AssistantEvent::TextDelta("La page d'accueil affiche l'inter".to_string()),
                    AssistantEvent::Thinking {
                        thinking: "je réfléchis".to_string(),
                        signature: String::new(),
                        redacted: None,
                    },
                    AssistantEvent::MessageStop,
                ],
                text_step("face complète."),
            ],
            vec![],
        );
        runtime.run_turn("décris la page", None).expect("tour");
        CUT_REPLIES.with(|cut| cut.set(0));
        let message = &runtime.session().messages[1];
        assert_eq!(
            super::message_text(message),
            "La page d'accueil affiche l'interface complète."
        );
        assert_eq!(message.blocks.len(), 2);
        // L'affichage sait raccorder une suite à ce qui est déjà écrit.
        let asking = vec![
            ConversationMessage::user_text("décris la page"),
            ConversationMessage::assistant(vec![ContentBlock::Text {
                text: "La page".to_string(),
            }]),
            ConversationMessage::user_text(super::CUT_REPLY_NUDGE),
        ];
        assert_eq!(
            super::cut_reply_continued(&asking).as_deref(),
            Some("La page")
        );
        assert_eq!(super::cut_reply_continued(&asking[..2]), None);
    }

    #[test]
    fn an_empty_continuation_is_asked_again_then_the_cut_reply_is_kept() {
        let _serial = crate::test_serial();
        // Suite vide, puis suite : elle rejoint la réponse coupée, sans la
        // relance des réponses vides.
        CUT_REPLIES.with(|cut| cut.set(1));
        let (mut runtime, observed, _) = aided_runtime(
            vec![
                text_step("Voici le début"),
                vec![AssistantEvent::MessageStop],
                text_step(" et la fin."),
            ],
            vec![],
        );
        runtime.run_turn("raconte", None).expect("tour");
        CUT_REPLIES.with(|cut| cut.set(0));
        assert_eq!(
            super::message_text(&runtime.session().messages[1]),
            "Voici le début et la fin."
        );
        {
            let observed = observed.borrow();
            assert_eq!(observed.requests.len(), 3);
            assert!(!observed.requests[2].contains("Your last reply was empty"));
        }

        // Deux suites vides : la réponse coupée est rendue telle quelle, et le
        // tour ne finit pas en erreur.
        CUT_REPLIES.with(|cut| cut.set(1));
        let (mut runtime, observed, _) = aided_runtime(
            vec![
                text_step("Voici le début"),
                vec![AssistantEvent::MessageStop],
                vec![AssistantEvent::MessageStop],
            ],
            vec![],
        );
        runtime.run_turn("raconte", None).expect("tour");
        CUT_REPLIES.with(|cut| cut.set(0));
        assert_eq!(runtime.session().messages.len(), 2);
        assert_eq!(
            super::message_text(&runtime.session().messages[1]),
            "Voici le début"
        );
        assert!(observed
            .borrow()
            .notices
            .iter()
            .any(|notice| notice.contains("n'est pas venue")));
    }

    #[test]
    fn no_continuation_is_asked_at_the_last_allowed_step() {
        let _serial = crate::test_serial();
        // Sous-agent à sa dernière étape : sa réponse, même coupée, est rendue.
        CUT_REPLIES.with(|cut| cut.set(1));
        let (runtime, observed, _) = aided_runtime(vec![text_step("Rapport : tout est")], vec![]);
        let mut runtime = runtime.with_max_iterations(1);
        let summary = runtime.run_turn("fais le rapport", None).expect("tour");
        CUT_REPLIES.with(|cut| cut.set(0));
        assert_eq!(observed.borrow().requests.len(), 1);
        assert_eq!(summary.assistant_messages.len(), 1);
    }

    #[test]
    fn a_reply_still_cut_after_two_continuations_is_given_as_it_is() {
        let _serial = crate::test_serial();
        CUT_REPLIES.with(|cut| cut.set(5));
        let (mut runtime, observed, _) = aided_runtime(
            vec![text_step("un"), text_step(" deux"), text_step(" trois")],
            vec![],
        );
        runtime
            .run_turn("compte jusqu'à trois", None)
            .expect("tour");
        CUT_REPLIES.with(|cut| cut.set(0));
        let observed = observed.borrow();
        assert_eq!(observed.requests.len(), 3);
        assert!(observed
            .notices
            .iter()
            .any(|notice| notice.contains("telle quelle")));
        assert_eq!(
            super::message_text(&runtime.session().messages[1]),
            "un deux trois"
        );
    }

    #[test]
    fn a_problem_report_gets_a_debugging_guide_and_evidence_is_asked_before_the_answer() {
        let _serial = crate::test_serial();
        let (mut runtime, observed, _) = aided_runtime(
            vec![
                text_step("Problème résolu : c'était probablement le CSS."),
                text_step("Je n'ai pas pu observer la page ; voici ce que j'ai changé."),
            ],
            vec![],
        );
        let summary = runtime
            .run_turn("c'est n'importe quoi", None)
            .expect("tour");
        assert_eq!(summary.iterations, 2);
        {
            let observed = observed.borrow();
            // La méthode accompagne la demande dès la première requête.
            assert!(observed.requests[0].contains("[Claw guide] The user is telling you"));
            assert!(observed.requests[0].contains("Reproduce: observe each symptom yourself"));
            // « Résolu » sans observation : les preuves sont demandées, une fois.
            assert!(observed.requests[1].contains("name the observation you made in this turn"));
            assert_eq!(observed.superseded, 1);
        }
        // Le guide est pour le modèle : il suit le texte de l'utilisateur dans
        // le même message, sans le remplacer.
        let first = &runtime.session().messages[0];
        assert!(
            matches!(&first.blocks[0], ContentBlock::Text { text } if text == "c'est n'importe quoi")
        );
        assert!(
            matches!(&first.blocks[1], ContentBlock::Text { text } if text.starts_with(crate::harness::GUIDE_PREFIX))
        );

        // Une demande d'explication : ni guide de débogage ni demande de
        // preuves ; c'est une question, la réponse est le travail.
        let (mut runtime, observed, _) = aided_runtime(vec![text_step("Voilà.")], vec![]);
        let summary = runtime.run_turn("explique ce fichier", None).expect("tour");
        assert_eq!(summary.iterations, 1);
        assert!(!observed.borrow().requests[0].contains("The user is telling you"));
        assert!(observed.borrow().requests[0].contains("The user's message is a question"));
    }

    #[test]
    fn the_evidence_of_a_reported_problem_covers_the_edge_cases_of_its_kind() {
        let _serial = crate::test_serial();
        // Essai réel t3 : le guide du début de tour est arrivé, et l'espace
        // insécable des exports Excel est restée oubliée.
        let (mut runtime, observed, _) =
            aided_runtime(vec![text_step("Corrigé."), text_step("Vérifié.")], vec![]);
        runtime
            .run_turn(
                "Mon script plante sur '12,5' ; je pense que ventes.csv est corrompu",
                None,
            )
            .expect("tour");
        let observed = observed.borrow();
        assert!(observed.requests[1].contains("name the observation you made in this turn"));
        assert!(observed.requests[1].contains("What experienced engineers always check"));
        assert!(observed.requests[1].contains("non-breaking space"));
    }

    fn capture() -> crate::attachments::ImageAttachment {
        crate::attachments::ImageAttachment {
            media_type: "image/png".to_string(),
            data: "AAAA".to_string(),
            path: std::path::PathBuf::from("capture.png"),
        }
    }

    #[test]
    fn images_sent_without_a_complaint_are_sorted_before_any_debugging() {
        let _serial = crate::test_serial();
        // Un constat bref avec une capture : le modèle trie d'abord, et rien
        // n'est demandé de plus quand il n'a rien modifié.
        let (mut runtime, observed, _) =
            aided_runtime(vec![text_step("C'est le résultat attendu.")], vec![]);
        let summary = runtime
            .run_turn_with_images("c'est la même chose", vec![capture()], None)
            .expect("tour");
        assert_eq!(summary.iterations, 1);
        {
            let observed = observed.borrow();
            assert!(observed.requests[0].contains("First decide what they mean"));
            assert!(
                !observed.requests[0].contains("The user is telling you that something is wrong")
            );
        }

        // Un long cahier des charges avec une capture où le lecteur voit un
        // défaut : la capture montre le résultat cassé, pas une maquette.
        DEFECTS.with(|defects| {
            *defects.borrow_mut() = vec![(1, "the comparison area is empty".to_string())];
        });
        let spec = format!(
            "{} et la page de comparaison",
            "voici le cahier des charges complet ".repeat(12)
        );
        let (mut runtime, observed, _) = aided_runtime(
            vec![
                text_step("Tout est fait, le projet est prêt."),
                text_step("La zone vide venait du rendu ; corrigé et observé."),
            ],
            vec![],
        );
        let summary = runtime
            .run_turn_with_images(&spec, vec![capture()], None)
            .expect("tour");
        DEFECTS.with(|defects| defects.borrow_mut().clear());
        assert_eq!(summary.iterations, 2);
        {
            let observed = observed.borrow();
            assert!(observed.requests[0].contains(
                "The image reader saw a defect in image 1: the comparison area is empty."
            ));
            assert!(observed.requests[1].contains("look again at each image they sent"));
        }

        // Une demande ordinaire avec une image : chaque image est lue.
        let (mut runtime, observed, _) = aided_runtime(vec![text_step("Voilà.")], vec![]);
        let summary = runtime
            .run_turn_with_images(
                "fais une interface comme sur l'image",
                vec![capture()],
                None,
            )
            .expect("tour");
        assert_eq!(summary.iterations, 1);
        assert!(observed.borrow().requests[0]
            .contains("Before you plan, write in one line what each one shows"));
    }

    #[test]
    fn a_reply_that_only_announces_the_work_is_sent_back_to_do_it() {
        let _serial = crate::test_serial();
        // Session #30 du 24 octobre 2026 : la réponse finale annonçait la PWA
        // et aucun fichier n'existait.
        const ANNOUNCED: &str =
            "Maintenant je construis la PWA complète. Je vais créer un fichier HTML unique.";
        let (mut runtime, observed, _) = aided_runtime(
            vec![
                text_step(ANNOUNCED),
                tool_step("write_file", r#"{"path":"notes.txt","content":"ok"}"#),
                text_step("Le fichier notes.txt est écrit."),
                text_step("Le fichier notes.txt est écrit."),
            ],
            vec![],
        );
        runtime.run_turn("fais-moi une appli", None).expect("tour");
        let observed = observed.borrow();
        assert!(observed.requests[1].contains("nothing has been written in this turn"));
        assert!(observed.requests[1].contains("Maintenant je construis la PWA complète."));
        // Un fichier écrit : la réponse suivante n'est plus renvoyée pour ça.
        assert!(observed.requests.iter().skip(2).all(|request| request
            .matches("nothing has been written in this turn")
            .count()
            <= 1));
        assert!(observed
            .notices
            .iter()
            .any(|notice| notice.contains("annonce le travail sans l'avoir fait")));
    }

    #[test]
    fn a_reply_that_stops_on_empty_titles_is_sent_back_to_do_the_work() {
        let _serial = crate::test_serial();
        const TITLES: &str =
            "Maintenant, la page :\n\n### 2a. Composant URLInput\n\n### 2b. Page /url";
        let (mut runtime, observed, _) = aided_runtime(
            vec![text_step(TITLES), text_step(TITLES), text_step(TITLES)],
            vec![],
        );
        let summary = runtime
            .run_turn("ajoute la page pour l'URL", None)
            .expect("tour");
        // Renvoyé deux fois au plus, puis la réponse part telle quelle.
        assert_eq!(summary.iterations, 3);
        let observed = observed.borrow();
        assert!(observed.requests[1].contains(
            r#"Your reply has section titles with nothing under them: \"2a. Composant URLInput\", \"2b. Page /url\""#
        ));
        assert_eq!(observed.superseded, 2);
        assert!(observed
            .notices
            .iter()
            .any(|notice| notice.contains("titres sans contenu")));
    }

    #[test]
    fn a_permission_request_left_unanswered_is_not_asked_again_in_the_turn() {
        let _serial = crate::test_serial();
        struct Away(usize);
        impl PermissionPrompter for Away {
            fn decide(&mut self, request: &PermissionRequest) -> PermissionPromptDecision {
                self.0 += 1;
                crate::permissions::unanswered_permission(&request.tool_name)
            }
        }
        let (mut runtime, observed, bash_calls) = aided_runtime(
            vec![
                tool_step("bash", r#"{"command":"npm test"}"#),
                tool_step("bash", r#"{"command":"npm test"}"#),
                tool_step("bash", r#"{"command":"npm run build"}"#),
                tool_step("bash", r#"{"command":"npm run lint"}"#),
                tool_step("bash", r#"{"command":"npm run dev"}"#),
                text_step("Les vérifications attendent votre accord."),
                tool_step("bash", r#"{"command":"npm test"}"#),
                text_step("Toujours sans réponse."),
            ],
            vec![],
        );
        // Un mode où bash demande l'autorisation.
        runtime
            .permission_policy
            .set_active_mode(PermissionMode::WorkspaceWrite);
        let mut away = Away(0);
        let summary = runtime
            .run_turn("vérifie le projet", Some(&mut away))
            .expect("tour");
        // Une seule demande : les suivantes n'ont pas attendu un utilisateur absent.
        assert_eq!(away.0, 1);
        assert!(bash_calls.borrow().is_empty());
        assert_eq!(summary.iterations, 6);
        assert!(crate::permissions::user_away());
        {
            let observed = observed.borrow();
            // Le modèle apprend que la commande n'a pas tourné, sans être bloquée.
            assert!(observed.requests[1]
                .contains("[Claw] Not run: nobody answered the permission request for `bash`"));
            assert!(observed.requests[1].contains("It is not hanging"));
            assert!(observed.requests[2].contains("so the request for `bash` was not asked"));
            // Des appels qui n'ont pas tourné ne sont pas des échecs.
            assert!(!observed
                .requests
                .iter()
                .any(|request| request.contains("tool calls in a row have failed")));
            assert_eq!(
                observed
                    .notices
                    .iter()
                    .filter(|notice| notice.contains("restée sans réponse"))
                    .count(),
                1
            );
        }

        // L'utilisateur écrit de nouveau : il est là, l'autorisation lui est demandée.
        runtime
            .run_turn("vas-y, je suis là", Some(&mut away))
            .expect("tour");
        assert_eq!(away.0, 2);
        crate::permissions::set_user_away(false);
    }

    #[test]
    fn commands_advised_without_being_run_send_the_model_back_to_try_them() {
        let _serial = crate::test_serial();
        const ADVICE: &str = "Pour lancer :\n\n```bash\nnpm run dev\n```";
        let (mut runtime, observed, bash_calls) = aided_runtime(
            vec![
                text_step(ADVICE),
                tool_step("bash", r#"{"command":"npm run dev"}"#),
                text_step(ADVICE),
            ],
            vec![],
        );
        let summary = runtime.run_turn("comment le lancer ?", None).expect("tour");
        assert_eq!(summary.iterations, 3);
        assert_eq!(bash_calls.borrow().len(), 1);
        let observed = observed.borrow();
        assert!(observed.requests[1]
            .contains("commands that you have not run in this session: `npm run dev`"));
        // Une fois lancée, la même commande n'est plus signalée.
        assert_eq!(observed.superseded, 1);
    }

    #[test]
    fn a_model_sent_back_to_work_by_a_check_says_again_what_it_does() {
        let _serial = crate::test_serial();
        const ADVICE: &str = "Pour lancer :\n\n```bash\nnpm run dev\n```";
        // Annonce au départ, puis une réponse qui conseille une commande jamais
        // lancée : le modèle, renvoyé l'essayer, s'y remet sans un mot. La
        // requête suivante lui fait dire ce qu'il fait, comme au départ.
        let (mut runtime, observed, _) = aided_runtime(
            vec![
                tool_step("bash", r#"{"command":"ls"}"#),
                text_step("Je regarde le projet, puis je dis comment le lancer."),
                text_step(ADVICE),
                tool_step("bash", r#"{"command":"npm run dev"}"#),
                text_step("J'essaie la commande conseillée avant de la donner."),
                text_step(ADVICE),
            ],
            vec![PASSING, PASSING],
        );
        let _announce = AnnouncementsOn::new();
        let summary = runtime.run_turn("comment le lancer ?", None).expect("tour");
        assert_eq!(summary.iterations, 6);
        let observed = observed.borrow();
        assert_eq!(observed.superseded, 1);
        // L'annonce du départ, puis celle de la reprise : en texte seul.
        assert_eq!(observed.text_only, vec![1, 4]);
        assert!(observed.requests[4].contains("You started working without a word for the user"));
    }

    #[test]
    fn a_model_that_starts_without_a_word_is_asked_to_announce_its_plan() {
        let _serial = crate::test_serial();
        // Premiers outils sans un mot : la requête suivante part en texte seul
        // (vu dans une vraie session : Nemotron 3 Super ne mettait son plan que
        // dans son raisonnement et ignorait une note jointe aux résultats).
        let (mut runtime, observed, _) = aided_runtime(
            vec![
                tool_step("bash", r#"{"command":"ls"}"#),
                text_step("Je regarde les fichiers du projet, puis je les range par dossier."),
                tool_step("bash", r#"{"command":"ls src"}"#),
                text_step("Fini."),
            ],
            vec![PASSING, PASSING],
        );
        let _announce = AnnouncementsOn::new();
        let summary = runtime.run_turn("range le projet", None).expect("tour");
        assert_eq!(summary.iterations, 4);
        let observed = observed.borrow();
        assert_eq!(
            observed.text_only,
            vec![1],
            "seule l'annonce part sans outils"
        );
        assert!(observed.requests[1].contains("You started working without a word for the user"));
        // L'annonce n'est pas la réponse du tour : le travail reprend.
        assert!(observed.requests[2].contains("Go on with the work now"));
        assert!(observed.requests[2].contains("puis je les range par dossier"));
        assert_eq!(
            observed
                .requests
                .iter()
                .filter(|request| request.contains("You started working without a word"))
                .count(),
            // La note reste dans l'historique ; elle n'est pas redemandée.
            3
        );
        assert!(observed
            .notices
            .iter()
            .all(|notice| !notice.contains("annoncer")));
        drop(observed);

        // Une réponse qui dit son plan avant ses outils : rien à demander.
        let announced = vec![
            AssistantEvent::TextDelta(
                "Je lis les fichiers du projet, puis je les range par dossier.".to_string(),
            ),
            AssistantEvent::ToolUse {
                id: "appel-bash".to_string(),
                name: "bash".to_string(),
                input: r#"{"command":"ls"}"#.to_string(),
            },
            AssistantEvent::MessageStop,
        ];
        let (mut runtime, observed, _) =
            aided_runtime(vec![announced, text_step("Fini.")], vec![PASSING]);
        let _announce = AnnouncementsOn::new();
        runtime.run_turn("range le projet", None).expect("tour");
        assert!(observed.borrow().text_only.is_empty());
        assert!(!observed.borrow().requests[1].contains("You started working"));

        // Annonce restée vide : le travail reprend sans elle, sans relance.
        let (mut runtime, observed, _) = aided_runtime(
            vec![
                tool_step("bash", r#"{"command":"ls"}"#),
                vec![AssistantEvent::MessageStop],
                text_step("Fini."),
            ],
            vec![PASSING],
        );
        let _announce = AnnouncementsOn::new();
        runtime.run_turn("range le projet", None).expect("tour");
        let observed = observed.borrow();
        assert_eq!(observed.requests.len(), 3);
        assert!(!observed.requests[2].contains("You started working"));
        assert!(!observed.requests[2].contains("Go on with the work"));
    }

    #[test]
    fn a_question_gets_its_answer_and_leaves_the_files_alone() {
        let _serial = crate::test_serial();
        // Vu le 22 octobre 2026 dans une session de Claw Web : à une simple
        // question, le modèle reprenait le projet sans y répondre. Une question
        // reçoit sa réponse ; écrire un fichier est refusé pendant ce tour.
        let (mut runtime, observed, _) = aided_runtime(
            vec![
                tool_step("write_file", r#"{"path":"notes.md","content":"x"}"#),
                text_step("Le formulaire garde les dépenses dans le navigateur (app.js:42)."),
                text_step("Le formulaire garde les dépenses dans le navigateur (app.js:42)."),
            ],
            vec![],
        );
        runtime
            .run_turn("Comment le formulaire enregistre les dépenses ?", None)
            .expect("tour");
        let observed = observed.borrow();
        assert!(observed.requests[0].contains("The user's message is a question, not a work order"));
        assert!(observed.requests[1]
            .contains("this turn answers the user's question and changes no files"));
        assert!(observed
            .notices
            .iter()
            .any(|notice| notice == "Question : Claw y répond sans modifier les fichiers"));
        drop(observed);

        // Une demande de travail : ni note ni refus.
        let (mut runtime, observed, _) = aided_runtime(vec![text_step("Fait.")], vec![]);
        runtime
            .run_turn("Corrige le formulaire", None)
            .expect("tour");
        assert!(!observed.borrow().requests[0].contains("is a question"));
    }

    #[test]
    fn a_question_about_the_work_gets_an_answer_and_no_work() {
        let _serial = crate::test_serial();
        // Vu dans une session de Claw Web : à « Tu fais quoi !? », 65 actions
        // et une réponse sur autre chose. La première requête part sans outils,
        // et sa réponse termine le tour.
        let (mut runtime, observed, _) = aided_runtime(
            vec![
                text_step("J'ai repris la refonte du site : les formulaires ne s'ouvraient pas, je les ai réparés. Je m'arrête là, dites-moi si je continue."),
                tool_step("bash", r#"{"command":"ls"}"#),
            ],
            vec![PASSING],
        );
        let request = "Tu fais quoi !?\n\n[Claw] Earlier conversation of this session, for context:\n\nAméliore le site";
        let summary = runtime.run_turn(request, None).expect("tour");
        assert_eq!(summary.iterations, 1, "une seule requête : la réponse");
        let observed = observed.borrow();
        assert_eq!(observed.text_only, vec![0]);
        assert!(observed.requests[0].contains("The user's message is a question about your work"));
        assert!(!observed.requests[0].contains("Go on with the work"));
        drop(observed);

        // Une demande de travail ordinaire : outils dès la première requête.
        let (mut runtime, observed, _) = aided_runtime(vec![text_step("Fait.")], vec![]);
        runtime
            .run_turn("Corrige le formulaire", None)
            .expect("tour");
        assert!(observed.borrow().text_only.is_empty());
    }

    #[test]
    fn a_model_silent_for_many_steps_is_asked_where_the_work_stands() {
        let _serial = crate::test_serial();
        let steps_before = crate::harness::SILENT_STEPS_BEFORE_UPDATE as usize;
        // Une annonce au départ, puis des outils sans un mot : un point d'étape
        // après la limite, en texte seul.
        let mut steps = vec![tool_step("bash", r#"{"command":"ls"}"#)];
        steps.push(text_step(
            "Je lis le projet, puis je corrige la page d'accueil.",
        ));
        for step in 0..steps_before {
            let input = format!(r#"{{"command":"ls dossier-{step}"}}"#);
            steps.push(tool_step("bash", &input));
        }
        steps.push(text_step(
            "La page d'accueil est corrigée ; il reste le pied de page.",
        ));
        steps.push(text_step("Fini."));
        let checks = vec![PASSING; steps_before + 1];
        let (mut runtime, observed, _) = aided_runtime(steps, checks);
        let _announce = AnnouncementsOn::new();
        runtime.run_turn("corrige le site", None).expect("tour");
        let observed = observed.borrow();
        // Requêtes : outils, annonce, les étapes muettes, puis le point d'étape.
        let update = steps_before + 2;
        assert_eq!(observed.text_only, vec![1, update]);
        assert!(observed.requests[update].contains(&format!(
            "You have worked through {steps_before} rounds of tool calls without a word"
        )));
    }

    #[test]
    fn an_answer_sent_back_by_a_check_stays_when_the_model_stops_answering() {
        let _serial = crate::test_serial();
        const ADVICE: &str = "Pour lancer :\n\n```bash\nnpm run dev\n```";
        // Le contrôle des commandes renvoie le modèle, qui ne répond plus
        // (quota épuisé) : le tour finit sur la réponse déjà donnée.
        let (mut runtime, observed, _) = aided_runtime(vec![text_step(ADVICE)], vec![]);
        let summary = runtime.run_turn("comment le lancer ?", None).expect("tour");
        assert_eq!(summary.iterations, 2);
        assert_eq!(
            summary
                .assistant_messages
                .last()
                .map(super::message_text)
                .as_deref(),
            Some(ADVICE)
        );
        let last = runtime.session.messages.last().expect("message");
        assert_eq!(last.role, MessageRole::Assistant);
        let observed = observed.borrow();
        assert_eq!((observed.superseded, observed.kept), (1, 1));
        assert!(observed
            .notices
            .iter()
            .any(|notice| notice.contains("(plus de réponse scriptée)")
                && notice.contains("la réponse déjà donnée reste celle du tour")));
        drop(observed);
        // Sans réponse déjà donnée, l'erreur reste l'issue du tour.
        let (mut runtime, _, _) = aided_runtime(vec![], vec![]);
        assert!(runtime.run_turn("bonjour", None).is_err());
    }

    #[test]
    fn no_visual_review_is_announced_without_a_page_to_capture() {
        let _serial = crate::test_serial();
        // Un module TypeScript modifié encore et encore, sans aucune page :
        // l'essai réel annonçait « revue maintenant » à chaque modification.
        let mut steps: Vec<Vec<AssistantEvent>> = (0..14)
            .map(|_| tool_step("write_file", r#"{"path":"remise.ts","content":"x"}"#))
            .collect();
        steps.push(text_step("Module écrit."));
        let (runtime, observed, _) = aided_runtime(steps, vec![]);
        let mut runtime = runtime.with_turn_request_limit(20);
        runtime.run_turn("écris remise.ts", None).expect("tour");
        let observed = observed.borrow();
        assert!(
            !observed
                .notices
                .iter()
                .any(|notice| notice.contains("sans revue visuelle")),
            "{:?}",
            observed.notices
        );
    }

    /// Un runtime d'essai dont l'outil `Screenshot` rend les mesures données,
    /// et une page `index.html` réelle dans un dossier temporaire.
    fn measured_page(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("claw-mesure-{}-{tag}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dossier");
        let page = dir.join("index.html");
        std::fs::write(&page, "<!doctype html><title>Club</title><h1>Club</h1>").expect("page");
        page
    }

    /// `measured` : ce que rend chaque appel de `Screenshot`, dans l'ordre
    /// (`None` : une erreur) ; le dernier se répète.
    fn measured_runtime(
        steps: Vec<Vec<AssistantEvent>>,
        measured: Vec<Option<&'static str>>,
    ) -> (Aided, std::rc::Rc<std::cell::Cell<u32>>) {
        let shots = std::rc::Rc::new(std::cell::Cell::new(0));
        let counted = std::rc::Rc::clone(&shots);
        let observed = std::rc::Rc::new(std::cell::RefCell::new(Observed::default()));
        let executor = StaticToolExecutor::new()
            .register("write_file", |input| {
                let path = serde_json::from_str::<serde_json::Value>(input)
                    .ok()
                    .and_then(|value| value["path"].as_str().map(ToString::to_string))
                    .unwrap_or_default();
                Ok(serde_json::json!({ "filePath": path, "content": "x" }).to_string())
            })
            .register("bash", |_| Ok(PASSING.to_string()))
            .register("Screenshot", move |_| {
                let index = (counted.get() as usize).min(measured.len() - 1);
                counted.set(counted.get() + 1);
                measured[index]
                    .map(ToString::to_string)
                    .ok_or_else(|| super::ToolError::new("la page ne s'ouvre pas"))
            });
        let runtime = ConversationRuntime::new(
            Session::new(),
            StepsApiClient {
                steps: steps.into(),
                observed: std::rc::Rc::clone(&observed),
                advisor: std::collections::VecDeque::new(),
                escalation: None,
            },
            executor,
            PermissionPolicy::new(PermissionMode::DangerFullAccess),
            vec!["système".to_string()],
        );
        (
            (
                runtime,
                observed,
                std::rc::Rc::new(std::cell::RefCell::new(Vec::new())),
            ),
            shots,
        )
    }

    const MEASURED: &str = r#"{"captures":[{"screen":"phone 390x844","problems":["Words that run together: a letter-spacing of -0.02em or tighter on text under 32px eats the space between words (h1.hero-title \"U11 vs FC Voisins\" 14px, letter-spacing -0.28px)"]}]}"#;

    const TWO_PROBLEMS: &str = r#"{"captures":[{"screen":"phone 390x844","problems":["Words that run together: a letter-spacing of -0.02em or tighter on text under 32px eats the space between words (h1.hero-title \"U11 vs FC Voisins\" 14px, letter-spacing -0.28px)","The page is 430px wide on a 390px-wide screen and scrolls sideways (because of decorations drawn past the edge: div.hero-card::before)"]}]}"#;
    const WIDE_ONLY: &str = r#"{"captures":[{"screen":"phone 390x844","problems":["The page is 430px wide on a 390px-wide screen and scrolls sideways (because of decorations drawn past the edge: div.hero-card::before)"]}]}"#;

    #[test]
    fn claw_looks_at_the_page_after_each_change_and_says_what_it_broke_or_fixed() {
        let _serial = crate::test_serial();
        // Demandé le 25 octobre 2026 : « À chaque modification il doit
        // vérifier visuellement la page ». Session #31 : 64 actions de CSS
        // avant la première capture.
        std::env::set_var("CLAW_DESIGN_REVIEW_ROUNDS", "0");
        let page = measured_page("chaque");
        let write = || {
            tool_step(
                "write_file",
                // Un chemin Windows a des barres obliques inverses : du JSON fait par serde.
                &serde_json::json!({ "path": page.display().to_string(), "content": "<h1>Club</h1>" })
                    .to_string(),
            )
        };
        let steps = vec![write(), write(), write(), text_step("Corrigé.")];
        let ((mut runtime, observed, _), shots) = measured_runtime(
            steps,
            vec![Some(MEASURED), Some(TWO_PROBLEMS), Some(WIDE_ONLY)],
        );
        runtime.run_turn("corrige le site", None).expect("tour");
        std::env::remove_var("CLAW_DESIGN_REVIEW_ROUNDS");
        let observed = observed.borrow();
        assert_eq!(shots.get(), 3);
        // La dernière vérification de chaque requête (la conversation garde
        // les précédentes).
        let last = |index: usize| {
            observed.requests[index]
                .rsplit("Claw looked at")
                .next()
                .unwrap_or_default()
                .to_string()
        };
        let first = &last(1);
        assert!(
            observed.requests[1].contains("Claw looked at")
                && first.contains("Still wrong on the page")
                && first.contains("U11 vs FC Voisins"),
            "{first}"
        );
        let second = &last(2);
        assert!(
            second.contains("Broken by your last change") && second.contains("430px wide"),
            "{second}"
        );
        let third = &last(3);
        assert!(
            third.contains("Your last change fixed 1 problem(s)"),
            "{third}"
        );
        assert!(!third.contains("Broken by your last change"), "{third}");
        assert!(observed.notices.iter().any(|notice| notice
            .contains("Claw vérifie la page après la modification : 1 défaut(s) nouveau(x)")));
    }

    #[test]
    fn the_check_after_a_change_runs_while_the_model_answers() {
        let _serial = crate::test_serial();
        // Session #32, passage 5 : 33 vérifications de 25 s, le modèle
        // attendant chaque fois. Lancée en arrière-plan, la vérification de la
        // première modification arrive avec la requête qui suit la seconde.
        std::env::set_var("CLAW_DESIGN_REVIEW_ROUNDS", "0");
        let page = measured_page("fond");
        let write = || {
            tool_step(
                "write_file",
                &serde_json::json!({ "path": page.display().to_string(), "content": "<h1>Club</h1>" })
                    .to_string(),
            )
        };
        let observed = std::rc::Rc::new(std::cell::RefCell::new(Observed::default()));
        let executor = StaticToolExecutor::new()
            .register("write_file", |input| {
                let path = serde_json::from_str::<serde_json::Value>(input)
                    .ok()
                    .and_then(|value| value["path"].as_str().map(ToString::to_string))
                    .unwrap_or_default();
                Ok(serde_json::json!({ "filePath": path, "content": "x" }).to_string())
            })
            .register("Screenshot", |_| {
                Err(super::ToolError::new("la vérification ne doit pas bloquer"))
            })
            .in_background("Screenshot", MEASURED);
        let mut runtime = ConversationRuntime::new(
            Session::new(),
            StepsApiClient {
                steps: vec![write(), write(), text_step("Corrigé.")].into(),
                observed: std::rc::Rc::clone(&observed),
                advisor: std::collections::VecDeque::new(),
                escalation: None,
            },
            executor,
            PermissionPolicy::new(PermissionMode::DangerFullAccess),
            vec!["système".to_string()],
        );
        runtime.run_turn("corrige le site", None).expect("tour");
        std::env::remove_var("CLAW_DESIGN_REVIEW_ROUNDS");
        let observed = observed.borrow();
        assert_eq!(observed.requests.len(), 3, "{:?}", observed.notices);
        // Pas d'attente : la requête qui suit la première modification part sans elle.
        assert!(
            !observed.requests[1].contains("Claw looked at"),
            "{}",
            observed.requests[1]
        );
        assert!(
            observed.requests[2].contains("Claw looked at")
                && observed.requests[2].contains("U11 vs FC Voisins"),
            "{}",
            observed.requests[2]
        );
        // Deux vérifications, la seconde reprise avant la conclusion.
        let checks = observed
            .notices
            .iter()
            .filter(|notice| notice.starts_with("Claw vérifie la page après la modification"))
            .count();
        assert_eq!(checks, 2, "{:?}", observed.notices);
    }

    #[test]
    fn three_searches_in_the_styles_point_to_inspect() {
        let _serial = crate::test_serial();
        // Session #31 (26 octobre 2026) : six recherches de suite pour trouver
        // la règle de l'espacement d'une section.
        std::env::set_var("CLAW_DESIGN_REVIEW_ROUNDS", "0");
        let page = measured_page("styles");
        let grep = |pattern: &str| {
            tool_step(
                "bash",
                &serde_json::json!({ "command": format!("grep -n '{pattern}' styles.css") })
                    .to_string(),
            )
        };
        let steps = vec![
            grep("\\.section"),
            grep("section.*padding"),
            grep("^\\.section\\s*\\{"),
            text_step("Trouvé."),
        ];
        let ((mut runtime, observed, _), _) = measured_runtime(steps, vec![Some(MEASURED)]);
        runtime.page_target = Some(format!("path:{}", page.display()));
        runtime.run_turn("corrige l'espace", None).expect("tour");
        std::env::remove_var("CLAW_DESIGN_REVIEW_ROUNDS");
        let observed = observed.borrow();
        assert!(
            !observed.requests[2].contains("\"inspect\""),
            "{}",
            observed.requests[2]
        );
        assert!(
            observed.requests[3].contains("Ask the page instead"),
            "{}",
            observed.requests[3]
        );
    }

    #[test]
    fn what_the_first_change_breaks_is_new_against_the_page_before_the_turn() {
        let _serial = crate::test_serial();
        // Session #31 (26 octobre 2026) : « 0 défaut mesuré » avant le tour,
        // puis après la première modification « 0 nouveau, 2 restants » :
        // ce qu'elle cassait passait pour un défaut d'avant.
        std::env::set_var("CLAW_DESIGN_REVIEW_ROUNDS", "0");
        let page = measured_page("depart");
        let write = tool_step(
            "write_file",
            // Un chemin Windows a des barres obliques inverses : du JSON fait par serde.
            &serde_json::json!({ "path": page.display().to_string(), "content": "<h1>Club</h1>" })
                .to_string(),
        );
        let steps = vec![write, text_step("Corrigé.")];
        let ((mut runtime, observed, _), shots) =
            measured_runtime(steps, vec![Some(MEASURED), Some(TWO_PROBLEMS)]);
        runtime.page_target = Some(format!("path:{}", page.display()));
        runtime
            .run_turn("fais un site plus joli", None)
            .expect("tour");
        std::env::remove_var("CLAW_DESIGN_REVIEW_ROUNDS");
        let observed = observed.borrow();
        assert_eq!(shots.get(), 2, "{:?}", observed.notices);
        assert!(
            observed
                .notices
                .iter()
                .any(|notice| notice.contains("État de la page avant ce tour : 1 défaut(s)")),
            "{:?}",
            observed.notices
        );
        assert!(
            observed.notices.iter().any(|notice| notice.contains(
                "après la modification : 1 défaut(s) nouveau(x), 0 réparé(s), 1 restant(s)"
            )),
            "{:?}",
            observed.notices
        );
        let check = observed.requests[1]
            .rsplit("Claw looked at")
            .next()
            .unwrap_or_default();
        assert!(
            check.contains("Broken by your last change") && check.contains("430px wide"),
            "{check}"
        );
        // Session #32, passage 7 (27 octobre 2026) : ce que la mesure du
        // début a vu part avec la demande.
        assert!(
            observed.requests[0].contains("[Claw] Before you change anything")
                && observed.requests[0].contains("Words that run together"),
            "{}",
            observed.requests[0]
        );
    }

    #[test]
    fn reservations_kept_in_each_phone_are_named_when_the_request_shares_them() {
        let _serial = crate::test_serial();
        // Session #32 (26 octobre 2026) : les clientes réservaient dans leur
        // navigateur ; la boulangère n'en aurait jamais rien vu.
        std::env::set_var("CLAW_DESIGN_REVIEW_ROUNDS", "0");
        let page = measured_page("partage");
        let script = page.with_file_name("app.js");
        // L'exécuteur des essais n'écrit pas : le fichier est posé ici.
        std::fs::write(
            &script,
            "function save(list) {\n  localStorage.setItem('reservations', JSON.stringify(list));\n}\n",
        )
        .expect("app.js");
        let write = tool_step(
            "write_file",
            &serde_json::json!({ "path": script.display().to_string(), "content": "function save(list) {\n  localStorage.setItem('reservations', JSON.stringify(list));\n}\n" })
                .to_string(),
        );
        let steps = vec![
            write,
            text_step("Le site est prêt."),
            text_step("Le site est prêt, mais chaque téléphone garde ses réservations."),
        ];
        let ((mut runtime, observed, _), _) =
            measured_runtime(steps, vec![Some(MEASURED), Some(MEASURED), Some(MEASURED)]);
        runtime.page_target = Some(format!("path:{}", page.display()));
        runtime
            .run_turn("j'aimerais un petit site où les clients réservent depuis leur téléphone, et où moi je vois le matin la liste de ce que je dois préparer", None)
            .expect("tour");
        std::env::remove_var("CLAW_DESIGN_REVIEW_ROUNDS");
        let observed = observed.borrow();
        assert!(
            observed
                .requests
                .iter()
                .any(|request| request.contains("several people share")
                    && request.contains("localStorage")),
            "{:?}",
            observed.notices
        );
        let _ = std::fs::remove_file(&script);
    }

    #[test]
    fn entries_sent_to_a_store_the_user_never_chose_lead_to_a_question_first() {
        let _serial = crate::test_serial();
        // Session #32, passage 7 (27 octobre 2026) : les réservations des
        // clientes envoyées à MantleDB, trouvé par une recherche, sans rien
        // demander à la boulangère.
        std::env::set_var("CLAW_DESIGN_REVIEW_ROUNDS", "0");
        let page = measured_page("tiers");
        let script = page.with_file_name("app.js");
        std::fs::write(
            &script,
            "const URL = 'https://mantledb.sh/v2/boulangerie/reservations';\nfunction save(entry) {\n  localStorage.setItem('r', JSON.stringify(entry));\n  return fetch(URL, { method: 'POST', body: JSON.stringify(entry) });\n}\n",
        )
        .expect("app.js");
        let write = tool_step(
            "write_file",
            &serde_json::json!({ "path": script.display().to_string(), "content": "x" })
                .to_string(),
        );
        let steps = vec![
            write,
            text_step("La synchronisation marche."),
            text_step("Je vous pose d'abord la question."),
        ];
        let ((mut runtime, observed, _), _) =
            measured_runtime(steps, vec![Some(MEASURED), Some(MEASURED), Some(MEASURED)]);
        runtime.page_target = Some(format!("path:{}", page.display()));
        runtime
            .run_turn(
                "je veux recevoir les réservations de mes clientes sans rien avoir à régler",
                None,
            )
            .expect("tour");
        std::env::remove_var("CLAW_DESIGN_REVIEW_ROUNDS");
        let _ = std::fs::remove_file(&script);
        let observed = observed.borrow();
        assert!(
            observed.requests.iter().any(|request| request
                .contains("Your code now sends what visitors enter to mantledb.sh")),
            "{:?}",
            observed.notices
        );
    }

    #[test]
    fn keys_of_an_account_the_user_must_create_lead_to_a_question_first() {
        let _serial = crate::test_serial();
        // Session #32 (26 octobre 2026) : Firebase branché d'office, avec
        // VOTRE_API_KEY dans le code, sans rien demander à la boulangère.
        std::env::set_var("CLAW_DESIGN_REVIEW_ROUNDS", "0");
        let page = measured_page("cles");
        let script = page.with_file_name("app.js");
        std::fs::write(
            &script,
            "const firebaseConfig = { apiKey: 'VOTRE_API_KEY', projectId: 'VOTRE_PROJECT_ID' };\n",
        )
        .expect("app.js");
        let write = tool_step(
            "write_file",
            &serde_json::json!({ "path": script.display().to_string(), "content": "x" })
                .to_string(),
        );
        let steps = vec![
            write,
            text_step("C'est corrigé."),
            text_step("Je vous pose d'abord la question."),
        ];
        let ((mut runtime, observed, _), _) =
            measured_runtime(steps, vec![Some(MEASURED), Some(MEASURED), Some(MEASURED)]);
        runtime.page_target = Some(format!("path:{}", page.display()));
        runtime
            .run_turn("je veux voir les commandes de tous mes clients", None)
            .expect("tour");
        std::env::remove_var("CLAW_DESIGN_REVIEW_ROUNDS");
        let _ = std::fs::remove_file(&script);
        let observed = observed.borrow();
        assert!(
            observed.requests.iter().any(|request| request
                .contains("Your code now waits for keys the user must create (VOTRE_API_KEY)")),
            "{:?}",
            observed.notices
        );
    }

    #[test]
    fn photos_saved_but_used_nowhere_are_named_before_the_answer() {
        let _serial = crate::test_serial();
        // Session #32 (26 octobre 2026) : 19 photos (11 Mo) dans images/,
        // aucune dans la page.
        let photo =
            std::env::temp_dir().join(format!("claw-photo-{}-baguette-1.jpg", std::process::id()));
        std::fs::write(&photo, b"jpeg").expect("photo");
        let shown = photo.display().to_string();
        let observed = std::rc::Rc::new(std::cell::RefCell::new(Observed::default()));
        let executor = StaticToolExecutor::new().register("Photo", move |_| {
            Ok(
                serde_json::json!({ "photos": [{ "path": shown.clone(), "kilobytes": 700 }] })
                    .to_string(),
            )
        });
        let mut runtime = ConversationRuntime::new(
            Session::new(),
            StepsApiClient {
                steps: vec![
                    tool_step("Photo", r#"{"search": "baguette"}"#),
                    text_step("Le site est prêt."),
                    text_step("Photo retirée, le site est prêt."),
                ]
                .into(),
                observed: std::rc::Rc::clone(&observed),
                advisor: std::collections::VecDeque::new(),
                escalation: None,
            },
            executor,
            PermissionPolicy::new(PermissionMode::DangerFullAccess),
            vec!["système".to_string()],
        );
        runtime
            .run_turn("une page pour ma boulangerie", None)
            .expect("tour");
        let _ = std::fs::remove_file(&photo);
        let observed = observed.borrow();
        assert!(
            observed.requests.iter().any(|request| request
                .contains("The page uses none of these photos you saved")
                && request.contains("baguette-1.jpg")),
            "{:?}",
            observed.notices
        );
    }

    #[test]
    fn an_answer_about_a_page_written_for_a_developer_is_rewritten_once() {
        let _serial = crate::test_serial();
        // Session #31 (26 octobre 2026) : « Les deux points du reviewer ont été
        // corrigés », `styles.css`, `app.js:725-731`, à un parent.
        std::env::set_var("CLAW_DESIGN_REVIEW_ROUNDS", "0");
        let page = measured_page("reponse");
        let steps = vec![
            text_step("Les deux points du reviewer ont été corrigés : `styles.css` charge `Inter`, et `app.js:725-731` raccourcit les titres."),
            text_step("Toujours `styles.css` et `app.js`."),
        ];
        let ((mut runtime, observed, _), _) = measured_runtime(steps, vec![Some(MEASURED)]);
        runtime.page_target = Some(format!("path:{}", page.display()));
        let summary = runtime.run_turn("corrige le menu", None).expect("tour");
        std::env::remove_var("CLAW_DESIGN_REVIEW_ROUNDS");
        let observed = observed.borrow();
        assert_eq!(observed.requests.len(), 2, "{:?}", observed.notices);
        let note = &observed.requests[1];
        assert!(
            note.contains(
                "Your final answer is written for a developer (it talks about Claw's reviewer"
            ) && note.contains("plain words, no code"),
            "{note}"
        );
        // Une seule fois : la seconde réponse, même maladroite, est gardée.
        let last = super::message_text(summary.assistant_messages.last().expect("réponse"));
        assert!(last.contains("Toujours"), "{last}");
    }

    #[test]
    fn the_same_capture_without_any_change_is_not_rendered_again() {
        let _serial = crate::test_serial();
        // Session #31 (26 octobre 2026, passage 4) : 74 captures du modèle,
        // souvent deux de suite sans rien changer entre elles.
        std::env::set_var("CLAW_DESIGN_REVIEW_ROUNDS", "0");
        let page = measured_page("meme-capture");
        let shot = || {
            tool_step(
                "Screenshot",
                &serde_json::json!({ "path": page.display().to_string(), "viewports": ["phone"] })
                    .to_string(),
            )
        };
        let reordered = tool_step(
            "Screenshot",
            &serde_json::json!({ "viewports": ["phone"], "path": page.display().to_string() })
                .to_string(),
        );
        let write = tool_step(
            "write_file",
            &serde_json::json!({ "path": page.display().to_string(), "content": "<h1>Club</h1>" })
                .to_string(),
        );
        let steps = vec![shot(), reordered, write, shot(), text_step("Fini.")];
        let ((mut runtime, observed, _), shots) = measured_runtime(steps, vec![Some(MEASURED)]);
        runtime.run_turn("regarde la page", None).expect("tour");
        std::env::remove_var("CLAW_DESIGN_REVIEW_ROUNDS");
        let observed = observed.borrow();
        // 1 capture, la même reprise, puis après la modification : la
        // vérification de Claw et la nouvelle capture du modèle.
        assert_eq!(shots.get(), 3, "{:?}", observed.notices);
        assert!(
            observed.requests[2]
                .contains("No file changed since your last identical Screenshot call"),
            "{}",
            observed.requests[2]
        );
        assert!(!observed.requests[4]
            .rsplit("Screenshot")
            .next()
            .unwrap_or_default()
            .contains("No file changed since"));
    }

    #[test]
    fn an_answer_that_says_verified_without_looking_at_the_page_is_sent_back() {
        let _serial = crate::test_serial();
        std::env::set_var("CLAW_DESIGN_REVIEW_ROUNDS", "0");
        let page = measured_page("verifie");
        let write = tool_step(
            "write_file",
            // Un chemin Windows a des barres obliques inverses : du JSON fait par serde.
            &serde_json::json!({ "path": page.display().to_string(), "content": "<h1>Club</h1>" })
                .to_string(),
        );
        let steps = vec![
            write,
            text_step("Ce qui fonctionne (vérifié) : toutes les interactions."),
            text_step("Il reste les mots collés du titre."),
            text_step("fin"),
        ];
        // La vérification après la modification échoue (page qui ne s'ouvre
        // pas) : rien n'a regardé la page, la réponse « vérifié » est renvoyée
        // avec la mesure faite à ce moment.
        let ((mut runtime, observed, _), shots) =
            measured_runtime(steps, vec![None, Some(MEASURED)]);
        runtime.run_turn("corrige le site", None).expect("tour");
        std::env::remove_var("CLAW_DESIGN_REVIEW_ROUNDS");
        let observed = observed.borrow();
        assert_eq!(shots.get(), 2);
        assert!(
            observed.requests[2].contains("Your answer says the work is checked")
                && observed.requests[2].contains("U11 vs FC Voisins"),
            "{}",
            observed.requests[2]
        );
    }

    #[test]
    fn a_new_message_gets_a_new_task_list_and_continue_keeps_the_old_one() {
        let _serial = crate::test_serial();
        let todo = r#"{"todos":[{"content":"Boutons vert pale","status":"in_progress","activeForm":"x"},{"content":"Icones sac a dos","status":"pending","activeForm":"y"},{"content":"Header","status":"completed","activeForm":"z"}]}"#;
        let (mut runtime, observed, _) = aided_runtime(
            vec![
                tool_step("TodoWrite", todo),
                text_step("Arrêté ici."),
                text_step("Je reprends."),
                text_step("Fait."),
            ],
            vec![],
        );
        runtime
            .run_turn("corrige les boutons et les icônes du site", None)
            .ok();
        runtime.run_turn("continue", None).ok();
        runtime
            .run_turn(
                "Sur mon téléphone le titre est collé et le menu est trop haut, corrige ça",
                None,
            )
            .ok();
        let observed = observed.borrow();
        let last = observed.requests.last().expect("requête");
        assert!(
            last.contains("still has 2 unfinished task(s)") && last.contains("Boutons vert pale"),
            "{last}"
        );
        assert!(!observed
            .requests
            .iter()
            .rev()
            .nth(1)
            .expect("relance")
            .contains("unfinished task(s) ("));
    }

    #[test]
    fn verification_claims_are_found_and_denials_are_not() {
        use crate::harness::verification_claim;
        assert_eq!(
            verification_claim("## État\n**Ce qui fonctionne (vérifié) :**\n- Header").as_deref(),
            Some("**Ce qui fonctionne (vérifié) :**")
        );
        assert!(verification_claim("- Persistance localStorage testée au rechargement").is_some());
        assert!(verification_claim("Ce qui est fait mais non vérifié : le menu").is_none());
        assert!(
            verification_claim("Les boutons ne fonctionnent pas encore.\n```\nverified\n```")
                .is_none()
        );
        assert!(verification_claim("Rien n'est à vérifier ici.").is_none());
    }

    #[test]
    fn api_errors_are_shown_without_their_json_body() {
        let error = "l'api a échoué après 1 tentatives : l'api a retourné 429 Too Many Requests : [{\n  \"error\": {}\n}]";
        assert_eq!(
            super::short_error(error),
            "l'api a échoué après 1 tentatives : l'api a retourné 429 Too Many Requests"
        );
        assert_eq!(super::short_error("délai dépassé"), "délai dépassé");
        assert!(super::short_error(&"x".repeat(400)).ends_with('…'));
    }

    #[test]
    fn a_tool_written_as_a_command_is_pointed_out_to_the_model() {
        let _serial = crate::test_serial();
        let (mut runtime, observed, _) = aided_runtime(
            vec![
                tool_step("bash", r#"{"command":"KillShell bash_4"}"#),
                text_step("Serveur arrêté."),
            ],
            vec![COMMAND_FAILED],
        );
        runtime.run_turn("arrête le serveur", None).expect("tour");
        assert!(observed.borrow().requests[1].contains("`KillShell` is one of your tools"));
    }

    const ASKING: &str = "Voici l'architecture proposée.\n\nCette architecture te convient-elle ?\nVeux-tu que j'ajoute des tests ?";

    #[test]
    fn questions_written_in_the_answer_are_asked_again_with_the_menu() {
        let _serial = crate::test_serial();
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
        /// `AskUserQuestion` montre les questions sans attendre (appli web).
        static ASK_LATER: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    }

    #[test]
    fn questions_shown_for_later_end_the_turn() {
        let _serial = crate::test_serial();
        // Appli web : les questions partent avec leurs réponses à toucher et
        // l'utilisateur répond au message suivant. Le tour finit sur elles, sans
        // autre requête ni contrôle de fin.
        CAN_ASK.with(|flag| flag.set(true));
        ASK_LATER.with(|flag| flag.set(true));
        let (mut runtime, observed, _) = aided_runtime(
            vec![
                text_step(ASKING),
                tool_step(
                    "AskUserQuestion",
                    r#"{"questions":[{"question":"Continuer ?","options":[{"label":"Oui"},{"label":"Non"}]}]}"#,
                ),
                text_step("Jamais demandé."),
            ],
            vec![],
        );
        let summary = runtime.run_turn("propose puis attends", None);
        CAN_ASK.with(|flag| flag.set(false));
        ASK_LATER.with(|flag| flag.set(false));
        let summary = summary.expect("tour");
        assert_eq!(summary.iterations, 2);
        let observed = observed.borrow();
        assert_eq!(observed.requests.len(), 2);
        assert!(observed
            .notices
            .iter()
            .any(|notice| notice.contains("Claw attend ses réponses")));
        // La proposition reste la réponse du tour.
        assert_eq!(observed.superseded, 0);
    }

    #[test]
    fn a_command_sent_to_the_background_verifies_nothing_until_its_end_is_read() {
        let _serial = crate::test_serial();
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
        let _serial = crate::test_serial();
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
        // Aucune vérification imposée ; seule l'annonce est demandée (premiers
        // outils lancés sans un mot pour l'utilisateur).
        assert!(observed
            .borrow()
            .notices
            .iter()
            .all(|notice| notice.contains("sans annoncer ce qu'il fait")));
    }

    #[test]
    fn verification_gives_up_after_a_few_corrections_and_says_so() {
        let _serial = crate::test_serial();
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
        let _serial = crate::test_serial();
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
        let _serial = crate::test_serial();
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
        let _serial = crate::test_serial();
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
        let _serial = crate::test_serial();
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
    fn after_a_compaction_a_short_follow_up_finds_the_request_claw_recorded() {
        let _serial = crate::test_serial();
        use super::{earlier_request, ConversationMessage};
        let request = "Traduis les mangas du japonais vers le français, en trois étapes";
        let session = crate::session::Session {
            version: 1,
            messages: vec![
                ConversationMessage::user_text(request),
                ConversationMessage::assistant(vec![ContentBlock::Text {
                    text: "Je commence par le backend.".to_string(),
                }]),
                ConversationMessage::user_text("ok"),
                ConversationMessage::assistant(vec![ContentBlock::Text {
                    text: "Le backend est prêt.".to_string(),
                }]),
            ],
        };
        let compacted = crate::compact::compact_session(
            &session,
            crate::compact::CompactionConfig {
                preserve_recent_messages: 2,
                max_estimated_tokens: 1,
            },
        )
        .compacted_session;
        // La demande d'origine n'est plus dans les messages, mais Claw l'a
        // notée mot pour mot sous le résumé.
        assert_eq!(compacted.messages.len(), 3);
        assert_eq!(
            earlier_request(&compacted.messages, "continue").as_deref(),
            Some(request)
        );
    }

    #[test]
    fn the_review_sees_the_answer_and_the_continued_request() {
        let _serial = crate::test_serial();
        let (path, output) = written_file("reprise");
        let (mut runtime, observed, _) = aided_runtime_with(
            vec![
                // Une annonce sans fichier écrit serait renvoyée au travail
                // (`announced_work_gate`) : le premier tour finit sur une question.
                text_step("Une question avant de commencer : quel format d'image ?"),
                tool_step("write_file", r#"{"path":"total.py","content":"x"}"#),
                text_step("Fait : l'onglet Depuis une URL est en place."),
                text_step("Chaque exigence est couverte."),
            ],
            vec![PASSING],
            vec!["Step 1.\n- a page to translate an image from a URL\nStep 2.\n- done\n\nVERDICT\nLGTM"],
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
        let _serial = crate::test_serial();
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
        let _serial = crate::test_serial();
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
        let _serial = crate::test_serial();
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
        let _serial = crate::test_serial();
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
        let _serial = crate::test_serial();
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
        let _serial = crate::test_serial();
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
        let _serial = crate::test_serial();
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
        // Le modèle confronte d'abord son travail aux exigences, la demande
        // citée mot pour mot…
        assert!(observed.requests[2].contains("check your work against every requirement"));
        assert!(observed.requests[2].contains(r"word for word:\n<<<\nécris total.py\n>>>"));
        // … puis les remarques du relecteur lui reviennent.
        assert!(observed.requests[3].contains("Another model reviewed your changes"));
        assert!(observed.requests[3].contains("liste vide non gérée"));
        assert!(observed.notices.iter().any(
            |notice| notice == "Relecture par relecteur : 1 remarque(s) transmise(s) à l'agent"
        ));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn a_bare_approval_is_checked_by_a_second_reviewer() {
        let _serial = crate::test_serial();
        // Rejoué sur un vrai travail : « VERDICT / LGTM » en 8 s, sans
        // l'analyse demandée, quand un autre relecteur y trouvait un bogue.
        let (path, output) = written_file("seconde");
        let (mut runtime, observed, _) = aided_runtime_with(
            vec![
                tool_step("write_file", r#"{"path":"total.py","content":"x"}"#),
                text_step("C'est fait."),
                text_step("Exigences vérifiées."),
                text_step("Corrigé."),
            ],
            vec![PASSING],
            vec![
                "VERDICT\nLGTM",
                "Step 1.\n- the total of an empty list\nStep 2.\n- not handled\n\nVERDICT\ntotal.py:2 - liste vide non gérée - renvoyer 0",
            ],
            output,
        );
        runtime.run_turn("écris total.py", None).expect("tour");
        let observed = observed.borrow();
        assert_eq!(observed.consults.len(), 2);
        assert!(observed.notices.iter().any(|notice| notice
            == "Relecture par relecteur : approbation sans analyse, un autre modèle relit"));
        // Les remarques du second relecteur reviennent à l'agent.
        assert!(observed.requests[3].contains("liste vide non gérée"));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn the_edge_cases_of_the_requested_feature_come_with_the_request() {
        let _serial = crate::test_serial();
        // Essais réels t5 : faits d'emblée, ils évitent une reprise au contrôle.
        let (mut runtime, observed, _) = aided_runtime(vec![text_step("Fait.")], vec![]);
        runtime
            .run_turn("Ajoute une recherche dans la liste des recettes", None)
            .expect("tour");
        let (mut other, plain, _) = aided_runtime(vec![text_step("Voilà.")], vec![]);
        other.run_turn("explique ce module", None).expect("tour");
        assert!(observed.borrow().requests[0].contains(
            "[Claw guide] What experienced engineers always check for this kind of work"
        ));
        assert!(observed.borrow().requests[0].contains("without case and accents"));
        assert!(!plain.borrow().requests[0].contains("What experienced engineers"));
    }

    #[test]
    fn two_bare_approvals_are_reported_as_no_real_review() {
        let _serial = crate::test_serial();
        // Essai réel t5b : les deux relecteurs ont répondu « VERDICT / LGTM ».
        let (path, output) = written_file("deux-nues");
        let (mut runtime, observed, _) = aided_runtime_with(
            vec![
                tool_step("write_file", r#"{"path":"total.py","content":"x"}"#),
                text_step("C'est fait."),
                text_step("Exigences vérifiées."),
            ],
            vec![PASSING],
            vec!["VERDICT\nLGTM", "VERDICT\nLGTM"],
            output,
        );
        runtime.run_turn("écris total.py", None).expect("tour");
        let observed = observed.borrow();
        assert!(observed.notices.iter().any(|notice| notice.ends_with(
            "approbation sans analyse elle aussi ; le travail n'a pas vraiment été relu"
        )));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn an_approved_review_ends_the_turn_and_small_edits_are_not_reviewed() {
        let _serial = crate::test_serial();
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
        let _serial = crate::test_serial();
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
        let _serial = crate::test_serial();
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
        let _serial = crate::test_serial();
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
        let _serial = crate::test_serial();
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
    fn rewriting_lines_of_a_data_file_is_flagged_once() {
        let _serial = crate::test_serial();
        // Essai réel t3 : un petit modèle « répare » le CSV de l'utilisateur
        // au lieu du script qui ne sait pas le lire.
        let cwd = std::env::current_dir().expect("dossier");
        let rewritten = serde_json::json!({
            "filePath": cwd.join("ventes.csv").to_string_lossy(),
            "structuredPatch": [{ "lines": ["-2026-09-02;Paris;12,5", "+2026-09-02;Paris;12.5"] }],
        })
        .to_string();
        let (mut runtime, observed, _) = aided_runtime_with(
            vec![
                tool_step("write_file", r#"{"path":"ventes.csv","content":"x"}"#),
                tool_step("write_file", r#"{"path":"ventes.csv","content":"y"}"#),
                text_step("Fait."),
            ],
            vec![],
            Vec::new(),
            rewritten,
        );
        runtime.run_turn("range ventes.csv", None).expect("tour");
        let observed = observed.borrow();
        assert_eq!(
            observed
                .notices
                .iter()
                .filter(|notice| notice.contains("ventes.csv, un fichier de données"))
                .count(),
            1
        );
        assert!(observed.requests[1].contains("a data file"));
        assert!(observed.requests[1].contains("fix the code that reads the file"));
        // Les lignes d'origine sont citées, pour une remise en place exacte.
        assert!(observed.requests[1].contains("2026-09-02;Paris;12,5"));
    }

    /// Runtime dont `write_file` écrit vraiment `après` dans le fichier, et dont
    /// `bash` supprime le fichier (`rm`) ou le remet comme avant (`checkout`).
    fn file_runtime(
        path: &std::path::Path,
        steps: Vec<Vec<AssistantEvent>>,
    ) -> (
        ConversationRuntime<StepsApiClient, StaticToolExecutor>,
        std::rc::Rc<std::cell::RefCell<Observed>>,
    ) {
        let observed = std::rc::Rc::new(std::cell::RefCell::new(Observed::default()));
        let (written, target) = (path.to_path_buf(), path.to_path_buf());
        let output = serde_json::json!({ "filePath": path.to_string_lossy() }).to_string();
        let executor = StaticToolExecutor::new()
            .register("write_file", move |_| {
                std::fs::write(&written, "après\n").expect("écrit");
                Ok(output.clone())
            })
            .register("bash", move |input| {
                if input.contains("rm ") {
                    let _ = std::fs::remove_file(&target);
                } else if input.contains("checkout") {
                    std::fs::write(&target, "avant\r\n").expect("remis");
                } else if input.contains("black") {
                    std::fs::write(&target, "après  \n").expect("formaté");
                }
                Ok(PASSING.to_string())
            });
        let runtime = ConversationRuntime::new(
            Session::new(),
            StepsApiClient {
                steps: steps.into(),
                observed: std::rc::Rc::clone(&observed),
                advisor: std::collections::VecDeque::new(),
                escalation: None,
            },
            executor,
            PermissionPolicy::new(PermissionMode::DangerFullAccess),
            vec!["système".to_string()],
        )
        .with_verify(crate::harness::VerifySetting::Commands(vec![
            "verifier".to_string()
        ]));
        (runtime, observed)
    }

    fn scratch_file(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "claw-perdu-{name}-{}.py",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |elapsed| elapsed.as_nanos())
        ))
    }

    #[test]
    fn a_command_that_loses_the_agent_s_changes_is_pointed_out() {
        let _serial = crate::test_serial();
        // Essai réel t6 avec le petit modèle : `sed ... && rm conges.py`, puis
        // `git checkout conges.py` ; ses modifications perdues, deux fois.
        let path = scratch_file("garde");
        std::fs::write(&path, "avant\n").expect("fichier");
        let write =
            serde_json::json!({ "path": path.to_string_lossy(), "content": "après" }).to_string();
        let (mut runtime, observed) = file_runtime(
            &path,
            vec![
                tool_step("write_file", &write),
                tool_step("bash", r#"{"command":"black x.py"}"#),
                tool_step("bash", r#"{"command":"sed -n 1p x.py > y.py && rm x.py"}"#),
                tool_step("write_file", &write),
                tool_step("bash", r#"{"command":"git checkout x.py"}"#),
                text_step("Fait."),
            ],
        );
        runtime.run_turn("modifie le fichier", None).expect("tour");
        let observed = observed.borrow();
        // Un formateur change le fichier : rien à dire.
        assert!(!observed.requests[2].contains("[Claw] This command deleted"));
        assert!(!observed.requests[2].contains("is back to its state"));
        // Supprimé : la dernière version (celle du formateur) est gardée.
        assert!(observed.requests[3].contains("[Claw] This command deleted `"));
        let kept =
            crate::harness::kept_copy_path(&std::env::current_dir().expect("dossier"), &path);
        assert!(observed.requests[3].contains(&crate::harness::shown_path(&kept.to_string_lossy())));
        assert!(observed.requests[3].contains("restores it in one command"));
        // Ramené à son état du début du tour, fins de ligne changées comprises.
        assert!(observed.requests[5].contains("is back to its state from before your changes"));
        assert_eq!(std::fs::read_to_string(&kept).expect("copie"), "après\n");
        assert!(observed
            .notices
            .iter()
            .any(|notice| notice.starts_with("Attention : une commande a supprimé ")));
        assert!(observed.notices.iter().any(|notice| notice
            .starts_with("Attention : une commande a ramené ")
            && notice.contains(
                " à son état du début du tour, effaçant les modifications du modèle ; "
            )));
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(&kept);
    }

    /// Runtime où un test échoue (`python -m pytest`), puis où `edit_file`
    /// rend le résultat donné après avoir écrit `after` dans `path`.
    fn guarded_runtime(
        path: &std::path::Path,
        after: &'static str,
        patch_lines: &[&str],
    ) -> (
        ConversationRuntime<StepsApiClient, StaticToolExecutor>,
        std::rc::Rc<std::cell::RefCell<Observed>>,
    ) {
        let observed = std::rc::Rc::new(std::cell::RefCell::new(Observed::default()));
        let target = path.to_path_buf();
        let output = serde_json::json!({
            "filePath": path.to_string_lossy(),
            "structuredPatch": [{ "lines": patch_lines }],
        })
        .to_string();
        let executor = StaticToolExecutor::new()
            .register("edit_file", move |_| {
                std::fs::write(&target, after).expect("écrit");
                Ok(output.clone())
            })
            .register("bash", |input| {
                Ok(if input.contains("pytest") {
                    FAILING
                } else {
                    PASSING
                }
                .to_string())
            });
        let edit = serde_json::json!({ "path": path.to_string_lossy(), "old_string": "x", "new_string": "y" })
            .to_string();
        let runtime = ConversationRuntime::new(
            Session::new(),
            StepsApiClient {
                steps: vec![
                    tool_step("bash", r#"{"command":"python -m pytest"}"#),
                    tool_step("edit_file", &edit),
                    // Contrôles de fin de tour (preuves, exigences) : de quoi répondre.
                    text_step("Fait."),
                    text_step("Fait."),
                    text_step("Fait."),
                ]
                .into(),
                observed: std::rc::Rc::clone(&observed),
                advisor: std::collections::VecDeque::new(),
                escalation: None,
            },
            executor,
            PermissionPolicy::new(PermissionMode::DangerFullAccess),
            vec!["système".to_string()],
        )
        .with_verify(crate::harness::VerifySetting::Commands(vec![
            "verifier".to_string()
        ]));
        (runtime, observed)
    }

    #[test]
    fn reading_a_file_already_shown_in_an_up_to_date_view_is_pointed_out() {
        let _serial = crate::test_serial();
        // Essai réel t6 : le petit modèle relisait les fichiers qu'une vue lui
        // montrait déjà en entier.
        let path = std::env::temp_dir().join(format!(
            "claw-vue-{}.py",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |elapsed| elapsed.as_nanos())
        ));
        std::fs::write(&path, "a = 1\n").expect("fichier");
        let observed = std::rc::Rc::new(std::cell::RefCell::new(Observed::default()));
        let target = path.clone();
        let edited = serde_json::json!({
            "filePath": path.to_string_lossy(),
            "structuredPatch": [{ "lines": ["-a = 1", "+a = 2"] }],
        })
        .to_string();
        let read = serde_json::json!({
            "type": "text",
            "file": { "filePath": path.to_string_lossy(), "content": "a = 2\n" },
        })
        .to_string();
        let executor = StaticToolExecutor::new()
            .register("edit_file", move |_| {
                std::fs::write(&target, "a = 2\n").expect("écrit");
                Ok(edited.clone())
            })
            .register("read_file", move |_| Ok(read.clone()))
            .register("bash", |_| Ok(PASSING.to_string()));
        let edit = serde_json::json!({ "path": path.to_string_lossy(), "old_string": "a = 1", "new_string": "a = 2" })
            .to_string();
        let read_input = serde_json::json!({ "path": path.to_string_lossy() }).to_string();
        let mut runtime = ConversationRuntime::new(
            Session::new(),
            StepsApiClient {
                steps: vec![
                    tool_step("edit_file", &edit),
                    tool_step("read_file", &read_input),
                    text_step("Fait."),
                    text_step("Fait."),
                ]
                .into(),
                observed: std::rc::Rc::clone(&observed),
                advisor: std::collections::VecDeque::new(),
                escalation: None,
            },
            executor,
            PermissionPolicy::new(PermissionMode::DangerFullAccess),
            vec!["système".to_string()],
        )
        .with_verify(crate::harness::VerifySetting::Commands(vec![
            "verifier".to_string()
        ]));
        runtime.run_turn("change a", None).expect("tour");
        let observed = observed.borrow();
        assert!(observed.requests[1].contains("as it is now"));
        assert!(observed.requests[2]
            .contains("is already shown in full above, in a view that is still up to date"));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn rewriting_an_existing_check_is_guarded_but_not_the_agent_s_own_new_test() {
        let _serial = crate::test_serial();
        let folder = std::env::temp_dir().join(format!(
            "claw-garde-tests-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |elapsed| elapsed.as_nanos())
        ));
        std::fs::create_dir_all(&folder).expect("dossier");
        let path = folder.join("test_stock.py");
        let original = "def test_a():\n    assert total() == 1\n";
        // Le modèle change une ligne qui existait : la garde parle.
        std::fs::write(&path, original).expect("tests");
        let (mut runtime, observed) = guarded_runtime(
            &path,
            "def test_a():\n    assert total() == 2\n",
            &["-    assert total() == 1", "+    assert total() == 2"],
        );
        runtime.run_turn("corrige", None).expect("tour");
        assert!(observed.borrow().requests[2].contains("which is used to check the work"));
        // Il corrige un test qu'il avait ajouté pendant le tour : rien à dire.
        std::fs::write(&path, original).expect("tests");
        let (mut runtime, observed) = guarded_runtime(
            &path,
            "def test_a():\n    assert total() == 1\n\ndef test_b():\n    assert stock() == 3\n",
            &["-    assert stock() == 2", "+    assert stock() == 3"],
        );
        runtime.run_turn("corrige", None).expect("tour");
        assert!(!observed.borrow().requests[2].contains("which is used to check the work"));
        // Il retire une copie d'une ligne d'origine qu'il avait mise dans un
        // test neuf (essai réel t5 : `assert statut == 200`) : rien non plus.
        std::fs::write(&path, original).expect("tests");
        let (mut runtime, observed) = guarded_runtime(
            &path,
            "def test_a():\n    assert total() == 1\n\ndef test_b():\n    pass\n",
            &["-    assert total() == 1", "+    pass"],
        );
        runtime.run_turn("corrige", None).expect("tour");
        assert!(!observed.borrow().requests[2].contains("which is used to check the work"));
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn a_data_file_reshaped_for_the_new_code_is_pointed_out() {
        let _serial = crate::test_serial();
        // Essai réel t7, deux fois : `"inscrits": []` ajouté à chaque atelier
        // de `donnees.json`, et un code qui plante sur l'ancien fichier.
        let path = std::env::temp_dir().join(format!(
            "claw-donnees-{}.json",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |elapsed| elapsed.as_nanos())
        ));
        std::fs::write(
            &path,
            r#"{"ateliers": {"Poterie": {"places": 2}, "Couture": {"places": 8}}}"#,
        )
        .expect("données");
        let observed = std::rc::Rc::new(std::cell::RefCell::new(Observed::default()));
        let target = path.clone();
        let output = serde_json::json!({ "filePath": path.to_string_lossy() }).to_string();
        let executor = StaticToolExecutor::new()
            .register("write_file", move |_| {
                std::fs::write(
                    &target,
                    r#"{"ateliers": {"Poterie": {"places": 2, "inscrits": []}, "Couture": {"places": 8, "inscrits": []}}}"#,
                )
                .expect("écrit");
                Ok(output.clone())
            })
            .register("bash", |_| Ok(PASSING.to_string()));
        let write =
            serde_json::json!({ "path": path.to_string_lossy(), "content": "x" }).to_string();
        let mut runtime = ConversationRuntime::new(
            Session::new(),
            StepsApiClient {
                steps: vec![tool_step("write_file", &write), text_step("Fait.")].into(),
                observed: std::rc::Rc::clone(&observed),
                advisor: std::collections::VecDeque::new(),
                escalation: None,
            },
            executor,
            PermissionPolicy::new(PermissionMode::DangerFullAccess),
            vec!["système".to_string()],
        )
        .with_verify(crate::harness::VerifySetting::Commands(vec![
            "verifier".to_string()
        ]));
        runtime
            .run_turn("ajoute les inscriptions", None)
            .expect("tour");
        let observed = observed.borrow();
        assert!(observed.requests[1]
            .contains("you added `inscrits` to entries that were already there"));
        assert!(observed
            .notices
            .iter()
            .any(|notice| notice.contains("les données que le programme enregistre")));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_file_created_and_removed_in_the_same_turn_is_not_reported() {
        let _serial = crate::test_serial();
        // Un script de travail écrit puis effacé par le modèle : voulu.
        let path = scratch_file("travail");
        let write =
            serde_json::json!({ "path": path.to_string_lossy(), "content": "après" }).to_string();
        let (mut runtime, observed) = file_runtime(
            &path,
            vec![
                tool_step("write_file", &write),
                tool_step("bash", r#"{"command":"rm x.py"}"#),
                text_step("Fait."),
            ],
        );
        runtime.run_turn("essaie un script", None).expect("tour");
        let observed = observed.borrow();
        assert!(!observed.requests[2].contains("[Claw] This command deleted"));
        assert!(!observed
            .notices
            .iter()
            .any(|notice| notice.starts_with("Attention : une commande a supprimé")));
    }

    #[test]
    fn a_data_file_created_in_the_turn_is_the_agent_s_own() {
        let _serial = crate::test_serial();
        // Créé puis retouché pendant le même tour : pas les données de
        // l'utilisateur, la garde ne dit rien.
        let cwd = std::env::current_dir().expect("dossier");
        let created = serde_json::json!({
            "filePath": cwd.join("exemple.csv").to_string_lossy(),
            "structuredPatch": [{ "lines": ["+a;b", "+1;2"] }],
        })
        .to_string();
        let (mut runtime, observed, _) = aided_runtime_with(
            vec![
                tool_step("write_file", r#"{"path":"exemple.csv","content":"x"}"#),
                text_step("Fait."),
            ],
            vec![],
            Vec::new(),
            created,
        );
        runtime.run_turn("ajoute un exemple", None).expect("tour");
        assert!(!observed
            .borrow()
            .notices
            .iter()
            .any(|notice| notice.contains("fichier de données")));
    }

    #[test]
    fn three_single_calls_in_a_row_get_a_note_about_grouping() {
        let _serial = crate::test_serial();
        // Essais réels t2 : sept commandes d'essai, chacune dans sa requête.
        let (mut runtime, observed, _) = aided_runtime(
            vec![
                tool_step("bash", r#"{"command":"python notes.py chercher a"}"#),
                tool_step("bash", r#"{"command":"python notes.py chercher b"}"#),
                tool_step("bash", r#"{"command":"python notes.py chercher c"}"#),
                tool_step("bash", r#"{"command":"python notes.py chercher d"}"#),
                text_step("Fait."),
            ],
            vec![PASSING, PASSING, PASSING, PASSING],
        );
        runtime.run_turn("essaie la recherche", None).expect("tour");
        let observed = observed.borrow();
        // La remarque arrive avec le troisième résultat, une seule fois, et
        // montre les trois commandes réunies en un appel.
        assert!(!observed.requests[2].contains("each made a single call"));
        assert!(observed.requests[3].contains("each made a single call"));
        assert!(observed.requests[3].contains(
            "fit in one `bash` call: `python notes.py chercher a; python notes.py chercher b; python notes.py chercher c`"
        ));
        assert_eq!(
            observed.requests[4]
                .matches("each made a single call")
                .count(),
            1
        );
    }

    #[test]
    fn single_calls_are_counted_through_failing_commands_and_noted_again() {
        let _serial = crate::test_serial();
        // Essai réel t4 : douze commandes une par requête. Un refus du guichet
        // essayé exprès (code de sortie 1) remettait le compteur à zéro, et la
        // remarque, venue tard, n'était dite qu'une fois.
        let mut steps = (0..13)
            .map(|n| {
                tool_step(
                    "bash",
                    &format!(r#"{{"command":"cd projet && python biblio.py essai{n}"}}"#),
                )
            })
            .collect::<Vec<_>>();
        steps.push(text_step("Fait."));
        let mut outputs = vec![PASSING; 13];
        outputs[1] = FAILING;
        outputs[4] = FAILING;
        let (mut runtime, observed, _) = aided_runtime(steps, outputs);
        runtime.run_turn("essaie le guichet", None).expect("tour");
        let observed = observed.borrow();
        assert!(!observed.requests[2].contains("made a single call"));
        // Troisième réponse d'un seul appel, malgré le refus de la deuxième ;
        // le `cd` commun n'est écrit qu'une fois dans l'exemple.
        assert!(observed.requests[3].contains("Your last 3 replies each made a single call"));
        assert!(observed.requests[3].contains(
            "`cd projet && python biblio.py essai0; python biblio.py essai1; python biblio.py essai2`"
        ));
        // Redite à la sixième et à la neuvième, puis plus rien.
        assert!(!observed.requests[5].contains("6 replies in a row"));
        assert!(observed.requests[6].contains("[Claw] 6 replies in a row"));
        assert!(observed.requests[9].contains("[Claw] 9 replies in a row"));
        let last = observed.requests.last().expect("requêtes");
        assert!(!last.contains("12 replies in a row"));
        assert_eq!(last.matches("made a single call").count(), 3);
    }

    #[test]
    fn a_tool_error_or_a_grouped_reply_breaks_the_series_of_single_calls() {
        let _serial = crate::test_serial();
        let (mut runtime, observed, _) = aided_runtime(
            vec![
                tool_step("bash", r#"{"command":"python a.py"}"#),
                tool_step("bash", r#"{"command":"python b.py"}"#),
                tool_step("probe", "{}"),
                tool_step("bash", r#"{"command":"python c.py"}"#),
                vec![
                    AssistantEvent::ToolUse {
                        id: "un".to_string(),
                        name: "bash".to_string(),
                        input: r#"{"command":"python d.py"}"#.to_string(),
                    },
                    AssistantEvent::ToolUse {
                        id: "deux".to_string(),
                        name: "bash".to_string(),
                        input: r#"{"command":"python e.py"}"#.to_string(),
                    },
                    AssistantEvent::MessageStop,
                ],
                tool_step("bash", r#"{"command":"python f.py"}"#),
                text_step("Fait."),
            ],
            vec![PASSING; 6],
        );
        runtime.run_turn("essaie", None).expect("tour");
        let observed = observed.borrow();
        assert!(!observed
            .requests
            .last()
            .expect("requêtes")
            .contains("made a single call"));
    }

    #[test]
    fn a_task_list_sent_alone_is_told_to_travel_with_the_next_call() {
        let _serial = crate::test_serial();
        // Essai réel t4 : sept requêtes pour la seule mise à jour de la liste.
        let open = r#"{"oldTodos":[],"newTodos":[{"content":"a","activeForm":"a","status":"in_progress"},{"content":"b","activeForm":"b","status":"pending"}]}"#;
        let done = r#"{"oldTodos":[],"newTodos":[{"content":"a","activeForm":"a","status":"completed"},{"content":"b","activeForm":"b","status":"completed"}]}"#;
        let todo = |id: &str| AssistantEvent::ToolUse {
            id: id.to_string(),
            name: "TodoWrite".to_string(),
            input: r#"{"todos":[]}"#.to_string(),
        };
        let (mut runtime, observed, _) = aided_runtime(
            vec![
                vec![todo("liste-1"), AssistantEvent::MessageStop],
                vec![
                    todo("liste-2"),
                    AssistantEvent::ToolUse {
                        id: "commande".to_string(),
                        name: "bash".to_string(),
                        input: r#"{"command":"python a.py"}"#.to_string(),
                    },
                    AssistantEvent::MessageStop,
                ],
                vec![todo("liste-3"), AssistantEvent::MessageStop],
                text_step("Fait."),
            ],
            vec![PASSING],
        );
        let mut outputs = std::collections::VecDeque::from(vec![open, open, done]);
        runtime.tool_executor = std::mem::take(&mut runtime.tool_executor)
            .register("TodoWrite", move |_| {
                Ok(outputs.pop_front().unwrap_or(done).to_string())
            });
        runtime.run_turn("fais a puis b", None).expect("tour");
        let observed = observed.borrow();
        // Seule, avec du travail à suivre : dit. Avec une commande dans la
        // même réponse, ou pour clore la liste : rien.
        assert!(observed.requests[1].contains("This reply only updated the task list"));
        assert_eq!(
            observed
                .requests
                .last()
                .expect("requêtes")
                .matches("This reply only updated the task list")
                .count(),
            1
        );
    }

    #[test]
    fn a_final_answer_with_open_tasks_is_sent_back_to_finish_them() {
        let _serial = crate::test_serial();
        // Vu le 24 octobre 2026 dans Claw Web : réponse finale sous une liste
        // à 2/4, « Relancer tous les tests » en cours, « Commiter » à faire.
        let open = r#"{"oldTodos":[],"newTodos":[{"content":"Corriger le bug","activeForm":"a","status":"completed"},{"content":"Relancer tous les tests","activeForm":"b","status":"in_progress"},{"content":"Commiter les changements","activeForm":"c","status":"pending"}]}"#;
        let done = r#"{"oldTodos":[],"newTodos":[{"content":"Corriger le bug","activeForm":"a","status":"completed"},{"content":"Relancer tous les tests","activeForm":"b","status":"completed"},{"content":"Commiter les changements","activeForm":"c","status":"completed"}]}"#;
        let todo = |id: &str| AssistantEvent::ToolUse {
            id: id.to_string(),
            name: "TodoWrite".to_string(),
            input: r#"{"todos":[]}"#.to_string(),
        };
        let (mut runtime, observed, _) = aided_runtime(
            vec![
                vec![
                    todo("liste-1"),
                    AssistantEvent::ToolUse {
                        id: "lecture".to_string(),
                        name: "read_file".to_string(),
                        input: r#"{"path":"a.py"}"#.to_string(),
                    },
                    AssistantEvent::MessageStop,
                ],
                text_step("Le bug est corrigé."),
                vec![
                    todo("liste-2"),
                    AssistantEvent::ToolUse {
                        id: "tests".to_string(),
                        name: "bash".to_string(),
                        input: r#"{"command":"python -m pytest"}"#.to_string(),
                    },
                    AssistantEvent::MessageStop,
                ],
                text_step("Le bug est corrigé, les tests passent et c'est commité."),
            ],
            vec![PASSING],
        );
        let mut outputs = std::collections::VecDeque::from(vec![open, done]);
        runtime.tool_executor = std::mem::take(&mut runtime.tool_executor)
            .register("TodoWrite", move |_| {
                Ok(outputs.pop_front().unwrap_or(done).to_string())
            })
            .register("read_file", |_| Ok("print(1)".to_string()));
        let summary = runtime.run_turn("corrige le bug", None).expect("tour");
        let observed = observed.borrow();
        // La première conclusion est renvoyée avec les deux tâches ouvertes ;
        // la liste finie, la seconde est la réponse (puis sa mise au propre,
        // comme après tout renvoi).
        assert_eq!(observed.requests.len(), 5);
        let sent_back = &observed.requests[2];
        assert!(sent_back.contains("2 unfinished task(s)"));
        assert!(sent_back.contains("- Relancer tous les tests (in progress)"));
        assert!(sent_back.contains("- Commiter les changements (not started)"));
        assert!(!sent_back.contains("- Corriger le bug"));
        assert_eq!(
            observed.requests[4].matches("unfinished task(s)").count(),
            1
        );
        let last = super::message_text(summary.assistant_messages.last().expect("réponse"));
        assert!(last.contains("c'est commité"));
    }

    #[test]
    fn a_turn_that_reaches_its_request_limit_is_asked_to_conclude() {
        let _serial = crate::test_serial();
        // Essai réel t4 (petit modèle) : 250 requêtes en 25 minutes, plus
        // aucun fichier modifié après la sixième minute, et rien pour l'arrêter.
        let (runtime, observed, bash_calls) = aided_runtime(
            vec![
                tool_step("write_file", r#"{"path":"notes.txt","content":"x"}"#),
                tool_step("bash", r#"{"command":"python essai.py 1"}"#),
                tool_step("bash", r#"{"command":"python essai.py 2"}"#),
                text_step(
                    "Bilan : le fichier est écrit, deux essais faits, rien d'autre de vérifié.",
                ),
                text_step("Ne doit pas être demandé."),
            ],
            vec![PASSING, PASSING],
        );
        let mut runtime = runtime.with_turn_request_limit(3);
        let summary = runtime.run_turn("écris et essaie", None).expect("tour");
        // Trois requêtes de travail, une pour conclure : la vérification
        // automatique (fichier modifié) ne renvoie pas le modèle au travail.
        assert_eq!(summary.iterations, 4);
        assert_eq!(bash_calls.borrow().len(), 2);
        let observed = observed.borrow();
        assert_eq!(observed.requests.len(), 4);
        assert!(!observed.requests[2].contains("the limit for one turn"));
        assert!(observed.requests[3].contains("[Claw] This turn has used 3 requests"));
        // La conclusion part sans outils : le modèle ne peut que répondre.
        assert_eq!(observed.text_only, vec![3]);
        assert!(observed
            .notices
            .iter()
            .any(|notice| notice.contains("la limite d'un tour")));
        // Le tour suivant repart avec un budget entier.
        drop(observed);
        runtime.run_turn("continue", None).expect("tour suivant");
    }

    #[test]
    fn a_turn_with_open_tasks_and_progress_goes_past_its_request_limit() {
        let _serial = crate::test_serial();
        // Session #31 du 25 octobre 2026 : arrêt à la limite de 100 requêtes
        // sur « Tâches 0/9 », et l'utilisateur : « Faut l'interdire de
        // s'arrêter s'il n'a pas fini les tâches ».
        let open = r#"{"oldTodos":[],"newTodos":[{"content":"Écrire a","activeForm":"a","status":"in_progress"},{"content":"Écrire b","activeForm":"b","status":"pending"}]}"#;
        let done = r#"{"oldTodos":[],"newTodos":[{"content":"Écrire a","activeForm":"a","status":"completed"},{"content":"Écrire b","activeForm":"b","status":"completed"}]}"#;
        let todo = |id: &str| AssistantEvent::ToolUse {
            id: id.to_string(),
            name: "TodoWrite".to_string(),
            input: r#"{"todos":[]}"#.to_string(),
        };
        let write = |id: &str, path: &str| AssistantEvent::ToolUse {
            id: id.to_string(),
            name: "write_file".to_string(),
            input: format!(r#"{{"path":"{path}","content":"x"}}"#),
        };
        let (runtime, observed, _) = aided_runtime(
            vec![
                vec![
                    todo("liste-1"),
                    write("a", "a.txt"),
                    AssistantEvent::MessageStop,
                ],
                tool_step("bash", r#"{"command":"python essai.py 1"}"#),
                tool_step("bash", r#"{"command":"python essai.py 2"}"#),
                vec![
                    todo("liste-2"),
                    write("b", "b.txt"),
                    AssistantEvent::MessageStop,
                ],
                text_step("a et b sont écrits."),
            ],
            vec![PASSING, PASSING, PASSING],
        );
        let mut outputs = std::collections::VecDeque::from(vec![open, done]);
        let mut runtime = runtime.with_turn_request_limit(3);
        runtime.tool_executor = std::mem::take(&mut runtime.tool_executor)
            .register("TodoWrite", move |_| {
                Ok(outputs.pop_front().unwrap_or(done).to_string())
            });
        let summary = runtime.run_turn("écris a puis b", None).expect("tour");
        let observed = observed.borrow();
        // La limite (3) tombe sur une liste ouverte dont le travail avance :
        // un palier de plus, la liste se finit et la réponse suit.
        assert!(observed.notices.iter().any(|notice| notice.contains(
            "Tâches pas finies (2 restantes) : le tour continue, 40 requêtes de plus (1/3)"
        )));
        assert!(!observed
            .requests
            .iter()
            .any(|request| request.contains("the limit for one turn")));
        let last = super::message_text(summary.assistant_messages.last().expect("réponse"));
        assert!(last.contains("a et b sont écrits"));
        drop(observed);

        // Sans rien de changé, la limite tient : le tour conclut et le dit.
        let (runtime, observed, _) = aided_runtime(
            vec![
                vec![
                    todo("liste-1"),
                    AssistantEvent::ToolUse {
                        id: "lecture".to_string(),
                        name: "read_file".to_string(),
                        input: r#"{"path":"a.py"}"#.to_string(),
                    },
                    AssistantEvent::MessageStop,
                ],
                tool_step("read_file", r#"{"path":"b.py"}"#),
                tool_step("read_file", r#"{"path":"c.py"}"#),
                text_step("Je n'ai rien changé."),
            ],
            vec![PASSING],
        );
        let mut runtime = runtime.with_turn_request_limit(3);
        runtime.tool_executor = std::mem::take(&mut runtime.tool_executor)
            .register("TodoWrite", move |_| Ok(open.to_string()))
            .register("read_file", |_| Ok("print(1)".to_string()));
        runtime.run_turn("regarde", None).expect("tour");
        let observed = observed.borrow();
        assert!(observed.notices.iter().any(|notice| notice
            .contains("Tâches pas finies (2 restantes) mais le tour conclut : rien n'a avancé")));
        assert!(observed
            .requests
            .last()
            .is_some_and(|request| request.contains("[Claw] This turn has used 3 requests")));
    }

    #[test]
    fn tool_calls_made_after_the_request_limit_do_not_run() {
        let _serial = crate::test_serial();
        let (runtime, observed, bash_calls) = aided_runtime(
            vec![
                tool_step("bash", r#"{"command":"python essai.py 1"}"#),
                tool_step("bash", r#"{"command":"python essai.py 2"}"#),
                tool_step("bash", r#"{"command":"python essai.py 3"}"#),
                text_step("Ne doit pas être demandé."),
            ],
            vec![PASSING, PASSING],
        );
        let mut runtime = runtime.with_turn_request_limit(2);
        let summary = runtime.run_turn("essaie", None).expect("tour");
        assert_eq!(summary.iterations, 3);
        // Le troisième appel, fait après la limite, n'a pas tourné.
        assert_eq!(bash_calls.borrow().len(), 2);
        let last = summary.tool_results.last().expect("résultat");
        assert!(super::tool_result_parts(last)
            .expect("sortie")
            .0
            .contains("Not run: this turn reached its limit of 2 requests"));
        assert_eq!(observed.borrow().requests.len(), 3);
        // Chaque appel a son résultat : la session reste valide pour la suite.
        let calls = runtime
            .session()
            .messages
            .iter()
            .flat_map(|message| &message.blocks)
            .filter(|block| matches!(block, ContentBlock::ToolUse { .. }))
            .count();
        let results = runtime
            .session()
            .messages
            .iter()
            .flat_map(|message| &message.blocks)
            .filter(|block| matches!(block, ContentBlock::ToolResult { .. }))
            .count();
        assert_eq!(calls, results);
    }

    #[test]
    fn when_the_session_time_is_over_the_model_concludes_and_no_tool_runs() {
        let _serial = crate::test_serial();
        let (runtime, observed, bash_calls) = aided_runtime(
            vec![
                tool_step("bash", r#"{"command":"python essai.py"}"#),
                text_step("Ne doit pas être demandé."),
            ],
            vec![PASSING],
        );
        // Échéance déjà passée : le moteur de Claw Web va couper le job.
        let mut runtime = runtime.with_deadline(Some(1));
        let summary = runtime.run_turn("essaie", None).expect("tour");
        assert_eq!(bash_calls.borrow().len(), 0);
        let last = summary.tool_results.last().expect("résultat");
        assert!(super::tool_result_parts(last)
            .expect("sortie")
            .0
            .contains("the time given to this session is over"));
        assert_eq!(observed.borrow().requests.len(), 1);
        // La conclusion s'adresse à l'utilisateur, point par point, sans code
        // (session #31 : du CSS et « relancer Screenshot » pour un parent).
        let request = &observed.borrow().requests[0];
        assert!(
            request
                .contains("written for the user, who may not be a developer: plain words, no code")
                && request.contains("Go through each thing the user asked")
                && request.contains("anything that no longer works"),
            "{request}"
        );
        assert!(crate::harness::deadline_reached(Some(1)));
        assert!(!crate::harness::deadline_reached(None));
        assert_eq!(crate::harness::parse_deadline(Some(" 100 ")), Some(100));
        assert_eq!(crate::harness::parse_deadline(Some("demain")), None);
    }

    #[test]
    fn a_file_read_again_unchanged_is_kept_once_and_said() {
        let _serial = crate::test_serial();
        // Essai réel t4 (petit modèle) : chaque fichier, lu au début du tour,
        // est relu juste avant d'être modifié. Quatre requêtes sur dix.
        let read = |content: &str| {
            serde_json::json!({ "type": "text", "file": {
                "filePath": std::env::current_dir().expect("dossier").join("services.py").to_string_lossy(),
                "content": content, "numLines": 1, "startLine": 1, "totalLines": 1 } })
            .to_string()
        };
        let (mut runtime, observed, _) = aided_runtime(
            vec![
                tool_step("read_file", r#"{"path":"services.py"}"#),
                tool_step("read_file", r#"{"path":"services.py"}"#),
                tool_step("read_file", r#"{"path":"services.py"}"#),
                text_step("Lu."),
                tool_step("read_file", r#"{"path":"services.py"}"#),
                text_step("Relu."),
            ],
            vec![],
        );
        let mut outputs = std::collections::VecDeque::from(vec![
            read("def emprunter(): pass"),
            read("def emprunter(): pass"),
            read("def emprunter(): return 1"),
            read("def emprunter(): return 1"),
        ]);
        runtime.tool_executor = std::mem::take(&mut runtime.tool_executor)
            .register("read_file", move |_| {
                Ok(outputs.pop_front().expect("lecture"))
            });
        runtime.run_turn("lis services.py", None).expect("tour");
        {
            let observed = observed.borrow();
            // Deuxième lecture, identique : dite, et l'ancienne copie s'efface.
            assert!(!observed.requests[1].contains("has not changed since you read it"));
            assert_eq!(
                observed.requests[1]
                    .matches("def emprunter(): pass")
                    .count(),
                1
            );
            assert!(observed.requests[2]
                .contains("`services.py` has not changed since you read it earlier in this turn"));
            assert_eq!(
                observed.requests[2]
                    .matches("def emprunter(): pass")
                    .count(),
                1
            );
            assert!(observed.requests[2].contains("An earlier read of `services.py` was here"));
            // Contenu différent (le fichier a changé) : une lecture normale, sans
            // reproche ; l'ancienne version laisse la place à la nouvelle (vu dans
            // une vraie session : les copies périmées s'accumulaient).
            assert_eq!(
                observed.requests[3]
                    .matches("has not changed since you read it")
                    .count(),
                1
            );
            assert_eq!(
                observed.requests[3]
                    .matches("def emprunter(): pass")
                    .count(),
                0
            );
            assert!(observed.requests[3]
                .contains("An older read of `services.py` was here. You read the file again later, after it changed"));
            assert_eq!(
                observed.requests[3]
                    .matches("def emprunter(): return 1")
                    .count(),
                1
            );
        }
        // Tour suivant : relire le même contenu n'est pas reproché, mais une
        // seule copie reste dans la conversation.
        runtime.run_turn("relis-le", None).expect("tour");
        let observed = observed.borrow();
        let last = observed.requests.last().expect("requêtes");
        assert_eq!(last.matches("has not changed since you read it").count(), 1);
        assert_eq!(last.matches("def emprunter(): return 1").count(), 1);
    }

    #[test]
    fn a_long_file_read_whole_again_and_again_gets_a_note_and_one_copy() {
        let _serial = crate::test_serial();
        // Vraie session de Claw Web : `base.html` (52 Ko) relu 28 fois en un tour.
        let read = |version: usize| {
            let content = format!(
                "<!-- version {version} -->\n{}",
                "<p>ligne</p>\n".repeat(1_200)
            );
            serde_json::json!({ "type": "text", "file": {
                "filePath": std::env::current_dir().expect("dossier").join("base.html").to_string_lossy(),
                "content": content, "numLines": 1_201, "startLine": 1, "totalLines": 1_201 } })
            .to_string()
        };
        let mut steps: Vec<Vec<AssistantEvent>> = (0..3)
            .map(|_| tool_step("read_file", r#"{"path":"base.html"}"#))
            .collect();
        steps.push(text_step("Lu."));
        let (mut runtime, observed, _) = aided_runtime(steps, vec![]);
        let mut outputs = std::collections::VecDeque::from(vec![read(1), read(2), read(3)]);
        runtime.tool_executor = std::mem::take(&mut runtime.tool_executor)
            .register("read_file", move |_| {
                Ok(outputs.pop_front().expect("lecture"))
            });
        runtime.run_turn("refais la page", None).expect("tour");
        let observed = observed.borrow();
        let last = observed.requests.last().expect("requêtes");
        // Une seule copie du fichier (la dernière), et la remarque à la troisième lecture.
        assert_eq!(
            last.matches("<p>ligne</p>").count(),
            1_200,
            "une seule copie gardée"
        );
        assert!(last.contains("<!-- version 3 -->") && !last.contains("<!-- version 1 -->"));
        assert!(last.contains("is long (1201 lines) and you have now read it whole 3 times"));
    }

    #[test]
    fn the_session_is_saved_at_each_step_of_a_turn() {
        let _serial = crate::test_serial();
        // Essai réel t4 (north) : arrêté après 25 minutes, le fichier de
        // session ne contenait rien du tour.
        let dir = std::env::temp_dir().join(format!("claw-autosave-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dossier");
        let path = dir.join("session.json");
        let (mut runtime, _, _) = aided_runtime(
            vec![
                tool_step("bash", r#"{"command":"python long.py"}"#),
                text_step("Fait."),
            ],
            vec![],
        );
        let during = std::rc::Rc::new(std::cell::RefCell::new(String::new()));
        let seen = std::rc::Rc::clone(&during);
        let read = path.clone();
        runtime.tool_executor =
            std::mem::take(&mut runtime.tool_executor).register("bash", move |_| {
                *seen.borrow_mut() = std::fs::read_to_string(&read).unwrap_or_default();
                Ok(PASSING.to_string())
            });
        runtime.autosave_to(&path);
        runtime.run_turn("lance le calcul", None).expect("tour");
        let after = std::fs::read_to_string(&path).expect("session enregistrée");
        let _ = std::fs::remove_dir_all(&dir);

        // Pendant la commande : la demande, l'appel en cours, et un résultat
        // qui dit qu'il a été interrompu (la session reste valide).
        let during = during.borrow();
        assert!(during.contains("lance le calcul"));
        assert!(during.contains("python long.py"));
        assert!(during.contains("Claw was stopped while this call was running"));
        let saved = Session::from_json(&crate::json::JsonValue::parse(&during).expect("JSON"))
            .expect("session");
        assert_eq!(saved.messages.len(), 3);
        // À la fin : le vrai résultat et la réponse.
        assert!(!after.contains("Claw was stopped while this call was running"));
        assert!(after.contains("Fait."));
    }

    #[test]
    fn only_the_latest_view_of_a_file_is_kept() {
        let _serial = crate::test_serial();
        use crate::harness::{FILE_VIEW_END, FILE_VIEW_PREFIX, FILE_VIEW_REPLACED};
        use crate::session::ConversationMessage;
        let view = |name: &str, text: &str| {
            format!(
                "{{\"edited\": true}}\n\n{FILE_VIEW_PREFIX}`{name}` as it is now (1 lines), after your change. You do not need to read it again.\n{text}\n{FILE_VIEW_END}"
            )
        };
        let mut session = Session::new();
        session.messages.push(ConversationMessage::tool_result(
            "t1",
            "edit_file",
            view("notes.py", "a = 1"),
            false,
        ));
        session.messages.push(ConversationMessage::tool_result(
            "t2",
            "edit_file",
            view("autre.py", "x = 1"),
            false,
        ));
        super::replace_file_views(
            &mut session,
            &format!("{FILE_VIEW_PREFIX}`notes.py`"),
            FILE_VIEW_REPLACED,
        );
        let text = |index: usize| match &session.messages[index].blocks[0] {
            ContentBlock::ToolResult { output, .. } => output.clone(),
            _ => String::new(),
        };
        assert!(!text(0).contains("a = 1"), "{}", text(0));
        assert!(text(0).contains(FILE_VIEW_REPLACED));
        assert!(text(1).contains("x = 1"));
        // Session rouverte : aucune vue ne dit plus « tel qu'il est ».
        let runtime = compacting_runtime(session, false);
        let reopened = match &runtime.session().messages[1].blocks[0] {
            ContentBlock::ToolResult { output, .. } => output.clone(),
            _ => String::new(),
        };
        assert!(!reopened.contains("x = 1"), "{reopened}");
        assert!(reopened.contains(crate::harness::FILE_VIEW_STALE));
    }

    #[test]
    fn rewriting_a_test_after_a_failed_check_is_flagged_once() {
        let _serial = crate::test_serial();
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

    /// Runtime du mode projet, ce que le modèle a reçu, commandes lancées.
    type ProjectRun = (
        ConversationRuntime<StepsApiClient, StaticToolExecutor>,
        std::rc::Rc<std::cell::RefCell<Observed>>,
        std::rc::Rc<std::cell::RefCell<Vec<String>>>,
    );

    /// Runtime du mode projet sur `root` : `write_file` écrit vraiment le
    /// fichier sous `root` ; `bash` note chaque commande et échoue, tant qu'il
    /// reste des échecs à rendre (`failures`), quand elle contient `fails`.
    fn project_runtime(
        root: &std::path::Path,
        steps: Vec<Vec<AssistantEvent>>,
        fails: &'static str,
        failures: usize,
    ) -> ProjectRun {
        project_runtime_with(root, steps, fails, failures, Vec::new())
    }

    /// Comme `project_runtime`, avec les réponses de l'autre modèle consulté.
    fn project_runtime_with(
        root: &std::path::Path,
        steps: Vec<Vec<AssistantEvent>>,
        fails: &'static str,
        failures: usize,
        advisor: Vec<&'static str>,
    ) -> ProjectRun {
        let observed = std::rc::Rc::new(std::cell::RefCell::new(Observed::default()));
        let commands = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let ran = std::rc::Rc::clone(&commands);
        let base = root.to_path_buf();
        let mut left = failures;
        let executor = StaticToolExecutor::new()
            .register("write_file", move |input| {
                let value: serde_json::Value = serde_json::from_str(input).expect("json");
                let path = base.join(value["path"].as_str().expect("chemin"));
                if let Some(folder) = path.parent() {
                    std::fs::create_dir_all(folder).expect("dossier");
                }
                std::fs::write(&path, value["content"].as_str().expect("contenu")).expect("écrit");
                Ok(serde_json::json!({ "filePath": path.to_string_lossy() }).to_string())
            })
            .register("bash", move |input| {
                let command = super::json_field(input, "command").unwrap_or_default();
                let failing = !fails.is_empty() && command.contains(fails) && left > 0;
                if failing {
                    left -= 1;
                }
                ran.borrow_mut().push(command);
                Ok(if failing { FAILING } else { PASSING }.to_string())
            })
            .register("AskUserQuestion", |_| {
                Ok("L'utilisateur a répondu : Valider".to_string())
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
        ]))
        .with_project_root(root)
        .with_environment_probe(false);
        (runtime, observed, commands)
    }

    fn project_folder(name: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!(
            "claw-mode-projet-{name}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |elapsed| elapsed.as_nanos())
        ));
        std::fs::create_dir_all(&path).expect("dossier");
        path
    }

    fn write_step(path: &str, content: &str) -> Vec<AssistantEvent> {
        tool_step(
            "write_file",
            &serde_json::json!({ "path": path, "content": content }).to_string(),
        )
    }

    const PROJECT_SPEC: &str = "# Bibliothèque\n\n- R1: Ajouter un livre. Check: `python biblio.py ajouter Dune` -> il apparaît dans la liste\n- R2: Emprunter un livre. Check: `python biblio.py emprunter Dune` -> il est marqué prêté\n- E1: titre vide -> refusé avec un message\n\n## Questions\nAucune.\n\n## Hors périmètre\nComptes utilisateurs.\n";

    const PROJECT_TESTS: &str = "def test_r1_ajout():\n    assert True\n\n# E1 : titre vide refusé\ndef test_titre_vide():\n    assert True\n";

    const PROJECT_REQUEST: &str =
        "Crée une application de gestion de bibliothèque en Python : ajouter des livres et les emprunter.";

    fn project_plan(first: &str, second: &str) -> String {
        format!(
            "# Plan\n\n## M1 - Squelette\nRequirements: R1, E1\nVerify: `python -m pytest -q`\nStatus: {first}\n\n## M2 - Emprunts\nRequirements: R2\nVerify: `python -m pytest -q tests/test_m2.py`\nStatus: {second}\n\n## Demo\nInstall: `python -m pip install -r requirements.txt`\nRun: `python biblio.py liste`\n"
        )
    }

    fn active_project(root: &std::path::Path) {
        crate::project::ProjectState {
            active: true,
            ..crate::project::ProjectState::default()
        }
        .save(root)
        .expect("état");
    }

    #[test]
    fn project_mode_takes_a_new_project_through_its_milestones_when_nobody_can_answer() {
        let _serial = crate::test_serial();
        CAN_ASK.with(|flag| flag.set(false));
        let root = project_folder("seul");
        let (mut runtime, observed, commands) = project_runtime(
            &root,
            vec![
                write_step("SPEC.md", PROJECT_SPEC),
                write_step("PLAN.md", &project_plan("todo", "todo")),
                write_step("tests/test_biblio.py", PROJECT_TESTS),
                write_step("PLAN.md", &project_plan("done", "todo")),
                text_step("Fini."),
                write_step("PLAN.md", &project_plan("done", "done")),
                text_step("Fini."),
                write_step(
                    "tests/test_m2.py",
                    "def test_r2_emprunt():\n    assert True\n",
                ),
                text_step("Fini."),
                text_step("Fini."),
                text_step("Fini."),
                text_step("Fini."),
            ],
            "",
            0,
        );
        runtime.run_turn(PROJECT_REQUEST, None).expect("tour");
        let observed = observed.borrow();
        // Toute la méthode, pour un modèle que personne ne peut renseigner.
        assert!(observed.requests[0].contains("Project mode is on for this folder"));
        assert!(observed.requests[0].contains("Nobody can answer you during this turn"));
        let state = std::fs::read_to_string(root.join(".claw").join("project.json"))
            .expect("état enregistré");
        assert!(state.contains("\"active\": true"));
        // Le jalon marqué fait est vérifié aussitôt, par sa propre commande.
        assert!(
            observed.requests[4].contains("M1: its Verify command passes (`python -m pytest -q`).")
        );
        assert!(commands
            .borrow()
            .iter()
            .any(|command| command == "python -m pytest -q"));
        assert!(commands
            .borrow()
            .iter()
            .any(|command| command == "python -m pytest -q tests/test_m2.py"));
        // Seul, le modèle qui s'arrête avant la fin du plan repart au jalon suivant.
        assert!(observed.requests[5].contains("Go on with M2 of 2: Emprunts."));
        // Une exigence d'un jalon fait qu'aucun test ne nomme : dite, puis redemandée.
        assert!(observed.requests[6].contains("no test file of the project names R2"));
        assert!(
            observed.requests[7].contains("Project mode: no test file of the project names R2 yet")
        );
        // Le contrôle des exigences cite le cahier des charges.
        assert!(observed.requests.iter().any(|request| request
            .contains("This project has a specification, SPEC.md.")
            && request.contains("R2: Emprunter un livre.")));
        assert!(observed
            .notices
            .iter()
            .any(|notice| notice.starts_with("Mode projet : Claw guide la création pas à pas")));
        assert!(observed
            .notices
            .iter()
            .any(|notice| notice
                .starts_with("Jalon M1 vérifié : sa commande de vérification réussit (")));
        assert!(observed
            .notices
            .iter()
            .any(|notice| notice
                == "Mode projet : jalons restants : Claw renvoie l'agent au jalon M2"));
        // Le coût de chaque jalon terminé est gardé pour le budget des suivants.
        let state = crate::project::ProjectState::load(&root);
        assert_eq!(state.milestone_requests.len(), 2);
        assert!(state
            .milestone_requests
            .values()
            .all(|requests| *requests > 0));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn project_mode_has_the_user_confirm_the_specification_and_try_each_milestone() {
        let _serial = crate::test_serial();
        CAN_ASK.with(|flag| flag.set(true));
        let root = project_folder("avec-utilisateur");
        // Cahier des charges écrit : à faire valider avant le plan.
        let (mut runtime, observed, _) = project_runtime(
            &root,
            vec![
                write_step("SPEC.md", PROJECT_SPEC),
                text_step("Voici le cahier des charges."),
                tool_step(
                    "AskUserQuestion",
                    r#"{"questions":[{"question":"Ce cahier des charges vous convient-il ?","options":[{"label":"Valider"},{"label":"Modifier"}]}]}"#,
                ),
                text_step("Je passe au plan."),
                text_step("Je passe au plan."),
                text_step("Je passe au plan."),
            ],
            "",
            0,
        );
        runtime.run_turn(PROJECT_REQUEST, None).expect("tour");
        {
            let observed = observed.borrow();
            assert!(observed.requests[0].contains("The user can answer you: after SPEC.md"));
            assert!(observed.requests[2].contains(
                "Before PLAN.md, show the user its requirements in a few lines and ask them with AskUserQuestion"
            ));
        }
        // Jalon terminé : l'utilisateur est invité à l'essayer avant le suivant.
        std::fs::write(root.join("PLAN.md"), project_plan("todo", "todo")).expect("plan");
        let (mut runtime, observed, _) = project_runtime(
            &root,
            vec![
                write_step("tests/test_biblio.py", PROJECT_TESTS),
                write_step("PLAN.md", &project_plan("done", "todo")),
                text_step("M1 est prêt."),
                text_step("Tout est couvert."),
                tool_step(
                    "AskUserQuestion",
                    r#"{"questions":[{"question":"M1 vous convient-il ?","options":[{"label":"Oui"},{"label":"Non"}]}]}"#,
                ),
                text_step("Merci : M2 au prochain tour."),
                text_step("Merci : M2 au prochain tour."),
            ],
            "",
            0,
        );
        runtime.run_turn("continue", None).expect("tour");
        let observed = observed.borrow();
        assert!(observed.requests[0].contains("Milestones: M1 (current), M2."));
        assert!(observed.requests[0].contains(
            "Then stop: tell the user how to try it and ask whether it does what they want."
        ));
        assert!(observed.requests.iter().any(|request| request.contains(
            "M1 is done. End your answer with how the user can try it (the exact commands, from the project folder), then ask them with AskUserQuestion whether it does what they want, before you start M2."
        )));
        assert!(observed.notices.iter().any(|notice| notice
            == "Mode projet : jalon terminé : Claw demande à l'agent de le faire essayer à l'utilisateur"));
        CAN_ASK.with(|flag| flag.set(false));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_milestone_marked_done_whose_check_fails_goes_back_to_the_agent() {
        let _serial = crate::test_serial();
        CAN_ASK.with(|flag| flag.set(false));
        let root = project_folder("echec");
        std::fs::write(root.join("SPEC.md"), PROJECT_SPEC).expect("spec");
        std::fs::write(root.join("PLAN.md"), project_plan("todo", "todo")).expect("plan");
        std::fs::create_dir_all(root.join("tests")).expect("tests");
        std::fs::write(root.join("tests").join("test_biblio.py"), PROJECT_TESTS).expect("tests");
        active_project(&root);
        let (mut runtime, observed, commands) = project_runtime(
            &root,
            vec![
                write_step("PLAN.md", &project_plan("done", "todo")),
                text_step("M1 est fait."),
                write_step("biblio.py", "LIVRES = []\n"),
                text_step("Corrigé."),
                text_step("Fini."),
                text_step("Fini."),
                text_step("Fini."),
            ],
            "python -m pytest -q",
            1,
        );
        runtime.run_turn("continue", None).expect("tour");
        let observed = observed.borrow();
        assert!(observed.requests[1]
            .contains("M1 is marked done in PLAN.md, but its Verify command fails"));
        // Rien n'a changé depuis l'échec : rappelé quand l'agent dit avoir fini.
        assert!(observed.requests[2].contains("M1 is still marked done in PLAN.md"));
        // Après une correction, la vérification est refaite et passe.
        assert!(observed.requests[3].contains("M1: its Verify command passes"));
        assert_eq!(
            commands
                .borrow()
                .iter()
                .filter(|command| *command == "python -m pytest -q")
                .count(),
            2
        );
        assert!(observed.notices.iter().any(|notice| notice
            == "Le jalon M1 est marqué fait mais sa vérification échoue : renvoyé à l'agent"));
        assert!(observed
            .notices
            .iter()
            .any(|notice| notice
                .starts_with("Jalon M1 vérifié : sa commande de vérification réussit (")));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_milestone_check_that_keeps_failing_is_run_three_times_at_most_in_a_turn() {
        let _serial = crate::test_serial();
        // Session 9 de l'appli web : un outil absent de la machine, M1 renvoyé 45 fois.
        CAN_ASK.with(|flag| flag.set(false));
        let root = project_folder("echec-repete");
        std::fs::write(root.join("SPEC.md"), PROJECT_SPEC).expect("spec");
        std::fs::write(root.join("PLAN.md"), project_plan("todo", "todo")).expect("plan");
        std::fs::create_dir_all(root.join("tests")).expect("tests");
        std::fs::write(root.join("tests").join("test_biblio.py"), PROJECT_TESTS).expect("tests");
        active_project(&root);
        let mut steps = vec![
            write_step("PLAN.md", &project_plan("done", "todo")),
            write_step("biblio.py", "LIVRES = [1]\n"),
            write_step("biblio.py", "LIVRES = [2]\n"),
            write_step("biblio.py", "LIVRES = [3]\n"),
        ];
        steps.extend((0..8).map(|_| text_step("Fini.")));
        let (mut runtime, observed, commands) =
            project_runtime(&root, steps, "python -m pytest -q", 100);
        runtime.run_turn("continue", None).expect("tour");
        let observed = observed.borrow();
        assert_eq!(
            commands
                .borrow()
                .iter()
                .filter(|command| *command == "python -m pytest -q")
                .count(),
            3
        );
        assert!(observed.requests[3]
            .contains("The Verify command of M1 has now failed 3 times in this turn"));
        // Quatrième modification : plus de nouvel échec renvoyé (chaque requête porte
        // l'historique, d'où le compte).
        let failed = |request: &String| request.matches("but its Verify command fails").count();
        assert_eq!(failed(&observed.requests[4]), failed(&observed.requests[3]));
        assert!(observed.notices.iter().any(|notice| notice.starts_with(
            "Le jalon M1 échoue encore à sa vérification (3 essais) : Claw ne la relance plus"
        )));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn project_mode_starts_for_a_new_project_or_when_asked_and_stops_when_told() {
        let _serial = crate::test_serial();
        CAN_ASK.with(|flag| flag.set(false));
        let root = project_folder("activation");
        std::fs::write(root.join("app.py"), "print('bonjour')\n").expect("code");
        // Un dossier qui a déjà du code : une demande de création ne suffit pas.
        let (mut runtime, observed, _) = project_runtime(&root, vec![text_step("Fait.")], "", 0);
        runtime.run_turn(PROJECT_REQUEST, None).expect("tour");
        assert!(!observed.borrow().requests[0].contains("Project mode"));
        assert!(!root.join(".claw").join("project.json").exists());
        // « mode projet » l'ouvre pour ce dossier.
        let (mut runtime, observed, _) =
            project_runtime(&root, vec![text_step("Fait."), text_step("Fait.")], "", 0);
        runtime
            .run_turn("Passe en mode projet pour ajouter les emprunts", None)
            .expect("tour");
        assert!(observed.borrow().requests[0].contains("Project mode is on for this folder"));
        assert!(crate::project::ProjectState::load(&root).active);
        // Le tour suivant, d'une autre session aussi, le mode reste ouvert.
        let (mut runtime, observed, _) =
            project_runtime(&root, vec![text_step("Fait."), text_step("Fait.")], "", 0);
        runtime
            .run_turn("Ajoute aussi les retours de livres", None)
            .expect("tour");
        assert!(observed.borrow().requests[0]
            .contains("[Claw guide] Project mode: SPEC.md, PLAN.md and DECISIONS.md"));
        // « sans mode projet » l'arrête.
        let (mut runtime, observed, _) = project_runtime(&root, vec![text_step("Fait.")], "", 0);
        runtime
            .run_turn("Continue sans mode projet", None)
            .expect("tour");
        assert!(!observed.borrow().requests[0].contains("Project mode"));
        assert!(!crate::project::ProjectState::load(&root).active);
        assert!(observed
            .borrow()
            .notices
            .iter()
            .any(|notice| notice == "Mode projet arrêté pour ce dossier"));
        // Option « Mode projet » de l'appli web (`CLAW_PROJECT_MODE=start`) :
        // une question reçoit sa réponse sans le guide du projet, une demande
        // de travail l'ouvre d'office, même dans un dossier qui a du code.
        std::env::set_var("CLAW_PROJECT_MODE", "start");
        let (mut runtime, observed, _) = project_runtime(
            &root,
            vec![text_step("Voici comment."), text_step("Voici comment.")],
            "",
            0,
        );
        let asked = runtime.run_turn("Comment marche le prêt d'un livre ?", None);
        let question_request = observed.borrow().requests[0].clone();
        let (mut runtime, observed, _) =
            project_runtime(&root, vec![text_step("Fait."), text_step("Fait.")], "", 0);
        let ordered = runtime.run_turn("Ajoute les amendes de retard", None);
        std::env::remove_var("CLAW_PROJECT_MODE");
        asked.expect("tour");
        ordered.expect("tour");
        assert!(!question_request.contains("Project mode"));
        assert!(observed.borrow().requests[0].contains("Project mode is on for this folder"));
        assert!(crate::project::ProjectState::load(&root).active);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn another_model_reviews_the_specification_and_the_plan_before_the_code() {
        let _serial = crate::test_serial();
        CAN_ASK.with(|flag| flag.set(false));
        let root = project_folder("architecture");
        std::fs::write(root.join("SPEC.md"), PROJECT_SPEC).expect("spec");
        active_project(&root);
        let (mut runtime, observed, _) = project_runtime_with(
            &root,
            vec![
                write_step("PLAN.md", &project_plan("todo", "todo")),
                text_step("Plan écrit."),
                text_step("Plan écrit."),
                text_step("Plan écrit."),
                text_step("Plan écrit."),
                text_step("Plan écrit."),
            ],
            "",
            0,
            vec!["Analyse.\nVERDICT\nSPEC.md - emprunter un livre déjà prêté n'a pas de cas limite - ajouter E2 et son test"],
        );
        runtime.run_turn("écris le plan", None).expect("tour");
        {
            let observed = observed.borrow();
            // Les requêtes notées sont la forme de débogage des messages : les
            // fins de ligne y sont écrites `\n`.
            assert!(observed.requests[1].contains(
                "Before the code, another model (relecteur) reviewed SPEC.md and PLAN.md and raised these points:\\nSPEC.md - emprunter un livre déjà prêté n'a pas de cas limite - ajouter E2 et son test"
            ));
            let designs: Vec<&String> = observed
                .consults
                .iter()
                .filter(|(purpose, _)| *purpose == super::ConsultPurpose::Design)
                .map(|(_, prompt)| prompt)
                .collect();
            assert_eq!(designs.len(), 1);
            assert!(designs[0].starts_with("The user asked:\nécris le plan"));
            assert!(observed.notices.iter().any(|notice| notice
                == "Relecture du plan par relecteur : 1 remarque(s) transmise(s) à l'agent"));
        }
        assert!(crate::project::ProjectState::load(&root)
            .design_reviewed
            .is_some());
        // Mêmes exigences, mêmes jalons : pas de seconde relecture.
        let (mut runtime, observed, _) = project_runtime_with(
            &root,
            vec![
                write_step("PLAN.md", &project_plan("todo", "todo")),
                text_step("Fait."),
                text_step("Fait."),
                text_step("Fait."),
                text_step("Fait."),
            ],
            "",
            0,
            vec!["VERDICT\nLGTM"],
        );
        runtime.run_turn("continue", None).expect("tour");
        assert!(!observed
            .borrow()
            .consults
            .iter()
            .any(|(purpose, _)| *purpose == super::ConsultPurpose::Design));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_final_demo_runs_in_an_empty_copy_and_a_failure_goes_back_to_the_agent() {
        let _serial = crate::test_serial();
        CAN_ASK.with(|flag| flag.set(false));
        let root = project_folder("demo");
        std::fs::write(root.join("SPEC.md"), PROJECT_SPEC).expect("spec");
        std::fs::write(root.join("PLAN.md"), project_plan("done", "todo")).expect("plan");
        std::fs::write(root.join("DECISIONS.md"), "- Python 3 : déjà installé\n")
            .expect("décisions");
        std::fs::create_dir_all(root.join("tests")).expect("tests");
        std::fs::write(root.join("tests").join("test_biblio.py"), PROJECT_TESTS).expect("tests");
        active_project(&root);
        let (mut runtime, observed, commands) = project_runtime(
            &root,
            vec![
                write_step(
                    "tests/test_m2.py",
                    "def test_r2_emprunt():\n    assert True\n",
                ),
                write_step("PLAN.md", &project_plan("done", "done")),
                text_step("Fini."),
                write_step("biblio.py", "LIVRES = []\n"),
                text_step("Corrigé."),
                write_step(
                    "README.md",
                    "# Bibliothèque\n\n    python biblio.py liste\n",
                ),
                text_step("README écrit."),
                text_step("Fini."),
                text_step("Fini."),
                text_step("Fini."),
            ],
            "biblio.py liste",
            1,
        );
        runtime.run_turn("continue", None).expect("tour");
        let observed = observed.borrow();
        // Lancée dans une copie vide du projet, installation puis lancement.
        let demo: Vec<String> = commands
            .borrow()
            .iter()
            .filter(|command| command.starts_with("cd \"") && command.contains("claw-demo-"))
            .cloned()
            .collect();
        assert_eq!(demo.len(), 2, "{demo:?}");
        assert!(demo[0].ends_with(
            "&& { python -m pip install -r requirements.txt; } && { python biblio.py liste; }"
        ));
        assert!(observed.requests.iter().any(|request| request.contains(
            "Final demo: Claw copied the project to an empty folder (5 files: those Git tracks or would track)"
        ) && request.contains(
            "This command failed:\\n$ python -m pip install -r requirements.txt && python biblio.py liste"
        )));
        // Réussie après la correction : notée, puis le README est demandé.
        assert!(observed.requests.iter().any(|request| request
            .contains("Final demo passed. README.md does not show how to run the project")));
        assert!(observed
            .notices
            .iter()
            .any(|notice| notice == "Démonstration finale réussie dans une copie vide du projet"));
        let state = crate::project::ProjectState::load(&root);
        assert!(state.demo_passed.is_some());
        assert_eq!(
            crate::project::Project::read(&root).phase(),
            crate::project::Phase::Done
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_short_free_quota_is_told_to_the_model_and_the_user_at_the_start_of_the_turn() {
        let _serial = crate::test_serial();
        CAN_ASK.with(|flag| flag.set(false));
        FREE_LEFT.with(|left| left.set(Some(40)));
        let root = project_folder("budget");
        std::fs::write(root.join("SPEC.md"), PROJECT_SPEC).expect("spec");
        std::fs::write(root.join("PLAN.md"), project_plan("todo", "todo")).expect("plan");
        active_project(&root);
        let (mut runtime, observed, _) = project_runtime(
            &root,
            // Contrôles de fin de tour (décisions, jalon suivant) : de quoi répondre.
            vec![
                text_step("Je commence."),
                text_step("Je commence."),
                text_step("Je commence."),
                text_step("Je commence."),
            ],
            "",
            0,
        );
        runtime.run_turn("continue", None).expect("tour");
        FREE_LEFT.with(|left| left.set(None));
        let observed = observed.borrow();
        assert!(observed.requests[0].contains(
            "Budget: 40 free requests are left today and a milestone of this project takes about 30: there is room for about 1 of the 2 milestones left."
        ));
        assert!(observed.notices.iter().any(|notice| notice
            == "Mode projet : il reste 40 requêtes gratuites aujourd'hui, de quoi faire environ 1 des 2 jalons restants"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn project_mode_resumes_in_a_fresh_clone_of_a_project_with_its_plan() {
        let _serial = crate::test_serial();
        CAN_ASK.with(|flag| flag.set(false));
        // Clone neuf du moteur de l'appli web : SPEC.md et PLAN.md, pas de .claw.
        let root = project_folder("reprise");
        std::fs::write(root.join("SPEC.md"), PROJECT_SPEC).expect("spec");
        std::fs::write(root.join("PLAN.md"), project_plan("done", "todo")).expect("plan");
        std::fs::write(root.join("app.py"), "print(1)\n").expect("code");
        let (mut runtime, observed, _) = project_runtime(
            &root,
            vec![
                text_step("Je reprends."),
                text_step("Je reprends."),
                text_step("Je reprends."),
            ],
            "",
            0,
        );
        runtime.run_turn("continue", None).expect("tour");
        {
            let observed = observed.borrow();
            assert!(observed.requests[0].contains("Milestones: M1 done, M2 (current)."));
            assert!(observed
                .notices
                .iter()
                .any(|notice| notice.starts_with("Mode projet repris")));
        }
        let state = crate::project::ProjectState::load(&root);
        assert!(state.active);
        assert_eq!(
            state.design_reviewed,
            Some(crate::project::Project::read(&root).design_fingerprint())
        );
        // Un arrêt enregistré reste un arrêt.
        crate::project::ProjectState::default()
            .save(&root)
            .expect("état");
        let (mut runtime, observed, _) = project_runtime(&root, vec![text_step("Fait.")], "", 0);
        runtime.run_turn("continue", None).expect("tour");
        assert!(!observed.borrow().requests[0].contains("Project mode"));
        // Un cahier des charges sans plan ne suffit pas à reprendre.
        let other = project_folder("reprise-sans-plan");
        std::fs::write(other.join("SPEC.md"), PROJECT_SPEC).expect("spec");
        std::fs::write(other.join("app.py"), "print(1)\n").expect("code");
        let (mut runtime, observed, _) = project_runtime(&other, vec![text_step("Fait.")], "", 0);
        runtime.run_turn("continue", None).expect("tour");
        assert!(!observed.borrow().requests[0].contains("Project mode"));
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&other);
    }

    #[test]
    fn the_notes_of_the_web_engine_do_not_start_project_mode_in_an_empty_repository() {
        let _serial = crate::test_serial();
        CAN_ASK.with(|flag| flag.set(false));
        // Dépôt neuf de l'appli web : README et .gitignore seulement.
        let root = project_folder("note-moteur");
        std::fs::write(root.join("README.md"), "# todo-app\n").expect("readme");
        std::fs::write(root.join(".gitignore"), "*.pyc\n").expect("gitignore");
        let (mut runtime, observed, _) = project_runtime(&root, vec![text_step("Fait.")], "", 0);
        runtime
            .run_turn(
                "Ajoute une licence MIT\n\n[Claw] You are working in the repository abasse-ali/todo-app, on the branch claw/session-3 (created from main). Nobody can answer questions during this task: make reasonable choices and state them in your final answer.",
                None,
            )
            .expect("tour");
        assert!(!observed.borrow().requests[0].contains("Project mode"));
        assert!(!crate::project::ProjectState::exists(&root));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_plan_claw_cannot_follow_is_pointed_out_once_after_it_is_written() {
        let _serial = crate::test_serial();
        CAN_ASK.with(|flag| flag.set(false));
        let root = project_folder("forme");
        std::fs::write(root.join("SPEC.md"), PROJECT_SPEC).expect("spec");
        active_project(&root);
        let (mut runtime, observed, _) = project_runtime(
            &root,
            vec![
                write_step(
                    "PLAN.md",
                    "# Plan\n\n## M1 - Squelette\nRequirements: R1, E1\nStatus: todo\n",
                ),
                text_step("Plan écrit."),
                text_step("Plan écrit."),
                text_step("Plan écrit."),
                text_step("Plan écrit."),
            ],
            "",
            0,
        );
        runtime.run_turn("écris le plan", None).expect("tour");
        let observed = observed.borrow();
        assert!(observed.requests[1].contains("[Claw] PLAN.md: M1 has no `Verify: <command>` line"));
        assert!(observed.requests[1].contains("R2 of SPEC.md is in no milestone"));
        assert!(observed.requests[1].contains("it has no `## Demo` section"));
        // La même remarque n'est pas refaite en fin de tour (chaque requête
        // reprend toute la conversation).
        assert_eq!(
            observed.requests[2].matches("PLAN.md: M1 has no").count(),
            1
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}
