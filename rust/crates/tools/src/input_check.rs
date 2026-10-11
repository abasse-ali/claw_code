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

/// La réponse s'est arrêtée sur la limite de sortie au milieu de son texte,
/// sans appel d'outil (un appel coupé est traité par `check_cut_calls`) : le
/// runtime en demandera la suite.
#[must_use]
pub fn reply_cut_by_output_limit(stop_reason: Option<&str>, events: &[AssistantEvent]) -> bool {
    matches!(stop_reason, Some("length" | "max_tokens"))
        && !events
            .iter()
            .any(|event| matches!(event, AssistantEvent::ToolUse { .. }))
        && events.iter().any(
            |event| matches!(event, AssistantEvent::TextDelta(text) if !text.trim().is_empty()),
        )
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

/// Nom de propriété tel qu'un schéma réduit l'envoie (`-A` devient `A`,
/// `$filter` devient `filter`) : sans ce qui précède sa première lettre ou son
/// premier `_`.
fn plain_name(name: &str) -> &str {
    let trimmed = name.trim_start_matches(|ch: char| !(ch.is_ascii_alphabetic() || ch == '_'));
    if trimmed.is_empty() {
        name
    } else {
        trimmed
    }
}

/// Les arguments d'un appel avec les noms de propriétés du schéma de l'outil.
/// Un serveur qui refuse les schémas tels quels reçoit des noms réduits
/// (`-i`, `-A`, `$filter` deviennent `i`, `A`, `filter`), et le modèle les
/// renvoie ainsi : sans cela, l'outil ignorerait l'option sans rien dire.
/// `None` quand il n'y a rien à changer.
#[must_use]
pub fn restore_property_names(input: &str, schema: &Value) -> Option<String> {
    let properties = schema.get("properties")?.as_object()?;
    let mut value: Value = serde_json::from_str(input.trim()).ok()?;
    let arguments = value.as_object_mut()?;
    let unknown: Vec<String> = arguments
        .keys()
        .filter(|key| !properties.contains_key(*key))
        .cloned()
        .collect();
    let mut changed = false;
    for key in unknown {
        let mut originals = properties
            .keys()
            .filter(|name| *name != &key && plain_name(name) == key);
        let (Some(original), None) = (originals.next(), originals.next()) else {
            continue;
        };
        if arguments.contains_key(original) {
            continue;
        }
        if let Some(argument) = arguments.remove(&key) {
            arguments.insert(original.clone(), argument);
            changed = true;
        }
    }
    changed.then(|| value.to_string())
}

/// Met à la forme de son outil chaque appel de la réponse (`schema_for` : le
/// schéma d'un outil, s'il est connu) ; ce qui a été changé ou ignoré est
/// gardé pour être dit au modèle avec le résultat (`take_argument_note`).
pub fn restore_names_in(events: &mut [AssistantEvent], schema_for: impl Fn(&str) -> Option<Value>) {
    for event in events {
        if let AssistantEvent::ToolUse { name, input, .. } = event {
            let Some(schema) = schema_for(name) else {
                continue;
            };
            let restored = restore_property_names(input, &schema);
            let current = restored.as_deref().unwrap_or(input.as_str());
            let (normalized, note) = match normalize_arguments(name, current, &schema) {
                Some((normalized, note)) => (Some(normalized), note),
                None => (restored, None),
            };
            if let Some(normalized) = normalized {
                *input = normalized;
            }
            if let Some(note) = note {
                remember_argument_note(name, input, note);
            }
        }
    }
}

/// Noms d'arguments d'autres agents (ou devinés par un modèle) et le nom
/// qu'ils ont dans Claw. Appliqués seulement quand l'outil connaît ce nom, ne
/// connaît pas l'autre, et que l'appel ne porte pas déjà le bon.
const ARGUMENT_SYNONYMS: &[(&str, &str)] = &[
    ("file_path", "path"),
    ("filePath", "path"),
    ("filepath", "path"),
    ("filename", "path"),
    ("file", "path"),
    ("target_file", "path"),
    ("old_str", "old_string"),
    ("old_text", "old_string"),
    ("search", "old_string"),
    ("new_str", "new_string"),
    ("new_text", "new_string"),
    ("replace", "new_string"),
    ("replacement", "new_string"),
    ("replaceAll", "replace_all"),
    ("contents", "content"),
    ("text", "content"),
    ("file_text", "content"),
    ("cmd", "command"),
    ("script", "command"),
    ("regex", "pattern"),
    ("query", "pattern"),
    ("include", "glob"),
    ("case_insensitive", "-i"),
    ("ignore_case", "-i"),
    ("timeout_ms", "timeout"),
];

/// Les arguments d'un appel mis à la forme de son outil, et ce qu'il faut en
/// dire au modèle : noms d'autres agents traduits (`file_path` pour `path`),
/// nombres et booléens écrits en texte convertis (`"50"`, `"true"`), liste
/// reçue comme texte décodée, valeur sous le minimum ramenée, `start_line` et
/// `end_line` de `read_file` lus comme `offset` et `limit`, `cwd` de `bash`
/// appliqué. Les arguments inconnus restent, ignorés par l'outil, et sont
/// nommés. `None` : rien à changer ni à dire.
#[must_use]
pub fn normalize_arguments(
    tool_name: &str,
    input: &str,
    schema: &Value,
) -> Option<(String, Option<String>)> {
    let properties = schema.get("properties")?.as_object()?;
    let mut value: Value = serde_json::from_str(input.trim()).ok()?;
    let arguments = value.as_object_mut()?;
    let mut changed = false;
    let mut said: Vec<String> = Vec::new();

    for (other, official) in ARGUMENT_SYNONYMS {
        if properties.contains_key(*official)
            && !properties.contains_key(*other)
            && !arguments.contains_key(*official)
        {
            if let Some(argument) = arguments.remove(*other) {
                arguments.insert((*official).to_string(), argument);
                said.push(format!("\"{other}\" was read as \"{official}\""));
                changed = true;
            }
        }
    }
    // `read_file` : des numéros de lignes à la place de `offset` et `limit`.
    if tool_name == "read_file" && !arguments.contains_key("offset") {
        let line = |key: &str| {
            arguments.get(key).and_then(|value| match value {
                Value::Number(number) => number.as_u64(),
                Value::String(text) => text.trim().parse().ok(),
                _ => None,
            })
        };
        if let Some(start) = line("start_line") {
            let end = line("end_line");
            arguments.remove("start_line");
            arguments.remove("end_line");
            arguments.insert("offset".to_string(), Value::from(start.saturating_sub(1)));
            if let Some(end) = end.filter(|end| *end >= start) {
                arguments.insert("limit".to_string(), Value::from(end - start + 1));
            }
            said.push("start_line and end_line were read as offset and limit".to_string());
            changed = true;
        }
    }
    // `bash` : un dossier de travail donné à part.
    if tool_name == "bash" {
        let folder = ["cwd", "workdir", "working_directory", "directory"]
            .iter()
            .find_map(|key| {
                let folder = arguments.get(*key)?.as_str()?.trim().to_string();
                arguments.remove(*key);
                (!folder.is_empty()).then_some((*key, folder))
            });
        if let (Some((key, folder)), Some(Value::String(command))) =
            (folder, arguments.get("command").cloned())
        {
            let quoted = folder.replace('"', "\\\"");
            arguments.insert(
                "command".to_string(),
                Value::String(format!("cd \"{quoted}\" && {command}")),
            );
            said.push(format!(
                "\"{key}\" was applied as cd \"{folder}\" before the command"
            ));
            changed = true;
        }
    }
    // Types : ce que le modèle a écrit en texte, ou un nombre à virgule entier.
    let keys: Vec<String> = arguments.keys().cloned().collect();
    for key in &keys {
        let Some(property) = properties.get(key) else {
            continue;
        };
        if let Some(coerced) = arguments
            .get(key)
            .and_then(|argument| coerce(argument, property))
        {
            arguments.insert(key.clone(), coerced);
            changed = true;
        }
    }
    let unknown: Vec<String> = keys
        .iter()
        .filter(|key| !properties.contains_key(*key))
        .map(|key| format!("\"{key}\""))
        .collect();
    if !unknown.is_empty() {
        let accepted: Vec<&str> = properties.keys().map(String::as_str).collect();
        said.push(format!(
            "unknown argument(s) {} were ignored ({tool_name} accepts: {})",
            unknown.join(", "),
            accepted.join(", ")
        ));
    }
    if !changed && said.is_empty() {
        return None;
    }
    // Pas de majuscule : la note suit « Your call was adjusted before it
    // ran: », et elle commence souvent par un nom d'argument.
    let note = (!said.is_empty()).then(|| format!("{}.", said.join("; ")));
    Some((value.to_string(), note))
}

/// La valeur convertie au type que le schéma de l'argument demande, quand la
/// conversion est sans ambiguïté ; `None` sinon (ou si rien n'est à faire).
fn coerce(value: &Value, property: &Value) -> Option<Value> {
    let kind = property.get("type").and_then(Value::as_str)?;
    let converted = match (kind, value) {
        ("integer", Value::String(text)) => text.trim().parse::<i64>().ok().map(Value::from),
        ("integer", Value::Number(number)) if number.as_i64().is_none() => number
            .as_f64()
            .filter(|float| float.fract() == 0.0)
            .and_then(|float| float.to_string().parse::<i64>().ok())
            .map(Value::from),
        ("number", Value::String(text)) => text
            .trim()
            .parse::<f64>()
            .ok()
            .and_then(serde_json::Number::from_f64)
            .map(Value::Number),
        ("boolean", Value::String(text)) => match text.trim().to_ascii_lowercase().as_str() {
            "true" | "yes" | "1" => Some(Value::Bool(true)),
            "false" | "no" | "0" => Some(Value::Bool(false)),
            _ => None,
        },
        ("string", Value::Number(_) | Value::Bool(_)) => Some(Value::String(value.to_string())),
        ("array", Value::String(text)) => match serde_json::from_str::<Value>(text) {
            Ok(list @ Value::Array(_)) => Some(list),
            _ => (property.pointer("/items/type").and_then(Value::as_str) == Some("string"))
                .then(|| Value::Array(vec![value.clone()])),
        },
        ("object", Value::String(text)) => serde_json::from_str::<Value>(text)
            .ok()
            .filter(Value::is_object),
        _ => None,
    };
    // Un entier sous le minimum du schéma (`offset: -1`) y est ramené.
    let minimum = property.get("minimum").and_then(Value::as_i64);
    let integer = converted.as_ref().unwrap_or(value).as_i64();
    match (kind, integer, minimum) {
        ("integer", Some(integer), Some(minimum)) if integer < minimum => {
            Some(Value::from(minimum))
        }
        _ => converted,
    }
}

/// Notes de mise en forme en attente, par (outil, arguments mis en forme).
static ARGUMENT_NOTES: std::sync::Mutex<Vec<((String, String), String)>> =
    std::sync::Mutex::new(Vec::new());

fn remember_argument_note(tool_name: &str, input: &str, note: String) {
    let mut notes = ARGUMENT_NOTES
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    notes.push(((tool_name.to_string(), input.to_string()), note));
    // Un appel jamais exécuté (refusé, interrompu) ne garde pas sa note.
    if notes.len() > 64 {
        notes.remove(0);
    }
}

/// Ce que la mise en forme de cet appel a changé ou ignoré, à dire au modèle
/// avec son résultat (une fois).
#[must_use]
pub fn take_argument_note(tool_name: &str, input: &str) -> Option<String> {
    let mut notes = ARGUMENT_NOTES
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let index = notes
        .iter()
        .position(|((name, arguments), _)| name == tool_name && arguments == input)?;
    Some(format!(
        "[Claw] Your call was adjusted before it ran: {}",
        notes.remove(index).1
    ))
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
/// `known` : les arguments que l'outil accepte, pour nommer ceux qu'il ne
/// connaît pas quand un obligatoire manque.
#[must_use]
pub fn problem(
    tool_name: &str,
    input: &str,
    required: &[String],
    known: &[String],
) -> Option<String> {
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
    // Des arguments envoyés sous un nom que l'outil ne connaît pas : les
    // nommer, plutôt que de dire que rien n'a été envoyé.
    let sent: Vec<String> = serde_json::from_str::<Value>(input.trim())
        .ok()
        .and_then(|value| value.as_object().cloned())
        .map(|arguments| {
            arguments
                .keys()
                .filter(|key| !known.is_empty() && !known.contains(key))
                .map(|key| format!("\"{key}\""))
                .collect()
        })
        .unwrap_or_default();
    if !sent.is_empty() {
        return Some(format!(
            "[Claw] {tool_name} was not executed: required argument(s) {} missing. You sent {}, which {tool_name} does not know. It accepts: {}.",
            missing.join(", "),
            sent.join(", "),
            known.join(", ")
        ));
    }
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
        let problem = |tool: &str, input: &str, required: &[String]| {
            super::problem(tool, input, required, &[])
        };
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
        // Un obligatoire manque et un argument inconnu a été envoyé : nommé.
        let known: Vec<String> = ["path", "old_string", "new_string", "replace_all"]
            .iter()
            .map(ToString::to_string)
            .collect();
        let named = super::problem(
            "edit_file",
            r#"{"target":"a.py","old_string":"x","new_string":"y"}"#,
            &required,
            &known,
        )
        .expect("argument manquant");
        assert!(
            named.contains("You sent \"target\", which edit_file does not know"),
            "{named}"
        );
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
        let told =
            problem("edit_file", &input(&events), &edit_arguments(), &[]).expect("appel marqué");
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
    fn a_text_reply_stopped_by_the_output_limit_is_seen_as_cut() {
        use super::reply_cut_by_output_limit;
        use runtime::AssistantEvent;
        let text = vec![
            AssistantEvent::TextDelta("Voici la".to_string()),
            AssistantEvent::MessageStop,
        ];
        assert!(reply_cut_by_output_limit(Some("length"), &text));
        assert!(reply_cut_by_output_limit(Some("max_tokens"), &text));
        assert!(!reply_cut_by_output_limit(Some("end_turn"), &text));
        assert!(!reply_cut_by_output_limit(None, &text));
        // Un appel d'outil coupé a son propre traitement.
        let with_call = vec![
            AssistantEvent::TextDelta("Je lance la commande".to_string()),
            AssistantEvent::ToolUse {
                id: "1".to_string(),
                name: "bash".to_string(),
                input: "{}".to_string(),
            },
        ];
        assert!(!reply_cut_by_output_limit(Some("length"), &with_call));
        // Rien d'écrit (le raisonnement a tout pris) : ce n'est pas une
        // réponse coupée, c'est une réponse vide.
        assert!(!reply_cut_by_output_limit(
            Some("length"),
            &[AssistantEvent::MessageStop]
        ));
    }

    #[test]
    fn arguments_are_put_in_the_tool_s_shape_and_the_model_is_told() {
        use super::{normalize_arguments, restore_names_in, take_argument_note};
        use runtime::AssistantEvent;
        let read = json!({
            "type": "object",
            "properties": {
                "path": { "type": "string" },
                "offset": { "type": "integer", "minimum": 0 },
                "limit": { "type": "integer", "minimum": 1 }
            },
            "required": ["path"]
        });
        // Nom d'un autre agent, nombre en texte.
        let (input, note) =
            normalize_arguments("read_file", r#"{"file_path":"a.py","offset":"50"}"#, &read)
                .expect("mis en forme");
        let input: serde_json::Value = serde_json::from_str(&input).expect("JSON");
        assert_eq!(input, json!({ "path": "a.py", "offset": 50 }));
        assert!(note
            .unwrap_or_default()
            .contains("\"file_path\" was read as \"path\""));
        // Numéros de lignes, valeur négative, argument inconnu.
        let (input, note) = normalize_arguments(
            "read_file",
            r#"{"path":"a.py","start_line":10,"end_line":19,"limit":-3,"mode":"x"}"#,
            &read,
        )
        .expect("mis en forme");
        let input: serde_json::Value = serde_json::from_str(&input).expect("JSON");
        assert_eq!(input["offset"], json!(9));
        assert_eq!(input["limit"], json!(10));
        let note = note.unwrap_or_default();
        assert!(
            note.contains("start_line and end_line were read as offset and limit"),
            "{note}"
        );
        assert!(
            note.contains("unknown argument(s) \"mode\" were ignored"),
            "{note}"
        );
        // `cwd` de bash appliqué.
        let bash = json!({
            "type": "object",
            "properties": { "command": { "type": "string" }, "timeout": { "type": "integer" } }
        });
        let (input, _) =
            normalize_arguments("bash", r#"{"command":"npm test","cwd":"frontend"}"#, &bash)
                .expect("mis en forme");
        assert!(input.contains(r#"cd \"frontend\" && npm test"#), "{input}");
        // Rien à changer : rien.
        assert_eq!(
            normalize_arguments("read_file", r#"{"path":"a.py"}"#, &read),
            None
        );

        // Dans une réponse : l'appel est corrigé, la note attend son résultat.
        let mut events = vec![AssistantEvent::ToolUse {
            id: "appel-1".to_string(),
            name: "read_file".to_string(),
            input: r#"{"filePath":"b.py","limit":"5"}"#.to_string(),
        }];
        restore_names_in(&mut events, |_| Some(read.clone()));
        let AssistantEvent::ToolUse { input, .. } = &events[0] else {
            panic!("appel attendu");
        };
        let note = take_argument_note("read_file", input).expect("note");
        assert!(
            note.starts_with("[Claw] Your call was adjusted before it ran"),
            "{note}"
        );
        assert_eq!(take_argument_note("read_file", input), None);
    }

    #[test]
    fn reduced_property_names_get_their_real_name_back() {
        use super::restore_property_names;
        let schema = json!({
            "type": "object",
            "properties": { "pattern": {}, "-i": {}, "-A": {}, "$filter": {}, "@type": {}, "type": {} }
        });
        // Le modèle a reçu `i`, `A`, `filter` à la place de `-i`, `-A`, `$filter`.
        let restored =
            restore_property_names(r#"{"pattern":"todo","i":true,"A":2,"filter":"x"}"#, &schema)
                .expect("noms rendus");
        let restored: serde_json::Value = serde_json::from_str(&restored).expect("JSON");
        assert_eq!(
            restored,
            json!({ "pattern": "todo", "-i": true, "-A": 2, "$filter": "x" })
        );
        // Un nom connu reste tel quel ; deux noms possibles, rien n'est deviné.
        assert_eq!(
            restore_property_names(r#"{"pattern":"todo","type":"rs"}"#, &schema),
            None
        );
        assert_eq!(
            restore_property_names(r#"{"pattern":"x","-i":true}"#, &schema),
            None
        );
        assert_eq!(restore_property_names("pas du json", &schema), None);
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
        assert_eq!(problem("edit_file", &quoting, &edit_arguments(), &[]), None);
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
