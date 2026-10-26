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
    /// Le fichier d'avant, pour Claw (gardes, vue du fichier) : le diff ne
    /// porte plus que les lignes changées. Retiré de ce que le modèle relit.
    #[serde(
        rename = "originalFile",
        skip_serializing_if = "Option::is_none",
        default
    )]
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
    /// Ce que le modèle doit savoir de ce qu'il a écrit (ligne d'élision).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EditFileOutput {
    #[serde(rename = "filePath")]
    pub file_path: String,
    #[serde(rename = "oldString")]
    pub old_string: String,
    #[serde(rename = "newString")]
    pub new_string: String,
    /// Le fichier d'avant, pour Claw (gardes, vue du fichier) : le diff ne
    /// porte plus que les lignes changées. Retiré de ce que le modèle relit.
    #[serde(rename = "originalFile", default)]
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
pub(crate) enum TextEncoding {
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
    pub(crate) const fn name(self) -> &'static str {
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
pub(crate) fn encode_text(text: &str, encoding: TextEncoding) -> Result<Vec<u8>, char> {
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

pub(crate) fn describe_char(ch: char) -> String {
    format!("'{ch}' (U+{:04X})", u32::from(ch))
}

/// Texte d'un fichier quel que soit son codage ; `None` s'il est binaire ou
/// illisible.
pub(crate) fn read_any_text(path: &Path) -> Option<String> {
    read_text(path).ok().map(|(text, _)| text)
}

/// Texte d'un fichier et son codage ; un fichier binaire est une erreur claire.
pub(crate) fn read_text(path: &Path) -> io::Result<(String, TextEncoding)> {
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
    if absolute_path.is_dir() {
        return Ok(directory_listing(&absolute_path));
    }
    let (content, encoding) = read_text(&absolute_path)?;
    let lines: Vec<&str> = content.lines().collect();
    let mut notes = Vec::new();
    // « 0 » veut souvent dire « sans limite » : la limite par défaut, dite.
    let limit = match limit {
        Some(0) => {
            notes.push(format!(
                "\"limit\": 0 was read as the default ({DEFAULT_READ_LINES} lines)."
            ));
            DEFAULT_READ_LINES
        }
        other => other.unwrap_or(DEFAULT_READ_LINES),
    };
    let start_index = offset.unwrap_or(0).min(lines.len());
    let end_index = start_index.saturating_add(limit).min(lines.len());
    let selected = lines[start_index..end_index]
        .iter()
        .map(|line| shorten_line(line))
        .collect::<Vec<_>>()
        .join("\n");
    if content.is_empty() {
        notes.push("This file exists and is empty (0 bytes).".to_string());
    }
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
    if absolute_path.is_dir() {
        return Err(not_a_file(path, "write_file"));
    }
    // Fichier existant : son texte (pour le diff) et son codage, qui est gardé
    // (un CSV d'Excel en Windows-1252 le reste).
    let original = read_text(&absolute_path).ok();
    let note = placeholder_check(
        &absolute_path,
        content,
        original.as_ref().map(|(text, _)| text.as_str()),
    )?;
    if let Some(parent) = absolute_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let encoding = original
        .as_ref()
        .map_or(TextEncoding::Utf8, |(_, encoding)| *encoding);
    // La marque UTF-8 (BOM) et les fins de ligne CRLF du fichier restent : un
    // script PowerShell 5.1 sans BOM est lu en Windows-1252 (accents abîmés),
    // un `.bat` en LF casse `goto`.
    let (kept_content, kept) = keep_file_conventions(
        &absolute_path,
        content,
        original.as_ref().map(|(text, _)| text.as_str()),
    );
    let content = kept_content.as_str();
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
    let encoding_note = match (encoding_note, kept) {
        (Some(note), Some(kept)) => Some(format!("{note} {kept}")),
        (note, kept) => note.or(kept),
    };

    let structured_patch = make_patch(original_file.as_deref().unwrap_or(""), content);
    let note = match (
        note,
        original_file
            .as_deref()
            .and_then(|original| rewrite_note(&absolute_path, original, &structured_patch)),
    ) {
        (Some(note), Some(rewrite)) => Some(format!("{note} {rewrite}")),
        (note, rewrite) => note.or(rewrite),
    };

    Ok(WriteFileOutput {
        kind: if original_file.is_some() {
            String::from("update")
        } else {
            String::from("create")
        },
        file_path: display_path(&absolute_path),
        content: content.to_owned(),
        structured_patch,
        original_file,
        git_diff: None,
        encoding_note,
        note,
    })
}

/// Fichier existant réécrit en entier pour en changer une petite partie. Vu
/// le 26 octobre 2026 (session #32) : 13 `write_file` sur index.html,
/// styles.css et app.js pour des retouches de 4 à 40 lignes, 28 des 40
/// minutes du passage à recopier des fichiers de 250 à 900 lignes.
fn rewrite_note(file: &Path, original: &str, hunks: &[StructuredPatchHunk]) -> Option<String> {
    let total = original.lines().count();
    let changed = hunks
        .iter()
        .flat_map(|hunk| &hunk.lines)
        .filter(|line| line.starts_with('+') || line.starts_with('-'))
        .count();
    if total < REWRITE_NOTE_MIN_LINES || changed == 0 || changed * 3 > total {
        return None;
    }
    let name = file.file_name().map_or_else(
        || file.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    );
    Some(format!(
        "[Claw] You sent all {total} lines of {name} again to change {changed}: writing a whole file takes minutes. For a change to an existing file, use edit_file (or MultiEdit for several places) with only the lines that change; keep write_file for a new file or a real rewrite."
    ))
}

/// Taille à partir de laquelle recopier un fichier pour une retouche coûte.
const REWRITE_NOTE_MIN_LINES: usize = 80;

/// Ce que le type du fichier impose, quel que soit le fichier d'avant : un
/// `.bat` ou un `.cmd` en CRLF et sans BOM (en LF, `cmd.exe` perd ses
/// étiquettes et `goto` échoue ; un BOM casse la première commande), un
/// script shell en LF (bash échoue sur CRLF : `$'\r': command not found`),
/// un script `PowerShell` accentué avec BOM (Windows `PowerShell` 5.1 lit un
/// fichier sans BOM en Windows-1252). `None` : rien à imposer.
pub(crate) fn type_conventions(path: &Path, text: &str) -> Option<(String, &'static str)> {
    let extension = path.extension()?.to_string_lossy().to_ascii_lowercase();
    match extension.as_str() {
        "bat" | "cmd" => {
            let plain = text.trim_start_matches('\u{feff}');
            let crlf = plain.replace("\r\n", "\n").replace('\n', "\r\n");
            (crlf != text).then_some((
                crlf,
                "Saved with CRLF line endings and no byte order mark, as cmd.exe needs (with LF its labels and goto break; a BOM breaks the first command).",
            ))
        }
        "sh" | "bash" | "zsh" => text.contains("\r\n").then(|| {
            (
                text.replace("\r\n", "\n"),
                "Saved with LF line endings: bash fails on CRLF ($'\\r': command not found).",
            )
        }),
        "ps1" | "psm1" | "psd1" => (!text.starts_with('\u{feff}') && !text.is_ascii()).then(|| {
            (
                format!("\u{feff}{text}"),
                "Saved in UTF-8 with a byte order mark: Windows PowerShell 5.1 reads a file without it as Windows-1252 and garbles accented text.",
            )
        }),
        _ => None,
    }
}

/// Le contenu à écrire avec la marque UTF-8 (BOM) et les fins de ligne CRLF
/// du fichier qu'il remplace, puis ce que son type impose, et ce qui a été
/// gardé ou changé.
fn keep_file_conventions(
    path: &Path,
    content: &str,
    original: Option<&str>,
) -> (String, Option<String>) {
    let (text, note) = keep_original_conventions(content, original);
    match type_conventions(path, &text) {
        Some((fixed, why)) => {
            let note = match note {
                Some(note)
                    if !matches!(
                        path.extension()
                            .map(|ext| ext.to_string_lossy().to_ascii_lowercase())
                            .as_deref(),
                        Some("sh" | "bash" | "zsh" | "bat" | "cmd")
                    ) =>
                {
                    format!("{note} {why}")
                }
                _ => why.to_string(),
            };
            (fixed, Some(note))
        }
        None => (text, note),
    }
}

/// Le contenu avec les conventions du fichier qu'il remplace.
fn keep_original_conventions(content: &str, original: Option<&str>) -> (String, Option<String>) {
    let Some(original) = original else {
        return (content.to_string(), None);
    };
    let mut kept = Vec::new();
    let mut text = content.to_string();
    let crlf = original.matches("\r\n").count();
    if crlf > 0 && crlf * 2 >= original.matches('\n').count() && !text.contains("\r\n") {
        text = text.replace('\n', "\r\n");
        kept.push("CRLF line endings");
    }
    if original.starts_with('\u{feff}') && !text.starts_with('\u{feff}') {
        text.insert(0, '\u{feff}');
        kept.push("UTF-8 byte order mark");
    }
    let note = (!kept.is_empty()).then(|| format!("The file's {} kept.", kept.join(" and ")));
    (text, note)
}

/// Ligne d'élision qu'un modèle met à la place du code qu'il ne recopie pas
/// (`# ... rest of the code remains unchanged`, `// ... existing code ...`) :
/// un commentaire qui commence par des points de suspension et parle du reste,
/// ou une tournure sans équivoque.
fn is_elision_line(line: &str) -> bool {
    const CLEAR: &[&str] = &[
        "rest of the code",
        "rest of the file",
        "rest of code",
        "existing code",
        "remains unchanged",
        "remain unchanged",
        "code unchanged",
        "omitted for brevity",
        "code omitted",
        "le reste du code",
        "le reste du fichier",
        "code existant",
        "reste inchang",
        "reste identique",
    ];
    const ABOUT_THE_REST: &[&str] = &[
        "rest",
        "remaining",
        "existing",
        "unchanged",
        "same as",
        "omitted",
        "previous",
        "other",
        "as before",
        "reste",
        "inchang",
        "existant",
        "identique",
        "autres",
    ];
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.chars().count() > 120 {
        return false;
    }
    let lower = trimmed.to_lowercase();
    let marker = ["{/*", "<!--", "/*", "//", "#", "*", "--", ";", "'"]
        .iter()
        .find_map(|start| lower.strip_prefix(start))
        .map(str::trim_start);
    // Sans marque de commentaire, seule une ligne qui commence par des points
    // de suspension compte (`...` seul est du Python valide).
    let (body, commented) = match marker {
        Some(body) => (body, true),
        None if lower.starts_with("...") || lower.starts_with('\u{2026}') => {
            (lower.as_str(), false)
        }
        None => return false,
    };
    let elided = body.starts_with("...") || body.starts_with('\u{2026}');
    CLEAR.iter().any(|phrase| body.contains(phrase))
        || (elided && ABOUT_THE_REST.iter().any(|word| body.contains(word)))
        || (commented
            && elided
            && body
                .trim_matches(|ch: char| {
                    ch == '.' || ch == '\u{2026}' || ch == '*' || ch == '/' || ch.is_whitespace()
                })
                .is_empty())
}

/// Première ligne d'élision du texte : son numéro (à partir de 1) et son texte.
pub(crate) fn elision_in(text: &str) -> Option<(usize, String)> {
    text.lines()
        .enumerate()
        .find(|(_, line)| is_elision_line(line))
        .map(|(index, line)| (index + 1, line.trim().to_string()))
}

/// Contenu abrégé : écrit tel quel, il remplacerait du code par un
/// commentaire, sans erreur ni avertissement (le contrôle de syntaxe ne voit
/// rien). Refusé quand le fichier existant fond de plus de moitié ; sinon
/// écrit, avec une note. `None` : rien à signaler.
fn placeholder_check(
    path: &Path,
    content: &str,
    original: Option<&str>,
) -> io::Result<Option<String>> {
    if !crate::harness::enabled(crate::harness::Aid::PlaceholderGuard) {
        return Ok(None);
    }
    let Some((line, text)) = elision_in(content) else {
        return Ok(None);
    };
    // La ligne était déjà dans le fichier : le modèle ne l'a pas inventée.
    if original.is_some_and(|original| original.lines().any(|kept| kept.trim() == text)) {
        return Ok(None);
    }
    let name = path.file_name().map_or_else(
        || display_path(path),
        |name| name.to_string_lossy().into_owned(),
    );
    let before = original.map_or(0, |original| original.lines().count());
    let after = content.lines().count();
    if before >= 10 && after * 2 < before {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "[Claw] write_file was not executed: line {line} of your content is a placeholder (\"{text}\"), not code, and {name} would shrink from {before} to {after} lines. write_file replaces the whole file with exactly what you send. To change part of {name}, use edit_file with the lines that change; to rewrite it, send every line."
            ),
        ));
    }
    Ok(Some(format!(
        "Line {line} of what was written is a placeholder (\"{text}\"), not code: write_file wrote it as is. If it stands for code you meant to keep, that code is not in the file: write it out."
    )))
}

/// Erreur d'un outil d'écriture qui a reçu un dossier.
pub(crate) fn not_a_file(path: &str, tool: &str) -> io::Error {
    let shown = path.trim_end_matches(['/', '\\']);
    io::Error::new(
        io::ErrorKind::InvalidInput,
        format!(
            "[Claw] \"{shown}\" is a directory: {tool} needs the path of a file inside it, for example {shown}/<file name>."
        ),
    )
}

/// Ce qu'un dossier contient, rendu par `read_file` à qui le lit comme un
/// fichier : l'erreur du système (« Accès refusé » sous Windows) ferait
/// croire à un problème de droits.
fn directory_listing(dir: &Path) -> ReadFileOutput {
    const SHOWN: usize = 200;
    let mut entries: Vec<(bool, String)> = fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| {
            let is_dir = entry.path().is_dir();
            let name = entry.file_name().to_string_lossy().into_owned();
            (!is_dir, if is_dir { format!("{name}/") } else { name })
        })
        .collect();
    entries.sort();
    let total = entries.len();
    let shown: Vec<String> = entries
        .into_iter()
        .take(SHOWN)
        .map(|(_, name)| name)
        .collect();
    let more = if total > SHOWN {
        format!(", the first {SHOWN} shown")
    } else {
        String::new()
    };
    ReadFileOutput {
        kind: String::from("text"),
        file: TextFilePayload {
            file_path: display_path(dir),
            num_lines: shown.len(),
            content: shown.join("\n"),
            start_line: 1,
            total_lines: total,
            note: Some(format!(
                "{} is a directory, not a file, so here is what it contains ({total} entries{more}; folders end with /). Call read_file on one of these files, or glob_search to look deeper.",
                display_path(dir)
            )),
        },
    }
}

pub fn edit_file(
    path: &str,
    old_string: &str,
    new_string: &str,
    replace_all: bool,
) -> io::Result<EditFileOutput> {
    // `old_string` vide sur un fichier absent ou vide : créé avec
    // `new_string`, comme write_file le ferait.
    if old_string.is_empty() {
        return create_by_edit(path, new_string, replace_all);
    }
    let absolute_path = normalize_path(path).map_err(|error| {
        if error.kind() == io::ErrorKind::NotFound {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!("{error}\nTo create this file, call write_file with its full content."),
            )
        } else {
            error
        }
    })?;
    if absolute_path.is_dir() {
        return Err(not_a_file(path, "edit_file"));
    }
    let (original_file, encoding) = read_text(&absolute_path)?;
    // Une ligne d'élision qui prend la place de lignes de code : le code
    // remplacé serait perdu.
    if crate::harness::enabled(crate::harness::Aid::PlaceholderGuard) {
        if let Some((line, text)) = elision_in(new_string)
            .filter(|(_, text)| !old_string.lines().any(|kept| kept.trim() == text))
        {
            let (removed, added) = (old_string.lines().count(), new_string.lines().count());
            if added < removed {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!(
                        "[Claw] edit_file was not executed: line {line} of new_string is a placeholder (\"{text}\"), not code, so the {removed} lines of old_string would be replaced by it and lost. new_string must hold the exact new text. Put only the lines that change in old_string and new_string."
                    ),
                ));
            }
        }
    }
    if old_string == new_string {
        let message = match original_file.find(old_string) {
            Some(at) => format!(
                "[Claw] edit_file changed nothing: old_string and new_string are identical, and this text is already in the file at line {}. If that is the result you wanted, go on; otherwise put the changed text in new_string.",
                line_index(&original_file, at) + 1
            ),
            None => "[Claw] edit_file changed nothing: old_string and new_string are identical. Put the changed text in new_string.".to_string(),
        };
        return Err(io::Error::new(io::ErrorKind::InvalidInput, message));
    }
    let (updated, mut match_note) = apply_edit(&original_file, old_string, new_string, replace_all)
        .map_err(|message| io::Error::new(io::ErrorKind::NotFound, message))?;
    // Ce que le type du fichier impose (fins de ligne, BOM), même après une
    // modification : un `.bat` modifié avec des lignes en LF reste en CRLF.
    let updated = match type_conventions(&absolute_path, &updated) {
        Some((fixed, why)) => {
            match_note = Some(match match_note {
                Some(existing) => format!("{existing} ; {why}"),
                None => why.to_string(),
            });
            fixed
        }
        None => updated,
    };
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

/// `edit_file` sans `old_string` : un fichier absent ou vide est créé avec
/// `new_string` ; un fichier qui a déjà du contenu reçoit une erreur qui dit
/// quoi faire.
fn create_by_edit(path: &str, new_string: &str, replace_all: bool) -> io::Result<EditFileOutput> {
    let target = normalize_path_allow_missing(path)?;
    if target.is_dir() {
        return Err(not_a_file(path, "edit_file"));
    }
    let empty = fs::metadata(&target).map_or(true, |meta| meta.len() == 0);
    if !empty {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "[Claw] edit_file changed nothing: old_string is empty but the file already has content. To change it, put the exact lines to replace in old_string; to rewrite it whole, use write_file.",
        ));
    }
    let written = write_file(path, new_string)?;
    Ok(EditFileOutput {
        file_path: written.file_path,
        old_string: String::new(),
        new_string: new_string.to_owned(),
        original_file: String::new(),
        structured_patch: written.structured_patch,
        user_modified: false,
        replace_all,
        git_diff: None,
        match_note: Some(
            "The file did not exist or was empty: it was created with new_string, as write_file would."
                .to_string(),
        ),
    })
}

/// Numéro de ligne (à partir de 0) de l'octet `at` du texte.
fn line_index(text: &str, at: usize) -> usize {
    text[..at.min(text.len())].matches('\n').count()
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
pub(crate) fn apply_edit(
    original: &str,
    old_string: &str,
    new_string: &str,
    replace_all: bool,
) -> Result<(String, Option<String>), String> {
    match apply_edit_strictly(original, old_string, new_string, replace_all) {
        Ok(applied) => return Ok(applied),
        Err(Some(ambiguous)) => return Err(ambiguous),
        Err(None) => {}
    }
    // Ce qu'un modèle abîme en recopiant : un `\"` pour `"`, un `\n` écrit en
    // deux caractères, une espace insécable. Essayé, et dit.
    for (old, new, note) in loose_variants(old_string, new_string) {
        match apply_edit_strictly(original, &old, &new, replace_all) {
            Ok((updated, inner)) => {
                let note =
                    inner.map_or_else(|| note.to_string(), |inner| format!("{note}; {inner}"));
                return Ok((updated, Some(note)));
            }
            Err(Some(ambiguous)) => return Err(ambiguous),
            Err(None) => {}
        }
    }
    Err(not_found_message(original, old_string))
}

/// Variantes de `old_string` (et `new_string`) à essayer quand le texte n'est
/// pas dans le fichier, chacune avec la note qui la dit.
fn loose_variants(old_string: &str, new_string: &str) -> Vec<(String, String, &'static str)> {
    let mut variants = Vec::new();
    if old_string.contains("\\\"") || old_string.contains("\\'") {
        let unescape = |text: &str| text.replace("\\\"", "\"").replace("\\'", "'");
        variants.push((
            unescape(old_string),
            unescape(new_string),
            "an extra backslash before quotes was removed (\\\" read as \")",
        ));
    }
    if !old_string.contains('\n') && (old_string.contains("\\n") || old_string.contains("\\t")) {
        let real = |text: &str| text.replace("\\n", "\n").replace("\\t", "\t");
        variants.push((
            real(old_string),
            real(new_string),
            "\\n and \\t written as two characters were read as a line break and a tab",
        ));
    }
    if old_string.contains('\u{a0}') {
        variants.push((
            old_string.replace('\u{a0}', " "),
            new_string.to_string(),
            "non-breaking spaces were read as spaces",
        ));
    }
    variants
}

/// Recherche exacte, puis CRLF, espaces de fin et indentation. `Err(None)` :
/// introuvable ; `Err(Some(message))` : plusieurs occurrences.
fn apply_edit_strictly(
    original: &str,
    old_string: &str,
    new_string: &str,
    replace_all: bool,
) -> Result<(String, Option<String>), Option<String>> {
    let count = original.matches(old_string).count();
    if count > 0 {
        if count > 1 && !replace_all {
            let starts: Vec<usize> = original
                .match_indices(old_string)
                .map(|(at, _)| line_index(original, at))
                .collect();
            return Err(Some(ambiguous_message(original, &starts)));
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
                let starts: Vec<usize> = original
                    .match_indices(old_crlf.as_str())
                    .map(|(at, _)| line_index(original, at))
                    .collect();
                return Err(Some(ambiguous_message(original, &starts)));
            }
            let updated = if replace_all {
                original.replace(&old_crlf, &new_crlf)
            } else {
                original.replacen(&old_crlf, &new_crlf, 1)
            };
            return Ok((
                updated,
                Some("the file's CRLF line endings were kept".to_string()),
            ));
        }
    }

    for (level, note) in [
        (
            MatchLevel::TrailingWhitespace,
            "trailing spaces were ignored",
        ),
        (
            MatchLevel::Indentation,
            "the indentation was adjusted to the file's",
        ),
    ] {
        if let Some(result) =
            line_match(original, old_string, new_string, replace_all, level).map_err(Some)?
        {
            return Ok((result, Some(format!("approximate match: {note}"))));
        }
    }
    Err(None)
}

/// Plusieurs occurrences : leurs lignes, et la ligne qui précède chacune,
/// pour que le modèle choisisse sans relire le fichier.
fn ambiguous_message(original: &str, starts: &[usize]) -> String {
    let lines: Vec<&str> = original.lines().collect();
    let count = starts.len();
    let numbers: Vec<String> = starts.iter().map(|start| (start + 1).to_string()).collect();
    let before: Vec<String> = starts
        .iter()
        .take(6)
        .map(
            |start| match start.checked_sub(1).and_then(|index| lines.get(index)) {
                Some(line) => format!(
                    "{start} \"{}\"",
                    line.trim().chars().take(80).collect::<String>()
                ),
                None => format!("{} (start of the file)", start + 1),
            },
        )
        .collect();
    format!(
        "[Claw] edit_file changed nothing: old_string appears {count} times, at lines {}. Line before each: {}. Add that line to old_string to pick one, or set replace_all to true to change all {count}.",
        numbers.join(", "),
        before.join(", ")
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
        return Err(ambiguous_message(original, &starts));
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

/// Texte introuvable : les lignes réelles du fichier autour de la plus
/// proche de la première ligne cherchée, numérotées, pour que le modèle
/// recopie sans relire le fichier (souvent, il recopiait un texte que sa
/// propre modification précédente avait déjà changé).
fn not_found_message(original: &str, old_string: &str) -> String {
    let first = old_string
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or_default();
    let lines: Vec<&str> = original.lines().collect();
    let closest = lines
        .iter()
        .enumerate()
        .map(|(index, line)| (index, similarity(line.trim(), first)))
        .filter(|(_, score)| *score > 0.5)
        .max_by(|left, right| left.1.total_cmp(&right.1));
    let Some((index, _)) = closest else {
        return "[Claw] edit_file changed nothing: old_string is not in the file, and no line looks like its first line. Check that this is the right file, then read it with read_file.".to_string();
    };
    let start = index.saturating_sub(3);
    let end = (index + old_string.lines().count().max(1) + 3)
        .min(lines.len())
        .min(start + 40);
    let mut message = format!(
        "[Claw] edit_file changed nothing: old_string is not in the file (even ignoring whitespace and indentation). The closest place is lines {} to {end}, shown below exactly as they are now; copy old_string from them (without the line numbers):",
        start + 1
    );
    for (offset, line) in lines[start..end].iter().enumerate() {
        let _ = write!(
            message,
            "\n{:>5}| {}",
            start + offset + 1,
            line.chars().take(300).collect::<String>()
        );
    }
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
    const SHOWN: usize = 100;
    let started = Instant::now();
    let base_dir = path
        .map(normalize_path)
        .transpose()?
        .unwrap_or(std::env::current_dir()?);
    let mut notes = Vec::new();
    let pattern = relative_glob(pattern, &base_dir);
    let patterns = compile_glob(&pattern, &mut notes)?;
    // Dépendances, caches et sorties de build écartés pendant le parcours
    // (jamais ouverts) : récents après une installation, ils passeraient
    // avant les fichiers du projet, et les parcourir prenait des dizaines de
    // secondes.
    let include_skipped = names_skipped_dir(&pattern) || path.is_some_and(names_skipped_dir);
    let (files, skipped) = collect_search_files(&base_dir, include_skipped);
    let relative = |file: &Path| {
        file.strip_prefix(&base_dir)
            .unwrap_or(file)
            .to_string_lossy()
            .replace('\\', "/")
    };
    let options = glob::MatchOptions {
        case_sensitive: true,
        require_literal_separator: true,
        require_literal_leading_dot: false,
    };
    let select = |patterns: &[Pattern]| -> Vec<PathBuf> {
        files
            .iter()
            .filter(|file| {
                let shown = relative(file);
                patterns
                    .iter()
                    .any(|glob| glob.matches_with(&shown, options))
            })
            .cloned()
            .collect()
    };
    let mut matches = select(&patterns);
    // Rien au premier niveau : un dossier nommé tel quel est listé, un motif
    // sans `**` est cherché dans les sous-dossiers.
    if matches.is_empty() && !pattern.contains("**") {
        let named = pattern.trim_end_matches('/');
        let wider = if !named.is_empty() && base_dir.join(named).is_dir() {
            format!("{named}/**/*")
        } else {
            format!("**/{pattern}")
        };
        if let Ok(wider_patterns) = compile_glob(&wider, &mut Vec::new()) {
            let found = select(&wider_patterns);
            if !found.is_empty() {
                notes.push(format!(
                    "Nothing matches \"{pattern}\" at the top of the search, so \"{wider}\" was searched instead: {} file(s).",
                    found.len()
                ));
                matches = found;
            }
        }
    }

    matches.sort_by_key(|path| {
        fs::metadata(path)
            .and_then(|metadata| metadata.modified())
            .ok()
            .map(Reverse)
    });
    let total = matches.len();
    let truncated = total > SHOWN;
    let filenames = matches
        .into_iter()
        .take(SHOWN)
        .map(|path| display_path(&path))
        .collect::<Vec<_>>();
    if filenames.is_empty() {
        notes.push(format!(
            "No file matches \"{pattern}\" under {} ({} files looked at). Check the pattern (\"**/*.ts\" searches every folder, \"src/*.ts\" only src), or list what exists with \"**/*\".",
            display_path(&base_dir),
            files.len()
        ));
        notes.extend(search_note(&skipped, 0));
    } else if truncated {
        notes.push(format!(
            "Showing the {SHOWN} most recently changed of {total} matching files: narrow the pattern or give a path."
        ));
    }
    Ok(GlobSearchOutput {
        duration_ms: started.elapsed().as_millis(),
        num_files: filenames.len(),
        note: (!notes.is_empty()).then(|| notes.join(" ")),
        filenames,
        truncated,
    })
}

/// Le motif de `glob_search`, relatif au dossier de la recherche et écrit
/// avec des `/` : un motif absolu qui part de ce dossier en est retiré, un
/// `./` de tête aussi.
fn relative_glob(pattern: &str, base: &Path) -> String {
    let pattern = pattern.trim().replace('\\', "/");
    let base_shown = display_path(base).replace('\\', "/");
    let pattern = pattern
        .strip_prefix(&format!("{base_shown}/"))
        .map(str::to_string)
        .unwrap_or(pattern);
    pattern.trim_start_matches("./").to_string()
}

/// Motifs à comparer aux chemins, accolades développées (`*.{ts,tsx}`). Les
/// fautes courantes sont réparées et dites (`**.ts` devient `**/*.ts`) ; un
/// motif encore invalide reçoit une erreur qui montre la bonne forme.
fn compile_glob(pattern: &str, notes: &mut Vec<String>) -> io::Result<Vec<Pattern>> {
    let repaired = repair_double_star(pattern);
    if repaired != pattern {
        notes.push(format!("\"{pattern}\" was read as \"{repaired}\"."));
    }
    expand_braces(&repaired)
        .iter()
        .map(|glob| {
            Pattern::new(glob).map_err(|error| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!(
                        "[Claw] \"{pattern}\" is not a valid glob ({}). Examples: \"**/*.py\" (every folder), \"src/*.ts\", \"*.{{ts,tsx}}\".",
                        error.msg
                    ),
                )
            })
        })
        .collect()
}

/// `**` doit être seul entre deux `/` : `**.ts` devient `**/*.ts`,
/// `src/**test*.py` devient `src/**/*test*.py`.
fn repair_double_star(pattern: &str) -> String {
    pattern
        .split('/')
        .map(|part| {
            if part.contains("**") && part != "**" {
                format!("**/{}", part.replace("**", "*"))
            } else {
                part.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("/")
}

pub fn grep_search(input: &GrepSearchInput) -> io::Result<GrepSearchOutput> {
    let base_path = input
        .path
        .as_deref()
        .map(normalize_path)
        .transpose()?
        .unwrap_or(std::env::current_dir()?);

    let mut notes: Vec<String> = Vec::new();
    let regex = search_regex(input, &mut notes)?;
    let globs = glob_filters(input.glob.as_deref())?;
    let types = input.file_type.as_deref().map(file_type_extensions);
    let output_mode = output_mode(input.output_mode.as_deref(), &mut notes);
    let multiline = input.multiline.unwrap_or(false);
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
    let (mut searched, mut filtered_out) = (0usize, 0usize);
    for file_path in files {
        if !matches_optional_filters(&file_path, &base_path, &globs, types.as_deref()) {
            filtered_out += 1;
            continue;
        }
        searched += 1;
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
        let matched_lines = matched_lines(&regex, &file_contents, &lines, multiline);
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
    // Des filtres ont écarté tout ce qui a été trouvé : le dire, sinon le
    // modèle conclut que ce qu'il cherche n'existe pas.
    if searched == 0 && filtered_out > 0 {
        notes.push(filtered_out_note(filtered_out, &base_path, input));
    }
    notes.extend(search_note(&skipped_dirs, large_files));
    let note = (!notes.is_empty()).then(|| notes.join(" "));
    if output_mode == "content" {
        // `head_limit` et `offset` découpent les lignes rendues, pas la liste
        // des fichiers, qui reste celle de tous les fichiers trouvés.
        let (lines, limit, offset) = apply_limit(content_lines, input.head_limit, input.offset);
        let (filenames, _, _) = apply_limit(filenames, None, None);
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

    let (filenames, applied_limit, applied_offset) =
        apply_limit(filenames, input.head_limit, input.offset);
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

/// L'expression de la recherche. Un motif qui n'en est pas une (`print(`,
/// `items[`, `${name}`) est cherché comme texte, et la note le dit ; les
/// tournures que le moteur ne connaît pas (regards avant ou arrière,
/// références arrière) ont une erreur qui dit quoi faire.
fn search_regex(input: &GrepSearchInput, notes: &mut Vec<String>) -> io::Result<regex::Regex> {
    let build = |pattern: &str| {
        RegexBuilder::new(pattern)
            .case_insensitive(input.case_insensitive.unwrap_or(false))
            .dot_matches_new_line(input.multiline.unwrap_or(false))
            .multi_line(true)
            .build()
    };
    let error = match build(&input.pattern) {
        Ok(regex) => return Ok(regex),
        Err(error) => error,
    };
    let pattern = &input.pattern;
    let backreference = pattern
        .as_bytes()
        .windows(2)
        .any(|pair| pair[0] == b'\\' && pair[1].is_ascii_digit() && pair[1] != b'0');
    if ["(?=", "(?!", "(?<=", "(?<!"]
        .iter()
        .any(|form| pattern.contains(form))
        || backreference
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "[Claw] grep_search does not support look-ahead, look-behind or backreferences. Search for the simpler part of the pattern here, or run grep -P in bash.",
        ));
    }
    let message = error.to_string();
    let reason = message
        .lines()
        .rev()
        .find_map(|line| line.trim().strip_prefix("error: "))
        .unwrap_or("syntax error")
        .to_string();
    notes.push(format!(
        "\"{pattern}\" is not a valid regular expression ({reason}), so it was searched as plain text. To use a regex, escape special characters with a backslash."
    ));
    build(&regex::escape(pattern))
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error.to_string()))
}

/// Mode de sortie demandé ; les synonymes courants sont lus (`lines` vaut
/// `content`), et la note dit comment.
fn output_mode(requested: Option<&str>, notes: &mut Vec<String>) -> String {
    let Some(requested) = requested.map(str::trim).filter(|mode| !mode.is_empty()) else {
        return String::from("files_with_matches");
    };
    let mode = match requested.to_ascii_lowercase().as_str() {
        "content" | "contents" | "lines" | "line" | "matches" | "text" => "content",
        "count" | "counts" => "count",
        "files_with_matches" | "files" | "files_only" | "filenames" | "paths" | "names" => {
            "files_with_matches"
        }
        _ => {
            notes.push(format!(
                "output_mode \"{requested}\" is unknown, so the file names are listed. The modes are content, files_with_matches and count."
            ));
            return String::from("files_with_matches");
        }
    };
    if mode != requested {
        notes.push(format!(
            "output_mode \"{requested}\" was read as \"{mode}\"."
        ));
    }
    mode.to_string()
}

/// Extensions que désigne `type` : le nom d'un langage (celui de ripgrep,
/// que les modèles connaissent) ou une extension, avec ou sans point.
fn file_type_extensions(file_type: &str) -> Vec<String> {
    let name = file_type
        .trim()
        .trim_start_matches("*.")
        .trim_start_matches('.')
        .to_ascii_lowercase();
    let known: &[&str] = match name.as_str() {
        "python" | "py" => &["py", "pyi"],
        "rust" | "rs" => &["rs"],
        "typescript" | "ts" => &["ts", "tsx", "mts", "cts"],
        "javascript" | "js" => &["js", "jsx", "mjs", "cjs"],
        "markdown" | "md" => &["md", "markdown"],
        "yaml" | "yml" => &["yml", "yaml"],
        "cpp" | "c++" | "cxx" => &["cpp", "cc", "cxx", "hpp", "hh", "h"],
        "c" => &["c", "h"],
        "shell" | "sh" | "bash" => &["sh", "bash"],
        "golang" | "go" => &["go"],
        "kotlin" | "kt" => &["kt", "kts"],
        "csharp" | "c#" | "cs" => &["cs"],
        "ruby" | "rb" => &["rb"],
        "html" | "htm" => &["html", "htm"],
        "css" => &["css", "scss", "sass", "less"],
        _ => return vec![name],
    };
    known.iter().map(ToString::to_string).collect()
}

/// Motifs du filtre `glob`, accolades développées (`*.{ts,tsx}` donne `*.ts`
/// et `*.tsx`, que le paquet `glob` ne connaît pas).
fn glob_filters(glob: Option<&str>) -> io::Result<Vec<Pattern>> {
    let Some(glob) = glob.map(str::trim).filter(|glob| !glob.is_empty()) else {
        return Ok(Vec::new());
    };
    expand_braces(glob)
        .iter()
        .map(|pattern| {
            Pattern::new(pattern).map_err(|error| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!(
                        "[Claw] \"{glob}\" is not a valid glob ({error}). Examples: \"*.py\", \"src/**/*.ts\", \"*.{{ts,tsx}}\"."
                    ),
                )
            })
        })
        .collect()
}

/// `a{b,c}d` donne `abd` et `acd` ; au plus 64 motifs.
fn expand_braces(pattern: &str) -> Vec<String> {
    let mut expanded = vec![pattern.to_string()];
    while let Some(index) = expanded.iter().position(|item| {
        item.find('{')
            .is_some_and(|open| item[open..].contains('}'))
    }) {
        if expanded.len() > 64 {
            break;
        }
        let item = expanded.remove(index);
        let open = item.find('{').unwrap_or_default();
        let close = open + item[open..].find('}').unwrap_or_default();
        let (head, tail) = (&item[..open], &item[close + 1..]);
        for (offset, alternative) in item[open + 1..close].split(',').enumerate() {
            expanded.insert(index + offset, format!("{head}{alternative}{tail}"));
        }
    }
    expanded
}

/// Lignes (à partir de 0) où le motif se trouve.
fn matched_lines(
    regex: &regex::Regex,
    contents: &str,
    lines: &[&str],
    multiline: bool,
) -> Vec<usize> {
    if multiline {
        lines_covered(regex, contents, lines.len())
    } else {
        (0..lines.len())
            .filter(|&index| regex.is_match(lines[index]))
            .collect()
    }
}

/// Note d'une recherche dont les filtres ont écarté tous les fichiers trouvés.
fn filtered_out_note(filtered_out: usize, base: &Path, input: &GrepSearchInput) -> String {
    format!(
        "No file was searched: {filtered_out} file(s) were found under {} but none passed the filter {}. Give type as a language or an extension (python, rs, ts), glob relative to the search path (src/**/*.ts, *.{{ts,tsx}}), or drop the filter.",
        display_path(base),
        describe_filters(input)
    )
}

/// Les filtres de la recherche, pour la note qui dit qu'ils ont tout écarté.
fn describe_filters(input: &GrepSearchInput) -> String {
    let mut filters = Vec::new();
    if let Some(file_type) = &input.file_type {
        filters.push(format!("type \"{file_type}\""));
    }
    if let Some(glob) = &input.glob {
        filters.push(format!("glob \"{glob}\""));
    }
    filters.join(" and ")
}

/// Lignes (à partir de 0) que couvrent les correspondances d'un motif sur
/// plusieurs lignes (`multiline`), cherché dans tout le texte.
fn lines_covered(regex: &regex::Regex, text: &str, line_count: usize) -> Vec<usize> {
    let newlines: Vec<usize> = text.match_indices('\n').map(|(at, _)| at).collect();
    let line_of = |at: usize| newlines.partition_point(|newline| *newline < at);
    let mut covered = std::collections::BTreeSet::new();
    for found in regex.find_iter(text) {
        let last = found.end().saturating_sub(1).max(found.start());
        covered.extend(line_of(found.start())..=line_of(last));
    }
    covered
        .into_iter()
        .filter(|line| *line < line_count)
        .collect()
}

/// Le fichier passe les filtres `glob` (comparé au chemin sous la base de la
/// recherche, avec des `/`, et au nom du fichier) et `type`.
fn matches_optional_filters(
    path: &Path,
    base: &Path,
    globs: &[Pattern],
    types: Option<&[String]>,
) -> bool {
    if !globs.is_empty() {
        let relative = path
            .strip_prefix(base)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let absolute = path.to_string_lossy().replace('\\', "/");
        if !globs
            .iter()
            .any(|glob| glob.matches(&relative) || glob.matches(&name) || glob.matches(&absolute))
        {
            return false;
        }
    }
    if let Some(types) = types {
        let extension = path
            .extension()
            .map(|extension| extension.to_string_lossy().to_ascii_lowercase());
        if !extension.is_some_and(|extension| types.contains(&extension)) {
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

/// Lignes de contexte gardées autour d'un changement, comme `git diff`.
const PATCH_CONTEXT: usize = 3;
/// Au-delà (lignes changées d'avant × d'après), la partie changée est donnée
/// en bloc, sans chercher les lignes communes en son milieu.
const PATCH_MAX_CELLS: usize = 4_000_000;

/// La différence entre deux textes, en passages comme `git diff` : seules les
/// lignes changées, avec trois lignes de contexte (vu le 21 octobre 2026 : un
/// diff qui retirait tout l'ancien fichier et ajoutait tout le nouveau faisait
/// voir chaque petite modification comme le fichier effacé puis réécrit, dans
/// l'appli comme pour le modèle, à qui « lignes 1 à 860 » était rapporté).
pub(crate) fn make_patch(original: &str, updated: &str) -> Vec<StructuredPatchHunk> {
    let old: Vec<&str> = original.lines().collect();
    let new: Vec<&str> = updated.lines().collect();
    if old.is_empty() {
        if new.is_empty() {
            return Vec::new();
        }
        return vec![StructuredPatchHunk {
            old_start: 1,
            old_lines: 0,
            new_start: 1,
            new_lines: new.len(),
            lines: new.iter().map(|line| format!("+{line}")).collect(),
        }];
    }
    let ops = line_ops(&old, &new);
    // Positions des lignes changées, regroupées en passages quand moins de
    // deux contextes les séparent.
    let changed: Vec<usize> = ops
        .iter()
        .enumerate()
        .filter(|(_, op)| !matches!(op, LineOp::Same(..)))
        .map(|(index, _)| index)
        .collect();
    let mut groups: Vec<(usize, usize)> = Vec::new();
    for index in changed {
        match groups.last_mut() {
            Some((_, end)) if index <= *end + 2 * PATCH_CONTEXT + 1 => *end = index,
            _ => groups.push((index, index)),
        }
    }
    // Numéros de ligne (avant, après) au début de chaque opération.
    let mut starts = Vec::with_capacity(ops.len() + 1);
    let (mut old_no, mut new_no) = (0usize, 0usize);
    for op in &ops {
        starts.push((old_no, new_no));
        match op {
            LineOp::Same(..) => {
                old_no += 1;
                new_no += 1;
            }
            LineOp::Removed(_) => old_no += 1,
            LineOp::Added(_) => new_no += 1,
        }
    }
    groups
        .into_iter()
        .map(|(first, last)| {
            let from = first.saturating_sub(PATCH_CONTEXT);
            let to = (last + PATCH_CONTEXT + 1).min(ops.len());
            let mut hunk = StructuredPatchHunk {
                old_start: starts[from].0 + 1,
                old_lines: 0,
                new_start: starts[from].1 + 1,
                new_lines: 0,
                lines: Vec::with_capacity(to - from),
            };
            for op in &ops[from..to] {
                match op {
                    LineOp::Same(line) => {
                        hunk.old_lines += 1;
                        hunk.new_lines += 1;
                        hunk.lines.push(format!(" {line}"));
                    }
                    LineOp::Removed(line) => {
                        hunk.old_lines += 1;
                        hunk.lines.push(format!("-{line}"));
                    }
                    LineOp::Added(line) => {
                        hunk.new_lines += 1;
                        hunk.lines.push(format!("+{line}"));
                    }
                }
            }
            hunk
        })
        .collect()
}

enum LineOp<'a> {
    Same(&'a str),
    Removed(&'a str),
    Added(&'a str),
}

/// Les lignes des deux textes dans l'ordre, chacune gardée, retirée ou
/// ajoutée : début et fin communs écartés, plus longue suite commune au
/// milieu (en bloc quand ce milieu est trop grand).
fn line_ops<'a>(old: &[&'a str], new: &[&'a str]) -> Vec<LineOp<'a>> {
    let prefix = old.iter().zip(new).take_while(|(a, b)| a == b).count();
    let suffix = old[prefix..]
        .iter()
        .rev()
        .zip(new[prefix..].iter().rev())
        .take_while(|(a, b)| a == b)
        .count();
    let (mid_old, mid_new) = (
        &old[prefix..old.len() - suffix],
        &new[prefix..new.len() - suffix],
    );
    let mut ops: Vec<LineOp<'a>> = old[..prefix]
        .iter()
        .map(|line| LineOp::Same(line))
        .collect();
    if mid_old.len().saturating_mul(mid_new.len()) > PATCH_MAX_CELLS {
        ops.extend(mid_old.iter().map(|line| LineOp::Removed(line)));
        ops.extend(mid_new.iter().map(|line| LineOp::Added(line)));
    } else {
        let width = mid_new.len() + 1;
        let mut table = vec![0u32; (mid_old.len() + 1) * width];
        for i in (0..mid_old.len()).rev() {
            for j in (0..mid_new.len()).rev() {
                table[i * width + j] = if mid_old[i] == mid_new[j] {
                    table[(i + 1) * width + j + 1] + 1
                } else {
                    table[(i + 1) * width + j].max(table[i * width + j + 1])
                };
            }
        }
        let (mut i, mut j) = (0, 0);
        while i < mid_old.len() || j < mid_new.len() {
            if i < mid_old.len() && j < mid_new.len() && mid_old[i] == mid_new[j] {
                ops.push(LineOp::Same(mid_old[i]));
                i += 1;
                j += 1;
            } else if j == mid_new.len()
                || (i < mid_old.len() && table[(i + 1) * width + j] >= table[i * width + j + 1])
            {
                ops.push(LineOp::Removed(mid_old[i]));
                i += 1;
            } else {
                ops.push(LineOp::Added(mid_new[j]));
                j += 1;
            }
        }
    }
    ops.extend(
        old[old.len() - suffix..]
            .iter()
            .map(|line| LineOp::Same(line)),
    );
    ops
}

/// Chemin montré au modèle : sans le préfixe `\\?\` que `canonicalize` ajoute
/// sous Windows (`\\?\D:\projet` devient `D:\projet`), qui déroute les modèles.
pub(crate) fn display_path(path: &Path) -> String {
    let text = path.to_string_lossy();
    match text.strip_prefix(r"\\?\") {
        Some(rest) if !rest.starts_with("UNC") => rest.to_string(),
        _ => text.into_owned(),
    }
}

/// Chemins écrits « à la Unix » par un modèle sous Windows : `/c/Users/x`,
/// `/mnt/c/x`, `/cygdrive/c/x` (lecteur) et `/tmp/x` (dossier temporaire).
#[must_use]
pub fn translate_foreign_path(path: &str) -> Option<PathBuf> {
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
    let candidate = candidate_path(path, root);
    // Le plus long début du chemin qui existe, s'il est dans le projet : le
    // modèle voit quel segment est faux et ce qu'il y a à sa place.
    let existing = candidate
        .ancestors()
        .skip(1)
        .find(|dir| dir.is_dir())
        .filter(|dir| dir.starts_with(root));
    let mut message = format!("Not found: {path}");
    match existing {
        Some(dir) => {
            let shown = dir
                .strip_prefix(root)
                .ok()
                .filter(|relative| relative.components().next().is_some())
                .map_or_else(
                    || format!("the working directory ({})", display_path(root)),
                    |relative| relative.to_string_lossy().replace('\\', "/"),
                );
            let missing = candidate
                .strip_prefix(dir)
                .ok()
                .and_then(|rest| rest.components().next())
                .map(|part| part.as_os_str().to_string_lossy().into_owned())
                .unwrap_or_default();
            let mut entries: Vec<(bool, String)> = fs::read_dir(dir)
                .into_iter()
                .flatten()
                .flatten()
                .map(|entry| {
                    let is_dir = entry.path().is_dir();
                    let name = entry.file_name().to_string_lossy().into_owned();
                    (!is_dir, if is_dir { format!("{name}/") } else { name })
                })
                .collect();
            entries.sort();
            let listed: Vec<String> = entries
                .iter()
                .take(15)
                .map(|(_, name)| name.clone())
                .collect();
            let more = if entries.len() > 15 { ", ..." } else { "" };
            let _ = write!(
                message,
                "\n{shown} has nothing named \"{missing}\". It contains: {}{more}",
                listed.join(", ")
            );
        }
        None => {
            let _ = write!(
                message,
                "\nThe working directory is {}. Use a path relative to it (for example `src/main.rs`), not an invented root.",
                display_path(root)
            );
        }
    }
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

pub(crate) fn normalize_path(path: &str) -> io::Result<PathBuf> {
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

pub(crate) fn normalize_path_allow_missing(path: &str) -> io::Result<PathBuf> {
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
        assert!(message.starts_with("Not found: /workspace/panier.py"));
        assert!(message.contains("The working directory is"));
        assert!(message.contains("src/panier.py"));
        assert!(message.contains("test_panier.py"));
        // Un dossier mal écrit au milieu : ce qui existe, et ce qu'il contient.
        let message = super::missing_path_message("src/paniers/panier.py", &root);
        assert!(
            message.contains("src has nothing named \"paniers\". It contains: panier.py"),
            "{message}"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn edit_file_failures_carry_what_the_model_needs() {
        let path = temp_path("echecs-edit.py");
        let name = path.to_string_lossy().into_owned();
        std::fs::write(
            &path,
            "def load(path):\n    return open(path)\n\ndef save(path):\n    return open(path)\n\nprint(\"ok\")\n",
        )
        .expect("fichier");
        // Plusieurs occurrences : leurs lignes et la ligne d'avant.
        let error = edit_file(&name, "    return open(path)\n", "    return None\n", false)
            .expect_err("ambigu")
            .to_string();
        assert!(error.contains("appears 2 times, at lines 2, 5"), "{error}");
        assert!(error.contains("1 \"def load(path):\""), "{error}");
        // Introuvable : les vraies lignes autour de la plus proche, numérotées.
        let error = edit_file(
            &name,
            "def save(chemin):\n",
            "def save(path, mode):\n",
            false,
        )
        .expect_err("introuvable")
        .to_string();
        assert!(error.contains("The closest place is lines"), "{error}");
        assert!(error.contains("    4| def save(path):"), "{error}");
        // Guillemets échappés deux fois : réessayé, et dit.
        let fixed = edit_file(&name, "print(\\\"ok\\\")", "print(\\\"fini\\\")", false)
            .expect("échappement en trop retiré");
        assert!(fixed
            .match_note
            .unwrap_or_default()
            .contains("extra backslash"));
        assert!(std::fs::read_to_string(&path)
            .expect("lu")
            .contains("print(\"fini\")"));
        // Textes identiques : la ligne où le texte est déjà.
        let error = edit_file(&name, "def load(path):", "def load(path):", false)
            .expect_err("identiques")
            .to_string();
        assert!(error.contains("already in the file at line 1"), "{error}");
        // `old_string` vide sur un fichier absent : créé.
        let created = temp_path("cree-par-edit.py");
        let made = edit_file(&created.to_string_lossy(), "", "x = 1\n", false).expect("créé");
        assert!(made
            .match_note
            .unwrap_or_default()
            .contains("it was created"));
        assert_eq!(std::fs::read_to_string(&created).expect("lu"), "x = 1\n");
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_file(created);
    }

    #[test]
    fn a_whole_file_sent_again_for_a_small_change_is_pointed_to_edit_file() {
        let dir = std::env::temp_dir().join(format!("claw-reecrit-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dossier");
        let path = dir.join("styles.css");
        let original: String = (0..200)
            .map(|i| format!(".r{i} {{ margin: {i}px; }}\n"))
            .collect();
        std::fs::write(&path, &original).expect("css");
        let path = path.display().to_string();
        let small = original.replace(".r5 { margin: 5px; }", ".r5 { margin: 6px; }");
        let output = write_file(&path, &small).expect("écrit");
        assert!(
            output.note.as_deref().is_some_and(|note| note
                .contains("You sent all 200 lines of styles.css again to change 2")
                && note.contains("use edit_file")),
            "{:?}",
            output.note
        );
        // Une vraie réécriture, ou un petit fichier : rien à dire.
        let rewritten: String = (0..200)
            .map(|i| format!(".n{i} {{ padding: {i}px; }}\n"))
            .collect();
        assert!(write_file(&path, &rewritten).expect("écrit").note.is_none());
        let short = dir.join("court.css").display().to_string();
        write_file(&short, "a { color: red; }\n").expect("créé");
        assert!(write_file(&short, "a { color: blue; }\n")
            .expect("écrit")
            .note
            .is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn write_file_keeps_the_byte_order_mark_and_crlf_of_the_file() {
        let path = temp_path("deploy.ps1");
        std::fs::write(
            &path,
            b"\xEF\xBB\xBFWrite-Host \"Op\xC3\xA9ration\"\r\nexit 0\r\n",
        )
        .expect("script");
        let written =
            write_file(&path.to_string_lossy(), "Write-Host \"Fini\"\nexit 0\n").expect("écrit");
        let bytes = std::fs::read(&path).expect("lu");
        assert!(bytes.starts_with(b"\xEF\xBB\xBF"), "{bytes:?}");
        assert_eq!(&bytes[3..], b"Write-Host \"Fini\"\r\nexit 0\r\n");
        assert!(written
            .encoding_note
            .unwrap_or_default()
            .contains("CRLF line endings and UTF-8 byte order mark kept"));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn scripts_get_the_line_endings_and_marks_their_interpreter_needs() {
        let dir = temp_path("scripts");
        std::fs::create_dir_all(&dir).expect("dossier");
        // Un .bat neuf écrit en LF, avec un BOM : CRLF, sans BOM.
        let batch = dir.join("build.bat");
        let written = write_file(
            &batch.to_string_lossy(),
            "\u{feff}@echo off\ngoto :fin\n:fin\nexit /b 0\n",
        )
        .expect("écrit");
        assert_eq!(
            std::fs::read(&batch).expect("lu"),
            b"@echo off\r\ngoto :fin\r\n:fin\r\nexit /b 0\r\n"
        );
        assert!(written
            .encoding_note
            .unwrap_or_default()
            .contains("cmd.exe"));
        // Une modification en LF garde le fichier en CRLF.
        let edited = edit_file(
            &batch.to_string_lossy(),
            "exit /b 0",
            "echo fini\nexit /b 0",
            false,
        )
        .expect("modifié");
        assert_eq!(
            std::fs::read(&batch).expect("lu"),
            b"@echo off\r\ngoto :fin\r\n:fin\r\necho fini\r\nexit /b 0\r\n"
        );
        assert!(edited.match_note.unwrap_or_default().contains("CRLF"));
        // Un script shell en CRLF, même s'il l'était : LF.
        let shell = dir.join("run.sh");
        std::fs::write(&shell, "#!/bin/sh\r\necho a\r\n").expect("script");
        write_file(&shell.to_string_lossy(), "#!/bin/sh\r\necho b\r\n").expect("écrit");
        assert_eq!(std::fs::read(&shell).expect("lu"), b"#!/bin/sh\necho b\n");
        // Un script PowerShell accentué : avec BOM ; sans accent, rien ne change.
        let accented = dir.join("rapport.ps1");
        write_file(&accented.to_string_lossy(), "Write-Output \"Opération\"\n").expect("écrit");
        assert!(std::fs::read(&accented)
            .expect("lu")
            .starts_with(b"\xEF\xBB\xBF"));
        let plain = dir.join("plain.ps1");
        write_file(&plain.to_string_lossy(), "Write-Output 'ok'\n").expect("écrit");
        assert_eq!(std::fs::read(&plain).expect("lu"), b"Write-Output 'ok'\n");
        let _ = std::fs::remove_dir_all(dir);
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
    fn edit_file_patch_holds_only_the_changed_lines_and_their_context() {
        // Vu le 21 octobre 2026 : chaque modification d'un gabarit de 860
        // lignes était rapportée comme 853 lignes retirées et 860 ajoutées.
        let file = temp_path("gabarit.html");
        let lines: Vec<String> = (1..=860).map(|n| format!("<p>ligne {n}</p>")).collect();
        std::fs::write(&file, lines.join("\n") + "\n").expect("gabarit");
        let path = file.to_string_lossy().into_owned();
        let output = edit_file(
            &path,
            "<p>ligne 400</p>",
            "<p>ligne quatre cents</p>",
            false,
        )
        .expect("modification");
        assert_eq!(output.structured_patch.len(), 1);
        let hunk = &output.structured_patch[0];
        assert_eq!(
            (
                hunk.old_start,
                hunk.old_lines,
                hunk.new_start,
                hunk.new_lines
            ),
            (397, 7, 397, 7)
        );
        assert_eq!(
            hunk.lines
                .iter()
                .filter(|line| !line.starts_with(' '))
                .collect::<Vec<_>>(),
            ["-<p>ligne 400</p>", "+<p>ligne quatre cents</p>"]
        );
        // Le fichier d'avant reste joint pour Claw.
        assert_eq!(output.original_file, lines.join("\n") + "\n");
        // Deux changements éloignés : deux passages ; rien de changé : aucun.
        assert_eq!(
            super::make_patch(
                "a\nb\nc\nd\ne\nf\ng\nh\ni\nj",
                "A\nb\nc\nd\ne\nf\ng\nh\ni\nJ"
            )
            .len(),
            2
        );
        assert!(super::make_patch("a\nb", "a\nb").is_empty());
    }

    fn grep_with(args: serde_json::Value) -> std::io::Result<super::GrepSearchOutput> {
        grep_search(&serde_json::from_value(args).expect("arguments de grep_search"))
    }

    #[test]
    fn grep_search_repairs_the_pattern_and_the_filters_and_says_so() {
        use serde_json::json;
        let dir = temp_path("grep-reprises");
        std::fs::create_dir_all(dir.join("src").join("ui")).expect("dossier");
        std::fs::write(dir.join("src").join("app.py"), "print(total)\nitems[0]\n").expect("py");
        std::fs::write(
            dir.join("src").join("ui").join("page.tsx"),
            "export const page = 1\n",
        )
        .expect("tsx");
        std::fs::write(
            dir.join("src").join("ui").join("util.ts"),
            "export const util = 2\n",
        )
        .expect("ts");
        let base = dir.to_string_lossy().into_owned();
        let note = |found: &super::GrepSearchOutput| found.note.clone().unwrap_or_default();

        // Un motif qui n'est pas une expression régulière : cherché comme texte.
        let found = grep_with(json!({"pattern": "print(", "path": base, "output_mode": "content"}))
            .expect("grep");
        assert_eq!(found.num_files, 1);
        assert!(found
            .content
            .clone()
            .unwrap_or_default()
            .contains("print(total)"));
        assert!(
            note(&found).contains("searched as plain text"),
            "{}",
            note(&found)
        );
        // Regard avant : une erreur qui dit quoi faire.
        let error = grep_with(json!({"pattern": "foo(?=bar)", "path": base}))
            .expect_err("non pris en charge");
        assert!(error.to_string().contains("look-ahead"), "{error}");

        // `type` en nom de langage, `glob` relatif et à accolades.
        for (args, expected) in [
            (
                json!({"pattern": "print", "path": base, "type": "python"}),
                1,
            ),
            (json!({"pattern": "print", "path": base, "type": ".py"}), 1),
            (
                json!({"pattern": "export", "path": base, "type": "typescript"}),
                2,
            ),
            (
                json!({"pattern": "export", "path": base, "glob": "src/ui/*.ts"}),
                1,
            ),
            (
                json!({"pattern": "export", "path": base, "glob": "*.{ts,tsx}"}),
                2,
            ),
        ] {
            assert_eq!(
                grep_with(args.clone()).expect("grep").num_files,
                expected,
                "{args}"
            );
        }
        // Un synonyme du mode de sortie, lu et dit.
        let lines = grep_with(json!({"pattern": "export", "path": base, "output_mode": "lines"}))
            .expect("grep");
        assert_eq!(lines.mode.as_deref(), Some("content"));
        assert!(
            note(&lines).contains("was read as \"content\""),
            "{}",
            note(&lines)
        );
        // Un filtre qui écarte tout : dit.
        let empty =
            grep_with(json!({"pattern": "export", "path": base, "type": "golang"})).expect("grep");
        assert_eq!(empty.num_files, 0);
        assert!(
            note(&empty).contains("none passed the filter type \"golang\""),
            "{}",
            note(&empty)
        );
        // Un motif sur plusieurs lignes, dans les trois modes.
        for mode in ["content", "files_with_matches", "count"] {
            let found = grep_with(json!({
                "pattern": "print\\(total\\)\\s*\\nitems",
                "path": base,
                "multiline": true,
                "output_mode": mode,
            }))
            .expect("grep");
            assert_eq!(found.num_files, 1, "{mode}");
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn an_abridged_rewrite_does_not_destroy_the_file() {
        let path = temp_path("abrege.py");
        let original = (1..=40)
            .map(|n| format!("def f{n}():\n    return {n}\n"))
            .collect::<Vec<_>>()
            .concat();
        std::fs::write(&path, &original).expect("original");
        let name = path.to_string_lossy().into_owned();
        // Réécrit en abrégé : refusé, le fichier reste entier.
        let abridged = "def f1():\n    return 1\n# ... rest of the code remains unchanged\n";
        let error = write_file(&name, abridged).expect_err("refusé");
        assert!(
            error
                .to_string()
                .contains("line 3 of your content is a placeholder"),
            "{error}"
        );
        assert_eq!(std::fs::read_to_string(&path).expect("lu"), original);
        // Un nouveau fichier avec une élision : écrit, avec une note.
        let fresh = temp_path("nouveau.js");
        let written = write_file(
            &fresh.to_string_lossy(),
            "const a = 1;\n// ... existing code ...\n",
        )
        .expect("écrit");
        assert!(written.note.unwrap_or_default().contains("placeholder"));
        // edit_file qui remplace du code par une élision : refusé.
        let error = edit_file(
            &name,
            "def f2():\n    return 2\ndef f3():\n    return 3\n",
            "# ...\n",
            false,
        )
        .expect_err("refusé");
        assert!(
            error.to_string().contains("edit_file was not executed"),
            "{error}"
        );
        // Ce qui n'est pas une élision passe.
        assert!(!super::is_elision_line("# Load existing config..."));
        assert!(!super::is_elision_line("    ..."));
        assert!(super::is_elision_line("    // ... existing code ..."));
        assert!(super::is_elision_line(
            "<!-- ... reste du code inchangé -->"
        ));
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_file(fresh);
    }

    #[test]
    fn a_directory_is_listed_instead_of_failing() {
        let dir = temp_path("dossier-lu");
        std::fs::create_dir_all(dir.join("components")).expect("dossier");
        std::fs::write(dir.join("App.tsx"), "export {}\n").expect("fichier");
        let listing = read_file(&dir.to_string_lossy(), None, None).expect("liste");
        assert_eq!(listing.file.content, "components/\nApp.tsx");
        assert!(listing
            .file
            .note
            .unwrap_or_default()
            .contains("is a directory, not a file"));
        let error = write_file(&dir.to_string_lossy(), "x").expect_err("dossier");
        assert!(
            error
                .to_string()
                .contains("is a directory: write_file needs the path of a file inside it"),
            "{error}"
        );
        // `limit: 0` et fichier vide.
        let empty = dir.join("vide.txt");
        std::fs::write(&empty, "").expect("vide");
        let read = read_file(&empty.to_string_lossy(), None, Some(0)).expect("lu");
        let note = read.file.note.unwrap_or_default();
        assert!(
            note.contains("\"limit\": 0 was read as the default"),
            "{note}"
        );
        assert!(note.contains("exists and is empty"), "{note}");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn glob_search_walks_the_tree_and_repairs_common_patterns() {
        // Un `[` dans le dossier de base (projet rangé dans « Site [v2] ») :
        // le motif ne le lit plus comme une classe de caractères.
        let dir = temp_path("glob-reprises [v2]");
        for (file, text) in [
            ("src/app.ts", "a"),
            ("src/ui/page.tsx", "b"),
            ("src/ui/style.css", "c"),
            ("node_modules/pkg/index.ts", "d"),
            ("README.md", "e"),
        ] {
            let path = dir.join(file);
            std::fs::create_dir_all(path.parent().expect("dossier")).expect("dossier");
            std::fs::write(path, text).expect("fichier");
        }
        let base = dir.to_string_lossy().into_owned();
        let glob = |pattern: &str| glob_search(pattern, Some(&base)).expect("glob");
        let note = |found: &super::GlobSearchOutput| found.note.clone().unwrap_or_default();

        // `*.ts` ne trouve rien au premier niveau : cherché dans les sous-dossiers.
        let found = glob("*.ts");
        assert_eq!(found.num_files, 1, "{:?}", found.filenames);
        assert!(
            note(&found).contains("\"**/*.ts\" was searched instead"),
            "{}",
            note(&found)
        );
        // Accolades, chemin relatif, `**` collé, dossier nommé tel quel.
        assert_eq!(glob("**/*.{ts,tsx}").num_files, 2);
        assert_eq!(glob("src/ui/*.tsx").num_files, 1);
        let css = glob("**.css");
        assert_eq!(css.num_files, 1);
        assert!(
            note(&css).contains("was read as \"**/*.css\""),
            "{}",
            note(&css)
        );
        assert_eq!(glob("src").num_files, 3);
        // Rien : la note dit ce qui a été parcouru.
        let empty = glob("**/*.vue");
        assert_eq!(empty.num_files, 0);
        assert!(
            note(&empty).contains("No file matches \"**/*.vue\""),
            "{}",
            note(&empty)
        );
        let _ = std::fs::remove_dir_all(dir);
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

        // Introuvable : les vraies lignes autour de la plus proche, numérotées.
        let error = apply_edit(
            "fn compute_total(items: &[u32]) -> u32 {\n",
            "fn compute_totals(items: &[u32]) -> u64 {",
            "x",
            false,
        )
        .expect_err("missing");
        assert!(
            error.contains("The closest place is lines 1 to 1"),
            "{error}"
        );
        assert!(error.contains("    1| fn compute_total(items"), "{error}");
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
        let note = output.encoding_note.unwrap_or_default();
        assert!(note.contains("now UTF-8"), "{note}");
        // Le fichier était en CRLF : il le reste.
        assert!(note.contains("CRLF line endings kept"), "{note}");
        assert_eq!(
            std::fs::read_to_string(&csv).expect("utf8"),
            "prix \u{2192} 5\r\n"
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
