//! Indicateur d'activité animé, à la manière de Claude Code :
//! `✻ Réflexion… (12 s · Échap pour interrompre)`.
//!
//! Un fil dessine l'indicateur sur la ligne courante tant que l'agent n'écrit
//! rien ; toute sortie passe par [`hide`] (ou par [`LiveStdout`], qui l'appelle)
//! pour effacer l'indicateur avant d'écrire : les deux ne se mélangent jamais.

use std::io::{self, IsTerminal, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

const FRAMES: [&str; 6] = ["·", "✢", "✳", "✶", "✻", "✽"];
const ACCENT: &str = "\x1b[38;2;215;119;87m";

struct State {
    active: bool,
    visible: bool,
    drawn: bool,
    label: String,
    started: Instant,
    frame: usize,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);
static THREAD: AtomicBool = AtomicBool::new(false);

fn enabled() -> bool {
    io::stdout().is_terminal()
        && std::env::var("CLAW_NO_SPINNER").map_or(true, |value| value != "1")
}

fn with_state<T>(action: impl FnOnce(&mut Option<State>) -> T) -> T {
    action(
        &mut STATE
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner),
    )
}

fn erase(state: &mut State) {
    if state.drawn {
        let mut out = io::stdout();
        let _ = write!(out, "\r\x1b[2K");
        let _ = out.flush();
        state.drawn = false;
    }
}

fn draw(state: &mut State) {
    let frame = FRAMES[state.frame % FRAMES.len()];
    state.frame += 1;
    let seconds = state.started.elapsed().as_secs();
    let mut out = io::stdout();
    let _ = write!(
        out,
        "\r\x1b[2K{ACCENT}{frame}\x1b[0m {ACCENT}{}\x1b[0m \x1b[2m({seconds} s · Échap pour interrompre)\x1b[0m",
        state.label
    );
    let _ = out.flush();
    state.drawn = true;
}

fn ensure_thread() {
    if THREAD.swap(true, Ordering::SeqCst) {
        return;
    }
    let _ = std::thread::Builder::new()
        .name("claw-live".to_string())
        .spawn(|| loop {
            std::thread::sleep(Duration::from_millis(120));
            with_state(|state| {
                if let Some(state) = state.as_mut() {
                    if state.active && state.visible {
                        draw(state);
                    }
                }
            });
        });
}

/// Début d'un tour : l'indicateur s'affiche avec ce libellé.
pub fn start(label: &str) {
    if !enabled() {
        return;
    }
    ensure_thread();
    with_state(|state| {
        if let Some(previous) = state.as_mut() {
            erase(previous);
        }
        *state = Some(State {
            active: true,
            visible: true,
            drawn: false,
            label: label.to_string(),
            started: Instant::now(),
            frame: 0,
        });
    });
}

/// Efface l'indicateur avant d'écrire (il ne revient qu'avec [`show`]).
pub fn hide() {
    with_state(|state| {
        if let Some(state) = state.as_mut() {
            state.visible = false;
            erase(state);
        }
    });
}

/// Réaffiche l'indicateur avec un nouveau libellé (l'agent attend ou travaille).
pub fn show(label: &str) {
    with_state(|state| {
        if let Some(state) = state.as_mut() {
            if state.active {
                state.label = label.to_string();
                state.visible = true;
            }
        }
    });
}

/// Écrit un avis pendant que l'indicateur tourne : il est effacé le temps de
/// l'écriture et revient au dessin suivant. `action` ne doit pas appeler ce module.
pub fn suspend<T>(action: impl FnOnce() -> T) -> T {
    let mut state = STATE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(state) = state.as_mut() {
        erase(state);
    }
    action()
}

/// Fin du tour : l'indicateur disparaît ; renvoie la durée du tour.
pub fn stop() -> Option<Duration> {
    with_state(|state| {
        let elapsed = state.as_mut().map(|current| {
            erase(current);
            current.started.elapsed()
        });
        *state = None;
        elapsed
    })
}

/// Sortie standard qui efface l'indicateur avant chaque écriture.
pub struct LiveStdout(io::Stdout);

impl LiveStdout {
    #[must_use]
    pub fn new() -> Self {
        Self(io::stdout())
    }
}

impl Write for LiveStdout {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if !buf.is_empty() {
            hide();
        }
        self.0.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.0.flush()
    }
}
