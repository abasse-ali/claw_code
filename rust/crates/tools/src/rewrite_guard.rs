//! Garde-fou des réécritures : un fichier existant réécrit d'un bloc peut perdre
//! ce qui marchait. Vu le 25 octobre 2026 (site d'un club de foot, huit
//! passages) : trois passages ont réécrit `index.html`, `styles.css` et
//! `app.js` en entier, et chaque fois des choses qui allaient se sont cassées
//! (l'en-tête coupé, un formulaire qui n'enregistrait plus).
//!
//! Après une écriture, le modèle apprend ce que la nouvelle version a retiré
//! (identifiants d'une page, fonctions et clés enregistrées d'un script) et ce
//! que les scripts du dossier cherchent encore dans des pages qui ne l'ont plus.

use std::collections::BTreeSet;
use std::path::Path;

/// Un fichier plus court n'est pas « réécrit » : il est écrit.
const REWRITE_BYTES: usize = 1500;
/// Noms cités par catégorie, au plus.
const MAX_NAMES: usize = 10;

/// Les valeurs entre guillemets qui suivent chaque `marker` dans `text`
/// (`id="hero"`, `getItem('club')`) ; seulement celles faites de lettres,
/// chiffres, `-` et `_`.
fn quoted_after(text: &str, marker: &str) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    let mut rest = text;
    while let Some(at) = rest.find(marker) {
        rest = &rest[at + marker.len()..];
        let Some(quote) = rest
            .chars()
            .next()
            .filter(|quote| matches!(quote, '"' | '\'' | '`'))
        else {
            continue;
        };
        let value: String = rest[1..]
            .chars()
            .take_while(|character| *character != quote)
            .collect();
        if !value.is_empty()
            && value.chars().all(|character| {
                character.is_ascii_alphanumeric() || matches!(character, '-' | '_')
            })
        {
            found.insert(value);
        }
    }
    found
}

/// Identifiants d'une page HTML (`id="…"`).
fn page_ids(html: &str) -> BTreeSet<String> {
    let mut ids = quoted_after(html, " id=");
    ids.extend(quoted_after(html, "\tid="));
    ids.extend(quoted_after(html, "\nid="));
    ids
}

/// Fonctions nommées d'un script (`function nom(`).
fn functions(script: &str) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    let mut rest = script;
    while let Some(at) = rest.find("function ") {
        rest = &rest[at + "function ".len()..];
        let name: String = rest
            .chars()
            .take_while(|character| {
                character.is_ascii_alphanumeric() || matches!(character, '_' | '$')
            })
            .collect();
        if !name.is_empty() && rest[name.len()..].trim_start().starts_with('(') {
            found.insert(name);
        }
    }
    found
}

/// Clés que le script enregistre ou relit (`localStorage.getItem('clé')`).
fn storage_keys(script: &str) -> BTreeSet<String> {
    let mut keys = quoted_after(script, "getItem(");
    keys.extend(quoted_after(script, "setItem("));
    keys
}

/// Identifiants qu'un script cherche dans la page (`getElementById('x')`,
/// `querySelector('#x')`).
fn wanted_ids(script: &str) -> BTreeSet<String> {
    let mut ids = quoted_after(script, "getElementById(");
    for marker in ["querySelector(", "querySelectorAll("] {
        let mut rest = script;
        while let Some(at) = rest.find(marker) {
            rest = &rest[at + marker.len()..];
            let Some(quote) = rest
                .chars()
                .next()
                .filter(|quote| matches!(quote, '"' | '\''))
            else {
                continue;
            };
            let selector: String = rest[1..]
                .chars()
                .take_while(|character| *character != quote)
                .collect();
            // Un sélecteur qui n'est qu'un identifiant : « #hero ».
            if let Some(id) = selector.strip_prefix('#') {
                if !id.is_empty()
                    && id.chars().all(|character| {
                        character.is_ascii_alphanumeric() || matches!(character, '-' | '_')
                    })
                {
                    ids.insert(id.to_string());
                }
            }
        }
    }
    ids
}

fn is_page(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            extension.eq_ignore_ascii_case("html") || extension.eq_ignore_ascii_case("htm")
        })
}

fn is_script(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| matches!(extension, "js" | "mjs"))
}

fn list(names: &BTreeSet<String>, prefix: &str) -> String {
    let shown: Vec<String> = names
        .iter()
        .take(MAX_NAMES)
        .map(|name| format!("{prefix}{name}"))
        .collect();
    let more = names.len().saturating_sub(MAX_NAMES);
    if more > 0 {
        format!("{} and {more} more", shown.join(", "))
    } else {
        shown.join(", ")
    }
}

/// Ce que la réécriture de `path` (`before` : le fichier d'avant, s'il y en
/// avait un) a retiré.
fn removed(path: &Path, before: &str, after: &str) -> Vec<String> {
    let mut lost = Vec::new();
    if is_page(path) {
        let gone: BTreeSet<String> = page_ids(before)
            .difference(&page_ids(after))
            .cloned()
            .collect();
        if !gone.is_empty() {
            lost.push(format!("element ids {}", list(&gone, "#")));
        }
    }
    if is_script(path) {
        let gone: BTreeSet<String> = functions(before)
            .difference(&functions(after))
            .cloned()
            .collect();
        if !gone.is_empty() {
            lost.push(format!("functions {}", list(&gone, "")));
        }
        let gone: BTreeSet<String> = storage_keys(before)
            .difference(&storage_keys(after))
            .cloned()
            .collect();
        if !gone.is_empty() {
            lost.push(format!(
                "saved keys {} (what visitors saved under them is no longer read)",
                list(&gone, "")
            ));
        }
    }
    lost
}

/// Identifiants que les scripts du dossier de `path` cherchent et qu'aucune
/// page du dossier n'a : leurs boutons et formulaires ne marchent plus.
fn dangling(path: &Path) -> Vec<String> {
    let Some(folder) = path.parent() else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(if folder.as_os_str().is_empty() {
        Path::new(".")
    } else {
        folder
    }) else {
        return Vec::new();
    };
    let mut defined = BTreeSet::new();
    let mut scripts = Vec::new();
    let mut pages = 0;
    for entry in entries.flatten().take(200) {
        let file = entry.path();
        let Ok(text) = std::fs::read_to_string(&file) else {
            continue;
        };
        if is_page(&file) {
            pages += 1;
            defined.extend(page_ids(&text));
        } else if is_script(&file) {
            scripts.push((file, text));
        }
    }
    if pages == 0 {
        return Vec::new();
    }
    let mut notes = Vec::new();
    for (file, text) in scripts {
        let missing: BTreeSet<String> = wanted_ids(&text).difference(&defined).cloned().collect();
        if !missing.is_empty() {
            let name = file
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("a script");
            notes.push(format!(
                "{name} looks for {}, which no page in this folder has",
                list(&missing, "#")
            ));
        }
    }
    notes
}

/// La remarque au modèle après l'écriture de `path`, ou `None` quand rien ne
/// s'est perdu. `before` : le fichier tel qu'il était, s'il existait.
#[must_use]
pub fn note(path: &Path, before: Option<&str>, after: &str) -> Option<String> {
    let mut lines = Vec::new();
    if let Some(before) = before.filter(|before| before.len() >= REWRITE_BYTES) {
        let lost = removed(path, before, after);
        if !lost.is_empty() {
            lines.push(format!(
                "This write replaced the whole file and removed: {}. If they still matter (other files use them, or they made something work), put them back.",
                lost.join("; ")
            ));
        }
    }
    if is_page(path) || is_script(path) {
        lines.extend(dangling(path));
    }
    (!lines.is_empty()).then(|| {
        format!(
            "[Claw] {} To change part of a file without losing the rest, use edit_file.",
            lines.join(" ")
        )
    })
}

#[cfg(test)]
mod tests {
    use super::{functions, note, page_ids, storage_keys, wanted_ids};
    use std::path::Path;

    #[test]
    fn names_are_read_from_pages_and_scripts() {
        let page = r#"<header id="top"><button id='je-conduis' class="b">x</button><div data-id="non"></div>"#;
        assert_eq!(
            page_ids(page).into_iter().collect::<Vec<_>>(),
            ["je-conduis", "top"]
        );
        let script = "function render(list) {}\nconst f = function () {};\nasync function save (x) {}\nlocalStorage.setItem('club-drivers', v); localStorage.getItem(\"club-drivers\");\ndocument.getElementById('hero'); document.querySelector('#menu'); document.querySelector('.card');";
        assert_eq!(
            functions(script).into_iter().collect::<Vec<_>>(),
            ["render", "save"]
        );
        assert_eq!(
            storage_keys(script).into_iter().collect::<Vec<_>>(),
            ["club-drivers"]
        );
        assert_eq!(
            wanted_ids(script).into_iter().collect::<Vec<_>>(),
            ["hero", "menu"]
        );
    }

    #[test]
    fn a_rewrite_that_loses_names_and_breaks_a_script_is_said() {
        let dir = std::env::temp_dir().join(format!("claw-rewrite-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dossier");
        let filler = "<p>texte</p>\n".repeat(150);
        let before =
            format!("<main id=\"hero\"><form id=\"form-je-conduis\"></form>{filler}</main>");
        let after = format!("<main id=\"hero\">{filler}</main>");
        std::fs::write(dir.join("index.html"), &after).expect("page");
        std::fs::write(
            dir.join("app.js"),
            "document.getElementById('form-je-conduis').onsubmit = save;",
        )
        .expect("script");
        let said = note(&dir.join("index.html"), Some(&before), &after).expect("remarque");
        let _ = std::fs::remove_dir_all(&dir);
        assert!(
            said.contains("removed: element ids #form-je-conduis"),
            "{said}"
        );
        assert!(
            said.contains("app.js looks for #form-je-conduis, which no page in this folder has"),
            "{said}"
        );
        // Une petite page neuve, sans script qui manque de rien : rien à dire.
        assert_eq!(note(Path::new("neuf/index.html"), None, "<p>x</p>"), None);
    }
}
