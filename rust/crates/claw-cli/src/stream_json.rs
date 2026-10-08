//! `--output-format stream-json` : un événement JSON par ligne (format proche
//! de Claude Code), pour piloter Claw depuis un script, une CI ou un éditeur.
//!
//! ```text
//! {"type":"system","subtype":"init","session_id":…,"model":…,"tools":[…]}
//! {"type":"assistant","message":{"content":[{"type":"text","text":…}]}}
//! {"type":"assistant","message":{"content":[{"type":"tool_use","name":…,"input":{…}}]}}
//! {"type":"user","message":{"content":[{"type":"tool_result","tool_name":…,"content":…}]}}
//! {"type":"result","subtype":"success","result":…,"num_turns":…,"usage":{…}}
//! ```
//! Avec `--input-format stream-json`, chaque ligne de l'entrée standard
//! (`{"type":"user","message":{"content":"…"}}`) est un nouveau tour.

use std::io::Write;
use std::sync::Mutex;

use runtime::{PermissionPromptDecision, PermissionPrompter, PermissionRequest};
use serde_json::{json, Value};

use crate::TurnObserver;

/// Écrit une ligne JSON sur la sortie standard (verrouillée).
pub(crate) fn emit(value: &Value) {
    let mut stdout = std::io::stdout().lock();
    let _ = writeln!(stdout, "{value}");
    let _ = stdout.flush();
}

pub(crate) struct StreamJsonObserver {
    session_id: String,
    text: Mutex<String>,
    thinking: Mutex<String>,
}

impl StreamJsonObserver {
    pub(crate) fn new(session_id: &str) -> Self {
        Self {
            session_id: session_id.to_string(),
            text: Mutex::new(String::new()),
            thinking: Mutex::new(String::new()),
        }
    }

    fn assistant(&self, content: Value) {
        emit(&json!({
            "type": "assistant",
            "session_id": self.session_id,
            "message": { "role": "assistant", "content": [content] },
        }));
    }

    /// Publie le texte (et le raisonnement) accumulés depuis le dernier événement.
    pub(crate) fn flush(&self) {
        let thinking = std::mem::take(
            &mut *self
                .thinking
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        );
        if !thinking.is_empty() {
            self.assistant(json!({ "type": "thinking", "thinking": thinking }));
        }
        let text = std::mem::take(
            &mut *self
                .text
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        );
        if !text.is_empty() {
            self.assistant(json!({ "type": "text", "text": text }));
        }
    }
}

impl TurnObserver for StreamJsonObserver {
    fn text_delta(&self, text: &str) {
        self.text
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push_str(text);
    }

    fn thinking_delta(&self, text: &str) {
        self.thinking
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push_str(text);
    }

    fn tool_call(&self, name: &str, input: &str) {
        self.flush();
        let input = serde_json::from_str::<Value>(input)
            .unwrap_or_else(|_| Value::String(input.to_string()));
        self.assistant(json!({ "type": "tool_use", "name": name, "input": input }));
    }

    fn tool_result(&self, name: &str, output: &str, is_error: bool) {
        emit(&json!({
            "type": "user",
            "session_id": self.session_id,
            "message": {
                "role": "user",
                "content": [{ "type": "tool_result", "tool_name": name, "content": output, "is_error": is_error }],
            },
        }));
    }

    fn notice(&self, text: &str) {
        emit(
            &json!({ "type": "system", "subtype": "notice", "session_id": self.session_id, "text": text }),
        );
    }
}

/// Sans terminal interactif, une demande d'autorisation est refusée et signalée
/// (régler l'accès avec `--permission-mode` ou `--allowedTools`).
pub(crate) struct StreamJsonPrompter;

impl PermissionPrompter for StreamJsonPrompter {
    fn decide(&mut self, request: &PermissionRequest) -> PermissionPromptDecision {
        emit(&json!({
            "type": "system",
            "subtype": "permission_denied",
            "tool_name": request.tool_name,
            "required_mode": request.required_mode.as_str(),
            "hint": "autorisez la commande par une règle (\"permissions\": {\"allow\": [\"Bash(python:*)\"]}) ou relancez avec --permission-mode danger-full-access",
        }));
        // Ce texte est lu par le modèle : il doit comprendre qu'insister (avec un
        // autre shell, une autre syntaxe) ne servira à rien.
        PermissionPromptDecision::Deny {
            reason: format!(
                "Refusé : {} demande l'accord de l'utilisateur (mode {} requis), impossible dans \
                 cette session non interactive. Ne relancez pas cette commande, même avec un autre \
                 outil ou une autre syntaxe : poursuivez sans elle et indiquez à la fin la règle qui \
                 l'autoriserait (par exemple \"Bash(python:*)\" dans permissions.allow).",
                request.tool_name,
                request.required_mode.as_str()
            ),
        }
    }
}

/// Texte d'un message utilisateur reçu en `--input-format stream-json`.
pub(crate) fn user_text_from_line(line: &str) -> Result<Option<String>, String> {
    let line = line.trim();
    if line.is_empty() {
        return Ok(None);
    }
    let value: Value =
        serde_json::from_str(line).map_err(|error| format!("ligne JSON invalide : {error}"))?;
    let content = value
        .get("message")
        .and_then(|message| message.get("content"))
        .or_else(|| value.get("content"))
        .or_else(|| value.get("prompt"))
        .ok_or("champ message.content manquant")?;
    let text = match content {
        Value::String(text) => text.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter_map(|part| part.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("\n"),
        other => return Err(format!("contenu non pris en charge : {other}")),
    };
    Ok(Some(text))
}

#[cfg(test)]
mod tests {
    use super::user_text_from_line;

    #[test]
    fn reads_user_messages_in_several_shapes() {
        assert_eq!(
            user_text_from_line(r#"{"type":"user","message":{"role":"user","content":"Bonjour"}}"#),
            Ok(Some("Bonjour".to_string()))
        );
        assert_eq!(
            user_text_from_line(
                r#"{"type":"user","message":{"content":[{"type":"text","text":"a"},{"type":"text","text":"b"}]}}"#
            ),
            Ok(Some("a\nb".to_string()))
        );
        assert_eq!(user_text_from_line("   "), Ok(None));
        assert!(user_text_from_line("pas du json").is_err());
    }
}
