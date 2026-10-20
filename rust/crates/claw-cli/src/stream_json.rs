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
    /// Blocs de la réponse en cours (raisonnement, texte, appels d'outils),
    /// publiés quand elle est gardée : une réponse écartée (flux coupé, faux
    /// message de l'utilisateur) n'est jamais montrée.
    pending: Mutex<Vec<Value>>,
}

fn locked<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl StreamJsonObserver {
    pub(crate) fn new(session_id: &str) -> Self {
        Self {
            session_id: session_id.to_string(),
            text: Mutex::new(String::new()),
            thinking: Mutex::new(String::new()),
            pending: Mutex::new(Vec::new()),
        }
    }

    fn assistant(&self, content: Value) {
        let mut event = json!({
            "type": "assistant",
            "session_id": self.session_id,
            "message": { "role": "assistant" },
        });
        event["message"]["content"] = Value::Array(vec![content]);
        emit(&event);
    }

    /// Range le raisonnement et le texte reçus dans les blocs de la réponse.
    fn stage(&self) {
        let thinking = std::mem::take(&mut *locked(&self.thinking));
        let text = std::mem::take(&mut *locked(&self.text));
        let mut pending = locked(&self.pending);
        if !thinking.is_empty() {
            pending.push(json!({ "type": "thinking", "thinking": thinking }));
        }
        if !text.is_empty() {
            pending.push(json!({ "type": "text", "text": text }));
        }
    }

    /// Publie les blocs de la réponse en cours, dans l'ordre reçu.
    pub(crate) fn flush(&self) {
        self.stage();
        let blocks = std::mem::take(&mut *locked(&self.pending));
        for block in blocks {
            self.assistant(block);
        }
    }
}

impl TurnObserver for StreamJsonObserver {
    fn text_delta(&self, text: &str) {
        locked(&self.text).push_str(text);
    }

    fn thinking_delta(&self, text: &str) {
        locked(&self.thinking).push_str(text);
    }

    fn tool_call(&self, name: &str, input: &str) {
        self.stage();
        let input = serde_json::from_str::<Value>(input)
            .unwrap_or_else(|_| Value::String(input.to_string()));
        locked(&self.pending).push(json!({ "type": "tool_use", "name": name, "input": input }));
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

    // La réponse est gardée : ses blocs partent, avant l'exécution de ses
    // outils (une annonce sans outil part dès sa fin, pas une requête plus tard).
    fn reply_done(&self) {
        self.flush();
    }

    // Réponse abandonnée puis redemandée : rien d'elle n'est publié.
    fn reply_discarded(&self) {
        locked(&self.text).clear();
        locked(&self.thinking).clear();
        locked(&self.pending).clear();
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

    #[test]
    fn a_discarded_reply_is_never_published() {
        use super::StreamJsonObserver;
        use crate::TurnObserver;
        let observer = StreamJsonObserver::new("essai");
        observer.text_delta("user\nFais autre chose");
        observer.tool_call("bash", r#"{"command":"ls"}"#);
        observer.reply_discarded();
        assert!(super::locked(&observer.pending).is_empty());
        // La réponse suivante, gardée, garde son ordre : texte puis appel.
        observer.thinking_delta("Je regarde.");
        observer.text_delta("Je lis le projet.");
        observer.tool_call("read_file", r#"{"path":"a.py"}"#);
        let kinds: Vec<String> = super::locked(&observer.pending)
            .iter()
            .map(|block| block["type"].as_str().unwrap_or_default().to_string())
            .collect();
        assert_eq!(kinds, ["thinking", "text", "tool_use"]);
        observer.reply_done();
        assert!(super::locked(&observer.pending).is_empty());
    }
}
