//! Commandes lancées en arrière-plan (`run_in_background`) : serveur de dev,
//! surveillance de fichiers, longue compilation… Leur sortie est conservée pour
//! être lue au fil de l'eau (`BashOutput`) et elles peuvent être arrêtées
//! (`KillShell`). Toutes sont arrêtées quand Claw se termine.

use std::io::Read;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// Sortie conservée par tâche (la fin la plus récente).
const MAX_BUFFER_BYTES: usize = 512 * 1024;

#[derive(Default)]
struct OutputBuffer {
    text: String,
    /// Octets écrits depuis le début, y compris ceux retirés du tampon.
    total: usize,
}

struct BackgroundTask {
    id: String,
    command: String,
    started: Instant,
    child: Child,
    pid_file: Option<PathBuf>,
    output: Arc<Mutex<OutputBuffer>>,
    read_offset: usize,
    exit_code: Option<i32>,
    killed: bool,
    /// Conteneur Docker/Podman de la tâche (bac à sable), à arrêter avec elle.
    container: Option<String>,
}

static TASKS: Mutex<Vec<BackgroundTask>> = Mutex::new(Vec::new());
static NEXT_ID: Mutex<u32> = Mutex::new(1);

fn with_tasks<T>(action: impl FnOnce(&mut Vec<BackgroundTask>) -> T) -> T {
    action(
        &mut TASKS
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner),
    )
}

/// État d'une tâche et sortie produite depuis la dernière lecture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskOutput {
    pub id: String,
    pub command: String,
    pub running: bool,
    pub exit_code: Option<i32>,
    pub new_output: String,
    pub elapsed_secs: u64,
    /// Une partie de la sortie a été perdue (plus de 512 Kio depuis la dernière lecture).
    pub truncated: bool,
}

/// Lance `command` (déjà préparé : shell, dossier, variables) en arrière-plan.
pub fn spawn(
    mut command: Command,
    display: &str,
    pid_file: Option<PathBuf>,
) -> std::io::Result<String> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn()?;
    let output = Arc::new(Mutex::new(OutputBuffer::default()));
    for pipe in [
        child
            .stdout
            .take()
            .map(|pipe| Box::new(pipe) as Box<dyn Read + Send>),
        child
            .stderr
            .take()
            .map(|pipe| Box::new(pipe) as Box<dyn Read + Send>),
    ]
    .into_iter()
    .flatten()
    {
        let output = Arc::clone(&output);
        std::thread::spawn(move || pump(pipe, &output));
    }
    let id = {
        let mut next = NEXT_ID
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let id = format!("bash_{}", *next);
        *next += 1;
        id
    };
    with_tasks(|tasks| {
        tasks.push(BackgroundTask {
            id: id.clone(),
            command: display.to_string(),
            started: Instant::now(),
            child,
            pid_file,
            output,
            read_offset: 0,
            exit_code: None,
            killed: false,
            container: None,
        });
    });
    Ok(id)
}

fn pump(mut pipe: Box<dyn Read + Send>, output: &Mutex<OutputBuffer>) {
    let mut chunk = [0_u8; 8192];
    loop {
        match pipe.read(&mut chunk) {
            Ok(0) | Err(_) => return,
            Ok(read) => {
                let text = String::from_utf8_lossy(&chunk[..read]);
                let mut buffer = output
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                buffer.text.push_str(&text);
                buffer.total += text.len();
                if buffer.text.len() > MAX_BUFFER_BYTES {
                    let mut cut = buffer.text.len() - MAX_BUFFER_BYTES;
                    while !buffer.text.is_char_boundary(cut) {
                        cut += 1;
                    }
                    buffer.text.drain(..cut);
                }
            }
        }
    }
}

fn refresh(task: &mut BackgroundTask) {
    if task.exit_code.is_none() {
        if let Ok(Some(status)) = task.child.try_wait() {
            task.exit_code = Some(status.code().unwrap_or(-1));
        }
    }
}

/// Sortie nouvelle d'une tâche (et son état).
pub fn read_output(id: &str) -> Result<TaskOutput, String> {
    with_tasks(|tasks| {
        let Some(index) = tasks
            .iter()
            .position(|task| task.id == id || task.child.id().to_string() == id)
        else {
            return Err(unknown_task(id, &tasks_ids(tasks)));
        };
        let task = &mut tasks[index];
        refresh(task);
        let buffer = task
            .output
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let start_of_buffer = buffer.total - buffer.text.len();
        let truncated = task.read_offset < start_of_buffer;
        let from = task.read_offset.max(start_of_buffer) - start_of_buffer;
        let mut from = from.min(buffer.text.len());
        while !buffer.text.is_char_boundary(from) {
            from -= 1;
        }
        let new_output = buffer.text[from..].to_string();
        let total = buffer.total;
        drop(buffer);
        task.read_offset = total;
        Ok(TaskOutput {
            id: task.id.clone(),
            command: task.command.clone(),
            running: task.exit_code.is_none(),
            exit_code: task.exit_code,
            new_output,
            elapsed_secs: task.started.elapsed().as_secs(),
            truncated,
        })
    })
}

/// Identifiants connus (sans reprendre le verrou, déjà tenu par l'appelant).
fn tasks_ids(tasks: &[BackgroundTask]) -> String {
    tasks
        .iter()
        .map(|task| task.id.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

fn unknown_task(id: &str, known: &str) -> String {
    if known.is_empty() {
        format!("unknown background task `{id}` (no background task is running)")
    } else {
        format!("unknown background task `{id}` (known: {known})")
    }
}

/// Arrête une tâche et ses sous-processus.
pub fn kill(id: &str) -> Result<TaskOutput, String> {
    with_tasks(|tasks| {
        let Some(index) = tasks
            .iter()
            .position(|task| task.id == id || task.child.id().to_string() == id)
        else {
            return Err(unknown_task(id, &tasks_ids(tasks)));
        };
        let task = &mut tasks[index];
        stop(task);
        Ok::<(), String>(())
    })?;
    read_output(id)
}

/// Associe le conteneur du bac à sable à une tâche.
pub(crate) fn set_container(id: &str, name: String) {
    with_tasks(|tasks| {
        if let Some(task) = tasks.iter_mut().find(|task| task.id == id) {
            task.container = Some(name);
        }
    });
}

fn stop(task: &mut BackgroundTask) {
    refresh(task);
    if let Some(name) = task.container.take() {
        crate::bash::stop_container(&name);
    }
    if task.exit_code.is_some() {
        return;
    }
    if let Some(pid_file) = &task.pid_file {
        crate::bash::kill_shell_group(pid_file);
    }
    crate::bash::kill_process_tree(Some(task.child.id()));
    let _ = task.child.kill();
    let _ = task.child.wait();
    task.killed = true;
    task.exit_code = Some(-1);
}

/// Tâches connues (pour `/bashes`).
#[must_use]
pub fn list() -> Vec<TaskOutput> {
    with_tasks(|tasks| {
        tasks
            .iter_mut()
            .map(|task| {
                refresh(task);
                TaskOutput {
                    id: task.id.clone(),
                    command: task.command.clone(),
                    running: task.exit_code.is_none(),
                    exit_code: task.exit_code,
                    new_output: String::new(),
                    elapsed_secs: task.started.elapsed().as_secs(),
                    truncated: false,
                }
            })
            .collect()
    })
}

/// Arrête toutes les tâches encore actives (fin de Claw).
pub fn kill_all() {
    with_tasks(|tasks| {
        for task in tasks.iter_mut() {
            stop(task);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::{kill, list, read_output, spawn};

    fn shell(command: &str) -> std::process::Command {
        plugins::posix_shell_command(command)
    }

    #[test]
    fn captures_output_incrementally_and_reports_exit() {
        let id = spawn(shell("echo un; sleep 1; echo deux"), "echo un…", None).expect("spawn");
        std::thread::sleep(std::time::Duration::from_millis(500));
        let first = read_output(&id).expect("first read");
        assert!(first.new_output.contains("un"), "{first:?}");
        assert!(first.running);
        std::thread::sleep(std::time::Duration::from_millis(1_500));
        let second = read_output(&id).expect("second read");
        assert!(
            second.new_output.contains("deux") && !second.new_output.contains("un"),
            "{second:?}"
        );
        assert!(!second.running);
        assert_eq!(second.exit_code, Some(0));
        assert!(read_output("bash_999").is_err());
    }

    #[test]
    fn kills_long_running_tasks() {
        let id = spawn(shell("sleep 30"), "sleep 30", None).expect("spawn");
        assert!(list().iter().any(|task| task.id == id && task.running));
        let started = std::time::Instant::now();
        let killed = kill(&id).expect("kill");
        assert!(!killed.running);
        assert!(started.elapsed() < std::time::Duration::from_secs(10));
    }
}
