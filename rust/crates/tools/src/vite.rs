//! Appli Vite du projet (React, le projet de départ de l'outil `Starter`) :
//! sa page `index.html` charge `/src/main.tsx`, qu'un navigateur ne sait pas
//! lire tel quel. Avant de la mesurer (`Screenshot`, `Browser`, `ViewPage`),
//! Claw installe ses bibliothèques s'il le faut, la construit quand ses
//! sources sont plus récentes que la dernière construction, et ouvre la page
//! construite (`docs/index.html`, celle que GitHub Pages publie). Une erreur
//! de construction revient au modèle avec sa sortie, pour qu'il la corrige.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime};

/// Une construction à la fois (deux captures en parallèle n'en lancent qu'une).
static BUILDING: Mutex<()> = Mutex::new(());

/// Installation des bibliothèques : la première fois, tout se télécharge.
const INSTALL_LIMIT: Duration = Duration::from_mins(10);
const BUILD_LIMIT: Duration = Duration::from_mins(5);

/// `npm` (`npm.cmd` sous Windows).
fn npm() -> Command {
    Command::new(if cfg!(windows) { "npm.cmd" } else { "npm" })
}

/// Lance `command` dans `dir`, l'arrête au-delà de `limit` ; rend sa sortie
/// (standard et erreurs), en erreur si elle a échoué.
fn run(mut command: Command, dir: &Path, limit: Duration) -> Result<String, String> {
    let mut child = command
        .current_dir(dir)
        .env("NO_COLOR", "1")
        .env("FORCE_COLOR", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                "Node.js (npm) is not installed on this computer".to_string()
            } else {
                error.to_string()
            }
        })?;
    let read_all = |pipe: Option<Box<dyn Read + Send>>| {
        std::thread::spawn(move || {
            let mut text = String::new();
            if let Some(mut pipe) = pipe {
                let _ = pipe.read_to_string(&mut text);
            }
            text
        })
    };
    let stdout = read_all(
        child
            .stdout
            .take()
            .map(|pipe| Box::new(pipe) as Box<dyn Read + Send>),
    );
    let stderr = read_all(
        child
            .stderr
            .take()
            .map(|pipe| Box::new(pipe) as Box<dyn Read + Send>),
    );
    let deadline = Instant::now() + limit;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if Instant::now() >= deadline || runtime::interrupt::interrupt_requested() => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(100)),
            Err(error) => return Err(error.to_string()),
        }
    };
    let output = format!(
        "{}{}",
        stdout.join().unwrap_or_default(),
        stderr.join().unwrap_or_default()
    );
    match status {
        Some(status) if status.success() => Ok(output),
        Some(_) => Err(tail(&output, 60)),
        None => Err(format!(
            "stopped after {} s{}",
            limit.as_secs(),
            if output.trim().is_empty() {
                String::new()
            } else {
                format!(":\n{}", tail(&output, 30))
            }
        )),
    }
}

/// Les `lines` dernières lignes utiles de `text` : sans couleurs de terminal
/// ni pile d'appels de Node (`at …`), qui cachaient la cause de l'erreur.
fn tail(text: &str, lines: usize) -> String {
    let mut plain = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(char) = chars.next() {
        if char == '\u{1b}' && chars.peek() == Some(&'[') {
            for next in chars.by_ref() {
                if next.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            plain.push(char);
        }
    }
    let kept: Vec<&str> = plain
        .lines()
        .filter(|line| {
            let trimmed = line.trim();
            !trimmed.is_empty()
                && !trimmed.starts_with("at ")
                && !matches!(trimmed, "}" | "errors: [Getter/Setter]")
        })
        .collect();
    kept[kept.len().saturating_sub(lines)..].join("\n")
}

/// Le dossier est une appli Vite : un `package.json` qui dépend de `vite`, et
/// un `index.html` qui charge ses sources (`/src/…`).
pub(crate) fn is_vite_app(dir: &Path) -> bool {
    let Ok(package) = std::fs::read_to_string(dir.join("package.json")) else {
        return false;
    };
    let Ok(package) = serde_json::from_str::<serde_json::Value>(&package) else {
        return false;
    };
    let depends = ["dependencies", "devDependencies"]
        .iter()
        .any(|field| package[field].get("vite").is_some());
    depends
        && std::fs::read_to_string(dir.join("index.html")).is_ok_and(|html| {
            html.contains("src=\"/src/")
                || html.contains("src=\"./src/")
                || html.contains("src=\"src/")
        })
}

/// Le dossier de la construction : `build.outDir` de `vite.config`, sinon `dist`.
fn out_dir(root: &Path) -> String {
    for name in [
        "vite.config.ts",
        "vite.config.js",
        "vite.config.mjs",
        "vite.config.mts",
    ] {
        let Ok(config) = std::fs::read_to_string(root.join(name)) else {
            continue;
        };
        if let Some(at) = config.find("outDir") {
            let rest = &config[at + "outDir".len()..];
            let value = rest
                .trim_start_matches(|c: char| c == ':' || c == '=' || c.is_whitespace())
                .chars()
                .next()
                .filter(|quote| matches!(quote, '"' | '\'' | '`'))
                .and_then(|quote| rest.split(quote).nth(1));
            if let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) {
                return value
                    .trim_start_matches("./")
                    .trim_end_matches('/')
                    .to_string();
            }
        }
        return "dist".to_string();
    }
    "dist".to_string()
}

/// Date de la source la plus récente de l'appli (hors bibliothèques et
/// construction) ; `None` s'il n'y en a pas.
fn newest_source(root: &Path, out: &Path) -> Option<SystemTime> {
    let mut newest = None;
    let mut folders = vec![(root.to_path_buf(), 0)];
    while let Some((folder, depth)) = folders.pop() {
        let Ok(entries) = std::fs::read_dir(&folder) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') || path == out {
                continue;
            }
            if path.is_dir() {
                if depth < 8 && !matches!(name.as_str(), "node_modules" | "dist" | "target") {
                    folders.push((path, depth + 1));
                }
            } else if let Ok(modified) = entry.metadata().and_then(|meta| meta.modified()) {
                if newest.is_none_or(|known| modified > known) {
                    newest = Some(modified);
                }
            }
        }
    }
    newest
}

/// Bibliothèques de l'appli installées (`node_modules/.bin/vite`).
fn installed(root: &Path) -> bool {
    let bin = root.join("node_modules").join(".bin");
    bin.join("vite").is_file() || bin.join("vite.cmd").is_file()
}

/// `npm install` dans `root` ; rend l'erreur d'installation, s'il y en a une.
pub(crate) fn npm_install(root: &Path) -> Result<(), String> {
    let mut command = npm();
    command.args(["install", "--no-audit", "--no-fund", "--loglevel=error"]);
    run(command, root, INSTALL_LIMIT)
        .map(|_| ())
        .map_err(|error| format!("npm install failed in {}: {error}", shown(root)))
}

fn shown(root: &Path) -> String {
    let cwd = std::env::current_dir().ok();
    let relative = cwd
        .as_deref()
        .and_then(|cwd| root.strip_prefix(cwd).ok())
        .map(|path| path.to_string_lossy().replace('\\', "/"))
        .filter(|path| !path.is_empty());
    relative.unwrap_or_else(|| ".".to_string())
}

/// Installe s'il le faut, puis construit l'appli quand ses sources sont plus
/// récentes que sa construction ; rend le dossier de la construction.
pub(crate) fn ensure_built(root: &Path) -> Result<PathBuf, String> {
    let _building = BUILDING
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let out = root.join(out_dir(root));
    if !installed(root) {
        npm_install(root)?;
    }
    let built = std::fs::metadata(out.join("index.html"))
        .and_then(|meta| meta.modified())
        .ok();
    let stale = match (built, newest_source(root, &out)) {
        (Some(built), Some(source)) => source > built,
        (None, _) => true,
        (Some(_), None) => false,
    };
    if stale {
        let mut command = npm();
        command.args(["run", "build"]);
        run(command, root, BUILD_LIMIT).map_err(|error| {
            format!(
                "the app did not build (npm run build in {}), so it cannot be shown. Fix this error, then look again:\n{error}",
                shown(root)
            )
        })?;
    }
    Ok(out)
}

/// La page à ouvrir pour `path` : la page construite quand `path` est la page
/// d'une appli Vite (son `index.html`, son dossier, ou une page de sa
/// construction, reconstruite si ses sources ont changé) ; `path` sinon.
pub(crate) fn built_page(cwd: &Path, path: &Path) -> Result<PathBuf, String> {
    let full = if path.is_absolute() {
        path.to_path_buf()
    } else {
        cwd.join(path)
    };
    let (folder, page) = if full.is_dir() {
        (full.clone(), PathBuf::from("index.html"))
    } else {
        match (full.parent(), full.file_name()) {
            (Some(folder), Some(name)) => (folder.to_path_buf(), PathBuf::from(name)),
            _ => return Ok(path.to_path_buf()),
        }
    };
    // La page source (`index.html` à la racine de l'appli).
    if is_vite_app(&folder) {
        let out = ensure_built(&folder)?;
        let built = out.join(&page);
        return Ok(if built.is_file() {
            built
        } else {
            out.join("index.html")
        });
    }
    // Une page de la construction (`docs/index.html`) : reconstruite si besoin.
    if let Some(root) = folder.parent().filter(|root| is_vite_app(root)) {
        if root.join(out_dir(root)) == folder {
            ensure_built(root)?;
            return Ok(folder.join(page));
        }
    }
    Ok(path.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::{built_page, is_vite_app, newest_source, out_dir};
    use std::path::Path;

    fn project(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("claw-vite-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src")).expect("dir");
        dir
    }

    #[test]
    fn a_vite_app_is_told_from_a_plain_page_and_its_build_folder_is_read() {
        let dir = project("detect");
        std::fs::write(
            dir.join("index.html"),
            "<script type=\"module\" src=\"/src/main.tsx\"></script>",
        )
        .expect("html");
        assert!(!is_vite_app(&dir), "no package.json");
        std::fs::write(
            dir.join("package.json"),
            r#"{"devDependencies":{"vite":"8.3.5"}}"#,
        )
        .expect("package");
        assert!(is_vite_app(&dir));
        assert_eq!(out_dir(&dir), "dist");
        std::fs::write(
            dir.join("vite.config.ts"),
            "export default { build: { outDir: \"docs\", emptyOutDir: true } };",
        )
        .expect("config");
        assert_eq!(out_dir(&dir), "docs");
        std::fs::write(dir.join("index.html"), "<script src=\"app.js\"></script>").expect("html");
        assert!(!is_vite_app(&dir), "a page without sources to build");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_build_error_keeps_its_cause_without_colors_or_stack() {
        let output = "\u{1b}[31m[UNRESOLVED_IMPORT] \u{1b}[0mCould not resolve './Absent' in src/main.tsx\n    at unwrapBindingResult (file:///x/rolldown.mjs:19:128)\n    at async build (file:///x/node.js:1:1) {\n  errors: [Getter/Setter]\n}\n";
        assert_eq!(
            super::tail(output, 40),
            "[UNRESOLVED_IMPORT] Could not resolve './Absent' in src/main.tsx"
        );
    }

    #[test]
    fn a_plain_page_is_opened_as_it_is() {
        let dir = project("plain");
        std::fs::write(dir.join("index.html"), "<h1>Salut</h1>").expect("html");
        assert_eq!(
            built_page(&dir, Path::new("index.html")).expect("page"),
            Path::new("index.html")
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_build_and_the_libraries_do_not_count_as_sources() {
        let dir = project("sources");
        std::fs::create_dir_all(dir.join("docs")).expect("docs");
        std::fs::create_dir_all(dir.join("node_modules/vite")).expect("modules");
        std::fs::write(dir.join("src/main.tsx"), "x").expect("main");
        let source = newest_source(&dir, &dir.join("docs")).expect("source");
        std::thread::sleep(std::time::Duration::from_millis(30));
        std::fs::write(dir.join("docs/index.html"), "built").expect("built");
        std::fs::write(dir.join("node_modules/vite/index.js"), "lib").expect("lib");
        assert_eq!(newest_source(&dir, &dir.join("docs")), Some(source));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
