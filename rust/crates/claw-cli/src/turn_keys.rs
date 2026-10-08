//! Clavier pendant un tour (REPL) : Échap ou Ctrl+C interrompt l'agent, et le
//! texte tapé est conservé (une ligne validée par Entrée part après le tour,
//! le reste pré-remplit la saisie suivante).
//!
//! Activé sous Windows, où le mode brut ne touche qu'à l'entrée. Sous Unix il
//! désactiverait aussi le traitement des fins de ligne en sortie : Ctrl+C reste
//! alors le moyen d'interrompre. `CLAW_NO_KEY_LISTENER=1` le désactive.

use std::io::{IsTerminal, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::terminal;

/// Une demande d'autorisation lit l'entrée standard : l'écoute se met en pause.
static PAUSED: AtomicBool = AtomicBool::new(false);
/// L'écoute a constaté la pause et ne lit plus le clavier.
static IDLE: AtomicBool = AtomicBool::new(true);
/// Une écoute est active (mode brut à rétablir après une pause).
static ACTIVE: AtomicBool = AtomicBool::new(false);

/// Texte saisi pendant un tour.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct TypeAhead {
    pub lines: Vec<String>,
    pub partial: String,
}

pub(crate) struct TurnKeyListener {
    stop: Arc<AtomicBool>,
    typed: Arc<Mutex<TypeAhead>>,
    handle: Option<JoinHandle<()>>,
}

fn enabled() -> bool {
    cfg!(windows)
        && std::io::stdin().is_terminal()
        && std::io::stdout().is_terminal()
        && std::env::var("CLAW_NO_KEY_LISTENER").map_or(true, |value| value.trim() != "1")
}

impl TurnKeyListener {
    /// Démarre l'écoute (sans effet si le terminal ne s'y prête pas).
    pub(crate) fn start() -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let typed = Arc::new(Mutex::new(TypeAhead::default()));
        if !enabled() || terminal::enable_raw_mode().is_err() {
            return Self {
                stop,
                typed,
                handle: None,
            };
        }
        ACTIVE.store(true, Ordering::SeqCst);
        PAUSED.store(false, Ordering::SeqCst);
        let thread_stop = Arc::clone(&stop);
        let thread_typed = Arc::clone(&typed);
        let handle = std::thread::spawn(move || listen(&thread_stop, &thread_typed));
        Self {
            stop,
            typed,
            handle: Some(handle),
        }
    }

    /// Arrête l'écoute et renvoie le texte saisi.
    pub(crate) fn finish(mut self) -> TypeAhead {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
            let _ = terminal::disable_raw_mode();
        }
        ACTIVE.store(false, Ordering::SeqCst);
        std::mem::take(
            &mut *self
                .typed
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        )
    }
}

impl Drop for TurnKeyListener {
    fn drop(&mut self) {
        if self.handle.is_some() {
            self.stop.store(true, Ordering::SeqCst);
            if let Some(handle) = self.handle.take() {
                let _ = handle.join();
            }
            let _ = terminal::disable_raw_mode();
            ACTIVE.store(false, Ordering::SeqCst);
        }
    }
}

fn notice(text: &str) {
    crate::live::suspend(|| {
        let mut stderr = std::io::stderr();
        let _ = write!(stderr, "\r\n\x1b[2m{text}\x1b[0m\r\n");
        let _ = stderr.flush();
    });
}

fn listen(stop: &AtomicBool, typed: &Mutex<TypeAhead>) {
    let mut last_interrupt: Option<Instant> = None;
    while !stop.load(Ordering::SeqCst) {
        if PAUSED.load(Ordering::SeqCst) {
            IDLE.store(true, Ordering::SeqCst);
            std::thread::sleep(Duration::from_millis(30));
            continue;
        }
        IDLE.store(false, Ordering::SeqCst);
        if !event::poll(Duration::from_millis(50)).unwrap_or(false) {
            continue;
        }
        let Ok(Event::Key(key)) = event::read() else {
            continue;
        };
        if !matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
            continue;
        }
        let ctrl_c =
            key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL);
        if key.code == KeyCode::Esc || ctrl_c {
            // Ctrl+C deux fois en 2 s : quitter, comme hors du mode brut.
            if ctrl_c && last_interrupt.is_some_and(|at| at.elapsed() < Duration::from_secs(2)) {
                let _ = terminal::disable_raw_mode();
                std::process::exit(130);
            }
            last_interrupt = Some(Instant::now());
            runtime::interrupt::request_interrupt();
            notice("⏹ Interruption demandée…");
            continue;
        }
        let mut buffer = typed
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match key.code {
            KeyCode::Enter => {
                let line = std::mem::take(&mut buffer.partial);
                if !line.trim().is_empty() {
                    buffer.lines.push(line.clone());
                    drop(buffer);
                    notice(&format!("↪ en file d'attente : {line}"));
                }
            }
            KeyCode::Backspace => {
                buffer.partial.pop();
            }
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                buffer.partial.push(c);
            }
            _ => {}
        }
    }
    IDLE.store(true, Ordering::SeqCst);
}

/// Avant de lire l'entrée standard (demande d'autorisation) : rend la main.
pub(crate) fn pause() -> PauseGuard {
    if !ACTIVE.load(Ordering::SeqCst) {
        return PauseGuard { resumed: true };
    }
    PAUSED.store(true, Ordering::SeqCst);
    let deadline = Instant::now() + Duration::from_millis(300);
    while !IDLE.load(Ordering::SeqCst) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    let _ = terminal::disable_raw_mode();
    PauseGuard { resumed: false }
}

/// Reprend l'écoute à la fin de la demande d'autorisation.
pub(crate) struct PauseGuard {
    resumed: bool,
}

impl Drop for PauseGuard {
    fn drop(&mut self) {
        if !self.resumed && ACTIVE.load(Ordering::SeqCst) {
            let _ = terminal::enable_raw_mode();
            PAUSED.store(false, Ordering::SeqCst);
        }
    }
}
