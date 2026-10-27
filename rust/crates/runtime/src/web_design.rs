//! Interfaces web belles et fonctionnelles, au niveau qu'on attend d'un studio
//! de design : ce qu'il manque d'abord à un petit modèle, c'est une méthode
//! (un plan de design avant le code, les marques d'une page générée à éviter,
//! un plancher de qualité) et un regard sur le résultat. Claw joint la méthode
//! à une demande d'interface, puis, quand le modèle a fini, regarde la page
//! comme un utilisateur (captures téléphone et ordinateur, défauts mesurés) et
//! la fait juger par un modèle qui voit ; les corrections repartent au modèle.

use regex::Regex;
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

/// Requête du tour à partir de laquelle une interface modifiée et jamais
/// revue l'est sans attendre la fin du tour : aux trois cinquièmes du budget
/// (`limit`), il reste de quoi appliquer ce que le juge demande. Vu dans un
/// essai réel : un modèle a consommé ses 100 requêtes à se corriger seul,
/// et la limite a emporté la revue avec elle.
#[must_use]
pub fn early_review_at(limit: usize) -> usize {
    (limit * 3 / 5).max(12)
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

/// Poursuites au plus d'un tour de design resté sous la barre
/// (`CLAW_DESIGN_CONTINUE`, 0 les coupe).
const DEFAULT_CONTINUATIONS: usize = 2;
/// Requêtes ajoutées au tour par une poursuite.
pub const CONTINUATION_REQUESTS: usize = 60;
/// Requêtes gratuites gardées pour la journée : en dessous, pas de poursuite.
pub const CONTINUATION_RESERVE: u64 = 150;
/// Hausse de la note moyenne en dessous de laquelle une revue n'a rien
/// gagné : la page ne progresse plus, poursuivre gaspillerait le quota.
const MIN_GAIN: f32 = 0.2;

/// Poursuites au plus d'un tour de design.
#[must_use]
pub fn max_continuations() -> usize {
    std::env::var("CLAW_DESIGN_CONTINUE")
        .ok()
        .and_then(|value| value.trim().parse::<usize>().ok())
        .unwrap_or(DEFAULT_CONTINUATIONS)
        .min(5)
}

/// Une revue notée : la moyenne des critères, et ceux restés sous la barre.
#[derive(Debug, Clone, PartialEq)]
pub struct Graded {
    pub mean: f32,
    pub under: Vec<String>,
    /// Les notes de chaque critère, repère du juge à la revue suivante.
    pub scores: Vec<(String, u8)>,
}

/// Ce que décide Claw quand un tour de design touche sa limite (requêtes ou
/// revues) : poursuivre, ou s'arrêter, et pourquoi.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Continuation {
    /// La note monte et reste sous la barre : le tour continue.
    Continue,
    /// Aucune revue notée, ou aucune depuis la dernière poursuite : rien ne
    /// dit qu'une poursuite de plus servirait.
    Unscored,
    /// Chaque critère a atteint la barre.
    Reached,
    /// La dernière revue n'a pas fait monter la note.
    Stalled,
    /// Toutes les poursuites permises ont servi.
    Exhausted,
    /// Il reste trop peu de requêtes gratuites aujourd'hui.
    LowQuota,
}

/// Faut-il poursuivre un tour de design arrivé à sa limite ? Oui tant que la
/// page reste sous la barre et que sa note monte d'une revue à l'autre, dans
/// la limite des poursuites permises et du quota du jour. Une seule revue
/// suffit : ses changements n'ont pas encore été essayés. `seen` est le
/// nombre de revues notées à la poursuite précédente : chaque poursuite en
/// demande une nouvelle.
#[must_use]
pub fn continuation(
    graded: &[Graded],
    seen: usize,
    used: usize,
    max: usize,
    free_left: Option<u64>,
) -> Continuation {
    let Some(last) = graded.last().filter(|_| graded.len() > seen) else {
        return Continuation::Unscored;
    };
    if last.under.is_empty() {
        return Continuation::Reached;
    }
    if used >= max {
        return Continuation::Exhausted;
    }
    if free_left.is_some_and(|left| left < CONTINUATION_RESERVE) {
        return Continuation::LowQuota;
    }
    // La meilleure note d'avant, pas seulement la précédente : vu le 25 octobre
    // 2026 (session #30), 4,9 puis 4,1 puis 4,6 valait une deuxième poursuite
    // (4,6 au-dessus de 4,1) et 120 requêtes de plus pour une page moins bien
    // notée qu'au départ.
    let best_before = graded[..graded.len() - 1]
        .iter()
        .map(|graded| graded.mean)
        .fold(None, |best: Option<f32>, mean| {
            Some(best.map_or(mean, |best| best.max(mean)))
        });
    if best_before.is_some_and(|best| last.mean < best + MIN_GAIN) {
        return Continuation::Stalled;
    }
    Continuation::Continue
}

/// Design atteint : la moyenne des critères à la barre au moins, et aucun
/// critère à plus d'un point sous elle. Ce que l'utilisateur appelle « 8/10 » ;
/// chaque critère à 8 exactement, un juge gratuit ne l'accorde presque jamais,
/// et le tour s'épuisait sur un seul critère à 7.
#[must_use]
pub fn design_reached(scores: &[(String, u8)], bar: u8) -> bool {
    if scores.is_empty() {
        return false;
    }
    let total: u32 = scores.iter().map(|(_, score)| u32::from(*score)).sum();
    #[allow(clippy::cast_precision_loss)]
    let mean = total as f32 / scores.len() as f32;
    mean >= f32::from(bar) && scores.iter().all(|(_, score)| score + 1 >= bar)
}

/// Ajout à la consigne du juge quand la note stagne sous la barre : des
/// changements de structure, pas des retouches. Vu le 26 octobre 2026
/// (session #32, passage 5) : 6,4 puis 6,6 puis 6,6, huit retouches à chaque
/// revue (une marge, un cœur en SVG) et un formulaire resté en cadres gris.
#[must_use]
pub fn restructure_note(trail: &str, bar: u8) -> String {
    format!("\n\nThe page is stuck under the bar ({trail}/10, bar {bar}) after reviews of small changes: small fixes will not raise it. This time give at most 4 changes, each a change of structure the developer can make in one go: what the first screen shows and in what order, how blocks are grouped and sized, a list turned into compact rows or a grid into a list, a different composition for a section, a stronger single idea for the identity. No padding, color value or radius alone.")
}

/// Note de fonctionnement sur 10, comptée sur ce que la revue a mesuré en
/// utilisant la page (pas devinée par un modèle) : 10 quand rien n'est cassé.
/// Une erreur du script, un formulaire principal qui ne marche pas, ou ce que
/// les autres doivent voir et ne voient pas plafonnent la note à 5 ; chaque
/// autre défaut retire un ou deux points. Vu le 26 octobre 2026 (session
/// #32) : une page notée 6,6 en design dont la liste de la boulangère plantait
/// et restait vide sur son téléphone, sans qu'aucune note ne le dise.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunctionScore {
    pub score: u8,
    /// Ce qui retire des points, en français pour l'utilisateur.
    pub reasons: Vec<String>,
}

/// Défauts de fonctionnement : motif dans le défaut mesuré, points retirés,
/// plafond de la note (10 : aucun), et ce qu'en lit l'utilisateur.
const FUNCTION_FAULTS: &[(&str, u8, u8, &str)] = &[
    ("an error in the page's script", 3, 5, "erreur du script"),
    (
        "raises an error in the page's script",
        3,
        5,
        "erreur du script",
    ),
    (
        "On a second device",
        3,
        5,
        "ce qu'envoie un visiteur ne se voit pas sur un autre appareil",
    ),
    ("The page shows nothing", 9, 1, "page blanche"),
    ("did not return an HTML page", 9, 1, "page introuvable"),
    // Vu le 27 octobre 2026 (session #35) : l'appli vide de ses montants, le
    // texte tapé noir sur noir, le graphique de l'historique vide ; aucune
    // mesure ne les voyait, la note de fonctionnement non plus.
    (
        "Figures the user gave that the page never shows",
        3,
        5,
        "chiffres de la demande absents de la page",
    ),
    (
        "Fields whose typed text cannot be read",
        2,
        6,
        "texte tapé illisible",
    ),
    ("Charts that show nothing", 2, 6, "graphique vide"),
    (
        "refuses plausible entries",
        2,
        6,
        "formulaire qui refuse des entrées normales",
    ),
    (
        "stays disabled once every field is filled",
        2,
        6,
        "bouton d'envoi qui reste grisé",
    ),
    (
        "does nothing: no submit event",
        2,
        6,
        "formulaire dont l'envoi ne fait rien",
    ),
    (
        "reloads the page (no script takes the submit event",
        2,
        6,
        "formulaire qui recharge la page",
    ),
    (
        "is gone after a reload",
        2,
        10,
        "donnée perdue au rechargement",
    ),
    (
        "after a reload that change is gone",
        2,
        10,
        "donnée perdue au rechargement",
    ),
    (
        "shows nothing: no confirmation",
        1,
        10,
        "envoi sans confirmation",
    ),
    ("is shown nowhere", 1, 10, "nom envoyé affiché nulle part"),
    ("do nothing when clicked", 1, 10, "bouton qui ne fait rien"),
    (
        "is written on the page for every visitor",
        2,
        6,
        "code d'accès lisible par tous les visiteurs",
    ),
    ("broken links", 1, 10, "lien cassé"),
    ("cannot be tapped", 1, 10, "bouton impossible à toucher"),
    ("does not close", 1, 10, "fenêtre qui ne se ferme pas"),
    (
        "closes when a finger slides",
        1,
        10,
        "fenêtre qui se ferme en faisant défiler",
    ),
    (
        "opened the browser's own",
        1,
        10,
        "boîte grise du navigateur",
    ),
    (
        "Buttons or links that were on this page at its first capture",
        1,
        10,
        "bouton disparu",
    ),
];

/// « erreur du script », avec le début du message : l'utilisateur et la
/// relecture du passage voient laquelle. Vu le 27 octobre 2026 (session #34) :
/// cinq revues à « erreur du script » sans que le journal dise laquelle.
fn script_reason(line: &str) -> String {
    let message = line
        .rsplit_once("uncaught: ")
        .or_else(|| line.rsplit_once("error in the page's script: "))
        .map_or(line, |(_, message)| message)
        .trim_start_matches("error: ")
        .trim_start_matches("Uncaught (in promise) ")
        .trim_start_matches("Uncaught ")
        .trim();
    let short: String = message.chars().take(70).collect();
    let more = if message.chars().count() > 70 {
        "…"
    } else {
        ""
    };
    format!("erreur du script : « {short}{more} »")
}

#[must_use]
pub fn function_score(audit: &Audit) -> FunctionScore {
    let mut lost = 0u8;
    let mut cap = 10u8;
    let mut reasons: Vec<String> = Vec::new();
    let mut count = |weight: u8, ceiling: u8, reason: &str| {
        lost = lost.saturating_add(weight);
        cap = cap.min(ceiling);
        if !reasons.iter().any(|seen| seen == reason) {
            reasons.push(reason.to_string());
        }
    };
    let mut scripted = false;
    for problem in &audit.problems {
        for (pattern, weight, ceiling, reason) in FUNCTION_FAULTS {
            if problem.contains(pattern) {
                // Plusieurs boutons morts dans un même défaut : un point chacun, trois au plus.
                let weight = if *pattern == "do nothing when clicked" {
                    problem
                        .split_whitespace()
                        .find_map(|word| word.trim_start_matches('[').parse::<u8>().ok())
                        .unwrap_or(1)
                        .clamp(1, 3)
                } else {
                    *weight
                };
                scripted |= reason.starts_with("erreur du script");
                if reason.starts_with("erreur du script") {
                    count(weight, *ceiling, &script_reason(problem));
                } else {
                    count(weight, *ceiling, reason);
                }
                break;
            }
        }
    }
    if let Some(first) = audit.console.first().filter(|_| !scripted) {
        count(3, 5, &script_reason(first));
    }
    // Le deuxième appareil n'a rien eu à comparer, parce que l'écran du
    // propriétaire ne montre pas ce qui vient d'être envoyé, ou qu'aucun écran
    // ne montre aux autres ce qu'on envoie : la demande partagée ne marche
    // pas. Vu le 27 octobre 2026 (session #32, passage 6) : « fonctionnement
    // 9/10 (partie du site pas atteinte) » pour une boulangère qui ne recevait
    // aucune réservation.
    let missed = audit
        .explored
        .iter()
        .find(|line| line.starts_with("Not reached:"))
        .map_or("", String::as_str);
    if missed.contains("did not show the entry the form had just sent") {
        count(
            3,
            5,
            "l'écran du propriétaire ne montre pas ce qui vient d'être envoyé",
        );
    } else if missed.contains("no screen of the page shows what others sent") {
        count(
            3,
            5,
            "aucun écran ne montre à quelqu'un d'autre ce que les visiteurs envoient",
        );
    } else if !missed.is_empty() {
        // Ce que la revue n'a pas pu ouvrir n'est pas vérifié.
        count(1, 10, "partie du site pas atteinte");
    }
    FunctionScore {
        score: 10u8.saturating_sub(lost).min(cap),
        reasons,
    }
}

/// La suite des notes, pour l'utilisateur : « 6,0 puis 7,3 ».
#[must_use]
pub fn score_trail(graded: &[Graded]) -> String {
    graded
        .iter()
        .map(|graded| format!("{:.1}", graded.mean).replace('.', ","))
        .collect::<Vec<_>>()
        .join(" puis ")
}

/// Ce que l'utilisateur lit quand le tour s'arrête sous la barre.
#[must_use]
pub fn stop_reason(decision: Continuation) -> Option<&'static str> {
    match decision {
        Continuation::Stalled => Some("la dernière revue n'a pas fait monter la note"),
        Continuation::Exhausted => {
            Some("les poursuites permises ont servi (`CLAW_DESIGN_CONTINUE`)")
        }
        Continuation::LowQuota => Some("il reste peu de requêtes gratuites aujourd'hui"),
        Continuation::Continue | Continuation::Unscored | Continuation::Reached => None,
    }
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
    // Vu le 27 octobre 2026 (session #35) : « je veux un appli web perso
    // utilisable sur téléphone » n'était pas reconnu ; seules les notes du
    // moteur, jointes à la demande, faisaient appliquer le guide de design.
    "appli web",
    "appli mobile",
    "application mobile",
    "pwa",
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
    // Une demande dite comme un souhait : « je veux une appli », « j'aimerais un site ».
    "veux",
    "voudrais",
    "aimerais",
    "want",
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

/// Marques d'une demande de corrections : un verbe qui répare, ou des
/// défauts signalés (« il reste », « toujours », « coupé »).
const REPAIR_VERBS: &[&str] = &["corrig", "repar", "remet", "fix", "repair"];
const COMPLAINT_WORDS: &[&str] = &[
    "il reste",
    "toujours",
    "encore",
    "ne marche",
    "marche pas",
    "n'ouvre",
    "n ouvre",
    "disparu",
    "coupe",
    "colle",
    "bug",
    "probleme",
    "still",
    "broken",
    "doesn't work",
    "not working",
    // Ce qu'un utilisateur constate sans nommer de défaut (session #32 : « sa
    // réservation n'apparaît pas dans ma liste », « ça ne me sert à rien »).
    "apparait pas",
    "affiche pas",
    "sert a rien",
    "vois pas",
    "voit pas",
    "trouve pas",
    "fonctionne pas",
    "rien ne se passe",
    "je dois remonter",
    "doesn't show",
    "can't see",
];
/// Marques d'une création ou d'une refonte : celles-là se jugent au design.
const CREATION_WORDS: &[&str] = &[
    "refais",
    "refaire",
    "refonte",
    "redesign",
    "nouveau site",
    "nouvelle page",
    "plus beau",
    "plus belle",
    "plus joli",
    "plus jolie",
    "plus moderne",
    "modernise",
    "embellis",
    "prettier",
    "more beautiful",
];

/// Créer ou construire *un site, une page, une appli* : « créer un compte »
/// n'en est pas une. Vu le 26 octobre 2026 (session #32, passage 5) : « je
/// ne saurai pas créer un compte Firebase » lu comme une création, et le
/// plan de design exigé, d'où trois fichiers réécrits en entier.
fn creates_a_page(folded: &str) -> bool {
    static PATTERN: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    PATTERN
        .get_or_init(|| {
            regex::Regex::new(
                r"\b(?:cree[sz]?|creer|construi[st]?|construire|fai[st]|faire|faudrait|veux|voudrais|aimerais|souhaite|besoin\s+d'|create|build|make|want|need)\s+(?:moi\s+|nous\s+)?(?:un|une|le|la|mon|ma|notre|a|an|the|my|our)?\s*(?:nouveau\s+|nouvelle\s+|petit\s+|petite\s+|new\s+|small\s+)?(?:site|page|appli|application|boutique|landing|website|web\s*app|app)\b",
            )
            .expect("motif de création")
        })
        .is_match(folded)
}

/// Clé ou identifiant à remplacer laissé dans le code (`VOTRE_API_KEY`,
/// `YOUR_PROJECT_ID`, `your-api-key`) : le site attend un compte que
/// l'utilisateur doit créer. Rend le premier trouvé.
#[must_use]
pub fn placeholder_credential(text: &str) -> Option<String> {
    // Une clé laissée vide (`apiKey: ""`) attend le même compte. Vu le 26
    // octobre 2026 (session #32, passage 3) : la configuration de Firebase
    // vidée, et le mode d'emploi pour la créer mis dans la page publique.
    const KEYS: &[&str] = &[
        "apikey",
        "api_key",
        "projectid",
        "databaseurl",
        "authdomain",
        "anon_key",
        "anonkey",
        "supabaseurl",
        "supabase_url",
    ];
    for line in text.lines() {
        let compact: String = line.chars().filter(|c| !c.is_whitespace()).collect();
        let lower = compact.to_ascii_lowercase();
        let empty_after = |at: usize| {
            let rest = lower[at..].trim_start_matches(['"', '\'']);
            rest.strip_prefix([':', '=']).is_some_and(|value| {
                ["\"\"", "''", "``"]
                    .iter()
                    .any(|empty| value.starts_with(empty))
            })
        };
        let found = KEYS.iter().any(|key| {
            lower
                .match_indices(key)
                .any(|(at, _)| empty_after(at + key.len()))
        });
        if found {
            return Some(compact.trim_end_matches([',', ';']).to_string());
        }
    }
    let mut words = text.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '-'));
    words.find_map(|word| {
        let upper = word.to_ascii_uppercase().replace('-', "_");
        let stem = upper
            .strip_prefix("VOTRE_")
            .or_else(|| upper.strip_prefix("YOUR_"))?;
        [
            "API", "KEY", "PROJECT", "PROJET", "SECRET", "TOKEN", "ID", "APP", "DATABASE", "URL",
            "SENDER", "BUCKET", "DOMAIN",
        ]
        .iter()
        .any(|part| stem.contains(part))
        .then(|| word.to_string())
    })
}

/// Message renvoyé au modèle dont le code attend des clés d'un compte que
/// l'utilisateur n'a pas choisi de créer. Vu le 26 octobre 2026 (session #32) :
/// Firebase branché d'office, `VOTRE_API_KEY` dans le code, pour une
/// boulangère « nulle en informatique ».
#[must_use]
pub fn account_setup_note(sample: &str) -> String {
    format!(
        "[Claw] Your code now waits for keys the user must create ({sample}): an account on a service, a project, keys to copy into the code. The user did not choose this and may not be a developer. Before building more on it, ask them with AskUserQuestion: 2 to 4 options in plain words, each saying what it asks of them (an account, the minutes of setup, a cost), with your recommendation first and, if there is one, an option that needs nothing from them. Until they answer, keep what works today working. Never put the setup steps for the site's owner in the page itself: every visitor would see them. The text before your question is your final answer and the last thing they read (the question ends the pass): plain words, no code, no file names or line numbers, no tool names; say what works now, what is not active yet, and what each option would ask of them."
    )
}

/// Plaintes sur l'apparence : elles se jugent au design, même au milieu de
/// corrections. Vu le 26 octobre 2026 (session #32) : le site n'avait jamais
/// été noté (premier passage arrêté, puis des corrections).
const LOOKS_COMPLAINTS: &[&str] = &[
    "triste",
    "moche",
    "pas joli",
    "pas beau",
    "pas belle",
    "fade",
    "terne",
    "austere",
    "ennuyeux",
    "sans vie",
    "ugly",
    "boring",
    "dull",
    "looks bad",
];

/// Le message demande une nouvelle page, une refonte ou un embellissement :
/// ce qui se juge au design, plan compris.
#[must_use]
pub fn asks_new_design(text: &str) -> bool {
    let folded = crate::team::without_accents(&text.to_lowercase()).replace('\u{2019}', "'");
    creates_a_page(&folded) || CREATION_WORDS.iter().any(|word| folded.contains(word))
}

/// Le message demande des corrections sur une interface déjà faite, pas une
/// création ni une refonte. Vu le 26 octobre 2026 (session #31, passage 4) :
/// une liste de sept défauts, réglée pour l'essentiel en 17 minutes, puis 89
/// minutes de revues pour monter une note de design de 4 à 8, jamais
/// atteinte.
#[must_use]
pub fn is_repair_request(text: &str) -> bool {
    let folded = crate::team::without_accents(&text.to_lowercase()).replace('\u{2019}', "'");
    if creates_a_page(&folded)
        || CREATION_WORDS
            .iter()
            .chain(LOOKS_COMPLAINTS)
            .any(|word| folded.contains(word))
    {
        return false;
    }
    REPAIR_VERBS
        .iter()
        .any(|verb| crate::team::has_word_start(&folded, verb))
        || COMPLAINT_WORDS
            .iter()
            .filter(|word| folded.contains(*word))
            .count()
            >= 2
}

/// Des gens qui entrent quelque chose (une réservation, une inscription) et
/// d'autres qui le voient : la demande suppose des données partagées.
const SHARING_PARTIES: &[&str] = &[
    "client",
    "parent",
    "membre",
    "adherent",
    "joueur",
    "eleve",
    "voisin",
    "benevole",
    "invite",
    "visiteur",
    "participant",
    "tout le monde",
    "les autres",
    "customer",
    "member",
    "guest",
    "player",
    "everyone",
];
const SHARING_SEES: &[&str] = &[
    "je vois",
    "je voie",
    "je puisse voir",
    "voient",
    "voir qui",
    "voir la liste",
    "savent",
    "sachent",
    "la liste",
    "tableau",
    " see ",
    "can see",
];
const SHARING_ENTRIES: &[&str] = &[
    "reserv", "inscri", "command", "propos", "conduit", "conduire", "vote", "ajout", "book",
    "sign up", "order", "register",
];

/// La demande fait entrer des choses par les uns et les fait voir par
/// d'autres, chacun sur son téléphone.
#[must_use]
pub fn shared_between_people(text: &str) -> bool {
    let folded = format!(
        " {} ",
        crate::team::without_accents(&text.to_lowercase()).replace('\u{2019}', "'")
    );
    let has = |words: &[&str]| words.iter().any(|word| folded.contains(word));
    has(SHARING_PARTIES) && has(SHARING_SEES) && has(SHARING_ENTRIES)
}

/// Le code garde ce que les visiteurs entrent dans leur propre navigateur, et
/// rien ne l'envoie ailleurs.
#[must_use]
pub fn kept_on_each_device(sources: &[&str]) -> bool {
    let stored = sources.iter().any(|source| {
        source.contains("localStorage.setItem")
            || source.contains("sessionStorage.setItem")
            || source.contains("indexedDB.open")
    });
    let sent = sources.iter().any(|source| {
        [
            "fetch(",
            "XMLHttpRequest",
            "firebase",
            "supabase",
            "WebSocket",
            "navigator.sendBeacon",
            "$.ajax",
            "axios",
        ]
        .iter()
        .any(|call| source.contains(call))
            || sends_a_message(source)
    });
    stored && !sent
}

/// Le code envoie ce que les visiteurs entrent par leur messagerie (SMS,
/// `WhatsApp`, e-mail, texte déjà écrit) : la personne qui doit le voir le
/// reçoit dans ses messages, pas sur un écran du site.
#[must_use]
pub fn sends_by_message(sources: &[&str]) -> bool {
    sources.iter().any(|source| sends_a_message(source))
}

/// Un lien qui ouvre la messagerie du visiteur (`WhatsApp`, SMS, e-mail) avec
/// le texte déjà écrit : ce qu'il entre part chez le destinataire sans
/// serveur ni compte à créer. Un simple lien de contact (`mailto:` sans
/// texte) n'envoie rien de ce qu'il a entré.
fn sends_a_message(source: &str) -> bool {
    let whatsapp = (source.contains("wa.me/") || source.contains("api.whatsapp.com/send"))
        && source.contains("text=");
    let written =
        (source.contains("mailto:") || source.contains("sms:")) && source.contains("body=");
    whatsapp || written
}

/// Texte que montrent les pages HTML du projet : sans balises, scripts ni styles.
#[must_use]
pub fn shown_text(root: &Path) -> String {
    let tags = Regex::new(r"(?is)<script\b.*?</script>|<style\b.*?</style>|<!--.*?-->|<[^>]+>")
        .expect("regex");
    page_texts(root)
        .iter()
        .filter(|text| {
            let lower = text.to_ascii_lowercase();
            lower.contains("<body") || lower.contains("<html")
        })
        .map(|text| {
            tags.replace_all(text, "\n")
                .replace("&nbsp;", " ")
                .replace("&#39;", "'")
                .replace("&amp;", "&")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Heures et durées d'un texte, écrites d'une seule façon (« 19 h », « 19h00 »
/// et « 19:00 » donnent « 19h »), et les canaux promis (« sms », « e-mail »).
fn facts_of(text: &str) -> Vec<String> {
    let folded = crate::team::without_accents(&text.to_lowercase()).replace('\u{2019}', "'");
    let pattern = Regex::new(
        r"(\d{1,2})\s?(?:h|:)\s?(\d{2})?\b|(\d+)\s?(€|euros?\b|%|jours?\b|heures?\b|minutes?\b|min\b)|\b(sms|e-?mail|courriel|livraison)\b",
    )
    .expect("regex");
    pattern
        .captures_iter(&folded)
        .map(|found| {
            if let Some(hour) = found.get(1) {
                match found.get(2).map(|minutes| minutes.as_str()) {
                    None | Some("00") => format!("{}h", hour.as_str()),
                    Some(minutes) => format!("{}h{minutes}", hour.as_str()),
                }
            } else if let Some(amount) = found.get(3) {
                let unit = found.get(4).map_or("", |unit| unit.as_str());
                format!("{} {}", amount.as_str(), unit.trim_end_matches('s'))
            } else {
                found
                    .get(5)
                    .map_or_else(String::new, |word| word.as_str().replace('-', ""))
            }
        })
        .collect()
}

/// Phrases de la page qui posent une règle ou une promesse (un délai, une
/// limite, une confirmation par SMS) avec une heure, une durée ou un canal que
/// l'utilisateur n'a jamais donnés. Vu le 26 octobre 2026 (session #32) :
/// « Annulation possible jusqu'à 19h la veille » et « Vous recevrez un SMS »,
/// inventés pour une boulangère qui n'envoie pas de SMS, gardés trois passages.
#[must_use]
pub fn invented_rules(page: &str, user: &str) -> Vec<String> {
    let rule = Regex::new(
        r"(?i)annul|jusqu'?\s?(?:a|à)\b|au plus tard|d[ée]lai|minimum|maximum|garanti|rembours|recevrez|confirmation par|par sms|par e-?mail|livr|deadline|at the latest|refund|you will receive",
    )
    .expect("regex");
    let given = facts_of(user);
    let mut found = Vec::new();
    for sentence in page.split(['.', '!', '?', '\n']) {
        let sentence = sentence.split_whitespace().collect::<Vec<_>>().join(" ");
        if sentence.len() < 8 || !rule.is_match(&sentence) {
            continue;
        }
        let facts = facts_of(&sentence);
        if facts.iter().any(|fact| !given.contains(fact)) && !found.contains(&sentence) {
            found.push(sentence.chars().take(140).collect());
        }
        if found.len() == 3 {
            break;
        }
    }
    found
}

/// Message renvoyé au modèle : des règles que l'utilisateur n'a pas données.
#[must_use]
pub fn invented_rules_note(sentences: &[String]) -> String {
    format!(
        "[Claw] The page states rules or promises the user never gave: {}. A rule written on a business's page (a deadline, a limit, a confirmation by SMS or e-mail) binds the user in front of their customers, and may be false. Remove these sentences from the page; if such a rule could help, ask the user in your answer whether they want it, and with which hour or limit.",
        sentences
            .iter()
            .map(|sentence| format!("« {sentence} »"))
            .collect::<Vec<_>>()
            .join(", ")
    )
}

/// Message renvoyé au modèle : la page s'ouvre avec un code que la réponse
/// ne donne pas. Vu le 26 octobre 2026 (session #32, passage 5) : « Espace
/// boulanger → code », et le code 1234 écrit dans app.js, jamais dit.
#[must_use]
pub fn access_code_note(codes: &[String]) -> String {
    format!(
        "[Claw] The page opens a reserved space with the code \"{}\", written in its script, and your answer never gives it: the user cannot get in. Say in your answer, in plain words, the code, where to type it and how to choose their own; and in one line, that this code keeps out the curious but not someone who reads the page's code. Give it in the answer only: never write it on the page (no hint, no example in the field), where every visitor would read it.",
        codes[0]
    )
}

/// Message renvoyé au modèle : la demande suppose un partage, le site garde
/// tout dans chaque téléphone. Vu le 26 octobre 2026 (session #32) : les
/// clientes réservaient dans leur navigateur, la boulangère n'en aurait
/// jamais rien vu ; le pied de page le disait en petit, la réponse non.
pub const SHARED_DATA_NOTE: &str = "[Claw] The user asked for something several people share: some enter it (a reservation, a sign-up, an offer) and someone else sees it on another phone. Your page keeps what a visitor enters in that visitor's own browser (localStorage), so nobody else ever sees it: as built, the request does not work, even if every check passes on one device. Do not hide this. Either make the data shared, or say it plainly at the top of your final answer: what works on one phone, what does not work between phones, and what is needed to make it work. Ways to share it: a small server of the project; a free hosted store the user sets up; or, when the user does not want to set anything up, send each entry to the person who must see it through the visitor's own messaging app: the form opens WhatsApp (https://wa.me/<number>?text=...), SMS (sms:<number>?body=...) or email (mailto:<address>?subject=...&body=...) with the entry already written in (everything that person needs to act on it: the visitor's name and phone number, what, which day, what time), and the visitor only taps send; nothing to create or configure, only that person's number or address is needed (ask for it if you do not have it, never invent one). Ask the user with AskUserQuestion which way they accept, in plain words, without technical names. Never send the entries to a service the user has not chosen (a free public store found online): whoever finds its address reads the customers' names and numbers. Keeping the data private means nobody else can read it, not hiding it from the owner: on the owner's own screen, behind their code, show the full names and phone numbers they need to call a customer back.";

/// Adresses que le code nomme sans y envoyer ce que les visiteurs entrent :
/// polices, bibliothèques, images, cartes, réseaux, messageries.
const NOT_DATA_HOSTS: &[&str] = &[
    "fonts.googleapis.com",
    "fonts.gstatic.com",
    "jsdelivr.net",
    "unpkg.com",
    "cdnjs.cloudflare.com",
    "jquery.com",
    "tailwindcss.com",
    "w3.org",
    "schema.org",
    "wa.me",
    "whatsapp.com",
    "google.com",
    "openstreetmap.org",
    "unsplash.com",
    "wikimedia.org",
    "wikipedia.org",
    "youtube.com",
    "vimeo.com",
    "github.com",
    "githubusercontent.com",
    "localhost",
    "example.com",
    "example.org",
    "instagram.com",
    "facebook.com",
];

/// Services d'un tiers à qui le code envoie des données (POST, PUT, PATCH,
/// `sendBeacon`), que `after` nomme et `before` non : un service ajouté par le
/// tour. Les adresses de liens et d'images (`href`, `src`), de fichiers
/// (`.css`, `.png`…) et de polices ou bibliothèques n'en sont pas.
#[must_use]
pub fn new_data_services(before: &str, after: &str) -> Vec<String> {
    let writes = Regex::new(
        r#"(?i)method\s*:\s*['"`](?:post|put|patch)\b|\.(?:post|put|patch)\s*\(|sendBeacon\s*\(|\.open\s*\(\s*['"`](?:post|put|patch)\b"#,
    )
    .expect("regex");
    if !writes.is_match(after) {
        return Vec::new();
    }
    let urls =
        Regex::new(r#"https?://([a-z0-9-]+(?:\.[a-z0-9-]+)+)(/[^\s'"`)<>]*)?"#).expect("regex");
    let asset = Regex::new(
        r"\.(css|m?js|png|jpe?g|gif|svg|webp|avif|ico|woff2?|ttf|otf|mp4|webm|pdf)(\?|#|$)",
    )
    .expect("regex");
    let before = before.to_lowercase();
    let after = after.to_lowercase();
    let mut hosts: Vec<String> = Vec::new();
    for found in urls.captures_iter(&after) {
        let whole = found.get(0).expect("adresse");
        let host = found[1].to_string();
        let path = found.get(2).map_or("", |path| path.as_str());
        let lead = after[..whole.start()].trim_end_matches(['"', '\'']);
        let linked = ["href=", "src=", "srcset=", "url(", "content="]
            .iter()
            .any(|attribute| lead.ends_with(attribute));
        let known = NOT_DATA_HOSTS
            .iter()
            .any(|name| host == *name || host.ends_with(&format!(".{name}")));
        if linked
            || known
            || asset.is_match(path)
            || before.contains(&host)
            || hosts.contains(&host)
        {
            continue;
        }
        hosts.push(host);
    }
    hosts
}

/// L'utilisateur a nommé ce service (« mantledb », « firebase ») dans ses
/// messages : il l'a choisi.
#[must_use]
pub fn named_by_user(host: &str, words: &str) -> bool {
    host.split('.')
        .rev()
        .nth(1)
        .filter(|name| name.len() >= 4)
        .is_some_and(|name| words.contains(name))
}

/// Message renvoyé au modèle : le code envoie ce que les visiteurs entrent à
/// un service que l'utilisateur n'a pas choisi.
#[must_use]
pub fn data_service_note(hosts: &[String]) -> String {
    format!(
        "[Claw] Your code sends what visitors enter to {}, a service run by someone else that the user never chose. Whoever finds that address can read, change or erase what is there (names, phone numbers: personal data the user answers for), the service can stop or wipe it without notice, and the user did not agree. Do not ship it this way. Ask the user first with AskUserQuestion, in plain words, without technical or service names, which way they accept: each entry sent straight to the person who must see it through the visitor's own messaging app (WhatsApp, SMS or email, already written in; nothing to set up, only that person's number or address); an account the user creates for a store of their own (a few minutes, you guide them step by step); or a small server of the project, with somewhere to host it. Until they answer, take that service out of the code and say so in your answer. Private means nobody else can read the data, not hiding it from the owner: keep full names and phone numbers on the owner's own screen.",
        hosts.join(", ")
    )
}

/// Photos que l'outil `Photo` a enregistrées dans le projet, d'après les
/// crédits qu'il tient (`images/CREDITS.md` : « - `images/pain-1.jpg`: … »).
#[must_use]
pub fn credited_photos(root: &Path) -> Vec<String> {
    let Ok(credits) = std::fs::read_to_string(root.join("images").join("CREDITS.md")) else {
        return Vec::new();
    };
    credits
        .lines()
        .filter_map(|line| line.trim_start().strip_prefix("- `"))
        .filter_map(|rest| rest.split('`').next())
        .filter(|path| !path.is_empty())
        .map(ToString::to_string)
        .collect()
}

/// Textes des pages et des styles du projet (HTML, CSS, scripts, gabarits),
/// hors dépendances : ce qui peut nommer une image.
#[must_use]
pub fn page_texts(root: &Path) -> Vec<String> {
    texts_of(root, PAGE_KINDS)
}

/// Pages et scripts du projet (sans styles, notes ni données) : ce qui garde
/// ou envoie ce que les visiteurs entrent.
#[must_use]
pub fn page_scripts(root: &Path) -> Vec<String> {
    texts_of(
        root,
        &[
            "html", "htm", "js", "mjs", "jsx", "ts", "tsx", "vue", "svelte", "astro",
        ],
    )
}

const PAGE_KINDS: &[&str] = &[
    "html",
    "htm",
    "css",
    "scss",
    "js",
    "mjs",
    "jsx",
    "ts",
    "tsx",
    "vue",
    "svelte",
    "astro",
    "md",
    "json",
    "webmanifest",
];

fn texts_of(root: &Path, kinds: &[&str]) -> Vec<String> {
    let mut texts = Vec::new();
    let mut folders = vec![(root.to_path_buf(), 0)];
    while let Some((folder, depth)) = folders.pop() {
        let Ok(entries) = std::fs::read_dir(&folder) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            if path.is_dir() {
                if depth < 4
                    && !name.starts_with('.')
                    && !matches!(name.as_str(), "node_modules" | "dist" | "build" | "target")
                {
                    folders.push((path, depth + 1));
                }
            } else if name != "CREDITS.md"
                && path.extension().is_some_and(|ext| {
                    kinds.contains(&ext.to_string_lossy().to_ascii_lowercase().as_str())
                })
            {
                if let Ok(text) = std::fs::read_to_string(&path) {
                    texts.push(text);
                }
            }
            if texts.len() > 400 {
                return texts;
            }
        }
    }
    texts
}

/// Méthode jointe à une demande d'interface (aussi à un message court qui
/// poursuit une telle demande).
#[must_use]
pub fn design_guide(request: &str, earlier: Option<&str>) -> Option<String> {
    let interface = is_interface_request(request)
        || (request.split_whitespace().count() <= 12 && earlier.is_some_and(is_interface_request));
    interface.then(|| format!("{GUIDE_PREFIX} {}", design_method(design_bar())))
}

/// Socle de code à recopier au début de la feuille de styles : il évite les
/// défauts qu'un petit modèle fait le plus souvent, pour que les revues
/// portent sur le design. Vu dans les essais réels : lien Google Fonts mal
/// formé (la page rendue en Times et Arial), débordement d'un élément de
/// grille resté à `min-width: auto`, aucun état `:hover` (finition notée
/// 5/10), mouvement que rien n'arrête, section vide sur ordinateur.
const FOUNDATION: &str = r#"Start the page with this foundation, copied as it is (rename the custom properties to your plan's tokens and give them its values): it prevents the defects free models make most often, so that the reviews can be about design.
In <head>, the fonts link exactly in this form (the "?family=" matters; wrong forms load nothing and the page falls back to Times and Arial):
<link rel="preconnect" href="https://fonts.googleapis.com"><link rel="preconnect" href="https://fonts.gstatic.com" crossorigin>
<link href="https://fonts.googleapis.com/css2?family=Display+Name:wght@500;700&family=Text+Name:wght@400;600&display=swap" rel="stylesheet">
At the top of the stylesheet:
:root { --paper: #f7f3ec; --ink: #1f1d1a; --muted: #5f5a52; --accent: #8a3b12; --error: #b42318; --font-display: "Display Name", serif; --font-text: "Text Name", sans-serif; --gutter: 1.25rem; --header-h: 4rem; }
*, *::before, *::after { box-sizing: border-box; }
html { scroll-padding-top: var(--header-h); }
body { margin: 0; font-family: var(--font-text); font-size: 1.0625rem; line-height: 1.6; color: var(--ink); background: var(--paper); }
img, svg, video { max-width: 100%; height: auto; }
.container { width: min(100% - 2 * var(--gutter), 72rem); margin-inline: auto; }
.grid > *, .split > *, .row > * { min-width: 0; }
input, select, textarea, button { font: inherit; color: inherit; }
fieldset { border: 0; padding: 0; margin: 0; min-width: 0; } legend { padding: 0; }
a, button, input, select, textarea, .card { transition: color .2s, background-color .2s, border-color .2s, box-shadow .2s, transform .2s; }
a:hover { color: var(--accent); }
button:hover, .button:hover { transform: translateY(-1px); }
button:active, .button:active { transform: translateY(1px); }
:focus-visible { outline: 2px solid var(--accent); outline-offset: 3px; }
input:user-invalid, select:user-invalid, textarea:user-invalid { border-color: var(--error); }
[hidden] { display: none !important; }
@media (prefers-reduced-motion: reduce) { *, *::before, *::after { animation-duration: .01ms !important; animation-iteration-count: 1 !important; transition-duration: .01ms !important; scroll-behavior: auto !important; } }
Give every grid or flex container the class grid, split or row (or set min-width: 0 on its children), and every interactive element its own hover style on top of these. With a fixed header, add body { padding-top: var(--header-h); }. When a media query hides one version of a content (a schedule as a list on phones), the same media query shows the other version (a table on desktops): no section is ever empty on one screen. Never give an element a width in vw or in px wider than a phone (100vw, width: 480px): use max-width."#;

/// Pièces du socle (`FOUNDATION`) absentes de toutes les feuilles de style du
/// projet : ce qui manque, en mots que le modèle reconnaît. Vu dans un essai
/// réel : le socle était dans la méthode, mais le modèle a écrit son CSS sans
/// lui, et la revue a trouvé 12 liens sur 12 sans survol et un mouvement que
/// rien n'arrêtait (finition 4/10).
#[must_use]
pub fn foundation_gaps(css: &str) -> Vec<&'static str> {
    let compact: String = css
        .to_ascii_lowercase()
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .collect();
    let pieces: [(&str, &[&str]); 6] = [
        (":hover states on links and buttons", &[":hover"]),
        (":focus-visible styles", &[":focus-visible"]),
        (
            "the prefers-reduced-motion block",
            &["prefers-reduced-motion"],
        ),
        ("box-sizing: border-box", &["box-sizing:border-box"]),
        ("min-width: 0 on grid and flex children", &["min-width:0"]),
        (
            "font: inherit on form controls",
            &["font:inherit", "font-family:inherit"],
        ),
    ];
    pieces
        .iter()
        .filter(|(_, needles)| !needles.iter().any(|needle| compact.contains(needle)))
        .map(|(label, _)| *label)
        .collect()
}

/// Ce qui est dit au modèle quand ses feuilles de style n'ont pas le socle.
#[must_use]
pub fn foundation_feedback(gaps: &[&str]) -> String {
    format!(
        "[Claw] The page's stylesheets lack parts of the foundation the method gives: {}. Add the foundation block from the method at the top of the stylesheet now (it is short and prevents the defects the review would send back), then go on.",
        gaps.join(", ")
    )
}

/// Feuilles de style du projet, lues ensemble (40 au plus, hors
/// `node_modules` et dossiers cachés) : le socle peut être dans une autre.
#[must_use]
pub fn project_css(root: &Path) -> String {
    fn walk(dir: &Path, depth: usize, out: &mut Vec<std::path::PathBuf>) {
        if depth > 4 || out.len() >= 40 {
            return;
        }
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with('.') || name == "node_modules" || name == "target" {
                continue;
            }
            if path.is_dir() {
                walk(&path, depth + 1, out);
            } else if path.extension().is_some_and(|ext| {
                ext.eq_ignore_ascii_case("css") || ext.eq_ignore_ascii_case("scss")
            }) {
                out.push(path);
            }
        }
    }
    let mut files = Vec::new();
    walk(root, 0, &mut files);
    files
        .iter()
        .filter_map(|path| std::fs::read_to_string(path).ok())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Méthode d'un studio exigeant : des cibles chiffrées plutôt que des
/// conseils, les marques d'une page générée interdites, et une note à
/// atteindre. Vu dans les essais réels : avec « change-les sans raison
/// propre au sujet », un modèle gratuit gardait ses badges en capitales et
/// Playfair Display en écrivant qu'il les avait évités.
fn design_method(bar: u8) -> String {
    let with_photos = crate::harness::enabled(crate::harness::Aid::Photos);
    let photos = if with_photos {
        " Pictures of the subject itself (the bread, the studio, the dish, the place) come from the Photo tool: free-licence photos saved in the project with their size and a credit for the footer; name what the photo shows, in English, and look at it in the page before keeping it."
    } else {
        ""
    };
    let photo_pictures = if with_photos {
        ", photos from the Photo tool (width and height attributes, object-fit: cover in their frame, the credits in the footer)"
    } else {
        ""
    };
    format!("This request builds or reshapes a web interface. The bar is a page a top design studio would ship and sign: distinctive, polished and fully working on phones and desktops, never a template. When you finish, Claw has the page scored by an art director, 1 to 10 on seven criteria (phone, hierarchy, spacing, typography, color, identity, finish), and sends back every criterion under {bar}.
1. Brief. In one line each: the subject, the audience, the page's main job. Use real, specific content in the user's language: no lorem ipsum, no \"Welcome to our website\", no stand-ins such as \"FB\" for a social link. Never write on the page a fact the user did not give: a rule (a cancellation deadline, a minimum order, a limit), a promise (a confirmation by SMS or e-mail, a delivery, a guarantee), an address or opening hours. What only the user knows is asked in your answer; example products and prices are said to be examples in your answer, so the user corrects them. The page speaks the visitor's words: no technical word (CSV, JSON, local storage, sync, database, API) unless the audience is developers.
1b. Tool or showcase. When the page's main job is something the visitor does (reserve, order, sign up, fill or follow a list), it is a tool first and the steps below serve that task: the first screen, on phones and desktops, shows the title and the start of the task (the form itself, or a button that leads to it), any picture small beside or under it; the form is one calm card in the page's style, its fields one under the other, each with one visible label (no browser frames around groups, no group title repeating the label); a list to choose from (products, time slots, people) is a list of compact rows, the name and price on the left and the control on the right, 64 to 88px high, never a grid of tall cards for a few words each; the owner's view is a plain list in the order they work, with counts in words (« 1 commande », « 3 commandes »). The identity then comes from the typography, the color and one picture, never from decoration that pushes the task down. A showcase (a shop, a club, a portfolio) follows the steps below as they are.
2. Design plan, written in your reply before any code, with numbers:
- Color: 4 to 6 named hex values taken from the subject's own world (its materials, products, place), one accent covering at most a tenth of the page; every text color checked at 4.5:1 against its background.
- Type: one display face and one text face chosen for this subject (from the Font tool: `search` with the subject's style, then `display` and `text` give the <link> with only the weights used and the CSS of the type scale; paste both as they are), 2 or 3 weights in all; never Inter, Roboto, Arial, Open Sans, Montserrat, Poppins, Lato, Playfair Display, Space Grotesk or system-ui. A modular scale of 5 to 7 sizes with a ratio of at least 1.25: h1 at least 3rem on desktops and 2.25rem on phones, body text 16 to 18px with a line-height of 1.5 to 1.7, lines of 45 to 75 characters.
- Space: one spacing scale on multiples of 8px (for example 8, 16, 24, 40, 64, 96) and nothing else; the same vertical padding for sections of the same rank; no empty band taller than half a screen.
- Layout: an ASCII wireframe of the first screen (phone and desktop), drawn with the Wireframe tool and pasted as it is in a ``` block (never drawn or retouched by hand: hand-drawn boxes come out misaligned and overflow), and the order of the sections. Text left-aligned (center only one-line titles and short captions). Vary the composition from one section to the next (split, full width, grid, list, offset), never the same centered title over a grid each time.
- One memorable element (a hero treatment, an illustration, a typographic idea, an interaction), with everything around it quiet and disciplined.
- Interactions: what the visitor can do on the page, from the Component tool, chosen for this subject: booking for classes, tables or appointments; open-now and directions for a shop, a restaurant or a studio; filter for a schedule, a menu or a catalog; cart for products or a quote; accordion for questions; carousel for a gallery; copy and share for an address or an event; theme-toggle when the brand suits a dark mode. Two or three that serve the visitor, not all of them.
- Motion: one entrance on the first screen, then reveal on a few chosen blocks (a list of cards, a key figure), count-up for key figures, at most one scroll effect (reading progress or a slow parallax on the hero picture), and micro-interactions on every link, button and card.
Forbidden unless the brief asks for it, because they mark a generated page: a plain colored band with centered \"Welcome\" text as the hero; rows of identical cards with the same soft shadow; a short line under every heading; ALL-CAPS badges or small labels above headings; one repeated icon standing in for pictures; cream with terracotta, or near-black with one acid accent; 01/02/03 markers on things that are not steps; a fade-in on every section; decorative gradients everywhere. Check your plan against this list and say in one line that it uses none of them. Emoji are forbidden everywhere, with no exception, even if the brief sounds playful: emoji are drawn by the device's own font and look different on every phone and computer. Every icon and pictogram comes from the Icon tool (polished Lucide line icons, ready-to-paste inline SVG: ask for several names in one call; set their `color`, `style` \"duotone\", `background` badge, `corners` or `rotate` in the call, and ask `sprite` for icons used many times), pasted as it is: never write an icon's shapes yourself, not even a simple arrow, check, cross, star, menu or social logo, and not in a CSS data URL either (a select's arrow is the chevron-down icon); Claw compares every small SVG of the page with the icon set and sends back any drawn by hand; every decorative shape (an organic blob behind a title, layered waves at the edge of a hero, an arch framing a picture, a medallion around an initial, a branch, a sun, a dot, line, contour or grain texture) comes from the Ornament tool in the plan's colors: shapes drawn by hand look crude, and these are drawn from formulas.{photos} An illustration you still draw yourself stays simple and geometric (with width, height and aria-hidden=\"true\", or role=\"img\" and an aria-label when it carries meaning); no emoji appears in any heading, button, label or paragraph.
3. Build. One file per role (index.html, styles.css, app.js, and the manifest and service worker of an app), each short enough to be written in one call (under about 400 lines; split a bigger script into modules): never the whole app packed into one huge HTML file, which a reply cuts before it is written. An app meant to work offline loads nothing from a CDN or another site (draw charts in inline SVG or on a canvas, keep every script and font it needs in the project, and list them in the service worker's cache), and its manifest icon is a real drawing: one SVG file (\"sizes\": \"any\", \"type\": \"image/svg+xml\"), never an empty or 1x1 placeholder PNG. CSS custom properties for every color, size and space of the plan, and no raw values outside them (except 0 and 1px borders). Semantic HTML (header, nav, main, section, footer) with exactly one h1 and no skipped heading levels; mobile first. On phones the navigation folds into a menu button that opens and closes it (aria-expanded), and a fixed header never covers content (offset the page by its height). Every link, button and field has hover, :focus-visible and active states, with transitions of 150 to 250ms, and @media (prefers-reduced-motion: reduce) stops the motion. Animate only transform and opacity (never width, height, margin or top): 150 to 250ms for hover and press, 400 to 700ms for an entrance, with an ease-out curve; nothing loops forever except one slow decorative detail; content never waits for a script to be visible. Tap targets of at least 44x44px on phones. Pictures: the user's images{photo_pictures}, Ornament shapes, or simple illustrations in inline SVG (each with an explicit width and height) or CSS, never broken or placeholder images. Forms: styled like the rest of the page (font: inherit), a visible label on every field, errors shown only after the user acts (:user-invalid, or on submit), a clear confirmation in the page. Watch CSS specificity: a .section rule and a .cta rule can cancel each other's spacing.
{FOUNDATION}
4. Make everything work: every link leads somewhere real (a section that exists, mailto:, tel:), every button does what its label says, forms validate and confirm, no console errors. Take the interactive parts from the Component tool (nav-menu, booking, filter, form, cart, dialog, toast, open-now, directions...; several names in one call) and paste them as they are, then adapt their texts and colors: they work, they are accessible, and their icons come from the icon set. During the review Claw walks the page like a visitor: it clicks every button, follows every link, fills and sends every form (those in windows too), closes every window with its own button and reloads the page, then reports what does nothing, throws an error, leads nowhere, cannot be tapped or is lost on reload.
5. Check and critique before you answer (a static page needs no server: give Screenshot and ViewPage the path of index.html, relative to the working directory, and start no server in the background): Screenshot with full_page true at phone and desktop sizes; then Browser, to use the page yourself like a visitor and see it as text (each thing on the screen with its size, font, colors and place, what each action changed, script errors, measured problems): go through the visitor's whole task on a phone in ONE call, with action \"steps\" and the list of steps (open, fill the form, send it empty then filled, open the menu, every window and every screen, with its code if it has one), each step with \"expect\" set to what must then show (a total, a name, a message) so the call proves the result, not only that buttons react; then resize to a desktop and look again; once the page works, Screenshot with windows and explore true walks it from top to bottom (every screen a button opens, with its code and with data, every window, every page of the site) and says what it could not reach; ViewPage for console errors. Say in your answer only what these checks saw. Fix every measured problem and every design note. Then score the screenshots yourself, 1 to 10 on the seven criteria, as a strict art director would (most generated pages deserve 4 to 6; an 8 must be earned by something you can point to), and improve every criterion under {bar} before you answer. Remove one decoration that does not serve the subject.")
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

/// Symboles de texte courants, qui ne sont pas des emoji : flèches, puces,
/// étoiles de notation, coches, signes légaux.
fn is_text_symbol(ch: char) -> bool {
    matches!(ch,
        '\u{2190}'..='\u{21FF}'
            | '\u{2022}' | '\u{2023}' | '\u{25E6}' | '\u{2043}' | '\u{2219}'
            | '\u{2605}' | '\u{2606}' | '\u{2713}'..='\u{2716}'
            | '\u{A9}' | '\u{AE}' | '\u{2122}' | '\u{2026}' | '\u{B7}' | '\u{2013}' | '\u{2014}'
            | '\u{2212}' | '\u{00D7}' | '\u{00B0}' | '\u{20AC}' | '\u{2039}' | '\u{203A}'
            | '\u{AB}' | '\u{BB}' | '\u{2018}'..='\u{201F}')
}

/// Un emoji : pictogramme des blocs emoji d'Unicode (visages, objets,
/// nourriture, symboles divers, drapeaux), hors symboles de texte.
fn is_emoji(ch: char) -> bool {
    if is_text_symbol(ch) {
        return false;
    }
    matches!(ch,
        '\u{1F300}'..='\u{1FAFF}' // pictogrammes, émoticônes, transports, symboles étendus
            | '\u{1F000}'..='\u{1F2FF}' // mahjong, dominos, cartes, pictogrammes clos
            | '\u{2600}'..='\u{27BF}' // symboles divers et dingbats
            | '\u{2B00}'..='\u{2BFF}' // flèches et symboles divers (étoile 2B50)
            | '\u{2300}'..='\u{23FF}') // techniques (sablier, montre, lecture)
        && !matches!(ch, '\u{2605}' | '\u{2606}' | '\u{2713}'..='\u{2716}')
}

/// Emoji écrits dans un fichier d'interface, avec leur ligne : `None` s'il
/// n'y en a pas. Les commentaires qui en parlent comptent aussi : le but est
/// qu'il n'en reste aucun dans ce que la page affiche, et un emoji dans un
/// commentaire finit souvent dans la page à la copie suivante.
#[must_use]
pub fn emoji_in_source(text: &str) -> Option<String> {
    let mut found: Vec<(usize, String)> = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let emoji: String = line.chars().filter(|ch| is_emoji(*ch)).take(4).collect();
        if !emoji.is_empty() {
            found.push((index + 1, emoji));
        }
    }
    if found.is_empty() {
        return None;
    }
    let shown: Vec<String> = found
        .iter()
        .take(6)
        .map(|(line, emoji)| format!("line {line}: {emoji}"))
        .collect();
    let more = found.len().saturating_sub(6);
    Some(format!(
        "{}{}",
        shown.join(", "),
        if more > 0 {
            format!(" and {more} more line(s)")
        } else {
            String::new()
        }
    ))
}

/// Ce qui est dit au modèle quand un fichier d'interface qu'il vient
/// d'écrire contient des emoji.
#[must_use]
pub fn emoji_feedback(path: &str, found: &str) -> String {
    format!(
        "[Claw] `{path}` contains emoji ({found}). Emoji are drawn by the device's own font, look different on every phone and computer, and belong to no design: this page must use none. Replace each one now with an inline SVG drawn for the subject (with width, height and aria-hidden=\"true\", or role=\"img\" and an aria-label when it carries meaning), or remove it from the text."
    )
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

/// Feuilles de style et scripts locaux qu'une page du projet appelle et qui
/// n'existent pas encore. Vu dans un essai réel : la revue visuelle a eu lieu
/// sur `index.html` avant que `styles.css` et `script.js` soient écrits (le
/// modèle avait annoncé la suite sans appeler d'outil), et une page en Times
/// et Arial a été notée 1,9/10 pour rien, une revue sur trois perdue.
#[must_use]
pub fn missing_local_assets(root: &Path, target: &str) -> Vec<String> {
    let Some(("path", page)) = target.split_once(':') else {
        return Vec::new();
    };
    let page = Path::new(page);
    let page = if page.is_absolute() {
        page.to_path_buf()
    } else {
        root.join(page)
    };
    let Ok(html) = std::fs::read_to_string(&page) else {
        return Vec::new();
    };
    let base = page.parent().unwrap_or(root);
    let lower = html.to_ascii_lowercase();
    let mut missing = Vec::new();
    let mut from = 0;
    while let Some(found) = lower[from..].find('<') {
        let start = from + found;
        let end = lower[start..]
            .find('>')
            .map_or(lower.len(), |end| start + end);
        let tag = &lower[start..end];
        from = end.max(start + 1);
        let attribute = if tag.starts_with("<link") && tag.contains("stylesheet") {
            "href"
        } else if tag.starts_with("<script") {
            "src"
        } else {
            continue;
        };
        // La valeur, prise dans le texte d'origine (la casse du chemin compte).
        let Some(value) = attribute_value(&html[start..end], attribute) else {
            continue;
        };
        let value = value.split(['?', '#']).next().unwrap_or_default().trim();
        let remote = value.is_empty()
            || value.starts_with("http:")
            || value.starts_with("https:")
            || value.starts_with("//")
            || value.starts_with("data:");
        if !remote
            && !base.join(value.trim_start_matches('/')).is_file()
            && !missing.contains(&value.to_string())
        {
            missing.push(value.to_string());
        }
    }
    missing
}

/// Valeur d'un attribut dans le texte d'une balise : `href="styles.css"`.
fn attribute_value<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let lower = tag.to_ascii_lowercase();
    let mut from = 0;
    while let Some(found) = lower[from..].find(name) {
        let at = from + found;
        from = at + name.len();
        let before = lower[..at].chars().last();
        if !before.is_some_and(char::is_whitespace) {
            continue;
        }
        let rest = tag[at + name.len()..].trim_start();
        let Some(rest) = rest.strip_prefix('=') else {
            continue;
        };
        let rest = rest.trim_start();
        let (quote, rest) = match rest.chars().next() {
            Some(quote @ ('"' | '\'')) => (Some(quote), &rest[1..]),
            _ => (None, rest),
        };
        let end = match quote {
            Some(quote) => rest.find(quote),
            None => rest.find(|ch: char| ch.is_whitespace() || ch == '>'),
        }
        .unwrap_or(rest.len());
        return Some(&rest[..end]);
    }
    None
}

/// Ce qui est dit au modèle quand sa page appelle des fichiers qui manquent.
#[must_use]
pub fn missing_assets_feedback(page: &str, missing: &[String]) -> String {
    let page = page.split_once(':').map_or(page, |(_, page)| page);
    format!(
        "[Claw] {page} links {}, which do not exist yet: the page cannot be reviewed without them. Write them now (complete, with the plan's styles and the page's behavior), then finish; Claw reviews the page once they exist.",
        missing.join(", ")
    )
}

/// Entrée de l'outil `Screenshot` pour la revue : page entière, téléphone puis ordinateur.
#[must_use]
pub fn screenshot_input(target: &str) -> String {
    // Les fenêtres aussi : le juge et l'utilisateur ne voyaient que la page.
    let mut input =
        json!({ "viewports": ["phone", "desktop"], "full_page": true, "windows": true });
    match target.split_once(':') {
        Some(("url", url)) => input["url"] = json!(url),
        Some(("path", path)) => input["path"] = json!(path),
        _ => input["path"] = json!(target),
    }
    input.to_string()
}

/// Entrée de `Screenshot` pour la revue de début et de fin de tour : la page
/// explorée de fond en comble (écrans cachés, avec leur code et des données ;
/// autres pages du site) et, quand la demande fait voir aux uns ce que les
/// autres entrent, un deuxième appareil. Vu le 26 octobre 2026 (session #32) :
/// la liste de la boulangère, derrière un code, jamais vue par le juge ; vide
/// sur tout autre téléphone que celui qui avait réservé.
#[must_use]
pub fn review_screenshot_input(target: &str, second_device: bool) -> String {
    let mut input: Value = serde_json::from_str(&screenshot_input(target)).unwrap_or_default();
    input["explore"] = json!(true);
    if second_device {
        input["second_device"] = json!(true);
    }
    input.to_string()
}

/// Entrée de `Screenshot` pour la vérification qui suit chaque modification :
/// le téléphone seul, page entière, avec les fenêtres que les boutons ouvrent
/// (leurs mesures : largeur, place, liste coupée). Vu le 26 octobre 2026
/// (session #31) : la fenêtre « Proposer ma voiture », collée à gauche et
/// restée en haut sur l'iPhone du parent, n'était jamais vérifiée après une
/// modification. Une attente courte après chaque chargement (600 ms au lieu
/// de 1,5 s) : sur le site du club, 16 s au lieu de 19, mêmes défauts.
#[must_use]
pub fn quick_screenshot_input(target: &str) -> String {
    let mut input: Value = serde_json::from_str(&screenshot_input(target)).unwrap_or_default();
    input["viewports"] = json!(["phone"]);
    input["wait_ms"] = json!(600);
    input.to_string()
}

/// Mode de couleurs que l'utilisateur a demandé pour la page : `dark` quand il
/// la veut sombre. Les captures et les mesures se font alors dans ce mode (le
/// juge voit la page demandée), et l'autre mode se mesure aussi quand la page
/// suit le réglage du téléphone. Vu le 27 octobre 2026 (session #35) : « Sobre
/// foncé » choisi, chaque capture en mode clair.
#[must_use]
pub fn requested_theme(words: &str) -> Option<&'static str> {
    const DARK: &[&str] = &[
        "sombre",
        "fonce",
        "dark mode",
        "dark theme",
        "theme dark",
        "mode nuit",
        "fond noir",
        "noir et ",
        "tout noir",
    ];
    let text = crate::team::without_accents(&words.to_lowercase());
    DARK.iter()
        .any(|word| text.contains(word))
        .then_some("dark")
}

/// Montants et chiffres que l'utilisateur a donnés et qu'une appli bâtie sur
/// eux doit montrer : les nombres à décimales, et ceux suivis d'une monnaie,
/// assez particuliers pour ne pas se trouver par hasard dans une page (ni 100,
/// ni 450, ni 8 : un compte rond ou un petit nombre). Rien en dessous de deux.
/// Vu le 27 octobre 2026 (session #35) : « salaire = 1235.15 euro, caf = 103 à
/// 206 euro… » ; l'appli s'ouvrait à 0,00 € partout.
#[must_use]
pub fn request_figures(words: &str) -> Vec<String> {
    static NUMBER: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let number = NUMBER.get_or_init(|| {
        regex::Regex::new(r"(\d{1,3}(?:[ \u{a0}\u{202f}]\d{3})+|\d+)(?:[.,](\d{1,2}))?")
            .expect("motif des nombres")
    });
    let mut figures: Vec<String> = Vec::new();
    for found in number.captures_iter(words) {
        let Some(whole) = found.get(0) else { continue };
        let before = words[..whole.start()].chars().next_back();
        let rest = &words[whole.end()..];
        let after = rest.chars().next();
        // Partie d'un autre jeton : version, date, heure, mot, pourcentage.
        if before.is_some_and(|c| c.is_alphanumeric() || matches!(c, '/' | ':' | '.' | ',' | '#'))
            || after.is_some_and(|c| c.is_alphanumeric() && !matches!(c, 'e' | 'E' | '€'))
            || after.is_some_and(|c| matches!(c, '%' | '/' | ':' | '.' | ','))
        {
            continue;
        }
        let integer: String = found[1].chars().filter(char::is_ascii_digit).collect();
        let Ok(units) = integer.parse::<u64>() else {
            continue;
        };
        let decimals = found
            .get(2)
            .map(|cents| cents.as_str().trim_end_matches('0').to_string())
            .unwrap_or_default();
        let near =
            crate::team::without_accents(&rest.chars().take(18).collect::<String>().to_lowercase());
        let money = ["€", "eur", "$", "£", "chf", "dollar", "fcfa", "dh"]
            .iter()
            .any(|unit| near.contains(unit));
        let distinctive = if decimals.is_empty() {
            money && units >= 20 && units % 50 != 0 && !(1900..=2100).contains(&units)
        } else {
            true
        };
        if !distinctive {
            continue;
        }
        let figure = if decimals.is_empty() {
            units.to_string()
        } else {
            format!("{units}.{decimals}")
        };
        if !figures.contains(&figure) && figures.len() < 12 {
            figures.push(figure);
        }
    }
    if figures.len() < 2 {
        figures.clear();
    }
    figures
}

/// L'entrée de `Screenshot` complétée par ce que la demande dit de la page :
/// son mode de couleurs et les chiffres qu'elle doit montrer.
#[must_use]
pub fn with_request(input: &str, theme: Option<&str>, figures: &[String]) -> String {
    let Ok(mut value) = serde_json::from_str::<Value>(input) else {
        return input.to_string();
    };
    if let Some(theme) = theme {
        if value.get("theme").is_none() {
            value["theme"] = json!(theme);
        }
    }
    if !figures.is_empty() {
        value["figures"] = json!(figures);
    }
    value.to_string()
}

/// Ce que l'outil `Screenshot` a mesuré, toutes tailles d'écran confondues.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Audit {
    pub problems: Vec<String>,
    pub notes: Vec<String>,
    pub console: Vec<String>,
    /// Ce que l'exploration a vu, et ce qu'elle n'a pas atteint.
    pub explored: Vec<String>,
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
    for line in value["explored"].as_array().into_iter().flatten() {
        if let Some(line) = line.as_str() {
            push_unique(&mut audit.explored, line.to_string());
        }
    }
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

/// Consigne du modèle qui relit le plan de design avant le code.
pub const PLAN_CRITIC_SYSTEM: &str = "You are the art director of a design studio known for distinctive, polished websites, and you are hard to please. A junior designer shows you the design plan of a page before building it. You judge the direction: will it produce a page your studio would sign? Changing a plan costs nothing; changing built code costs a lot, so be demanding now.";

/// Requête du juge du plan : la direction, avant le code. Vu dans un essai
/// réel : la hiérarchie et l'identité, notées 7/10 une fois la page faite,
/// coûtaient alors une réécriture que le budget du tour ne permettait plus.
#[must_use]
pub fn plan_critique_prompt(request: &str, plan: &str, bar: u8) -> String {
    let pictures = pictures_note();
    let brief: String = request.chars().take(1_500).collect();
    let plan: String = plan.chars().take(5_000).collect();
    format!(
        "The client's brief:\n\"{brief}\"\n\nThe designer's plan, written before any code:\n{plan}\n\nJudge this direction against what a top studio would ship (the built page will be scored 1 to 10 on phone, hierarchy, spacing, typography, color, identity and finish, and must reach {bar} on each):\n1. identity: one memorable idea specific to this subject, or a template in disguise (a centered hero band, identical cards, the same layout in every section, generic stock imagery)?\n2. hierarchy: one clear focal point on the first screen, and an obvious next step?\n3. typography: typefaces chosen for this subject (not Inter, Roboto, Montserrat, Poppins, Playfair Display and the like), a scale with real contrast between levels?\n4. color: a palette taken from the subject's world, readable contrast, an accent used sparingly?\n5. composition: do the sections vary (split, full width, list, offset), with an even rhythm?\n6. interactions: can the visitor do what the brief asks, in the fewest steps, with a few well-chosen ready-made components, and is the motion restrained (one entrance, a few reveals)?\n7. task: when the visitor comes to do something (reserve, order, sign up), does the first screen start that task on phones and desktops, is the form one calm card, and is a list to choose from made of compact rows rather than tall cards?\nYou direct the design of what the client asked for: never add a feature, a page, an access code or password, a rule (opening hours, deadlines, limits) or data the brief does not ask for. Each feature added is work the client did not order and something more that can break.\nReply with at most 5 changes to the plan, the most important first, one per line, each as:\nN. [criterion] what is weak in the plan -> the exact change (a typeface name, hex values, a layout, a concrete idea)\nBe specific to this subject; no generic advice. Small UI icons (phone, clock, menu) come from a polished line icon set (Lucide) on purpose: do not ask to redraw them by hand; the page's identity comes from its illustration, typography, color and composition. {pictures} If the plan would already reach {bar}/10 on every criterion, reply exactly: PLAN OK"
    )
}

/// Ce que le développeur peut obtenir en images, pour que les juges
/// proposent ce qui se fait : des ornements tracés par formules, et des
/// photos libres quand l'outil `Photo` est actif.
fn pictures_note() -> &'static str {
    if crate::harness::enabled(crate::harness::Aid::Photos) {
        "The developer can fetch free-licence photos by keywords and generate clean SVG ornaments (organic blobs, layered waves, arches, medallions, branches, suns, dot, line, contour or grain textures): suggest a photo by what it should show, or an ornament by kind and place, rather than a scene drawn by hand."
    } else {
        "The developer cannot get new photos but can generate clean SVG ornaments (organic blobs, layered waves, arches, medallions, branches, suns, dot, line, contour or grain textures): suggest an ornament by kind and place, or CSS shapes, rather than a scene drawn by hand."
    }
}

/// Changements demandés au plan ; `None` quand le juge l'approuve.
#[must_use]
pub fn plan_changes(text: &str) -> Option<String> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    let lines: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|line| {
            line.chars()
                .next()
                .is_some_and(|first| first.is_ascii_digit())
        })
        .take(5)
        .collect();
    if lines.is_empty() {
        return (!text.to_ascii_uppercase().contains("PLAN OK"))
            .then(|| text.chars().take(1_500).collect());
    }
    Some(lines.join("\n"))
}

/// Le texte du tour contient un plan de design : au moins trois couleurs
/// différentes en hexadécimal à six chiffres (`#c46b3a`), la palette que la
/// méthode demande. Les formes courtes (`#fff`) et les numéros (`#12`) ne
/// comptent pas : un message qui cite un ticket n'est pas un plan.
#[must_use]
pub fn looks_like_design_plan(text: &str) -> bool {
    let bytes = text.as_bytes();
    let mut colors = std::collections::BTreeSet::new();
    for (index, byte) in bytes.iter().enumerate() {
        if *byte != b'#' {
            continue;
        }
        let digits = bytes[index + 1..]
            .iter()
            .take_while(|byte| byte.is_ascii_hexdigit())
            .count();
        if digits == 6 {
            colors.insert(text[index + 1..=index + digits].to_ascii_lowercase());
        }
    }
    colors.len() >= 3
}

/// Message joint au premier fichier d'interface écrit sans plan de design.
pub const PLAN_REQUEST: &str = "[Claw] You started writing the page without the design plan the method asks for. Before going further, write the plan in your reply: the brief, 4 to 6 named hex colors, the two typefaces and the type scale, the spacing scale, an ASCII wireframe of the first screen drawn with the Wireframe tool, the memorable element, and one line saying it uses none of the forbidden marks. Then change in this file only what the plan changes, with edit_file: it is already written, do not send it again whole. An art director reviews the plan before the rest of the page is built.";

/// Message joint au premier fichier d'interface écrit après le plan.
#[must_use]
pub fn plan_feedback(model: &str, changes: &str) -> String {
    format!(
        "[Claw] An art director ({model}) reviewed your design plan before the code:\n{changes}\nApply these changes now: revise the plan in one line each, then build with them: in the file you just wrote with edit_file (only the lines that change), and in the next ones. Leave out any change that adds what the user did not ask for (a feature, a page, an access code, a rule such as a deadline): design advice only."
    )
}

/// Requêtes d'un tour de design, quand l'utilisateur n'a pas réglé la limite
/// lui-même : le travail de page (plan, code, captures, deux ou trois revues)
/// dépassait les 100 requêtes ordinaires, et la deuxième revue n'avait jamais lieu.
pub const DESIGN_TURN_REQUESTS: usize = 160;

/// Budget de requêtes d'un tour : celui d'un tour de design quand la méthode
/// est jointe et que la limite est celle par défaut (`default`) sans réglage
/// de l'utilisateur (`chosen_by_user`), sinon la limite réglée.
#[must_use]
pub fn turn_request_budget(
    design_guided: bool,
    configured: usize,
    default: usize,
    chosen_by_user: bool,
) -> usize {
    if design_guided && configured == default && !chosen_by_user {
        DESIGN_TURN_REQUESTS
    } else {
        configured
    }
}

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
pub fn critique_prompt(
    request: &str,
    images: usize,
    audit: &Audit,
    bar: u8,
    previous: Option<&[(String, u8)]>,
) -> String {
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
    let pictures = pictures_note();
    // Les notes de la revue précédente servent de repère : vu dans un essai
    // réel, le même juge a noté 7,9 puis 6,0 une page qui s'était améliorée.
    let anchor = previous
        .filter(|scores| !scores.is_empty())
        .map_or_else(String::new, |scores| {
            let line = scores
                .iter()
                .map(|(criterion, score)| format!("{criterion}={score}"))
                .collect::<Vec<_>>()
                .join(" ");
            format!("Your previous review of this page, before the developer's last changes, scored: {line}. Keep the same scale: a score moves when something visible changed, up or down; when a score moves by 2 or more, the change lines must name what you see that justifies it. If the scores barely move from that review, your last changes were too small to matter: lead with the change of structure that would move them.\n\n")
        });
    format!(
        "The client's brief:\n\"{brief}\"\n\n{images} screenshots follow, full page: the phone view first, then the desktop view, then what the page shows after a tap, on the phone (its windows, its menu, the other screens its buttons open, its other pages): judge each screen the visitor or the owner uses, not only the first.\n\nAlready measured in the page and sent to the developer (do not repeat these):\n{measured}\n\n{anchor}Score the page from 1 to 10 on each criterion, against what a top studio would ship: 5 is a competent template, 8 is what a strong studio would sign, 10 is award-level. Be strict: most generated pages deserve 4 to 6, and an 8 must be earned by something specific you can point to.\n1. phone: nothing cut, covered or cramped; a menu that fits; comfortable tap targets; text sized for a phone.\n2. hierarchy: a clear first screen with one strong focal point; the eye knows where to go next; headings, text and actions clearly ranked.\n3. spacing: one consistent vertical rhythm, aligned edges, generous but even breathing room, no crowded areas and no empty bands.\n4. typography: typefaces chosen for this subject, a clear scale with real contrast between levels, comfortable line length and leading, nothing default-looking.\n5. color: a harmonious palette tied to the subject, readable contrast, the accent used sparingly and on purpose.\n6. identity: does it look made for this subject, with a memorable idea, or like a template (a plain colored hero band, identical cards, icon placeholders for photos, lines under every heading, ALL-CAPS badges, the same layout in every section)?\n7. finish: craft in the details (alignment, consistent radii and shadows, polished icons and illustrations, states of buttons and fields); anything unstyled, broken, empty or unfinished. Any emoji anywhere in the page caps finish at 4: icons must be inline SVG.\n\nThe first line of your reply is exactly in this form:\nSCORES: phone=N hierarchy=N spacing=N typography=N color=N identity=N finish=N\nThen, for every criterion under {bar}, the changes that would raise it, the most important first, at most 8 in all, one per line, each as:\nN. [criterion] what is wrong, as seen in the screenshots -> the exact change to make (CSS properties and values, HTML structure, copy)\nOrder the changes by how much they would raise the scores: first what changes the structure (what the first screen shows, how blocks are grouped and sized, a list as compact rows or as tall cards, the order of sections), then the details (a padding, a color value, a radius). A review of small tweaks leaves a page with a weak structure where it is: when the structure keeps a criterion under {bar}, the first change says so. When the visitor comes to do something (reserve, order, sign up), the task comes first: judge phone and hierarchy on how soon and how easily it is done (does it start on the first screen, does the form read as one calm card rather than the frames of an admin screen, is a list to choose from made of compact rows), and never ask for decoration that pushes it down. Never ask for a feature, a rule (a deadline, a limit) or a text the brief does not give. Be specific to this page; no generic advice. {pictures} Small UI icons come from a polished line icon set (Lucide) on purpose: judge how they are sized, colored and placed, do not ask to redraw them by hand. If every score is {bar} or more, write NO CHANGES NEEDED after the scores line."
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

/// Avis de plusieurs juges rendus en même temps (mode équipe) réunis en un
/// seul, dans la forme d'un juge : la note de chaque critère est la moyenne
/// des juges (arrondie), et leurs changements alternent, huit au plus,
/// renumérotés. Un juge seul varie d'une revue à l'autre (vu dans un essai
/// réel : 7,9 puis 6,0 pour une page qui s'était améliorée) ; deux lissent.
#[must_use]
pub fn merge_critiques(texts: &[&str]) -> String {
    let scored: Vec<Vec<(String, u8)>> = texts.iter().map(|text| critique_scores(text)).collect();
    let mut line = Vec::new();
    for criterion in CRITERIA {
        let values: Vec<u16> = scored
            .iter()
            .filter_map(|scores| {
                scores
                    .iter()
                    .find(|(name, _)| name == criterion)
                    .map(|(_, score)| u16::from(*score))
            })
            .collect();
        if values.is_empty() {
            continue;
        }
        #[allow(clippy::cast_possible_truncation)]
        let count = values.len() as u16;
        let mean = (values.iter().sum::<u16>() * 2 + count) / (count * 2);
        line.push(format!("{criterion}={mean}"));
    }
    let changes: Vec<Vec<String>> = texts
        .iter()
        .map(|text| {
            text.lines()
                .map(str::trim)
                .filter(|line| {
                    line.chars()
                        .next()
                        .is_some_and(|first| first.is_ascii_digit())
                })
                .filter(|line| !line.to_ascii_uppercase().starts_with("SCORES"))
                .map(|line| {
                    line.trim_start_matches(|ch: char| {
                        ch.is_ascii_digit() || ch == '.' || ch == ')'
                    })
                    .trim()
                    .to_string()
                })
                .collect()
        })
        .collect();
    let mut merged: Vec<String> = Vec::new();
    for rank in 0..8 {
        for list in &changes {
            if let Some(change) = list.get(rank) {
                if merged.len() < 8 && !merged.contains(change) {
                    merged.push(change.clone());
                }
            }
        }
    }
    let satisfied = texts
        .iter()
        .all(|text| text.to_ascii_uppercase().contains("NO CHANGES NEEDED"));
    let mut text = format!("SCORES: {}", line.join(" "));
    if satisfied {
        text.push_str("\nNO CHANGES NEEDED");
    }
    for (number, change) in merged.iter().enumerate() {
        let _ = write!(text, "\n{}. {change}", number + 1);
    }
    text
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

/// La règle d'un défaut mesuré (« [phone 390x844] Text partly cut off by the
/// edge of a box…: span#lieu… »), sans son écran ni ses mesures : le même
/// défaut garde la même clé quand ses chiffres ou ses éléments changent.
#[must_use]
pub fn problem_key(problem: &str) -> String {
    let rule = problem
        .split_once("] ")
        .map_or(problem, |(_, rest)| rest)
        .split([':', '('])
        .next()
        .unwrap_or_default();
    rule.chars()
        .filter(|character| !character.is_ascii_digit())
        .take(80)
        .collect::<String>()
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Les défauts mesurés maintenant dont la règle n'était pas relevée au début
/// du tour (`baseline` : clés de `problem_key`) : le tour les a introduits.
/// Vu le 25 octobre 2026 (site d'un club, huit passages) : chaque passage
/// corrigeait des défauts et en créait d'autres, sans que rien ne le dise ;
/// une réécriture avait coupé l'en-tête, qui allait bien avant.
#[must_use]
pub fn regressions(baseline: &[String], audit: &Audit) -> Vec<String> {
    audit
        .problems
        .iter()
        .filter(|problem| !baseline.contains(&problem_key(problem)))
        .cloned()
        .collect()
}

/// La tête d'une revue quand le tour a cassé ce qui allait : à réparer d'abord.
#[must_use]
pub fn regression_note(regressions: &[String]) -> String {
    format!(
        "[Claw] Regressions: Claw measured the page before this turn's first change, and these problems were not there. Your changes in this turn broke them. Repair them first, without undoing what the user asked for (compare with the files as they were: git diff shows your changes):\n{}",
        regressions
            .iter()
            .map(|problem| format!("- {problem}"))
            .collect::<Vec<_>>()
            .join("\n")
    )
}

/// Ce que Claw a mesuré sur la page sans juge (ni captures pour le modèle) :
/// régressions du tour d'abord, puis défauts et erreurs de la console.
/// `None` quand la mesure ne trouve rien.
#[must_use]
pub fn measured_list(audit: &Audit, broken: &[String]) -> Option<String> {
    let mut lines: Vec<String> = broken
        .iter()
        .map(|problem| {
            format!("- broken by this turn (it was fine when the turn started): {problem}")
        })
        .collect();
    lines.extend(
        audit
            .problems
            .iter()
            .filter(|problem| !broken.contains(problem))
            .map(|problem| format!("- {problem}")),
    );
    lines.extend(
        audit
            .console
            .iter()
            .take(6)
            .map(|line| format!("- console: {line}")),
    );
    (!lines.is_empty()).then(|| lines.join("\n"))
}

/// Ce qu'une modification a changé sur la page : défauts apparus depuis la
/// vérification précédente (`before`, `None` pour la première), défauts
/// disparus, et ceux qui restent.
#[must_use]
pub fn check_delta(before: Option<&[String]>, audit: &Audit) -> (Vec<String>, usize, Vec<String>) {
    let keys = |list: &[String]| {
        list.iter()
            .map(|problem| problem_key(problem))
            .collect::<Vec<_>>()
    };
    let now: Vec<String> = audit
        .problems
        .iter()
        .chain(&audit.console)
        .cloned()
        .collect();
    let Some(before) = before else {
        return (Vec::new(), 0, now);
    };
    let (old, current) = (keys(before), keys(&now));
    // Réparé : seulement ce que la vérification (au téléphone) peut revoir ;
    // un défaut relevé à l'ordinateur au début du tour n'y est jamais mesuré.
    let fixed = before
        .iter()
        .filter(|problem| !problem.starts_with('[') || problem.starts_with("[phone"))
        .map(|problem| problem_key(problem))
        .collect::<std::collections::BTreeSet<_>>()
        .iter()
        .filter(|key| !current.contains(key))
        .count();
    let (new, open): (Vec<String>, Vec<String>) = now
        .into_iter()
        .partition(|problem| !old.contains(&problem_key(problem)));
    (new, fixed, open)
}

/// Message au modèle après la vérification de la page qui suit sa
/// modification de l'interface : ce qu'elle a cassé, réparé, ce qui reste.
#[must_use]
pub fn visual_check_feedback(
    target: &str,
    new: &[String],
    fixed: usize,
    open: &[String],
    with_image: bool,
) -> String {
    let page = target.split_once(':').map_or(target, |(_, page)| page);
    let mut text = format!(
        "[Claw] Claw looked at {page} after your change, rendered in a phone browser (full page, every button tapped, every form sent){}.",
        if with_image { "; the capture is attached" } else { "" }
    );
    if new.is_empty() && open.is_empty() {
        text.push_str(" Nothing measured wrong.");
        return text;
    }
    if !new.is_empty() {
        text.push_str("\nBroken by your last change (it was fine before it), fix it first:\n");
        text.push_str(
            &new.iter()
                .map(|problem| format!("- {problem}"))
                .collect::<Vec<_>>()
                .join("\n"),
        );
    }
    if fixed > 0 {
        let _ = write!(text, "\nYour last change fixed {fixed} problem(s).");
    }
    if !open.is_empty() {
        let more = open.len().saturating_sub(8);
        text.push_str("\nStill wrong on the page:\n");
        text.push_str(
            &open
                .iter()
                .take(8)
                .map(|problem| format!("- {problem}"))
                .collect::<Vec<_>>()
                .join("\n"),
        );
        if more > 0 {
            let _ = write!(text, "\n- and {more} more");
        }
    }
    text
}

/// Remarque à la première version complète d'une page : la confronter à la
/// demande, phrase par phrase, et construire ce qui manque avant toute
/// retouche visuelle ; avec les chiffres que l'utilisateur a donnés.
#[must_use]
pub fn early_requirements_note(request: &str, earlier: Option<&str>, figures: &[String]) -> String {
    let mut words = crate::project::user_words_with_history(request);
    if let Some(earlier) = earlier.map(str::trim).filter(|earlier| !earlier.is_empty()) {
        words = format!("{earlier}\n{words}");
    }
    let words: String = words.chars().take(4_000).collect();
    let mut text = String::from(
        "[Claw] The first complete version of the page is written. Before any visual polish, make sure it does what the user asked: go through their request below, sentence by sentence, and for each one name in one line where the page does it (file and function) or that it is missing. Build what is missing now, data and behaviour first (the user's own figures and rules, what each button does, what is kept after a reload), then go back to the look. Keep this check short: no new file to describe it.",
    );
    if !figures.is_empty() {
        let _ = write!(
            text,
            "\nThe user gave these figures: {}. The app must show them from its first opening, filled in as values the user can change, not ask to type them again.",
            figures.join(", ")
        );
    }
    let _ = write!(text, "\nThe user's words:\n<<<\n{words}\n>>>");
    text
}

/// La vérification après une modification n'a rien vu bouger : une ligne au
/// lieu de la liste entière, et, à la troisième fois de suite, de quoi sortir
/// de la boucle (relire la règle nommée, ou dire pourquoi un défaut n'en est
/// pas un, et passer au reste de la demande).
#[must_use]
pub fn unchanged_check_feedback(target: &str, open: usize, streak: u32) -> String {
    let page = target.split_once(':').map_or(target, |(_, page)| page);
    let mut text = format!(
        "[Claw] Claw looked at {page} after your change: nothing new, nothing fixed; the {open} problem(s) listed before remain."
    );
    if streak == 3 {
        text.push_str(" Your last three changes moved none of them. Step back before editing again: read the rule each problem names (its file and line, and for colors the light or dark mode it was measured in) and fix that rule, not a neighbour. If you are sure a problem is not real, say why in one sentence and go on with the rest of the request: the final review measures the page again.");
    }
    text
}

/// Ce que Claw a vu de la page avant le travail, donné à l'agent avec la
/// demande ; `None` quand la page n'a rien à reprendre. Vu le 27 octobre 2026
/// (session #32, passage 7) : la mesure du début avait trouvé l'erreur qui
/// vidait la liste de la boulangère (« `Cannot set property className of
/// SVGElement` »), mais ne servait qu'à comparer ; l'agent a cherché la cause
/// dans le filtre des dates, puis dans `file://`, et n'a jamais réparé la
/// liste.
#[must_use]
pub fn start_brief(
    target: &str,
    audit: &Audit,
    shared_note: bool,
    services: &[String],
) -> Option<String> {
    let missed = audit
        .explored
        .iter()
        .any(|line| line.starts_with("Not reached:"));
    if audit.is_clean() && !missed && !shared_note && services.is_empty() {
        return None;
    }
    let page = target.split_once(':').map_or(target, |(_, page)| page);
    let mut text = format!(
        "[Claw] Before you change anything: Claw opened {page} in a real browser, at phone and desktop sizes, and used it like a visitor (every button, form and screen it could reach). This is measured, not guessed. As it is now, the page works at {}/10.",
        function_score(audit).score
    );
    if !audit.explored.is_empty() {
        text.push(' ');
        text.push_str(&audit.explored.join(" "));
    }
    let bullets = |lines: &mut dyn Iterator<Item = &String>| {
        lines
            .map(|line| format!("- {line}"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    if !audit.problems.is_empty() {
        text.push_str("\nWhat is wrong now:\n");
        text.push_str(&bullets(&mut audit.problems.iter()));
    }
    if !audit.console.is_empty() {
        text.push_str("\nConsole errors:\n");
        text.push_str(&bullets(&mut audit.console.iter().take(8)));
    }
    text.push_str("\nStart with what the request is about. A script error above is why that screen fails: find the line that throws (search the files for what the message names) and fix it first, before looking for any other cause. To see a screen yourself, use the Browser tool (open the page, click, type, look).");
    if shared_note {
        text.push('\n');
        text.push_str(SHARED_DATA_NOTE);
    }
    // Un service choisi par l'agent à un tour précédent, sans l'utilisateur :
    // la garde de fin de tour ne voit que ce que le tour ajoute.
    if !services.is_empty() {
        text.push('\n');
        text.push_str(&data_service_note(services));
    }
    Some(text)
}

/// Services de tiers à qui les pages du projet envoient des données, que
/// l'utilisateur n'a pas nommés dans ses messages (`words`, en minuscules).
#[must_use]
pub fn unchosen_services(root: &Path, words: &str) -> Vec<String> {
    let mut hosts: Vec<String> = Vec::new();
    for text in page_scripts(root) {
        for host in new_data_services("", &text) {
            if !hosts.contains(&host) && !named_by_user(&host, words) {
                hosts.push(host);
            }
        }
    }
    hosts
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
    let function = function_score(audit);
    if function.score < bar {
        let _ = write!(
            text,
            " It works at {}/10 (bar {bar}), counted on what was measured while using it: fix the problems that break it first (script errors, forms, what others must see, dead buttons), before any design change.",
            function.score
        );
    }
    let bullets = |lines: &mut dyn Iterator<Item = &String>| {
        lines
            .map(|line| format!("- {line}"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    if !audit.explored.is_empty() {
        text.push(' ');
        text.push_str(&audit.explored.join(" "));
    }
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
        page_target, problem_key, read_audit, regression_note, regressions, review_feedback,
        screenshot_input, Audit,
    };

    #[test]
    fn the_figures_of_session_35_are_the_ones_a_page_must_show() {
        let request = "+(gain) -(perte) ~(gain non assuré)\n\nTous les mois je reçois :\n+ salaire = 1235.15 euro\n+ mobili-jeune = 100 euro\n+ caf = 103 à 206 euro\n~ espece = 80 à 200 euro\net mes dépenses impossible à évitées :\n- loyer = 450 euro\n- forfait = 10 euro\n- claude = 21.60 euro\n- transport = 8 euro (normalement 16 mais grace à Orange, il me rembourse 50%)\n- alimentaire = 100 euro\n\nJe veux un appli web perso utilisable sur téléphone.";
        assert_eq!(
            super::request_figures(request),
            vec!["1235.15", "103", "206", "80", "21.6"]
        );
        // Ni dates, ni heures, ni versions, ni pourcentages, ni comptes ronds.
        assert!(super::request_figures(
            "Le 27/10/2026 à 10:30, version 2.5, 50% de remise, 3 fois par semaine, 100 €, 2026 €"
        )
        .is_empty());
        // Un seul chiffre ne suffit pas à conclure.
        assert!(super::request_figures("un pain à 1,20 €").is_empty());
        assert_eq!(
            super::request_figures("pain 1,20 €, croissant 1,10 € et la baguette à 1 135,50 €"),
            vec!["1.2", "1.1", "1135.5"]
        );
    }

    #[test]
    fn a_dark_page_asked_for_is_measured_dark() {
        assert_eq!(
            super::requested_theme("Fonctions : Tout\nStyle : Sobre foncé"),
            Some("dark")
        );
        assert_eq!(
            super::requested_theme("je veux un mode sombre"),
            Some("dark")
        );
        assert_eq!(super::requested_theme("A dark theme please"), Some("dark"));
        assert_eq!(super::requested_theme("Style : Minimaliste chaud"), None);
        let input = super::with_request(
            &screenshot_input("path:index.html"),
            Some("dark"),
            &["1235.15".to_string(), "103".to_string()],
        );
        let value: serde_json::Value = serde_json::from_str(&input).expect("json");
        assert_eq!(value["theme"], "dark");
        assert_eq!(value["figures"], serde_json::json!(["1235.15", "103"]));
        assert_eq!(value["path"], "index.html");
        let plain = super::with_request(&screenshot_input("path:index.html"), None, &[]);
        assert_eq!(plain, screenshot_input("path:index.html"));
    }

    #[test]
    fn an_unchanged_check_is_one_line_and_the_third_one_steps_back() {
        let first = super::unchanged_check_feedback("path:index.html", 6, 1);
        assert!(first.contains("nothing new, nothing fixed") && first.contains("6 problem(s)"));
        assert!(!first.contains("Step back"));
        assert!(super::unchanged_check_feedback("path:index.html", 6, 3).contains("Step back"));
        assert!(!super::unchanged_check_feedback("path:index.html", 6, 4).contains("Step back"));
    }

    #[test]
    fn missing_figures_typed_text_and_empty_charts_count_against_function() {
        let audit = Audit {
            problems: vec![
                "Figures the user gave that the page never shows, not even on its hidden screens: 1235.15, 103".to_string(),
                "In the window opened by button#add: Fields whose typed text cannot be read, its color is too close to the field's background (3:1 at least): input#amount (#000000 on #050505, 1.0:1)".to_string(),
                "In the screen opened by a \"Historique\": Charts that show nothing: canvas#chart is 0x0px".to_string(),
            ],
            ..Audit::default()
        };
        let score = super::function_score(&audit);
        assert!(score.score <= 5, "{score:?}");
        let reasons = score.reasons.join(" ; ");
        assert!(
            reasons.contains("chiffres de la demande absents"),
            "{reasons}"
        );
        assert!(reasons.contains("texte tapé illisible"), "{reasons}");
        assert!(reasons.contains("graphique vide"), "{reasons}");
    }

    #[test]
    fn a_public_store_the_turn_sends_entries_to_is_found() {
        // Session #32, passage 7 (27 octobre 2026) : les réservations des
        // clientes envoyées à MantleDB, trouvé par une recherche.
        let before = "const KEY = 'reservations';\nfunction save(list) { localStorage.setItem(KEY, JSON.stringify(list)); }\n";
        let after = "const MANTLE_URL = 'https://mantledb.sh/v2/boulangerie-3place/reservations';\nconst FONT = 'https://fonts.googleapis.com/css2?family=Fraunces';\nfunction save(entry) {\n  return fetch(MANTLE_URL, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(entry) });\n}\nfooter.innerHTML = '<a href=\"https://www.tripadvisor.fr/boulangerie\">Avis</a><img src=\"https://cdn.example.net/pain.jpg\">';\n";
        assert_eq!(
            super::new_data_services(before, after),
            vec!["mantledb.sh".to_string()]
        );
        // Déjà là avant le tour, ou rien n'y est écrit : rien de nouveau.
        assert!(super::new_data_services(after, after).is_empty());
        assert!(super::new_data_services(
            before,
            "fetch('https://api.open-meteo.com/v1/forecast?latitude=43').then((r) => r.json());"
        )
        .is_empty());
        assert!(super::named_by_user(
            "mantledb.sh",
            "mettez mes réservations sur mantledb"
        ));
        assert!(!super::named_by_user(
            "mantledb.sh",
            "je ne veux rien régler"
        ));
        let note = super::data_service_note(&["mantledb.sh".to_string()]);
        assert!(
            note.contains("sends what visitors enter to mantledb.sh")
                && note.contains("AskUserQuestion")
        );
    }

    #[test]
    fn a_store_chosen_at_an_earlier_turn_is_found_in_the_project() {
        // Session #32, passage 8 (27 octobre 2026) : MantleDB ajouté au tour
        // précédent ; la garde de fin de tour ne voit que ce que le tour ajoute.
        let root = std::env::temp_dir().join(format!("claw-tiers-{}", std::process::id()));
        std::fs::create_dir_all(&root).expect("dossier");
        std::fs::write(
            root.join("app.js"),
            "const BASE = 'https://mantledb.sh/v2/boulangerie/reservations';\nfetch(BASE, { method: 'POST', body: '{}' });\n",
        )
        .expect("app.js");
        let found = super::unchosen_services(&root, "je ne veux rien régler");
        let named = super::unchosen_services(&root, "mettez tout sur mantledb");
        let _ = std::fs::remove_dir_all(&root);
        assert_eq!(found, vec!["mantledb.sh".to_string()]);
        assert!(named.is_empty());
    }

    #[test]
    fn what_the_page_does_wrong_before_the_turn_goes_to_the_agent() {
        // Session #32, passage 7 (27 octobre 2026) : l'erreur qui vidait la
        // liste était mesurée au début du tour, mais l'agent ne l'a jamais lue.
        let audit = Audit {
            problems: vec!["In the screen \"Préparation du jour\" (opened by button.nav-link \"Espace boulanger\", with the code \"1234\"): opening it raises an error in the page's script: uncaught: Uncaught (in promise) TypeError: Cannot set property className of #<SVGElement> which has only a getter".to_string()],
            explored: vec!["Explored like a visitor: the page; the screen \"Préparation du jour\".".to_string()],
            ..Audit::default()
        };
        let brief = super::start_brief(
            "path:index.html",
            &audit,
            true,
            &["mantledb.sh".to_string()],
        )
        .expect("bilan");
        assert!(
            brief.starts_with("[Claw] Before you change anything: Claw opened index.html"),
            "{brief}"
        );
        assert!(brief.contains("works at 5/10"), "{brief}");
        assert!(brief.contains("Cannot set property className"), "{brief}");
        assert!(brief.contains("several people share"), "{brief}");
        // Session #32, passage 8 : MantleDB ajouté au tour précédent.
        assert!(
            brief.contains("Your code sends what visitors enter to mantledb.sh"),
            "{brief}"
        );
        assert!(super::start_brief("path:index.html", &Audit::default(), false, &[]).is_none());
    }

    #[test]
    fn photos_are_read_back_from_their_credits() {
        let root = std::env::temp_dir().join(format!("claw-credits-{}", std::process::id()));
        std::fs::create_dir_all(root.join("images")).expect("dossier");
        std::fs::write(
            root.join("images/CREDITS.md"),
            "# Photo credits\n\nPhotos under free licences…\n\n- `images/baguette-1.jpg`: \"Ajvar\" by X, CC BY-SA 4.0\n- `images/croissant-2.jpg`: \"Croissant\" by Y, CC0\n",
        )
        .expect("crédits");
        let photos = super::credited_photos(&root);
        let _ = std::fs::remove_dir_all(&root);
        assert_eq!(photos, ["images/baguette-1.jpg", "images/croissant-2.jpg"]);
        assert!(super::credited_photos(std::path::Path::new("/nulle/part")).is_empty());
    }

    #[test]
    fn the_texts_that_can_name_an_image_are_read_outside_dependencies() {
        let root = std::env::temp_dir().join(format!("claw-textes-{}", std::process::id()));
        std::fs::create_dir_all(root.join("node_modules/x")).expect("dossiers");
        std::fs::create_dir_all(root.join("images")).expect("dossiers");
        std::fs::write(root.join("index.html"), "<img src=\"images/pain.jpg\">").expect("page");
        std::fs::write(
            root.join("styles.css"),
            ".hero { background: url(images/four.jpg); }",
        )
        .expect("css");
        std::fs::write(root.join("node_modules/x/a.js"), "seigle.jpg").expect("dépendance");
        std::fs::write(root.join("images/CREDITS.md"), "seigle.jpg par X").expect("crédits");
        let texts = super::page_texts(&root).join("\n");
        let _ = std::fs::remove_dir_all(&root);
        assert!(texts.contains("pain.jpg") && texts.contains("four.jpg"));
        assert!(!texts.contains("seigle.jpg"), "{texts}");
    }

    #[test]
    fn data_entered_by_some_and_seen_by_others_is_told_from_a_private_tool() {
        use super::{kept_on_each_device, shared_between_people};
        // Session #32 et #31 : la boulangerie, le club de foot.
        assert!(shared_between_people("j'aimerais un petit site où les clients réservent eux-mêmes depuis leur téléphone, et où moi je vois le matin la liste de ce que je dois préparer"));
        assert!(shared_between_people(
            "les parents savent jamais quand c'est les matchs ni qui conduit"
        ));
        // Un outil pour soi, ou une vitrine : rien de partagé.
        for private in [
            "je note tout sur des bouts de papier, j'aimerais un truc pour noter mes séances et voir si je progresse",
            "un petit truc pour gérer mes sous chaque mois, je note ce que je dépense",
            "un site pour que mes clients puissent voir mes horaires et mes tarifs",
        ] {
            assert!(!shared_between_people(private), "{private}");
        }
        let local = "localStorage.setItem('reservations', JSON.stringify(list));";
        assert!(kept_on_each_device(&[local, "<form id=\"r\"></form>"]));
        assert!(!kept_on_each_device(&[
            local,
            "await fetch('/api/reservations', { method: 'POST' });"
        ]));
        assert!(!kept_on_each_device(&[
            "const x = localStorage.getItem('pin');"
        ]));
        // Session #32, passage 5 : Firebase retiré, plus rien ne partait. Un
        // lien WhatsApp ou SMS au texte déjà écrit envoie la réservation ; un
        // lien de contact du pied de page, non.
        assert!(!kept_on_each_device(&[
            local,
            "location.href = `https://wa.me/33612345678?text=${encodeURIComponent(msg)}`;"
        ]));
        assert!(!kept_on_each_device(&[
            local,
            "a.href = 'sms:0612345678?body=' + encodeURIComponent(msg);"
        ]));
        assert!(kept_on_each_device(&[
            local,
            "<a href=\"mailto:contact@boulangerie.fr\">Écrire</a>"
        ]));
    }

    #[test]
    fn rules_the_user_never_gave_are_found_and_their_own_hours_are_not() {
        use super::invented_rules;
        // Session #32 : la boulangère a donné son adresse et ses horaires.
        let user = "Mon adresse c'est 3 place de l'Église à Saint-Martin, ouvert du mardi au dimanche de 7 h à 13 h. Et je n'envoie pas de SMS.";
        let page = "Réservez votre pain\nOuvert du mardi au dimanche, 7h - 13h\nRetrait jusqu'à 13h00.\nConfirmation par téléphone. Annulation possible jusqu'à 19h la veille.\nCommande minimum 10 €.";
        let found = invented_rules(page, user);
        assert_eq!(
            found,
            [
                "Annulation possible jusqu'à 19h la veille",
                "Commande minimum 10 €"
            ],
            "{found:?}"
        );
        assert!(super::invented_rules(
            "Annulation jusqu'à 19 h la veille.",
            "annulation possible jusqu'à 19h00 la veille"
        )
        .is_empty());
        assert_eq!(
            super::invented_rules(
                "Vous recevrez un SMS de confirmation.",
                "un site pour réserver"
            )
            .len(),
            1
        );
        assert!(super::shown_text(std::path::Path::new("/nonexistent")).is_empty());
    }

    #[test]
    fn keys_left_to_fill_in_are_found() {
        use super::placeholder_credential;
        assert_eq!(
            placeholder_credential("const firebaseConfig = {\n  apiKey: \"VOTRE_API_KEY\",\n  projectId: \"VOTRE_PROJECT_ID\" };").as_deref(),
            Some("VOTRE_API_KEY")
        );
        assert_eq!(
            placeholder_credential("key = 'your-api-key'").as_deref(),
            Some("your-api-key")
        );
        // Session #32, passage 3 : la configuration vidée.
        assert_eq!(
            placeholder_credential(
                "const firebaseConfig = {\n    apiKey: \"\",\n    projectId: \"\" };"
            )
            .as_deref(),
            Some("apiKey:\"\"")
        );
        assert!(placeholder_credential("const SUPABASE_ANON_KEY = '';").is_some());
        assert!(placeholder_credential("{\"apiKey\": \"\"}").is_some());
        // La question clôt le passage : la réponse qui la précède est la dernière lue.
        assert!(super::account_setup_note("apiKey:\"\"").contains("no file names or line numbers"));
        for plain in [
            "const VOTRE_PANIER = [];",
            "Votre commande est prête",
            "YOUR_NAME",
            "apiKey: process.env.API_KEY,",
            "const apiKeyLabel = '';",
        ] {
            assert_eq!(placeholder_credential(plain), None, "{plain}");
        }
    }

    #[test]
    fn a_bug_reported_in_plain_words_is_a_repair() {
        use super::{asks_new_design, is_repair_request};
        // Session #32, passage 2 : aucun verbe de réparation, deux constats.
        let reported = "C'est joli, merci ! Mais j'ai essayé sur mon téléphone et sur celui de ma fille : la réservation qu'elle a faite sur le sien n'apparaît pas dans ma liste du matin. Et quand j'appuie sur « Espace boulanger » j'arrive tout en bas de la page, je dois remonter. Il faut que je voie les commandes de tous mes clients, sinon ça ne me sert à rien.";
        assert!(is_repair_request(reported));
        // Une plainte sur l'apparence se juge au design.
        assert!(!is_repair_request("Je ne saurai pas créer un compte toute seule, il me faut quelque chose où je n'ai rien à régler. Et le site fait un peu triste : il n'y a pas de photo de mon pain, et le titre est caché, je dois remonter."));
        assert!(!asks_new_design(reported));
        assert!(asks_new_design("Refais la page d'accueil, plus moderne"));
        // Session #32, passage 5 : créer un compte n'est pas créer une page.
        assert!(!asks_new_design(
            "Je ne saurai pas créer un compte Firebase toute seule. Et le site fait un peu triste."
        ));
        for creation in [
            "crée-moi un site pour ma boulangerie",
            "il me faudrait un petit site",
            "Build a landing page",
            "fais une appli de budget",
            "Construis une nouvelle page tarifs",
        ] {
            assert!(
                asks_new_design(&creation.replace("crée-moi", "crée moi")),
                "{creation}"
            );
        }
        assert!(!is_repair_request("Ajoute une page pour les tarifs"));
    }

    #[test]
    fn a_list_of_fixes_is_told_from_a_creation_or_a_redesign() {
        // Messages réels de la session #31.
        for repair in [
            "Le texte des places est bien maintenant, merci. Mais sur mon téléphone :\n- Quand j'appuie sur « Je conduis », la fenêtre est collée à gauche.\n- Le bouton « Je propose ma voiture » a disparu.\nCorrige ça, regarde la page après chaque changement.",
            "C'est mieux en haut, merci. Mais sur mon téléphone il reste : les titres ont toujours les mots collés.",
            "the menu is still broken on my phone and the form doesn't work",
        ] {
            assert!(super::is_repair_request(repair), "{repair}");
        }
        // Ce que Claw Web ajoute au message (« created from projet-vierge »)
        // ne compte pas : seuls les mots de l'utilisateur sont lus.
        let with_notes = "Corrige ça, et dis-moi simplement à la fin ce qui est fait.\n\n[Claw] Earlier conversation of this session, for context:\n\nil me faudrait un petit site\n\n[Claw] You are working in the repository o/claw-projet, on the branch claw/session-4 (created from projet-vierge).";
        assert!(super::is_repair_request(crate::project::own_words(
            with_notes
        )));
        // L'historique, lui, contient la demande de création du site.
        assert!(!super::is_repair_request(with_notes));
        for creation in [
            "bonjour, je m'occupe du club de foot des petits de mon quartier. il me faudrait un petit site pour ça, que les parents puissent regarder sur leur tel",
            "Refais la page d'accueil, plus moderne",
            "Corrige les couleurs et rends le site plus beau",
            "Crée un site pour ma boulangerie",
        ] {
            assert!(!super::is_repair_request(creation), "{creation}");
        }
    }

    #[test]
    fn a_dead_button_found_on_desktop_before_the_turn_is_not_new_on_the_phone() {
        // La mesure du début du tour essaie les boutons à l'ordinateur, la
        // vérification après chaque modification au téléphone.
        let before = vec![
            "[desktop 1280x800] 1 button(s) do nothing when clicked: button#details".to_string(),
            "[desktop 1280x800] No visible keyboard focus: nothing changes".to_string(),
            "[phone 390x844] Words that run together: h1".to_string(),
        ];
        let audit = super::Audit {
            problems: vec![
                "[phone 390x844] 1 button(s) do nothing when clicked: button#details".to_string(),
            ],
            ..super::Audit::default()
        };
        let (new, fixed, open) = super::check_delta(Some(&before), &audit);
        assert!(new.is_empty(), "{new:?}");
        // Les mots collés (téléphone) sont réparés ; le focus (ordinateur
        // seul) n'est pas revu, donc pas compté.
        assert_eq!(fixed, 1);
        assert_eq!(open.len(), 1);
    }
    use serde_json::json;

    #[test]
    fn problems_absent_when_the_turn_started_are_named_as_regressions() {
        // Site du club, 25 octobre 2026 : l'en-tête allait bien à 10:50, la
        // réécriture de 12:40 l'a coupé ; le texte rogné, lui, était déjà là.
        let before = Audit {
            problems: vec![
                "[phone 390x844] Text partly cut off by the edge of a box around it (overflow: hidden on a parent): span#hero-location \"Stade Municipal\" shows 0x26px of 121x26px".to_string(),
            ],
            ..Audit::default()
        };
        let baseline: Vec<String> = before
            .problems
            .iter()
            .map(|problem| problem_key(problem))
            .collect();
        let after = Audit {
            problems: vec![
                // Le même défaut, d'autres mesures : pas une régression.
                "[desktop 1280x800] Text partly cut off by the edge of a box around it (overflow: hidden on a parent): span.meta \"Terrain A\" shows 10x20px of 80x20px".to_string(),
                "[phone 390x844] The text of the fixed header.header (60px high) does not fit in it: span.logo-text starts above the top of the screen and is cut".to_string(),
            ],
            ..Audit::default()
        };
        let broken = regressions(&baseline, &after);
        assert_eq!(broken.len(), 1);
        assert!(broken[0].contains("fixed header"), "{broken:?}");
        let note = regression_note(&broken);
        assert!(
            note.starts_with("[Claw] Regressions")
                && note.contains("- [phone 390x844] The text of the fixed header")
        );
        assert_eq!(
            problem_key("[phone 390x844] Icons that cannot be seen, drawn in the color of their background: the icon in div.hero"),
            "icons that cannot be seen, drawn in the color of their background"
        );
    }

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
        // Session #35 (27 octobre 2026).
        assert!(is_interface_request(
            "Je veux un appli web perso utilisable sur téléphone incluant tous les fonctionnalités que j'ai besoin"
        ));
        assert!(is_interface_request(
            "j'aimerais une appli mobile pour mes comptes"
        ));
        assert!(!is_interface_request(
            "je veux un script python qui renomme mes photos"
        ));
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
        // La revue explore ; le deuxième appareil, quand la demande est partagée.
        let review: serde_json::Value =
            serde_json::from_str(&super::review_screenshot_input("path:index.html", true))
                .expect("json");
        assert_eq!(review["explore"], true);
        assert_eq!(review["second_device"], true);
        assert_eq!(review["windows"], true);
        let alone: serde_json::Value =
            serde_json::from_str(&super::review_screenshot_input("path:index.html", false))
                .expect("json");
        assert!(alone.get("second_device").is_none());
        assert!(
            serde_json::from_str::<serde_json::Value>(&screenshot_input("path:index.html"))
                .expect("json")
                .get("explore")
                .is_none()
        );
    }

    #[test]
    fn what_the_exploration_saw_reaches_the_review() {
        let output = json!({
            "captures": [{ "screen": "phone 390x844, screen \"Préparation du jour\" opened by button \"Espace boulanger\"", "problems": ["In the screen \"Préparation du jour\": opening it raises an error in the page's script: TypeError"] }],
            "explored": ["Explored like a visitor: the page; the screen \"Préparation du jour\".", "Not reached: a second device.", "Only what is listed here was seen: do not say in your answer that anything else was checked."]
        })
        .to_string();
        let audit = super::read_audit(&output);
        assert_eq!(audit.explored.len(), 3);
        let feedback =
            super::review_feedback("path:index.html", &audit, None, 1, 2, 8).expect("revue");
        assert!(
            feedback.contains("Explored like a visitor: the page; the screen"),
            "{feedback}"
        );
        assert!(feedback.contains("do not say in your answer that anything else was checked"));
        assert!(feedback.contains("opening it raises an error in the page's script"));
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
    fn two_judges_answering_together_make_one_review() {
        let first = "SCORES: phone=7 hierarchy=6 spacing=7 typography=8 color=7 identity=5 finish=6\n1. [identity] cards everywhere -> one hero photo\n2. [hierarchy] no focal point -> bigger h1";
        let second = "SCORES: phone=8 hierarchy=7 spacing=6 typography=8 color=8 identity=6 finish=7\n1. [identity] cards everywhere -> one hero photo\n2. [spacing] crowded form -> 24px gaps\n3. [finish] grey frames -> remove fieldset borders";
        let merged = super::merge_critiques(&[first, second]);
        assert_eq!(
            merged.lines().next(),
            Some("SCORES: phone=8 hierarchy=7 spacing=7 typography=8 color=8 identity=6 finish=7")
        );
        assert!(merged.contains("1. [identity] cards everywhere -> one hero photo"));
        assert!(merged.contains("2. [hierarchy] no focal point -> bigger h1"));
        assert!(merged.contains("3. [spacing] crowded form -> 24px gaps"));
        assert!(merged.contains("4. [finish] grey frames -> remove fieldset borders"));
        assert!(!merged.contains("NO CHANGES NEEDED"));
        let scores = super::critique_scores(&merged);
        assert_eq!(scores.len(), 7);
        // Les deux satisfaits : la revue l'est.
        let pleased = "SCORES: phone=9 hierarchy=8 spacing=8 typography=9 color=8 identity=8 finish=8\nNO CHANGES NEEDED";
        assert!(super::merge_critiques(&[pleased, pleased]).contains("NO CHANGES NEEDED"));
    }

    #[test]
    fn the_function_score_is_counted_on_what_was_measured() {
        use super::{function_score, Audit};
        let audit = |problems: &[&str], console: &[&str], explored: &[&str]| Audit {
            problems: problems.iter().map(ToString::to_string).collect(),
            notes: Vec::new(),
            console: console.iter().map(ToString::to_string).collect(),
            explored: explored.iter().map(ToString::to_string).collect(),
        };
        assert_eq!(function_score(&audit(&[], &[], &[])).score, 10);
        // Session #32, passage 5 : la liste plantait, la boulangère ne voyait
        // rien sur son téléphone, « 1 COMMANDES ».
        let bakery = function_score(&audit(
            &[
                "[phone 390x844, screen \"Préparation du jour\" opened by button] In the screen \"Préparation du jour\": opening it raises an error in the page's script: TypeError",
                "[second device] On a second device (a fresh browser with its own memory), the screen \"Préparation du jour\" shows none of the entries",
                "[phone 390x844] A count of 0 or 1 followed by a plural: div.stat \"1 COMMANDES\"",
            ],
            &[],
            &[],
        ));
        assert_eq!(bakery.score, 4, "{bakery:?}");
        assert_eq!(
            bakery.reasons,
            [
                "erreur du script : « TypeError »",
                "ce qu'envoie un visiteur ne se voit pas sur un autre appareil"
            ]
        );
        // Session #34 : le message dit laquelle.
        let sms = function_score(&audit(
            &[],
            &["uncaught: ReferenceError: NUMERO_SMS is not defined (at app.js:253)"],
            &[],
        ));
        assert_eq!(
            sms.reasons,
            ["erreur du script : « ReferenceError: NUMERO_SMS is not defined (at app.js:253) »"]
        );
        // Des boutons morts : un point chacun, trois au plus ; une partie du site pas atteinte.
        let minor = function_score(&audit(
            &["[desktop 1280x800] 2 button(s) do nothing when clicked (no change in the page): a; b"],
            &[],
            &["Not reached: the page infos.html (it did not open)."],
        ));
        assert_eq!(minor.score, 7, "{minor:?}");
        // Session #32, passage 6 : rien à comparer sur le deuxième appareil.
        let unseen = function_score(&audit(
            &[],
            &[],
            &["Not reached: a second device (on the first device, the screen \"Préparation du jour\" did not show the entry the form had just sent either, so there was nothing to compare)."],
        ));
        assert_eq!(unseen.score, 5, "{unseen:?}");
        let nowhere = function_score(&audit(
            &[],
            &[],
            &["Not reached: a second device (no screen of the page shows what others sent, so there was nothing to compare)."],
        ));
        assert_eq!(nowhere.score, 5, "{nowhere:?}");
        // Une erreur de la console seule plafonne aussi.
        assert_eq!(
            function_score(&audit(&[], &["error: x is not defined"], &[])).score,
            5
        );
        // Le design atteint : moyenne à la barre, aucun critère sous barre - 1.
        let scores = |values: &[u8]| -> Vec<(String, u8)> {
            values
                .iter()
                .enumerate()
                .map(|(index, value)| (format!("c{index}"), *value))
                .collect()
        };
        assert!(super::design_reached(&scores(&[8, 8, 9, 7, 8, 8, 9]), 8));
        assert!(!super::design_reached(&scores(&[9, 9, 9, 6, 9, 9, 9]), 8));
        assert!(!super::design_reached(&scores(&[8, 8, 7, 7, 8, 8, 7]), 8));
        assert!(!super::design_reached(&[], 8));
        assert!(super::restructure_note("6,4 puis 6,6", 8).contains("change of structure"));
    }

    #[test]
    fn a_design_turn_goes_on_while_its_score_rises_under_the_bar() {
        use super::{continuation, score_trail, stop_reason, Continuation, Graded};
        let graded = |mean: f32, under: &[&str]| Graded {
            mean,
            under: under.iter().map(ToString::to_string).collect(),
            scores: Vec::new(),
        };
        // Aucune revue notée : pas de poursuite.
        assert_eq!(continuation(&[], 0, 0, 2, None), Continuation::Unscored);
        // Une revue sous la barre : ses changements méritent un essai.
        let first = [graded(6.0, &["identity", "finish"])];
        assert_eq!(
            continuation(&first, 0, 0, 2, Some(600)),
            Continuation::Continue
        );
        // Plus de revue depuis la dernière poursuite : rien de neuf.
        assert_eq!(
            continuation(&first, 1, 1, 2, Some(600)),
            Continuation::Unscored
        );
        // La note monte : on continue ; elle stagne : on s'arrête.
        let rising = [graded(6.0, &["identity"]), graded(7.3, &["identity"])];
        assert_eq!(
            continuation(&rising, 1, 1, 2, Some(600)),
            Continuation::Continue
        );
        let flat = [graded(7.3, &["identity"]), graded(7.4, &["identity"])];
        assert_eq!(
            continuation(&flat, 1, 1, 2, Some(600)),
            Continuation::Stalled
        );
        // Session #30 : 4,9 puis 4,1 puis 4,6, remontée sous la meilleure note.
        let below_best = [
            graded(4.9, &["phone"]),
            graded(4.1, &["phone"]),
            graded(4.6, &["phone"]),
        ];
        assert_eq!(
            continuation(&below_best, 2, 1, 2, Some(600)),
            Continuation::Stalled
        );
        // Barre atteinte, poursuites épuisées, quota du jour trop bas.
        let done = [graded(6.0, &["finish"]), graded(8.3, &[])];
        assert_eq!(
            continuation(&done, 0, 0, 2, Some(600)),
            Continuation::Reached
        );
        assert_eq!(
            continuation(&rising, 1, 2, 2, Some(600)),
            Continuation::Exhausted
        );
        assert_eq!(
            continuation(&rising, 1, 1, 2, Some(90)),
            Continuation::LowQuota
        );
        assert_eq!(score_trail(&rising), "6,0 puis 7,3");
        assert!(stop_reason(Continuation::Stalled).is_some());
        assert_eq!(stop_reason(Continuation::Reached), None);
        assert_eq!(super::max_continuations(), 2);
        // Les notes précédentes servent de repère au juge.
        let audit = super::Audit::default();
        let previous = [("phone".to_string(), 7), ("identity".to_string(), 8)];
        let anchored = super::critique_prompt("site", 2, &audit, 8, Some(&previous));
        assert!(anchored.contains("previous review") && anchored.contains("phone=7 identity=8"));
        assert!(!super::critique_prompt("site", 2, &audit, 8, None).contains("previous review"));
    }

    #[test]
    fn a_stylesheet_without_the_foundation_is_noticed() {
        use super::{foundation_feedback, foundation_gaps, FOUNDATION};
        // Essai réel : un CSS écrit sans le socle.
        let bare = ".hero { display: flex; }\na { color: #8a3b12; }\n@keyframes breathe { from { transform: scale(1); } }";
        let gaps = foundation_gaps(bare);
        assert_eq!(gaps.len(), 6, "{gaps:?}");
        assert!(foundation_feedback(&gaps).contains(":hover states on links and buttons"));
        // Le socle de la méthode, recopié, les couvre toutes, espaces ou pas.
        assert!(
            foundation_gaps(FOUNDATION).is_empty(),
            "{:?}",
            foundation_gaps(FOUNDATION)
        );
        assert!(foundation_gaps("*{box-sizing:border-box}a:hover{}:focus-visible{}@media (prefers-reduced-motion: reduce){}.grid>*{min-width: 0}button{font-family: inherit}").is_empty());
    }

    #[test]
    fn a_page_whose_styles_are_not_written_yet_is_not_reviewed() {
        use super::{missing_assets_feedback, missing_local_assets};
        let dir = std::env::temp_dir().join(format!("claw-assets-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("css")).expect("dossier");
        std::fs::write(
            dir.join("index.html"),
            r#"<html><head><link rel="preconnect" href="https://fonts.gstatic.com"><link href="https://fonts.googleapis.com/css2?family=Fraunces" rel="stylesheet"><link rel="stylesheet" href="css/base.css"><LINK REL="stylesheet" HREF="Styles.css?v=2"><script src="script.js" defer></script><script>var x = 1;</script></head><body></body></html>"#,
        )
        .expect("page");
        std::fs::write(dir.join("css").join("base.css"), "body{}").expect("css");
        // Essai réel : styles.css et script.js pas encore écrits.
        assert_eq!(
            missing_local_assets(&dir, "path:index.html"),
            ["Styles.css", "script.js"]
        );
        std::fs::write(dir.join("Styles.css"), "body{}").expect("css");
        std::fs::write(dir.join("script.js"), "").expect("js");
        assert!(missing_local_assets(&dir, "path:index.html").is_empty());
        // Une adresse de serveur ne se lit pas sur le disque : rien à dire.
        assert!(missing_local_assets(&dir, "url:http://localhost:8000/").is_empty());
        let _ = std::fs::remove_dir_all(&dir);
        assert!(
            missing_assets_feedback("path:index.html", &["styles.css".to_string()])
                .starts_with("[Claw] index.html links styles.css, which do not exist yet")
        );
    }

    #[test]
    fn a_design_plan_is_recognized_and_its_review_reads_the_judge() {
        use super::{
            looks_like_design_plan, plan_changes, plan_critique_prompt, plan_feedback,
            turn_request_budget, DESIGN_TURN_REQUESTS,
        };
        let plan = "Couleurs : #F5E6D3 sable, #8B4A3A ocre, #6B8E7A sauge, #c46b3a safran. Typo : Fraunces et Karla.";
        assert!(looks_like_design_plan(plan));
        // Deux couleurs, ou la même trois fois, ou des numéros (#1, #12) : pas un plan.
        assert!(!looks_like_design_plan(
            "#fff et #000, issue #12, #123456 #123456 #123456"
        ));
        let prompt = plan_critique_prompt("Crée un site pour un fleuriste", plan, 8);
        assert!(
            prompt.contains("#8B4A3A")
                && prompt.contains("reach 8 on each")
                && prompt.contains("PLAN OK")
        );
        // Session #32 (26 octobre 2026) : le juge du plan a fait ajouter un code
        // d'accès et une heure limite que la boulangère n'avait pas demandés.
        assert!(prompt.contains("never add a feature, a page, an access code"));
        assert!(!prompt.contains("book, find the hours"));
        assert_eq!(plan_changes("PLAN OK"), None);
        assert_eq!(plan_changes("The plan is strong. PLAN OK"), None);
        let changes = plan_changes("Notes:\n1. [identity] centered hero -> a split hero with a pressed-flower SVG\n2. [typography] Poppins -> Fraunces 600\nGood luck").expect("changes");
        assert_eq!(changes.lines().count(), 2);
        let feedback = plan_feedback("juge", &changes);
        assert!(feedback.starts_with("[Claw] An art director (juge) reviewed your design plan"));
        assert!(feedback.contains("design advice only"));
        // Budget : 160 pour un tour de design, sauf réglage de l'utilisateur.
        assert_eq!(
            turn_request_budget(true, 100, 100, false),
            DESIGN_TURN_REQUESTS
        );
        assert_eq!(turn_request_budget(true, 100, 100, true), 100);
        assert_eq!(turn_request_budget(true, 40, 100, false), 40);
        assert_eq!(turn_request_budget(false, 100, 100, false), 100);
    }

    #[test]
    fn emoji_in_an_interface_file_are_found_but_text_symbols_are_not() {
        use super::{emoji_feedback, emoji_in_source};
        let page = "<h2>Nos pains \u{1F956}</h2>\n<span class=\"icon\">\u{1F4CD}</span>\n<p>Lu \u{2192} Ve, \u{2605}\u{2605}\u{2605}\u{2605}\u{2606}, \u{A9} 2026, \u{2713} fait</p>\n<li>\u{2022} point</li>\n<p>Note : \u{2B50} 4,8</p>";
        let found = emoji_in_source(page).expect("emoji");
        assert_eq!(
            found,
            "line 1: \u{1F956}, line 2: \u{1F4CD}, line 5: \u{2B50}"
        );
        assert!(emoji_feedback("index.html", &found).contains("inline SVG"));
        // Flèches, étoiles de notation, coches, puces, signes légaux : du texte.
        assert_eq!(
            emoji_in_source("<p>\u{2192} \u{2605}\u{2606} \u{2713} \u{2022} \u{A9} \u{AE} \u{2122} \u{2026} \u{AB} \u{BB} 12 \u{20AC} 20\u{B0}</p>"),
            None
        );
        assert_eq!(emoji_in_source("<svg><path d=\"M0 0\"/></svg>"), None);
        // Au-delà de six lignes, le compte.
        let many: String = (0..8).map(|_| "\u{1F600}\n").collect();
        assert!(emoji_in_source(&many)
            .expect("emoji")
            .ends_with("and 2 more line(s)"));
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
        // La barre, le seuil de la revue anticipée et la méthode.
        assert_eq!(design_bar(), 8);
        assert_eq!(super::early_review_at(100), 60);
        assert_eq!(super::early_review_at(40), 24);
        assert_eq!(super::early_review_at(10), 12);
        let guide = design_guide("Crée un site pour un fleuriste", None).expect("méthode");
        assert!(
            guide.contains("under 8")
                && guide.contains("Forbidden unless the brief asks for it")
                && guide.contains("multiples of 8px"),
            "{guide}"
        );
        // Le socle est inséré tel quel, accolades comprises, à sa place.
        assert!(
            guide.contains("https://fonts.googleapis.com/css2?family=Display+Name")
                && guide.contains(".grid > *, .split > *, .row > * { min-width: 0; }")
                && guide.contains("@media (prefers-reduced-motion: reduce) { *, *::before")
                && guide.contains("from the Icon tool"),
            "{guide}"
        );
        let foundation = guide
            .find("Start the page with this foundation")
            .expect("socle");
        assert!(
            guide.find("3. Build.").expect("étape 3") < foundation
                && foundation < guide.find("4. Make everything work").expect("étape 4")
        );
    }
}
