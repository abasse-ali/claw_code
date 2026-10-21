//! Dessins en caractères (wireframes, schémas en cadres) : leurs traits se rejoignent-ils ?
//!
//! Vu le 21 octobre 2026 dans les plans de design de Claw Web : les modèles comptent mal les
//! colonnes ; un bord droit qui dépasse d'un caractère, un `│` décalé sous un `┐`, et le
//! wireframe « déborde ». Chaque trait vertical doit rejoindre la ligne du dessus ou du
//! dessous : sinon le dessin est faux, et l'endroit est dit (ligne, colonne).

/// Caractères dont le trait descend vers la ligne du dessous.
const DOWN: &[char] = &[
    '│', '┌', '┐', '├', '┤', '┬', '┼', '╷', '║', '╔', '╗', '╠', '╣', '╦', '╬', '┃', '┏', '┓', '┣',
    '┫', '┳', '╋', '╭', '╮',
];
/// Caractères dont le trait monte vers la ligne du dessus.
const UP: &[char] = &[
    '│', '└', '┘', '├', '┤', '┴', '┼', '╵', '║', '╚', '╝', '╠', '╣', '╩', '╬', '┃', '┗', '┛', '┣',
    '┫', '┻', '╋', '╰', '╯',
];
const BOX: &[char] = &[
    '─', '━', '═', '│', '┃', '║', '┌', '┐', '└', '┘', '├', '┤', '┬', '┴', '┼', '╔', '╗', '╚', '╝',
    '╠', '╣', '╦', '╩', '╬', '╭', '╮', '╰', '╯', '┏', '┓', '┗', '┛',
];

/// Largeur d'affichage d'un caractère : 0 (accent combiné, sélecteur), 2 (idéogramme, emoji),
/// sinon 1.
#[must_use]
pub fn char_width(ch: char) -> usize {
    let code = u32::from(ch);
    match code {
        0x0300..=0x036F | 0x200B..=0x200F | 0xFE00..=0xFE0F | 0x20D0..=0x20FF => 0,
        0x1100..=0x115F
        | 0x2E80..=0x303E
        | 0x3041..=0x33FF
        | 0x3400..=0x4DBF
        | 0x4E00..=0x9FFF
        | 0xA000..=0xA4CF
        | 0xAC00..=0xD7A3
        | 0xF900..=0xFAFF
        | 0xFE30..=0xFE4F
        | 0xFF00..=0xFF60
        | 0xFFE0..=0xFFE6
        | 0x1F300..=0x1F64F
        | 0x1F900..=0x1FAFF
        | 0x20000..=0x3FFFD => 2,
        _ => 1,
    }
}

/// Largeur d'affichage d'un texte.
#[must_use]
pub fn display_width(text: &str) -> usize {
    text.chars().map(char_width).sum()
}

/// Les caractères d'une ligne rangés par colonne d'affichage (un caractère large occupe deux
/// colonnes ; la seconde est vide).
fn columns(line: &str) -> Vec<Option<char>> {
    let mut out = Vec::new();
    for ch in line.chars() {
        match char_width(ch) {
            0 => {}
            2 => {
                out.push(Some(ch));
                out.push(None);
            }
            _ => out.push(Some(ch)),
        }
    }
    out
}

/// Blocs de code (entre ``` ) qui contiennent un dessin en cadres : leur première ligne dans le
/// texte (à partir de 1) et leurs lignes.
fn drawings(text: &str) -> Vec<(usize, Vec<&str>)> {
    let mut found = Vec::new();
    let mut current: Option<(usize, Vec<&str>)> = None;
    for (index, line) in text.lines().enumerate() {
        if line.trim_start().starts_with("```") {
            match current.take() {
                Some((start, lines)) => {
                    if lines.iter().any(|line| line.contains(BOX)) {
                        found.push((start, lines));
                    }
                }
                None => current = Some((index + 2, Vec::new())),
            }
            continue;
        }
        if let Some((_, lines)) = current.as_mut() {
            lines.push(line);
        }
    }
    found
}

/// Les endroits où un dessin en cadres est faux : un trait vertical ou un coin qui ne rejoint
/// rien au-dessus ou au-dessous. Au plus `limit` endroits, avec leur ligne dans le texte.
#[must_use]
pub fn misaligned(text: &str, limit: usize) -> Vec<String> {
    let mut problems = Vec::new();
    for (start, lines) in drawings(text) {
        let grid: Vec<Vec<Option<char>>> = lines.iter().map(|line| columns(line)).collect();
        let at = |row: usize, column: usize| {
            grid.get(row)
                .and_then(|line| line.get(column))
                .copied()
                .flatten()
        };
        for (row, line) in grid.iter().enumerate() {
            for (column, cell) in line.iter().enumerate() {
                let Some(ch) = *cell else {
                    continue;
                };
                let needs_up = UP.contains(&ch);
                let needs_down = DOWN.contains(&ch);
                if !needs_up && !needs_down {
                    continue;
                }
                let above = row
                    .checked_sub(1)
                    .and_then(|previous| at(previous, column))
                    .is_some_and(|other| DOWN.contains(&other));
                let below = at(row + 1, column).is_some_and(|other| UP.contains(&other));
                let broken = (needs_up && !above) || (needs_down && !below);
                if broken {
                    let what = match (needs_up && !above, needs_down && !below) {
                        (true, true) => "joins nothing above or below",
                        (true, false) => "joins nothing above",
                        _ => "joins nothing below",
                    };
                    problems.push(format!(
                        "line {}, column {}: `{ch}` {what} (in `{}`)",
                        start + row,
                        column + 1,
                        lines[row].trim_end().chars().take(70).collect::<String>()
                    ));
                    if problems.len() >= limit {
                        return problems;
                    }
                    // Un seul endroit par ligne : le reste de la ligne suit souvent du même décalage.
                    break;
                }
            }
        }
    }
    problems
}

/// Ce que Claw dit au modèle d'un dessin faux.
#[must_use]
pub fn feedback(problems: &[String]) -> String {
    format!(
        "[Claw] The ASCII drawing in your text is misaligned: its boxes do not close, so it overflows when shown. {}\nDo not fix it by counting characters: draw it with the Wireframe tool (rows, columns, labels and lines; it aligns every border and wraps long text) and paste its output as it is, inside a ``` block.",
        problems.join("; ")
    )
}

#[cfg(test)]
mod tests {
    use super::{display_width, misaligned};

    #[test]
    fn a_closed_drawing_passes_and_a_shifted_border_is_found() {
        let good = "Plan\n```\n┌──────────┐\n│ Header   │\n├──────────┤\n│ ┌──┐ ┌──┐│\n│ │A │ │B ││\n│ └──┘ └──┘│\n└──────────┘\n```\n";
        assert!(misaligned(good, 5).is_empty(), "{:?}", misaligned(good, 5));
        let bad = "```\n┌──────────┐\n│ Header    │\n└──────────┘\n```";
        let found = misaligned(bad, 5);
        assert!(
            found.iter().any(|problem| problem
                .starts_with("line 3, column 13: `│` joins nothing above or below")),
            "{found:?}"
        );
        // Un texte sans dessin, ou un tableau Markdown hors bloc : rien.
        assert!(misaligned("| a | b |\n|---|---|", 5).is_empty());
        assert_eq!(display_width("Épargne 1 250,00 €"), 18);
    }
}
