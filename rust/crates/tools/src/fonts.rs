//! Outil `Font` : une bibliothèque de polices de Google Fonts choisies pour les interfaces,
//! rangées par style et par usage, et le code prêt à coller pour s'en servir : la balise
//! `<link>` avec les seules graisses demandées, puis le CSS (variables de police, échelle de
//! tailles fluide calculée de la taille de base et du rapport, graisses et interlignes des
//! titres et du texte). Demandé le 24 octobre 2026 : « une bibliothèque où il peut customiser
//! couleur, taille, toute sorte […] et pareil pour les Fonts ». Les polices trop vues, que la
//! méthode de design de Claw écarte (Inter, Roboto, Arial, Open Sans, Montserrat, Poppins,
//! Lato, Playfair Display, Space Grotesk), n'y sont pas.

use serde::Deserialize;
use serde_json::{json, Value};

/// Une police : nom de famille, genre, graisses disponibles chez Google Fonts, italique,
/// styles (mots anglais), rôle (`display` : titres ; `text` : texte et interface ; `both` ;
/// `code`).
struct Font {
    name: &'static str,
    kind: &'static str,
    weights: &'static [u16],
    italic: bool,
    moods: &'static str,
    role: &'static str,
}

const fn font(
    name: &'static str,
    kind: &'static str,
    weights: &'static [u16],
    italic: bool,
    moods: &'static str,
    role: &'static str,
) -> Font {
    Font {
        name,
        kind,
        weights,
        italic,
        moods,
        role,
    }
}

const W100_900: &[u16] = &[100, 200, 300, 400, 500, 600, 700, 800, 900];
const W200_800: &[u16] = &[200, 300, 400, 500, 600, 700, 800];
const W200_900: &[u16] = &[200, 300, 400, 500, 600, 700, 800, 900];
const W300_700: &[u16] = &[300, 400, 500, 600, 700];
const W300_900: &[u16] = &[300, 400, 500, 600, 700, 800, 900];
const W400_700: &[u16] = &[400, 500, 600, 700];
const W400_800: &[u16] = &[400, 500, 600, 700, 800];
const W400_900: &[u16] = &[400, 500, 600, 700, 800, 900];
const W400: &[u16] = &[400];

/// La bibliothèque. Les graisses sont vérifiées contre Google Fonts
/// (`rust/scripts/fonts-check.py`).
const FONTS: &[Font] = &[
    // Titres et texte avec empattements.
    font(
        "Fraunces",
        "serif",
        W100_900,
        true,
        "elegant editorial warm retro luxury bakery food",
        "both",
    ),
    font(
        "DM Serif Display",
        "serif",
        W400,
        true,
        "elegant editorial luxury fashion",
        "display",
    ),
    font(
        "Cormorant Garamond",
        "serif",
        W300_700,
        true,
        "elegant luxury wedding fashion classic",
        "display",
    ),
    font(
        "Libre Baskerville",
        "serif",
        &[400, 700],
        false,
        "classic editorial serious law",
        "both",
    ),
    font(
        "Lora",
        "serif",
        W400_700,
        true,
        "warm editorial classic blog",
        "both",
    ),
    font(
        "Merriweather",
        "serif",
        W300_900,
        true,
        "serious editorial readable news",
        "text",
    ),
    font(
        "Source Serif 4",
        "serif",
        W200_900,
        true,
        "serious editorial readable",
        "text",
    ),
    font(
        "Crimson Pro",
        "serif",
        W200_900,
        true,
        "classic editorial book",
        "text",
    ),
    font(
        "EB Garamond",
        "serif",
        W400_800,
        true,
        "classic elegant book luxury",
        "both",
    ),
    font(
        "Bitter",
        "serif",
        W100_900,
        true,
        "warm readable sturdy",
        "both",
    ),
    font(
        "Literata",
        "serif",
        W200_900,
        true,
        "readable book editorial",
        "text",
    ),
    font(
        "Newsreader",
        "serif",
        W200_800,
        true,
        "editorial news serious",
        "both",
    ),
    font(
        "Young Serif",
        "serif",
        W400,
        false,
        "warm retro friendly food",
        "display",
    ),
    font(
        "Instrument Serif",
        "serif",
        W400,
        true,
        "elegant editorial modern fashion",
        "display",
    ),
    font(
        "Gloock",
        "serif",
        W400,
        false,
        "bold elegant editorial",
        "display",
    ),
    font(
        "Abril Fatface",
        "serif",
        W400,
        false,
        "bold retro editorial poster",
        "display",
    ),
    // Sans empattements : interfaces et texte.
    font(
        "DM Sans",
        "sans",
        W100_900,
        true,
        "modern minimal friendly clean finance",
        "both",
    ),
    font(
        "Manrope",
        "sans",
        W200_800,
        false,
        "modern tech minimal finance",
        "both",
    ),
    font(
        "Plus Jakarta Sans",
        "sans",
        W200_800,
        true,
        "modern friendly startup finance",
        "both",
    ),
    font(
        "Outfit",
        "sans",
        W100_900,
        false,
        "modern geometric friendly",
        "both",
    ),
    font(
        "Sora",
        "sans",
        W100_900,
        false,
        "tech modern geometric",
        "both",
    ),
    font(
        "Urbanist",
        "sans",
        W100_900,
        true,
        "modern geometric minimal",
        "both",
    ),
    font(
        "Figtree",
        "sans",
        W300_900,
        true,
        "friendly clean modern",
        "both",
    ),
    font(
        "Work Sans",
        "sans",
        W100_900,
        true,
        "minimal clean readable",
        "text",
    ),
    font(
        "Karla",
        "sans",
        W200_800,
        true,
        "friendly quirky readable",
        "text",
    ),
    font(
        "Rubik",
        "sans",
        W300_900,
        true,
        "friendly rounded playful",
        "both",
    ),
    font(
        "Nunito",
        "sans",
        W200_900,
        true,
        "friendly rounded kids soft",
        "both",
    ),
    font(
        "Lexend",
        "sans",
        W100_900,
        false,
        "readable accessible clean",
        "text",
    ),
    font(
        "Public Sans",
        "sans",
        W100_900,
        true,
        "serious neutral government",
        "text",
    ),
    font(
        "IBM Plex Sans",
        "sans",
        W100_900,
        true,
        "tech serious corporate",
        "both",
    ),
    font(
        "Albert Sans",
        "sans",
        W100_900,
        true,
        "modern clean minimal",
        "both",
    ),
    font(
        "Red Hat Display",
        "sans",
        W300_900,
        true,
        "bold modern tech",
        "display",
    ),
    font(
        "Bricolage Grotesque",
        "sans",
        W200_800,
        false,
        "bold quirky modern startup",
        "display",
    ),
    font(
        "Schibsted Grotesk",
        "sans",
        W400_900,
        true,
        "editorial news modern",
        "both",
    ),
    font(
        "Onest",
        "sans",
        W100_900,
        false,
        "modern clean neutral",
        "both",
    ),
    font(
        "Hanken Grotesk",
        "sans",
        W100_900,
        true,
        "minimal modern clean",
        "both",
    ),
    // Affiches, jeux, enfants.
    font(
        "Bebas Neue",
        "display",
        W400,
        false,
        "bold poster sport retro",
        "display",
    ),
    font(
        "Archivo Black",
        "display",
        W400,
        false,
        "bold poster strong",
        "display",
    ),
    font(
        "Anton",
        "display",
        W400,
        false,
        "bold poster condensed sport",
        "display",
    ),
    font(
        "Righteous",
        "display",
        W400,
        false,
        "retro playful bold",
        "display",
    ),
    font(
        "Fredoka",
        "display",
        W300_700,
        false,
        "playful kids rounded friendly",
        "both",
    ),
    font(
        "Baloo 2",
        "display",
        W400_800,
        false,
        "playful kids rounded friendly",
        "both",
    ),
    font(
        "Syne",
        "display",
        W400_800,
        false,
        "bold artistic modern",
        "display",
    ),
    font(
        "Unbounded",
        "display",
        W200_900,
        false,
        "bold tech futuristic",
        "display",
    ),
    // Écriture à la main.
    font(
        "Caveat",
        "handwriting",
        W400_700,
        false,
        "handwritten casual notes friendly",
        "display",
    ),
    font(
        "Kalam",
        "handwriting",
        &[300, 400, 700],
        false,
        "handwritten casual notes",
        "display",
    ),
    font(
        "Patrick Hand",
        "handwriting",
        W400,
        false,
        "handwritten kids playful",
        "display",
    ),
    font(
        "Dancing Script",
        "handwriting",
        W400_700,
        false,
        "handwritten elegant wedding",
        "display",
    ),
    font(
        "Pacifico",
        "handwriting",
        W400,
        false,
        "handwritten retro playful surf",
        "display",
    ),
    // Code et chiffres.
    font(
        "JetBrains Mono",
        "mono",
        &[100, 200, 300, 400, 500, 600, 700, 800],
        true,
        "code tech numbers",
        "code",
    ),
    font(
        "IBM Plex Mono",
        "mono",
        &[100, 200, 300, 400, 500, 600, 700],
        true,
        "code tech numbers corporate",
        "code",
    ),
    font(
        "DM Mono",
        "mono",
        &[300, 400, 500],
        true,
        "code minimal numbers",
        "code",
    ),
    font("Fira Code", "mono", W300_700, false, "code tech", "code"),
    font(
        "Space Mono",
        "mono",
        &[400, 700],
        true,
        "code retro tech",
        "code",
    ),
];

/// Paires éprouvées : police de titres, police de texte, styles.
const PAIRS: &[(&str, &str, &str)] = &[
    (
        "Fraunces",
        "DM Sans",
        "warm elegant food bakery editorial friendly",
    ),
    ("DM Serif Display", "Figtree", "elegant luxury fashion"),
    (
        "Cormorant Garamond",
        "Albert Sans",
        "luxury wedding elegant fashion",
    ),
    (
        "Instrument Serif",
        "Hanken Grotesk",
        "editorial modern elegant minimal",
    ),
    ("Newsreader", "Public Sans", "editorial news serious"),
    ("Young Serif", "Karla", "warm retro friendly food"),
    ("Bricolage Grotesque", "Onest", "startup modern bold quirky"),
    ("Sora", "Manrope", "tech finance modern minimal"),
    (
        "Plus Jakarta Sans",
        "Plus Jakarta Sans",
        "finance startup friendly modern",
    ),
    ("Red Hat Display", "IBM Plex Sans", "tech corporate serious"),
    ("Unbounded", "Outfit", "tech futuristic bold"),
    ("Fredoka", "Nunito", "kids playful friendly rounded"),
    ("Baloo 2", "Rubik", "kids playful friendly"),
    ("Bebas Neue", "Work Sans", "sport poster bold"),
    ("Abril Fatface", "Lora", "retro editorial poster"),
    ("Caveat", "Figtree", "handwritten casual notes friendly"),
    ("Syne", "Albert Sans", "artistic bold modern"),
    ("Gloock", "Schibsted Grotesk", "editorial bold elegant"),
];

/// Mots courants (français surtout) et le style qu'ils désignent.
const WORDS: &[(&str, &str)] = &[
    ("elegant", "elegant"),
    ("chic", "elegant"),
    ("luxe", "luxury"),
    ("luxueux", "luxury"),
    ("mariage", "wedding"),
    ("mode", "fashion"),
    ("moderne", "modern"),
    ("sobre", "minimal"),
    ("minimaliste", "minimal"),
    ("epure", "minimal"),
    ("ludique", "playful"),
    ("fun", "playful"),
    ("jeu", "playful"),
    ("enfant", "kids"),
    ("enfants", "kids"),
    ("chaleureux", "warm"),
    ("convivial", "friendly"),
    ("sympa", "friendly"),
    ("amical", "friendly"),
    ("manuscrit", "handwritten"),
    ("main", "handwritten"),
    ("serieux", "serious"),
    ("pro", "serious"),
    ("professionnel", "serious"),
    ("entreprise", "corporate"),
    ("retro", "retro"),
    ("vintage", "retro"),
    ("gras", "bold"),
    ("fort", "bold"),
    ("affiche", "poster"),
    ("technique", "tech"),
    ("techno", "tech"),
    ("startup", "startup"),
    ("banque", "finance"),
    ("argent", "finance"),
    ("budget", "finance"),
    ("compte", "finance"),
    ("finance", "finance"),
    ("journal", "editorial"),
    ("magazine", "editorial"),
    ("blog", "blog"),
    ("livre", "book"),
    ("lecture", "readable"),
    ("lisible", "readable"),
    ("accessible", "accessible"),
    ("code", "code"),
    ("chiffres", "numbers"),
    ("boulangerie", "bakery"),
    ("restaurant", "food"),
    ("cuisine", "food"),
    ("sport", "sport"),
    ("rond", "rounded"),
    ("arrondi", "rounded"),
    ("doux", "soft"),
    ("futuriste", "futuristic"),
    ("artistique", "artistic"),
];

#[derive(Debug, Default, Deserialize)]
pub(crate) struct FontInput {
    /// Mots de style ou d'usage (« élégant », « finance », « enfants », « code »).
    #[serde(default)]
    search: Option<String>,
    /// Police des titres.
    #[serde(default)]
    display: Option<String>,
    /// Police du texte et de l'interface.
    #[serde(default)]
    text: Option<String>,
    /// Police du code ou des chiffres alignés.
    #[serde(default)]
    mono: Option<String>,
    /// Graisses des titres (700 par défaut, ou la plus proche).
    #[serde(default)]
    display_weights: Option<Vec<u16>>,
    /// Graisses du texte (400 et 600 par défaut, ou les plus proches).
    #[serde(default)]
    text_weights: Option<Vec<u16>>,
    /// Italiques du texte en plus.
    #[serde(default)]
    italic: Option<bool>,
    /// Taille du texte courant, en pixels (17 par défaut).
    #[serde(default)]
    base_size: Option<f32>,
    /// Rapport entre deux tailles de l'échelle (1.25 par défaut).
    #[serde(default)]
    scale: Option<f32>,
}

fn fold(text: &str) -> String {
    text.to_lowercase()
        .chars()
        .map(|ch| match ch {
            'à' | 'â' | 'ä' => 'a',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'î' | 'ï' => 'i',
            'ô' | 'ö' => 'o',
            'ù' | 'û' | 'ü' => 'u',
            'ç' => 'c',
            other => other,
        })
        .collect()
}

fn find(name: &str) -> Option<&'static Font> {
    let wanted = fold(name.trim()).replace(['-', '_', '+'], " ");
    FONTS
        .iter()
        .find(|font| fold(font.name) == wanted.split_whitespace().collect::<Vec<_>>().join(" "))
}

/// Styles visés par les mots de la recherche.
fn moods(query: &str) -> Vec<String> {
    let mut found = Vec::new();
    for word in fold(query)
        .split(|ch: char| !ch.is_alphanumeric())
        .filter(|word| word.len() > 1)
    {
        let mood = WORDS
            .iter()
            .find(|(french, _)| *french == word)
            .map_or(word, |(_, mood)| *mood)
            .to_string();
        if !found.contains(&mood) {
            found.push(mood);
        }
    }
    found
}

fn score(moods_of: &str, wanted: &[String]) -> usize {
    let own: Vec<&str> = moods_of.split_whitespace().collect();
    wanted
        .iter()
        .filter(|mood| own.contains(&mood.as_str()))
        .count()
}

fn describe(font: &Font) -> Value {
    json!({
        "name": font.name,
        "kind": font.kind,
        "role": font.role,
        "weights": font.weights,
        "italic": font.italic,
        "styles": font.moods,
    })
}

/// La graisse disponible la plus proche de celle demandée.
fn nearest(font: &Font, weight: u16) -> u16 {
    *font
        .weights
        .iter()
        .min_by_key(|available| available.abs_diff(weight))
        .unwrap_or(&400)
}

fn weights_of(font: &Font, asked: Option<&Vec<u16>>, default: &[u16]) -> Vec<u16> {
    let mut chosen: Vec<u16> = asked
        .filter(|list| !list.is_empty())
        .map_or(default, Vec::as_slice)
        .iter()
        .map(|weight| nearest(font, *weight))
        .collect();
    chosen.sort_unstable();
    chosen.dedup();
    chosen
}

/// Morceau `family=` de l'adresse de Google Fonts.
fn family(font: &Font, weights: &[u16], italic: bool) -> String {
    let name = font.name.replace(' ', "+");
    if font.weights == W400 && !italic {
        return name;
    }
    if italic && font.italic {
        let mut axes: Vec<String> = weights.iter().map(|weight| format!("0,{weight}")).collect();
        axes.extend(weights.iter().map(|weight| format!("1,{weight}")));
        format!("{name}:ital,wght@{}", axes.join(";"))
    } else {
        let list: Vec<String> = weights.iter().map(u16::to_string).collect();
        format!("{name}:wght@{}", list.join(";"))
    }
}

fn stack(font: &Font) -> String {
    let fallback = match font.kind {
        "serif" => "Georgia, \"Times New Roman\", serif",
        "mono" => "ui-monospace, \"SFMono-Regular\", Consolas, monospace",
        "handwriting" => "\"Comic Sans MS\", cursive",
        _ => "system-ui, -apple-system, \"Segoe UI\", sans-serif",
    };
    format!("\"{}\", {fallback}", font.name)
}

/// Une taille fluide : `size` pixels sur un écran large, un peu moins sur un téléphone.
fn fluid(phone: f32, desktop: f32) -> String {
    if (desktop - phone).abs() < 0.5 {
        return format!("{:.3}rem", desktop / 16.0);
    }
    // De 360 à 1280 px de large.
    let slope = (desktop - phone) / (1280.0 - 360.0);
    let at_zero = phone - slope * 360.0;
    format!(
        "clamp({:.3}rem, {:.3}rem + {:.3}vw, {:.3}rem)",
        phone / 16.0,
        at_zero / 16.0,
        slope * 100.0,
        desktop / 16.0
    )
}

fn unknown(name: &str) -> String {
    let closest: Vec<&str> = FONTS
        .iter()
        .filter(|font| {
            fold(font.name)
                .split_whitespace()
                .any(|part| fold(name).contains(part))
        })
        .map(|font| font.name)
        .take(5)
        .collect();
    if closest.is_empty() {
        format!("`{name}` is not in the library: search it by style with `search` (for example \"elegant\", \"finance\", \"kids\")")
    } else {
        format!(
            "`{name}` is not in the library; close names: {}",
            closest.join(", ")
        )
    }
}

/// Ce que l'outil `Font` rend au modèle.
pub(crate) fn fonts(input: &FontInput) -> Result<Value, String> {
    let query = input.search.as_deref().map(str::trim).unwrap_or_default();
    let named = [&input.display, &input.text, &input.mono]
        .iter()
        .any(|name| name.as_deref().is_some_and(|name| !name.trim().is_empty()));
    if query.is_empty() && !named {
        return Err("give `search` (style or use: \"elegant\", \"finance\", \"kids\", \"code\") to choose fonts, or `display` and `text` (font names from the library) to get the code".to_string());
    }
    let mut result = json!({});
    if !query.is_empty() {
        let wanted = moods(query);
        let mut ranked: Vec<(usize, &Font)> = FONTS
            .iter()
            .map(|font| (score(font.moods, &wanted), font))
            .filter(|(points, _)| *points > 0)
            .collect();
        ranked.sort_by(|a, b| b.0.cmp(&a.0));
        let mut pairs: Vec<(usize, &(&str, &str, &str))> = PAIRS
            .iter()
            .map(|pair| (score(pair.2, &wanted), pair))
            .filter(|(points, _)| *points > 0)
            .collect();
        pairs.sort_by(|a, b| b.0.cmp(&a.0));
        result["fonts"] = json!(ranked
            .iter()
            .take(10)
            .map(|(_, font)| describe(font))
            .collect::<Vec<_>>());
        result["pairs"] = json!(pairs
            .iter()
            .take(4)
            .map(|(_, (display, text, styles))| json!({ "display": display, "text": text, "styles": styles }))
            .collect::<Vec<_>>());
        if ranked.is_empty() {
            result["hint"] = json!("no font matches these words: try elegant, modern, minimal, friendly, playful, kids, warm, serious, tech, finance, editorial, retro, bold, handwritten, code");
        }
    }
    if named {
        let pick = |name: &Option<String>| -> Result<Option<&'static Font>, String> {
            match name
                .as_deref()
                .map(str::trim)
                .filter(|name| !name.is_empty())
            {
                None => Ok(None),
                Some(name) => find(name).map(Some).ok_or_else(|| unknown(name)),
            }
        };
        let display = pick(&input.display)?;
        let text = pick(&input.text)?.or(display);
        let display = display.or(text);
        let mono = pick(&input.mono)?;
        let base = input.base_size.unwrap_or(17.0).clamp(14.0, 22.0);
        let ratio = input.scale.unwrap_or(1.25).clamp(1.1, 1.6);
        let italic = input.italic.unwrap_or(false);
        let mut families = Vec::new();
        let mut used = json!({});
        let display_weights =
            display.map(|font| weights_of(font, input.display_weights.as_ref(), &[700]));
        let text_weights =
            text.map(|font| weights_of(font, input.text_weights.as_ref(), &[400, 600]));
        match (display, text) {
            (Some(display), Some(text)) if display.name == text.name => {
                let mut all = display_weights.clone().unwrap_or_default();
                all.extend(text_weights.clone().unwrap_or_default());
                all.sort_unstable();
                all.dedup();
                families.push(family(text, &all, italic));
                used[text.name] = json!(all);
            }
            _ => {
                if let (Some(font), Some(weights)) = (display, &display_weights) {
                    families.push(family(font, weights, false));
                    used[font.name] = json!(weights);
                }
                if let (Some(font), Some(weights)) = (text, &text_weights) {
                    families.push(family(font, weights, italic));
                    used[font.name] = json!(weights);
                }
            }
        }
        if let Some(font) = mono {
            let weights = weights_of(font, None, &[400]);
            families.push(family(font, &weights, false));
            used[font.name] = json!(weights);
        }
        let href = format!(
            "https://fonts.googleapis.com/css2?{}&display=swap",
            families
                .iter()
                .map(|family| format!("family={family}"))
                .collect::<Vec<_>>()
                .join("&")
        );
        let link = format!(
            "<link rel=\"preconnect\" href=\"https://fonts.googleapis.com\">\n<link rel=\"preconnect\" href=\"https://fonts.gstatic.com\" crossorigin>\n<link href=\"{href}\" rel=\"stylesheet\">"
        );
        // Échelle : un rapport plus doux au téléphone, pour que h1 y tienne.
        let phone_ratio = 1.0 + (ratio - 1.0) * 0.75;
        let steps: Vec<String> = (-1..=5)
            .map(|step: i32| {
                let desktop = base * ratio.powi(step);
                let phone = (base - 1.0).max(14.0) * phone_ratio.powi(step);
                let name = if step < 0 {
                    "-1".to_string()
                } else {
                    step.to_string()
                };
                format!("  --step-{name}: {};", fluid(phone, desktop))
            })
            .collect();
        let display_font = display.or(text).expect("une police au moins");
        let text_font = text.or(display).expect("une police au moins");
        let heading_weight = display_weights
            .as_ref()
            .and_then(|weights| weights.last().copied())
            .unwrap_or(700);
        let body_weight = text_weights
            .as_ref()
            .and_then(|weights| weights.first().copied())
            .unwrap_or(400);
        let strong_weight = text_weights
            .as_ref()
            .and_then(|weights| weights.last().copied())
            .unwrap_or(600);
        let tight = if display_font.kind == "display" || display_font.kind == "handwriting" {
            "0"
        } else {
            "-0.015em"
        };
        let mut css = format!(
            ":root {{\n  --font-display: {};\n  --font-text: {};\n{}{}\n}}\n\nbody {{ font-family: var(--font-text); font-size: var(--step-0); font-weight: {body_weight}; line-height: 1.6; }}\nh1, h2, h3, h4 {{ font-family: var(--font-display); font-weight: {heading_weight}; line-height: 1.15; letter-spacing: {tight}; text-wrap: balance; }}\nh1 {{ font-size: var(--step-5); }}\nh2 {{ font-size: var(--step-4); }}\nh3 {{ font-size: var(--step-2); }}\nh4 {{ font-size: var(--step-1); }}\nsmall, .small {{ font-size: var(--step--1); }}\nstrong, b, button, .button {{ font-weight: {strong_weight}; }}\n",
            stack(display_font),
            stack(text_font),
            mono.map_or_else(String::new, |font| format!("  --font-mono: {};\n", stack(font))),
            steps.join("\n"),
        );
        if mono.is_some() {
            css.push_str("code, pre, kbd, .number { font-family: var(--font-mono); font-variant-numeric: tabular-nums; }\n");
        }
        result["link"] = json!(link);
        result["css"] = json!(css);
        result["weights"] = used;
        result["scale"] = json!({ "base_px": base, "ratio": ratio });
    }
    result["note"] = json!("Fonts from Google Fonts (SIL Open Font License). Put `link` in <head>, before your stylesheet, and the `css` at the top of it; then use var(--font-display), var(--font-text) and the --step-* sizes everywhere instead of new font names or pixel sizes. Ask only the weights you use (each one is a download). Change the look with `display_weights`, `text_weights`, `italic`, `base_size` (px of body text) and `scale` (ratio between sizes: 1.2 calm, 1.25 balanced, 1.333 marked, 1.5 dramatic).");
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::{fonts, FontInput, FONTS, PAIRS};

    fn input() -> FontInput {
        FontInput::default()
    }

    #[test]
    fn a_style_in_french_finds_fonts_and_pairs() {
        let result = fonts(&FontInput {
            search: Some("une appli de budget, moderne et sympa".to_string()),
            ..input()
        })
        .expect("recherche");
        let names: Vec<&str> = result["fonts"]
            .as_array()
            .expect("polices")
            .iter()
            .filter_map(|font| font["name"].as_str())
            .collect();
        assert!(names.contains(&"DM Sans"), "{names:?}");
        assert!(!result["pairs"].as_array().expect("paires").is_empty());
    }

    #[test]
    fn a_pair_gives_its_link_with_only_the_weights_asked_and_a_fluid_scale() {
        let result = fonts(&FontInput {
            display: Some("fraunces".to_string()),
            text: Some("DM Sans".to_string()),
            display_weights: Some(vec![650]),
            italic: Some(true),
            base_size: Some(18.0),
            scale: Some(1.333),
            ..input()
        })
        .expect("code");
        let link = result["link"].as_str().expect("lien");
        assert!(
            link.contains("family=Fraunces:wght@600&family=DM+Sans:ital,wght@0,400;0,600;1,400;1,600&display=swap"),
            "{link}"
        );
        let css = result["css"].as_str().expect("css");
        assert!(
            css.contains("--font-display: \"Fraunces\", Georgia"),
            "{css}"
        );
        assert!(css.contains("--step-5: clamp("), "{css}");
        assert!(css.contains("font-weight: 600; line-height: 1.15"), "{css}");
    }

    #[test]
    fn a_single_family_and_unknown_names_are_handled() {
        let result = fonts(&FontInput {
            text: Some("Plus Jakarta Sans".to_string()),
            mono: Some("JetBrains Mono".to_string()),
            ..input()
        })
        .expect("code");
        let link = result["link"].as_str().expect("lien");
        assert!(
            link.contains("family=Plus+Jakarta+Sans:wght@400;600;700"),
            "{link}"
        );
        assert!(link.contains("family=JetBrains+Mono:wght@400"), "{link}");
        let error = fonts(&FontInput {
            display: Some("Inter".to_string()),
            ..input()
        })
        .expect_err("absente");
        assert!(error.contains("not in the library"), "{error}");
        assert!(fonts(&input()).is_err());
    }

    #[test]
    fn every_pair_names_fonts_of_the_library() {
        for (display, text, _) in PAIRS {
            assert!(FONTS.iter().any(|font| font.name == *display), "{display}");
            assert!(FONTS.iter().any(|font| font.name == *text), "{text}");
        }
    }
}
