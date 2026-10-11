//! Résolution du shell POSIX utilisé par l'outil bash, les hooks et les plugins.
//!
//! Sous Unix, on garde `sh -lc`. Sous Windows, `sh` n'est généralement pas dans
//! le PATH : on cherche le bash de Git for Windows (ou un bash MSYS2/Cygwin),
//! en évitant le lanceur WSL de System32 qui exécuterait la commande hors du
//! système de fichiers Windows. `CLAW_SHELL` permet d'imposer un chemin.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

/// Programme du shell POSIX, ou `None` si aucun n'a été trouvé (Windows seulement).
#[must_use]
pub fn posix_shell_program() -> Option<&'static Path> {
    static SHELL: OnceLock<Option<PathBuf>> = OnceLock::new();
    SHELL.get_or_init(resolve_posix_shell).as_deref()
}

/// Programme et arguments pour exécuter `command` dans le shell POSIX.
/// Retombe sur `sh` si aucun shell n'a été trouvé : l'erreur de lancement
/// sera alors explicite (voir [`missing_shell_message`]).
#[must_use]
pub fn posix_shell_invocation(command: &str) -> (PathBuf, Vec<String>) {
    let program = posix_shell_program().map_or_else(|| PathBuf::from("sh"), Path::to_path_buf);
    // Sous Windows, un shell de connexion (-l) peut changer de répertoire courant.
    let flag = if cfg!(windows) { "-c" } else { "-lc" };
    (program, vec![flag.to_string(), command.to_string()])
}

/// `Command` prête à exécuter `command` dans le shell POSIX.
#[must_use]
pub fn posix_shell_command(command: &str) -> Command {
    let (program, args) = posix_shell_invocation(command);
    let mut process = Command::new(program);
    process.args(args);
    if cfg!(windows) {
        // Empêche les profils MSYS de faire `cd $HOME` au démarrage.
        process.env("CHERE_INVOKING", "1");
    }
    process
}

/// Commande pour un hook ou une étape de cycle de vie : un script existant est
/// passé directement au shell, sinon la chaîne est interprétée par `-c`.
/// Sous Windows sans bash disponible, on retombe sur `cmd /C`.
#[must_use]
pub fn script_or_shell_command(command: &str) -> Command {
    if cfg!(windows) && posix_shell_program().is_none() {
        let mut process = Command::new("cmd");
        process.arg("/C").arg(command);
        return process;
    }
    if Path::new(command).is_file() {
        let program = posix_shell_program().map_or_else(|| PathBuf::from("sh"), Path::to_path_buf);
        let mut process = Command::new(program);
        process.arg(command);
        return process;
    }
    posix_shell_command(command)
}

/// `Command` pour lancer un programme de plugin. Sous Windows, un script sans
/// extension exécutable (`.sh`, script avec shebang…) passe par le shell POSIX.
#[must_use]
pub fn program_command(program: &str) -> Command {
    if cfg!(windows) && is_non_native_script(Path::new(program)) {
        if let Some(shell) = posix_shell_program() {
            let mut process = Command::new(shell);
            process.arg(program);
            return process;
        }
    }
    Command::new(program)
}

fn is_non_native_script(path: &Path) -> bool {
    let native = path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            ["exe", "com", "cmd", "bat"]
                .iter()
                .any(|native| extension.eq_ignore_ascii_case(native))
        });
    path.is_file() && !native
}

#[must_use]
pub fn missing_shell_message() -> &'static str {
    "aucun shell POSIX trouvé : installez Git for Windows (https://git-scm.com/download/win) \
     ou définissez CLAW_SHELL avec le chemin de bash.exe"
}

fn resolve_posix_shell() -> Option<PathBuf> {
    if let Some(shell) = std::env::var_os("CLAW_SHELL").filter(|value| !value.is_empty()) {
        return Some(PathBuf::from(shell));
    }
    if cfg!(windows) {
        find_windows_bash()
    } else {
        // bash quand il est installé : les modèles écrivent sa syntaxe (`&>`,
        // `|&`, `[[ ]]`, `pipefail`), que dash, le `sh` de Debian et d'Ubuntu,
        // lit autrement (`cmd &> f` y lance `cmd` en arrière-plan).
        Some(find_in_paths("bash", std::env::var_os("PATH")).unwrap_or_else(|| PathBuf::from("sh")))
    }
}

/// Premier fichier `name` des dossiers de `paths` (au format de PATH).
fn find_in_paths(name: &str, paths: Option<std::ffi::OsString>) -> Option<PathBuf> {
    std::env::split_paths(&paths?)
        .map(|dir| dir.join(name))
        .find(|candidate| candidate.is_file())
}

fn find_windows_bash() -> Option<PathBuf> {
    let path_dirs: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|paths| std::env::split_paths(&paths).collect())
        .unwrap_or_default();
    let mut candidates = Vec::new();

    // Git for Windows : git.exe vit dans <Git>\cmd, <Git>\bin ou <Git>\mingw64\bin,
    // et le bash complet (avec coreutils dans le PATH) dans <Git>\bin\bash.exe.
    for dir in &path_dirs {
        if dir.join("git.exe").is_file() {
            for ancestor in dir.ancestors().skip(1).take(2) {
                candidates.push(ancestor.join("bin").join("bash.exe"));
            }
        }
    }
    for var in ["ProgramW6432", "ProgramFiles", "ProgramFiles(x86)"] {
        if let Some(root) = std::env::var_os(var) {
            candidates.push(PathBuf::from(root).join("Git").join("bin").join("bash.exe"));
        }
    }
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        candidates.push(
            PathBuf::from(local)
                .join("Programs")
                .join("Git")
                .join("bin")
                .join("bash.exe"),
        );
    }
    // bash ou sh déjà dans le PATH (MSYS2, Cygwin…), hors lanceurs WSL.
    for dir in path_dirs.iter().filter(|dir| !is_wsl_launcher_dir(dir)) {
        candidates.push(dir.join("bash.exe"));
        candidates.push(dir.join("sh.exe"));
    }

    candidates.into_iter().find(|candidate| candidate.is_file())
}

fn is_wsl_launcher_dir(dir: &Path) -> bool {
    let lower = dir.to_string_lossy().to_ascii_lowercase();
    let lower = lower.trim_end_matches(['\\', '/']);
    lower.ends_with("system32") || lower.ends_with("windowsapps")
}

#[cfg(test)]
mod tests {
    use super::{find_in_paths, is_wsl_launcher_dir, posix_shell_command, posix_shell_program};
    use std::path::Path;

    #[test]
    fn a_program_is_found_in_the_path_folders() {
        let dir = std::env::temp_dir().join(format!("claw-shell-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dossier");
        std::fs::write(dir.join("bash"), "").expect("fichier");
        let paths = Some(dir.clone().into_os_string());
        assert_eq!(find_in_paths("bash", paths.clone()), Some(dir.join("bash")));
        assert_eq!(find_in_paths("absent", paths), None);
        assert_eq!(find_in_paths("bash", None), None);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn skips_wsl_launcher_directories() {
        assert!(is_wsl_launcher_dir(Path::new(r"C:\Windows\System32")));
        assert!(is_wsl_launcher_dir(Path::new(
            r"C:\Users\me\AppData\Local\Microsoft\WindowsApps\"
        )));
        assert!(!is_wsl_launcher_dir(Path::new(r"C:\Program Files\Git\bin")));
    }

    #[test]
    fn runs_a_posix_command_when_a_shell_is_available() {
        if posix_shell_program().is_none() {
            return;
        }
        let output = posix_shell_command("printf 'hello'")
            .output()
            .expect("le shell doit démarrer");
        assert_eq!(String::from_utf8_lossy(&output.stdout), "hello");
    }
}
