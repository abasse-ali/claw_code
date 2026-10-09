//! Typographie des textes écrits par Claw : jamais de tiret cadratin (un
//! simple « - » à la place) et jamais d'emoji.
//!
//! Le filtre s'applique à ce qu'un modèle écrit en sortie (réponses, réflexion
//! affichée, sous-agents, avis, résumés), pas aux fichiers qu'il lit ou écrit.

use std::borrow::Cow;

/// Tiret cadratin. Claw ne l'écrit jamais : il devient « - ».
pub const EM_DASH: char = '\u{2014}';
/// Barre horizontale, qui se confond avec lui à l'écran.
const HORIZONTAL_BAR: char = '\u{2015}';
/// Sélecteur de variante qui donne à un symbole son dessin d'emoji.
const EMOJI_SELECTOR: char = '\u{FE0F}';
/// Liant des emoji composés (famille, métier).
const ZERO_WIDTH_JOINER: char = '\u{200D}';
/// Cadre de touche qui suit un chiffre (« 1 » dans une touche).
const KEYCAP: char = '\u{20E3}';

/// Emoji d'Unicode : points de code `Extended_Pictographic` ou
/// `Emoji_Presentation` (fichier emoji-data.txt), en plages triées.
const EMOJI_RANGES: [(u32, u32); 77] = [
    (0x00A9, 0x00A9),
    (0x00AE, 0x00AE),
    (0x203C, 0x203C),
    (0x2049, 0x2049),
    (0x2122, 0x2122),
    (0x2139, 0x2139),
    (0x2194, 0x2199),
    (0x21A9, 0x21AA),
    (0x231A, 0x231B),
    (0x2328, 0x2328),
    (0x2388, 0x2388),
    (0x23CF, 0x23CF),
    (0x23E9, 0x23F3),
    (0x23F8, 0x23FA),
    (0x24C2, 0x24C2),
    (0x25AA, 0x25AB),
    (0x25B6, 0x25B6),
    (0x25C0, 0x25C0),
    (0x25FB, 0x25FE),
    (0x2600, 0x2605),
    (0x2607, 0x2612),
    (0x2614, 0x2685),
    (0x2690, 0x2705),
    (0x2708, 0x2712),
    (0x2714, 0x2714),
    (0x2716, 0x2716),
    (0x271D, 0x271D),
    (0x2721, 0x2721),
    (0x2728, 0x2728),
    (0x2733, 0x2734),
    (0x2744, 0x2744),
    (0x2747, 0x2747),
    (0x274C, 0x274C),
    (0x274E, 0x274E),
    (0x2753, 0x2755),
    (0x2757, 0x2757),
    (0x2763, 0x2767),
    (0x2795, 0x2797),
    (0x27A1, 0x27A1),
    (0x27B0, 0x27B0),
    (0x27BF, 0x27BF),
    (0x2934, 0x2935),
    (0x2B05, 0x2B07),
    (0x2B1B, 0x2B1C),
    (0x2B50, 0x2B50),
    (0x2B55, 0x2B55),
    (0x3030, 0x3030),
    (0x303D, 0x303D),
    (0x3297, 0x3297),
    (0x3299, 0x3299),
    (0x1F000, 0x1F0FF),
    (0x1F10D, 0x1F10F),
    (0x1F12F, 0x1F12F),
    (0x1F16C, 0x1F171),
    (0x1F17E, 0x1F17F),
    (0x1F18E, 0x1F18E),
    (0x1F191, 0x1F19A),
    (0x1F1AD, 0x1F1FF),
    (0x1F201, 0x1F20F),
    (0x1F21A, 0x1F21A),
    (0x1F22F, 0x1F22F),
    (0x1F232, 0x1F23A),
    (0x1F23C, 0x1F23F),
    (0x1F249, 0x1F53D),
    (0x1F546, 0x1F64F),
    (0x1F680, 0x1F6FF),
    (0x1F774, 0x1F77F),
    (0x1F7D5, 0x1F7FF),
    (0x1F80C, 0x1F80F),
    (0x1F848, 0x1F84F),
    (0x1F85A, 0x1F85F),
    (0x1F888, 0x1F88F),
    (0x1F8AE, 0x1F8FF),
    (0x1F90C, 0x1F93A),
    (0x1F93C, 0x1F945),
    (0x1F947, 0x1FAFF),
    (0x1FC00, 0x1FFFD),
];

fn is_long_dash(character: char) -> bool {
    character == EM_DASH || character == HORIZONTAL_BAR
}

/// `character` est un emoji (pictogramme, drapeau, symbole dessiné).
#[must_use]
pub fn is_emoji(character: char) -> bool {
    let code = character as u32;
    code >= EMOJI_RANGES[0].0
        && EMOJI_RANGES
            .binary_search_by(|&(first, last)| {
                if code < first {
                    std::cmp::Ordering::Greater
                } else if code > last {
                    std::cmp::Ordering::Less
                } else {
                    std::cmp::Ordering::Equal
                }
            })
            .is_ok()
}

/// Ce qui accompagne un emoji et disparaît avec lui : liant, étiquettes de drapeau.
fn follows_an_emoji(character: char) -> bool {
    character == ZERO_WIDTH_JOINER || ('\u{E0020}'..='\u{E007F}').contains(&character)
}

/// Caractère que Claw n'écrit pas tel quel.
fn is_banned(character: char) -> bool {
    is_long_dash(character)
        || character == EMOJI_SELECTOR
        || character == KEYCAP
        || is_emoji(character)
}

/// Signe typographique qui garde le sens d'un emoji courant (coche, croix,
/// flèche) ; les autres emoji sont retirés sans rien laisser.
fn emoji_replacement(character: char) -> Option<&'static str> {
    Some(match character {
        '\u{2705}' | '\u{2714}' | '\u{2611}' => "✓",
        '\u{274C}' | '\u{274E}' | '\u{2716}' | '\u{2612}' => "✗",
        '\u{2610}' | '\u{25AB}' | '\u{25FB}' | '\u{25FD}' | '\u{2B1C}' => "□",
        '\u{25AA}' | '\u{25FC}' | '\u{25FE}' | '\u{2B1B}' | '\u{23F9}' => "■",
        '\u{27A1}' => "→",
        '\u{2B05}' => "←",
        '\u{2B06}' => "↑",
        '\u{2B07}' => "↓",
        '\u{2194}' => "<->",
        '\u{25B6}' => "▸",
        '\u{25C0}' => "◂",
        '\u{2733}' | '\u{2734}' | '\u{2747}' => "✻",
        '\u{23FA}' => "●",
        '\u{23F8}' => "‖",
        '\u{203C}' => "!!",
        '\u{2049}' => "!?",
        '\u{2753}' | '\u{2754}' => "?",
        '\u{2755}' | '\u{2757}' => "!",
        '\u{2139}' => "i",
        '\u{2795}' => "+",
        '\u{2796}' => "-",
        '\u{00A9}' => "(c)",
        '\u{00AE}' => "(r)",
        '\u{2122}' => "(tm)",
        _ => return None,
    })
}

/// Ponctuation qui se colle au tiret : pas d'espace ajoutée devant elle.
fn hugs_the_dash(character: char) -> bool {
    character.is_whitespace() || matches!(character, ',' | '.' | ')' | ']')
}

/// `text` tel que Claw l'écrit : sans tiret cadratin (un « - » le remplace,
/// séparé de ses voisins par une espace : « mot\u{2014}mot » devient
/// « mot - mot ») et sans emoji.
#[must_use]
pub fn cleaned(text: &str) -> Cow<'_, str> {
    if text.contains(is_banned) {
        Cow::Owned(OutputFilter::default().push(text))
    } else {
        Cow::Borrowed(text)
    }
}

/// Le même nettoyage pour un texte reçu par morceaux : le filtre retient où
/// il en est, pour que l'espacement reste juste quand un tiret ou un emoji
/// tombe au bord d'un morceau.
#[derive(Debug, Default, Clone)]
pub struct OutputFilter {
    last: Option<char>,
    after_dash: bool,
    /// Le caractère précédent était un emoji : son liant ou sa variante suit.
    after_emoji: bool,
    /// Un emoji retiré laissait deux espaces de suite : la suivante part aussi.
    swallow_space: bool,
}

impl OutputFilter {
    fn emit(&mut self, output: &mut String, character: char) {
        if self.after_dash && !hugs_the_dash(character) {
            output.push(' ');
        }
        self.after_dash = false;
        output.push(character);
        self.last = Some(character);
    }

    /// Le morceau suivant, nettoyé.
    pub fn push(&mut self, piece: &str) -> String {
        let mut output = String::with_capacity(piece.len());
        for character in piece.chars() {
            if is_long_dash(character) {
                self.after_emoji = false;
                self.swallow_space = false;
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
            } else if is_emoji(character) {
                self.after_emoji = true;
                if let Some(replacement) = emoji_replacement(character) {
                    self.swallow_space = false;
                    for sign in replacement.chars() {
                        self.emit(&mut output, sign);
                    }
                } else if self.last.is_none_or(char::is_whitespace) {
                    // En début de ligne ou après une espace : l'espace qui
                    // suivait l'emoji ne doit pas rester en double.
                    self.swallow_space = true;
                }
            } else if character == EMOJI_SELECTOR
                || character == KEYCAP
                || (self.after_emoji && follows_an_emoji(character))
            {
                // Disparaît avec l'emoji qu'il accompagne.
            } else if self.swallow_space && matches!(character, ' ' | '\t') {
                self.swallow_space = false;
                self.after_emoji = false;
            } else {
                self.swallow_space = false;
                self.after_emoji = false;
                self.emit(&mut output, character);
            }
        }
        output
    }
}

#[cfg(test)]
mod tests {
    use super::{cleaned, is_banned, is_emoji, OutputFilter, EMOJI_RANGES, EM_DASH};
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
    fn project_sources_contain_no_em_dash_or_emoji() {
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
                let line = text.lines().position(|line| line.contains(is_banned))?;
                Some(format!("{}:{}", path.display(), line + 1))
            })
            .collect();
        assert!(
            offenders.is_empty(),
            "tiret cadratin (à remplacer par « - ») ou emoji : {offenders:?}"
        );
    }

    #[test]
    fn replaces_em_dashes_with_spaced_hyphens() {
        let dash = EM_DASH;
        assert_eq!(
            cleaned(&format!("Le test passe {dash} tout est vert.")),
            "Le test passe - tout est vert."
        );
        assert_eq!(
            cleaned(&format!("fast{dash}and correct")),
            "fast - and correct"
        );
        assert_eq!(
            cleaned(&format!("{dash} Bonjour\n{dash}Au revoir")),
            "- Bonjour\n- Au revoir"
        );
        assert_eq!(
            cleaned(&format!(
                "(voir plus bas{dash}), fin{dash}{dash}ici\u{2015}là"
            )),
            "(voir plus bas -), fin - ici - là"
        );
        // Rien à changer : le texte n'est pas copié, et le trait d'union reste.
        assert!(matches!(
            cleaned("un demi-tour - 10–20 ✓ ✗ → ● ■"),
            Cow::Borrowed(_)
        ));
    }

    #[test]
    fn keeps_spacing_right_across_stream_pieces() {
        let dash = EM_DASH.to_string();
        let mut filter = OutputFilter::default();
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

    #[test]
    fn emoji_ranges_are_sorted_and_spare_plain_symbols() {
        assert!(EMOJI_RANGES
            .windows(2)
            .all(|pair| pair[0].1 < pair[1].0 && pair[0].0 <= pair[0].1));
        // Fusée, coche verte, attention, drapeau, teinte de peau.
        for code in [0x1F680, 0x2705, 0x26A0, 0x1F1EB, 0x1F3FD, 0x1F9E0] {
            assert!(
                is_emoji(char::from_u32(code).expect("caractère")),
                "{code:X}"
            );
        }
        // Signes typographiques, lettres accentuées, flèches et tirets simples.
        for sign in "✓✗●■□▲✻❯→←↑↓─╭‖«»éàç€°–…·".chars() {
            assert!(!is_emoji(sign), "{sign}");
        }
    }

    #[test]
    fn removes_emoji_and_keeps_the_meaning_of_common_ones() {
        // Titres et listes tels qu'un modèle les décore.
        assert_eq!(
            cleaned("## \u{2705} Projet terminé \u{1F680}\n\n### \u{1F4C1} Structure\n- \u{1F3A8} Design moderne"),
            "## ✓ Projet terminé \n\n### Structure\n- Design moderne"
        );
        // Une espace de chaque côté n'en laisse qu'une.
        assert_eq!(
            cleaned("Votre portfolio est prêt ! \u{1F680} Le projet est autonome."),
            "Votre portfolio est prêt ! Le projet est autonome."
        );
        // Symbole suivi du sélecteur d'emoji, emoji composé, drapeau, touche.
        assert_eq!(
            cleaned("\u{26A0}\u{FE0F} Attention : \u{1F468}\u{200D}\u{1F4BB} dev \u{1F1EB}\u{1F1F7}, étape 1\u{FE0F}\u{20E3}"),
            "Attention : dev , étape 1"
        );
        // Coches, croix et flèches gardent leur sens.
        assert_eq!(
            cleaned("| \u{2705} Présent | \u{274C} Absent | \u{2714}\u{FE0F} ok | suite \u{27A1}\u{FE0F} fin |"),
            "| ✓ Présent | ✗ Absent | ✓ ok | suite → fin |"
        );
        assert!(!cleaned("\u{1F389}\u{1F389} Fini \u{2728}").contains(is_banned));
        assert_eq!(cleaned("\u{1F389}\u{1F389} Fini \u{2728}"), "Fini ");
    }

    #[test]
    fn removes_emoji_split_across_stream_pieces() {
        let mut filter = OutputFilter::default();
        let pieces = [
            "## ",
            "\u{1F3AF}",
            " Vérification\n",
            "\u{26A0}",
            "\u{FE0F}",
            " Attention ",
            "\u{1F468}",
            "\u{200D}\u{1F4BB}",
            " fin",
        ];
        let text: String = pieces.iter().map(|piece| filter.push(piece)).collect();
        assert_eq!(text, "## Vérification\nAttention fin");
    }
}
