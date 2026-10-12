//! Mode équipe : au lieu d'un seul modèle pour tout, Claw confie chaque étape
//! du travail au modèle gratuit le plus fort pour elle. Comprendre et
//! préparer, écrire le code, déboguer après un échec, relire, lire une image,
//! traiter un sujet spécialisé (traduction, droit, finance…) : chaque rôle a
//! son modèle, choisi d'après le catalogue et les mesures de Claw.
//!
//! Le runtime sait où en est le travail (fichiers modifiés, contrôle en échec,
//! remarque d'un contrôle de Claw) : il annonce le rôle de chaque étape au
//! client, qui choisit le modèle. Chaque modèle reçoit la conversation entière
//! et une consigne qui lui dit son rôle et qu'il reprend le travail des autres
//! membres : il sait toujours où en est la tâche et ce qu'il lui reste à faire.

use std::sync::atomic::{AtomicBool, Ordering};

use crate::harness::{self, Aid};

static TEAM: AtomicBool = AtomicBool::new(false);

/// Active ou coupe le mode équipe pour la session.
pub fn set_team(on: bool) {
    TEAM.store(on, Ordering::SeqCst);
}

/// Le mode équipe est actif (et pas coupé par `CLAW_HARNESS=0`).
#[must_use]
pub fn team() -> bool {
    TEAM.load(Ordering::SeqCst) && harness::enabled(Aid::Team)
}

/// Rôle d'une étape du travail.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// Comprendre la demande, lire le code, préparer le plan.
    Plan,
    /// Écrire et modifier le code.
    Code,
    /// Un contrôle ou un outil vient d'échouer : trouver la cause, corriger.
    Debug,
    /// Relire ou vérifier le travail d'un autre (vérificateur ultracode).
    Review,
    /// Demande hors programmation, sur un sujet spécialisé : clé de catégorie
    /// d'`OpenRouter` (`translation`, `legal`, `finance`…).
    Topic(&'static str),
}

impl Role {
    /// Clé du rôle, aussi celle du réglage `teamModels`.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Plan => "plan",
            Self::Code => "code",
            Self::Debug => "debug",
            Self::Review => "review",
            Self::Topic(category) => category,
        }
    }

    /// Nom du rôle pour l'utilisateur.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Plan => "comprendre et préparer",
            Self::Code => "écrire le code",
            Self::Debug => "déboguer",
            Self::Review => "relire et vérifier",
            Self::Topic(category) => topic_label(category),
        }
    }

    /// Ce que le modèle a à faire dans ce rôle (consigne en anglais).
    #[must_use]
    pub fn instruction(self) -> String {
        match self {
            Self::Plan => "understand and prepare. Find out what the request really needs, read the code involved, and lay out the work (TodoWrite when there are several steps). As soon as the plan is clear, start carrying it out in this same step: do not stop on a plan.".to_string(),
            Self::Code => "write the code. Carry out the next open task of the plan with small, exact edits, keep the todo list current, and run what checks your change.".to_string(),
            Self::Debug => "debug. A check or a tool has just failed, or Claw has just pointed out a problem. Read the whole error, find the root cause (state it in one sentence), fix it and run the check again; do not move on before it passes.".to_string(),
            Self::Review => "review. Check the work of another member against the request with fresh eyes: find what is wrong or missing, do not approve by default.".to_string(),
            Self::Topic(category) => format!(
                "specialist in {}. Answer with the precision of an expert of this field: exact terms, sound reasoning, and the caveats or sources that matter.",
                topic_english(category)
            ),
        }
    }
}

/// Une étape annoncée au client : son rôle et pourquoi.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Step {
    pub role: Role,
    /// Pourquoi ce rôle, pour l'utilisateur.
    pub reason: &'static str,
}

/// Rôle de la prochaine étape, d'après l'état du travail.
#[must_use]
pub fn step_for(topic: Option<&'static str>, troubled: bool, changed: bool) -> Step {
    if let Some(category) = topic {
        return Step {
            role: Role::Topic(category),
            reason: "sujet de la demande",
        };
    }
    if troubled {
        return Step {
            role: Role::Debug,
            reason: "un contrôle ou un outil vient d'échouer",
        };
    }
    if changed {
        Step {
            role: Role::Code,
            reason: "le travail sur les fichiers a commencé",
        }
    } else {
        Step {
            role: Role::Plan,
            reason: "début du travail",
        }
    }
}

/// Sujets reconnus dans une demande qui ne porte pas sur du code : catégorie
/// d'`OpenRouter`, puis les mots qui la signalent (minuscules, sans accent
/// requis : le texte est comparé tel quel et sans accents).
const TOPICS: &[(&str, &[&str])] = &[
    (
        "translation",
        &[
            "tradui",
            "traduction",
            "translate",
            "translation",
            "en anglais",
            "en espagnol",
            "en allemand",
            "en italien",
            "en francais",
            "in english",
        ],
    ),
    (
        "legal",
        &[
            "juridique",
            "contrat",
            "clause",
            "rgpd",
            "gdpr",
            "loi ",
            "legal",
            "droit du",
            "droit de",
            "tribunal",
            "avocat",
            "jurisprudence",
            "licence logicielle",
        ],
    ),
    (
        "finance",
        &[
            "finance",
            "financier",
            "comptab",
            "budget",
            "fiscal",
            "impot",
            "bourse",
            "investiss",
            "bilan",
            "tresorerie",
            "facturation tva",
        ],
    ),
    (
        "health",
        &[
            "sante",
            "medical",
            "medecin",
            "symptome",
            "maladie",
            "traitement medical",
            "health",
            "medicament",
        ],
    ),
    (
        "science",
        &[
            "physique",
            "chimie",
            "biologie",
            "scientifique",
            "quantique",
            "astronomie",
            "mathematique",
            "theoreme",
            "demonstration mathematique",
        ],
    ),
    (
        "academia",
        &[
            "these",
            "memoire de",
            "article scientifique",
            "bibliographie",
            "etat de l'art",
            "dissertation",
            "expose sur",
            "cours sur",
        ],
    ),
    (
        "marketing",
        &[
            "marketing",
            "slogan",
            "publicite",
            "campagne",
            "seo",
            "referencement",
            "persona",
            "newsletter",
        ],
    ),
];

/// Mots qui font d'une demande un travail de programmation : elle reste alors
/// au rôle du code, quel que soit le sujet de l'application.
const CODING_WORDS: &[&str] = &[
    "code",
    "fonction",
    "fichier",
    "bug",
    "erreur",
    "test",
    "api",
    "page",
    "frontend",
    "backend",
    "script",
    "classe",
    "module",
    "projet",
    "application",
    "appli",
    "compile",
    "build",
    "commit",
    "git",
    "serveur",
    "base de donnees",
    "sql",
    "python",
    "rust",
    "javascript",
    "typescript",
    "html",
    "css",
    "composant",
    "endpoint",
    "route",
    "installe",
    "deploie",
    "refactor",
    "variable",
    "requete",
    "interface",
    "programme",
    "implemente",
    "debug",
    "dossier",
    "repo",
];

pub(crate) fn without_accents(text: &str) -> String {
    text.chars()
        .map(|c| match c {
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

pub(crate) fn has_word_start(text: &str, word: &str) -> bool {
    text.match_indices(word).any(|(index, _)| {
        text[..index]
            .chars()
            .next_back()
            .is_none_or(|before| !before.is_alphanumeric())
    })
}

/// Sujet spécialisé d'une demande qui ne porte pas sur du code ; `None` pour
/// une demande de programmation (le cas courant) ou sans sujet reconnu.
#[must_use]
pub fn topic_of(request: &str) -> Option<&'static str> {
    let text = without_accents(&request.to_lowercase());
    let coding = CODING_WORDS.iter().any(|word| has_word_start(&text, word))
        || text.contains('/')
        || text.contains('@')
        || text.contains("```")
        || [".py", ".rs", ".js", ".ts", ".html", ".css", ".json", ".md"]
            .iter()
            .any(|extension| text.contains(extension));
    if coding {
        return None;
    }
    TOPICS
        .iter()
        .find(|(_, words)| words.iter().any(|word| has_word_start(&text, word)))
        .map(|(category, _)| *category)
}

fn topic_label(category: &str) -> &'static str {
    match category {
        "translation" => "traduction",
        "legal" => "droit",
        "finance" => "finance",
        "health" => "santé",
        "science" => "sciences",
        "academia" => "études et recherche",
        "marketing" => "marketing",
        _ => "sujet spécialisé",
    }
}

fn topic_english(category: &str) -> &'static str {
    match category {
        "translation" => "translation",
        "legal" => "law",
        "finance" => "finance",
        "health" => "health",
        "science" => "science",
        "academia" => "academic research and teaching",
        "marketing" => "marketing",
        _ => "this field",
    }
}

/// Consigne ajoutée au prompt système de l'étape : le membre de l'équipe sait
/// qui il est, qu'il reprend le travail des autres, et ce qu'il a à faire.
#[must_use]
pub fn role_section(role: Role, model: &str) -> String {
    format!(
        "# Team mode\nYou are {model}, one member of a team of models: Claw gives each step of the work to the member strongest at it. Earlier messages of this conversation may come from other members: treat them as your own work, continue from the latest state, keep the user's request, the plan and the todo list as they are, and do not redo what is already done. Your role for this step: {}",
        role.instruction()
    )
}

#[cfg(test)]
mod tests {
    use super::{role_section, step_for, topic_of, Role};

    #[test]
    fn a_request_about_code_stays_with_the_coders_whatever_its_subject() {
        // L'application traduit des images, mais la demande est du code.
        assert_eq!(
            topic_of("Ajoute une page frontend pour traduire une image"),
            None
        );
        assert_eq!(topic_of("corrige la facturation dans billing.py"), None);
        assert_eq!(
            topic_of("Traduis ce paragraphe en anglais"),
            Some("translation")
        );
        assert_eq!(
            topic_of("Cette clause de contrat est-elle valable ?"),
            Some("legal")
        );
        assert_eq!(
            topic_of("Explique-moi la théorie quantique simplement"),
            Some("science")
        );
        assert_eq!(topic_of("Bonjour, comment vas-tu ?"), None);
        // « loi » en mot entier : pas « emploi ».
        assert_eq!(topic_of("Quel emploi du temps me conseilles-tu ?"), None);
    }

    #[test]
    fn the_role_follows_the_state_of_the_work() {
        assert_eq!(step_for(None, false, false).role, Role::Plan);
        assert_eq!(step_for(None, false, true).role, Role::Code);
        assert_eq!(step_for(None, true, true).role, Role::Debug);
        assert_eq!(
            step_for(Some("legal"), true, false).role,
            Role::Topic("legal")
        );
        let section = role_section(Role::Debug, "vendor/fixer:free");
        assert!(section.starts_with("# Team mode\nYou are vendor/fixer:free"));
        assert!(section.contains("treat them as your own work"));
        assert!(section.contains("find the root cause"));
        assert_eq!(Role::Topic("finance").key(), "finance");
        assert_eq!(Role::Code.label(), "écrire le code");
    }
}
