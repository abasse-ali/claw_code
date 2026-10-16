//! Sessions en arrière-plan, à la manière de la vue des agents de Claude Code.
//!
//! `claw --bg "tâche"` (ou `/bg` dans une session) lance une session qui
//! continue sans terminal ; `claw agents` les montre toutes, et
//! `claw attach|logs|stop|rm|reply <id>` les gèrent depuis le shell.
//!
//! Chaque session est un processus `claw` détaché, sans superviseur. Son état
//! vit dans `~/.claw/jobs/<id>/` :
//! - `job.json` : description (nom, dossier, modèle, première demande…),
//!   écrite par la commande qui lance, renomme ou épingle ;
//! - `state.json` : état écrit par le processus de la session (en cours,
//!   attend une réponse, terminée…) ;
//! - `inbox/` : réponses envoyées à la session, lues dans l'ordre ;
//! - `output.log` : ce que la session affiche ;
//! - `heartbeat` : réécrit toutes les 2 s tant que le processus vit ;
//! - `tmp/` : dossier de travail de la session (`CLAW_JOB_DIR/tmp`).

use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// Un processus dont le battement date de plus que ceci est considéré arrêté.
const ALIVE_WITHIN_MS: u64 = 8_000;
/// Sans demande pendant ce temps, une session non épinglée libère son processus.
const DEFAULT_IDLE_SECS: u64 = 3_600;

pub const WORKING: &str = "working";
pub const BLOCKED: &str = "blocked";
pub const DONE: &str = "done";
pub const FAILED: &str = "failed";
pub const STOPPED: &str = "stopped";

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct JobWorktree {
    pub repo: String,
    pub path: String,
    pub branch: String,
    pub base: String,
}

/// Description d'une session en arrière-plan.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Job {
    pub id: String,
    pub name: String,
    /// `claw` (conversation) ou `exec` (commande shell).
    pub kind: String,
    /// Dossier de travail de la session (son worktree, s'il y en a un).
    pub cwd: String,
    /// Dossier d'où la session a été lancée.
    pub origin: String,
    #[serde(default)]
    pub worktree: Option<JobWorktree>,
    pub model: String,
    pub permission_mode: String,
    #[serde(default)]
    pub prompt: String,
    #[serde(default)]
    pub command: Option<String>,
    /// Conversation reprise (`/bg`), sinon celle que la session crée.
    #[serde(default)]
    pub session_path: Option<String>,
    pub created_at: u64,
    #[serde(default)]
    pub pinned: bool,
}

/// État écrit par le processus de la session.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct JobState {
    pub state: String,
    /// `permission prompt` ou `input needed` quand la session attend.
    #[serde(default)]
    pub waiting_for: Option<String>,
    #[serde(default)]
    pub question: Option<String>,
    #[serde(default)]
    pub detail: String,
    #[serde(default)]
    pub pid: Option<u32>,
    #[serde(default)]
    pub session_path: Option<String>,
    #[serde(default)]
    pub updated_at: u64,
    #[serde(default)]
    pub finished_at: Option<u64>,
    #[serde(default)]
    pub turns: u32,
    /// Ouverte dans un terminal (`claw attach`).
    #[serde(default)]
    pub attached: bool,
}

#[must_use]
pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| {
            u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX)
        })
}

#[must_use]
pub fn jobs_root() -> PathBuf {
    runtime::ConfigLoader::default_for(".")
        .config_home()
        .join("jobs")
}

#[must_use]
pub fn job_dir(id: &str) -> PathBuf {
    jobs_root().join(id)
}

#[must_use]
pub fn log_path(id: &str) -> PathBuf {
    job_dir(id).join("output.log")
}

/// Identifiant court (8 caractères hexadécimaux).
#[must_use]
pub fn new_id() -> String {
    let mut hasher = DefaultHasher::new();
    SystemTime::now().hash(&mut hasher);
    std::process::id().hash(&mut hasher);
    thread::current().id().hash(&mut hasher);
    loop {
        let id = format!("{:08x}", hasher.finish() & 0xffff_ffff);
        if !job_dir(&id).exists() {
            return id;
        }
        now_ms().hash(&mut hasher);
    }
}

fn write_json<T: Serialize>(path: &Path, value: &T) -> io::Result<()> {
    let text = serde_json::to_string_pretty(value).map_err(io::Error::other)?;
    let temp = path.with_extension("tmp");
    fs::write(&temp, text)?;
    fs::rename(&temp, path)
}

#[must_use]
pub fn load(id: &str) -> Option<Job> {
    let text = fs::read_to_string(job_dir(id).join("job.json")).ok()?;
    serde_json::from_str(&text).ok()
}

pub fn save(job: &Job) -> io::Result<()> {
    write_json(&job_dir(&job.id).join("job.json"), job)
}

#[must_use]
pub fn load_state(id: &str) -> JobState {
    fs::read_to_string(job_dir(id).join("state.json"))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

pub fn save_state(id: &str, state: &JobState) -> io::Result<()> {
    let mut state = state.clone();
    state.updated_at = now_ms();
    write_json(&job_dir(id).join("state.json"), &state)
}

/// Modifie l'état sur disque (relu juste avant).
pub fn update_state(id: &str, change: impl FnOnce(&mut JobState)) {
    let mut state = load_state(id);
    change(&mut state);
    let _ = save_state(id, &state);
}

/// Toutes les sessions, les plus récentes d'abord.
#[must_use]
pub fn all() -> Vec<(Job, JobState)> {
    let Ok(entries) = fs::read_dir(jobs_root()) else {
        return Vec::new();
    };
    let mut jobs: Vec<(Job, JobState)> = entries
        .flatten()
        .filter_map(|entry| {
            let id = entry.file_name().to_string_lossy().into_owned();
            let job = load(&id)?;
            let state = load_state(&id);
            Some((job, state))
        })
        .collect();
    jobs.sort_by(|a, b| b.0.created_at.cmp(&a.0.created_at));
    jobs
}

/// Session désignée par son identifiant (ou son début) ou par une partie de
/// son nom.
pub fn find(reference: &str) -> Result<Job, String> {
    let reference = reference.trim();
    if reference.is_empty() {
        return Err("indiquez l'identifiant ou le nom d'une session".to_string());
    }
    if let Some(job) = load(reference) {
        return Ok(job);
    }
    let jobs = all();
    let lower = reference.to_lowercase();
    let by_id: Vec<&Job> = jobs
        .iter()
        .map(|(job, _)| job)
        .filter(|job| job.id.starts_with(&lower))
        .collect();
    let matches = if by_id.is_empty() {
        jobs.iter()
            .map(|(job, _)| job)
            .filter(|job| job.name.to_lowercase().contains(&lower))
            .collect()
    } else {
        by_id
    };
    match matches.as_slice() {
        [job] => Ok((*job).clone()),
        [] => Err(format!(
            "aucune session en arrière-plan ne correspond à « {reference} »"
        )),
        several => Err(format!(
            "« {reference} » désigne plusieurs sessions : {}",
            several
                .iter()
                .map(|job| format!("{} ({})", job.id, job.name))
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}

/// Le processus de la session vit (battement récent).
#[must_use]
pub fn alive(id: &str) -> bool {
    fs::read_to_string(job_dir(id).join("heartbeat"))
        .ok()
        .and_then(|text| text.trim().parse::<u64>().ok())
        .is_some_and(|beat| now_ms().saturating_sub(beat) < ALIVE_WITHIN_MS)
}

/// Battement du processus de la session, tant que la valeur vit.
pub struct Heartbeat {
    stop: Arc<AtomicBool>,
    id: String,
}

impl Heartbeat {
    #[must_use]
    pub fn start(id: &str) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&stop);
        let path = job_dir(id).join("heartbeat");
        let _ = fs::write(&path, now_ms().to_string());
        let _ = thread::Builder::new()
            .name("claw-heartbeat".to_string())
            .spawn(move || {
                while !flag.load(Ordering::Relaxed) {
                    let _ = fs::write(&path, now_ms().to_string());
                    thread::sleep(Duration::from_secs(2));
                }
            });
        Self {
            stop,
            id: id.to_string(),
        }
    }
}

impl Drop for Heartbeat {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        let _ = fs::remove_file(job_dir(&self.id).join("heartbeat"));
    }
}

/// Nom lisible tiré de la demande (six mots au plus).
#[must_use]
pub fn name_from_prompt(prompt: &str) -> String {
    // Le BOM d'un fichier UTF-8 lu comme saisie ne fait pas partie du nom.
    let words: Vec<&str> = prompt
        .split(|ch: char| ch.is_whitespace() || ch == '\u{feff}')
        .filter(|word| !word.is_empty())
        .filter(|word| !word.starts_with('@') && !word.starts_with('['))
        .take(6)
        .collect();
    let name = words.join(" ");
    let name: String = name.chars().take(40).collect();
    if name.trim().is_empty() {
        "session".to_string()
    } else {
        name.trim_end_matches(['.', ',', ':', ';']).to_string()
    }
}

/// `nom`, ou `nom (2)` si une autre session porte déjà ce nom.
#[must_use]
pub fn unique_name(base: &str) -> String {
    let taken: Vec<String> = all().into_iter().map(|(job, _)| job.name).collect();
    if !taken.iter().any(|name| name == base) {
        return base.to_string();
    }
    (2..10_000)
        .map(|number| format!("{base} ({number})"))
        .find(|candidate| !taken.contains(candidate))
        .unwrap_or_else(|| base.to_string())
}

/// `worktree.bgIsolation: "none"` : les sessions lancées en arrière-plan
/// modifient directement le dossier de travail.
fn isolation_enabled(cwd: &Path) -> bool {
    runtime::ConfigLoader::default_for(cwd)
        .load()
        .ok()
        .and_then(|config| {
            config
                .get("worktree")
                .and_then(|value| value.as_object().cloned())
        })
        .and_then(|object| {
            object
                .get("bgIsolation")
                .and_then(|v| v.as_str().map(str::to_string))
        })
        .is_none_or(|mode| mode != "none")
}

/// Worktree propre à la session (dépôt git, isolation active).
fn prepare_worktree(origin: &Path, id: &str) -> Option<JobWorktree> {
    if !isolation_enabled(origin) {
        return None;
    }
    let repo = runtime::worktree::main_root(origin).ok()?;
    let worktree = runtime::worktree::create(&repo, &format!("bg-{id}"), None).ok()?;
    Some(JobWorktree {
        repo: repo.display().to_string(),
        path: worktree.path.display().to_string(),
        branch: worktree.branch,
        base: worktree.base,
    })
}

/// Consigne donnée à une session qui travaille dans son propre worktree.
#[must_use]
pub fn isolation_note(worktree: &JobWorktree) -> String {
    format!(
        "[Claw] You run as a background session, in your own git worktree {} on branch {}, separate from the user's checkout so parallel sessions do not collide. Work and edit files only there. Before you finish, commit your changes on this branch with a clear message (do not push unless the user asks).",
        worktree.path, worktree.branch
    )
}

/// Ce qu'il faut pour lancer une session.
#[derive(Debug, Clone, Default)]
pub struct Dispatch {
    pub prompt: String,
    pub name: Option<String>,
    /// Commande shell (`--exec`) au lieu d'une conversation.
    pub command: Option<String>,
    pub model: String,
    pub permission_mode: String,
    pub origin: PathBuf,
    /// Conversation à continuer (`/bg`, `/fork`).
    pub session_path: Option<PathBuf>,
    /// `/bg` : la session continue là où elle travaillait, sans worktree.
    pub in_place: bool,
}

/// Crée la session et lance son processus.
pub fn dispatch(request: &Dispatch) -> Result<Job, String> {
    let id = new_id();
    let dir = job_dir(&id);
    fs::create_dir_all(dir.join("inbox")).map_err(|error| error.to_string())?;
    fs::create_dir_all(dir.join("tmp")).map_err(|error| error.to_string())?;
    let base_name = request
        .name
        .clone()
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| name_from_prompt(request.command.as_deref().unwrap_or(&request.prompt)));
    let worktree = if request.command.is_none() && !request.in_place {
        prepare_worktree(&request.origin, &id)
    } else {
        None
    };
    let job = Job {
        id: id.clone(),
        name: unique_name(&base_name),
        kind: if request.command.is_some() {
            "exec"
        } else {
            "claw"
        }
        .to_string(),
        cwd: worktree.as_ref().map_or_else(
            || request.origin.display().to_string(),
            |tree| tree.path.clone(),
        ),
        origin: request.origin.display().to_string(),
        worktree,
        model: request.model.clone(),
        permission_mode: request.permission_mode.clone(),
        prompt: request.prompt.clone(),
        command: request.command.clone(),
        session_path: request
            .session_path
            .as_ref()
            .map(|path| path.display().to_string()),
        created_at: now_ms(),
        pinned: false,
    };
    save(&job).map_err(|error| error.to_string())?;
    let initial = JobState {
        state: if job.prompt.trim().is_empty() && job.command.is_none() {
            DONE.to_string()
        } else {
            WORKING.to_string()
        },
        detail: if job.prompt.trim().is_empty() && job.command.is_none() {
            "En attente d'une demande".to_string()
        } else {
            "Démarrage…".to_string()
        },
        session_path: job.session_path.clone(),
        ..JobState::default()
    };
    save_state(&id, &initial).map_err(|error| error.to_string())?;
    spawn_worker(&id).map_err(|error| format!("lancement impossible : {error}"))?;
    Ok(job)
}

/// Programme lancé pour une session. Sous Windows, un programme en cours
/// d'exécution ne peut pas être remplacé : les sessions tournent sur une copie
/// (`jobs/bin/`), pour que `claw` reste installable ou recompilable pendant
/// qu'elles travaillent. Les copies inutilisées sont effacées au passage.
fn worker_executable() -> io::Result<PathBuf> {
    let exe = std::env::current_exe()?;
    if !cfg!(windows) {
        return Ok(exe);
    }
    let metadata = fs::metadata(&exe)?;
    let stamp = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |elapsed| elapsed.as_secs());
    let bin = jobs_root().join("bin");
    fs::create_dir_all(&bin)?;
    let copy = bin.join(format!("claw-{}-{stamp}.exe", metadata.len()));
    if !copy.is_file() {
        let partial = copy.with_extension("partial");
        fs::copy(&exe, &partial)?;
        fs::rename(&partial, &copy)?;
    }
    if let Ok(entries) = fs::read_dir(&bin) {
        for entry in entries.flatten() {
            if entry.path() != copy {
                // Une copie encore utilisée par une session refuse l'effacement.
                let _ = fs::remove_file(entry.path());
            }
        }
    }
    Ok(copy)
}

/// Lance le processus de la session, détaché du terminal.
pub fn spawn_worker(id: &str) -> io::Result<u32> {
    let exe = worker_executable()?;
    // Écriture ordinaire placée à la fin, pas le mode « ajout » : bash de Git
    // for Windows (MSYS), qui hérite de ce fichier pour `--exec`, n'écrit pas
    // dans un fichier ouvert en ajout seul (sortie vide, code 1).
    let mut log = fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(log_path(id))?;
    log.seek(SeekFrom::End(0))?;
    let job = load(id).ok_or_else(|| io::Error::other("session inconnue"))?;
    let mut command = Command::new(exe);
    command
        .args(["__job-worker", id])
        .current_dir(&job.cwd)
        .stdin(Stdio::null())
        .stdout(log.try_clone()?)
        .stderr(log)
        .env("CLAW_JOB_DIR", job_dir(id))
        .env("CLAW_JOB_ID", id)
        .env("CLAW_PLAIN_PROMPTS", "1")
        .env("CLAW_NO_SPINNER", "1");
    detach(&mut command)
}

#[cfg(windows)]
fn detach(command: &mut Command) -> io::Result<u32> {
    use std::os::windows::process::CommandExt;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;
    // Hors du « job » du terminal quand c'est permis : fermer le terminal ne
    // l'arrête pas.
    command.creation_flags(CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW | CREATE_BREAKAWAY_FROM_JOB);
    if let Ok(child) = command.spawn() {
        Ok(child.id())
    } else {
        command.creation_flags(CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW);
        command.spawn().map(|child| child.id())
    }
}

#[cfg(not(windows))]
fn detach(command: &mut Command) -> io::Result<u32> {
    use std::os::unix::process::CommandExt;
    // Son propre groupe : Ctrl+C dans le terminal ne l'atteint pas.
    command.process_group(0);
    command.spawn().map(|child| child.id())
}

/// Dépose une réponse pour la session.
pub fn send(id: &str, text: &str) -> io::Result<()> {
    let inbox = job_dir(id).join("inbox");
    fs::create_dir_all(&inbox)?;
    let mut name = format!("{:016}", now_ms());
    while inbox.join(format!("{name}.txt")).exists() {
        name.push('b');
    }
    fs::write(inbox.join(format!("{name}.txt")), text)
}

/// La plus ancienne réponse en attente, retirée de la boîte.
#[must_use]
pub fn take_message(id: &str) -> Option<String> {
    let inbox = job_dir(id).join("inbox");
    let mut files: Vec<PathBuf> = fs::read_dir(&inbox)
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("txt"))
        .collect();
    files.sort();
    let first = files.into_iter().next()?;
    let text = fs::read_to_string(&first).ok()?;
    let _ = fs::remove_file(&first);
    Some(text)
}

pub fn set_flag(id: &str, flag: &str) {
    let _ = fs::write(job_dir(id).join(flag), now_ms().to_string());
}

#[must_use]
pub fn take_flag(id: &str, flag: &str) -> bool {
    fs::remove_file(job_dir(id).join(flag)).is_ok()
}

/// Attend une réponse ; `None` si la session doit s'arrêter.
#[must_use]
pub fn wait_message(id: &str) -> Option<String> {
    loop {
        if let Some(message) = take_message(id) {
            return Some(message);
        }
        if take_flag(id, "stop") {
            return None;
        }
        thread::sleep(Duration::from_millis(400));
    }
}

/// Attente d'une nouvelle demande après un tour : `Some` quand elle arrive,
/// `None` quand la session doit libérer son processus (`claw attach`,
/// `claw stop`, ou une heure sans demande hors épinglage).
#[must_use]
pub fn wait_next_prompt(id: &str) -> Option<String> {
    let idle = std::env::var("CLAW_JOB_IDLE_SECS")
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .unwrap_or(DEFAULT_IDLE_SECS);
    let started = std::time::Instant::now();
    loop {
        if let Some(message) = take_message(id) {
            return Some(message);
        }
        if take_flag(id, "release") || take_flag(id, "stop") {
            return None;
        }
        let pinned = load(id).is_some_and(|job| job.pinned);
        if !pinned && started.elapsed() >= Duration::from_secs(idle) {
            return None;
        }
        thread::sleep(Duration::from_millis(500));
    }
}

/// Envoie une réponse ; relance le processus s'il s'était arrêté.
pub fn reply(reference: &str, text: &str) -> Result<String, String> {
    let job = find(reference)?;
    let text = text.trim();
    if text.is_empty() {
        return Err("réponse vide".to_string());
    }
    let state = load_state(&job.id);
    if state.attached {
        return Err(format!(
            "{} est ouverte dans un terminal : répondez-y directement",
            job.name
        ));
    }
    send(&job.id, text).map_err(|error| error.to_string())?;
    if alive(&job.id) {
        let queued = state.state == WORKING && state.waiting_for.is_none();
        return Ok(if queued {
            format!(
                "Réponse mise en file : {} la lira à la fin de son tour",
                job.name
            )
        } else {
            format!("Réponse envoyée à {}", job.name)
        });
    }
    if job.kind == "exec" {
        return Err("une commande shell terminée ne reçoit plus de réponse".to_string());
    }
    update_state(&job.id, |state| {
        state.state = WORKING.to_string();
        state.detail = "Reprise…".to_string();
    });
    spawn_worker(&job.id).map_err(|error| format!("relance impossible : {error}"))?;
    Ok(format!("{} reprend avec votre réponse", job.name))
}

fn kill_process(pid: u32) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let _ = Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .creation_flags(0x0800_0000)
            .output();
    }
    #[cfg(not(windows))]
    {
        let group = Command::new("kill")
            .args(["-TERM", "--", &format!("-{pid}")])
            .output();
        if !group.is_ok_and(|output| output.status.success()) {
            let _ = Command::new("kill")
                .args(["-TERM", &pid.to_string()])
                .output();
        }
    }
}

/// Arrête la session (son processus et ce qu'il a lancé).
pub fn stop(reference: &str) -> Result<String, String> {
    let job = find(reference)?;
    let state = load_state(&job.id);
    let running = alive(&job.id);
    if running {
        set_flag(&job.id, "stop");
        if let Some(pid) = state.pid {
            kill_process(pid);
        }
        let _ = fs::remove_file(job_dir(&job.id).join("heartbeat"));
    }
    update_state(&job.id, |state| {
        if running || state.state == WORKING || state.state == BLOCKED {
            state.state = STOPPED.to_string();
            state.waiting_for = None;
            state.question = None;
            state.finished_at = Some(now_ms());
        }
        state.pid = None;
    });
    Ok(if running {
        format!("{} ({}) arrêtée", job.name, job.id)
    } else {
        format!(
            "{} ({}) n'avait pas de processus en cours",
            job.name, job.id
        )
    })
}

/// Retire la session de la liste. Son worktree est supprimé s'il ne contient
/// rien de nouveau ; sinon il reste, avec sa branche. La conversation reste
/// dans `.claw/sessions` (`claw --resume`).
pub fn remove(reference: &str) -> Result<String, String> {
    let job = find(reference)?;
    if alive(&job.id) {
        stop(&job.id)?;
    }
    let mut notes = vec![format!("{} ({}) supprimée", job.name, job.id)];
    if let Some(tree) = &job.worktree {
        let path = Path::new(&tree.path);
        if path.exists() {
            if runtime::worktree::has_changes(path, &tree.base) {
                notes.push(format!(
                    "worktree gardé : {} (branche {}, modifications non fusionnées)",
                    tree.path, tree.branch
                ));
            } else {
                match runtime::worktree::remove(Path::new(&tree.repo), &tree.path, false) {
                    Ok(_) => {
                        let _ = runtime::worktree::git(
                            Path::new(&tree.repo),
                            &["branch", "-D", &tree.branch],
                        );
                        notes.push("worktree sans modification supprimé".to_string());
                    }
                    Err(error) => notes.push(format!("worktree gardé : {error}")),
                }
            }
        }
    }
    fs::remove_dir_all(job_dir(&job.id)).map_err(|error| error.to_string())?;
    Ok(notes.join("\n"))
}

/// Fin de `output.log` (au plus `bytes` octets), en lignes entières.
#[must_use]
pub fn log_tail(id: &str, bytes: u64) -> String {
    let Ok(mut file) = fs::File::open(log_path(id)) else {
        return String::new();
    };
    let length = file.metadata().map_or(0, |metadata| metadata.len());
    let start = length.saturating_sub(bytes);
    if file.seek(SeekFrom::Start(start)).is_err() {
        return String::new();
    }
    let mut buffer = Vec::new();
    let _ = file.read_to_end(&mut buffer);
    let text = String::from_utf8_lossy(&buffer).into_owned();
    if start > 0 {
        text.split_once('\n')
            .map_or(text.clone(), |(_, rest)| rest.to_string())
    } else {
        text
    }
}

/// Dernière action visible d'une session au travail (« ● Commande(cargo test) »).
#[must_use]
pub fn live_detail(id: &str) -> Option<String> {
    let tail = crate::transcript::strip_ansi(&log_tail(id, 16 * 1024));
    tail.lines()
        .rev()
        .map(str::trim)
        .find(|line| line.starts_with('●') || line.starts_with('⎿'))
        .map(|line| line.trim_start_matches(['●', '⎿']).trim().to_string())
        .filter(|line| !line.is_empty())
}

/// Phrase montrée sur la ligne de la session.
#[must_use]
pub fn row_detail(job: &Job, state: &JobState) -> String {
    if state.attached {
        return "Ouverte dans un terminal".to_string();
    }
    let text = match (&state.question, state.state.as_str()) {
        (Some(question), _) if state.waiting_for.is_some() => question.clone(),
        (_, WORKING) => live_detail(&job.id).unwrap_or_else(|| state.detail.clone()),
        _ => state.detail.clone(),
    };
    text.lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or_default()
        .to_string()
}

/// État affiché : une session qui attend une permission ou une réponse est
/// « blocked » même si son état principal dit « working ».
#[must_use]
pub fn effective_state(job: &Job, state: &JobState) -> String {
    let running = alive(&job.id) || state.attached;
    match state.state.as_str() {
        WORKING | BLOCKED if !running => {
            if state.state == BLOCKED {
                BLOCKED.to_string()
            } else {
                // Processus disparu en plein travail (arrêt de la machine…).
                STOPPED.to_string()
            }
        }
        _ if state.waiting_for.is_some() => BLOCKED.to_string(),
        other => other.to_string(),
    }
}

/// « 12 s », « 3 min », « 2 h », « 4 j ».
#[must_use]
pub fn age(since_ms: u64, until_ms: u64) -> String {
    let seconds = until_ms.saturating_sub(since_ms) / 1000;
    match seconds {
        0..=59 => format!("{seconds} s"),
        60..=3_599 => format!("{} min", seconds / 60),
        3_600..=86_399 => format!("{} h", seconds / 3_600),
        _ => format!("{} j", seconds / 86_400),
    }
}

/// Sessions qui attendent une réponse (ligne d'état du terminal).
#[must_use]
pub fn needing_input() -> usize {
    all()
        .iter()
        .filter(|(job, state)| effective_state(job, state) == BLOCKED)
        .count()
}

/// `claw agents --json` : sessions actives (toutes avec `all`), limitées aux
/// dossiers sous `cwd` si demandé.
#[must_use]
pub fn list_json(include_all: bool, cwd: Option<&Path>) -> String {
    let entries: Vec<serde_json::Value> = all()
        .into_iter()
        .filter(|(job, _)| {
            cwd.is_none_or(|cwd| {
                Path::new(&job.origin).starts_with(cwd) || Path::new(&job.cwd).starts_with(cwd)
            })
        })
        .filter_map(|(job, state)| {
            let effective = effective_state(&job, &state);
            let running = alive(&job.id);
            if !include_all && !running && !matches!(effective.as_str(), WORKING | BLOCKED) {
                return None;
            }
            let mut entry = serde_json::json!({
                "id": job.id,
                "name": job.name,
                "kind": "background",
                "cwd": job.cwd,
                "startedAt": job.created_at,
                "state": effective,
                "detail": row_detail(&job, &state),
            });
            if running {
                entry["pid"] = serde_json::json!(state.pid);
                entry["status"] =
                    serde_json::json!(match (effective.as_str(), &state.waiting_for) {
                        (_, Some(_)) => "waiting",
                        (WORKING, None) => "busy",
                        _ => "idle",
                    });
            }
            if let Some(waiting) = &state.waiting_for {
                entry["waitingFor"] = serde_json::json!(waiting);
            }
            if let Some(session) = state.session_path.as_ref().or(job.session_path.as_ref()) {
                entry["sessionPath"] = serde_json::json!(session);
            }
            if job.kind == "exec" {
                entry["command"] = serde_json::json!(job.command);
            }
            Some(entry)
        })
        .collect();
    serde_json::to_string_pretty(&entries).unwrap_or_else(|_| "[]".to_string())
}

/// Ce qui est montré après le lancement (`claw --bg`, `/bg`).
#[must_use]
pub fn launch_report(job: &Job) -> String {
    let mut lines = vec![format!("en arrière-plan · {} · {}", job.id, job.name)];
    if let Some(tree) = &job.worktree {
        lines.push(format!(
            "  worktree         {} (branche {})",
            tree.path, tree.branch
        ));
    }
    lines.extend([
        "  claw agents             voir les sessions".to_string(),
        format!("  claw attach {}    ouvrir dans ce terminal", job.id),
        format!("  claw logs {}      dernières sorties", job.id),
        format!("  claw reply {} …   répondre", job.id),
        format!("  claw stop {}      arrêter", job.id),
    ]);
    lines.join("\n")
}

/// Permission demandée par une session sans terminal : elle attend une
/// réponse (`o` oui, `t` toujours, `n` non, ou une consigne) envoyée par
/// `claw agents`, `claw reply` ou `claw attach`.
pub struct JobPermissionPrompter {
    pub id: String,
}

/// Résumé d'un appel d'outil pour la question posée.
#[must_use]
pub fn describe_call(tool: &str, input: &str) -> String {
    let value = serde_json::from_str::<serde_json::Value>(input).unwrap_or_default();
    let field = |key: &str| {
        value
            .get(key)
            .and_then(serde_json::Value::as_str)
            .map(str::to_string)
    };
    let detail = field("command")
        .or_else(|| field("path"))
        .or_else(|| field("file_path"))
        .or_else(|| field("url"))
        .unwrap_or_else(|| input.chars().take(80).collect());
    let detail: String = detail
        .lines()
        .next()
        .unwrap_or_default()
        .chars()
        .take(100)
        .collect();
    format!("{tool} : {detail}")
}

impl runtime::PermissionPrompter for JobPermissionPrompter {
    fn decide(
        &mut self,
        request: &runtime::PermissionRequest,
    ) -> runtime::PermissionPromptDecision {
        let call = describe_call(&request.tool_name, &request.input);
        let always = if request.suggested_rule.is_some() {
            ", t : toujours"
        } else {
            ""
        };
        let question = format!("Autoriser {call} ? (o : oui{always}, n : non)");
        println!("  ? {question}");
        update_state(&self.id, |state| {
            state.waiting_for = Some("permission prompt".to_string());
            state.question = Some(question.clone());
        });
        let answer = wait_message(&self.id);
        update_state(&self.id, |state| {
            state.waiting_for = None;
            state.question = None;
        });
        let Some(answer) = answer else {
            return runtime::PermissionPromptDecision::Deny {
                reason: "the background session was stopped".to_string(),
            };
        };
        println!("  ⎿  réponse : {}", answer.trim());
        match answer.trim().to_lowercase().as_str() {
            "o" | "oui" | "y" | "yes" | "ok" | "allow" | "autoriser" => {
                runtime::PermissionPromptDecision::Allow
            }
            "t" | "toujours" | "a" | "always" => match &request.suggested_rule {
                Some(rule) => runtime::PermissionPromptDecision::AllowAlways { rule: rule.clone() },
                None => runtime::PermissionPromptDecision::Allow,
            },
            "n" | "non" | "no" | "deny" | "refuser" => runtime::PermissionPromptDecision::Deny {
                reason: "the user denied this call".to_string(),
            },
            other => runtime::PermissionPromptDecision::Deny {
                reason: format!("the user did not allow this call and answered: {other}"),
            },
        }
    }
}

/// Questions de l'agent (`AskUserQuestion`) dans une session sans terminal.
#[must_use]
pub fn question_asker(id: &str) -> tools::SharedQuestionAsker {
    let id = id.to_string();
    Arc::new(move |questions: &[tools::UserQuestion]| {
        let mut text = Vec::new();
        for (index, question) in questions.iter().enumerate() {
            let prefix = if questions.len() > 1 {
                format!("{}. ", index + 1)
            } else {
                String::new()
            };
            text.push(format!("{prefix}{}", question.question));
            for (number, option) in question.options.iter().enumerate() {
                text.push(format!("   {}) {}", number + 1, option.label));
            }
        }
        let rendered = text.join("\n");
        println!("  ? {rendered}");
        update_state(&id, |state| {
            state.waiting_for = Some("input needed".to_string());
            state.question = Some(rendered.clone());
        });
        let answer = wait_message(&id);
        update_state(&id, |state| {
            state.waiting_for = None;
            state.question = None;
        });
        let answer = answer.ok_or_else(|| "la session a été arrêtée".to_string())?;
        println!("  ⎿  réponse : {}", answer.trim());
        let parts: Vec<&str> = if questions.len() > 1 {
            answer.split(['\n', ';']).collect()
        } else {
            vec![answer.as_str()]
        };
        Ok(questions
            .iter()
            .enumerate()
            .map(|(index, question)| {
                let raw = parts.get(index).copied().unwrap_or(answer.as_str()).trim();
                let chosen = raw
                    .parse::<usize>()
                    .ok()
                    .and_then(|number| question.options.get(number.wrapping_sub(1)))
                    .map_or_else(|| raw.to_string(), |option| option.label.clone());
                tools::UserAnswer {
                    question: question.question.clone(),
                    answer: chosen,
                }
            })
            .collect())
    })
}

#[cfg(test)]
mod tests {
    use super::{age, describe_call, name_from_prompt};

    #[test]
    fn names_and_ages_read_well() {
        assert_eq!(
            name_from_prompt("Corrige le test login qui échoue depuis hier soir"),
            "Corrige le test login qui échoue"
        );
        assert_eq!(name_from_prompt("   "), "session");
        assert_eq!(name_from_prompt("\u{feff}Retiens ceci"), "Retiens ceci");
        assert_eq!(age(0, 42_000), "42 s");
        assert_eq!(age(0, 3 * 60_000), "3 min");
        assert_eq!(age(0, 2 * 3_600_000), "2 h");
        assert_eq!(age(0, 3 * 86_400_000), "3 j");
        assert_eq!(
            describe_call("bash", r#"{"command":"cargo test\nsecond"}"#),
            "bash : cargo test"
        );
    }
}
