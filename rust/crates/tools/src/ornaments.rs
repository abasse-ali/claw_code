//! Outil `Ornament` : des éléments décoratifs SVG calculés à partir de la
//! palette du plan (formes organiques, vagues en couches, motifs, grain,
//! arches, médaillons, rameaux, rayons). Un petit modèle dessine mal en SVG :
//! vu dans les essais réels, ses illustrations faites main (des arches en
//! forme de cuillère, des avatars en goutte, une vague coupée) laissaient
//! l'identité et la finition sous la barre. Ces ornements sont tracés par
//! des formules : leur rendu est propre, et ils restent du SVG.

use serde::Deserialize;
use serde_json::{json, Value};
use std::fmt::Write as _;

/// Les sortes d'ornements, avec leur usage, pour l'aide et les erreurs.
const KINDS: &[(&str, &str)] = &[
    (
        "blob",
        "an organic shape, one to three layers: behind a title, an image or a card",
    ),
    (
        "waves",
        "layered waves across the full width: the bottom edge of a hero, between two sections",
    ),
    ("dots", "a grid of dots: a quiet background texture"),
    (
        "lines",
        "parallel lines at an angle: a quiet background texture",
    ),
    (
        "topography",
        "contour lines around a center: an elegant background for a hero or a band",
    ),
    (
        "grain",
        "film grain to lay over a flat color: depth without noise",
    ),
    (
        "arch",
        "one to three arches: a frame for an image or a text, calm and architectural",
    ),
    (
        "frame",
        "a round medallion with an inner dashed ring: around an avatar, an initial or an icon",
    ),
    (
        "sprig",
        "a branch with alternating leaves: a botanical accent",
    ),
    ("rays", "a sun with rays: light, energy, morning"),
];

#[derive(Debug, Deserialize)]
pub(crate) struct OrnamentInput {
    kind: String,
    /// Couleurs du plan, en hexadécimal (`#c46b3a`).
    #[serde(default)]
    colors: Option<Vec<String>>,
    #[serde(default)]
    width: Option<u32>,
    #[serde(default)]
    height: Option<u32>,
    /// Graine : une autre valeur donne une autre forme de la même sorte.
    #[serde(default)]
    seed: Option<u32>,
    /// Nombre de couches, d'anneaux, de feuilles, d'arches ou de rayons.
    #[serde(default)]
    count: Option<u32>,
    /// Opacité générale, de 0.05 à 1.
    #[serde(default)]
    opacity: Option<f32>,
}

/// Générateur pseudo-aléatoire reproductible (xorshift).
struct Rng(u32);

impl Rng {
    fn new(seed: u32) -> Self {
        Self(seed.wrapping_mul(2_654_435_761).max(1))
    }

    fn next(&mut self) -> f32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        // 24 bits suffisent : la conversion en f32 reste exacte.
        f32::from(u16::try_from(x >> 16).unwrap_or(0)) / 65_535.0
    }

    fn between(&mut self, low: f32, high: f32) -> f32 {
        low + (high - low) * self.next()
    }
}

/// Une couleur sûre à écrire dans un attribut : `#abc` ou `#aabbcc`.
fn hex_color(text: &str) -> Option<String> {
    let text = text.trim();
    let digits = text.strip_prefix('#')?;
    (matches!(digits.len(), 3 | 6) && digits.chars().all(|ch| ch.is_ascii_hexdigit()))
        .then(|| format!("#{}", digits.to_ascii_lowercase()))
}

fn number(value: f32) -> String {
    let rounded = (value * 10.0).round() / 10.0;
    if (rounded.fract()).abs() < f32::EPSILON {
        format!("{rounded:.0}")
    } else {
        format!("{rounded:.1}")
    }
}

/// Chemin fermé et lisse par une suite de points (Catmull-Rom vers Bézier).
fn smooth_closed(points: &[(f32, f32)]) -> String {
    let n = points.len();
    let mut path = format!("M{} {}", number(points[0].0), number(points[0].1));
    for i in 0..n {
        let p0 = points[(i + n - 1) % n];
        let p1 = points[i];
        let p2 = points[(i + 1) % n];
        let p3 = points[(i + 2) % n];
        let c1 = (p1.0 + (p2.0 - p0.0) / 6.0, p1.1 + (p2.1 - p0.1) / 6.0);
        let c2 = (p2.0 - (p3.0 - p1.0) / 6.0, p2.1 - (p3.1 - p1.1) / 6.0);
        let _ = write!(
            path,
            " C{} {} {} {} {} {}",
            number(c1.0),
            number(c1.1),
            number(c2.0),
            number(c2.1),
            number(p2.0),
            number(p2.1)
        );
    }
    path.push('Z');
    path
}

/// Une boucle irrégulière autour de `(cx, cy)`, de rayon moyen `radius`.
fn wobbly_ring(rng: &mut Rng, cx: f32, cy: f32, radius: f32, points: usize, wobble: f32) -> String {
    let outline: Vec<(f32, f32)> = (0..points)
        .map(|i| {
            #[allow(clippy::cast_precision_loss)]
            let angle = std::f32::consts::TAU * i as f32 / points as f32;
            let r = radius * rng.between(1.0 - wobble, 1.0 + wobble);
            (cx + r * angle.cos(), cy + r * angle.sin())
        })
        .collect();
    smooth_closed(&outline)
}

fn blob(rng: &mut Rng, colors: &[String], layers: u32) -> (String, (u32, u32)) {
    let mut body = String::new();
    for layer in 0..layers.clamp(1, 3) {
        // Couches décalées et de taille décroissante : des galets posés l'un
        // sur l'autre, pas des anneaux centrés.
        #[allow(clippy::cast_precision_loss)]
        let scale = 1.0 - 0.24 * layer as f32;
        let (dx, dy) = if layer == 0 {
            (0.0, 0.0)
        } else {
            (rng.between(-16.0, 16.0), rng.between(-16.0, 16.0))
        };
        let points = 7 + usize::try_from(layer).unwrap_or(0) % 2;
        let path = wobbly_ring(rng, 100.0 + dx, 100.0 + dy, 84.0 * scale, points, 0.11);
        let color = &colors[usize::try_from(layer).unwrap_or(0) % colors.len()];
        let _ = write!(body, "<path d=\"{path}\" fill=\"{color}\"/>");
    }
    (body, (200, 200))
}

fn waves(rng: &mut Rng, colors: &[String], layers: u32, height: u32) -> (String, (u32, u32)) {
    let width = 1440.0;
    #[allow(clippy::cast_precision_loss)]
    let h = height as f32;
    let layers = layers.clamp(1, 4);
    let mut body = String::new();
    for layer in 0..layers {
        #[allow(clippy::cast_precision_loss)]
        let base = h * (0.25 + 0.5 * layer as f32 / layers as f32);
        let amplitude = h * rng.between(0.08, 0.16);
        let frequency = rng.between(1.0, 2.2);
        let phase = rng.between(0.0, std::f32::consts::TAU);
        let mut path = String::from("M0 ");
        path.push_str(&number(h));
        for step in 0..=24 {
            #[allow(clippy::cast_precision_loss)]
            let x = width * step as f32 / 24.0;
            let y = base
                + amplitude * (frequency * std::f32::consts::TAU * x / width + phase).sin()
                + amplitude * 0.35 * (2.7 * std::f32::consts::TAU * x / width + phase * 1.7).sin();
            let _ = write!(path, " L{} {}", number(x), number(y));
        }
        let _ = write!(path, " L{} {} Z", number(width), number(h));
        let color = &colors[usize::try_from(layer).unwrap_or(0) % colors.len()];
        #[allow(clippy::cast_precision_loss)]
        let alpha = 0.55 + 0.45 * (layer + 1) as f32 / layers as f32;
        let _ = write!(
            body,
            "<path d=\"{path}\" fill=\"{color}\" fill-opacity=\"{}\"/>",
            number(alpha)
        );
    }
    (body, (1440, height))
}

fn dots(id: &str, colors: &[String], size: (u32, u32), gap: u32) -> (String, (u32, u32)) {
    let gap = gap.clamp(8, 64);
    #[allow(clippy::cast_precision_loss)]
    let radius = (gap as f32 * 0.09).max(1.0);
    (
        format!(
            "<defs><pattern id=\"{id}\" width=\"{gap}\" height=\"{gap}\" patternUnits=\"userSpaceOnUse\"><circle cx=\"{half}\" cy=\"{half}\" r=\"{r}\" fill=\"{color}\"/></pattern></defs><rect width=\"100%\" height=\"100%\" fill=\"url(#{id})\"/>",
            half = gap / 2,
            r = number(radius),
            color = colors[0]
        ),
        size,
    )
}

fn lines(
    id: &str,
    rng: &mut Rng,
    colors: &[String],
    size: (u32, u32),
    gap: u32,
) -> (String, (u32, u32)) {
    let gap = gap.clamp(6, 48);
    let angle =
        [30, 45, 60, -45][usize::from(rng.next() > 0.5) * 2 + usize::from(rng.next() > 0.5)];
    (
        format!(
            "<defs><pattern id=\"{id}\" width=\"{gap}\" height=\"{gap}\" patternUnits=\"userSpaceOnUse\" patternTransform=\"rotate({angle})\"><line x1=\"0\" y1=\"0\" x2=\"0\" y2=\"{gap}\" stroke=\"{color}\" stroke-width=\"1\"/></pattern></defs><rect width=\"100%\" height=\"100%\" fill=\"url(#{id})\"/>",
            color = colors[0]
        ),
        size,
    )
}

fn topography(
    rng: &mut Rng,
    colors: &[String],
    rings: u32,
    size: (u32, u32),
) -> (String, (u32, u32)) {
    #[allow(clippy::cast_precision_loss)]
    let (w, h) = (size.0 as f32, size.1 as f32);
    let (cx, cy) = (w * rng.between(0.35, 0.65), h * rng.between(0.35, 0.65));
    let rings = rings.clamp(4, 14);
    let mut body = String::new();
    for ring in 1..=rings {
        #[allow(clippy::cast_precision_loss)]
        let radius = w.max(h) * 0.07 * ring as f32;
        let path = wobbly_ring(rng, cx, cy, radius, 9, 0.12);
        let color = &colors[usize::try_from(ring).unwrap_or(0) % colors.len()];
        let _ = write!(
            body,
            "<path d=\"{path}\" fill=\"none\" stroke=\"{color}\" stroke-width=\"1.2\"/>"
        );
    }
    (body, size)
}

fn grain(id: &str, size: (u32, u32)) -> (String, (u32, u32)) {
    (
        format!(
            "<filter id=\"{id}\"><feTurbulence type=\"fractalNoise\" baseFrequency=\"0.85\" numOctaves=\"3\" stitchTiles=\"stitch\"/><feColorMatrix type=\"saturate\" values=\"0\"/></filter><rect width=\"100%\" height=\"100%\" filter=\"url(#{id})\" opacity=\"0.12\"/>"
        ),
        size,
    )
}

fn arch(colors: &[String], count: u32) -> (String, (u32, u32)) {
    let count = count.clamp(1, 3);
    let mut body = String::new();
    for index in 0..count {
        let x = 20 + index * 200;
        let fill = &colors[usize::try_from(index).unwrap_or(0) % colors.len()];
        let line = &colors[(usize::try_from(index).unwrap_or(0) + 1) % colors.len()];
        let _ = write!(
            body,
            "<path d=\"M{x} 260 V100 A80 80 0 0 1 {right} 100 V260 Z\" fill=\"{fill}\"/><path d=\"M{ix} 260 V104 A66 66 0 0 1 {iright} 104 V260\" fill=\"none\" stroke=\"{line}\" stroke-width=\"2\"/>",
            right = x + 160,
            ix = x + 14,
            iright = x + 146
        );
    }
    (body, (count * 200, 270))
}

fn frame(colors: &[String]) -> (String, (u32, u32)) {
    let outer = &colors[0];
    let inner = &colors[1 % colors.len()];
    let mut body = format!(
        "<circle cx=\"100\" cy=\"100\" r=\"92\" fill=\"none\" stroke=\"{outer}\" stroke-width=\"3\"/><circle cx=\"100\" cy=\"100\" r=\"80\" fill=\"none\" stroke=\"{inner}\" stroke-width=\"1.5\" stroke-dasharray=\"2 6\" stroke-linecap=\"round\"/>"
    );
    for (cx, cy) in [(100, 8), (192, 100), (100, 192), (8, 100)] {
        let _ = write!(
            body,
            "<circle cx=\"{cx}\" cy=\"{cy}\" r=\"4\" fill=\"{outer}\"/>"
        );
    }
    (body, (200, 200))
}

fn sprig(rng: &mut Rng, colors: &[String], leaves: u32) -> (String, (u32, u32)) {
    let stem = &colors[1 % colors.len()];
    let leaf = &colors[0];
    let bend = rng.between(-26.0, 26.0);
    let mut body = format!(
        "<path d=\"M60 236 Q{} 120 60 12\" fill=\"none\" stroke=\"{stem}\" stroke-width=\"2.5\" stroke-linecap=\"round\"/>",
        number(60.0 + bend)
    );
    let leaves = leaves.clamp(3, 9);
    for index in 0..leaves {
        #[allow(clippy::cast_precision_loss)]
        let t = 0.15 + 0.75 * index as f32 / leaves as f32;
        // Point de la courbe quadratique, et côté alterné.
        let x = (1.0 - t) * (1.0 - t) * 60.0 + 2.0 * (1.0 - t) * t * (60.0 + bend) + t * t * 60.0;
        let y = (1.0 - t) * (1.0 - t) * 236.0 + 2.0 * (1.0 - t) * t * 120.0 + t * t * 12.0;
        let side = if index % 2 == 0 { 1.0 } else { -1.0 };
        let length = rng.between(26.0, 36.0) * (1.0 - t * 0.35);
        let angle = side * rng.between(35.0, 55.0) - 90.0;
        let _ = write!(
            body,
            "<path d=\"M0 0 Q{h} {w} {l} 0 Q{h} -{w} 0 0Z\" fill=\"{leaf}\" fill-opacity=\"{op}\" transform=\"translate({x} {y}) rotate({a})\"/>",
            h = number(length / 2.0),
            w = number(length * 0.38),
            l = number(length),
            op = number(rng.between(0.75, 1.0)),
            x = number(x),
            y = number(y),
            a = number(angle)
        );
    }
    (body, (120, 248))
}

fn rays(colors: &[String], count: u32) -> (String, (u32, u32)) {
    let sun = &colors[0];
    let ray = &colors[1 % colors.len()];
    let count = count.clamp(8, 36);
    let mut body = format!("<circle cx=\"100\" cy=\"100\" r=\"34\" fill=\"{sun}\"/>");
    for index in 0..count {
        #[allow(clippy::cast_precision_loss)]
        let angle = std::f32::consts::TAU * index as f32 / count as f32;
        let (inner, outer) = if index % 2 == 0 {
            (46.0, 92.0)
        } else {
            (48.0, 76.0)
        };
        let _ = write!(
            body,
            "<line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"{ray}\" stroke-width=\"2\" stroke-linecap=\"round\"/>",
            number(100.0 + inner * angle.cos()),
            number(100.0 + inner * angle.sin()),
            number(100.0 + outer * angle.cos()),
            number(100.0 + outer * angle.sin())
        );
    }
    (body, (200, 200))
}

/// SVG encodé pour un `background-image: url("data:image/svg+xml,…")`.
fn data_uri(svg: &str) -> String {
    let mut out = String::from("data:image/svg+xml,");
    for ch in svg.chars() {
        match ch {
            '"' => out.push('\''),
            '<' => out.push_str("%3C"),
            '>' => out.push_str("%3E"),
            '#' => out.push_str("%23"),
            '%' => out.push_str("%25"),
            '\n' => {}
            other => out.push(other),
        }
    }
    out
}

/// Ce que l'outil `Ornament` rend au modèle.
pub(crate) fn ornament(input: &OrnamentInput) -> Result<Value, String> {
    let kind = input.kind.trim().to_ascii_lowercase();
    let Some((_, usage)) = KINDS.iter().find(|(name, _)| *name == kind) else {
        let known: Vec<String> = KINDS
            .iter()
            .map(|(name, usage)| format!("{name} ({usage})"))
            .collect();
        return Err(format!(
            "unknown kind `{}`: use one of {}",
            input.kind,
            known.join("; ")
        ));
    };
    let mut colors: Vec<String> = input
        .colors
        .iter()
        .flatten()
        .filter_map(|color| hex_color(color))
        .collect();
    if colors.is_empty() {
        colors = vec![
            "#c46b3a".to_string(),
            "#e8d5c4".to_string(),
            "#6b8e7a".to_string(),
        ];
    }
    let seed = input.seed.unwrap_or_else(|| {
        kind.bytes().fold(7_u32, |acc, byte| {
            acc.wrapping_mul(31).wrapping_add(u32::from(byte))
        })
    });
    let mut rng = Rng::new(seed);
    let id = format!("ornament-{kind}-{seed}");
    let size = (
        input.width.unwrap_or(400).clamp(16, 2400),
        input.height.unwrap_or(300).clamp(16, 2400),
    );
    let (body, view) = match kind.as_str() {
        "blob" => blob(&mut rng, &colors, input.count.unwrap_or(2)),
        "waves" => waves(
            &mut rng,
            &colors,
            input.count.unwrap_or(3),
            input.height.unwrap_or(160).clamp(40, 600),
        ),
        "dots" => dots(&id, &colors, size, input.count.unwrap_or(20)),
        "lines" => lines(&id, &mut rng, &colors, size, input.count.unwrap_or(14)),
        "topography" => topography(&mut rng, &colors, input.count.unwrap_or(8), size),
        "grain" => grain(&id, size),
        "arch" => arch(&colors, input.count.unwrap_or(1)),
        "frame" => frame(&colors),
        "sprig" => sprig(&mut rng, &colors, input.count.unwrap_or(6)),
        _ => rays(&colors, input.count.unwrap_or(16)),
    };
    let opacity = input.opacity.unwrap_or(1.0).clamp(0.05, 1.0);
    let opacity_attribute = if opacity < 1.0 {
        format!(" opacity=\"{}\"", number(opacity))
    } else {
        String::new()
    };
    // Une vague s'étire sur toute la largeur ; le reste garde ses proportions.
    let aspect = if kind == "waves" {
        " preserveAspectRatio=\"none\""
    } else {
        ""
    };
    let (width, height) = match kind.as_str() {
        "waves" => (view.0, view.1),
        "dots" | "lines" | "topography" | "grain" => size,
        _ => (
            input.width.unwrap_or(view.0),
            input.height.unwrap_or(view.1),
        ),
    };
    let svg = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" class=\"ornament ornament-{kind}\" width=\"{width}\" height=\"{height}\" viewBox=\"0 0 {} {}\"{aspect}{opacity_attribute} aria-hidden=\"true\" focusable=\"false\">{body}</svg>",
        view.0, view.1
    );
    let placement = match kind.as_str() {
        "waves" => "Place it at the bottom of the section, full width: .ornament-waves { display: block; width: 100%; height: 120px; } (the waves stretch to the width).",
        "dots" | "lines" | "grain" | "topography" => "Use it as a background: absolutely positioned behind the content (position: absolute; inset: 0; z-index: -1; width: 100%; height: 100%), or as background-image with the data_uri.",
        "frame" => "Put the avatar, the initial or an Icon inside: a wrapper with position: relative, the frame absolutely positioned over it.",
        _ => "Size it with CSS (width, height: auto), and keep it decorative: aria-hidden stays.",
    };
    Ok(json!({
        "kind": kind,
        "use": usage,
        "seed": seed,
        "svg": svg,
        "data_uri": data_uri(&svg),
        "note": format!("Paste the svg as is; a different seed gives another shape of the same kind. {placement} Give it the plan's colors, and keep one ornament family per page so they read as one idea."),
    }))
}

#[cfg(test)]
mod tests {
    use super::{hex_color, ornament, OrnamentInput, KINDS};
    use serde_json::json;

    fn make(value: serde_json::Value) -> serde_json::Value {
        let input: OrnamentInput = serde_json::from_value(value).expect("entrée");
        ornament(&input).expect("ornement")
    }

    #[test]
    fn every_kind_draws_a_well_formed_decorative_svg() {
        for (kind, _) in KINDS {
            let result = make(json!({ "kind": kind, "colors": ["#C46B3A", "#e8d5c4", "#6b8e7a"] }));
            let svg = result["svg"].as_str().expect("svg");
            assert!(
                svg.starts_with("<svg xmlns=\"http://www.w3.org/2000/svg\"")
                    && svg.contains("aria-hidden=\"true\"")
                    && svg.contains("viewBox=\"0 0 ")
                    && svg.ends_with("</svg>"),
                "{kind}: {svg}"
            );
            // Imbrication correcte, comme la vérifie Claw à l'écriture.
            assert_eq!(runtime::harness::html_structure(svg), None, "{kind}: {svg}");
            assert!(
                !svg.contains("NaN") && !svg.contains("inf"),
                "{kind}: {svg}"
            );
            assert!(result["data_uri"]
                .as_str()
                .expect("uri")
                .starts_with("data:image/svg+xml,%3Csvg"));
        }
    }

    #[test]
    fn shapes_follow_the_seed_and_colors_are_checked() {
        let one = make(json!({ "kind": "blob", "seed": 3 }));
        let again = make(json!({ "kind": "blob", "seed": 3 }));
        let other = make(json!({ "kind": "blob", "seed": 4 }));
        assert_eq!(one["svg"], again["svg"]);
        assert_ne!(one["svg"], other["svg"]);
        // Une couleur qui n'est pas de l'hexadécimal n'entre pas dans le SVG.
        let safe = make(json!({ "kind": "rays", "colors": ["red\" onload=\"x", "#abc"] }));
        let svg = safe["svg"].as_str().expect("svg");
        assert!(!svg.contains("onload") && svg.contains("#abc"), "{svg}");
        assert_eq!(hex_color(" #A1B2C3 ").as_deref(), Some("#a1b2c3"));
        assert_eq!(hex_color("#12"), None);
        let input: OrnamentInput =
            serde_json::from_value(json!({ "kind": "dragon" })).expect("entrée");
        assert!(ornament(&input).expect_err("inconnu").contains("blob ("));
        // Une vague s'étire sur toute la largeur.
        let waves = make(json!({ "kind": "waves", "height": 120 }));
        assert!(waves["svg"]
            .as_str()
            .expect("svg")
            .contains("preserveAspectRatio=\"none\""));
    }
}
