//! `--output-format stream-json` : un événement JSON par ligne (format proche
//! de Claude Code), pour piloter Claw depuis un script, une CI ou un éditeur.
//!
//! ```text
//! {"type":"system","subtype":"init","session_id":…,"model":…,"tools":[…]}
//! {"type":"assistant","message":{"id":"reply-1","content":[{"type":"text","text":…}]}}
//! {"type":"assistant","message":{"id":"reply-1","content":[{"type":"tool_use","name":…,"input":{…}}]}}
//! {"type":"user","message":{"content":[{"type":"tool_result","tool_name":…,"content":…}]}}
//! {"type":"system","subtype":"answer_superseded"}
//! {"type":"result","subtype":"success","result":…,"num_turns":…,"usage":{…}}
//! ```
//! Chaque bloc d'une réponse du modèle part à part ; `message.id` est le même
//! pour tous les blocs d'une réponse (une requête au modèle). Une réponse sans
//! outil qu'un contrôle de Claw fait reprendre est suivie d'un
//! `answer_superseded` ; `answer_kept` dit qu'elle redevient la réponse.
//! Avec `--input-format stream-json`, chaque ligne de l'entrée standard
//! (`{"type":"user","message":{"content":"…"}}`) est un nouveau tour.

#[cfg(not(test))]
use std::io::Write;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use runtime::{PermissionPromptDecision, PermissionPrompter, PermissionRequest};
use serde_json::{json, Value};

use crate::TurnObserver;

/// Écrit une ligne JSON sur la sortie standard (verrouillée).
pub(crate) fn emit(value: &Value) {
    // Essais : les événements sont gardés pour être relus, rien n'est écrit.
    #[cfg(test)]
    EMITTED.with(|emitted| emitted.borrow_mut().push(value.clone()));
    #[cfg(not(test))]
    {
        let mut stdout = std::io::stdout().lock();
        let _ = writeln!(stdout, "{value}");
        let _ = stdout.flush();
    }
}

#[cfg(test)]
thread_local! {
    static EMITTED: std::cell::RefCell<Vec<Value>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Questions de l'agent (`AskUserQuestion`) montrées sans attendre, avec
/// `CLAW_DEFERRED_QUESTIONS=1` (moteur de l'appli web) : elles partent dans un
/// événement `{"type":"system","subtype":"questions"}`, l'utilisateur y répond
/// d'un toucher au message suivant, et le tour s'arrête sur elles. Sans la
/// variable, personne ne répond (script, CI) : l'outil n'est pas proposé.
pub(crate) fn deferred_asker() -> Option<tools::SharedQuestionAsker> {
    (std::env::var("CLAW_DEFERRED_QUESTIONS").as_deref() == Ok("1")).then(|| {
        Arc::new(|questions: &[tools::UserQuestion]| {
            emit(&json!({ "type": "system", "subtype": "questions", "questions": questions }));
            Ok(Vec::new())
        }) as tools::SharedQuestionAsker
    })
}

pub(crate) struct StreamJsonObserver {
    session_id: String,
    text: Mutex<String>,
    thinking: Mutex<String>,
    /// Blocs de la réponse en cours (raisonnement, texte, appels d'outils),
    /// publiés quand elle est gardée : une réponse écartée (flux coupé, faux
    /// message de l'utilisateur) n'est jamais montrée.
    pending: Mutex<Vec<Value>>,
    /// Réponses publiées, pour le numéro (`message.id`) de la suivante.
    replies: AtomicUsize,
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
            replies: AtomicUsize::new(0),
        }
    }

    fn assistant(&self, reply: &str, content: Value) {
        let mut event = json!({
            "type": "assistant",
            "session_id": self.session_id,
            "message": { "role": "assistant", "id": reply },
        });
        event["message"]["content"] = Value::Array(vec![content]);
        emit(&event);
    }

    fn system(&self, subtype: &str) {
        emit(&json!({ "type": "system", "subtype": subtype, "session_id": self.session_id }));
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

    /// Publie les blocs de la réponse en cours, dans l'ordre reçu, sous le
    /// numéro de cette réponse.
    pub(crate) fn flush(&self) {
        self.stage();
        let blocks = std::mem::take(&mut *locked(&self.pending));
        if blocks.is_empty() {
            return;
        }
        let reply = format!("reply-{}", self.replies.fetch_add(1, Ordering::Relaxed) + 1);
        for block in blocks {
            self.assistant(&reply, block);
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

    // Un contrôle de Claw renvoie le modèle au travail : sa dernière réponse,
    // déjà publiée, est provisoire (l'appli web la replie).
    fn answer_superseded(&self) {
        self.flush();
        self.system("answer_superseded");
    }

    fn answer_kept(&self) {
        self.system("answer_kept");
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

    #[test]
    fn the_blocks_of_a_reply_share_its_number_and_a_reply_sent_back_is_said() {
        use super::StreamJsonObserver;
        use crate::TurnObserver;
        super::EMITTED.with(|emitted| emitted.borrow_mut().clear());
        let observer = StreamJsonObserver::new("essai");
        // Une réponse (une requête au modèle) : trois blocs, un seul numéro.
        observer.thinking_delta("Je regarde.");
        observer.text_delta("Je lance les tests.");
        observer.tool_call("bash", r#"{"command":"pytest"}"#);
        observer.reply_done();
        // La réponse finale, qu'un contrôle fait reprendre, puis garder.
        observer.text_delta("Tout passe.");
        observer.reply_done();
        observer.answer_superseded();
        observer.answer_kept();
        let emitted = super::EMITTED.with(std::cell::RefCell::take);
        let shape: Vec<(&str, &str)> = emitted
            .iter()
            .map(|event| {
                let kind = event["type"].as_str().unwrap_or_default();
                let detail = event["message"]["id"]
                    .as_str()
                    .or_else(|| event["subtype"].as_str())
                    .unwrap_or_default();
                (kind, detail)
            })
            .collect();
        assert_eq!(
            shape,
            [
                ("assistant", "reply-1"),
                ("assistant", "reply-1"),
                ("assistant", "reply-1"),
                ("assistant", "reply-2"),
                ("system", "answer_superseded"),
                ("system", "answer_kept"),
            ]
        );
    }

    #[test]
    fn questions_are_shown_for_later_only_for_the_web_app() {
        use tools::{QuestionOption, UserQuestion};
        std::env::remove_var("CLAW_DEFERRED_QUESTIONS");
        assert!(super::deferred_asker().is_none());
        std::env::set_var("CLAW_DEFERRED_QUESTIONS", "1");
        let ask = super::deferred_asker().expect("appli web");
        std::env::remove_var("CLAW_DEFERRED_QUESTIONS");
        super::EMITTED.with(|emitted| emitted.borrow_mut().clear());
        let question = UserQuestion {
            question: "Quelle version capturer ?".to_string(),
            header: "Version".to_string(),
            options: vec![
                QuestionOption {
                    label: "Celle de la session (Recommandé)".to_string(),
                    description: "Les fichiers de la branche, pas encore en ligne".to_string(),
                },
                QuestionOption {
                    label: "Celle en ligne".to_string(),
                    description: String::new(),
                },
            ],
            multi_select: false,
        };
        // Aucune réponse tout de suite : elle viendra au message suivant.
        assert_eq!(ask(&[question]), Ok(Vec::new()));
        let emitted = super::EMITTED.with(std::cell::RefCell::take);
        assert_eq!(emitted.len(), 1);
        assert_eq!(emitted[0]["subtype"], "questions");
        assert_eq!(emitted[0]["questions"][0]["header"], "Version");
        assert_eq!(
            emitted[0]["questions"][0]["options"][1]["label"],
            "Celle en ligne"
        );
    }
}
