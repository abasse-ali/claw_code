//! Ultracode : un travail conséquent passe par un déroulé que Claw tient
//! lui-même, au lieu de compter sur le modèle pour s'organiser. Avant le
//! travail, un agent au contexte vierge étudie la demande et le code et
//! rédige un dossier (exigences, code concerné, risques, plan et façon de
//! vérifier chaque étape). Après le travail, un vérificateur indépendant, qui
//! n'a pas écrit le code, contrôle chaque exigence, lance les tests et rend
//! un verdict ; ses objections repartent à l'agent.
//!
//! C'est ce qui manque le plus à un petit modèle : il s'arrête trop tôt, se
//! juge lui-même et perd de vue la demande. Le déroulé coûte des requêtes
//! (une dizaine par agent) : il s'active à la demande, pour la session
//! (`/think ultracode`, `"ultracode": true`, `CLAW_ULTRACODE=1`) ou pour un
//! seul message qui contient le mot « ultracode » ; et de lui-même quand
//! l'utilisateur signale de nouveau un problème que l'agent disait réglé
//! (« c'est toujours là », « tu mens », « still broken ») : vu le 31 octobre
//! 2026 (session #46 de Claw Web), trois « corrigé et vérifié » de suite
//! pour un bogue toujours là, et des tests qui ne tournaient même pas
//! (`vitest: not found`).

use std::fmt::Write as _;
use std::sync::atomic::{AtomicBool, Ordering};

use serde_json::{json, Value};

use crate::harness::{self, Aid};

static ULTRACODE: AtomicBool = AtomicBool::new(false);

/// Active ou coupe ultracode pour la session.
pub fn set_ultracode(on: bool) {
    ULTRACODE.store(on, Ordering::SeqCst);
}

/// Ultracode est actif pour la session (et pas coupé par `CLAW_HARNESS=0`).
#[must_use]
pub fn ultracode() -> bool {
    ULTRACODE.load(Ordering::SeqCst) && harness::enabled(Aid::Ultracode)
}

/// Le message demande ultracode pour lui seul : il contient le mot
/// « ultracode » (casse indifférente, mot entier).
#[must_use]
pub fn keyword_in(text: &str) -> bool {
    text.split(|c: char| !c.is_alphanumeric() && c != '_')
        .any(|word| word.eq_ignore_ascii_case("ultracode"))
}

/// Le déroulé s'applique à ce message : ultracode actif pour la session, ou
/// demandé par le message lui-même.
#[must_use]
pub fn active_for(request: &str) -> bool {
    harness::enabled(Aid::Ultracode) && (ULTRACODE.load(Ordering::SeqCst) || keyword_in(request))
}

/// Mots en dessous desquels un message est une relance ou un mot de
/// politesse (« reprendre », « merci », « vas-y »), pas une demande à étudier.
const SUBSTANTIVE_MIN_WORDS: usize = 4;

#[must_use]
pub fn substantive(request: &str) -> bool {
    request.split_whitespace().count() >= SUBSTANTIVE_MIN_WORDS
}

/// Ce que dit un utilisateur dont le problème n'est pas réglé, minuscules et
/// sans accents.
const RELAPSE_PHRASES: &[&str] = &[
    "toujours present",
    "toujours la",
    "toujours le meme",
    "toujours la meme",
    "toujours pas",
    "toujours horrible",
    "toujours casse",
    "toujours cassee",
    "toujours en double",
    "c'est toujours",
    "ca ne marche toujours",
    "ne marche toujours",
    "ne fonctionne toujours",
    "encore la",
    "encore le meme",
    "encore la meme",
    "encore casse",
    "ca recommence",
    "rien n'a change",
    "rien a change",
    "pas corrige",
    "pas regle",
    "n'est pas corrige",
    "n'est pas regle",
    "tu mens",
    "vous mentez",
    "mensonge",
    "meme probleme",
    "meme bug",
    "meme erreur",
    "pourquoi tu ne corriges",
    "tu n'as pas corrige",
    "still there",
    "still broken",
    "still happening",
    "still not",
    "still the same",
    "not fixed",
    "same problem",
    "same bug",
    "same error",
    "you lied",
    "nothing changed",
    "happens again",
];

fn folded(text: &str) -> String {
    text.to_lowercase()
        .chars()
        .map(|c| match c {
            'à' | 'â' | 'ä' => 'a',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'î' | 'ï' => 'i',
            'ô' | 'ö' => 'o',
            'ù' | 'û' | 'ü' => 'u',
            'ç' => 'c',
            '\u{2019}' => '\'',
            other => other,
        })
        .collect()
}

/// L'utilisateur dit que le problème est toujours là (ou revenu) après une
/// correction annoncée.
#[must_use]
pub fn relapse(text: &str) -> bool {
    let text = folded(text);
    RELAPSE_PHRASES.iter().any(|phrase| text.contains(phrase))
}

/// Ultracode s'active de lui-même sur une rechute (`CLAW_ULTRACODE_RELAPSE=0`
/// le coupe ; `CLAW_ULTRACODE=0` coupe tout ultracode).
#[must_use]
pub fn relapse_escalation(text: &str, earlier_turn: bool) -> bool {
    earlier_turn
        && harness::enabled(Aid::Ultracode)
        && std::env::var("CLAW_ULTRACODE_RELAPSE").map_or(true, |value| value.trim() != "0")
        && relapse(text)
}

/// Vérifications indépendantes au plus par tour (la première, puis une après
/// chaque correction demandée).
pub const MAX_VERIFY_RUNS: usize = 3;
/// En dessous, une réponse sans modification de fichier est trop courte pour
/// mériter une contre-vérification.
pub const FACT_CHECK_MIN_CHARS: usize = 600;
const MAX_DIFF_CHARS: usize = 16_000;
const MAX_TEXT_CHARS: usize = 6_000;
const MAX_FEEDBACK_CHARS: usize = 5_000;

/// Début du dossier joint à la demande : ni un message de l'utilisateur, ni
/// une note de contrôle (`[Claw]`), pour que la page ne l'affiche pas comme tel.
pub const BRIEF_PREFIX: &str = "[Ultracode]";

fn shortened(text: &str, limit: usize) -> String {
    let text = text.trim();
    if text.chars().count() <= limit {
        return text.to_string();
    }
    text.chars().take(limit).collect::<String>() + "\n… (shortened)"
}

fn request_section(request: &str, earlier: Option<&str>) -> String {
    let mut section = format!("The user's request:\n{}\n", request.trim());
    if let Some(earlier) = earlier.map(str::trim).filter(|text| !text.is_empty()) {
        let _ = write!(
            section,
            "\nEarlier request in the same conversation, which the message above may continue or refine:\n{}\n",
            shortened(earlier, MAX_TEXT_CHARS)
        );
    }
    section
}

/// Ce que les agents du déroulé savent de la conversation : les messages
/// précédents de l'utilisateur et la dernière réponse de l'agent. Sans eux,
/// « c'est toujours là » ne dit ni quoi, ni ce que l'agent prétendait.
#[derive(Debug, Clone, Default)]
pub struct Context<'a> {
    pub earlier: Option<&'a str>,
    /// Messages précédents de l'utilisateur, du plus ancien au plus récent.
    pub history: &'a [String],
    /// La dernière réponse de l'agent avant ce message.
    pub last_answer: Option<&'a str>,
    /// L'utilisateur signale un problème (ou une rechute).
    pub problem: bool,
    /// L'utilisateur dit que le problème est toujours là après une correction annoncée.
    pub relapse: bool,
}

fn history_section(context: &Context) -> String {
    let mut section = String::new();
    if !context.history.is_empty() {
        section.push_str("\nThe user's earlier messages in this conversation, oldest first:\n");
        for message in context.history {
            let _ = writeln!(section, "- {}", shortened(message, 700).replace('\n', " "));
        }
    }
    if let Some(answer) = context
        .last_answer
        .map(str::trim)
        .filter(|text| !text.is_empty())
    {
        let _ = write!(
            section,
            "\nThe agent's previous answer to the user (what it claimed it had done):\n{}\n",
            shortened(answer, 1_500)
        );
    }
    if context.relapse {
        section.push_str("\nThe user now says the problem is still there: the earlier fix did not work, whatever the previous answer claimed. Assume the previous diagnosis was wrong or incomplete.\n");
    }
    section
}

/// Mission de l'agent d'exploration (outils en lecture seule, contexte vierge).
#[must_use]
pub fn explore_prompt(request: &str, context: &Context) -> String {
    let problem = if context.problem {
        "\n7. Reproduction: the user reports a problem. Give the exact steps that show the symptom (commands to run, or what to do in the app, screen by screen, including reloading the page or changing month when the data is kept), what should be seen instead, and ranked hypotheses for the cause, each with the code (file:line) that supports it. Look for every place that produces the symptom, not only the first one."
    } else {
        ""
    };
    format!(
        "You prepare the ground for a coding agent that will carry out the request below in this workspace. You have a clean context and read-only tools. Do not solve the task and do not write the code: investigate, so that the agent starts with an accurate map.\n\n{}{}\nWrite a preparation brief with these parts:\n1. Goal: the request restated in one or two sentences.\n2. Requirements: a numbered list (R1, R2, ...) of every part of the request (each feature, page, file, fix, question or explanation asked for), then what it implies without saying it (edge cases, error handling, compatibility, tests, documentation). Mark assumptions as assumptions.\n3. Relevant code: the files, functions and line numbers involved, how they work today, who calls them and which tests cover them. Quote short excerpts when they matter.\n4. Conventions and commands: language, framework, style, and the exact build, type-check and test commands of this project (from its manifests, README or CI).\n5. Risks: what could break, tricky cases, pitfalls you noticed.\n6. Plan: ordered steps, each with how to verify it (an exact command or check). If a needed tool is not installed (no node_modules, no virtual environment), the first step installs it with the project's own command.{problem}\nOnly state what you checked in the files; say what you could not find. Stay under 900 words.",
        request_section(request, context.earlier),
        history_section(context),
    )
}

/// Dossier joint à la demande de l'utilisateur, pour l'agent principal.
#[must_use]
pub fn brief_block(report: &str) -> String {
    format!(
        "{BRIEF_PREFIX} Preparation brief written by an exploration agent with a clean context (read-only; it did not see this conversation). Use it as a map and check in the files whatever you rely on: it can be wrong or incomplete. Treat every requirement it lists as part of the work, verify each step as its plan says, and keep the user's own words as the authority. When you are done, an independent verifier will check every requirement.\n\n{}",
        shortened(report, MAX_TEXT_CHARS)
    )
}

fn brief_section(brief: Option<&str>) -> String {
    brief
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(|brief| {
            format!(
                "\nPreparation brief written before the work (its requirement list is the checklist, but the user's request has the last word):\n{}\n",
                shortened(brief, MAX_TEXT_CHARS)
            )
        })
        .unwrap_or_default()
}

fn answer_section(answer: &str) -> String {
    let answer = answer.trim();
    format!(
        "\nThe agent's final answer to the user:\n{}\n",
        if answer.is_empty() {
            "(none)".to_string()
        } else {
            shortened(answer, MAX_TEXT_CHARS)
        }
    )
}

const VERDICT_RULES: &str = "Report one line per requirement: `PASS`, `FAIL` or `UNVERIFIED`, the requirement, and the evidence (file:line, or the command you ran and what it showed). Then list each problem found with file:line and what to change. End with a last line that is exactly `VERDICT: PASS` when everything required is done and works, otherwise exactly `VERDICT: FAIL`. Something you could not check is UNVERIFIED, not FAIL, unless you have a concrete reason to think it is broken.";

/// Mission du vérificateur quand le tour a modifié des fichiers.
#[must_use]
pub fn verify_prompt(
    request: &str,
    context: &Context,
    brief: Option<&str>,
    diffs: &[String],
    answer: &str,
    interface: bool,
) -> String {
    let interface = if interface {
        " The changes touch a web page or app: build it if it has a build step, then open it with the Browser tool and use it as the user would (the screens, the buttons and forms the request is about, a reload to check what is kept, changing month or screen and coming back). Reading the code is not enough for an interface."
    } else {
        ""
    };
    let problem = if context.problem {
        " The user reported a problem: the first requirement is that the symptom they describe no longer happens. Reproduce their scenario for real, the way they would meet it, and watch for the symptom. If you cannot make the scenario run, that requirement is FAIL, not UNVERIFIED: a fix nobody saw working is not a fix. Look also for other code paths that produce the same symptom."
    } else {
        ""
    };
    let mut changes = diffs.join("\n");
    if changes.chars().count() > MAX_DIFF_CHARS {
        changes = changes.chars().take(MAX_DIFF_CHARS).collect::<String>()
            + "\n… (diff shortened: read the files)";
    }
    format!(
        "You are an independent verifier with a clean context. A coding agent says it has completed the request below. You did not write this code: your job is to find what is wrong or missing, not to approve it. Do not modify the project's source files; you may run commands that read, build or test (test suites, type checkers, builds, installing the project's declared dependencies, small scripts that exercise the code).\n\n{}{}{}\nChanges the agent made during this turn (unified diff):\n{changes}\n{}\nSteps:\n1. List the requirements: those of the brief if there is one, completed by anything else the request asks for.\n2. For each requirement, read the code that implements it (the changed files in full when needed) and decide whether it really does what is asked, including the edge cases it implies.\n3. Run the project's tests, type checker or build, and exercise the changed behaviour with real inputs. A command that fails because a tool is missing (`not found`, no node_modules) proves nothing: install with the project's own command (npm install, pip install -r requirements.txt) and run it again.{interface}{problem}\n4. Check every claim of the agent's final answer against the files and your command outputs: a feature, file, test or result it mentions must exist, and a claim that something was tested or verified needs a command that really ran.\n\n{VERDICT_RULES}",
        request_section(request, context.earlier),
        history_section(context),
        brief_section(brief),
        answer_section(answer),
    )
}

/// Mission du vérificateur quand le tour n'a rien modifié : contre-vérifier
/// la réponse (analyse, explication, conseil) dans le code.
#[must_use]
pub fn fact_check_prompt(
    request: &str,
    context: &Context,
    brief: Option<&str>,
    answer: &str,
) -> String {
    format!(
        "You are an independent verifier with a clean context. An agent answered the request below without changing any file. Your job is to find what is wrong, unsupported or missing in its answer, not to approve it. Do not modify any file.\n\n{}{}{}{}\nSteps:\n1. List what the request asks for (each question, analysis or explanation): these are the requirements.\n2. Check every factual claim of the answer about this project (files, functions, behaviour, numbers, commands) by reading the code or running read-only commands.\n3. Note what the answer leaves out that the request needs, and any recommendation that rests on a wrong fact.\n\n{VERDICT_RULES}",
        request_section(request, context.earlier),
        history_section(context),
        brief_section(brief),
        answer_section(answer),
    )
}

/// Verdict du vérificateur.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Pass,
    Fail,
    /// Pas de ligne `VERDICT:` lisible.
    Unclear,
}

/// Lit le verdict : la dernière ligne `VERDICT: …` ; sans elle, une ligne
/// qui commence par `FAIL` vaut un échec.
#[must_use]
pub fn verdict(report: &str) -> Verdict {
    let cleaned = |line: &str| {
        line.trim()
            .trim_matches(|c: char| matches!(c, '*' | '`' | '_' | '#' | '-' | ' '))
            .to_ascii_uppercase()
    };
    for line in report.lines().rev() {
        let line = cleaned(line);
        if let Some(rest) = line.strip_prefix("VERDICT") {
            let rest = rest.trim_start_matches([':', ' ', '*', '`']);
            if rest.starts_with("PASS") {
                return Verdict::Pass;
            }
            if rest.starts_with("FAIL") {
                return Verdict::Fail;
            }
        }
    }
    if report.lines().any(|line| cleaned(line).starts_with("FAIL")) {
        return Verdict::Fail;
    }
    Verdict::Unclear
}

/// Objections du vérificateur, renvoyées à l'agent principal.
#[must_use]
pub fn verify_feedback(report: &str, changed_files: bool) -> String {
    let what = if changed_files {
        "Check each point against the code. Fix the real problems now with your tools and verify each fix by running it; if a point is wrong, say why in one sentence. Do not rewrite what already works."
    } else {
        "Check each point against the code. Correct your answer where it is wrong, unsupported or incomplete, reading the code again where needed; if a point is wrong, say why in one sentence."
    };
    format!(
        "[Claw] Ultracode: an independent verifier with a clean context checked your work and reports these problems:\n{}\n\n{what} {}",
        shortened(report, MAX_FEEDBACK_CHARS),
        harness::FINAL_ANSWER_RULE
    )
}

/// Dernière vérification encore en échec : l'agent ne doit pas annoncer un
/// travail réglé ou vérifié. Il dit à l'utilisateur ce qui marche, ce qui
/// reste en échec et ce qu'il essaierait ensuite. Avant, Claw ne le notait
/// que dans la progression, et la réponse pouvait dire « corrigé et vérifié ».
#[must_use]
pub fn unresolved_feedback(report: &str) -> String {
    format!(
        "[Claw] Ultracode: after {MAX_VERIFY_RUNS} rounds, the independent verifier still reports these problems:\n{}\n\nFix what you still can now and check it by running it. Then write your final answer without saying the work is fixed, done or verified where the verifier disagrees: tell the user plainly what works (with what you ran to see it), what still fails or could not be checked, and what you would try next. {}",
        shortened(report, MAX_FEEDBACK_CHARS),
        harness::FINAL_ANSWER_RULE
    )
}

/// Arguments de l'outil `Agent` pour une étape du déroulé ; `model` : celui
/// que le mode équipe choisit pour ce rôle.
#[must_use]
pub fn agent_input(
    subagent_type: &str,
    description: &str,
    prompt: &str,
    model: Option<&str>,
) -> String {
    let mut input = json!({
        "description": description,
        "prompt": prompt,
        "subagent_type": subagent_type,
    });
    if let Some(model) = model {
        input["model"] = Value::String(model.to_string());
    }
    input.to_string()
}

/// Rapport final d'un sous-agent, lu dans le résultat de l'outil `Agent`.
#[must_use]
pub fn agent_report(output: &str) -> Option<String> {
    let value: Value = serde_json::from_str(output).ok()?;
    value
        .get("result")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(ToOwned::to_owned)
}

#[cfg(test)]
mod tests {
    use super::{
        agent_report, brief_block, explore_prompt, fact_check_prompt, keyword_in, relapse,
        substantive, unresolved_feedback, verdict, verify_feedback, verify_prompt, Context,
        Verdict, BRIEF_PREFIX,
    };

    #[test]
    fn the_keyword_asks_for_one_turn_only_as_a_whole_word() {
        assert!(keyword_in("ultracode: audite les routes"));
        assert!(keyword_in("Corrige ce bug en mode ULTRACODE."));
        assert!(!keyword_in("ultracoder la page"));
        assert!(!keyword_in("corrige le bug"));
        assert!(substantive("corrige le bug de connexion"));
        assert!(!substantive("reprendre"));
        assert!(!substantive("ok vas-y"));
    }

    #[test]
    fn the_verdict_is_read_from_the_last_verdict_line() {
        assert_eq!(verdict("R1 PASS - ok\nVERDICT: PASS"), Verdict::Pass);
        assert_eq!(
            verdict("R1 FAIL - absent\n\n**VERDICT: FAIL**"),
            Verdict::Fail
        );
        assert_eq!(verdict("verdict : pass"), Verdict::Pass);
        assert_eq!(verdict("`VERDICT: FAIL`\n"), Verdict::Fail);
        // Sans ligne de verdict : un échec signalé compte.
        assert_eq!(verdict("FAIL - R2 - la page n'existe pas"), Verdict::Fail);
        assert_eq!(verdict("Tout semble correct."), Verdict::Unclear);
    }

    #[test]
    fn the_agents_get_everything_they_need_and_the_brief_is_not_a_claw_note() {
        let explore = explore_prompt(
            "Ajoute la page de traduction",
            &Context {
                earlier: Some("Traduis des images"),
                ..Context::default()
            },
        );
        assert!(explore.contains("Do not solve the task"));
        assert!(explore.contains("Requirements: a numbered list (R1, R2, ...)"));
        assert!(explore.contains("Earlier request in the same conversation"));
        let verify = verify_prompt(
            "Ajoute la page",
            &Context::default(),
            Some("R1 la page"),
            &["--- a/x\n+++ b/x\n+page".to_string()],
            "Fait : la page est en place.",
            false,
        );
        assert!(verify.contains("You did not write this code"));
        assert!(verify.contains("R1 la page"));
        assert!(verify.contains("+page"));
        assert!(verify.contains("Fait : la page est en place."));
        assert!(verify.contains("`VERDICT: PASS`"));
        // La page ne doit le prendre ni pour une demande ni pour une note de Claw.
        let block = brief_block("R1 la page");
        assert!(block.starts_with(BRIEF_PREFIX));
        assert!(!block.starts_with(crate::harness::CLAW_NOTE_PREFIX));
        let feedback = verify_feedback("R1 FAIL - absente\nVERDICT: FAIL", true);
        assert!(feedback.starts_with("[Claw] Ultracode: an independent verifier"));
        assert!(feedback.contains("end with your final answer"));
    }

    #[test]
    fn the_report_is_read_from_the_agent_tool_result() {
        assert_eq!(
            agent_report(r#"{"agentId":"a1","status":"completed","result":"  R1 PASS  "}"#)
                .as_deref(),
            Some("R1 PASS")
        );
        assert_eq!(agent_report(r#"{"status":"failed","result":null}"#), None);
        assert_eq!(agent_report("pas du JSON"), None);
    }

    #[test]
    fn a_problem_said_to_be_still_there_is_a_relapse() {
        // Les messages de la session #46 de Claw Web (31 octobre 2026).
        assert!(relapse("Tu mens c'est toujours persent"));
        assert!(relapse("Toujours horrible pourquoi tu ne corriges pas"));
        assert!(relapse("Les rentrées sont TOUJOURS en double"));
        assert!(relapse("ça ne marche toujours pas"));
        assert!(relapse("Rien n'a changé"));
        assert!(relapse("It's still broken after your fix"));
        // Une demande nouvelle, ou « toujours » dans un autre sens, n'en est pas une.
        assert!(!relapse("Ajoute un graphique des dépenses"));
        assert!(!relapse("Garde toujours le mois choisi"));
        assert!(!relapse("Corrige le bug du calendrier"));
    }

    #[test]
    fn a_relapse_brings_the_history_and_a_real_reproduction() {
        let history = vec![
            "Les rentrées de chaque mois se rajoutent quand je change de mois".to_string(),
            "Tu mens c'est toujours persent".to_string(),
        ];
        let context = Context {
            earlier: None,
            history: &history,
            last_answer: Some("Correction du bug de duplication effectuée et vérifiée."),
            problem: true,
            relapse: true,
        };
        let explore = explore_prompt("Toujours horrible pourquoi tu ne corriges pas", &context);
        assert!(explore.contains("se rajoutent quand je change de mois"));
        assert!(explore.contains("effectuée et vérifiée"));
        assert!(explore.contains("the earlier fix did not work"));
        assert!(explore.contains("7. Reproduction"));
        let verify = verify_prompt(
            "Toujours horrible pourquoi tu ne corriges pas",
            &context,
            None,
            &["+fix".to_string()],
            "C'est corrigé.",
            true,
        );
        // Le symptôme doit être vu disparaître, dans l'appli elle-même.
        assert!(verify.contains("that requirement is FAIL, not UNVERIFIED"));
        assert!(verify.contains("open it with the Browser tool"));
        assert!(verify.contains("a reload to check what is kept"));
        // Un outil absent ne prouve rien (`vitest: not found`).
        assert!(verify.contains("A command that fails because a tool is missing"));
        assert!(verify.contains("se rajoutent quand je change de mois"));
        // Une demande ordinaire ne reçoit ni l'un ni l'autre.
        let plain = verify_prompt(
            "Ajoute la page",
            &Context::default(),
            None,
            &[],
            "Fait.",
            false,
        );
        assert!(!plain.contains("Browser tool"));
        assert!(!plain.contains("The user reported a problem"));
        let check = fact_check_prompt("Explique le calcul", &context, None, "Le calcul est juste.");
        assert!(check.contains("se rajoutent quand je change de mois"));
    }

    #[test]
    fn an_unresolved_verdict_forbids_claiming_the_work_is_done() {
        let feedback = unresolved_feedback("R1 FAIL - toujours en double\nVERDICT: FAIL");
        assert!(feedback.starts_with("[Claw] Ultracode: after 3 rounds"));
        assert!(feedback.contains("toujours en double"));
        assert!(feedback.contains("without saying the work is fixed, done or verified"));
        assert!(feedback.contains("what still fails"));
    }
}
