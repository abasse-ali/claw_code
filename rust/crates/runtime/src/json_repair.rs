//! Réparation des arguments d'appels d'outils mal formés, fréquents avec les
//! petits modèles : bloc de code Markdown autour du JSON, texte avant l'objet,
//! virgules finales, retours à la ligne bruts dans les chaînes, `True`/`None`
//! à la Python, accolades ou guillemets non refermés (sortie tronquée).

use serde_json::Value;

/// Renvoie un JSON valide (objet) équivalent à `input`, ou `None` si rien de
/// raisonnable n'a pu être reconstruit. Une entrée déjà valide est rendue telle quelle.
#[must_use]
pub fn repair_tool_input(input: &str) -> Option<String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Some("{}".to_string());
    }
    if serde_json::from_str::<Value>(trimmed).is_ok_and(|value| value.is_object()) {
        return Some(trimmed.to_string());
    }
    let unfenced = strip_code_fence(trimmed);
    let candidate = match unfenced.find('{') {
        Some(start) => &unfenced[start..],
        None => return None,
    };
    let repaired = scan_and_fix(candidate);
    match serde_json::from_str::<Value>(&repaired) {
        Ok(value) if value.is_object() => Some(value.to_string()),
        _ => None,
    }
}

fn strip_code_fence(text: &str) -> &str {
    let Some(rest) = text.strip_prefix("```") else {
        return text;
    };
    let rest = rest.split_once('\n').map_or(rest, |(_, body)| body);
    rest.trim_end().strip_suffix("```").unwrap_or(rest).trim()
}

/// Parcourt le texte en suivant chaînes et imbrications : échappe les
/// caractères de contrôle dans les chaînes, retire les virgules finales,
/// convertit les littéraux Python, s'arrête à la fin de l'objet racine et
/// referme ce qui reste ouvert.
fn scan_and_fix(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 8);
    let mut stack: Vec<char> = Vec::new();
    let mut in_string = false;
    let mut escaped = false;
    let chars: Vec<char> = text.chars().collect();
    let mut index = 0;
    while index < chars.len() {
        let c = chars[index];
        if in_string {
            if escaped {
                out.push(c);
                escaped = false;
            } else {
                match c {
                    '\\' => {
                        out.push(c);
                        escaped = true;
                    }
                    '"' => {
                        out.push(c);
                        in_string = false;
                    }
                    '\n' => out.push_str("\\n"),
                    '\r' => out.push_str("\\r"),
                    '\t' => out.push_str("\\t"),
                    c if c.is_control() => {}
                    c => out.push(c),
                }
            }
            index += 1;
            continue;
        }
        match c {
            '"' => {
                in_string = true;
                out.push(c);
            }
            '{' | '[' => {
                stack.push(if c == '{' { '}' } else { ']' });
                out.push(c);
            }
            '}' | ']' => {
                remove_trailing_comma(&mut out);
                if stack.last() == Some(&c) {
                    stack.pop();
                    out.push(c);
                    if stack.is_empty() {
                        // Fin de l'objet racine : le texte qui suit est ignoré.
                        return out;
                    }
                }
            }
            _ => {
                let rest: String = chars[index..chars.len().min(index + 5)].iter().collect();
                if let Some((word, replacement)) =
                    [("True", "true"), ("False", "false"), ("None", "null")]
                        .into_iter()
                        .find(|(word, _)| rest.starts_with(word))
                {
                    out.push_str(replacement);
                    index += word.len();
                    continue;
                }
                out.push(c);
            }
        }
        index += 1;
    }
    // Sortie tronquée : on referme la chaîne puis les imbrications.
    if in_string {
        if escaped {
            out.pop();
        }
        out.push('"');
    }
    remove_trailing_comma(&mut out);
    while let Some(closer) = stack.pop() {
        remove_dangling_key(&mut out);
        out.push(closer);
    }
    out
}

fn remove_trailing_comma(out: &mut String) {
    let trimmed_len = out.trim_end().len();
    if out[..trimmed_len].ends_with(',') {
        out.truncate(trimmed_len - 1);
    }
}

/// `{"a": 1, "b"` ou `{"a": 1, "b":` → `{"a": 1`.
fn remove_dangling_key(out: &mut String) {
    let trimmed = out.trim_end();
    let without_colon = trimmed.strip_suffix(':').map_or(trimmed, str::trim_end);
    if let Some(body) = without_colon.strip_suffix('"') {
        // Cherche le guillemet ouvrant de cette clé et ce qui le précède.
        if let Some(open) = body.rfind('"') {
            let before = body[..open].trim_end();
            if before.ends_with(',') || before.ends_with('{') {
                let keep = if before.ends_with(',') {
                    before.len() - 1
                } else {
                    before.len()
                };
                // Seulement si c'était bien une clé (suivie de « : » ou de rien).
                if trimmed.ends_with(':') || before.ends_with(',') || before.ends_with('{') {
                    out.truncate(keep);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::repair_tool_input;
    use serde_json::{json, Value};

    fn parsed(input: &str) -> Value {
        serde_json::from_str(&repair_tool_input(input).expect("réparable")).expect("json")
    }

    #[test]
    fn keeps_valid_json_and_fills_empty_input() {
        assert_eq!(
            repair_tool_input(r#"{"path":"a.rs"}"#).as_deref(),
            Some(r#"{"path":"a.rs"}"#)
        );
        assert_eq!(repair_tool_input("  ").as_deref(), Some("{}"));
    }

    #[test]
    fn repairs_common_model_mistakes() {
        assert_eq!(
            parsed("```json\n{\"path\": \"a.rs\"}\n```"),
            json!({"path": "a.rs"})
        );
        assert_eq!(
            parsed("Voici l'appel : {\"path\": \"a.rs\"} merci"),
            json!({"path": "a.rs"})
        );
        assert_eq!(
            parsed(r#"{"paths": ["a", "b",], "x": 1,}"#),
            json!({"paths": ["a", "b"], "x": 1})
        );
        assert_eq!(
            parsed("{\"content\": \"ligne 1\nligne 2\tfin\"}"),
            json!({"content": "ligne 1\nligne 2\tfin"})
        );
        assert_eq!(
            parsed(r#"{"all": True, "n": None}"#),
            json!({"all": true, "n": null})
        );
        assert_eq!(
            parsed(r#"{"text": "True story"}"#),
            json!({"text": "True story"})
        );
    }

    #[test]
    fn closes_truncated_output() {
        assert_eq!(
            parsed(r#"{"command": "cargo test"#),
            json!({"command": "cargo test"})
        );
        assert_eq!(parsed(r#"{"a": [1, 2"#), json!({"a": [1, 2]}));
        assert_eq!(parsed(r#"{"a": 1, "b": "#), json!({"a": 1}));
        assert!(repair_tool_input("pas de json ici").is_none());
    }
}
