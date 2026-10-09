//! Typographie des textes écrits par Claw : jamais de tiret cadratin, un
//! simple « - » à la place.

use std::borrow::Cow;

/// Tiret cadratin. Claw ne l'écrit jamais : il devient « - ».
pub const EM_DASH: char = '\u{2014}';
/// Barre horizontale, qui se confond avec lui à l'écran.
const HORIZONTAL_BAR: char = '\u{2015}';

fn is_long_dash(character: char) -> bool {
    character == EM_DASH || character == HORIZONTAL_BAR
}

/// Ponctuation qui se colle au tiret : pas d'espace ajoutée devant elle.
fn hugs_the_dash(character: char) -> bool {
    character.is_whitespace() || matches!(character, ',' | '.' | ')' | ']')
}

/// `text` sans tiret cadratin : un « - » le remplace, séparé de ses voisins
/// par une espace (« mot\u{2014}mot » devient « mot - mot »).
#[must_use]
pub fn without_em_dash(text: &str) -> Cow<'_, str> {
    if text.contains(is_long_dash) {
        Cow::Owned(DashFilter::default().push(text))
    } else {
        Cow::Borrowed(text)
    }
}

/// Le même remplacement pour un texte reçu par morceaux : le filtre retient
/// le dernier caractère écrit, pour que l'espacement reste juste quand le
/// tiret tombe au bord d'un morceau.
#[derive(Debug, Default, Clone)]
pub struct DashFilter {
    last: Option<char>,
    after_dash: bool,
}

impl DashFilter {
    /// Le morceau suivant, tirets cadratins remplacés.
    pub fn push(&mut self, piece: &str) -> String {
        let mut output = String::with_capacity(piece.len());
        for character in piece.chars() {
            if is_long_dash(character) {
                // Deux tirets à la suite n'en font qu'un.
                if self.after_dash {
                    continue;
                }
                if self.last.is_some_and(|last| !last.is_whitespace()) {
                    output.push(' ');
                }
                output.push('-');
                self.last = Some('-');
                self.after_dash = true;
            } else {
                if self.after_dash && !hugs_the_dash(character) {
                    output.push(' ');
                }
                self.after_dash = false;
                output.push(character);
                self.last = Some(character);
            }
        }
        output
    }
}

#[cfg(test)]
mod tests {
    use super::{is_long_dash, without_em_dash, DashFilter, EM_DASH};
    use std::borrow::Cow;
    use std::path::{Path, PathBuf};

    /// Fichiers texte du projet sous `directory`, hors dossiers générés.
    fn text_files(directory: &Path, found: &mut Vec<PathBuf>) {
        // Dossiers générés ou téléchargés, et dossiers cachés (`.vscode-test`…).
        const SKIPPED: [&str; 4] = ["target", "node_modules", "out", "dist"];
        const TEXT: [&str; 11] = [
            "rs", "html", "md", "js", "mjs", "ts", "json", "toml", "py", "css", "txt",
        ];
        let Ok(entries) = std::fs::read_dir(directory) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            if path.is_dir() {
                if !name.starts_with('.') && !SKIPPED.contains(&name.as_str()) {
                    text_files(&path, found);
                }
            } else if path
                .extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| TEXT.contains(&extension))
            {
                found.push(path);
            }
        }
    }

    #[test]
    fn project_sources_contain_no_em_dash() {
        // De `crates/runtime` à la racine du dépôt (README, `rust/`, `src/`).
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let mut files = Vec::new();
        text_files(&root.join("rust"), &mut files);
        text_files(&root.join("src"), &mut files);
        files.push(root.join("README.md"));
        assert!(files.len() > 50, "sources introuvables depuis {root:?}");
        let offenders: Vec<String> = files
            .iter()
            .filter_map(|path| {
                let text = std::fs::read_to_string(path).ok()?;
                let line = text.lines().position(|line| line.contains(is_long_dash))?;
                Some(format!("{}:{}", path.display(), line + 1))
            })
            .collect();
        assert!(
            offenders.is_empty(),
            "tiret cadratin à remplacer par « - » : {offenders:?}"
        );
    }

    #[test]
    fn replaces_em_dashes_with_spaced_hyphens() {
        let dash = EM_DASH;
        assert_eq!(
            without_em_dash(&format!("Le test passe {dash} tout est vert.")),
            "Le test passe - tout est vert."
        );
        assert_eq!(
            without_em_dash(&format!("fast{dash}and correct")),
            "fast - and correct"
        );
        assert_eq!(
            without_em_dash(&format!("{dash} Bonjour\n{dash}Au revoir")),
            "- Bonjour\n- Au revoir"
        );
        assert_eq!(
            without_em_dash(&format!(
                "(voir plus bas{dash}), fin{dash}{dash}ici\u{2015}là"
            )),
            "(voir plus bas -), fin - ici - là"
        );
        // Rien à changer : le texte n'est pas copié, et le trait d'union reste.
        assert!(matches!(
            without_em_dash("un demi-tour - 10–20"),
            Cow::Borrowed(_)
        ));
    }

    #[test]
    fn keeps_spacing_right_across_stream_pieces() {
        let dash = EM_DASH.to_string();
        let mut filter = DashFilter::default();
        let pieces = [
            "C'est fait",
            dash.as_str(),
            "les tests",
            " passent ",
            &dash,
            " tous.",
        ];
        let text: String = pieces.iter().map(|piece| filter.push(piece)).collect();
        assert_eq!(text, "C'est fait - les tests passent - tous.");
        assert!(!text.contains(EM_DASH));
    }
}
