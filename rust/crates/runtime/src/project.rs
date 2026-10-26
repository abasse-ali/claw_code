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
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

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
/// Vérifications en échec d'un même jalon dans un tour, après quoi Claw ne la relance plus
/// à chaque modification du projet (un outil absent de la machine la fait échouer en boucle).
pub const MAX_MILESTONE_CHECKS: usize = 3;
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

    /// Claw a déjà enregistré un état pour ce dossier (même « mode arrêté »).
    #[must_use]
    pub fn exists(root: &Path) -> bool {
        Self::path(root).is_file()
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

/// Les mots de l'utilisateur : la demande jusqu'à la première ligne qui commence
/// par `[Claw]`. Ce qui suit, un moteur l'y ajoute pour le modèle (la conversation
/// qui précède, le dépôt de la tâche) ; le mode projet ne se décide pas dessus.
#[must_use]
pub fn own_words(request: &str) -> &str {
    let mut end = 0;
    for line in request.split_inclusive('\n') {
        if line.trim_start().starts_with("[Claw]") {
            return request[..end].trim_end();
        }
        end += line.len();
    }
    request
}

/// Les mots de l'utilisateur avec ceux de ses messages précédents, que Claw Web
/// rappelle après la demande sous `[Request]` et `[Message]` (les réponses de
/// l'agent, `[Your answer]`, et les notes `[Claw]` restent dehors). Un besoin
/// posé au premier message vaut encore quand le suivant n'en parle plus.
#[must_use]
pub fn user_words_with_history(request: &str) -> String {
    let mut words = own_words(request).to_string();
    let mut kept = false;
    for line in request.lines() {
        let label = line.trim_end();
        if label.starts_with('[') {
            kept = label == "[Request]" || label == "[Message]";
            continue;
        }
        if kept {
            words.push('\n');
            words.push_str(line);
        }
    }
    words
}

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

/// Ce qu'on crée quand c'est un projet : une application, un site, un outil... Juste
/// après un premier mot de texte, il en fait un projet (« a resume *builder* »).
const PROJECT_NOUNS: &[&str] = &[
    "builder",
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

/// Ce qu'une demande crée sans que ce soit un projet : un texte ou un document. Comparé au
/// premier mot qui suit le verbe (pluriel compris).
const TEXT_OBJECTS: &[&str] = &[
    "liste",
    "list",
    "idee",
    "idea",
    "explication",
    "explanation",
    "resume",
    "summary",
    "conseil",
    "advice",
    "tip",
    "exemple",
    "example",
    "description",
    "article",
    "email",
    "mail",
    "lettre",
    "letter",
    "message",
    "texte",
    "text",
    "reponse",
    "answer",
    "question",
    "poeme",
    "poem",
    "histoire",
    "story",
    "traduction",
    "translation",
    "tutoriel",
    "tutorial",
    "plan",
    "rapport",
    "report",
    "presentation",
    "cahier",
    "specification",
    "spec",
    "readme",
    "licence",
    "license",
    "gitignore",
    "documentation",
    "doc",
    "changelog",
    "contributing",
    "note",
    "commentaire",
    "comment",
];

/// Mots sautés entre le verbe et ce qu'il crée : articles, pronoms, adjectifs courants.
/// « fichier » aussi : le mot suivant dit de quel fichier il s'agit.
const FILLER_WORDS: &[&str] = &[
    "un", "une", "des", "du", "de", "d", "le", "la", "les", "l", "au", "aux", "moi", "nous", "me",
    "m", "te", "t", "toi", "lui", "leur", "mon", "ma", "mes", "ton", "ta", "tes", "son", "sa",
    "ses", "notre", "nos", "votre", "vos", "ce", "cet", "cette", "ces", "petit", "petite",
    "petits", "petites", "simple", "nouveau", "nouvelle", "nouvel", "vrai", "vraie", "bon",
    "bonne", "joli", "jolie", "beau", "belle", "super", "autre", "en", "pour", "tout", "toute",
    "vite", "stp", "svp", "fichier", "fichiers", "a", "an", "the", "my", "our", "your", "some",
    "new", "small", "little", "quick", "basic", "nice", "please", "us", "for", "file", "files",
];

/// Premiers mots d'une question d'information : « Comment créer une appli... ? » demande
/// une explication, pas un projet (« Peux-tu créer... ? » en est un).
const QUESTION_WORDS: &[&str] = &[
    "comment", "pourquoi", "quel", "quelle", "quels", "quelles", "quoi", "qu", "combien", "quand",
    "how", "why", "what", "which", "when", "where", "who",
];

/// Mots devant un nom : après eux, « code », « build » ou « écriture » ne sont pas des
/// verbes (« Explique ce code », « fix the build »).
const DETERMINERS: &[&str] = &[
    "ce", "cet", "cette", "ces", "le", "la", "les", "l", "un", "une", "des", "du", "mon", "ma",
    "mes", "ton", "ta", "tes", "son", "sa", "ses", "notre", "nos", "votre", "vos", "leur", "leurs",
    "the", "this", "that", "these", "those", "my", "your", "our", "their", "its", "a", "an",
];

/// Mots d'un texte replié (minuscules, sans accents), sans ponctuation.
fn words(text: &str) -> Vec<&str> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .collect()
}

/// Positions (dans `words`) juste après chaque occurrence de `phrase` : ses mots dans
/// l'ordre, le dernier comme début de mot (« cree » couvre « creer »), sauf espace finale
/// (mot entier : « fais »). Pas après un déterminant : c'est alors un nom.
fn after_phrase(words: &[&str], phrase: &str) -> Vec<usize> {
    let whole = phrase.ends_with(' ');
    let parts: Vec<&str> = phrase
        .split(|c: char| !c.is_alphanumeric())
        .filter(|part| !part.is_empty())
        .collect();
    let Some((last, first)) = parts.split_last() else {
        return Vec::new();
    };
    (0..words.len().saturating_sub(first.len()))
        .filter(|&start| {
            (start == 0 || !DETERMINERS.contains(&words[start - 1]))
                && first
                    .iter()
                    .enumerate()
                    .all(|(offset, part)| words[start + offset] == *part)
                && {
                    let word = words[start + first.len()];
                    if whole {
                        word == *last
                    } else {
                        word.starts_with(last)
                    }
                }
        })
        .map(|start| start + parts.len())
        .collect()
}

fn same_word(word: &str, list: &[&str]) -> bool {
    list.iter().any(|item| {
        word == *item
            || word.strip_suffix('s') == Some(item)
            || word.strip_suffix('x') == Some(item)
    })
}

fn project_noun(word: &str) -> bool {
    PROJECT_NOUNS
        .iter()
        .any(|noun| match noun.strip_suffix(' ') {
            Some(core) => word == core,
            None => !noun.contains(' ') && word.starts_with(noun),
        })
}

/// Après le verbe (ou le besoin), ce qui est créé fait-il un projet ? Un texte ou un
/// document en premier mot n'en fait pas un, sauf si un projet le suit aussitôt
/// (« a resume builder »). Après « je veux », un verbe à l'infinitif (« comprendre »,
/// « to ») dit autre chose qu'un objet.
fn creates_a_project(words: &[&str], from: usize, wish: bool) -> bool {
    let object: Vec<&str> = words[from..]
        .iter()
        .copied()
        .filter(|word| !FILLER_WORDS.contains(word))
        .take(3)
        .collect();
    let Some(first) = object.first() else {
        return false;
    };
    if wish
        && (*first == "to"
            || (first.len() > 4
                && ["er", "ir", "re", "oir"]
                    .iter()
                    .any(|end| first.ends_with(end))))
    {
        return false;
    }
    !same_word(first, TEXT_OBJECTS) || object.get(1).is_some_and(|word| project_noun(word))
}

/// La demande crée un projet : un verbe de création (ou un besoin) suivi de ce qu'il faut
/// créer, en au moins huit mots ; ou « de A à Z », « from scratch ». Pas une question
/// d'information, ni un texte ou un document seul (liste, idées, README, licence...).
#[must_use]
pub fn creation_request(request: &str) -> bool {
    let text = fold(request);
    if WHOLE_PROJECT_PHRASES
        .iter()
        .any(|phrase| has_word_start(&text, phrase))
    {
        return true;
    }
    if request.split_whitespace().count() < CREATION_MIN_WORDS {
        return false;
    }
    let words = words(&text);
    let question = request.trim_end().ends_with('?')
        && words
            .first()
            .is_some_and(|word| QUESTION_WORDS.contains(word));
    if question {
        return false;
    }
    CREATION_VERBS
        .iter()
        .map(|verb| (verb, false))
        .chain(WISH_PHRASES.iter().map(|wish| (wish, true)))
        .any(|(phrase, wish)| {
            after_phrase(&words, phrase)
                .into_iter()
                .any(|from| creates_a_project(&words, from, wish))
        })
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
    /// Son état est écrit : ligne `Status:`, case `[ ]`/`[x]` ou « (done) » dans le titre.
    pub status_given: bool,
}

impl Milestone {
    /// Jalon lu dans un titre ; `status` : l'état que le titre donne, s'il en donne un.
    fn new(id: String, title: String, status: Option<bool>) -> Self {
        let mut items = Vec::new();
        ids_in(&title, &mut items);
        Self {
            id,
            title,
            items,
            verify: Vec::new(),
            done: status == Some(true),
            status_given: status.is_some(),
        }
    }
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
/// avec une case cochée ou un état entre crochets ou parenthèses (`Some(fait)` : l'état est
/// dans le titre).
fn milestone_heading(heading: &str) -> Option<(String, String, Option<bool>)> {
    let mut rest = heading.trim();
    let mut checked = None;
    for (checkbox, value) in [("[ ]", false), ("[x]", true), ("[X]", true)] {
        if let Some(after) = rest.strip_prefix(checkbox) {
            rest = after.trim_start();
            checked = Some(value);
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
    Some((
        id,
        title.to_string(),
        if marked { Some(true) } else { checked },
    ))
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

/// Lignes d'un bloc de code qui commence à `index` (sans commentaires ni
/// invite `$ `) ; `index` passe après sa fin.
fn fenced_commands(lines: &[&str], index: &mut usize) -> Vec<String> {
    let mut commands = Vec::new();
    *index += 1;
    while *index < lines.len() && !lines[*index].trim_start().starts_with("```") {
        let line = lines[*index].trim();
        if !line.is_empty() && !line.starts_with('#') {
            commands.push(line.strip_prefix("$ ").unwrap_or(line).to_string());
        }
        *index += 1;
    }
    *index += 1;
    commands
}

/// Une commande sans son commentaire de fin (`activate  # Windows`), hors
/// guillemets.
#[must_use]
pub fn without_comment(command: &str) -> &str {
    let mut quote: Option<char> = None;
    let mut previous = ' ';
    for (at, character) in command.char_indices() {
        match (quote, character) {
            (Some(open), _) if character == open => quote = None,
            (None, '\'' | '"') => quote = Some(character),
            (None, '#') if previous.is_whitespace() => return command[..at].trim_end(),
            _ => {}
        }
        previous = character;
    }
    command.trim_end()
}

/// Les lignes de la démonstration en une seule commande, dans l'ordre, comme
/// les tape l'utilisateur dans un même terminal (un environnement activé reste
/// actif), depuis le dossier `place`.
#[must_use]
pub fn demo_script(place: &str, demo: &Demo) -> String {
    let steps: Vec<String> = demo
        .install
        .iter()
        .chain(&demo.run)
        .map(|command| format!("{{ {}; }}", without_comment(command)))
        .collect();
    format!("cd \"{place}\" && {}", steps.join(" && "))
}

/// Un passage entre accents graves est une commande : pas une option seule
/// (`--hors-ligne`), ni un nom de fichier seul (`verif_edt.py`, `edt_sync.yml`).
fn is_command(text: &str) -> bool {
    let file_name =
        !text.contains(char::is_whitespace) && text.contains('.') && !text.contains('/');
    !(text.starts_with('-') || file_name)
}

/// Commandes d'une valeur : chaque passage entre accents graves qui est une
/// commande, sinon la valeur entière ; vide, celles du bloc de code qui suit
/// (`index` avance alors après lui).
fn commands_of(value: &str, lines: &[&str], index: &mut usize) -> Vec<String> {
    let value = value.trim();
    let mut commands: Vec<String> = if value.contains('`') {
        value
            .split('`')
            .skip(1)
            .step_by(2)
            .map(str::trim)
            .filter(|command| !command.is_empty() && is_command(command))
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
        // Sous-titre de la démonstration (`### Install`, `### Run`) : vrai pour
        // l'installation, faux pour le lancement ; son bloc de code suit.
        let mut demo_part: Option<bool> = None;
        let mut index = 0;
        while index < lines.len() {
            let line = lines[index].trim();
            if line.starts_with("```") {
                if let (false, Section::Demo(demo, _), Some(install)) =
                    (fence, &mut section, demo_part)
                {
                    let commands = fenced_commands(&lines, &mut index);
                    if install {
                        demo.install.extend(commands);
                    } else {
                        demo.run.extend(commands);
                    }
                    continue;
                }
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
                let folded = fold(heading);
                if matches!(&section, Section::Demo(_, open) if level > *open) {
                    demo_part = if is_install_label(&folded) {
                        Some(true)
                    } else if is_run_label(&folded) || folded.contains("demo") {
                        Some(false)
                    } else {
                        None
                    };
                    index += 1;
                    continue;
                }
                demo_part = None;
                if let Some((id, title, status)) = milestone_heading(heading) {
                    plan.close(std::mem::replace(&mut section, Section::None));
                    section = Section::Milestone(Milestone::new(id, title, status), level);
                } else if folded.contains("demo") {
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
                    milestone.status_given = true;
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
            "{} {} no `Verify: <command>` line (one command that exits with 0 only when the milestone works, usually its tests, written as is in backticks: no file name or option alone)",
            listed(&unchecked),
            if unchecked.len() > 1 { "have" } else { "has" }
        ));
    }
    let unmarked: Vec<String> = plan
        .milestones
        .iter()
        .filter(|milestone| !milestone.status_given)
        .map(|milestone| milestone.id.clone())
        .collect();
    if !unmarked.is_empty() {
        problems.push(format!(
            "{} {} no `Status: todo` or `Status: done` line: Claw runs the Verify command of a milestone when its Status says done, and only then is it done",
            listed(&unmarked),
            if unmarked.len() > 1 { "have" } else { "has" }
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

/// Le journal des décisions manque alors que le plan est écrit.
#[must_use]
pub fn decisions_problems(project: &Project) -> Vec<String> {
    let planned = !project.milestones().is_empty();
    let written = project
        .decisions
        .as_deref()
        .is_some_and(|text| !text.trim().is_empty());
    if planned && !written {
        vec![format!(
            "it does not exist yet (or is empty). {DECISIONS_STEP} Start with the language, the libraries, how data is stored and the answers you chose to the Questions of SPEC.md"
        )]
    } else {
        Vec::new()
    }
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

const PLAN_STEP: &str = "Milestones that each leave a project that runs: `## M1 - <title>`, then the lines `Requirements: R1, E1`, `Verify: <one command that exits with 0 only when the milestone works, usually its tests>` and `Status: todo`. M1 is a walking skeleton: the smallest version that runs from end to end. Every R and E line of SPEC.md belongs to one milestone. End with a `## Demo` section: `Install: <commands to run in an empty copy of the project>` and `Run: <a command that shows the project working and exits by itself>`; Claw runs them in that order in one session of the bash tool's shell, from the project folder.";

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

/// Vérification d'un jalon en échec trop de fois dans le tour : Claw ne la relance plus.
#[must_use]
pub fn milestone_blocked_note(id: &str, failures: usize) -> String {
    format!(
        "{CLAW_NOTE_PREFIX} The Verify command of {id} has now failed {failures} times in this turn: Claw will not run it again in this turn. Do not mark {id} done again in this turn: set its Status back to todo. Then fix what makes the check fail; or, if this machine lacks something the check needs (a system tool, a service, network access), say exactly what in {PLAN_FILE} and in your final answer, and go on with what you can verify."
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

// --- Sonde de l'environnement --------------------------------------------------------------

/// Délai d'une commande de la sonde.
const PROBE_TIMEOUT: Duration = Duration::from_secs(8);

/// Ce que la sonde a trouvé sur la machine.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Environment {
    /// Windows, et l'outil `bash` y lance Git Bash.
    pub windows: bool,
    pub git_bash: bool,
    /// Commande qui lance Python (`python`, `py -3`) et sa version.
    pub python: Option<(String, String)>,
    /// Codage de la sortie de Python quand elle va dans un tube ou un fichier.
    pub python_encoding: Option<String>,
    /// Version de pytest, s'il est installé pour ce Python.
    pub pytest: Option<String>,
    /// `zoneinfo` trouve les fuseaux horaires (paquet `tzdata` ou base du système).
    pub time_zones: Option<bool>,
    /// Autres outils, avec leur version, ou `None` s'ils manquent.
    pub tools: Vec<(String, Option<String>)>,
}

/// Sortie (stdout puis stderr) d'une commande qui réussit dans le délai.
fn probe(program: &str, args: &[&str]) -> Option<String> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .ok()?;
    let deadline = Instant::now() + PROBE_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let output = child.wait_with_output().ok()?;
                if !status.success() {
                    return None;
                }
                let text = format!(
                    "{}\n{}",
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                );
                return Some(text.trim().to_string());
            }
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(20));
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
}

/// Le numéro de version d'une sortie : `v22.11.0`, `go1.23.1`, `"21.0.1"`.
fn version_in(text: &str) -> Option<String> {
    text.split_whitespace().find_map(|word| {
        let word = word.trim_matches(|character: char| matches!(character, '"' | '\'' | ','));
        let digits = word
            .strip_prefix("go")
            .or_else(|| word.strip_prefix('v'))
            .unwrap_or(word);
        digits
            .chars()
            .next()
            .is_some_and(|first| first.is_ascii_digit())
            .then(|| digits.to_string())
    })
}

/// Ce que Python dit de lui : codage de sa sortie dans un tube, pytest,
/// fuseaux horaires. Une ligne, sans `try`, pour passer en argument.
const PYTHON_FACTS: &str = r#"import sys, json, os, importlib.util as u, importlib.metadata as m; print(json.dumps({"encoding": sys.stdout.encoding, "pytest": m.version("pytest") if u.find_spec("pytest") else None, "zones": bool(u.find_spec("tzdata")) or os.path.isdir("/usr/share/zoneinfo")}))"#;

fn probe_python(environment: &mut Environment) {
    let candidates: &[(&str, &[&str])] = if cfg!(windows) {
        &[("python", &[]), ("py", &["-3"]), ("python3", &[])]
    } else {
        &[("python3", &[]), ("python", &[])]
    };
    for (program, prefix) in candidates {
        let mut args = prefix.to_vec();
        args.push("--version");
        let Some(version) = probe(program, &args)
            .filter(|text| text.starts_with("Python"))
            .and_then(|text| version_in(&text))
        else {
            continue;
        };
        let command = std::iter::once(*program)
            .chain(prefix.iter().copied())
            .collect::<Vec<_>>()
            .join(" ");
        environment.python = Some((command, version));
        let mut args = prefix.to_vec();
        args.extend(["-c", PYTHON_FACTS]);
        if let Some(facts) = probe(program, &args)
            .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        {
            environment.python_encoding = facts["encoding"].as_str().map(str::to_string);
            environment.pytest = facts["pytest"].as_str().map(str::to_string);
            environment.time_zones = facts["zones"].as_bool();
        }
        return;
    }
}

/// Les outils de la machine : Python (avec pytest, le codage de sa sortie, les
/// fuseaux horaires), Node, npm, Git, Cargo, Go, Java. Deux secondes au plus
/// d'ordinaire ; chaque commande s'arrête au bout de huit.
#[must_use]
pub fn probe_environment() -> Environment {
    let mut environment = Environment {
        windows: cfg!(windows),
        git_bash: cfg!(windows) && plugins::posix_shell_program().is_some(),
        ..Environment::default()
    };
    let npm = if cfg!(windows) { "npm.cmd" } else { "npm" };
    let others: [(&str, &str, &[&str]); 6] = [
        ("Node", "node", &["--version"]),
        ("npm", npm, &["--version"]),
        ("Git", "git", &["--version"]),
        ("Cargo", "cargo", &["--version"]),
        ("Go", "go", &["version"]),
        ("Java", "java", &["-version"]),
    ];
    let tools = std::thread::scope(|scope| {
        let python = scope.spawn(|| {
            let mut found = Environment::default();
            probe_python(&mut found);
            found
        });
        let handles: Vec<_> = others
            .iter()
            .map(|(name, program, args)| {
                scope.spawn(move || {
                    let version = probe(program, args).and_then(|text| version_in(&text));
                    ((*name).to_string(), version)
                })
            })
            .collect();
        let tools: Vec<(String, Option<String>)> = handles
            .into_iter()
            .filter_map(|handle| handle.join().ok())
            .collect();
        (python.join().unwrap_or_default(), tools)
    });
    let (python, tools) = tools;
    environment.python = python.python;
    environment.python_encoding = python.python_encoding;
    environment.pytest = python.pytest;
    environment.time_zones = python.time_zones;
    environment.tools = tools;
    environment
}

/// `(3, 13)` pour `3.13.1`.
fn python_release(version: &str) -> Option<(u32, u32)> {
    let mut parts = version.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts
        .next()?
        .chars()
        .take_while(char::is_ascii_digit)
        .collect::<String>()
        .parse()
        .ok()?;
    Some((major, minor))
}

/// Ce que la sonde a trouvé, dit au modèle avec les pièges connus de cette
/// machine : Python du Store, sortie en cp1252, modules retirés de Python,
/// fuseaux horaires absents, pytest à installer.
#[must_use]
pub fn environment_note(environment: &Environment) -> String {
    let mut lines = Vec::new();
    if environment.windows {
        lines.push(if environment.git_bash {
            "Windows; the bash tool runs Git Bash.".to_string()
        } else {
            "Windows.".to_string()
        });
    }
    match &environment.python {
        Some((command, version)) => {
            let mut line = format!("Python {version}, run it as `{command}`.");
            match &environment.pytest {
                Some(pytest) => {
                    let _ = write!(line, " pytest {pytest} is installed.");
                }
                None => {
                    let _ = write!(
                        line,
                        " pytest is not installed for it: run `{command} -m pip install pytest` before your first test and list pytest in the project's requirements."
                    );
                }
            }
            lines.push(line);
            if let Some(encoding) = environment.python_encoding.as_deref().filter(|encoding| {
                !encoding
                    .to_ascii_lowercase()
                    .replace('-', "")
                    .starts_with("utf8")
            }) {
                lines.push(format!(
                    "When the program's output goes to a pipe or a file (the Git Bash terminal, `> out.txt`, another program), Python writes it as {encoding} (the bash tool sets PYTHONIOENCODING=utf-8, which hides it): print() of a character outside {encoding} (an arrow, a check mark, an emoji, Asian scripts) raises UnicodeEncodeError there, and accents come out garbled in Git Bash. Call `sys.stdout.reconfigure(encoding=\"utf-8\")` at start, or keep printed text within {encoding}; open files with `encoding=\"utf-8\"`."
                ));
            }
            if python_release(version).is_some_and(|release| release >= (3, 12)) {
                lines.push(
                    "Not in this Python (removed in 3.12 and 3.13): distutils, imp, cgi, cgitb, imghdr, pipes, telnetlib, crypt, audioop, uu; do not import them."
                        .to_string(),
                );
            }
            if environment.time_zones == Some(false) {
                lines.push(
                    "zoneinfo finds no time zone data on this machine: add `tzdata` to the requirements if the project uses zoneinfo."
                        .to_string(),
                );
            }
        }
        None => lines.push("Python is not installed.".to_string()),
    }
    let found: Vec<String> = environment
        .tools
        .iter()
        .filter_map(|(name, version)| version.as_ref().map(|version| format!("{name} {version}")))
        .collect();
    if !found.is_empty() {
        lines.push(format!("{}.", found.join("; ")));
    }
    let missing: Vec<&str> = environment
        .tools
        .iter()
        .filter(|(_, version)| version.is_none())
        .map(|(name, _)| name.as_str())
        .collect();
    if !missing.is_empty() {
        lines.push(format!("Not installed: {}.", missing.join(", ")));
    }
    format!(
        "Environment probed by Claw on this machine (use these commands; do not build on what is not installed):\n- {}",
        lines.join("\n- ")
    )
}

// --- Démonstration finale -------------------------------------------------------------------

/// Fichiers et octets au plus d'une copie pour la démonstration finale.
const DEMO_MAX_FILES: usize = 3_000;
const DEMO_MAX_BYTES: u64 = 100_000_000;
/// Délai d'une commande `Run` de la démonstration, en millisecondes.
pub const DEMO_RUN_TIMEOUT_MS: u64 = 120_000;
/// Renvois au plus pour une démonstration finale en échec.
pub const MAX_DEMO_ROUNDS: usize = 3;

/// Fichiers que liste `git ls-files` avec ces options (chemins relatifs) ;
/// `None` hors d'un dépôt.
fn git_files(root: &Path, options: &[&str]) -> Option<Vec<PathBuf>> {
    let output = Command::new("git")
        .arg("ls-files")
        .args(options)
        .arg("-z")
        .current_dir(root)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()
        .filter(|output| output.status.success())?;
    Some(
        String::from_utf8_lossy(&output.stdout)
            .split('\0')
            .filter(|name| !name.is_empty())
            .map(PathBuf::from)
            .collect(),
    )
}

/// Données qu'un programme écrit en tournant (données JSON, CSV, bases,
/// journaux) : une installation neuve part sans elles, sauf si Git les suit.
fn run_time_data(file: &Path) -> bool {
    let extension = file
        .extension()
        .map(|extension| extension.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    matches!(
        extension.as_str(),
        "csv" | "db" | "sqlite" | "sqlite3" | "log"
    ) || crate::harness::is_json_data_file(file)
}

/// Fichiers qu'aurait l'utilisateur (chemins relatifs) : ceux que Git suit,
/// et ceux qu'il suivrait (`--others --exclude-standard`) sauf les données que
/// le programme écrit en tournant, laissées de côté (second élément). Hors d'un
/// dépôt : tout sauf les dossiers cachés, de dépendances et de sortie, et sauf
/// ces données. Jamais `.claw`.
#[must_use]
pub fn project_files(root: &Path) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let listed =
        git_files(root, &["--cached"]).zip(git_files(root, &["--others", "--exclude-standard"]));
    let (tracked, untracked) = listed.unwrap_or_else(|| {
        let walked = walkdir::WalkDir::new(root)
            .into_iter()
            .filter_entry(|entry| {
                entry.depth() == 0
                    || !(entry.file_type().is_dir() && skipped_dir(entry.path(), root))
            })
            .flatten()
            .filter(|entry| entry.file_type().is_file())
            .filter_map(|entry| entry.path().strip_prefix(root).ok().map(Path::to_path_buf))
            .collect();
        (Vec::new(), walked)
    });
    let usable = |file: &PathBuf| !file.starts_with(".claw") && root.join(file).is_file();
    let mut kept: Vec<PathBuf> = tracked.into_iter().filter(usable).collect();
    let mut left_out = Vec::new();
    for file in untracked.into_iter().filter(usable) {
        if run_time_data(&file) {
            left_out.push(file);
        } else {
            kept.push(file);
        }
    }
    kept.sort();
    kept.dedup();
    left_out.sort();
    (kept, left_out)
}

/// La copie vide du projet où tourne la démonstration finale.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CleanCopy {
    pub folder: PathBuf,
    /// Fichiers copiés.
    pub files: usize,
    /// Données du programme laissées de côté (chemins relatifs, barres obliques).
    pub left_out: Vec<String>,
}

/// Copie les fichiers du projet dans un dossier neuf du dossier temporaire.
pub fn clean_copy(root: &Path) -> Result<CleanCopy, String> {
    let (files, left_out) = project_files(root);
    if files.len() > DEMO_MAX_FILES {
        return Err(format!("{} fichiers", files.len()));
    }
    let bytes: u64 = files
        .iter()
        .filter_map(|file| std::fs::metadata(root.join(file)).ok())
        .map(|meta| meta.len())
        .sum();
    if bytes > DEMO_MAX_BYTES {
        return Err(format!("{} Mo", bytes / 1_000_000));
    }
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_nanos());
    let copy = std::env::temp_dir().join(format!("claw-demo-{}-{stamp}", std::process::id()));
    std::fs::create_dir_all(&copy).map_err(|error| error.to_string())?;
    for file in &files {
        let target = copy.join(file);
        if let Some(folder) = target.parent() {
            std::fs::create_dir_all(folder).map_err(|error| error.to_string())?;
        }
        std::fs::copy(root.join(file), &target).map_err(|error| error.to_string())?;
    }
    Ok(CleanCopy {
        folder: copy,
        files: files.len(),
        left_out: left_out
            .iter()
            .map(|file| file.to_string_lossy().replace('\\', "/"))
            .collect(),
    })
}

fn squeezed(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Le README du projet montre chacune de ces commandes.
#[must_use]
pub fn readme_shows(root: &Path, commands: &[String]) -> bool {
    let readme = [
        "README.md",
        "readme.md",
        "Readme.md",
        "README.txt",
        "README",
    ]
    .iter()
    .find_map(|name| std::fs::read_to_string(root.join(name)).ok());
    let Some(readme) = readme else {
        return false;
    };
    let text = squeezed(&readme);
    commands
        .iter()
        .all(|command| text.contains(&squeezed(command)))
}

/// Tous les jalons sont faits mais rien ne dit comment montrer le projet.
#[must_use]
pub fn demo_missing_feedback() -> String {
    format!(
        "{CLAW_NOTE_PREFIX} Project mode: every milestone of {PLAN_FILE} is done, but it has no `## Demo` section with a `Run:` line, so the project cannot be shown working the way the user will run it. Add `## Demo` with `Install: <commands to run in an empty copy of the project>` and `Run: <a command that shows the project working and exits by itself>`, keep README.md in step with them, then finish again: Claw runs them in an empty copy."
    )
}

/// Une commande de la démonstration finale a échoué dans la copie vide.
#[must_use]
pub fn demo_failed_feedback(
    copy: &CleanCopy,
    command: &str,
    excerpt: &str,
    timed_out: bool,
) -> String {
    let what = if timed_out {
        "did not finish in time (a Demo command must exit by itself; for a server: start it, request a page, stop it)"
    } else {
        "failed"
    };
    let left_out = if copy.left_out.is_empty() {
        String::new()
    } else {
        format!(
            " Left out of the copy, as data the program writes when it runs and that Git does not track: {}. A program must create its data files when they are missing; if it needs one of them from the start, track it with `git add`.",
            copy.left_out.join(", ")
        )
    };
    format!(
        "{CLAW_NOTE_PREFIX} Final demo: Claw copied the project to an empty folder ({} files: those Git tracks or would track) and ran the Demo lines of {PLAN_FILE} there, as the user will.{left_out} This command {what}:\n$ {command}\n{excerpt}\n\nUsual causes: a file the program needs that is not in the project (made by hand, or ignored by .gitignore), a package missing from the requirements, a path relative to the current folder instead of the program's own folder, a wrong command in README.md. Fix the project (or the Demo line, if that is what is wrong), then finish again: Claw runs the demo again.",
        copy.files
    )
}

/// La démonstration a réussi, mais le README ne montre pas ses commandes.
#[must_use]
pub fn readme_feedback(run: &[String]) -> String {
    let commands = run
        .iter()
        .map(|command| format!("`{command}`"))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "{CLAW_NOTE_PREFIX} Final demo passed. README.md does not show how to run the project the way the Demo lines do ({commands}): write or update README.md so that the user can install and run it with exactly these commands."
    )
}

// --- Budget de requêtes -----------------------------------------------------------------------

/// Requêtes comptées pour un jalon tant qu'aucun jalon du projet n'a été mesuré.
pub const DEFAULT_MILESTONE_REQUESTS: u64 = 30;

/// Requêtes que prend un jalon de ce projet : la moyenne des jalons terminés
/// (mesurés dans le tour où ils se terminent), sinon une estimation.
#[must_use]
pub fn milestone_cost(state: &ProjectState) -> u64 {
    let spent: Vec<u64> = state
        .milestone_requests
        .values()
        .copied()
        .filter(|requests| *requests > 0)
        .collect();
    let count = u64::try_from(spent.len()).unwrap_or(u64::MAX);
    if count == 0 {
        DEFAULT_MILESTONE_REQUESTS
    } else {
        (spent.iter().sum::<u64>() / count).max(1)
    }
}

/// Le quota gratuit du jour ne suffit pas pour les `remaining` jalons restants
/// à `cost` requêtes chacun : le guide pour le modèle, et la notice pour
/// l'utilisateur. `None` quand le quota suffit.
#[must_use]
pub fn budget_note(left: u64, cost: u64, remaining: usize) -> Option<(String, String)> {
    let remaining_count = u64::try_from(remaining).unwrap_or(u64::MAX);
    let cost = cost.max(1);
    if remaining == 0 || left >= cost.saturating_mul(remaining_count) {
        return None;
    }
    if left < cost {
        return Some((
            format!(
                "{GUIDE_PREFIX} Budget: {left} free requests are left today, and a milestone of this project takes about {cost}. Do only what fits in about {} requests, then stop with the project in a clean state: its tests passing and {PLAN_FILE} saying what is done and what is left. The free quota comes back at 00:00 UTC.",
                left / 2
            ),
            format!(
                "Mode projet : il reste {left} requêtes gratuites aujourd'hui pour environ {cost} par jalon : l'agent s'arrêtera sur un état propre"
            ),
        ));
    }
    let fits = left / cost;
    Some((
        format!(
            "{GUIDE_PREFIX} Budget: {left} free requests are left today and a milestone of this project takes about {cost}: there is room for about {fits} of the {remaining} milestones left. Finish them one at a time, each leaving the project in a working state, and stop cleanly when the budget runs low."
        ),
        format!(
            "Mode projet : il reste {left} requêtes gratuites aujourd'hui, de quoi faire environ {fits} des {remaining} jalons restants"
        ),
    ))
}

/// Fin de la note de Claw quand le budget de requêtes du tour est atteint.
pub const WRAP_UP_NOTE: &str = " Project mode: before you stop, make sure PLAN.md says which milestones are done (their Status) and what is left in the current one, so that the next turn starts from there.";

// --- Relecture d'architecture ------------------------------------------------------------

/// Consignes du relecteur d'architecture : un autre modèle lit le cahier des
/// charges et le plan avant que le code soit écrit.
pub const DESIGN_REVIEW_SYSTEM: &str = "You are a senior software architect. A coding agent is about to build a project for a user. Before any code is written, you review what it plans: the user's request, the specification (SPEC.md: requirements R1..., edge cases E1...), the plan (PLAN.md: milestones M1... with the command that verifies each, and the final demo) and its decisions (DECISIONS.md), with the tools installed on the machine. Look for what would make the finished project fail the user: a behavior the request implies that SPEC.md misses (wrong or missing input, empty data, duplicates, data kept between runs, two users at once, money, dates, text encodings); a requirement written so that nobody can check it; a Verify command that would pass without proving the requirements of its milestone; a milestone order that leaves nothing runnable (M1 must run from end to end); a choice the machine does not support (a tool or library that is not installed, a module removed from this Python, Windows paths and encodings); Demo lines that would fail in an empty copy of the project. Report only real problems, at most eight, the most important first, one line each. Finish with a line containing only the word VERDICT, followed by either the single line LGTM, or one line per problem in the form `FILE - problem - what to change`.";

/// Taille gardée de chaque fichier montré au relecteur d'architecture.
const DESIGN_FILE_MAX_CHARS: usize = 8_000;

/// Demande au relecteur d'architecture : la demande de l'utilisateur, les
/// trois fichiers du projet et l'environnement sondé.
#[must_use]
pub fn design_review_prompt(request: &str, project: &Project, environment: Option<&str>) -> String {
    let file = |name: &str| {
        std::fs::read_to_string(project.root.join(name))
            .ok()
            .map(|text| clipped(text.trim(), DESIGN_FILE_MAX_CHARS))
            .filter(|text| !text.is_empty())
            .unwrap_or_else(|| "(none yet)".to_string())
    };
    let mut prompt = format!(
        "The user asked:\n{}\n\n{SPEC_FILE}:\n{}\n\n{PLAN_FILE}:\n{}\n\n{DECISIONS_FILE}:\n{}\n\n",
        request.trim(),
        file(SPEC_FILE),
        file(PLAN_FILE),
        file(DECISIONS_FILE)
    );
    if let Some(environment) = environment {
        prompt.push_str(environment);
        prompt.push_str("\n\n");
    }
    prompt.push_str("Review them now, before any code is written, and end with the VERDICT line.");
    prompt
}

/// Remarques du relecteur d'architecture, pour le modèle.
#[must_use]
pub fn design_review_feedback(model: &str, findings: &str) -> String {
    format!(
        "{CLAW_NOTE_PREFIX} Before the code, another model ({model}) reviewed {SPEC_FILE} and {PLAN_FILE} and raised these points:\n{findings}\n\nFor each point: if it is right, change {SPEC_FILE}, {PLAN_FILE} or {DECISIONS_FILE} now, before writing code; if it is wrong, add a line to {DECISIONS_FILE} that says why you keep your choice. Then go on with the plan."
    )
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
    /// Vérifications en échec de chaque jalon pendant ce tour.
    pub failures: BTreeMap<String, usize>,
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
    /// La demande du tour (avec celle qu'une relance poursuit), pour la
    /// relecture d'architecture.
    pub request: String,
    /// La relecture d'architecture a déjà été tentée pendant ce tour.
    pub design_review_tried: bool,
    /// Démonstrations finales en échec renvoyées pendant ce tour.
    pub demo_rounds: usize,
    /// Requêtes du tour au moment où le dernier jalon s'est terminé (0 au
    /// début du tour) : le coût du jalon suivant part de là.
    pub requests_at_milestone: usize,
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
        // Ce qui est créé n'a pas à être connu de Claw (appli web, 14 octobre 2026).
        assert!(creation_request(
            "Crée un Comparateur de prix : Scraping (legal), alertes baisse, historique \nPour les supermarchés au alentour"
        ));
        assert!(creation_request(
            "Je veux un comparateur de prix pour les supermarchés de mon quartier"
        ));
        assert!(creation_request(
            "Build a price comparison tool that scrapes local supermarkets every day"
        ));
        assert!(creation_request(
            "Peux-tu créer une application de quiz en Python avec Flask ?"
        ));
        assert!(creation_request(
            "Crée-moi un bot Discord qui annonce les matchs de foot du jour"
        ));
        assert!(creation_request(
            "Build a resume builder website with templates and a PDF export"
        ));
        // Un texte ou un document seul, une question, un savoir : pas un projet.
        assert!(!creation_request(
            "Fais-moi une liste d'idées de projets web pour débuter en JavaScript"
        ));
        assert!(!creation_request(
            "Écris un README qui présente le dépôt et ses objectifs principaux"
        ));
        assert!(!creation_request(
            "Crée un fichier .gitignore pour Python, Node et les éditeurs courants"
        ));
        assert!(!creation_request(
            "Écris le cahier des charges d'une application de covoiturage entre voisins"
        ));
        assert!(!creation_request(
            "Prépare un plan de migration de la base de données vers PostgreSQL"
        ));
        assert!(!creation_request(
            "Comment créer une application de quiz en Python avec Flask ?"
        ));
        assert!(!creation_request(
            "J'aimerais comprendre comment fonctionnent les hooks de React en détail"
        ));
        assert!(!creation_request(
            "I want to understand how React hooks work under the hood"
        ));
        assert!(!creation_request("Donne des idées de projet web"));
        // « ce code », « le développement » : des noms, pas des verbes.
        assert!(!creation_request(
            "Explique ce code\n\nFichier : src/lib.rs, lignes 3 à 5\n\n```rust\nfn main() {}\n```"
        ));
        assert!(!creation_request(
            "Explique le développement d'une application mobile avec Flutter en quelques mots"
        ));
        assert!(creation_request(
            "Code un jeu du serpent en JavaScript avec un tableau des scores"
        ));
        assert!(creation_request(
            "Essaie de créer une application de quiz avec Flask et SQLite"
        ));
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
    fn verify_lines_keep_only_commands_and_a_milestone_without_status_is_pointed_out() {
        // Plans écrits dans les sessions 5 (test_claw) et 9 (edt_stri) de l'appli web.
        let plan = Plan::parse(
            "### M1 - Existant\n**Requirements:** R1\n**Verify:** `python tests/verif_edt.py` (sans `--hors-ligne`) → 0 anomalie\n**Status:** done\n\n### M2 - CI\n**Requirements:** R1\n**Verify:** Workflow GitHub Actions `edt_sync.yml` passe avec `verif_edt.py` (sans `--sans-fraicheur`)\n**Status:** todo\n\n### M3 - Deux commandes\n**Requirements:** R1\n**Verify:** `python tests/a.py` et `./tests/b.sh --vite`\n\n## Demo\nInstall: `pip install -r requirements.txt`\nRun: `python app.py`\n",
        );
        assert_eq!(plan.milestones[0].verify, ["python tests/verif_edt.py"]);
        assert!(plan.milestones[0].done && plan.milestones[0].status_given);
        assert!(plan.milestones[1].verify.is_empty());
        assert_eq!(
            plan.milestones[2].verify,
            ["python tests/a.py", "./tests/b.sh --vite"]
        );
        assert!(!plan.milestones[2].done && !plan.milestones[2].status_given);
        let problems = plan_problems(&plan, None);
        assert!(problems
            .iter()
            .any(|problem| problem.starts_with("M2 has no `Verify: <command>` line")));
        assert!(problems
            .iter()
            .any(|problem| problem.starts_with("M3 has no `Status: todo` or `Status: done` line")));
        // L'état écrit dans le titre compte.
        let plan = Plan::parse("## [x] M1 - Fait\nVerify: `pytest`\n\n## M2 - Ensuite (done)\nVerify: `pytest`\n\n## [ ] M3 - Plus tard\nVerify: `pytest`\n");
        assert!(plan.milestones[0].done && plan.milestones[0].status_given);
        assert!(plan.milestones[1].done && plan.milestones[1].status_given);
        assert!(!plan.milestones[2].done && plan.milestones[2].status_given);
    }

    #[test]
    fn the_notes_an_engine_adds_are_not_the_users_words() {
        let request = "Ajoute une licence MIT\n\n[Claw] Earlier conversation:\n\n[Request]\nsans mode projet\n\n[Claw] You are working in the repository abasse-ali/todo-app: make reasonable choices.";
        assert_eq!(own_words(request), "Ajoute une licence MIT");
        assert_eq!(read_wish(own_words(request)), Wish::Nothing);
        assert!(!creation_request(own_words(request)));
        assert!(creation_request(request));
        assert_eq!(own_words("Crée une appli\n"), "Crée une appli\n");
    }

    #[test]
    fn earlier_messages_of_the_user_are_kept_and_the_answers_left_out() {
        // Session #32, passage 5 : la réponse de la boulangère ne parlait plus
        // des clientes qui réservent ; le premier message, si.
        let request = "Je ne saurai pas créer un compte Firebase.\n\n[Claw] Earlier conversation of this session, for context only.\n\n[Request]\nles clients réservent, moi je vois la liste\n\n[Your answer]\nJ'ai ajouté Firebase.\n\n[Message]\nla réservation de ma fille n'apparait pas\n\n[Claw] End of the earlier conversation.\n\n> Je ne saurai pas créer un compte Firebase.";
        let words = user_words_with_history(request);
        assert!(words.starts_with("Je ne saurai pas créer un compte Firebase."));
        assert!(words.contains("les clients réservent, moi je vois la liste"));
        assert!(words.contains("ma fille n'apparait pas"));
        assert!(!words.contains("J'ai ajouté Firebase"), "{words}");
        assert!(!words.contains("End of the earlier"), "{words}");
        assert_eq!(user_words_with_history("Crée une appli"), "Crée une appli");
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
    fn the_environment_note_names_the_tools_and_the_known_pitfalls() {
        let environment = Environment {
            windows: true,
            git_bash: true,
            python: Some(("python".to_string(), "3.13.1".to_string())),
            python_encoding: Some("cp1252".to_string()),
            pytest: None,
            time_zones: Some(false),
            tools: vec![
                ("Node".to_string(), Some("22.11.0".to_string())),
                ("Git".to_string(), Some("2.47.1.windows.1".to_string())),
                ("Cargo".to_string(), None),
                ("Java".to_string(), None),
            ],
        };
        let note = environment_note(&environment);
        assert!(note.contains("- Windows; the bash tool runs Git Bash."));
        assert!(note.contains("- Python 3.13.1, run it as `python`. pytest is not installed for it: run `python -m pip install pytest`"));
        assert!(note.contains("Python writes it as cp1252"));
        assert!(note.contains("cgi, cgitb, imghdr"));
        assert!(note.contains("add `tzdata` to the requirements"));
        assert!(note.contains("- Node 22.11.0; Git 2.47.1.windows.1."));
        assert!(note.contains("- Not installed: Cargo, Java."));
        // Python en UTF-8, ancien, avec pytest et les fuseaux : rien de tout cela.
        let quiet = environment_note(&Environment {
            python: Some(("python3".to_string(), "3.11.4".to_string())),
            python_encoding: Some("utf-8".to_string()),
            pytest: Some("8.3.4".to_string()),
            time_zones: Some(true),
            ..Environment::default()
        });
        assert!(quiet.contains("pytest 8.3.4 is installed."));
        assert!(!quiet.contains("UnicodeEncodeError"));
        assert!(!quiet.contains("cgi"));
        assert!(!quiet.contains("tzdata"));
        assert_eq!(
            version_in("git version 2.47.1.windows.1").as_deref(),
            Some("2.47.1.windows.1")
        );
        assert_eq!(
            version_in("go version go1.23.1 windows/amd64").as_deref(),
            Some("1.23.1")
        );
        assert_eq!(version_in("v22.11.0").as_deref(), Some("22.11.0"));
        assert_eq!(
            version_in("openjdk version \"21.0.1\" 2023-10-17").as_deref(),
            Some("21.0.1")
        );
        assert_eq!(python_release("3.13.1"), Some((3, 13)));
        assert_eq!(python_release("3.9.0rc1"), Some((3, 9)));
    }

    #[test]
    fn the_probe_finds_git_and_reports_missing_programs() {
        // Git est là partout où ces essais tournent.
        assert!(probe("git", &["--version"]).is_some_and(|text| text.starts_with("git version")));
        assert!(probe("claw-programme-qui-n-existe-pas", &["--version"]).is_none());
    }

    #[test]
    fn the_architecture_review_reads_the_request_the_three_files_and_the_machine() {
        let root = folder("relecture");
        std::fs::write(root.join(SPEC_FILE), "- R1: Ajouter un livre\n").expect("spec");
        std::fs::write(root.join(PLAN_FILE), PLAN).expect("plan");
        let project = Project::read(&root);
        let prompt = design_review_prompt(
            "Crée une bibliothèque",
            &project,
            Some("Environment probed by Claw on this machine:\n- Python 3.13.1"),
        );
        assert!(prompt.starts_with(
            "The user asked:\nCrée une bibliothèque\n\nSPEC.md:\n- R1: Ajouter un livre"
        ));
        assert!(prompt.contains("PLAN.md:\n# Plan"));
        assert!(prompt.contains("DECISIONS.md:\n(none yet)"));
        assert!(prompt.contains("- Python 3.13.1"));
        assert!(prompt.ends_with("end with the VERDICT line."));
        assert!(DESIGN_REVIEW_SYSTEM.contains("M1 must run from end to end"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_demo_written_with_subheadings_and_code_blocks_is_read_and_run_in_one_session() {
        let plan = Plan::parse(
            "## M1 - x\nVerify: pytest\nStatus: done\n\n## Demo\n\n### Install\n```bash\npython -m venv .venv\nsource .venv/Scripts/activate  # Windows\n$ pip install -r requirements.txt\n```\n\n### Run\n```bash\n# lancer\npython app.py stock\n```\n",
        );
        let demo = plan.demo.expect("démo");
        assert_eq!(
            demo.install,
            [
                "python -m venv .venv",
                "source .venv/Scripts/activate  # Windows",
                "pip install -r requirements.txt"
            ]
        );
        assert_eq!(demo.run, ["python app.py stock"]);
        assert_eq!(
            demo_script("C:/tmp/copie", &demo),
            "cd \"C:/tmp/copie\" && { python -m venv .venv; } && { source .venv/Scripts/activate; } && { pip install -r requirements.txt; } && { python app.py stock; }"
        );
        assert_eq!(
            without_comment("echo '# pas un commentaire' # fin"),
            "echo '# pas un commentaire'"
        );
        assert_eq!(without_comment("grep -c '#' a.txt"), "grep -c '#' a.txt");
    }

    #[test]
    fn the_clean_copy_holds_the_project_files_and_not_claw_s_own() {
        let root = folder("copie");
        std::fs::write(root.join("app.py"), "print(1)\n").expect("code");
        std::fs::create_dir_all(root.join("tests")).expect("tests");
        std::fs::write(
            root.join("tests").join("test_app.py"),
            "def test_r1(): pass\n",
        )
        .expect("test");
        std::fs::create_dir_all(root.join(".claw")).expect("claw");
        std::fs::write(root.join(".claw").join("project.json"), "{}").expect("état");
        std::fs::create_dir_all(root.join("__pycache__")).expect("cache");
        std::fs::write(root.join("__pycache__").join("app.pyc"), "x").expect("pyc");
        // Données écrites par les essais du modèle : une installation neuve part sans elles.
        std::fs::write(root.join("materiel.json"), "{\"prets\": [1]}").expect("données");
        std::fs::write(root.join("package.json"), "{}").expect("configuration");
        let copied = clean_copy(&root).expect("copie");
        let copy = copied.folder.clone();
        assert_eq!(copied.files, 3);
        assert_eq!(copied.left_out, ["materiel.json"]);
        assert!(copy.join("app.py").is_file());
        assert!(copy.join("package.json").is_file());
        assert!(copy.join("tests").join("test_app.py").is_file());
        assert!(!copy.join("materiel.json").exists());
        assert!(!copy.join(".claw").exists());
        assert!(!copy.join("__pycache__").exists());
        assert!(demo_failed_feedback(&copied, "python app.py", "erreur", false).contains(
            "Left out of the copy, as data the program writes when it runs and that Git does not track: materiel.json."
        ));
        let _ = std::fs::remove_dir_all(&copy);
        let run = vec!["python app.py liste".to_string()];
        assert!(!readme_shows(&root, &run));
        std::fs::write(
            root.join("README.md"),
            "Lancer :\n\n    python  app.py liste\n",
        )
        .expect("readme");
        assert!(readme_shows(&root, &run));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_budget_note_compares_the_free_quota_with_the_milestones_left() {
        let mut state = ProjectState::default();
        assert_eq!(milestone_cost(&state), DEFAULT_MILESTONE_REQUESTS);
        state.milestone_requests.insert("M1".to_string(), 20);
        state.milestone_requests.insert("M2".to_string(), 40);
        assert_eq!(milestone_cost(&state), 30);
        // Assez pour tout : rien à dire.
        assert!(budget_note(200, 30, 3).is_none());
        assert!(budget_note(10, 30, 0).is_none());
        let (guide, notice) = budget_note(70, 30, 3).expect("pas assez pour tout");
        assert!(guide.contains("there is room for about 2 of the 3 milestones left"));
        assert_eq!(
            notice,
            "Mode projet : il reste 70 requêtes gratuites aujourd'hui, de quoi faire environ 2 des 3 jalons restants"
        );
        let (guide, notice) = budget_note(12, 30, 2).expect("pas assez pour un jalon");
        assert!(guide.contains(
            "Do only what fits in about 6 requests, then stop with the project in a clean state"
        ));
        assert!(notice.contains("l'agent s'arrêtera sur un état propre"));
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
