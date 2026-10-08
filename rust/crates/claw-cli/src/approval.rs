//! Demande d'autorisation encadrée, à la manière de Claude Code : choix aux
//! flèches (ou au chiffre), Entrée valide, Échap refuse, Ctrl+C refuse et
//! interrompt le tour. L'encadré s'efface une fois la réponse donnée.

use std::io::{self, IsTerminal, Write};

use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::terminal;

use crate::transcript::{self, ACCENT, BOLD, DIM, RESET};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Answer {
    /// Option choisie (indice dans la liste).
    Pick(usize),
    /// Échap : refus.
    Escape,
    /// Ctrl+C : refus et interruption du tour.
    Interrupt,
}

/// Le menu interactif est possible (sinon : question en mode texte).
#[must_use]
pub fn interactive() -> bool {
    io::stdin().is_terminal()
        && io::stdout().is_terminal()
        && std::env::var("CLAW_PLAIN_PROMPTS").map_or(true, |value| value.trim() != "1")
}

/// Contenu de l'encadré (sans les bords), option `selected` mise en avant.
fn content(
    title: &str,
    body: &[String],
    question: &str,
    options: &[String],
    selected: usize,
    inner: usize,
) -> Vec<String> {
    let mut lines = vec![format!("{ACCENT}{BOLD}{title}{RESET}"), String::new()];
    lines.extend(body.iter().map(|line| format!("  {line}")));
    if !body.is_empty() {
        lines.push(String::new());
    }
    lines.push(question.to_string());
    for (index, option) in options.iter().enumerate() {
        let label = transcript::clip(&format!("{}. {option}", index + 1), inner.saturating_sub(2));
        lines.push(if index == selected {
            format!("{ACCENT}❯ {label}{RESET}")
        } else {
            format!("  {label}")
        });
    }
    lines
}

/// Corps limité à la hauteur du terminal (l'encadré doit tenir à l'écran).
fn fit_body(body: &[String], options: usize) -> Vec<String> {
    let height = terminal::size().map_or(24, |(_, rows)| usize::from(rows));
    let room = height.saturating_sub(options + 9).max(3);
    if body.len() <= room {
        return body.to_vec();
    }
    let mut kept = body[..room - 1].to_vec();
    kept.push(format!("{DIM}… +{} lignes{RESET}", body.len() - (room - 1)));
    kept
}

/// Largeur de l'encadré.
#[must_use]
pub fn box_width() -> usize {
    transcript::terminal_width().saturating_sub(1).min(110)
}

fn draw(out: &mut impl Write, lines: &[String], previous: usize) -> io::Result<usize> {
    if previous > 0 {
        write!(out, "\x1b[{previous}A\r\x1b[J")?;
    }
    for line in lines {
        write!(out, "{line}\r\n")?;
    }
    out.flush()?;
    Ok(lines.len())
}

/// Pose la question ; `body` : lignes déjà découpées à `box_width() - 6`.
pub fn ask(title: &str, body: &[String], question: &str, options: &[String]) -> io::Result<Answer> {
    let body = fit_body(body, options.len());
    // L'écoute du clavier pendant le tour (Échap) rend la main le temps du choix.
    let keys = crate::turn_keys::pause();
    terminal::enable_raw_mode()?;
    let answer = menu(&mut io::stdout(), title, &body, question, options);
    let _ = terminal::disable_raw_mode();
    drop(keys);
    answer
}

fn menu(
    out: &mut impl Write,
    title: &str,
    body: &[String],
    question: &str,
    options: &[String],
) -> io::Result<Answer> {
    let width = box_width();
    let inner = width.saturating_sub(4);
    let count = options.len().max(1);
    let mut selected = 0usize;
    let mut drawn = 0usize;
    let answer = loop {
        let lines = transcript::boxed(
            &content(title, body, question, options, selected, inner),
            width,
            ACCENT,
        );
        drawn = draw(out, &lines, drawn)?;
        let Event::Key(key) = event::read()? else {
            continue;
        };
        if !matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
            continue;
        }
        match key.code {
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                break Answer::Interrupt
            }
            KeyCode::Up | KeyCode::Char('k') | KeyCode::BackTab => {
                selected = (selected + count - 1) % count;
            }
            KeyCode::Down | KeyCode::Char('j') | KeyCode::Tab => {
                selected = (selected + 1) % count;
            }
            KeyCode::Enter => break Answer::Pick(selected),
            KeyCode::Esc | KeyCode::Char('n') => break Answer::Escape,
            KeyCode::Char(digit @ '1'..='9') => {
                let index = digit as usize - '1' as usize;
                if index < options.len() {
                    break Answer::Pick(index);
                }
            }
            KeyCode::Char('o' | 'y') => break Answer::Pick(0),
            _ => {}
        }
    };
    // L'encadré disparaît : seul le résumé du choix reste à l'écran.
    write!(out, "\x1b[{drawn}A\r\x1b[J")?;
    out.flush()?;
    Ok(answer)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_selected_option_carries_the_pointer() {
        let options = vec!["Oui".to_string(), "Non (Échap)".to_string()];
        let lines = content(
            "Commande shell",
            &["cls".to_string()],
            "Continuer ?",
            &options,
            1,
            40,
        );
        let plain: Vec<String> = lines
            .iter()
            .map(|line| transcript::strip_ansi(line))
            .collect();
        assert_eq!(
            plain,
            vec![
                "Commande shell",
                "",
                "  cls",
                "",
                "Continuer ?",
                "  1. Oui",
                "❯ 2. Non (Échap)"
            ]
        );
    }

    #[test]
    fn long_bodies_are_cut_to_fit_the_screen() {
        let body: Vec<String> = (0..200).map(|n| n.to_string()).collect();
        let kept = fit_body(&body, 3);
        assert!(kept.len() < body.len());
        assert!(transcript::strip_ansi(kept.last().unwrap()).starts_with("… +"));
    }
}
