//! Arguments des appels d'outils reçus d'un modèle : repérer un appel coupé
//! (réponse arrêtée avant la fin de ses arguments) ou incomplet, avant de
//! demander une autorisation ou d'exécuter quoi que ce soit. Sert à l'agent
//! principal comme aux sous-agents.

use runtime::AssistantEvent;
use serde_json::{json, Map, Value};

/// Clé mise à la place des arguments d'un appel que le fournisseur a clos
/// avant d'envoyer ses arguments, quand la réponse n'est pas redemandée.
const ENDED_BEFORE_ARGUMENTS: &str = "claw_reply_ended_before_arguments";

/// Clé mise à la place des arguments d'un appel coupé par la limite de sortie
/// du modèle : l'appel n'est pas exécuté et le modèle apprend pourquoi.
const CUT_BY_OUTPUT_LIMIT: &str = "claw_reply_cut_by_output_limit";

/// Arguments obligatoires d'un outil, d'après son schéma.
#[must_use]
pub fn required_arguments(schema: &Value) -> Vec<String> {
    schema
        .get("required")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect()
}

/// Les arguments obligatoires absents de `input`, ou l'erreur de lecture si
/// `input` n'est pas un objet JSON. Des arguments vides valent `{}`.
fn missing_arguments(input: &str, required: &[String]) -> Result<Vec<String>, String> {
    let trimmed = input.trim();
    let value = if trimmed.is_empty() {
        Value::Object(Map::new())
    } else {
        serde_json::from_str(trimmed).map_err(|error| error.to_string())?
    };
    let object = value
        .as_object()
        .ok_or_else(|| "expected a JSON object".to_string())?;
    Ok(required
        .iter()
        .filter(|name| object.get(*name).is_none_or(Value::is_null))
        .cloned()
        .collect())
}

/// Ce qui est arrivé à un appel d'outil inexécutable dans une réponse qui ne
/// s'est pas terminée sur un appel d'outil.
#[derive(Debug, PartialEq, Eq)]
pub enum Cut {
    /// Le modèle a atteint sa limite de sortie au milieu des arguments.
    OutputLimit,
    /// Le fournisseur a clos la réponse avant la fin de l'appel.
    Unfinished,
}

/// L'appel a été coupé : la réponse s'est arrêtée pour une autre raison qu'un
/// appel d'outil (`stop_reason`), et ses arguments ne sont pas complets.
#[must_use]
pub fn cut_call(stop_reason: Option<&str>, input: &str, required: &[String]) -> Option<Cut> {
    if stop_reason == Some("tool_use")
        || missing_arguments(input, required).is_ok_and(|missing| missing.is_empty())
    {
        return None;
    }
    Some(match stop_reason {
        Some("length" | "max_tokens") => Cut::OutputLimit,
        _ => Cut::Unfinished,
    })
}

/// Arguments mis à la place de ceux d'un appel coupé par la limite de sortie.
#[must_use]
pub fn output_limit_marker() -> String {
    json!({ CUT_BY_OUTPUT_LIMIT: true }).to_string()
}

/// Ces arguments sont exactement la marque `key` posée par Claw (et non un
/// texte qui la citerait, comme le code de Claw lui-même).
fn is_marker(input: &str, key: &str) -> bool {
    serde_json::from_str::<Value>(input.trim()).is_ok_and(|value| value == json!({ key: true }))
}

/// Repère les appels d'outils coupés d'une réponse qui ne s'est pas terminée
/// sur un appel d'outil. Coupé par la limite de sortie, l'appel est marqué :
/// il ne sera pas exécuté et le modèle apprendra pourquoi. Clos par le
/// fournisseur avant ses arguments, c'est une erreur si `retry_unfinished`
/// (la réponse sera redemandée), sinon l'appel est marqué lui aussi.
/// `required_for` donne les arguments obligatoires d'un outil.
///
/// # Errors
///
/// Le message de [`unfinished_error`] quand un appel est clos avant ses
/// arguments et que `retry_unfinished` est vrai.
pub fn check_cut_calls(
    events: &mut [AssistantEvent],
    stop_reason: Option<&str>,
    required_for: impl Fn(&str) -> Vec<String>,
    retry_unfinished: bool,
) -> Result<(), String> {
    if stop_reason == Some("tool_use") {
        return Ok(());
    }
    for event in events {
        let AssistantEvent::ToolUse { name, input, .. } = event else {
            continue;
        };
        match cut_call(stop_reason, input, &required_for(name)) {
            None => {}
            Some(Cut::OutputLimit) => *input = output_limit_marker(),
            Some(Cut::Unfinished) if retry_unfinished => return Err(unfinished_error(name, input)),
            Some(Cut::Unfinished) => *input = json!({ ENDED_BEFORE_ARGUMENTS: true }).to_string(),
        }
    }
    Ok(())
}

/// Erreur d'une réponse close avant la fin d'un appel d'outil : la réponse
/// est redemandée au modèle (voir `is_broken_stream`).
#[must_use]
pub fn unfinished_error(tool_name: &str, input: &str) -> String {
    let what = if matches!(input.trim(), "" | "{}") {
        "sans ses arguments"
    } else {
        "avec des arguments tronqués"
    };
    format!("{UNFINISHED_REPLY} : l'appel de l'outil {tool_name} est arrivé {what}")
}

/// Début du message d'erreur d'une réponse close avant la fin d'un appel.
pub const UNFINISHED_REPLY: &str = "réponse incomplète du modèle";

/// Ce qui est dit à l'utilisateur quand les nouvelles tentatives n'y changent
/// rien et qu'il doit choisir : réessayer, changer de modèle ou arrêter.
#[must_use]
pub fn unfinished_reason(error: &str) -> Option<String> {
    error
        .contains(UNFINISHED_REPLY)
        .then(|| "ses réponses s'arrêtent avant la fin des appels d'outils".to_string())
}

/// Ce qui rend l'appel inexécutable, dit au modèle : rien n'est alors demandé
/// à l'utilisateur ni exécuté. `None` si les arguments sont utilisables.
#[must_use]
pub fn problem(tool_name: &str, input: &str, required: &[String]) -> Option<String> {
    if is_marker(input, CUT_BY_OUTPUT_LIMIT) {
        return Some(format!(
            "Your reply reached the output limit before the arguments of `{tool_name}` were complete, so nothing was executed. Keep your reasoning short and send a smaller call: write a long file in several parts (create it with the first part, then extend it with edit_file), or change fewer lines per edit_file call."
        ));
    }
    if is_marker(input, ENDED_BEFORE_ARGUMENTS) {
        return Some(format!(
            "Your reply ended before the arguments of `{tool_name}` arrived, so nothing was executed. Send the call again with its arguments."
        ));
    }
    let missing = match missing_arguments(input, required) {
        Ok(missing) => missing,
        Err(error) => {
            return Some(format!(
                "invalid tool input JSON: {error} - nothing was executed, send the call again with a valid JSON object"
            ))
        }
    };
    if missing.is_empty() {
        return None;
    }
    let needed = required.join(", ");
    Some(if missing.len() == required.len() {
        format!(
            "`{tool_name}` was called without its arguments, so nothing was executed. It needs: {needed}. Send the call again with them; if your reply was cut off before the arguments, send a smaller call."
        )
    } else {
        format!(
            "`{tool_name}` is missing its required argument(s) {}, so nothing was executed. It needs: {needed}. Send the call again with every one of them.",
            missing.join(", ")
        )
    })
}

#[cfg(test)]
mod tests {
    use super::{
        cut_call, output_limit_marker, problem, required_arguments, unfinished_error, Cut,
    };
    use serde_json::json;

    fn edit_arguments() -> Vec<String> {
        required_arguments(&json!({
            "type": "object",
            "properties": { "path": {}, "old_string": {}, "new_string": {}, "replace_all": {} },
            "required": ["path", "old_string", "new_string"]
        }))
    }

    #[test]
    fn a_reply_that_stops_before_the_arguments_is_a_cut_call() {
        let required = edit_arguments();
        let whole = r#"{"path":"a.py","old_string":"x","new_string":"y"}"#;
        // Le fournisseur a clos la réponse sans terminer l'appel.
        assert_eq!(
            cut_call(Some("end_turn"), "", &required),
            Some(Cut::Unfinished)
        );
        assert_eq!(
            cut_call(Some("end_turn"), "{}", &required),
            Some(Cut::Unfinished)
        );
        assert_eq!(
            cut_call(None, r#"{"path":"a.py","old_str"#, &required),
            Some(Cut::Unfinished)
        );
        // Limite de sortie atteinte au milieu des arguments.
        assert_eq!(
            cut_call(
                Some("length"),
                r#"{"path":"a.py","old_string":"x"#,
                &required
            ),
            Some(Cut::OutputLimit)
        );
        assert_eq!(
            cut_call(Some("max_tokens"), "", &required),
            Some(Cut::OutputLimit)
        );
        // Appel terminé par le fournisseur, ou arguments complets malgré une
        // autre raison d'arrêt (certains serveurs disent « stop ») : pas coupé.
        assert_eq!(cut_call(Some("tool_use"), "{}", &required), None);
        assert_eq!(cut_call(Some("end_turn"), whole, &required), None);
        assert_eq!(cut_call(Some("length"), whole, &required), None);
        // Outil sans argument obligatoire : des arguments vides sont complets.
        assert_eq!(cut_call(Some("end_turn"), "", &[]), None);
    }

    #[test]
    fn unusable_arguments_are_explained_to_the_model() {
        let required = edit_arguments();
        assert_eq!(
            problem(
                "edit_file",
                r#"{"path":"a.py","old_string":"x","new_string":""}"#,
                &required
            ),
            None
        );
        let empty = problem("edit_file", "{}", &required).expect("appel vide");
        assert!(empty.contains("`edit_file` was called without its arguments"));
        assert!(empty.contains("It needs: path, old_string, new_string"));
        let partial = problem(
            "edit_file",
            r#"{"path":"a.py","old_string":null}"#,
            &required,
        )
        .expect("arguments manquants");
        assert!(partial.contains("required argument(s) old_string, new_string"));
        let broken =
            problem("edit_file", r#"{"path": "a.py", "#, &required).expect("JSON invalide");
        assert!(broken.starts_with("invalid tool input JSON: "));
        let cut = problem("write_file", &output_limit_marker(), &required).expect("appel coupé");
        assert!(cut.contains("reached the output limit before the arguments of `write_file`"));
        // Un outil sans argument obligatoire accepte un appel vide.
        assert_eq!(problem("ExitPlanMode", "", &[]), None);
    }

    #[test]
    fn a_reply_is_checked_as_a_whole() {
        use super::check_cut_calls;
        use runtime::AssistantEvent;
        let call = |input: &str| AssistantEvent::ToolUse {
            id: "appel-1".to_string(),
            name: "edit_file".to_string(),
            input: input.to_string(),
        };
        let input = |events: &[AssistantEvent]| match &events[0] {
            AssistantEvent::ToolUse { input, .. } => input.clone(),
            other => panic!("appel attendu, reçu {other:?}"),
        };
        let required = |_: &str| edit_arguments();

        // Agent principal : la réponse close avant les arguments est redemandée.
        let mut events = vec![call("")];
        assert_eq!(
            check_cut_calls(&mut events, Some("end_turn"), required, true),
            Err("réponse incomplète du modèle : l'appel de l'outil edit_file est arrivé sans ses arguments".to_string())
        );
        // Sous-agent : l'appel est marqué, et le modèle apprend quoi faire.
        let mut events = vec![call("")];
        assert_eq!(
            check_cut_calls(&mut events, Some("end_turn"), required, false),
            Ok(())
        );
        let told = problem("edit_file", &input(&events), &edit_arguments()).expect("appel marqué");
        assert!(told.starts_with("Your reply ended before the arguments of `edit_file` arrived"));
        // Limite de sortie : marqué dans les deux cas.
        let mut events = vec![call(r#"{"path":"a.py","old_str"#)];
        assert_eq!(
            check_cut_calls(&mut events, Some("length"), required, true),
            Ok(())
        );
        assert_eq!(input(&events), output_limit_marker());
        // Réponse terminée sur un appel d'outil : rien n'est touché.
        let mut events = vec![call("{}")];
        assert_eq!(
            check_cut_calls(&mut events, Some("tool_use"), required, true),
            Ok(())
        );
        assert_eq!(input(&events), "{}");
    }

    #[test]
    fn text_that_quotes_a_marker_is_not_a_marker() {
        // Un fichier qui cite la marque (le code de Claw lui-même) s'écrit normalement.
        let quoting = json!({
            "path": "notes.md",
            "old_string": "x",
            "new_string": "la clé claw_reply_cut_by_output_limit marque un appel coupé"
        })
        .to_string();
        assert_eq!(problem("edit_file", &quoting, &edit_arguments()), None);
    }

    #[test]
    fn the_unfinished_error_says_what_arrived() {
        assert_eq!(
            unfinished_error("edit_file", "{}"),
            "réponse incomplète du modèle : l'appel de l'outil edit_file est arrivé sans ses arguments"
        );
        assert!(unfinished_error("write_file", r#"{"path":"a"#)
            .ends_with("avec des arguments tronqués"));
    }
}
