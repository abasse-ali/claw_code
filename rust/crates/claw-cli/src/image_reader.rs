//! Lecteur d'images : quand le modèle de la session ne lit que du texte, un
//! modèle gratuit qui lit les images décrit chacune d'elles, une seule fois,
//! et sa description prend la place de l'image dans ce que reçoit le modèle.
//! Sans cela, `OpenRouter` refuse toute requête qui porte une image pour un
//! modèle qui n'en lit pas (erreur 404).

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use api::{ImageSource, InputContentBlock, InputMessage};

/// Mis à la place d'une image que personne n'a pu décrire.
const IMAGE_NOT_SENT: &str =
    "[Image not sent: the current model cannot read images. Ask the user to describe it if you need its content.]";

const READER_SYSTEM: &str = "You describe images for a software engineering assistant that cannot see them. Be exact and complete, and never guess at what is not visible.";

const READER_PROMPT: &str = "Describe this image so that someone who cannot see it can work from your description alone.\n- Transcribe every visible piece of text exactly: error messages, code, commands, file names, labels, values, titles. Keep the line breaks of code and logs.\n- Say what kind of image it is (screenshot of an editor, a terminal, a web page or an application, a diagram, a photo) and describe its layout: which windows, panels or sections it has and what each one contains.\n- Point out what stands out: errors, warnings, selected or highlighted elements, anything that looks wrong, stuck or unfinished.\n- For a diagram or a chart, give its elements, how they relate and the numbers shown.\nWrite plain text, without any preamble.";

/// Jetons accordés à une description.
const DESCRIPTION_TOKENS: u32 = 2_500;
/// Modèles essayés pour une image avant de renoncer.
const READERS_TRIED: usize = 3;

/// Réglage `imageReader` (`CLAW_IMAGE_READER`).
#[derive(Debug, Clone, PartialEq, Eq)]
enum Reader {
    /// Les images ne sont pas décrites : le modèle apprend seulement qu'il en manque une.
    Off,
    /// Les modèles gratuits qui lisent les images, le plus puissant d'abord.
    Auto,
    /// Le modèle que l'utilisateur a écrit lui-même.
    Model(String),
}

impl Reader {
    fn parse(setting: Option<&str>) -> Self {
        match setting.map(str::trim).filter(|value| !value.is_empty()) {
            None => Self::Auto,
            Some(value) => match value.to_ascii_lowercase().as_str() {
                "off" | "none" | "non" | "aucun" | "false" | "0" => Self::Off,
                "auto" | "free" | "gratuit" => Self::Auto,
                _ => Self::Model(api::resolve_model_alias(value)),
            },
        }
    }

    fn configured() -> Self {
        let setting = std::env::var("CLAW_IMAGE_READER")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .or_else(|| {
                let cwd = std::env::current_dir().ok()?;
                let config = runtime::ConfigLoader::default_for(&cwd).load().ok()?;
                config.image_reader().map(ToOwned::to_owned)
            });
        Self::parse(setting.as_deref())
    }

    /// Modèles à qui demander la description, dans l'ordre.
    fn models(&self, current: &str) -> Vec<String> {
        match self {
            Self::Off => Vec::new(),
            Self::Auto => crate::models::free_image_readers(current)
                .into_iter()
                .take(READERS_TRIED)
                .collect(),
            Self::Model(model) => vec![model.clone()],
        }
    }
}

/// Description d'une image et modèle qui l'a faite.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Description {
    model: String,
    text: String,
}

/// Descriptions connues de ce processus, par empreinte d'image.
static DESCRIPTIONS: Mutex<BTreeMap<String, Description>> = Mutex::new(BTreeMap::new());
/// Images qu'aucun modèle n'a su décrire : pas de nouvel essai à chaque requête.
static UNREADABLE: Mutex<BTreeSet<String>> = Mutex::new(BTreeSet::new());
/// Dernier état signalé à l'utilisateur : modèle, images décrites, images absentes.
static LAST_NOTICE: Mutex<(String, usize, usize)> = Mutex::new((String::new(), 0, 0));

fn descriptions() -> std::sync::MutexGuard<'static, BTreeMap<String, Description>> {
    DESCRIPTIONS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn unreadable() -> std::sync::MutexGuard<'static, BTreeSet<String>> {
    UNREADABLE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Empreinte d'une image (FNV-1a sur 64 bits de ses données) : nom de son
/// fichier de description.
fn image_key(source: &ImageSource) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in source.data.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}-{}", source.data.len())
}

/// Dossier du projet où les descriptions sont gardées d'une session à l'autre.
fn cache_dir() -> Option<PathBuf> {
    Some(
        std::env::current_dir()
            .ok()?
            .join(".claw")
            .join("image-descriptions"),
    )
}

fn load(dir: &Path, key: &str) -> Option<Description> {
    let contents = std::fs::read_to_string(dir.join(format!("{key}.txt"))).ok()?;
    let (header, text) = contents.split_once("\n\n")?;
    let model = header.strip_prefix("model: ")?.trim();
    (!text.trim().is_empty()).then(|| Description {
        model: model.to_string(),
        text: text.trim().to_string(),
    })
}

fn save(dir: &Path, key: &str, description: &Description) {
    if std::fs::create_dir_all(dir).is_ok() {
        let contents = format!("model: {}\n\n{}\n", description.model, description.text);
        let _ = std::fs::write(dir.join(format!("{key}.txt")), contents);
    }
}

/// Le catalogue dit que ce modèle ne lit pas les images. Faux pour un modèle
/// inconnu du catalogue (serveur local, autre fournisseur) : on ne sait pas.
pub(crate) fn model_cannot_read_images(model: &str) -> bool {
    api::model_limits(&api::resolve_model_alias(model))
        .is_some_and(|limits| !limits.supports_images)
}

/// Images de la conversation, dans l'ordre.
fn images(messages: &[InputMessage]) -> Vec<&ImageSource> {
    messages
        .iter()
        .flat_map(|message| &message.content)
        .filter_map(|block| match block {
            InputContentBlock::Image { source } => Some(source),
            _ => None,
        })
        .collect()
}

/// Ce qui prend la place d'une image dans la requête à un modèle qui n'en lit
/// pas : sa description si elle est connue, sinon la mention qu'elle manque.
pub(crate) fn replacement(source: &ImageSource) -> String {
    match descriptions().get(&image_key(source)) {
        Some(description) => format!(
            "[Image described by {} because the current model cannot read images. The description may miss a detail: ask the user when one matters.]\n{}",
            description.model, description.text
        ),
        None => IMAGE_NOT_SENT.to_string(),
    }
}

/// Avant une requête à `model` : s'il ne lit pas les images, fait décrire
/// celles de la conversation qui ne l'ont pas encore été. `announce` reçoit ce
/// qu'il faut dire à l'utilisateur, au fur et à mesure.
pub(crate) fn prepare(model: &str, messages: &[InputMessage], announce: &mut dyn FnMut(&str)) {
    if !model_cannot_read_images(model) || images(messages).is_empty() {
        return;
    }
    let Some(dir) = cache_dir() else {
        return;
    };
    // Les lecteurs (réglages, catalogue) ne sont cherchés que s'il reste une
    // image à décrire : la plupart des requêtes n'en ont pas besoin.
    let found = std::cell::OnceCell::new();
    let readers = || {
        found
            .get_or_init(|| Reader::configured().models(model))
            .clone()
    };
    prepare_with(model, messages, &dir, &readers, announce, |source| {
        tools::advisors::ask_about_image(
            &readers(),
            READER_SYSTEM,
            READER_PROMPT,
            source,
            DESCRIPTION_TOKENS,
        )
        .map(|opinion| (opinion.model, opinion.text))
    });
}

/// Le même travail, avec le dossier des descriptions, les modèles lecteurs et
/// l'appel au lecteur donnés par l'appelant.
fn prepare_with(
    model: &str,
    messages: &[InputMessage],
    dir: &Path,
    readers: &dyn Fn() -> Vec<String>,
    announce: &mut dyn FnMut(&str),
    describe: impl Fn(&ImageSource) -> Result<(String, String), String>,
) {
    let images = images(messages);
    if images.is_empty() {
        return;
    }
    // Descriptions déjà faites, dans ce processus ou lors d'une session passée.
    let mut missing = Vec::new();
    for source in &images {
        let key = image_key(source);
        if descriptions().contains_key(&key) {
            continue;
        }
        if let Some(known) = load(dir, &key) {
            descriptions().insert(key, known);
        } else if !unreadable().contains(&key) && !missing.iter().any(|(known, _)| *known == key) {
            missing.push((key, *source));
        }
    }

    let mut failure = None;
    let had_missing = !missing.is_empty();
    let readers = if had_missing { readers() } else { Vec::new() };
    if had_missing && !readers.is_empty() {
        announce(&format!(
            "{model} ne lit pas les images : description de {} demandée à un modèle qui les lit ({})…",
            count(missing.len(), "image"),
            readers[0]
        ));
        for (key, source) in missing {
            match describe(source) {
                Ok((reader, text)) if !text.trim().is_empty() => {
                    let description = Description {
                        model: reader,
                        text: text.trim().to_string(),
                    };
                    save(dir, &key, &description);
                    descriptions().insert(key, description);
                }
                Ok(_) => {
                    unreadable().insert(key);
                    failure = Some("description vide".to_string());
                }
                Err(error) => {
                    unreadable().insert(key);
                    failure = Some(error);
                    // Les mêmes modèles échoueraient pour les images suivantes.
                    break;
                }
            }
        }
    }

    let described = images
        .iter()
        .filter(|source| descriptions().contains_key(&image_key(source)))
        .count();
    let absent = images.len() - described;
    let state = (model.to_string(), described, absent);
    let mut last = LAST_NOTICE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if *last == state {
        return;
    }
    *last = state;
    let mut notice = format!("{model} ne lit pas les images");
    if described > 0 {
        let reader = images
            .iter()
            .find_map(|source| {
                descriptions()
                    .get(&image_key(source))
                    .map(|d| d.model.clone())
            })
            .unwrap_or_default();
        let _ = write!(
            notice,
            " : il reçoit à la place la description de {} par {reader}",
            count(described, "image")
        );
    }
    if absent > 0 {
        let why = match &failure {
            Some(error) => error.clone(),
            None if had_missing => {
                "aucun modèle gratuit qui lit les images n'est disponible".to_string()
            }
            None => "un essai précédent a échoué".to_string(),
        };
        let (could, sent) = if absent == 1 {
            ("n'a pas pu lui être décrite", "ne lui est pas envoyée")
        } else {
            ("n'ont pas pu lui être décrites", "ne lui sont pas envoyées")
        };
        let _ = write!(
            notice,
            "{} {} {could} ({why}) et {sent}. Un modèle qui lit les images y aurait accès (/models vision)",
            if described > 0 { " ;" } else { " :" },
            count(absent, "image")
        );
    }
    notice.push('.');
    announce(&notice);
}

/// « 1 image », « 2 images ».
fn count(number: usize, noun: &str) -> String {
    format!("{number} {noun}{}", if number > 1 { "s" } else { "" })
}

#[cfg(test)]
mod tests {
    use super::{image_key, load, prepare_with, replacement, save, Description, Reader};
    use api::{ImageSource, InputContentBlock, InputMessage};
    use std::cell::RefCell;

    fn image(data: &str) -> ImageSource {
        ImageSource::base64("image/png".to_string(), data.to_string())
    }

    fn conversation(images: &[&ImageSource]) -> Vec<InputMessage> {
        vec![InputMessage {
            role: "user".to_string(),
            content: std::iter::once(InputContentBlock::Text {
                text: "regarde".to_string(),
            })
            .chain(images.iter().map(|source| InputContentBlock::Image {
                source: (*source).clone(),
            }))
            .collect(),
        }]
    }

    fn temp_dir(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("claw-images-{}-{name}", std::process::id()))
    }

    #[test]
    fn the_setting_names_a_reader_or_switches_it_off() {
        assert_eq!(Reader::parse(None), Reader::Auto);
        assert_eq!(Reader::parse(Some(" gratuit ")), Reader::Auto);
        assert_eq!(Reader::parse(Some("off")), Reader::Off);
        assert_eq!(
            Reader::parse(Some("acme/vision:free")),
            Reader::Model("acme/vision:free".to_string())
        );
        assert!(Reader::Off.models("x").is_empty());
        assert_eq!(
            Reader::Model("acme/vision:free".to_string()).models("x"),
            ["acme/vision:free"]
        );
    }

    #[test]
    fn a_description_is_kept_on_disk_under_the_fingerprint_of_its_image() {
        let dir = temp_dir("disque");
        let first = image("AAAAimage-une");
        assert_ne!(image_key(&first), image_key(&image("AAAAimage-deux")));
        assert_eq!(image_key(&first), image_key(&image("AAAAimage-une")));
        let description = Description {
            model: "acme/vision:free".to_string(),
            text: "Un terminal.\n\nErreur : fichier introuvable".to_string(),
        };
        assert_eq!(load(&dir, &image_key(&first)), None);
        save(&dir, &image_key(&first), &description);
        assert_eq!(load(&dir, &image_key(&first)), Some(description));
        std::fs::remove_dir_all(&dir).expect("nettoyage");
    }

    #[test]
    fn images_are_described_once_then_reused() {
        let dir = temp_dir("une-fois");
        let (first, second) = (image("essai-lecteur-1"), image("essai-lecteur-2"));
        let messages = conversation(&[&first, &second, &first]);
        let readers = vec!["acme/vision:free".to_string()];
        let asked = RefCell::new(0);
        let notices = RefCell::new(Vec::new());
        let run = || {
            prepare_with(
                "acme/texte:free",
                &messages,
                &dir,
                &|| readers.clone(),
                &mut |text: &str| notices.borrow_mut().push(text.to_string()),
                |source| {
                    *asked.borrow_mut() += 1;
                    Ok((
                        "acme/vision:free".to_string(),
                        format!("Capture {}", source.data),
                    ))
                },
            );
        };
        run();
        // Deux images différentes (l'une figure deux fois) : deux descriptions.
        assert_eq!(*asked.borrow(), 2);
        assert_eq!(
            *notices.borrow(),
            [
                "acme/texte:free ne lit pas les images : description de 2 images demandée à un modèle qui les lit (acme/vision:free)…",
                "acme/texte:free ne lit pas les images : il reçoit à la place la description de 3 images par acme/vision:free.",
            ]
        );
        assert_eq!(
            replacement(&first),
            "[Image described by acme/vision:free because the current model cannot read images. The description may miss a detail: ask the user when one matters.]\nCapture essai-lecteur-1"
        );
        // Requête suivante : rien n'est redemandé, rien n'est redit.
        run();
        assert_eq!(*asked.borrow(), 2);
        assert_eq!(notices.borrow().len(), 2);
        std::fs::remove_dir_all(&dir).expect("nettoyage");
    }

    #[test]
    fn an_image_nobody_can_describe_is_reported_and_not_retried() {
        let dir = temp_dir("echec");
        let lost = image("essai-lecteur-illisible");
        let messages = conversation(&[&lost]);
        let readers = vec!["acme/vision:free".to_string()];
        let asked = RefCell::new(0);
        let notices = RefCell::new(Vec::new());
        let run = |readers: &[String]| {
            prepare_with(
                "acme/texte-2:free",
                &messages,
                &dir,
                &|| readers.to_vec(),
                &mut |text: &str| notices.borrow_mut().push(text.to_string()),
                |_| {
                    *asked.borrow_mut() += 1;
                    Err("erreur 429".to_string())
                },
            );
        };
        run(&readers);
        run(&readers);
        assert_eq!(*asked.borrow(), 1);
        assert_eq!(
            notices.borrow().last().map(String::as_str),
Some("acme/texte-2:free ne lit pas les images : 1 image n'a pas pu lui être décrite (erreur 429) et ne lui est pas envoyée. Un modèle qui lit les images y aurait accès (/models vision).")
        );
        assert!(replacement(&lost).starts_with("[Image not sent"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
