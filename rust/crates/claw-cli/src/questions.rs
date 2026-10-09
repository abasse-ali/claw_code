//! Questions de l'agent (outil `AskUserQuestion`) posées dans le terminal :
//! un menu par question, avec la possibilité de taper une autre réponse.

use std::io::{self, IsTerminal, Write};
use std::sync::Arc;

use tools::{SharedQuestionAsker, UserAnswer, UserQuestion};

/// Rendu au modèle quand l'utilisateur écarte les questions.
pub(crate) const DISMISSED: &str = "the user dismissed the questions without answering: continue with the most reasonable choices and say which ones you made, or ask again later if a choice is essential";

/// De quoi poser les questions dans le terminal ; `None` hors d'un terminal
/// interactif, où personne ne répondrait.
pub(crate) fn terminal_asker() -> Option<SharedQuestionAsker> {
    (io::stdin().is_terminal() && io::stdout().is_terminal())
        .then(|| Arc::new(ask_in_terminal) as SharedQuestionAsker)
}

fn ask_in_terminal(questions: &[UserQuestion]) -> Result<Vec<UserAnswer>, String> {
    // L'indicateur d'activité s'efface le temps des questions.
    crate::live::hide();
    let mut answers = Vec::with_capacity(questions.len());
    for (index, question) in questions.iter().enumerate() {
        let title = question_title(index, questions.len(), &question.header);
        let answer = if question.multi_select {
            ask_several(&title, question)?
        } else {
            ask_one(&title, question)?
        };
        answers.push(UserAnswer {
            question: question.question.clone(),
            answer,
        });
    }
    Ok(answers)
}

/// « Question 2 sur 4 · OCR ».
pub(crate) fn question_title(index: usize, total: usize, header: &str) -> String {
    let position = if total > 1 {
        format!("Question {} sur {total}", index + 1)
    } else {
        "Question".to_string()
    };
    if header.trim().is_empty() {
        position
    } else {
        format!("{position} · {}", header.trim())
    }
}

fn option_line(option: &tools::QuestionOption) -> String {
    if option.description.is_empty() {
        option.label.clone()
    } else {
        format!("{} - {}", option.label, option.description)
    }
}

fn ask_one(title: &str, question: &UserQuestion) -> Result<String, String> {
    let width = crate::approval::box_width().saturating_sub(6).max(20);
    let body = crate::transcript::wrap(&question.question, width);
    let mut options: Vec<String> = question.options.iter().map(option_line).collect();
    options.push("Autre réponse (à saisir)".to_string());
    match crate::approval::ask(title, &body, "Votre choix", &options) {
        Ok(crate::approval::Answer::Pick(index)) => match question.options.get(index) {
            Some(option) => Ok(option.label.clone()),
            None => read_answer("Votre réponse : "),
        },
        _ => Err(DISMISSED.to_string()),
    }
}

/// Plusieurs réponses possibles : les numéros choisis, ou une réponse saisie.
fn ask_several(title: &str, question: &UserQuestion) -> Result<String, String> {
    println!("\n{title}");
    println!("  {}", question.question);
    for (index, option) in question.options.iter().enumerate() {
        println!("  {}. {}", index + 1, option_line(option));
    }
    let typed = read_answer("Numéros séparés par des virgules, ou votre réponse : ")?;
    Ok(several_from(&typed, question))
}

/// « 1, 3 » devient les libellés des choix 1 et 3 ; tout autre texte est la réponse.
pub(crate) fn several_from(typed: &str, question: &UserQuestion) -> String {
    let picked: Option<Vec<&str>> = typed
        .split([',', ';', ' '])
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .map(|token| {
            let number: usize = token.parse().ok()?;
            let option = question.options.get(number.checked_sub(1)?)?;
            Some(option.label.as_str())
        })
        .collect();
    match picked {
        Some(labels) if !labels.is_empty() => labels.join(", "),
        _ => typed.trim().to_string(),
    }
}

fn read_answer(prompt: &str) -> Result<String, String> {
    // L'écoute du clavier (Échap) rend la main le temps de la saisie.
    let _keys = crate::turn_keys::pause();
    print!("{prompt}");
    let _ = io::stdout().flush();
    let mut line = String::new();
    io::stdin()
        .read_line(&mut line)
        .map_err(|error| error.to_string())?;
    let answer = line.trim();
    if answer.is_empty() {
        Err(DISMISSED.to_string())
    } else {
        Ok(answer.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::{question_title, several_from};
    use tools::{QuestionOption, UserQuestion};

    #[test]
    fn titles_and_typed_choices_read_naturally() {
        assert_eq!(question_title(1, 4, "OCR"), "Question 2 sur 4 · OCR");
        assert_eq!(question_title(0, 1, " "), "Question");

        let option = |label: &str| QuestionOption {
            label: label.to_string(),
            description: String::new(),
        };
        let question = UserQuestion {
            question: "Quelles langues ?".to_string(),
            header: String::new(),
            options: vec![option("Français"), option("Anglais"), option("Espagnol")],
            multi_select: true,
        };
        assert_eq!(several_from("1, 3", &question), "Français, Espagnol");
        assert_eq!(several_from("2", &question), "Anglais");
        // Un numéro hors liste ou du texte : la saisie est la réponse.
        assert_eq!(several_from("1, 9", &question), "1, 9");
        assert_eq!(
            several_from(" italien et grec ", &question),
            "italien et grec"
        );
    }
}
