//! Lien avec l'éditeur de l'utilisateur (VS Code) : Claw lui demande les
//! problèmes qu'il voit dans des fichiers (erreurs de compilation, de types,
//! de lint soulignées dans l'éditeur) et attend sa réponse.
//!
//! L'éditeur se déclare en ouvrant le flux d'événements de `claw serve` ; la
//! demande part par ce flux et la réponse revient par l'API. Sans éditeur
//! relié, tout renvoie `None` aussitôt.

use std::collections::HashMap;
use std::sync::mpsc;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

/// Gravité d'un problème, de la plus forte à la plus faible.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Error,
    Warning,
    Info,
    Hint,
}

impl Severity {
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warning => "warning",
            Self::Info => "info",
            Self::Hint => "hint",
        }
    }
}

/// Un problème signalé par l'éditeur (ligne et colonne à partir de 1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Diagnostic {
    pub line: u32,
    #[serde(default)]
    pub column: u32,
    pub severity: Severity,
    pub message: String,
    /// Outil qui le signale (`rustc`, `ts`, `Pylance`…).
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub code: Option<String>,
}

/// Problèmes d'un fichier.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileDiagnostics {
    pub path: String,
    #[serde(default)]
    pub diagnostics: Vec<Diagnostic>,
}

/// Envoie une demande à l'éditeur : identifiant de la demande, fichiers
/// concernés (aucun = tout l'espace de travail).
pub type Requester = Arc<dyn Fn(u64, &[String]) + Send + Sync>;

#[derive(Default)]
struct State {
    /// Éditeurs reliés, par nom.
    editors: Vec<String>,
    requester: Option<Requester>,
    waiting: HashMap<u64, mpsc::Sender<Vec<FileDiagnostics>>>,
    next_id: u64,
}

fn state() -> MutexGuard<'static, State> {
    static STATE: OnceLock<Mutex<State>> = OnceLock::new();
    STATE
        .get_or_init(Mutex::default)
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Indique comment joindre l'éditeur (`claw serve` publie un événement).
pub fn set_requester(requester: Requester) {
    state().requester = Some(requester);
}

/// Présence d'un éditeur : il n'est plus relié quand elle est abandonnée.
#[derive(Debug)]
pub struct Connection(String);

impl Drop for Connection {
    fn drop(&mut self) {
        let mut state = state();
        if let Some(index) = state.editors.iter().position(|name| *name == self.0) {
            state.editors.remove(index);
        }
    }
}

/// Un éditeur se relie (il écoute les demandes de Claw).
#[must_use]
pub fn connect(name: &str) -> Connection {
    state().editors.push(name.to_string());
    Connection(name.to_string())
}

/// Nom de l'éditeur relié, s'il y en a un et qu'il peut être joint.
#[must_use]
pub fn connected() -> Option<String> {
    let state = state();
    state.requester.as_ref()?;
    state.editors.last().cloned()
}

/// Demande à l'éditeur les problèmes de `paths` (tous ceux de l'espace de
/// travail si la liste est vide) et attend sa réponse, `timeout` au plus.
/// `None` sans éditeur relié, sans réponse à temps, ou si le tour est interrompu.
#[must_use]
pub fn check(paths: &[String], timeout: Duration) -> Option<Vec<FileDiagnostics>> {
    let (id, requester, receiver) = {
        let mut state = state();
        if state.editors.is_empty() {
            return None;
        }
        let requester = state.requester.clone()?;
        state.next_id += 1;
        let id = state.next_id;
        let (sender, receiver) = mpsc::channel();
        state.waiting.insert(id, sender);
        (id, requester, receiver)
    };
    requester(id, paths);
    let deadline = Instant::now() + timeout;
    let answer = loop {
        match receiver.recv_timeout(Duration::from_millis(200)) {
            Ok(files) => break Some(files),
            Err(mpsc::RecvTimeoutError::Timeout)
                if Instant::now() < deadline && !crate::interrupt::interrupt_requested() => {}
            Err(_) => break None,
        }
    };
    state().waiting.remove(&id);
    answer
}

/// Réponse de l'éditeur à la demande `id`. `false` si personne ne l'attend plus.
#[must_use]
pub fn deliver(id: u64, files: Vec<FileDiagnostics>) -> bool {
    let sender = state().waiting.remove(&id);
    sender.is_some_and(|sender| sender.send(files).is_ok())
}

/// Problèmes d'au moins la gravité `least`, fichiers sans problème retirés.
#[must_use]
pub fn at_least(files: Vec<FileDiagnostics>, least: Severity) -> Vec<FileDiagnostics> {
    files
        .into_iter()
        .filter_map(|mut file| {
            file.diagnostics
                .retain(|diagnostic| diagnostic.severity <= least);
            (!file.diagnostics.is_empty()).then_some(file)
        })
        .collect()
}

/// Nombre total de problèmes.
#[must_use]
pub fn count(files: &[FileDiagnostics]) -> usize {
    files.iter().map(|file| file.diagnostics.len()).sum()
}

/// Liste lisible par le modèle : `chemin:ligne:colonne: gravité: message [source code]`,
/// `limit` lignes au plus.
#[must_use]
pub fn render(files: &[FileDiagnostics], limit: usize) -> String {
    let mut lines = Vec::new();
    let total = count(files);
    'files: for file in files {
        for diagnostic in &file.diagnostics {
            if lines.len() == limit {
                break 'files;
            }
            let origin = match (&diagnostic.source, &diagnostic.code) {
                (Some(source), Some(code)) => format!(" [{source} {code}]"),
                (Some(source), None) => format!(" [{source}]"),
                (None, Some(code)) => format!(" [{code}]"),
                (None, None) => String::new(),
            };
            // Une ligne par problème : le message est ramené sur une ligne.
            let message = diagnostic
                .message
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            lines.push(format!(
                "{}:{}:{}: {}: {message}{origin}",
                file.path,
                diagnostic.line,
                diagnostic.column.max(1),
                diagnostic.severity.label(),
            ));
        }
    }
    if total > lines.len() {
        lines.push(format!("… and {} more", total - lines.len()));
    }
    lines.join("\n")
}

#[cfg(test)]
pub(crate) mod tests {
    use super::{
        at_least, check, connect, connected, count, deliver, render, set_requester, Diagnostic,
        FileDiagnostics, Severity,
    };
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    pub(crate) fn problem(line: u32, severity: Severity, message: &str) -> Diagnostic {
        Diagnostic {
            line,
            column: 5,
            severity,
            message: message.to_string(),
            source: Some("rustc".to_string()),
            code: None,
        }
    }

    /// Éditeur factice : répond aussitôt `answer` à chaque demande, dans un
    /// autre fil comme le ferait l'API. Garde les fichiers demandés.
    pub(crate) fn fake_editor(answer: Vec<FileDiagnostics>) -> Arc<Mutex<Vec<Vec<String>>>> {
        let asked = Arc::new(Mutex::new(Vec::new()));
        let seen = Arc::clone(&asked);
        set_requester(Arc::new(move |id, paths| {
            seen.lock().expect("demandes").push(paths.to_vec());
            let answer = answer.clone();
            std::thread::spawn(move || deliver(id, answer));
        }));
        asked
    }

    #[test]
    fn asks_the_connected_editor_and_waits_for_its_answer() {
        let answer = vec![FileDiagnostics {
            path: "src/lib.rs".to_string(),
            diagnostics: vec![
                problem(3, Severity::Error, "mismatched\n  types"),
                problem(9, Severity::Warning, "unused variable"),
            ],
        }];
        let asked = fake_editor(answer.clone());

        // Sans éditeur relié : aucune demande, aucune attente.
        assert_eq!(connected(), None);
        assert_eq!(check(&[], Duration::from_secs(5)), None);
        assert!(asked.lock().expect("demandes").is_empty());

        let connection = connect("VS Code");
        assert_eq!(connected().as_deref(), Some("VS Code"));
        let files = check(&["src/lib.rs".to_string()], Duration::from_secs(5)).expect("réponse");
        assert_eq!(files, answer);
        assert_eq!(*asked.lock().expect("demandes"), [["src/lib.rs"]]);

        let errors = at_least(files.clone(), Severity::Error);
        assert_eq!(count(&errors), 1);
        assert_eq!(
            render(&errors, 10),
            "src/lib.rs:3:5: error: mismatched types [rustc]"
        );
        assert_eq!(
            render(&files, 1),
            "src/lib.rs:3:5: error: mismatched types [rustc]\n… and 1 more"
        );
        // Une réponse à une demande que plus personne n'attend est ignorée.
        assert!(!deliver(999, Vec::new()));

        drop(connection);
        assert_eq!(connected(), None);
    }

    #[test]
    fn gives_up_when_the_editor_stays_silent() {
        set_requester(Arc::new(|_, _| {}));
        let _connection = connect("VS Code");
        assert_eq!(check(&[], Duration::from_millis(250)), None);
    }
}
