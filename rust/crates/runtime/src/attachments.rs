//! Pièces jointes d'une demande : `@capture.png`, `@"C:\mes images\maquette.jpg"`
//! ou un chemin d'image collé tel quel (glisser-déposer dans le terminal).

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

/// Taille maximale d'une image (limite courante des API de modèles).
pub const MAX_IMAGE_BYTES: usize = 5 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageAttachment {
    pub media_type: String,
    /// Contenu encodé en base64.
    pub data: String,
    pub path: PathBuf,
}

/// Type MIME d'après l'extension, pour les formats acceptés par les modèles.
#[must_use]
pub fn image_media_type(path: &Path) -> Option<&'static str> {
    let extension = path.extension()?.to_str()?.to_ascii_lowercase();
    Some(match extension.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        _ => return None,
    })
}

/// Lit une image et l'encode.
pub fn load_image(path: &Path) -> Result<ImageAttachment, String> {
    let media_type = image_media_type(path).ok_or_else(|| {
        format!(
            "{} : format non pris en charge (png, jpg, gif, webp)",
            path.display()
        )
    })?;
    let bytes = std::fs::read(path).map_err(|error| format!("{} : {error}", path.display()))?;
    if bytes.len() > MAX_IMAGE_BYTES {
        return Err(format!(
            "{} : image trop lourde ({} Mo, maximum 5 Mo)",
            path.display(),
            bytes.len() / (1024 * 1024)
        ));
    }
    Ok(ImageAttachment {
        media_type: media_type.to_string(),
        data: encode_base64(&bytes),
        path: path.to_path_buf(),
    })
}

/// Extrait les images citées dans `input` (relatives à `cwd`). Renvoie le texte
/// où chaque image est remplacée par `[image : nom]`, les images et les erreurs.
#[must_use]
pub fn extract_image_attachments(
    input: &str,
    cwd: &Path,
) -> (String, Vec<ImageAttachment>, Vec<String>) {
    let mut text = String::with_capacity(input.len());
    let mut images = Vec::new();
    let mut errors = Vec::new();
    let mut rest = input;
    while !rest.is_empty() {
        let Some((start, token, raw_len)) = next_candidate(rest) else {
            text.push_str(rest);
            break;
        };
        text.push_str(&rest[..start]);
        let path = {
            let candidate = PathBuf::from(&token);
            if candidate.is_absolute() {
                candidate
            } else {
                cwd.join(candidate)
            }
        };
        let looks_like_image = image_media_type(&path).is_some();
        let explicit = rest[start..].starts_with('@');
        if looks_like_image && (explicit || path.is_file()) {
            match load_image(&path) {
                Ok(image) => {
                    let name = path
                        .file_name()
                        .map_or_else(|| token.clone(), |name| name.to_string_lossy().to_string());
                    let _ = write!(text, "[image : {name}]");
                    images.push(image);
                }
                Err(error) => {
                    errors.push(error);
                    text.push_str(&rest[start..start + raw_len]);
                }
            }
        } else {
            text.push_str(&rest[start..start + raw_len]);
        }
        rest = &rest[start + raw_len..];
    }
    (text, images, errors)
}

/// Taille maximale d'un fichier texte joint avec `@chemin`.
pub const MAX_FILE_MENTION_BYTES: usize = 200 * 1024;

/// Contenu des fichiers (et listes des dossiers) cités avec `@chemin` dans la
/// demande, hors images. Renvoie les blocs à ajouter au message.
#[must_use]
pub fn file_mention_blocks(input: &str, cwd: &Path) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut seen = Vec::new();
    let mut rest = input;
    while let Some((start, token, raw_len)) = next_candidate(rest) {
        let explicit = rest[start..].starts_with('@');
        rest = &rest[start + raw_len..];
        if !explicit || seen.contains(&token) {
            continue;
        }
        let path = {
            let candidate = PathBuf::from(&token);
            if candidate.is_absolute() {
                candidate
            } else {
                cwd.join(candidate)
            }
        };
        if image_media_type(&path).is_some() {
            continue;
        }
        if path.is_dir() {
            let mut names = std::fs::read_dir(&path)
                .map(|entries| {
                    entries
                        .flatten()
                        .map(|entry| {
                            let name = entry.file_name().to_string_lossy().to_string();
                            if entry.path().is_dir() {
                                format!("{name}/")
                            } else {
                                name
                            }
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            names.sort();
            names.truncate(200);
            blocks.push(format!(
                "<directory path=\"{token}\">\n{}\n</directory>",
                names.join("\n")
            ));
            seen.push(token);
        } else if let Ok(bytes) = std::fs::read(&path) {
            if bytes.len() > MAX_FILE_MENTION_BYTES {
                blocks.push(format!(
                    "<file path=\"{token}\">(file too large to inline: {} KiB - read it with read_file)</file>",
                    bytes.len() / 1024
                ));
            } else if let Ok(text) = String::from_utf8(bytes) {
                blocks.push(format!("<file path=\"{token}\">\n{text}\n</file>"));
            }
            seen.push(token);
        }
    }
    blocks
}

/// Prochain `@chemin`, `@"chemin"` ou `"chemin.png"` : (début, chemin, longueur brute).
fn next_candidate(text: &str) -> Option<(usize, String, usize)> {
    let mut offset = 0;
    while offset < text.len() {
        let slice = &text[offset..];
        let position = slice.find(['@', '"'])?;
        let start = offset + position;
        let at_word_start = start == 0
            || text[..start]
                .chars()
                .next_back()
                .is_some_and(char::is_whitespace);
        let tail = &text[start..];
        if at_word_start {
            if let Some(after) = tail.strip_prefix("@\"").or_else(|| tail.strip_prefix('"')) {
                if let Some(end) = after.find('"') {
                    let token = after[..end].to_string();
                    let prefix = tail.len() - after.len();
                    if !token.is_empty()
                        && (tail.starts_with('@') || image_media_type(Path::new(&token)).is_some())
                    {
                        return Some((start, token, prefix + end + 1));
                    }
                }
            } else if let Some(after) = tail.strip_prefix('@') {
                let end = after.find(char::is_whitespace).unwrap_or(after.len());
                let token = after[..end].trim_end_matches([',', ';', ')']).to_string();
                if !token.is_empty() {
                    return Some((start, token.clone(), 1 + token.len()));
                }
            }
        }
        offset = start + 1;
    }
    None
}

const BASE64_ALPHABET: &[u8; 64] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

#[must_use]
pub fn encode_base64(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = u32::from(chunk[0]);
        let b1 = u32::from(chunk.get(1).copied().unwrap_or(0));
        let b2 = u32::from(chunk.get(2).copied().unwrap_or(0));
        let triple = (b0 << 16) | (b1 << 8) | b2;
        output.push(char::from(BASE64_ALPHABET[(triple >> 18) as usize & 63]));
        output.push(char::from(BASE64_ALPHABET[(triple >> 12) as usize & 63]));
        output.push(if chunk.len() > 1 {
            char::from(BASE64_ALPHABET[(triple >> 6) as usize & 63])
        } else {
            '='
        });
        output.push(if chunk.len() > 2 {
            char::from(BASE64_ALPHABET[triple as usize & 63])
        } else {
            '='
        });
    }
    output
}

#[cfg(test)]
mod tests {
    use super::{encode_base64, extract_image_attachments};
    use std::fs;

    #[test]
    fn encodes_base64_like_the_standard() {
        assert_eq!(encode_base64(b""), "");
        assert_eq!(encode_base64(b"f"), "Zg==");
        assert_eq!(encode_base64(b"fo"), "Zm8=");
        assert_eq!(encode_base64(b"foo"), "Zm9v");
        assert_eq!(encode_base64(b"foobar"), "Zm9vYmFy");
        assert_eq!(encode_base64(&[0xff, 0xfe, 0x00]), "//4A");
    }

    #[test]
    fn extracts_explicit_quoted_and_dropped_images() {
        let dir = std::env::temp_dir().join(format!(
            "claw-attach-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        fs::create_dir_all(dir.join("mes images")).expect("dir");
        fs::write(dir.join("capture.png"), b"\x89PNG").expect("png");
        fs::write(dir.join("mes images").join("maquette.jpg"), b"jpg").expect("jpg");

        let input = "Compare @capture.png et @\"mes images/maquette.jpg\", écris à toto@exemple.fr";
        let (text, images, errors) = extract_image_attachments(input, &dir);
        assert_eq!(images.len(), 2, "{errors:?}");
        assert_eq!(images[0].media_type, "image/png");
        assert_eq!(images[1].media_type, "image/jpeg");
        assert_eq!(
            text,
            "Compare [image : capture.png] et [image : maquette.jpg], écris à toto@exemple.fr"
        );

        // Chemin collé entre guillemets (glisser-déposer), sans @.
        let dropped = format!("Que montre \"{}\" ?", dir.join("capture.png").display());
        let (text, images, _) = extract_image_attachments(&dropped, &dir);
        assert_eq!(images.len(), 1);
        assert_eq!(text, "Que montre [image : capture.png] ?");

        // @ vers un fichier qui n'est pas une image : texte inchangé.
        let (text, images, errors) = extract_image_attachments("Lis @src/main.rs", &dir);
        assert!(images.is_empty() && errors.is_empty());
        assert_eq!(text, "Lis @src/main.rs");

        // Image citée mais absente : erreur signalée.
        let (_, images, errors) = extract_image_attachments("Vois @absente.png", &dir);
        assert!(images.is_empty());
        assert_eq!(errors.len(), 1);
        let _ = fs::remove_dir_all(dir);
    }
}
