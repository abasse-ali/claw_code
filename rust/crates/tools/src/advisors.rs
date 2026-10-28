//! Avis d'autres modèles que celui qui travaille : relecture des
//! modifications, diagnostic quand l'agent n'avance plus, second avis sur une
//! décision difficile. Les modèles consultés répondent sans outils : ils ne
//! voient que la question et le contexte qu'on leur donne.

use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use api::{
    ApiError, ContentBlockDelta, InputMessage, MessageRequest, OutputContentBlock, ProviderClient,
    StreamEvent,
};

/// Réponse d'un modèle consulté.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Opinion {
    pub model: String,
    pub text: String,
}

/// Durée totale d'une consultation, tous modèles confondus.
const TIMEOUT: Duration = Duration::from_secs(90);
/// Délai accordé à un modèle pour donner un premier signe de vie ; au-delà,
/// le suivant est consulté.
const FIRST_SIGN: Duration = Duration::from_secs(20);
/// Temps accordé à un modèle pour finir sa réponse.
const PER_MODEL: Duration = Duration::from_secs(50);

/// Temps laissé à une consultation : à chaque modèle pour finir sa réponse,
/// et en tout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Patience {
    pub per_model: Duration,
    pub total: Duration,
}

impl Patience {
    /// Un conseil attendu pendant le travail : l'agent ne patiente pas longtemps.
    pub const SHORT: Self = Self {
        per_model: PER_MODEL,
        total: TIMEOUT,
    };
    /// La relecture de fin de tour : un modèle gratuit qui raisonne avant
    /// d'écrire dépasse souvent 50 s sur un vrai diff. Dans les essais réels,
    /// les trois relecteurs échouaient ainsi (réponse inachevée, ou sans
    /// texte) et le travail partait sans avoir été relu.
    pub const REVIEW: Self = Self {
        per_model: Duration::from_secs(240),
        total: Duration::from_secs(360),
    };
    /// La conception d'une interface : un plan et une feuille de base écrits
    /// d'un trait, par un modèle souvent lent (GLM-5.3 ne répondait pas en 45 s
    /// à l'essai du catalogue, le 28 octobre 2026), une fois par création.
    pub const DESIGN: Self = Self {
        per_model: Duration::from_mins(7),
        total: Duration::from_mins(12),
    };
}

/// Jetons de sortie de la conception d'une interface, raisonnement compris :
/// un plan d'environ 700 mots et une feuille de base d'environ 260 lignes.
pub const DESIGN_MAX_TOKENS: u32 = 32_000;

/// Jetons de sortie d'une relecture, raisonnement compris. Sur le diff d'une
/// tâche de cinq fichiers, deux relecteurs gratuits ont raisonné 27 000 et
/// 33 000 caractères (environ 8 000 jetons) sans avoir fini : ces modèles ne
/// connaissent pas de niveaux d'effort, seule la place de sortie les borne.
pub const REVIEW_MAX_TOKENS: u32 = 20_000;

/// Plus haut niveau de raisonnement demandé à un modèle consulté. Claw demande
/// d'ordinaire le plus haut qu'il accepte ; dans le petit budget de sortie
/// d'une consultation, ce raisonnement prenait toute la place, et la relecture
/// d'un vrai diff revenait vide chez les trois relecteurs (essais réels t4).
/// Les consignes de relecture font écrire le raisonnement dans la réponse.
const CONSULT_EFFORT: api::ReasoningEffort = api::ReasoningEffort::Medium;
/// Longueur gardée de la cause d'un échec, dans le message final.
const REASON_CHARS: usize = 90;
/// Mise de côté d'un modèle muet ou en panne.
const SET_ASIDE: Duration = Duration::from_secs(300);
/// Mise de côté d'un modèle qui refuse (403).
const REFUSED_SET_ASIDE: Duration = Duration::from_secs(7 * 24 * 3_600);

type Resolver = Box<dyn Fn() -> Vec<String> + Send + Sync>;

fn resolver() -> &'static Mutex<Option<Resolver>> {
    static RESOLVER: OnceLock<Mutex<Option<Resolver>>> = OnceLock::new();
    RESOLVER.get_or_init(|| Mutex::new(None))
}

/// Indique comment trouver les modèles à consulter (réglage `advisorModels`,
/// sinon les meilleurs modèles gratuits autres que celui en cours).
pub fn set_resolver(resolve: impl Fn() -> Vec<String> + Send + Sync + 'static) {
    *resolver()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(Box::new(resolve));
}

/// Modèles à consulter, du préféré au moins préféré (vide : aucun).
#[must_use]
pub fn configured_models() -> Vec<String> {
    resolver()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .as_ref()
        .map(|resolve| resolve())
        .unwrap_or_default()
}

fn request(model: &str, system: &str, prompt: &str, max_tokens: u32) -> MessageRequest {
    MessageRequest {
        model: model.to_string(),
        max_tokens: api::max_tokens_for_model(model).min(max_tokens),
        messages: vec![InputMessage::user_text(prompt.to_string())],
        system: Some(system.to_string()),
        tools: None,
        tool_choice: None,
        stream: false,
    }
}

/// Cause d'une erreur en quelques mots : « erreur 503 (Service overloaded) ».
fn reason(error: &ApiError) -> String {
    match error {
        ApiError::Api {
            status,
            message: Some(message),
            ..
        } => format!("erreur {} ({message})", status.as_u16()),
        ApiError::Api { status, .. } => format!("erreur {}", status.as_u16()),
        ApiError::RetriesExhausted { last_error, .. } => reason(last_error),
        other => other.to_string(),
    }
}

/// Refus 403 : le modèle est réservé à certaines applications (« only
/// available on agentic harnesses ») ou interdit à cette clé ; il le restera.
fn refused(error: &ApiError) -> bool {
    match error {
        ApiError::Api { status, .. } => status.as_u16() == 403,
        ApiError::RetriesExhausted { last_error, .. } => refused(last_error),
        _ => false,
    }
}

/// Erreur d'un modèle consulté ; une panne qui lui est propre le met de côté
/// (une semaine pour un refus, qui ne passera pas).
fn failure(model: &str, error: &ApiError) -> String {
    if refused(error) {
        api::mark_model_unavailable(model, REFUSED_SET_ASIDE);
    } else if error.is_fallback_worthy() {
        api::mark_model_unavailable(model, SET_ASIDE);
    }
    reason(error)
}

/// Réponse d'un modèle consulté : son texte et, pour dire pourquoi une
/// réponse est vide, ce qui est venu à la place.
struct Reply {
    text: String,
    /// Caractères de raisonnement reçus.
    thought: usize,
    /// Raison de l'arrêt donnée par le fournisseur.
    stop: Option<String>,
}

/// Pourquoi une réponse est arrivée sans texte, en quelques mots ; `again` :
/// c'était déjà la seconde demande à ce modèle.
fn empty_reply_reason(reply: &Reply, again: bool) -> String {
    let stop = reply.stop.as_deref().unwrap_or("inconnu");
    let again = if again { ", redemandé" } else { "" };
    if reply.thought > 0 {
        format!(
            "réponse sans texte après {} caractères de raisonnement (arrêt : {stop}{again})",
            reply.thought
        )
    } else {
        format!("réponse vide (arrêt : {stop}{again})")
    }
}

/// Réponse de `model`, lue en flux jusqu'à `deadline`.
async fn answer(model: &str, request: &MessageRequest, deadline: Instant) -> Result<Reply, String> {
    let started = Instant::now();
    let client = ProviderClient::from_model(model).map_err(|error| failure(model, &error))?;
    let opening = async {
        let mut stream = client.stream_message(request).await?;
        let first = stream.next_event().await?;
        Ok::<_, ApiError>((stream, first))
    };
    let patience = FIRST_SIGN.min(deadline.saturating_duration_since(Instant::now()));
    let Ok(opened) = tokio::time::timeout(patience, opening).await else {
        api::mark_model_unavailable(model, SET_ASIDE);
        return Err(format!("pas de réponse après {} s", patience.as_secs()));
    };
    let (mut stream, mut event) = opened.map_err(|error| failure(model, &error))?;
    crate::note_model_request(model);

    let mut text = String::new();
    let mut thought = 0;
    let mut stop = None;
    let reading = async {
        while let Some(current) = event {
            match current {
                StreamEvent::ContentBlockStart(start) => {
                    if let OutputContentBlock::Text { text: piece } = start.content_block {
                        text.push_str(&piece);
                    }
                }
                StreamEvent::ContentBlockDelta(delta) => match delta.delta {
                    ContentBlockDelta::TextDelta { text: piece } => text.push_str(&piece),
                    ContentBlockDelta::ThinkingDelta { thinking } => {
                        thought += thinking.chars().count();
                    }
                    _ => {}
                },
                StreamEvent::MessageDelta(delta) => stop = delta.delta.stop_reason,
                StreamEvent::MessageStop(_) => break,
                _ => {}
            }
            event = stream.next_event().await?;
        }
        Ok::<_, ApiError>(())
    };
    let left = deadline.saturating_duration_since(Instant::now());
    match tokio::time::timeout(left, reading).await {
        Ok(Ok(())) => Ok(Reply {
            text: runtime::typography::cleaned(text.trim()).into_owned(),
            thought,
            stop,
        }),
        Ok(Err(error)) => Err(failure(model, &error)),
        Err(_) => Err(format!(
            "réponse inachevée après {} s",
            started.elapsed().as_secs()
        )),
    }
}

/// Le premier modèle de `models` qui répond donne son avis : ceux qui sont
/// disponibles d'abord ; un modèle muet, en panne ou sans texte laisse la
/// place au suivant. Lancé dans son propre fil : utilisable depuis n'importe où.
pub fn ask(
    models: &[String],
    system: &str,
    prompt: &str,
    max_tokens: u32,
) -> Result<Opinion, String> {
    ask_within(models, system, prompt, max_tokens, Patience::SHORT)
}

/// Comme [`ask`], avec le temps que cette consultation peut prendre.
pub fn ask_within(
    models: &[String],
    system: &str,
    prompt: &str,
    max_tokens: u32,
    patience: Patience,
) -> Result<Opinion, String> {
    ask_with(models, patience, |model| {
        request(model, system, prompt, max_tokens)
    })
}

/// La même consultation à propos d'une image : le premier modèle de `models`
/// qui répond la décrit. Tous doivent savoir lire les images.
pub fn ask_about_image(
    models: &[String],
    system: &str,
    prompt: &str,
    image: &api::ImageSource,
    max_tokens: u32,
) -> Result<Opinion, String> {
    ask_about_image_within(models, system, prompt, image, max_tokens, Patience::SHORT)
}

/// La même chose, avec la patience donnée : une description détaillée, section
/// par section, prend plus de temps qu'un conseil.
pub fn ask_about_image_within(
    models: &[String],
    system: &str,
    prompt: &str,
    image: &api::ImageSource,
    max_tokens: u32,
    patience: Patience,
) -> Result<Opinion, String> {
    ask_with(models, patience, |model| MessageRequest {
        messages: vec![InputMessage {
            role: "user".to_string(),
            content: vec![
                api::InputContentBlock::Image {
                    source: image.clone(),
                },
                api::InputContentBlock::Text {
                    text: prompt.to_string(),
                },
            ],
        }],
        ..request(model, system, "", max_tokens)
    })
}

/// Comme [`ask_about_image_within`], avec plusieurs images dans la même
/// requête (captures d'une page au téléphone et à l'ordinateur).
pub fn ask_about_images_within(
    models: &[String],
    system: &str,
    prompt: &str,
    images: &[api::ImageSource],
    max_tokens: u32,
    patience: Patience,
) -> Result<Opinion, String> {
    ask_with(models, patience, |model| {
        let mut content: Vec<api::InputContentBlock> = images
            .iter()
            .map(|image| api::InputContentBlock::Image {
                source: image.clone(),
            })
            .collect();
        content.push(api::InputContentBlock::Text {
            text: prompt.to_string(),
        });
        MessageRequest {
            messages: vec![InputMessage {
                role: "user".to_string(),
                content,
            }],
            ..request(model, system, "", max_tokens)
        }
    })
}

/// Consulte les modèles l'un après l'autre avec la requête que `build` leur
/// prépare, jusqu'à la première réponse.
fn ask_with(
    models: &[String],
    patience: Patience,
    build: impl Fn(&str) -> MessageRequest + Send,
) -> Result<Opinion, String> {
    if models.is_empty() {
        return Err("aucun autre modèle à consulter".to_string());
    }
    let (available, set_aside): (Vec<String>, Vec<String>) = models
        .iter()
        .cloned()
        .partition(|model| api::model_available(model));
    std::thread::scope(|scope| {
        scope
            .spawn(move || {
                api::cap_thread_reasoning(Some(CONSULT_EFFORT));
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .map_err(|error| error.to_string())?;
                let deadline = Instant::now() + patience.total;
                // Ce que chaque modèle a répondu, pour le message final.
                let mut failures = Vec::new();
                for model in available.into_iter().chain(set_aside) {
                    let mut asked_again = false;
                    let reason =
                        loop {
                            let now = Instant::now();
                            if now >= deadline {
                                break None;
                            }
                            let request = build(&model);
                            let answered = runtime.block_on(runtime::interrupt::interruptible(
                                answer(&model, &request, deadline.min(now + patience.per_model)),
                            ));
                            match answered {
                                Ok(reply) if !reply.text.is_empty() => {
                                    return Ok(Opinion {
                                        model,
                                        text: reply.text,
                                    })
                                }
                                Ok(reply) => {
                                    // Tout son budget est parti en raisonnement : le
                                    // flux lui a retiré un cran (`reasoning_exhausted`),
                                    // il est consulté une seconde fois.
                                    let lowered = api::take_reasoning_notice().is_some();
                                    if lowered && !asked_again {
                                        asked_again = true;
                                        continue;
                                    }
                                    break Some(empty_reply_reason(&reply, asked_again));
                                }
                                Err(_) if runtime::interrupt::interrupt_requested() => {
                                    return Err("consultation interrompue".to_string());
                                }
                                Err(error) => break Some(error.to_string()),
                            }
                        };
                    let Some(reason) = reason else {
                        break;
                    };
                    let reason: String = reason.chars().take(REASON_CHARS).collect();
                    failures.push(format!("{model} : {reason}"));
                }
                if failures.is_empty() {
                    failures.push(format!(
                        "pas de réponse après {} s",
                        patience.total.as_secs()
                    ));
                }
                Err(failures.join(" ; "))
            })
            .join()
            .unwrap_or_else(|_| Err("consultation interrompue".to_string()))
    })
}

/// Chaque modèle répond de son côté, en même temps (second avis).
#[must_use]
pub fn ask_each(
    models: &[String],
    system: &str,
    prompt: &str,
    max_tokens: u32,
) -> Vec<Result<Opinion, String>> {
    std::thread::scope(|scope| {
        let handles = models
            .iter()
            .map(|model| {
                scope.spawn(move || {
                    ask(std::slice::from_ref(model), system, prompt, max_tokens)
                        .map_err(|error| format!("{model} : {error}"))
                })
            })
            .collect::<Vec<_>>();
        handles
            .into_iter()
            .map(|handle| {
                handle
                    .join()
                    .unwrap_or_else(|_| Err("consultation interrompue".to_string()))
            })
            .collect()
    })
}

/// Consignes des modèles consultés par l'outil `Consult`.
pub const CONSULT_SYSTEM: &str = "You are an experienced software engineer giving a second opinion to a coding agent. You see only the question and the context below, not the project. Answer the question directly: give your recommendation first, then the reasons and the main risk. If something essential is missing to answer well, say what in one sentence and answer under a stated assumption. Be concrete and concise (at most 250 words).";

#[cfg(test)]
mod tests {
    use super::{ask, configured_models, empty_reply_reason, reason, set_resolver, Reply};

    #[test]
    fn an_empty_reply_says_what_came_instead() {
        let reply = |thought: usize, stop: Option<&str>| Reply {
            text: String::new(),
            thought,
            stop: stop.map(str::to_string),
        };
        assert_eq!(
            empty_reply_reason(&reply(31_200, Some("length")), false),
            "réponse sans texte après 31200 caractères de raisonnement (arrêt : length)"
        );
        // Déjà redemandé, avec un raisonnement plus court : ce n'est pas mieux.
        assert_eq!(
            empty_reply_reason(&reply(9_000, Some("length")), true),
            "réponse sans texte après 9000 caractères de raisonnement (arrêt : length, redemandé)"
        );
        assert_eq!(
            empty_reply_reason(&reply(0, None), false),
            "réponse vide (arrêt : inconnu)"
        );
    }

    #[test]
    fn asking_nobody_is_an_error_and_the_resolver_lists_models() {
        assert!(ask(&[], "s", "q", 100).is_err());
        set_resolver(|| vec!["a/b:free".to_string(), "c/d:free".to_string()]);
        assert_eq!(configured_models(), vec!["a/b:free", "c/d:free"]);
    }

    #[test]
    fn a_failure_is_reported_in_a_few_words() {
        let overloaded = api::ApiError::Api {
            status: reqwest::StatusCode::SERVICE_UNAVAILABLE,
            error_type: None,
            message: Some("Service overloaded".to_string()),
            body: r#"{"error":{"message":"Service overloaded","code":503}}"#.to_string(),
            retryable: true,
        };
        assert_eq!(reason(&overloaded), "erreur 503 (Service overloaded)");
        let exhausted = api::ApiError::RetriesExhausted {
            attempts: 3,
            last_error: Box::new(overloaded),
        };
        assert_eq!(reason(&exhausted), "erreur 503 (Service overloaded)");
    }

    /// Essai en direct (réseau et clé `OpenRouter`) : un vrai modèle diagnostique
    /// une fin de session bloquée. `CLAW_LIVE_ADVISORS` liste les modèles.
    /// `cargo test -p tools -- --ignored live_advisor --nocapture`
    #[test]
    #[ignore = "demande le réseau et une clé OpenRouter"]
    fn live_advisor_diagnoses_a_stuck_session() {
        let models: Vec<String> = std::env::var("CLAW_LIVE_ADVISORS")
            .expect("CLAW_LIVE_ADVISORS")
            .split(',')
            .map(|model| model.trim().to_string())
            .collect();
        let tail = "User request: Dans commande.py, le total doit être arrondi une seule fois, à la fin.\n\
            [tool call] edit_file commande.py: return round(sum(prix_ttc(prix, TVA) * quantite for prix, quantite in lignes), 2)\n\
            [tool call] bash: python -m unittest -q\n\
            [tool output] FAIL: test_total_arrondi\nAssertionError: 12.0 != 11.99\n\
            [tool call] read_file tarifs.py\n\
            [tool output] def prix_ttc(prix_ht, taux_tva):\n    return round(prix_ht * (1 + taux_tva), 2)\n\
            [tool call] bash: python -m unittest -q\n\
            [tool output] FAIL: test_total_arrondi\nAssertionError: 12.0 != 11.99";
        let opinion = ask(
            &models,
            runtime::harness::UNBLOCK_SYSTEM,
            &runtime::harness::unblock_prompt(tail),
            3_000,
        )
        .expect("conseil");
        println!("{} :\n{}", opinion.model, opinion.text);
        assert!(opinion.text.contains("prix_ttc") || opinion.text.contains("tarifs"));
    }
}
