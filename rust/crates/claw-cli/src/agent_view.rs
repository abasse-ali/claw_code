//! Vue des agents (`claw agents`) : toutes les sessions en arrière-plan sur
//! un écran, groupées par état, avec un aperçu pour répondre sans ouvrir la
//! session et une saisie pour en lancer de nouvelles.

use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crossterm::cursor::{MoveTo, Show};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use crossterm::terminal::{self, Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::{execute, queue};

use crate::jobs::{self, Job, JobState, BLOCKED, DONE, FAILED, STOPPED, WORKING};

const ACCENT: &str = "\x1b[38;2;215;119;87m";
const YELLOW: &str = "\x1b[38;5;220m";
const GREEN: &str = "\x1b[38;5;114m";
const RED: &str = "\x1b[38;5;203m";
const DIM: &str = "\x1b[2m";
const BOLD: &str = "\x1b[1m";
const RESET: &str = "\x1b[0m";
const SPINNER: [char; 5] = ['✢', '*', '✶', '✻', '✽'];
/// Délai pour confirmer une seconde pression (Ctrl+X, Ctrl+C).
const CONFIRM: Duration = Duration::from_secs(2);

/// Ce que l'utilisateur a choisi en quittant la vue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Attach(String),
    Exit,
}

/// Réglages des sessions lancées depuis la vue.
#[derive(Debug, Clone)]
pub struct Defaults {
    pub model: String,
    pub permission_mode: String,
    pub cwd: PathBuf,
    /// `claw agents --cwd` : seulement les sessions lancées sous ce dossier.
    pub filter: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    List,
    Peek,
    Rename,
}

struct Row {
    job: Job,
    state: JobState,
    effective: String,
    alive: bool,
}

struct View {
    defaults: Defaults,
    rows: Vec<Row>,
    selected: Option<String>,
    input: String,
    mode: Mode,
    notice: Option<(String, Instant)>,
    stop_armed: Option<(String, Instant)>,
    exit_armed: Option<Instant>,
    frame: usize,
    last_render: String,
}

/// Groupes, dans l'ordre d'affichage.
fn group_of(row: &Row) -> usize {
    if row.job.pinned {
        return 0;
    }
    match row.effective.as_str() {
        BLOCKED => 1,
        WORKING => 2,
        _ => 3,
    }
}

const GROUPS: [&str; 4] = [
    "Épinglées",
    "Attendent une réponse",
    "En cours",
    "Terminées",
];

impl View {
    fn refresh(&mut self) {
        let filter = self.defaults.filter.clone();
        let mut rows: Vec<Row> = jobs::all()
            .into_iter()
            .filter(|(job, _)| {
                filter.as_ref().is_none_or(|dir| {
                    Path::new(&job.origin).starts_with(dir) || Path::new(&job.cwd).starts_with(dir)
                })
            })
            .map(|(job, state)| {
                let effective = jobs::effective_state(&job, &state);
                let alive = jobs::alive(&job.id) || state.attached;
                Row {
                    job,
                    state,
                    effective,
                    alive,
                }
            })
            .collect();
        rows.sort_by(|a, b| {
            group_of(a)
                .cmp(&group_of(b))
                .then(b.job.created_at.cmp(&a.job.created_at))
        });
        self.rows = rows;
        let still_there = self
            .selected
            .as_ref()
            .is_some_and(|id| self.rows.iter().any(|row| &row.job.id == id));
        if !still_there {
            self.selected = self.rows.first().map(|row| row.job.id.clone());
        }
    }

    fn selected_index(&self) -> Option<usize> {
        let id = self.selected.as_ref()?;
        self.rows.iter().position(|row| &row.job.id == id)
    }

    fn selected_row(&self) -> Option<&Row> {
        self.selected_index().map(|index| &self.rows[index])
    }

    fn move_selection(&mut self, delta: isize) {
        if self.rows.is_empty() {
            return;
        }
        let current = self.selected_index().unwrap_or(0);
        let last = self.rows.len() - 1;
        let next = current.saturating_add_signed(delta).min(last);
        self.selected = Some(self.rows[next].job.id.clone());
    }

    fn say(&mut self, text: impl Into<String>) {
        self.notice = Some((text.into(), Instant::now()));
    }

    fn icon(&self, row: &Row) -> String {
        match row.effective.as_str() {
            BLOCKED => format!("{YELLOW}✻{RESET}"),
            WORKING => format!("{ACCENT}{}{RESET}", SPINNER[self.frame % SPINNER.len()]),
            DONE => {
                let shape = if row.alive { '✻' } else { '∙' };
                format!("{GREEN}{shape}{RESET}")
            }
            FAILED => format!("{RED}∙{RESET}"),
            _ => format!("{DIM}∙{RESET}"),
        }
    }

    #[allow(clippy::too_many_lines)]
    fn render(&mut self, out: &mut impl Write) -> io::Result<()> {
        let (columns, rows_available) = terminal::size().unwrap_or((100, 30));
        let width = usize::from(columns).max(40);
        let height = usize::from(rows_available).max(12);
        let now = jobs::now_ms();
        let count = |state: &str| {
            self.rows
                .iter()
                .filter(|row| row.effective == state)
                .count()
        };
        let finished = self.rows.len() - count(BLOCKED) - count(WORKING);
        let mut lines = vec![
            format!(
                " {ACCENT}✻{RESET} {BOLD}Agents{RESET}{DIM} · {} attend{} une réponse · {} en cours · {} terminée{}{RESET}",
                count(BLOCKED),
                if count(BLOCKED) > 1 { "ent" } else { "" },
                count(WORKING),
                finished,
                if finished > 1 { "s" } else { "" }
            ),
            String::new(),
        ];
        if self.rows.is_empty() {
            lines.push(format!(
                "{DIM}  Aucune session en arrière-plan. Décrivez une tâche ci-dessous pour en lancer une.{RESET}"
            ));
        }
        let name_width = self
            .rows
            .iter()
            .map(|row| row.job.name.chars().count())
            .max()
            .unwrap_or(10)
            .clamp(10, 28);
        let peek_room = if self.mode == Mode::Peek { 12 } else { 0 };
        let list_room = height.saturating_sub(6 + peek_room).max(4);
        let mut current_group = None;
        for (shown, row) in self.rows.iter().enumerate() {
            if shown >= list_room {
                lines.push(format!(
                    "{DIM}  … {} de plus{RESET}",
                    self.rows.len() - shown
                ));
                break;
            }
            let group = group_of(row);
            if current_group != Some(group) {
                if current_group.is_some() {
                    lines.push(String::new());
                }
                lines.push(format!(" {BOLD}{}{RESET}", GROUPS[group]));
                current_group = Some(group);
            }
            let selected = self.selected.as_deref() == Some(row.job.id.as_str());
            let marker = if selected {
                format!("{ACCENT}›{RESET}")
            } else {
                " ".to_string()
            };
            let ended = matches!(row.effective.as_str(), DONE | FAILED | STOPPED);
            let until = if ended {
                row.state.finished_at.unwrap_or(now)
            } else {
                now
            };
            let age = jobs::age(row.job.created_at, until);
            let name: String = row.job.name.chars().take(name_width).collect();
            let detail_room = width.saturating_sub(name_width + age.chars().count() + 10);
            let detail: String = jobs::row_detail(&row.job, &row.state)
                .chars()
                .take(detail_room)
                .collect();
            let padding = detail_room.saturating_sub(detail.chars().count());
            let name_style = if selected { BOLD } else { "" };
            lines.push(format!(
                " {marker} {} {name_style}{name:<name_width$}{RESET}  {DIM}{detail}{RESET}{} {DIM}{age}{RESET}",
                self.icon(row),
                " ".repeat(padding)
            ));
        }
        if self.mode == Mode::Peek {
            if let Some(row) = self.selected_row() {
                lines.push(String::new());
                lines.extend(peek_lines(row, width));
            }
        }
        // Bas de l'écran : avis, saisie, aide.
        while lines.len() + 4 < height {
            lines.push(String::new());
        }
        let notice = self
            .notice
            .as_ref()
            .filter(|(_, at)| at.elapsed() < Duration::from_secs(5))
            .map(|(text, _)| format!(" {YELLOW}{text}{RESET}"))
            .unwrap_or_default();
        lines.push(notice);
        lines.push(format!(
            "{DIM}{}{RESET}",
            "─".repeat(width.saturating_sub(1))
        ));
        let (label, placeholder) = match self.mode {
            Mode::Rename => ("nouveau nom", ""),
            Mode::Peek => ("réponse", "o : autoriser · n : refuser · ou votre réponse"),
            Mode::List => (
                ">",
                "décrivez une tâche pour lancer une session (!commande pour un shell)",
            ),
        };
        let shown_input = if self.input.is_empty() {
            format!("{DIM}{placeholder}{RESET}")
        } else {
            self.input.clone()
        };
        lines.push(format!(" {ACCENT}{label}{RESET} {shown_input}"));
        lines.push(format!(
            "{DIM} ↑↓ choisir · Espace aperçu · Entrée ouvrir ou lancer · Ctrl+X arrêter/supprimer · Ctrl+R renommer · Ctrl+T épingler · Échap quitter{RESET}"
        ));
        let rendered = lines.join("\r\n");
        if rendered == self.last_render {
            return Ok(());
        }
        queue!(out, MoveTo(0, 0), Clear(ClearType::All))?;
        write!(out, "{rendered}")?;
        // Curseur dans la saisie.
        let input_row = u16::try_from(lines.len().saturating_sub(2)).unwrap_or(0);
        let input_col =
            u16::try_from(label.chars().count() + 2 + self.input.chars().count()).unwrap_or(0);
        queue!(out, MoveTo(input_col, input_row))?;
        out.flush()?;
        self.last_render = rendered;
        Ok(())
    }

    /// Une touche ; `Some` pour quitter la vue.
    fn handle_key(&mut self, key: KeyEvent) -> Option<Outcome> {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        if !ctrl || !matches!(key.code, KeyCode::Char('c' | 'C')) {
            self.exit_armed = None;
        }
        match key.code {
            KeyCode::Char('c' | 'C') if ctrl => {
                if !self.input.is_empty() {
                    self.input.clear();
                } else if self.exit_armed.is_some_and(|at| at.elapsed() < CONFIRM) {
                    return Some(Outcome::Exit);
                } else {
                    self.exit_armed = Some(Instant::now());
                    self.say("Ctrl+C encore pour quitter");
                }
            }
            KeyCode::Char('x' | 'X') if ctrl => self.stop_or_delete(),
            KeyCode::Char('r' | 'R') if ctrl => {
                if let Some(row) = self.selected_row() {
                    self.input = row.job.name.clone();
                    self.mode = Mode::Rename;
                }
            }
            KeyCode::Char('t' | 'T') if ctrl => self.toggle_pin(),
            KeyCode::Char(' ') if self.input.is_empty() && self.mode != Mode::Rename => {
                self.mode = if self.mode == Mode::Peek {
                    Mode::List
                } else {
                    Mode::Peek
                };
            }
            KeyCode::Char(ch) if !ctrl => self.input.push(ch),
            KeyCode::Backspace => {
                self.input.pop();
            }
            KeyCode::Up => self.move_selection(-1),
            KeyCode::Down => self.move_selection(1),
            KeyCode::Home => self.selected = self.rows.first().map(|row| row.job.id.clone()),
            KeyCode::End => self.selected = self.rows.last().map(|row| row.job.id.clone()),
            KeyCode::Right if self.input.is_empty() => {
                return self.selected.clone().map(Outcome::Attach);
            }
            KeyCode::Enter => return self.submit(),
            KeyCode::Esc => {
                if self.mode == Mode::Rename {
                    self.mode = Mode::List;
                    self.input.clear();
                } else if self.mode == Mode::Peek {
                    self.mode = Mode::List;
                } else if !self.input.is_empty() {
                    self.input.clear();
                } else {
                    return Some(Outcome::Exit);
                }
            }
            _ => {}
        }
        None
    }

    fn submit(&mut self) -> Option<Outcome> {
        let text = self.input.trim().to_string();
        match self.mode {
            Mode::Rename => {
                if let (Some(row), false) = (self.selected_row(), text.is_empty()) {
                    let mut job = row.job.clone();
                    job.name = text;
                    match jobs::save(&job) {
                        Ok(()) => self.say(format!("Renommée « {} »", job.name)),
                        Err(error) => self.say(format!("Renommage impossible : {error}")),
                    }
                }
                self.input.clear();
                self.mode = Mode::List;
                self.refresh();
                None
            }
            Mode::Peek if !text.is_empty() => {
                if let Some(id) = self.selected.clone() {
                    match jobs::reply(&id, &text) {
                        Ok(message) => self.say(message),
                        Err(error) => self.say(error),
                    }
                }
                self.input.clear();
                self.refresh();
                None
            }
            _ if text.is_empty() => self.selected.clone().map(Outcome::Attach),
            _ => {
                let (prompt, command) = match text.strip_prefix('!') {
                    Some(command) => (String::new(), Some(command.trim().to_string())),
                    None => (text.clone(), None),
                };
                let request = jobs::Dispatch {
                    prompt,
                    name: None,
                    command,
                    model: self.defaults.model.clone(),
                    permission_mode: self.defaults.permission_mode.clone(),
                    origin: self.defaults.cwd.clone(),
                    session_path: None,
                    in_place: false,
                };
                match jobs::dispatch(&request) {
                    Ok(job) => {
                        self.say(format!("Lancée · {} · {}", job.id, job.name));
                        self.selected = Some(job.id);
                    }
                    Err(error) => self.say(format!("Lancement impossible : {error}")),
                }
                self.input.clear();
                self.refresh();
                None
            }
        }
    }

    fn stop_or_delete(&mut self) {
        let Some(id) = self.selected.clone() else {
            return;
        };
        let confirmed = self
            .stop_armed
            .as_ref()
            .is_some_and(|(armed, at)| armed == &id && at.elapsed() < CONFIRM);
        if confirmed {
            self.stop_armed = None;
            match jobs::remove(&id) {
                Ok(message) => self.say(message.replace('\n', " · ")),
                Err(error) => self.say(error),
            }
        } else {
            match jobs::stop(&id) {
                Ok(message) => self.say(format!("{message} · Ctrl+X encore pour la supprimer")),
                Err(error) => self.say(error),
            }
            self.stop_armed = Some((id, Instant::now()));
        }
        self.refresh();
    }

    fn toggle_pin(&mut self) {
        let Some(row) = self.selected_row() else {
            return;
        };
        let mut job = row.job.clone();
        job.pinned = !job.pinned;
        let message = if job.pinned {
            "Épinglée : son processus reste prêt"
        } else {
            "Désépinglée"
        };
        if jobs::save(&job).is_ok() {
            self.say(message);
        }
        self.refresh();
    }
}

/// Panneau d'aperçu : la question en attente, ou les dernières sorties.
fn peek_lines(row: &Row, width: usize) -> Vec<String> {
    let inner = width.saturating_sub(6).max(20);
    let mut body = Vec::new();
    let state_label = match row.effective.as_str() {
        BLOCKED => format!("{YELLOW}attend une réponse{RESET}"),
        WORKING => format!("{ACCENT}en cours{RESET}"),
        DONE => format!("{GREEN}terminée{RESET}"),
        FAILED => format!("{RED}échec{RESET}"),
        _ => format!("{DIM}arrêtée{RESET}"),
    };
    body.push(format!(
        "{BOLD}{}{RESET} · {} · {state_label}",
        row.job.name, row.job.id
    ));
    if let Some(tree) = &row.job.worktree {
        body.push(format!(
            "{DIM}worktree {} ({}){RESET}",
            tree.path, tree.branch
        ));
    }
    let text = match (&row.state.question, row.effective.as_str()) {
        (Some(question), _) => question.clone(),
        (None, WORKING) => {
            let tail = crate::transcript::strip_ansi(&jobs::log_tail(&row.job.id, 6 * 1024));
            let lines: Vec<&str> = tail
                .lines()
                .filter(|line| !line.trim().is_empty())
                .collect();
            lines[lines.len().saturating_sub(8)..].join("\n")
        }
        _ => row.state.detail.clone(),
    };
    for line in text.lines().take(9) {
        let clipped: String = line.chars().take(inner).collect();
        body.push(clipped);
    }
    let mut lines = vec![format!("{DIM} ╭{}╮{RESET}", "─".repeat(inner + 2))];
    for line in body {
        let plain = crate::transcript::strip_ansi(&line);
        // Trop longue (chemin du worktree…) : coupée, sans ses couleurs.
        let line = if plain.chars().count() > inner {
            plain.chars().take(inner).collect()
        } else {
            line
        };
        let visible = crate::transcript::strip_ansi(&line).chars().count();
        lines.push(format!(
            "{DIM} │{RESET} {line}{} {DIM}│{RESET}",
            " ".repeat(inner.saturating_sub(visible))
        ));
    }
    lines.push(format!("{DIM} ╰{}╯{RESET}", "─".repeat(inner + 2)));
    lines
}

/// Ouvre la vue jusqu'à ce que l'utilisateur la quitte ou ouvre une session.
pub fn run(defaults: Defaults, selected: Option<String>) -> io::Result<Outcome> {
    let mut out = io::stdout();
    terminal::enable_raw_mode()?;
    execute!(out, EnterAlternateScreen)?;
    let mut view = View {
        defaults,
        rows: Vec::new(),
        selected,
        input: String::new(),
        mode: Mode::List,
        notice: None,
        stop_armed: None,
        exit_armed: None,
        frame: 0,
        last_render: String::new(),
    };
    view.refresh();
    let result = (|| -> io::Result<Outcome> {
        let mut last_refresh = Instant::now();
        loop {
            view.render(&mut out)?;
            if event::poll(Duration::from_millis(250))? {
                if let Event::Key(key) = event::read()? {
                    if matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
                        if let Some(outcome) = view.handle_key(key) {
                            return Ok(outcome);
                        }
                    }
                }
            }
            if last_refresh.elapsed() > Duration::from_millis(700) {
                view.frame += 1;
                view.refresh();
                last_refresh = Instant::now();
            }
        }
    })();
    let _ = execute!(out, LeaveAlternateScreen, Show);
    let _ = terminal::disable_raw_mode();
    result
}

/// Liste simple (hors terminal) : une ligne par session.
#[must_use]
pub fn plain_list(filter: Option<&Path>) -> String {
    let now = jobs::now_ms();
    let rows: Vec<String> = jobs::all()
        .into_iter()
        .filter(|(job, _)| {
            filter.is_none_or(|dir| {
                Path::new(&job.origin).starts_with(dir) || Path::new(&job.cwd).starts_with(dir)
            })
        })
        .map(|(job, state)| {
            let effective = jobs::effective_state(&job, &state);
            format!(
                "{}  {:<8} {:<28} {}  ({})",
                job.id,
                effective,
                job.name.chars().take(28).collect::<String>(),
                jobs::row_detail(&job, &state),
                jobs::age(job.created_at, state.finished_at.unwrap_or(now))
            )
        })
        .collect();
    if rows.is_empty() {
        "Aucune session en arrière-plan (claw --bg \"tâche\" pour en lancer une).".to_string()
    } else {
        rows.join("\n")
    }
}
