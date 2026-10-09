//! Variables d'environnement de l'utilisateur que le programme qui a lancé
//! Claw ne lui a pas transmises.
//!
//! Sous Windows, un programme garde l'environnement qu'il a reçu à son
//! démarrage. VS Code ouvert par le menu contextuel de l'Explorateur, une
//! tâche planifiée ou un terminal resté ouvert ignorent donc une variable
//! définie depuis : la clé `OPENROUTER_API_KEY` existe dans le compte, mais
//! Claw ne la voit pas (« clé manquante »). Au démarrage, Claw relit les
//! variables du compte dans le registre et adopte celles qui lui servent et
//! que son environnement n'a pas. Une variable déjà présente n'est jamais
//! remplacée. `CLAW_NO_USER_ENV=1` coupe cette reprise.

use std::sync::OnceLock;

/// Noms des variables reprises au démarrage (jamais leurs valeurs).
static ADOPTED: OnceLock<Vec<String>> = OnceLock::new();

/// Variables utiles à Claw : clés et adresses des fournisseurs, réglages
/// `CLAW_*`, serveur Ollama.
fn wanted(name: &str) -> bool {
    let name = name.to_ascii_uppercase();
    name.starts_with("CLAW_")
        || name.starts_with("OLLAMA_")
        || ["_API_KEY", "_AUTH_TOKEN", "_BASE_URL"]
            .iter()
            .any(|suffix| name.ends_with(suffix))
}

/// Variables listées par `reg query` : une ligne `nom    type    valeur` par
/// variable. Seules les chaînes simples sont gardées ; une valeur qui fait
/// référence à une autre variable (`%TEMP%`) est laissée de côté.
#[cfg_attr(not(windows), allow(dead_code))]
fn parse_reg_query(output: &str) -> Vec<(String, String)> {
    output
        .lines()
        .filter_map(|line| {
            let line = line.strip_prefix("    ")?;
            let mut fields = line.splitn(3, "    ");
            let name = fields.next()?.trim();
            let kind = fields.next()?.trim();
            let value = fields.next()?.trim_end_matches(['\r', '\n']);
            let plain = matches!(kind, "REG_SZ" | "REG_EXPAND_SZ") && !value.contains('%');
            (plain && !name.is_empty() && !value.trim().is_empty())
                .then(|| (name.to_string(), value.to_string()))
        })
        .collect()
}

/// Parmi `stored`, les variables utiles à Claw que l'environnement n'a pas
/// (`present` : la variable existe déjà, même vide).
fn missing_from(
    stored: Vec<(String, String)>,
    present: impl Fn(&str) -> bool,
) -> Vec<(String, String)> {
    let mut adopted: Vec<(String, String)> = Vec::new();
    for (name, value) in stored {
        let known = adopted
            .iter()
            .any(|(other, _)| other.eq_ignore_ascii_case(&name));
        if wanted(&name) && !present(&name) && !known {
            adopted.push((name, value));
        }
    }
    adopted
}

/// Variables du compte de l'utilisateur.
const USER_KEY: &str = r"HKCU\Environment";
/// Variables de la machine, communes à tous les comptes.
const MACHINE_KEY: &str = r"HKLM\SYSTEM\CurrentControlSet\Control\Session Manager\Environment";

/// Variables enregistrées sous `key` dans le registre (une lecture, sans fenêtre).
#[cfg(windows)]
fn stored_variables(key: &str) -> Vec<(String, String)> {
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Stdio};
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    // Le programme du système, pas un `reg` trouvé dans le PATH.
    let reg = std::env::var_os("SystemRoot").map_or_else(
        || std::path::PathBuf::from("reg.exe"),
        |root| std::path::Path::new(&root).join("System32").join("reg.exe"),
    );
    Command::new(reg)
        .args(["query", key])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| parse_reg_query(&String::from_utf8_lossy(&output.stdout)))
        .unwrap_or_default()
}

#[cfg(not(windows))]
fn stored_variables(_key: &str) -> Vec<(String, String)> {
    Vec::new()
}

/// Adopte les variables de `key` qui manquent ; renvoie leurs noms.
fn adopt_from(key: &str) -> Vec<String> {
    missing_from(stored_variables(key), |name| {
        std::env::var_os(name).is_some()
    })
    .into_iter()
    .map(|(name, value)| {
        std::env::set_var(&name, value);
        name
    })
    .collect()
}

/// L'environnement contient une clé de fournisseur.
fn has_a_key() -> bool {
    std::env::vars_os().any(|(name, value)| {
        let name = name.to_string_lossy().to_ascii_uppercase();
        (name.ends_with("_API_KEY") || name.ends_with("_AUTH_TOKEN")) && !value.is_empty()
    })
}

/// À appeler au tout début, avant que d'autres threads ne démarrent : adopte
/// les variables du compte qui manquent à l'environnement de Claw. Celles de
/// la machine ne sont lues que si aucune clé n'a été trouvée. Rien n'est lu
/// pour afficher la version ou l'aide.
pub(crate) fn adopt() {
    let disabled = std::env::var("CLAW_NO_USER_ENV").is_ok_and(|value| {
        !matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "" | "0" | "false" | "non" | "no" | "off"
        )
    });
    let trivial = std::env::args().nth(1).is_some_and(|argument| {
        matches!(
            argument.as_str(),
            "--version" | "-V" | "version" | "--help" | "-h" | "help"
        )
    });
    if disabled || trivial || !cfg!(windows) {
        return;
    }
    let mut names = adopt_from(USER_KEY);
    if !has_a_key() {
        names.extend(adopt_from(MACHINE_KEY));
    }
    let _ = ADOPTED.set(names);
}

/// `name` vient des variables du compte, pas du programme qui a lancé Claw.
pub(crate) fn was_adopted(name: &str) -> bool {
    ADOPTED
        .get()
        .is_some_and(|names| names.iter().any(|other| other.eq_ignore_ascii_case(name)))
}

#[cfg(test)]
mod tests {
    use super::{missing_from, parse_reg_query, wanted};

    #[test]
    fn reads_plain_variables_from_a_registry_listing() {
        let listing = "\r\nHKEY_CURRENT_USER\\Environment\r\n    OPENROUTER_API_KEY    REG_SZ    cle-d-essai\r\n    Path    REG_EXPAND_SZ    C:\\Outils;%USERPROFILE%\\bin\r\n    OLLAMA_MODELS    REG_SZ    D:\\Modeles  Ollama\r\n    VIDE    REG_SZ    \r\n    Compteur    REG_DWORD    0x1\r\n\r\n";
        assert_eq!(
            parse_reg_query(listing),
            [
                ("OPENROUTER_API_KEY".to_string(), "cle-d-essai".to_string()),
                (
                    "OLLAMA_MODELS".to_string(),
                    "D:\\Modeles  Ollama".to_string()
                ),
            ]
        );
        assert!(parse_reg_query("ERREUR : cle introuvable").is_empty());
    }

    #[test]
    fn adopts_only_what_claw_uses_and_never_overrides() {
        for name in [
            "OPENROUTER_API_KEY",
            "Groq_Api_Key",
            "ANTHROPIC_AUTH_TOKEN",
            "OLLAMA_BASE_URL",
            "OLLAMA_CONTEXT_LENGTH",
            "CLAW_SEARXNG_URL",
        ] {
            assert!(wanted(name), "{name}");
        }
        for name in ["Path", "TEMP", "JAVA_HOME", "API_KEY_BACKUP_DIR"] {
            assert!(!wanted(name), "{name}");
        }

        let stored = |pairs: &[(&str, &str)]| {
            pairs
                .iter()
                .map(|(name, value)| ((*name).to_string(), (*value).to_string()))
                .collect::<Vec<_>>()
        };
        // Variable du compte d'abord, puis celle de la machine du même nom.
        let adopted = missing_from(
            stored(&[
                ("OPENROUTER_API_KEY", "du-compte"),
                ("GROQ_API_KEY", "déjà-dans-le-processus"),
                ("Path", "C:\\Outils"),
                ("openrouter_api_key", "de-la-machine"),
                ("CLAW_MAX_TOKENS", "8000"),
            ]),
            |name| name == "GROQ_API_KEY",
        );
        assert_eq!(
            adopted,
            stored(&[
                ("OPENROUTER_API_KEY", "du-compte"),
                ("CLAW_MAX_TOKENS", "8000")
            ])
        );
    }
}
