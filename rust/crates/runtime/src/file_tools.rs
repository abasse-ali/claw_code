//! Outils de fichiers ajoutés le 21 octobre 2026 à côté de `edit_file` :
//! plusieurs remplacements en un appel (`MultiEdit`), correctif au format des
//! modèles d'OpenAI et de beaucoup de modèles ouverts (`apply_patch`),
//! arborescence d'un dossier (`list_directory`), déplacement et suppression
//! (`move_file`, `delete_file`). Ils passent par les routines d'`edit_file` :
//! recherche tolérante du texte, codage et conventions du fichier gardés. Un
//! appel qui échoue ne laisse rien à moitié fait : tout est vérifié en mémoire
//! avant la première écriture.

use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::file_ops::{
    apply_edit, describe_char, display_path, elision_in, encode_text, make_patch, normalize_path,
    normalize_path_allow_missing, not_a_file, read_text, type_conventions, StructuredPatchHunk,
    TextEncoding,
};

// ------------------------------------------------------------ classement des outils

/// Outils du modèle qui créent, modifient, déplacent ou suppriment des fichiers.
#[must_use]
pub fn writes_files(tool_name: &str) -> bool {
    matches!(
        tool_name,
        "write_file"
            | "edit_file"
            | "MultiEdit"
            | "NotebookEdit"
            | "apply_patch"
            | "move_file"
            | "delete_file"
    )
}

/// Outils dont le résultat porte un seul fichier (`filePath`,
/// `structuredPatch`, `originalFile`), traité comme celui d'`edit_file`.
#[must_use]
pub fn edits_one_file(tool_name: &str) -> bool {
    matches!(tool_name, "write_file" | "edit_file" | "MultiEdit")
}

/// Outils dont le résultat liste les fichiers changés (`files`), chacun de la
/// forme d'un résultat d'`edit_file`.
#[must_use]
pub fn changes_listed_files(tool_name: &str) -> bool {
    matches!(tool_name, "apply_patch" | "delete_file")
}

/// Chemins, tels que le modèle les a écrits, que l'appel crée, modifie,
/// déplace ou supprime (destination d'un déplacement comprise).
#[must_use]
pub fn written_paths(tool_name: &str, input: &str) -> Vec<String> {
    let Ok(value) = serde_json::from_str::<Value>(input) else {
        return Vec::new();
    };
    let field = |key: &str| {
        value
            .get(key)
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
    };
    match tool_name {
        "write_file" | "edit_file" | "MultiEdit" | "delete_file" => {
            field("path").into_iter().collect()
        }
        "NotebookEdit" => field("notebook_path").into_iter().collect(),
        "move_file" => field("source")
            .into_iter()
            .chain(field("destination"))
            .collect(),
        "apply_patch" => field("patch")
            .or_else(|| field("input"))
            .map(|patch| patch_paths(&patch))
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

/// Chemin d'un élément qui doit exister tel quel : pour supprimer ou
/// déplacer, jamais un fichier au nom voisin trouvé à sa place.
fn exact_existing(path: &str, tool: &str) -> io::Result<PathBuf> {
    let target = normalize_path_allow_missing(path)?;
    if target.exists() || fs::symlink_metadata(&target).is_ok() {
        Ok(target)
    } else {
        Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!(
                "[Claw] {tool} was not executed: {} does not exist. Check the path with list_directory or glob_search.",
                display_path(&target)
            ),
        ))
    }
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.into())
}

/// Écrit `text` avec le codage du fichier et les conventions de son type
/// (fins de ligne, BOM) ; rend le texte écrit et ce qu'il faut en dire.
fn store_text(
    absolute_path: &Path,
    text: String,
    encoding: TextEncoding,
) -> io::Result<(String, Option<String>)> {
    let mut notes = Vec::new();
    let text = match type_conventions(absolute_path, &text) {
        Some((fixed, why)) => {
            notes.push(why.to_string());
            fixed
        }
        None => text,
    };
    let bytes = encode_text(&text, encoding).map_err(|ch| {
        invalid(format!(
            "{} is encoded in {}, which cannot store {}: write it with characters this encoding has (for example ' instead of a typographic quote), or rewrite the whole file with write_file to turn it into UTF-8",
            display_path(absolute_path),
            encoding.name(),
            describe_char(ch)
        ))
    })?;
    if let TextEncoding::Utf8Lossy(count) = encoding {
        notes.push(format!(
            "{count} invalid byte(s) of the file rewritten as U+FFFD"
        ));
    }
    if let Some(parent) = absolute_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(absolute_path, bytes)?;
    Ok((text, (!notes.is_empty()).then(|| notes.join(" ; "))))
}

// ------------------------------------------------------------ MultiEdit

/// Un remplacement de `MultiEdit`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EditOperation {
    #[serde(alias = "oldString", alias = "old_str", alias = "old")]
    pub old_string: String,
    #[serde(alias = "newString", alias = "new_str", alias = "new")]
    pub new_string: String,
    #[serde(default, alias = "replaceAll")]
    pub replace_all: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MultiEditOutput {
    #[serde(rename = "filePath")]
    pub file_path: String,
    /// Nombre de remplacements appliqués.
    pub edits: usize,
    /// Le fichier d'avant, pour Claw ; retiré de ce que le modèle relit.
    #[serde(rename = "originalFile", default)]
    pub original_file: String,
    #[serde(rename = "structuredPatch")]
    pub structured_patch: Vec<StructuredPatchHunk>,
    #[serde(rename = "userModified")]
    pub user_modified: bool,
    #[serde(rename = "matchNote", skip_serializing_if = "Option::is_none", default)]
    pub match_note: Option<String>,
}

/// Applique les remplacements dans l'ordre, chacun au texte que laissent les
/// précédents, puis écrit le fichier une fois. Un seul remplacement introuvable
/// et rien n'est écrit. Un premier `old_string` vide crée le fichier (absent
/// ou vide) avec son `new_string`.
pub fn multi_edit_file(path: &str, edits: &[EditOperation]) -> io::Result<MultiEditOutput> {
    if edits.is_empty() {
        return Err(invalid(
            "[Claw] MultiEdit was not executed: `edits` is empty. Give a list of {\"old_string\", \"new_string\"} objects.",
        ));
    }
    let creates = edits[0].old_string.is_empty();
    let (absolute_path, original, encoding) = if creates {
        let target = normalize_path_allow_missing(path)?;
        if target.is_dir() {
            return Err(not_a_file(path, "MultiEdit"));
        }
        match read_text(&target) {
            Ok((text, _)) if !text.is_empty() => {
                return Err(invalid(
                    "[Claw] MultiEdit was not executed: the first edit has an empty old_string, but the file already has content. Put the exact text to replace in old_string; to rewrite the whole file, use write_file.",
                ));
            }
            Ok((_, encoding)) => (target, String::new(), encoding),
            Err(_) => (target, String::new(), TextEncoding::Utf8),
        }
    } else {
        let target = normalize_path(path)?;
        if target.is_dir() {
            return Err(not_a_file(path, "MultiEdit"));
        }
        let (text, encoding) = read_text(&target)?;
        (target, text, encoding)
    };
    let total = edits.len();
    let mut current = original.clone();
    let mut notes = Vec::new();
    for (index, edit) in edits.iter().enumerate() {
        let number = index + 1;
        let refuse = |message: &str| {
            invalid(format!(
                "[Claw] MultiEdit was not executed and the file is unchanged: edit {number} of {total}: {}",
                message.trim_start_matches("[Claw] ")
            ))
        };
        if index == 0 && creates {
            current.clone_from(&edit.new_string);
            continue;
        }
        if edit.old_string.is_empty() {
            return Err(refuse(
                "old_string is empty. Only the first edit may have an empty old_string, to create the file.",
            ));
        }
        if edit.old_string == edit.new_string {
            return Err(refuse(
                "old_string and new_string are identical. Remove this edit, or put the changed text in new_string.",
            ));
        }
        if crate::harness::enabled(crate::harness::Aid::PlaceholderGuard) {
            if let Some((line, text)) = elision_in(&edit.new_string)
                .filter(|(_, text)| !edit.old_string.lines().any(|kept| kept.trim() == text))
            {
                if edit.new_string.lines().count() < edit.old_string.lines().count() {
                    return Err(refuse(&format!(
                        "line {line} of new_string is a placeholder (\"{text}\"), not code, so the lines of old_string would be lost. new_string must hold the exact new text."
                    )));
                }
            }
        }
        let (updated, note) =
            apply_edit(&current, &edit.old_string, &edit.new_string, edit.replace_all).map_err(
                |message| {
                    refuse(&format!(
                        "{message}\nThe edits apply in order, each to the text the earlier ones leave: old_string must match the file as it is after them."
                    ))
                },
            )?;
        if let Some(note) = note {
            notes.push(format!("edit {number}: {note}"));
        }
        current = updated;
    }
    let (written, note) = store_text(&absolute_path, current, encoding)?;
    notes.extend(note);
    Ok(MultiEditOutput {
        file_path: display_path(&absolute_path),
        edits: total,
        structured_patch: make_patch(&original, &written),
        original_file: original,
        user_modified: false,
        match_note: (!notes.is_empty()).then(|| notes.join(" ; ")),
    })
}

// ------------------------------------------------------------ fichiers changés

/// Un fichier changé par `apply_patch` ou `delete_file`, de la forme d'un
/// résultat d'`edit_file` (gardes, vue du fichier, journal).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChangedFile {
    #[serde(rename = "filePath")]
    pub file_path: String,
    /// `create`, `update`, `delete` ou `move`.
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(rename = "movedFrom", skip_serializing_if = "Option::is_none", default)]
    pub moved_from: Option<String>,
    #[serde(rename = "structuredPatch")]
    pub structured_patch: Vec<StructuredPatchHunk>,
    /// Le fichier d'avant, pour Claw ; retiré de ce que le modèle relit.
    #[serde(
        rename = "originalFile",
        skip_serializing_if = "Option::is_none",
        default
    )]
    pub original_file: Option<String>,
    #[serde(rename = "matchNote", skip_serializing_if = "Option::is_none", default)]
    pub match_note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FilesChangedOutput {
    pub files: Vec<ChangedFile>,
}

// ------------------------------------------------------------ apply_patch

#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct PatchChunk {
    /// Texte après `@@` : une ligne du fichier qui précède le passage.
    header: Option<String>,
    old: Vec<String>,
    new: Vec<String>,
    /// Suivi de `*** End of File` : le passage finit le fichier.
    at_end: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PatchAction {
    Add {
        path: String,
        lines: Vec<String>,
    },
    Delete {
        path: String,
    },
    Update {
        path: String,
        move_to: Option<String>,
        chunks: Vec<PatchChunk>,
    },
}

impl PatchAction {
    fn path(&self) -> &str {
        match self {
            Self::Add { path, .. } | Self::Delete { path } | Self::Update { path, .. } => path,
        }
    }
}

const PATCH_FORMAT: &str = "The patch format is:\n*** Begin Patch\n*** Update File: path/to/file\n@@ a line just above the change (optional)\n unchanged line\n-removed line\n+added line\n*** Add File: path/to/new_file\n+its first line\n*** Delete File: path/to/old_file\n*** End Patch\nEach line of an update starts with a space (unchanged), - (removed) or + (added). Add `*** Move to: new/path` right after `*** Update File:` to rename.";

/// Chemins nommés par un correctif (fichiers ajoutés, modifiés, supprimés,
/// et destinations de déplacement), sans le valider.
#[must_use]
pub fn patch_paths(patch: &str) -> Vec<String> {
    let mut paths = Vec::new();
    for line in patch.lines() {
        let line = line.trim_end_matches('\r');
        for marker in [
            "*** Add File:",
            "*** Delete File:",
            "*** Update File:",
            "*** Move to:",
        ] {
            if let Some(rest) = line.strip_prefix(marker) {
                let path = rest.trim().to_string();
                if !path.is_empty() && !paths.contains(&path) {
                    paths.push(path);
                }
            }
        }
    }
    paths
}

/// En-tête `@@` d'un diff unifié (`@@ -12,3 +12,4 @@`) : des numéros, pas une
/// ligne du fichier.
fn is_numeric_header(header: &str) -> bool {
    let trimmed = header.trim().trim_end_matches("@@").trim();
    !trimmed.is_empty()
        && trimmed
            .chars()
            .all(|ch| ch.is_ascii_digit() || matches!(ch, '-' | '+' | ',' | ' '))
}

#[allow(clippy::too_many_lines)]
fn parse_patch(patch: &str) -> Result<Vec<PatchAction>, String> {
    let text = patch.replace("\r\n", "\n");
    let mut lines: Vec<&str> = text.lines().collect();
    // Bloc de code autour du correctif.
    while lines.first().is_some_and(|line| line.trim().is_empty()) {
        lines.remove(0);
    }
    if lines
        .first()
        .is_some_and(|line| line.trim_start().starts_with("```"))
    {
        lines.remove(0);
        while lines.last().is_some_and(|line| line.trim().is_empty()) {
            lines.pop();
        }
        if lines.last().is_some_and(|line| line.trim() == "```") {
            lines.pop();
        }
    }
    if lines.iter().any(|line| line.starts_with("diff --git "))
        || (lines.first().is_some_and(|line| line.starts_with("--- "))
            && lines.get(1).is_some_and(|line| line.starts_with("+++ ")))
    {
        return Err(format!(
            "[Claw] apply_patch was not executed: this is a unified diff (`--- a/` / `+++ b/`), not the apply_patch format. {PATCH_FORMAT}"
        ));
    }
    let mut actions: Vec<PatchAction> = Vec::new();
    for (index, raw) in lines.iter().enumerate() {
        let number = index + 1;
        let line = *raw;
        let trimmed = line.trim_end();
        if trimmed == "*** Begin Patch" {
            continue;
        }
        if trimmed == "*** End Patch" {
            break;
        }
        if let Some(rest) = trimmed.strip_prefix("*** Add File:") {
            actions.push(PatchAction::Add {
                path: rest.trim().to_string(),
                lines: Vec::new(),
            });
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("*** Delete File:") {
            actions.push(PatchAction::Delete {
                path: rest.trim().to_string(),
            });
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("*** Update File:") {
            actions.push(PatchAction::Update {
                path: rest.trim().to_string(),
                move_to: None,
                chunks: Vec::new(),
            });
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("*** Move to:") {
            match actions.last_mut() {
                Some(PatchAction::Update { move_to, .. }) => {
                    *move_to = Some(rest.trim().to_string());
                    continue;
                }
                _ => {
                    return Err(format!(
                        "[Claw] apply_patch was not executed: line {number}, `*** Move to:` must come right after an `*** Update File:` line. {PATCH_FORMAT}"
                    ))
                }
            }
        }
        if trimmed == "*** End of File" {
            if let Some(PatchAction::Update { chunks, .. }) = actions.last_mut() {
                if let Some(chunk) = chunks.last_mut() {
                    chunk.at_end = true;
                }
            }
            continue;
        }
        match actions.last_mut() {
            None => {
                if trimmed.is_empty() {
                    continue;
                }
                return Err(format!(
                    "[Claw] apply_patch was not executed: line {number} (`{}`) comes before any `*** Add File:`, `*** Update File:` or `*** Delete File:` line. {PATCH_FORMAT}",
                    excerpt(trimmed)
                ));
            }
            Some(PatchAction::Add { lines, .. }) => {
                lines.push(line.strip_prefix('+').unwrap_or(line).to_string());
            }
            Some(PatchAction::Delete { path }) => {
                if !trimmed.is_empty() {
                    return Err(format!(
                        "[Claw] apply_patch was not executed: line {number}, `*** Delete File: {path}` takes no content lines. {PATCH_FORMAT}"
                    ));
                }
            }
            Some(PatchAction::Update { path, chunks, .. }) => {
                if let Some(rest) = line.strip_prefix("@@") {
                    let header = rest.trim().trim_end_matches("@@").trim();
                    chunks.push(PatchChunk {
                        header: (!header.is_empty() && !is_numeric_header(header))
                            .then(|| header.to_string()),
                        ..PatchChunk::default()
                    });
                    continue;
                }
                if chunks.is_empty() {
                    chunks.push(PatchChunk::default());
                }
                let chunk = chunks.last_mut().expect("passage ouvert");
                if line.is_empty() {
                    chunk.old.push(String::new());
                    chunk.new.push(String::new());
                } else if let Some(rest) = line.strip_prefix(' ') {
                    chunk.old.push(rest.to_string());
                    chunk.new.push(rest.to_string());
                } else if let Some(rest) = line.strip_prefix('-') {
                    chunk.old.push(rest.to_string());
                } else if let Some(rest) = line.strip_prefix('+') {
                    chunk.new.push(rest.to_string());
                } else {
                    return Err(format!(
                        "[Claw] apply_patch was not executed: line {number} (`{}`) of the update of {} does not start with a space, - or +. {PATCH_FORMAT}",
                        excerpt(trimmed),
                        path
                    ));
                }
            }
        }
    }
    if actions.is_empty() {
        return Err(format!(
            "[Claw] apply_patch was not executed: the patch names no file. {PATCH_FORMAT}"
        ));
    }
    for action in &mut actions {
        if action.path().is_empty() {
            return Err(format!(
                "[Claw] apply_patch was not executed: a `***` line has no path after it. {PATCH_FORMAT}"
            ));
        }
        if let PatchAction::Update {
            path,
            move_to,
            chunks,
        } = action
        {
            // Un passage vide (en-tête seul) ne change rien.
            chunks.retain(|chunk| !(chunk.old.is_empty() && chunk.new.is_empty()));
            if chunks.is_empty() && move_to.is_none() {
                return Err(format!(
                    "[Claw] apply_patch was not executed: `*** Update File: {path}` has no change lines. {PATCH_FORMAT}"
                ));
            }
        }
    }
    Ok(actions)
}

fn excerpt(text: &str) -> String {
    let mut short: String = text.chars().take(80).collect();
    if short.len() < text.len() {
        short.push('…');
    }
    short
}

/// Position de `wanted` dans `lines` à partir de `from` (puis depuis le
/// début), exacte d'abord, puis aux espaces de fin près, puis aux espaces
/// près. Un passage marqué fin de fichier est d'abord cherché à la fin.
fn find_lines(lines: &[String], wanted: &[String], from: usize, at_end: bool) -> Option<usize> {
    if wanted.len() > lines.len() {
        return None;
    }
    let last = lines.len() - wanted.len();
    let comparisons: [fn(&str, &str) -> bool; 3] = [
        |a, b| a == b,
        |a, b| a.trim_end() == b.trim_end(),
        |a, b| a.trim() == b.trim(),
    ];
    for same in comparisons {
        let fits = |at: usize| {
            lines[at..at + wanted.len()]
                .iter()
                .zip(wanted)
                .all(|(line, want)| same(line, want))
        };
        if at_end && fits(last) {
            return Some(last);
        }
        if let Some(at) = (from.min(last + 1)..=last).find(|&at| fits(at)) {
            return Some(at);
        }
        if let Some(at) = (0..from.min(last + 1)).find(|&at| fits(at)) {
            return Some(at);
        }
    }
    None
}

/// Applique les passages d'une mise à jour au texte du fichier.
fn apply_chunks(text: &str, chunks: &[PatchChunk], shown: &str) -> Result<String, String> {
    let crlf = text.matches("\r\n").count() * 2 > text.matches('\n').count();
    let final_newline = text.ends_with('\n') || text.is_empty();
    let mut lines: Vec<String> = text.lines().map(ToOwned::to_owned).collect();
    let mut cursor = 0usize;
    for (index, chunk) in chunks.iter().enumerate() {
        if let Some(header) = &chunk.header {
            let wanted = header.trim();
            let found = (cursor..lines.len())
                .chain(0..cursor.min(lines.len()))
                .find(|&at| lines[at].trim() == wanted)
                .or_else(|| {
                    (cursor..lines.len())
                        .chain(0..cursor.min(lines.len()))
                        .find(|&at| lines[at].contains(wanted))
                });
            if let Some(found) = found {
                cursor = found + 1;
            }
        }
        let at = if chunk.old.is_empty() {
            // Lignes ajoutées sans contexte : après l'en-tête trouvé, sinon à la fin.
            if chunk.header.is_some() && !chunk.at_end && cursor > 0 {
                cursor
            } else {
                lines.len()
            }
        } else {
            find_lines(&lines, &chunk.old, cursor, chunk.at_end).ok_or_else(|| {
                let first = chunk
                    .old
                    .iter()
                    .find(|line| !line.trim().is_empty())
                    .map_or(String::new(), |line| excerpt(line.trim()));
                format!(
                    "[Claw] apply_patch was not executed and no file was changed: in {shown}, the lines of change {} of {} (unchanged and - lines, starting with `{first}`) are not in the file. Copy them exactly from the file (read it first), with their indentation.",
                    index + 1,
                    chunks.len()
                )
            })?
        };
        let end = at + chunk.old.len();
        lines
            .splice(at..end, chunk.new.iter().cloned())
            .for_each(drop);
        cursor = at + chunk.new.len();
    }
    let separator = if crlf { "\r\n" } else { "\n" };
    let mut out = lines.join(separator);
    if final_newline && !lines.is_empty() {
        out.push_str(separator);
    }
    Ok(out)
}

/// État d'un fichier pendant la préparation d'un correctif : le texte qu'il
/// aura, ou `None` s'il sera supprimé.
struct Pending {
    original: Option<String>,
    encoding: TextEncoding,
    text: Option<String>,
    shown: String,
}

/// Applique un correctif au format `*** Begin Patch` : tout est vérifié en
/// mémoire, puis les fichiers sont écrits. Un passage introuvable et aucun
/// fichier n'est changé.
#[allow(clippy::too_many_lines)]
pub fn apply_patch(patch: &str) -> io::Result<FilesChangedOutput> {
    let actions = parse_patch(patch).map_err(invalid)?;
    let mut pending: HashMap<PathBuf, Pending> = HashMap::new();
    let mut order: Vec<PathBuf> = Vec::new();
    // (chemin, type, chemin d'origine pour un déplacement), dans l'ordre du correctif.
    let mut reports: Vec<(PathBuf, &'static str, Option<PathBuf>)> = Vec::new();

    let load =
        |path: &PathBuf, pending: &mut HashMap<PathBuf, Pending>, order: &mut Vec<PathBuf>| {
            if !pending.contains_key(path) {
                let (original, encoding) = match read_text(path) {
                    Ok((text, encoding)) => (Some(text), encoding),
                    Err(_) => (None, TextEncoding::Utf8),
                };
                pending.insert(
                    path.clone(),
                    Pending {
                        text: original.clone(),
                        original,
                        encoding,
                        shown: display_path(path),
                    },
                );
                order.push(path.clone());
            }
        };

    for action in &actions {
        match action {
            PatchAction::Add { path, lines } => {
                let target = normalize_path_allow_missing(path)?;
                if target.is_dir() {
                    return Err(not_a_file(path, "apply_patch"));
                }
                load(&target, &mut pending, &mut order);
                let entry = pending.get_mut(&target).expect("chargé");
                if entry.text.as_ref().is_some_and(|text| !text.is_empty()) {
                    return Err(invalid(format!(
                        "[Claw] apply_patch was not executed and no file was changed: `*** Add File: {path}` but {path} already exists. Use `*** Update File: {path}` to change it."
                    )));
                }
                let mut text = lines.join("\n");
                if !lines.is_empty() {
                    text.push('\n');
                }
                entry.text = Some(text);
                reports.push((target, "create", None));
            }
            PatchAction::Delete { path } => {
                let target = exact_existing(path, "apply_patch")?;
                if target.is_dir() {
                    return Err(not_a_file(path, "apply_patch"));
                }
                load(&target, &mut pending, &mut order);
                let entry = pending.get_mut(&target).expect("chargé");
                if entry.text.is_none() && entry.original.is_none() && !target.exists() {
                    return Err(invalid(format!(
                        "[Claw] apply_patch was not executed: {path} does not exist, so it cannot be deleted."
                    )));
                }
                entry.text = None;
                reports.push((target, "delete", None));
            }
            PatchAction::Update {
                path,
                move_to,
                chunks,
            } => {
                let source = normalize_path(path)?;
                if source.is_dir() {
                    return Err(not_a_file(path, "apply_patch"));
                }
                load(&source, &mut pending, &mut order);
                let entry = pending.get_mut(&source).expect("chargé");
                let Some(current) = entry.text.clone() else {
                    return Err(invalid(format!(
                        "[Claw] apply_patch was not executed: {path} cannot be read as text, or this patch deletes it before updating it."
                    )));
                };
                let updated = apply_chunks(&current, chunks, &entry.shown).map_err(invalid)?;
                match move_to {
                    None => {
                        entry.text = Some(updated);
                        reports.push((source, "update", None));
                    }
                    Some(destination) => {
                        let target = normalize_path_allow_missing(destination)?;
                        if target.is_dir() {
                            return Err(not_a_file(destination, "apply_patch"));
                        }
                        let encoding = entry.encoding;
                        entry.text = None;
                        load(&target, &mut pending, &mut order);
                        let moved = pending.get_mut(&target).expect("chargé");
                        if moved.original.is_some() && target != source {
                            return Err(invalid(format!(
                                "[Claw] apply_patch was not executed: `*** Move to: {destination}` but {destination} already exists. Move to a new path, or update that file instead."
                            )));
                        }
                        moved.encoding = encoding;
                        moved.text = Some(updated);
                        reports.push((target, "move", Some(source)));
                    }
                }
            }
        }
    }

    // Écriture : rien n'a été touché avant ce point.
    let mut notes: HashMap<PathBuf, String> = HashMap::new();
    let mut written: HashMap<PathBuf, String> = HashMap::new();
    for path in &order {
        let entry = &pending[path];
        match &entry.text {
            Some(text) if entry.original.as_deref() != Some(text.as_str()) => {
                let (stored, note) = store_text(path, text.clone(), entry.encoding)?;
                if let Some(note) = note {
                    notes.insert(path.clone(), note);
                }
                written.insert(path.clone(), stored);
            }
            Some(text) => {
                written.insert(path.clone(), text.clone());
            }
            None => {
                if path.exists() {
                    fs::remove_file(path)?;
                }
            }
        }
    }

    let mut files: Vec<ChangedFile> = Vec::new();
    for (path, kind, moved_from) in reports {
        let entry = &pending[&path];
        let before = match &moved_from {
            Some(source) => pending[source].original.clone(),
            None => entry.original.clone(),
        };
        let after = written.get(&path).cloned().unwrap_or_default();
        let structured_patch = make_patch(before.as_deref().unwrap_or(""), &after);
        let changed = ChangedFile {
            file_path: entry.shown.clone(),
            kind: kind.to_string(),
            moved_from: moved_from.as_ref().map(|source| display_path(source)),
            structured_patch,
            original_file: before,
            match_note: notes.get(&path).cloned(),
        };
        // Le même fichier changé deux fois : un seul compte rendu, le dernier.
        files.retain(|file| file.file_path != changed.file_path);
        files.push(changed);
    }
    Ok(FilesChangedOutput { files })
}

// ------------------------------------------------------------ delete_file

/// Supprime un fichier, ou un dossier vide.
pub fn delete_file(path: &str) -> io::Result<FilesChangedOutput> {
    let target = exact_existing(path, "delete_file")?;
    if target.is_dir() {
        let empty = fs::read_dir(&target)?.next().is_none();
        if !empty {
            return Err(invalid(format!(
                "[Claw] delete_file was not executed: {} is a directory with files in it. delete_file removes one file, or an empty directory: delete the files you mean to remove one by one.",
                display_path(&target)
            )));
        }
        fs::remove_dir(&target)?;
        return Ok(FilesChangedOutput {
            files: vec![ChangedFile {
                file_path: display_path(&target),
                kind: "delete".to_string(),
                moved_from: None,
                structured_patch: Vec::new(),
                original_file: None,
                match_note: Some("empty directory removed".to_string()),
            }],
        });
    }
    let original = read_text(&target).ok().map(|(text, _)| text);
    fs::remove_file(&target)?;
    Ok(FilesChangedOutput {
        files: vec![ChangedFile {
            file_path: display_path(&target),
            kind: "delete".to_string(),
            moved_from: None,
            structured_patch: original
                .as_deref()
                .map(|text| make_patch(text, ""))
                .unwrap_or_default(),
            match_note: original
                .is_none()
                .then(|| "not a text file: its content is not shown".to_string()),
            original_file: original,
        }],
    })
}

// ------------------------------------------------------------ move_file

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MoveFileOutput {
    pub source: String,
    pub destination: String,
    /// `file` ou `directory`.
    #[serde(rename = "type")]
    pub kind: String,
    /// Un fichier existait à la destination et a été remplacé.
    pub overwritten: bool,
}

/// Déplace ou renomme un fichier ou un dossier. Une destination qui est un
/// dossier existant reçoit l'élément sous son nom.
pub fn move_file(source: &str, destination: &str, overwrite: bool) -> io::Result<MoveFileOutput> {
    let from = exact_existing(source, "move_file")?;
    let mut to = normalize_path_allow_missing(destination)?;
    if to.is_dir() && (!from.is_dir() || destination.ends_with(['/', '\\'])) {
        if let Some(name) = from.file_name() {
            to = to.join(name);
        }
    }
    if to == from {
        return Err(invalid(format!(
            "[Claw] move_file changed nothing: source and destination are the same path, {}.",
            display_path(&from)
        )));
    }
    if from.is_dir() && to.starts_with(&from) {
        return Err(invalid(format!(
            "[Claw] move_file was not executed: {} cannot be moved inside itself.",
            display_path(&from)
        )));
    }
    let exists = to.exists();
    if exists && (to.is_dir() || !overwrite) {
        return Err(invalid(format!(
            "[Claw] move_file was not executed: {} already exists. Choose another destination{}.",
            display_path(&to),
            if to.is_dir() {
                ""
            } else {
                ", or set overwrite to true to replace it"
            }
        )));
    }
    if let Some(parent) = to.parent() {
        fs::create_dir_all(parent)?;
    }
    let directory = from.is_dir();
    if fs::rename(&from, &to).is_err() {
        // Autre disque : copie puis suppression (fichiers seulement).
        if directory {
            fs::rename(&from, &to)?;
        }
        fs::copy(&from, &to)?;
        fs::remove_file(&from)?;
    }
    Ok(MoveFileOutput {
        source: display_path(&from),
        destination: display_path(&to),
        kind: if directory { "directory" } else { "file" }.to_string(),
        overwritten: exists,
    })
}

// ------------------------------------------------------------ list_directory

/// Dossiers montrés sans leur contenu : dépendances, caches, sorties de build.
const UNLISTED_DIRS: &[&str] = &[
    ".git",
    "node_modules",
    ".venv",
    "venv",
    "__pycache__",
    ".next",
    ".nuxt",
    ".cache",
    "target",
    ".mypy_cache",
    ".pytest_cache",
    ".ruff_cache",
    ".gradle",
    ".idea",
    ".claw",
];
const LIST_MAX_ENTRIES: usize = 400;
const LIST_DEFAULT_DEPTH: usize = 3;
const LIST_MAX_DEPTH: usize = 8;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ListDirectoryOutput {
    pub path: String,
    /// Une ligne par élément, indentée par niveau ; un dossier finit par `/`.
    pub tree: String,
    pub directories: usize,
    pub files: usize,
    pub truncated: bool,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub note: Option<String>,
}

fn human_size(bytes: u64) -> String {
    #[allow(clippy::cast_precision_loss)]
    let value = bytes as f64;
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", value / 1024.0)
    } else {
        format!("{:.1} MB", value / (1024.0 * 1024.0))
    }
}

struct Listing {
    lines: Vec<String>,
    directories: usize,
    files: usize,
    truncated: bool,
}

fn list_into(dir: &Path, level: usize, depth: usize, listing: &mut Listing) {
    let Ok(read) = fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<(bool, String, PathBuf, Option<fs::Metadata>)> = read
        .filter_map(Result::ok)
        .map(|entry| {
            let meta = fs::symlink_metadata(entry.path()).ok();
            let is_dir = meta.as_ref().is_some_and(fs::Metadata::is_dir);
            (
                is_dir,
                entry.file_name().to_string_lossy().into_owned(),
                entry.path(),
                meta,
            )
        })
        .collect();
    // Dossiers d'abord, puis par nom.
    entries.sort_by(|a, b| {
        b.0.cmp(&a.0)
            .then_with(|| a.1.to_lowercase().cmp(&b.1.to_lowercase()))
    });
    let indent = "  ".repeat(level);
    for (is_dir, name, path, meta) in entries {
        if listing.lines.len() >= LIST_MAX_ENTRIES {
            listing.truncated = true;
            return;
        }
        let is_link = meta
            .as_ref()
            .is_some_and(|meta| meta.file_type().is_symlink());
        if is_link {
            let target = fs::read_link(&path)
                .map(|target| target.display().to_string())
                .unwrap_or_default();
            listing.lines.push(format!("{indent}{name} -> {target}"));
            listing.files += 1;
        } else if is_dir {
            listing.directories += 1;
            if UNLISTED_DIRS.contains(&name.as_str()) {
                listing.lines.push(format!("{indent}{name}/ (not listed)"));
            } else if level + 1 >= depth {
                let count = fs::read_dir(&path).map_or(0, Iterator::count);
                listing
                    .lines
                    .push(format!("{indent}{name}/ ({count} entries, not listed)"));
            } else {
                listing.lines.push(format!("{indent}{name}/"));
                list_into(&path, level + 1, depth, listing);
            }
        } else {
            listing.files += 1;
            let size = meta.map_or(0, |meta| meta.len());
            listing
                .lines
                .push(format!("{indent}{name} ({})", human_size(size)));
        }
    }
}

/// Arborescence d'un dossier, `depth` niveaux (3 par défaut), sans le contenu
/// des dossiers de dépendances et de caches.
pub fn list_directory(path: Option<&str>, depth: Option<usize>) -> io::Result<ListDirectoryOutput> {
    let path = path.filter(|path| !path.trim().is_empty()).unwrap_or(".");
    let dir = normalize_path(path)?;
    if !dir.is_dir() {
        return Err(invalid(format!(
            "[Claw] {} is a file, not a directory: read it with read_file.",
            display_path(&dir)
        )));
    }
    let depth = depth.unwrap_or(LIST_DEFAULT_DEPTH).clamp(1, LIST_MAX_DEPTH);
    let mut listing = Listing {
        lines: Vec::new(),
        directories: 0,
        files: 0,
        truncated: false,
    };
    list_into(&dir, 0, depth, &mut listing);
    let note = if listing.truncated {
        Some(format!(
            "Only the first {LIST_MAX_ENTRIES} entries are shown: list a subdirectory, or a smaller depth."
        ))
    } else if listing.lines.is_empty() {
        Some("The directory is empty.".to_string())
    } else {
        None
    };
    Ok(ListDirectoryOutput {
        path: display_path(&dir),
        tree: listing.lines.join("\n"),
        directories: listing.directories,
        files: listing.files,
        truncated: listing.truncated,
        note,
    })
}

#[cfg(test)]
mod tests {
    use super::{
        apply_patch, delete_file, list_directory, move_file, multi_edit_file, parse_patch,
        patch_paths, written_paths, EditOperation,
    };
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("horloge")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("claw-file-tools-{name}-{unique}"));
        fs::create_dir_all(&dir).expect("dossier");
        dir
    }

    fn edit(old: &str, new: &str) -> EditOperation {
        EditOperation {
            old_string: old.to_string(),
            new_string: new.to_string(),
            replace_all: false,
        }
    }

    fn path_of(dir: &std::path::Path, name: &str) -> String {
        dir.join(name).to_string_lossy().into_owned()
    }

    #[test]
    fn multi_edit_applies_every_edit_in_order_or_none() {
        let dir = temp_dir("multi");
        let file = path_of(&dir, "app.py");
        fs::write(&file, "x = 1\ny = 2\nz = 3\n").expect("fichier");
        let output = multi_edit_file(
            &file,
            &[
                edit("x = 1", "x = 10"),
                edit("x = 10\ny = 2", "x = 10\ny = 20"),
            ],
        )
        .expect("modifications");
        assert_eq!(output.edits, 2);
        assert_eq!(
            fs::read_to_string(&file).expect("relu"),
            "x = 10\ny = 20\nz = 3\n"
        );
        assert_eq!(output.original_file, "x = 1\ny = 2\nz = 3\n");
        // La deuxième introuvable : la première n'est pas écrite non plus.
        let error = multi_edit_file(&file, &[edit("z = 3", "z = 30"), edit("absent", "x")])
            .expect_err("refus");
        assert!(error.to_string().contains("edit 2 of 2"), "{error}");
        assert_eq!(
            fs::read_to_string(&file).expect("relu"),
            "x = 10\ny = 20\nz = 3\n"
        );
        // Premier old_string vide : création.
        let created = path_of(&dir, "sub/new.txt");
        multi_edit_file(&created, &[edit("", "a\nb\n"), edit("b", "c")]).expect("création");
        assert_eq!(fs::read_to_string(&created).expect("relu"), "a\nc\n");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn apply_patch_adds_updates_moves_and_deletes_files() {
        let dir = temp_dir("patch");
        let keep = path_of(&dir, "keep.py");
        let gone = path_of(&dir, "gone.txt");
        let old = path_of(&dir, "old_name.py");
        fs::write(&keep, "def a():\n    return 1\n\ndef b():\n    return 2\n").expect("keep");
        fs::write(&gone, "bye\n").expect("gone");
        fs::write(&old, "VALUE = 1\n").expect("old");
        let new_name = path_of(&dir, "pkg/new_name.py");
        let added = path_of(&dir, "added.md");
        let patch = format!(
            "*** Begin Patch\n*** Update File: {keep}\n@@ def b():\n-    return 2\n+    return 3\n*** Add File: {added}\n+# Titre\n+texte\n*** Delete File: {gone}\n*** Update File: {old}\n*** Move to: {new_name}\n-VALUE = 1\n+VALUE = 2\n*** End Patch"
        );
        assert_eq!(patch_paths(&patch).len(), 5);
        let output = apply_patch(&patch).expect("correctif");
        assert_eq!(
            fs::read_to_string(&keep).expect("keep"),
            "def a():\n    return 1\n\ndef b():\n    return 3\n"
        );
        assert_eq!(
            fs::read_to_string(&added).expect("added"),
            "# Titre\ntexte\n"
        );
        assert!(!std::path::Path::new(&gone).exists());
        assert!(!std::path::Path::new(&old).exists());
        assert_eq!(fs::read_to_string(&new_name).expect("moved"), "VALUE = 2\n");
        let kinds: Vec<&str> = output.files.iter().map(|file| file.kind.as_str()).collect();
        assert_eq!(kinds, ["update", "create", "delete", "move"]);
        // Le diff de la mise à jour ne porte que la ligne changée et son contexte.
        let update = &output.files[0].structured_patch;
        assert_eq!(
            update[0]
                .lines
                .iter()
                .filter(|line| !line.starts_with(' '))
                .count(),
            2
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn apply_patch_changes_nothing_when_one_change_is_not_found() {
        let dir = temp_dir("patch-refus");
        let first = path_of(&dir, "a.txt");
        let second = path_of(&dir, "b.txt");
        fs::write(&first, "un\ndeux\n").expect("a");
        fs::write(&second, "trois\n").expect("b");
        let patch = format!(
            "*** Begin Patch\n*** Update File: {first}\n-un\n+UN\n*** Update File: {second}\n-quatre\n+QUATRE\n*** End Patch"
        );
        let error = apply_patch(&patch).expect_err("refus");
        assert!(error.to_string().contains("no file was changed"), "{error}");
        assert_eq!(fs::read_to_string(&first).expect("a"), "un\ndeux\n");
        // Diff unifié : refusé avec le bon format.
        let unified = parse_patch("--- a/x\n+++ b/x\n@@ -1 +1 @@\n-a\n+b\n").expect_err("diff");
        assert!(unified.contains("*** Begin Patch"), "{unified}");
        // Espaces de fin recopiés de travers : trouvé quand même.
        let loose =
            format!("*** Begin Patch\n*** Update File: {second}\n-trois   \n+TROIS\n*** End Patch");
        apply_patch(&loose).expect("tolérant");
        assert_eq!(fs::read_to_string(&second).expect("b"), "TROIS\n");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn move_and_delete_files() {
        let dir = temp_dir("move");
        let source = path_of(&dir, "a.txt");
        fs::write(&source, "a\n").expect("a");
        fs::create_dir_all(dir.join("docs")).expect("docs");
        let moved = move_file(&source, &path_of(&dir, "docs"), false).expect("déplacé");
        assert!(
            moved.destination.ends_with("a.txt"),
            "{}",
            moved.destination
        );
        let inside = path_of(&dir, "docs/a.txt");
        fs::write(path_of(&dir, "b.txt"), "b\n").expect("b");
        let refused = move_file(&path_of(&dir, "b.txt"), &inside, false).expect_err("existe");
        assert!(refused.to_string().contains("overwrite"), "{refused}");
        move_file(&path_of(&dir, "b.txt"), &inside, true).expect("remplacé");
        assert_eq!(fs::read_to_string(&inside).expect("relu"), "b\n");
        let deleted = delete_file(&inside).expect("supprimé");
        assert_eq!(deleted.files[0].original_file.as_deref(), Some("b\n"));
        assert!(!std::path::Path::new(&inside).exists());
        let not_empty = delete_file(&path_of(&dir, ".")).expect_err("dossier plein");
        assert!(not_empty.to_string().contains("directory"), "{not_empty}");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn list_directory_shows_a_tree_without_dependencies() {
        let dir = temp_dir("list");
        fs::create_dir_all(dir.join("src/ui")).expect("src");
        fs::create_dir_all(dir.join("node_modules/x")).expect("deps");
        fs::write(dir.join("src/ui/page.tsx"), "x").expect("page");
        fs::write(dir.join("README.md"), "# r").expect("readme");
        let output = list_directory(Some(&dir.to_string_lossy()), None).expect("liste");
        assert_eq!(
            output.tree,
            "node_modules/ (not listed)\nsrc/\n  ui/\n    page.tsx (1 B)\nREADME.md (3 B)"
        );
        assert_eq!((output.directories, output.files), (3, 2));
        let shallow = list_directory(Some(&dir.to_string_lossy()), Some(1)).expect("liste");
        assert!(
            shallow.tree.contains("src/ (1 entries, not listed)"),
            "{}",
            shallow.tree
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn written_paths_names_every_file_a_call_touches() {
        assert_eq!(
            written_paths("move_file", r#"{"source":"a.txt","destination":"b/a.txt"}"#),
            ["a.txt", "b/a.txt"]
        );
        assert_eq!(
            written_paths(
                "apply_patch",
                r#"{"patch":"*** Begin Patch\n*** Update File: x.py\n*** Move to: y.py\n-a\n+b\n*** End Patch"}"#
            ),
            ["x.py", "y.py"]
        );
        assert_eq!(
            written_paths("MultiEdit", r#"{"path":"m.rs","edits":[]}"#),
            ["m.rs"]
        );
        assert!(written_paths("read_file", r#"{"path":"r.rs"}"#).is_empty());
    }
}
