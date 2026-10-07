//! Points de restauration : avant qu'un outil ne modifie un fichier, son contenu
//! précédent est enregistré pour le tour en cours, afin de pouvoir annuler les
//! derniers tours (`/undo`). Les modifications faites par des commandes shell
//! (`sed -i`, formateurs…) ne sont pas suivies.
//!
//! Une seule session est active par processus : le magasin est global, ce qui
//! permet aussi d'enregistrer les modifications des sous-agents dans le tour
//! du parent.

use std::path::{Component, Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use serde_json::Value;

const MAX_TURNS: usize = 100;
const FILE_TOOLS: &[(&str, &str)] = &[
    ("write_file", "path"),
    ("edit_file", "path"),
    ("NotebookEdit", "notebook_path"),
];

/// Contenu d'un fichier avant sa première modification dans un tour.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileSnapshot {
    pub path: PathBuf,
    /// `None` : le fichier n'existait pas (l'annulation le supprime).
    pub content: Option<String>,
    /// Contenu non textuel : impossible à restaurer fidèlement.
    #[serde(default)]
    pub binary: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TurnCheckpoint {
    /// Index du message utilisateur qui a ouvert le tour.
    pub message_index: usize,
    pub request: String,
    pub created_at: u64,
    pub files: Vec<FileSnapshot>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UndoReport {
    pub turns: usize,
    pub restored: Vec<PathBuf>,
    pub deleted: Vec<PathBuf>,
    pub skipped: Vec<PathBuf>,
    /// Index du premier message annulé et texte de la demande correspondante.
    pub message_index: usize,
    pub request: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CheckpointStore {
    path: Option<PathBuf>,
    turns: Vec<TurnCheckpoint>,
}

impl CheckpointStore {
    /// Charge le magasin d'une session (vide si le fichier n'existe pas).
    #[must_use]
    pub fn open(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        let turns = std::fs::read_to_string(&path)
            .ok()
            .and_then(|contents| serde_json::from_str(&contents).ok())
            .unwrap_or_default();
        Self {
            path: Some(path),
            turns,
        }
    }

    #[must_use]
    pub fn turns(&self) -> &[TurnCheckpoint] {
        &self.turns
    }

    pub fn begin_turn(&mut self, message_index: usize, request: &str) {
        // Un tour reprend un index déjà utilisé (session réécrite) : l'ancien
        // point de restauration n'a plus de sens pour la conversation.
        self.turns.retain(|turn| turn.message_index < message_index);
        self.turns.push(TurnCheckpoint {
            message_index,
            request: request.chars().take(200).collect(),
            created_at: unix_now(),
            files: Vec::new(),
        });
        if self.turns.len() > MAX_TURNS {
            let excess = self.turns.len() - MAX_TURNS;
            self.turns.drain(..excess);
        }
        self.save();
    }

    /// À appeler juste avant l'exécution d'un outil : enregistre le contenu
    /// actuel du fichier ciblé s'il s'agit d'une écriture.
    pub fn record_before_write(&mut self, tool_name: &str, input: &str, cwd: &Path) {
        let Some(field) = FILE_TOOLS
            .iter()
            .find(|(name, _)| *name == tool_name)
            .map(|(_, field)| *field)
        else {
            return;
        };
        let Some(raw_path) = serde_json::from_str::<Value>(input).ok().and_then(|value| {
            value
                .get(field)
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
        }) else {
            return;
        };
        let path = normalize(&cwd.join(raw_path));
        let Some(turn) = self.turns.last_mut() else {
            return;
        };
        if turn.files.iter().any(|snapshot| snapshot.path == path) {
            return;
        }
        let snapshot = match std::fs::read(&path) {
            Ok(bytes) => match String::from_utf8(bytes) {
                Ok(text) => FileSnapshot {
                    path,
                    content: Some(text),
                    binary: false,
                },
                Err(_) => FileSnapshot {
                    path,
                    content: None,
                    binary: true,
                },
            },
            Err(_) => FileSnapshot {
                path,
                content: None,
                binary: false,
            },
        };
        turn.files.push(snapshot);
        self.save();
    }

    /// Annule les `count` derniers tours : restaure les fichiers (le plus ancien
    /// état l'emporte) et retire ces tours du magasin.
    pub fn undo(&mut self, count: usize) -> Result<UndoReport, String> {
        if self.turns.is_empty() {
            return Err("aucun tour à annuler".to_string());
        }
        let count = count.clamp(1, self.turns.len());
        let undone = self.turns.split_off(self.turns.len() - count);
        let mut report = UndoReport {
            turns: undone.len(),
            message_index: undone[0].message_index,
            request: undone[0].request.clone(),
            ..UndoReport::default()
        };
        // Du plus récent au plus ancien : l'état d'avant le premier tour gagne.
        for turn in undone.iter().rev() {
            for snapshot in &turn.files {
                if snapshot.binary {
                    push_unique(&mut report.skipped, &snapshot.path);
                    continue;
                }
                if let Some(content) = &snapshot.content {
                    if let Some(parent) = snapshot.path.parent() {
                        let _ = std::fs::create_dir_all(parent);
                    }
                    std::fs::write(&snapshot.path, content).map_err(|error| {
                        format!(
                            "restauration de {} impossible : {error}",
                            snapshot.path.display()
                        )
                    })?;
                    report.deleted.retain(|path| path != &snapshot.path);
                    push_unique(&mut report.restored, &snapshot.path);
                } else {
                    let _ = std::fs::remove_file(&snapshot.path);
                    report.restored.retain(|path| path != &snapshot.path);
                    push_unique(&mut report.deleted, &snapshot.path);
                }
            }
        }
        self.save();
        Ok(report)
    }

    fn save(&self) {
        let Some(path) = &self.path else {
            return;
        };
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(contents) = serde_json::to_string(&self.turns) {
            let _ = std::fs::write(path, contents);
        }
    }
}

fn push_unique(paths: &mut Vec<PathBuf>, path: &Path) {
    if !paths.iter().any(|existing| existing == path) {
        paths.push(path.to_path_buf());
    }
}

fn normalize(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other),
        }
    }
    normalized
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

// ------------------------------------------------------------ magasin actif

static ACTIVE: Mutex<Option<CheckpointStore>> = Mutex::new(None);

fn with_active<T>(action: impl FnOnce(&mut CheckpointStore) -> T) -> Option<T> {
    let mut guard = ACTIVE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    guard.as_mut().map(action)
}

/// Active le magasin de la session courante (fichier `path`).
pub fn activate(path: impl Into<PathBuf>) {
    *ACTIVE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(CheckpointStore::open(path));
}

pub(crate) fn begin_turn(message_index: usize, request: &str) {
    with_active(|store| store.begin_turn(message_index, request));
}

/// Enregistre le fichier ciblé par un outil d'écriture (sans effet si aucun
/// magasin n'est actif). Utilisé par le runtime et par les sous-agents.
pub fn record_before_write(tool_name: &str, input: &str) {
    let Ok(cwd) = std::env::current_dir() else {
        return;
    };
    with_active(|store| store.record_before_write(tool_name, input, &cwd));
}

pub fn undo(count: usize) -> Result<UndoReport, String> {
    with_active(|store| store.undo(count))
        .unwrap_or_else(|| Err("points de restauration inactifs".to_string()))
}

#[must_use]
pub fn list() -> Vec<TurnCheckpoint> {
    with_active(|store| store.turns().to_vec()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::CheckpointStore;
    use std::fs;

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("claw-checkpoint-{name}-{nanos}"));
        fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    #[test]
    fn undoes_edits_and_creations_across_turns() {
        let dir = temp_dir("undo");
        fs::write(dir.join("lib.rs"), "v1").expect("seed");
        let mut store = CheckpointStore::open(dir.join("store.json"));

        store.begin_turn(0, "modifie lib.rs");
        store.record_before_write("edit_file", r#"{"path":"lib.rs"}"#, &dir);
        fs::write(dir.join("lib.rs"), "v2").expect("edit");
        store.record_before_write("edit_file", r#"{"path":"./lib.rs"}"#, &dir);
        fs::write(dir.join("lib.rs"), "v3").expect("edit again");

        store.begin_turn(4, "crée new.rs");
        store.record_before_write("write_file", r#"{"path":"src/new.rs","content":"x"}"#, &dir);
        fs::create_dir_all(dir.join("src")).expect("src");
        fs::write(dir.join("src/new.rs"), "x").expect("create");
        store.record_before_write("read_file", r#"{"path":"lib.rs"}"#, &dir);

        // Le magasin est persisté puis rechargé.
        let mut reloaded = CheckpointStore::open(dir.join("store.json"));
        assert_eq!(reloaded.turns().len(), 2);
        assert_eq!(
            reloaded.turns()[0].files.len(),
            1,
            "un seul instantané par fichier et par tour"
        );

        let report = reloaded.undo(1).expect("undo last turn");
        assert!(!dir.join("src/new.rs").exists());
        assert_eq!(report.deleted.len(), 1);
        assert_eq!(fs::read_to_string(dir.join("lib.rs")).expect("read"), "v3");

        let report = reloaded.undo(1).expect("undo first turn");
        assert_eq!(fs::read_to_string(dir.join("lib.rs")).expect("read"), "v1");
        assert_eq!(report.message_index, 0);
        assert_eq!(report.request, "modifie lib.rs");
        assert!(reloaded.undo(1).is_err());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn undoing_several_turns_restores_the_oldest_state() {
        let dir = temp_dir("multi");
        fs::write(dir.join("a.txt"), "origine").expect("seed");
        let mut store = CheckpointStore::open(dir.join("store.json"));
        for (index, content) in ["un", "deux", "trois"].iter().enumerate() {
            store.begin_turn(index * 2, content);
            store.record_before_write("write_file", r#"{"path":"a.txt"}"#, &dir);
            fs::write(dir.join("a.txt"), content).expect("write");
        }
        let report = store.undo(2).expect("undo two turns");
        assert_eq!(report.turns, 2);
        assert_eq!(report.message_index, 2);
        assert_eq!(fs::read_to_string(dir.join("a.txt")).expect("read"), "un");
        let _ = fs::remove_dir_all(dir);
    }
}
