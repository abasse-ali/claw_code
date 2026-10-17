//! Rendu plein écran (comme `/tui fullscreen` de Claude Code) : la session
//! s'affiche sur l'écran alternatif du terminal, comme `vim` ou `htop`. Le
//! terminal n'est redessiné que là où il change (pas de scintillement, même
//! dans le terminal de VS Code ou tmux), la conversation défile dans Claw
//! (PgUp/PgDn, molette, Ctrl+Origine/Fin) sans déplacer la saisie, et la
//! mémoire reste bornée.
//!
//! La session tourne dans un pseudo-terminal (`ConPTY` sous Windows) dont un
//! émulateur VT garde l'écran et l'historique ; ce processus en dessine la
//! partie visible et lui passe le clavier.

use std::fmt::Write as _;
use std::io::{self, IsTerminal, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

use crossterm::event::{
    self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind,
};
use crossterm::{execute, terminal};
use portable_pty::{native_pty_system, CommandBuilder, PtySize};

/// Code de sortie de la session quand l'utilisateur repasse au rendu classique.
pub(crate) const RELAUNCH_CLASSIC: u8 = 75;
/// Lignes d'historique gardées (la mémoire reste bornée).
const SCROLLBACK: usize = 20_000;

fn flag(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_lowercase())
        .filter(|value| !value.is_empty())
}

/// Réglage `tui` de `~/.claw/settings.json` (`fullscreen` ou `default`).
pub(crate) fn tui_setting() -> Option<String> {
    let path = runtime::ConfigLoader::default_for(".")
        .config_home()
        .join("settings.json");
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .and_then(|settings| settings.get("tui")?.as_str().map(str::to_string))
}

/// Cette session tourne déjà dans le rendu plein écran.
pub(crate) fn inside() -> bool {
    std::env::var_os("CLAW_FULLSCREEN_CHILD").is_some()
}

/// Démarrer en plein écran : `CLAW_NO_FLICKER=1` (ou `CLAUDE_CODE_NO_FLICKER`),
/// sinon le réglage `"tui": "fullscreen"`. `CLAW_DISABLE_ALTERNATE_SCREEN=1`
/// (ou celui de Claude Code) et `NO_FLICKER=0` gardent le rendu classique,
/// comme une entrée ou une sortie qui n'est pas un terminal.
pub(crate) fn wanted() -> bool {
    if inside() || !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return false;
    }
    let disabled = [
        "CLAW_DISABLE_ALTERNATE_SCREEN",
        "CLAUDE_CODE_DISABLE_ALTERNATE_SCREEN",
    ]
    .iter()
    .any(|name| matches!(flag(name).as_deref(), Some("1" | "true")));
    if disabled || flag("TERM").as_deref() == Some("dumb") {
        return false;
    }
    for name in ["CLAW_NO_FLICKER", "CLAUDE_CODE_NO_FLICKER"] {
        match flag(name).as_deref() {
            Some("1" | "true") => return true,
            Some("0" | "false") => return false,
            _ => {}
        }
    }
    tui_setting().as_deref() == Some("fullscreen")
}

/// Distance d'un cran de molette (`CLAW_SCROLL_SPEED`, comme
/// `CLAUDE_CODE_SCROLL_SPEED`), 3 lignes par défaut.
fn scroll_step() -> usize {
    ["CLAW_SCROLL_SPEED", "CLAUDE_CODE_SCROLL_SPEED"]
        .iter()
        .find_map(|name| flag(name)?.parse::<f64>().ok())
        .filter(|speed| *speed > 0.0)
        .map_or(3, |speed| {
            // Arrondi vers le haut, au moins une ligne, vingt au plus.
            let lines = (speed.min(20.0) * 3.0).ceil();
            format!("{lines:.0}").parse::<usize>().unwrap_or(3).max(1)
        })
}

/// Touche du clavier en séquence pour le terminal de la session ; les flèches
/// suivent le mode « curseur d'application » demandé par la session.
fn encode_key(key: &KeyEvent, application_cursor: bool) -> Option<Vec<u8>> {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let alt = key.modifiers.contains(KeyModifiers::ALT);
    let shift = key.modifiers.contains(KeyModifiers::SHIFT);
    let modifier = 1 + u8::from(shift) + 2 * u8::from(alt) + 4 * u8::from(ctrl);
    let special = |final_byte: char| -> Vec<u8> {
        if modifier > 1 {
            format!("\x1b[1;{modifier}{final_byte}").into_bytes()
        } else if application_cursor && matches!(final_byte, 'A' | 'B' | 'C' | 'D') {
            format!("\x1bO{final_byte}").into_bytes()
        } else {
            format!("\x1b[{final_byte}").into_bytes()
        }
    };
    let tilde = |number: u8| -> Vec<u8> {
        if modifier > 1 {
            format!("\x1b[{number};{modifier}~").into_bytes()
        } else {
            format!("\x1b[{number}~").into_bytes()
        }
    };
    let mut bytes = match key.code {
        KeyCode::Char(character) if ctrl => {
            let lower = character.to_ascii_lowercase();
            match lower {
                'a'..='z' => vec![u8::try_from(lower).ok()? - b'a' + 1],
                ' ' | '@' | '2' => vec![0],
                '[' | '3' => vec![27],
                '\\' | '4' => vec![28],
                ']' | '5' => vec![29],
                '^' | '6' => vec![30],
                '_' | '7' | '-' => vec![31],
                _ => character.to_string().into_bytes(),
            }
        }
        KeyCode::Char(character) => character.to_string().into_bytes(),
        // Maj+Entrée : saut de ligne (Ctrl+J dans l'éditeur de Claw).
        KeyCode::Enter if shift || ctrl => vec![b'\n'],
        KeyCode::Enter => vec![b'\r'],
        KeyCode::Backspace if ctrl => vec![0x08],
        KeyCode::Backspace => vec![0x7f],
        KeyCode::Tab => vec![b'\t'],
        KeyCode::BackTab => b"\x1b[Z".to_vec(),
        KeyCode::Esc => vec![0x1b],
        KeyCode::Up => special('A'),
        KeyCode::Down => special('B'),
        KeyCode::Right => special('C'),
        KeyCode::Left => special('D'),
        KeyCode::Home => special('H'),
        KeyCode::End => special('F'),
        KeyCode::Insert => tilde(2),
        KeyCode::Delete => tilde(3),
        KeyCode::PageUp => tilde(5),
        KeyCode::PageDown => tilde(6),
        KeyCode::F(number @ 1..=4) => {
            format!("\x1bO{}", char::from(b'P' + number - 1)).into_bytes()
        }
        KeyCode::F(number) => {
            let code = match number {
                5 => 15,
                6 => 17,
                7 => 18,
                8 => 19,
                9 => 20,
                10 => 21,
                11 => 23,
                12 => 24,
                _ => return None,
            };
            tilde(code)
        }
        _ => return None,
    };
    if alt && matches!(key.code, KeyCode::Char(_) | KeyCode::Backspace) {
        bytes.insert(0, 0x1b);
    }
    Some(bytes)
}

/// Réponses du terminal aux demandes de la session : position du curseur
/// (`ESC [ 6 n`, attendue par `ConPTY` au démarrage) et identité (`ESC [ c`).
fn terminal_replies(chunk: &[u8], screen: &vt100::Screen) -> Vec<u8> {
    let mut replies = Vec::new();
    let mut index = 0;
    while let Some(found) = chunk[index..].windows(2).position(|pair| pair == b"\x1b[") {
        let start = index + found + 2;
        let rest = &chunk[start..];
        if rest.starts_with(b"6n") {
            let (row, col) = screen.cursor_position();
            replies.extend(format!("\x1b[{};{}R", row + 1, col + 1).into_bytes());
        } else if rest.starts_with(b"c") || rest.starts_with(b"0c") {
            replies.extend_from_slice(b"\x1b[?1;2c");
        }
        index = start;
    }
    replies
}

/// Lignes d'historique au-dessus de l'écran.
fn history_len(parser: &mut vt100::Parser) -> usize {
    let current = parser.screen().scrollback();
    parser.set_scrollback(usize::MAX);
    let total = parser.screen().scrollback();
    parser.set_scrollback(current);
    total
}

/// Barre du bas pendant qu'on relit l'historique.
fn scroll_bar(rows: u16, cols: u16, unseen: usize) -> Vec<u8> {
    let mut text = String::from(" ↓ Aller en bas : Fin, molette ou clic ici");
    if unseen > 0 {
        let _ = write!(text, " · {unseen} nouvelle(s) ligne(s)");
    }
    text.push_str(" · PgUp/PgDn défilent ");
    let width = usize::from(cols);
    let mut shown: String = text.chars().take(width).collect();
    let count = shown.chars().count();
    if count < width {
        shown.push_str(&" ".repeat(width - count));
    }
    format!("\x1b[{};1H\x1b[0;7m{shown}\x1b[0m\x1b[?25l", rows.max(1)).into_bytes()
}

/// Sous Windows, la console doit interpréter les séquences VT (écran
/// alternatif, couleurs, curseur) : le mode est activé, ou le plein écran
/// n'est pas possible.
#[cfg(windows)]
fn enable_vt() -> io::Result<()> {
    use crossterm_winapi::{ConsoleMode, Handle};
    const ENABLE_VIRTUAL_TERMINAL_PROCESSING: u32 = 0x0004;
    let mode = ConsoleMode::from(Handle::current_out_handle()?);
    let current = mode.mode()?;
    mode.set_mode(current | ENABLE_VIRTUAL_TERMINAL_PROCESSING)
}

#[cfg(not(windows))]
#[allow(clippy::unnecessary_wraps)]
fn enable_vt() -> io::Result<()> {
    Ok(())
}

struct Terminal;

impl Terminal {
    fn enter(mouse: bool) -> io::Result<Self> {
        enable_vt()?;
        terminal::enable_raw_mode()?;
        let mut out = io::stdout();
        // Écran alternatif en VT (l'API de la console Windows changerait de
        // tampon sans y rediriger la sortie).
        out.write_all(b"\x1b[?1049h\x1b[H\x1b[2J")?;
        out.flush()?;
        execute!(out, event::EnableBracketedPaste)?;
        if mouse {
            execute!(out, event::EnableMouseCapture)?;
        }
        Ok(Self)
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        let mut out = io::stdout();
        let _ = execute!(
            out,
            event::DisableMouseCapture,
            event::DisableBracketedPaste
        );
        let _ = out.write_all(b"\x1b[0m\x1b[?1049l\x1b[?25h");
        let _ = out.flush();
        let _ = terminal::disable_raw_mode();
    }
}

/// Ce que la vue montre : la fin (suivie) ou l'historique, `offset` lignes
/// plus haut.
struct View {
    parser: vt100::Parser,
    offset: usize,
    unseen: usize,
    shown: Option<vt100::Screen>,
    rows: u16,
    cols: u16,
}

impl View {
    fn render(&mut self, out: &mut impl Write) -> io::Result<()> {
        // Sortie synchronisée : le terminal affiche l'image d'un coup.
        let mut frame = b"\x1b[?2026h".to_vec();
        if self.offset == 0 {
            self.parser.set_scrollback(0);
            let screen = self.parser.screen().clone();
            if let Some(previous) = &self.shown {
                frame.extend(screen.state_diff(previous));
            } else {
                frame.extend_from_slice(b"\x1b[0m\x1b[H\x1b[2J");
                frame.extend(screen.state_formatted());
            }
            frame.extend(screen.cursor_state_formatted());
            self.shown = Some(screen);
        } else {
            self.parser.set_scrollback(self.offset);
            let screen = self.parser.screen();
            frame.extend_from_slice(b"\x1b[0m\x1b[H\x1b[2J");
            frame.extend(screen.contents_formatted());
            frame.extend(scroll_bar(self.rows, self.cols, self.unseen));
            // L'image suivante, revenue en bas, repartira de zéro.
            self.shown = None;
            self.parser.set_scrollback(0);
        }
        frame.extend_from_slice(b"\x1b[?2026l");
        out.write_all(&frame)?;
        out.flush()
    }

    fn scroll_by(&mut self, lines: isize) {
        let total = history_len(&mut self.parser);
        let offset = if lines >= 0 {
            self.offset.saturating_add(lines.unsigned_abs())
        } else {
            self.offset.saturating_sub(lines.unsigned_abs())
        };
        self.offset = offset.min(total);
        if self.offset == 0 {
            self.unseen = 0;
        }
    }

    fn follow(&mut self) {
        self.offset = 0;
        self.unseen = 0;
    }
}

/// Lance la session dans le rendu plein écran, avec `args` (ceux de la ligne
/// de commande) et la conversation `resume` le cas échéant. Rend la
/// conversation à reprendre en rendu classique si l'utilisateur y repasse
/// (`/tui default`), `None` à la fin de la session.
#[allow(clippy::too_many_lines)]
pub(crate) fn run(args: &[String], resume: Option<&Path>) -> io::Result<Option<PathBuf>> {
    let (cols, rows) = terminal::size()?;
    let pty = native_pty_system()
        .openpty(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|error| io::Error::other(error.to_string()))?;
    let handoff = std::env::temp_dir().join(format!("claw-tui-{}.txt", std::process::id()));
    let _ = std::fs::remove_file(&handoff);
    let mut command = CommandBuilder::new(std::env::current_exe()?);
    command.args(args);
    command.cwd(std::env::current_dir()?);
    command.env("CLAW_FULLSCREEN_CHILD", "1");
    command.env("CLAW_TUI_HANDOFF", &handoff);
    if let Some(resume) = resume {
        command.env("CLAW_TUI_RESUME", resume);
    }
    let mut child = pty
        .slave
        .spawn_command(command)
        .map_err(|error| io::Error::other(error.to_string()))?;
    drop(pty.slave);
    let mut reader = pty
        .master
        .try_clone_reader()
        .map_err(|error| io::Error::other(error.to_string()))?;
    let mut writer = pty
        .master
        .take_writer()
        .map_err(|error| io::Error::other(error.to_string()))?;
    let (sender, output) = mpsc::channel::<Vec<u8>>();
    std::thread::Builder::new()
        .name("claw-fullscreen-output".to_string())
        .spawn(move || {
            let mut buffer = vec![0_u8; 64 * 1024];
            loop {
                match reader.read(&mut buffer) {
                    Ok(0) | Err(_) => break,
                    Ok(read) => {
                        if sender.send(buffer[..read].to_vec()).is_err() {
                            break;
                        }
                    }
                }
            }
        })?;

    let mouse = flag("CLAW_FULLSCREEN_MOUSE").as_deref() != Some("0");
    let guard = Terminal::enter(mouse)?;
    let mut out = io::stdout();
    let mut view = View {
        parser: vt100::Parser::new(rows, cols, SCROLLBACK),
        offset: 0,
        unseen: 0,
        shown: None,
        rows,
        cols,
    };
    let step = scroll_step();
    let mut finished = false;
    let mut exit_status = None;
    let mut closing: Option<std::time::Instant> = None;
    loop {
        let mut dirty = false;
        loop {
            match output.try_recv() {
                Ok(chunk) => {
                    let before = history_len(&mut view.parser);
                    view.parser.process(&chunk);
                    // Ce processus est le terminal de la session : il répond
                    // à ses questions (ConPTY attend la position du curseur).
                    let replies = terminal_replies(&chunk, view.parser.screen());
                    if !replies.is_empty() {
                        writer.write_all(&replies)?;
                        writer.flush()?;
                    }
                    if view.offset > 0 {
                        // La vue reste sur les mêmes lignes pendant que la
                        // session écrit plus bas.
                        let added = history_len(&mut view.parser).saturating_sub(before);
                        view.offset += added;
                        view.unseen += added;
                    }
                    dirty = true;
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    finished = true;
                    break;
                }
            }
        }
        if dirty {
            view.render(&mut out)?;
        }
        if finished {
            break;
        }
        // La session s'est arrêtée : sous Windows, la sortie de ConPTY ne se
        // ferme pas d'elle-même ; ce qui restait à afficher arrive vite.
        if exit_status.is_none() {
            if let Ok(Some(status)) = child.try_wait() {
                exit_status = Some(status);
                closing = Some(std::time::Instant::now());
            }
        }
        if closing.is_some_and(|since| since.elapsed() >= Duration::from_millis(200)) {
            break;
        }
        if !event::poll(Duration::from_millis(15))? {
            continue;
        }
        let application_cursor = view.parser.screen().application_cursor();
        match event::read()? {
            Event::Key(key) if matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) => {
                let half = isize::try_from(view.rows / 2).unwrap_or(1).max(1);
                match key.code {
                    KeyCode::PageUp => view.scroll_by(half),
                    KeyCode::PageDown => view.scroll_by(-half),
                    KeyCode::Home if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        view.scroll_by(isize::MAX);
                    }
                    KeyCode::End if key.modifiers.contains(KeyModifiers::CONTROL) => view.follow(),
                    KeyCode::End if view.offset > 0 => view.follow(),
                    _ => {
                        if let Some(bytes) = encode_key(&key, application_cursor) {
                            // Taper ramène à la saisie, comme dans un terminal.
                            view.follow();
                            writer.write_all(&bytes)?;
                            writer.flush()?;
                        }
                    }
                }
                view.render(&mut out)?;
            }
            Event::Paste(text) => {
                view.follow();
                let mut bytes = Vec::new();
                if view.parser.screen().bracketed_paste() {
                    bytes.extend_from_slice(b"\x1b[200~");
                    bytes.extend_from_slice(text.as_bytes());
                    bytes.extend_from_slice(b"\x1b[201~");
                } else {
                    bytes.extend_from_slice(text.replace('\n', "\r").as_bytes());
                }
                writer.write_all(&bytes)?;
                writer.flush()?;
                view.render(&mut out)?;
            }
            Event::Mouse(mouse) => {
                let step = isize::try_from(step).unwrap_or(3);
                match mouse.kind {
                    MouseEventKind::ScrollUp => view.scroll_by(step),
                    MouseEventKind::ScrollDown => view.scroll_by(-step),
                    MouseEventKind::Down(MouseButton::Left)
                        if view.offset > 0 && mouse.row + 1 >= view.rows =>
                    {
                        view.follow();
                    }
                    _ => continue,
                }
                view.render(&mut out)?;
            }
            Event::Resize(cols, rows) => {
                let _ = pty.master.resize(PtySize {
                    rows,
                    cols,
                    pixel_width: 0,
                    pixel_height: 0,
                });
                view.parser.set_size(rows, cols);
                view.rows = rows;
                view.cols = cols;
                view.shown = None;
                view.render(&mut out)?;
            }
            _ => {}
        }
    }
    let status = exit_status.or_else(|| child.wait().ok());
    // Ferme le pseudo-terminal (et la lecture de sa sortie).
    drop(writer);
    drop(pty.master);
    drop(guard);
    // Ce que l'écran montrait reste lisible après la sortie.
    view.parser.set_scrollback(0);
    let last = view.parser.screen().contents();
    let last = last.trim_end();
    if !last.is_empty() {
        println!("{last}");
    }
    let relaunch = status.is_some_and(|status| status.exit_code() == u32::from(RELAUNCH_CLASSIC));
    let session = std::fs::read_to_string(&handoff)
        .ok()
        .map(|text| PathBuf::from(text.trim()))
        .filter(|path| path.is_file());
    let _ = std::fs::remove_file(&handoff);
    Ok(if relaunch { session } else { None })
}

/// Fin de la session dans le rendu plein écran pour repasser au rendu
/// classique : la conversation à reprendre est confiée au processus parent.
pub(crate) fn hand_off(session: &Path) -> bool {
    match std::env::var_os("CLAW_TUI_HANDOFF") {
        Some(path) => std::fs::write(path, session.display().to_string()).is_ok(),
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::{encode_key, scroll_bar};
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    #[test]
    fn keys_become_terminal_sequences() {
        let key = |code, modifiers| KeyEvent::new(code, modifiers);
        assert_eq!(
            encode_key(&key(KeyCode::Char('c'), KeyModifiers::CONTROL), false),
            Some(vec![3])
        );
        assert_eq!(
            encode_key(&key(KeyCode::Up, KeyModifiers::NONE), false),
            Some(b"\x1b[A".to_vec())
        );
        assert_eq!(
            encode_key(&key(KeyCode::Up, KeyModifiers::NONE), true),
            Some(b"\x1bOA".to_vec())
        );
        assert_eq!(
            encode_key(&key(KeyCode::Left, KeyModifiers::CONTROL), false),
            Some(b"\x1b[1;5D".to_vec())
        );
        assert_eq!(
            encode_key(&key(KeyCode::Enter, KeyModifiers::SHIFT), false),
            Some(vec![b'\n'])
        );
        assert_eq!(
            encode_key(&key(KeyCode::Char('b'), KeyModifiers::ALT), false),
            Some(b"\x1bb".to_vec())
        );
        assert_eq!(
            encode_key(&key(KeyCode::BackTab, KeyModifiers::SHIFT), false),
            Some(b"\x1b[Z".to_vec())
        );
        assert!(String::from_utf8_lossy(&scroll_bar(10, 30, 4)).contains("Aller en bas"));
        let mut parser = vt100::Parser::new(5, 20, 0);
        parser.process(b"ab");
        assert_eq!(
            super::terminal_replies(b"x\x1b[6ny\x1b[c", parser.screen()),
            b"\x1b[1;3R\x1b[?1;2c".to_vec()
        );
    }
}
