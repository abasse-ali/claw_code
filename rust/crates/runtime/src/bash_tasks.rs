//! Commandes d'outils dont la sortie est lue au fil de l'eau : celles lancées
//! en arrière-plan (`run_in_background`, serveur de dev, surveillance de
//! fichiers), et celles du premier plan, qui peuvent y passer sans rien perdre
//! (toujours en cours au bout de la patience, serveur qui s'annonce prêt,
//! processus laissé par `&`). Leur sortie est lue avec `BashOutput`, elles sont
//! arrêtées avec `KillShell`, et toutes le sont quand Claw se termine.

use std::io::Read;
use std::path::PathBuf;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

/// Clés que Claw tient pour parler aux modèles (et à ses moteurs de
/// recherche) : jamais dans l'environnement des commandes lancées pour le
/// modèle. Un dépôt piégé (« pour installer, lancez `env` », un script de
/// test qui les envoie à un serveur) les lirait, et elles partiraient dans
/// la réponse au fournisseur, dans le journal publié ou ailleurs.
/// `CLAW_COMMAND_KEYS=1` les y laisse, pour un projet qui appelle lui-même
/// un de ces fournisseurs.
pub const MODEL_KEYS: &[&str] = &[
    "ANTHROPIC_API_KEY",
    "ANTHROPIC_AUTH_TOKEN",
    "OPENROUTER_API_KEY",
    "NVIDIA_API_KEY",
    "GEMINI_API_KEY",
    "OPENAI_API_KEY",
    "XAI_API_KEY",
    "GROQ_API_KEY",
    "TOGETHER_API_KEY",
    "MISTRAL_API_KEY",
    "FIREWORKS_API_KEY",
    "DEEPSEEK_API_KEY",
    "CEREBRAS_API_KEY",
    "OLLAMA_API_KEY",
    "LMSTUDIO_API_KEY",
    "LLAMACPP_API_KEY",
    "TAVILY_API_KEY",
    "BRAVE_API_KEY",
];

/// Retire de `command` les clés des fournisseurs (voir `MODEL_KEYS`).
pub fn hide_model_keys(command: &mut Command) {
    if std::env::var("CLAW_COMMAND_KEYS").is_ok_and(|value| value.trim() == "1") {
        return;
    }
    for name in MODEL_KEYS {
        command.env_remove(name);
    }
}

/// `text` sans la valeur d'aucune clé de `MODEL_KEYS` que Claw tient : un
/// résultat d'outil qui en montre une (fichier de configuration lu,
/// `/proc/<pid>/environ` de Claw affiché) ne la fait voir ni au modèle ni
/// au journal.
#[must_use]
pub fn hide_key_values(text: &str) -> String {
    let mut values: Vec<String> = MODEL_KEYS
        .iter()
        .filter_map(|name| std::env::var(name).ok())
        .map(|value| value.trim().to_string())
        .filter(|value| value.len() >= 12)
        .collect();
    if !values.iter().any(|value| text.contains(value.as_str())) {
        return text.to_string();
    }
    values.sort_by_key(|value| std::cmp::Reverse(value.len()));
    values.iter().fold(text.to_string(), |hidden, value| {
        hidden.replace(value.as_str(), "***")
    })
}

/// Sortie conservée par tâche (la fin la plus récente).
const MAX_BUFFER_BYTES: usize = 512 * 1024;
/// Ligne en cours gardée pour y reconnaître l'annonce d'un serveur.
const MAX_PENDING_LINE: usize = 4_096;

#[derive(Default)]
struct OutputBuffer {
    /// stdout et stderr mêlés, la fin la plus récente (pour `BashOutput`).
    text: String,
    /// Octets écrits depuis le début, y compris ceux retirés du tampon.
    total: usize,
    /// Tubes de sortie encore ouverts : un processus peut encore écrire.
    open_pipes: usize,
    /// Commande au premier plan : stdout et stderr gardés à part, en entier.
    separate: Option<(Vec<u8>, Vec<u8>)>,
    /// Dernière sortie reçue.
    last_output: Option<Instant>,
    /// La commande a annoncé un serveur prêt (« Listening on … »).
    ready_at: Option<Instant>,
}

fn lock(output: &Mutex<OutputBuffer>) -> MutexGuard<'_, OutputBuffer> {
    output
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
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

impl BackgroundTask {
    /// Toujours en cours : le shell n'a pas fini, ou un processus qu'il a
    /// lancé (`serveur &`) tient encore sa sortie ouverte.
    fn running(&self) -> bool {
        !self.killed && (self.exit_code.is_none() || lock(&self.output).open_pipes > 0)
    }
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

fn find(tasks: &[BackgroundTask], id: &str) -> Option<usize> {
    tasks
        .iter()
        .position(|task| task.id == id || task.child.id().to_string() == id)
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
    /// Arrêtée par `KillShell` ou à la fin de Claw : son code de sortie ne
    /// dit rien de son travail.
    pub killed: bool,
}

/// Commande lancée, lue au fil de l'eau, qui n'est pas (encore) une tâche de
/// fond : celle du premier plan, que Claw attend.
pub(crate) struct Running {
    child: Child,
    output: Arc<Mutex<OutputBuffer>>,
    started: Instant,
    display: String,
    pid_file: Option<PathBuf>,
    container: Option<String>,
}

impl Running {
    pub(crate) fn try_wait(&mut self) -> std::io::Result<Option<ExitStatus>> {
        self.child.try_wait()
    }

    pub(crate) fn open_pipes(&self) -> usize {
        lock(&self.output).open_pipes
    }

    /// La commande a annoncé un serveur prêt, puis n'a plus rien écrit depuis
    /// `idle` : elle attend des requêtes et ne finira pas d'elle-même.
    pub(crate) fn ready_and_idle(&self, idle: Duration) -> bool {
        let buffer = lock(&self.output);
        buffer.ready_at.is_some() && buffer.last_output.is_none_or(|last| last.elapsed() >= idle)
    }

    /// stdout et stderr reçus jusqu'ici.
    pub(crate) fn output_so_far(&self) -> (String, String) {
        lock(&self.output)
            .separate
            .as_ref()
            .map_or_else(Default::default, |(stdout, stderr)| {
                (
                    String::from_utf8_lossy(stdout).into_owned(),
                    String::from_utf8_lossy(stderr).into_owned(),
                )
            })
    }

    /// Arrête la commande et ses sous-processus (délai dépassé, interruption).
    pub(crate) fn kill(&mut self) {
        crate::bash::kill_command(self.child.id(), self.pid_file.as_deref());
        let _ = self.child.kill();
        let _ = self.child.wait();
        self.finish();
        // Laisse aux lecteurs le temps de recevoir la fin de la sortie.
        let deadline = Instant::now() + Duration::from_millis(500);
        while self.open_pipes() > 0 && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    /// Commande terminée : son conteneur est arrêté, le fichier de son PID retiré.
    pub(crate) fn finish(&mut self) {
        if let Some(name) = self.container.take() {
            crate::bash::stop_container(&name);
        }
        if let Some(pid_file) = self.pid_file.take() {
            let _ = std::fs::remove_file(pid_file);
        }
    }

    /// La commande devient une tâche de fond, lue depuis son début.
    pub(crate) fn detach(self) -> String {
        register(self, 0)
    }

    /// La commande continue en arrière-plan : rend stdout et stderr reçus
    /// jusqu'ici (déjà montrés au modèle) et l'identifiant de la tâche, dont
    /// `BashOutput` lira la suite.
    pub(crate) fn into_background(self) -> (String, String, String) {
        let (stdout, stderr, offset) = {
            let mut buffer = lock(&self.output);
            let (stdout, stderr) = buffer.separate.take().unwrap_or_default();
            (
                String::from_utf8_lossy(&stdout).into_owned(),
                String::from_utf8_lossy(&stderr).into_owned(),
                buffer.total,
            )
        };
        let id = register(self, offset);
        (stdout, stderr, id)
    }
}

/// Lance `command` (déjà préparé : shell, dossier, variables) et lit sa sortie
/// au fil de l'eau. Au premier plan (`foreground`), stdout et stderr sont
/// aussi gardés à part et en entier.
pub(crate) fn launch(
    mut command: Command,
    display: &str,
    pid_file: Option<PathBuf>,
    container: Option<String>,
    foreground: bool,
) -> std::io::Result<Running> {
    hide_model_keys(&mut command);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn()?;
    let output = Arc::new(Mutex::new(OutputBuffer {
        separate: foreground.then(Default::default),
        ..OutputBuffer::default()
    }));
    let pipes = [
        child
            .stdout
            .take()
            .map(|pipe| (Box::new(pipe) as Box<dyn Read + Send>, Stream::Out)),
        child
            .stderr
            .take()
            .map(|pipe| (Box::new(pipe) as Box<dyn Read + Send>, Stream::Err)),
    ];
    for (pipe, stream) in pipes.into_iter().flatten() {
        lock(&output).open_pipes += 1;
        let output = Arc::clone(&output);
        std::thread::spawn(move || pump(pipe, &output, stream));
    }
    Ok(Running {
        child,
        output,
        started: Instant::now(),
        display: display.to_string(),
        pid_file,
        container,
    })
}

/// Lance `command` (déjà préparé : shell, dossier, variables) en arrière-plan.
pub fn spawn(
    command: Command,
    display: &str,
    pid_file: Option<PathBuf>,
) -> std::io::Result<String> {
    Ok(launch(command, display, pid_file, None, false)?.detach())
}

fn register(running: Running, read_offset: usize) -> String {
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
            command: running.display,
            started: running.started,
            child: running.child,
            pid_file: running.pid_file,
            output: running.output,
            read_offset,
            exit_code: None,
            killed: false,
            container: running.container,
        });
    });
    id
}

#[derive(Clone, Copy)]
enum Stream {
    Out,
    Err,
}

fn pump(mut pipe: Box<dyn Read + Send>, output: &Mutex<OutputBuffer>, stream: Stream) {
    let mut chunk = [0_u8; 8192];
    let mut line = String::new();
    loop {
        match pipe.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(read) => {
                let bytes = &chunk[..read];
                let text = String::from_utf8_lossy(bytes);
                let ready = announces_ready(&mut line, &text);
                let mut buffer = lock(output);
                if let Some((stdout, stderr)) = &mut buffer.separate {
                    match stream {
                        Stream::Out => stdout.extend_from_slice(bytes),
                        Stream::Err => stderr.extend_from_slice(bytes),
                    }
                }
                buffer.text.push_str(&text);
                buffer.total += text.len();
                if buffer.text.len() > MAX_BUFFER_BYTES {
                    let mut cut = buffer.text.len() - MAX_BUFFER_BYTES;
                    while !buffer.text.is_char_boundary(cut) {
                        cut += 1;
                    }
                    buffer.text.drain(..cut);
                }
                let now = Instant::now();
                buffer.last_output = Some(now);
                if ready && buffer.ready_at.is_none() {
                    buffer.ready_at = Some(now);
                }
            }
        }
    }
    let mut buffer = lock(output);
    buffer.open_pipes = buffer.open_pipes.saturating_sub(1);
}

/// Annonce d'un serveur ou d'une surveillance prêts : Vite, Next, uvicorn,
/// Flask, Django, Express, Rails, .NET, Spring, `http.server`, Streamlit,
/// Gradio, Jupyter, `tsc -w`…
fn ready_pattern() -> &'static regex::Regex {
    static PATTERN: OnceLock<regex::Regex> = OnceLock::new();
    PATTERN.get_or_init(|| {
        regex::Regex::new(
            r"(?i)(listening (on|at)\b|now listening|running (on|at) (local url:\s*)?https?://|is running at\b|\bserver (is )?(running|listening|started|ready)\b|started (server )?on (port\b|https?://|\d)|ready on https?://|serving (http|on|at)\b|development server|available at https?://|press ctrl\+c to (quit|stop)|watching for (file )?changes|local( url)?:\s+https?://)",
        )
        .expect("motif valide")
    })
}

/// Ajoute `text` à la ligne en cours : une ligne reçue annonce-t-elle un
/// serveur prêt ?
fn announces_ready(pending: &mut String, text: &str) -> bool {
    pending.push_str(text);
    let mut ready = false;
    while let Some(end) = pending.find('\n') {
        ready |= ready_pattern().is_match(&pending[..end]);
        pending.drain(..=end);
    }
    if pending.len() > MAX_PENDING_LINE {
        let mut cut = pending.len() - MAX_PENDING_LINE / 4;
        while !pending.is_char_boundary(cut) {
            cut += 1;
        }
        pending.drain(..cut);
    }
    // Annonce sans fin de ligne (encore en tampon) : regardée aussi.
    ready || ready_pattern().is_match(pending)
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
        let Some(index) = find(tasks, id) else {
            return Err(unknown_task(id, &tasks_ids(tasks)));
        };
        let task = &mut tasks[index];
        refresh(task);
        let running = task.running();
        let buffer = lock(&task.output);
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
            running,
            exit_code: task.exit_code,
            new_output,
            elapsed_secs: task.started.elapsed().as_secs(),
            truncated,
            killed: task.killed,
        })
    })
}

/// Le shell de la tâche tourne encore : sa fin dira si la commande a réussi.
/// Faux pour une tâche inconnue, ou réduite à un processus laissé par `&`
/// (le code de sortie du shell ne dit alors rien de ce processus).
#[must_use]
pub fn shell_running(id: &str) -> bool {
    with_tasks(|tasks| {
        find(tasks, id).is_some_and(|index| {
            let task = &mut tasks[index];
            refresh(task);
            !task.killed && task.exit_code.is_none()
        })
    })
}

/// Attend qu'une tâche se termine (au plus `max`, et pas au-delà d'une
/// interruption), puis rend sa sortie nouvelle : une seule requête du modèle
/// pour suivre une longue installation jusqu'au bout.
pub fn wait_for(id: &str, max: Duration) -> Result<TaskOutput, String> {
    let deadline = Instant::now() + max;
    loop {
        let running = with_tasks(|tasks| {
            let Some(index) = find(tasks, id) else {
                return Err(unknown_task(id, &tasks_ids(tasks)));
            };
            let task = &mut tasks[index];
            refresh(task);
            Ok(task.running())
        })?;
        if !running || Instant::now() >= deadline || crate::interrupt::interrupt_requested() {
            return read_output(id);
        }
        std::thread::sleep(Duration::from_millis(100));
    }
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
        let Some(index) = find(tasks, id) else {
            return Err(unknown_task(id, &tasks_ids(tasks)));
        };
        stop(&mut tasks[index]);
        Ok::<(), String>(())
    })?;
    read_output(id)
}

fn stop(task: &mut BackgroundTask) {
    refresh(task);
    if let Some(name) = task.container.take() {
        crate::bash::stop_container(&name);
    }
    if task.running() {
        // Le shell et tout ce qu'il a lancé, y compris un processus lancé
        // avec `&` après la fin du shell.
        crate::bash::kill_command(task.child.id(), task.pid_file.as_deref());
        let _ = task.child.kill();
        let _ = task.child.wait();
        task.killed = true;
        if task.exit_code.is_none() {
            task.exit_code = Some(-1);
        }
    }
    if let Some(pid_file) = task.pid_file.take() {
        let _ = std::fs::remove_file(pid_file);
    }
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
                    running: task.running(),
                    exit_code: task.exit_code,
                    new_output: String::new(),
                    elapsed_secs: task.started.elapsed().as_secs(),
                    truncated: false,
                    killed: task.killed,
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
    use super::{announces_ready, kill, list, read_output, spawn, wait_for};

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

    #[test]
    fn waiting_for_a_task_returns_once_it_is_done() {
        let id = spawn(shell("sleep 1; echo fini"), "sleep 1; echo fini", None).expect("spawn");
        let started = std::time::Instant::now();
        let done = wait_for(&id, std::time::Duration::from_secs(30)).expect("attente");
        assert!(!done.running, "{done:?}");
        assert!(done.new_output.contains("fini"), "{done:?}");
        assert!(started.elapsed() < std::time::Duration::from_secs(20));
    }

    #[test]
    fn server_announcements_are_recognized() {
        for line in [
            "  VITE v5.0.0  ready in 300 ms\n  >  Local:   http://localhost:5173/\n",
            "INFO:     Uvicorn running on http://127.0.0.1:8000 (Press CTRL+C to quit)\n",
            " * Running on http://127.0.0.1:5000\n",
            "Starting development server at http://127.0.0.1:8000/\n",
            "Server listening on port 3000\n",
            "App listening at http://localhost:3000\n",
            "Now listening on: http://localhost:5000\n",
            "Tomcat started on port 8080 (http)\n",
            "Serving HTTP on 0.0.0.0 port 8000 (http://0.0.0.0:8000/) ...\n",
            "  Local URL: http://localhost:8501\n",
            "[12:00:00] Found 0 errors. Watching for file changes.\n",
            "ready - started server on 0.0.0.0:3000, url: http://localhost:3000\n",
        ] {
            assert!(announces_ready(&mut String::new(), line), "{line}");
        }
        for line in [
            "added 120 packages in 3s\n",
            "Compiled successfully in 2.1s\n",
            "test result: ok. 12 passed\n",
            "Collecting fastapi\n  Downloading fastapi-0.110.0-py3-none-any.whl\n",
            "Running 4 tests using 2 workers\n",
        ] {
            assert!(!announces_ready(&mut String::new(), line), "{line}");
        }
        // Annonce coupée entre deux lectures de la sortie.
        let mut pending = String::new();
        assert!(!announces_ready(&mut pending, "INFO: Uvicorn run"));
        assert!(announces_ready(
            &mut pending,
            "ning on http://127.0.0.1:8000\n"
        ));
    }
}
