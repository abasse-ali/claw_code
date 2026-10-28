//! Contrôles du code écrit, langage par langage, dès l'écriture du fichier :
//! la syntaxe des langages que le contrôle de base ne lisait pas (`PowerShell`,
//! `TypeScript`, CSS, TOML), puis les pièges qui font un code faux sans erreur
//! de syntaxe (un nom jamais défini en Python, `>` pris pour une comparaison
//! en `PowerShell`, une variable lue avec `%` dans un bloc d'un `.bat`, une
//! même règle CSS redite dans dix règles de plus en plus longues…).
//! Chaque constat dit sa ligne et la correction : un petit modèle corrige ce
//! qu'on lui montre, rarement ce qu'il ne voit pas.

use std::collections::BTreeMap;
use std::io::Read as _;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use regex::Regex;

const TIMEOUT: Duration = Duration::from_secs(10);
/// `PowerShell` : son démarrage (.NET) peut dépasser `TIMEOUT` sur une machine chargée.
const POWERSHELL_TIMEOUT: Duration = Duration::from_secs(30);
/// Constats au plus par fichier : les premiers suffisent à corriger.
const MAX_FINDINGS: usize = 10;

struct Run {
    success: bool,
    stdout: String,
    stderr: String,
}

impl Run {
    /// Ce que l'outil a dit de l'erreur : sa sortie, sinon sa sortie d'erreur.
    fn report(&self) -> String {
        let text = if self.stdout.trim().is_empty() {
            &self.stderr
        } else {
            &self.stdout
        };
        text.trim().lines().take(8).collect::<Vec<_>>().join("\n")
    }
}

/// Lance un outil de contrôle ; `None` s'il est absent ou trop lent.
fn run(program: &str, args: &[&str], envs: &[(&str, &str)], cwd: Option<&Path>) -> Option<Run> {
    run_within(TIMEOUT, program, args, envs, cwd)
}

/// `run` avec son propre délai : `PowerShell` met parfois plus de dix secondes à
/// démarrer sur une machine chargée (vu sur les machines Ubuntu de GitHub, où le
/// contrôle restait muet et le test échouait au hasard).
fn run_within(
    timeout: Duration,
    program: &str,
    args: &[&str],
    envs: &[(&str, &str)],
    cwd: Option<&Path>,
) -> Option<Run> {
    let executable = crate::paths::find_executable(program)?;
    let mut command = Command::new(executable);
    command
        .args(args)
        .envs(envs.iter().copied())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(cwd) = cwd.filter(|cwd| !cwd.as_os_str().is_empty()) {
        command.current_dir(cwd);
    }
    let mut child = command.spawn().ok()?;
    let (stdout, stderr) = (child.stdout.take(), child.stderr.take());
    let read = |pipe: Option<Box<dyn std::io::Read + Send>>| {
        std::thread::spawn(move || {
            let mut text = String::new();
            if let Some(mut pipe) = pipe {
                let mut bytes = Vec::new();
                let _ = pipe.read_to_end(&mut bytes);
                text = String::from_utf8_lossy(&bytes).into_owned();
            }
            text
        })
    };
    let out = read(stdout.map(|pipe| Box::new(pipe) as Box<dyn std::io::Read + Send>));
    let err = read(stderr.map(|pipe| Box::new(pipe) as Box<dyn std::io::Read + Send>));
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() > timeout => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(_) => return None,
        }
    };
    Some(Run {
        success: status.success(),
        stdout: out.join().unwrap_or_default(),
        stderr: err.join().unwrap_or_default(),
    })
}

fn extension(path: &Path) -> String {
    path.extension()
        .map(|ext| ext.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default()
}

fn read(path: &Path) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    Some(
        String::from_utf8_lossy(&bytes)
            .trim_start_matches('\u{feff}')
            .to_string(),
    )
}

// --- Syntaxe ---------------------------------------------------------------------

/// Erreur de syntaxe d'un fichier des langages lus ici ; `None` : rien à
/// signaler, ou pas d'outil pour le dire.
#[must_use]
pub fn syntax(path: &Path) -> Option<String> {
    match extension(path).as_str() {
        "ps1" | "psm1" | "psd1" => powershell_syntax(path),
        "ts" | "mts" | "cts" => typescript_syntax(path),
        "css" | "scss" | "less" => css_structure(&read(path)?),
        "toml" => toml_syntax(path),
        _ => None,
    }
}

/// Le script demande `PowerShell` 7 (`#Requires -Version 7`).
fn wants_powershell_7(text: &str) -> bool {
    Regex::new(r"(?im)^\s*#requires\s+-version\s+([6-9]|\d\d)")
        .is_ok_and(|pattern| pattern.is_match(text))
}

/// Analyse le script avec l'analyseur de `PowerShell` : Windows `PowerShell` 5.1
/// (celui de tout Windows) sauf si le script demande la version 7. La syntaxe
/// de `PowerShell` 7 (`&&`, `??`, `?:`) y est donc une erreur, comme elle le
/// serait chez l'utilisateur.
fn powershell_syntax(path: &Path) -> Option<String> {
    let text = read(path)?;
    let seven = wants_powershell_7(&text);
    let programs: &[&str] = if seven {
        &["pwsh", "powershell"]
    } else {
        &["powershell", "pwsh"]
    };
    let program = programs
        .iter()
        .find(|program| crate::paths::find_executable(program).is_some())?;
    let file = path.to_string_lossy().to_string();
    let script = "[Console]::OutputEncoding = [Text.Encoding]::UTF8; $errors = $null; [void][System.Management.Automation.Language.Parser]::ParseFile($env:CLAW_PS_FILE, [ref]$null, [ref]$errors); foreach ($e in $errors) { 'line ' + $e.Extent.StartLineNumber + ': ' + $e.Message }";
    let result = run_within(
        POWERSHELL_TIMEOUT,
        program,
        &["-NoProfile", "-NonInteractive", "-Command", script],
        &[("CLAW_PS_FILE", file.as_str())],
        path.parent(),
    )?;
    let errors: Vec<&str> = result
        .stdout
        .lines()
        .filter(|line| line.starts_with("line "))
        .take(6)
        .collect();
    if errors.is_empty() {
        return None;
    }
    let mut report = errors.join("\n");
    if *program == "powershell" {
        report.push_str("\n(parsed by Windows PowerShell 5.1, the version every Windows has: PowerShell 7 syntax such as &&, ||, ??, ?. or the ternary ?: fails there; add #Requires -Version 7 only if the script is meant for PowerShell 7)");
    }
    Some(report)
}

/// Retire les types avec Node (22.6 et plus), puis fait lire le `JavaScript`
/// obtenu par `node --check` : un `TypeScript` valide passe, une erreur dit sa
/// ligne. `node --check` seul lit un `.ts` comme du `JavaScript` et se trompe.
/// Seule une erreur de syntaxe est dite : un \u00E9chec de Node lui-m\u00EAme (vu en
/// int\u00E9gration continue : \u00AB Cannot allocate Wasm memory \u00BB) laisse le fichier
/// non v\u00E9rifi\u00E9, au lieu d'\u00EAtre pris pour une faute du mod\u00E8le.
fn typescript_syntax(path: &Path) -> Option<String> {
    const SCRIPT: &str = r#"
const m = require("node:module"), fs = require("node:fs"), os = require("node:os"), p = require("node:path"), cp = require("node:child_process");
if (typeof m.stripTypeScriptTypes !== "function") process.exit(0);
const file = process.argv[1], source = fs.readFileSync(file, "utf8").replace(/^\uFEFF/, "");
const report = (error) => {
  if (!error || (error.code !== "ERR_INVALID_TYPESCRIPT_SYNTAX" && error.name !== "SyntaxError")) process.exit(0);
  console.log(String(error.message).split("\n").slice(0, 8).join("\n"));
  process.exit(1);
};
let js;
try { js = m.stripTypeScriptTypes(source, { mode: "strip" }); }
catch (error) {
  if (error && error.code === "ERR_UNSUPPORTED_TYPESCRIPT_SYNTAX") {
    try { js = m.stripTypeScriptTypes(source, { mode: "transform" }); } catch (inner) { report(inner); }
  } else { report(error); }
}
const esm = /^\s*(import|export)\s/m.test(js) || /\.mts$/i.test(file);
const temp = p.join(os.tmpdir(), `claw-ts-${process.pid}${esm ? ".mjs" : ".cjs"}`);
fs.writeFileSync(temp, js);
const checked = cp.spawnSync(process.execPath, ["--check", temp], { encoding: "utf8" });
fs.rmSync(temp, { force: true });
if (checked.status !== 0 && /SyntaxError/.test(String(checked.stderr))) { console.log(String(checked.stderr).split(temp).join(p.basename(file)).split("\n").filter((line) => !/^\s+at |^Node\.js/.test(line)).slice(0, 8).join("\n")); process.exit(1); }
"#;
    let file = path.to_string_lossy().to_string();
    let result = run(
        "node",
        &["--no-warnings", "-e", SCRIPT, &file],
        &[],
        path.parent(),
    )?;
    (!result.success)
        .then(|| result.report())
        .filter(|text| !text.is_empty())
}

/// Accolades d'une feuille de style : une accolade jamais fermée ou fermée
/// en trop rend muette toute la suite de la feuille, sans erreur visible.
#[must_use]
pub fn css_structure(text: &str) -> Option<String> {
    let mut open: Vec<usize> = Vec::new();
    let mut line = 1;
    let mut chars = text.chars().peekable();
    let mut quote: Option<char> = None;
    while let Some(ch) = chars.next() {
        if ch == '\n' {
            line += 1;
        }
        if let Some(q) = quote {
            if ch == '\\' {
                chars.next();
            } else if ch == q || ch == '\n' {
                quote = None;
            }
            continue;
        }
        match ch {
            '/' if chars.peek() == Some(&'*') => {
                chars.next();
                let mut previous = ' ';
                for inner in chars.by_ref() {
                    if inner == '\n' {
                        line += 1;
                    }
                    if previous == '*' && inner == '/' {
                        break;
                    }
                    previous = inner;
                }
            }
            '"' | '\'' => quote = Some(ch),
            '{' => open.push(line),
            '}' => {
                if open.pop().is_none() {
                    return Some(format!(
                        "line {line}: a closing }} without an opening {{: the rules after it may be ignored"
                    ));
                }
            }
            _ => {}
        }
    }
    open.first().map(|first| {
        format!(
            "line {first}: this {{ is never closed: every rule after it is swallowed by the block"
        )
    })
}

fn toml_syntax(path: &Path) -> Option<String> {
    let file = path.to_string_lossy().to_string();
    let result = run(
        crate::harness::python_program(),
        &[
            "-I",
            "-c",
            "import sys\ntry:\n    import tomllib\nexcept ImportError:\n    sys.exit(0)\ntry:\n    tomllib.load(open(sys.argv[1], 'rb'))\nexcept tomllib.TOMLDecodeError as error:\n    print(error)\n    sys.exit(1)",
            &file,
        ],
        &[],
        None,
    )?;
    (!result.success)
        .then(|| result.report())
        .filter(|text| !text.is_empty())
}

// --- Pièges ----------------------------------------------------------------------

/// Les pièges du code d'un fichier, une phrase par constat avec sa ligne.
#[must_use]
pub fn pitfalls(path: &Path) -> Vec<String> {
    let found = match extension(path).as_str() {
        "py" | "pyw" => python_pitfalls(path),
        "ps1" | "psm1" => read(path)
            .map(|text| powershell_pitfalls(&text))
            .unwrap_or_default(),
        "bat" | "cmd" => read(path)
            .map(|text| batch_pitfalls(&text))
            .unwrap_or_default(),
        "sh" | "bash" => shell_pitfalls(path),
        // Wireframes et schémas en cadres d'un document : leurs traits doivent se rejoindre.
        "md" | "markdown" | "mdx" | "txt" => read(path)
            .map(|text| crate::ascii_art::misaligned(&text, 6))
            .map(|problems| {
                problems
                    .into_iter()
                    .map(|problem| format!("{problem}: the ASCII drawing is misaligned; redraw it with the Wireframe tool and paste its output as it is"))
                    .collect()
            })
            .unwrap_or_default(),
        "css" => read(path)
            .map(|text| css_pitfalls(&text))
            .unwrap_or_default(),
        "html" | "htm" | "js" | "mjs" | "jsx" | "ts" | "tsx" | "vue" | "svelte" | "json" => read(path)
            .map(|text| {
                let mut found = injected_test_data(&text);
                found.extend(past_dates(&text, &crate::prompt::current_date_iso()));
                found
            })
            .unwrap_or_default(),
        _ => Vec::new(),
    };
    found.into_iter().take(MAX_FINDINGS).collect()
}

/// Mots qui disent qu'une écriture dans le stockage n'est là que pour essayer.
const TEST_DATA_WORDS: &[&str] = &[
    "debug",
    "test data",
    "testdata",
    "données de test",
    "donnees de test",
    "fake data",
    "sample data",
    "mock data",
    "dummy",
];

/// Données de test ou de débogage écrites dans le stockage du navigateur au
/// chargement d'une page (`localStorage.setItem` près d'un commentaire
/// « debug », « test data »…) : à chaque ouverture, elles remplacent les
/// données de l'utilisateur. Vu le 24 octobre 2026 (session de test de Claw
/// Web) : un bloc « Debug: inject test data » laissé dans `index.html` d'une
/// appli de budget, chaque dépense ajoutée disparaissait au rechargement.
#[must_use]
pub fn injected_test_data(text: &str) -> Vec<String> {
    let lines: Vec<&str> = text.lines().collect();
    let marked = |line: &&str| {
        let lower = line.to_lowercase();
        TEST_DATA_WORDS.iter().any(|word| lower.contains(word))
    };
    lines
        .iter()
        .enumerate()
        .find(|(index, line)| {
            (line.contains("localStorage.setItem") || line.contains("sessionStorage.setItem"))
                && lines[index.saturating_sub(8)..(index + 3).min(lines.len())]
                    .iter()
                    .any(marked)
        })
        .map(|(index, _)| {
            vec![format!(
                "line {}: test or debug data written into the browser storage as the page loads: each time the user opens the app, it replaces their own data. Remove it from the project; to see the page with sample data, give them to the Screenshot tool's `storage` instead",
                index + 1
            )]
        })
        .unwrap_or_default()
}

/// Codes d'accès écrits dans un script (`const DEFAULT_PIN = '1234'`,
/// `password: "boulange"`) : ceux qui ouvrent l'espace réservé d'une page, que
/// le parcours de Screenshot essaie pour voir ce qu'il y a derrière. Les chiffres
/// d'abord ; une clé de rangement (`PIN_KEY = 'boulangerie_pin'`) n'en est pas un.
#[must_use]
pub fn access_codes(text: &str) -> Vec<String> {
    let assigned = Regex::new(
        r#"(?i)\b[\w$]*(?:pin|code|passe?|password|motdepasse|passcode|secret)[\w$]*["']?\s*(?:=|:)\s*["'`]([^"'`\s]{3,32})["'`]"#,
    )
    .expect("regex");
    let mut codes: Vec<String> = Vec::new();
    for found in assigned.captures_iter(text) {
        let whole = found[0].to_lowercase();
        let code = found[1].to_string();
        let not_access = [
            "key", "storage", "postal", "zip", "country", "promo", "coupon", "qr", "bar", "color",
            "colour", "lang", "status", "error", "area", "dial", "currency", "event", "http",
        ];
        if not_access.iter().any(|word| whole.contains(word)) || codes.contains(&code) {
            continue;
        }
        codes.push(code);
    }
    codes.sort_by_key(|code| !code.chars().all(|c| c.is_ascii_digit()));
    codes.truncate(4);
    codes
}

/// Dates d'un fichier toutes passées (`AAAA-MM-JJ`, trois au moins) : les
/// exemples d'une appli de calendrier ou d'événements datés d'avant aujourd'hui
/// (`today`, même format). Vu le 25 octobre 2026 (session #31) : un site de club
/// de foot aux matchs de 2025, « Prochain match » d'il y a un an et « Aucun
/// événement à venir » sous la liste ; le modèle avait la date du jour.
#[must_use]
pub fn past_dates(text: &str, today: &str) -> Vec<String> {
    let mut dates: Vec<(usize, &str)> = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let bytes = line.as_bytes();
        let mut start = 0;
        while start + 10 <= bytes.len() {
            // Les octets d'abord : dix octets peuvent finir au milieu d'un
            // caractère (« • », un accent), que la tranche du texte refuserait.
            let shaped = bytes[start..start + 10]
                .iter()
                .enumerate()
                .all(|(position, byte)| match position {
                    4 | 7 => *byte == b'-',
                    _ => byte.is_ascii_digit(),
                });
            let bounded = (start == 0 || !bytes[start - 1].is_ascii_alphanumeric())
                && bytes
                    .get(start + 10)
                    .is_none_or(|byte| !byte.is_ascii_digit());
            if shaped && bounded && bytes[start..].starts_with(b"20") {
                dates.push((index + 1, &line[start..start + 10]));
                start += 10;
            } else {
                start += line[start..].chars().next().map_or(1, char::len_utf8);
            }
        }
    }
    let Some(&(line, latest)) = dates.iter().max_by_key(|(_, date)| *date) else {
        return Vec::new();
    };
    if dates.len() < 3 || latest >= today {
        return Vec::new();
    }
    vec![format!(
        "line {line}: the {} dates of this file are all before today ({today}), the latest is {latest}: events, matches or deadlines shown as coming are already past, and lists of what is upcoming stay empty. Date the sample data from today onward",
        dates.len()
    )]
}

/// Ce qui est dit au modèle quand un fichier a des pièges.
#[must_use]
pub fn pitfalls_feedback(path: &str, findings: &[String]) -> String {
    format!(
        "[Claw] {path} was checked as it was written: {} pitfall(s), each a bug waiting to happen (the code runs, then does the wrong thing or fails on a real input):\n{}\nFix each one now, or say in your answer why it is intended.",
        findings.len(),
        findings
            .iter()
            .map(|finding| format!("- {finding}"))
            .collect::<Vec<_>>()
            .join("\n")
    )
}

/// Règles rapiécées dites au plus par feuille : les plus redites d'abord.
const MAX_PATCHED_RULES: usize = 3;
/// `!important` au-delà desquels une feuille se bat contre elle-même.
const MAX_IMPORTANT: usize = 20;

/// Par endroit (`@media`…) et sélecteur : chaque propriété et le nombre de
/// règles qui la posent.
type CssRules = BTreeMap<(String, String), BTreeMap<String, usize>>;

/// Par endroit et sélecteur retenu : les propriétés redites, le nombre de
/// règles, et si des sélecteurs plus longs du même élément en font partie.
type CssPiles = BTreeMap<(String, String), Vec<(String, usize, bool)>>;

/// Un bloc ouvert d'une feuille de style : une règle `@` (`@media`…) ou une
/// règle de style, avec les propriétés déjà comptées dans ce bloc.
enum CssBlock {
    At(String),
    Style {
        selectors: Vec<String>,
        counted: Vec<String>,
    },
}

/// Les règles `@` qui entourent l'endroit lu (« @media (prefers-color-scheme:
/// dark) ») : une même règle redite en clair et en sombre n'est pas rapiécée.
fn css_context(stack: &[CssBlock]) -> String {
    stack
        .iter()
        .filter_map(|block| match block {
            CssBlock::At(prelude) => Some(prelude.as_str()),
            CssBlock::Style { .. } => None,
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Les sélecteurs d'une règle, séparés aux virgules hors parenthèses
/// (`:is(a, b)`), placés sous ceux de la règle parente quand elle est imbriquée.
fn css_selectors(prelude: &str, parents: Option<&[String]>) -> Vec<String> {
    let mut parts = Vec::new();
    let mut depth = 0_i32;
    let mut current = String::new();
    for ch in prelude.chars() {
        match ch {
            '(' | '[' => depth += 1,
            ')' | ']' => depth -= 1,
            ',' if depth == 0 => {
                parts.push(std::mem::take(&mut current));
                continue;
            }
            _ => {}
        }
        current.push(ch);
    }
    parts.push(current);
    parts
        .iter()
        .map(|part| css_tidy_selector(part))
        .filter(|part| !part.is_empty())
        .flat_map(|part| match parents {
            Some(parents) if !parents.is_empty() => parents
                .iter()
                .map(|parent| {
                    if part.contains('&') {
                        part.replace('&', parent)
                    } else {
                        format!("{parent} {part}")
                    }
                })
                .collect(),
            _ => vec![part],
        })
        .collect()
}

/// Un sélecteur écrit d'une seule façon : les combinateurs (`>`, `+`, `~`)
/// entourés d'une espace, hors parenthèses et crochets, les espaces réduites.
fn css_tidy_selector(selector: &str) -> String {
    let mut out = String::new();
    let mut depth = 0_i32;
    for ch in selector.chars() {
        match ch {
            '(' | '[' => depth += 1,
            ')' | ']' => depth -= 1,
            '>' | '+' | '~' if depth == 0 => {
                out.push(' ');
                out.push(ch);
                out.push(' ');
                continue;
            }
            _ => {}
        }
        out.push(ch);
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Une déclaration lue (`color: red !important`) : ses `!important` comptés,
/// sa propriété comptée une fois pour chaque sélecteur de la règle.
fn css_declaration(
    text: &str,
    stack: &mut [CssBlock],
    context: &str,
    rules: &mut CssRules,
    important: &mut usize,
) {
    let text = text.trim();
    if text.is_empty() {
        return;
    }
    if let Some(at) = text.rfind('!') {
        if text[at + 1..]
            .trim_start()
            .to_ascii_lowercase()
            .starts_with("important")
        {
            *important += 1;
        }
    }
    let Some(CssBlock::Style { selectors, counted }) = stack.last_mut() else {
        return;
    };
    let Some((property, _)) = text.split_once(':') else {
        return;
    };
    let property = property.trim().to_ascii_lowercase();
    if property.is_empty() || property.contains(char::is_whitespace) || counted.contains(&property)
    {
        return;
    }
    counted.push(property.clone());
    for selector in selectors.iter() {
        *rules
            .entry((context.to_string(), selector.clone()))
            .or_default()
            .entry(property.clone())
            .or_default() += 1;
    }
}

/// Les règles rapiécées d'une feuille, les plus redites d'abord.
fn css_piles(rules: &CssRules) -> Vec<String> {
    // Par endroit et par propriété : les sélecteurs qui la posent, et dans
    // combien de règles. Un même élément visé par des sélecteurs de plus en
    // plus longs (`.seg span`, `.sheet .seg span`, `dialog.sheet .seg span`)
    // compte pour un : l'agent qui ne sait plus quelle règle gagne en
    // ajoute une plus précise. Cinq règles au moins pour ce groupe, trois
    // pour un sélecteur redit tel quel.
    let mut by_place: BTreeMap<(String, String), Vec<(String, usize)>> = BTreeMap::new();
    for ((context, selector), properties) in rules {
        for (property, count) in properties {
            by_place
                .entry((context.clone(), property.clone()))
                .or_default()
                .push((selector.clone(), *count));
        }
    }
    let mut piles = CssPiles::new();
    for ((context, property), selectors) in &by_place {
        let mut candidates: Vec<(&str, usize, bool)> = Vec::new();
        for (selector, own) in selectors {
            let longer = format!(" {selector}");
            let total: usize = selectors
                .iter()
                .filter(|(other, _)| other == selector || other.ends_with(&longer))
                .map(|(_, count)| count)
                .sum();
            if *own >= 3 || (total >= 5 && selector.contains(['.', '#', '['])) {
                candidates.push((selector, total, total > *own));
            }
        }
        // Le plus redit d'abord, le plus court à égalité ; un sélecteur plus
        // long du même élément est déjà dans son groupe.
        candidates.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.len().cmp(&b.0.len())));
        let mut picked: Vec<&str> = Vec::new();
        for (selector, count, longer) in candidates {
            let same = |other: &&str| {
                *other == selector
                    || selector.ends_with(&format!(" {other}"))
                    || other.ends_with(&format!(" {selector}"))
            };
            if picked.iter().any(same) {
                continue;
            }
            picked.push(selector);
            piles
                .entry((context.clone(), selector.to_string()))
                .or_default()
                .push((property.clone(), count, longer));
        }
    }
    let mut patched: Vec<(usize, String)> = Vec::new();
    for ((context, selector), mut repeated) in piles {
        repeated.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        let what = repeated
            .iter()
            .take(3)
            .map(|(property, count, _)| format!("{property} in {count}"))
            .collect::<Vec<_>>()
            .join(", ");
        let place = if context.is_empty() {
            String::new()
        } else {
            format!(" (inside {context})")
        };
        let longer = if repeated.iter().any(|(_, _, longer)| *longer) {
            ", some with longer selectors for the same element"
        } else {
            ""
        };
        patched.push((
            repeated[0].1,
            format!("`{selector}`{place} is styled again and again in separate rules{longer} ({what} rules): only one of them paints it, and the pile hides which one wins. Keep a single rule for it and change that rule in place (edit_file), instead of adding another one further down"),
        ));
    }
    patched.sort_by_key(|item| std::cmp::Reverse(item.0));
    patched.into_iter().map(|(_, line)| line).collect()
}

/// Feuille de style rapiécée : la même propriété d'un même sélecteur posée
/// dans trois règles ou plus au même endroit (hors `@media`, ou dans le même
/// `@media`) au lieu d'être changée sur place, et des `!important` partout.
/// Vu le 28 octobre 2026 (session #39) : un `styles.css` de 1 091 lignes et
/// 217 `!important`, la couleur du choix coché du volet d'ajout posée dans
/// quinze règles ; l'agent n'a plus su laquelle l'emportait (blanc sur blanc).
#[must_use]
pub fn css_pitfalls(text: &str) -> Vec<String> {
    let mut stack: Vec<CssBlock> = Vec::new();
    let mut rules = CssRules::new();
    let mut important = 0;
    let mut buffer = String::new();
    let mut chars = text.chars().peekable();
    let mut quote: Option<char> = None;
    while let Some(ch) = chars.next() {
        if let Some(q) = quote {
            buffer.push(ch);
            if ch == '\\' {
                if let Some(next) = chars.next() {
                    buffer.push(next);
                }
            } else if ch == q || ch == '\n' {
                quote = None;
            }
            continue;
        }
        match ch {
            '/' if chars.peek() == Some(&'*') => {
                chars.next();
                let mut previous = ' ';
                for inner in chars.by_ref() {
                    if previous == '*' && inner == '/' {
                        break;
                    }
                    previous = inner;
                }
            }
            '"' | '\'' => {
                quote = Some(ch);
                buffer.push(ch);
            }
            '{' => {
                let prelude = buffer.split_whitespace().collect::<Vec<_>>().join(" ");
                buffer.clear();
                if prelude.starts_with('@') {
                    stack.push(CssBlock::At(prelude));
                } else {
                    let parents = stack.iter().rev().find_map(|block| match block {
                        CssBlock::Style { selectors, .. } => Some(selectors.clone()),
                        CssBlock::At(_) => None,
                    });
                    let selectors = css_selectors(&prelude, parents.as_deref());
                    stack.push(CssBlock::Style {
                        selectors,
                        counted: Vec::new(),
                    });
                }
            }
            ';' | '}' => {
                let context = css_context(&stack);
                css_declaration(&buffer, &mut stack, &context, &mut rules, &mut important);
                buffer.clear();
                if ch == '}' {
                    stack.pop();
                }
            }
            _ => buffer.push(ch),
        }
    }
    let mut found: Vec<String> = css_piles(&rules)
        .into_iter()
        .take(MAX_PATCHED_RULES)
        .collect();
    if important > MAX_IMPORTANT {
        found.push(format!(
            "!important written {} times or more: each one overrides the kit and the previous fix, and the next fix needs another one. Remove them and give each rule the selector it needs (!important only for [hidden] and reduced motion)",
            important / 10 * 10
        ));
    }
    found
}

/// Script Python lancé sur le fichier : noms jamais définis, f oublié devant
/// une chaîne qui montre `{nom}`, argument par défaut mutable, `is` devant
/// un littéral, `except:` nu ou large et muet, `open()` sans encodage,
/// `__name__ == "main"`. Les noms se suivent avec `symtable`, comme
/// l'interpréteur les résout.
const PYTHON_PITFALLS: &str = r#"
import ast, builtins, json, re, symtable, sys
path = sys.argv[1]
raw = open(path, "rb").read()
try:
    source = raw.decode("utf-8-sig")
except UnicodeDecodeError:
    source = raw.decode("latin-1")
try:
    tree = ast.parse(source, path)
except SyntaxError:
    print("[]")
    raise SystemExit(0)
found = []
def add(node, text):
    found.append([getattr(node, "lineno", 0), text])
known = set(dir(builtins)) | {"__file__", "__name__", "__doc__", "__builtins__", "__spec__", "__loader__", "__package__", "__path__", "__annotations__", "__dict__", "__module__", "__qualname__", "__class__", "__debug__", "__cached__", "WindowsError", "reveal_type"}
star = any(isinstance(node, ast.ImportFrom) and any(alias.name == "*" for alias in node.names) for node in ast.walk(tree))
dynamic = any(word in source for word in ("globals()", "exec(", "__getattr__", "locals()", "vars()"))
assigned = set()
for node in ast.walk(tree):
    if isinstance(node, ast.Name) and isinstance(node.ctx, (ast.Store, ast.Del)):
        assigned.add(node.id)
    elif isinstance(node, ast.arg):
        assigned.add(node.arg)
    elif isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef, ast.ClassDef)):
        assigned.add(node.name)
    elif isinstance(node, (ast.Import, ast.ImportFrom)):
        for alias in node.names:
            assigned.add((alias.asname or alias.name).split(".")[0])
    elif isinstance(node, ast.Global):
        assigned.update(node.names)
if not star and not dynamic:
    table = symtable.symtable(source, path, "exec")
    module = {s.get_name() for s in table.get_symbols() if s.is_assigned() or s.is_imported()}
    for node in ast.walk(tree):
        if isinstance(node, ast.Global):
            module.update(node.names)
    missing = set()
    def visit(scope):
        for symbol in scope.get_symbols():
            name = symbol.get_name()
            if not symbol.is_referenced() or name in known or name in module:
                continue
            if scope.get_type() == "module":
                if not (symbol.is_assigned() or symbol.is_imported()):
                    missing.add(name)
            elif symbol.is_global():
                missing.add(name)
        for child in scope.get_children():
            visit(child)
    visit(table)
    for node in sorted((n for n in ast.walk(tree) if isinstance(n, ast.Name)), key=lambda n: (n.lineno, n.col_offset)):
        if isinstance(node.ctx, ast.Load) and node.id in missing:
            add(node, f"`{node.id}` is used but never defined, imported or built in: a typo, or a missing import or assignment (NameError when this line runs)")
            missing.discard(node.id)
placeholder = re.compile(r"\{([A-Za-z_][A-Za-z0-9_]*)(?:\.[A-Za-z_][A-Za-z0-9_]*|\[[^\]{}]*\])*(?:![rsa])?(?::[^{}]*)?\}")
def literal_texts(call):
    for argument in call.args:
        if isinstance(argument, ast.Constant) and isinstance(argument.value, str):
            yield argument
shown = {"print", "info", "debug", "warning", "error", "critical", "exception", "write", "input", "echo", "secho"}
calls = []
for node in ast.walk(tree):
    if isinstance(node, ast.Call):
        func = node.func
        name = func.id if isinstance(func, ast.Name) else func.attr if isinstance(func, ast.Attribute) else ""
        if name in shown:
            calls.append(node)
    elif isinstance(node, ast.Raise) and isinstance(node.exc, ast.Call):
        calls.append(node.exc)
for call in calls:
    for literal in literal_texts(call):
        names = [m.group(1) for m in placeholder.finditer(literal.value) if m.group(1) in assigned]
        if names:
            add(literal, f"the text shows {{{names[0]}}} literally: add the f prefix (f\"...\") to put the value in")
            break
for node in ast.walk(tree):
    if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef, ast.Lambda)):
        for default in node.args.defaults + [d for d in node.args.kw_defaults if d is not None]:
            mutable = isinstance(default, (ast.List, ast.Dict, ast.Set)) or (isinstance(default, ast.Call) and isinstance(default.func, ast.Name) and default.func.id in ("list", "dict", "set"))
            if mutable:
                add(default, f"the default {ast.unparse(default)} is created once and shared by every call: default to None and create it inside the function")
    elif isinstance(node, ast.Compare):
        operands = [node.left] + node.comparators
        for index, op in enumerate(node.ops):
            if isinstance(op, (ast.Is, ast.IsNot)):
                for operand in (operands[index], operands[index + 1]):
                    if isinstance(operand, ast.Constant) and not isinstance(operand.value, bool) and operand.value is not None and operand.value is not Ellipsis:
                        add(node, "`is` compares identity, not value: use == or != with a string or a number")
                        break
        if isinstance(node.left, ast.Name) and node.left.id == "__name__" and any(isinstance(c, ast.Constant) and c.value == "main" for c in node.comparators):
            add(node, "__name__ is \"__main__\", never \"main\": this block never runs")
    elif isinstance(node, ast.ExceptHandler):
        silent = all(isinstance(stmt, ast.Pass) or (isinstance(stmt, ast.Expr) and isinstance(stmt.value, ast.Constant)) for stmt in node.body)
        if node.type is None:
            add(node, "a bare `except:` also catches Ctrl+C, SystemExit and typos: catch the exceptions you expect (except ValueError:)")
        elif silent and isinstance(node.type, ast.Name) and node.type.id in ("Exception", "BaseException"):
            add(node, f"`except {node.type.id}: pass` hides every error, typos included: catch what you expect, or log the error")
    elif isinstance(node, ast.Call):
        func = node.func
        keywords = {k.arg for k in node.keywords}
        if isinstance(func, ast.Name) and func.id == "open" and "encoding" not in keywords:
            mode = node.args[1] if len(node.args) > 1 else next((k.value for k in node.keywords if k.arg == "mode"), None)
            text_mode = mode is None or (isinstance(mode, ast.Constant) and isinstance(mode.value, str) and "b" not in mode.value)
            if text_mode:
                add(node, "open() without encoding= reads and writes in the Windows code page (cp1252) on Windows: accented text breaks or raises; pass encoding=\"utf-8\"")
        elif isinstance(func, ast.Attribute) and func.attr in ("read_text", "write_text") and "encoding" not in keywords:
            add(node, f".{func.attr}() without encoding= uses the Windows code page on Windows: pass encoding=\"utf-8\"")
unique = []
for line, text in sorted(found, key=lambda item: item[0]):
    if [line, text] not in unique:
        unique.append([line, text])
print(json.dumps(unique))
"#;

fn python_pitfalls(path: &Path) -> Vec<String> {
    let file = path.to_string_lossy().to_string();
    let Some(result) = run(
        crate::harness::python_program(),
        &["-I", "-c", PYTHON_PITFALLS, &file],
        &[("PYTHONIOENCODING", "utf-8")],
        None,
    ) else {
        return Vec::new();
    };
    let Ok(found) = serde_json::from_str::<Vec<(u64, String)>>(result.stdout.trim()) else {
        return Vec::new();
    };
    found
        .into_iter()
        .map(|(line, text)| format!("line {line}: {text}"))
        .collect()
}

/// Les lignes de code d'un script `PowerShell`, sans commentaires (`#…`,
/// `<# … #>`), avec leur numéro.
fn powershell_code_lines(text: &str) -> Vec<(usize, String)> {
    let mut lines = Vec::new();
    let mut in_block = false;
    for (index, raw) in text.lines().enumerate() {
        let mut line = raw.to_string();
        if in_block {
            match line.find("#>") {
                Some(end) => {
                    line = line[end + 2..].to_string();
                    in_block = false;
                }
                None => continue,
            }
        }
        if let Some(start) = line.find("<#") {
            if let Some(end) = line[start..].find("#>") {
                line.replace_range(start..start + end + 2, "");
            } else {
                line.truncate(start);
                in_block = true;
            }
        }
        // Un commentaire de fin de ligne, hors chaîne.
        let mut quote: Option<char> = None;
        let mut cut = None;
        for (at, ch) in line.char_indices() {
            match (quote, ch) {
                (Some(q), c) if c == q => quote = None,
                (None, '"' | '\'') => quote = Some(ch),
                (None, '#') => {
                    cut = Some(at);
                    break;
                }
                _ => {}
            }
        }
        if let Some(cut) = cut {
            line.truncate(cut);
        }
        if !line.trim().is_empty() {
            lines.push((index + 1, line));
        }
    }
    lines
}

/// Le texte d'une ligne avec ses chaînes vidées (`"…"` et `'…'` deviennent
/// `""`), pour chercher des opérateurs hors du texte.
fn without_strings(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut quote: Option<char> = None;
    for ch in line.chars() {
        match (quote, ch) {
            (Some(q), c) if c == q => {
                quote = None;
                out.push(c);
            }
            (Some(_), _) => {}
            (None, '"' | '\'') => {
                quote = Some(ch);
                out.push(ch);
            }
            (None, c) => out.push(c),
        }
    }
    out
}

fn powershell_pitfalls(text: &str) -> Vec<String> {
    let mut found = Vec::new();
    let lines = powershell_code_lines(text);
    let seven = wants_powershell_7(text);
    let condition = Regex::new(r"(?i)\b(if|elseif|while|until)\s*\((.*)\)").expect("regex");
    let redirect = Regex::new(r"[^\-=2>]\s(>=?|<=?)\s").expect("regex");
    let null_right = Regex::new(r"(?i)(\$[\w:.\[\]]+|\))\s+-(eq|ne)\s+\$null\b").expect("regex");
    let assignment =
        Regex::new(r"(?i)\b(if|elseif|while)\s*\(\s*\$[\w:]+\s*=\s*[^=\s]").expect("regex");
    let seven_only = Regex::new(
        r"(?i)(ConvertFrom-Json\b[^|;]*-AsHashtable|ForEach-Object\b[^|;]*-Parallel|-AsByteStream|-SkipCertificateCheck|Get-Error\b|\bClean\s*\{)",
    )
    .expect("regex");
    let encoding_default =
        Regex::new(r"(?i)\b(Out-File|Set-Content|Add-Content)\b").expect("regex");
    for (number, line) in &lines {
        let code = without_strings(line);
        if let Some(found_condition) = condition.captures(&code) {
            if redirect.is_match(&format!(" {} ", &found_condition[2])) {
                found.push(format!("line {number}: > and < redirect output to a file in PowerShell, they do not compare: use -gt, -lt, -ge or -le"));
            }
        }
        if null_right.is_match(&code) {
            found.push(format!("line {number}: put $null on the left ($null -eq $x): with an array on the left, -eq filters the array instead of comparing it"));
        }
        if assignment.is_match(&code) {
            found.push(format!(
                "line {number}: = assigns in PowerShell; to compare, use -eq"
            ));
        }
        if !seven {
            if let Some(matched) = seven_only.find(&code) {
                found.push(format!(
                    "line {number}: {} exists only in PowerShell 7, not in Windows PowerShell 5.1 that every Windows has: use a 5.1 way, or add #Requires -Version 7",
                    matched.as_str().split_whitespace().next().unwrap_or("this")
                ));
            }
            if encoding_default.is_match(&code) && !code.to_ascii_lowercase().contains("-encoding")
            {
                found.push(format!("line {number}: without -Encoding, Windows PowerShell 5.1 writes Out-File in UTF-16 and Set-Content in Windows-1252: add -Encoding utf8"));
            }
        }
    }
    if !Regex::new(r"(?i)\$ErrorActionPreference\s*=\s*['\x22]?Stop")
        .is_ok_and(|pattern| pattern.is_match(text))
    {
        found.extend(unseen_failures(&lines));
    }
    found
}

/// Un `try` dont le bloc appelle une commande de fichiers, de services ou de
/// processus sans `-ErrorAction Stop` : ses échecs ne sont pas « terminants »,
/// et le `catch` ne les voit jamais.
fn unseen_failures(lines: &[(usize, String)]) -> Vec<String> {
    let cmdlet = Regex::new(
        r"(?i)\b(Get|Set|Copy|Move|Remove|New|Rename|Add|Clear|Import|Export|Stop|Start|Restart|Get)-(Item|ChildItem|Content|ItemProperty|Csv|Service|Process|Acl|Location)\b",
    )
    .expect("regex");
    let mut found = Vec::new();
    let mut index = 0;
    while index < lines.len() {
        let (number, line) = &lines[index];
        let lower = line.trim_start().to_ascii_lowercase();
        if !(lower.starts_with("try") && lower[3..].trim_start().starts_with(['{', ' '])
            || lower == "try")
        {
            index += 1;
            continue;
        }
        // Le bloc du try : jusqu'à la ligne de son catch.
        let mut end = index;
        while end < lines.len() && !lines[end].1.to_ascii_lowercase().contains("catch") {
            end += 1;
        }
        let body = &lines[index..end.min(lines.len())];
        let silent = body.iter().any(|(_, code)| {
            cmdlet.is_match(code)
                && !code.to_ascii_lowercase().contains("-erroraction")
                && !code.to_ascii_lowercase().contains("-ea ")
        });
        if silent && end < lines.len() {
            found.push(format!("line {number}: the commands in this try report most failures (a missing file, a stopped service) without stopping, so the catch never runs: add -ErrorAction Stop to them, or $ErrorActionPreference = 'Stop' at the top"));
        }
        index = end.max(index + 1);
    }
    found
}

/// Ligne d'un `.bat` sans ses chaînes ni ses caractères échappés (`^(`).
fn batch_code(line: &str) -> String {
    let mut out = String::new();
    let mut chars = line.chars();
    let mut quoted = false;
    while let Some(ch) = chars.next() {
        match ch {
            '^' if !quoted => {
                chars.next();
            }
            '"' => quoted = !quoted,
            c if !quoted => out.push(c),
            _ => {}
        }
    }
    out
}

fn batch_pitfalls(text: &str) -> Vec<String> {
    let mut found = Vec::new();
    let label = Regex::new(r"^\s*:([A-Za-z0-9_.\-]+)").expect("regex");
    let goto = Regex::new(r"(?i)\bgoto\s+:?([A-Za-z0-9_.\-]+)").expect("regex");
    let call = Regex::new(r"(?i)\bcall\s+:([A-Za-z0-9_.\-]+)").expect("regex");
    let spaced_set =
        Regex::new(r#"(?i)^\s*@?\s*set\s+(/p\s+)?"?([A-Za-z_][\w.\-]*)\s+="#).expect("regex");
    let block_set =
        Regex::new(r#"(?i)\bset\s+(?:/a\s+|/p\s+)?"?([A-Za-z_][\w.\-]*)\s*="#).expect("regex");
    let empty_if = Regex::new(r"(?i)\bif\s+(not\s+)?%[\w~]+%\s*==").expect("regex");
    let bare_exit = Regex::new(r"(?i)^\s*@?exit(\s+\d+)?\s*$").expect("regex");
    let comment = Regex::new(r"(?i)^\s*(@?rem\b|::)").expect("regex");
    let lines: Vec<&str> = text.lines().collect();
    let labels: std::collections::HashSet<String> = lines
        .iter()
        .filter(|line| !line.trim_start().starts_with("::"))
        .filter_map(|line| label.captures(line))
        .map(|captures| captures[1].to_ascii_lowercase())
        .collect();
    let mut depth: i32 = 0;
    let mut block_vars: Vec<String> = Vec::new();
    let mut reported: Vec<String> = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        let number = index + 1;
        if comment.is_match(line) || label.is_match(line) && !line.contains('(') {
            continue;
        }
        let code = batch_code(line);
        for captures in goto.captures_iter(&code).chain(call.captures_iter(&code)) {
            let target = captures[1].to_ascii_lowercase();
            if target != "eof" && !labels.contains(&target) {
                found.push(format!(
                    "line {number}: no :{} label exists in this file, so this jump fails",
                    &captures[1]
                ));
            }
        }
        if let Some(captures) = spaced_set.captures(line) {
            found.push(format!(
                "line {number}: set {name} = ... makes a variable named \"{name} \" (with the space) holding a value that starts with a space: write set \"{name}=value\"",
                name = &captures[2]
            ));
        }
        if empty_if.is_match(line) {
            found.push(format!("line {number}: if %X%==value is a syntax error when X is empty: quote both sides, if \"%X%\"==\"value\""));
        }
        if bare_exit.is_match(line) {
            found.push(format!("line {number}: exit without /b closes the whole console when the script is run from cmd: use exit /b with the code"));
        }
        let trimmed = line.trim_start().to_ascii_lowercase();
        if depth > 0 {
            if trimmed.starts_with("echo") && code.contains('(') && code.contains(')') {
                found.push(format!("line {number}: the ) in this echo closes the ( ) block it is in: escape it as ^) or put the text in quotes"));
            }
            for var in &block_vars {
                let percent = format!("%{}%", var.to_ascii_lowercase());
                if line.to_ascii_lowercase().contains(&percent) && !reported.contains(var) {
                    reported.push(var.clone());
                    found.push(format!("line {number}: %{var}% inside a ( ) block is read when the block starts, before set {var}= changes it: use setlocal EnableDelayedExpansion and !{var}!"));
                }
            }
            if line.to_ascii_lowercase().contains("%errorlevel%")
                && !reported.contains(&"errorlevel".to_string())
            {
                reported.push("errorlevel".to_string());
                found.push(format!("line {number}: %errorlevel% inside a ( ) block is read when the block starts: use if errorlevel 1 (or !errorlevel! with delayed expansion)"));
            }
        }
        let opens = i32::try_from(code.matches('(').count()).unwrap_or(0);
        let closes = i32::try_from(code.matches(')').count()).unwrap_or(0);
        if depth > 0 || opens > closes {
            for captures in block_set.captures_iter(&code) {
                block_vars.push(captures[1].to_string());
            }
        }
        depth += opens - closes;
        if depth <= 0 {
            depth = 0;
            block_vars.clear();
        }
    }
    let opens = text
        .lines()
        .filter(|line| !comment.is_match(line))
        .map(|line| batch_code(line).matches('(').count())
        .sum::<usize>();
    let closes = text
        .lines()
        .filter(|line| !comment.is_match(line))
        .map(|line| batch_code(line).matches(')').count())
        .sum::<usize>();
    if opens != closes {
        found.push(format!("the file has {opens} ( for {closes} ): a block is not closed, or an unescaped ) in an echo closes one too early"));
    }
    if !text.is_ascii() && !text.to_ascii_lowercase().contains("chcp 65001") {
        found.push("the file has accented or special characters but no chcp 65001 >nul at the top: cmd shows them garbled".to_string());
    }
    found
}

/// Une commande bash qui passe du `PowerShell` entre guillemets doubles avec
/// des variables (`powershell -Command "… $_ …"`) : bash les remplace avant
/// que `PowerShell` ne lise la commande. Vu dans un essai réel : `$_.FullName`
/// est devenu `14377.FullName`, et le modèle a cherché quatre fois pourquoi.
#[must_use]
pub fn bash_powershell_quoting(command: &str) -> Option<&'static str> {
    let pattern = Regex::new(
        r#"(?i)\b(powershell|pwsh)(\.exe)?\b[^|;&]*?\s-(c|command)\s+"[^"]*\$[A-Za-z_?(]"#,
    )
    .ok()?;
    pattern.is_match(command).then_some(
        "[Claw] This command passed PowerShell code to bash in double quotes: bash replaced its $variables before PowerShell read it ($_ becomes the last argument of the previous command, $x becomes empty, $(...) runs in bash). Put the PowerShell code in single quotes (-Command '...'), or in a .ps1 file run with -File.",
    )
}

/// shellcheck, quand il est installé : ses avertissements et ses erreurs.
fn shell_pitfalls(path: &Path) -> Vec<String> {
    let file = path.to_string_lossy().to_string();
    let Some(result) = run(
        "shellcheck",
        &["-f", "gcc", "-S", "warning", &file],
        &[],
        path.parent(),
    ) else {
        return Vec::new();
    };
    result
        .stdout
        .lines()
        .filter_map(|line| {
            // fichier:ligne:colonne: niveau: message [SCxxxx]
            let mut parts = line.splitn(4, ':');
            let (_, number, _, message) =
                (parts.next()?, parts.next()?, parts.next()?, parts.next()?);
            Some(format!("line {number}:{}", message.trim_end()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    #[test]
    fn sample_dates_all_in_the_past_are_found() {
        // Session #31 : les matchs d'un club en 2025, un an avant la session.
        let data = "const MATCHES = [\n  { date: \"2025-10-14\" },\n  { date: \"2025-10-21\" },\n  { date: \"2025-11-04\" },\n];";
        let found = super::past_dates(data, "2026-10-25");
        assert_eq!(found.len(), 1);
        assert!(found[0].starts_with("line 4: the 3 dates of this file are all before today (2026-10-25), the latest is 2025-11-04"));
        // Une date à venir suffit ; moins de trois dates ou un numéro ne comptent pas.
        assert!(
            super::past_dates(&data.replace("2025-11-04", "2026-11-04"), "2026-10-25").is_empty()
        );
        assert!(
            super::past_dates("créé le 2025-01-02, revu le 2025-03-04", "2026-10-25").is_empty()
        );
        // Session #31 relancée : un « • » coupé en deux faisait tomber Claw.
        assert_eq!(
            super::past_dates(
                "<p>U11 • 2025-10-14 • 2025-10-21 • 2025-11-04 é</p>\nabcdefghi• Équipe",
                "2026-10-25"
            )
            .len(),
            1
        );
        assert!(super::past_dates(
            "v12025-10-14 x2025-10-21 2025-10-210 2025-10-1",
            "2026-10-25"
        )
        .is_empty());
    }

    #[test]
    fn access_codes_written_in_a_script_are_found_and_storage_keys_are_not() {
        // Session #32, passage 5 : l'espace boulanger et son code par défaut.
        let script = "const PIN_KEY = 'boulangerie_pin';\n  const DEFAULT_PIN = '1234';\n  const config = { adminPassword: \"fournil\" };\n  const productCode = \"1234\";";
        assert_eq!(super::access_codes(script), ["1234", "fournil"]);
        assert!(super::access_codes("let total = 0; const name = 'Léa';").is_empty());
        assert!(super::access_codes("<input type=\"password\" id=\"pin-input\">").is_empty());
        assert!(super::access_codes(
            "const shop = { postalCode: \"75001\", promoCode: 'BIENVENUE', countryCode: 'FR' };"
        )
        .is_empty());
    }

    #[test]
    fn test_data_injected_into_a_page_is_found_and_real_storage_is_not() {
        let page = "<body>\n  <!-- Debug: inject test data -->\n  <script>\n    var t = [{\"amount\": 2500}];\n    localStorage.setItem('expenses', JSON.stringify(t));\n  </script>\n</body>";
        let found = super::injected_test_data(page);
        assert_eq!(found.len(), 1);
        assert!(
            found[0].starts_with("line 5: test or debug data"),
            "{found:?}"
        );
        // Une appli qui enregistre les données de l'utilisateur : rien à dire.
        let app = "function save(expenses) {\n  // Enregistre les dépenses du mois\n  localStorage.setItem(key, JSON.stringify(expenses));\n}";
        assert!(super::injected_test_data(app).is_empty());
    }

    use super::{batch_pitfalls, css_structure, pitfalls_feedback, powershell_pitfalls};

    /// Un dossier neuf pour les fichiers d'un essai.
    fn scratch(name: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("claw-code-checks-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dossier");
        dir
    }

    #[test]
    fn python_pitfalls_come_from_the_interpreter_itself() {
        if crate::paths::find_executable(crate::harness::python_program()).is_none() {
            return;
        }
        let dir = scratch("py");
        let file = dir.join("rapport.py");
        std::fs::write(
            &file,
            "import json\n\ndef load(path, cache=[]):\n    with open(path) as handle:\n        data = json.load(handle)\n    total = sum(data)\n    print(\"Total : {total}\")\n    return totl\n\ndef main():\n    try:\n        load('a.json')\n    except:\n        pass\n    if 'a' is 'b':\n        pass\n\nif __name__ == \"main\":\n    main()\n",
        )
        .expect("fichier");
        let found = super::pitfalls(&file).join("\n");
        let _ = std::fs::remove_dir_all(&dir);
        for expected in [
            "line 3: the default []",
            "line 4: open() without encoding=",
            "line 7: the text shows {total} literally",
            "line 8: `totl` is used but never defined",
            "line 13: a bare `except:`",
            "line 15: `is` compares identity",
            "line 18: __name__ is \"__main__\"",
        ] {
            assert!(found.contains(expected), "{expected}\n{found}");
        }
        // Un code correct ne dit rien : les noms des fonctions, des
        // paramètres, des imports et des compréhensions sont connus.
        let dir = scratch("py-ok");
        let file = dir.join("ok.py");
        std::fs::write(
            &file,
            "import sys\nfrom pathlib import Path\n\nLIMIT = 3\n\nclass Store:\n    def __init__(self, items=None):\n        self.items = items or []\n\n    def first(self):\n        return [item for item in self.items if item][:LIMIT]\n\ndef main(argv=None):\n    path = Path(argv[0] if argv else 'x.txt')\n    text = path.read_text(encoding='utf-8')\n    print(f\"{len(text)} caractères\")\n    try:\n        return int(text)\n    except ValueError as error:\n        print(error, file=sys.stderr)\n        return 1\n\nif __name__ == '__main__':\n    sys.exit(main(sys.argv[1:]))\n",
        )
        .expect("fichier");
        let found = super::pitfalls(&file);
        let _ = std::fs::remove_dir_all(&dir);
        assert!(found.is_empty(), "{found:?}");
    }

    #[test]
    fn powershell_typescript_and_toml_syntax_use_their_own_parsers() {
        let dir = scratch("syntax");
        if crate::paths::find_executable("powershell").is_some()
            || crate::paths::find_executable("pwsh").is_some()
        {
            let script = dir.join("deploy.ps1");
            std::fs::write(
                &script,
                "function Get-X {\n  param([string]$a\n  Write-Output $a\n}\n",
            )
            .expect("script");
            let error = super::syntax(&script).expect("erreur");
            assert!(error.starts_with("line 2:"), "{error}");
            std::fs::write(
                &script,
                "param([string]$Name = 'monde')\nWrite-Output \"Bonjour $Name\"\n",
            )
            .expect("script");
            assert_eq!(super::syntax(&script), None);
            // La syntaxe de PowerShell 7 échoue dans Windows PowerShell 5.1.
            if crate::paths::find_executable("powershell").is_some() {
                std::fs::write(&script, "git status && Write-Output 'ok'\n").expect("script");
                let error = super::syntax(&script).expect("erreur 5.1");
                assert!(error.contains("Windows PowerShell 5.1"), "{error}");
            }
        }
        // Node sait retirer les types ici (22.6 et plus, et assez de mémoire
        // pour son analyseur en WebAssembly, ce qu'une machine d'intégration
        // continue chargée n'a pas toujours).
        let strips_types = std::process::Command::new("node")
            .args([
                "--no-warnings",
                "-e",
                "require('node:module').stripTypeScriptTypes('let a: number = 1')",
            ])
            .output()
            .is_ok_and(|output| output.status.success());
        if strips_types {
            let module = dir.join("prix.ts");
            std::fs::write(&module, "interface Item { price: number }\nexport function total(items: Item[]): number {\n  return items.reduce((sum, item) => sum + item.price, 0);\n}\n").expect("ts");
            assert_eq!(super::syntax(&module), None);
            std::fs::write(&module, "export const total: number = ;\n").expect("ts");
            assert!(super::syntax(&module).is_some());
        }
        if crate::paths::find_executable(crate::harness::python_program()).is_some() {
            let config = dir.join("config.toml");
            std::fs::write(&config, "[server]\nport = 8080\nname = \"site\"\n").expect("toml");
            assert_eq!(super::syntax(&config), None);
            std::fs::write(&config, "[server]\nport = \n").expect("toml");
            // Sans tomllib (Python antérieur à 3.11), rien n'est dit.
            if let Some(error) = super::syntax(&config) {
                assert!(error.contains("line 2"), "{error}");
            }
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn powershell_code_in_double_quotes_from_bash_is_flagged() {
        use super::bash_powershell_quoting;
        assert!(bash_powershell_quoting(
            "powershell -NoProfile -Command \"Get-ChildItem x | ForEach-Object { $_.FullName }\""
        )
        .is_some());
        assert!(bash_powershell_quoting("pwsh -c \"$env:PATH\"").is_some());
        // Entre guillemets simples, avec -File, ou sans variable : rien.
        assert!(
            bash_powershell_quoting("powershell -Command 'Get-ChildItem | % { $_.Name }'")
                .is_none()
        );
        assert!(bash_powershell_quoting(
            "powershell -NoProfile -File .\\ranger.ps1 -Path \"C:\\x\" -WhatIf"
        )
        .is_none());
        assert!(bash_powershell_quoting("powershell -Command \"Get-Date\"").is_none());
    }

    /// Session #39 (28 octobre 2026) : le choix coché du volet d'ajout posé
    /// dans des règles de plus en plus longues, en clair, en sombre et
    /// ailleurs, avec des `!important` partout.
    #[test]
    fn a_patched_stylesheet_names_its_pile_of_rules() {
        use std::fmt::Write as _;
        let mut sheet = String::from(
            ".seg.seg--choice label span { color: #333; padding: 8px; }\n\
             .sheet .seg.seg--choice label span { color: #111 !important; }\n\
             .sheet__body .seg.seg--choice label span { color: #000 !important; }\n\
             dialog.sheet .sheet__body .seg.seg--choice label span { color: #0f172a !important; }\n\
             .sheet .sheet__body .seg.seg--choice label span{color:#0f172a!important}\n\
             @media (prefers-color-scheme: dark) { .seg.seg--choice label span { color: #f5f5f5; } }\n\
             .row__amount { color: red; }\n.row__amount { color: green; }\n.row__amount { color: var(--income); }\n",
        );
        for index in 0..18 {
            let _ = writeln!(sheet, ".extra-{index} {{ margin: 0 !important; }}");
        }
        let found = super::css_pitfalls(&sheet);
        assert_eq!(found.len(), 3, "{found:?}");
        assert!(
            found[0].starts_with("`.seg.seg--choice label span` is styled again and again in separate rules, some with longer selectors for the same element (color in 5 rules"),
            "{found:?}"
        );
        assert!(
            found[1].starts_with(
                "`.row__amount` is styled again and again in separate rules (color in 3 rules)"
            ),
            "{found:?}"
        );
        assert!(
            found[2].starts_with("!important written 20 times or more"),
            "{found:?}"
        );
        assert!(
            !found
                .iter()
                .any(|line| line.contains("prefers-color-scheme")),
            "{found:?}"
        );
    }

    /// Une feuille tenue : jetons du clair et du sombre, tailles d'écran,
    /// repli dans une même règle, variantes d'un même élément selon son
    /// conteneur ; le kit d'appli aussi.
    #[test]
    fn a_tidy_stylesheet_has_no_pile_of_rules() {
        let sheet = ":root { --accent: #5b4bd6; }\n\
            @media (prefers-color-scheme: dark) { :root { --accent: #9085e9; } }\n\
            :root[data-theme=\"dark\"] { --accent: #9085e9; }\n\
            .card { padding: 12px; background: #fff; background: color-mix(in srgb, #fff 90%, var(--accent)); }\n\
            @media (min-width: 600px) { .card { padding: 16px; } }\n\
            @media (min-width: 900px) { .card { padding: 20px; } }\n\
            .row span { color: #333; }\n.card .row span { color: #555; }\n.hero .row span { color: #fff; }\n\
            .seg span { color: #555; }\n.seg input:checked + span { color: #111; }\n\
            a[href~=\"x\"] > b { color: red; } /* .row span { color: blue } */\n\
            [hidden] { display: none !important; }\n";
        assert_eq!(super::css_pitfalls(sheet), Vec::<String>::new());
        let kit = include_str!("../../tools/src/components/app-kit.css");
        assert_eq!(super::css_pitfalls(kit), Vec::<String>::new());
    }

    #[test]
    fn css_braces_are_counted_outside_comments_and_strings() {
        assert_eq!(
            css_structure("a { color: red; }\n/* { */\nb::after { content: \"}\"; }"),
            None
        );
        assert!(css_structure("a { color: red;\nb { color: blue; }")
            .expect("ouverte")
            .starts_with("line 1: this { is never closed"));
        assert!(css_structure("a { }\n}\n")
            .expect("en trop")
            .starts_with("line 2: a closing }"));
    }

    #[test]
    fn powershell_pitfalls_are_found_with_their_line() {
        let script = "param([int]$n)\n# if ($n > 3) dans un commentaire\nif ($n > 3) { 'grand' }\nif ($items -eq $null) { 'vide' }\nif ($null -eq $items) { 'ok' }\nif ($n = 4) { 'quatre' }\n$data = Get-Content x.json | ConvertFrom-Json -AsHashtable\n'texte' | Out-File rapport.txt\ntry {\n  Get-Item C:\\absent\n} catch {\n  'jamais'\n}\nWrite-Output \"a > b\"\n";
        let found = powershell_pitfalls(script);
        let text = found.join("\n");
        assert!(text.contains("line 3: > and <"), "{text}");
        assert!(!text.contains("line 2:"), "{text}");
        assert!(text.contains("line 4: put $null on the left"), "{text}");
        assert!(!text.contains("line 5:"), "{text}");
        assert!(text.contains("line 6: = assigns"), "{text}");
        assert!(
            text.contains("line 7: ConvertFrom-Json exists only in PowerShell 7"),
            "{text}"
        );
        assert!(text.contains("line 8: without -Encoding"), "{text}");
        assert!(text.contains("line 9: the commands in this try"), "{text}");
        assert!(!text.contains("line 14"), "{text}");
        // Un script pour PowerShell 7, avec Stop : rien sur ces points.
        let modern = "#Requires -Version 7\n$ErrorActionPreference = 'Stop'\n$h = '{}' | ConvertFrom-Json -AsHashtable\ntry { Get-Item x } catch { 'non' }\n";
        assert!(
            powershell_pitfalls(modern).is_empty(),
            "{:?}",
            powershell_pitfalls(modern)
        );
    }

    #[test]
    fn batch_pitfalls_are_found_with_their_line() {
        let script = "@echo off\r\nset name = Ana\r\nif %choice%==1 goto menu\r\ncall :helper\r\nif exist data.txt (\r\n  set count=1\r\n  echo %count%\r\n  copy data.txt backup\\\r\n  echo Fin (ok)\r\n  if %errorlevel% neq 0 echo erreur\r\n)\r\nexit 1\r\n:helper\r\nexit /b 0\r\n";
        let text = batch_pitfalls(script).join("\n");
        assert!(text.contains("line 2: set name = ..."), "{text}");
        assert!(text.contains("line 3: if %X%==value"), "{text}");
        assert!(text.contains("line 3: no :menu label"), "{text}");
        assert!(!text.contains("helper"), "{text}");
        assert!(
            text.contains("line 7: %count% inside a ( ) block"),
            "{text}"
        );
        assert!(text.contains("line 9: the ) in this echo"), "{text}");
        assert!(
            text.contains("line 10: %errorlevel% inside a ( ) block"),
            "{text}"
        );
        assert!(text.contains("line 12: exit without /b"), "{text}");
        // Un script propre : rien.
        let clean = "@echo off\r\nsetlocal EnableDelayedExpansion\r\nset \"name=Ana\"\r\nif \"%~1\"==\"\" goto usage\r\nif exist data.txt (\r\n  set count=1\r\n  echo !count!\r\n  echo Fin ^(ok^)\r\n)\r\nexit /b 0\r\n:usage\r\necho usage: build.bat fichier\r\nexit /b 1\r\n";
        assert!(
            batch_pitfalls(clean).is_empty(),
            "{:?}",
            batch_pitfalls(clean)
        );
        let feedback = pitfalls_feedback("build.bat", &["line 2: x".to_string()]);
        assert!(feedback.contains("build.bat was checked") && feedback.contains("- line 2: x"));
    }
}
