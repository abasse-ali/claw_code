//! Outil `AppIcon` : les icônes PNG d'une appli installable, dessinées par le
//! navigateur à partir de son icône SVG. Un téléphone veut des PNG (192 et
//! 512 px pour Android, 180 px pour l'écran d'accueil de l'iPhone) ; un petit
//! modèle ne sait pas en écrire, et livrait un PNG vide ou de 1 px, ou aucun
//! (vu le 27 octobre 2026, session #35 : l'appli de budget, sans icône).

use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::{json, Value};

use super::{decode_base64, page_socket_path, Browser, DevTools};

#[derive(Debug, Deserialize)]
pub(crate) struct AppIconInput {
    svg: String,
    #[serde(default)]
    folder: Option<String>,
    #[serde(default)]
    background: Option<String>,
}

/// Une icône à dessiner : nom du fichier, côté en px, côté du dessin au
/// centre, fond plein ou transparent.
struct Job {
    file: &'static str,
    size: u32,
    side: u32,
    opaque: bool,
}

const JOBS: &[Job] = &[
    Job {
        file: "icon-192.png",
        size: 192,
        side: 192,
        opaque: false,
    },
    Job {
        file: "icon-512.png",
        size: 512,
        side: 512,
        opaque: false,
    },
    // Android découpe une icône « maskable » en cercle ou en goutte : le dessin
    // tient dans la zone sûre (le cercle de 80 % du côté), sur un fond plein.
    Job {
        file: "icon-maskable-512.png",
        size: 512,
        side: 368,
        opaque: true,
    },
    // L'iPhone remplit la transparence de noir : fond plein, coins arrondis par lui.
    Job {
        file: "apple-touch-icon.png",
        size: 180,
        side: 180,
        opaque: true,
    },
];

/// Dessine chaque icône sur un canevas et rend, par fichier, le PNG en base64
/// et ce qu'il montre (part de pixels dessinés, nombre de couleurs).
const DRAW_SCRIPT: &str = r#"(async (images, jobs, background) => {
  const out = {};
  for (const job of jobs) {
    const image = new Image();
    image.src = "data:image/svg+xml;charset=utf-8," + encodeURIComponent(images[job.file]);
    try { await image.decode(); } catch (_) { return { error: "the browser cannot draw this SVG: check that it is a complete <svg> with xmlns=\"http://www.w3.org/2000/svg\"" }; }
    const canvas = document.createElement("canvas");
    canvas.width = canvas.height = job.size;
    const context = canvas.getContext("2d");
    if (job.opaque) { context.fillStyle = background; context.fillRect(0, 0, job.size, job.size); }
    const side = job.side;
    const offset = Math.round((job.size - side) / 2);
    context.drawImage(image, offset, offset, side, side);
    let png;
    let pixels;
    try {
      png = canvas.toDataURL("image/png").split(",")[1];
      pixels = context.getImageData(0, 0, job.size, job.size).data;
    } catch (_) {
      return { error: "the SVG loads something from elsewhere (an image or a font by address): keep everything inside the SVG" };
    }
    let drawn = 0;
    const colors = new Set();
    for (let index = 0; index < pixels.length; index += 4) {
      if (pixels[index + 3] > 16) drawn++;
      if (colors.size < 64) colors.add(`${pixels[index] >> 4},${pixels[index + 1] >> 4},${pixels[index + 2] >> 4},${pixels[index + 3] >> 6}`);
    }
    out[job.file] = { png, drawn: drawn / (job.size * job.size), colors: colors.size };
  }
  return out;
})"#;

/// Dossier du projet où écrire les icônes (dans le dossier de travail).
fn project_folder(root: &Path, folder: &str) -> Result<PathBuf, String> {
    let relative = Path::new(folder.trim().trim_start_matches("./"));
    // `/etc` n'est pas « absolu » sous Windows (aucun lecteur) mais part de la racine du disque :
    // refusé comme un chemin absolu, de même qu'un lecteur seul (`C:icons`).
    if relative.is_absolute()
        || relative.components().any(|part| {
            matches!(
                part,
                std::path::Component::ParentDir
                    | std::path::Component::RootDir
                    | std::path::Component::Prefix(_)
            )
        })
    {
        return Err(format!(
            "folder `{folder}` must be a folder inside the project, written relative to it (for example \"icons\")"
        ));
    }
    Ok(root.join(relative))
}

/// Le SVG à la taille `size` : largeur et hauteur de la balise `<svg>`
/// remplacées, pour que le navigateur le dessine net à cette taille.
fn sized(svg: &str, size: u32) -> Result<String, String> {
    let start = svg
        .find("<svg")
        .ok_or("the file has no <svg> element: give the path of an SVG drawing")?;
    let end = start
        + svg[start..]
            .find('>')
            .ok_or("the <svg> tag is not closed")?;
    let tag = &svg[start..end];
    if !(tag.contains("viewBox") || (tag.contains("width=") && tag.contains("height="))) {
        return Err(
            "the <svg> has neither a viewBox nor a width and height: add viewBox=\"0 0 512 512\""
                .to_string(),
        );
    }
    let mut cleaned = String::new();
    let mut rest = tag;
    // Retire width="…" et height="…" (pas stroke-width ni les autres).
    while let Some(at) = rest.find(['w', 'h']) {
        let before = rest[..at].chars().last();
        let name = if rest[at..].starts_with("width=") {
            Some("width=")
        } else if rest[at..].starts_with("height=") {
            Some("height=")
        } else {
            None
        };
        if let Some(name) = name.filter(|_| before.is_some_and(char::is_whitespace)) {
            cleaned.push_str(&rest[..at]);
            let value = &rest[at + name.len()..];
            let quote = value.chars().next().unwrap_or('"');
            let skip = value[1..].find(quote).map_or(value.len(), |end| end + 2);
            rest = &value[skip..];
        } else {
            cleaned.push_str(&rest[..=at]);
            rest = &rest[at + 1..];
        }
    }
    cleaned.push_str(rest);
    let mut tag = cleaned.trim_end().to_string();
    if !tag.contains("xmlns=") {
        tag.push_str(" xmlns=\"http://www.w3.org/2000/svg\"");
    }
    Ok(format!(
        "{}{tag} width=\"{size}\" height=\"{size}\"{}",
        &svg[..start],
        &svg[end..]
    ))
}

/// Couleur de fond par défaut : celle du manifeste de l'appli s'il est près de
/// l'icône, sinon blanc.
fn manifest_background(root: &Path, svg: &Path) -> Option<String> {
    let mut places: Vec<PathBuf> = svg.ancestors().skip(1).map(Path::to_path_buf).collect();
    places.retain(|place| place.starts_with(root));
    for place in places {
        for name in ["manifest.webmanifest", "manifest.json"] {
            let Ok(text) = std::fs::read_to_string(place.join(name)) else {
                continue;
            };
            let Ok(manifest) = serde_json::from_str::<Value>(&text) else {
                continue;
            };
            if let Some(color) = manifest["background_color"]
                .as_str()
                .or_else(|| manifest["theme_color"].as_str())
            {
                return Some(color.to_string());
            }
        }
    }
    None
}

/// Une couleur CSS simple (nom, `#rgb`, `#rrggbb`, `rgb(…)`, `hsl(…)`), sans
/// rien qui sorte de la chaîne du script.
fn plain_color(color: &str) -> Result<String, String> {
    let color = color.trim();
    if color.is_empty()
        || color.len() > 40
        || !color
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || " #(),.%/-".contains(ch))
    {
        return Err(format!(
            "background `{color}` is not a CSS color: give a value like \"#161412\""
        ));
    }
    Ok(color.to_string())
}

/// Le dossier de travail et l'icône SVG du projet qu'on y désigne.
fn project_svg(svg: &str) -> Result<(PathBuf, PathBuf), String> {
    let root = std::env::current_dir().map_err(|error| error.to_string())?;
    let root = root.canonicalize().unwrap_or(root);
    let svg_path = root
        .join(svg.trim().trim_start_matches("./"))
        .canonicalize()
        .map_err(|_| format!("{svg} does not exist: write the SVG icon first"))?;
    if !svg_path.starts_with(&root) {
        return Err(format!(
            "{svg} is outside the working directory: give the project's SVG icon"
        ));
    }
    if !svg_path
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("svg"))
    {
        return Err(format!("{svg} is not an .svg file"));
    }
    Ok((root, svg_path))
}

/// Les icônes dessinées par le navigateur : par fichier, le PNG en base64 et
/// ce qu'il montre.
fn draw(text: &str, background: &str) -> Result<Value, String> {
    let mut images = serde_json::Map::new();
    for job in JOBS {
        images.insert(job.file.to_string(), json!(sized(text, job.side)?));
    }
    let jobs: Vec<Value> = JOBS
        .iter()
        .map(|job| json!({ "file": job.file, "size": job.size, "side": job.side, "opaque": job.opaque }))
        .collect();
    let (_browser, port) = Browser::launch_first(&crate::page_view::browser_candidates())?;
    let path = page_socket_path(port)?;
    let mut devtools = DevTools::connect(port, &path)?;
    devtools.call("Runtime.enable", json!({}))?;
    let drawn = devtools.evaluate(&format!(
        "{DRAW_SCRIPT}({}, {}, {})",
        Value::Object(images),
        Value::Array(jobs),
        json!(background)
    ))?;
    match drawn["error"].as_str() {
        Some(error) => Err(error.to_string()),
        None => Ok(drawn),
    }
}

pub(crate) fn app_icon(input: &AppIconInput) -> Result<Value, String> {
    let (root, svg_path) = project_svg(&input.svg)?;
    let text = std::fs::read_to_string(&svg_path).map_err(|error| error.to_string())?;
    let folder = match input.folder.as_deref() {
        Some(folder) => project_folder(&root, folder)?,
        None => svg_path.parent().map_or(root.clone(), Path::to_path_buf),
    };
    let background = match input.background.as_deref() {
        Some(color) => plain_color(color)?,
        None => manifest_background(&root, &svg_path)
            .and_then(|color| plain_color(&color).ok())
            .unwrap_or_else(|| "#ffffff".to_string()),
    };
    let drawn = draw(&text, &background)?;
    let big = &drawn["icon-512.png"];
    let share = big["drawn"].as_f64().unwrap_or(0.0);
    if share < 0.02 || big["colors"].as_u64().unwrap_or(0) < 2 {
        return Err(format!(
            "{} draws almost nothing ({:.0} % of the square): draw the icon with its colors written in the SVG (an Icon tool icon with `color` and `background`, or an Ornament shape), then call AppIcon again",
            input.svg,
            share * 100.0
        ));
    }
    std::fs::create_dir_all(&folder).map_err(|error| error.to_string())?;
    let relative = |path: &Path| {
        path.strip_prefix(&root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/")
    };
    let mut written = Vec::new();
    for job in JOBS {
        let data = drawn[job.file]["png"]
            .as_str()
            .and_then(decode_base64)
            .filter(|bytes| bytes.starts_with(b"\x89PNG"))
            .ok_or_else(|| format!("the browser gave no PNG for {}", job.file))?;
        let path = folder.join(job.file);
        std::fs::write(&path, &data).map_err(|error| error.to_string())?;
        written.push(json!({ "path": relative(&path), "size": format!("{0}x{0}", job.size), "bytes": data.len() }));
    }
    let base = relative(&folder);
    let src = |file: &str| {
        if base.is_empty() {
            file.to_string()
        } else {
            format!("{base}/{file}")
        }
    };
    let mut result = json!({
        "written": written,
        "background": background,
        "manifest_icons": [
            { "src": relative(&svg_path), "sizes": "any", "type": "image/svg+xml" },
            { "src": src("icon-192.png"), "sizes": "192x192", "type": "image/png" },
            { "src": src("icon-512.png"), "sizes": "512x512", "type": "image/png" },
            { "src": src("icon-maskable-512.png"), "sizes": "512x512", "type": "image/png", "purpose": "maskable" }
        ],
        "head": format!(
            "<link rel=\"icon\" href=\"{}\" type=\"image/svg+xml\">\n<link rel=\"apple-touch-icon\" href=\"{}\">",
            relative(&svg_path),
            src("apple-touch-icon.png")
        ),
        "note": "Paths are relative to the working directory: write them relative to the page that uses them. Put manifest_icons as the manifest's \"icons\", the head tags in index.html's <head>, and every file in the service worker's list.",
    });
    if text.contains("currentColor") {
        result["warning"] = json!(
            "The SVG uses currentColor, which an image draws in black: write its colors in the SVG (Icon tool `color`)."
        );
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::{app_icon, plain_color, project_folder, sized, AppIconInput};
    use serde_json::json;

    #[test]
    fn the_svg_is_given_the_size_of_each_icon() {
        let svg = r##"<?xml version="1.0"?><svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" stroke-width="2"><circle cx="12" cy="12" r="10" fill="#e0a040"/></svg>"##;
        let resized = sized(svg, 180).expect("svg");
        assert!(resized.contains(r#"width="180" height="180""#), "{resized}");
        assert!(!resized.contains(r#"width="24""#), "{resized}");
        assert!(resized.contains(r#"stroke-width="2""#), "{resized}");
        assert!(resized.starts_with("<?xml"));
        let bare = sized(
            r#"<svg viewBox="0 0 10 10"><rect width="10" height="10"/></svg>"#,
            64,
        )
        .expect("svg");
        assert!(bare.contains("xmlns=") && bare.contains(r#"width="64""#));
        // Le rectangle garde sa taille : seule la balise <svg> change.
        assert!(bare.contains(r#"<rect width="10" height="10"/>"#), "{bare}");
        assert!(sized("<svg><circle r=\"3\"/></svg>", 64).is_err());
        assert!(sized("pas de dessin", 64).is_err());
    }

    #[test]
    fn colors_and_folders_stay_plain_and_inside_the_project() {
        assert_eq!(plain_color(" #161412 ").expect("couleur"), "#161412");
        assert!(plain_color("rgb(22 20 18 / 90%)").is_ok());
        assert!(plain_color("red\"); alert(1); (\"").is_err());
        assert!(plain_color("").is_err());
        let root = std::path::Path::new("/projet");
        assert_eq!(
            project_folder(root, "./icons").expect("dossier"),
            root.join("icons")
        );
        assert!(project_folder(root, "../ailleurs").is_err());
        assert!(project_folder(root, "/etc").is_err());
    }

    #[test]
    #[ignore = "lance le navigateur installé"]
    fn png_icons_are_drawn_from_the_svg_in_a_real_browser() {
        let original = std::env::current_dir().expect("dossier");
        let dir = original.join(format!("claw-app-icons-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("icons")).expect("dossier");
        std::fs::write(
            dir.join("manifest.webmanifest"),
            r##"{ "name": "Budget", "background_color": "#161412" }"##,
        )
        .expect("manifeste");
        std::fs::write(
            dir.join("icons/icon.svg"),
            r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><circle cx="12" cy="12" r="11" fill="#e0a040"/><path d="M7 12h10M12 7v10" stroke="#161412" stroke-width="2.5" stroke-linecap="round"/></svg>"##,
        )
        .expect("svg");
        std::fs::write(
            dir.join("icons/vide.svg"),
            r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"></svg>"#,
        )
        .expect("svg");
        std::env::set_current_dir(&dir).expect("cd");
        let input: AppIconInput =
            serde_json::from_value(json!({ "svg": "icons/icon.svg" })).expect("entrée");
        let result = app_icon(&input);
        let empty: AppIconInput =
            serde_json::from_value(json!({ "svg": "icons/vide.svg" })).expect("entrée");
        let refused = app_icon(&empty);
        std::env::set_current_dir(&original).expect("cd");
        let written: Vec<(String, Vec<u8>)> = [
            "icon-192.png",
            "icon-512.png",
            "icon-maskable-512.png",
            "apple-touch-icon.png",
        ]
        .iter()
        .map(|name| {
            (
                (*name).to_string(),
                std::fs::read(dir.join("icons").join(name)).unwrap_or_default(),
            )
        })
        .collect();
        let _ = std::fs::remove_dir_all(&dir);
        let result = result.expect("icônes");
        assert_eq!(result["background"], "#161412", "{result}");
        assert_eq!(result["manifest_icons"][3]["purpose"], "maskable");
        assert!(result["head"]
            .as_str()
            .expect("head")
            .contains("icons/apple-touch-icon.png"));
        for (name, bytes) in written {
            assert!(bytes.starts_with(b"\x89PNG"), "{name}");
            // Largeur et hauteur de l'en-tête IHDR.
            let side = u32::from_be_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]);
            let expected = if name.starts_with("apple") {
                180
            } else if name.contains("192") {
                192
            } else {
                512
            };
            assert_eq!(side, expected, "{name}");
            assert!(bytes.len() > 600, "{name}: {} octets", bytes.len());
        }
        let refused = refused.expect_err("icône vide");
        assert!(refused.contains("draws almost nothing"), "{refused}");
    }
}
