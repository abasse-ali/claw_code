//! Outil `Wireframe` : un wireframe ASCII dessiné par Claw, juste au caractère près.
//!
//! Les modèles dessinent leurs wireframes en comptant les colonnes, et se trompent : bords
//! décalés, texte qui déborde du cadre (vu le 21 octobre 2026 dans les plans de design de
//! Claw Web). Ici le modèle décrit les zones (lignes du cadre, colonnes, libellés, textes) et
//! le dessin est calculé : chaque bord tombe à sa place, un texte trop long passe à la ligne,
//! rien ne dépasse.

use runtime::ascii_art::{char_width, display_width};

/// Une colonne d'une zone : une boîte avec son libellé et ses lignes.
pub struct Column {
    pub label: String,
    pub lines: Vec<String>,
    /// Part de la largeur (1 par défaut) : 2 fait une colonne deux fois plus large.
    pub weight: usize,
}

/// Une zone du cadre, de haut en bas : son libellé, ses lignes, puis ses colonnes côte à côte.
pub struct Row {
    pub label: String,
    pub lines: Vec<String>,
    pub columns: Vec<Column>,
}

pub struct Wireframe {
    pub title: String,
    /// Largeur totale en caractères (30 à 120, 64 par défaut).
    pub width: usize,
    pub rows: Vec<Row>,
}

const MIN_BOX: usize = 7;
const GAP: usize = 2;

/// Coupe un texte en lignes d'au plus `width` colonnes, aux espaces, et un mot trop long
/// au caractère.
pub fn wrap(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut lines = Vec::new();
    for paragraph in text.split('\n') {
        let mut line = String::new();
        let mut used = 0;
        for word in paragraph.split_whitespace() {
            let size = display_width(word);
            if used > 0 && used + 1 + size > width {
                lines.push(std::mem::take(&mut line));
                used = 0;
            }
            if size > width {
                // Mot plus long que la ligne : coupé.
                for ch in word.chars() {
                    let step = char_width(ch);
                    if used + step > width {
                        lines.push(std::mem::take(&mut line));
                        used = 0;
                    }
                    line.push(ch);
                    used += step;
                }
                continue;
            }
            if used > 0 {
                line.push(' ');
                used += 1;
            }
            line.push_str(word);
            used += size;
        }
        lines.push(line);
    }
    lines
}

/// Le texte complété d'espaces jusqu'à `width` colonnes.
fn pad(text: &str, width: usize) -> String {
    let used = display_width(text);
    format!("{text}{}", " ".repeat(width.saturating_sub(used)))
}

/// Largeurs des colonnes (boîtes comprises) pour `available` colonnes de texte.
fn widths(columns: &[Column], available: usize) -> Result<Vec<usize>, String> {
    let count = columns.len();
    let room = available.saturating_sub(GAP * (count - 1));
    if room < MIN_BOX * count {
        return Err(format!(
            "{count} columns do not fit in this width: use fewer columns in that row, or a larger width (at least {}).",
            MIN_BOX * count + GAP * (count - 1) + 4
        ));
    }
    let total: usize = columns.iter().map(|column| column.weight.max(1)).sum();
    let mut sizes: Vec<usize> = columns
        .iter()
        .map(|column| (room * column.weight.max(1) / total).max(MIN_BOX))
        .collect();
    // Le reste de la division va aux premières colonnes ; un excès est repris aux plus larges.
    let mut used: usize = sizes.iter().sum();
    let mut index = 0;
    while used < room {
        sizes[index % count] += 1;
        used += 1;
        index += 1;
    }
    while used > room {
        let widest = (0..count).max_by_key(|&slot| sizes[slot]).unwrap_or(0);
        if sizes[widest] <= MIN_BOX {
            break;
        }
        sizes[widest] -= 1;
        used -= 1;
    }
    Ok(sizes)
}

/// Le wireframe dessiné, ou ce qui l'empêche.
pub fn render(frame: &Wireframe) -> Result<String, String> {
    if frame.rows.is_empty() {
        return Err("give at least one row (a region of the screen, top to bottom)".to_string());
    }
    let width = frame.width.clamp(30, 120);
    let inner = width - 4;
    let mut out = Vec::new();
    if !frame.title.trim().is_empty() {
        out.extend(wrap(frame.title.trim(), width));
    }
    let line = |content: &str| format!("│ {} │", pad(content, inner));
    out.push(format!("┌{}┐", "─".repeat(width - 2)));
    for (index, row) in frame.rows.iter().enumerate() {
        if index > 0 {
            out.push(format!("├{}┤", "─".repeat(width - 2)));
        }
        if !row.label.trim().is_empty() {
            for text in wrap(row.label.trim(), inner) {
                out.push(line(&text));
            }
        }
        for text in &row.lines {
            for piece in wrap(text, inner) {
                out.push(line(&piece));
            }
        }
        if row.columns.is_empty() {
            if row.label.trim().is_empty() && row.lines.is_empty() {
                out.push(line(""));
            }
            continue;
        }
        let sizes =
            widths(&row.columns, inner).map_err(|error| format!("row {}: {error}", index + 1))?;
        // Contenu de chaque boîte, coupé à sa largeur ; toutes ont la hauteur de la plus haute.
        let contents: Vec<Vec<String>> = row
            .columns
            .iter()
            .zip(&sizes)
            .map(|(column, size)| {
                let text_width = size - 4;
                let mut lines = Vec::new();
                if !column.label.trim().is_empty() {
                    lines.extend(wrap(column.label.trim(), text_width));
                }
                for text in &column.lines {
                    lines.extend(wrap(text, text_width));
                }
                lines
            })
            .collect();
        let height = contents.iter().map(Vec::len).max().unwrap_or(0).max(1);
        let join = |parts: Vec<String>| line(&parts.join(&" ".repeat(GAP)));
        out.push(join(
            sizes
                .iter()
                .map(|size| format!("┌{}┐", "─".repeat(size - 2)))
                .collect(),
        ));
        for level in 0..height {
            out.push(join(
                contents
                    .iter()
                    .zip(&sizes)
                    .map(|(lines, size)| {
                        let text = lines.get(level).map_or("", String::as_str);
                        format!("│ {} │", pad(text, size - 4))
                    })
                    .collect(),
            ));
        }
        out.push(join(
            sizes
                .iter()
                .map(|size| format!("└{}┘", "─".repeat(size - 2)))
                .collect(),
        ));
    }
    out.push(format!("└{}┘", "─".repeat(width - 2)));
    Ok(out.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::{render, wrap, Column, Row, Wireframe};
    use runtime::ascii_art::{display_width, misaligned};

    fn column(label: &str, lines: &[&str]) -> Column {
        Column {
            label: label.to_string(),
            lines: lines.iter().map(|line| (*line).to_string()).collect(),
            weight: 1,
        }
    }

    #[test]
    fn every_line_has_the_same_width_and_every_border_joins() {
        let frame = Wireframe {
            title: "Premier écran, ordinateur".to_string(),
            width: 64,
            rows: vec![
                Row {
                    label:
                        "EN-TÊTE fixe : Gestionnaire de Budget · ← Octobre 2026 → · [Exporter CSV]"
                            .to_string(),
                    lines: Vec::new(),
                    columns: Vec::new(),
                },
                Row {
                    label: "SOLDE NET (héros)".to_string(),
                    lines: vec!["1 234,56 € en grand, barre verte à gauche".to_string()],
                    columns: Vec::new(),
                },
                Row {
                    label: "GRILLE 3 colonnes".to_string(),
                    lines: Vec::new(),
                    columns: vec![
                        column(
                            "REVENUS",
                            &[
                                "+ 2 500,00 €",
                                "[+ Ajouter]",
                                "• Salaire 2 000",
                                "• Freelance 500",
                            ],
                        ),
                        column("DÉPENSES", &["- 850,00 €", "[+ Ajouter]", "• Courses 350"]),
                        column(
                            "ÉPARGNE extrêmement longue pour tester le retour à la ligne",
                            &["+ 300,00 €"],
                        ),
                    ],
                },
            ],
        };
        let drawing = render(&frame).expect("dessin");
        let lines: Vec<&str> = drawing.lines().skip(1).collect();
        assert!(
            lines.iter().all(|line| display_width(line) == 64),
            "{drawing}"
        );
        assert!(
            misaligned(&format!("```\n{}\n```", lines.join("\n")), 5).is_empty(),
            "{drawing}"
        );
        assert!(
            drawing.contains("│ ÉPARGNE") && drawing.contains("ligne"),
            "{drawing}"
        );
        // Trop de colonnes pour la largeur : dit, sans dessin cassé.
        let crowded = Wireframe {
            title: String::new(),
            width: 30,
            rows: vec![Row {
                label: String::new(),
                lines: Vec::new(),
                columns: (0..6)
                    .map(|index| column(&index.to_string(), &[]))
                    .collect(),
            }],
        };
        assert!(render(&crowded).is_err());
        assert_eq!(wrap("un deux trois", 7), ["un deux", "trois"]);
        assert_eq!(
            wrap("anticonstitutionnellement", 10),
            ["anticonsti", "tutionnell", "ement"]
        );
    }
}
