//! Routines (`/schedule`) : l'équivalent gratuit des routines de Claude Code.
//!
//! Une routine est un workflow GitHub Actions du dépôt,
//! `.github/workflows/claw-routine-<nom>.yml`, qui lance Claw (l'action
//! `.github/actions/claw` du dépôt de Claw) selon un horaire cron, et à la
//! demande depuis l'onglet Actions. Elle tourne chez GitHub, ordinateur éteint,
//! avec un modèle gratuit d'`OpenRouter` (secret `OPENROUTER_API_KEY`). Quand
//! Claw modifie des fichiers, elle pousse une branche et ouvre une pull request.
//!
//! La demande est gardée à part, dans `.github/claw-routines/<nom>.md`, et lue
//! par le workflow : elle n'est jamais interprétée comme une expression
//! `${{ … }}` de GitHub Actions et se modifie sans toucher au YAML.

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

/// Action de Claw utilisée par défaut (surcharge : `CLAW_ROUTINE_ACTION` ou
/// la clé `routineAction` des réglages).
pub const DEFAULT_ACTION: &str = "abasse-ali/claw_code/.github/actions/claw@main";

const WORKFLOW_PREFIX: &str = "claw-routine-";
const PROMPTS_DIR: &str = "claw-routines";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Schedule {
    /// Expression cron, en heure UTC (celle de GitHub Actions).
    pub cron: String,
    /// Description lisible (« chaque jour à 07:17 UTC »).
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Help,
    List,
    Create { schedule: Schedule, prompt: String },
    Delete { name: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Routine {
    pub name: String,
    pub cron: Option<String>,
    pub prompt: Option<String>,
    pub workflow: PathBuf,
}

/// Arguments de `/schedule`.
pub fn parse_command(args: Option<&str>) -> Result<Action, String> {
    let Some(args) = args.map(str::trim).filter(|args| !args.is_empty()) else {
        return Ok(Action::List);
    };
    let lowered = args.to_lowercase();
    if matches!(lowered.as_str(), "help" | "aide" | "?") {
        return Ok(Action::Help);
    }
    if matches!(lowered.as_str(), "list" | "liste" | "ls") {
        return Ok(Action::List);
    }
    for verb in ["delete ", "supprimer ", "remove ", "rm "] {
        if lowered.starts_with(verb) {
            let name = args[verb.len()..].trim();
            if name.is_empty() {
                return Err("nom de routine manquant : /schedule delete <nom>".to_string());
            }
            return Ok(Action::Delete {
                name: name.to_string(),
            });
        }
    }
    let (spec, prompt) = split_schedule(args)?;
    if prompt.trim().is_empty() {
        return Err(format!(
            "demande manquante après l'horaire « {spec} »\n{}",
            usage()
        ));
    }
    Ok(Action::Create {
        schedule: parse_schedule(&spec)?,
        prompt: prompt.trim().to_string(),
    })
}

/// Sépare l'horaire de la demande : un premier mot, ou une expression cron
/// entre guillemets.
fn split_schedule(args: &str) -> Result<(String, String), String> {
    for quote in ['"', '\'', '`'] {
        if let Some(rest) = args.strip_prefix(quote) {
            let Some(end) = rest.find(quote) else {
                return Err("guillemet fermant manquant autour de l'expression cron".to_string());
            };
            return Ok((rest[..end].to_string(), rest[end + 1..].to_string()));
        }
    }
    let (spec, prompt) = args.split_once(char::is_whitespace).unwrap_or((args, ""));
    Ok((spec.to_string(), prompt.to_string()))
}

/// Horaire : `horaire`, `quotidien[@HH:MM]`, `jours-ouvres[@HH:MM]`,
/// `hebdo[@jour[-HH:MM]]`, chacun avec un décalage facultatif (`@09:00+02:00`,
/// sinon l'heure est en UTC), ou une expression cron de cinq champs.
pub fn parse_schedule(spec: &str) -> Result<Schedule, String> {
    let spec = spec.trim();
    if spec.split_whitespace().count() == 5 {
        return parse_cron(spec);
    }
    let lowered = fold(spec);
    let (kind, when) = lowered
        .split_once('@')
        .map_or((lowered.as_str(), None), |(kind, when)| (kind, Some(when)));
    match kind {
        "horaire" | "hourly" | "chaque-heure" | "toutes-les-heures" => Ok(Schedule {
            cron: "17 * * * *".to_string(),
            label: "toutes les heures (à la 17e minute)".to_string(),
        }),
        "quotidien" | "daily" | "chaque-jour" | "jour" | "tous-les-jours" => {
            let (minute, hour, shift) = clock(when.unwrap_or("07:17"))?;
            let _ = shift;
            Ok(Schedule {
                cron: format!("{minute} {hour} * * *"),
                label: format!("chaque jour à {hour:02}:{minute:02} UTC"),
            })
        }
        "jours-ouvres" | "weekdays" | "semaine" | "en-semaine" => {
            let (minute, hour, shift) = clock(when.unwrap_or("07:17"))?;
            let days = shift_days(&[1, 2, 3, 4, 5], shift);
            Ok(Schedule {
                cron: format!("{minute} {hour} * * {days}"),
                label: format!("du lundi au vendredi à {hour:02}:{minute:02} UTC"),
            })
        }
        "hebdo" | "weekly" | "hebdomadaire" | "chaque-semaine" => {
            let when = when.unwrap_or("lundi-07:17");
            let (day_name, time) = when.split_once('-').unwrap_or((when, "07:17"));
            let day = weekday(day_name)?;
            let (minute, hour, shift) = clock(time)?;
            let days = shift_days(&[day], shift);
            Ok(Schedule {
                cron: format!("{minute} {hour} * * {days}"),
                label: format!(
                    "chaque semaine, {} à {hour:02}:{minute:02} UTC",
                    weekday_name(days.parse().unwrap_or(day))
                ),
            })
        }
        _ => Err(format!("horaire inconnu « {spec} »\n{}", usage())),
    }
}

fn parse_cron(spec: &str) -> Result<Schedule, String> {
    let fields: Vec<&str> = spec.split_whitespace().collect();
    let valid = fields.iter().all(|field| {
        field
            .chars()
            .all(|character| character.is_ascii_digit() || "*/,-".contains(character))
    });
    if !valid {
        return Err(format!(
            "expression cron invalide « {spec} » : chiffres, *, /, - et , seulement"
        ));
    }
    if fields[0] == "*"
        || fields[0].starts_with("*/") && fields[0][2..].parse::<u32>().unwrap_or(0) < 5
    {
        return Err(
            "GitHub Actions ne lance pas un workflow planifié plus d'une fois toutes les 5 minutes"
                .to_string(),
        );
    }
    Ok(Schedule {
        cron: spec.to_string(),
        label: format!("cron « {spec} » (UTC)"),
    })
}

/// `HH:MM[±HH:MM]` → minute et heure en UTC, et le décalage de jour (-1, 0, 1).
fn clock(text: &str) -> Result<(u32, u32, i32), String> {
    let text = text.trim();
    let (time, offset) = match text.find(['+', '-']) {
        Some(index) if index > 0 => (&text[..index], Some(&text[index..])),
        _ => (text, None),
    };
    let (hour, minute) = parse_hh_mm(time)?;
    let mut total = i32::try_from(hour * 60 + minute).unwrap_or(0);
    if let Some(offset) = offset {
        let sign = if offset.starts_with('-') { -1 } else { 1 };
        let (offset_hour, offset_minute) = parse_hh_mm(&offset[1..])?;
        total -= sign * i32::try_from(offset_hour * 60 + offset_minute).unwrap_or(0);
    }
    let shift = total.div_euclid(24 * 60);
    let total = u32::try_from(total.rem_euclid(24 * 60)).unwrap_or(0);
    Ok((total % 60, total / 60, shift))
}

fn parse_hh_mm(text: &str) -> Result<(u32, u32), String> {
    let (hour, minute) = text.split_once([':', 'h']).unwrap_or((text, "0"));
    let hour: u32 = hour
        .trim()
        .parse()
        .map_err(|_| format!("heure invalide « {text} » (attendu HH:MM)"))?;
    let minute: u32 = if minute.trim().is_empty() {
        0
    } else {
        minute
            .trim()
            .parse()
            .map_err(|_| format!("minutes invalides « {text} » (attendu HH:MM)"))?
    };
    if hour > 23 || minute > 59 {
        return Err(format!("heure hors limites « {text} »"));
    }
    Ok((hour, minute))
}

fn shift_days(days: &[u32], shift: i32) -> String {
    days.iter()
        .map(|day| {
            let shifted = (i32::try_from(*day).unwrap_or(0) + shift).rem_euclid(7);
            shifted.to_string()
        })
        .collect::<Vec<_>>()
        .join(",")
}

fn weekday(name: &str) -> Result<u32, String> {
    Ok(match name.trim() {
        "dimanche" | "dim" | "sunday" | "sun" | "0" | "7" => 0,
        "lundi" | "lun" | "monday" | "mon" | "1" => 1,
        "mardi" | "mar" | "tuesday" | "tue" | "2" => 2,
        "mercredi" | "mer" | "wednesday" | "wed" | "3" => 3,
        "jeudi" | "jeu" | "thursday" | "thu" | "4" => 4,
        "vendredi" | "ven" | "friday" | "fri" | "5" => 5,
        "samedi" | "sam" | "saturday" | "sat" | "6" => 6,
        other => return Err(format!("jour inconnu « {other} »")),
    })
}

fn weekday_name(day: u32) -> &'static str {
    match day % 7 {
        0 => "le dimanche",
        1 => "le lundi",
        2 => "le mardi",
        3 => "le mercredi",
        4 => "le jeudi",
        5 => "le vendredi",
        _ => "le samedi",
    }
}

/// Nom de routine tiré de la demande (quelques mots), unique dans le dépôt.
pub fn routine_name(repo: &Path, prompt: &str) -> String {
    let words: Vec<String> = prompt
        .split_whitespace()
        .map(slug)
        .filter(|word| word.len() > 2)
        .take(4)
        .collect();
    let base = if words.is_empty() {
        "routine".to_string()
    } else {
        words.join("-")
    };
    let base: String = base.chars().take(40).collect();
    let base = base.trim_end_matches('-').to_string();
    let workflows = repo.join(".github").join("workflows");
    let mut name = base.clone();
    let mut counter = 2;
    while workflows
        .join(format!("{WORKFLOW_PREFIX}{name}.yml"))
        .exists()
    {
        name = format!("{base}-{counter}");
        counter += 1;
    }
    name
}

/// Modèle utilisable par la routine : un identifiant d'`OpenRouter`, sinon
/// aucun (l'action choisit alors le meilleur modèle gratuit pour le code).
#[must_use]
pub fn routine_model(model: &str) -> String {
    let clean = model.trim();
    let allowed = clean
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || "/:._-".contains(character));
    if allowed && clean.contains('/') && !clean.starts_with("ollama") {
        clean.to_string()
    } else {
        String::new()
    }
}

/// Contenu du workflow de la routine `name`.
/// Modèle du workflow : chaîne brute, pour que l'indentation (qui fait le sens
/// en YAML) reste telle quelle. Les `__CHAMPS__` sont remplacés.
const WORKFLOW_TEMPLATE: &str = r#"# Routine Claw créée par /schedule. La demande est dans __PROMPT_FILE__ ;
# l'horaire ci-dessous est en heure UTC (celle de GitHub Actions).
# Lancement à la main : onglet Actions → « Claw : __NAME__ » → Run workflow.
name: "Claw : __NAME__"

on:
  schedule:
    - cron: "__CRON__" # __LABEL__
  workflow_dispatch:

permissions:
  contents: write
  pull-requests: write

concurrency:
  group: claw-routine-__NAME__
  cancel-in-progress: false

jobs:
  routine:
    runs-on: ubuntu-latest
    timeout-minutes: 30
    steps:
      - uses: actions/checkout@v4

      - name: Demande
        id: demande
        shell: bash
        run: |
          {
            echo 'prompt<<CLAW_ROUTINE_EOF'
            cat '__PROMPT_FILE__'
            echo
            echo 'CLAW_ROUTINE_EOF'
          } >> "$GITHUB_OUTPUT"

      - name: Claw
        id: claw
        uses: __ACTION__
        with:
          prompt: ${{ steps.demande.outputs.prompt }}
__MODEL_LINE__          permission-mode: workspace-write
          openrouter-api-key: ${{ secrets.OPENROUTER_API_KEY }}

      - name: Pull request des changements
        if: steps.claw.outputs.changed == 'true'
        shell: bash
        env:
          GH_TOKEN: ${{ github.token }}
          RESULT: ${{ steps.claw.outputs.result }}
        run: |
          branch="claw/routine-__NAME__-$(date -u +%Y%m%d-%H%M)"
          git config user.name "claw-routine"
          git config user.email "claw-routine@users.noreply.github.com"
          git checkout -b "$branch"
          git add -A -- . ':!.claw'
          git commit -m "Routine Claw : __NAME__"
          git push origin "$branch"
          printf '%s\n' "$RESULT" > "$RUNNER_TEMP/body.md"
          gh pr create --title "Routine Claw : __NAME__" --body-file "$RUNNER_TEMP/body.md" --head "$branch"
"#;

#[must_use]
pub fn render_workflow(name: &str, schedule: &Schedule, model: &str, action: &str) -> String {
    let prompt_file = format!(".github/{PROMPTS_DIR}/{name}.md");
    // Sans modèle, l'action choisit le meilleur modèle gratuit pour le code.
    let model_line = if model.is_empty() {
        String::new()
    } else {
        format!("          model: \"{model}\"\n")
    };
    WORKFLOW_TEMPLATE
        .replace("__PROMPT_FILE__", &prompt_file)
        .replace("__NAME__", name)
        .replace("__CRON__", &schedule.cron)
        .replace("__LABEL__", &schedule.label)
        .replace("__ACTION__", action)
        .replace("__MODEL_LINE__", &model_line)
}

/// Écrit le workflow et la demande de la routine, renvoie le nom et les chemins.
pub fn create_routine(
    repo: &Path,
    schedule: &Schedule,
    prompt: &str,
    model: &str,
    action: &str,
) -> Result<(String, PathBuf, PathBuf), String> {
    let name = routine_name(repo, prompt);
    let workflows = repo.join(".github").join("workflows");
    let prompts = repo.join(".github").join(PROMPTS_DIR);
    fs::create_dir_all(&workflows).map_err(|error| error.to_string())?;
    fs::create_dir_all(&prompts).map_err(|error| error.to_string())?;
    let workflow = workflows.join(format!("{WORKFLOW_PREFIX}{name}.yml"));
    let prompt_path = prompts.join(format!("{name}.md"));
    fs::write(&prompt_path, format!("{}\n", prompt.trim())).map_err(|error| error.to_string())?;
    fs::write(
        &workflow,
        render_workflow(&name, schedule, &routine_model(model), action),
    )
    .map_err(|error| error.to_string())?;
    Ok((name, workflow, prompt_path))
}

/// Routines du dépôt.
#[must_use]
pub fn list_routines(repo: &Path) -> Vec<Routine> {
    let workflows = repo.join(".github").join("workflows");
    let Ok(entries) = fs::read_dir(&workflows) else {
        return Vec::new();
    };
    let mut routines: Vec<Routine> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter_map(|path| {
            let file = path.file_name()?.to_str()?;
            let name = file
                .strip_prefix(WORKFLOW_PREFIX)?
                .strip_suffix(".yml")?
                .to_string();
            let cron = fs::read_to_string(&path).ok().and_then(|contents| {
                contents.lines().find_map(|line| {
                    let rest = line.trim().strip_prefix("- cron:")?;
                    Some(
                        rest.trim()
                            .trim_start_matches('"')
                            .split('"')
                            .next()?
                            .to_string(),
                    )
                })
            });
            let prompt = fs::read_to_string(
                repo.join(".github")
                    .join(PROMPTS_DIR)
                    .join(format!("{name}.md")),
            )
            .ok()
            .map(|text| text.trim().to_string());
            Some(Routine {
                name,
                cron,
                prompt,
                workflow: path,
            })
        })
        .collect();
    routines.sort_by(|left, right| left.name.cmp(&right.name));
    routines
}

/// Supprime le workflow et la demande de la routine `name`.
pub fn delete_routine(repo: &Path, name: &str) -> Result<Vec<PathBuf>, String> {
    let name = name
        .trim()
        .trim_start_matches(WORKFLOW_PREFIX)
        .trim_end_matches(".yml");
    let workflow = repo
        .join(".github")
        .join("workflows")
        .join(format!("{WORKFLOW_PREFIX}{name}.yml"));
    if !workflow.is_file() {
        return Err(format!(
            "routine « {name} » introuvable ({} absent)",
            workflow.display()
        ));
    }
    let prompt = repo
        .join(".github")
        .join(PROMPTS_DIR)
        .join(format!("{name}.md"));
    let mut removed = Vec::new();
    for path in [workflow, prompt] {
        if path.is_file() {
            fs::remove_file(&path).map_err(|error| error.to_string())?;
            removed.push(path);
        }
    }
    Ok(removed)
}

#[must_use]
pub fn render_list(routines: &[Routine]) -> String {
    if routines.is_empty() {
        return format!("Routines\n  Aucune routine dans ce dépôt.\n\n{}", usage());
    }
    let mut report = String::from("Routines (GitHub Actions)\n");
    for routine in routines {
        let prompt = routine.prompt.as_deref().unwrap_or("?");
        let first_line = prompt.lines().next().unwrap_or_default();
        let shown: String = first_line.chars().take(70).collect();
        let _ = writeln!(
            report,
            "  {:<28} {:<18} {shown}",
            routine.name,
            routine.cron.as_deref().unwrap_or("?")
        );
    }
    let _ = write!(report, "\n{}", usage());
    report
}

#[must_use]
pub fn usage() -> String {
    "Usage
  /schedule                         liste les routines du dépôt
  /schedule <horaire> <demande>     crée une routine
  /schedule delete <nom>            supprime une routine
Horaires (heure UTC, ou avec décalage : quotidien@09:00+02:00)
  horaire                           toutes les heures
  quotidien[@HH:MM]                 chaque jour (07:17 par défaut)
  jours-ouvres[@HH:MM]              du lundi au vendredi
  hebdo[@jour-HH:MM]                chaque semaine (lundi-07:17 par défaut)
  \"M H J M S\"                       expression cron entre guillemets"
        .to_string()
}

fn fold(value: &str) -> String {
    value
        .trim()
        .chars()
        .map(|character| match character {
            'à' | 'â' | 'ä' => 'a',
            'é' | 'è' | 'ê' | 'ë' | 'É' => 'e',
            'î' | 'ï' => 'i',
            'ô' | 'ö' => 'o',
            'ù' | 'û' | 'ü' => 'u',
            'ç' => 'c',
            '_' | ' ' => '-',
            other => other.to_ascii_lowercase(),
        })
        .collect()
}

fn slug(word: &str) -> String {
    fold(word)
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{
        create_routine, delete_routine, list_routines, parse_command, parse_schedule,
        render_workflow, routine_model, Action, DEFAULT_ACTION,
    };
    use std::fs;

    #[test]
    fn schedules_become_utc_cron_expressions() {
        assert_eq!(
            parse_schedule("horaire").expect("horaire").cron,
            "17 * * * *"
        );
        assert_eq!(
            parse_schedule("quotidien").expect("jour").cron,
            "17 7 * * *"
        );
        assert_eq!(
            parse_schedule("quotidien@09:30").expect("jour").cron,
            "30 9 * * *"
        );
        // 09:00 à Paris l'été (UTC+2) : 07:00 UTC.
        assert_eq!(
            parse_schedule("quotidien@09:00+02:00")
                .expect("décalage")
                .cron,
            "0 7 * * *"
        );
        assert_eq!(
            parse_schedule("jours-ouvrés@08:00").expect("semaine").cron,
            "0 8 * * 1,2,3,4,5"
        );
        // 01:00 le lundi à UTC+2 : dimanche 23:00 UTC.
        assert_eq!(
            parse_schedule("hebdo@lundi-01:00+02:00")
                .expect("hebdo")
                .cron,
            "0 23 * * 0"
        );
        assert_eq!(parse_schedule("0 9 * * 1").expect("cron").cron, "0 9 * * 1");
        assert!(parse_schedule("* * * * *").is_err());
        assert!(parse_schedule("*/2 * * * *").is_err());
        assert!(parse_schedule("chaque-lune").is_err());
        assert!(parse_schedule("quotidien@25:00").is_err());
    }

    #[test]
    fn the_command_reads_the_schedule_then_the_prompt() {
        assert_eq!(parse_command(None), Ok(Action::List));
        assert_eq!(
            parse_command(Some("delete revue-prs")),
            Ok(Action::Delete {
                name: "revue-prs".to_string()
            })
        );
        match parse_command(Some("\"0 6 * * 1\" audite les dépendances")) {
            Ok(Action::Create { schedule, prompt }) => {
                assert_eq!(schedule.cron, "0 6 * * 1");
                assert_eq!(prompt, "audite les dépendances");
            }
            other => panic!("création attendue : {other:?}"),
        }
        assert!(parse_command(Some("quotidien")).is_err());
    }

    #[test]
    fn a_routine_is_written_listed_and_deleted() {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_nanos());
        let repo = std::env::temp_dir().join(format!("claw-routines-{stamp}"));
        fs::create_dir_all(&repo).expect("dépôt");
        let schedule = parse_schedule("quotidien@06:00").expect("horaire");
        let (name, workflow, prompt) = create_routine(
            &repo,
            &schedule,
            "Relis les pull requests ouvertes et commente les risques ${{ secrets.X }}",
            "qwen/qwen3-coder:free",
            DEFAULT_ACTION,
        )
        .expect("création");
        assert_eq!(name, "relis-les-pull-requests");
        let yaml = fs::read_to_string(&workflow).expect("workflow");
        assert!(yaml.contains("- cron: \"0 6 * * *\""));
        assert!(yaml.contains("model: \"qwen/qwen3-coder:free\""));
        assert!(yaml.contains("prompt: ${{ steps.demande.outputs.prompt }}"));
        // La demande reste dans son fichier : jamais dans le YAML.
        assert!(!yaml.contains("secrets.X"));
        assert!(fs::read_to_string(&prompt)
            .expect("demande")
            .contains("${{ secrets.X }}"));
        // Un second nom identique reçoit un suffixe.
        let (second, ..) = create_routine(
            &repo,
            &schedule,
            "Relis les pull requests ouvertes",
            "ollama/qwen3",
            DEFAULT_ACTION,
        )
        .expect("seconde routine");
        assert_eq!(second, "relis-les-pull-requests-2");
        let routines = list_routines(&repo);
        assert_eq!(routines.len(), 2);
        assert_eq!(routines[0].cron.as_deref(), Some("0 6 * * *"));
        let removed = delete_routine(&repo, &second).expect("suppression");
        assert_eq!(removed.len(), 2);
        assert_eq!(list_routines(&repo).len(), 1);
        assert!(delete_routine(&repo, "absente").is_err());
        fs::remove_dir_all(repo).expect("nettoyage");
    }

    #[test]
    fn only_openrouter_models_go_into_the_workflow() {
        assert_eq!(
            routine_model("qwen/qwen3-coder:free"),
            "qwen/qwen3-coder:free"
        );
        assert_eq!(routine_model("ollama/qwen3"), "");
        assert_eq!(routine_model("claude-opus-4-6"), "");
        assert_eq!(routine_model("bad\"model/x"), "");
        let yaml = render_workflow(
            "test",
            &parse_schedule("horaire").expect("horaire"),
            "",
            DEFAULT_ACTION,
        );
        // Sans modèle, pas de ligne `model` : l'action choisit.
        assert!(!yaml.contains("model:"));
        assert!(yaml.contains(&format!("        uses: {DEFAULT_ACTION}\n")));
        assert!(!yaml.contains("__"), "champ non remplacé :\n{yaml}");
        let yaml = render_workflow(
            "test",
            &parse_schedule("horaire").expect("horaire"),
            "qwen/qwen3-coder:free",
            DEFAULT_ACTION,
        );
        assert!(yaml.contains(
            "          prompt: ${{ steps.demande.outputs.prompt }}\n          model: \"qwen/qwen3-coder:free\"\n          permission-mode: workspace-write\n"
        ));
    }

    #[test]
    fn the_workflow_keeps_its_yaml_indentation() {
        let yaml = render_workflow(
            "veille",
            &parse_schedule("quotidien@08:00").expect("jour"),
            "",
            DEFAULT_ACTION,
        );
        // L'indentation fait le sens en YAML : une version sans elle avait été
        // produite par des fins de ligne `\` dans une chaîne Rust.
        assert!(yaml.contains("\non:\n  schedule:\n    - cron: \"0 8 * * *\""));
        assert!(yaml.contains("\njobs:\n  routine:\n    runs-on: ubuntu-latest\n"));
        assert!(yaml.contains("\n    steps:\n      - uses: actions/checkout@v4\n"));
        assert!(yaml.contains(
            "\n        run: |\n          {\n            echo 'prompt<<CLAW_ROUTINE_EOF'\n"
        ));
        assert!(yaml.contains("cat '.github/claw-routines/veille.md'"));
        assert!(yaml.contains("printf '%s\\n' \"$RESULT\""));
        for line in yaml.lines().filter(|line| !line.is_empty()) {
            let indent = line.len() - line.trim_start().len();
            assert!(indent % 2 == 0, "indentation impaire : {line:?}");
        }
    }
}
