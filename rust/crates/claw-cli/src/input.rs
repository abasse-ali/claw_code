use std::borrow::Cow;
use std::collections::VecDeque;
use std::io::{self, IsTerminal, Write};
use std::time::{Duration, Instant};

use crossterm::cursor::{MoveToColumn, MoveUp};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use crossterm::queue;
use crossterm::terminal::{self, Clear, ClearType};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadOutcome {
    Submit(String),
    Cancel,
    Exit,
    /// Maj+Tab : changer de mode de permission, en gardant le texte saisi.
    CycleMode(String),
    /// Ctrl+O : sorties et raisonnement complets du dernier tour (texte gardé).
    ShowDetails(String),
    /// Échap deux fois, saisie vide : revenir avant un message précédent.
    Rewind,
}

/// Un collage plus long que ceci est remplacé par « [Texte collé n°1 : 40 lignes] ».
const PASTE_MAX_LINES: usize = 6;
const PASTE_MAX_CHARS: usize = 1_200;
/// Lignes du menu des commandes affichées sous la saisie.
const MENU_ROWS: usize = 6;
/// Délai maximal entre deux Échap pour « Échap Échap ».
const DOUBLE_ESCAPE: Duration = Duration::from_millis(800);
const ACCENT: &str = "\x1b[38;2;215;119;87m";

/// Cadre de saisie à la Claude Code : un trait au-dessus et au-dessous de la
/// zone de saisie, puis une ligne d'état (mode, modèle…).
#[derive(Debug, Clone, PartialEq, Eq)]
struct Frame {
    width: usize,
    status: String,
    /// Lignes affichées à la place de la ligne d'état (menu, avis).
    below: Vec<String>,
}

impl Frame {
    fn rule(&self) -> String {
        format!("\x1b[38;5;240m{}\x1b[0m", "─".repeat(self.width))
    }

    fn below_lines(&self) -> Vec<String> {
        if self.below.is_empty() {
            vec![format!("\x1b[2m{}\x1b[0m", clip(&self.status, self.width))]
        } else {
            self.below.clone()
        }
    }
}

/// Coupe un texte sans couleurs à `limit` caractères.
fn clip(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_string();
    }
    let kept: String = text.chars().take(limit.saturating_sub(1)).collect();
    format!("{kept}…")
}

fn terminal_columns() -> usize {
    terminal::size().map_or(80, |(columns, _)| usize::from(columns).max(20))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EditorMode {
    Plain,
    Insert,
    Normal,
    Visual,
    Command,
}

impl EditorMode {
    fn indicator(self, vim_enabled: bool) -> Option<&'static str> {
        if !vim_enabled {
            return None;
        }

        Some(match self {
            Self::Plain => "PLAIN",
            Self::Insert => "INSERT",
            Self::Normal => "NORMAL",
            Self::Visual => "VISUAL",
            Self::Command => "COMMAND",
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct YankBuffer {
    text: String,
    linewise: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct EditSession {
    text: String,
    cursor: usize,
    mode: EditorMode,
    pending_operator: Option<char>,
    visual_anchor: Option<usize>,
    command_buffer: String,
    command_cursor: usize,
    history_index: Option<usize>,
    history_backup: Option<String>,
    rendered_cursor_row: usize,
    rendered_lines: usize,
    /// Le dernier rendu avait un cadre (trait au-dessus de la saisie).
    framed: bool,
    /// Ligne choisie dans le menu des commandes.
    menu_index: usize,
    /// Menu fermé avec Échap (jusqu'à la prochaine modification du texte).
    menu_hidden: bool,
    last_escape: Option<Instant>,
    /// Avis affiché sous la saisie jusqu'à la touche suivante.
    notice: Option<String>,
}

impl EditSession {
    fn new(vim_enabled: bool) -> Self {
        Self {
            text: String::new(),
            cursor: 0,
            mode: if vim_enabled {
                EditorMode::Insert
            } else {
                EditorMode::Plain
            },
            pending_operator: None,
            visual_anchor: None,
            command_buffer: String::new(),
            command_cursor: 0,
            history_index: None,
            history_backup: None,
            rendered_cursor_row: 0,
            rendered_lines: 1,
            framed: false,
            menu_index: 0,
            menu_hidden: false,
            last_escape: None,
            notice: None,
        }
    }

    fn active_text(&self) -> &str {
        if self.mode == EditorMode::Command {
            &self.command_buffer
        } else {
            &self.text
        }
    }

    fn current_len(&self) -> usize {
        self.active_text().len()
    }

    fn has_input(&self) -> bool {
        !self.active_text().is_empty()
    }

    fn current_line(&self) -> String {
        self.active_text().to_string()
    }

    fn set_text_from_history(&mut self, entry: String) {
        self.text = entry;
        self.cursor = self.text.len();
        self.pending_operator = None;
        self.visual_anchor = None;
        if self.mode != EditorMode::Plain && self.mode != EditorMode::Insert {
            self.mode = EditorMode::Normal;
        }
    }

    fn enter_insert_mode(&mut self) {
        self.mode = EditorMode::Insert;
        self.pending_operator = None;
        self.visual_anchor = None;
    }

    fn enter_normal_mode(&mut self) {
        self.mode = EditorMode::Normal;
        self.pending_operator = None;
        self.visual_anchor = None;
    }

    fn enter_visual_mode(&mut self) {
        self.mode = EditorMode::Visual;
        self.pending_operator = None;
        self.visual_anchor = Some(self.cursor);
    }

    fn enter_command_mode(&mut self) {
        self.mode = EditorMode::Command;
        self.pending_operator = None;
        self.visual_anchor = None;
        self.command_buffer.clear();
        self.command_buffer.push(':');
        self.command_cursor = self.command_buffer.len();
    }

    fn exit_command_mode(&mut self) {
        self.command_buffer.clear();
        self.command_cursor = 0;
        self.enter_normal_mode();
    }

    fn visible_buffer(&self) -> Cow<'_, str> {
        if self.mode != EditorMode::Visual {
            return Cow::Borrowed(self.active_text());
        }

        let Some(anchor) = self.visual_anchor else {
            return Cow::Borrowed(self.active_text());
        };
        let Some((start, end)) = selection_bounds(&self.text, anchor, self.cursor) else {
            return Cow::Borrowed(self.active_text());
        };

        Cow::Owned(render_selected_text(&self.text, start, end))
    }

    fn prompt<'a>(&self, base_prompt: &'a str, vim_enabled: bool) -> Cow<'a, str> {
        match self.mode.indicator(vim_enabled) {
            Some(mode) => Cow::Owned(format!("[{mode}] {base_prompt}")),
            None => Cow::Borrowed(base_prompt),
        }
    }

    fn clear_render(&self, out: &mut impl Write) -> io::Result<()> {
        let rows_up = self.rendered_cursor_row + usize::from(self.framed);
        if rows_up > 0 {
            queue!(out, MoveUp(to_u16(rows_up)?))?;
        }
        queue!(out, MoveToColumn(0), Clear(ClearType::FromCursorDown))?;
        out.flush()
    }

    fn render(
        &mut self,
        out: &mut impl Write,
        base_prompt: &str,
        vim_enabled: bool,
        frame: Option<&Frame>,
        columns: usize,
    ) -> io::Result<()> {
        self.clear_render(out)?;

        if let Some(frame) = frame {
            write!(out, "{}\r\n", frame.rule())?;
        }
        let prompt = self.prompt(base_prompt, vim_enabled);
        let buffer = self.visible_buffer();
        write!(out, "{prompt}{}", buffer.replace('\n', "\r\n"))?;

        let (cursor_row, cursor_col, total_lines) = self.cursor_layout(prompt.as_ref(), columns);
        let mut rows_to_move_up = total_lines.saturating_sub(cursor_row + 1);
        if let Some(frame) = frame {
            // Trait du bas puis ligne d'état (ou menu) sous la saisie, puis
            // retour au curseur.
            let below = frame.below_lines();
            write!(out, "\r\n{}", frame.rule())?;
            for line in &below {
                write!(out, "\r\n{line}")?;
            }
            rows_to_move_up += 1 + below.len();
        }
        if rows_to_move_up > 0 {
            queue!(out, MoveUp(to_u16(rows_to_move_up)?))?;
        }
        queue!(out, MoveToColumn(to_u16(cursor_col)?))?;
        out.flush()?;

        self.rendered_cursor_row = cursor_row;
        self.rendered_lines = total_lines;
        self.framed = frame.is_some();
        Ok(())
    }

    /// Après validation : le cadre disparaît, le message reste en gris (« > … »).
    fn finalize_render(
        &self,
        out: &mut impl Write,
        base_prompt: &str,
        vim_enabled: bool,
    ) -> io::Result<()> {
        self.clear_render(out)?;
        let prompt = self.prompt(base_prompt, vim_enabled);
        let buffer = self.visible_buffer();
        let buffer = buffer.replace('\n', "\r\n");
        if self.framed {
            write!(out, "\x1b[38;5;245m{prompt}{buffer}\x1b[0m")?;
        } else {
            write!(out, "{prompt}{buffer}")?;
        }
        write!(out, "\r\n")?;
        out.flush()
    }

    /// Rangée et colonne du curseur, nombre de rangées occupées, en tenant
    /// compte des lignes plus longues que le terminal (retour à la ligne).
    fn cursor_layout(&self, prompt: &str, columns: usize) -> (usize, usize, usize) {
        let columns = columns.max(1);
        let active_text = self.active_text();
        let cursor = if self.mode == EditorMode::Command {
            self.command_cursor
        } else {
            self.cursor
        };
        let prompt_width = prompt.chars().count();
        let cursor_line = active_text[..cursor].matches('\n').count();
        let line_start = active_text[..cursor]
            .rfind('\n')
            .map_or(0, |index| index + 1);

        let mut rows_before = 0;
        let mut total_rows = 0;
        let mut cursor_line_width = 0;
        for (index, line) in active_text.split('\n').enumerate() {
            let width = line.chars().count() + if index == 0 { prompt_width } else { 0 };
            let rows = width.div_ceil(columns).max(1);
            if index < cursor_line {
                rows_before += rows;
            }
            if index == cursor_line {
                cursor_line_width = width;
            }
            total_rows += rows;
        }
        let offset = active_text[line_start..cursor].chars().count()
            + if cursor_line == 0 { prompt_width } else { 0 };
        let (mut row, mut col) = (rows_before + offset / columns, offset % columns);
        // Juste après le dernier caractère d'une ligne qui remplit sa rangée :
        // le terminal garde le curseur au bout de cette rangée.
        if col == 0 && offset > 0 && offset == cursor_line_width {
            row -= 1;
            col = columns - 1;
        }
        (row, col, total_rows)
    }
}

enum KeyAction {
    Continue,
    Submit(String),
    Cancel,
    Exit,
    ToggleVim,
    CycleMode,
    ShowDetails,
    Rewind,
}

pub struct LineEditor {
    prompt: String,
    completions: Vec<String>,
    history: Vec<String>,
    yank_buffer: YankBuffer,
    vim_enabled: bool,
    completion_state: Option<CompletionState>,
    /// Texte placé dans la prochaine saisie (tapé pendant le tour précédent).
    initial_text: Option<String>,
    /// Ligne d'état sous la saisie ; `None` : saisie sans cadre.
    status: Option<String>,
    /// Commandes slash et leur description, pour le menu sous la saisie.
    commands: Vec<(String, String)>,
    /// Collages longs et images remplacés dans la saisie par un repère
    /// (« [Image n°1] »), remis à leur place à l'envoi.
    attachments: Vec<(String, String)>,
    paste_count: usize,
    image_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CompletionState {
    prefix: String,
    matches: Vec<String>,
    next_index: usize,
}

impl LineEditor {
    #[must_use]
    pub fn new(prompt: impl Into<String>, completions: Vec<String>) -> Self {
        Self {
            prompt: prompt.into(),
            completions,
            history: Vec::new(),
            yank_buffer: YankBuffer::default(),
            vim_enabled: false,
            completion_state: None,
            initial_text: None,
            status: None,
            commands: Vec::new(),
            attachments: Vec::new(),
            paste_count: 0,
            image_count: 0,
        }
    }

    /// Encadre la saisie (traits) avec cette ligne d'état dessous.
    pub fn set_status(&mut self, status: impl Into<String>) {
        self.status = Some(status.into());
    }

    /// Commandes proposées dans le menu qui s'ouvre quand on tape « / ».
    pub fn set_command_help(&mut self, commands: Vec<(String, String)>) {
        self.commands = commands;
    }

    fn draw(&self, session: &mut EditSession, out: &mut impl Write) -> io::Result<()> {
        let columns = terminal_columns();
        let frame = self.status.as_ref().map(|status| Frame {
            width: columns.saturating_sub(1).max(20),
            status: status.clone(),
            below: self.below_lines(session, columns.saturating_sub(1).max(20)),
        });
        session.render(out, &self.prompt, self.vim_enabled, frame.as_ref(), columns)
    }

    /// Avis, ou menu des commandes qui commencent par le texte saisi.
    fn below_lines(&self, session: &EditSession, width: usize) -> Vec<String> {
        if let Some(notice) = &session.notice {
            return vec![format!(
                "\x1b[38;5;203m  {}\x1b[0m",
                clip(notice, width.saturating_sub(2))
            )];
        }
        let matches = self.menu_matches(session);
        if matches.is_empty() {
            return Vec::new();
        }
        let selected = session.menu_index.min(matches.len() - 1);
        let start = (selected + 1).saturating_sub(MENU_ROWS);
        let window = &matches[start..(start + MENU_ROWS).min(matches.len())];
        let name_width = window
            .iter()
            .map(|index| self.commands[*index].0.chars().count())
            .max()
            .unwrap_or(0)
            .min(24);
        let mut lines = Vec::new();
        for (offset, index) in window.iter().enumerate() {
            let (name, summary) = &self.commands[*index];
            let head = format!("  {name:<name_width$}  ");
            let summary = clip(summary, width.saturating_sub(head.chars().count()));
            lines.push(if start + offset == selected {
                format!("{ACCENT}{head}{summary}\x1b[0m")
            } else {
                format!("{head}\x1b[2m{summary}\x1b[0m")
            });
        }
        if matches.len() > MENU_ROWS {
            lines.push(format!(
                "\x1b[2m  {}/{} · ↑↓ pour choisir · Tab complète · Entrée lance\x1b[0m",
                selected + 1,
                matches.len()
            ));
        }
        lines
    }

    /// Commandes dont le nom commence par le texte saisi (« /mo » : /model…).
    fn menu_matches(&self, session: &EditSession) -> Vec<usize> {
        let text = &session.text;
        if self.commands.is_empty()
            || session.menu_hidden
            || !matches!(session.mode, EditorMode::Plain | EditorMode::Insert)
            || !text.starts_with('/')
            || text.contains(char::is_whitespace)
            || session.cursor != text.len()
        {
            return Vec::new();
        }
        self.commands
            .iter()
            .enumerate()
            .filter(|(_, (name, _))| name.starts_with(text.as_str()))
            .map(|(index, _)| index)
            .collect()
    }

    /// Touches du menu : flèches, Tab, Entrée, Échap. `None` : touche ordinaire.
    fn handle_menu_key(&mut self, session: &mut EditSession, key: KeyEvent) -> Option<KeyAction> {
        let matches = self.menu_matches(session);
        if matches.is_empty()
            || key
                .modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
        {
            return None;
        }
        let count = matches.len();
        let selected = session.menu_index.min(count - 1);
        let name = self.commands[matches[selected]].0.clone();
        match key.code {
            KeyCode::Up => session.menu_index = (selected + count - 1) % count,
            KeyCode::Down => session.menu_index = (selected + 1) % count,
            KeyCode::Tab => {
                session.text = format!("{name} ");
                session.cursor = session.text.len();
            }
            KeyCode::Enter if !key.modifiers.contains(KeyModifiers::SHIFT) => {
                session.text = name;
                session.cursor = session.text.len();
                return Some(Self::submit_or_toggle(session));
            }
            KeyCode::Esc => session.menu_hidden = true,
            _ => return None,
        }
        Some(KeyAction::Continue)
    }

    /// Un long collage devient un repère dans la saisie ; le texte complet
    /// est remis à l'envoi.
    fn collapse_paste(&mut self, session: &mut EditSession, start: usize) {
        let end = session.cursor;
        if session.mode == EditorMode::Command || start >= end || end > session.text.len() {
            return;
        }
        let pasted = session.text[start..end].to_string();
        let lines = pasted.lines().count();
        if lines <= PASTE_MAX_LINES && pasted.chars().count() <= PASTE_MAX_CHARS {
            return;
        }
        self.paste_count += 1;
        let placeholder = format!("[Texte collé n°{} : {lines} lignes]", self.paste_count);
        session.text.replace_range(start..end, &placeholder);
        session.cursor = start + placeholder.len();
        self.attachments.push((placeholder, pasted));
    }

    /// Alt+V : image du presse-papiers jointe à la demande (« [Image n°1] »).
    fn paste_image(&mut self, session: &mut EditSession) {
        match crate::clipboard::save_image() {
            Ok(path) => {
                self.image_count += 1;
                let placeholder = format!("[Image n°{}]", self.image_count);
                let after_word = session.text[..session.cursor]
                    .chars()
                    .next_back()
                    .is_some_and(|ch| !ch.is_whitespace());
                let insertion = if after_word {
                    format!(" {placeholder} ")
                } else {
                    format!("{placeholder} ")
                };
                Self::insert_active_text(session, &insertion);
                self.attachments
                    .push((placeholder, format!("@\"{}\"", path.display())));
            }
            Err(error) => session.notice = Some(error),
        }
    }

    /// Texte envoyé : les repères de collage et d'image reprennent leur contenu.
    fn expand_attachments(&mut self, line: String) -> String {
        let mut expanded = line;
        for (placeholder, content) in self.attachments.drain(..) {
            expanded = expanded.replace(&placeholder, &content);
        }
        expanded
    }

    /// Touche reçue au milieu d'un collage : du texte, jamais une commande.
    fn paste_key(session: &mut EditSession, key: KeyEvent) {
        match key.code {
            KeyCode::Enter => Self::insert_active_text(session, "\n"),
            KeyCode::Tab => Self::insert_active_text(session, "\t"),
            KeyCode::Char(ch) => Self::insert_active_char(session, ch),
            _ => {}
        }
    }

    /// Pré-remplit la prochaine saisie.
    pub fn set_initial_text(&mut self, text: String) {
        self.initial_text = Some(text);
    }

    pub fn push_history(&mut self, entry: impl Into<String>) {
        let entry = entry.into();
        if entry.trim().is_empty() {
            return;
        }

        self.history.push(entry);
    }

    #[allow(clippy::too_many_lines)]
    pub fn read_line(&mut self) -> io::Result<ReadOutcome> {
        if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
            return self.read_line_fallback();
        }

        let _raw_mode = RawModeGuard::new()?;
        let mut stdout = io::stdout();
        let mut session = EditSession::new(self.vim_enabled);
        if let Some(text) = self.initial_text.take() {
            session.cursor = text.len();
            session.text = text;
        }
        self.draw(&mut session, &mut stdout)?;

        // Un collage arrive comme une rafale de touches : une Entrée suivie
        // d'autres touches déjà reçues est un saut de ligne, pas un envoi.
        let mut pending: VecDeque<Event> = VecDeque::new();
        let mut burst_origin: Option<usize> = None;
        let mut pasting: Option<usize> = None;
        loop {
            let event = match pending.pop_front() {
                Some(event) => event,
                None => event::read()?,
            };
            let queued = !pending.is_empty() || event::poll(Duration::ZERO)?;
            let action = match event {
                Event::Paste(text) => {
                    let start = session.cursor;
                    let text = text.replace("\r\n", "\n").replace('\r', "\n");
                    Self::insert_active_text(&mut session, &text);
                    self.collapse_paste(&mut session, start);
                    KeyAction::Continue
                }
                Event::Key(key)
                    if matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) =>
                {
                    let key = normalize_key(key);
                    if pasting.is_some() {
                        Self::paste_key(&mut session, key);
                        KeyAction::Continue
                    } else {
                        if queued && burst_origin.is_none() {
                            burst_origin = Some(session.cursor);
                        }
                        let plain_enter = key.code == KeyCode::Enter && key.modifiers.is_empty();
                        if plain_enter && queued && paste_follows(&mut pending)? {
                            pasting = Some(burst_origin.unwrap_or(session.cursor));
                            Self::insert_active_text(&mut session, "\n");
                            KeyAction::Continue
                        } else if queued && key.code == KeyCode::Tab && key.modifiers.is_empty() {
                            Self::insert_active_text(&mut session, "\t");
                            KeyAction::Continue
                        } else {
                            let before = session.text.clone();
                            let action = self.handle_key_event(&mut session, key);
                            if session.text != before {
                                session.menu_index = 0;
                                session.menu_hidden = false;
                            }
                            action
                        }
                    }
                }
                _ => KeyAction::Continue,
            };
            // Fin de la rafale : le collage est replié s'il est long.
            let wait = if pasting.is_some() {
                Duration::from_millis(15)
            } else {
                Duration::ZERO
            };
            if pending.is_empty() && !event::poll(wait)? {
                burst_origin = None;
                if let Some(start) = pasting.take() {
                    self.collapse_paste(&mut session, start);
                }
            }

            match action {
                KeyAction::Continue => {
                    if pasting.is_none() {
                        self.draw(&mut session, &mut stdout)?;
                    }
                }
                KeyAction::CycleMode => {
                    session.clear_render(&mut stdout)?;
                    return Ok(ReadOutcome::CycleMode(session.text.clone()));
                }
                KeyAction::ShowDetails => {
                    session.clear_render(&mut stdout)?;
                    return Ok(ReadOutcome::ShowDetails(session.text.clone()));
                }
                KeyAction::Rewind => {
                    session.clear_render(&mut stdout)?;
                    return Ok(ReadOutcome::Rewind);
                }
                KeyAction::Submit(line) => {
                    session.finalize_render(&mut stdout, &self.prompt, self.vim_enabled)?;
                    return Ok(ReadOutcome::Submit(self.expand_attachments(line)));
                }
                KeyAction::Cancel => {
                    self.attachments.clear();
                    session.clear_render(&mut stdout)?;
                    write!(stdout, "\r\n")?;
                    return Ok(ReadOutcome::Cancel);
                }
                KeyAction::Exit => {
                    session.clear_render(&mut stdout)?;
                    write!(stdout, "\r\n")?;
                    return Ok(ReadOutcome::Exit);
                }
                KeyAction::ToggleVim => {
                    session.clear_render(&mut stdout)?;
                    self.vim_enabled = !self.vim_enabled;
                    write!(
                        stdout,
                        "Mode vim {}.\r\n",
                        if self.vim_enabled {
                            "activé"
                        } else {
                            "désactivé"
                        }
                    )?;
                    session = EditSession::new(self.vim_enabled);
                    self.draw(&mut session, &mut stdout)?;
                }
            }
        }
    }

    fn read_line_fallback(&mut self) -> io::Result<ReadOutcome> {
        loop {
            let mut stdout = io::stdout();
            write!(stdout, "{}", self.prompt)?;
            stdout.flush()?;

            let mut buffer = String::new();
            let bytes_read = io::stdin().read_line(&mut buffer)?;
            if bytes_read == 0 {
                return Ok(ReadOutcome::Exit);
            }

            while matches!(buffer.chars().last(), Some('\n' | '\r')) {
                buffer.pop();
            }

            if Self::handle_submission(&buffer) == Submission::ToggleVim {
                self.vim_enabled = !self.vim_enabled;
                writeln!(
                    stdout,
                    "Mode Vim {}.",
                    if self.vim_enabled {
                        "activé"
                    } else {
                        "désactivé"
                    }
                )?;
                continue;
            }

            return Ok(ReadOutcome::Submit(buffer));
        }
    }

    fn handle_key_event(&mut self, session: &mut EditSession, key: KeyEvent) -> KeyAction {
        if key.code != KeyCode::Tab {
            self.completion_state = None;
        }
        session.notice = None;
        if let Some(action) = self.handle_menu_key(session, key) {
            return action;
        }
        if key.code != KeyCode::Esc {
            session.last_escape = None;
        }

        // Alt+V (ou Ctrl+V si le terminal la laisse passer) : image du presse-papiers.
        let alt_v = key.modifiers.contains(KeyModifiers::ALT)
            && !key.modifiers.contains(KeyModifiers::CONTROL)
            && matches!(key.code, KeyCode::Char('v' | 'V'));
        if alt_v {
            self.paste_image(session);
            return KeyAction::Continue;
        }

        if key.modifiers.contains(KeyModifiers::CONTROL) {
            match key.code {
                KeyCode::Char('o' | 'O') => return KeyAction::ShowDetails,
                KeyCode::Char('v' | 'V') if !key.modifiers.contains(KeyModifiers::ALT) => {
                    self.paste_image(session);
                    return KeyAction::Continue;
                }
                KeyCode::Char('c' | 'C') => {
                    return if session.has_input() {
                        KeyAction::Cancel
                    } else {
                        KeyAction::Exit
                    };
                }
                KeyCode::Char('j' | 'J') => {
                    if session.mode != EditorMode::Normal && session.mode != EditorMode::Visual {
                        Self::insert_active_text(session, "\n");
                    }
                    return KeyAction::Continue;
                }
                KeyCode::Char('d' | 'D') => {
                    if session.current_len() == 0 {
                        return KeyAction::Exit;
                    }
                    Self::delete_char_under_cursor(session);
                    return KeyAction::Continue;
                }
                _ => {}
            }
        }

        match key.code {
            KeyCode::Enter if key.modifiers.contains(KeyModifiers::SHIFT) => {
                if session.mode != EditorMode::Normal && session.mode != EditorMode::Visual {
                    Self::insert_active_text(session, "\n");
                }
                KeyAction::Continue
            }
            KeyCode::Enter => Self::submit_or_toggle(session),
            KeyCode::Esc => Self::handle_escape(session),
            KeyCode::Backspace => {
                Self::handle_backspace(session);
                KeyAction::Continue
            }
            KeyCode::Delete => {
                Self::delete_char_under_cursor(session);
                KeyAction::Continue
            }
            KeyCode::Left => {
                Self::move_left(session);
                KeyAction::Continue
            }
            KeyCode::Right => {
                Self::move_right(session);
                KeyAction::Continue
            }
            KeyCode::Up => {
                self.history_up(session);
                KeyAction::Continue
            }
            KeyCode::Down => {
                self.history_down(session);
                KeyAction::Continue
            }
            KeyCode::Home => {
                Self::move_line_start(session);
                KeyAction::Continue
            }
            KeyCode::End => {
                Self::move_line_end(session);
                KeyAction::Continue
            }
            KeyCode::Tab => {
                self.complete_slash_command(session);
                KeyAction::Continue
            }
            KeyCode::BackTab => KeyAction::CycleMode,
            KeyCode::Char(ch) => {
                self.handle_char(session, ch);
                KeyAction::Continue
            }
            _ => KeyAction::Continue,
        }
    }

    fn handle_char(&mut self, session: &mut EditSession, ch: char) {
        match session.mode {
            EditorMode::Plain | EditorMode::Insert | EditorMode::Command => {
                Self::insert_active_char(session, ch);
            }
            EditorMode::Normal => self.handle_normal_char(session, ch),
            EditorMode::Visual => Self::handle_visual_char(session, ch),
        }
    }

    fn handle_normal_char(&mut self, session: &mut EditSession, ch: char) {
        if let Some(operator) = session.pending_operator.take() {
            match (operator, ch) {
                ('d', 'd') => {
                    self.delete_current_line(session);
                    return;
                }
                ('y', 'y') => {
                    self.yank_current_line(session);
                    return;
                }
                _ => {}
            }
        }

        match ch {
            'h' => Self::move_left(session),
            'j' => Self::move_down(session),
            'k' => Self::move_up(session),
            'l' => Self::move_right(session),
            'd' | 'y' => session.pending_operator = Some(ch),
            'p' => self.paste_after(session),
            'i' => session.enter_insert_mode(),
            'v' => session.enter_visual_mode(),
            ':' => session.enter_command_mode(),
            _ => {}
        }
    }

    fn handle_visual_char(session: &mut EditSession, ch: char) {
        match ch {
            'h' => Self::move_left(session),
            'j' => Self::move_down(session),
            'k' => Self::move_up(session),
            'l' => Self::move_right(session),
            'v' => session.enter_normal_mode(),
            _ => {}
        }
    }

    fn handle_escape(session: &mut EditSession) -> KeyAction {
        match session.mode {
            // Échap deux fois : efface la saisie, ou (saisie vide) propose de
            // revenir avant un message précédent.
            EditorMode::Plain => {
                let now = Instant::now();
                let double = session
                    .last_escape
                    .is_some_and(|at| now.duration_since(at) < DOUBLE_ESCAPE);
                session.last_escape = if double { None } else { Some(now) };
                if !double {
                    KeyAction::Continue
                } else if session.text.is_empty() {
                    KeyAction::Rewind
                } else {
                    session.text.clear();
                    session.cursor = 0;
                    KeyAction::Continue
                }
            }
            EditorMode::Insert => {
                if session.cursor > 0 {
                    session.cursor = previous_boundary(&session.text, session.cursor);
                }
                session.enter_normal_mode();
                KeyAction::Continue
            }
            EditorMode::Normal => KeyAction::Continue,
            EditorMode::Visual => {
                session.enter_normal_mode();
                KeyAction::Continue
            }
            EditorMode::Command => {
                session.exit_command_mode();
                KeyAction::Continue
            }
        }
    }

    fn handle_backspace(session: &mut EditSession) {
        match session.mode {
            EditorMode::Normal | EditorMode::Visual => Self::move_left(session),
            EditorMode::Command => {
                if session.command_cursor <= 1 {
                    session.exit_command_mode();
                } else {
                    remove_previous_char(&mut session.command_buffer, &mut session.command_cursor);
                }
            }
            EditorMode::Plain | EditorMode::Insert => {
                remove_previous_char(&mut session.text, &mut session.cursor);
            }
        }
    }

    fn submit_or_toggle(session: &EditSession) -> KeyAction {
        let line = session.current_line();
        match Self::handle_submission(&line) {
            Submission::Submit => KeyAction::Submit(line),
            Submission::ToggleVim => KeyAction::ToggleVim,
        }
    }

    fn handle_submission(line: &str) -> Submission {
        if line.trim() == "/vim" {
            Submission::ToggleVim
        } else {
            Submission::Submit
        }
    }

    fn insert_active_char(session: &mut EditSession, ch: char) {
        let mut buffer = [0; 4];
        Self::insert_active_text(session, ch.encode_utf8(&mut buffer));
    }

    fn insert_active_text(session: &mut EditSession, text: &str) {
        if session.mode == EditorMode::Command {
            session
                .command_buffer
                .insert_str(session.command_cursor, text);
            session.command_cursor += text.len();
        } else {
            session.text.insert_str(session.cursor, text);
            session.cursor += text.len();
        }
    }

    fn move_left(session: &mut EditSession) {
        if session.mode == EditorMode::Command {
            session.command_cursor =
                previous_command_boundary(&session.command_buffer, session.command_cursor);
        } else {
            session.cursor = previous_boundary(&session.text, session.cursor);
        }
    }

    fn move_right(session: &mut EditSession) {
        if session.mode == EditorMode::Command {
            session.command_cursor = next_boundary(&session.command_buffer, session.command_cursor);
        } else {
            session.cursor = next_boundary(&session.text, session.cursor);
        }
    }

    fn move_line_start(session: &mut EditSession) {
        if session.mode == EditorMode::Command {
            session.command_cursor = 1;
        } else {
            session.cursor = line_start(&session.text, session.cursor);
        }
    }

    fn move_line_end(session: &mut EditSession) {
        if session.mode == EditorMode::Command {
            session.command_cursor = session.command_buffer.len();
        } else {
            session.cursor = line_end(&session.text, session.cursor);
        }
    }

    fn move_up(session: &mut EditSession) {
        if session.mode == EditorMode::Command {
            return;
        }
        session.cursor = move_vertical(&session.text, session.cursor, -1);
    }

    fn move_down(session: &mut EditSession) {
        if session.mode == EditorMode::Command {
            return;
        }
        session.cursor = move_vertical(&session.text, session.cursor, 1);
    }

    fn delete_char_under_cursor(session: &mut EditSession) {
        match session.mode {
            EditorMode::Command => {
                if session.command_cursor < session.command_buffer.len() {
                    let end = next_boundary(&session.command_buffer, session.command_cursor);
                    session.command_buffer.drain(session.command_cursor..end);
                }
            }
            _ => {
                if session.cursor < session.text.len() {
                    let end = next_boundary(&session.text, session.cursor);
                    session.text.drain(session.cursor..end);
                }
            }
        }
    }

    fn delete_current_line(&mut self, session: &mut EditSession) {
        let (line_start_idx, line_end_idx, delete_start_idx) =
            current_line_delete_range(&session.text, session.cursor);
        self.yank_buffer.text = session.text[line_start_idx..line_end_idx].to_string();
        self.yank_buffer.linewise = true;
        session.text.drain(delete_start_idx..line_end_idx);
        session.cursor = delete_start_idx.min(session.text.len());
    }

    fn yank_current_line(&mut self, session: &mut EditSession) {
        let (line_start_idx, line_end_idx, _) =
            current_line_delete_range(&session.text, session.cursor);
        self.yank_buffer.text = session.text[line_start_idx..line_end_idx].to_string();
        self.yank_buffer.linewise = true;
    }

    fn paste_after(&mut self, session: &mut EditSession) {
        if self.yank_buffer.text.is_empty() {
            return;
        }

        if self.yank_buffer.linewise {
            let line_end_idx = line_end(&session.text, session.cursor);
            let insert_at = if line_end_idx < session.text.len() {
                line_end_idx + 1
            } else {
                session.text.len()
            };
            let mut insertion = self.yank_buffer.text.clone();
            if insert_at == session.text.len()
                && !session.text.is_empty()
                && !session.text.ends_with('\n')
            {
                insertion.insert(0, '\n');
            }
            if insert_at < session.text.len() && !insertion.ends_with('\n') {
                insertion.push('\n');
            }
            session.text.insert_str(insert_at, &insertion);
            session.cursor = if insertion.starts_with('\n') {
                insert_at + 1
            } else {
                insert_at
            };
            return;
        }

        let insert_at = next_boundary(&session.text, session.cursor);
        session.text.insert_str(insert_at, &self.yank_buffer.text);
        session.cursor = insert_at + self.yank_buffer.text.len();
    }

    fn complete_slash_command(&mut self, session: &mut EditSession) {
        if session.mode == EditorMode::Command {
            self.completion_state = None;
            return;
        }
        if let Some(state) = self
            .completion_state
            .as_mut()
            .filter(|_| session.cursor == session.text.len())
            .filter(|state| {
                state
                    .matches
                    .iter()
                    .any(|candidate| candidate == &session.text)
            })
        {
            let candidate = state.matches[state.next_index % state.matches.len()].clone();
            state.next_index += 1;
            session.text.replace_range(..session.cursor, &candidate);
            session.cursor = candidate.len();
            return;
        }
        let (prefix, matches) = if let Some(prefix) =
            slash_command_prefix(&session.text, session.cursor)
        {
            let matches = self
                .completions
                .iter()
                .filter(|candidate| candidate.starts_with(prefix) && candidate.as_str() != prefix)
                .cloned()
                .collect::<Vec<_>>();
            (prefix.to_string(), matches)
        } else if let Some((prefix, matches)) = at_file_completions(&session.text, session.cursor) {
            (prefix, matches)
        } else {
            self.completion_state = None;
            return;
        };
        let prefix = prefix.as_str();
        if matches.is_empty() {
            self.completion_state = None;
            return;
        }

        let candidate = if let Some(state) = self
            .completion_state
            .as_mut()
            .filter(|state| state.prefix == prefix && state.matches == matches)
        {
            let index = state.next_index % state.matches.len();
            state.next_index += 1;
            state.matches[index].clone()
        } else {
            let candidate = matches[0].clone();
            self.completion_state = Some(CompletionState {
                prefix: prefix.to_string(),
                matches,
                next_index: 1,
            });
            candidate
        };

        session.text.replace_range(..session.cursor, &candidate);
        session.cursor = candidate.len();
    }

    fn history_up(&self, session: &mut EditSession) {
        if session.mode == EditorMode::Command || self.history.is_empty() {
            return;
        }

        let next_index = if let Some(index) = session.history_index {
            index.saturating_sub(1)
        } else {
            session.history_backup = Some(session.text.clone());
            self.history.len() - 1
        };

        session.history_index = Some(next_index);
        session.set_text_from_history(self.history[next_index].clone());
    }

    fn history_down(&self, session: &mut EditSession) {
        if session.mode == EditorMode::Command {
            return;
        }

        let Some(index) = session.history_index else {
            return;
        };

        if index + 1 < self.history.len() {
            let next_index = index + 1;
            session.history_index = Some(next_index);
            session.set_text_from_history(self.history[next_index].clone());
            return;
        }

        session.history_index = None;
        let restored = session.history_backup.take().unwrap_or_default();
        session.set_text_from_history(restored);
        if self.vim_enabled {
            session.enter_insert_mode();
        } else {
            session.mode = EditorMode::Plain;
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Submission {
    Submit,
    ToggleVim,
}

struct RawModeGuard;

impl RawModeGuard {
    fn new() -> io::Result<Self> {
        terminal::enable_raw_mode().map_err(io::Error::other)?;
        // Hors Windows, le terminal signale les collages (Event::Paste).
        #[cfg(not(windows))]
        let _ = crossterm::execute!(io::stdout(), crossterm::event::EnableBracketedPaste);
        Ok(Self)
    }
}

impl Drop for RawModeGuard {
    fn drop(&mut self) {
        #[cfg(not(windows))]
        let _ = crossterm::execute!(io::stdout(), crossterm::event::DisableBracketedPaste);
        let _ = terminal::disable_raw_mode();
    }
}

/// Certains terminaux livrent les sauts de ligne d'un collage comme des
/// caractères : ils valent la touche Entrée.
fn normalize_key(key: KeyEvent) -> KeyEvent {
    match key.code {
        KeyCode::Char('\r' | '\n')
            if !key
                .modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
        {
            KeyEvent {
                code: KeyCode::Enter,
                ..key
            }
        }
        _ => key,
    }
}

/// Après une Entrée suivie d'autres événements déjà reçus : lit ceux qui
/// attendent et dit s'il s'agit d'un collage (d'autres touches suivent) ou
/// seulement des relâchements de touches.
fn paste_follows(pending: &mut VecDeque<Event>) -> io::Result<bool> {
    let is_key = |event: &Event| {
        matches!(event, Event::Key(key) if matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat)
            && matches!(key.code, KeyCode::Char(_) | KeyCode::Enter | KeyCode::Tab))
    };
    for wait in [Duration::ZERO, Duration::from_millis(10)] {
        while event::poll(wait)? {
            pending.push_back(event::read()?);
            if pending.iter().any(is_key) {
                return Ok(true);
            }
        }
    }
    Ok(pending.iter().any(is_key))
}

fn previous_boundary(text: &str, cursor: usize) -> usize {
    if cursor == 0 {
        return 0;
    }

    text[..cursor]
        .char_indices()
        .next_back()
        .map_or(0, |(index, _)| index)
}

fn previous_command_boundary(text: &str, cursor: usize) -> usize {
    previous_boundary(text, cursor).max(1)
}

fn next_boundary(text: &str, cursor: usize) -> usize {
    if cursor >= text.len() {
        return text.len();
    }

    text[cursor..]
        .chars()
        .next()
        .map_or(text.len(), |ch| cursor + ch.len_utf8())
}

fn remove_previous_char(text: &mut String, cursor: &mut usize) {
    if *cursor == 0 {
        return;
    }

    let start = previous_boundary(text, *cursor);
    text.drain(start..*cursor);
    *cursor = start;
}

fn line_start(text: &str, cursor: usize) -> usize {
    text[..cursor].rfind('\n').map_or(0, |index| index + 1)
}

fn line_end(text: &str, cursor: usize) -> usize {
    text[cursor..]
        .find('\n')
        .map_or(text.len(), |index| cursor + index)
}

fn move_vertical(text: &str, cursor: usize, delta: isize) -> usize {
    let starts = line_starts(text);
    let current_row = text[..cursor].bytes().filter(|byte| *byte == b'\n').count();
    let current_start = starts[current_row];
    let current_col = text[current_start..cursor].chars().count();

    let max_row = starts.len().saturating_sub(1);
    let target_row = if delta < 0 {
        current_row.saturating_sub(delta.unsigned_abs())
    } else {
        (current_row + delta.unsigned_abs()).min(max_row)
    };
    if target_row == current_row {
        return cursor;
    }

    let target_start = starts[target_row];
    let target_end = if target_row + 1 < starts.len() {
        starts[target_row + 1] - 1
    } else {
        text.len()
    };
    byte_index_for_char_column(&text[target_start..target_end], current_col) + target_start
}

fn line_starts(text: &str) -> Vec<usize> {
    let mut starts = vec![0];
    for (index, ch) in text.char_indices() {
        if ch == '\n' {
            starts.push(index + 1);
        }
    }
    starts
}

fn byte_index_for_char_column(text: &str, column: usize) -> usize {
    text.char_indices()
        .nth(column)
        .map_or(text.len(), |(index, _)| index)
}

fn current_line_delete_range(text: &str, cursor: usize) -> (usize, usize, usize) {
    let line_start_idx = line_start(text, cursor);
    let line_end_core = line_end(text, cursor);
    let line_end_idx = if line_end_core < text.len() {
        line_end_core + 1
    } else {
        line_end_core
    };
    let delete_start_idx = if line_end_idx == text.len() && line_start_idx > 0 {
        line_start_idx - 1
    } else {
        line_start_idx
    };
    (line_start_idx, line_end_idx, delete_start_idx)
}

fn selection_bounds(text: &str, anchor: usize, cursor: usize) -> Option<(usize, usize)> {
    if text.is_empty() {
        return None;
    }

    if cursor >= anchor {
        let end = next_boundary(text, cursor);
        Some((anchor.min(text.len()), end.min(text.len())))
    } else {
        let end = next_boundary(text, anchor);
        Some((cursor.min(text.len()), end.min(text.len())))
    }
}

fn render_selected_text(text: &str, start: usize, end: usize) -> String {
    let mut rendered = String::new();
    let mut in_selection = false;

    for (index, ch) in text.char_indices() {
        if !in_selection && index == start {
            rendered.push_str("\x1b[7m");
            in_selection = true;
        }
        if in_selection && index == end {
            rendered.push_str("\x1b[0m");
            in_selection = false;
        }
        rendered.push(ch);
    }

    if in_selection {
        rendered.push_str("\x1b[0m");
    }

    rendered
}

/// `… @src/ma` + Tab : lignes complètes avec les fichiers et dossiers qui
/// commencent ainsi (relatifs au dossier courant, dossiers suivis de `/`).
fn at_file_completions(line: &str, pos: usize) -> Option<(String, Vec<String>)> {
    if pos != line.len() {
        return None;
    }
    let word_start = line.rfind(char::is_whitespace).map_or(0, |index| index + 1);
    let partial = line[word_start..].strip_prefix('@')?;
    let (dir_part, name_part) = match partial.rfind(['/', '\\']) {
        Some(index) => (&partial[..=index], &partial[index + 1..]),
        None => ("", partial),
    };
    let base = std::env::current_dir()
        .ok()?
        .join(if dir_part.is_empty() { "." } else { dir_part });
    let lower = name_part.to_lowercase();
    let mut entries = std::fs::read_dir(base)
        .ok()?
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().to_string();
            let hidden = name.starts_with('.') && !name_part.starts_with('.');
            if hidden || !name.to_lowercase().starts_with(&lower) {
                return None;
            }
            let suffix = if entry.path().is_dir() { "/" } else { "" };
            Some(format!("{}@{dir_part}{name}{suffix}", &line[..word_start]))
        })
        .collect::<Vec<_>>();
    entries.sort();
    entries.truncate(50);
    entries.retain(|candidate| candidate != line);
    Some((line.to_string(), entries))
}

fn slash_command_prefix(line: &str, pos: usize) -> Option<&str> {
    if pos != line.len() {
        return None;
    }

    let prefix = &line[..pos];
    if prefix.contains(char::is_whitespace) || !prefix.starts_with('/') {
        return None;
    }

    Some(prefix)
}

fn to_u16(value: usize) -> io::Result<u16> {
    u16::try_from(value).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "terminal position overflowed u16",
        )
    })
}

#[cfg(test)]
mod tests {
    use super::{
        selection_bounds, slash_command_prefix, EditSession, EditorMode, KeyAction, LineEditor,
    };
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    #[test]
    fn extracts_only_terminal_slash_command_prefixes() {
        // étant donné
        let complete_prefix = slash_command_prefix("/he", 3);
        let whitespace_prefix = slash_command_prefix("/help me", 5);
        let plain_text_prefix = slash_command_prefix("hello", 5);
        let mid_buffer_prefix = slash_command_prefix("/help", 2);

        // quand
        let result = (
            complete_prefix,
            whitespace_prefix,
            plain_text_prefix,
            mid_buffer_prefix,
        );

        // alors
        assert_eq!(result, (Some("/he"), None, None, None));
    }

    #[test]
    fn toggle_submission_flips_vim_mode() {
        // étant donné
        let mut editor = LineEditor::new("> ", vec!["/help".to_string(), "/vim".to_string()]);

        // quand
        let first = LineEditor::handle_submission("/vim");
        editor.vim_enabled = true;
        let second = LineEditor::handle_submission("/vim");

        // alors
        assert!(matches!(first, super::Submission::ToggleVim));
        assert!(matches!(second, super::Submission::ToggleVim));
    }

    #[test]
    fn normal_mode_supports_motion_and_insert_transition() {
        // étant donné
        let mut editor = LineEditor::new("> ", vec![]);
        editor.vim_enabled = true;
        let mut session = EditSession::new(true);
        session.text = "hello".to_string();
        session.cursor = session.text.len();
        let _ = LineEditor::handle_escape(&mut session);

        // quand
        editor.handle_char(&mut session, 'h');
        editor.handle_char(&mut session, 'i');
        editor.handle_char(&mut session, '!');

        // alors
        assert_eq!(session.mode, EditorMode::Insert);
        assert_eq!(session.text, "hel!lo");
    }

    #[test]
    fn yy_and_p_paste_yanked_line_after_current_line() {
        // étant donné
        let mut editor = LineEditor::new("> ", vec![]);
        editor.vim_enabled = true;
        let mut session = EditSession::new(true);
        session.text = "alpha\nbeta\ngamma".to_string();
        session.cursor = 0;
        let _ = LineEditor::handle_escape(&mut session);

        // quand
        editor.handle_char(&mut session, 'y');
        editor.handle_char(&mut session, 'y');
        editor.handle_char(&mut session, 'p');

        // alors
        assert_eq!(session.text, "alpha\nalpha\nbeta\ngamma");
    }

    #[test]
    fn dd_and_p_paste_deleted_line_after_current_line() {
        // étant donné
        let mut editor = LineEditor::new("> ", vec![]);
        editor.vim_enabled = true;
        let mut session = EditSession::new(true);
        session.text = "alpha\nbeta\ngamma".to_string();
        session.cursor = 0;
        let _ = LineEditor::handle_escape(&mut session);

        // quand
        editor.handle_char(&mut session, 'j');
        editor.handle_char(&mut session, 'd');
        editor.handle_char(&mut session, 'd');
        editor.handle_char(&mut session, 'p');

        // alors
        assert_eq!(session.text, "alpha\ngamma\nbeta\n");
    }

    #[test]
    fn visual_mode_tracks_selection_with_motions() {
        // étant donné
        let mut editor = LineEditor::new("> ", vec![]);
        editor.vim_enabled = true;
        let mut session = EditSession::new(true);
        session.text = "alpha\nbeta".to_string();
        session.cursor = 0;
        let _ = LineEditor::handle_escape(&mut session);

        // quand
        editor.handle_char(&mut session, 'v');
        editor.handle_char(&mut session, 'j');
        editor.handle_char(&mut session, 'l');

        // alors
        assert_eq!(session.mode, EditorMode::Visual);
        assert_eq!(
            selection_bounds(
                &session.text,
                session.visual_anchor.unwrap_or(0),
                session.cursor
            ),
            Some((0, 8))
        );
    }

    #[test]
    fn command_mode_submits_colon_prefixed_input() {
        // étant donné
        let mut editor = LineEditor::new("> ", vec![]);
        editor.vim_enabled = true;
        let mut session = EditSession::new(true);
        session.text = "draft".to_string();
        session.cursor = session.text.len();
        let _ = LineEditor::handle_escape(&mut session);

        // quand
        editor.handle_char(&mut session, ':');
        editor.handle_char(&mut session, 'q');
        editor.handle_char(&mut session, '!');
        let action = LineEditor::submit_or_toggle(&session);

        // alors
        assert_eq!(session.mode, EditorMode::Command);
        assert_eq!(session.command_buffer, ":q!");
        assert!(matches!(action, KeyAction::Submit(line) if line == ":q!"));
    }

    #[test]
    fn push_history_ignores_blank_entries() {
        // étant donné
        let mut editor = LineEditor::new("> ", vec!["/help".to_string()]);

        // quand
        editor.push_history("   ");
        editor.push_history("/help");

        // alors
        assert_eq!(editor.history, vec!["/help".to_string()]);
    }

    #[test]
    fn tab_completes_matching_slash_commands() {
        // étant donné
        let mut editor = LineEditor::new("> ", vec!["/help".to_string(), "/hello".to_string()]);
        let mut session = EditSession::new(false);
        session.text = "/he".to_string();
        session.cursor = session.text.len();

        // quand
        editor.complete_slash_command(&mut session);

        // alors
        assert_eq!(session.text, "/help");
        assert_eq!(session.cursor, 5);
    }

    #[test]
    fn tab_cycles_between_matching_slash_commands() {
        // étant donné
        let mut editor = LineEditor::new(
            "> ",
            vec!["/permissions".to_string(), "/plugin".to_string()],
        );
        let mut session = EditSession::new(false);
        session.text = "/p".to_string();
        session.cursor = session.text.len();

        // quand
        editor.complete_slash_command(&mut session);
        let first = session.text.clone();
        session.cursor = session.text.len();
        editor.complete_slash_command(&mut session);
        let second = session.text.clone();

        // alors
        assert_eq!(first, "/permissions");
        assert_eq!(second, "/plugin");
    }

    #[test]
    fn ctrl_c_cancels_when_input_exists() {
        // étant donné
        let mut editor = LineEditor::new("> ", vec![]);
        let mut session = EditSession::new(false);
        session.text = "draft".to_string();
        session.cursor = session.text.len();

        // quand
        let action = editor.handle_key_event(
            &mut session,
            KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
        );

        // alors
        assert!(matches!(action, KeyAction::Cancel));
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn session_with(text: &str) -> EditSession {
        let mut session = EditSession::new(false);
        session.text = text.to_string();
        session.cursor = session.text.len();
        session
    }

    #[test]
    fn long_lines_wrap_over_several_rows() {
        // 25 caractères après « > » sur 10 colonnes : 3 rangées.
        let session = session_with(&"a".repeat(25));
        assert_eq!(session.cursor_layout("> ", 10), (2, 7, 3));

        // Ligne qui remplit exactement sa rangée : le curseur reste au bout.
        let session = session_with(&"a".repeat(8));
        assert_eq!(session.cursor_layout("> ", 10), (0, 9, 1));

        // Plusieurs lignes, dont une longue.
        let session = session_with("abc\ndefghijklmno");
        assert_eq!(session.cursor_layout("> ", 10), (2, 2, 3));
    }

    #[test]
    fn long_pastes_become_a_placeholder_restored_on_submit() {
        let mut editor = LineEditor::new("> ", vec![]);
        let pasted = (1..=10)
            .map(|n| format!("ligne {n}"))
            .collect::<Vec<_>>()
            .join("\n");
        let mut session = session_with("voici : ");
        let start = session.cursor;
        LineEditor::insert_active_text(&mut session, &pasted);
        editor.collapse_paste(&mut session, start);
        assert_eq!(session.text, "voici : [Texte collé n°1 : 10 lignes]");
        assert_eq!(
            editor.expand_attachments(session.text.clone()),
            format!("voici : {pasted}")
        );

        // Un court collage reste tel quel.
        let mut session = session_with("");
        LineEditor::insert_active_text(&mut session, "a\nb");
        editor.collapse_paste(&mut session, 0);
        assert_eq!(session.text, "a\nb");
    }

    #[test]
    fn slash_menu_filters_commands_and_runs_the_selection() {
        let mut editor = LineEditor::new("> ", vec![]);
        editor.set_command_help(vec![
            ("/help".to_string(), "Aide".to_string()),
            ("/model".to_string(), "Change de modèle".to_string()),
            ("/models".to_string(), "Modèles gratuits".to_string()),
        ]);
        let mut session = session_with("/mo");
        assert_eq!(editor.menu_matches(&session).len(), 2);
        let lines = editor.below_lines(&session, 60);
        assert!(lines[0].contains("/model") && lines[0].contains("Change de modèle"));
        assert!(lines[1].contains("/models"));

        let _ = editor.handle_key_event(&mut session, key(KeyCode::Down));
        let action = editor.handle_key_event(&mut session, key(KeyCode::Enter));
        assert!(matches!(action, KeyAction::Submit(line) if line == "/models"));

        // Tab complète, Échap ferme le menu.
        let mut session = session_with("/he");
        let _ = editor.handle_key_event(&mut session, key(KeyCode::Tab));
        assert_eq!(session.text, "/help ");
        let mut session = session_with("/mo");
        let _ = editor.handle_key_event(&mut session, key(KeyCode::Esc));
        assert!(editor.menu_matches(&session).is_empty());
    }

    #[test]
    fn double_escape_clears_the_input_then_offers_to_rewind() {
        let mut editor = LineEditor::new("> ", vec![]);
        let mut session = session_with("brouillon");
        let first = editor.handle_key_event(&mut session, key(KeyCode::Esc));
        assert!(matches!(first, KeyAction::Continue));
        assert_eq!(session.text, "brouillon");
        let _ = editor.handle_key_event(&mut session, key(KeyCode::Esc));
        assert!(session.text.is_empty());

        let _ = editor.handle_key_event(&mut session, key(KeyCode::Esc));
        let action = editor.handle_key_event(&mut session, key(KeyCode::Esc));
        assert!(matches!(action, KeyAction::Rewind));
    }

    #[test]
    fn ctrl_o_asks_for_the_full_last_turn() {
        let mut editor = LineEditor::new("> ", vec![]);
        let mut session = session_with("en cours");
        let action = editor.handle_key_event(
            &mut session,
            KeyEvent::new(KeyCode::Char('o'), KeyModifiers::CONTROL),
        );
        assert!(matches!(action, KeyAction::ShowDetails));
    }
}
