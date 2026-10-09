//! Carte du projet donnée au modèle en début de session : les fichiers, par
//! dossier, et les principaux symboles (fonctions, types) des fichiers
//! sources les plus importants. Un petit modèle perd beaucoup d'étapes à
//! chercher où sont les choses ; la carte lui évite de partir à l'aveugle.
//!
//! La carte tient dans un budget de caractères (`CLAW_REPO_MAP_CHARS`, 6000
//! par défaut) : les dossiers profonds sont résumés par un simple compte.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::OnceLock;

use regex::Regex;

const DEFAULT_BUDGET: usize = 6_000;
const MAX_FILES: usize = 4_000;
const MAX_FILE_BYTES: u64 = 200 * 1024;
const NAMES_PER_DIR: usize = 14;
const SYMBOLS_PER_FILE: usize = 14;
const SYMBOL_FILES: usize = 60;

const SKIPPED_DIRS: &[&str] = &[
    ".git",
    "node_modules",
    "target",
    ".venv",
    "venv",
    "__pycache__",
    "dist",
    "build",
    ".claw",
    ".idea",
    ".vscode",
    ".next",
    ".cache",
    "vendor",
    "coverage",
];

const SKIPPED_EXTENSIONS: &[&str] = &[
    "png", "jpg", "jpeg", "gif", "ico", "bmp", "webp", "pdf", "zip", "gz", "tar", "7z", "exe",
    "dll", "so", "dylib", "bin", "woff", "woff2", "ttf", "otf", "mp3", "mp4", "mov", "lock", "map",
    "pyc", "class", "o", "a", "vsix", "wasm",
];

fn extension(path: &str) -> String {
    path.rsplit_once('.')
        .map(|(_, extension)| extension.to_ascii_lowercase())
        .unwrap_or_default()
}

fn is_listed(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    !SKIPPED_EXTENSIONS.contains(&extension(path).as_str())
        && !name.ends_with(".min.js")
        && !name.ends_with(".min.css")
        && !path.split('/').any(|part| SKIPPED_DIRS.contains(&part))
}

/// Fichiers du projet, chemins relatifs avec `/` : ceux que git connaît
/// (suivis ou nouveaux, hors `.gitignore`), sinon un parcours du dossier.
fn project_files(root: &Path) -> Vec<String> {
    let from_git = Command::new("git")
        .args(["ls-files", "--cached", "--others", "--exclude-standard"])
        .current_dir(root)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| {
            String::from_utf8_lossy(&output.stdout)
                .lines()
                .map(|line| line.trim().replace('\\', "/"))
                .filter(|line| !line.is_empty())
                .collect::<Vec<_>>()
        })
        .filter(|files| !files.is_empty());
    let mut files = from_git.unwrap_or_else(|| walk(root));
    files.retain(|path| is_listed(path));
    files.sort();
    files.truncate(MAX_FILES);
    files
}

fn walk(root: &Path) -> Vec<String> {
    let mut files = Vec::new();
    let mut pending = vec![(root.to_path_buf(), 0usize)];
    while let Some((dir, depth)) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            if path.is_dir() {
                if depth < 8 && !SKIPPED_DIRS.contains(&name.as_str()) {
                    pending.push((path, depth + 1));
                }
            } else if let Ok(relative) = path.strip_prefix(root) {
                files.push(relative.to_string_lossy().replace('\\', "/"));
            }
            if files.len() >= MAX_FILES {
                return files;
            }
        }
    }
    files
}

fn symbol_patterns(extension: &str) -> Option<&'static [Regex]> {
    static RUST: OnceLock<Vec<Regex>> = OnceLock::new();
    static PYTHON: OnceLock<Vec<Regex>> = OnceLock::new();
    static SCRIPT: OnceLock<Vec<Regex>> = OnceLock::new();
    static GO: OnceLock<Vec<Regex>> = OnceLock::new();
    static OTHER: OnceLock<Vec<Regex>> = OnceLock::new();
    let compile = |patterns: &[&str]| {
        patterns
            .iter()
            .filter_map(|pattern| Regex::new(pattern).ok())
            .collect::<Vec<_>>()
    };
    let patterns = match extension {
        "rs" => RUST.get_or_init(|| {
            compile(&[
                r"^(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?(?:unsafe\s+)?(fn|struct|enum|trait|mod|type)\s+([A-Za-z_][A-Za-z0-9_]*)",
                r"^(impl)(?:<[^>]*>)?\s+(?:[A-Za-z_][A-Za-z0-9_:<>, ]*\s+for\s+)?([A-Za-z_][A-Za-z0-9_]*)",
            ])
        }),
        "py" => PYTHON.get_or_init(|| compile(&[r"^(class|def|async def)\s+([A-Za-z_][A-Za-z0-9_]*)"])),
        "js" | "jsx" | "ts" | "tsx" | "mjs" | "cjs" => SCRIPT.get_or_init(|| {
            compile(&[
                r"^(?:export\s+)?(?:default\s+)?(?:async\s+)?(function|class|interface|type|enum)\*?\s+([A-Za-z_$][A-Za-z0-9_$]*)",
                r"^(?:export\s+)?(const)\s+([A-Za-z_$][A-Za-z0-9_$]*)\s*=\s*(?:async\s*)?(?:\(|function)",
            ])
        }),
        "go" => GO.get_or_init(|| {
            compile(&[
                r"^(func)\s+(?:\([^)]*\)\s*)?([A-Za-z_][A-Za-z0-9_]*)",
                r"^(type)\s+([A-Za-z_][A-Za-z0-9_]*)",
            ])
        }),
        "java" | "kt" | "cs" | "swift" | "php" | "rb" | "scala" | "dart" => OTHER.get_or_init(|| {
            compile(&[
                r"^\s{0,4}(?:(?:public|private|protected|internal|abstract|final|static|data|sealed|open|export)\s+)*(class|interface|enum|struct|record|object|trait|module|def|func|fun)\s+([A-Za-z_][A-Za-z0-9_]*)",
            ])
        }),
        _ => return None,
    };
    Some(patterns.as_slice())
}

/// Symboles de premier niveau d'un fichier source : « fn main », « class Panier ».
fn symbols(path: &str, text: &str) -> Vec<String> {
    let Some(patterns) = symbol_patterns(&extension(path)) else {
        return Vec::new();
    };
    let mut found: Vec<String> = Vec::new();
    for line in text.lines() {
        for pattern in patterns {
            if let Some(captures) = pattern.captures(line) {
                let symbol = format!("{} {}", &captures[1], &captures[2]);
                if !found.contains(&symbol) {
                    found.push(symbol);
                }
                break;
            }
        }
    }
    found
}

/// Les fichiers d'entrée et proches de la racine d'abord, tests en dernier.
fn importance(path: &str) -> (u8, usize, String) {
    let name = path.rsplit('/').next().unwrap_or(path).to_ascii_lowercase();
    let stem = name.split('.').next().unwrap_or(&name).to_string();
    let lower = path.to_ascii_lowercase();
    let is_test = lower.contains("test") || lower.contains("spec") || lower.contains("bench_tasks");
    let is_entry = matches!(
        stem.as_str(),
        "main" | "lib" | "index" | "app" | "mod" | "__init__" | "server" | "cli"
    );
    let rank = match (is_test, is_entry) {
        (true, _) => 3,
        (false, true) => 0,
        (false, false) => 1,
    };
    (rank, path.matches('/').count(), path.to_string())
}

fn budget() -> usize {
    std::env::var("CLAW_REPO_MAP_CHARS")
        .ok()
        .and_then(|value| value.trim().parse::<usize>().ok())
        .filter(|value| *value >= 500)
        .unwrap_or(DEFAULT_BUDGET)
}

/// Carte du projet situé dans `root`, ou `None` s'il n'y a rien à montrer.
#[must_use]
pub fn build(root: &Path) -> Option<String> {
    render(root, &project_files(root), budget())
}

fn render(root: &Path, files: &[String], budget: usize) -> Option<String> {
    if files.is_empty() {
        return None;
    }
    let mut by_dir: BTreeMap<String, Vec<&str>> = BTreeMap::new();
    for path in files {
        let (dir, name) = path.rsplit_once('/').unwrap_or(("", path));
        by_dir.entry(dir.to_string()).or_default().push(name);
    }

    let mut out = format!(
        "# Project map\nGenerated by Claw from the files on disk ({} files). Paths are relative to the working directory; pass them as they are to the file tools. The map can be incomplete or slightly out of date: confirm with glob_search, grep_search or read_file before relying on a detail.\n\n## Files",
        files.len()
    );
    // Les dossiers proches de la racine d'abord ; la moitié du budget au plus.
    let mut dirs: Vec<(&String, &Vec<&str>)> = by_dir.iter().collect();
    dirs.sort_by_key(|(dir, _)| {
        (
            dir.matches('/').count() + usize::from(!dir.is_empty()),
            (*dir).clone(),
        )
    });
    let files_budget = budget / 2;
    let mut hidden_dirs = 0usize;
    let mut hidden_files = 0usize;
    for (dir, names) in dirs {
        if out.len() > files_budget {
            hidden_dirs += 1;
            hidden_files += names.len();
            continue;
        }
        let label = if dir.is_empty() { "." } else { dir.as_str() };
        let shown = names
            .iter()
            .take(NAMES_PER_DIR)
            .copied()
            .collect::<Vec<_>>()
            .join(", ");
        let more = names.len().saturating_sub(NAMES_PER_DIR);
        let _ = write!(out, "\n{label}/: {shown}");
        if more > 0 {
            let _ = write!(out, " (+{more} more)");
        }
    }
    if hidden_dirs > 0 {
        let _ = write!(
            out,
            "\n… {hidden_dirs} deeper directories with {hidden_files} files are not listed (use glob_search)."
        );
    }

    // Symboles des fichiers sources les plus importants, tant qu'il reste du budget.
    let mut sources: Vec<&String> = files
        .iter()
        .filter(|path| symbol_patterns(&extension(path)).is_some())
        .collect();
    sources.sort_by_key(|path| importance(path));
    let mut section = String::new();
    for path in sources.into_iter().take(SYMBOL_FILES) {
        if out.len() + section.len() > budget {
            break;
        }
        let full = root.join(path);
        if full
            .metadata()
            .map_or(true, |metadata| metadata.len() > MAX_FILE_BYTES)
        {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&full) else {
            continue;
        };
        let found = symbols(path, &text);
        if found.is_empty() {
            continue;
        }
        let more = found.len().saturating_sub(SYMBOLS_PER_FILE);
        let _ = write!(
            section,
            "\n{path} ({} lines): {}",
            text.lines().count(),
            found
                .iter()
                .take(SYMBOLS_PER_FILE)
                .cloned()
                .collect::<Vec<_>>()
                .join(", ")
        );
        if more > 0 {
            let _ = write!(section, " (+{more} more)");
        }
    }
    if !section.is_empty() {
        out.push_str("\n\n## Main symbols");
        out.push_str(&section);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::{is_listed, render, symbols};
    use std::fs;

    #[test]
    fn extracts_top_level_symbols_per_language() {
        let rust = "use std::io;\n\npub struct Config {\n    name: String,\n}\n\nimpl Config {\n    pub fn new() -> Self { todo!() }\n}\n\npub(crate) async fn run() {}\nfn helper() {}\n";
        assert_eq!(
            symbols("src/lib.rs", rust),
            vec!["struct Config", "impl Config", "fn run", "fn helper"]
        );
        let python = "import os\n\nclass Panier:\n    def total(self):\n        pass\n\ndef ajouter(article):\n    pass\n\nasync def charger():\n    pass\n";
        assert_eq!(
            symbols("panier.py", python),
            vec!["class Panier", "def ajouter", "async def charger"]
        );
        let script = "export default function App() {}\nexport const charger = async () => {};\nconst x = 3;\nclass Store {}\nexport interface Props {}\n";
        assert_eq!(
            symbols("src/App.tsx", script),
            vec![
                "function App",
                "const charger",
                "class Store",
                "interface Props"
            ]
        );
        assert!(symbols("README.md", "# Titre").is_empty());
    }

    #[test]
    fn lists_files_by_directory_and_skips_noise() {
        assert!(is_listed("src/main.rs"));
        assert!(!is_listed("assets/logo.png"));
        assert!(!is_listed("node_modules/x/index.js"));
        assert!(!is_listed("Cargo.lock"));

        let root = std::env::temp_dir().join(format!("claw-map-{}", std::process::id()));
        fs::create_dir_all(root.join("src")).expect("dossier");
        fs::write(root.join("src/main.rs"), "fn main() {}\npub fn run() {}\n").expect("fichier");
        fs::write(root.join("src/util.rs"), "pub fn aide() {}\n").expect("fichier");
        fs::write(root.join("README.md"), "# Projet\n").expect("fichier");
        let files = vec![
            "README.md".to_string(),
            "src/main.rs".to_string(),
            "src/util.rs".to_string(),
        ];
        let map = render(&root, &files, 6_000).expect("carte");
        assert!(map.starts_with("# Project map"));
        assert!(map.contains("./: README.md"));
        assert!(map.contains("src/: main.rs, util.rs"));
        assert!(map.contains("src/main.rs (2 lines): fn main, fn run"));
        // Le fichier d'entrée passe avant les autres.
        assert!(map.find("src/main.rs (2 lines)") < map.find("src/util.rs (1 lines)"));

        // Petit budget : la carte reste courte et le dit.
        let many: Vec<String> = (0..400).map(|n| format!("pkg/sous{n}/f{n}.txt")).collect();
        let small = render(&root, &many, 800).expect("carte");
        assert!(small.len() < 1_600);
        assert!(small.contains("deeper directories"));
        assert_eq!(render(&root, &[], 800), None);
        let _ = fs::remove_dir_all(root);
    }
}
