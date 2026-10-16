use std::borrow::Cow;
use std::collections::VecDeque;
use std::io::{self, IsTerminal, Write};
use std::time::{Duration, Instant};

use crossterm::cursor::{MoveTo, MoveToColumn, MoveUp};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use crossterm::queue;
use crossterm::terminal::{self, Clear, ClearType};

use crate::keybindings::{Action, Chord, Context, Keymap, Keystroke, Resolution};

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

/// Recherche inversée dans l'historique (Ctrl+R).
#[derive(Debug, Clone, PartialEq, Eq)]
struct HistorySearch {
    query: String,
    /// Entrée de l'historique affichée.
    position: Option<usize>,
    original: String,
    original_cursor: usize,
}

/// États de la saisie gardés pour Ctrl+_.
const UNDO_LIMIT: usize = 200;

#[allow(clippy::struct_excessive_bools)]
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
    /// Raccourci à plusieurs touches commencé (`ctrl+x`).
    chord: Chord,
    /// Recherche dans l'historique (Ctrl+R) en cours.
    search: Option<HistorySearch>,
    /// États précédents de la saisie (texte, curseur), pour Ctrl+_.
    undo_stack: Vec<(String, usize)>,
    /// La dernière modification était une lettre tapée : les lettres d'un
    /// même mot s'annulent ensemble.
    typing_run: bool,
    /// La touche qui vient d'être traitée était une annulation.
    undid: bool,
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
            chord: Chord::default(),
            search: None,
            undo_stack: Vec::new(),
            typing_run: false,
            undid: false,
        }
    }

    /// Garde l'état d'avant une modification, pour Ctrl+_.
    fn record_edit(&mut self, before: String, before_cursor: usize) {
        let typed_char =
            self.text.chars().count() == before.chars().count() + 1 && self.cursor > before_cursor;
        let word_char = typed_char
            && self.text[..self.cursor]
                .chars()
                .last()
                .is_some_and(|ch| !ch.is_whitespace());
        if !(word_char && self.typing_run) {
            self.undo_stack.push((before, before_cursor));
            if self.undo_stack.len() > UNDO_LIMIT {
                self.undo_stack.remove(0);
            }
        }
        self.typing_run = word_char;
    }

    /// Ctrl+_ : revient à l'état d'avant la dernière modification.
    fn undo(&mut self) {
        if let Some((text, cursor)) = self.undo_stack.pop() {
            self.cursor = cursor.min(text.len());
            self.text = text;
            self.undid = true;
            self.typing_run = false;
        } else {
            self.notice = Some("Rien à annuler".to_string());
        }
    }

    /// Le curseur est sur la première (ou la dernière) ligne de la saisie.
    fn cursor_on_edge_line(&self, top: bool) -> bool {
        if top {
            !self.text[..self.cursor].contains('\n')
        } else {
            !self.text[self.cursor..].contains('\n')
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

#[derive(Debug, PartialEq, Eq)]
enum KeyAction {
    Continue,
    Submit(String),
    Cancel,
    Exit,
    ToggleVim,
    CycleMode,
    ShowDetails,
    Rewind,
    /// Ctrl+G : saisie ouverte dans l'éditeur de texte.
    ExternalEditor,
    /// Ctrl+L : écran effacé, saisie redessinée.
    Redraw,
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
    /// Raccourcis en vigueur (relus à chaque saisie).
    keymap: Keymap,
    /// Saisie mise de côté avec Ctrl+S.
    stash: Option<String>,
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
            keymap: Keymap::default(),
            stash: None,
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

        // Les raccourcis modifiés s'appliquent sans redémarrer.
        self.keymap = Keymap::load();
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
            let (before, before_cursor) = (session.text.clone(), session.cursor);
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
                            self.handle_key_event(&mut session, key)
                        }
                    }
                }
                _ => KeyAction::Continue,
            };
            if session.text != before {
                if !session.undid {
                    session.record_edit(before, before_cursor);
                }
                session.menu_index = 0;
                session.menu_hidden = false;
            }
            session.undid = false;
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
                KeyAction::ExternalEditor => {
                    session.clear_render(&mut stdout)?;
                    session.rendered_cursor_row = 0;
                    session.framed = false;
                    let _ = terminal::disable_raw_mode();
                    let edited = edit_in_external_editor(&session.text);
                    let _ = terminal::enable_raw_mode();
                    match edited {
                        Ok(text) => {
                            if text != session.text {
                                let previous = std::mem::replace(&mut session.text, text);
                                session.undo_stack.push((previous, session.cursor));
                                session.typing_run = false;
                            }
                            session.cursor = session.text.len();
                        }
                        Err(error) => session.notice = Some(error),
                    }
                    self.draw(&mut session, &mut stdout)?;
                }
                KeyAction::Redraw => {
                    queue!(stdout, Clear(ClearType::All), MoveTo(0, 0))?;
                    session.rendered_cursor_row = 0;
                    session.framed = false;
                    self.draw(&mut session, &mut stdout)?;
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

    #[allow(clippy::too_many_lines)]
    fn handle_key_event(&mut self, session: &mut EditSession, key: KeyEvent) -> KeyAction {
        if key.code != KeyCode::Tab {
            self.completion_state = None;
        }
        session.notice = None;
        if session.search.is_some() {
            return self.handle_search_key(session, key);
        }
        if key.code != KeyCode::Esc {
            session.last_escape = None;
        }
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);

        // Ctrl+C et Ctrl+D ne se changent pas : effacer ou quitter.
        if ctrl && !alt {
            match key.code {
                KeyCode::Char('c' | 'C') => {
                    session.chord = Chord::default();
                    return if session.has_input() {
                        KeyAction::Cancel
                    } else {
                        KeyAction::Exit
                    };
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

        // Mode vim : Échap et les lettres hors insertion restent à vim.
        let vim_owns = session.mode != EditorMode::Plain
            && (key.code == KeyCode::Esc
                || (session.mode != EditorMode::Insert
                    && matches!(key.code, KeyCode::Char(_))
                    && !ctrl
                    && !alt));
        if !vim_owns {
            if let Some(stroke) = Keystroke::from_event(key) {
                let contexts: &[Context] = if self.menu_matches(session).is_empty() {
                    &[Context::Chat, Context::Global]
                } else {
                    &[Context::Autocomplete, Context::Chat, Context::Global]
                };
                match self.keymap.resolve(contexts, &mut session.chord, stroke) {
                    Resolution::Action(action) => return self.run_action(session, action),
                    Resolution::Pending => {
                        session.notice = Some(format!(
                            "{}… (touche suivante du raccourci)",
                            session.chord.label()
                        ));
                        return KeyAction::Continue;
                    }
                    Resolution::Cancelled => {
                        session.notice = Some("Raccourci annulé".to_string());
                        return KeyAction::Continue;
                    }
                    Resolution::Unbound => {}
                }
            }
        }

        // Touches d'édition, comme dans Claude Code (non personnalisables).
        if ctrl && !alt {
            match key.code {
                KeyCode::Char(ch) => self.handle_ctrl_edit(session, ch.to_ascii_lowercase()),
                KeyCode::Backspace => self.handle_ctrl_edit(session, 'w'),
                KeyCode::Left => {
                    self.handle_alt_edit(session, 'b');
                }
                KeyCode::Right => {
                    self.handle_alt_edit(session, 'f');
                }
                KeyCode::Delete => {
                    self.handle_alt_edit(session, 'd');
                }
                KeyCode::Enter => return Self::submit_or_toggle(session),
                _ => {}
            }
            return KeyAction::Continue;
        }
        if alt && !ctrl {
            match key.code {
                KeyCode::Char(ch) => {
                    if !self.handle_alt_edit(session, ch.to_ascii_lowercase()) {
                        self.handle_char(session, ch);
                    }
                    return KeyAction::Continue;
                }
                KeyCode::Backspace => {
                    self.handle_ctrl_edit(session, 'w');
                    return KeyAction::Continue;
                }
                KeyCode::Enter => {
                    if session.mode != EditorMode::Normal && session.mode != EditorMode::Visual {
                        Self::insert_active_text(session, "\n");
                    }
                    return KeyAction::Continue;
                }
                _ => {}
            }
        }

        match key.code {
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
                Self::move_up(session);
                KeyAction::Continue
            }
            KeyCode::Down => {
                Self::move_down(session);
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
            // Ctrl+Alt : AltGr sous Windows (« @ », « # » sur un clavier AZERTY).
            KeyCode::Char(ch) => {
                self.handle_char(session, ch);
                KeyAction::Continue
            }
            _ => KeyAction::Continue,
        }
    }

    /// Action d'un raccourci (`keybindings.json`).
    #[allow(clippy::too_many_lines)]
    fn run_action(&mut self, session: &mut EditSession, action: Action) -> KeyAction {
        let matches = self.menu_matches(session);
        let menu = (!matches.is_empty()).then(|| {
            let selected = session.menu_index.min(matches.len() - 1);
            (
                matches.len(),
                selected,
                self.commands[matches[selected]].0.clone(),
            )
        });
        match action {
            Action::Submit => {
                if let Some((_, _, name)) = menu {
                    session.text = name;
                    session.cursor = session.text.len();
                }
                Self::submit_or_toggle(session)
            }
            Action::Newline => {
                if session.mode != EditorMode::Normal && session.mode != EditorMode::Visual {
                    Self::insert_active_text(session, "\n");
                }
                KeyAction::Continue
            }
            Action::Cancel => Self::handle_escape(session),
            Action::CycleMode => KeyAction::CycleMode,
            Action::ImagePaste => {
                self.paste_image(session);
                KeyAction::Continue
            }
            Action::ExternalEditor => KeyAction::ExternalEditor,
            Action::Stash => {
                self.toggle_stash(session);
                KeyAction::Continue
            }
            Action::Undo => {
                session.undo();
                KeyAction::Continue
            }
            Action::Redraw => KeyAction::Redraw,
            Action::ShowDetails => KeyAction::ShowDetails,
            Action::HistoryPrevious => {
                // Saisie sur plusieurs lignes : le curseur monte d'abord.
                if session.cursor_on_edge_line(true) {
                    self.history_up(session);
                } else {
                    Self::move_up(session);
                }
                KeyAction::Continue
            }
            Action::HistoryNext => {
                if session.cursor_on_edge_line(false) {
                    self.history_down(session);
                } else {
                    Self::move_down(session);
                }
                KeyAction::Continue
            }
            Action::HistorySearch => {
                if session.mode != EditorMode::Command {
                    session.search = Some(HistorySearch {
                        query: String::new(),
                        position: None,
                        original: session.text.clone(),
                        original_cursor: session.cursor,
                    });
                    self.show_search(session, true);
                }
                KeyAction::Continue
            }
            Action::AutocompleteAccept => {
                if let Some((_, _, name)) = menu {
                    session.text = format!("{name} ");
                    session.cursor = session.text.len();
                } else {
                    self.complete_slash_command(session);
                }
                KeyAction::Continue
            }
            Action::AutocompleteDismiss => {
                if menu.is_some() {
                    session.menu_hidden = true;
                    KeyAction::Continue
                } else {
                    Self::handle_escape(session)
                }
            }
            Action::AutocompletePrevious => {
                match menu {
                    Some((count, selected, _)) => {
                        session.menu_index = (selected + count - 1) % count;
                    }
                    None => self.history_up(session),
                }
                KeyAction::Continue
            }
            Action::AutocompleteNext => {
                match menu {
                    Some((count, selected, _)) => session.menu_index = (selected + 1) % count,
                    None => self.history_down(session),
                }
                KeyAction::Continue
            }
            Action::SearchNext
            | Action::SearchAccept
            | Action::SearchCancel
            | Action::SearchExecute => KeyAction::Continue,
        }
    }

    /// Touche pendant la recherche dans l'historique : les lettres complètent
    /// la recherche, Ctrl+R passe à une entrée plus ancienne, Tab ou Échap
    /// gardent l'entrée, Entrée l'envoie, Ctrl+C annule.
    fn handle_search_key(&mut self, session: &mut EditSession, key: KeyEvent) -> KeyAction {
        let mut chord = Chord::default();
        let resolution = Keystroke::from_event(key).map_or(Resolution::Unbound, |stroke| {
            self.keymap
                .resolve(&[Context::HistorySearch], &mut chord, stroke)
        });
        let Some(search) = session.search.as_mut() else {
            return KeyAction::Continue;
        };
        match resolution {
            Resolution::Action(Action::SearchNext) => {
                let before = search.position.unwrap_or(self.history.len());
                let found = self.find_in_history(&search.query, before, &session.text);
                if let Some(index) = found {
                    search.position = Some(index);
                    session.text.clone_from(&self.history[index]);
                    session.cursor = session.text.len();
                }
                self.show_search(session, found.is_some());
                return KeyAction::Continue;
            }
            Resolution::Action(Action::SearchAccept) => {
                session.search = None;
                return KeyAction::Continue;
            }
            Resolution::Action(Action::SearchCancel) => {
                Self::cancel_search(session);
                return KeyAction::Continue;
            }
            Resolution::Action(Action::SearchExecute) => {
                session.search = None;
                return Self::submit_or_toggle(session);
            }
            _ => {}
        }
        let typed = match key.code {
            KeyCode::Backspace => {
                if search.query.pop().is_none() {
                    Self::cancel_search(session);
                    return KeyAction::Continue;
                }
                true
            }
            KeyCode::Char(ch)
                if !key.modifiers.contains(KeyModifiers::CONTROL)
                    || key.modifiers.contains(KeyModifiers::ALT) =>
            {
                search.query.push(ch);
                true
            }
            _ => false,
        };
        if !typed {
            // Autre touche : l'entrée trouvée est gardée et la touche agit.
            session.search = None;
            return self.handle_key_event(session, key);
        }
        let query = search.query.clone();
        let found = self.find_in_history(&query, self.history.len(), "");
        if let Some(search) = session.search.as_mut() {
            search.position = found;
        }
        if let Some(index) = found {
            session.text.clone_from(&self.history[index]);
            session.cursor = session.text.len();
        }
        self.show_search(session, found.is_some() || query.is_empty());
        KeyAction::Continue
    }

    fn cancel_search(session: &mut EditSession) {
        if let Some(search) = session.search.take() {
            session.text = search.original;
            session.cursor = search.original_cursor.min(session.text.len());
        }
    }

    /// Entrée la plus récente avant `before` qui contient `query` (sans tenir
    /// compte de la casse) et diffère de `current`.
    fn find_in_history(&self, query: &str, before: usize, current: &str) -> Option<usize> {
        let needle = query.to_lowercase();
        (0..before.min(self.history.len())).rev().find(|index| {
            let entry = &self.history[*index];
            entry != current && entry.to_lowercase().contains(&needle)
        })
    }

    fn show_search(&self, session: &mut EditSession, found: bool) {
        let Some(search) = &session.search else {
            return;
        };
        let next = self
            .keymap
            .keys_for(Action::SearchNext)
            .first()
            .cloned()
            .unwrap_or_else(|| "ctrl+r".to_string());
        session.notice = Some(if found {
            format!(
                "Recherche : « {} » · {next} plus ancien · Tab garder · Entrée envoyer · Ctrl+C annuler",
                search.query
            )
        } else {
            format!("Recherche : « {} » · aucune correspondance", search.query)
        });
    }

    /// Ctrl+S : met la saisie de côté ; sur une saisie vide, la reprend.
    fn toggle_stash(&mut self, session: &mut EditSession) {
        let key = self
            .keymap
            .keys_for(Action::Stash)
            .first()
            .cloned()
            .unwrap_or_else(|| "ctrl+s".to_string());
        if session.text.trim().is_empty() {
            if let Some(text) = self.stash.take() {
                session.text = text;
                session.cursor = session.text.len();
                session.notice = Some("Saisie reprise".to_string());
            } else {
                session.notice = Some(format!("Rien de mis de côté ({key} met la saisie de côté)"));
            }
            return;
        }
        let current = std::mem::take(&mut session.text);
        session.cursor = 0;
        if let Some(previous) = self.stash.replace(current) {
            session.text = previous;
            session.cursor = session.text.len();
        }
        session.notice = Some(format!(
            "Saisie mise de côté : {key} sur une saisie vide la reprend"
        ));
    }

    /// Ctrl+A, E, K, U, W, Y (et Ctrl+Retour arrière) : déplacements et
    /// suppressions de readline ; le texte supprimé se recolle avec Ctrl+Y.
    fn handle_ctrl_edit(&mut self, session: &mut EditSession, ch: char) {
        if session.mode == EditorMode::Command {
            return;
        }
        let cursor = session.cursor;
        let removed = match ch {
            'a' => {
                Self::move_line_start(session);
                None
            }
            'e' => {
                Self::move_line_end(session);
                None
            }
            'k' => {
                let end = line_end(&session.text, cursor);
                Some(cursor..end)
            }
            'u' => {
                let start = line_start(&session.text, cursor);
                Some(start..cursor)
            }
            'w' | 'h' => {
                let head = &session.text[..cursor];
                let trimmed = head.trim_end_matches([' ', '\t']);
                let start = trimmed
                    .rfind(char::is_whitespace)
                    .map_or(0, |index| index + 1);
                Some(start..cursor)
            }
            'y' => {
                if !self.yank_buffer.text.is_empty() {
                    let text = self.yank_buffer.text.clone();
                    Self::insert_active_text(session, &text);
                }
                None
            }
            _ => None,
        };
        if let Some(range) = removed.filter(|range| range.start < range.end) {
            self.yank_buffer.text = session.text[range.clone()].to_string();
            self.yank_buffer.linewise = false;
            session.cursor = range.start;
            session.text.drain(range);
        }
    }

    /// Alt+B, Alt+F, Alt+D : mot précédent, mot suivant, supprimer le mot.
    /// `false` : touche sans effet d'édition.
    fn handle_alt_edit(&mut self, session: &mut EditSession, ch: char) -> bool {
        if session.mode == EditorMode::Command || !matches!(ch, 'b' | 'f' | 'd') {
            return false;
        }
        let text = session.text.clone();
        let is_word = |c: char| c.is_alphanumeric();
        let cursor = session.cursor;
        let next_word_end = || {
            let rest = &text[cursor..];
            let start = rest.find(is_word).unwrap_or(rest.len());
            let end = rest[start..]
                .find(|c: char| !is_word(c))
                .map_or(rest.len(), |offset| start + offset);
            cursor + end
        };
        match ch {
            'b' => {
                let head = &text[..cursor];
                let end = head.trim_end_matches(|c: char| !is_word(c)).len();
                session.cursor = head[..end].rfind(|c: char| !is_word(c)).map_or(0, |index| {
                    index + head[index..].chars().next().map_or(1, char::len_utf8)
                });
            }
            'f' => session.cursor = next_word_end(),
            _ => {
                let end = next_word_end();
                if end > cursor {
                    self.yank_buffer.text = text[cursor..end].to_string();
                    self.yank_buffer.linewise = false;
                    session.text.drain(cursor..end);
                }
            }
        }
        true
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

/// Ctrl+G : la saisie est ouverte dans `$VISUAL`, `$EDITOR` ou, à défaut,
/// le Bloc-notes (Windows) ou `vi` ; le texte enregistré la remplace.
fn edit_in_external_editor(text: &str) -> Result<String, String> {
    let path = std::env::temp_dir().join(format!("claw-prompt-{}.md", std::process::id()));
    std::fs::write(&path, text)
        .map_err(|error| format!("Éditeur externe : fichier temporaire impossible ({error})"))?;
    let editor = ["VISUAL", "EDITOR"]
        .iter()
        .find_map(|name| {
            std::env::var(name)
                .ok()
                .filter(|value| !value.trim().is_empty())
        })
        .unwrap_or_else(|| if cfg!(windows) { "notepad" } else { "vi" }.to_string());
    let status = {
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            std::process::Command::new("cmd")
                .arg("/C")
                .raw_arg(format!("{editor} \"{}\"", path.display()))
                .status()
        }
        #[cfg(not(windows))]
        {
            std::process::Command::new("sh")
                .arg("-c")
                .arg(format!("{editor} \"$1\""))
                .arg("sh")
                .arg(&path)
                .status()
        }
    };
    let edited = std::fs::read_to_string(&path);
    let _ = std::fs::remove_file(&path);
    match status {
        Ok(status) if status.success() => {}
        Ok(status) => {
            return Err(format!(
                "Éditeur externe « {editor} » : arrêt avec le code {}",
                status.code().unwrap_or(-1)
            ))
        }
        Err(error) => {
            return Err(format!(
                "Éditeur externe « {editor} » introuvable ({error})"
            ))
        }
    }
    let edited =
        edited.map_err(|error| format!("Éditeur externe : relecture impossible ({error})"))?;
    Ok(edited
        .replace("\r\n", "\n")
        .trim_end_matches('\n')
        .to_string())
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
    fn ctrl_s_stashes_and_restores_the_input() {
        let mut editor = LineEditor::new("> ", vec![]);
        let mut session = session_with("brouillon");
        let ctrl_s = KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL);
        let _ = editor.handle_key_event(&mut session, ctrl_s);
        assert!(session.text.is_empty());
        let _ = editor.handle_key_event(&mut session, ctrl_s);
        assert_eq!(session.text, "brouillon");
    }

    #[test]
    fn ctrl_underscore_undoes_word_by_word() {
        let mut editor = LineEditor::new("> ", vec![]);
        let mut session = session_with("");
        for ch in "un deux".chars() {
            let before = (session.text.clone(), session.cursor);
            let _ = editor.handle_key_event(&mut session, key(KeyCode::Char(ch)));
            session.record_edit(before.0, before.1);
        }
        let undo = KeyEvent::new(KeyCode::Char('7'), KeyModifiers::CONTROL);
        let _ = editor.handle_key_event(&mut session, undo);
        assert_eq!(session.text, "un ");
        let _ = editor.handle_key_event(&mut session, undo);
        let _ = editor.handle_key_event(&mut session, undo);
        assert_eq!(session.text, "");
    }

    #[test]
    fn ctrl_r_searches_the_history() {
        let mut editor = LineEditor::new("> ", vec![]);
        editor.push_history("cargo test");
        editor.push_history("git status");
        editor.push_history("cargo build");
        let mut session = session_with("brouillon");
        let ctrl_r = KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL);
        let _ = editor.handle_key_event(&mut session, ctrl_r);
        for ch in "carg".chars() {
            let _ = editor.handle_key_event(&mut session, key(KeyCode::Char(ch)));
        }
        assert_eq!(session.text, "cargo build");
        let _ = editor.handle_key_event(&mut session, ctrl_r);
        assert_eq!(session.text, "cargo test");
        let ctrl_c = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        let _ = editor.handle_key_event(&mut session, ctrl_c);
        assert_eq!(session.text, "brouillon");
        assert!(session.search.is_none());
        let _ = editor.handle_key_event(&mut session, ctrl_r);
        let _ = editor.handle_key_event(&mut session, key(KeyCode::Char('g')));
        let _ = editor.handle_key_event(&mut session, key(KeyCode::Char('i')));
        let action = editor.handle_key_event(&mut session, key(KeyCode::Enter));
        assert_eq!(action, KeyAction::Submit("git status".to_string()));
    }

    #[test]
    fn readline_keys_edit_and_chords_open_the_editor() {
        let mut editor = LineEditor::new("> ", vec![]);
        let mut session = session_with("git commit --amend");
        let ctrl = |ch| KeyEvent::new(KeyCode::Char(ch), KeyModifiers::CONTROL);
        let _ = editor.handle_key_event(&mut session, ctrl('w'));
        assert_eq!(session.text, "git commit ");
        let _ = editor.handle_key_event(&mut session, ctrl('a'));
        assert_eq!(session.cursor, 0);
        let _ = editor.handle_key_event(&mut session, ctrl('k'));
        assert!(session.text.is_empty());
        let _ = editor.handle_key_event(&mut session, ctrl('y'));
        assert_eq!(session.text, "git commit ");
        let first = editor.handle_key_event(&mut session, ctrl('x'));
        assert_eq!(first, KeyAction::Continue);
        assert!(session
            .notice
            .as_deref()
            .unwrap_or("")
            .starts_with("ctrl+x"));
        let second = editor.handle_key_event(&mut session, ctrl('e'));
        assert_eq!(second, KeyAction::ExternalEditor);
        // AltGr (Ctrl+Alt) tape toujours son caractère.
        let altgr = KeyEvent::new(
            KeyCode::Char('@'),
            KeyModifiers::CONTROL | KeyModifiers::ALT,
        );
        let _ = editor.handle_key_event(&mut session, altgr);
        assert!(session.text.ends_with('@'));
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
