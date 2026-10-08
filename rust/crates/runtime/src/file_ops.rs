use std::cmp::Reverse;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Instant;

use glob::Pattern;
use regex::RegexBuilder;
use serde::{Deserialize, Serialize};
use walkdir::WalkDir;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TextFilePayload {
    #[serde(rename = "filePath")]
    pub file_path: String,
    pub content: String,
    #[serde(rename = "numLines")]
    pub num_lines: usize,
    #[serde(rename = "startLine")]
    pub start_line: usize,
    #[serde(rename = "totalLines")]
    pub total_lines: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReadFileOutput {
    #[serde(rename = "type")]
    pub kind: String,
    pub file: TextFilePayload,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StructuredPatchHunk {
    #[serde(rename = "oldStart")]
    pub old_start: usize,
    #[serde(rename = "oldLines")]
    pub old_lines: usize,
    #[serde(rename = "newStart")]
    pub new_start: usize,
    #[serde(rename = "newLines")]
    pub new_lines: usize,
    pub lines: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WriteFileOutput {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(rename = "filePath")]
    pub file_path: String,
    pub content: String,
    #[serde(rename = "structuredPatch")]
    pub structured_patch: Vec<StructuredPatchHunk>,
    #[serde(rename = "originalFile", skip_serializing, default)]
    pub original_file: Option<String>,
    #[serde(rename = "gitDiff")]
    pub git_diff: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EditFileOutput {
    #[serde(rename = "filePath")]
    pub file_path: String,
    #[serde(rename = "oldString")]
    pub old_string: String,
    #[serde(rename = "newString")]
    pub new_string: String,
    #[serde(rename = "originalFile", skip_serializing, default)]
    pub original_file: String,
    #[serde(rename = "structuredPatch")]
    pub structured_patch: Vec<StructuredPatchHunk>,
    #[serde(rename = "userModified")]
    pub user_modified: bool,
    #[serde(rename = "replaceAll")]
    pub replace_all: bool,
    #[serde(rename = "gitDiff")]
    pub git_diff: Option<serde_json::Value>,
    /// Présent quand le texte a été trouvé de façon approximative.
    #[serde(rename = "matchNote", skip_serializing_if = "Option::is_none", default)]
    pub match_note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GlobSearchOutput {
    #[serde(rename = "durationMs")]
    pub duration_ms: u128,
    #[serde(rename = "numFiles")]
    pub num_files: usize,
    pub filenames: Vec<String>,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GrepSearchInput {
    pub pattern: String,
    pub path: Option<String>,
    pub glob: Option<String>,
    #[serde(rename = "output_mode")]
    pub output_mode: Option<String>,
    #[serde(rename = "-B")]
    pub before: Option<usize>,
    #[serde(rename = "-A")]
    pub after: Option<usize>,
    #[serde(rename = "-C")]
    pub context_short: Option<usize>,
    pub context: Option<usize>,
    #[serde(rename = "-n")]
    pub line_numbers: Option<bool>,
    #[serde(rename = "-i")]
    pub case_insensitive: Option<bool>,
    #[serde(rename = "type")]
    pub file_type: Option<String>,
    pub head_limit: Option<usize>,
    pub offset: Option<usize>,
    pub multiline: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GrepSearchOutput {
    pub mode: Option<String>,
    #[serde(rename = "numFiles")]
    pub num_files: usize,
    pub filenames: Vec<String>,
    pub content: Option<String>,
    #[serde(rename = "numLines")]
    pub num_lines: Option<usize>,
    #[serde(rename = "numMatches")]
    pub num_matches: Option<usize>,
    #[serde(rename = "appliedLimit")]
    pub applied_limit: Option<usize>,
    #[serde(rename = "appliedOffset")]
    pub applied_offset: Option<usize>,
}

pub fn read_file(
    path: &str,
    offset: Option<usize>,
    limit: Option<usize>,
) -> io::Result<ReadFileOutput> {
    let absolute_path = normalize_path(path)?;
    let content = fs::read_to_string(&absolute_path)?;
    let lines: Vec<&str> = content.lines().collect();
    let start_index = offset.unwrap_or(0).min(lines.len());
    let end_index = limit.map_or(lines.len(), |limit| {
        start_index.saturating_add(limit).min(lines.len())
    });
    let selected = lines[start_index..end_index].join("\n");

    Ok(ReadFileOutput {
        kind: String::from("text"),
        file: TextFilePayload {
            file_path: absolute_path.to_string_lossy().into_owned(),
            content: selected,
            num_lines: end_index.saturating_sub(start_index),
            start_line: start_index.saturating_add(1),
            total_lines: lines.len(),
        },
    })
}

pub fn write_file(path: &str, content: &str) -> io::Result<WriteFileOutput> {
    let absolute_path = normalize_path_allow_missing(path)?;
    let original_file = fs::read_to_string(&absolute_path).ok();
    if let Some(parent) = absolute_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&absolute_path, content)?;

    Ok(WriteFileOutput {
        kind: if original_file.is_some() {
            String::from("update")
        } else {
            String::from("create")
        },
        file_path: absolute_path.to_string_lossy().into_owned(),
        content: content.to_owned(),
        structured_patch: make_patch(original_file.as_deref().unwrap_or(""), content),
        original_file,
        git_diff: None,
    })
}

pub fn edit_file(
    path: &str,
    old_string: &str,
    new_string: &str,
    replace_all: bool,
) -> io::Result<EditFileOutput> {
    let absolute_path = normalize_path(path)?;
    let original_file = fs::read_to_string(&absolute_path)?;
    if old_string == new_string {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "old_string and new_string must differ",
        ));
    }
    if old_string.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "old_string must not be empty (use write_file to create or overwrite a file)",
        ));
    }
    let (updated, match_note) = apply_edit(&original_file, old_string, new_string, replace_all)
        .map_err(|message| io::Error::new(io::ErrorKind::NotFound, message))?;
    fs::write(&absolute_path, &updated)?;

    Ok(EditFileOutput {
        file_path: absolute_path.to_string_lossy().into_owned(),
        old_string: old_string.to_owned(),
        new_string: new_string.to_owned(),
        original_file: original_file.clone(),
        structured_patch: make_patch(&original_file, &updated),
        user_modified: false,
        replace_all,
        git_diff: None,
        match_note,
    })
}

/// Remplace `old_string` par `new_string`. Recherche exacte d'abord, puis de
/// plus en plus tolérante (fins de ligne, espaces de fin, indentation) : les
/// modèles recopient souvent mal les blancs. Une occurrence ambiguë est refusée
/// sauf `replace_all`.
fn apply_edit(
    original: &str,
    old_string: &str,
    new_string: &str,
    replace_all: bool,
) -> Result<(String, Option<String>), String> {
    let count = original.matches(old_string).count();
    if count > 0 {
        if count > 1 && !replace_all {
            return Err(ambiguous_message(count));
        }
        let updated = if replace_all {
            original.replace(old_string, new_string)
        } else {
            original.replacen(old_string, new_string, 1)
        };
        return Ok((updated, None));
    }

    // Fichier en CRLF (Windows), texte du modèle en LF.
    if original.contains("\r\n") && !old_string.contains("\r\n") {
        let old_crlf = old_string.replace('\n', "\r\n");
        let new_crlf = new_string.replace("\r\n", "\n").replace('\n', "\r\n");
        let count = original.matches(old_crlf.as_str()).count();
        if count > 0 {
            if count > 1 && !replace_all {
                return Err(ambiguous_message(count));
            }
            let updated = if replace_all {
                original.replace(&old_crlf, &new_crlf)
            } else {
                original.replacen(&old_crlf, &new_crlf, 1)
            };
            return Ok((
                updated,
                Some("fins de ligne CRLF du fichier conservées".to_string()),
            ));
        }
    }

    for (level, note) in [
        (
            MatchLevel::TrailingWhitespace,
            "espaces de fin de ligne ignorés",
        ),
        (
            MatchLevel::Indentation,
            "indentation ajustée à celle du fichier",
        ),
    ] {
        if let Some(result) = line_match(original, old_string, new_string, replace_all, level)? {
            return Ok((
                result,
                Some(format!("correspondance approximative : {note}")),
            ));
        }
    }
    Err(not_found_message(original, old_string))
}

fn ambiguous_message(count: usize) -> String {
    format!(
        "old_string appears {count} times in the file: add surrounding lines to make it unique, or set replace_all to true"
    )
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum MatchLevel {
    TrailingWhitespace,
    Indentation,
}

fn same_line(file_line: &str, old_line: &str, level: MatchLevel) -> bool {
    match level {
        MatchLevel::TrailingWhitespace => file_line.trim_end() == old_line.trim_end(),
        MatchLevel::Indentation => file_line.trim() == old_line.trim(),
    }
}

fn leading_whitespace(line: &str) -> &str {
    &line[..line.len() - line.trim_start().len()]
}

/// Recherche ligne à ligne ; renvoie le fichier modifié si au moins une
/// occurrence (une seule sans `replace_all`) est trouvée.
fn line_match(
    original: &str,
    old_string: &str,
    new_string: &str,
    replace_all: bool,
    level: MatchLevel,
) -> Result<Option<String>, String> {
    let newline = if original.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let file_lines: Vec<&str> = original
        .split('\n')
        .map(|line| line.trim_end_matches('\r'))
        .collect();
    let old_lines: Vec<&str> = old_string
        .trim_end_matches(['\n', '\r'])
        .split('\n')
        .map(|line| line.trim_end_matches('\r'))
        .collect();
    if old_lines.iter().all(|line| line.trim().is_empty()) || old_lines.len() > file_lines.len() {
        return Ok(None);
    }
    let mut starts = Vec::new();
    let mut index = 0;
    while index + old_lines.len() <= file_lines.len() {
        if old_lines
            .iter()
            .zip(&file_lines[index..])
            .all(|(old, file)| same_line(file, old, level))
        {
            starts.push(index);
            index += old_lines.len();
        } else {
            index += 1;
        }
    }
    if starts.is_empty() {
        return Ok(None);
    }
    if starts.len() > 1 && !replace_all {
        return Err(ambiguous_message(starts.len()));
    }
    let new_lines: Vec<&str> = new_string
        .trim_end_matches(['\n', '\r'])
        .split('\n')
        .map(|line| line.trim_end_matches('\r'))
        .collect();
    let mut output: Vec<String> = Vec::new();
    let mut cursor = 0;
    for start in starts {
        output.extend(
            file_lines[cursor..start]
                .iter()
                .map(|line| (*line).to_string()),
        );
        let replacement = if level == MatchLevel::Indentation {
            reindent(&new_lines, old_lines[0], file_lines[start])
        } else {
            new_lines.iter().map(|line| (*line).to_string()).collect()
        };
        if !new_string.is_empty() {
            output.extend(replacement);
        }
        cursor = start + old_lines.len();
    }
    output.extend(file_lines[cursor..].iter().map(|line| (*line).to_string()));
    Ok(Some(output.join(newline)))
}

/// Applique au nouveau texte le décalage d'indentation entre le modèle et le fichier.
fn reindent(new_lines: &[&str], old_first: &str, file_first: &str) -> Vec<String> {
    let old_indent = leading_whitespace(old_first);
    let file_indent = leading_whitespace(file_first);
    new_lines
        .iter()
        .map(|line| {
            if line.trim().is_empty() {
                return String::new();
            }
            let current = leading_whitespace(line);
            let rest = &line[current.len()..];
            if let Some(extra) = current.strip_prefix(old_indent) {
                format!("{file_indent}{extra}{rest}")
            } else {
                format!("{file_indent}{rest}")
            }
        })
        .collect()
}

/// Message d'échec utile au modèle : ligne du fichier la plus proche de la
/// première ligne cherchée.
fn not_found_message(original: &str, old_string: &str) -> String {
    let first = old_string
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or_default();
    let closest = original
        .lines()
        .enumerate()
        .map(|(index, line)| (index, line, similarity(line.trim(), first)))
        .filter(|(_, _, score)| *score > 0.5)
        .max_by(|left, right| left.2.total_cmp(&right.2));
    let mut message =
        "old_string not found in file (even ignoring whitespace and indentation)".to_string();
    if let Some((index, line, _)) = closest {
        message.push_str(&format!(
            ". Closest line {}: `{}`",
            index + 1,
            line.trim().chars().take(160).collect::<String>()
        ));
    }
    message.push_str(". Re-read the file with read_file and copy the exact text.");
    message
}

/// Similarité de 0 à 1 (coefficient de Dice sur les paires de caractères).
fn similarity(left: &str, right: &str) -> f64 {
    fn bigrams(text: &str) -> Vec<(char, char)> {
        let chars: Vec<char> = text.chars().collect();
        chars.windows(2).map(|pair| (pair[0], pair[1])).collect()
    }
    let left = bigrams(left);
    let mut right = bigrams(right);
    if left.is_empty() || right.is_empty() {
        return 0.0;
    }
    let total = left.len() + right.len();
    let mut common = 0usize;
    for pair in &left {
        if let Some(position) = right.iter().position(|candidate| candidate == pair) {
            right.swap_remove(position);
            common += 1;
        }
    }
    #[allow(clippy::cast_precision_loss)]
    let score = (2 * common) as f64 / total as f64;
    score
}

pub fn glob_search(pattern: &str, path: Option<&str>) -> io::Result<GlobSearchOutput> {
    let started = Instant::now();
    let base_dir = path
        .map(normalize_path)
        .transpose()?
        .unwrap_or(std::env::current_dir()?);
    let search_pattern = if Path::new(pattern).is_absolute() {
        pattern.to_owned()
    } else {
        base_dir.join(pattern).to_string_lossy().into_owned()
    };

    let mut matches = Vec::new();
    let entries = glob::glob(&search_pattern)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error.to_string()))?;
    for entry in entries.flatten() {
        if entry.is_file() {
            matches.push(entry);
        }
    }

    matches.sort_by_key(|path| {
        fs::metadata(path)
            .and_then(|metadata| metadata.modified())
            .ok()
            .map(Reverse)
    });

    let truncated = matches.len() > 100;
    let filenames = matches
        .into_iter()
        .take(100)
        .map(|path| path.to_string_lossy().into_owned())
        .collect::<Vec<_>>();

    Ok(GlobSearchOutput {
        duration_ms: started.elapsed().as_millis(),
        num_files: filenames.len(),
        filenames,
        truncated,
    })
}

pub fn grep_search(input: &GrepSearchInput) -> io::Result<GrepSearchOutput> {
    let base_path = input
        .path
        .as_deref()
        .map(normalize_path)
        .transpose()?
        .unwrap_or(std::env::current_dir()?);

    let regex = RegexBuilder::new(&input.pattern)
        .case_insensitive(input.case_insensitive.unwrap_or(false))
        .dot_matches_new_line(input.multiline.unwrap_or(false))
        .build()
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error.to_string()))?;

    let glob_filter = input
        .glob
        .as_deref()
        .map(Pattern::new)
        .transpose()
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error.to_string()))?;
    let file_type = input.file_type.as_deref();
    let output_mode = input
        .output_mode
        .clone()
        .unwrap_or_else(|| String::from("files_with_matches"));
    let context = input.context.or(input.context_short).unwrap_or(0);

    let mut filenames = Vec::new();
    let mut content_lines = Vec::new();
    let mut total_matches = 0usize;

    for file_path in collect_search_files(&base_path)? {
        if !matches_optional_filters(&file_path, glob_filter.as_ref(), file_type) {
            continue;
        }

        let Ok(file_contents) = fs::read_to_string(&file_path) else {
            continue;
        };

        if output_mode == "count" {
            let count = regex.find_iter(&file_contents).count();
            if count > 0 {
                filenames.push(file_path.to_string_lossy().into_owned());
                total_matches += count;
            }
            continue;
        }

        let lines: Vec<&str> = file_contents.lines().collect();
        let mut matched_lines = Vec::new();
        for (index, line) in lines.iter().enumerate() {
            if regex.is_match(line) {
                total_matches += 1;
                matched_lines.push(index);
            }
        }

        if matched_lines.is_empty() {
            continue;
        }

        filenames.push(file_path.to_string_lossy().into_owned());
        if output_mode == "content" {
            for index in matched_lines {
                let start = index.saturating_sub(input.before.unwrap_or(context));
                let end = (index + input.after.unwrap_or(context) + 1).min(lines.len());
                for (current, line) in lines.iter().enumerate().take(end).skip(start) {
                    let prefix = if input.line_numbers.unwrap_or(true) {
                        format!("{}:{}:", file_path.to_string_lossy(), current + 1)
                    } else {
                        format!("{}:", file_path.to_string_lossy())
                    };
                    content_lines.push(format!("{prefix}{line}"));
                }
            }
        }
    }

    let (filenames, applied_limit, applied_offset) =
        apply_limit(filenames, input.head_limit, input.offset);
    let content_output = if output_mode == "content" {
        let (lines, limit, offset) = apply_limit(content_lines, input.head_limit, input.offset);
        return Ok(GrepSearchOutput {
            mode: Some(output_mode),
            num_files: filenames.len(),
            filenames,
            num_lines: Some(lines.len()),
            content: Some(lines.join("\n")),
            num_matches: None,
            applied_limit: limit,
            applied_offset: offset,
        });
    } else {
        None
    };

    Ok(GrepSearchOutput {
        mode: Some(output_mode.clone()),
        num_files: filenames.len(),
        filenames,
        content: content_output,
        num_lines: None,
        num_matches: (output_mode == "count").then_some(total_matches),
        applied_limit,
        applied_offset,
    })
}

fn collect_search_files(base_path: &Path) -> io::Result<Vec<PathBuf>> {
    if base_path.is_file() {
        return Ok(vec![base_path.to_path_buf()]);
    }

    let mut files = Vec::new();
    for entry in WalkDir::new(base_path) {
        let entry = entry.map_err(|error| io::Error::other(error.to_string()))?;
        if entry.file_type().is_file() {
            files.push(entry.path().to_path_buf());
        }
    }
    Ok(files)
}

fn matches_optional_filters(
    path: &Path,
    glob_filter: Option<&Pattern>,
    file_type: Option<&str>,
) -> bool {
    if let Some(glob_filter) = glob_filter {
        let path_string = path.to_string_lossy();
        if !glob_filter.matches(&path_string) && !glob_filter.matches_path(path) {
            return false;
        }
    }

    if let Some(file_type) = file_type {
        let extension = path.extension().and_then(|extension| extension.to_str());
        if extension != Some(file_type) {
            return false;
        }
    }

    true
}

fn apply_limit<T>(
    items: Vec<T>,
    limit: Option<usize>,
    offset: Option<usize>,
) -> (Vec<T>, Option<usize>, Option<usize>) {
    let offset_value = offset.unwrap_or(0);
    let mut items = items.into_iter().skip(offset_value).collect::<Vec<_>>();
    let explicit_limit = limit.unwrap_or(250);
    if explicit_limit == 0 {
        return (items, None, (offset_value > 0).then_some(offset_value));
    }

    let truncated = items.len() > explicit_limit;
    items.truncate(explicit_limit);
    (
        items,
        truncated.then_some(explicit_limit),
        (offset_value > 0).then_some(offset_value),
    )
}

fn make_patch(original: &str, updated: &str) -> Vec<StructuredPatchHunk> {
    let mut lines = Vec::new();
    for line in original.lines() {
        lines.push(format!("-{line}"));
    }
    for line in updated.lines() {
        lines.push(format!("+{line}"));
    }

    vec![StructuredPatchHunk {
        old_start: 1,
        old_lines: original.lines().count(),
        new_start: 1,
        new_lines: updated.lines().count(),
        lines,
    }]
}

fn normalize_path(path: &str) -> io::Result<PathBuf> {
    let candidate = if Path::new(path).is_absolute() {
        PathBuf::from(path)
    } else {
        std::env::current_dir()?.join(path)
    };
    candidate.canonicalize()
}

fn normalize_path_allow_missing(path: &str) -> io::Result<PathBuf> {
    let candidate = if Path::new(path).is_absolute() {
        PathBuf::from(path)
    } else {
        std::env::current_dir()?.join(path)
    };

    if let Ok(canonical) = candidate.canonicalize() {
        return Ok(canonical);
    }

    if let Some(parent) = candidate.parent() {
        let canonical_parent = parent
            .canonicalize()
            .unwrap_or_else(|_| parent.to_path_buf());
        if let Some(name) = candidate.file_name() {
            return Ok(canonical_parent.join(name));
        }
    }

    Ok(candidate)
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::{edit_file, glob_search, grep_search, read_file, write_file, GrepSearchInput};

    fn temp_path(name: &str) -> std::path::PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time should move forward")
            .as_nanos();
        std::env::temp_dir().join(format!("claw-native-{name}-{unique}"))
    }

    #[test]
    fn edits_tolerate_line_endings_whitespace_and_indentation() {
        use super::apply_edit;

        // Fichier en CRLF, texte du modèle en LF : les CRLF sont conservés.
        let (updated, note) = apply_edit(
            "fn a() {\r\n    1\r\n}\r\n",
            "fn a() {\n    1\n}",
            "fn a() {\n    2\n}",
            false,
        )
        .expect("crlf");
        assert_eq!(updated, "fn a() {\r\n    2\r\n}\r\n");
        assert!(note.is_some());

        // Espaces de fin de ligne différents.
        let (updated, _) = apply_edit(
            "let x = 1;   \nlet y = 2;\n",
            "let x = 1;\nlet y = 2;",
            "let x = 3;\nlet y = 4;",
            false,
        )
        .expect("trailing");
        assert_eq!(updated, "let x = 3;\nlet y = 4;\n");

        // Indentation recopiée sans le niveau d'imbrication : réindentée.
        let file = "impl A {\n    fn f(&self) {\n        old();\n    }\n}\n";
        let (updated, note) = apply_edit(
            file,
            "fn f(&self) {\n    old();\n}",
            "fn f(&self) {\n    new();\n    other();\n}",
            false,
        )
        .expect("indent");
        assert_eq!(
            updated,
            "impl A {\n    fn f(&self) {\n        new();\n        other();\n    }\n}\n"
        );
        assert!(note.expect("note").contains("indentation"));

        // Occurrences multiples : refus sans replace_all, remplacement avec.
        let error = apply_edit("a\nb\na\n", "a", "c", false).expect_err("ambiguous");
        assert!(error.contains("2 times"));
        assert_eq!(
            apply_edit("a\nb\na\n", "a", "c", true).expect("all").0,
            "c\nb\nc\n"
        );

        // Introuvable : la ligne la plus proche est indiquée.
        let error = apply_edit(
            "fn compute_total(items: &[u32]) -> u32 {\n",
            "fn compute_totals(items: &[u32]) -> u64 {",
            "x",
            false,
        )
        .expect_err("missing");
        assert!(error.contains("Closest line 1"), "{error}");
    }

    #[test]
    fn reads_and_writes_files() {
        let path = temp_path("read-write.txt");
        let write_output = write_file(path.to_string_lossy().as_ref(), "one\ntwo\nthree")
            .expect("write should succeed");
        assert_eq!(write_output.kind, "create");

        let read_output = read_file(path.to_string_lossy().as_ref(), Some(1), Some(1))
            .expect("read should succeed");
        assert_eq!(read_output.file.content, "two");
    }

    #[test]
    fn edits_file_contents() {
        let path = temp_path("edit.txt");
        write_file(path.to_string_lossy().as_ref(), "alpha beta alpha")
            .expect("initial write should succeed");
        let output = edit_file(path.to_string_lossy().as_ref(), "alpha", "omega", true)
            .expect("edit should succeed");
        assert!(output.replace_all);
    }

    #[test]
    fn globs_and_greps_directory() {
        let dir = temp_path("search-dir");
        std::fs::create_dir_all(&dir).expect("directory should be created");
        let file = dir.join("demo.rs");
        write_file(
            file.to_string_lossy().as_ref(),
            "fn main() {\n println!(\"hello\");\n}\n",
        )
        .expect("file write should succeed");

        let globbed = glob_search("**/*.rs", Some(dir.to_string_lossy().as_ref()))
            .expect("glob should succeed");
        assert_eq!(globbed.num_files, 1);

        let grep_output = grep_search(&GrepSearchInput {
            pattern: String::from("hello"),
            path: Some(dir.to_string_lossy().into_owned()),
            glob: Some(String::from("**/*.rs")),
            output_mode: Some(String::from("content")),
            before: None,
            after: None,
            context_short: None,
            context: None,
            line_numbers: Some(true),
            case_insensitive: Some(false),
            file_type: None,
            head_limit: Some(10),
            offset: Some(0),
            multiline: Some(false),
        })
        .expect("grep should succeed");
        assert!(grep_output.content.unwrap_or_default().contains("hello"));
    }
}
