//! Ligne d'état personnalisée (clé `statusLine` des réglages, comme dans
//! Claude Code) : une commande reçoit sur son entrée standard un JSON décrivant
//! la session (modèle, dossier, contexte, coût…) et sa sortie remplace la ligne
//! d'état intégrée sous la saisie.
//!
//! ```json
//! { "statusLine": { "type": "command", "command": "~/.claw/statusline.sh", "timeout": 2 } }
//! ```
//!
//! Une commande en échec, trop lente ou muette laisse la ligne intégrée.

use std::io::{Read as _, Write as _};
use std::process::Stdio;
use std::time::{Duration, Instant};

use runtime::ConfigLoader;
use serde_json::{json, Value};

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_LINES: usize = 3;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusCommand {
    pub command: String,
    pub timeout: Duration,
}

/// Ce que la commande reçoit (champs repris de Claude Code).
#[derive(Debug, Clone, Default)]
pub struct StatusInput {
    pub session_id: String,
    pub transcript_path: String,
    pub cwd: String,
    pub model: String,
    pub permission_mode: String,
    pub output_style: Option<String>,
    pub context_used_percent: Option<usize>,
    pub context_window: Option<u64>,
    pub total_cost_usd: Option<f64>,
    pub goal: Option<String>,
    pub version: String,
}

impl StatusInput {
    #[must_use]
    pub fn to_json(&self) -> Value {
        json!({
            "hook_event_name": "Status",
            "session_id": self.session_id,
            "transcript_path": self.transcript_path,
            "cwd": self.cwd,
            "model": { "id": self.model, "display_name": self.model },
            "workspace": { "current_dir": self.cwd, "project_dir": self.cwd },
            "version": self.version,
            "permission_mode": self.permission_mode,
            "output_style": { "name": self.output_style.as_deref().unwrap_or("default") },
            "context_window": {
                "used_percentage": self.context_used_percent,
                "context_window_size": self.context_window,
            },
            "cost": { "total_cost_usd": self.total_cost_usd },
            "goal": self.goal,
        })
    }
}

/// Commande configurée : objet `{ "type": "command", "command": … }` ou chaîne.
#[must_use]
pub fn configured_command(setting: Option<&Value>) -> Option<StatusCommand> {
    let setting = setting?;
    let (command, timeout) = match setting {
        Value::String(command) => (command.clone(), None),
        Value::Object(object) => {
            let kind = object
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or("command");
            if kind != "command" {
                return None;
            }
            (
                object.get("command")?.as_str()?.to_string(),
                object.get("timeout").and_then(Value::as_f64),
            )
        }
        _ => return None,
    };
    let command = command.trim().to_string();
    if command.is_empty() {
        return None;
    }
    let timeout = timeout
        .filter(|seconds| seconds.is_finite() && *seconds > 0.0)
        .map_or(DEFAULT_TIMEOUT, |seconds| {
            Duration::from_secs_f64(seconds.min(30.0))
        });
    Some(StatusCommand { command, timeout })
}

/// Commande des réglages fusionnés du dossier courant.
#[must_use]
pub fn load_command() -> Option<StatusCommand> {
    let cwd = std::env::current_dir().ok()?;
    let config = ConfigLoader::default_for(&cwd).load().ok()?;
    let setting = config
        .get("statusLine")
        .map(|value| serde_json::from_str::<Value>(&value.render()).unwrap_or(Value::Null));
    configured_command(setting.as_ref())
}

/// Sortie de la commande (au plus trois lignes), ou `None` en cas d'échec.
#[must_use]
pub fn run(command: &StatusCommand, input: &StatusInput) -> Option<String> {
    let mut process = plugins::script_or_shell_command(&expand_home(&command.command));
    process
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    if cfg!(windows) && std::env::var_os("PYTHONUTF8").is_none() {
        process.env("PYTHONUTF8", "1");
    }
    let mut child = process.spawn().ok()?;
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(input.to_json().to_string().as_bytes());
    }
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                if !status.success() {
                    return None;
                }
                break;
            }
            Ok(None) if started.elapsed() < command.timeout => {
                std::thread::sleep(Duration::from_millis(15));
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
    let mut output = String::new();
    child.stdout.take()?.read_to_string(&mut output).ok()?;
    shape(&output)
}

/// Lignes non vides de la sortie, au plus trois, chacune avec deux espaces
/// devant comme la ligne intégrée.
fn shape(output: &str) -> Option<String> {
    let lines: Vec<String> = output
        .lines()
        .map(str::trim_end)
        .filter(|line| !line.trim().is_empty())
        .take(MAX_LINES)
        .map(|line| format!("  {line}"))
        .collect();
    (!lines.is_empty()).then(|| lines.join("\n"))
}

fn expand_home(command: &str) -> String {
    if let Some(rest) = command.strip_prefix("~/") {
        if let Some(home) = runtime::home_dir() {
            return format!("{}/{rest}", home.display().to_string().replace('\\', "/"));
        }
    }
    command.to_string()
}

#[cfg(test)]
mod tests {
    use super::{configured_command, run, shape, StatusCommand, StatusInput};
    use serde_json::json;
    use std::time::Duration;

    #[test]
    fn the_setting_takes_an_object_or_a_string() {
        let command = configured_command(Some(&json!({
            "type": "command",
            "command": "echo hi",
            "timeout": 0.5
        })))
        .expect("commande");
        assert_eq!(command.command, "echo hi");
        assert_eq!(command.timeout, Duration::from_millis(500));
        assert_eq!(
            configured_command(Some(&json!("printf ok"))).map(|command| command.command),
            Some("printf ok".to_string())
        );
        assert_eq!(configured_command(Some(&json!({ "type": "static" }))), None);
        assert_eq!(configured_command(Some(&json!({ "command": "  " }))), None);
        assert_eq!(configured_command(None), None);
    }

    #[test]
    fn the_output_keeps_three_non_empty_lines() {
        assert_eq!(shape("\n\n"), None);
        assert_eq!(
            shape("un\n\ndeux\ntrois\nquatre\n").as_deref(),
            Some("  un\n  deux\n  trois")
        );
    }

    #[test]
    fn the_command_reads_the_session_on_its_input() {
        if cfg!(windows) && plugins::posix_shell_program().is_none() {
            return;
        }
        let input = StatusInput {
            model: "qwen/qwen3-coder:free".to_string(),
            ..StatusInput::default()
        };
        let command = StatusCommand {
            command: "cat".to_string(),
            timeout: Duration::from_secs(10),
        };
        let line = run(&command, &input).expect("sortie");
        assert!(line.contains("qwen/qwen3-coder:free"));
        let failing = StatusCommand {
            command: "exit 3".to_string(),
            timeout: Duration::from_secs(10),
        };
        assert_eq!(run(&failing, &input), None);
    }
}
