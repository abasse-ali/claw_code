//! Outils de développeur ajoutés le 21 octobre 2026 : lire plusieurs fichiers en un appel
//! (`read_many_files`), le plan d'un fichier (`file_outline`), lancer les tests du projet avec
//! les échecs résumés (`run_tests`), git en lecture (`git`) et la validation (`git_commit`).

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::io;
use std::path::Path;
use std::process::Command;
use std::sync::OnceLock;

use regex::{Regex, RegexBuilder};
use serde::Deserialize;
use serde_json::{json, Value};

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.into())
}

/// Début et fin d'un long texte, avec ce qui manque entre les deux.
fn excerpt(text: &str, limit: usize) -> String {
    let count = text.chars().count();
    if count <= limit {
        return text.to_string();
    }
    let head: String = text.chars().take(limit / 3).collect();
    let tail: String = text.chars().skip(count - limit * 2 / 3).collect();
    format!(
        "{head}\n… [{} characters omitted] …\n{tail}",
        count - head.chars().count() - tail.chars().count()
    )
}

// ============================================================ read_many_files

#[derive(Debug, Deserialize)]
pub struct ReadManyFilesInput {
    pub paths: Vec<String>,
    /// Lignes lues au plus par fichier (400 par défaut).
    #[serde(default)]
    pub max_lines: Option<usize>,
}

const READ_MANY_MAX_FILES: usize = 30;
const READ_MANY_BUDGET: usize = 160_000;

/// Les fichiers demandés (motifs glob compris), chacun comme `read_file` le rendrait.
pub fn read_many_files(input: &ReadManyFilesInput) -> io::Result<Value> {
    if input.paths.is_empty() {
        return Err(invalid(
            "[Claw] read_many_files was not executed: `paths` is empty. Give the files to read, or glob patterns such as src/**/*.ts.",
        ));
    }
    let limit = input.max_lines.unwrap_or(400).clamp(1, 2_000);
    let mut paths: Vec<String> = Vec::new();
    let mut notes: Vec<String> = Vec::new();
    for raw in &input.paths {
        if raw.contains(['*', '?', '[']) {
            match crate::file_ops::glob_search(raw, None) {
                Ok(found) if !found.filenames.is_empty() => {
                    for name in found.filenames {
                        if !paths.contains(&name) {
                            paths.push(name);
                        }
                    }
                }
                Ok(_) => notes.push(format!("no file matches `{raw}`")),
                Err(error) => notes.push(format!("`{raw}`: {error}")),
            }
        } else if !paths.contains(raw) {
            paths.push(raw.clone());
        }
    }
    if paths.len() > READ_MANY_MAX_FILES {
        notes.push(format!(
            "{} files matched: only the first {READ_MANY_MAX_FILES} are read; ask for the others in another call",
            paths.len()
        ));
        paths.truncate(READ_MANY_MAX_FILES);
    }
    let mut files = Vec::new();
    let mut used = 0usize;
    for path in paths {
        if used >= READ_MANY_BUDGET {
            files.push(json!({ "filePath": path, "skipped": "not read: this call already returned enough text; read it in another call" }));
            continue;
        }
        match crate::file_ops::read_file(&path, None, Some(limit)) {
            Ok(output) => {
                let file = output.file;
                used += file.content.len();
                let mut entry = json!({
                    "filePath": file.file_path,
                    "totalLines": file.total_lines,
                    "content": file.content,
                });
                if file.num_lines < file.total_lines {
                    entry["note"] = json!(format!(
                        "lines 1-{} of {}: call read_file with offset {} to read on",
                        file.num_lines, file.total_lines, file.num_lines
                    ));
                } else if let Some(note) = file.note {
                    entry["note"] = json!(note);
                }
                files.push(entry);
            }
            Err(error) => files.push(json!({ "filePath": path, "error": error.to_string() })),
        }
    }
    let mut output = json!({ "files": files });
    if !notes.is_empty() {
        output["notes"] = json!(notes);
    }
    Ok(output)
}

// ============================================================ file_outline

struct Rule {
    pattern: Regex,
    kind: Option<&'static str>,
}

/// Motifs d'une famille de fichiers : chacun capture l'indentation (1), puis le genre (2,
/// sauf `kind` fixé) et le nom (3, ou 2 avec `kind`).
fn rules(extension: &str) -> Option<&'static [Rule]> {
    fn build(list: &[(&str, Option<&'static str>)]) -> Vec<Rule> {
        list.iter()
            .filter_map(|(pattern, kind)| {
                RegexBuilder::new(pattern)
                    .case_insensitive(false)
                    .build()
                    .ok()
                    .map(|pattern| Rule {
                        pattern,
                        kind: *kind,
                    })
            })
            .collect()
    }
    static RUST: OnceLock<Vec<Rule>> = OnceLock::new();
    static PYTHON: OnceLock<Vec<Rule>> = OnceLock::new();
    static SCRIPT: OnceLock<Vec<Rule>> = OnceLock::new();
    static GO: OnceLock<Vec<Rule>> = OnceLock::new();
    static CLIKE: OnceLock<Vec<Rule>> = OnceLock::new();
    static STYLE: OnceLock<Vec<Rule>> = OnceLock::new();
    static MARKUP: OnceLock<Vec<Rule>> = OnceLock::new();
    static MARKDOWN: OnceLock<Vec<Rule>> = OnceLock::new();
    static SQL: OnceLock<Vec<Rule>> = OnceLock::new();
    static YAML: OnceLock<Vec<Rule>> = OnceLock::new();
    static TOML: OnceLock<Vec<Rule>> = OnceLock::new();
    let rules = match extension {
        "rs" => RUST.get_or_init(|| {
            build(&[
                (r#"^(\s*)(?:pub(?:\([^)]*\))?\s+)?(?:(?:async|unsafe|const|extern(?:\s+"[^"]*")?)\s+)*(fn|struct|enum|trait|mod|type|union)\s+([A-Za-z_][A-Za-z0-9_]*)"#, None),
                (r"^(\s*)(?:pub(?:\([^)]*\))?\s+)?(const|static)\s+([A-Z_][A-Z0-9_]*)\s*:", None),
                (r"^(\s*)(macro_rules!)\s*([A-Za-z_][A-Za-z0-9_]*)", None),
                (r"^(\s*)(impl)(?:<[^>]*>)?\s+([^{]+?)\s*(?:where\b.*)?\{?\s*$", None),
            ])
        }),
        "py" | "pyi" => PYTHON.get_or_init(|| {
            build(&[(r"^(\s*)(class|def|async def)\s+([A-Za-z_]\w*)", None)])
        }),
        "js" | "jsx" | "ts" | "tsx" | "mjs" | "cjs" | "mts" | "cts" => SCRIPT.get_or_init(|| {
            build(&[
                (r"^(\s*)(?:export\s+)?(?:default\s+)?(?:declare\s+)?(?:abstract\s+)?(?:async\s+)?(function\*?|class|interface|type|enum|namespace)\s+([A-Za-z_$][\w$]*)", None),
                (r"^(\s*)(?:export\s+)?(?:const|let|var)\s+([A-Za-z_$][\w$]*)\s*(?::[^=]+)?=\s*(?:async\s*)?(?:\([^)]*\)|[A-Za-z_$][\w$]*)\s*(?::[^=]+)?=>", Some("function")),
                (r"^(\s*)(?:export\s+)?(?:const|let|var)\s+([A-Za-z_$][\w$]*)\s*=\s*(?:async\s+)?function\b", Some("function")),
                (r"^(\s+)(?:(?:public|private|protected|static|async|readonly|override|get|set)\s+)*([A-Za-z_$#][\w$]*)\s*(?:<[^>]*>)?\([^)]*\)\s*(?::\s*[^{]+)?\{\s*$", Some("method")),
            ])
        }),
        "go" => GO.get_or_init(|| {
            build(&[
                (r"^(\s*)(func)\s+(?:\([^)]*\)\s*)?([A-Za-z_]\w*)", None),
                (r"^(\s*)(type)\s+([A-Za-z_]\w*)", None),
            ])
        }),
        "java" | "kt" | "kts" | "cs" | "swift" | "php" | "rb" | "scala" | "dart" | "c" | "h"
        | "cc" | "cpp" | "hpp" | "cxx" | "m" | "groovy" => CLIKE.get_or_init(|| {
            build(&[
                (r"^(\s*)(?:(?:public|private|protected|internal|abstract|final|static|data|sealed|open|export|partial|readonly)\s+)*(class|interface|enum|struct|record|object|trait|module|namespace|def|func|fun|function|protocol|extension|impl)\s+([A-Za-z_][\w]*)", None),
                (r"^(\s*)(?:(?:public|private|protected|internal|static|final|abstract|virtual|override|async|synchronized|inline|const|unsafe|extern)\s+)*[\w<>\[\],.*&:?]+\s+\**([A-Za-z_]\w*)\s*\([^;{}]*\)\s*(?:const\s*)?(?:throws\s+[\w., ]+)?\s*\{?\s*$", Some("function")),
            ])
        }),
        "css" | "scss" | "sass" | "less" => STYLE.get_or_init(|| {
            build(&[
                (r"^(\s*)(@(?:media|supports|keyframes|layer|container|font-face|import)\b)\s*([^{;]*)", None),
                (r"^(\s*)([^\s@/{}][^{}]*?)\s*\{\s*$", Some("rule")),
            ])
        }),
        "html" | "htm" | "vue" | "svelte" | "astro" | "jinja" | "j2" | "twig" | "hbs" => MARKUP.get_or_init(|| {
            build(&[
                (r"^(\s*)<(h[1-6])\b[^>]*>\s*([^<]{1,80})", None),
                (r#"^(\s*)<(\w+)\b[^>]*\bid="([^"]+)""#, None),
                (r"^(\s*)<(header|nav|main|section|article|aside|footer|form|template|script|style|dialog|table)\b", Some("element")),
                (r"^(\s*)\{%-?\s*(block|macro)\s+(\w+)", None),
            ])
        }),
        "md" | "markdown" | "mdx" | "rst" => MARKDOWN.get_or_init(|| {
            build(&[(r"^()(#{1,6})\s+(.+?)\s*#*\s*$", None)])
        }),
        "sql" => SQL.get_or_init(|| {
            vec![Rule {
                pattern: RegexBuilder::new(r#"^(\s*)CREATE\s+(?:OR\s+REPLACE\s+)?(?:TEMP(?:ORARY)?\s+)?(TABLE|VIEW|INDEX|UNIQUE INDEX|FUNCTION|PROCEDURE|TRIGGER|TYPE|SCHEMA)\s+(?:IF\s+NOT\s+EXISTS\s+)?([\w."`\[\]]+)"#)
                    .case_insensitive(true)
                    .build()
                    .expect("motif SQL"),
                kind: None,
            }]
        }),
        "yml" | "yaml" => YAML.get_or_init(|| {
            build(&[(r"^( {0,4})([A-Za-z_][\w.-]*):(?:\s|$)", Some("key"))])
        }),
        "toml" | "ini" | "cfg" => TOML.get_or_init(|| {
            build(&[(r"^()\[\[?([^\]]+)\]\]?\s*$", Some("section"))])
        }),
        _ => return None,
    };
    Some(rules.as_slice())
}

#[derive(Debug, Deserialize)]
pub struct FileOutlineInput {
    pub path: String,
}

const OUTLINE_MAX: usize = 400;
const NOT_NAMES: &[&str] = &[
    "if", "for", "while", "switch", "catch", "return", "function", "else", "do", "try", "with",
    "new", "delete", "sizeof", "foreach", "using", "lock", "elif", "await", "typeof",
];

/// Plan d'un fichier : ses définitions (classes, fonctions, méthodes, règles CSS, titres…),
/// avec leur ligne et leur profondeur, sans le lire en entier.
pub fn file_outline(path: &str) -> io::Result<Value> {
    let absolute = crate::file_ops::normalize_path(path)?;
    if absolute.is_dir() {
        return Err(invalid(format!(
            "[Claw] {path} is a directory: file_outline takes one file (list a directory with list_directory)."
        )));
    }
    let (content, _) = crate::file_ops::read_text(&absolute)?;
    let file_path = crate::file_ops::display_path(&absolute);
    let extension = Path::new(&file_path)
        .extension()
        .map(|extension| extension.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    let Some(rules) = rules(&extension) else {
        return Err(invalid(format!(
            "[Claw] file_outline does not know `.{extension}` files: read the file with read_file, or search it with grep_search."
        )));
    };
    let mut found: Vec<(usize, usize, String, String, String)> = Vec::new();
    let mut in_comment = false;
    for (index, line) in content.lines().enumerate() {
        let trimmed = line.trim_start();
        // Blocs de commentaires C, et commentaires d'une ligne : pas de définitions.
        if in_comment {
            if trimmed.contains("*/") {
                in_comment = false;
            }
            continue;
        }
        if trimmed.starts_with("/*") && !trimmed.contains("*/") {
            in_comment = true;
            continue;
        }
        let hash_is_syntax = matches!(
            extension.as_str(),
            "md" | "markdown" | "mdx" | "css" | "scss" | "sass" | "less"
        );
        if trimmed.starts_with("//") || (trimmed.starts_with('#') && !hash_is_syntax) {
            continue;
        }
        for rule in rules {
            let Some(captures) = rule.pattern.captures(line) else {
                continue;
            };
            let indent = captures.get(1).map_or(0, |found| {
                found
                    .as_str()
                    .chars()
                    .map(|ch| if ch == '\t' { 4 } else { 1 })
                    .sum()
            });
            let (kind, name) = match rule.kind {
                Some(kind) => (
                    kind.to_string(),
                    captures
                        .get(2)
                        .map_or("", |found| found.as_str())
                        .to_string(),
                ),
                None => (
                    captures
                        .get(2)
                        .map_or("", |found| found.as_str())
                        .to_string(),
                    captures
                        .get(3)
                        .map_or("", |found| found.as_str())
                        .trim()
                        .to_string(),
                ),
            };
            let kind = if extension == "md" || extension == "markdown" || extension == "mdx" {
                format!("h{}", kind.len())
            } else {
                kind
            };
            // Un mot-clé pris pour un nom (`if (…) {`, `return f(…)`) : pas une définition.
            if NOT_NAMES.contains(&name.as_str()) {
                continue;
            }
            let signature: String = line.trim().chars().take(140).collect();
            found.push((index + 1, indent, kind, name, signature));
            break;
        }
        if found.len() > OUTLINE_MAX {
            break;
        }
    }
    // Profondeur : rang de l'indentation parmi celles du fichier.
    let mut levels: Vec<usize> = found.iter().map(|entry| entry.1).collect();
    levels.sort_unstable();
    levels.dedup();
    let truncated = found.len() > OUTLINE_MAX;
    found.truncate(OUTLINE_MAX);
    let symbols: Vec<Value> = found
        .into_iter()
        .map(|(line, indent, kind, name, signature)| {
            let depth = if kind.starts_with('h') && kind.len() == 2 {
                kind[1..].parse::<usize>().unwrap_or(1) - 1
            } else {
                levels
                    .iter()
                    .position(|level| *level == indent)
                    .unwrap_or(0)
            };
            json!({ "line": line, "depth": depth, "kind": kind, "name": name, "code": signature })
        })
        .collect();
    let mut output = json!({
        "filePath": file_path,
        "totalLines": content.lines().count(),
        "symbols": symbols,
    });
    if truncated {
        output["note"] = json!(format!(
            "only the first {OUTLINE_MAX} definitions are listed"
        ));
    } else if output["symbols"].as_array().is_some_and(Vec::is_empty) {
        output["note"] = json!("no definition found: read the file with read_file");
    }
    Ok(output)
}

// ============================================================ run_tests

#[derive(Debug, Deserialize)]
pub struct RunTestsInput {
    /// Commande des tests ; sans elle, trouvée d'après les fichiers du projet.
    #[serde(default)]
    pub command: Option<String>,
    /// Ne lancer que les tests dont le nom contient ce texte.
    #[serde(default)]
    pub filter: Option<String>,
    /// Dossier du projet à tester (sous-projet), le dossier courant par défaut.
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub timeout_seconds: Option<u64>,
}

/// Une valeur entre apostrophes pour le shell.
fn quoted(value: &str) -> String {
    format!("'{}'", value.replace('\'', r"'\''"))
}

/// Commande de test du projet de `root`, et l'outil qu'elle utilise.
pub fn detect_test_command(root: &Path, filter: Option<&str>) -> Option<(String, &'static str)> {
    let has = |name: &str| root.join(name).exists();
    let filter = filter.map(str::trim).filter(|filter| !filter.is_empty());
    let with = |base: &str, flag: &str| match filter {
        Some(filter) if flag.is_empty() => format!("{base} {}", quoted(filter)),
        Some(filter) => format!("{base} {flag} {}", quoted(filter)),
        None => base.to_string(),
    };
    if has("Cargo.toml") {
        return Some((with("cargo test", ""), "cargo"));
    }
    if has("package.json") {
        let package: Value = std::fs::read_to_string(root.join("package.json"))
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or(Value::Null);
        let script = package["scripts"]["test"].as_str().unwrap_or_default();
        let runner = if has("pnpm-lock.yaml") {
            "pnpm"
        } else if has("yarn.lock") {
            "yarn"
        } else if has("bun.lockb") || has("bun.lock") {
            "bun run"
        } else {
            "npm"
        };
        let depends = |name: &str| {
            package["devDependencies"].get(name).is_some()
                || package["dependencies"].get(name).is_some()
        };
        if !script.is_empty() && !script.contains("no test specified") {
            let tool = if script.contains("vitest") {
                "vitest"
            } else if script.contains("jest") {
                "jest"
            } else if script.contains("mocha") {
                "mocha"
            } else {
                "npm"
            };
            let base = if runner == "npm" {
                "npm test --silent".to_string()
            } else {
                format!("{runner} test")
            };
            let command = match (filter, tool) {
                (Some(filter), "vitest" | "jest") => format!("{base} -- -t {}", quoted(filter)),
                (Some(filter), "mocha") => format!("{base} -- --grep {}", quoted(filter)),
                (Some(filter), _) => format!("{base} -- {}", quoted(filter)),
                (None, _) => base,
            };
            return Some((command, tool));
        }
        if depends("vitest") {
            return Some((with("npx vitest run", "-t"), "vitest"));
        }
        if depends("jest") {
            return Some((with("npx jest", "-t"), "jest"));
        }
    }
    let python = if cfg!(windows) { "python" } else { "python3" };
    let pytest_config = has("pytest.ini")
        || has("conftest.py")
        || std::fs::read_to_string(root.join("pyproject.toml"))
            .is_ok_and(|text| text.contains("pytest"))
        || std::fs::read_to_string(root.join("setup.cfg"))
            .is_ok_and(|text| text.contains("pytest"))
        || std::fs::read_to_string(root.join("tox.ini")).is_ok_and(|text| text.contains("pytest"));
    let python_tests = ["tests", "test"].iter().any(|dir| {
        std::fs::read_dir(root.join(dir)).is_ok_and(|entries| {
            entries.filter_map(Result::ok).any(|entry| {
                let name = entry.file_name().to_string_lossy().to_string();
                name.ends_with(".py") && (name.starts_with("test") || name.ends_with("_test.py"))
            })
        })
    }) || std::fs::read_dir(root).is_ok_and(|entries| {
        entries.filter_map(Result::ok).any(|entry| {
            let name = entry.file_name().to_string_lossy().to_string();
            name.starts_with("test_") && name.ends_with(".py")
        })
    });
    if pytest_config || python_tests {
        return Some((with(&format!("{python} -m pytest -q -rfE"), "-k"), "pytest"));
    }
    if has("go.mod") {
        return Some((with("go test ./...", "-run"), "go"));
    }
    if has("pom.xml") {
        return Some((
            match filter {
                Some(filter) => format!("mvn -q test -Dtest={}", quoted(filter)),
                None => "mvn -q test".to_string(),
            },
            "maven",
        ));
    }
    if has("build.gradle") || has("build.gradle.kts") {
        let base = if has("gradlew") {
            "./gradlew test"
        } else {
            "gradle test"
        };
        return Some((with(base, "--tests"), "gradle"));
    }
    let dotnet = std::fs::read_dir(root).is_ok_and(|entries| {
        entries.filter_map(Result::ok).any(|entry| {
            let name = entry.file_name().to_string_lossy().to_string();
            name.ends_with(".sln") || name.ends_with(".csproj")
        })
    });
    if dotnet {
        return Some((with("dotnet test", "--filter"), "dotnet"));
    }
    if has("composer.json") && has("vendor/bin/phpunit") {
        return Some((with("vendor/bin/phpunit", "--filter"), "phpunit"));
    }
    if has("Gemfile") && has("spec") {
        return Some((with("bundle exec rspec", "-e"), "rspec"));
    }
    let make_test = std::fs::read_to_string(root.join("Makefile"))
        .is_ok_and(|text| text.lines().any(|line| line.starts_with("test:")));
    if make_test {
        return Some(("make test".to_string(), "make"));
    }
    None
}

/// Comptes et échecs lus dans la sortie des tests.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct TestSummary {
    pub passed: Option<u64>,
    pub failed: Option<u64>,
    pub skipped: Option<u64>,
    pub failures: Vec<(String, String)>,
}

fn regex(pattern: &str) -> Regex {
    Regex::new(pattern).expect("motif de test")
}

/// Ce que la sortie dit des tests : nombres de réussis et d'échecs, et pour chaque échec
/// son nom et le début de son message. Formats reconnus : cargo, pytest, Jest, Vitest, Mocha,
/// go test, unittest, et les comptes génériques (« 3 passed, 1 failed »).
#[allow(clippy::too_many_lines)]
pub fn summarize_tests(output: &str) -> TestSummary {
    let mut summary = TestSummary::default();
    let lines: Vec<&str> = output.lines().collect();
    let add = |slot: &mut Option<u64>, value: u64| *slot = Some(slot.unwrap_or(0) + value);
    // cargo : une ligne « test result » par binaire, à additionner.
    let cargo = regex(r"test result: \w+\. (\d+) passed; (\d+) failed; (\d+) ignored");
    let mut cargo_seen = false;
    for line in &lines {
        if let Some(captures) = cargo.captures(line) {
            cargo_seen = true;
            add(&mut summary.passed, captures[1].parse().unwrap_or(0));
            add(&mut summary.failed, captures[2].parse().unwrap_or(0));
            add(&mut summary.skipped, captures[3].parse().unwrap_or(0));
        }
    }
    if cargo_seen {
        let failed_test = regex(r"^test (\S+) \.\.\. FAILED");
        for line in &lines {
            if let Some(captures) = failed_test.captures(line) {
                let name = captures[1].to_string();
                let marker = format!("---- {name} stdout ----");
                let message = lines
                    .iter()
                    .position(|line| line.trim() == marker)
                    .map(|start| {
                        lines[start + 1..]
                            .iter()
                            .take_while(|line| {
                                !line.starts_with("---- ") && !line.starts_with("failures:")
                            })
                            .filter(|line| {
                                !line.trim().is_empty() && !line.starts_with("note: run with")
                            })
                            .take(8)
                            .copied()
                            .collect::<Vec<_>>()
                            .join("\n")
                    })
                    .unwrap_or_default();
                summary.failures.push((name, message));
            }
        }
        return summary;
    }
    // Comptes génériques : la dernière mention de chaque mot (pytest, Jest, Vitest, Mocha…).
    let counts =
        regex(r"(?i)\b(\d+)\s+(passed|passing|failed|failing|errors?|skipped|pending|todo)\b");
    let (mut passed, mut failed, mut errors, mut skipped) = (None, None, None, None);
    for line in &lines {
        // Ligne de résumé seulement : « Tests: 1 failed, 5 passed », « === 2 failed, 3 passed in 0.4s === ».
        let lower = line.to_ascii_lowercase();
        if lower.contains("test suites:") {
            continue;
        }
        for captures in counts.captures_iter(line) {
            let value: u64 = captures[1].parse().unwrap_or(0);
            match captures[2].to_ascii_lowercase().as_str() {
                "passed" | "passing" => passed = Some(value),
                "failed" | "failing" => failed = Some(value),
                "error" | "errors" => errors = Some(value),
                _ => skipped = Some(value),
            }
        }
    }
    // unittest : « Ran 12 tests », « FAILED (failures=1, errors=2) ».
    let ran = regex(r"^Ran (\d+) tests?");
    let unittest_failed = regex(r"FAILED \((?:failures=(\d+))?(?:, )?(?:errors=(\d+))?");
    for line in &lines {
        if let Some(captures) = ran.captures(line) {
            let total: u64 = captures[1].parse().unwrap_or(0);
            let bad = lines
                .iter()
                .find_map(|line| unittest_failed.captures(line))
                .map_or(0, |captures| {
                    captures
                        .get(1)
                        .map_or(0, |found| found.as_str().parse().unwrap_or(0))
                        + captures
                            .get(2)
                            .map_or(0, |found| found.as_str().parse().unwrap_or(0))
                });
            passed = Some(total.saturating_sub(bad));
            failed = Some(bad);
        }
    }
    summary.passed = passed;
    summary.failed = match (failed, errors) {
        (None, None) => None,
        (failed, errors) => Some(failed.unwrap_or(0) + errors.unwrap_or(0)),
    };
    summary.skipped = skipped;
    // Noms des échecs.
    let patterns = [
        regex(r"^(?:FAILED|ERROR) (\S+?)(?: - (.*))?$"), // pytest -rfE
        regex(r"^\s*● (.+?)(?: › (.+))?$"),              // Jest
        regex(r"^\s*(?:×|✕|✗|FAIL)\s+(.+?)(?:\s+\d+ms)?$"), // Vitest, Jest
        regex(r"^--- FAIL: (\S+)"),                      // go test
        regex(r"^(?:FAIL|ERROR): (\S+) \((.+)\)"),       // unittest
        regex(r"^\s+\d+\) (.+)$"),                       // Mocha
    ];
    for (index, line) in lines.iter().enumerate() {
        for pattern in &patterns {
            let Some(captures) = pattern.captures(line) else {
                continue;
            };
            let name = captures[1].trim().to_string();
            if name.is_empty() || summary.failures.iter().any(|(known, _)| *known == name) {
                break;
            }
            let message = captures
                .get(2)
                .map(|found| found.as_str().trim().to_string())
                .filter(|message| !message.is_empty())
                .unwrap_or_else(|| {
                    lines[index + 1..]
                        .iter()
                        .filter(|line| !line.trim().is_empty())
                        .take(4)
                        .map(|line| line.trim())
                        .collect::<Vec<_>>()
                        .join("\n")
                });
            summary.failures.push((name, message));
            break;
        }
        if summary.failures.len() >= 30 {
            break;
        }
    }
    summary
}

/// Lance les tests et rend leur bilan.
pub fn run_tests(input: &RunTestsInput) -> io::Result<Value> {
    let cwd = std::env::current_dir()?;
    let root = match input.path.as_deref().filter(|path| !path.trim().is_empty()) {
        Some(path) => cwd.join(path),
        None => cwd.clone(),
    };
    if !root.is_dir() {
        return Err(invalid(format!(
            "[Claw] run_tests was not executed: {} is not a directory of the project.",
            root.display()
        )));
    }
    let (command, tool) = match input.command.as_deref().map(str::trim).filter(|command| !command.is_empty()) {
        Some(command) => (command.to_string(), "custom"),
        None => detect_test_command(&root, input.filter.as_deref()).ok_or_else(|| {
            invalid("[Claw] run_tests found no test setup here (Cargo.toml, a package.json test script or Jest/Vitest, pytest or tests/test_*.py, go.mod, pom.xml, build.gradle, a .NET project, phpunit, rspec, a Makefile test target). Pass `command` with the command that runs the tests, or write the tests first.")
        })?,
    };
    let full = if root == cwd {
        command.clone()
    } else {
        format!("cd {} && {command}", quoted(&root.to_string_lossy()))
    };
    let timeout_ms = input.timeout_seconds.unwrap_or(600).clamp(10, 3_600) * 1_000;
    let bash: crate::BashCommandInput = serde_json::from_value(json!({
        "command": full,
        "timeout": timeout_ms,
        "description": "Run the project's tests",
    }))
    .map_err(|error| invalid(error.to_string()))?;
    let output = crate::execute_bash(bash)?;
    let text = format!("{}\n{}", output.stdout, output.stderr);
    let summary = summarize_tests(&text);
    let code = output
        .return_code_interpretation
        .as_deref()
        .and_then(|code| code.strip_prefix("exit_code:"))
        .and_then(|code| code.parse::<i64>().ok());
    let succeeded = code == Some(0) && summary.failed.unwrap_or(0) == 0;
    let mut result = json!({
        "command": command,
        "tool": tool,
        "exitCode": code,
        "succeeded": succeeded,
        "passed": summary.passed,
        "failed": summary.failed,
        "skipped": summary.skipped,
        "failures": summary.failures.iter().take(20).map(|(name, message)| json!({ "name": name, "message": excerpt(message, 600) })).collect::<Vec<_>>(),
        "output": excerpt(text.trim(), 6_000),
    });
    if output.background_task_id.is_some() {
        result["note"] = json!("the tests did not finish in time and went on in the background: follow them with BashOutput");
    } else if !succeeded && summary.failures.is_empty() {
        result["note"] = json!("the command failed without a recognizable test failure: read `output` (a build or import error, or a missing tool)");
    }
    Ok(result)
}

// ============================================================ git

fn git(args: &[&str]) -> io::Result<(bool, String, String)> {
    let output = Command::new("git")
        .args(args)
        .env("GIT_PAGER", "cat")
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .map_err(|error| {
            io::Error::new(
                error.kind(),
                format!("[Claw] git could not be run ({error}): is git installed?"),
            )
        })?;
    Ok((
        output.status.success(),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    ))
}

fn git_checked(args: &[&str]) -> io::Result<String> {
    let (ok, stdout, stderr) = git(args)?;
    if ok {
        Ok(stdout)
    } else {
        let message = stderr.trim();
        Err(io::Error::other(
            if message.contains("not a git repository") {
                "[Claw] This folder is not a git repository: run `git init` with bash first if the project should be one.".to_string()
            } else {
                format!(
                    "git {}: {}",
                    args.join(" "),
                    message.lines().take(6).collect::<Vec<_>>().join("\n")
                )
            },
        ))
    }
}

#[derive(Debug, Deserialize)]
pub struct GitInput {
    pub action: String,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default, rename = "ref", alias = "revision", alias = "commit")]
    pub reference: Option<String>,
    #[serde(default)]
    pub staged: Option<bool>,
    #[serde(default)]
    pub limit: Option<usize>,
    /// `blame` : lignes « 10,40 ».
    #[serde(default)]
    pub lines: Option<String>,
}

/// État du dépôt en listes : branche, avance et retard, fichiers indexés, modifiés, nouveaux.
fn git_status() -> io::Result<Value> {
    let text = git_checked(&["status", "--porcelain=v1", "-b", "--untracked-files=all"])?;
    let mut branch = String::new();
    let (mut ahead, mut behind) = (0u64, 0u64);
    let mut groups: BTreeMap<&str, Vec<String>> = BTreeMap::new();
    let number = |text: &str, word: &str| {
        text.split(word)
            .nth(1)
            .and_then(|rest| rest.trim().split(|ch: char| !ch.is_ascii_digit()).next())
            .and_then(|digits| digits.parse().ok())
            .unwrap_or(0)
    };
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("## ") {
            // « ## No commits yet on main » dans un dépôt neuf.
            let rest = rest.strip_prefix("No commits yet on ").unwrap_or(rest);
            branch = rest
                .split("...")
                .next()
                .unwrap_or(rest)
                .split(' ')
                .next()
                .unwrap_or("")
                .to_string();
            ahead = number(rest, "ahead ");
            behind = number(rest, "behind ");
            continue;
        }
        if line.len() < 4 {
            continue;
        }
        let (code, path) = line.split_at(2);
        let path = path.trim().to_string();
        let (x, y) = (code.as_bytes()[0], code.as_bytes()[1]);
        if code == "??" {
            groups.entry("untracked").or_default().push(path);
        } else if x == b'U' || y == b'U' || code == "AA" || code == "DD" {
            groups.entry("conflicts").or_default().push(path);
        } else {
            if x != b' ' {
                groups
                    .entry("staged")
                    .or_default()
                    .push(format!("{} {path}", x as char));
            }
            if y != b' ' {
                groups
                    .entry("modified")
                    .or_default()
                    .push(format!("{} {path}", y as char));
            }
        }
    }
    let clean = groups.is_empty();
    let mut output = json!({ "branch": branch, "ahead": ahead, "behind": behind, "clean": clean });
    for (name, files) in groups {
        output[name] = json!(files);
    }
    Ok(output)
}

/// git en lecture : `status`, `diff`, `log`, `show`, `blame`, `branches`.
pub fn git_read(input: &GitInput) -> io::Result<Value> {
    let path = input
        .path
        .as_deref()
        .map(str::trim)
        .filter(|path| !path.is_empty());
    let reference = input
        .reference
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if reference.is_some_and(|value| value.starts_with('-')) {
        return Err(invalid(
            "[Claw] git was not executed: `ref` cannot start with '-'.",
        ));
    }
    match input.action.trim().to_ascii_lowercase().as_str() {
        "status" => git_status(),
        "diff" => {
            let mut args = vec!["diff"];
            if input.staged.unwrap_or(false) {
                args.push("--cached");
            }
            if let Some(reference) = reference {
                args.push(reference);
            }
            let mut stat_args = args.clone();
            stat_args.insert(1, "--stat");
            if let Some(path) = path {
                args.extend(["--", path]);
                stat_args.extend(["--", path]);
            }
            let stat = git_checked(&stat_args)?;
            let patch = git_checked(&args)?;
            let mut output = json!({ "stat": stat.trim(), "diff": excerpt(&patch, 40_000) });
            if patch.trim().is_empty() {
                output["note"] = json!(if input.staged.unwrap_or(false) {
                    "nothing staged"
                } else {
                    "no change (compared with the index; add `staged: true` for what is staged, or a `ref`)"
                });
            }
            Ok(output)
        }
        "log" => {
            let limit = input.limit.unwrap_or(15).clamp(1, 200).to_string();
            let mut args = vec!["log", "-n", limit.as_str(), "--date=short", "--format=%h%x09%ad%x09%an%x09%s"];
            if let Some(reference) = reference {
                args.push(reference);
            }
            if let Some(path) = path {
                args.extend(["--", path]);
            }
            let text = git_checked(&args)?;
            let commits: Vec<Value> = text
                .lines()
                .filter_map(|line| {
                    let mut parts = line.splitn(4, '\t');
                    Some(json!({
                        "commit": parts.next()?,
                        "date": parts.next()?,
                        "author": parts.next()?,
                        "subject": parts.next().unwrap_or(""),
                    }))
                })
                .collect();
            Ok(json!({ "commits": commits }))
        }
        "show" => {
            let reference = reference.unwrap_or("HEAD");
            let text = git_checked(&["show", "--stat", "--patch", "--format=commit %H%nAuthor: %an <%ae>%nDate:   %ad%n%n%B", reference])?;
            Ok(json!({ "show": excerpt(&text, 40_000) }))
        }
        "blame" => {
            let Some(path) = path else {
                return Err(invalid("[Claw] git blame needs `path`, and `lines` such as \"10,40\"."));
            };
            let range = input
                .lines
                .as_deref()
                .map(|lines| lines.trim().replace('-', ","))
                .filter(|lines| !lines.is_empty());
            let mut args = vec!["blame", "--date=short"];
            if let Some(range) = &range {
                args.extend(["-L", range.as_str()]);
            }
            args.extend(["--", path]);
            let text = git_checked(&args)?;
            Ok(json!({ "blame": excerpt(&text, 30_000) }))
        }
        "branches" | "branch" => {
            let text = git_checked(&["branch", "-a", "--format=%(HEAD)%09%(refname:short)%09%(objectname:short)%09%(subject)"])?;
            let branches: Vec<Value> = text
                .lines()
                .filter_map(|line| {
                    let mut parts = line.splitn(4, '\t');
                    let current = parts.next()? == "*";
                    Some(json!({
                        "name": parts.next()?,
                        "current": current,
                        "commit": parts.next().unwrap_or(""),
                        "subject": parts.next().unwrap_or(""),
                    }))
                })
                .collect();
            Ok(json!({ "branches": branches }))
        }
        other => Err(invalid(format!(
            "[Claw] git does not know the action \"{other}\": use status, diff, log, show, blame or branches. To commit, use git_commit; for anything else (checkout, merge, push), use bash."
        ))),
    }
}

#[derive(Debug, Deserialize)]
pub struct GitCommitInput {
    pub message: String,
    /// Fichiers à valider ; sans eux, tous les changements (`git add -A`).
    #[serde(default)]
    pub paths: Option<Vec<String>>,
}

/// Indexe les changements (tous, ou ceux de `paths`) et les valide.
pub fn git_commit(input: &GitCommitInput) -> io::Result<Value> {
    let message = input.message.trim();
    if message.is_empty() {
        return Err(invalid(
            "[Claw] git_commit was not executed: the commit message is empty.",
        ));
    }
    match input.paths.as_ref().filter(|paths| !paths.is_empty()) {
        Some(paths) => {
            let mut args = vec!["add", "--"];
            args.extend(paths.iter().map(String::as_str));
            git_checked(&args)?;
        }
        None => {
            git_checked(&["add", "-A"])?;
        }
    }
    let (nothing, _, _) = git(&["diff", "--cached", "--quiet"])?;
    if nothing {
        return Err(invalid(
            "[Claw] git_commit changed nothing: there is no change to commit (check with git, action status).",
        ));
    }
    git_checked(&["commit", "--quiet", "-m", message])?;
    let hash = git_checked(&["rev-parse", "--short", "HEAD"])?;
    let stat = git_checked(&["show", "--stat", "--format=", "HEAD"])?;
    let branch = git_checked(&["rev-parse", "--abbrev-ref", "HEAD"])?;
    let mut summary = String::new();
    let _ = write!(summary, "{}", stat.trim());
    Ok(json!({
        "commit": hash.trim(),
        "branch": branch.trim(),
        "message": message,
        "stat": summary,
    }))
}

#[cfg(test)]
mod tests {
    use super::{detect_test_command, summarize_tests, TestSummary};
    use std::fs;

    #[test]
    fn outlines_list_nested_definitions_with_their_lines() {
        let dir = std::env::temp_dir().join(format!("claw-outline-{}", std::process::id()));
        fs::create_dir_all(&dir).expect("dossier");
        let ts = dir.join("cart.ts");
        fs::write(&ts, "// export function hidden() {}\nexport class Cart {\n  async add(item: Item): Promise<void> {\n    if (item) {\n    }\n  }\n}\nexport const total = (items: Item[]) => 0\n").expect("ts");
        let outline = super::file_outline(&ts.to_string_lossy()).expect("plan");
        let names: Vec<(u64, String, String, u64)> = outline["symbols"]
            .as_array()
            .expect("symboles")
            .iter()
            .map(|symbol| {
                (
                    symbol["line"].as_u64().unwrap_or(0),
                    symbol["kind"].as_str().unwrap_or("").to_string(),
                    symbol["name"].as_str().unwrap_or("").to_string(),
                    symbol["depth"].as_u64().unwrap_or(9),
                )
            })
            .collect();
        assert_eq!(
            names,
            [
                (2, "class".to_string(), "Cart".to_string(), 0),
                (3, "method".to_string(), "add".to_string(), 1),
                (8, "function".to_string(), "total".to_string(), 0),
            ]
        );
        let md = dir.join("README.md");
        fs::write(&md, "# Projet\n\n## Installer\n").expect("md");
        let outline = super::file_outline(&md.to_string_lossy()).expect("plan");
        assert_eq!(outline["symbols"][1]["kind"], "h2");
        assert_eq!(outline["symbols"][1]["depth"], 1);
        assert!(super::file_outline(&dir.join("x.unknownext").to_string_lossy()).is_err());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn test_commands_are_found_from_the_project_files() {
        let dir = std::env::temp_dir().join(format!("claw-run-tests-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("tests")).expect("dossier");
        assert_eq!(detect_test_command(&dir, None), None);
        fs::write(dir.join("tests/test_app.py"), "def test_x(): pass\n").expect("test");
        let (command, tool) = detect_test_command(&dir, Some("panier")).expect("pytest");
        assert_eq!(tool, "pytest");
        assert!(
            command.ends_with("-m pytest -q -rfE -k 'panier'"),
            "{command}"
        );
        fs::write(
            dir.join("package.json"),
            r#"{"scripts":{"test":"vitest run"}}"#,
        )
        .expect("js");
        assert_eq!(
            detect_test_command(&dir, Some("total")),
            Some(("npm test --silent -- -t 'total'".to_string(), "vitest"))
        );
        fs::write(dir.join("Cargo.toml"), "[package]\n").expect("rust");
        assert_eq!(
            detect_test_command(&dir, None),
            Some(("cargo test".to_string(), "cargo"))
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn git_reads_and_commits_in_a_repository() {
        let dir = std::env::temp_dir().join(format!("claw-git-tool-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("dossier");
        let previous = std::env::current_dir().expect("dossier courant");
        std::env::set_current_dir(&dir).expect("cd");
        let run = |args: &[&str]| {
            std::process::Command::new("git")
                .args(args)
                .output()
                .expect("git");
        };
        run(&["init", "-q"]);
        run(&["config", "user.email", "claw@example.invalid"]);
        run(&["config", "user.name", "Claw"]);
        fs::write("app.py", "print(1)\n").expect("fichier");
        let status = super::git_read(&super::GitInput {
            action: "status".to_string(),
            path: None,
            reference: None,
            staged: None,
            limit: None,
            lines: None,
        })
        .expect("status");
        assert_eq!(status["untracked"], serde_json::json!(["app.py"]));
        let committed = super::git_commit(&super::GitCommitInput {
            message: "Premier commit".to_string(),
            paths: None,
        })
        .expect("commit");
        assert_eq!(committed["message"], "Premier commit");
        let log = super::git_read(&super::GitInput {
            action: "log".to_string(),
            path: None,
            reference: None,
            staged: None,
            limit: Some(5),
            lines: None,
        })
        .expect("log");
        assert_eq!(log["commits"][0]["subject"], "Premier commit");
        let again = super::git_commit(&super::GitCommitInput {
            message: "Rien".to_string(),
            paths: None,
        });
        assert!(again.is_err());
        std::env::set_current_dir(previous).expect("retour");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn failures_are_read_from_common_test_outputs() {
        let cargo = "running 2 tests\ntest tests::adds ... ok\ntest tests::rounds ... FAILED\n\nfailures:\n\n---- tests::rounds stdout ----\nthread 'tests::rounds' panicked at src/lib.rs:9:5:\nassertion `left == right` failed\n  left: 3\n right: 4\n\nfailures:\n    tests::rounds\n\ntest result: FAILED. 1 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out\n";
        let summary = summarize_tests(cargo);
        assert_eq!((summary.passed, summary.failed), (Some(1), Some(1)));
        assert_eq!(summary.failures[0].0, "tests::rounds");
        assert!(
            summary.failures[0].1.contains("left: 3"),
            "{}",
            summary.failures[0].1
        );

        let pytest = "..F.\nFAILED tests/test_cart.py::test_total - AssertionError: assert 9 == 10\n1 failed, 3 passed in 0.12s\n";
        let summary = summarize_tests(pytest);
        assert_eq!((summary.passed, summary.failed), (Some(3), Some(1)));
        assert_eq!(
            summary.failures,
            [(
                "tests/test_cart.py::test_total".to_string(),
                "AssertionError: assert 9 == 10".to_string()
            )]
        );

        let jest = "FAIL src/cart.test.js\n  ● cart › computes the total\n\n    expect(received).toBe(expected)\n\nTests:       1 failed, 4 passed, 5 total\n";
        let summary = summarize_tests(jest);
        assert_eq!((summary.passed, summary.failed), (Some(4), Some(1)));
        assert!(
            summary.failures.iter().any(|(name, _)| name == "cart"),
            "{:?}",
            summary.failures
        );

        let go = "--- FAIL: TestTotal (0.00s)\n    cart_test.go:12: got 9, want 10\nFAIL\n";
        assert_eq!(summarize_tests(go).failures[0].0, "TestTotal");
        assert_eq!(summarize_tests("all good"), TestSummary::default());
    }
}
