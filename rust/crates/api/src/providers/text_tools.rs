//! Appels d'outils « en texte » pour les modèles ou serveurs sans appel de
//! fonctions natif (beaucoup de modèles locaux, llama.cpp sans `--jinja`…).
//!
//! Les outils sont décrits dans le prompt système et le modèle répond avec des
//! blocs `<tool_call>{"name": …, "arguments": {…}}</tool_call>` (format Hermes,
//! connu de Qwen, Mistral, Llama…), que l'on retransforme en appels d'outils.

use std::collections::BTreeSet;
use std::fmt::Write as _;
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
        // vLLM lancé sans `--enable-auto-tool-choice --tool-call-parser`.
        "tool choice requires",
        "--enable-auto-tool-choice",
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
        let _ = writeln!(
            prompt,
            "- {}: {}\n  parameters: {}",
            tool.name,
            tool.description.as_deref().unwrap_or("").replace('\n', " "),
            compact_schema(&tool.input_schema)
        );
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

/// Appels écrits en JSON brut au lieu d'outils natifs (qwen2.5-coder sous
/// Ollama répond `{"name": "read_file", "arguments": {…}}` en texte). Retenu
/// seulement si toute la réponse est faite de tels objets (éventuellement dans
/// un bloc de code `json`) et qu'ils nomment des outils proposés.
#[must_use]
pub fn salvage_json_tool_calls(text: &str, known_tools: &[String]) -> Option<Vec<(String, Value)>> {
    let mut body = text.trim();
    if let Some(fenced) = body.strip_prefix("```") {
        body = fenced
            .split_once('\n')?
            .1
            .trim_end()
            .strip_suffix("```")?
            .trim();
    }
    if !body.starts_with('{') {
        return None;
    }
    let mut calls = Vec::new();
    for value in serde_json::Deserializer::from_str(body).into_iter::<Value>() {
        let value = value.ok()?;
        let name = value.get("name")?.as_str()?;
        if !known_tools.iter().any(|tool| tool == name) {
            return None;
        }
        let arguments = match value
            .get("arguments")
            .or_else(|| value.get("parameters"))?
            .clone()
        {
            Value::String(text) => serde_json::from_str(&text).unwrap_or(Value::String(text)),
            other => other,
        };
        calls.push((name.to_string(), arguments));
    }
    (!calls.is_empty()).then_some(calls)
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

const INVOKE_TAG: &str = "<invoke name=\"";
const INVOKE_CLOSE: &str = "</invoke>";
const PARAMETER_TAG: &str = "<parameter name=\"";
const PARAMETER_CLOSE: &str = "</parameter>";

/// Appels écrits à la manière de Claude, `<invoke name="outil"><parameter
/// name="p">valeur</parameter></invoke>`, dans une balise d'enveloppe
/// quelconque (`<function_calls>`, `<dots_function_call>`…) : certains modèles
/// les écrivent dans leur texte, voire dans leur raisonnement, au lieu d'un
/// appel natif. Retenus seulement s'ils terminent le texte, hors bloc de
/// code, complets, et s'ils nomment des outils proposés. Renvoie la position
/// où ils commencent (balise d'enveloppe comprise) et les appels.
#[must_use]
pub fn salvage_invoke_calls(
    text: &str,
    tools: &[ToolDefinition],
) -> Option<(usize, Vec<(String, Value)>)> {
    let mut from = 0;
    while let Some(position) = invoke_outside_code(text, from) {
        if let Some(calls) = invoke_run(&text[position..], tools) {
            return Some((wrapper_start(&text[..position]), calls));
        }
        from = position + INVOKE_TAG.len();
    }
    None
}

/// Premier `<invoke name="` à partir de `from` qui n'est ni dans un bloc de
/// code ni dans du code en ligne (un exemple n'est pas un appel).
fn invoke_outside_code(text: &str, from: usize) -> Option<usize> {
    let mut from = from;
    loop {
        let position = from + text.get(from..)?.find(INVOKE_TAG)?;
        let before = &text[..position];
        if before.matches("```").count().is_multiple_of(2) && !before.ends_with('`') {
            return Some(position);
        }
        from = position + INVOKE_TAG.len();
    }
}

/// Suite d'appels jusqu'à la fin du texte, séparés seulement par des blancs
/// et des balises d'enveloppe.
fn invoke_run(text: &str, tools: &[ToolDefinition]) -> Option<Vec<(String, Value)>> {
    let mut calls = Vec::new();
    let mut rest = text;
    loop {
        rest = skip_wrapper_tags(rest);
        // Un modèle qui ne s'arrête pas invente ensuite le résultat : il est écarté.
        let invented = ["<function_results", "<function_result", "<tool_result"]
            .iter()
            .any(|tag| rest.starts_with(tag));
        if rest.is_empty() || (invented && !calls.is_empty()) {
            return (!calls.is_empty()).then_some(calls);
        }
        let (name, after) = rest.strip_prefix(INVOKE_TAG)?.split_once("\">")?;
        let tool = tools.iter().find(|tool| tool.name == name)?;
        let end = after.find(INVOKE_CLOSE)?;
        calls.push((
            name.to_string(),
            invoke_arguments(&after[..end], &tool.input_schema)?,
        ));
        rest = &after[end + INVOKE_CLOSE.len()..];
    }
}

fn invoke_arguments(body: &str, schema: &Value) -> Option<Value> {
    let mut arguments = serde_json::Map::new();
    let mut rest = body.trim_start();
    while !rest.is_empty() {
        let (name, after) = rest.strip_prefix(PARAMETER_TAG)?.split_once("\">")?;
        let end = after.find(PARAMETER_CLOSE)?;
        arguments.insert(
            name.to_string(),
            parameter_value(schema, name, &after[..end]),
        );
        rest = after[end + PARAMETER_CLOSE.len()..].trim_start();
    }
    Some(Value::Object(arguments))
}

/// Valeur d'un paramètre écrit en texte. Le retour à la ligne qui suit la
/// balise ouvrante et celui qui précède la balise fermante sont de la mise en
/// forme ; le schéma de l'outil dit si la valeur se lit comme du JSON
/// (nombre, booléen, liste, objet) ou reste du texte.
fn parameter_value(schema: &Value, name: &str, raw: &str) -> Value {
    let value = raw
        .strip_prefix("\r\n")
        .or_else(|| raw.strip_prefix('\n'))
        .unwrap_or(raw);
    let value = value
        .strip_suffix("\r\n")
        .or_else(|| value.strip_suffix('\n'))
        .unwrap_or(value);
    let kind = schema
        .get("properties")
        .and_then(|properties| properties.get(name))
        .and_then(|property| property.get("type"));
    let textual = match kind {
        Some(Value::String(kind)) => kind == "string",
        Some(Value::Array(kinds)) => kinds.iter().any(|kind| kind == "string"),
        _ => false,
    };
    if textual {
        return Value::String(value.to_string());
    }
    serde_json::from_str(value.trim()).unwrap_or_else(|_| Value::String(value.to_string()))
}

/// `<function_calls>`, `</dots_function_call>`… : balise sans attribut,
/// ouvrante ou fermante, qui n'est ni celle d'un appel ou d'un paramètre,
/// ni celle d'un résultat (que le modèle inventerait).
fn is_wrapper_tag(tag: &str) -> bool {
    let Some(inner) = tag.strip_prefix('<').and_then(|tag| tag.strip_suffix('>')) else {
        return false;
    };
    let name = inner.strip_prefix('/').unwrap_or(inner);
    name.chars()
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic() || first == '_')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | ':' | '.'))
        && !matches!(name, "invoke" | "parameter")
        && !name.starts_with("function_result")
        && !name.starts_with("tool_result")
}

fn skip_wrapper_tags(text: &str) -> &str {
    let mut rest = text;
    loop {
        rest = rest.trim_start();
        match rest.find('>') {
            Some(end) if is_wrapper_tag(&rest[..=end]) => rest = &rest[end + 1..],
            _ => return rest,
        }
    }
}

/// Début de la balise d'enveloppe ouvrante qui termine `before` (blancs
/// compris), sinon fin du texte qui précède les blancs.
fn wrapper_start(before: &str) -> usize {
    let trimmed = before.trim_end();
    match trimmed.rfind('<') {
        Some(open) if !trimmed[open..].starts_with("</") && is_wrapper_tag(&trimmed[open..]) => {
            open
        }
        _ => trimmed.len(),
    }
}

/// Fin de morceau gardée en réserve : un début d'appel coupé (`<invoke na`),
/// une balise en cours d'écriture (`<function_ca`), ou une balise d'enveloppe
/// complète qui peut précéder un appel.
fn invoke_reserve(pending: &str) -> usize {
    let partial = (1..INVOKE_TAG.len())
        .rev()
        .find(|length| pending.ends_with(&INVOKE_TAG[..*length]))
        .unwrap_or(0);
    let before = &pending[..pending.len() - partial];
    if partial == 0 {
        if let Some(open) = before.rfind('<') {
            let tail = &before[open + 1..];
            if tail
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | ':' | '.' | '/'))
            {
                return pending.len() - open;
            }
        }
    }
    let start = wrapper_start(before);
    if start < before.trim_end().len() {
        return pending.len() - start;
    }
    partial
}

/// Affichage en continu, en mode natif : le texte passe, sauf à partir d'un
/// appel écrit en XML (`<invoke name="…">`) et de sa balise d'enveloppe,
/// retenus jusqu'à la fin de la réponse, où l'on saura si c'était un appel.
#[derive(Debug, Default)]
pub struct InvokeFilter {
    raw: String,
    emitted: usize,
    held_from: Option<usize>,
}

impl InvokeFilter {
    /// Ajoute un morceau ; renvoie la partie affichable.
    pub fn push(&mut self, delta: &str) -> String {
        self.raw.push_str(delta);
        if self.held_from.is_some() {
            return String::new();
        }
        if let Some(position) = invoke_outside_code(&self.raw, self.emitted) {
            let start = self.emitted + wrapper_start(&self.raw[self.emitted..position]);
            let visible = self.raw[self.emitted..start].to_string();
            self.held_from = Some(start);
            self.emitted = self.raw.len();
            return visible;
        }
        let pending = &self.raw[self.emitted..];
        let cut = pending.len() - invoke_reserve(pending);
        let visible = pending[..cut].to_string();
        self.emitted += cut;
        visible
    }

    /// Fin du flux : texte complet et position de ce qui n'a pas été affiché.
    pub fn finish(&mut self) -> (String, usize) {
        let held_from = self.held_from.take().unwrap_or(self.emitted);
        self.emitted = 0;
        (std::mem::take(&mut self.raw), held_from)
    }
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
    use super::{
        extract_tool_calls, is_tools_unsupported_error, salvage_invoke_calls,
        salvage_json_tool_calls, InvokeFilter, TextToolFilter,
    };
    use crate::types::ToolDefinition;
    use serde_json::json;

    fn invoke_tools() -> Vec<ToolDefinition> {
        vec![
            ToolDefinition {
                name: "edit_file".to_string(),
                description: None,
                input_schema: json!({"type": "object", "properties": {
                    "path": {"type": "string"},
                    "new_string": {"type": "string"},
                    "replace_all": {"type": "boolean"}
                }}),
            },
            ToolDefinition {
                name: "bash".to_string(),
                description: None,
                input_schema: json!({"type": "object", "properties": {
                    "command": {"type": "string"},
                    "timeout": {"type": "integer"}
                }}),
            },
        ]
    }

    #[test]
    fn salvages_claude_style_calls_written_in_text_or_reasoning() {
        let tools = invoke_tools();
        // Comme le modèle Dots dans sa réflexion : enveloppe qui lui est propre,
        // valeurs entourées de retours à la ligne, booléen écrit en texte.
        let reasoning = "<dots_function_call>\n<invoke name=\"edit_file\">\n<parameter name=\"new_string\">\n    return 0\n</parameter>\n<parameter name=\"path\">\nbackend/app/main.py\n</parameter>\n<parameter name=\"replace_all\">\ntrue\n</parameter>\n</invoke>\n</dots_function_call>";
        let (start, calls) = salvage_invoke_calls(reasoning, &tools).expect("appel récupéré");
        assert_eq!(start, 0);
        assert_eq!(
            calls,
            vec![(
                "edit_file".to_string(),
                json!({"new_string": "    return 0", "path": "backend/app/main.py", "replace_all": true})
            )]
        );

        // Texte avant, deux appels, entier lu comme un nombre, résultat inventé écarté.
        let text = "Je vérifie.\n<function_calls>\n<invoke name=\"bash\"><parameter name=\"command\">ls</parameter><parameter name=\"timeout\">5000</parameter></invoke>\n<invoke name=\"bash\"><parameter name=\"command\">pwd</parameter></invoke>\n</function_calls>\n<function_results>src</function_results>\nIl y a src.";
        let (start, calls) = salvage_invoke_calls(text, &tools).expect("deux appels");
        assert_eq!(&text[..start], "Je vérifie.\n");
        assert_eq!(
            calls,
            vec![
                (
                    "bash".to_string(),
                    json!({"command": "ls", "timeout": 5000})
                ),
                ("bash".to_string(), json!({"command": "pwd"})),
            ]
        );
    }

    #[test]
    fn examples_unknown_tools_and_unfinished_calls_are_not_salvaged() {
        let tools = invoke_tools();
        let fenced = "Le format est :\n```xml\n<invoke name=\"bash\"><parameter name=\"command\">rm -rf build</parameter></invoke>\n```";
        assert!(salvage_invoke_calls(fenced, &tools).is_none());
        let inline =
            "Écrivez `<invoke name=\"bash\"><parameter name=\"command\">ls</parameter></invoke>`";
        assert!(salvage_invoke_calls(inline, &tools).is_none());
        let unknown = "<invoke name=\"rm_rf\"><parameter name=\"path\">/</parameter></invoke>";
        assert!(salvage_invoke_calls(unknown, &tools).is_none());
        // Coupé par la limite de sortie : un fichier tronqué ne doit pas être écrit.
        let cut = "<invoke name=\"edit_file\"><parameter name=\"path\">a.py</parameter><parameter name=\"new_string\">def f(";
        assert!(salvage_invoke_calls(cut, &tools).is_none());
        // Suivi d'un texte : ce n'est pas la fin de la réponse, donc pas un appel.
        let followed = "<invoke name=\"bash\"><parameter name=\"command\">ls</parameter></invoke>\nEnsuite je lirai le fichier.";
        assert!(salvage_invoke_calls(followed, &tools).is_none());
    }

    #[test]
    fn invoke_filter_holds_back_a_call_and_its_wrapper_even_when_split() {
        let mut filter = InvokeFilter::default();
        let mut shown = String::new();
        for piece in [
            "Je lis le fichier.\n<func",
            "tion_calls>\n<inv",
            "oke name=\"read_file\">",
            "<parameter name=\"path\">a</parameter></invoke>\n</function_calls>",
        ] {
            shown.push_str(&filter.push(piece));
        }
        let (raw, held_from) = filter.finish();
        assert_eq!(shown.trim_end(), "Je lis le fichier.");
        assert!(raw[held_from..].starts_with("<function_calls>\n<invoke"));

        // Du code ordinaire passe en entier, au plus un morceau plus tard.
        let mut plain = InvokeFilter::default();
        let mut shown = String::new();
        for piece in ["let v: Vec<", "String> = Vec::new();", " if a < b {"] {
            shown.push_str(&plain.push(piece));
        }
        let (raw, held_from) = plain.finish();
        shown.push_str(&raw[held_from..]);
        assert_eq!(shown, "let v: Vec<String> = Vec::new(); if a < b {");
    }

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
        assert!(is_tools_unsupported_error(
            "\"auto\" tool choice requires --enable-auto-tool-choice and --tool-call-parser to be set"
        ));
        assert!(!is_tools_unsupported_error("rate limit exceeded"));
    }

    #[test]
    fn salvages_raw_json_calls_naming_known_tools_only() {
        let tools = vec!["read_file".to_string(), "bash".to_string()];
        let calls = salvage_json_tool_calls(
            "{\"name\": \"read_file\", \"arguments\": {\"path\": \"stats.py\"}}",
            &tools,
        )
        .expect("appel récupéré");
        assert_eq!(
            calls,
            vec![("read_file".to_string(), json!({"path": "stats.py"}))]
        );

        let several = "```json
{\"name\": \"bash\", \"arguments\": \"{\\\"command\\\": \\\"ls\\\"}\"}
{\"name\": \"read_file\", \"parameters\": {\"path\": \"a\"}}
```";
        let calls = salvage_json_tool_calls(several, &tools).expect("deux appels");
        assert_eq!(calls[0], ("bash".to_string(), json!({"command": "ls"})));
        assert_eq!(calls[1].0, "read_file");

        // Outil inconnu, texte autour, JSON sans arguments : pas un appel.
        assert!(
            salvage_json_tool_calls("{\"name\": \"rm_rf\", \"arguments\": {}}", &tools).is_none()
        );
        assert!(
            salvage_json_tool_calls("Voici : {\"name\": \"bash\", \"arguments\": {}}", &tools)
                .is_none()
        );
        assert!(salvage_json_tool_calls("{\"name\": \"bash\"}", &tools).is_none());
        assert!(salvage_json_tool_calls("{\"nom\": \"Dupont\", \"age\": 3}", &tools).is_none());
    }
}
