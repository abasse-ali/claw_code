//! Méthode de code par langage : ce qu'un ingénieur soigneux fait d'office en
//! Python, en `PowerShell`, en batch, en Bash, en `JavaScript` ou `TypeScript`
//! (et, plus court, en Rust, Go, C#, Java, PHP, SQL). Un petit modèle écrit du
//! code qui marche sur l'exemple et casse sur un vrai cas : un chemin avec des
//! espaces, un fichier absent, des accents, une erreur avalée, un script jamais
//! lancé. La méthode est jointe à une demande de code qui nomme le langage, ou
//! donnée la première fois qu'un fichier de ce langage est écrit dans le tour.

use std::fmt::Write as _;
use std::path::Path;

use crate::harness::GUIDE_PREFIX;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Language {
    Python,
    PowerShell,
    Batch,
    Shell,
    JavaScript,
    Rust,
    Go,
    CSharp,
    Java,
    Php,
    Sql,
}

impl Language {
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Python => "Python",
            Self::PowerShell => "PowerShell",
            Self::Batch => "batch (.bat, .cmd)",
            Self::Shell => "Bash",
            Self::JavaScript => "JavaScript and TypeScript",
            Self::Rust => "Rust",
            Self::Go => "Go",
            Self::CSharp => "C#",
            Self::Java => "Java",
            Self::Php => "PHP",
            Self::Sql => "SQL",
        }
    }

    /// Le langage d'un fichier, d'après son extension.
    #[must_use]
    pub fn of(path: &Path) -> Option<Self> {
        let extension = path.extension()?.to_string_lossy().to_ascii_lowercase();
        Some(match extension.as_str() {
            "py" | "pyw" => Self::Python,
            "ps1" | "psm1" => Self::PowerShell,
            "bat" | "cmd" => Self::Batch,
            "sh" | "bash" | "zsh" => Self::Shell,
            "js" | "mjs" | "cjs" | "ts" | "mts" | "cts" => Self::JavaScript,
            "rs" => Self::Rust,
            "go" => Self::Go,
            "cs" => Self::CSharp,
            "java" => Self::Java,
            "php" => Self::Php,
            "sql" => Self::Sql,
            _ => return None,
        })
    }

    /// Les règles du langage, une par ligne.
    #[must_use]
    pub fn rules(self) -> &'static str {
        match self {
            Self::Python => PYTHON,
            Self::PowerShell => POWERSHELL,
            Self::Batch => BATCH,
            Self::Shell => SHELL,
            Self::JavaScript => JAVASCRIPT,
            Self::Rust => "- Result and ? instead of unwrap or expect outside tests; errors with context (anyhow or thiserror in the project's style).\n- Borrow rather than clone; iterators rather than index loops; no unsafe without a written reason.\n- Check: cargo clippy -- -D warnings and cargo test.",
            Self::Go => "- Check every err and wrap it with context: return fmt.Errorf(\"reading %s: %w\", path, err).\n- defer Close right after a successful Open; context.Context first in long calls; no goroutine without a way to stop it.\n- Check: gofmt, go vet ./..., go test ./....",
            Self::CSharp => "- Nullable reference types on; using or await using for every IDisposable; async all the way (no .Result or .Wait()).\n- Specific exceptions with messages; no empty catch.\n- Check: dotnet build and dotnet test.",
            Self::Java => "- try-with-resources for streams and connections; specific exceptions; Optional instead of returning null.\n- Immutable values where possible; equals and hashCode together.\n- Check: the project's build (mvn -q verify or gradle test).",
            Self::Php => "- declare(strict_types=1); PDO with prepared statements, never user input in a query string.\n- htmlspecialchars on every value written into HTML; password_hash and password_verify for passwords.\n- Check: php -l on each file, then the project's tests.",
            Self::Sql => "- Parameterized queries from the calling code, never user input concatenated into SQL.\n- Explicit column lists (no SELECT * in code), a transaction around multi-step changes, NOT NULL and foreign keys where the data requires them.\n- Try each query on a small sample and check the row counts.",
        }
    }

    /// Comment lancer un fichier de ce langage pour l'essayer.
    #[must_use]
    pub fn run_hint(self, file: &str) -> String {
        match self {
            Self::Python => format!("python {file} --help, then on a small sample"),
            Self::PowerShell => format!("powershell -NoProfile -ExecutionPolicy Bypass -File .\\{file} -WhatIf, then on a test folder"),
            Self::Batch => format!("cmd /c {file} with test arguments"),
            Self::Shell => format!("bash -n {file}, then bash {file} on test data"),
            Self::JavaScript => format!("node {file} on a small sample"),
            Self::Rust | Self::Go | Self::CSharp | Self::Java | Self::Php | Self::Sql => {
                format!("the project's build and tests, or {file} on a sample")
            }
        }
    }
}

const PYTHON: &str = "- Standard library first; pathlib.Path for paths; with open(path, encoding=\"utf-8\") (on Windows the default is not UTF-8), csv with newline=\"\".\n- A script: def main() -> int, argparse with a helpful --help, and if __name__ == \"__main__\": sys.exit(main()); errors to stderr with exit code 1.\n- Catch the exceptions you expect (except FileNotFoundError:), never a bare except or except Exception: pass; raise with a message that says what failed.\n- No mutable default arguments; f-strings; type hints on function signatures; small functions with one job.\n- subprocess.run([...], check=True) with a list, never shell=True on user input; json, csv, datetime and zoneinfo instead of parsing by hand.\n- Tests with pytest (or unittest if the project uses it) for the logic, including an empty input and a bad one.";

const POWERSHELL: &str = "- Target Windows PowerShell 5.1, the one every Windows has, unless the user asks for 7 (then #Requires -Version 7): no &&, ||, ??, ?:, ?., ForEach-Object -Parallel or ConvertFrom-Json -AsHashtable.\n- Start with [CmdletBinding(SupportsShouldProcess)] param(...) with typed parameters, [Parameter(Mandatory)], [ValidateSet()] or [ValidateScript()]; then Set-StrictMode -Version Latest and $ErrorActionPreference = 'Stop', so that try/catch sees cmdlet failures.\n- Put every change (delete, move, write, stop) under if ($PSCmdlet.ShouldProcess($target, 'action')) so that -WhatIf shows it without doing it.\n- Paths: Join-Path, -LiteralPath for paths that come from users or files, Test-Path before use, $PSScriptRoot for files next to the script.\n- Compare with -eq -ne -gt -lt -like -match, $null on the left ($null -eq $x); > and < redirect to files.\n- Functions return objects; Write-Verbose and Write-Warning for messages, throw or Write-Error for errors, Write-Host only for decoration; full cmdlet names, no aliases (Get-ChildItem, not ls or gci).\n- Native programs: & \"$path\" @arguments, then check $LASTEXITCODE; files written with -Encoding utf8.
- To run it from bash, use powershell -NoProfile -ExecutionPolicy Bypass -File script.ps1, or put -Command code in single quotes: in double quotes bash replaces $_ and the $variables before PowerShell reads them.";

const BATCH: &str = "- @echo off, then setlocal EnableExtensions EnableDelayedExpansion; chcp 65001 >nul when any text has accents; Claw saves the file with CRLF and no BOM, as cmd.exe needs.\n- Quote every argument and path: \"%~1\", \"%~dp0data.txt\" (%~dp0 is the script's folder, with its backslash).\n- set \"name=value\" (quotes, no spaces around =); set /a for numbers; inside a ( ) block, read variables set in the block with !name!; escape ( ) & | < > ^ in echo text with ^, or quote it.\n- Check the inputs first: if \"%~1\"==\"\" goto :usage, if not exist \"%~1\" ( echo File not found: %~1 1>&2 & exit /b 1 ).\n- After each command that can fail: if errorlevel 1 ( echo what failed 1>&2 & exit /b 1 ); end with exit /b 0; subroutines with call :name, each ending with exit /b; labels exist for every goto.\n- Batch is for short orchestration: dates, JSON, text parsing or loops over structured data belong in PowerShell (a .ps1 called with powershell -NoProfile -ExecutionPolicy Bypass -File).";

const SHELL: &str = "- #!/usr/bin/env bash and set -euo pipefail; Claw saves the file with LF line endings.\n- Quote every expansion: \"$var\", \"$@\", \"${array[@]}\"; [[ ]] for tests; $(...) rather than backticks; local in functions.\n- Check what the script needs: command -v tool >/dev/null || { echo \"tool is missing\" >&2; exit 1; }; print the usage when arguments are missing.\n- Temporary files with mktemp and trap 'rm -f \"$tmp\"' EXIT; read lines with while IFS= read -r line; never parse ls (globs, or find -print0 with xargs -0).\n- Messages to stderr (>&2) and meaningful exit codes; shellcheck-clean.";

const JAVASCRIPT: &str = "- ES modules (import and export, \"type\": \"module\" or .mjs) unless the project uses CommonJS; node: prefixes for built-ins (node:fs/promises, node:path).\n- Every promise awaited or handled; the entry point catches errors, prints them to stderr and sets process.exitCode = 1.\n- Validate external input (arguments, JSON, request bodies) before use; === and !==; const by default, never var.\n- path.join for paths, fs/promises for files; fetch with response.ok checked before reading the body.\n- TypeScript: strict mode, no any (unknown, then narrowing), types on function boundaries, exhaustive switch with a never check; Node 22.6+ runs .ts by stripping types, and tsc --noEmit checks them.\n- Tests with the project's runner (node --test, vitest or jest) for the logic.";

/// Ce que tout code demande, quel que soit le langage.
const COMMON: &str = "1. Before writing, say in two or three lines what goes in (arguments, files, environment), what comes out (output, files, exit code) and the cases that break naive code: an empty or missing input, a path with spaces or accents, a file that does not exist or is locked, a large input, a second run.\n2. Read the code you change and the code that calls it, and follow its conventions. Never invent an API, an option or a module: check that it exists (its --help, its documentation, its source) before using it.\n3. Handle errors where they happen, with a message that says what failed and what to do; never swallow one. A script exits with 0 on success and a non-zero code on failure.\n4. No secret, personal absolute path or machine-specific value in the code: arguments, environment variables or a configuration file.\n5. Finish by running it: the tests if there are any, otherwise the program itself on a small real input (in its dry-run mode for anything destructive). Read the output, fix, run again, and say what you ran and what you saw.";

/// Mots qui désignent chaque langage dans une demande.
const LANGUAGE_WORDS: &[(Language, &[&str])] = &[
    (
        Language::Python,
        &[
            "python", "py", "pip", "pandas", "django", "flask", "fastapi", "pytest", "numpy",
        ],
    ),
    (
        Language::PowerShell,
        &["powershell", "pwsh", "ps1", "cmdlet"],
    ),
    (
        Language::Batch,
        &[
            "batch",
            "bat",
            "cmd",
            "fichier de commandes",
            "invite de commandes",
        ],
    ),
    (Language::Shell, &["bash", "shell", "sh", "zsh"]),
    (
        Language::JavaScript,
        &[
            "javascript",
            "typescript",
            "node",
            "nodejs",
            "node.js",
            "npm",
            "deno",
            "express",
            "ts",
            "js",
        ],
    ),
    (Language::Rust, &["rust", "cargo"]),
    (Language::Go, &["golang"]),
    (Language::CSharp, &["c#", "csharp", "dotnet", ".net"]),
    (Language::Java, &["java", "maven", "gradle", "spring"]),
    (Language::Php, &["php", "laravel", "symfony"]),
    (
        Language::Sql,
        &[
            "sql",
            "postgres",
            "postgresql",
            "mysql",
            "sqlite",
            "requete sql",
        ],
    ),
];

/// Verbes d'une demande d'écriture ou de correction de code.
const CODE_WORDS: &[&str] = &[
    "cree",
    "creer",
    "ecris",
    "ecrire",
    "fais",
    "faire",
    "developpe",
    "code",
    "coder",
    "programme",
    "implemente",
    "ajoute",
    "corrige",
    "repare",
    "modifie",
    "refactorise",
    "reecris",
    "convertis",
    "automatise",
    "script",
    "fonction",
    "outil",
    "write",
    "create",
    "build",
    "make",
    "implement",
    "fix",
    "add",
    "refactor",
    "rewrite",
    "convert",
    "automate",
    "function",
    "tool",
];

fn has(folded: &str, word: &str) -> bool {
    let word = crate::team::without_accents(word);
    if word.contains([' ', '.', '#']) {
        return folded.contains(&word);
    }
    folded
        .split(|ch: char| !(ch.is_alphanumeric() || ch == '#'))
        .any(|token| token == word)
}

/// Les langages qu'une demande d'écriture ou de correction de code nomme.
#[must_use]
pub fn languages_in(request: &str) -> Vec<Language> {
    let folded = crate::team::without_accents(&request.to_lowercase());
    if !CODE_WORDS.iter().any(|word| has(&folded, word)) {
        return Vec::new();
    }
    LANGUAGE_WORDS
        .iter()
        .filter(|(_, words)| words.iter().any(|word| has(&folded, word)))
        .map(|(language, _)| *language)
        .collect()
}

/// La méthode jointe à une demande de code qui nomme ses langages (aussi à
/// un message court qui poursuit une telle demande).
#[must_use]
pub fn code_guide(request: &str, earlier: Option<&str>) -> Option<(String, Vec<Language>)> {
    let mut languages = languages_in(request);
    if languages.is_empty() && request.split_whitespace().count() <= 12 {
        languages = earlier.map(languages_in).unwrap_or_default();
    }
    if languages.is_empty() {
        return None;
    }
    let mut text = format!(
        "{GUIDE_PREFIX} This request writes or changes code. Write it as a careful senior engineer would: correct on real inputs, not only on the example.\n{COMMON}"
    );
    for language in &languages {
        let _ = write!(text, "\n{} rules:\n{}", language.name(), language.rules());
    }
    Some((text, languages))
}

/// Un fichier qui se lance lui-même (un script, pas un module) : un `.ps1`,
/// un `.bat`, un script shell, un fichier Python avec `__main__` ou qui lit
/// ses arguments, un fichier `JavaScript` avec un shebang ou `process.argv`.
#[must_use]
pub fn is_runnable_script(path: &Path, text: &str) -> bool {
    let extension = path
        .extension()
        .map(|ext| ext.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    match extension.as_str() {
        "ps1" | "bat" | "cmd" | "sh" | "bash" | "zsh" => true,
        "py" | "pyw" => ["__main__", "sys.argv", "argparse"]
            .iter()
            .any(|mark| text.contains(mark)),
        "js" | "mjs" | "cjs" | "ts" | "mts" => {
            text.starts_with("#!") || text.contains("process.argv")
        }
        _ => false,
    }
}

/// Les règles d'un langage, données la première fois qu'un fichier de ce
/// langage est écrit dans le tour.
#[must_use]
pub fn rules_note(language: Language, file: &str) -> String {
    format!(
        "[Claw] {file} is {} code. Rules to follow for it (some of them are checked when you write):\n{}\nWhen it is written, run it ({}), read the output and fix what fails.",
        language.name(),
        language.rules(),
        language.run_hint(file)
    )
}

/// Ce qui est dit au modèle quand des scripts écrits n'ont jamais été lancés.
#[must_use]
pub fn run_scripts_feedback(unrun: &[(String, Language)]) -> String {
    let names: Vec<&str> = unrun.iter().map(|(name, _)| name.as_str()).collect();
    let hints: Vec<String> = unrun
        .iter()
        .map(|(name, language)| language.run_hint(name))
        .collect();
    format!(
        "[Claw] You wrote {} in this turn but never ran {}. Code that was never run is not done: run each one now on a small, safe input and read the output ({}). Work in a temporary folder or on a copy if it changes files. If running it would do something irreversible (delete real data, send messages, install software, call a paid service), do not run it: say so in your answer, and how you checked it instead. Fix what fails, then answer.\n\n{}",
        names.join(", "),
        if names.len() > 1 { "them" } else { "it" },
        hints.join("; "),
        crate::harness::FINAL_ANSWER_RULE
    )
}

#[cfg(test)]
mod tests {
    use super::{code_guide, languages_in, rules_note, Language};
    use std::path::Path;

    #[test]
    fn a_code_request_brings_the_rules_of_the_languages_it_names() {
        assert_eq!(
            languages_in("Écris un script PowerShell qui sauvegarde mes photos"),
            [Language::PowerShell]
        );
        assert_eq!(
            languages_in("crée un fichier .bat et un script Python pour le rapport"),
            [Language::Python, Language::Batch]
        );
        assert_eq!(
            languages_in("Fix the TypeScript build"),
            [Language::JavaScript]
        );
        // Une question, pas une demande de code ; un mot qui contient « py ».
        assert!(languages_in("Qu'est-ce que Python ?").is_empty());
        assert!(languages_in("corrige la copy du bouton").is_empty());
        let (guide, languages) =
            code_guide("Écris un script python qui trie les factures", None).expect("guide");
        assert_eq!(languages, [Language::Python]);
        assert!(guide.contains("Python rules:") && guide.contains("encoding=\"utf-8\""));
        assert!(guide.contains("5. Finish by running it"));
        // « continue » poursuit la demande précédente.
        assert!(code_guide("continue", Some("Écris un script bash de sauvegarde")).is_some());
        assert!(code_guide("Bonjour", None).is_none());
    }

    #[test]
    fn a_file_gets_the_rules_of_its_language() {
        assert_eq!(
            Language::of(Path::new("deploy.PS1")),
            Some(Language::PowerShell)
        );
        assert_eq!(Language::of(Path::new("build.cmd")), Some(Language::Batch));
        assert_eq!(
            Language::of(Path::new("index.ts")),
            Some(Language::JavaScript)
        );
        assert_eq!(Language::of(Path::new("notes.txt")), None);
        let note = rules_note(Language::Batch, "build.bat");
        assert!(
            note.contains("build.bat is batch (.bat, .cmd) code")
                && note.contains("cmd /c build.bat")
        );
        for language in [
            Language::Python,
            Language::PowerShell,
            Language::Batch,
            Language::Shell,
            Language::JavaScript,
            Language::Rust,
            Language::Go,
            Language::CSharp,
            Language::Java,
            Language::Php,
            Language::Sql,
        ] {
            assert!(language.rules().starts_with("- "), "{}", language.name());
        }
    }
}
