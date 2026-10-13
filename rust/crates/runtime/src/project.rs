//! Mode projet : créer un projet complet de A à Z avec un modèle gratuit.
//!
//! Le modèle tient trois fichiers à la racine du projet, que Claw relit à
//! chaque tour pour savoir où en est le projet et ce qu'il doit vérifier :
//! `SPEC.md` (exigences observables `R1`, cas limites `E1`), `PLAN.md`
//! (jalons `M1` avec leur commande `Verify` et leur état, démonstration
//! finale) et `DECISIONS.md` (choix et raisons). Ce que Claw retient lui-même
//! (mode actif, plan relu, démonstration réussie) est dans
//! `.claw/project.json`. Suivi : `docs/suivi/p1-mode-projet.md`.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::harness::{CLAW_NOTE_PREFIX, GUIDE_PREFIX};
use crate::team::{has_word_start, without_accents};

pub const SPEC_FILE: &str = "SPEC.md";
pub const PLAN_FILE: &str = "PLAN.md";
pub const DECISIONS_FILE: &str = "DECISIONS.md";
const STATE_FILE: &str = "project.json";

/// Renvois au plus, par tour, pour un jalon dont la vérification échoue.
pub const MAX_GATE_ROUNDS: usize = 3;
/// Longueur gardée du texte d'une exigence montrée au modèle.
const ITEM_TEXT_MAX_CHARS: usize = 300;
/// Longueur gardée des exigences jointes au contrôle et à la relecture.
const REFERENCE_MAX_CHARS: usize = 4_000;

fn fold(text: &str) -> String {
    without_accents(&text.to_lowercase()).replace('\u{2019}', "'")
}

fn digest(parts: &[String]) -> String {
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update(part.as_bytes());
        hasher.update([0]);
    }
    hasher
        .finalize()
        .iter()
        .take(8)
        .fold(String::new(), |mut text, byte| {
            let _ = write!(text, "{byte:02x}");
            text
        })
}

fn clipped(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_string();
    }
    text.chars().take(limit).collect::<String>() + "..."
}

/// « R1, R2 et E3 » écrit pour le modèle : `R1, R2, E3`.
fn listed(ids: &[String]) -> String {
    ids.join(", ")
}

// --- État gardé par Claw ----------------------------------------------------------------

/// Ce que Claw retient du projet d'une session à l'autre (`.claw/project.json`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectState {
    /// Le mode projet est actif pour ce dossier.
    #[serde(default)]
    pub active: bool,
    /// Empreinte des exigences et des jalons déjà relus par un autre modèle.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub design_reviewed: Option<String>,
    /// Empreinte du plan dont la démonstration finale a réussi.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub demo_passed: Option<String>,
    /// Requêtes au modèle passées sur chaque jalon terminé.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub milestone_requests: BTreeMap<String, u64>,
}

impl ProjectState {
    fn path(root: &Path) -> PathBuf {
        root.join(".claw").join(STATE_FILE)
    }

    /// L'état enregistré, ou un état vide (mode inactif).
    #[must_use]
    pub fn load(root: &Path) -> Self {
        std::fs::read_to_string(Self::path(root))
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, root: &Path) -> std::io::Result<()> {
        let path = Self::path(root);
        if let Some(folder) = path.parent() {
            std::fs::create_dir_all(folder)?;
        }
        let text = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
        std::fs::write(path, text + "\n")
    }
}

// --- Ce que demande l'utilisateur ------------------------------------------------------

/// Ce que le message de l'utilisateur dit du mode projet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wish {
    /// « mode projet » : l'activer pour ce dossier.
    Start,
    /// « sans mode projet » : l'arrêter pour ce dossier.
    Stop,
    Nothing,
}

const STOP_PHRASES: &[&str] = &[
    "sans mode projet",
    "sans le mode projet",
    "hors mode projet",
    "quitte le mode projet",
    "quitter le mode projet",
    "arrete le mode projet",
    "arreter le mode projet",
    "desactive le mode projet",
    "desactiver le mode projet",
    "sors du mode projet",
    "sortir du mode projet",
    "fin du mode projet",
    "without project mode",
    "exit project mode",
    "leave project mode",
    "stop project mode",
    "disable project mode",
    "turn off project mode",
    "no project mode",
];

const START_PHRASES: &[&str] = &["mode projet", "project mode"];

/// Ce que le message dit du mode projet. Une question (« qu'est-ce que le
/// mode projet ? ») ne l'active pas.
#[must_use]
pub fn read_wish(request: &str) -> Wish {
    let text = fold(request);
    if STOP_PHRASES.iter().any(|phrase| text.contains(phrase)) {
        Wish::Stop
    } else if START_PHRASES.iter().any(|phrase| text.contains(phrase))
        && !request.trim_end().ends_with('?')
    {
        Wish::Start
    } else {
        Wish::Nothing
    }
}

/// Verbes d'une demande de création (début de mot ; une espace finale : mot entier).
const CREATION_VERBS: &[&str] = &[
    "cree",
    "construi",
    "developp",
    "realis",
    "concoi",
    "concevoir ",
    "fabriqu",
    "ecri",
    "fais ",
    "faites ",
    "faire ",
    "programm",
    "code ",
    "coder ",
    "codez ",
    "implement",
    "genere ",
    "generer ",
    "generez ",
    "monte ",
    "monter ",
    "montez ",
    "mets en place",
    "met en place",
    "mettre en place",
    "prepare ",
    "preparer ",
    "create",
    "build",
    "make ",
    "write",
    "develop",
    "generate ",
    "set up",
    "scaffold",
];

/// Ce qu'on crée : une application, un site, un outil...
const PROJECT_NOUNS: &[&str] = &[
    "application",
    "appli",
    "app ",
    "apps ",
    "site",
    "projet",
    "outil",
    "logiciel",
    "programme",
    "jeu ",
    "jeux ",
    "api ",
    "script",
    "bot ",
    "chatbot",
    "service",
    "serveur",
    "interface",
    "tableau de bord",
    "dashboard",
    "gestionnaire",
    "gestion ",
    "systeme",
    "plateforme",
    "extension",
    "plugin",
    "cli ",
    "calculatrice",
    "convertisseur",
    "generateur",
    "simulateur",
    "editeur",
    "agenda",
    "calendrier",
    "boutique",
    "blog",
    "bibliotheque",
    "inventaire",
    "carnet",
    "annuaire",
    "catalogue",
    "quiz",
    "game",
    "tool",
    "website",
    "web app",
    "webapp",
    "project",
    "program",
    "software",
    "server",
    "platform",
    "library",
    "package",
    "backend",
    "frontend",
    "calculator",
    "converter",
    "generator",
    "simulator",
    "editor",
    "planner",
    "tracker",
    "todo",
    "to-do",
];

/// Tournures qui disent un besoin sans verbe de création (« je veux une
/// application de... »).
const WISH_PHRASES: &[&str] = &[
    "je veux",
    "je voudrais",
    "j'aimerais",
    "il me faut",
    "il nous faut",
    "j'ai besoin",
    "on a besoin",
    "nous avons besoin",
    "i want",
    "i need",
    "i'd like",
    "i would like",
    "we need",
    "we want",
];

/// Tournures qui disent à elles seules un projet complet.
const WHOLE_PROJECT_PHRASES: &[&str] = &[
    "de a a z",
    "from scratch",
    "projet complet",
    "application complete",
    "appli complete",
    "site complet",
    "complete project",
    "complete app",
    "full project",
    "full app",
];

/// Mots au moins d'une demande de création : « écris un script bonjour » ne
/// vaut pas un cahier des charges et un plan.
const CREATION_MIN_WORDS: usize = 8;

/// La demande crée un projet : un verbe de création (ou un besoin) et ce qu'il
/// faut créer, en au moins huit mots ; ou « de A à Z », « from scratch ».
#[must_use]
pub fn creation_request(request: &str) -> bool {
    let text = fold(request);
    if WHOLE_PROJECT_PHRASES
        .iter()
        .any(|phrase| has_word_start(&text, phrase))
    {
        return true;
    }
    let wants = CREATION_VERBS
        .iter()
        .chain(WISH_PHRASES)
        .any(|word| has_word_start(&text, word));
    let object = PROJECT_NOUNS.iter().any(|word| has_word_start(&text, word));
    wants && object && request.split_whitespace().count() >= CREATION_MIN_WORDS
}

/// Extensions des fichiers qui ne sont pas du code : documents, notes, images
/// (maquettes). Un dossier qui n'a que cela est encore vide de projet.
const DOCUMENT_EXTENSIONS: &[&str] = &[
    "md", "txt", "rst", "adoc", "org", "pdf", "doc", "docx", "odt", "rtf", "png", "jpg", "jpeg",
    "gif", "svg", "webp", "bmp", "ico",
];
const DOCUMENT_NAMES: &[&str] = &[
    "license",
    "licence",
    "readme",
    "authors",
    "notice",
    "changelog",
    "copying",
];
/// Fichiers regardés au plus pour décider qu'un dossier est vide.
const EMPTY_FOLDER_MAX_FILES: usize = 500;

fn is_document(path: &Path) -> bool {
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    if name.starts_with('.') {
        return true;
    }
    match path.extension() {
        Some(extension) => {
            DOCUMENT_EXTENSIONS.contains(&extension.to_string_lossy().to_lowercase().as_str())
        }
        None => DOCUMENT_NAMES.contains(&name.as_str()),
    }
}

/// Dossier caché, de dépendances ou de sortie de build : sauté.
fn skipped_dir(path: &Path, root: &Path) -> bool {
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    name.starts_with('.') || crate::file_ops::generated_dir(path, &name, root)
}

/// Le dossier n'a encore aucun fichier de code ni de données : seulement des
/// documents (README, notes, licence, maquettes) et des dossiers cachés.
#[must_use]
pub fn empty_folder(root: &Path) -> bool {
    let walker = walkdir::WalkDir::new(root)
        .max_depth(4)
        .into_iter()
        .filter_entry(|entry| {
            entry.depth() == 0 || !(entry.file_type().is_dir() && skipped_dir(entry.path(), root))
        });
    let mut seen = 0;
    for entry in walker.flatten() {
        if !entry.file_type().is_file() {
            continue;
        }
        seen += 1;
        if seen > EMPTY_FOLDER_MAX_FILES || !is_document(entry.path()) {
            return false;
        }
    }
    true
}

// --- Cahier des charges ----------------------------------------------------------------

/// Une ligne d'exigence (`R1`) ou de cas limite (`E1`) du cahier des charges.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub id: String,
    pub text: String,
}

/// Les exigences et cas limites de `SPEC.md`, dans l'ordre.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Spec {
    pub items: Vec<Item>,
}

/// `R12` au début de `text` (une lettre parmi `kinds`, un à trois chiffres,
/// puis rien qui continue le mot) : l'identifiant sans zéro en tête, et la suite.
fn split_id<'a>(text: &'a str, kinds: &[char]) -> Option<(String, &'a str)> {
    let kind = text.chars().next().filter(|kind| kinds.contains(kind))?;
    let rest = &text[1..];
    let count = rest.chars().take_while(char::is_ascii_digit).count();
    if count == 0 || count > 3 {
        return None;
    }
    let after = &rest[count..];
    if after
        .chars()
        .next()
        .is_some_and(|next| next.is_alphanumeric() || next == '_')
    {
        return None;
    }
    let number: u32 = rest[..count].parse().ok()?;
    Some((format!("{kind}{number}"), after))
}

/// Retire d'un début de ligne les marques de titre, de liste, de case à cocher
/// et de mise en forme ; vrai en second si la ligne est une ligne de tableau.
fn line_body(line: &str) -> (&str, bool) {
    let mut rest = line.trim();
    let table = rest.starts_with('|');
    rest = rest.trim_start_matches(['#', '|', ' ', '\t']);
    for marker in ["- ", "* ", "+ "] {
        if let Some(after) = rest.strip_prefix(marker) {
            rest = after.trim_start();
            break;
        }
    }
    let digits = rest.chars().take_while(char::is_ascii_digit).count();
    if digits > 0 && rest[digits..].starts_with(['.', ')']) {
        rest = rest[digits + 1..].trim_start();
    }
    for checkbox in ["[ ] ", "[x] ", "[X] "] {
        if let Some(after) = rest.strip_prefix(checkbox) {
            rest = after.trim_start();
            break;
        }
    }
    (rest.trim_start_matches(['*', '_', '`']), table)
}

/// Séparateurs entre un identifiant et son texte (`R1:`, `**R1** -`, `R1 |`).
fn trim_separators(text: &str) -> &str {
    text.trim_start_matches(|character: char| {
        matches!(
            character,
            '*' | '_' | '`' | ':' | '.' | ')' | '|' | '-' | ' ' | '\t' | '\u{2013}' | '\u{2014}'
        )
    })
}

/// Identifiant et texte d'une ligne `R1: ...` ou `E1: ...`, écrite en liste,
/// en titre, en gras ou en ligne de tableau.
fn item_line(line: &str) -> Option<(String, String)> {
    let (body, table) = line_body(line);
    let (id, after) = split_id(body, &['R', 'E'])?;
    let text = trim_separators(after);
    let text = if table {
        text.split('|')
            .map(str::trim)
            .filter(|cell| !cell.is_empty())
            .collect::<Vec<_>>()
            .join(" - ")
    } else {
        text.trim().to_string()
    };
    Some((id, clipped(&text, ITEM_TEXT_MAX_CHARS)))
}

impl Spec {
    #[must_use]
    pub fn parse(text: &str) -> Self {
        let lines: Vec<&str> = text.lines().collect();
        let mut items = Vec::new();
        let mut fence = false;
        for (index, line) in lines.iter().enumerate() {
            if line.trim_start().starts_with("```") {
                fence = !fence;
                continue;
            }
            if fence {
                continue;
            }
            let Some((id, mut text)) = item_line(line) else {
                continue;
            };
            // Titre seul (`### R1`) : le texte est la ligne suivante.
            if text.is_empty() {
                text = lines[index + 1..]
                    .iter()
                    .map(|next| next.trim())
                    .find(|next| !next.is_empty())
                    .filter(|next| item_line(next).is_none() && !next.starts_with('#'))
                    .map(|next| clipped(next, ITEM_TEXT_MAX_CHARS))
                    .unwrap_or_default();
            }
            items.push(Item { id, text });
        }
        Self { items }
    }

    /// Exigences (`R`).
    #[must_use]
    pub fn requirements(&self) -> usize {
        self.items
            .iter()
            .filter(|item| item.id.starts_with('R'))
            .count()
    }

    /// Cas limites (`E`).
    #[must_use]
    pub fn edge_cases(&self) -> usize {
        self.items
            .iter()
            .filter(|item| item.id.starts_with('E'))
            .count()
    }

    #[must_use]
    pub fn item(&self, id: &str) -> Option<&Item> {
        self.items.iter().find(|item| item.id == id)
    }

    fn duplicates(&self) -> Vec<String> {
        let mut seen = BTreeSet::new();
        let mut twice = Vec::new();
        for item in &self.items {
            if !seen.insert(item.id.as_str()) && !twice.contains(&item.id) {
                twice.push(item.id.clone());
            }
        }
        twice
    }
}

// --- Plan en jalons ---------------------------------------------------------------------

/// Un jalon de `PLAN.md`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Milestone {
    /// `M1`, `M2`...
    pub id: String,
    pub title: String,
    /// Exigences et cas limites (`R3`, `E2`) que le jalon nomme.
    pub items: Vec<String>,
    /// Commandes de sa ligne `Verify`.
    pub verify: Vec<String>,
    pub done: bool,
}

/// La démonstration finale : installation puis lancement, depuis une copie
/// vide du projet.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Demo {
    pub install: Vec<String>,
    pub run: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Plan {
    pub milestones: Vec<Milestone>,
    pub demo: Option<Demo>,
}

/// Niveau d'un titre Markdown (`## ...` : 2) ; `None` pour une autre ligne.
fn heading_level(line: &str) -> Option<usize> {
    let level = line
        .chars()
        .take_while(|character| *character == '#')
        .count();
    ((1..=6).contains(&level) && line[level..].starts_with([' ', '\t'])).then_some(level)
}

/// Mots d'un état à faire ou fait (début de mot ; une espace finale : mot entier).
const NOT_DONE_WORDS: &[&str] = &[
    "todo",
    "to do ",
    "a faire",
    "not ",
    "pas ",
    "non ",
    "en cours",
    "in progress",
    "wip ",
    "pending",
    "ouvert",
    "open ",
    "[ ]",
];
const DONE_WORDS: &[&str] = &[
    "done ",
    "fait ",
    "termine",
    "complete",
    "valide",
    "ok ",
    "[x]",
    "finished ",
    "fini ",
    "passed ",
    "reussi",
];

fn status_done(value: &str) -> bool {
    let value = fold(value);
    !NOT_DONE_WORDS
        .iter()
        .any(|word| has_word_start(&value, word))
        && DONE_WORDS.iter().any(|word| has_word_start(&value, word))
}

/// Jalon annoncé par un titre : `M2 - Emprunts`, `Milestone 2: ...`, `Jalon 2 ...`,
/// avec une case cochée ou un état entre crochets ou parenthèses.
fn milestone_heading(heading: &str) -> Option<(String, String, bool)> {
    let mut rest = heading.trim();
    let mut checked = false;
    for (checkbox, value) in [("[ ]", false), ("[x]", true), ("[X]", true)] {
        if let Some(after) = rest.strip_prefix(checkbox) {
            rest = after.trim_start();
            checked = value;
            break;
        }
    }
    let rest = rest.trim_start_matches(['*', '_', '`']);
    let folded = fold(rest);
    // « Milestone 2 », « Jalon 2 » : comme « M2 ».
    let spelled = ["milestone ", "jalon "]
        .into_iter()
        .find(|word| folded.starts_with(word))
        .and_then(|word| rest.get(word.len()..))
        .map(|number| format!("M{number}"));
    let (id, after) = split_id(spelled.as_deref().unwrap_or(rest), &['M'])?;
    let title = trim_separators(after)
        .trim()
        .trim_end_matches(['*', '_', '`'])
        .trim();
    let folded_title = fold(title);
    let marked = [
        "(done)",
        "[done]",
        "(fait)",
        "[fait]",
        "(termine)",
        "[termine]",
    ]
    .iter()
    .any(|mark| folded_title.contains(mark));
    Some((id, title.to_string(), checked || marked))
}

/// `label: valeur` (étiquette de trois mots au plus), étiquette ramenée à des
/// minuscules sans accents.
fn label_line(line: &str) -> Option<(String, &str)> {
    let mut rest = line.trim();
    for marker in ["- ", "* ", "+ "] {
        if let Some(after) = rest.strip_prefix(marker) {
            rest = after.trim_start();
            break;
        }
    }
    let (label, value) = rest.split_once(':')?;
    let label = fold(
        label.trim_matches(|character: char| matches!(character, '*' | '_' | '`' | ' ' | '\t')),
    );
    if label.is_empty() || label.len() > 30 || label.split_whitespace().count() > 3 {
        return None;
    }
    Some((
        label,
        value.trim_matches(|character: char| matches!(character, '*' | '_' | ' ' | '\t')),
    ))
}

fn is_verify_label(label: &str) -> bool {
    label.starts_with("verif") || label.starts_with("check") || label.starts_with("controle")
}

fn is_status_label(label: &str) -> bool {
    ["status", "statut", "etat", "state"]
        .iter()
        .any(|word| label.starts_with(word))
}

fn is_install_label(label: &str) -> bool {
    label.starts_with("install") || label.starts_with("setup")
}

fn is_run_label(label: &str) -> bool {
    [
        "run",
        "lance",
        "demo",
        "start",
        "execut",
        "usage",
        "utilisation",
    ]
    .iter()
    .any(|word| label.starts_with(word))
}

/// Commandes d'une valeur : chaque passage entre accents graves, sinon la
/// valeur entière ; vide, celles du bloc de code qui suit (`index` avance alors
/// après lui).
fn commands_of(value: &str, lines: &[&str], index: &mut usize) -> Vec<String> {
    let value = value.trim();
    let mut commands: Vec<String> = if value.contains('`') {
        value
            .split('`')
            .skip(1)
            .step_by(2)
            .map(str::trim)
            .filter(|command| !command.is_empty())
            .map(str::to_string)
            .collect()
    } else if value.is_empty() {
        let mut next = *index + 1;
        while next < lines.len() && lines[next].trim().is_empty() {
            next += 1;
        }
        let mut found = Vec::new();
        if next < lines.len() && lines[next].trim_start().starts_with("```") {
            next += 1;
            while next < lines.len() && !lines[next].trim_start().starts_with("```") {
                let line = lines[next].trim();
                if !line.is_empty() && !line.starts_with('#') {
                    found.push(line.to_string());
                }
                next += 1;
            }
            *index = next;
        }
        found
    } else {
        vec![value.trim_end_matches('.').to_string()]
    };
    for command in &mut commands {
        if let Some(rest) = command.strip_prefix("$ ") {
            *command = rest.to_string();
        }
    }
    commands.retain(|command| {
        !matches!(
            fold(command).as_str(),
            "none" | "aucune" | "aucun" | "n/a" | "-" | "tbd"
        )
    });
    commands
}

/// Identifiants `R3`, `E2` d'une ligne, plages `R1-R3` comprises, sans doublon.
fn ids_in(line: &str, found: &mut Vec<String>) {
    let bytes = line.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        let kind = bytes[index];
        let starts = index == 0 || !bytes[index - 1].is_ascii_alphanumeric();
        if starts && (kind == b'R' || kind == b'E') {
            if let Some((id, after)) = split_id(&line[index..], &['R', 'E']) {
                push_unique(found, id.clone());
                // Plage `R1-R3` : les identifiants entre les deux.
                let range = after
                    .strip_prefix('-')
                    .or_else(|| after.strip_prefix('\u{2013}'))
                    .and_then(|rest| split_id(rest, &[char::from(kind)]));
                if let Some((last, _)) = range {
                    let first: u32 = id[1..].parse().unwrap_or(0);
                    let last: u32 = last[1..].parse().unwrap_or(0);
                    if first < last && last - first <= 50 {
                        for number in first + 1..=last {
                            push_unique(found, format!("{}{number}", char::from(kind)));
                        }
                    }
                }
                index += id.len();
                continue;
            }
        }
        index += 1;
    }
}

fn push_unique(found: &mut Vec<String>, id: String) {
    if !found.contains(&id) {
        found.push(id);
    }
}

enum Section {
    Milestone(Milestone, usize),
    Demo(Demo, usize),
    None,
}

impl Plan {
    #[must_use]
    pub fn parse(text: &str) -> Self {
        let lines: Vec<&str> = text.lines().collect();
        let mut plan = Plan::default();
        let mut section = Section::None;
        let mut fence = false;
        let mut index = 0;
        while index < lines.len() {
            let line = lines[index].trim();
            if line.starts_with("```") {
                fence = !fence;
                index += 1;
                continue;
            }
            if fence {
                index += 1;
                continue;
            }
            if let Some(level) = heading_level(line) {
                let heading = line[level..].trim();
                if let Some((id, title, done)) = milestone_heading(heading) {
                    plan.close(std::mem::replace(&mut section, Section::None));
                    let mut items = Vec::new();
                    ids_in(&title, &mut items);
                    section = Section::Milestone(
                        Milestone {
                            id,
                            title,
                            items,
                            verify: Vec::new(),
                            done,
                        },
                        level,
                    );
                } else if fold(heading).contains("demo") {
                    plan.close(std::mem::replace(&mut section, Section::None));
                    section = Section::Demo(Demo::default(), level);
                } else {
                    let ends = match &section {
                        Section::Milestone(_, open) | Section::Demo(_, open) => level <= *open,
                        Section::None => false,
                    };
                    if ends {
                        plan.close(std::mem::replace(&mut section, Section::None));
                    }
                }
                index += 1;
                continue;
            }
            let label = label_line(line);
            match (&mut section, label) {
                (Section::Milestone(milestone, _), Some((label, value)))
                    if is_verify_label(&label) =>
                {
                    let commands = commands_of(value, &lines, &mut index);
                    milestone.verify.extend(commands);
                }
                (Section::Milestone(milestone, _), Some((label, value)))
                    if is_status_label(&label) =>
                {
                    milestone.done = status_done(value);
                }
                (Section::Milestone(milestone, _), _) => ids_in(line, &mut milestone.items),
                (Section::Demo(demo, _), Some((label, value))) if is_install_label(&label) => {
                    let commands = commands_of(value, &lines, &mut index);
                    demo.install.extend(commands);
                }
                (Section::Demo(demo, _), Some((label, value))) if is_run_label(&label) => {
                    let commands = commands_of(value, &lines, &mut index);
                    demo.run.extend(commands);
                }
                _ => {}
            }
            index += 1;
        }
        plan.close(section);
        plan
    }

    fn close(&mut self, section: Section) {
        match section {
            Section::Milestone(milestone, _) => self.milestones.push(milestone),
            Section::Demo(demo, _) => {
                let known = self.demo.get_or_insert_with(Demo::default);
                known.install.extend(demo.install);
                known.run.extend(demo.run);
            }
            Section::None => {}
        }
    }

    /// Empreinte de ce qui décide de la démonstration : jalons, leur état et
    /// leurs commandes, lignes de la démonstration.
    #[must_use]
    pub fn fingerprint(&self) -> String {
        let mut parts = Vec::new();
        for milestone in &self.milestones {
            parts.push(format!(
                "{}|{}|{}",
                milestone.id,
                milestone.done,
                milestone.verify.join("&&")
            ));
        }
        if let Some(demo) = &self.demo {
            parts.push(demo.install.join("&&"));
            parts.push(demo.run.join("&&"));
        }
        digest(&parts)
    }
}

/// Empreinte des commandes de vérification d'un jalon : un jalon déjà vérifié
/// l'est de nouveau si elles changent.
#[must_use]
pub fn check_key(milestone: &Milestone) -> String {
    digest(&milestone.verify)
}

// --- Le projet et sa phase ------------------------------------------------------------

/// Où en est le projet, d'après ses fichiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// Pas encore de cahier des charges (ou aucune exigence `R`).
    Spec,
    /// Cahier des charges écrit, pas encore de jalon.
    Plan,
    /// Le jalon à cet indice est le premier qui n'est pas fait.
    Build(usize),
    /// Tous les jalons faits, démonstration finale à passer.
    Finish,
    /// Démonstration finale réussie sur ce plan.
    Done,
}

#[derive(Debug, Clone)]
pub struct Project {
    pub root: PathBuf,
    pub spec: Option<Spec>,
    pub plan: Option<Plan>,
    pub decisions: Option<String>,
    pub state: ProjectState,
}

impl Project {
    #[must_use]
    pub fn read(root: &Path) -> Self {
        let read = |name: &str| std::fs::read_to_string(root.join(name)).ok();
        Self {
            root: root.to_path_buf(),
            spec: read(SPEC_FILE).map(|text| Spec::parse(&text)),
            plan: read(PLAN_FILE).map(|text| Plan::parse(&text)),
            decisions: read(DECISIONS_FILE),
            state: ProjectState::load(root),
        }
    }

    #[must_use]
    pub fn phase(&self) -> Phase {
        if self
            .spec
            .as_ref()
            .is_none_or(|spec| spec.requirements() == 0)
        {
            return Phase::Spec;
        }
        let Some(plan) = self
            .plan
            .as_ref()
            .filter(|plan| !plan.milestones.is_empty())
        else {
            return Phase::Plan;
        };
        if let Some(index) = plan.milestones.iter().position(|milestone| !milestone.done) {
            return Phase::Build(index);
        }
        if self.state.demo_passed.as_deref() == Some(plan.fingerprint().as_str()) {
            Phase::Done
        } else {
            Phase::Finish
        }
    }

    #[must_use]
    pub fn milestones(&self) -> &[Milestone] {
        self.plan
            .as_ref()
            .map_or(&[], |plan| plan.milestones.as_slice())
    }

    /// Empreinte de ce qu'une relecture d'architecture a lu : exigences et
    /// jalons (pas leur état).
    #[must_use]
    pub fn design_fingerprint(&self) -> String {
        let mut parts: Vec<String> = self
            .spec
            .iter()
            .flat_map(|spec| &spec.items)
            .map(|item| item.id.clone())
            .collect();
        parts.extend(
            self.milestones()
                .iter()
                .map(|milestone| format!("{} {}", milestone.id, milestone.title)),
        );
        digest(&parts)
    }

    /// `R3: texte` pour chaque identifiant, ou « (not in SPEC.md) ».
    fn item_lines(&self, ids: &[String]) -> Vec<String> {
        ids.iter()
            .map(|id| {
                match self
                    .spec
                    .as_ref()
                    .and_then(|spec| spec.item(id))
                    .filter(|item| !item.text.is_empty())
                {
                    Some(item) => format!("{id}: {}", item.text),
                    None => format!("{id} (not in {SPEC_FILE})"),
                }
            })
            .collect()
    }

    /// Le jalon à faire, ses lignes du cahier des charges et sa vérification.
    fn milestone_brief(&self, index: usize) -> String {
        let milestones = self.milestones();
        let Some(milestone) = milestones.get(index) else {
            return String::new();
        };
        let mut text = format!(
            "{} of {}: {}.",
            milestone.id,
            milestones.len(),
            milestone.title
        );
        if milestone.items.is_empty() {
            text.push_str(" It names no R or E line of SPEC.md yet: add them to it.");
        } else {
            let _ = write!(
                text,
                " Its lines in {SPEC_FILE}: {}.",
                self.item_lines(&milestone.items).join("; ")
            );
        }
        if milestone.verify.is_empty() {
            text.push_str(" It has no `Verify:` line yet: add one.");
        } else {
            let _ = write!(
                text,
                " Its check: {}.",
                milestone
                    .verify
                    .iter()
                    .map(|command| format!("`{command}`"))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
        text
    }

    /// « M1 done, M2 (current), M3 ».
    fn progress(&self) -> String {
        let current = self
            .milestones()
            .iter()
            .position(|milestone| !milestone.done);
        self.milestones()
            .iter()
            .enumerate()
            .map(|(index, milestone)| {
                if milestone.done {
                    format!("{} done", milestone.id)
                } else if Some(index) == current {
                    format!("{} (current)", milestone.id)
                } else {
                    milestone.id.clone()
                }
            })
            .collect::<Vec<_>>()
            .join(", ")
    }
}

// --- Contrôles de structure --------------------------------------------------------------

const SPEC_LINE_FORMAT: &str =
    "`R1: <behavior a user can observe>. Check: <exact command or action> -> <expected result>`";

/// Ce qui empêche Claw de lire le cahier des charges.
#[must_use]
pub fn spec_problems(spec: &Spec) -> Vec<String> {
    let mut problems = Vec::new();
    if spec.requirements() == 0 {
        problems.push(format!(
            "it has no requirement line in the form {SPEC_LINE_FORMAT}"
        ));
    }
    let twice = spec.duplicates();
    if !twice.is_empty() {
        problems.push(format!(
            "{} {} used on several lines: give each line its own id",
            listed(&twice),
            if twice.len() > 1 { "are" } else { "is" }
        ));
    }
    problems
}

/// Ce qui empêche Claw de suivre le plan.
#[must_use]
pub fn plan_problems(plan: &Plan, spec: Option<&Spec>) -> Vec<String> {
    let mut problems = Vec::new();
    if plan.milestones.is_empty() {
        problems.push(
            "it has no milestone heading in the form `## M1 - <title>` (with the lines `Requirements:`, `Verify:` and `Status:` under it)"
                .to_string(),
        );
        return problems;
    }
    let unchecked: Vec<String> = plan
        .milestones
        .iter()
        .filter(|milestone| milestone.verify.is_empty())
        .map(|milestone| milestone.id.clone())
        .collect();
    if !unchecked.is_empty() {
        problems.push(format!(
            "{} {} no `Verify: <command>` line (one command that exits with 0 only when the milestone works, usually its tests)",
            listed(&unchecked),
            if unchecked.len() > 1 { "have" } else { "has" }
        ));
    }
    if let Some(spec) = spec.filter(|spec| !spec.items.is_empty()) {
        let named: Vec<String> = plan
            .milestones
            .iter()
            .flat_map(|milestone| milestone.items.iter().cloned())
            .collect();
        let unknown: Vec<String> = named
            .iter()
            .filter(|id| spec.item(id).is_none())
            .cloned()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        if !unknown.is_empty() {
            problems.push(format!(
                "it names {}, which {SPEC_FILE} does not define",
                listed(&unknown)
            ));
        }
        let mut uncovered: Vec<String> = Vec::new();
        for item in &spec.items {
            if !named.contains(&item.id) && !uncovered.contains(&item.id) {
                uncovered.push(item.id.clone());
            }
        }
        if !uncovered.is_empty() {
            problems.push(format!(
                "{} of {SPEC_FILE} {} in no milestone: name each on the `Requirements:` line of the milestone that delivers it",
                listed(&uncovered),
                if uncovered.len() > 1 { "are" } else { "is" }
            ));
        }
    }
    match &plan.demo {
        None => problems.push(
            "it has no `## Demo` section with the lines `Install:` and `Run:` (what Claw runs in an empty copy of the project once every milestone is done)"
                .to_string(),
        ),
        Some(demo) if demo.run.is_empty() => problems.push(
            "its `## Demo` section has no `Run:` line (a command that shows the project working and exits by itself)"
                .to_string(),
        ),
        Some(_) => {}
    }
    problems
}

/// Remarque au modèle : ce qu'il faut corriger dans `SPEC.md` ou `PLAN.md`.
#[must_use]
pub fn structure_note(file: &str, problems: &[String]) -> String {
    format!(
        "{CLAW_NOTE_PREFIX} {file}: {}. Claw reads this file to guide and check the project: fix it now.",
        problems.join("; ")
    )
}

// --- Tests d'acceptation -----------------------------------------------------------------

const TEST_EXTENSIONS: &[&str] = &[
    "py", "js", "jsx", "mjs", "cjs", "ts", "tsx", "go", "rs", "java", "kt", "rb", "php", "cs",
    "sh", "bats",
];
const TEST_DIRS: &[&str] = &["tests", "test", "__tests__", "spec", "specs"];
const TEST_FILE_MAX_BYTES: u64 = 300_000;
const TEST_FILES_MAX: usize = 400;

fn is_test_file(relative: &Path) -> bool {
    let name = relative
        .file_name()
        .map(|name| name.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    let stem = name
        .rsplit_once('.')
        .map_or(name.as_str(), |(stem, _)| stem);
    name.starts_with("test_")
        || name.starts_with("test.")
        || stem.ends_with("_test")
        || stem.ends_with("_tests")
        || name.contains(".test.")
        || name.contains(".spec.")
        || relative.parent().is_some_and(|parent| {
            parent.components().any(|component| {
                TEST_DIRS.contains(
                    &component
                        .as_os_str()
                        .to_string_lossy()
                        .to_lowercase()
                        .as_str(),
                )
            })
        })
}

/// Contenu, en minuscules, des fichiers de test du projet (et des fichiers
/// Rust qui portent des tests).
fn test_texts(root: &Path) -> Vec<String> {
    let walker = walkdir::WalkDir::new(root)
        .max_depth(8)
        .into_iter()
        .filter_entry(|entry| {
            entry.depth() == 0 || !(entry.file_type().is_dir() && skipped_dir(entry.path(), root))
        });
    let mut texts = Vec::new();
    for entry in walker.flatten() {
        if texts.len() >= TEST_FILES_MAX {
            break;
        }
        let path = entry.path();
        let extension = path
            .extension()
            .map(|extension| extension.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        if !entry.file_type().is_file()
            || !TEST_EXTENSIONS.contains(&extension.as_str())
            || entry
                .metadata()
                .is_ok_and(|meta| meta.len() > TEST_FILE_MAX_BYTES)
        {
            continue;
        }
        let relative = path.strip_prefix(root).unwrap_or(path);
        let Ok(text) = std::fs::read_to_string(path) else {
            continue;
        };
        if is_test_file(relative) || (extension == "rs" && text.contains("#[test]")) {
            texts.push(text.to_lowercase());
        }
    }
    texts
}

/// Le texte (en minuscules) nomme l'identifiant (`r3`) : `test_r3_ajout`,
/// `# R3`, mais ni `r30` ni `ar3`.
fn names_id(text: &str, id: &str) -> bool {
    text.match_indices(id).any(|(at, _)| {
        let before = text[..at].chars().next_back();
        let after = text[at + id.len()..].chars().next();
        before.is_none_or(|character| !character.is_ascii_alphanumeric())
            && after.is_none_or(|character| !character.is_ascii_digit())
    })
}

/// Les identifiants qu'aucun fichier de test du projet ne nomme.
#[must_use]
pub fn untested(root: &Path, ids: &[String]) -> Vec<String> {
    if ids.is_empty() {
        return Vec::new();
    }
    let texts = test_texts(root);
    ids.iter()
        .filter(|id| {
            let id = id.to_ascii_lowercase();
            !texts.iter().any(|text| names_id(text, &id))
        })
        .cloned()
        .collect()
}

// --- Guides ------------------------------------------------------------------------------

const SPEC_STEP: &str = "One line per requirement a user can observe: `R1: <behavior>. Check: <exact command or action> -> <expected result>`; one line per edge case: `E1: <case> -> <expected result>`. Write requirements for every sentence of the request and for what it implies without saying it: the pitfalls listed above when there are some, wrong or missing input, empty data, data kept between runs. End with a `Questions` section (only what really changes what you build; \"none\" is fine) and an `Out of scope` section.";

const PLAN_STEP: &str = "Milestones that each leave a project that runs: `## M1 - <title>`, then the lines `Requirements: R1, E1`, `Verify: <one command that exits with 0 only when the milestone works, usually its tests>` and `Status: todo`. M1 is a walking skeleton: the smallest version that runs from end to end. Every R and E line of SPEC.md belongs to one milestone. End with a `## Demo` section: `Install: <commands to run in an empty copy of the project>` and `Run: <a command that shows the project working and exits by itself>`.";

const DECISIONS_STEP: &str = "One line per choice (language, libraries, storage, file formats) with its reason: `- <decision>: <reason>`.";

const MILESTONE_STEP: &str = "Write the acceptance tests of its R and E lines first, each test named after its id (`test_r3_...`) or with the id in a comment; run them and watch them fail; write the code; run its Verify command; then set `Status: done`. Claw runs the Verify command of every milestone marked done.";

fn ask_part(can_ask: bool) -> &'static str {
    if can_ask {
        "The user can answer you: after SPEC.md, show them its requirements in a few lines and ask them with AskUserQuestion to confirm or correct them (and to answer its Questions) before you write PLAN.md. After each milestone, stop: tell them how to try it (the exact commands) and ask whether it does what they want."
    } else {
        "Nobody can answer you during this turn: answer the Questions of SPEC.md yourself with the most reasonable choice, write each answer in DECISIONS.md, and go through every milestone without stopping."
    }
}

/// Guide joint à la demande qui ouvre le mode projet : toute la méthode.
#[must_use]
pub fn activation_guide(can_ask: bool) -> String {
    format!(
        "{GUIDE_PREFIX} Project mode is on for this folder: build the project in the steps below, in order. Claw reads the files you write at the root of the project and checks each step. Keep the ids (R1, E1, M1) and the labels (Check, Requirements, Verify, Status, Install, Run) exactly as shown; write the rest in the user's language.\n1. {SPEC_FILE}, the specification. {SPEC_STEP}\n2. {PLAN_FILE}. {PLAN_STEP}\n3. {DECISIONS_FILE}. {DECISIONS_STEP}\n4. Then one milestone at a time, in order. {MILESTONE_STEP}\n5. When every milestone is done, Claw copies the project to an empty folder and runs the Demo lines there; README.md gives the user the same commands.\n{}",
        ask_part(can_ask)
    )
}

/// Guide des tours suivants : où en est le projet, et l'étape à faire.
#[must_use]
pub fn phase_guide(project: &Project, can_ask: bool) -> String {
    let mut text = format!(
        "{GUIDE_PREFIX} Project mode: {SPEC_FILE}, {PLAN_FILE} and {DECISIONS_FILE} at the root of the project; one milestone at a time, its acceptance tests first; at the end, Claw runs the Demo lines in an empty copy of the project. If the user's message asks for something else, do that first and keep these files in step with it.\n"
    );
    match project.phase() {
        Phase::Spec => {
            let _ = write!(
                text,
                "Now: {SPEC_FILE} is not written yet (or has no `R1:` line). Write it first. {SPEC_STEP}\n{}",
                ask_part(can_ask)
            );
        }
        Phase::Plan => {
            let spec = project.spec.clone().unwrap_or_default();
            let _ = write!(
                text,
                "Now: {SPEC_FILE} has {} requirements and {} edge cases; write {PLAN_FILE}. {PLAN_STEP} Write {DECISIONS_FILE} too. {DECISIONS_STEP}",
                spec.requirements(),
                spec.edge_cases()
            );
        }
        Phase::Build(index) => {
            let _ = write!(
                text,
                "Milestones: {}.\nNow: {} {MILESTONE_STEP} {}",
                project.progress(),
                project.milestone_brief(index),
                if can_ask {
                    "Then stop: tell the user how to try it and ask whether it does what they want."
                } else {
                    "Then go on with the next milestone."
                }
            );
        }
        Phase::Finish => {
            let _ = write!(
                text,
                "Milestones: {}.\nNow: every milestone is done. When you finish, Claw runs the final demo: the `## Demo` lines of {PLAN_FILE} in an empty copy of the project. README.md must give the user the same commands.",
                project.progress()
            );
        }
        Phase::Done => text.push_str(
            "The project is complete: every milestone is done and the final demo passed. For a new request, add its R and E lines to SPEC.md (new ids) and a milestone to PLAN.md first, then work as before.",
        ),
    }
    if let Some(decisions) = project
        .decisions
        .as_deref()
        .map(str::trim)
        .filter(|text| !text.is_empty())
    {
        let _ = write!(
            text,
            "\nDecisions taken so far ({DECISIONS_FILE}); follow them unless the user changes them:\n{}",
            last_chars(decisions, 2_000)
        );
    }
    text
}

/// La fin d'un texte, `limit` caractères au plus, coupée à une ligne.
fn last_chars(text: &str, limit: usize) -> String {
    let count = text.chars().count();
    if count <= limit {
        return text.to_string();
    }
    let tail: String = text.chars().skip(count - limit).collect();
    match tail.split_once('\n') {
        Some((_, rest)) => format!("...\n{rest}"),
        None => tail,
    }
}

// --- Remarques et renvois ------------------------------------------------------------------

/// Un jalon marqué fait dont la vérification échoue.
#[must_use]
pub fn milestone_failed_note(id: &str, command: &str, excerpt: &str, timed_out: bool) -> String {
    let what = if timed_out {
        "did not finish in time; a Verify command must exit by itself (for a server: start it, request it, stop it)"
    } else {
        "fails"
    };
    format!(
        "{CLAW_NOTE_PREFIX} {id} is marked done in {PLAN_FILE}, but its Verify command {what}:\n$ {command}\n{excerpt}\n\nA milestone is done only when its check passes. Find the cause and fix the code (never weaken the check), run the check again, and only then go on."
    )
}

/// Un jalon toujours marqué fait, sans changement depuis l'échec de sa vérification.
#[must_use]
pub fn still_failing_note(id: &str) -> String {
    format!(
        "{CLAW_NOTE_PREFIX} {id} is still marked done in {PLAN_FILE}, although its Verify command failed and nothing changed since: fix the code and run the check again, or set its Status back to todo if it is not done."
    )
}

/// Un jalon marqué fait sans commande de vérification.
#[must_use]
pub fn milestone_without_check_note(id: &str) -> String {
    format!(
        "{CLAW_NOTE_PREFIX} {id} is marked done, but it has no `Verify:` line in {PLAN_FILE}: add the command that shows it works (usually its tests) and run it."
    )
}

/// Un jalon vérifié par Claw ; `untested` : ses lignes qu'aucun test ne nomme.
#[must_use]
pub fn milestone_checked_note(id: &str, commands: &[String], untested: &[String]) -> String {
    let ran = commands
        .iter()
        .map(|command| format!("`{command}`"))
        .collect::<Vec<_>>()
        .join(", ");
    let mut note = format!("{CLAW_NOTE_PREFIX} {id}: its Verify command passes ({ran}).");
    if !untested.is_empty() {
        let example = untested[0].to_ascii_lowercase();
        let _ = write!(
            note,
            " But no test file of the project names {}: each R and E line of a milestone needs an acceptance test named after its id (`test_{example}_...`) or with the id in a comment. Write these tests now and run them.",
            listed(untested)
        );
    }
    note
}

/// Le modèle a modifié le projet, ou s'arrête seul, sans cahier des charges.
#[must_use]
pub fn spec_first_feedback(changed: bool) -> String {
    let what = if changed {
        "you changed the project without SPEC.md"
    } else {
        "SPEC.md is not written yet and nobody can answer you during this turn"
    };
    format!(
        "{CLAW_NOTE_PREFIX} Project mode: {what}. Write {SPEC_FILE} now from the request. {SPEC_STEP} Then write {PLAN_FILE} and {DECISIONS_FILE}, and mark done only the milestones whose Verify command passes."
    )
}

/// Jalons faits dont des lignes n'ont toujours aucun test qui les nomme.
#[must_use]
pub fn untested_feedback(ids: &[String]) -> String {
    let example = ids
        .first()
        .map_or("r1".to_string(), |id| id.to_ascii_lowercase());
    format!(
        "{CLAW_NOTE_PREFIX} Project mode: no test file of the project names {} yet, although their milestones are marked done. Write their acceptance tests now, each named after its id (`test_{example}_...`) or with the id in a comment, and run them.",
        listed(ids)
    )
}

/// Seul, le modèle s'arrête après le cahier des charges : le plan suit.
#[must_use]
pub fn plan_next_feedback() -> String {
    format!(
        "{CLAW_NOTE_PREFIX} Project mode: {SPEC_FILE} is written and nobody can answer you during this turn: write {PLAN_FILE} now. {PLAN_STEP} Then {DECISIONS_FILE}, then go through the milestones."
    )
}

/// Quelqu'un peut répondre : le cahier des charges est à faire valider.
#[must_use]
pub fn confirm_spec_feedback() -> String {
    format!(
        "{CLAW_NOTE_PREFIX} Project mode: {SPEC_FILE} is written. Before {PLAN_FILE}, show the user its requirements in a few lines and ask them with AskUserQuestion to confirm or correct them, together with the Questions of {SPEC_FILE}."
    )
}

/// Seul, le modèle s'arrête avec des jalons à faire : il continue.
#[must_use]
pub fn continue_feedback(project: &Project, index: usize) -> String {
    let remaining: Vec<String> = project
        .milestones()
        .iter()
        .filter(|milestone| !milestone.done)
        .map(|milestone| milestone.id.clone())
        .collect();
    format!(
        "{CLAW_NOTE_PREFIX} Project mode: {PLAN_FILE} still has milestones to do ({}) and nobody can answer you during this turn. Go on with {} {MILESTONE_STEP} Stop only when every milestone is done, or when only the user can unblock you (then say exactly what you need).",
        listed(&remaining),
        project.milestone_brief(index)
    )
}

/// Quelqu'un peut répondre : le jalon terminé est à faire essayer.
#[must_use]
pub fn validation_feedback(done: &[String], next: Option<&str>) -> String {
    let verb = if done.len() > 1 { "are" } else { "is" };
    let before = next.map_or(String::new(), |next| format!(", before you start {next}"));
    format!(
        "{CLAW_NOTE_PREFIX} Project mode: {} {verb} done. End your answer with how the user can try it (the exact commands, from the project folder), then ask them with AskUserQuestion whether it does what they want{before}.",
        listed(done)
    )
}

/// Les lignes du cahier des charges des jalons faits (toutes, sans plan), à
/// joindre au contrôle des exigences et à la relecture.
#[must_use]
pub fn reference_for_checks(project: &Project) -> Option<String> {
    let spec = project
        .spec
        .as_ref()
        .filter(|spec| !spec.items.is_empty())?;
    let milestones = project.milestones();
    let ids: Vec<String> = if milestones.is_empty() {
        spec.items.iter().map(|item| item.id.clone()).collect()
    } else {
        let mut ids = Vec::new();
        for milestone in milestones.iter().filter(|milestone| milestone.done) {
            for id in &milestone.items {
                push_unique(&mut ids, id.clone());
            }
        }
        ids
    };
    if ids.is_empty() {
        return None;
    }
    let mut lines = String::new();
    for line in project.item_lines(&ids) {
        if lines.len() + line.len() > REFERENCE_MAX_CHARS {
            lines.push_str("\n- ...");
            break;
        }
        lines.push_str("\n- ");
        lines.push_str(&line);
    }
    Some(format!(
        "This project has a specification, {SPEC_FILE}. Its lines for the work done so far are part of the request; each must be shown working by a test or a command output:{lines}"
    ))
}

// --- Suivi d'un tour ------------------------------------------------------------------------

/// Ce que le mode projet suit pendant un tour.
#[derive(Debug, Default)]
pub(crate) struct Turn {
    pub root: PathBuf,
    /// Jalons déjà faits au début du tour : leur vérification n'est pas refaite.
    pub done_at_start: BTreeSet<String>,
    /// `SPEC.md` avait déjà des exigences au début du tour.
    pub spec_at_start: bool,
    /// Jalons vérifiés pendant ce tour : empreinte de leurs commandes, et vrai
    /// si la vérification a réussi.
    pub checked: BTreeMap<String, (String, bool)>,
    /// Changements du projet comptés au moment de chaque vérification en échec.
    pub failed_at: BTreeMap<String, usize>,
    /// Remarques de structure déjà faites.
    pub noted: BTreeSet<String>,
    /// Renvois de fin de tour déjà faits (sujet).
    pub asked: BTreeSet<String>,
    /// Renvois de fin de tour pour une vérification de jalon en échec.
    pub gate_rounds: usize,
    /// Jalons terminés (vérification réussie) pendant ce tour.
    pub completed: Vec<String>,
    /// La validation des jalons terminés a été demandée.
    pub validation_asked: bool,
}

impl Turn {
    #[must_use]
    pub fn start(project: &Project) -> Self {
        Self {
            root: project.root.clone(),
            done_at_start: project
                .milestones()
                .iter()
                .filter(|milestone| milestone.done)
                .map(|milestone| milestone.id.clone())
                .collect(),
            spec_at_start: project.phase() != Phase::Spec,
            ..Self::default()
        }
    }
}

/// Empreinte d'une remarque, pour ne pas la refaire.
#[must_use]
pub fn note_key(note: &str) -> String {
    digest(&[note.to_string()])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "claw-projet-{name}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |elapsed| elapsed.as_nanos())
        ));
        std::fs::create_dir_all(&path).expect("dossier");
        path
    }

    #[test]
    fn a_creation_request_is_told_from_other_requests() {
        assert!(creation_request(
            "Crée une application de gestion de bibliothèque avec emprunts, retours et pénalités"
        ));
        assert!(creation_request(
            "Je veux une petite appli pour suivre mes dépenses du mois par catégorie"
        ));
        assert!(creation_request(
            "Build a todo app with tags, due dates and a JSON file for storage"
        ));
        assert!(creation_request("Fais-moi tout de A à Z"));
        // Trop court pour un cahier des charges et un plan.
        assert!(!creation_request("Écris un script qui affiche bonjour"));
        // Pas une création.
        assert!(!creation_request(
            "Corrige le bug qui fait planter le calcul des totaux quand la liste est vide"
        ));
        assert_eq!(read_wish("Passe en mode projet pour la suite"), Wish::Start);
        assert_eq!(
            read_wish("Continue sans mode projet s'il te plaît"),
            Wish::Stop
        );
        assert_eq!(read_wish("Exit project mode"), Wish::Stop);
        assert_eq!(read_wish("Ajoute un bouton"), Wish::Nothing);
    }

    #[test]
    fn a_folder_with_only_documents_is_empty() {
        let root = folder("vide");
        std::fs::write(root.join("README.md"), "# Idée\n").expect("readme");
        std::fs::write(root.join(".gitignore"), "*.pyc\n").expect("gitignore");
        std::fs::write(root.join("maquette.png"), [0_u8, 1, 2]).expect("image");
        std::fs::create_dir_all(root.join(".git")).expect("git");
        std::fs::write(root.join(".git").join("config"), "x").expect("config");
        std::fs::create_dir_all(root.join("node_modules").join("x")).expect("modules");
        std::fs::write(root.join("node_modules").join("x").join("index.js"), "x").expect("js");
        assert!(empty_folder(&root));
        std::fs::write(root.join("app.py"), "print(1)\n").expect("code");
        assert!(!empty_folder(&root));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_specification_lines_are_read_in_every_usual_form() {
        let spec = Spec::parse(
            "# Bibliothèque\n\n## Exigences\n- R1: Ajouter un livre. Check: `python biblio.py ajouter \"Dune\"` -> il apparaît dans `liste`\n* **R2**: Emprunter un livre disponible\n3. R03 - Rendre un livre\n### R4\nLe prêt dure 14 jours.\n| R5 | Pénalité de retard | 0,50 EUR par jour |\n\n## Cas limites\n- E1: titre vide -> refusé avec un message\n- [ ] E2: emprunter un livre déjà prêté -> refusé\n\n```\nR9: dans un bloc de code, ignoré\n```\nRun: python biblio.py\nR2D2 n'est pas une exigence.\n",
        );
        let ids: Vec<&str> = spec.items.iter().map(|item| item.id.as_str()).collect();
        assert_eq!(ids, ["R1", "R2", "R3", "R4", "R5", "E1", "E2"]);
        assert!(spec.items[0].text.starts_with("Ajouter un livre. Check:"));
        assert_eq!(spec.items[2].text, "Rendre un livre");
        assert_eq!(spec.items[3].text, "Le prêt dure 14 jours.");
        assert_eq!(spec.items[4].text, "Pénalité de retard - 0,50 EUR par jour");
        assert_eq!(spec.requirements(), 5);
        assert_eq!(spec.edge_cases(), 2);
        assert!(spec_problems(&spec).is_empty());
        let twice = Spec::parse("- R1: a\n- R1: b\n");
        assert_eq!(
            spec_problems(&twice),
            ["R1 is used on several lines: give each line its own id"]
        );
        assert!(
            spec_problems(&Spec::parse("# Idée\nUne appli.\n"))[0].contains("no requirement line")
        );
    }

    const PLAN: &str = "# Plan\n\n## M1 - Squelette (R1, E1)\nRequirements: R1, E1\nVerify: `python -m pytest -q tests/test_m1.py`\nStatus: done\n\n## [x] M2 - Emprunts\n### Tâches\n- Requirements: R2-R4\n- **Verify:** `python -m pytest -q` puis `python biblio.py liste`\n\n## M3 - Pénalités\nRequirements: R5, E2\nVerify:\n```bash\n$ python -m pytest -q tests/test_m3.py\n```\nStatus: todo\n\n## Notes\nR9 ici n'est dans aucun jalon.\n\n## Demo\nInstall: `python -m pip install -r requirements.txt`\nRun: `python biblio.py liste`\n";

    #[test]
    fn the_plan_milestones_their_checks_and_the_demo_are_read() {
        let plan = Plan::parse(PLAN);
        assert_eq!(plan.milestones.len(), 3);
        let first = &plan.milestones[0];
        assert_eq!(
            (first.id.as_str(), first.title.as_str()),
            ("M1", "Squelette (R1, E1)")
        );
        assert_eq!(first.items, ["R1", "E1"]);
        assert_eq!(first.verify, ["python -m pytest -q tests/test_m1.py"]);
        assert!(first.done);
        let second = &plan.milestones[1];
        assert!(second.done, "case cochée dans le titre");
        assert_eq!(second.items, ["R2", "R3", "R4"]);
        assert_eq!(
            second.verify,
            ["python -m pytest -q", "python biblio.py liste"]
        );
        let third = &plan.milestones[2];
        assert!(!third.done);
        assert_eq!(third.verify, ["python -m pytest -q tests/test_m3.py"]);
        assert_eq!(third.items, ["R5", "E2"]);
        let demo = plan.demo.expect("démo");
        assert_eq!(demo.install, ["python -m pip install -r requirements.txt"]);
        assert_eq!(demo.run, ["python biblio.py liste"]);
        for (status, done) in [
            ("done", true),
            ("Fait", true),
            ("terminé (tests verts)", true),
            ("todo", false),
            ("not done", false),
            ("pas fait", false),
            ("en cours", false),
            ("", false),
        ] {
            let plan = Plan::parse(&format!("## M1 - x\nStatus: {status}\n"));
            assert_eq!(plan.milestones[0].done, done, "{status}");
        }
        let jalon = Plan::parse("## Jalon 2 : Export\nVérification : pytest\n");
        assert_eq!(jalon.milestones[0].id, "M2");
        assert_eq!(jalon.milestones[0].title, "Export");
        assert_eq!(jalon.milestones[0].verify, ["pytest"]);
    }

    #[test]
    fn the_phase_follows_the_files_and_the_plan_problems_are_named() {
        let root = folder("phases");
        let project = Project::read(&root);
        assert_eq!(project.phase(), Phase::Spec);
        std::fs::write(
            root.join(SPEC_FILE),
            "- R1: a\n- R2: b\n- R3: c\n- R4: d\n- R5: e\n- R6: f\n- E1: x -> y\n- E2: z -> w\n",
        )
        .expect("spec");
        assert_eq!(Project::read(&root).phase(), Phase::Plan);
        std::fs::write(root.join(PLAN_FILE), PLAN).expect("plan");
        let project = Project::read(&root);
        assert_eq!(project.phase(), Phase::Build(2));
        let problems = plan_problems(project.plan.as_ref().expect("plan"), project.spec.as_ref());
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].starts_with("R6 of SPEC.md is in no milestone"));
        let guide = phase_guide(&project, false);
        assert!(guide.contains("Milestones: M1 done, M2 done, M3 (current)."));
        assert!(guide.contains("M3 of 3: Pénalités. Its lines in SPEC.md: R5: e; E2: z -> w."));
        assert!(guide.contains("Its check: `python -m pytest -q tests/test_m3.py`."));
        assert!(guide.contains("Then go on with the next milestone."));
        assert!(phase_guide(&project, true).contains("Then stop: tell the user how to try it"));
        // Tous les jalons faits : démonstration finale, puis projet terminé.
        std::fs::write(
            root.join(PLAN_FILE),
            PLAN.replace("Status: todo", "Status: done"),
        )
        .expect("plan");
        let mut project = Project::read(&root);
        assert_eq!(project.phase(), Phase::Finish);
        project.state.demo_passed = Some(project.plan.as_ref().expect("plan").fingerprint());
        project.state.save(&root).expect("état");
        assert_eq!(Project::read(&root).phase(), Phase::Done);
        assert!(ProjectState::load(&root).demo_passed.is_some());
        let empty = Plan::parse("# Plan\nÀ venir.\n");
        assert!(plan_problems(&empty, None)[0].contains("no milestone heading"));
        let bare = Plan::parse("## M1 - x\n## M2 - y\nVerify: pytest\n");
        let problems = plan_problems(&bare, Some(&Spec::parse("- R1: a\n")));
        assert!(problems[0].starts_with("M1 has no `Verify: <command>` line"));
        assert!(problems
            .iter()
            .any(|problem| problem.contains("no `## Demo` section")));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn requirements_without_a_test_that_names_them_are_found() {
        let root = folder("tests");
        std::fs::create_dir_all(root.join("tests")).expect("tests");
        std::fs::write(
            root.join("tests").join("test_biblio.py"),
            "def test_r1_ajout():\n    pass\n\n# E1 : titre vide\ndef test_vide():\n    pass\n",
        )
        .expect("test");
        std::fs::write(root.join("biblio.py"), "# R2 dans le code ne compte pas\n").expect("code");
        std::fs::create_dir_all(root.join("src")).expect("src");
        std::fs::write(
            root.join("src").join("lib.rs"),
            "#[cfg(test)]\nmod tests {\n    #[test]\n    fn r3_rendre() {}\n}\n",
        )
        .expect("rust");
        let ids: Vec<String> = ["R1", "R2", "R3", "E1", "R10"]
            .iter()
            .map(ToString::to_string)
            .collect();
        assert_eq!(untested(&root, &ids), ["R2", "R10"]);
        assert!(names_id("test_r1_ajout", "r1"));
        assert!(!names_id("test_r10", "r1"));
        assert!(!names_id("ar1", "r1"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_guides_name_the_formats_claw_reads() {
        for can_ask in [true, false] {
            let guide = activation_guide(can_ask);
            for label in [
                "`R1: <behavior>. Check:",
                "`E1: <case> -> <expected result>`",
                "`## M1 - <title>`",
                "`Verify:",
                "`Status: todo`",
                "`## Demo`",
                "`Install:",
                "`Run:",
                "`test_r3_...`",
            ] {
                assert!(guide.contains(label), "{label}");
            }
            assert!(!guide.contains('\u{2014}'));
        }
        assert!(activation_guide(true).contains("AskUserQuestion"));
        assert!(activation_guide(false).contains("Nobody can answer you"));
        let note = milestone_checked_note("M2", &["pytest".to_string()], &["R4".to_string()]);
        assert!(note.contains("no test file of the project names R4"));
        assert!(note.contains("`test_r4_...`"));
        assert!(validation_feedback(&["M1".to_string()], Some("M2"))
            .contains("M1 is done. End your answer with how the user can try it"));
    }
}
