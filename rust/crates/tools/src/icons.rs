//! Outil `Icon` : des icônes SVG soignées, prêtes à coller dans une page, tirées
//! de Lucide (1 539 icônes, licence ISC, `crates/runtime/src/icons/LUCIDE_LICENSE`). Un petit
//! modèle dessine mal en SVG : vu dans les essais réels, ses icônes faites à
//! la main valaient à la page une finition notée 5/10. Claw interdit les emoji
//! dans une interface ; ces icônes sont du SVG, au trait, de la couleur du
//! texte (`currentColor`).

use serde::Deserialize;
use serde_json::{json, Value};

/// Une ligne par icône : `nom<TAB>mots-clés,…<TAB>contenu de la balise <svg>`.
const DATA: &str = runtime::icon_set::LUCIDE;

/// Icônes rendues par appel, au plus.
const MAX_NAMES: usize = 24;
/// Résultats d'une recherche, au plus.
const MAX_RESULTS: usize = 16;

/// Mots courants d'un site (français, ou anglais absents des mots-clés de
/// Lucide), et le mot-clé de Lucide qui les trouve.
const FRENCH: &[(&str, &str)] = &[
    ("pain", "bakery"),
    ("bread", "bakery"),
    ("loaf", "bakery"),
    ("baguette", "bakery"),
    ("viennoiserie", "pastry"),
    ("croissant", "croissant"),
    ("boulangerie", "bakery"),
    ("patisserie", "pastry"),
    ("gateau", "cake"),
    ("cafe", "coffee"),
    ("the", "tea"),
    ("vin", "wine"),
    ("restaurant", "restaurant"),
    ("cuisine", "cooking"),
    ("telephone", "phone"),
    ("tel", "phone"),
    ("adresse", "location"),
    ("carte", "map"),
    ("lieu", "location"),
    ("courriel", "mail"),
    ("email", "mail"),
    ("horaire", "clock"),
    ("horaires", "clock"),
    ("heure", "clock"),
    ("calendrier", "calendar"),
    ("date", "calendar"),
    ("planning", "calendar"),
    ("etoile", "star"),
    ("avis", "star"),
    ("coeur", "heart"),
    ("fleur", "flower"),
    ("feuille", "leaf"),
    ("plante", "plant"),
    ("nature", "leaf"),
    ("soleil", "sun"),
    ("lune", "moon"),
    ("eau", "water"),
    ("vague", "waves"),
    ("montagne", "mountain"),
    ("maison", "home"),
    ("menu", "menu"),
    ("fermer", "close"),
    ("recherche", "search"),
    ("panier", "cart"),
    ("utilisateur", "user"),
    ("profil", "user"),
    ("equipe", "users"),
    ("prix", "currency"),
    ("tarif", "currency"),
    ("paiement", "payment"),
    ("cadeau", "gift"),
    ("livraison", "truck"),
    ("securite", "shield"),
    ("valider", "check"),
    ("fleche", "arrow"),
    ("musique", "music"),
    ("sport", "dumbbell"),
    ("velo", "bike"),
    ("voiture", "car"),
    ("enfant", "baby"),
    ("chien", "dog"),
    ("chat", "cat"),
    ("livre", "book"),
    ("ecole", "school"),
    ("sante", "health"),
    ("medecin", "stethoscope"),
    ("meditation", "zen"),
    ("yoga", "zen"),
    ("respiration", "wind"),
];

#[derive(Debug, Deserialize)]
pub(crate) struct IconInput {
    /// Une ou plusieurs icônes par leur nom (`map-pin`, `croissant`).
    #[serde(default)]
    names: Option<Vec<String>>,
    /// Un seul nom, pour les modèles qui l'écrivent ainsi.
    #[serde(default)]
    name: Option<String>,
    /// Mots à chercher dans les noms et les mots-clés (`bread pastry`).
    #[serde(default)]
    search: Option<String>,
    /// Côté en pixels (24 par défaut).
    #[serde(default)]
    size: Option<u32>,
    /// Épaisseur du trait (2 par défaut).
    #[serde(default)]
    stroke_width: Option<f32>,
    /// Texte pour un lecteur d'écran, quand l'icône porte un sens à elle seule.
    #[serde(default)]
    label: Option<String>,
    /// Couleur du trait (CSS : `#0f766e`, `rgb(…)`, `teal`) ; la couleur du texte sinon.
    #[serde(default)]
    color: Option<String>,
    /// `outline` (traits, par défaut), `duotone` (traits sur un aplat léger de la même
    /// couleur, ou de `fill`).
    #[serde(default)]
    style: Option<String>,
    /// Couleur de l'aplat d'une icône `duotone`.
    #[serde(default)]
    fill: Option<String>,
    /// `round` (par défaut) ou `sharp` : bouts et angles des traits.
    #[serde(default)]
    corners: Option<String>,
    /// Pastille derrière l'icône : sa couleur.
    #[serde(default)]
    background: Option<String>,
    /// Forme de la pastille : `circle` (par défaut), `rounded` ou `square`.
    #[serde(default)]
    shape: Option<String>,
    /// Rotation en degrés (une flèche vers la gauche faite d'une flèche vers la droite).
    #[serde(default)]
    rotate: Option<f32>,
    /// Les icônes en sprite : une seule balise `<svg>` cachée de `<symbol>` qui gardent
    /// leurs traits, et un `<use>` par icône.
    #[serde(default)]
    sprite: Option<bool>,
}

/// Allure d'une icône, réglée par l'appel.
#[derive(Debug, Clone, Default)]
struct Look {
    stroke_color: Option<String>,
    duotone: Option<String>,
    sharp: bool,
    badge: Option<(String, &'static str)>,
    rotate: Option<f32>,
}

/// Une couleur CSS sûre à mettre dans un attribut (`#0f766e`, `rgb(15 118 110)`, `teal`).
fn color(value: Option<&String>, field: &str) -> Result<Option<String>, String> {
    let Some(value) = value
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
    else {
        return Ok(None);
    };
    let safe = value.len() <= 40
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || "#(),.% -/".contains(ch));
    if safe {
        Ok(Some(value.to_string()))
    } else {
        Err(format!(
            "`{field}` must be a CSS color such as \"#0f766e\", \"rgb(15 118 110)\" or \"teal\""
        ))
    }
}

impl Look {
    fn of(input: &IconInput) -> Result<Self, String> {
        let stroke_color = color(input.color.as_ref(), "color")?;
        let duotone = match input.style.as_deref().map(str::trim) {
            None | Some("" | "outline" | "line") => None,
            Some("duotone") => Some(
                color(input.fill.as_ref(), "fill")?
                    .or_else(|| stroke_color.clone())
                    .unwrap_or_else(|| "currentColor".to_string()),
            ),
            Some(other) => {
                return Err(format!(
                    "`style` is \"outline\" or \"duotone\", not \"{other}\""
                ))
            }
        };
        let sharp = match input.corners.as_deref().map(str::trim) {
            None | Some("" | "round" | "rounded") => false,
            Some("sharp" | "square") => true,
            Some(other) => {
                return Err(format!(
                    "`corners` is \"round\" or \"sharp\", not \"{other}\""
                ))
            }
        };
        let badge = match color(input.background.as_ref(), "background")? {
            None => None,
            Some(background) => Some((
                background,
                match input.shape.as_deref().map(str::trim) {
                    None | Some("" | "circle") => "circle",
                    Some("rounded") => "rounded",
                    Some("square") => "square",
                    Some(other) => {
                        return Err(format!(
                            "`shape` is \"circle\", \"rounded\" or \"square\", not \"{other}\""
                        ))
                    }
                },
            )),
        };
        let rotate = input.rotate.filter(|angle| angle.abs() > f32::EPSILON);
        Ok(Self {
            stroke_color,
            duotone,
            sharp,
            badge,
            rotate,
        })
    }

    /// Attributs de trait, sur le `<svg>` ou le `<g>` qui porte le dessin.
    fn stroke_attributes(&self, stroke: &str) -> String {
        let (cap, join) = if self.sharp {
            ("square", "miter")
        } else {
            ("round", "round")
        };
        format!(
            "fill=\"none\" stroke=\"{}\" stroke-width=\"{stroke}\" stroke-linecap=\"{cap}\" stroke-linejoin=\"{join}\"",
            self.stroke_color.as_deref().unwrap_or("currentColor")
        )
    }

    /// Le dessin de l'icône : aplat, rotation, puis traits.
    fn drawing(&self, inner: &str, stroke: &str) -> String {
        let mut body = String::new();
        if let Some(tint) = &self.duotone {
            body.push_str(&format!(
                "<g fill=\"{tint}\" fill-opacity=\"0.2\" stroke=\"none\">{inner}</g>"
            ));
        }
        body.push_str(&format!(
            "<g {}>{inner}</g>",
            self.stroke_attributes(stroke)
        ));
        let body = match self.rotate {
            Some(angle) => format!("<g transform=\"rotate({angle} 12 12)\">{body}</g>"),
            None => body,
        };
        match &self.badge {
            Some((background, shape)) => {
                let radius = match *shape {
                    "circle" => "18",
                    "rounded" => "8",
                    _ => "0",
                };
                format!("<rect x=\"-6\" y=\"-6\" width=\"36\" height=\"36\" rx=\"{radius}\" fill=\"{background}\"/>{body}")
            }
            None => body,
        }
    }

    fn view_box(&self) -> &'static str {
        if self.badge.is_some() {
            "-6 -6 36 36"
        } else {
            "0 0 24 24"
        }
    }

    fn plain(&self) -> bool {
        self.stroke_color.is_none()
            && self.duotone.is_none()
            && !self.sharp
            && self.badge.is_none()
            && self.rotate.is_none()
    }
}

struct Icon<'a> {
    name: &'a str,
    tags: Vec<&'a str>,
    inner: &'a str,
}

fn icons() -> impl Iterator<Item = Icon<'static>> {
    DATA.lines().filter_map(|line| {
        let mut fields = line.splitn(3, '\t');
        let name = fields.next()?;
        let tags = fields
            .next()?
            .split(',')
            .filter(|tag| !tag.is_empty())
            .collect();
        let inner = fields.next()?;
        Some(Icon { name, tags, inner })
    })
}

/// Nom tel que Lucide l'écrit : minuscules, tirets.
fn normalize(name: &str) -> String {
    name.trim()
        .to_ascii_lowercase()
        .replace([' ', '_'], "-")
        .trim_start_matches("lucide-")
        .to_string()
}

/// Sans accents ni majuscules, pour lire « téléphone » comme « telephone ».
fn fold(word: &str) -> String {
    word.to_lowercase()
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

/// Mots de la recherche, avec leur équivalent anglais pour les mots français.
/// Un nom à tirets compte entier et par morceaux (`bread-loaf`, `bread`, `loaf`).
fn query_words(query: &str) -> Vec<String> {
    let mut words = Vec::new();
    for word in query
        .split(|ch: char| !ch.is_alphanumeric() && ch != '-')
        .filter(|word| word.len() >= 2)
    {
        let pieces = std::iter::once(word).chain(
            word.split('-')
                .filter(|piece| piece.len() >= 2 && *piece != word),
        );
        for piece in pieces {
            let folded = fold(piece);
            if let Some((_, english)) = FRENCH.iter().find(|(french, _)| *french == folded) {
                words.push((*english).to_string());
            }
            if !words.contains(&folded) {
                words.push(folded);
            }
        }
    }
    words
}

/// Pertinence d'une icône pour les mots cherchés ; 0 si aucun ne compte.
fn score(icon: &Icon<'_>, words: &[String]) -> u32 {
    let parts: Vec<&str> = icon.name.split('-').collect();
    words
        .iter()
        .map(|word| {
            if icon.name == word {
                6
            } else if parts.contains(&word.as_str()) {
                4
            } else if icon.tags.iter().any(|tag| tag == word) {
                3
            } else {
                // Un préfixe ne compte que pour un pluriel (« tree » pour
                // « trees ») : « bread » ne doit pas trouver « breadth ».
                u32::from(
                    icon.name.contains(word.as_str())
                        || icon.tags.iter().any(|tag| {
                            tag.starts_with(word.as_str()) && tag.len() <= word.len() + 2
                        }),
                )
            }
        })
        .sum()
}

fn search(query: &str) -> Vec<Value> {
    let words = query_words(query);
    let mut found: Vec<(u32, Icon<'static>)> = icons()
        .map(|icon| (score(&icon, &words), icon))
        .filter(|(points, _)| *points > 0)
        .collect();
    found.sort_by(|a, b| {
        b.0.cmp(&a.0)
            .then_with(|| a.1.name.len().cmp(&b.1.name.len()))
    });
    found
        .into_iter()
        .take(MAX_RESULTS)
        .map(|(_, icon)| json!({ "name": icon.name, "tags": icon.tags }))
        .collect()
}

/// Balise `<svg>` complète d'une icône, prête à coller.
fn markup(icon: &Icon<'_>, size: u32, stroke: f32, label: Option<&str>) -> String {
    styled(icon, size, stroke, label, &Look::default())
}

fn access(label: Option<&str>) -> String {
    match label.map(str::trim).filter(|label| !label.is_empty()) {
        Some(label) => {
            let label = label
                .replace('&', "&amp;")
                .replace('"', "&quot;")
                .replace('<', "&lt;");
            format!("role=\"img\" aria-label=\"{label}\"")
        }
        None => "aria-hidden=\"true\" focusable=\"false\"".to_string(),
    }
}

fn stroke_text(stroke: f32) -> String {
    if (stroke.fract()).abs() < f32::EPSILON {
        format!("{stroke:.0}")
    } else {
        format!("{stroke}")
    }
}

/// Une icône avec l'allure demandée ; telle que Lucide la dessine sans réglage.
fn styled(icon: &Icon<'_>, size: u32, stroke: f32, label: Option<&str>, look: &Look) -> String {
    if !look.plain() {
        return format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" class=\"icon icon-{name}\" width=\"{size}\" height=\"{size}\" viewBox=\"{view}\" {access}>{body}</svg>",
            name = icon.name,
            view = look.view_box(),
            access = access(label),
            body = look.drawing(icon.inner, &stroke_text(stroke)),
        );
    }
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" class=\"icon icon-{name}\" width=\"{size}\" height=\"{size}\" viewBox=\"0 0 24 24\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"{stroke}\" stroke-linecap=\"round\" stroke-linejoin=\"round\" {access}>{inner}</svg>",
        name = icon.name,
        stroke = stroke_text(stroke),
        access = access(label),
        inner = icon.inner
    )
}

/// Le SVG d'une icône par son nom, pour les composants de l'outil `Component`.
pub(crate) fn svg(name: &str, size: u32) -> Option<String> {
    icons()
        .find(|icon| icon.name == name)
        .map(|icon| markup(&icon, size, 2.0, None))
}

/// Sprite des icônes : une balise `<svg>` cachée, un `<symbol>` par icône dont le `<g>`
/// garde les attributs de trait (sans eux, une icône de sprite se dessine en tache noire :
/// vu le 24 octobre 2026, session de test de Claw Web).
fn sprite(found: &[Icon<'_>], stroke: f32, look: &Look) -> String {
    let symbols: String = found
        .iter()
        .map(|icon| {
            format!(
                "<symbol id=\"icon-{}\" viewBox=\"{}\">{}</symbol>",
                icon.name,
                look.view_box(),
                look.drawing(icon.inner, &stroke_text(stroke))
            )
        })
        .collect();
    format!("<svg xmlns=\"http://www.w3.org/2000/svg\" style=\"display:none\">{symbols}</svg>")
}

/// Ce que l'outil `Icon` rend au modèle.
pub(crate) fn icon(input: &IconInput) -> Result<Value, String> {
    let size = input.size.unwrap_or(24).clamp(12, 160);
    let stroke = input.stroke_width.unwrap_or(2.0).clamp(0.5, 3.0);
    let look = Look::of(input)?;
    let as_sprite = input.sprite.unwrap_or(false);
    let mut names: Vec<String> = input.names.clone().unwrap_or_default();
    if let Some(name) = &input.name {
        names.push(name.clone());
    }
    let names: Vec<String> = names
        .iter()
        .map(|name| normalize(name))
        .filter(|name| !name.is_empty())
        .take(MAX_NAMES)
        .collect();
    let query = input.search.as_deref().map(str::trim).unwrap_or_default();
    if names.is_empty() && query.is_empty() {
        return Err("give `names` (for example [\"map-pin\", \"phone\"]) or `search` (for example \"bread pastry\")".to_string());
    }
    let mut result = json!({});
    if !names.is_empty() {
        let mut found = Vec::new();
        let mut drawn = Vec::new();
        let mut missing = Vec::new();
        for name in &names {
            match icons().find(|icon| icon.name == name) {
                Some(icon) => {
                    found.push(if as_sprite {
                        json!({
                            "name": icon.name,
                            "use": format!("<svg class=\"icon icon-{0}\" width=\"{size}\" height=\"{size}\" {1}><use href=\"#icon-{0}\"></use></svg>", icon.name, access(input.label.as_deref())),
                        })
                    } else {
                        json!({
                            "name": icon.name,
                            "svg": styled(&icon, size, stroke, input.label.as_deref(), &look),
                        })
                    });
                    drawn.push(icon);
                }
                None => missing.push(json!({ "name": name, "closest": search(name).into_iter().take(5).collect::<Vec<_>>() })),
            }
        }
        if as_sprite && !drawn.is_empty() {
            result["sprite"] = json!(sprite(&drawn, stroke, &look));
        }
        result["icons"] = json!(found);
        if !missing.is_empty() {
            result["unknown"] = json!(missing);
        }
    }
    if !query.is_empty() {
        let matches = search(query);
        if matches.is_empty() {
            result["matches"] = json!([]);
            result["hint"] = json!("nothing matches: search in English with simple words (bread, phone, location, calendar, leaf)");
        } else {
            result["matches"] = json!(matches);
        }
    }
    result["note"] = json!(if as_sprite {
        "Lucide icons (ISC license) as a sprite: paste `sprite` once, right after <body>, then each icon's `use` where it goes (size it with CSS width and height; without `color` it takes the text color). Keep the sprite as given: its <g> carries the stroke attributes, without them the lines are drawn as black blobs. Credit the set once in the page source, for example <!-- Icons: Lucide, ISC license -->."
    } else {
        "Lucide icons (ISC license). Paste each svg as is: without `color` it takes the text color (currentColor), so color and size it with CSS (color, width, height). Customize the look in the call: `color`, `style` \"duotone\" (with `fill`), `corners` \"sharp\", `background` (with `shape` circle, rounded or square), `rotate`, `stroke_width`, `size`; `sprite`: true gives one hidden sprite and a <use> per icon, for icons used many times. Keep aria-hidden on a decorative icon; give `label` when the icon alone carries meaning. Credit the set once in the page source, for example <!-- Icons: Lucide, ISC license -->."
    });
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::{icon, icons, query_words, IconInput};
    use serde_json::json;

    fn request(value: serde_json::Value) -> IconInput {
        serde_json::from_value(value).expect("entrée")
    }

    #[test]
    fn the_icon_set_is_complete_and_icons_come_ready_to_paste() {
        assert_eq!(icons().count(), 1539);
        let result = icon(&request(
            json!({ "names": ["map-pin", "Phone", "croissant"], "size": 20 }),
        ))
        .expect("icônes");
        let svgs: Vec<&str> = result["icons"]
            .as_array()
            .expect("liste")
            .iter()
            .map(|entry| entry["svg"].as_str().expect("svg"))
            .collect();
        assert_eq!(svgs.len(), 3);
        assert!(
            svgs[0].starts_with("<svg xmlns=\"http://www.w3.org/2000/svg\" class=\"icon icon-map-pin\" width=\"20\" height=\"20\" viewBox=\"0 0 24 24\"")
                && svgs[0].contains("stroke=\"currentColor\" stroke-width=\"2\"")
                && svgs[0].contains("aria-hidden=\"true\"")
                && svgs[0].ends_with("</svg>"),
            "{}",
            svgs[0]
        );
        // Une icône qui porte un sens : nommée pour un lecteur d'écran.
        let labelled = icon(&request(
            json!({ "name": "phone", "label": "Appeler \"La Mie\"" }),
        ))
        .expect("icône");
        assert!(labelled["icons"][0]["svg"]
            .as_str()
            .expect("svg")
            .contains("role=\"img\" aria-label=\"Appeler &quot;La Mie&quot;\""));
    }

    #[test]
    fn icons_take_the_color_style_corners_badge_and_rotation_asked() {
        let result = icon(&request(json!({
            "names": ["wallet"], "color": "#0f766e", "style": "duotone", "corners": "sharp",
            "background": "#ccfbf1", "shape": "rounded", "rotate": 90
        })))
        .expect("icône");
        let svg = result["icons"][0]["svg"].as_str().expect("svg");
        assert!(svg.contains("viewBox=\"-6 -6 36 36\""), "{svg}");
        assert!(
            svg.contains(
                "<rect x=\"-6\" y=\"-6\" width=\"36\" height=\"36\" rx=\"8\" fill=\"#ccfbf1\"/>"
            ),
            "{svg}"
        );
        assert!(
            svg.contains("<g fill=\"#0f766e\" fill-opacity=\"0.2\" stroke=\"none\">"),
            "{svg}"
        );
        assert!(svg.contains("stroke=\"#0f766e\" stroke-width=\"2\" stroke-linecap=\"square\" stroke-linejoin=\"miter\""), "{svg}");
        assert!(svg.contains("transform=\"rotate(90 12 12)\""), "{svg}");
        // Une couleur qui ouvrirait l'attribut est refusée.
        assert!(icon(&request(
            json!({ "names": ["wallet"], "color": "red\" onload=\"x" })
        ))
        .is_err());
        assert!(icon(&request(json!({ "names": ["wallet"], "style": "3d" }))).is_err());
    }

    #[test]
    fn a_sprite_keeps_the_stroke_of_its_symbols() {
        let result = icon(&request(
            json!({ "names": ["plus", "trash-2"], "sprite": true, "size": 20 }),
        ))
        .expect("sprite");
        let sprite = result["sprite"].as_str().expect("sprite");
        assert!(sprite.starts_with("<svg xmlns=\"http://www.w3.org/2000/svg\" style=\"display:none\"><symbol id=\"icon-plus\" viewBox=\"0 0 24 24\"><g fill=\"none\" stroke=\"currentColor\""), "{sprite}");
        assert_eq!(sprite.matches("<symbol").count(), 2);
        assert_eq!(
            result["icons"][1]["use"],
            json!("<svg class=\"icon icon-trash-2\" width=\"20\" height=\"20\" aria-hidden=\"true\" focusable=\"false\"><use href=\"#icon-trash-2\"></use></svg>")
        );
    }

    #[test]
    fn unknown_names_get_their_closest_icons_and_french_words_are_searched_in_english() {
        let result = icon(&request(json!({ "names": ["bread-loaf"] }))).expect("réponse");
        assert!(result["icons"].as_array().expect("liste").is_empty());
        let closest = result["unknown"][0]["closest"].to_string();
        assert!(
            closest.contains("\"croissant\"") || closest.contains("\"sandwich\""),
            "{closest}"
        );
        assert!(query_words("Téléphone").contains(&"phone".to_string()));
        let result = icon(&request(json!({ "search": "téléphone adresse" }))).expect("recherche");
        let names = result["matches"].to_string();
        assert!(
            names.contains("\"phone\"") && names.contains("\"map-pin\""),
            "{names}"
        );
        assert!(icon(&request(json!({}))).is_err());
    }
}
