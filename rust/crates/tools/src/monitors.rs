//! Outil `Monitor` (comme dans Claude Code) : une commande tourne en
//! arrière-plan et chaque ligne qu'elle écrit revient au modèle entre deux
//! tours, sans bloquer la conversation (suivre un journal, attendre la fin
//! d'une CI, surveiller des fichiers). Chaque surveillance a une échéance
//! (5 minutes par défaut, 30 au plus) ; à l'échéance, un avis prévient le
//! modèle, qui peut la relancer.

use std::fmt::Write as _;
use std::io::{BufRead, BufReader};
use std::process::{Child, Stdio};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

const DEFAULT_TIMEOUT_MS: u64 = 300_000;
const MAX_TIMEOUT_MS: u64 = 1_800_000;
/// Lignes gardées au plus par surveillance entre deux remises.
const MAX_PENDING_LINES: usize = 400;
/// Une ligne attend ce délai que d'autres la rejoignent avant d'être remise.
const BATCH_DELAY: Duration = Duration::from_millis(1_500);

struct Watch {
    id: String,
    description: String,
    command: String,
    started: Instant,
    deadline: Instant,
    child: Option<Arc<Mutex<Child>>>,
    pending: Vec<String>,
    dropped: usize,
    first_pending: Option<Instant>,
    ended: Option<String>,
    delivered_end: bool,
}

fn watches() -> &'static Mutex<Vec<Watch>> {
    static WATCHES: OnceLock<Mutex<Vec<Watch>>> = OnceLock::new();
    WATCHES.get_or_init(|| Mutex::new(Vec::new()))
}

fn lock() -> std::sync::MutexGuard<'static, Vec<Watch>> {
    watches()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn push_line(id: &str, line: &str) {
    let mut list = lock();
    if let Some(watch) = list.iter_mut().find(|watch| watch.id == id) {
        let line: String = line.chars().take(2_000).collect();
        if watch.pending.len() >= MAX_PENDING_LINES {
            watch.pending.remove(0);
            watch.dropped += 1;
        }
        watch.pending.push(line);
        watch.first_pending.get_or_insert_with(Instant::now);
    }
}

fn end(id: &str, reason: String) {
    let mut list = lock();
    if let Some(watch) = list.iter_mut().find(|watch| watch.id == id) {
        if watch.ended.is_none() {
            watch.ended = Some(reason);
        }
        watch.child = None;
    }
}

fn kill_tree(child: &mut Child) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let _ = std::process::Command::new("taskkill")
            .args(["/PID", &child.id().to_string(), "/T", "/F"])
            .creation_flags(0x0800_0000)
            .output();
    }
    let _ = child.kill();
    let _ = child.wait();
}

/// Lance la surveillance ; renvoie son identifiant (`monitor-N`).
#[allow(clippy::too_many_lines)]
pub fn start(
    command: &str,
    description: &str,
    timeout_ms: Option<u64>,
) -> Result<(String, Duration), String> {
    let command = command.trim();
    if command.is_empty() {
        return Err("command is empty".to_string());
    }
    let timeout = Duration::from_millis(
        timeout_ms
            .unwrap_or(DEFAULT_TIMEOUT_MS)
            .clamp(1_000, MAX_TIMEOUT_MS),
    );
    let mut process = plugins::posix_shell_command(command);
    process
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Ok(cwd) = std::env::current_dir() {
        process.current_dir(cwd);
    }
    let mut child = process
        .spawn()
        .map_err(|error| format!("could not start the command: {error}"))?;
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let child = Arc::new(Mutex::new(child));
    let id = {
        let mut list = lock();
        let id = format!("monitor-{}", list.len() + 1);
        list.push(Watch {
            id: id.clone(),
            description: description.trim().to_string(),
            command: command.to_string(),
            started: Instant::now(),
            deadline: Instant::now() + timeout,
            child: Some(Arc::clone(&child)),
            pending: Vec::new(),
            dropped: 0,
            first_pending: None,
            ended: None,
            delivered_end: false,
        });
        id
    };
    for stream in [
        stdout.map(|out| Box::new(out) as Box<dyn std::io::Read + Send>),
        stderr.map(|err| Box::new(err) as Box<dyn std::io::Read + Send>),
    ]
    .into_iter()
    .flatten()
    {
        let id = id.clone();
        let _ = std::thread::Builder::new()
            .name(format!("claw-{id}-read"))
            .spawn(move || {
                for line in BufReader::new(stream).lines().map_while(Result::ok) {
                    if !line.trim().is_empty() {
                        push_line(&id, &line);
                    }
                }
            });
    }
    let watch_id = id.clone();
    let _ = std::thread::Builder::new()
        .name(format!("claw-{id}-wait"))
        .spawn(move || loop {
            std::thread::sleep(Duration::from_millis(250));
            let deadline = lock()
                .iter()
                .find(|watch| watch.id == watch_id)
                .map(|watch| (watch.deadline, watch.ended.is_some()));
            let Some((deadline, ended)) = deadline else {
                return;
            };
            if ended {
                return;
            }
            let mut process = child
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            match process.try_wait() {
                Ok(Some(status)) => {
                    drop(process);
                    // Laisse aux lecteurs le temps de remettre les dernières lignes.
                    std::thread::sleep(Duration::from_millis(300));
                    end(
                        &watch_id,
                        format!("the command exited (code {})", status.code().unwrap_or(-1)),
                    );
                    return;
                }
                Ok(None) if Instant::now() >= deadline => {
                    kill_tree(&mut process);
                    drop(process);
                    end(&watch_id, "it reached its deadline".to_string());
                    return;
                }
                Ok(None) => {}
                Err(error) => {
                    drop(process);
                    end(&watch_id, format!("it could not be followed ({error})"));
                    return;
                }
            }
        });
    Ok((id, timeout))
}

/// Arrête la surveillance `id`.
pub fn stop(id: &str) -> Result<String, String> {
    let child = {
        let list = lock();
        let watch = list
            .iter()
            .find(|watch| watch.id == id)
            .ok_or_else(|| format!("no monitor {id}"))?;
        watch.child.clone()
    };
    if let Some(child) = child {
        kill_tree(
            &mut child
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        );
    }
    let mut list = lock();
    if let Some(watch) = list.iter_mut().find(|watch| watch.id == id) {
        watch.ended = Some("it was stopped".to_string());
        watch.delivered_end = true;
        watch.child = None;
    }
    Ok(format!("{id} stopped"))
}

/// Arrête toutes les surveillances (fin de Claw).
pub fn stop_all() {
    let ids: Vec<String> = lock()
        .iter()
        .filter(|watch| watch.child.is_some())
        .map(|watch| watch.id.clone())
        .collect();
    for id in ids {
        let _ = stop(&id);
    }
}

/// Des lignes (ou une fin) attendent d'être remises au modèle.
#[must_use]
pub fn has_events() -> bool {
    lock().iter().any(|watch| {
        (watch.ended.is_some() && !watch.delivered_end)
            || watch.pending.len() >= 20
            || watch
                .first_pending
                .is_some_and(|first| first.elapsed() >= BATCH_DELAY)
    })
}

/// Ce que les surveillances ont rapporté depuis la dernière remise, en
/// texte pour le modèle.
#[must_use]
pub fn take_events() -> Option<String> {
    let mut list = lock();
    let mut parts = Vec::new();
    for watch in list.iter_mut() {
        let has_lines = !watch.pending.is_empty();
        let has_end = watch.ended.is_some() && !watch.delivered_end;
        if !has_lines && !has_end {
            continue;
        }
        let label = if watch.description.is_empty() {
            watch.command.chars().take(60).collect::<String>()
        } else {
            watch.description.clone()
        };
        let mut part = format!("[Claw] Monitor {} ({label}) reported:", watch.id);
        if watch.dropped > 0 {
            let _ = write!(part, "\n({} earlier lines dropped)", watch.dropped);
        }
        for line in watch.pending.drain(..) {
            part.push('\n');
            part.push_str(&line);
        }
        watch.dropped = 0;
        watch.first_pending = None;
        if has_end {
            let elapsed = watch.started.elapsed().as_secs();
            let _ = write!(
                part,
                "\n[Claw] Monitor {} ended after {elapsed} s: {}. Start a new Monitor if you still need to watch.",
                watch.id,
                watch.ended.clone().unwrap_or_default()
            );
            watch.delivered_end = true;
        }
        parts.push(part);
    }
    if parts.is_empty() {
        return None;
    }
    parts.push("[Claw] React only if something needs attention (tell the user in one or two sentences); otherwise answer in one short line.".to_string());
    Some(parts.join("\n\n"))
}

/// Surveillances de la session (`/tasks`).
#[must_use]
pub fn lines() -> Vec<String> {
    lock()
        .iter()
        .map(|watch| {
            let state = if watch.ended.is_some() {
                "finie"
            } else {
                "en cours"
            };
            let label = if watch.description.is_empty() {
                watch.command.clone()
            } else {
                watch.description.clone()
            };
            format!(
                "  {}  {state:<9} {}",
                watch.id,
                label.chars().take(70).collect::<String>()
            )
        })
        .collect()
}
