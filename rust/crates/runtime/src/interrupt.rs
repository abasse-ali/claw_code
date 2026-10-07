//! Interruption du tour en cours (Ctrl+C dans le terminal, bouton « Arrêter »
//! de l'interface web). Un seul tour s'exécute à la fois par processus : un
//! indicateur global suffit, et il est visible des outils comme du client API.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

static INTERRUPTED: AtomicBool = AtomicBool::new(false);

/// Message d'erreur d'un tour interrompu (reconnu par les interfaces).
pub const INTERRUPTED_MESSAGE: &str = "tour interrompu par l'utilisateur";

pub fn request_interrupt() {
    INTERRUPTED.store(true, Ordering::SeqCst);
}

pub fn clear_interrupt() {
    INTERRUPTED.store(false, Ordering::SeqCst);
}

#[must_use]
pub fn interrupt_requested() -> bool {
    INTERRUPTED.load(Ordering::SeqCst)
}

/// Se termine dès qu'une interruption est demandée (à combiner avec `select!`).
pub async fn wait_for_interrupt() {
    while !interrupt_requested() {
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

#[must_use]
pub fn is_interruption(message: &str) -> bool {
    message.contains(INTERRUPTED_MESSAGE)
}

/// Attend `future`, sauf si une interruption est demandée entre-temps.
pub async fn interruptible<T, E: std::fmt::Display>(
    future: impl std::future::Future<Output = Result<T, E>>,
) -> Result<T, crate::RuntimeError> {
    tokio::select! {
        result = future => result.map_err(|error| crate::RuntimeError::new(error.to_string())),
        () = wait_for_interrupt() => Err(crate::RuntimeError::new(INTERRUPTED_MESSAGE)),
    }
}
