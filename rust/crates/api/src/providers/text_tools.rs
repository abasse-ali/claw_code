//! Appels d'outils « en texte » pour les modèles ou serveurs sans appel de
//! fonctions natif (beaucoup de modèles locaux, llama.cpp sans `--jinja`…).
//!
//! Les outils sont décrits dans le prompt système et le modèle répond avec des
//! blocs `<tool_call>{"name": …, "arguments": {…}}</tool_call>` (format Hermes,
//! connu de Qwen, Mistral, Llama…), que l'on retransforme en appels d'outils.

use std::collections::BTreeSet;
use std::sync::RwLock;

use serde_json::{json, Value};

use crate::types::ToolDefinition;

const OPEN_TAG: &str = "<tool_call>";
const CLOSE_TAG: &str = "</tool_call>";

static TEXT_MODE_MODELS: RwLock<BTreeSet<String>> = RwLock::new(BTreeSet::new());

/// Mode texte imposé (`CLAW_TOOL_MODE=text`), configuré pour le fournisseur
/// (`"toolMode": "text"`), ou appris après un refus du serveur.
#[must_use]
pub fn text_tools_enabled(model: &str) -> bool {
    if std::env::var("CLAW_TOOL_MODE").is_ok_and(|value| value.trim().eq_ignore_ascii_case("text"))
    {
        return true;
    }
    if super::compat::split_compat_model(model).is_some_and(|(provider, _)| provider.text_tools) {
        return true;
    }
    TEXT_MODE_MODELS
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .contains(model)
}

/// Retient qu'un modèle doit passer par le mode texte.
pub fn enable_text_tools(model: &str) {
    TEXT_MODE_MODELS
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .insert(model.to_string());
}

/// Le serveur refuse le paramètre `tools` pour ce modèle.
#[must_use]
pub fn is_tools_unsupported_error(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    [
        "does not support tools",
        "support tool use",
        "tools are not supported",
        "tools is not supported",
        "tool calling is not supported",
        "tool use is not supported",
        "function calling is not",
        "requires --jinja",
        "does not support function",
    ]
    .iter()
    .any(|pattern| lower.contains(pattern))
}

/// Section du prompt système décrivant les outils et le format d'appel.
#[must_use]
pub fn tool_protocol_prompt(tools: &[ToolDefinition]) -> String {
    let mut prompt = String::from(
        "# Tool calling\nYou can call tools. To call one, write a block exactly like this (one block per call, valid JSON, nothing else inside):\n<tool_call>\n{\"name\": \"tool_name\", \"arguments\": {\"param\": \"value\"}}\n</tool_call>\nAfter your tool calls, stop writing and wait: the results come back in <tool_result> blocks. Never invent tool results. When the task is done, answer normally without any <tool_call> block.\n\n## Available tools\n",
    );
    for tool in tools {
        prompt.push_str(&format!(
            "- {}: {}\n  parameters: {}\n",
            tool.name,
            tool.description.as_deref().unwrap_or("").replace('\n', " "),
            compact_schema(&tool.input_schema)
        ));
    }
    prompt
}

/// Schéma JSON réduit à l'essentiel (propriétés, types, champs requis).
fn compact_schema(schema: &Value) -> String {
    let Some(properties) = schema.get("properties").and_then(Value::as_object) else {
        return schema.to_string();
    };
    let required: Vec<&str> = schema
        .get("required")
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    let fields = properties
        .iter()
        .map(|(name, property)| {
            let kind = property.get("type").map_or_else(
                || "any".to_string(),
                |kind| {
                    kind.as_str()
                        .map_or_else(|| kind.to_string(), ToOwned::to_owned)
                },
            );
            let marker = if required.contains(&name.as_str()) {
                ""
            } else {
                "?"
            };
            format!("{name}{marker}: {kind}")
        })
        .collect::<Vec<_>>();
    format!("{{{}}}", fields.join(", "))
}

#[must_use]
pub fn render_tool_call(name: &str, input: &Value) -> String {
    format!(
        "{OPEN_TAG}\n{}\n{CLOSE_TAG}",
        json!({ "name": name, "arguments": input })
    )
}

#[must_use]
pub fn render_tool_result(name: &str, output: &str, is_error: bool) -> String {
    let status = if is_error { " error=\"true\"" } else { "" };
    format!("<tool_result name=\"{name}\"{status}>\n{output}\n</tool_result>")
}

/// Appels trouvés dans le texte du modèle, et texte restant à afficher.
#[must_use]
pub fn extract_tool_calls(text: &str) -> (String, Vec<(String, Value)>) {
    let mut visible = String::new();
    let mut calls = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find(OPEN_TAG) {
        visible.push_str(&rest[..start]);
        let after = &rest[start + OPEN_TAG.len()..];
        let (body, next) = match after.find(CLOSE_TAG) {
            Some(end) => (&after[..end], &after[end + CLOSE_TAG.len()..]),
            // Balise non refermée (sortie coupée) : on prend tout ce qui suit.
            None => (after, ""),
        };
        if let Some(call) = parse_call(body) {
            calls.push(call);
        }
        rest = next;
    }
    visible.push_str(rest);
    if calls.is_empty() {
        // Repli : bloc ```json contenant {"name": …, "arguments": …}.
        if let Some(call) = fenced_call(text) {
            return (String::new(), vec![call]);
        }
    }
    (visible.trim().to_string(), calls)
}

fn parse_call(body: &str) -> Option<(String, Value)> {
    let repaired = crate::providers::text_tools::lenient_json(body.trim())?;
    let name = repaired.get("name")?.as_str()?.to_string();
    let arguments = repaired
        .get("arguments")
        .or_else(|| repaired.get("parameters"))
        .or_else(|| repaired.get("input"))
        .cloned()
        .unwrap_or_else(|| json!({}));
    // Certains modèles mettent les arguments en chaîne JSON.
    let arguments = match arguments {
        Value::String(text) => serde_json::from_str(&text).unwrap_or(Value::String(text)),
        other => other,
    };
    Some((name, arguments))
}

fn fenced_call(text: &str) -> Option<(String, Value)> {
    let start = text.find("```")?;
    let after = &text[start + 3..];
    let body_start = after.find('\n')? + 1;
    let body = &after[body_start..];
    let end = body.find("```")?;
    let call = parse_call(&body[..end])?;
    // Seulement si l'objet ressemble vraiment à un appel d'outil.
    lenient_json(body[..end].trim())
        .filter(|value| value.get("arguments").is_some() || value.get("parameters").is_some())
        .map(|_| call)
}

/// JSON tolérant : accolades autour, virgules finales… (réparation minimale).
fn lenient_json(text: &str) -> Option<Value> {
    if let Ok(value) = serde_json::from_str::<Value>(text) {
        return Some(value);
    }
    let start = text.find('{')?;
    let end = text.rfind('}')?;
    serde_json::from_str(&text[start..=end]).ok()
}

/// Filtre l'affichage en continu : le texte passe, sauf à partir d'un bloc
/// `<tool_call>` (y compris un début de balise en fin de morceau).
#[derive(Debug, Default)]
pub struct TextToolFilter {
    raw: String,
    emitted: usize,
    hidden: bool,
}

impl TextToolFilter {
    /// Ajoute un morceau ; renvoie la partie affichable.
    pub fn push(&mut self, delta: &str) -> String {
        self.raw.push_str(delta);
        if self.hidden {
            return String::new();
        }
        let pending = &self.raw[self.emitted..];
        if let Some(position) = pending.find(OPEN_TAG) {
            self.hidden = true;
            let visible = pending[..position].to_string();
            self.emitted = self.raw.len();
            return visible;
        }
        // Garde en réserve une éventuelle balise coupée (`<tool_ca`).
        let keep = (1..OPEN_TAG.len())
            .rev()
            .find(|length| pending.ends_with(&OPEN_TAG[..*length]))
            .unwrap_or(0);
        let cut = pending.len() - keep;
        let visible = pending[..cut].to_string();
        self.emitted += cut;
        visible
    }

    /// Fin du flux : reste affichable et texte brut complet.
    pub fn finish(&mut self) -> (String, String) {
        let rest = if self.hidden {
            String::new()
        } else {
            self.raw[self.emitted..].to_string()
        };
        self.emitted = self.raw.len();
        (rest, std::mem::take(&mut self.raw))
    }
}

#[cfg(test)]
mod tests {
    use super::{extract_tool_calls, is_tools_unsupported_error, TextToolFilter};
    use serde_json::json;

    #[test]
    fn extracts_hermes_style_calls_and_keeps_visible_text() {
        let text = "Je lis le fichier.\n<tool_call>\n{\"name\": \"read_file\", \"arguments\": {\"path\": \"src/lib.rs\"}}\n</tool_call>\n<tool_call>{\"name\":\"bash\",\"arguments\":\"{\\\"command\\\":\\\"ls\\\"}\"}</tool_call>";
        let (visible, calls) = extract_tool_calls(text);
        assert_eq!(visible, "Je lis le fichier.");
        assert_eq!(calls.len(), 2);
        assert_eq!(
            calls[0],
            ("read_file".to_string(), json!({"path": "src/lib.rs"}))
        );
        assert_eq!(calls[1], ("bash".to_string(), json!({"command": "ls"})));
    }

    #[test]
    fn accepts_unclosed_tags_and_fenced_json() {
        let (_, calls) = extract_tool_calls(
            "<tool_call>{\"name\": \"glob_search\", \"arguments\": {\"pattern\": \"*.rs\"}}",
        );
        assert_eq!(calls[0].0, "glob_search");
        let (visible, calls) = extract_tool_calls(
            "```json\n{\"name\": \"bash\", \"parameters\": {\"command\": \"pwd\"}}\n```",
        );
        assert!(visible.is_empty());
        assert_eq!(calls[0], ("bash".to_string(), json!({"command": "pwd"})));
        let (visible, calls) = extract_tool_calls("Voici un exemple :\n```json\n{\"a\": 1}\n```");
        assert!(calls.is_empty());
        assert!(visible.contains("exemple"));
    }

    #[test]
    fn stream_filter_hides_tool_blocks_even_when_split() {
        let mut filter = TextToolFilter::default();
        let mut shown = String::new();
        for piece in [
            "Analyse en cours",
            " <tool_",
            "call>{\"name\":\"bash\"",
            "}</tool_call>",
        ] {
            shown.push_str(&filter.push(piece));
        }
        let (rest, raw) = filter.finish();
        shown.push_str(&rest);
        assert_eq!(shown, "Analyse en cours ");
        assert!(raw.contains("<tool_call>"));
        let mut plain = TextToolFilter::default();
        assert_eq!(plain.push("a < b"), "a < b");
        assert_eq!(plain.push(" et <"), " et ");
        assert_eq!(plain.finish().0, "<");
    }

    #[test]
    fn recognizes_servers_refusing_tools() {
        assert!(is_tools_unsupported_error(
            "registry.ollama.ai/library/gemma2:2b does not support tools"
        ));
        assert!(is_tools_unsupported_error(
            "No endpoints found that support tool use"
        ));
        assert!(!is_tools_unsupported_error("rate limit exceeded"));
    }
}
