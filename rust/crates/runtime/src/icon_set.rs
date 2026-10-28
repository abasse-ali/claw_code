//! Les icônes Lucide (licence ISC, `icons/LUCIDE_LICENSE`) : leurs données,
//! pour l'outil `Icon`, et leur empreinte, pour reconnaître une icône dessinée
//! à la main dans une page. Un petit modèle dessine mal en SVG : ses flèches,
//! coches et téléphones faits main sont tordus et inégaux, et le juge le voit.
//! Une icône de la page est comparée aux 1 539 icônes par la géométrie de ses
//! formes (chemins, cercles, rectangles, lignes), quelle que soit la façon
//! dont le code est écrit : une icône recopiée de l'outil passe, une icône
//! inventée est signalée.

use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, OnceLock};

/// Données de Lucide : nom, mots-clés, formes de l'icône (une par ligne).
pub const LUCIDE: &str = include_str!("icons/lucide.tsv");

/// Données de Feather (licence MIT, `icons/FEATHER_LICENSE`), l'ancêtre de
/// Lucide : nom, formes. Les modèles recopient ses icônes de mémoire ; elles
/// sont aussi soignées que celles de Lucide, et ne sont pas des dessins.
const FEATHER: &str = include_str!("icons/feather.tsv");

/// Plus grand côté de la `viewBox` d'une icône (Lucide dessine sur 24, les
/// autres jeux sur 16 à 32) ; au-delà, c'est une illustration.
const ICON_BOX: f64 = 48.0;

/// Une icône dessinée à la main : sa ligne, de quoi la désigner, et les mots
/// (classe, étiquette, titre) qui disent ce qu'elle représente.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DrawnIcon {
    pub line: usize,
    pub label: String,
    pub words: Vec<String>,
}

/// Les attributs d'une balise ouvrante (`<path d="…" />`), noms en
/// minuscules ; les valeurs JSX entre accolades sont lues sans les accolades.
fn attributes(tag: &str) -> Vec<(String, String)> {
    let mut found = Vec::new();
    let bytes = tag.as_bytes();
    let mut at = 0;
    // Le nom de la balise d'abord.
    while at < bytes.len() && !bytes[at].is_ascii_whitespace() {
        at += 1;
    }
    while at < bytes.len() {
        while at < bytes.len() && (bytes[at].is_ascii_whitespace() || bytes[at] == b'/') {
            at += 1;
        }
        let start = at;
        while at < bytes.len()
            && !bytes[at].is_ascii_whitespace()
            && !matches!(bytes[at], b'=' | b'/' | b'>')
        {
            at += 1;
        }
        if start == at {
            at += 1;
            continue;
        }
        let name = tag[start..at].to_ascii_lowercase();
        while at < bytes.len() && bytes[at].is_ascii_whitespace() {
            at += 1;
        }
        if at >= bytes.len() || bytes[at] != b'=' {
            found.push((name, String::new()));
            continue;
        }
        at += 1;
        while at < bytes.len() && bytes[at].is_ascii_whitespace() {
            at += 1;
        }
        let value = match bytes.get(at) {
            Some(&quote @ (b'"' | b'\'')) => {
                let end = tag[at + 1..]
                    .find(char::from(quote))
                    .map_or(tag.len(), |end| at + 1 + end);
                let value = &tag[at + 1..end.min(tag.len())];
                at = end + 1;
                value
            }
            Some(b'{') => {
                let end = tag[at..].find('}').map_or(tag.len(), |end| at + end);
                let value = tag[at + 1..end]
                    .trim()
                    .trim_matches(|ch| ch == '"' || ch == '\'' || ch == '`');
                at = end + 1;
                value
            }
            _ => {
                let start = at;
                while at < bytes.len() && !bytes[at].is_ascii_whitespace() && bytes[at] != b'>' {
                    at += 1;
                }
                &tag[start..at]
            }
        };
        found.push((name, value.to_string()));
    }
    found
}

fn attribute<'a>(attributes: &'a [(String, String)], name: &str) -> Option<&'a str> {
    attributes
        .iter()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.as_str())
}

/// Écart admis entre une coordonnée de la page et celle de Lucide : une
/// ancienne version d'une icône (le menu tracé de 3 à 21 au lieu de 4 à 20)
/// reste une icône du jeu, pas un dessin.
const TOLERANCE: f64 = 1.0;

/// Un tracé d'une icône : ses commandes (avec les drapeaux des arcs) et ses
/// nombres en coordonnées absolues. Une ligne, une polyligne et un chemin qui
/// passent par les mêmes points donnent le même tracé.
#[derive(Debug, Clone, PartialEq)]
struct Stroke {
    key: String,
    numbers: Vec<f64>,
}

impl Stroke {
    fn text(&self) -> String {
        let numbers: Vec<String> = self
            .numbers
            .iter()
            .map(|value| format!("{}", (value * 100.0).round() / 100.0))
            .collect();
        format!("{}:{}", self.key, numbers.join(","))
    }

    /// Un tracé fait de segments droits seulement (`M`, puis des `L`).
    fn is_polyline(&self) -> bool {
        self.key.starts_with('M') && self.key[1..].chars().all(|ch| ch == 'L')
    }

    fn close_to(&self, other: &Self) -> bool {
        let near = |a: &[f64], b: &[f64]| {
            a.len() == b.len() && a.iter().zip(b).all(|(a, b)| (a - b).abs() <= TOLERANCE)
        };
        if self.key != other.key {
            return false;
        }
        if near(&self.numbers, &other.numbers) {
            return true;
        }
        // La même polyligne parcourue dans l'autre sens.
        self.is_polyline() && {
            let reversed: Vec<f64> = self.numbers.chunks(2).rev().flatten().copied().collect();
            near(&reversed, &other.numbers)
        }
    }
}

/// Lecteur des données d'un chemin SVG (`d`), nombre par nombre.
struct PathReader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl PathReader<'_> {
    fn skip(&mut self) {
        while self
            .bytes
            .get(self.at)
            .is_some_and(|byte| byte.is_ascii_whitespace() || *byte == b',')
        {
            self.at += 1;
        }
    }

    fn command(&mut self) -> Option<u8> {
        self.skip();
        let byte = *self.bytes.get(self.at)?;
        (byte.is_ascii_alphabetic() && !matches!(byte, b'e' | b'E')).then(|| {
            self.at += 1;
            byte
        })
    }

    fn has_number(&mut self) -> bool {
        self.skip();
        self.bytes
            .get(self.at)
            .is_some_and(|byte| byte.is_ascii_digit() || matches!(byte, b'-' | b'+' | b'.'))
    }

    fn number(&mut self) -> Option<f64> {
        self.skip();
        let start = self.at;
        if matches!(self.bytes.get(self.at), Some(b'-' | b'+')) {
            self.at += 1;
        }
        let mut dot = false;
        while let Some(&byte) = self.bytes.get(self.at) {
            if byte.is_ascii_digit() {
                self.at += 1;
            } else if byte == b'.' && !dot {
                dot = true;
                self.at += 1;
            } else if matches!(byte, b'e' | b'E')
                && self
                    .bytes
                    .get(self.at + 1)
                    .is_some_and(|next| next.is_ascii_digit() || matches!(next, b'-' | b'+'))
            {
                self.at += 2;
                while self.bytes.get(self.at).is_some_and(u8::is_ascii_digit) {
                    self.at += 1;
                }
                break;
            } else {
                break;
            }
        }
        std::str::from_utf8(&self.bytes[start..self.at])
            .ok()?
            .parse()
            .ok()
    }

    /// Un drapeau d'arc : un seul chiffre, même collé au suivant (`a1 1 0 011 1`).
    fn flag(&mut self) -> Option<u8> {
        self.skip();
        let byte = *self.bytes.get(self.at)?;
        matches!(byte, b'0' | b'1').then(|| {
            self.at += 1;
            byte
        })
    }
}

/// Un segment d'un chemin (ni `M` ni `Z`) depuis le point `from`, en
/// coordonnées absolues : sa commande et ses nombres, le point d'arrivée en
/// dernier. Les segments droits (`L`, `H`, `V`) s'écrivent tous `L`.
fn segment(
    reader: &mut PathReader<'_>,
    command: u8,
    (x, y): (f64, f64),
) -> Option<(String, Vec<f64>)> {
    let (dx, dy) = if command.is_ascii_lowercase() {
        (x, y)
    } else {
        (0.0, 0.0)
    };
    let mut pairs = |count: usize| -> Option<Vec<f64>> {
        let mut found = Vec::with_capacity(count * 2);
        for _ in 0..count {
            found.push(reader.number()? + dx);
            found.push(reader.number()? + dy);
        }
        Some(found)
    };
    let upper = command.to_ascii_uppercase();
    Some(match upper {
        b'L' | b'T' => (char::from(upper).to_string(), pairs(1)?),
        b'H' => ("L".to_string(), vec![reader.number()? + dx, y]),
        b'V' => ("L".to_string(), vec![x, reader.number()? + dy]),
        b'C' => ("C".to_string(), pairs(3)?),
        b'S' | b'Q' => (char::from(upper).to_string(), pairs(2)?),
        b'A' => {
            let (rx, ry, rotation) = (reader.number()?, reader.number()?, reader.number()?);
            let (mut large, sweep) = (reader.flag()?, reader.flag()?);
            let (end_x, end_y) = (reader.number()? + dx, reader.number()? + dy);
            // Un demi-cercle (ou un rayon trop court, que le navigateur
            // agrandit) : les deux arcs possibles sont le même tracé.
            let chord = (end_x - x).hypot(end_y - y);
            if (rx - ry).abs() < 0.01 && chord >= 2.0 * rx - 0.05 {
                large = b'0';
            }
            (
                format!("A{}{}", char::from(large), char::from(sweep)),
                vec![rx, ry, rotation, end_x, end_y],
            )
        }
        _ => return None,
    })
}

/// Les tracés d'un chemin, en coordonnées absolues ; `None` s'il est illisible.
fn path_strokes(data: &str) -> Option<Vec<Stroke>> {
    let mut reader = PathReader {
        bytes: data.as_bytes(),
        at: 0,
    };
    let mut strokes = Vec::new();
    let mut current: Option<Stroke> = None;
    let (mut x, mut y, mut start_x, mut start_y) = (0.0, 0.0, 0.0, 0.0);
    let mut command = reader.command()?;
    loop {
        let relative = command.is_ascii_lowercase();
        let (dx, dy) = if relative { (x, y) } else { (0.0, 0.0) };
        let push = |current: &mut Option<Stroke>, key: &str, numbers: &[f64]| {
            if let Some(stroke) = current.as_mut() {
                stroke.key.push_str(key);
                stroke.numbers.extend_from_slice(numbers);
            }
        };
        match command.to_ascii_uppercase() {
            b'M' => {
                x = reader.number()? + dx;
                y = reader.number()? + dy;
                (start_x, start_y) = (x, y);
                if let Some(stroke) = current.take() {
                    strokes.push(stroke);
                }
                current = Some(Stroke {
                    key: "M".to_string(),
                    numbers: vec![x, y],
                });
                // Les paires qui suivent un `M` sont des segments droits.
                command = if relative { b'l' } else { b'L' };
                if !reader.has_number() {
                    command = reader.command().unwrap_or(0);
                }
                if command == 0 {
                    break;
                }
                continue;
            }
            b'Z' => {
                // Une fermeture qui revient au point de départ ne trace rien.
                let back = (x - start_x).abs() < 0.01 && (y - start_y).abs() < 0.01;
                (x, y) = (start_x, start_y);
                if !back {
                    push(&mut current, "Z", &[]);
                }
                match reader.command() {
                    Some(next) => {
                        command = next;
                        continue;
                    }
                    None => break,
                }
            }
            _ => {
                let (key, numbers) = segment(&mut reader, command, (x, y))?;
                if let [.., end_x, end_y] = numbers[..] {
                    (x, y) = (end_x, end_y);
                }
                push(&mut current, &key, &numbers);
            }
        }
        if !reader.has_number() {
            match reader.command() {
                Some(next) => command = next,
                None => break,
            }
        }
    }
    strokes.extend(current);
    Some(strokes)
}

fn number_attribute(attributes: &[(String, String)], key: &str) -> Option<f64> {
    attribute(attributes, key)?
        .trim()
        .trim_end_matches("px")
        .parse()
        .ok()
}

/// Les points d'une polyligne ou d'un polygone.
fn points(text: &str) -> Vec<f64> {
    let mut reader = PathReader {
        bytes: text.as_bytes(),
        at: 0,
    };
    let mut found = Vec::new();
    while reader.has_number() {
        match reader.number() {
            Some(value) => found.push(value),
            None => break,
        }
    }
    found
}

/// Les tracés d'une icône, triés : l'ordre des formes ne compte pas, et une
/// ligne vaut le chemin qui passe par les mêmes points. `None` sans aucune
/// forme, ou avec une forme illisible.
fn strokes(inner: &str) -> Option<Vec<Stroke>> {
    let lower = inner.to_ascii_lowercase();
    let mut found = Vec::new();
    let mut at = 0;
    while let Some(offset) = lower[at..].find('<') {
        let start = at + offset + 1;
        let end = lower[start..]
            .find('>')
            .map_or(lower.len(), |end| start + end);
        let tag = &inner[start..end];
        at = end;
        let name: String = tag
            .chars()
            .take_while(char::is_ascii_alphanumeric)
            .collect::<String>()
            .to_ascii_lowercase();
        let attributes = attributes(tag);
        let number = |key: &str| number_attribute(&attributes, key).unwrap_or(0.0);
        match name.as_str() {
            "path" => found.extend(path_strokes(attribute(&attributes, "d").unwrap_or(""))?),
            "line" => found.push(Stroke {
                key: "ML".to_string(),
                numbers: vec![number("x1"), number("y1"), number("x2"), number("y2")],
            }),
            "polyline" | "polygon" => {
                let numbers = points(attribute(&attributes, "points").unwrap_or(""));
                if numbers.len() < 4 || !numbers.len().is_multiple_of(2) {
                    return None;
                }
                let mut key = "M".to_string();
                key.push_str(&"L".repeat(numbers.len() / 2 - 1));
                // Un polygone se ferme, sauf s'il revient déjà à son départ.
                let closed = numbers[..2] == numbers[numbers.len() - 2..];
                if name == "polygon" && !closed {
                    key.push('Z');
                }
                found.push(Stroke { key, numbers });
            }
            "circle" => found.push(Stroke {
                key: "circle".to_string(),
                numbers: vec![number("cx"), number("cy"), number("r")],
            }),
            "ellipse" => found.push(Stroke {
                key: "ellipse".to_string(),
                numbers: vec![number("cx"), number("cy"), number("rx"), number("ry")],
            }),
            "rect" => {
                let rx = number_attribute(&attributes, "rx");
                let ry = number_attribute(&attributes, "ry");
                found.push(Stroke {
                    key: "rect".to_string(),
                    numbers: vec![
                        number("x"),
                        number("y"),
                        number("width"),
                        number("height"),
                        rx.or(ry).unwrap_or(0.0),
                        ry.or(rx).unwrap_or(0.0),
                    ],
                });
            }
            _ => {}
        }
    }
    if found.is_empty() {
        return None;
    }
    found.sort_by_key(Stroke::text);
    Some(found)
}

/// Les icônes de Lucide et de Feather : leurs empreintes exactes, et leurs
/// tracés rangés par structure (la suite des commandes) pour la comparaison à
/// un point près.
struct Lucide {
    exact: HashSet<String>,
    by_structure: HashMap<String, Vec<Vec<Stroke>>>,
}

fn structure(strokes: &[Stroke]) -> String {
    let mut keys: Vec<&str> = strokes.iter().map(|stroke| stroke.key.as_str()).collect();
    keys.sort_unstable();
    keys.join("|")
}

fn fingerprint(strokes: &[Stroke]) -> String {
    strokes
        .iter()
        .map(Stroke::text)
        .collect::<Vec<_>>()
        .join("|")
}

fn lucide() -> &'static Lucide {
    static SET: OnceLock<Lucide> = OnceLock::new();
    SET.get_or_init(|| {
        let mut set = Lucide {
            exact: HashSet::new(),
            by_structure: HashMap::new(),
        };
        let lucide = LUCIDE
            .lines()
            .filter_map(|line| line.splitn(3, '\t').nth(2));
        let feather = FEATHER
            .lines()
            .filter_map(|line| line.split_once('\t').map(|(_, inner)| inner));
        for inner in lucide.chain(feather) {
            if let Some(found) = strokes(inner) {
                set.exact.insert(fingerprint(&found));
                set.by_structure
                    .entry(structure(&found))
                    .or_default()
                    .push(found);
            }
        }
        set
    })
}

/// Les tracés sont ceux d'une icône de Lucide ou de Feather, exactement ou à
/// un point près.
fn from_the_set(candidate: &[Stroke]) -> bool {
    let set = lucide();
    if set.exact.contains(&fingerprint(candidate)) {
        return true;
    }
    let Some(icons) = set.by_structure.get(&structure(candidate)) else {
        return false;
    };
    icons.iter().any(|icon| {
        let mut used = vec![false; icon.len()];
        candidate.iter().all(|stroke| {
            let matched = icon
                .iter()
                .enumerate()
                .find(|(index, known)| !used[*index] && stroke.close_to(known));
            matched.is_some_and(|(index, _)| {
                used[index] = true;
                true
            })
        })
    })
}

/// Le nom existe dans Lucide (`phone`, `map-pin`).
#[must_use]
pub fn is_lucide_name(name: &str) -> bool {
    LUCIDE
        .lines()
        .any(|line| line.split('\t').next() == Some(name))
}

/// Mots qui ne disent rien de ce qu'une icône représente.
const NOISE: &[&str] = &[
    "icon", "icons", "svg", "lucide", "ico", "inline", "small", "large", "sm", "md", "lg", "xl",
    "img", "image", "true", "false", "hidden", "aria", "the", "and",
];

/// Ce qu'une icône représente, d'après sa classe, son id, son étiquette ou
/// son titre : `class="icon icon-phone"` donne `phone`.
fn meaning(attributes: &[(String, String)], inner: &str) -> Vec<String> {
    let mut text = String::new();
    for key in [
        "class",
        "classname",
        "id",
        "aria-label",
        "data-icon",
        "data-lucide",
        "name",
    ] {
        if let Some(value) = attribute(attributes, key) {
            text.push(' ');
            text.push_str(value);
        }
    }
    let lower = inner.to_ascii_lowercase();
    if let Some(start) = lower.find("<title>") {
        let rest = &inner[start + 7..];
        text.push(' ');
        text.push_str(&rest[..rest.find('<').unwrap_or(rest.len())]);
    }
    let mut words: Vec<String> = Vec::new();
    // Un nom à tirets entier d'abord (`map-pin`), puis ses morceaux.
    for chunk in text.split(|ch: char| ch.is_whitespace() || ch == '_') {
        let chunk = chunk
            .trim_matches(|ch: char| !ch.is_alphanumeric())
            .to_ascii_lowercase();
        let chunk = chunk
            .trim_start_matches("icon-")
            .trim_start_matches("lucide-")
            .to_string();
        if chunk.contains('-') && is_lucide_name(&chunk) && !words.contains(&chunk) {
            words.push(chunk.clone());
        }
        for word in chunk.split('-') {
            if word.len() >= 2 && !NOISE.contains(&word) && !words.iter().any(|known| known == word)
            {
                words.push(word.to_string());
            }
        }
    }
    words
}

/// Ce qui exempte une petite image vectorielle : un ornement de l'outil
/// `Ornament`, un texte, une image, ou un renvoi à un symbole (`<use
/// href="#…">`, contrôlé là où il est défini). Un logo, une marque ou un
/// drapeau ne le sont plus : ils se récupèrent (paquet officiel, fichier de la
/// marque), jamais ne se dessinent (demande de l'utilisateur, 23 octobre 2026).
/// Une icône rendue par un paquet de Lucide (`lucide-react`, `lucide`), qui
/// la marque `class="lucide lucide-<nom>"`, vient du jeu même quand sa
/// version du tracé est plus récente que celle de Claw. Vu le 28 octobre 2026
/// (session #41, appli React) : `settings`, `receipt` et `shopping-cart` de
/// lucide-react 1.63 dits « dessinés à la main ».
fn exempt(attributes: &[(String, String)], inner: &str) -> bool {
    let names: String = ["class", "classname", "id", "aria-label", "role"]
        .iter()
        .filter_map(|key| attribute(attributes, key))
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase();
    let lower = inner.to_ascii_lowercase();
    let classes: Vec<String> = attribute(attributes, "class")
        .or_else(|| attribute(attributes, "classname"))
        .map(|class| {
            class
                .split_whitespace()
                .map(str::to_ascii_lowercase)
                .collect()
        })
        .unwrap_or_default();
    let lucide_package = classes.iter().any(|class| class == "lucide")
        && classes
            .iter()
            .any(|class| class.len() > "lucide-".len() && class.starts_with("lucide-"));
    lucide_package
        || ["ornament"].iter().any(|word| names.contains(word))
        || ["<text", "<image", "<foreignobject"]
            .iter()
            .any(|tag| lower.contains(tag))
}

/// La `viewBox` (ou la largeur et la hauteur) dit une icône, pas une
/// illustration. Sans aucune des deux, rien ne permet de le dire.
fn icon_sized(attributes: &[(String, String)]) -> bool {
    if let Some(view) = attribute(attributes, "viewbox") {
        let numbers: Vec<f64> = view
            .split(|ch: char| ch.is_whitespace() || ch == ',')
            .filter_map(|part| part.parse::<f64>().ok())
            .collect();
        return numbers.len() == 4 && numbers[2] <= ICON_BOX && numbers[3] <= ICON_BOX;
    }
    let side = |key: &str| {
        attribute(attributes, key)
            .and_then(|value| value.trim().trim_end_matches("px").parse::<f64>().ok())
    };
    matches!((side("width"), side("height")), (Some(width), Some(height)) if width <= ICON_BOX && height <= ICON_BOX)
}

/// Les SVG des adresses `data:image/svg+xml` (fonds CSS, `<img src>`),
/// décodés, avec la position de leur début dans `text`.
fn data_uri_svgs(text: &str) -> Vec<(usize, String)> {
    let mut found = Vec::new();
    let mut at = 0;
    while let Some(start) = text[at..].find("data:image/svg+xml") {
        let start = at + start;
        let body_start = text[start..]
            .find(',')
            .map_or(text.len(), |comma| start + comma + 1);
        let header = &text[start..body_start];
        // L'adresse finit au signe qui l'ouvre : un SVG entre guillemets
        // doubles garde ses guillemets simples.
        let closing: &[char] = match text[..start].chars().next_back() {
            Some('"') => &['"'],
            Some('\'') => &['\''],
            _ => &[')', '"', '\'', ' '],
        };
        let end = text[body_start..]
            .find(closing)
            .map_or(text.len(), |end| body_start + end);
        let body = &text[body_start..end];
        let decoded = if header.contains(";base64") {
            crate::attachments::decode_base64(body)
                .and_then(|bytes| String::from_utf8(bytes).ok())
                .unwrap_or_default()
        } else {
            percent_decode(body)
        };
        found.push((start, decoded));
        at = end.max(start + 1);
    }
    found
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at] == b'%' && at + 2 < bytes.len() + 1 {
            if let Some(byte) = text
                .get(at + 1..at + 3)
                .and_then(|hex| u8::from_str_radix(hex, 16).ok())
            {
                out.push(byte);
                at += 3;
                continue;
            }
        }
        out.push(bytes[at]);
        at += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Les icônes dessinées à la main dans `markup` (un `<svg>` ou un
/// `<symbol>` à la taille d'une icône, dont les formes ne sont celles
/// d'aucune icône de Lucide), avec leur position dans le texte.
fn scan(markup: &str) -> Vec<(usize, DrawnIcon)> {
    let lower = markup.to_ascii_lowercase();
    let mut found = Vec::new();
    let mut at = 0;
    while let Some(offset) = lower[at..].find('<') {
        let start = at + offset;
        at = start + 1;
        let (kind, close) = if lower[start..].starts_with("<svg") {
            ("svg", "</svg")
        } else if lower[start..].starts_with("<symbol") {
            ("symbol", "</symbol")
        } else {
            continue;
        };
        let after = lower[start + 1 + kind.len()..].chars().next();
        if !after.is_some_and(|ch| ch.is_whitespace() || ch == '>' || ch == '/') {
            continue;
        }
        let Some(tag_end) = lower[start..].find('>').map(|end| start + end) else {
            break;
        };
        let tag = &markup[start + 1..tag_end];
        if tag.ends_with('/') {
            continue;
        }
        let inner_end = lower[tag_end..]
            .find(close)
            .map_or(lower.len(), |end| tag_end + end);
        let inner = &markup[tag_end + 1..inner_end];
        let attributes = attributes(tag);
        // Un conteneur (un sprite de `<symbol>`, une illustration) est lu
        // aussi de l'intérieur ; une icône est jugée en entier.
        if !icon_sized(&attributes) {
            at = tag_end;
            continue;
        }
        at = inner_end.max(start + 1);
        if exempt(&attributes, inner) {
            continue;
        }
        let broken = broken_path(inner);
        if !broken {
            let Some(found_strokes) = strokes(inner) else {
                continue;
            };
            if from_the_set(&found_strokes) || was_fetched(&found_strokes) {
                continue;
            }
        }
        let words = meaning(&attributes, inner);
        let label =
            match attribute(&attributes, "class").or_else(|| attribute(&attributes, "classname")) {
                Some(class) if !class.trim().is_empty() => {
                    format!("<{kind} class=\"{}\">", class.trim())
                }
                _ => match attribute(&attributes, "id") {
                    Some(id) => format!("<{kind} id=\"{id}\">"),
                    None => format!("<{kind}>"),
                },
            };
        let label = if broken {
            format!("{label} with broken path data")
        } else {
            label
        };
        found.push((
            start,
            DrawnIcon {
                line: 0,
                label,
                words,
            },
        ));
    }
    found
}

/// Un `<path>` dont les données ne commencent pas par un déplacement (`M`) :
/// le navigateur ne le dessine pas. Vu le 22 octobre 2026 : une icône de
/// Lucide recopiée de travers (`d="9 18h6"`), « corrigée » ensuite d'une
/// lettre à la main au lieu d'être reprise de l'outil.
fn broken_path(inner: &str) -> bool {
    let lower = inner.to_ascii_lowercase();
    let mut at = 0;
    while let Some(offset) = lower[at..].find("<path") {
        let start = at + offset + 1;
        let end = lower[start..]
            .find('>')
            .map_or(lower.len(), |end| start + end);
        at = end;
        let attributes = attributes(&inner[start..end]);
        if let Some(data) = attribute(&attributes, "d") {
            let data = data.trim_start();
            if !data.is_empty() && !data.starts_with(['M', 'm']) {
                return true;
            }
        }
    }
    false
}

/// Formes des SVG que l'agent a récupérés pendant la session (téléchargés,
/// lus dans un paquet, donnés par un outil) : ce ne sont pas des dessins.
fn fetched() -> &'static Mutex<HashSet<String>> {
    static FETCHED: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    FETCHED.get_or_init(|| Mutex::new(HashSet::new()))
}

fn was_fetched(candidate: &[Stroke]) -> bool {
    fetched()
        .lock()
        .is_ok_and(|known| known.contains(&fingerprint(candidate)))
}

/// Retient les SVG d'un texte récupéré (sortie de `curl`, fichier d'un paquet,
/// page lue) : une icône, un logo ou un drapeau qui en vient n'est pas dessiné
/// à la main. Le texte peut être du JSON (guillemets échappés).
pub fn learn_fetched(text: &str) {
    let text = text.replace("\\\"", "\"").replace("\\n", "\n");
    let lower = text.to_ascii_lowercase();
    let mut learned = Vec::new();
    for (open, close) in [("<svg", "</svg"), ("<symbol", "</symbol")] {
        let mut at = 0;
        while let Some(offset) = lower[at..].find(open) {
            let start = at + offset;
            let Some(tag_end) = lower[start..].find('>').map(|end| start + end) else {
                break;
            };
            let inner_end = lower[tag_end..]
                .find(close)
                .map_or(lower.len(), |end| tag_end + end);
            if let Some(found) = strokes(&text[tag_end + 1..inner_end]) {
                learned.push(fingerprint(&found));
            }
            at = tag_end + 1;
        }
    }
    if let Ok(mut known) = fetched().lock() {
        known.extend(learned);
    }
}

/// Les icônes dessinées à la main dans le texte d'un fichier d'interface
/// (HTML, composant, script qui écrit du HTML, feuille de style avec des
/// adresses `data:`), dans l'ordre, avec leur ligne.
#[must_use]
pub fn drawn_icons(text: &str) -> Vec<DrawnIcon> {
    let line_of = |offset: usize| text[..offset.min(text.len())].matches('\n').count() + 1;
    let mut found: Vec<DrawnIcon> = scan(text)
        .into_iter()
        .map(|(offset, icon)| DrawnIcon {
            line: line_of(offset),
            ..icon
        })
        .collect();
    for (offset, svg) in data_uri_svgs(text) {
        for (_, icon) in scan(&svg) {
            found.push(DrawnIcon {
                line: line_of(offset),
                label: format!("{} in a data: URL", icon.label),
                ..icon
            });
        }
    }
    found.sort_by_key(|icon| icon.line);
    found
}

/// Les noms d'icônes à demander pour remplacer celles dessinées à la main,
/// quand leurs mots en désignent une (`phone`, `map-pin`).
#[must_use]
pub fn suggested_names(icons: &[DrawnIcon]) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for icon in icons {
        if let Some(name) = icon.words.iter().find(|word| is_lucide_name(word)) {
            if !names.contains(name) {
                names.push(name.clone());
            }
        }
    }
    names
}

/// Ce qui est dit au modèle quand un fichier dessine des icônes à la main.
#[must_use]
pub fn drawn_icons_feedback(path: &str, icons: &[DrawnIcon]) -> String {
    let places: Vec<String> = icons
        .iter()
        .take(8)
        .map(|icon| format!("line {} {}", icon.line, icon.label))
        .collect();
    let more = icons.len().saturating_sub(8);
    let names = suggested_names(icons);
    let ask = if names.is_empty() {
        "call the Icon tool with `search` and what each one shows".to_string()
    } else {
        format!(
            "call the Icon tool with names {} (and `search` for the others)",
            serde_json::to_string(&names).unwrap_or_default()
        )
    };
    format!(
        "[Claw] {path} draws {} icon(s) by hand ({}{}). An SVG is never drawn or retouched by hand, not even a simple arrow, check, cross or menu, and not a logo, favicon or flag either: hand-drawn shapes look uneven and crude, and Claw compares every small inline SVG with the icon set. Replace each icon with the Icon tool's SVG pasted as it is ({ask}); size and color it with CSS. A broken icon is fetched again from the Icon tool, never patched. A brand logo or a flag comes from its official file (for example the simple-icons or flag-icons package, installed with npm or downloaded with curl); the project's own logo is its existing file, or an Icon tool icon with the name in text. A decorative shape comes from the Ornament tool.",
        icons.len(),
        places.join("; "),
        if more > 0 {
            format!("; and {more} more")
        } else {
            String::new()
        }
    )
}

#[cfg(test)]
mod tests {
    use super::{
        drawn_icons, drawn_icons_feedback, is_lucide_name, lucide, path_strokes, strokes,
        suggested_names, LUCIDE,
    };

    /// Le SVG complet d'une icône de Lucide, comme l'outil `Icon` le rend.
    fn icon(name: &str) -> String {
        let inner = LUCIDE
            .lines()
            .find(|line| line.split('\t').next() == Some(name))
            .and_then(|line| line.splitn(3, '\t').nth(2))
            .expect("icône");
        format!("<svg xmlns=\"http://www.w3.org/2000/svg\" class=\"icon icon-{name}\" width=\"24\" height=\"24\" viewBox=\"0 0 24 24\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"2\" aria-hidden=\"true\">{inner}</svg>")
    }

    #[test]
    fn icons_from_the_set_pass_however_they_are_written() {
        // Chaque icône du jeu se lit, et se reconnaît elle-même.
        let unread: Vec<&str> = LUCIDE
            .lines()
            .filter(|line| strokes(line.splitn(3, '\t').nth(2).unwrap_or("")).is_none())
            .filter_map(|line| line.split('\t').next())
            .collect();
        assert!(unread.is_empty(), "{unread:?}");
        assert!(lucide().exact.len() > 1_400);
        assert!(is_lucide_name("map-pin") && !is_lucide_name("dragon-head"));
        for name in [
            "phone",
            "x",
            "map-pin",
            "calendar",
            "chevron-down",
            "instagram",
        ] {
            let svg = icon(name);
            assert!(drawn_icons(&svg).is_empty(), "{svg}");
        }
        // Réécrite par un modèle : virgules, zéros, ordre des attributs, JSX.
        let rewritten = "<svg viewBox='0 0 24 24' className=\"w-5 h-5\"><path d=\"M18,6 L6,18\"></path><path d=\"M6 6l12 12\" /></svg>";
        assert!(drawn_icons(rewritten).is_empty(), "x réécrite");
        let circle = "<svg viewBox=\"0 0 24 24\"><circle r=\"10\" cy=\"12.0\" cx=\"12\"/></svg>";
        assert!(drawn_icons(circle).is_empty(), "circle de Lucide");
        // Vu sur de vraies pages : des icônes de Lucide ou de Feather dans une
        // ancienne version, recopiées de mémoire, ne sont pas des dessins.
        let old_arrow = "<svg viewBox=\"0 0 24 24\"><line x1=\"12\" x2=\"12\" y1=\"5\" y2=\"19\"></line><polyline points=\"19 12 12 19 5 12\"></polyline></svg>";
        assert!(drawn_icons(old_arrow).is_empty(), "flèche de Feather");
        let old_menu = "<svg class=\"icon icon-menu\" viewBox=\"0 0 24 24\"><path d=\"M3 6h18\"/><path d=\"M3 12h18\"/><path d=\"M3 18h18\"/></svg>";
        assert!(drawn_icons(old_menu).is_empty(), "menu de 3 à 21");
        let chevron = "<svg viewBox=\"0 0 24 24\"><path d=\"M6 9l6 6 6-6\"/></svg>";
        assert!(drawn_icons(chevron).is_empty(), "chevron");
        // Arcs aux drapeaux collés, et commandes relatives après une fermeture.
        let arcs = path_strokes("M3 3a2 2 0 011 1zm4 4h2").expect("chemin");
        assert_eq!(arcs.len(), 2);
        assert_eq!(arcs[0].key, "MA01Z");
        // Feather, recopiée de mémoire : son épingle et son Instagram.
        let feather = "<svg viewBox=\"0 0 24 24\"><path d=\"M21 10c0 7-9 13-9 13s-9-6-9-13a9 9 0 0 1 18 0z\"></path><circle cx=\"12\" cy=\"10\" r=\"3\"></circle></svg>";
        assert!(drawn_icons(feather).is_empty(), "épingle de Feather");
        // Une fermeture qui ne trace rien ne change pas l'icône (épingle de
        // Lucide terminée par un z, vue sur une vraie page).
        let pin = "<svg viewBox=\"0 0 24 24\"><path d=\"M20 10c0 4.993-5.539 10.193-7.399 11.799a1 1 0 0 1-1.202 0C9.539 20.193 4 14.993 4 10a8 8 0 0 1 16 0z\"/><circle cx=\"12\" cy=\"10\" r=\"3\"/></svg>";
        assert!(drawn_icons(pin).is_empty(), "épingle fermée");
        assert_eq!(arcs[1].numbers, [7.0, 7.0, 9.0, 7.0]);
    }

    #[test]
    fn hand_drawn_icons_are_found_with_their_line_and_meaning() {
        let page = "<header>\n<button aria-label=\"Menu\">\n<svg class=\"icon icon-menu\" viewBox=\"0 0 24 24\"><path d=\"M2 5h20M2 12h13M2 19h20\"/></svg>\n</button>\n<svg class=\"logo\" viewBox=\"0 0 32 32\"><path d=\"M1 2 L30 30\"/></svg>\n<svg viewBox=\"0 0 1440 120\"><path d=\"M0 40 C 200 0 400 80 1440 40\"/></svg>\n<a class=\"social\"><svg width=\"20\" height=\"20\" viewBox=\"0 0 20 20\"><title>Téléphone phone</title><path d=\"M2 3c1 5 6 10 11 11l2-2\"/></svg></a>\n</header>";
        let found = drawn_icons(page);
        // Le logo dessiné à la main est signalé lui aussi.
        assert_eq!(found.len(), 3, "{found:?}");
        assert_eq!(found[0].line, 3);
        assert_eq!(found[0].label, "<svg class=\"icon icon-menu\">");
        assert!(found[0].words.contains(&"menu".to_string()));
        assert_eq!(found[1].label, "<svg class=\"logo\">");
        assert_eq!(found[2].line, 7);
        assert_eq!(suggested_names(&found), ["menu", "phone"]);
        let feedback = drawn_icons_feedback("index.html", &found);
        assert!(
            feedback.contains("draws 3 icon(s) by hand")
                && feedback.contains("line 3 <svg class=\"icon icon-menu\">")
                && feedback.contains("[\"menu\",\"phone\"]"),
            "{feedback}"
        );
        // Un sprite de symboles dessinés à la main, et une flèche de liste
        // déroulante en adresse data: dans le CSS.
        let sprite = "<svg style=\"display:none\"><symbol id=\"i-check\" viewBox=\"0 0 16 16\"><path d=\"M2 8l4 4 8-8\"/></symbol></svg>";
        assert_eq!(drawn_icons(sprite).len(), 1);
        let css = "select {\n  background: url(\"data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 12 8'%3E%3Cpath d='M1 1l5 5 5-5'/%3E%3C/svg%3E\");\n}";
        let found = drawn_icons(css);
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].line, 2);
        assert!(found[0].label.ends_with("in a data: URL"));
        // La même flèche prise dans Lucide passe.
        let chevron = icon("chevron-down").replace('"', "'");
        let css = format!(
            "select {{ background: url(\"data:image/svg+xml,{}\"); }}",
            chevron.replace('<', "%3C").replace('>', "%3E")
        );
        assert!(drawn_icons(&css).is_empty(), "{css}");
    }

    #[test]
    fn a_broken_icon_is_reported_and_a_fetched_logo_passes() {
        // Session 20 (22 octobre 2026) : `d="9 18h6"`, sans le `M` du départ.
        let broken = "<svg viewBox=\"0 0 24 24\"><path d=\"9 18h6\"/><path d=\"M10 22h4\"/></svg>";
        let found = drawn_icons(broken);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].label.ends_with("with broken path data"));
        let feedback = drawn_icons_feedback("index.html", &found);
        assert!(feedback.contains("fetched again from the Icon tool, never patched"));
        // Un logo téléchargé (sortie de curl, ou fichier d'un paquet lu en
        // JSON) n'est pas un dessin ; le même tracé inventé l'est.
        let logo = "<svg role=\"img\" viewBox=\"0 0 24 24\"><path d=\"M3 3 L21 3 L12 21 Z M8 7 L16 7\"/></svg>";
        assert_eq!(drawn_icons(logo).len(), 1);
        super::learn_fetched(&serde_json::json!({ "content": logo }).to_string());
        assert!(drawn_icons(logo).is_empty());
    }

    #[test]
    fn an_icon_rendered_by_a_lucide_package_comes_from_the_set() {
        // Session #41 (28 octobre 2026) : la roue des réglages de lucide-react
        // 1.63, d'un tracé plus récent que celui de Claw, dite dessinée à la main.
        let newer = "<path d=\"M9.671 4.136a2.34 2.34 0 0 1 4.659 0 2.34 2.34 0 0 0 3.319 1.915 2.34 2.34 0 0 1 2.33 4.033 2.34 2.34 0 0 0 0 3.831 2.34 2.34 0 0 1-2.33 4.033 2.34 2.34 0 0 0-3.319 1.915 2.34 2.34 0 0 1-4.659 0 2.34 2.34 0 0 0-3.32-1.915 2.34 2.34 0 0 1-2.33-4.033 2.34 2.34 0 0 0 0-3.831A2.34 2.34 0 0 1 6.35 6.051a2.34 2.34 0 0 0 3.319-1.915\"></path><circle cx=\"12\" cy=\"12\" r=\"3\"></circle>";
        let package = format!("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"20\" height=\"20\" viewBox=\"0 0 24 24\" fill=\"none\" stroke=\"currentColor\" class=\"lucide lucide-settings icon\" aria-hidden=\"true\">{newer}</svg>");
        assert!(
            drawn_icons(&package).is_empty(),
            "{:?}",
            drawn_icons(&package)
        );
        // Le mot seul ne suffit pas : un dessin nommé « lucide » reste un dessin
        // (un tracé à lui : la liste des tracés récupérés est commune aux tests).
        let named = "<svg viewBox=\"0 0 24 24\" class=\"lucide\"><path d=\"M4 5 L20 5 L12 19 Z M9 9 L15 9\"/></svg>";
        assert_eq!(drawn_icons(named).len(), 1);
    }
}
