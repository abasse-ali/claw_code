//! Styles de réponse (`/output-style`) : une consigne ajoutée au prompt système
//! qui change la façon dont Claw répond, sans changer ce qu'il sait faire.
//!
//! Styles intégrés : `default` (aucune consigne), `explanatory` (explique ses
//! choix), `learning` (fait écrire à l'utilisateur les morceaux qui comptent) et
//! `concise` (réponses courtes). Un style personnalisé est un fichier Markdown
//! dans `.claw/output-styles/` ou `.claude/output-styles/` (projet), ou dans
//! `~/.claw/output-styles/` (utilisateur) : l'en-tête YAML donne `name` et
//! `description`, le corps est la consigne. Le style actif est la clé
//! `outputStyle` des réglages ; `/output-style <nom>` l'enregistre dans
//! `.claw/settings.local.json`.

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

/// Clé des réglages qui porte le style actif.
pub const SETTING_KEY: &str = "outputStyle";

const STYLE_DIRS: &[&str] = &[".claw", ".claude"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StyleSource {
    Builtin,
    Project,
    User,
}

impl StyleSource {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Builtin => "intégré",
            Self::Project => "projet",
            Self::User => "utilisateur",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputStyle {
    pub name: String,
    pub description: String,
    /// Consigne ajoutée au prompt système (vide pour le style par défaut).
    pub prompt: String,
    pub source: StyleSource,
    pub path: Option<PathBuf>,
}

struct Builtin {
    name: &'static str,
    aliases: &'static [&'static str],
    description: &'static str,
    prompt: &'static str,
}

const BUILTINS: &[Builtin] = &[
    Builtin {
        name: "default",
        aliases: &["défaut", "defaut", "normal"],
        description: "Le comportement habituel de Claw : faire le travail efficacement",
        prompt: "",
    },
    Builtin {
        name: "explanatory",
        aliases: &["explicatif", "explication", "explications"],
        description: "Explique ses choix et les particularités du code au fil du travail",
        prompt: "# Output style: Explanatory\n\
Besides completing the task, help the user understand the codebase and your choices. \
Before or after significant changes, add a short block that starts with \"※ Insight\" and \
gives 2 or 3 specific points: patterns this codebase uses, why you picked one approach over \
another, trade-offs or pitfalls worth knowing. Keep each insight about this project, not \
generic advice, and keep doing the task with the same care.",
    },
    Builtin {
        name: "learning",
        aliases: &["apprentissage", "apprendre", "pedagogique", "pédagogique"],
        description:
            "Fait écrire à l'utilisateur les morceaux de logique qui comptent, pour apprendre",
        prompt: "# Output style: Learning\n\
Work with the user so they learn by doing. Write the boilerplate and the routine parts \
yourself. For meaningful pieces of logic of about 5 to 10 lines (business rules, error \
handling choices, a key algorithm step), do not write them: put a `TODO(human)` comment at \
the exact place in the code, explain in your reply what to write there and why it matters, \
then stop and ask the user to write it. Never leave more than one `TODO(human)` open at a \
time. When the user is done, review what they wrote and continue. Also add short \
\"※ Insight\" notes about the codebase's patterns and your choices.",
    },
    Builtin {
        name: "concise",
        aliases: &["concis", "court", "bref"],
        description: "Réponses courtes : le résultat d'abord, sans préambule ni récapitulatif",
        prompt: "# Output style: Concise\n\
Keep replies short. Lead with the answer or the result, without preamble and without a \
closing recap. Prefer a few bullet points to paragraphs. Show only the lines of code that \
changed, not whole files. Ask at most one question at a time.",
    },
];

/// Styles intégrés.
#[must_use]
pub fn builtin_styles() -> Vec<OutputStyle> {
    BUILTINS
        .iter()
        .map(|builtin| OutputStyle {
            name: builtin.name.to_string(),
            description: builtin.description.to_string(),
            prompt: builtin.prompt.to_string(),
            source: StyleSource::Builtin,
            path: None,
        })
        .collect()
}

/// Tous les styles : intégrés, puis ceux du projet (du dossier le plus proche
/// au plus lointain), puis ceux de l'utilisateur. Un style personnalisé du même
/// nom qu'un style déjà trouvé est ignoré.
#[must_use]
pub fn discover_styles(cwd: &Path, config_home: &Path) -> Vec<OutputStyle> {
    let mut styles = builtin_styles();
    let push = |style: OutputStyle, styles: &mut Vec<OutputStyle>| {
        if !styles
            .iter()
            .any(|existing| same_name(&existing.name, &style.name))
        {
            styles.push(style);
        }
    };
    for ancestor in cwd.ancestors() {
        for dir in STYLE_DIRS {
            for style in load_dir(
                &ancestor.join(dir).join("output-styles"),
                StyleSource::Project,
            ) {
                push(style, &mut styles);
            }
        }
    }
    for style in load_dir(&config_home.join("output-styles"), StyleSource::User) {
        push(style, &mut styles);
    }
    styles
}

/// Style nommé `name` (sans tenir compte de la casse ni des accents ; les
/// noms français des styles intégrés sont reconnus).
#[must_use]
pub fn find_style(styles: &[OutputStyle], name: &str) -> Option<OutputStyle> {
    let wanted = name.trim();
    if wanted.is_empty() {
        return None;
    }
    if let Some(builtin) = BUILTINS.iter().find(|builtin| {
        same_name(builtin.name, wanted)
            || builtin.aliases.iter().any(|alias| same_name(alias, wanted))
    }) {
        return styles
            .iter()
            .find(|style| style.source == StyleSource::Builtin && style.name == builtin.name)
            .cloned();
    }
    styles
        .iter()
        .find(|style| same_name(&style.name, wanted))
        .cloned()
}

/// Section du prompt système pour ce style (aucune pour le style par défaut).
#[must_use]
pub fn prompt_section(style: &OutputStyle) -> Option<String> {
    let prompt = style.prompt.trim();
    (!prompt.is_empty()).then(|| prompt.to_string())
}

/// Section du prompt système du style configuré (`outputStyle`), s'il y en a un.
/// Un nom inconnu est ignoré ici ; `/output-style` le signale.
#[must_use]
pub fn configured_section(
    cwd: &Path,
    config_home: &Path,
    configured: Option<&str>,
) -> Option<String> {
    let styles = discover_styles(cwd, config_home);
    find_style(&styles, configured?).and_then(|style| prompt_section(&style))
}

/// Liste des styles, le style actif marqué.
#[must_use]
pub fn render_styles_report(styles: &[OutputStyle], active: Option<&str>) -> String {
    let active = active
        .and_then(|name| find_style(styles, name))
        .map_or_else(|| "default".to_string(), |style| style.name);
    let mut report = String::from("Styles de réponse\n");
    for style in styles {
        let marker = if style.name == active { "●" } else { " " };
        let _ = writeln!(
            report,
            "  {marker} {:<16} {} ({})",
            style.name,
            style.description,
            style.source.label()
        );
    }
    let _ = write!(
        report,
        "\n  Changer          /output-style <nom> (enregistré dans .claw/settings.local.json)\n  \
Créer            /output-style new <nom> (fichier dans .claw/output-styles/)"
    );
    report
}

/// Contenu d'un nouveau style personnalisé.
#[must_use]
pub fn new_style_template(name: &str) -> String {
    format!(
        "---\nname: {name}\ndescription: Décrivez ce style en une ligne\n---\n\n\
# Output style: {name}\n\
Describe here how Claw should answer: tone, length, structure, what to explain,\n\
what to leave out. This text is added to the system prompt as is.\n"
    )
}

/// Crée `.claw/output-styles/<nom>.md` dans `cwd` et renvoie son chemin.
pub fn create_style_file(cwd: &Path, name: &str) -> Result<PathBuf, String> {
    let slug = slug(name);
    if slug.is_empty() {
        return Err("nom de style vide".to_string());
    }
    let dir = cwd.join(".claw").join("output-styles");
    let path = dir.join(format!("{slug}.md"));
    if path.exists() {
        return Err(format!("{} existe déjà", path.display()));
    }
    fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    fs::write(&path, new_style_template(name.trim())).map_err(|error| error.to_string())?;
    Ok(path)
}

/// Enregistre `outputStyle` dans `.claw/settings.local.json` (créé si besoin).
pub fn persist_style(cwd: &Path, name: &str) -> Result<PathBuf, String> {
    let path = cwd.join(".claw").join("settings.local.json");
    let mut root = match fs::read_to_string(&path) {
        Ok(contents) if !contents.trim().is_empty() => {
            serde_json::from_str::<serde_json::Value>(&contents)
                .map_err(|error| format!("{} illisible : {error}", path.display()))?
        }
        Ok(_) => serde_json::json!({}),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => serde_json::json!({}),
        Err(error) => return Err(error.to_string()),
    };
    let Some(object) = root.as_object_mut() else {
        return Err(format!("{} n'est pas un objet JSON", path.display()));
    };
    object.insert(
        SETTING_KEY.to_string(),
        serde_json::Value::String(name.to_string()),
    );
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let rendered = serde_json::to_string_pretty(&root).map_err(|error| error.to_string())?;
    fs::write(&path, format!("{rendered}\n")).map_err(|error| error.to_string())?;
    Ok(path)
}

fn load_dir(dir: &Path, source: StyleSource) -> Vec<OutputStyle> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut paths: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("md"))
        })
        .collect();
    paths.sort();
    paths
        .into_iter()
        .filter_map(|path| {
            let contents = fs::read_to_string(&path).ok()?;
            let stem = path.file_stem()?.to_string_lossy().to_string();
            let (frontmatter, body) = split_frontmatter(&contents);
            let name = frontmatter_value(frontmatter, "name").unwrap_or(stem);
            let description = frontmatter_value(frontmatter, "description")
                .unwrap_or_else(|| "style personnalisé".to_string());
            Some(OutputStyle {
                name,
                description,
                prompt: body.trim().to_string(),
                source,
                path: Some(path),
            })
        })
        .collect()
}

fn split_frontmatter(contents: &str) -> (&str, &str) {
    let trimmed = contents.trim_start_matches('\u{feff}');
    let Some(rest) = trimmed
        .strip_prefix("---\r\n")
        .or_else(|| trimmed.strip_prefix("---\n"))
    else {
        return ("", trimmed);
    };
    for (index, _) in rest.match_indices("---") {
        let at_line_start = index == 0 || rest[..index].ends_with('\n');
        if at_line_start {
            let after = &rest[index + 3..];
            let body = after
                .strip_prefix("\r\n")
                .or_else(|| after.strip_prefix('\n'))
                .unwrap_or(after);
            return (&rest[..index], body);
        }
    }
    ("", trimmed)
}

fn frontmatter_value(frontmatter: &str, key: &str) -> Option<String> {
    frontmatter.lines().find_map(|line| {
        let (name, value) = line.split_once(':')?;
        (name.trim() == key)
            .then(|| value.trim().trim_matches(['"', '\'']).trim().to_string())
            .filter(|value| !value.is_empty())
    })
}

fn same_name(left: &str, right: &str) -> bool {
    fold(left) == fold(right)
}

fn fold(value: &str) -> String {
    value
        .trim()
        .chars()
        .map(|character| match character {
            'à' | 'â' | 'ä' | 'À' | 'Â' | 'Ä' => 'a',
            'é' | 'è' | 'ê' | 'ë' | 'É' | 'È' | 'Ê' | 'Ë' => 'e',
            'î' | 'ï' | 'Î' | 'Ï' => 'i',
            'ô' | 'ö' | 'Ô' | 'Ö' => 'o',
            'ù' | 'û' | 'ü' | 'Ù' | 'Û' | 'Ü' => 'u',
            'ç' | 'Ç' => 'c',
            '_' | ' ' => '-',
            other => other.to_ascii_lowercase(),
        })
        .collect()
}

fn slug(value: &str) -> String {
    let folded = fold(value);
    let mut slug = String::new();
    for character in folded.chars() {
        if character.is_ascii_alphanumeric() {
            slug.push(character);
        } else if !slug.ends_with('-') && !slug.is_empty() {
            slug.push('-');
        }
    }
    slug.trim_end_matches('-').to_string()
}

#[cfg(test)]
mod tests {
    use super::{
        builtin_styles, configured_section, create_style_file, discover_styles, find_style,
        persist_style, prompt_section, render_styles_report, StyleSource,
    };
    use std::fs;
    use std::path::PathBuf;

    fn temp_dir(label: &str) -> PathBuf {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_nanos());
        let dir = std::env::temp_dir().join(format!("claw-styles-{label}-{stamp}"));
        fs::create_dir_all(&dir).expect("dossier temporaire");
        dir
    }

    #[test]
    fn builtin_styles_are_found_by_their_french_names() {
        let styles = builtin_styles();
        assert_eq!(
            find_style(&styles, "Explicatif").map(|style| style.name),
            Some("explanatory".to_string())
        );
        assert_eq!(
            find_style(&styles, "apprentissage").map(|style| style.name),
            Some("learning".to_string())
        );
        assert_eq!(
            find_style(&styles, "défaut").map(|style| style.name),
            Some("default".to_string())
        );
        let default = find_style(&styles, "default").expect("défaut");
        assert_eq!(prompt_section(&default), None);
        let concise = find_style(&styles, "concis").expect("concis");
        assert!(prompt_section(&concise)
            .expect("consigne")
            .contains("Concise"));
    }

    #[test]
    fn custom_styles_come_from_the_project_and_the_user() {
        let root = temp_dir("custom");
        let home = root.join("home");
        let project = root.join("projet");
        fs::create_dir_all(project.join(".claw").join("output-styles")).expect("dossier projet");
        fs::create_dir_all(home.join("output-styles")).expect("dossier utilisateur");
        fs::write(
            project.join(".claw").join("output-styles").join("revue.md"),
            "---\nname: Revue\ndescription: Relecture stricte\n---\n\nPoint every risk.\n",
        )
        .expect("style projet");
        fs::write(
            home.join("output-styles").join("perso.md"),
            "Answer like a pirate.",
        )
        .expect("style utilisateur");

        let styles = discover_styles(&project, &home);
        let revue = find_style(&styles, "revue").expect("style du projet");
        assert_eq!(revue.source, StyleSource::Project);
        assert_eq!(revue.description, "Relecture stricte");
        assert_eq!(revue.prompt, "Point every risk.");
        let perso = find_style(&styles, "perso").expect("style de l'utilisateur");
        assert_eq!(perso.source, StyleSource::User);
        assert_eq!(
            configured_section(&project, &home, Some("Revue")).as_deref(),
            Some("Point every risk.")
        );
        assert_eq!(configured_section(&project, &home, Some("inconnu")), None);
        let report = render_styles_report(&styles, Some("revue"));
        assert!(report.contains("● Revue"));
        fs::remove_dir_all(root).expect("nettoyage");
    }

    #[test]
    fn a_new_style_file_and_the_setting_are_written_in_the_project() {
        let root = temp_dir("persist");
        let path = create_style_file(&root, "Mon Style").expect("création");
        assert!(path.ends_with("mon-style.md"));
        assert!(create_style_file(&root, "Mon Style").is_err());
        fs::write(
            root.join(".claw").join("settings.local.json"),
            r#"{"permissions":{"allow":["Bash(ls)"]}}"#,
        )
        .expect("réglages");
        let settings = persist_style(&root, "explanatory").expect("enregistrement");
        let saved: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(settings).expect("lecture")).expect("JSON");
        assert_eq!(saved["outputStyle"], "explanatory");
        assert_eq!(saved["permissions"]["allow"][0], "Bash(ls)");
        fs::remove_dir_all(root).expect("nettoyage");
    }
}
