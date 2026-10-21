//! Réparation des arguments d'appels d'outils mal formés, fréquents avec les
//! petits modèles : bloc de code Markdown autour du JSON, texte avant l'objet,
//! virgules finales, retours à la ligne bruts dans les chaînes, `True`/`None`
//! à la Python, guillemets simples, clés sans guillemets, barre oblique
//! inverse seule (`"\d+"`, chemin Windows), accolades ou guillemets non
//! refermés (sortie tronquée).

use serde_json::Value;

/// Arguments qui nomment un fichier : un caractère de contrôle n'y a pas sa
/// place, c'est un chemin Windows dont les barres ont été lues comme des
/// échappements (`"C:\temp\new"` donne une tabulation et un saut de ligne).
const PATH_ARGUMENTS: [&str; 3] = ["path", "file_path", "notebook_path"];

/// Renvoie un JSON valide (objet) équivalent à `input`, ou `None` si rien de
/// raisonnable n'a pu être reconstruit. Une entrée déjà valide est rendue telle
/// quelle, sauf un chemin aux barres lues comme des échappements.
#[must_use]
pub fn repair_tool_input(input: &str) -> Option<String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Some("{}".to_string());
    }
    if let Ok(mut value) = serde_json::from_str::<Value>(trimmed) {
        if value.is_object() {
            return Some(if restore_path_escapes(&mut value) {
                value.to_string()
            } else {
                trimmed.to_string()
            });
        }
    }
    let unfenced = strip_code_fence(trimmed);
    let candidate = match unfenced.find('{') {
        Some(start) => &unfenced[start..],
        None => return None,
    };
    let repaired = scan_and_fix(candidate);
    match serde_json::from_str::<Value>(&repaired) {
        Ok(mut value) if value.is_object() => {
            restore_path_escapes(&mut value);
            Some(value.to_string())
        }
        _ => None,
    }
}

/// Remet dans un argument de chemin la séquence d'origine de chaque caractère
/// de contrôle (tabulation vers `\t`...). Vrai si quelque chose a changé.
fn restore_path_escapes(value: &mut Value) -> bool {
    let Some(object) = value.as_object_mut() else {
        return false;
    };
    let mut changed = false;
    for key in PATH_ARGUMENTS {
        let Some(Value::String(path)) = object.get_mut(key) else {
            continue;
        };
        if !path.chars().any(char::is_control) {
            continue;
        }
        let mut restored = String::with_capacity(path.len() + 4);
        for ch in path.chars() {
            match ch {
                '\t' => restored.push_str("\\t"),
                '\n' => restored.push_str("\\n"),
                '\r' => restored.push_str("\\r"),
                '\u{8}' => restored.push_str("\\b"),
                '\u{c}' => restored.push_str("\\f"),
                ch if ch.is_control() => {}
                ch => restored.push(ch),
            }
        }
        *path = restored;
        changed = true;
    }
    changed
}

fn strip_code_fence(text: &str) -> &str {
    let Some(rest) = text.strip_prefix("```") else {
        return text;
    };
    let rest = rest.split_once('\n').map_or(rest, |(_, body)| body);
    rest.trim_end().strip_suffix("```").unwrap_or(rest).trim()
}

/// Parcourt le texte en suivant chaînes et imbrications : échappe les
/// caractères de contrôle dans les chaînes, double une barre oblique inverse
/// qui n'ouvre pas un échappement JSON, lit une chaîne entre guillemets
/// simples et une clé sans guillemets, retire les virgules finales, convertit
/// les littéraux Python, s'arrête à la fin de l'objet racine et referme ce qui
/// reste ouvert.
fn scan_and_fix(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 8);
    let mut stack: Vec<char> = Vec::new();
    // Guillemet qui a ouvert la chaîne en cours (`"` ou `'`).
    let mut quote: Option<char> = None;
    let chars: Vec<char> = text.chars().collect();
    let mut index = 0;
    while index < chars.len() {
        let c = chars[index];
        if let Some(open) = quote {
            match c {
                '\\' => {
                    index += push_escape(&chars, index, open, &mut out);
                    continue;
                }
                c if c == open => {
                    out.push('"');
                    quote = None;
                }
                '"' => out.push_str("\\\""),
                '\n' => out.push_str("\\n"),
                '\r' => out.push_str("\\r"),
                '\t' => out.push_str("\\t"),
                c if c.is_control() => {}
                c => out.push(c),
            }
            index += 1;
            continue;
        }
        match c {
            '"' | '\'' => {
                quote = Some(c);
                out.push('"');
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
            c if (c.is_alphabetic() || c == '_') && stack.last() == Some(&'}') => {
                if let Some(end) = bare_key_end(&chars, index, &out) {
                    out.push('"');
                    out.extend(&chars[index..end]);
                    out.push('"');
                    index = end;
                    continue;
                }
                index += push_word(&chars, index, &mut out);
                continue;
            }
            c if c.is_alphabetic() => {
                index += push_word(&chars, index, &mut out);
                continue;
            }
            c => out.push(c),
        }
        index += 1;
    }
    // Sortie tronquée : on referme la chaîne puis les imbrications.
    if quote.is_some() {
        out.push('"');
    }
    remove_trailing_comma(&mut out);
    while let Some(closer) = stack.pop() {
        remove_dangling_key(&mut out);
        out.push(closer);
    }
    out
}

/// Écrit l'échappement qui commence à `chars[index]` (une barre oblique
/// inverse) dans une chaîne ouverte par `open`, et rend le nombre de
/// caractères lus. Un échappement JSON est gardé ; `\'` d'une chaîne entre
/// guillemets simples devient `'` ; toute autre barre (`\d`, `D:\Mes_Projets`)
/// est doublée, pour rester le caractère que le modèle a écrit. Une barre en
/// fin de texte tronqué est retirée.
fn push_escape(chars: &[char], index: usize, open: char, out: &mut String) -> usize {
    let Some(&next) = chars.get(index + 1) else {
        return 1;
    };
    match next {
        '\'' if open == '\'' => {
            out.push('\'');
            2
        }
        '"' | '\\' | '/' | 'b' | 'f' | 'n' | 'r' | 't' => {
            out.push('\\');
            out.push(next);
            2
        }
        'u' if chars
            .get(index + 2..index + 6)
            .is_some_and(|hex| hex.iter().all(char::is_ascii_hexdigit)) =>
        {
            out.extend(&chars[index..index + 6]);
            6
        }
        _ => {
            out.push_str("\\\\");
            1
        }
    }
}

/// Fin d'une clé sans guillemets (`{path: "a.py"}`) qui commence à
/// `chars[index]` : un nom juste après `{` ou `,`, suivi de `:`.
fn bare_key_end(chars: &[char], index: usize, out: &str) -> Option<usize> {
    if !matches!(out.trim_end().chars().last(), Some('{' | ',')) {
        return None;
    }
    let end = index
        + chars[index..]
            .iter()
            .take_while(|ch| ch.is_alphanumeric() || **ch == '_' || **ch == '-')
            .count();
    let after = chars[end..].iter().find(|ch| !ch.is_whitespace());
    (after == Some(&':')).then_some(end)
}

/// Écrit le mot qui commence à `chars[index]`, `True`, `False` et `None` à la
/// Python devenant `true`, `false` et `null`, et rend sa longueur.
fn push_word(chars: &[char], index: usize, out: &mut String) -> usize {
    let length = chars[index..]
        .iter()
        .take_while(|ch| ch.is_alphanumeric() || **ch == '_')
        .count()
        .max(1);
    let word: String = chars[index..index + length].iter().collect();
    out.push_str(match word.as_str() {
        "True" => "true",
        "False" => "false",
        "None" => "null",
        _ => &word,
    });
    length
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
    fn repairs_python_quotes_bare_keys_and_lone_backslashes() {
        assert_eq!(
            parsed(r#"{'path': 'a.py', 'content': 'dit "oui"'}"#),
            json!({"path": "a.py", "content": "dit \"oui\""})
        );
        assert_eq!(parsed(r"{'text': 'l\'eau'}"), json!({"text": "l'eau"}));
        assert_eq!(
            parsed(r#"{path: "a.py", old_str: "x", new-str: "y"}"#),
            json!({"path": "a.py", "old_str": "x", "new-str": "y"})
        );
        assert_eq!(
            parsed(r#"{"pattern": "\d+\."}"#),
            json!({"pattern": r"\d+\."})
        );
        assert_eq!(
            parsed(r#"{"path": "D:\Mes_Projets\app.py", "all": True}"#),
            json!({"path": r"D:\Mes_Projets\app.py", "all": true})
        );
        assert_eq!(
            parsed(r#"{"command": "echo \u00e9 \"ok\"", "x": 1,}"#),
            json!({"command": "echo é \"ok\"", "x": 1})
        );
        // Les apostrophes d'une chaîne entre guillemets doubles restent du texte.
        assert_eq!(
            parsed(r#"{"text": "l'eau, l'air",}"#),
            json!({"text": "l'eau, l'air"})
        );
    }

    #[test]
    fn restores_windows_paths_read_as_escapes() {
        // JSON valide, mais `\t`, `\n` et `\f` y sont lus comme des caractères de contrôle.
        assert_eq!(
            parsed(r#"{"path": "C:\temp\new\file.txt"}"#),
            json!({"path": r"C:\temp\new\file.txt"})
        );
        // Ailleurs qu'un chemin, une tabulation reste une tabulation.
        assert_eq!(
            repair_tool_input(r#"{"content": "a\tb"}"#).as_deref(),
            Some(r#"{"content": "a\tb"}"#)
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
