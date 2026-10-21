//! Résultats d'outils tels que le modèle les relit dans la suite de la session.
//!
//! Un outil rend tout ce que l'interface affiche : l'ancien et le nouveau
//! texte d'une modification, son diff ligne à ligne, le contenu du fichier
//! écrit, l'état du bac à sable d'une commande. Le modèle, lui, sait déjà ce
//! qu'il vient d'écrire. Dans une vraie session, ces redites occupaient 45 % du
//! contexte renvoyé à chaque requête : elles sont retirées de ce qui est gardé
//! dans la session, l'interface ayant déjà reçu le résultat complet.

use serde_json::{json, Map, Value};

/// Le résultat de `tool_name` réduit à ce qui sert au modèle, ou `None` s'il
/// n'y a rien à retirer. Ce qui suit le JSON du résultat (remarques de Claw,
/// messages de hooks) est gardé tel quel.
#[must_use]
pub fn lean_tool_output(tool_name: &str, output: &str) -> Option<String> {
    let mut values = serde_json::Deserializer::from_str(output).into_iter::<Value>();
    let Some(Ok(Value::Object(fields))) = values.next() else {
        return None;
    };
    let rest = &output[values.byte_offset()..];
    let lean = match tool_name {
        "edit_file" | "MultiEdit" => lean_edit(&fields)?,
        "apply_patch" | "delete_file" | "replace_in_files" => lean_files(&fields)?,
        "write_file" => lean_write(&fields)?,
        "bash" | "PowerShell" => lean_command(&fields)?,
        "TodoWrite" => lean_todos(&fields)?,
        _ => return None,
    };
    let lean = format!(
        "{}{rest}",
        serde_json::to_string_pretty(&Value::Object(lean)).ok()?
    );
    (lean.len() < output.len()).then_some(lean)
}

/// Où la modification a porté, sans les lignes : le modèle connaît son texte.
fn hunk_positions(fields: &Map<String, Value>) -> Value {
    let hunks = fields
        .get("structuredPatch")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|hunk| {
            let mut position = Map::new();
            for key in ["oldStart", "oldLines", "newStart", "newLines"] {
                if let Some(value) = hunk.get(key) {
                    position.insert(key.to_string(), value.clone());
                }
            }
            Value::Object(position)
        })
        .collect();
    Value::Array(hunks)
}

fn lean_edit(fields: &Map<String, Value>) -> Option<Map<String, Value>> {
    let mut lean = Map::new();
    lean.insert("filePath".to_string(), fields.get("filePath")?.clone());
    lean.insert("edited".to_string(), json!(true));
    lean.insert("structuredPatch".to_string(), hunk_positions(fields));
    for flag in ["replaceAll", "userModified"] {
        if fields.get(flag) == Some(&json!(true)) {
            lean.insert(flag.to_string(), json!(true));
        }
    }
    // Texte retrouvé de façon approximative : le modèle doit le savoir.
    if let Some(note) = fields.get("matchNote").filter(|note| !note.is_null()) {
        lean.insert("matchNote".to_string(), note.clone());
    }
    Some(lean)
}

/// Fichiers changés par `apply_patch` ou `delete_file` : pour chacun, son
/// chemin, ce qui lui est arrivé et où, sans les lignes ni le fichier d'avant.
fn lean_files(fields: &Map<String, Value>) -> Option<Map<String, Value>> {
    let files = fields.get("files")?.as_array()?;
    let lean_files: Vec<Value> = files
        .iter()
        .filter_map(Value::as_object)
        .map(|file| {
            let mut lean = Map::new();
            for key in ["filePath", "type", "movedFrom", "matchNote"] {
                if let Some(value) = file.get(key).filter(|value| !value.is_null()) {
                    lean.insert(key.to_string(), value.clone());
                }
            }
            lean.insert("structuredPatch".to_string(), hunk_positions(file));
            Value::Object(lean)
        })
        .collect();
    let mut lean = Map::new();
    lean.insert("files".to_string(), Value::Array(lean_files));
    Some(lean)
}

fn lean_write(fields: &Map<String, Value>) -> Option<Map<String, Value>> {
    let content = fields.get("content")?.as_str()?;
    let mut lean = Map::new();
    if let Some(kind) = fields.get("type") {
        lean.insert("type".to_string(), kind.clone());
    }
    lean.insert("filePath".to_string(), fields.get("filePath")?.clone());
    lean.insert("lines".to_string(), json!(content.lines().count()));
    lean.insert("structuredPatch".to_string(), hunk_positions(fields));
    // Contenu abrégé écrit tel quel, codage changé : le modèle doit le lire.
    for key in ["note", "encodingNote"] {
        if let Some(value) = fields.get(key).filter(|value| !value.is_null()) {
            lean.insert(key.to_string(), value.clone());
        }
    }
    Some(lean)
}

/// Liste de tâches : le modèle vient de l'envoyer en entier (et l'ancienne
/// avant elle). Il lui suffit de savoir qu'elle est enregistrée et où il en
/// est. Dans un essai réel, sept mises à jour répétaient 15 000 caractères.
fn lean_todos(fields: &Map<String, Value>) -> Option<Map<String, Value>> {
    let todos = fields.get("newTodos")?.as_array()?;
    let with_status = |status: &str| {
        todos
            .iter()
            .filter(|todo| todo.get("status").and_then(Value::as_str) == Some(status))
            .collect::<Vec<_>>()
    };
    let mut lean = Map::new();
    lean.insert("saved".to_string(), json!(true));
    lean.insert("tasks".to_string(), json!(todos.len()));
    lean.insert(
        "completed".to_string(),
        json!(with_status("completed").len()),
    );
    let current = with_status("in_progress")
        .iter()
        .filter_map(|todo| todo.get("content").cloned())
        .collect::<Vec<_>>();
    lean.insert("inProgress".to_string(), Value::Array(current));
    if fields.get("verificationNudgeNeeded") == Some(&json!(true)) {
        lean.insert("verificationNudgeNeeded".to_string(), json!(true));
    }
    Some(lean)
}

/// Au-delà, la sortie d'une commande est raccourcie dans la session : son
/// début et sa fin (où sont presque toujours les erreurs) sont gardés. Sinon
/// un `npm install` bavard repartirait en entier avec chaque requête.
const MAX_COMMAND_OUTPUT_CHARS: usize = 30_000;
const KEPT_HEAD_CHARS: usize = 8_000;
const KEPT_TAIL_CHARS: usize = 20_000;

fn bounded(text: &str) -> Value {
    let count = text.chars().count();
    if count <= MAX_COMMAND_OUTPUT_CHARS {
        return Value::String(text.to_string());
    }
    let head: String = text.chars().take(KEPT_HEAD_CHARS).collect();
    let tail: String = text.chars().skip(count - KEPT_TAIL_CHARS).collect();
    Value::String(format!(
        "{head}\n… [{} characters omitted: run the command again with grep, head or tail to see that part] …\n{tail}",
        count - KEPT_HEAD_CHARS - KEPT_TAIL_CHARS
    ))
}

/// Sortie d'une commande : ce qu'elle a écrit (bornée) et comment elle s'est
/// terminée. L'état du bac à sable n'est gardé que s'il isole vraiment la commande.
fn lean_command(fields: &Map<String, Value>) -> Option<Map<String, Value>> {
    if !fields.contains_key("stdout") && !fields.contains_key("stderr") {
        return None;
    }
    let mut lean = Map::new();
    for (key, value) in fields {
        if matches!(key.as_str(), "stdout" | "stderr") {
            lean.insert(
                key.clone(),
                value.as_str().map_or_else(|| value.clone(), bounded),
            );
            continue;
        }
        let kept = match key.as_str() {
            "sandboxStatus" => ["namespace_active", "network_active", "in_container"]
                .iter()
                .any(|flag| value.get(flag) == Some(&json!(true))),
            "noOutputExpected" | "dangerouslyDisableSandbox" => false,
            _ => !value.is_null() && value != &json!(false),
        };
        if kept {
            lean.insert(key.clone(), value.clone());
        }
    }
    Some(lean)
}

#[cfg(test)]
mod tests {
    use super::lean_tool_output;
    use serde_json::{json, Value};

    fn parsed(output: &str) -> Value {
        serde_json::Deserializer::from_str(output)
            .into_iter::<Value>()
            .next()
            .expect("un résultat")
            .expect("JSON valide")
    }

    #[test]
    fn patch_results_keep_paths_and_positions_but_not_the_files() {
        let original = "x\n".repeat(400);
        let output = json!({ "files": [
            { "filePath": "/p/a.py", "type": "update", "originalFile": original,
              "structuredPatch": [{ "oldStart": 3, "oldLines": 1, "newStart": 3, "newLines": 1,
                                    "lines": ["-x", "+y"] }] },
            { "filePath": "/p/b.py", "type": "delete", "originalFile": "z\n",
              "structuredPatch": [] }
        ]});
        let lean = lean_tool_output("apply_patch", &output.to_string()).expect("allégé");
        assert!(
            !lean.contains("originalFile") && !lean.contains("\"-x\""),
            "{lean}"
        );
        let files = &parsed(&lean)["files"];
        assert_eq!(files[0]["type"], "update");
        assert_eq!(files[0]["structuredPatch"][0]["oldStart"], 3);
        assert_eq!(files[1]["filePath"], "/p/b.py");
        let deleted = json!({ "files": [{ "filePath": "/p/c.txt", "type": "delete",
            "originalFile": "long\n".repeat(100), "structuredPatch": [] }] });
        assert!(!lean_tool_output("delete_file", &deleted.to_string())
            .expect("allégé")
            .contains("originalFile"));
        let multi = json!({ "filePath": "/p/m.rs", "edits": 2, "originalFile": original,
            "structuredPatch": [], "userModified": false });
        assert!(!lean_tool_output("MultiEdit", &multi.to_string())
            .expect("allégé")
            .contains("originalFile"));
    }

    #[test]
    fn a_huge_command_output_keeps_its_start_and_its_end() {
        let log = (0..6_000)
            .map(|n| format!("ligne {n:05}\n"))
            .collect::<Vec<_>>()
            .concat();
        let full =
            json!({ "stdout": log, "stderr": "", "returnCodeInterpretation": "exit_code:1" });
        let lean = lean_tool_output("bash", &full.to_string()).expect("raccourcie");
        let stdout = parsed(&lean)["stdout"]
            .as_str()
            .expect("stdout")
            .to_string();
        assert!(
            stdout.chars().count() < 28_300,
            "{}",
            stdout.chars().count()
        );
        assert!(stdout.starts_with("ligne 00000"));
        assert!(stdout.trim_end().ends_with("ligne 05999"));
        assert!(stdout.contains("characters omitted: run the command again with grep"));
        assert_eq!(parsed(&lean)["returnCodeInterpretation"], "exit_code:1");
    }

    #[test]
    fn an_edit_result_keeps_where_it_landed_not_the_text() {
        let full = json!({
            "filePath": "D:/projet/app.py",
            "oldString": "ancien texte ".repeat(40),
            "newString": "nouveau texte ".repeat(60),
            "structuredPatch": [{
                "oldStart": 12, "oldLines": 3, "newStart": 12, "newLines": 5,
                "lines": ["-ancien", "+nouveau", " contexte"]
            }],
            "userModified": false,
            "replaceAll": false,
            "gitDiff": null
        });
        let output = format!(
            "{}\n\n[Claw] Syntax check failed for `app.py`",
            serde_json::to_string_pretty(&full).expect("json")
        );
        let lean = lean_tool_output("edit_file", &output).expect("résultat allégé");
        assert!(
            lean.len() * 4 < output.len(),
            "{} contre {}",
            lean.len(),
            output.len()
        );
        assert_eq!(
            parsed(&lean),
            json!({
                "filePath": "D:/projet/app.py",
                "edited": true,
                "structuredPatch": [{ "oldStart": 12, "oldLines": 3, "newStart": 12, "newLines": 5 }]
            })
        );
        // La remarque de Claw jointe au résultat reste.
        assert!(lean.ends_with("\n\n[Claw] Syntax check failed for `app.py`"));

        let noted = json!({
            "filePath": "a.py", "oldString": "x".repeat(200), "newString": "y".repeat(200),
            "structuredPatch": [], "userModified": false, "replaceAll": true,
            "matchNote": "indentation différente"
        });
        let lean = lean_tool_output("edit_file", &noted.to_string()).expect("résultat allégé");
        assert_eq!(parsed(&lean)["replaceAll"], true);
        assert_eq!(parsed(&lean)["matchNote"], "indentation différente");
    }

    #[test]
    fn a_write_result_does_not_repeat_the_file() {
        let full = json!({
            "type": "create",
            "filePath": "notes.md",
            "content": "ligne\n".repeat(300),
            "structuredPatch": [],
            "gitDiff": null
        });
        let lean = lean_tool_output("write_file", &full.to_string()).expect("résultat allégé");
        assert_eq!(
            parsed(&lean),
            json!({ "type": "create", "filePath": "notes.md", "lines": 300, "structuredPatch": [] })
        );
    }

    #[test]
    fn a_command_result_keeps_its_output_and_how_it_ended() {
        let full = json!({
            "stdout": "", "stderr": "erreur\n", "rawOutputPath": null, "interrupted": false,
            "isImage": null, "backgroundTaskId": null, "backgroundedByUser": null,
            "assistantAutoBackgrounded": null, "dangerouslyDisableSandbox": null,
            "returnCodeInterpretation": "exit_code:1", "noOutputExpected": false,
            "structuredContent": null, "persistedOutputPath": null, "persistedOutputSize": null,
            "sandboxStatus": { "enabled": true, "namespace_active": false, "network_active": false,
                "in_container": false, "filesystem_active": true, "fallback_reason": "indisponible" }
        });
        let lean = lean_tool_output("bash", &full.to_string()).expect("résultat allégé");
        assert_eq!(
            parsed(&lean),
            json!({ "stdout": "", "stderr": "erreur\n", "returnCodeInterpretation": "exit_code:1" })
        );

        // Une commande vraiment isolée garde l'état de son bac à sable.
        let isolated = json!({
            "stdout": "ok", "stderr": "", "interrupted": true, "backgroundTaskId": "bash_1",
            "noOutputExpected": true,
            "sandboxStatus": { "enabled": true, "network_active": true }
        });
        let isolated = serde_json::to_string_pretty(&isolated).expect("json");
        let lean = lean_tool_output("bash", &isolated).expect("résultat allégé");
        assert_eq!(
            parsed(&lean),
            json!({ "stdout": "ok", "stderr": "", "interrupted": true, "backgroundTaskId": "bash_1",
                "sandboxStatus": { "enabled": true, "network_active": true } })
        );
    }

    #[test]
    fn a_task_list_result_says_where_the_work_stands_not_the_list_again() {
        let task = |content: &str, status: &str| json!({ "content": content, "activeForm": format!("{content} en cours"), "status": status });
        let full = json!({
            "oldTodos": [task("Lire le code", "in_progress"), task("Écrire le test", "pending"), task("Corriger", "pending")],
            "newTodos": [task("Lire le code", "completed"), task("Écrire le test", "in_progress"), task("Corriger", "pending")],
            "verificationNudgeNeeded": null
        });
        let output = format!(
            "{}\n\n[Claw] This reply only updated the task list",
            serde_json::to_string_pretty(&full).expect("json")
        );
        let lean = lean_tool_output("TodoWrite", &output).expect("résultat allégé");
        assert_eq!(
            parsed(&lean),
            json!({ "saved": true, "tasks": 3, "completed": 1, "inProgress": ["Écrire le test"] })
        );
        assert!(lean.ends_with("\n\n[Claw] This reply only updated the task list"));
        // Déjà allégé (session rouverte) : laissé tel quel.
        assert_eq!(lean_tool_output("TodoWrite", &lean), None);
    }

    #[test]
    fn other_results_and_plain_errors_are_left_alone() {
        assert_eq!(
            lean_tool_output("read_file", r#"{"type":"text","file":{"content":"x"}}"#),
            None
        );
        assert_eq!(lean_tool_output("edit_file", "missing field `path`"), None);
        assert_eq!(
            lean_tool_output("bash", r#"{"stdout":"a","stderr":""}"#),
            None
        );
        assert_eq!(lean_tool_output("edit_file", r#"["pas un objet"]"#), None);
    }
}
