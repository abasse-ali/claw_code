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
const DEFAULT_REVIEW_ROUNDS: usize = 3;
/// Sous ce nombre de requêtes gratuites restantes, la revue est sautée.
pub const MIN_FREE_REQUESTS: u64 = 25;
/// Note sur 10 que chaque critère doit atteindre (`CLAW_DESIGN_BAR`) : 8, ce
/// qu'un bon studio livrerait.
const DEFAULT_DESIGN_BAR: u8 = 8;

/// Revues visuelles au plus par tour.
#[must_use]
pub fn max_review_rounds() -> usize {
    std::env::var("CLAW_DESIGN_REVIEW_ROUNDS")
        .ok()
        .and_then(|value| value.trim().parse::<usize>().ok())
        .unwrap_or(DEFAULT_REVIEW_ROUNDS)
        .min(5)
}

/// Exigence de la revue : la note que chaque critère doit atteindre, de 5
/// (un gabarit correct) à 10 (digne d'un prix).
#[must_use]
pub fn design_bar() -> u8 {
    std::env::var("CLAW_DESIGN_BAR")
        .ok()
        .and_then(|value| value.trim().parse::<u8>().ok())
        .unwrap_or(DEFAULT_DESIGN_BAR)
        .clamp(5, 10)
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
    interface.then(|| format!("{GUIDE_PREFIX} {}", design_method(design_bar())))
}

/// Méthode d'un studio exigeant : des cibles chiffrées plutôt que des
/// conseils, les marques d'une page générée interdites, et une note à
/// atteindre. Vu dans les essais réels : avec « change-les sans raison
/// propre au sujet », un modèle gratuit gardait ses badges en capitales et
/// Playfair Display en écrivant qu'il les avait évités.
fn design_method(bar: u8) -> String {
    format!("This request builds or reshapes a web interface. The bar is a page a top design studio would ship and sign: distinctive, polished and fully working on phones and desktops, never a template. When you finish, Claw has the page scored by an art director, 1 to 10 on seven criteria (phone, hierarchy, spacing, typography, color, identity, finish), and sends back every criterion under {bar}.
1. Brief. In one line each: the subject, the audience, the page's main job. Use real, specific content in the user's language: no lorem ipsum, no \"Welcome to our website\", no stand-ins such as \"FB\" for a social link.
2. Design plan, written in your reply before any code, with numbers:
- Color: 4 to 6 named hex values taken from the subject's own world (its materials, products, place), one accent covering at most a tenth of the page; every text color checked at 4.5:1 against its background.
- Type: one display face and one text face chosen for this subject (Google Fonts with a <link>), 2 or 3 weights in all; never Inter, Roboto, Arial, Open Sans, Montserrat, Poppins, Lato, Playfair Display, Space Grotesk or system-ui. A modular scale of 5 to 7 sizes with a ratio of at least 1.25: h1 at least 3rem on desktops and 2.25rem on phones, body text 16 to 18px with a line-height of 1.5 to 1.7, lines of 45 to 75 characters.
- Space: one spacing scale on multiples of 8px (for example 8, 16, 24, 40, 64, 96) and nothing else; the same vertical padding for sections of the same rank; no empty band taller than half a screen.
- Layout: an ASCII wireframe of the first screen and the order of the sections. Text left-aligned (center only one-line titles and short captions). Vary the composition from one section to the next (split, full width, grid, list, offset), never the same centered title over a grid each time.
- One memorable element (a hero treatment, an illustration, a typographic idea, an interaction), with everything around it quiet and disciplined.
Forbidden unless the brief asks for it, because they mark a generated page: a plain colored band with centered \"Welcome\" text as the hero; rows of identical cards with the same soft shadow; a short line under every heading; ALL-CAPS badges or small labels above headings; emoji or one repeated icon standing in for pictures; cream with terracotta, or near-black with one acid accent; 01/02/03 markers on things that are not steps; a fade-in on every section; decorative gradients everywhere. Check your plan against this list and say in one line that it uses none of them.
3. Build. CSS custom properties for every color, size and space of the plan, and no raw values outside them (except 0 and 1px borders). Semantic HTML (header, nav, main, section, footer) with exactly one h1 and no skipped heading levels; mobile first. On phones the navigation folds into a menu button that opens and closes it (aria-expanded), and a fixed header never covers content (offset the page by its height). Every link, button and field has hover, :focus-visible and active states, with transitions of 150 to 250ms, and @media (prefers-reduced-motion: reduce) stops the motion. Tap targets of at least 44x44px on phones. Pictures: the user's images, or illustrations drawn for the subject with inline SVG (each with an explicit width and height) or CSS, never broken or placeholder images. Forms: styled like the rest of the page (font: inherit), a visible label on every field, errors shown only after the user acts (:user-invalid, or on submit), a clear confirmation in the page. Watch CSS specificity: a .section rule and a .cta rule can cancel each other's spacing.
4. Make everything work: every link leads somewhere real (a section that exists, mailto:, tel:), every button does what its label says, forms validate and confirm, no console errors.
5. Check and critique before you answer: Screenshot with full_page true at phone and desktop sizes, then again with actions that open the menu and send the form empty and filled; ViewPage for console errors. Fix every measured problem and every design note. Then score the screenshots yourself, 1 to 10 on the seven criteria, as a strict art director would (most generated pages deserve 4 to 6; an 8 must be earned by something you can point to), and improve every criterion under {bar} before you answer. Remove one decoration that does not serve the subject.")
}

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
pub const CRITIC_SYSTEM: &str = "You are the art director of a design studio known for distinctive, polished websites, and you are hard to please. You review screenshots of a page built by a junior developer, score it honestly against what your studio would ship, and give precise, actionable direction. You judge only what you can see in the screenshots.";

/// Critères notés par le juge, dans l'ordre de la ligne `SCORES:`.
pub const CRITERIA: [&str; 7] = [
    "phone",
    "hierarchy",
    "spacing",
    "typography",
    "color",
    "identity",
    "finish",
];

/// Ce qui est demandé au modèle qui juge les captures : une note par
/// critère, puis les changements qui relèvent chaque note sous `bar`.
#[must_use]
pub fn critique_prompt(request: &str, images: usize, audit: &Audit, bar: u8) -> String {
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
        "The client's brief:\n\"{brief}\"\n\n{images} screenshots follow, full page: the phone view first, then the desktop view.\n\nAlready measured in the page and sent to the developer (do not repeat these):\n{measured}\n\nScore the page from 1 to 10 on each criterion, against what a top studio would ship: 5 is a competent template, 8 is what a strong studio would sign, 10 is award-level. Be strict: most generated pages deserve 4 to 6, and an 8 must be earned by something specific you can point to.\n1. phone: nothing cut, covered or cramped; a menu that fits; comfortable tap targets; text sized for a phone.\n2. hierarchy: a clear first screen with one strong focal point; the eye knows where to go next; headings, text and actions clearly ranked.\n3. spacing: one consistent vertical rhythm, aligned edges, generous but even breathing room, no crowded areas and no empty bands.\n4. typography: typefaces chosen for this subject, a clear scale with real contrast between levels, comfortable line length and leading, nothing default-looking.\n5. color: a harmonious palette tied to the subject, readable contrast, the accent used sparingly and on purpose.\n6. identity: does it look made for this subject, with a memorable idea, or like a template (a plain colored hero band, identical cards, emoji or icon placeholders for photos, lines under every heading, ALL-CAPS badges, the same layout in every section)?\n7. finish: craft in the details (alignment, consistent radii and shadows, polished icons and illustrations, states of buttons and fields); anything unstyled, broken, empty or unfinished.\n\nThe first line of your reply is exactly in this form:\nSCORES: phone=N hierarchy=N spacing=N typography=N color=N identity=N finish=N\nThen, for every criterion under {bar}, the changes that would raise it, the most important first, at most 8 in all, one per line, each as:\nN. [criterion] what is wrong, as seen in the screenshots -> the exact change to make (CSS properties and values, HTML structure, copy)\nBe specific to this page; no generic advice. The developer cannot get new photos: suggest inline SVG, CSS shapes or gradients instead. If every score is {bar} or more, write NO CHANGES NEEDED after the scores line."
    )
}

/// Notes du juge, lues sur sa ligne `SCORES:` (`typography=7`, `color: 8/10`).
#[must_use]
pub fn critique_scores(text: &str) -> Vec<(String, u8)> {
    let Some(line) = text
        .lines()
        .find(|line| line.to_ascii_uppercase().contains("SCORES"))
    else {
        return Vec::new();
    };
    let lower = line.to_ascii_lowercase();
    CRITERIA
        .iter()
        .filter_map(|criterion| {
            let after = &lower[lower.find(criterion)? + criterion.len()..];
            let after =
                after.trim_start_matches(|ch: char| ch == '=' || ch == ':' || ch.is_whitespace());
            let digits: String = after.chars().take_while(char::is_ascii_digit).collect();
            let score: u8 = digits.parse().ok()?;
            (1..=10)
                .contains(&score)
                .then(|| ((*criterion).to_string(), score))
        })
        .collect()
}

/// Moyenne des notes, pour l'annoncer.
#[must_use]
pub fn average_score(scores: &[(String, u8)]) -> Option<f32> {
    let count = u16::try_from(scores.len())
        .ok()
        .filter(|count| *count > 0)?;
    let total: u16 = scores.iter().map(|(_, score)| u16::from(*score)).sum();
    Some(f32::from(total) / f32::from(count))
}

/// Changements demandés par le juge, précédés de ses notes ; `None` quand
/// chaque critère atteint `bar`. Un « rien à changer » ne compte pas tant
/// qu'une note reste sous la barre.
#[must_use]
pub fn critique_changes(text: &str, bar: u8) -> Option<String> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    let scores = critique_scores(text);
    let below: Vec<&(String, u8)> = scores.iter().filter(|(_, score)| *score < bar).collect();
    let satisfied = text.to_ascii_uppercase().contains("NO CHANGES NEEDED");
    if below.is_empty() && (satisfied || !scores.is_empty()) {
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
        .take(8)
        .collect();
    let mut verdict = String::new();
    if !scores.is_empty() {
        let shown: Vec<String> = scores
            .iter()
            .map(|(criterion, score)| format!("{criterion} {score}"))
            .collect();
        let under: Vec<&str> = below
            .iter()
            .map(|(criterion, _)| criterion.as_str())
            .collect();
        let _ = writeln!(
            verdict,
            "Scores (the bar is {bar}/10 on each): {}; under the bar: {}",
            shown.join(", "),
            under.join(", ")
        );
    }
    if !lines.is_empty() {
        verdict.push_str(&lines.join("\n"));
    } else if satisfied {
        verdict.push_str("Raise every criterion under the bar: look at the page again with that criterion in mind and improve it.");
    } else {
        verdict.extend(text.chars().take(2_000));
    }
    Some(verdict)
}

/// Message au modèle après la revue ; `None` quand il n'y a rien à corriger.
#[must_use]
pub fn review_feedback(
    target: &str,
    audit: &Audit,
    critique: Option<(&str, &str)>,
    round: usize,
    rounds: usize,
    bar: u8,
) -> Option<String> {
    if audit.is_clean() && audit.notes.is_empty() && critique.is_none() {
        return None;
    }
    let page = target.split_once(':').map_or(target, |(_, page)| page);
    let mut text = format!(
        "[Claw] Claw looked at the page you built ({page}) as a user would, full page at phone and desktop sizes (review {round} of {rounds}). The bar is a page a top studio would ship: {bar}/10 or more on every criterion."
    );
    let bullets = |lines: &mut dyn Iterator<Item = &String>| {
        lines
            .map(|line| format!("- {line}"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    if !audit.problems.is_empty() {
        text.push_str("\nMeasured problems, fix every one:\n");
        text.push_str(&bullets(&mut audit.problems.iter()));
    }
    if !audit.console.is_empty() {
        text.push_str("\nConsole errors:\n");
        text.push_str(&bullets(&mut audit.console.iter().take(8)));
    }
    if !audit.notes.is_empty() {
        text.push_str("\nDesign notes, measured choices that make the page look generic or unpolished: fix each one; keep one only if the brief asks for it, and say why in your answer:\n");
        text.push_str(&bullets(&mut audit.notes.iter()));
    }
    if let Some((model, changes)) = critique {
        let _ = write!(
            text,
            "\nArt direction from another model ({model}), which looked at the screenshots:\n{changes}"
        );
    }
    text.push_str("\nMake these changes in the code, keep what already works, check again with Screenshot (full_page, phone and desktop), then give your answer.");
    if round >= rounds {
        text.push_str(" This was the last review of this turn: in your answer, give the user the scores and say plainly what is still under the bar.");
    }
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
        assert_eq!(critique_changes("Looks great. NO CHANGES NEEDED", 8), None);
        let changes = critique_changes(
            "Here are my notes:\n1. [Hero] flat band -> add an SVG\n2. [Type] Inter -> Fraunces",
            8,
        )
        .expect("changes");
        assert_eq!(changes.lines().count(), 2);
        let feedback = review_feedback(
            "path:index.html",
            &audit,
            Some(("vision-model", &changes)),
            1,
            2,
            8,
        )
        .expect("feedback");
        assert!(
            feedback.contains("index.html")
                && feedback.contains("Header covers")
                && feedback.contains("vision-model")
                && feedback.contains("8/10")
                && !feedback.contains("last review")
        );
        // Dernière revue du tour : le modèle doit dire ce qui reste sous la barre.
        assert!(review_feedback("path:index.html", &audit, None, 2, 2, 8)
            .expect("feedback")
            .contains("last review"));
        assert_eq!(
            review_feedback("path:index.html", &Audit::default(), None, 1, 2, 8),
            None
        );
    }

    #[test]
    fn the_judge_scores_are_read_and_a_score_under_the_bar_is_never_good_enough() {
        use super::{average_score, critique_scores, design_bar, design_guide};
        let reply = "SCORES: phone=8 hierarchy=7 spacing=9 typography=6 color=8 identity=5 finish=8\n1. [identity] generic cards -> draw one SVG per product\n2. [typography] Inter -> Fraunces 600 for headings";
        let scores = critique_scores(reply);
        assert_eq!(scores.len(), 7);
        assert_eq!(scores[3], ("typography".to_string(), 6));
        let mean = average_score(&scores).expect("moyenne");
        assert!((mean - 51.0 / 7.0).abs() < 0.001, "{mean}");
        let changes = critique_changes(reply, 8).expect("sous la barre");
        assert!(
            changes.starts_with("Scores (the bar is 8/10 on each)")
                && changes.contains("under the bar: hierarchy, typography, identity")
                && changes.lines().count() == 3,
            "{changes}"
        );
        // Toutes les notes à la barre : rien à changer, même sans « NO CHANGES NEEDED ».
        assert_eq!(
            critique_changes(
                "SCORES: phone=8 hierarchy=9 spacing=8 typography=8 color=8 identity=8 finish=10",
                8
            ),
            None
        );
        // « Rien à changer » avec une note sous la barre : le juge s'est satisfait trop vite.
        let lazy = critique_changes(
            "SCORES: phone=9 hierarchy=8 spacing=8 typography=8 color=8 identity=6 finish=8\nNO CHANGES NEEDED",
            8,
        )
        .expect("identity sous la barre");
        assert!(
            lazy.contains("Raise every criterion under the bar"),
            "{lazy}"
        );
        // Autres écritures des notes.
        assert_eq!(
            critique_scores("Scores: phone: 7/10, hierarchy: 8/10").len(),
            2
        );
        assert!(critique_scores("No scores here").is_empty());
        // La barre et la méthode.
        assert_eq!(design_bar(), 8);
        let guide = design_guide("Crée un site pour un fleuriste", None).expect("méthode");
        assert!(
            guide.contains("under 8")
                && guide.contains("Forbidden unless the brief asks for it")
                && guide.contains("multiples of 8px"),
            "{guide}"
        );
    }
}
