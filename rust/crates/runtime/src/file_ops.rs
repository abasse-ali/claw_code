use std::cmp::Reverse;
use std::fmt::Write as _;
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
    /// Le fichier continue au-delà de ce qui est montré : comment lire la suite.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub note: Option<String>,
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
    /// Le codage d'origine du fichier n'a pas pu être gardé.
    #[serde(
        rename = "encodingNote",
        skip_serializing_if = "Option::is_none",
        default
    )]
    pub encoding_note: Option<String>,
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
    /// Ce qui n'a pas été cherché (dossiers de dépendances…), quand rien n'est trouvé.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GrepSearchInput {
    pub pattern: String,
    pub path: Option<String>,
    pub glob: Option<String>,
    #[serde(rename = "output_mode")]
    pub output_mode: Option<String>,
    // Les noms sans tiret sont ceux d'un schéma réduit (serveur qui refuse
    // les schémas tels quels).
    #[serde(rename = "-B", alias = "B")]
    pub before: Option<usize>,
    #[serde(rename = "-A", alias = "A")]
    pub after: Option<usize>,
    #[serde(rename = "-C", alias = "C")]
    pub context_short: Option<usize>,
    pub context: Option<usize>,
    #[serde(rename = "-n", alias = "n")]
    pub line_numbers: Option<bool>,
    #[serde(rename = "-i", alias = "i")]
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
    /// Ce qui n'a pas été cherché (gros fichiers, dossiers de dépendances…).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub note: Option<String>,
}

/// Lignes lues quand le modèle ne donne pas de limite : de quoi comprendre un
/// fichier sans remplir le contexte de toutes les requêtes suivantes.
pub const DEFAULT_READ_LINES: usize = 2_000;
/// Au-delà, une ligne (code minifié, données sur une ligne) est raccourcie.
const MAX_READ_LINE_CHARS: usize = 2_000;

fn shorten_line(line: &str) -> String {
    let count = line.chars().count();
    if count <= MAX_READ_LINE_CHARS {
        return line.to_string();
    }
    let kept: String = line.chars().take(MAX_READ_LINE_CHARS).collect();
    format!("{kept} … (line shortened: {count} characters)")
}

// --- Codage des fichiers texte -------------------------------------------------------

/// Codage d'un fichier texte, gardé quand Claw le réécrit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TextEncoding {
    Utf8,
    /// UTF-8 avec ce nombre d'octets invalides, lus comme U+FFFD.
    Utf8Lossy(usize),
    /// Windows-1252 (Excel, anciens fichiers Windows), qui comprend Latin-1.
    Windows1252,
    /// UTF-16 avec sa marque d'ordre (`>` de `PowerShell` 5, fichiers .reg…).
    Utf16 {
        big_endian: bool,
    },
}

impl TextEncoding {
    const fn name(self) -> &'static str {
        match self {
            Self::Utf8 | Self::Utf8Lossy(_) => "UTF-8",
            Self::Windows1252 => "Windows-1252",
            Self::Utf16 { .. } => "UTF-16",
        }
    }

    /// Ce que le modèle doit savoir du codage en lisant le fichier.
    fn note(self) -> Option<String> {
        match self {
            Self::Utf8 => None,
            Self::Utf8Lossy(count) => Some(format!(
                "This file is UTF-8 with {count} invalid byte(s), shown as U+FFFD (an edit writes them as U+FFFD)."
            )),
            Self::Windows1252 | Self::Utf16 { .. } => Some(format!(
                "This file is encoded in {}, not UTF-8: edit_file and write_file keep that encoding.",
                self.name()
            )),
        }
    }
}

/// Octets 0x80 à 0x9F de Windows-1252. Les autres octets valent leur point
/// de code Latin-1 ; les cinq octets non définis gardent leur contrôle C1.
const WINDOWS_1252_HIGH: [char; 32] = [
    '\u{20AC}', '\u{81}', '\u{201A}', '\u{192}', '\u{201E}', '\u{2026}', '\u{2020}', '\u{2021}',
    '\u{2C6}', '\u{2030}', '\u{160}', '\u{2039}', '\u{152}', '\u{8D}', '\u{17D}', '\u{8F}',
    '\u{90}', '\u{2018}', '\u{2019}', '\u{201C}', '\u{201D}', '\u{2022}', '\u{2013}', '\u{2014}',
    '\u{2DC}', '\u{2122}', '\u{161}', '\u{203A}', '\u{153}', '\u{9D}', '\u{17E}', '\u{178}',
];

/// Image, archive, exécutable : un octet nul dans le début, hors UTF-16.
fn is_binary(bytes: &[u8]) -> bool {
    !(bytes.starts_with(&[0xFF, 0xFE]) || bytes.starts_with(&[0xFE, 0xFF]))
        && bytes.iter().take(8_192).any(|byte| *byte == 0)
}

/// Texte d'un fichier et son codage : UTF-16 (marque d'ordre), UTF-8, UTF-8
/// abîmé par quelques octets (gardé en UTF-8), sinon Windows-1252.
fn decode_text(bytes: &[u8]) -> (String, TextEncoding) {
    for (mark, big_endian) in [([0xFF_u8, 0xFE], false), ([0xFE, 0xFF], true)] {
        if let Some(rest) = bytes.strip_prefix(&mark) {
            let units = rest.chunks_exact(2).map(|pair| {
                let pair = [pair[0], pair[1]];
                if big_endian {
                    u16::from_be_bytes(pair)
                } else {
                    u16::from_le_bytes(pair)
                }
            });
            let text = char::decode_utf16(units)
                .map(|unit| unit.unwrap_or(char::REPLACEMENT_CHARACTER))
                .collect();
            return (text, TextEncoding::Utf16 { big_endian });
        }
    }
    if let Ok(text) = std::str::from_utf8(bytes) {
        return (text.to_string(), TextEncoding::Utf8);
    }
    // Des caractères UTF-8 bien formés à côté de rares octets invalides : de
    // l'UTF-8 abîmé. Sinon (un « é » sur un octet), du Windows-1252.
    let (well_formed, invalid) = utf8_census(bytes);
    if well_formed > 0 && well_formed >= invalid {
        return (
            String::from_utf8_lossy(bytes).into_owned(),
            TextEncoding::Utf8Lossy(invalid),
        );
    }
    let text = bytes
        .iter()
        .map(|&byte| match byte {
            0x80..=0x9F => WINDOWS_1252_HIGH[usize::from(byte - 0x80)],
            _ => char::from(byte),
        })
        .collect();
    (text, TextEncoding::Windows1252)
}

/// Caractères non ASCII bien formés et séquences invalides d'octets qui ne
/// sont pas tout à fait de l'UTF-8.
fn utf8_census(mut bytes: &[u8]) -> (usize, usize) {
    let non_ascii = |text: &str| text.chars().filter(|ch| !ch.is_ascii()).count();
    let (mut well_formed, mut invalid) = (0, 0);
    loop {
        match std::str::from_utf8(bytes) {
            Ok(text) => return (well_formed + non_ascii(text), invalid),
            Err(error) => {
                let (valid, rest) = bytes.split_at(error.valid_up_to());
                well_formed += std::str::from_utf8(valid).map_or(0, non_ascii);
                invalid += 1;
                bytes = &rest[error.error_len().unwrap_or(rest.len())..];
            }
        }
    }
}

/// Octets de `text` dans `encoding`, ou le premier caractère que ce codage ne
/// peut pas représenter.
fn encode_text(text: &str, encoding: TextEncoding) -> Result<Vec<u8>, char> {
    match encoding {
        TextEncoding::Utf8 | TextEncoding::Utf8Lossy(_) => Ok(text.as_bytes().to_vec()),
        TextEncoding::Windows1252 => text
            .chars()
            .map(|ch| windows_1252_byte(ch).ok_or(ch))
            .collect(),
        TextEncoding::Utf16 { big_endian } => {
            let mut bytes = if big_endian {
                vec![0xFE, 0xFF]
            } else {
                vec![0xFF, 0xFE]
            };
            for unit in text.encode_utf16() {
                bytes.extend(if big_endian {
                    unit.to_be_bytes()
                } else {
                    unit.to_le_bytes()
                });
            }
            Ok(bytes)
        }
    }
}

fn windows_1252_byte(ch: char) -> Option<u8> {
    let code = u32::from(ch);
    if code < 0x80 || (0xA0..=0xFF).contains(&code) {
        return u8::try_from(code).ok();
    }
    WINDOWS_1252_HIGH
        .iter()
        .position(|&high| high == ch)
        .and_then(|index| u8::try_from(0x80 + index).ok())
}

fn describe_char(ch: char) -> String {
    format!("'{ch}' (U+{:04X})", u32::from(ch))
}

/// Texte d'un fichier quel que soit son codage ; `None` s'il est binaire ou
/// illisible.
pub(crate) fn read_any_text(path: &Path) -> Option<String> {
    read_text(path).ok().map(|(text, _)| text)
}

/// Texte d'un fichier et son codage ; un fichier binaire est une erreur claire.
fn read_text(path: &Path) -> io::Result<(String, TextEncoding)> {
    // Un fichier énorme (journal, export) ne tient pas en mémoire deux fois.
    let size = fs::metadata(path)?.len();
    if size > MAX_TEXT_FILE_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "{} is {} MB: too large to load. Use bash on it (head, tail, sed -n, grep) to read the part you need",
                display_path(path),
                size / (1024 * 1024)
            ),
        ));
    }
    let bytes = fs::read(path)?;
    if is_binary(&bytes) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "{} is a binary file ({} bytes), not text: read_file and edit_file cannot handle it",
                display_path(path),
                bytes.len()
            ),
        ));
    }
    Ok(decode_text(&bytes))
}

pub fn read_file(
    path: &str,
    offset: Option<usize>,
    limit: Option<usize>,
) -> io::Result<ReadFileOutput> {
    let absolute_path = normalize_path(path)?;
    let (content, encoding) = read_text(&absolute_path)?;
    let lines: Vec<&str> = content.lines().collect();
    let start_index = offset.unwrap_or(0).min(lines.len());
    let end_index = start_index
        .saturating_add(limit.unwrap_or(DEFAULT_READ_LINES))
        .min(lines.len());
    let selected = lines[start_index..end_index]
        .iter()
        .map(|line| shorten_line(line))
        .collect::<Vec<_>>()
        .join("\n");
    let mut notes = Vec::new();
    if end_index < lines.len() {
        notes.push(format!(
            "Showing lines {}-{end_index} of {}. Call read_file again with offset {end_index} to read on, or use grep_search to find the part you need.",
            start_index + 1,
            lines.len()
        ));
    } else if let Some(offset) = offset.filter(|offset| *offset >= lines.len() && *offset > 0) {
        // Rien à montrer : le modèle doit savoir pourquoi, pas croire le fichier vide.
        notes.push(format!(
            "Nothing to show: offset {offset} is past the end of the file, which has {} line(s) (offset 0 is the first line).",
            lines.len()
        ));
    }
    notes.extend(encoding.note());
    let note = (!notes.is_empty()).then(|| notes.join(" "));

    Ok(ReadFileOutput {
        kind: String::from("text"),
        file: TextFilePayload {
            file_path: display_path(&absolute_path),
            content: selected,
            num_lines: end_index.saturating_sub(start_index),
            start_line: start_index.saturating_add(1),
            total_lines: lines.len(),
            note,
        },
    })
}

pub fn write_file(path: &str, content: &str) -> io::Result<WriteFileOutput> {
    let absolute_path = normalize_path_allow_missing(path)?;
    // Fichier existant : son texte (pour le diff) et son codage, qui est gardé
    // (un CSV d'Excel en Windows-1252 le reste).
    let original = read_text(&absolute_path).ok();
    if let Some(parent) = absolute_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let encoding = original
        .as_ref()
        .map_or(TextEncoding::Utf8, |(_, encoding)| *encoding);
    let (bytes, encoding_note) = match encode_text(content, encoding) {
        // Codage d'origine gardé : dit au modèle quand ce n'est pas de l'UTF-8.
        Ok(bytes) => (
            bytes,
            matches!(
                encoding,
                TextEncoding::Windows1252 | TextEncoding::Utf16 { .. }
            )
            .then(|| format!("Written in the file's encoding, {}.", encoding.name())),
        ),
        Err(ch) => (
            content.as_bytes().to_vec(),
            Some(format!(
                "The file was encoded in {}, which cannot store {}: it is now UTF-8.",
                encoding.name(),
                describe_char(ch)
            )),
        ),
    };
    fs::write(&absolute_path, bytes)?;
    let original_file = original.map(|(text, _)| text);

    Ok(WriteFileOutput {
        kind: if original_file.is_some() {
            String::from("update")
        } else {
            String::from("create")
        },
        file_path: display_path(&absolute_path),
        content: content.to_owned(),
        structured_patch: make_patch(original_file.as_deref().unwrap_or(""), content),
        original_file,
        git_diff: None,
        encoding_note,
    })
}

pub fn edit_file(
    path: &str,
    old_string: &str,
    new_string: &str,
    replace_all: bool,
) -> io::Result<EditFileOutput> {
    let absolute_path = normalize_path(path)?;
    let (original_file, encoding) = read_text(&absolute_path)?;
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
    let (updated, mut match_note) = apply_edit(&original_file, old_string, new_string, replace_all)
        .map_err(|message| io::Error::new(io::ErrorKind::NotFound, message))?;
    // Le fichier garde son codage ; un caractère qu'il ne peut pas stocker est
    // refusé plutôt que de changer le codage en douce.
    let bytes = encode_text(&updated, encoding).map_err(|ch| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "{} is encoded in {}, which cannot store {}: write it with characters this encoding has (for example ' instead of a typographic quote, -> instead of an arrow), or rewrite the whole file with write_file to turn it into UTF-8",
                display_path(&absolute_path),
                encoding.name(),
                describe_char(ch)
            ),
        )
    })?;
    if let TextEncoding::Utf8Lossy(count) = encoding {
        let note = format!("{count} octet(s) invalide(s) du fichier réécrit(s) en U+FFFD");
        match_note = Some(match match_note {
            Some(existing) => format!("{existing} ; {note}"),
            None => note,
        });
    }
    fs::write(&absolute_path, bytes)?;

    Ok(EditFileOutput {
        file_path: display_path(&absolute_path),
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

/// Ce que donnerait une écriture ou une modification, sans toucher au fichier :
/// de quoi montrer le changement proposé avant de l'autoriser.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditPreview {
    /// Chemin absolu du fichier.
    pub path: String,
    /// Contenu actuel ; `None` si le fichier n'existe pas encore.
    pub original: Option<String>,
    pub updated: String,
}

/// Aperçu de `write_file`.
pub fn preview_write(path: &str, content: &str) -> io::Result<EditPreview> {
    let absolute_path = normalize_path_allow_missing(path)?;
    Ok(EditPreview {
        path: display_path(&absolute_path),
        original: read_text(&absolute_path).ok().map(|(text, _)| text),
        updated: content.to_owned(),
    })
}

/// Aperçu de `edit_file`, avec la même recherche tolérante que l'outil.
pub fn preview_edit(
    path: &str,
    old_string: &str,
    new_string: &str,
    replace_all: bool,
) -> io::Result<EditPreview> {
    let absolute_path = normalize_path(path)?;
    let (original, _) = read_text(&absolute_path)?;
    let (updated, _) = apply_edit(&original, old_string, new_string, replace_all)
        .map_err(|message| io::Error::new(io::ErrorKind::NotFound, message))?;
    Ok(EditPreview {
        path: display_path(&absolute_path),
        original: Some(original),
        updated,
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
        let _ = write!(
            message,
            ". Closest line {}: `{}`",
            index + 1,
            line.trim().chars().take(160).collect::<String>()
        );
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
        display_path(&base_dir.join(pattern))
    };

    let mut matches = Vec::new();
    let entries = glob::glob(&search_pattern)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error.to_string()))?;
    // Dépendances, caches et sorties de build écartés : récents après une
    // installation, ils passeraient avant les fichiers du projet.
    let include_skipped = names_skipped_dir(pattern) || path.is_some_and(names_skipped_dir);
    let base_shown = PathBuf::from(display_path(&base_dir));
    let mut verdicts: std::collections::HashMap<PathBuf, bool> = std::collections::HashMap::new();
    let mut skipped = std::collections::BTreeSet::new();
    for entry in entries.flatten() {
        if !entry.is_file() {
            continue;
        }
        if !include_skipped {
            let (base, relative) = entry
                .strip_prefix(&base_shown)
                .map(|relative| (base_shown.as_path(), relative))
                .or_else(|_| {
                    entry
                        .strip_prefix(&base_dir)
                        .map(|relative| (base_dir.as_path(), relative))
                })
                .unwrap_or((Path::new(""), entry.as_path()));
            // Les dossiers entre la base et le fichier, pas le nom du fichier
            // (un script nommé `build` est du code).
            let mut dir = base.to_path_buf();
            let mut hidden = None;
            for component in relative.parent().into_iter().flat_map(Path::components) {
                dir.push(component);
                let name = component.as_os_str().to_string_lossy().into_owned();
                let generated = *verdicts
                    .entry(dir.clone())
                    .or_insert_with(|| generated_dir(&dir, &name, base));
                if generated {
                    hidden = Some(name);
                    break;
                }
            }
            if let Some(name) = hidden {
                skipped.insert(name);
                continue;
            }
        }
        matches.push(entry);
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
        .map(|path| display_path(&path))
        .collect::<Vec<_>>();

    let skipped: Vec<String> = skipped.into_iter().collect();
    Ok(GlobSearchOutput {
        duration_ms: started.elapsed().as_millis(),
        num_files: filenames.len(),
        note: filenames
            .is_empty()
            .then(|| search_note(&skipped, 0))
            .flatten(),
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

    let include_skipped = input.path.as_deref().is_some_and(names_skipped_dir)
        || input.glob.as_deref().is_some_and(names_skipped_dir);
    // Le fichier que nomme `path` est toujours lu, même gros (un journal).
    let named_file = base_path.is_file();
    let (files, skipped_dirs) = collect_search_files(&base_path, include_skipped);
    let mut large_files = 0usize;
    for file_path in files {
        if !matches_optional_filters(&file_path, glob_filter.as_ref(), file_type) {
            continue;
        }
        let Some(file_contents) = searchable_text(&file_path, named_file, &mut large_files) else {
            continue;
        };

        if output_mode == "count" {
            let count = regex.find_iter(&file_contents).count();
            if count > 0 {
                filenames.push(display_path(&file_path));
                total_matches += count;
            }
            continue;
        }

        let lines: Vec<&str> = file_contents.lines().collect();
        let matched_lines: Vec<usize> = (0..lines.len())
            .filter(|&index| regex.is_match(lines[index]))
            .collect();
        if matched_lines.is_empty() {
            continue;
        }
        total_matches += matched_lines.len();

        let shown_path = display_path(&file_path);
        if output_mode == "content" {
            push_matched_lines(
                input,
                context,
                &shown_path,
                &lines,
                &matched_lines,
                &mut content_lines,
            );
        }
        filenames.push(shown_path);
    }

    // Un résultat vide ne doit pas passer pour une absence : ce qui n'a pas
    // été cherché est dit (les dossiers sautés, seulement si rien n'est trouvé).
    let skipped_dirs = if filenames.is_empty() {
        skipped_dirs
    } else {
        Vec::new()
    };
    let note = search_note(&skipped_dirs, large_files);
    let (filenames, applied_limit, applied_offset) =
        apply_limit(filenames, input.head_limit, input.offset);
    if output_mode == "content" {
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
            note,
        });
    }

    Ok(GrepSearchOutput {
        num_matches: (output_mode == "count").then_some(total_matches),
        mode: Some(output_mode),
        num_files: filenames.len(),
        filenames,
        content: None,
        num_lines: None,
        applied_limit,
        applied_offset,
        note,
    })
}

/// Lignes trouvées d'un fichier et leur contexte, préfixées du chemin (et du
/// numéro de ligne). Deux contextes qui se recouvrent ne répètent pas leurs
/// lignes communes.
fn push_matched_lines(
    input: &GrepSearchInput,
    context: usize,
    shown_path: &str,
    lines: &[&str],
    matched: &[usize],
    out: &mut Vec<String>,
) {
    let mut next = 0;
    for &index in matched {
        let start = index
            .saturating_sub(input.before.unwrap_or(context))
            .max(next);
        let end = (index + input.after.unwrap_or(context) + 1).min(lines.len());
        for (current, line) in lines.iter().enumerate().take(end).skip(start) {
            let prefix = if input.line_numbers.unwrap_or(true) {
                format!("{shown_path}:{}:", current + 1)
            } else {
                format!("{shown_path}:")
            };
            out.push(format!("{prefix}{line}"));
        }
        next = next.max(end);
    }
}

/// Texte d'un fichier où chercher, quel que soit son codage (Windows-1252 et
/// UTF-16 compris, comme `read_file` les lit). `None` pour un fichier
/// illisible ou binaire, et pour un fichier trop gros trouvé en parcourant un
/// dossier (généré ou minifié : compté dans `large_files`) ; celui que le
/// modèle nomme (`named`) est toujours lu.
fn searchable_text(path: &Path, named: bool, large_files: &mut usize) -> Option<String> {
    let limit = if named {
        MAX_TEXT_FILE_BYTES
    } else {
        MAX_GREP_FILE_BYTES
    };
    if fs::metadata(path).is_ok_and(|metadata| metadata.len() > limit) {
        *large_files += 1;
        return None;
    }
    let bytes = fs::read(path).ok()?;
    (!is_binary(&bytes)).then(|| decode_text(&bytes).0)
}

/// Ce qu'une recherche n'a pas parcouru, pour qu'un résultat vide ne passe
/// pas pour une absence.
fn search_note(skipped_dirs: &[String], large_files: usize) -> Option<String> {
    let mut parts = Vec::new();
    // `.git` et `.claw` ne contiennent pas de code : inutile de le dire.
    let shown: Vec<&str> = skipped_dirs
        .iter()
        .map(String::as_str)
        .filter(|name| !matches!(*name, ".git" | ".claw"))
        .collect();
    if !shown.is_empty() {
        parts.push(format!(
            "Not searched: {} (dependencies, caches or build output); name one in path to search it.",
            shown.join(", ")
        ));
    }
    if large_files > 0 {
        parts.push(format!(
            "{large_files} file(s) too large to search here (over 4 MB): name one in path to search it, or use grep in bash for a file over 256 MB."
        ));
    }
    (!parts.is_empty()).then(|| parts.join(" "))
}

/// Au-delà, un fichier trouvé en parcourant un dossier n'est pas lu par une
/// recherche de contenu.
const MAX_GREP_FILE_BYTES: u64 = 4 * 1024 * 1024;
/// Au-delà, un fichier n'est jamais chargé en entier (lecture, modification,
/// recherche dans le fichier nommé).
const MAX_TEXT_FILE_BYTES: u64 = 256 * 1024 * 1024;

/// Fichiers où chercher, et noms des dossiers sautés : dépendances, caches,
/// sorties de build (sauf `include_skipped`). Un dossier illisible est ignoré
/// au lieu de faire échouer toute la recherche.
fn collect_search_files(base_path: &Path, include_skipped: bool) -> (Vec<PathBuf>, Vec<String>) {
    if base_path.is_file() {
        return (vec![base_path.to_path_buf()], Vec::new());
    }
    let mut skipped = Vec::new();
    let files = WalkDir::new(base_path)
        .into_iter()
        .filter_entry(|entry| {
            let name = entry.file_name().to_string_lossy();
            let skip = !include_skipped
                && entry.depth() > 0
                && entry.file_type().is_dir()
                && generated_dir(entry.path(), &name, base_path);
            if skip {
                skipped.push(name.into_owned());
            }
            !skip
        })
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_file())
        .map(|entry| entry.path().to_path_buf())
        .collect();
    skipped.sort();
    skipped.dedup();
    (files, skipped)
}

/// Dossier que les recherches sautent : dépôt git, dépendances, environnements
/// Python, caches d'outils, et sorties de build quand ce sont bien des sorties
/// (marquées par l'outil, ou ignorées par git) : un dossier `build`, `dist` ou
/// `target` peut aussi être du vrai code.
pub(crate) fn generated_dir(path: &Path, name: &str, base: &Path) -> bool {
    match name {
        ".git" | "node_modules" | "__pycache__" | ".claw" | ".venv" | "site-packages"
        | "dist-packages" | ".mypy_cache" | ".pytest_cache" | ".ruff_cache" | ".tox" | ".next"
        | ".nuxt" | ".svelte-kit" | ".turbo" | ".parcel-cache" | ".gradle" => true,
        "venv" | "env" => path.join("pyvenv.cfg").is_file(),
        "target" | "dist" | "build" | "out" | "coverage" => {
            path.join("CACHEDIR.TAG").is_file() || ignored_by_git(path, base)
        }
        _ => false,
    }
}

/// Un `.gitignore` du dossier parent ou d'un dossier au-dessus écarte ce
/// dossier (`build/`, `/dist`, `target`, `**/out`…). On remonte jusqu'à la
/// racine du dépôt ; hors dépôt, pas au-delà du dossier de la recherche.
fn ignored_by_git(path: &Path, base: &Path) -> bool {
    let Some(name) = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
    else {
        return false;
    };
    let top = path
        .ancestors()
        .skip(1)
        .take(32)
        .find(|dir| dir.join(".git").exists())
        .unwrap_or(base)
        .to_path_buf();
    let mut relative = name.clone();
    for dir in path.ancestors().skip(1) {
        if let Ok(rules) = fs::read_to_string(dir.join(".gitignore")) {
            if rules
                .lines()
                .any(|rule| gitignore_rule_matches(rule, &name, &relative))
            {
                return true;
            }
        }
        if dir == top || !dir.starts_with(&top) {
            break;
        }
        let Some(dir_name) = dir.file_name() else {
            break;
        };
        relative = format!("{}/{relative}", dir_name.to_string_lossy());
    }
    false
}

/// Une règle de `.gitignore` vise-t-elle ce dossier ? `relative` : son chemin
/// depuis le dossier du `.gitignore`.
fn gitignore_rule_matches(rule: &str, name: &str, relative: &str) -> bool {
    let rule = rule.trim();
    if rule.is_empty() || rule.starts_with('#') || rule.starts_with('!') {
        return false;
    }
    let rule = rule.trim_end_matches('/');
    let rule = rule.strip_prefix("**/").unwrap_or(rule);
    // `/dist` ou `web/dist` : depuis le dossier du `.gitignore` ; `dist` :
    // à toute profondeur.
    let (anchored, rule) = match rule.strip_prefix('/') {
        Some(rest) => (true, rest),
        None => (rule.contains('/'), rule),
    };
    let target = if anchored { relative } else { name };
    if rule.contains(['*', '?', '[']) {
        Pattern::new(rule).is_ok_and(|pattern| pattern.matches(target))
    } else {
        rule == target
    }
}

/// Le fichier `relative` (chemin depuis `root`) est sous un dossier que les
/// recherches sautent : une dépendance, un cache, une sortie de build.
pub(crate) fn under_generated_dir(root: &Path, relative: &Path) -> bool {
    let mut dir = root.to_path_buf();
    relative
        .parent()
        .into_iter()
        .flat_map(Path::components)
        .any(|component| {
            dir.push(component);
            generated_dir(&dir, &component.as_os_str().to_string_lossy(), root)
        })
}

/// Le chemin ou le motif nomme un dossier que les recherches sautent : le
/// modèle le vise exprès, on y cherche.
fn names_skipped_dir(text: &str) -> bool {
    text.split(['/', '\\']).any(|part| {
        matches!(
            part,
            ".git"
                | "node_modules"
                | "__pycache__"
                | ".claw"
                | ".venv"
                | "venv"
                | "env"
                | "site-packages"
                | "dist-packages"
                | ".mypy_cache"
                | ".pytest_cache"
                | ".ruff_cache"
                | ".tox"
                | ".next"
                | ".nuxt"
                | ".svelte-kit"
                | ".turbo"
                | ".parcel-cache"
                | ".gradle"
                | "target"
                | "dist"
                | "build"
                | "out"
                | "coverage"
        )
    })
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

/// Chemin montré au modèle : sans le préfixe `\\?\` que `canonicalize` ajoute
/// sous Windows (`\\?\D:\projet` devient `D:\projet`), qui déroute les modèles.
fn display_path(path: &Path) -> String {
    let text = path.to_string_lossy();
    match text.strip_prefix(r"\\?\") {
        Some(rest) if !rest.starts_with("UNC") => rest.to_string(),
        _ => text.into_owned(),
    }
}

/// Chemins écrits « à la Unix » par un modèle sous Windows : `/c/Users/x`,
/// `/mnt/c/x`, `/cygdrive/c/x` (lecteur) et `/tmp/x` (dossier temporaire).
fn translate_foreign_path(path: &str) -> Option<PathBuf> {
    if !cfg!(windows) || !path.starts_with('/') {
        return None;
    }
    let rest = path.trim_start_matches('/');
    if let Some(tail) = rest
        .strip_prefix("tmp/")
        .or_else(|| (rest == "tmp").then_some(""))
    {
        return Some(std::env::temp_dir().join(tail));
    }
    let rest = rest
        .strip_prefix("mnt/")
        .or_else(|| rest.strip_prefix("cygdrive/"))
        .unwrap_or(rest);
    let (drive, tail) = rest.split_once('/').unwrap_or((rest, ""));
    (drive.len() == 1 && drive.chars().all(|letter| letter.is_ascii_alphabetic())).then(|| {
        PathBuf::from(format!(
            "{}:\\{}",
            drive.to_ascii_uppercase(),
            tail.replace('/', "\\")
        ))
    })
}

/// Chemin demandé, rapporté au dossier de travail s'il est relatif.
fn candidate_path(path: &str, root: &Path) -> PathBuf {
    if let Some(translated) = translate_foreign_path(path) {
        return translated;
    }
    if Path::new(path).is_absolute() {
        PathBuf::from(path)
    } else {
        root.join(path)
    }
}

/// Un chemin commence par une racine inventée (`/workspace/src/a.py`) alors
/// que le fichier existe dans le projet (`src/a.py`) : la plus longue fin du
/// chemin qui existe sous `root`.
fn recover_existing(path: &str, root: &Path) -> Option<PathBuf> {
    let parts: Vec<&str> = path
        .split(['/', '\\'])
        .filter(|part| !part.is_empty() && *part != "." && *part != ".." && !part.ends_with(':'))
        .collect();
    (1..parts.len())
        .map(|start| parts[start..].iter().collect::<PathBuf>())
        .map(|suffix| root.join(suffix))
        .find(|found| found.exists())
}

/// Fichiers du projet dont le nom ressemble à celui demandé (5 au plus).
fn similar_files(path: &str, root: &Path) -> Vec<String> {
    let wanted = path
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(path)
        .to_ascii_lowercase();
    let stem = wanted.split('.').next().unwrap_or(&wanted).to_string();
    if stem.is_empty() {
        return Vec::new();
    }
    let mut exact = Vec::new();
    let mut close = Vec::new();
    let mut pending = vec![(root.to_path_buf(), 0usize)];
    let mut visited = 0usize;
    while let Some((dir, depth)) = pending.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            visited += 1;
            if visited > 5_000 {
                break;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            let entry_path = entry.path();
            if entry_path.is_dir() {
                if depth < 6 && !generated_dir(&entry_path, &name, root) {
                    pending.push((entry_path, depth + 1));
                }
                continue;
            }
            let shown = entry_path
                .strip_prefix(root)
                .unwrap_or(&entry_path)
                .to_string_lossy()
                .replace('\\', "/");
            let lower = name.to_ascii_lowercase();
            if lower == wanted {
                exact.push(shown);
            } else if lower.contains(&stem) {
                close.push(shown);
            }
        }
    }
    exact.sort();
    close.sort();
    exact.into_iter().chain(close).take(5).collect()
}

/// Erreur lisible par un modèle : où l'on est, comment écrire le chemin, et
/// les fichiers qui portent un nom proche.
fn missing_path_message(path: &str, root: &Path) -> String {
    let mut message = format!(
        "File not found: {path}\nThe working directory is {}. Use a path relative to it (for example `src/main.rs`), not an invented root.",
        display_path(root)
    );
    let similar = similar_files(path, root);
    if !similar.is_empty() {
        let _ = write!(
            message,
            "\nFiles with a similar name: {}",
            similar.join(", ")
        );
    }
    message
}

fn normalize_path(path: &str) -> io::Result<PathBuf> {
    let root = std::env::current_dir()?;
    let candidate = candidate_path(path, &root);
    match candidate.canonicalize() {
        Ok(found) => Ok(found),
        Err(error)
            if error.kind() == io::ErrorKind::NotFound
                && crate::harness::enabled(crate::harness::Aid::PathHelp) =>
        {
            recover_existing(path, &root)
                .and_then(|found| found.canonicalize().ok())
                .ok_or_else(|| {
                    io::Error::new(io::ErrorKind::NotFound, missing_path_message(path, &root))
                })
        }
        Err(error) => Err(error),
    }
}

fn normalize_path_allow_missing(path: &str) -> io::Result<PathBuf> {
    let root = std::env::current_dir()?;
    let mut candidate = candidate_path(path, &root);
    // Racine inventée (`/workspace/nouveau.py`) : le fichier est créé dans le
    // projet, pas à la racine du disque.
    let invented_root = path.starts_with('/')
        && translate_foreign_path(path).is_none()
        && !candidate.parent().is_some_and(Path::exists);
    if invented_root {
        let mut parts = path.split('/').filter(|part| !part.is_empty());
        let first = parts.next().unwrap_or_default();
        let rest: PathBuf = parts.collect();
        if !Path::new("/").join(first).exists() && rest.components().next().is_some() {
            candidate = root.join(rest);
        }
    }

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

    #[test]
    fn shown_paths_have_no_windows_verbatim_prefix() {
        use std::path::Path;
        assert_eq!(
            super::display_path(Path::new(r"\\?\D:\projet\a.rs")),
            r"D:\projet\a.rs"
        );
        assert_eq!(
            super::display_path(Path::new(r"\\?\UNC\serveur\partage")),
            r"\\?\UNC\serveur\partage"
        );
        assert_eq!(super::display_path(Path::new("/home/a.rs")), "/home/a.rs");
    }

    #[test]
    fn missing_paths_are_recovered_or_explained() {
        let root = temp_path("chemins");
        std::fs::create_dir_all(root.join("src")).expect("dossier");
        std::fs::write(root.join("src").join("panier.py"), "x = 1\n").expect("fichier");
        std::fs::write(root.join("test_panier.py"), "").expect("fichier");

        // Racine inventée : la fin du chemin existe dans le projet.
        let found = super::recover_existing("/workspace/app/src/panier.py", &root)
            .expect("fichier retrouvé");
        assert!(found.ends_with("src/panier.py") || found.ends_with("src\\panier.py"));
        assert_eq!(super::recover_existing("/workspace/absent.py", &root), None);

        // Sinon l'erreur dit où l'on est et propose les fichiers proches.
        let message = super::missing_path_message("/workspace/panier.py", &root);
        assert!(message.starts_with("File not found: /workspace/panier.py"));
        assert!(message.contains("The working directory is"));
        assert!(message.contains("src/panier.py"));
        assert!(message.contains("test_panier.py"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn unix_style_paths_are_translated_on_windows() {
        if cfg!(windows) {
            assert_eq!(
                super::translate_foreign_path("/c/Users/moi/a.txt"),
                Some(std::path::PathBuf::from(r"C:\Users\moi\a.txt"))
            );
            assert_eq!(
                super::translate_foreign_path("/mnt/d/projet"),
                Some(std::path::PathBuf::from(r"D:\projet"))
            );
            assert_eq!(
                super::translate_foreign_path("/tmp/essai/a.txt"),
                Some(std::env::temp_dir().join("essai/a.txt"))
            );
            assert_eq!(super::translate_foreign_path("/workspace/a.txt"), None);
        } else {
            assert_eq!(super::translate_foreign_path("/c/Users/moi/a.txt"), None);
        }
    }

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
        assert!(read_output.file.note.is_some(), "une ligne reste après");
    }

    #[test]
    fn long_odd_or_binary_files_are_read_sensibly() {
        // Long fichier : 2 000 lignes, puis la façon de lire la suite.
        let long = temp_path("long.txt");
        let content = (1..=2_500)
            .map(|n| format!("ligne {n}\n"))
            .collect::<Vec<_>>()
            .concat();
        std::fs::write(&long, content).expect("long");
        let read = read_file(long.to_string_lossy().as_ref(), None, None).expect("lecture");
        assert_eq!(read.file.num_lines, 2_000);
        assert_eq!(read.file.total_lines, 2_500);
        assert!(read
            .file
            .note
            .as_deref()
            .is_some_and(|note| note.contains("offset 2000")));
        // Ligne minifiée : raccourcie.
        let minified = temp_path("min.js");
        std::fs::write(&minified, "x".repeat(5_000)).expect("min");
        let read = read_file(minified.to_string_lossy().as_ref(), None, None).expect("min");
        assert!(read
            .file
            .content
            .ends_with("(line shortened: 5000 characters)"));
        assert!(read.file.note.is_none());
        // Windows-1252 : « café », et les octets 0x80 à 0x9F (euro, apostrophe
        // typographique, œ) qui ne sont pas du Latin-1.
        let latin = temp_path("latin.txt");
        std::fs::write(
            &latin,
            [
                b'c', b'a', b'f', 0xE9, b' ', b'1', b'2', 0x80, b' ', b'l', 0x92, 0x9C, b'u',
            ],
        )
        .expect("latin");
        let read = read_file(latin.to_string_lossy().as_ref(), None, None).expect("latin");
        assert_eq!(read.file.content, "café 12\u{20AC} l\u{2019}\u{153}u");
        assert!(read
            .file
            .note
            .as_deref()
            .is_some_and(|note| note.contains("Windows-1252")));
        // UTF-8 avec un seul octet abîmé : les accents justes restent justes.
        let damaged = temp_path("damaged.txt");
        let mut bytes = "élève été ".as_bytes().to_vec();
        bytes.push(0x92);
        std::fs::write(&damaged, &bytes).expect("abîmé");
        let read = read_file(damaged.to_string_lossy().as_ref(), None, None).expect("abîmé");
        assert_eq!(read.file.content, "élève été \u{FFFD}");
        // UTF-16 (écrit par PowerShell 5) : lu, pas pris pour un binaire.
        let utf16 = temp_path("utf16.txt");
        let mut bytes = vec![0xFF, 0xFE];
        bytes.extend("héllo\r\n".encode_utf16().flat_map(u16::to_le_bytes));
        std::fs::write(&utf16, &bytes).expect("utf16");
        let read = read_file(utf16.to_string_lossy().as_ref(), None, None).expect("utf16");
        assert_eq!(read.file.content, "héllo");
        // Binaire : un message clair, pas une erreur d'encodage.
        let binary = temp_path("image.png");
        std::fs::write(&binary, [0x89, b'P', b'N', b'G', 0, 0, 0, 13]).expect("png");
        let error = read_file(binary.to_string_lossy().as_ref(), None, None).expect_err("binaire");
        assert!(
            error.to_string().contains("is a binary file (8 bytes)"),
            "{error}"
        );
    }

    #[test]
    fn edits_and_rewrites_keep_the_encoding_of_the_file() {
        // CSV d'Excel en Windows-1252 : la modification le laisse en
        // Windows-1252, octet pour octet hors de la partie changée.
        let csv = temp_path("prix.csv");
        let original = [
            b'p', b'r', b'i', b'x', b';', b'1', b'2', b' ', 0x80, b'\r', b'\n', b'c', b'a', b'f',
            0xE9, b';', b'2', b'\r', b'\n',
        ];
        std::fs::write(&csv, original).expect("csv");
        let path = csv.to_string_lossy().into_owned();
        edit_file(&path, "café;2", "thé;3", false).expect("modification");
        let bytes = std::fs::read(&csv).expect("relecture");
        assert_eq!(
            bytes,
            [
                b'p', b'r', b'i', b'x', b';', b'1', b'2', b' ', 0x80, b'\r', b'\n', b't', b'h',
                0xE9, b';', b'3', b'\r', b'\n'
            ]
        );
        // Un caractère que Windows-1252 n'a pas : refusé, le fichier intact.
        let error = edit_file(&path, "thé;3", "thé\u{2192}3", false).expect_err("refus");
        assert!(error.to_string().contains("Windows-1252"), "{error}");
        assert_eq!(std::fs::read(&csv).expect("intact"), bytes);
        // write_file garde aussi le codage, et le dit quand il ne peut pas.
        let output = write_file(&path, "prix;5 \u{20AC}\r\n").expect("réécriture");
        assert_eq!(output.kind, "update");
        assert_eq!(
            output.original_file.as_deref(),
            Some("prix;12 \u{20AC}\r\nthé;3\r\n")
        );
        assert!(output
            .encoding_note
            .as_deref()
            .is_some_and(|note| note.contains("Windows-1252")));
        assert_eq!(std::fs::read(&csv).expect("cp1252"), b"prix;5 \x80\r\n");
        let output = write_file(&path, "prix \u{2192} 5\n").expect("conversion");
        assert!(output
            .encoding_note
            .as_deref()
            .is_some_and(|note| note.contains("now UTF-8")));
        assert_eq!(
            std::fs::read_to_string(&csv).expect("utf8"),
            "prix \u{2192} 5\n"
        );
        // UTF-16 : réécrit en UTF-16, avec sa marque d'ordre.
        let utf16 = temp_path("notes16.txt");
        let encode = |text: &str| {
            let mut bytes = vec![0xFF, 0xFE];
            bytes.extend(text.encode_utf16().flat_map(u16::to_le_bytes));
            bytes
        };
        std::fs::write(&utf16, encode("un\r\ndeux\r\n")).expect("utf16");
        edit_file(&utf16.to_string_lossy(), "deux", "trois", false).expect("modification");
        assert_eq!(
            std::fs::read(&utf16).expect("utf16"),
            encode("un\r\ntrois\r\n")
        );
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

    #[test]
    fn searches_skip_dependencies_unless_asked_for() {
        let dir = temp_path("search-skip");
        let source = dir.join("src");
        let dependency = dir.join("node_modules").join("lib");
        std::fs::create_dir_all(&source).expect("src");
        std::fs::create_dir_all(&dependency).expect("node_modules");
        std::fs::write(source.join("app.js"), "const marker = 1;\n").expect("source");
        std::fs::write(dependency.join("index.js"), "const marker = 2;\n").expect("dep");
        let base = dir.to_string_lossy().into_owned();
        let grep = |path: &str| {
            grep_search(&GrepSearchInput {
                pattern: String::from("marker"),
                path: Some(path.to_string()),
                glob: None,
                output_mode: None,
                before: None,
                after: None,
                context_short: None,
                context: None,
                line_numbers: None,
                case_insensitive: None,
                file_type: None,
                head_limit: None,
                offset: None,
                multiline: None,
            })
            .expect("grep")
        };
        let found = grep(&base);
        assert_eq!(found.num_files, 1, "{:?}", found.filenames);
        assert!(found.filenames[0].ends_with("app.js"));
        // Le modèle vise le dossier de dépendances : il y cherche.
        let inside = grep(&dependency.to_string_lossy());
        assert_eq!(inside.num_files, 1);

        let globbed = glob_search("**/*.js", Some(&base)).expect("glob");
        assert_eq!(globbed.num_files, 1, "{:?}", globbed.filenames);
        let explicit = glob_search("node_modules/**/*.js", Some(&base)).expect("glob explicite");
        assert_eq!(explicit.num_files, 1);
        // Rien trouvé : la note dit ce qui n'a pas été cherché.
        let absent = grep_search(&GrepSearchInput {
            pattern: String::from("const marker = 2"),
            path: Some(base.clone()),
            glob: None,
            output_mode: None,
            before: None,
            after: None,
            context_short: None,
            context: None,
            line_numbers: None,
            case_insensitive: None,
            file_type: None,
            head_limit: None,
            offset: None,
            multiline: None,
        })
        .expect("grep");
        assert_eq!(absent.num_files, 0);
        assert!(
            absent
                .note
                .as_deref()
                .is_some_and(|note| note.contains("node_modules")),
            "{:?}",
            absent.note
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn build_or_target_folders_are_skipped_only_when_they_are_build_output() {
        let dir = temp_path("search-build");
        // Code source dans un dossier nommé `build` (Go, pypa/build, Vue CLI 2).
        std::fs::create_dir_all(dir.join("internal").join("build")).expect("build source");
        std::fs::write(
            dir.join("internal").join("build").join("builder.go"),
            "package build // marker\n",
        )
        .expect("go");
        // Sortie de build ignorée par git, et dossier `target` de Cargo.
        std::fs::create_dir_all(dir.join("dist")).expect("dist");
        std::fs::write(dir.join("dist").join("bundle.js"), "// marker\n").expect("bundle");
        std::fs::write(dir.join(".gitignore"), "/dist\n# commentaire\n").expect("gitignore");
        std::fs::create_dir_all(dir.join("target").join("debug")).expect("target");
        std::fs::write(dir.join("target").join("CACHEDIR.TAG"), "Signature: x\n").expect("tag");
        std::fs::write(
            dir.join("target").join("debug").join("out.rs"),
            "// marker\n",
        )
        .expect("out");
        // Un script nommé `build`, sans extension, à la racine.
        std::fs::write(dir.join("build"), "#!/bin/sh\n# marker\n").expect("script");
        let base = dir.to_string_lossy().into_owned();
        let found = grep_search(&GrepSearchInput {
            pattern: String::from("marker"),
            path: Some(base.clone()),
            glob: None,
            output_mode: None,
            before: None,
            after: None,
            context_short: None,
            context: None,
            line_numbers: None,
            case_insensitive: None,
            file_type: None,
            head_limit: None,
            offset: None,
            multiline: None,
        })
        .expect("grep");
        let mut names: Vec<String> = found
            .filenames
            .iter()
            .map(|name| name.replace('\\', "/"))
            .collect();
        names.sort();
        assert_eq!(names.len(), 2, "{names:?}");
        assert!(names[0].ends_with("/build"), "{names:?}");
        assert!(names[1].ends_with("internal/build/builder.go"), "{names:?}");
        let globbed = glob_search("**/*", Some(&base)).expect("glob");
        let globbed: Vec<String> = globbed
            .filenames
            .iter()
            .map(|name| name.replace('\\', "/"))
            .collect();
        assert!(
            globbed.iter().any(|name| name.ends_with("/build")),
            "{globbed:?}"
        );
        assert!(
            globbed
                .iter()
                .any(|name| name.ends_with("internal/build/builder.go")),
            "{globbed:?}"
        );
        assert!(
            !globbed
                .iter()
                .any(|name| name.contains("/dist/") || name.contains("/target/")),
            "{globbed:?}"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_big_file_named_in_path_is_searched() {
        // Un journal de plus de 4 Mo : lu quand le modèle le nomme.
        let log = temp_path("big.log");
        let mut content = "ligne ordinaire\n".repeat(300_000);
        content.push_str("ERROR disque plein\n");
        std::fs::write(&log, content).expect("journal");
        let found = grep_search(&GrepSearchInput {
            pattern: String::from("ERROR"),
            path: Some(log.to_string_lossy().into_owned()),
            glob: None,
            output_mode: Some(String::from("content")),
            before: None,
            after: None,
            context_short: None,
            context: None,
            line_numbers: None,
            case_insensitive: None,
            file_type: None,
            head_limit: None,
            offset: None,
            multiline: None,
        })
        .expect("grep");
        assert_eq!(found.num_files, 1);
        assert!(found
            .content
            .as_deref()
            .is_some_and(|content| content.contains("ERROR disque plein")));
        let _ = std::fs::remove_file(log);
    }
}
