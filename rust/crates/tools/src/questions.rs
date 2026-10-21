//! Outil `AskUserQuestion` : l'agent pose des questions à l'utilisateur avec
//! des réponses proposées, que l'interface présente comme des choix à
//! cliquer (page web, panneau de VS Code) ou un menu (terminal).
//!
//! L'application enregistre de quoi poser les questions ; sans personne pour
//! répondre (commande dans un script, sous-agent), l'outil n'est pas proposé.

use std::sync::{Arc, Mutex, OnceLock};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// Questions par appel, et réponses proposées par question.
const MAX_QUESTIONS: usize = 4;
const MAX_OPTIONS: usize = 6;

/// Réponse proposée.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuestionOption {
    pub label: String,
    /// Ce que ce choix implique (une phrase).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
}

/// Une question et ses réponses proposées. L'interface ajoute toujours la
/// possibilité de taper une autre réponse.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserQuestion {
    pub question: String,
    /// Étiquette courte du sujet (« OCR », « Traduction »).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub header: String,
    pub options: Vec<QuestionOption>,
    /// Plusieurs réponses peuvent être choisies ensemble.
    #[serde(default)]
    pub multi_select: bool,
}

/// Réponse de l'utilisateur à une question.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserAnswer {
    pub question: String,
    pub answer: String,
}

/// Pose les questions dans l'interface en cours et attend les réponses.
/// `Err` : l'utilisateur n'a pas répondu (questions écartées, tour interrompu).
pub type SharedQuestionAsker =
    Arc<dyn Fn(&[UserQuestion]) -> Result<Vec<UserAnswer>, String> + Send + Sync>;

fn asker() -> &'static Mutex<Option<SharedQuestionAsker>> {
    static SLOT: OnceLock<Mutex<Option<SharedQuestionAsker>>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(None))
}

/// Indique comment poser les questions ; `None` quand personne ne peut répondre.
pub fn set_question_asker(next: Option<SharedQuestionAsker>) {
    *asker()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = next;
}

/// Quelqu'un peut répondre : l'outil est proposé au modèle.
#[must_use]
pub fn question_asker_available() -> bool {
    asker()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .is_some()
}

fn text(value: &Value, keys: &[&str]) -> String {
    keys.iter()
        .find_map(|key| value.get(key).and_then(Value::as_str))
        .map(|text| text.trim().to_string())
        .unwrap_or_default()
}

/// Choix « autre » que certains modèles ajoutent : l'interface le propose déjà.
fn is_other(label: &str) -> bool {
    matches!(
        label.trim().to_lowercase().as_str(),
        "other" | "autre" | "autres" | "other (please specify)" | "autre (préciser)"
    )
}

fn parse_option(value: &Value) -> Option<QuestionOption> {
    // Les petits modèles envoient souvent une simple chaîne.
    let (label, description) = match value {
        Value::String(label) => (label.trim().to_string(), String::new()),
        Value::Object(_) => (
            text(value, &["label", "title", "name", "value"]),
            text(value, &["description", "detail", "hint"]),
        ),
        _ => return None,
    };
    (!label.is_empty() && !is_other(&label)).then_some(QuestionOption { label, description })
}

fn parse_question(value: &Value) -> Result<UserQuestion, String> {
    let question = text(value, &["question", "text", "prompt"]);
    if question.is_empty() {
        return Err("each question needs a non-empty `question`".to_string());
    }
    let mut options: Vec<QuestionOption> = Vec::new();
    for option in value
        .get("options")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .filter_map(parse_option)
    {
        if !options.iter().any(|known| known.label == option.label) {
            options.push(option);
        }
    }
    if options.len() < 2 {
        return Err(format!(
            "the question \"{question}\" needs at least 2 options (the user can always type another answer)"
        ));
    }
    options.truncate(MAX_OPTIONS);
    let multi_select = ["multiSelect", "multi_select", "multiselect"]
        .iter()
        .any(|key| value.get(key).and_then(Value::as_bool) == Some(true));
    Ok(UserQuestion {
        question,
        header: text(value, &["header", "label", "topic"]),
        options,
        multi_select,
    })
}

/// Questions lues dans l'entrée de l'outil : `{"questions": [...]}`, ou une
/// seule question posée directement.
pub fn parse_questions(input: &Value) -> Result<Vec<UserQuestion>, String> {
    let questions = match input.get("questions").and_then(Value::as_array) {
        Some(list) => list
            .iter()
            .map(parse_question)
            .collect::<Result<Vec<_>, _>>()?,
        None if input.get("question").is_some() => vec![parse_question(input)?],
        None => return Err("`questions` must be a list of 1 to 4 questions".to_string()),
    };
    if questions.is_empty() || questions.len() > MAX_QUESTIONS {
        return Err(format!(
            "ask 1 to {MAX_QUESTIONS} questions per call (got {})",
            questions.len()
        ));
    }
    Ok(questions)
}

/// Pose les questions et rend les réponses au modèle.
pub(crate) fn run_ask_user_question(input: &Value) -> Result<String, String> {
    let questions = parse_questions(input)?;
    let ask = asker()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    let Some(ask) = ask else {
        return Err(
            "no user is available to answer right now: write your questions in your reply and stop, or continue with the most reasonable choice and say which one you made"
                .to_string(),
        );
    };
    let answers = ask(&questions)?;
    // Interface qui montre les questions sans attendre (appli web) : les réponses
    // viendront au message suivant, le tour s'arrête sur elles.
    if answers.is_empty() {
        return Ok(runtime::harness::QUESTIONS_DEFERRED.to_string());
    }
    serde_json::to_string_pretty(&json!({
        "answers": answers,
        "note": "These are the user's choices. Continue the task with them; do not ask the same questions again.",
    }))
    .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::{
        parse_questions, question_asker_available, run_ask_user_question, set_question_asker,
        UserAnswer,
    };
    use serde_json::json;
    use std::sync::Arc;

    #[test]
    fn reads_questions_with_full_or_plain_options() {
        let questions = parse_questions(&json!({
            "questions": [
                {
                    "question": "Quel moteur d'OCR ?",
                    "header": "OCR",
                    "options": [
                        { "label": "EasyOCR (Recommandé)", "description": "Plus simple, multi-langues" },
                        { "label": "manga-ocr", "description": "Meilleur pour le japonais" },
                        { "label": "Autre" }
                    ]
                },
                {
                    "question": "Quelles langues cibles ?",
                    "options": ["Français", "Anglais", "Français", " "],
                    "multiSelect": true
                }
            ]
        }))
        .expect("questions");
        assert_eq!(questions.len(), 2);
        assert_eq!(questions[0].header, "OCR");
        // Le choix « Autre » du modèle est retiré : l'interface le propose toujours.
        let labels: Vec<&str> = questions[0]
            .options
            .iter()
            .map(|option| option.label.as_str())
            .collect();
        assert_eq!(labels, ["EasyOCR (Recommandé)", "manga-ocr"]);
        assert_eq!(
            questions[0].options[0].description,
            "Plus simple, multi-langues"
        );
        // Chaînes simples, doublon et vide écartés.
        assert!(questions[1].multi_select);
        assert_eq!(questions[1].options.len(), 2);

        // Une seule question posée directement.
        let single = parse_questions(&json!({ "question": "Docker ?", "options": ["Oui", "Non"] }))
            .expect("question");
        assert_eq!(single[0].question, "Docker ?");

        assert!(parse_questions(&json!({ "questions": [] })).is_err());
        assert!(parse_questions(&json!({ "question": "Seule ?", "options": ["Oui"] })).is_err());
        assert!(parse_questions(&json!({ "questions": [{ "options": ["a", "b"] }] })).is_err());
    }

    #[test]
    fn asks_through_the_registered_interface_and_returns_the_answers() {
        let input = json!({ "question": "Docker ?", "options": ["Oui", "Non"] });
        set_question_asker(None);
        assert!(!question_asker_available());
        let unavailable = run_ask_user_question(&input).expect_err("personne pour répondre");
        assert!(unavailable.contains("no user is available"));

        set_question_asker(Some(Arc::new(|questions| {
            Ok(questions
                .iter()
                .map(|question| UserAnswer {
                    question: question.question.clone(),
                    answer: question.options[1].label.clone(),
                })
                .collect())
        })));
        assert!(question_asker_available());
        let output = run_ask_user_question(&input).expect("réponses");
        let value: serde_json::Value = serde_json::from_str(&output).expect("json");
        assert_eq!(value["answers"][0]["question"], "Docker ?");
        assert_eq!(value["answers"][0]["answer"], "Non");

        // L'utilisateur écarte les questions : le modèle en est informé.
        set_question_asker(Some(Arc::new(|_| {
            Err("the user dismissed the questions".to_string())
        })));
        assert!(run_ask_user_question(&input)
            .expect_err("écartées")
            .contains("dismissed"));

        // Questions montrées sans attendre (appli web) : la réponse viendra au
        // message suivant, le tour s'arrête.
        set_question_asker(Some(Arc::new(|_| Ok(Vec::new()))));
        assert_eq!(
            run_ask_user_question(&input).expect("posées"),
            runtime::harness::QUESTIONS_DEFERRED
        );
        set_question_asker(None);
    }
}
