//! Outil `Component` : des composants interactifs prêts à coller (HTML, CSS,
//! JS sans dépendance), accessibles, animés avec mesure et arrêtés par
//! `prefers-reduced-motion`. Un petit modèle écrit des boutons qui ne font
//! rien, des menus qui ne s'ouvrent pas, des formulaires sans retour : ces
//! composants marchent dès qu'ils sont collés, aux couleurs du plan (les
//! variables du socle de la méthode : `--paper`, `--ink`, `--muted`,
//! `--accent`, `--error`). Leurs icônes viennent de Lucide, comme celles de
//! l'outil `Icon`.

use serde::Deserialize;
use serde_json::{json, Value};

struct Component {
    name: &'static str,
    purpose: &'static str,
    html: &'static str,
    css: &'static str,
    js: &'static str,
    /// Autres composants dont celui-ci se sert (rendus avec lui).
    needs: &'static [&'static str],
    note: &'static str,
}

macro_rules! files {
    ($name:literal) => {
        (
            include_str!(concat!("components/", $name, ".html")),
            include_str!(concat!("components/", $name, ".css")),
        )
    };
}

const fn component(
    name: &'static str,
    purpose: &'static str,
    (html, css): (&'static str, &'static str),
    js: &'static str,
    needs: &'static [&'static str],
    note: &'static str,
) -> Component {
    Component {
        name,
        purpose,
        html,
        css,
        js,
        needs,
        note,
    }
}

const COMPONENTS: &[Component] = &[
    component(
        "nav-menu",
        "site header with the navigation folded into a menu button on phones (opens, closes on Escape, a link or a click outside), a shadow once the page scrolls, and the current section marked",
        files!("nav-menu"),
        include_str!("components/nav-menu.js"),
        &[],
        "Put your sections' ids in the links; the call to action keeps the accent.",
    ),
    component(
        "accordion",
        "questions and answers (FAQ) that open one at a time, smoothly where the browser can",
        files!("accordion"),
        "",
        &[],
        "Native <details>: it works without script. The same name attribute makes them open one at a time; remove it to let several stay open.",
    ),
    component(
        "tabs",
        "accessible tabs (click, arrow keys, Home and End) to show one panel at a time: plans, menus, days",
        files!("tabs"),
        include_str!("components/tabs.js"),
        &[],
        "Each tab's aria-controls names its panel's id; only the first panel is shown at first.",
    ),
    component(
        "dialog",
        "modal window (native <dialog>) opened by any button with data-dialog-open, closed by Escape, a close button or a click on the backdrop, focus given back",
        files!("dialog"),
        include_str!("components/dialog.js"),
        &[],
        "One script serves every dialog of the page: give each one an id and point a button at it with data-dialog-open=\"id\".",
    ),
    component(
        "toast",
        "short messages announced to screen readers (window.toast(\"Adresse copiée\")), used by the other components to confirm an action",
        files!("toast"),
        include_str!("components/toast.js"),
        &[],
        "Call window.toast(message) or window.toast(message, { tone: \"error\" }) after any action that changes nothing visible.",
    ),
    component(
        "carousel",
        "swipeable row of pictures or cards (native scroll-snap) with arrows, dots and keyboard",
        files!("carousel"),
        include_str!("components/carousel.js"),
        &[],
        "Give real images (Photo tool) with their width and height; the dots are made by the script.",
    ),
    component(
        "booking",
        "booking of a class, a table or an appointment: days of the next two weeks that have slots, places left, confirmation, and an .ics file for the visitor's calendar",
        files!("booking"),
        include_str!("components/booking.js"),
        &["toast"],
        "Edit the slots in the JSON script (day 0 is Sunday). Without a server, bookings stay in the visitor's browser: set data-endpoint on the section to a form service URL to receive them; data-location fills the calendar event's place.",
    ),
    component(
        "filter",
        "instant filter of a list (classes, products, dishes) by tags and by search, with the count of results, a reset, and the state kept in the address",
        files!("filter"),
        include_str!("components/filter.js"),
        &[],
        "Each item lists its tags in data-tags; each chip's data-filter-tag names one (empty for all).",
    ),
    component(
        "form",
        "contact or request form that shows errors only after a field is filled, keeps a draft, shows the sending, then a success message; sent to a form service (data-endpoint) or as a prefilled e-mail (data-mailto)",
        files!("form"),
        include_str!("components/form.js"),
        &["toast"],
        "Keep one .smart-form__field per field with its error text (data-error-for); set data-mailto to the real address, or data-endpoint to a form service.",
    ),
    component(
        "cart",
        "basket or quote: add buttons, quantities, removal, live total kept on the device, order sent to data-endpoint or as an e-mail summary",
        files!("cart"),
        include_str!("components/cart.js"),
        &["dialog", "toast"],
        "One add button per product (data-id, data-name, data-price); the cart button goes in the header.",
    ),
    component(
        "theme-toggle",
        "dark mode switch that follows the device, then the visitor's choice, remembered",
        files!("theme-toggle"),
        include_str!("components/theme-toggle.js"),
        &[],
        "Set the dark values of your own tokens under :root[data-theme=\"dark\"]; load the script in the <head> to avoid a flash.",
    ),
    component(
        "copy",
        "button that copies an address, a code or a number, with a confirmation",
        files!("copy"),
        include_str!("components/copy.js"),
        &["toast"],
        "data-copy=\"text\", or data-copy-target=\"#id\" to copy an element's text.",
    ),
    component(
        "share",
        "share button: the phone's share sheet, or the link copied elsewhere",
        files!("share"),
        include_str!("components/share.js"),
        &["toast"],
        "data-share-title sets the shared title; the page address is shared.",
    ),
    component(
        "open-now",
        "opening hours with a live status (open now, closes at; closed, opens tomorrow at) and today's row marked",
        files!("open-now"),
        include_str!("components/open-now.js"),
        &[],
        "Edit the hours in the JSON script (day 0 is Sunday) and keep the table in step.",
    ),
    component(
        "directions",
        "address with Directions, Call and Write actions, and a lazy OpenStreetMap map",
        files!("directions"),
        "",
        &[],
        "Put the real address in the directions link (URL-encoded), the phone number in tel: and the map's bbox and marker around the place.",
    ),
    component(
        "reveal",
        "blocks that appear once as they come into view, one after the other in a list; content stays visible without script",
        files!("reveal"),
        include_str!("components/reveal.js"),
        &[],
        "Use it on a few chosen blocks (a list of cards, a key figure), never on every section: a fade on everything marks a generated page.",
    ),
    component(
        "count-up",
        "key figures that count up to their value when reached",
        files!("count-up"),
        include_str!("components/count-up.js"),
        &[],
        "data-count-to holds the value, data-count-suffix and data-count-prefix the unit; the final number stays written in the page.",
    ),
    component(
        "scroll-effects",
        "reading progress bar and a hero picture that drifts slower than the page, in CSS only (scroll-driven animations)",
        files!("scroll-effects"),
        "",
        &[],
        "Pure CSS: browsers without scroll-driven animations show a still page. Keep one effect per page.",
    ),
    component(
        "page-transitions",
        "smooth cross-fade between the pages of a multi-page site, the title gliding from one page to the next",
        files!("page-transitions"),
        "",
        &[],
        "Put the CSS in every page's stylesheet; same-document changes (tabs, filters) already use view transitions where supported.",
    ),
];

/// Mots courants (français et anglais) qui désignent un composant.
const ALIASES: &[(&str, &str)] = &[
    ("menu", "nav-menu"),
    ("navigation", "nav-menu"),
    ("nav", "nav-menu"),
    ("header", "nav-menu"),
    ("faq", "accordion"),
    ("accordeon", "accordion"),
    ("onglets", "tabs"),
    ("modal", "dialog"),
    ("modale", "dialog"),
    ("popup", "dialog"),
    ("notification", "toast"),
    ("message", "toast"),
    ("carrousel", "carousel"),
    ("slider", "carousel"),
    ("galerie", "carousel"),
    ("gallery", "carousel"),
    ("reservation", "booking"),
    ("calendrier", "booking"),
    ("calendar", "booking"),
    ("rendez-vous", "booking"),
    ("filtre", "filter"),
    ("filtres", "filter"),
    ("recherche", "filter"),
    ("search", "filter"),
    ("formulaire", "form"),
    ("contact", "form"),
    ("panier", "cart"),
    ("devis", "cart"),
    ("shop", "cart"),
    ("boutique", "cart"),
    ("mode-sombre", "theme-toggle"),
    ("dark-mode", "theme-toggle"),
    ("theme", "theme-toggle"),
    ("copier", "copy"),
    ("partager", "share"),
    ("horaires", "open-now"),
    ("hours", "open-now"),
    ("ouvert", "open-now"),
    ("itineraire", "directions"),
    ("carte", "directions"),
    ("map", "directions"),
    ("plan", "directions"),
    ("apparition", "reveal"),
    ("animation", "reveal"),
    ("chiffres", "count-up"),
    ("compteur", "count-up"),
    ("counter", "count-up"),
    ("parallaxe", "scroll-effects"),
    ("parallax", "scroll-effects"),
    ("progression", "scroll-effects"),
    ("transitions", "page-transitions"),
];

#[derive(Debug, Deserialize)]
pub(crate) struct ComponentInput {
    #[serde(default)]
    names: Option<Vec<String>>,
    #[serde(default)]
    name: Option<String>,
}

fn find(name: &str) -> Option<&'static Component> {
    let key: String = name
        .trim()
        .to_lowercase()
        .replace(['é', 'è', 'ê'], "e")
        .replace(['à', 'â'], "a")
        .replace([' ', '_'], "-");
    let key = ALIASES
        .iter()
        .find(|(alias, _)| *alias == key)
        .map_or(key.as_str(), |(_, real)| real);
    COMPONENTS.iter().find(|component| component.name == key)
}

/// Les icônes `{{icon:nom}}` remplacées par le SVG de Lucide.
fn with_icons(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("{{icon:") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 7..];
        let Some(end) = after.find("}}") else {
            out.push_str(&rest[start..]);
            return out;
        };
        let name = &after[..end];
        match crate::icons::svg(name, 20) {
            Some(svg) => out.push_str(&svg),
            None => out.push_str(&rest[start..start + 7 + end + 2]),
        }
        rest = &after[end + 2..];
    }
    out.push_str(rest);
    out
}

fn rendered(component: &Component) -> Value {
    let mut value = json!({
        "name": component.name,
        "purpose": component.purpose,
        "html": with_icons(component.html),
        "css": component.css,
        "note": component.note,
    });
    if !component.js.is_empty() {
        value["js"] = json!(with_icons(component.js));
    }
    if !component.needs.is_empty() {
        value["needs"] = json!(component.needs);
    }
    value
}

fn catalog() -> Vec<Value> {
    COMPONENTS
        .iter()
        .map(|component| json!({ "name": component.name, "purpose": component.purpose }))
        .collect()
}

/// Ce que l'outil `Component` rend au modèle.
pub(crate) fn component_tool(input: &ComponentInput) -> Result<Value, String> {
    let mut names: Vec<String> = input.names.clone().unwrap_or_default();
    if let Some(name) = &input.name {
        names.push(name.clone());
    }
    if names.iter().all(|name| name.trim().is_empty()) {
        return Ok(json!({
            "components": catalog(),
            "note": "Ask for the components you need by name, several in one call (names: [\"nav-menu\", \"booking\", \"toast\"]).",
        }));
    }
    let mut chosen: Vec<&'static Component> = Vec::new();
    let mut unknown = Vec::new();
    for name in &names {
        match find(name) {
            Some(found) => {
                // Les composants dont il se sert d'abord, chacun une fois.
                for need in found.needs.iter().filter_map(|need| find(need)) {
                    if !chosen.iter().any(|known| known.name == need.name) {
                        chosen.push(need);
                    }
                }
                if !chosen.iter().any(|known| known.name == found.name) {
                    chosen.push(found);
                }
            }
            None => unknown.push(name.clone()),
        }
    }
    if chosen.is_empty() {
        return Err(format!(
            "unknown component(s) {}: use one of {}",
            unknown.join(", "),
            COMPONENTS
                .iter()
                .map(|component| component.name)
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    let mut result = json!({
        "components": chosen.iter().map(|component| rendered(component)).collect::<Vec<_>>(),
        "note": "Paste each component's html where it belongs, its css into the stylesheet and its js into the page's script (or a <script> before </body>), as they are: the data and ARIA attributes make them work. Write the visible texts in the page's language and content, and restyle freely with the plan's tokens (they already use --paper, --ink, --muted, --accent and --error, with fallbacks). Every button does what its label says; Claw clicks each one in the review.",
    });
    if !unknown.is_empty() {
        result["unknown"] = json!(unknown);
        result["catalog"] = json!(catalog());
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::{component_tool, find, with_icons, ComponentInput, COMPONENTS};
    use serde_json::json;
    use std::fmt::Write as _;

    fn ask(value: serde_json::Value) -> serde_json::Value {
        let input: ComponentInput = serde_json::from_value(value).expect("entrée");
        component_tool(&input).expect("composants")
    }

    #[test]
    fn every_component_is_complete_and_draws_no_icon_by_hand() {
        assert_eq!(COMPONENTS.len(), 19);
        for component in COMPONENTS {
            let html = with_icons(component.html);
            let js = with_icons(component.js);
            for text in [&html, &js] {
                assert!(
                    !text.contains("{{icon:"),
                    "{}: icône inconnue",
                    component.name
                );
                assert!(
                    runtime::icon_set::drawn_icons(text).is_empty(),
                    "{}: icône dessinée",
                    component.name
                );
                assert!(
                    runtime::web_design::emoji_in_source(text).is_none(),
                    "{}: emoji",
                    component.name
                );
            }
            assert_eq!(
                runtime::harness::html_structure(&html),
                None,
                "{}: imbrication",
                component.name
            );
            assert!(component.css.contains('{'), "{}: css", component.name);
            for need in component.needs {
                assert!(find(need).is_some(), "{} : {need}", component.name);
            }
        }
    }

    /// Une page qui réunit tous les composants, comme un modèle les collerait.
    pub(crate) fn demo_page() -> String {
        let mut html = String::new();
        let mut css = String::from(":root { --paper: #f7f3ec; --ink: #1f1d1a; --muted: #5f5a52; --accent: #8a3b12; --error: #b42318; --font-display: Georgia, serif; --font-text: system-ui, sans-serif; }\n*, *::before, *::after { box-sizing: border-box; }\nbody { margin: 0; font-family: var(--font-text); font-size: 1.0625rem; line-height: 1.6; color: var(--ink); background: var(--paper); }\nmain { width: min(100% - 2.5rem, 60rem); margin: 0 auto; padding: 2rem 0 6rem; }\nmain > section + section { margin-top: 3rem; }\n:focus-visible { outline: 2px solid var(--accent); outline-offset: 3px; }\nimg { max-width: 100%; height: auto; }\n");
        let mut js = String::new();
        for component in COMPONENTS {
            let body = with_icons(component.html)
                .replace("images/bouquet-1.jpg", "data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 800 1000'%3E%3Crect width='800' height='1000' fill='%23c9a27e'/%3E%3C/svg%3E")
                .replace("images/bouquet-2.jpg", "data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 800 1000'%3E%3Crect width='800' height='1000' fill='%238fa98b'/%3E%3C/svg%3E")
                .replace("images/bouquet-3.jpg", "data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 800 1000'%3E%3Crect width='800' height='1000' fill='%23b8655a'/%3E%3C/svg%3E")
                .replace("images/hero.jpg", "data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 1600 1000'%3E%3Crect width='1600' height='1000' fill='%23d8c3a5'/%3E%3C/svg%3E");
            match component.name {
                // L'en-tête en haut, ses liens vers des sections de la page.
                "nav-menu" => html.insert_str(
                    0,
                    &body
                        .replace("#services", "#demo-tabs")
                        .replace("#tarifs", "#demo-cart")
                        .replace("#avis", "#demo-form")
                        .replace("#contact", "#demo-directions")
                        .replace("#reservation", "#demo-booking"),
                ),
                // La zone des messages vit hors des sections, comme dans une page.
                "toast" | "page-transitions" => html.push_str(&body),
                name => {
                    let _ = writeln!(
                        html,
                        "<section id=\"demo-{name}\" aria-label=\"{name}\"><h2>{name}</h2>{body}</section>"
                    );
                }
            }
            css.push_str(component.css);
            css.push('\n');
            js.push_str(&with_icons(component.js));
            js.push('\n');
        }
        let (header, rest) = html.split_at(html.find("</header>").map_or(0, |end| end + 9));
        format!("<!doctype html><html lang=\"fr\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\"><title>Composants</title><style>{css}</style></head><body id=\"top\">{header}<main>{rest}</main><script>{js}</script></body></html>")
    }

    #[test]
    #[ignore = "lance le navigateur installé"]
    fn every_component_works_in_a_real_browser() {
        let cwd = std::env::current_dir().expect("dossier");
        let dir = cwd.join(format!("claw-components-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dossier");
        std::fs::write(dir.join("index.html"), demo_page()).expect("page");
        if let Ok(copy) = std::env::var("CLAW_COMPONENTS_DEMO") {
            let _ = std::fs::write(copy, demo_page());
        }
        let input: crate::screenshot::ScreenshotInput = serde_json::from_value(json!({
            "path": format!("claw-components-{}/index.html", std::process::id()),
            "viewports": ["phone", "desktop"],
            "full_page": true
        }))
        .expect("entrée");
        let result = crate::screenshot::take(&input);
        let _ = std::fs::remove_dir_all(&dir);
        let result = result.expect("captures");
        for capture in result["captures"].as_array().expect("captures") {
            let problems = capture["problems"].to_string();
            let console = capture["console"].to_string();
            for defect in [
                "do nothing when clicked",
                "clicking ",
                "drawn by hand",
                "stay invisible",
                "menu button",
            ] {
                assert!(!problems.contains(defect), "{defect}: {problems}");
            }
            assert!(
                !console.contains("uncaught") && !console.contains("error:"),
                "{console}"
            );
        }
    }

    #[test]
    fn components_are_found_by_name_or_alias_with_what_they_need() {
        let result = ask(json!({ "names": ["panier", "booking"] }));
        let names: Vec<&str> = result["components"]
            .as_array()
            .expect("liste")
            .iter()
            .map(|component| component["name"].as_str().expect("nom"))
            .collect();
        // Le panier se sert de dialog et de toast, la réservation de toast :
        // chacun une fois, avant celui qui s'en sert.
        assert_eq!(names, ["dialog", "toast", "cart", "booking"]);
        assert!(result["components"][2]["js"]
            .as_str()
            .expect("js")
            .contains("data-cart"));
        assert!(find("Réservation").is_some() && find("FAQ").is_some());
        // Sans nom : le catalogue.
        let catalog = ask(json!({}));
        assert_eq!(
            catalog["components"].as_array().expect("catalogue").len(),
            19
        );
        // Un nom inconnu reçoit le catalogue avec les autres.
        let mixed = ask(json!({ "names": ["dragon", "tabs"] }));
        assert_eq!(mixed["unknown"], json!(["dragon"]));
        let input: ComponentInput =
            serde_json::from_value(json!({ "names": ["dragon"] })).expect("entrée");
        assert!(component_tool(&input)
            .expect_err("inconnu")
            .contains("nav-menu"));
    }
}
