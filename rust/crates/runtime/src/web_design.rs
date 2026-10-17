//! Interfaces web belles et fonctionnelles, au niveau qu'on attend d'un studio
//! de design : ce qu'il manque d'abord à un petit modèle, c'est une méthode
//! (un plan de design avant le code, les marques d'une page générée à éviter,
//! un plancher de qualité) et un regard sur le résultat. Claw joint la méthode
//! à une demande d'interface, puis, quand le modèle a fini, regarde la page
//! comme un utilisateur (captures téléphone et ordinateur, défauts mesurés) et
//! la fait juger par un modèle qui voit ; les corrections repartent au modèle.

use std::fmt::Write as _;
use std::path::Path;

use serde_json::{json, Value};

use crate::harness::GUIDE_PREFIX;

fn is_html(path: &str) -> bool {
    Path::new(path)
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("html") || ext.eq_ignore_ascii_case("htm"))
}

/// Revues visuelles au plus par tour (`CLAW_DESIGN_REVIEW_ROUNDS`).
const DEFAULT_REVIEW_ROUNDS: usize = 2;
/// Sous ce nombre de requêtes gratuites restantes, la revue est sautée.
pub const MIN_FREE_REQUESTS: u64 = 25;

/// Revues visuelles au plus par tour.
#[must_use]
pub fn max_review_rounds() -> usize {
    std::env::var("CLAW_DESIGN_REVIEW_ROUNDS")
        .ok()
        .and_then(|value| value.trim().parse::<usize>().ok())
        .unwrap_or(DEFAULT_REVIEW_ROUNDS)
        .min(5)
}

/// Mots qui désignent une interface web.
const INTERFACE_WORDS: &[&str] = &[
    "site",
    "page",
    "landing",
    "vitrine",
    "portfolio",
    "interface",
    "ui",
    "ux",
    "frontend",
    "front-end",
    "maquette",
    "dashboard",
    "tableau de bord",
    "html",
    "css",
    "webapp",
    "web app",
    "application web",
    "app web",
    "blog",
    "boutique en ligne",
    "e-commerce",
    "ecommerce",
    "navbar",
    "header",
    "footer",
    "hero",
    "mise en page",
    "layout",
    "responsive",
    "theme",
    "composant",
    "component",
    "website",
    "webpage",
    "homepage",
    "accueil",
];

/// Verbes d'une demande de construction ou de refonte.
const MAKE_WORDS: &[&str] = &[
    "cree",
    "creer",
    "crée",
    "fais",
    "faire",
    "fait",
    "construis",
    "construire",
    "developpe",
    "développe",
    "genere",
    "génère",
    "ecris",
    "écris",
    "realise",
    "réalise",
    "ajoute",
    "ajouter",
    "refais",
    "refaire",
    "refonte",
    "ameliore",
    "améliore",
    "embellis",
    "rends",
    "modernise",
    "redesign",
    "restyle",
    "stylise",
    "make",
    "build",
    "create",
    "design",
    "redo",
    "improve",
    "generate",
    "write",
    "add",
];

/// Mots de goût : « plus beau », « moderne », « joli ».
const TASTE_WORDS: &[&str] = &[
    "beau",
    "belle",
    "joli",
    "jolie",
    "moderne",
    "esthetique",
    "élégant",
    "elegant",
    "design",
    "professionnel",
    "pro",
    "magnifique",
    "superbe",
    "attrayant",
    "beautiful",
    "pretty",
    "modern",
    "stunning",
    "polished",
    "sleek",
];

fn has_any(folded: &str, words: &[&str]) -> bool {
    words.iter().any(|word| {
        let word = crate::team::without_accents(word);
        crate::team::has_word_start(folded, &word)
    })
}

/// La demande construit ou refait une interface web.
#[must_use]
pub fn is_interface_request(text: &str) -> bool {
    let folded = crate::team::without_accents(&text.to_lowercase()).replace('\u{2019}', "'");
    has_any(&folded, INTERFACE_WORDS)
        && (has_any(&folded, MAKE_WORDS) || has_any(&folded, TASTE_WORDS))
}

/// Méthode jointe à une demande d'interface (aussi à un message court qui
/// poursuit une telle demande).
#[must_use]
pub fn design_guide(request: &str, earlier: Option<&str>) -> Option<String> {
    let interface = is_interface_request(request)
        || (request.split_whitespace().count() <= 12 && earlier.is_some_and(is_interface_request));
    interface.then(|| format!("{GUIDE_PREFIX} {DESIGN_METHOD}"))
}

const DESIGN_METHOD: &str = "This request builds or reshapes a web interface. Aim for what a strong design studio would ship: distinctive, polished and fully working on phones and desktops, not a template.
1. Brief. In one line each: the subject, the audience, the page's main job. Use real, specific content in the user's language: no lorem ipsum, no \"Welcome to our website\", no stand-ins such as \"FB\" for a social link.
2. Design plan, written in your reply before any code:
- Color: 4 to 6 named hex values taken from the subject's own world (its materials, products, place), one accent used sparingly.
- Type: one or two typefaces chosen for this subject (Google Fonts with a <link>), and a type scale (sizes, weights, line heights). Not the usual defaults: Inter, Roboto, Arial, Open Sans, Montserrat, Poppins, Lato, Playfair Display, system-ui.
- Layout: an ASCII wireframe of the first screen and the order of the sections; text left-aligned (center only short lines); lines under 75 characters.
- One memorable element (a hero treatment, an illustration, a typographic idea, an interaction), with everything around it quiet and disciplined.
Then compare the plan with these marks of a generated page, and change each one you used without a reason specific to this brief: a plain colored band with centered \"Welcome\" text as the hero; rows of identical rounded cards with the same soft shadow; a short line under every heading; ALL-CAPS badges or labels above headings; emoji or one repeated icon standing in for photos; cream with terracotta, or near-black with one acid accent; 01/02/03 markers on things that are not steps; a fade-in on every section.
3. Build. CSS custom properties for the plan's tokens; semantic HTML (header, nav, main, section, footer); mobile first. On phones the navigation folds into a menu button that opens and closes it (aria-expanded), and a fixed header never covers content (offset the page by its height). Pictures: the user's images, or illustrations drawn for the subject with inline SVG or CSS, never broken or placeholder images. Forms: styled like the rest of the page (font: inherit), a visible label on every field, errors shown only after the user acts (:user-invalid, or on submit), a clear confirmation in the page. Visible :focus-visible styles, @media (prefers-reduced-motion: reduce), text contrast of at least 4.5:1. Watch CSS specificity: a .section rule and a .cta rule can cancel each other's spacing.
4. Make everything work: every link leads somewhere real (a section that exists, mailto:, tel:), every button does what its label says, forms validate and confirm, no console errors.
5. Check it as a user would: Screenshot with full_page true at phone and desktop sizes, then again with actions that open the menu and send the form empty and filled; ViewPage for console errors. Fix every problem listed, judge the images against your plan and the marks above, improve the weakest part, and remove one decoration that does not serve the subject. When you finish, Claw reviews the page as well.";

/// Fichier qui décide de l'apparence d'une page.
#[must_use]
pub fn is_interface_file(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    [
        ".html", ".htm", ".css", ".scss", ".sass", ".less", ".jsx", ".tsx", ".vue", ".svelte",
        ".astro", ".js", ".ts",
    ]
    .iter()
    .any(|extension| lower.ends_with(extension))
        && !lower.contains("node_modules")
        && !lower.ends_with(".test.ts")
        && !lower.ends_with(".test.js")
        && !lower.ends_with(".spec.ts")
}

/// Page que le modèle a regardée avec `Screenshot` ou `ViewPage` :
/// `url:<adresse>` ou `path:<fichier>`.
#[must_use]
pub fn page_target(input: &str) -> Option<String> {
    let value: Value = serde_json::from_str(input).ok()?;
    if let Some(url) = value["url"].as_str().map(str::trim) {
        let lower = url.to_ascii_lowercase();
        let local = [
            "http://localhost",
            "http://127.0.0.1",
            "http://0.0.0.0",
            "https://localhost",
            "file:",
        ]
        .iter()
        .any(|prefix| lower.starts_with(prefix));
        return local.then(|| format!("url:{url}"));
    }
    value["path"]
        .as_str()
        .map(str::trim)
        .filter(|path| !path.is_empty())
        .map(|path| format!("path:{path}"))
}

/// Page à revoir quand le modèle ne l'a pas regardée lui-même : un fichier
/// .html modifié (index.html d'abord), sinon `index.html` à la racine.
#[must_use]
pub fn page_from_files(root: &Path, changed: &[String]) -> Option<String> {
    let mut pages: Vec<&String> = changed
        .iter()
        .filter(|path| {
            let lower = path.to_ascii_lowercase();
            is_html(&lower) && !lower.contains("node_modules")
        })
        .filter(|path| root.join(path).is_file() || Path::new(path).is_file())
        .collect();
    pages.sort_by_key(|path| {
        (
            !path.to_ascii_lowercase().ends_with("index.html"),
            path.len(),
        )
    });
    if let Some(page) = pages.first() {
        return Some(format!("path:{page}"));
    }
    root.join("index.html")
        .is_file()
        .then(|| "path:index.html".to_string())
}

/// Entrée de l'outil `Screenshot` pour la revue : page entière, téléphone puis ordinateur.
#[must_use]
pub fn screenshot_input(target: &str) -> String {
    let mut input = json!({ "viewports": ["phone", "desktop"], "full_page": true });
    match target.split_once(':') {
        Some(("url", url)) => input["url"] = json!(url),
        Some(("path", path)) => input["path"] = json!(path),
        _ => input["path"] = json!(target),
    }
    input.to_string()
}

/// Ce que l'outil `Screenshot` a mesuré, toutes tailles d'écran confondues.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Audit {
    pub problems: Vec<String>,
    pub notes: Vec<String>,
    pub console: Vec<String>,
}

impl Audit {
    #[must_use]
    pub fn is_clean(&self) -> bool {
        self.problems.is_empty() && self.console.is_empty()
    }
}

fn push_unique(list: &mut Vec<String>, line: String) {
    if !list.contains(&line) {
        list.push(line);
    }
}

/// Lit le résultat de `Screenshot` (défauts par écran, remarques, console).
#[must_use]
pub fn read_audit(output: &str) -> Audit {
    let mut audit = Audit::default();
    let Ok(value) = serde_json::from_str::<Value>(output) else {
        return audit;
    };
    for capture in value["captures"].as_array().into_iter().flatten() {
        let screen = capture["screen"].as_str().unwrap_or("screen");
        for problem in capture["problems"].as_array().into_iter().flatten() {
            if let Some(problem) = problem.as_str() {
                push_unique(&mut audit.problems, format!("[{screen}] {problem}"));
            }
        }
        for note in capture["designNotes"].as_array().into_iter().flatten() {
            if let Some(note) = note.as_str() {
                push_unique(&mut audit.notes, note.to_string());
            }
        }
        for line in capture["console"].as_array().into_iter().flatten() {
            if let Some(line) = line.as_str() {
                let lower = line.to_ascii_lowercase();
                if lower.starts_with("error")
                    || lower.contains("exception")
                    || lower.contains("uncaught")
                {
                    push_unique(&mut audit.console, line.to_string());
                }
            }
        }
        if let Some(error) = capture["error"].as_str() {
            push_unique(&mut audit.problems, format!("[{screen}] {error}"));
        }
    }
    audit
}

/// Consigne du modèle qui juge les captures.
pub const CRITIC_SYSTEM: &str = "You are the art director of a design studio known for distinctive, polished websites. You review screenshots of a page built by a junior developer and give precise, actionable direction. You judge only what you can see in the screenshots.";

/// Ce qui est demandé au modèle qui juge les captures.
#[must_use]
pub fn critique_prompt(request: &str, images: usize, audit: &Audit) -> String {
    let measured = if audit.problems.is_empty() && audit.notes.is_empty() {
        "nothing".to_string()
    } else {
        audit
            .problems
            .iter()
            .chain(&audit.notes)
            .map(|line| format!("- {line}"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let brief: String = request.chars().take(1_500).collect();
    format!(
        "The client's brief:\n\"{brief}\"\n\n{images} screenshots follow, full page: the phone view first, then the desktop view.\n\nAlready measured in the page and sent to the developer (do not repeat these):\n{measured}\n\nJudge the page on:\n1. Phone: nothing cut, covered or cramped; a menu that fits; comfortable tap targets.\n2. Hierarchy: a clear first screen, the eye knows where to go, one strong focal point.\n3. Spacing and alignment: a consistent vertical rhythm, aligned edges, enough breathing room, no crowded or empty-looking areas.\n4. Typography: typefaces that fit the subject, a clear scale, comfortable line length, nothing default-looking.\n5. Color: a harmonious palette tied to the subject, readable contrast, the accent used sparingly.\n6. Identity: does it look made for this subject, or like a template (a plain colored hero band, identical cards, emoji or icon placeholders for photos, lines under every heading, ALL-CAPS badges)?\n7. Finish: anything unstyled, broken, empty, misaligned or unfinished.\n\nReply with at most 6 changes, the most important first, one per line, each as:\nN. [area] what is wrong, as seen in the screenshots -> the exact change to make (CSS properties and values, HTML structure, copy)\nBe specific to this page; no generic advice. The developer cannot get new photos: suggest inline SVG, CSS shapes or gradients instead. If the page is already polished and distinctive with nothing important left to change, reply exactly: NO CHANGES NEEDED"
    )
}

/// Changements demandés par le juge ; `None` s'il n'y a rien à changer.
#[must_use]
pub fn critique_changes(text: &str) -> Option<String> {
    let text = text.trim();
    if text.is_empty() || text.to_ascii_uppercase().contains("NO CHANGES NEEDED") {
        return None;
    }
    let lines: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|line| {
            line.chars()
                .next()
                .is_some_and(|first| first.is_ascii_digit())
                || line.starts_with('-')
        })
        .take(6)
        .collect();
    if lines.is_empty() {
        return Some(text.chars().take(2_000).collect());
    }
    Some(lines.join("\n"))
}

/// Message au modèle après la revue ; `None` quand il n'y a rien à corriger.
#[must_use]
pub fn review_feedback(
    target: &str,
    audit: &Audit,
    critique: Option<(&str, &str)>,
    round: usize,
    rounds: usize,
) -> Option<String> {
    if audit.is_clean() && critique.is_none() {
        return None;
    }
    let page = target.split_once(':').map_or(target, |(_, page)| page);
    let mut text = format!(
        "[Claw] Claw looked at the page you built ({page}) as a user would, full page at phone and desktop sizes (review {round} of {rounds})."
    );
    if !audit.problems.is_empty() {
        text.push_str("\nMeasured problems, fix every one:\n");
        text.push_str(
            &audit
                .problems
                .iter()
                .map(|line| format!("- {line}"))
                .collect::<Vec<_>>()
                .join("\n"),
        );
    }
    if !audit.console.is_empty() {
        text.push_str("\nConsole errors:\n");
        text.push_str(
            &audit
                .console
                .iter()
                .take(8)
                .map(|line| format!("- {line}"))
                .collect::<Vec<_>>()
                .join("\n"),
        );
    }
    if !audit.notes.is_empty() {
        text.push_str("\nDesign notes (choices that make the page look generic; keep one only if it was deliberate for this subject):\n");
        text.push_str(
            &audit
                .notes
                .iter()
                .map(|line| format!("- {line}"))
                .collect::<Vec<_>>()
                .join("\n"),
        );
    }
    if let Some((model, changes)) = critique {
        let _ = write!(
            text,
            "\nArt direction from another model ({model}), which looked at the screenshots:\n{changes}"
        );
    }
    text.push_str("\nMake these changes in the code, keep what already works, check again with Screenshot (full_page, phone and desktop), then give your answer.");
    Some(text)
}

#[cfg(test)]
mod tests {
    use super::{
        critique_changes, design_guide, is_interface_file, is_interface_request, page_from_files,
        page_target, read_audit, review_feedback, screenshot_input, Audit,
    };
    use serde_json::json;

    #[test]
    fn interface_requests_are_recognized() {
        assert!(is_interface_request(
            "Crée un site vitrine pour une boulangerie"
        ));
        assert!(is_interface_request("Rends la page d'accueil plus belle"));
        assert!(is_interface_request("Build a landing page for my app"));
        assert!(is_interface_request(
            "Fais une interface moderne pour le tableau de bord"
        ));
        assert!(!is_interface_request(
            "Ajoute un test pour la fonction de tri"
        ));
        assert!(!is_interface_request(
            "Pourquoi le serveur plante au démarrage ?"
        ));
        assert!(design_guide("Crée un site pour un fleuriste", None).is_some());
        assert!(design_guide("plus coloré", Some("Crée un site pour un fleuriste")).is_some());
        assert!(design_guide("corrige le calcul de la TVA", None).is_none());
    }

    #[test]
    fn pages_and_files_are_found() {
        assert!(is_interface_file("assets/css/style.css"));
        assert!(is_interface_file("src/App.tsx"));
        assert!(!is_interface_file("node_modules/x/index.js"));
        assert!(!is_interface_file("src/main.rs"));
        assert_eq!(
            page_target(r#"{"url":"http://localhost:8000/"}"#).as_deref(),
            Some("url:http://localhost:8000/")
        );
        assert_eq!(page_target(r#"{"url":"https://example.com"}"#), None);
        assert_eq!(
            page_target(r#"{"path":"index.html"}"#).as_deref(),
            Some("path:index.html")
        );
        let dir = std::env::temp_dir().join(format!("claw-web-design-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("pages")).expect("dossier");
        std::fs::write(dir.join("pages").join("contact.html"), "<p>x</p>").expect("page");
        std::fs::write(dir.join("index.html"), "<p>x</p>").expect("index");
        assert_eq!(
            page_from_files(
                &dir,
                &["pages/contact.html".to_string(), "index.html".to_string()]
            )
            .as_deref(),
            Some("path:index.html")
        );
        assert_eq!(
            page_from_files(&dir, &[]).as_deref(),
            Some("path:index.html")
        );
        let _ = std::fs::remove_dir_all(&dir);
        let input: serde_json::Value =
            serde_json::from_str(&screenshot_input("url:http://localhost:5173/")).expect("json");
        assert_eq!(input["url"], "http://localhost:5173/");
        assert_eq!(input["full_page"], true);
    }

    #[test]
    fn the_review_reads_the_audit_and_the_critique() {
        let output = json!({
            "captures": [
                { "screen": "phone 390x844", "problems": ["Header covers the title"], "designNotes": [], "console": ["error: x is not defined", "log: ok"] },
                { "screen": "desktop 1280x800", "problems": [], "designNotes": ["Typefaces Inter are defaults"], "console": ["error: x is not defined"] }
            ]
        })
        .to_string();
        let audit = read_audit(&output);
        assert_eq!(
            audit.problems,
            vec!["[phone 390x844] Header covers the title"]
        );
        assert_eq!(audit.console, vec!["error: x is not defined"]);
        assert_eq!(audit.notes.len(), 1);
        assert_eq!(critique_changes("Looks great. NO CHANGES NEEDED"), None);
        let changes = critique_changes(
            "Here are my notes:\n1. [Hero] flat band -> add an SVG\n2. [Type] Inter -> Fraunces",
        )
        .expect("changes");
        assert_eq!(changes.lines().count(), 2);
        let feedback = review_feedback(
            "path:index.html",
            &audit,
            Some(("vision-model", &changes)),
            1,
            2,
        )
        .expect("feedback");
        assert!(
            feedback.contains("index.html")
                && feedback.contains("Header covers")
                && feedback.contains("vision-model")
        );
        assert_eq!(
            review_feedback("path:index.html", &Audit::default(), None, 1, 2),
            None
        );
    }
}
