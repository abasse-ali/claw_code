//! Tâches planifiées de la session (`/loop`, outils `CronCreate`,
//! `CronList`, `CronDelete`, `ScheduleWakeup`), comme dans Claude Code.
//!
//! Une tâche relance une demande selon une expression cron à cinq champs, en
//! heure locale. Elle se déclenche entre deux tours, quand la session attend
//! (jamais au milieu d'une réponse) ; une échéance manquée pendant un long
//! tour se rattrape une seule fois. Une tâche récurrente expire au bout de
//! sept jours (dernier passage, puis suppression) ; une session en garde 50
//! au plus. Les tâches sont enregistrées à côté de la conversation
//! (`<session>.tasks.json`) et reviennent avec `--resume`.
//! `CLAW_DISABLE_CRON=1` (ou `CLAUDE_CODE_DISABLE_CRON=1`) coupe tout.

use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// Tâches gardées au plus par session.
pub const MAX_TASKS: usize = 50;
/// Durée de vie d'une tâche récurrente.
const EXPIRY_SECS: u64 = 7 * 86_400;

/// Expression cron à cinq champs : minute, heure, jour du mois, mois, jour
/// de la semaine (0 ou 7 pour dimanche).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CronExpr {
    minutes: u64,
    hours: u32,
    days: u32,
    months: u16,
    weekdays: u8,
    any_day: bool,
    any_weekday: bool,
}

fn parse_field(field: &str, min: u32, max: u32, name: &str) -> Result<(u64, bool), String> {
    let mut mask = 0u64;
    let star = field == "*";
    for part in field.split(',') {
        let (range, step) = match part.split_once('/') {
            Some((range, step)) => {
                let step: u32 = step
                    .parse()
                    .ok()
                    .filter(|step| *step > 0)
                    .ok_or_else(|| format!("pas invalide « {step} » ({name})"))?;
                (range, step)
            }
            None => (part, 1),
        };
        let (start, end) = if range == "*" {
            (min, max)
        } else if let Some((low, high)) = range.split_once('-') {
            let low: u32 = low
                .parse()
                .map_err(|_| format!("valeur invalide « {low} » ({name})"))?;
            let high: u32 = high
                .parse()
                .map_err(|_| format!("valeur invalide « {high} » ({name})"))?;
            (low, high)
        } else {
            let value: u32 = range
                .parse()
                .map_err(|_| format!("valeur invalide « {range} » ({name})"))?;
            // `5/15` : de 5 jusqu'au bout, de 15 en 15.
            (value, if step > 1 { max } else { value })
        };
        if start < min || end > max || start > end {
            return Err(format!("« {part} » sort des limites {min}-{max} ({name})"));
        }
        let mut value = start;
        while value <= end {
            mask |= 1 << value;
            value += step;
        }
    }
    Ok((mask, star))
}

impl CronExpr {
    pub fn parse(expression: &str) -> Result<Self, String> {
        let fields: Vec<&str> = expression.split_whitespace().collect();
        if fields.len() != 5 {
            return Err(format!(
                "expression cron « {expression} » : cinq champs attendus (minute heure jour mois jour-de-semaine)"
            ));
        }
        let (minutes, _) = parse_field(fields[0], 0, 59, "minute")?;
        let (hours, _) = parse_field(fields[1], 0, 23, "heure")?;
        let (days, any_day) = parse_field(fields[2], 1, 31, "jour du mois")?;
        let (months, _) = parse_field(fields[3], 1, 12, "mois")?;
        let (mut weekdays, any_weekday) = parse_field(fields[4], 0, 7, "jour de la semaine")?;
        // 7 vaut dimanche, comme 0.
        if weekdays & (1 << 7) != 0 {
            weekdays = (weekdays & !(1 << 7)) | 1;
        }
        Ok(Self {
            minutes,
            hours: u32::try_from(hours).unwrap_or(0),
            days: u32::try_from(days).unwrap_or(0),
            months: u16::try_from(months).unwrap_or(0),
            weekdays: u8::try_from(weekdays).unwrap_or(0),
            any_day,
            any_weekday,
        })
    }

    /// La minute locale `local_minute` (minutes depuis 1970, heure locale)
    /// correspond à l'expression.
    #[must_use]
    pub fn matches(&self, local_minute: i64) -> bool {
        let time = LocalTime::from_minutes(local_minute);
        if self.minutes & (1 << time.minute) == 0
            || self.hours & (1 << time.hour) == 0
            || self.months & (1 << time.month) == 0
        {
            return false;
        }
        let day = self.days & (1 << time.day) != 0;
        let weekday = self.weekdays & (1 << time.weekday) != 0;
        // Jour du mois et jour de la semaine tous deux restreints : l'un ou
        // l'autre suffit (comme vixie-cron).
        match (self.any_day, self.any_weekday) {
            (true, true) => true,
            (true, false) => weekday,
            (false, true) => day,
            (false, false) => day || weekday,
        }
    }

    /// Première minute locale strictement après `after` qui correspond
    /// (recherche sur quatre ans au plus).
    #[must_use]
    pub fn next_after(&self, after: i64) -> Option<i64> {
        (after + 1..after + 4 * 366 * 1_440).find(|minute| self.matches(*minute))
    }
}

/// Date et heure locales d'une minute.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocalTime {
    pub year: i64,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    /// 0 dimanche … 6 samedi.
    pub weekday: u32,
}

impl LocalTime {
    #[must_use]
    pub fn from_minutes(local_minute: i64) -> Self {
        let days = local_minute.div_euclid(1_440);
        let in_day = local_minute.rem_euclid(1_440);
        let (year, month, day) = civil_from_days(days);
        Self {
            year,
            month: u32::try_from(month).unwrap_or(1),
            day: u32::try_from(day).unwrap_or(1),
            hour: u32::try_from(in_day / 60).unwrap_or(0),
            minute: u32::try_from(in_day % 60).unwrap_or(0),
            // Le 1er janvier 1970 était un jeudi.
            weekday: u32::try_from((days + 4).rem_euclid(7)).unwrap_or(0),
        }
    }
}

fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    };
    (year_of_era + era * 400 + i64::from(month <= 2), month, day)
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

/// `+02:00`, `-0530`, `+2` : décalage en secondes.
fn parse_offset(text: &str) -> Option<i64> {
    let text = text.trim();
    let (sign, rest) = match text.chars().next()? {
        '+' => (1, &text[1..]),
        '-' => (-1, &text[1..]),
        _ => (1, text),
    };
    let digits: String = rest.chars().filter(char::is_ascii_digit).collect();
    let (hours, minutes) = match digits.len() {
        1 | 2 => (digits.parse::<i64>().ok()?, 0),
        3 | 4 => {
            let split = digits.len() - 2;
            (
                digits[..split].parse::<i64>().ok()?,
                digits[split..].parse::<i64>().ok()?,
            )
        }
        _ => return None,
    };
    (hours <= 14 && minutes < 60).then_some(sign * (hours * 3_600 + minutes * 60))
}

/// Décalage de l'heure locale sur UTC, en secondes : `CLAW_TZ_OFFSET`
/// (`+02:00`), sinon celui du système (relu toutes les 30 minutes, pour les
/// changements d'heure), sinon UTC.
#[must_use]
pub fn local_offset_secs() -> i64 {
    static CACHE: OnceLock<Mutex<Option<(Instant, i64)>>> = OnceLock::new();
    if let Some(offset) = std::env::var("CLAW_TZ_OFFSET")
        .ok()
        .as_deref()
        .and_then(parse_offset)
    {
        return offset;
    }
    let cache = CACHE.get_or_init(|| Mutex::new(None));
    let mut guard = cache
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some((at, offset)) = *guard {
        if at.elapsed() < Duration::from_secs(1_800) {
            return offset;
        }
    }
    let offset = system_offset().unwrap_or(0);
    *guard = Some((Instant::now(), offset));
    offset
}

fn system_offset() -> Option<i64> {
    let from_date = plugins::posix_shell_command("date +%z")
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| parse_offset(&String::from_utf8_lossy(&output.stdout)));
    if from_date.is_some() || !cfg!(windows) {
        return from_date;
    }
    let output = std::process::Command::new("powershell")
        .args([
            "-NoProfile",
            "-Command",
            "[int][TimeZoneInfo]::Local.GetUtcOffset([DateTime]::Now).TotalMinutes",
        ])
        .output()
        .ok()?;
    let minutes: i64 = String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse()
        .ok()?;
    Some(minutes * 60)
}

/// Minute locale d'un instant UTC (secondes).
fn local_minute(utc_secs: u64) -> i64 {
    (i64::try_from(utc_secs).unwrap_or(0) + local_offset_secs()).div_euclid(60)
}

/// Instant UTC (secondes) du début d'une minute locale.
fn utc_secs_of(local_minute: i64) -> u64 {
    u64::try_from(local_minute * 60 - local_offset_secs()).unwrap_or(0)
}

/// « 14:05 », ou « 16/10 14:05 » si ce n'est pas aujourd'hui.
#[must_use]
pub fn format_local(utc_secs: u64) -> String {
    let time = LocalTime::from_minutes(local_minute(utc_secs));
    let today = LocalTime::from_minutes(local_minute(now_secs()));
    if (time.year, time.month, time.day) == (today.year, today.month, today.day) {
        format!("{:02}:{:02}", time.hour, time.minute)
    } else {
        format!(
            "{:02}/{:02} {:02}:{:02}",
            time.day, time.month, time.hour, time.minute
        )
    }
}

/// Une tâche planifiée.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduledTask {
    pub id: String,
    pub cron: String,
    pub prompt: String,
    pub recurring: bool,
    /// Création (secondes UTC).
    pub created_at: u64,
    /// Prochain passage (secondes UTC).
    pub next_run: u64,
    #[serde(default)]
    pub runs: u32,
    /// Nombre de passages au plus (`/loop` : `CLAW_LOOP_MAX`).
    #[serde(default)]
    pub max_runs: Option<u32>,
    /// `/loop` qui choisit lui-même son intervalle (`ScheduleWakeup`).
    #[serde(default)]
    pub self_paced: bool,
    /// Créée par `/loop` (arrêtée par `/loop stop`).
    #[serde(default)]
    pub from_loop: bool,
    /// Réveil de secours d'une boucle dont le passage n'a rien replanifié :
    /// si ce passage-là ne replanifie rien non plus, la boucle s'arrête.
    #[serde(default)]
    pub fallback: bool,
}

impl ScheduledTask {
    /// Ligne lisible : « a1b2c3d4 · */5 * * * * · prochain 14:05 · vérifie… ».
    #[must_use]
    pub fn describe(&self) -> String {
        let kind = if self.self_paced {
            "boucle au rythme choisi".to_string()
        } else if self.recurring {
            self.cron.clone()
        } else {
            format!("une fois ({})", self.cron)
        };
        let prompt: String = self
            .prompt
            .lines()
            .next()
            .unwrap_or_default()
            .chars()
            .take(70)
            .collect();
        format!(
            "{} · {kind} · prochain {} · {prompt}",
            self.id,
            format_local(self.next_run)
        )
    }
}

struct Scheduler {
    tasks: Vec<ScheduledTask>,
    store: Option<PathBuf>,
}

fn scheduler() -> &'static Mutex<Scheduler> {
    static SCHEDULER: OnceLock<Mutex<Scheduler>> = OnceLock::new();
    SCHEDULER.get_or_init(|| {
        Mutex::new(Scheduler {
            tasks: Vec::new(),
            store: None,
        })
    })
}

fn lock() -> std::sync::MutexGuard<'static, Scheduler> {
    scheduler()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// `CLAW_DISABLE_CRON=1` ou `CLAUDE_CODE_DISABLE_CRON=1`.
#[must_use]
pub fn disabled() -> bool {
    ["CLAW_DISABLE_CRON", "CLAUDE_CODE_DISABLE_CRON"]
        .iter()
        .any(|name| std::env::var(name).is_ok_and(|value| value.trim() == "1"))
}

fn save(state: &Scheduler) {
    let Some(path) = &state.store else {
        return;
    };
    if state.tasks.is_empty() {
        let _ = fs::remove_file(path);
        return;
    }
    if let Ok(text) = serde_json::to_string_pretty(&state.tasks) {
        let _ = fs::write(path, text);
    }
}

/// Fichier des tâches d'une conversation (`session-1.json` →
/// `session-1.tasks.json`).
#[must_use]
pub fn store_path_for(session: &Path) -> PathBuf {
    session.with_extension("tasks.json")
}

/// Rattache les tâches à la conversation `session` : celles enregistrées
/// reviennent (sauf les récurrentes expirées et les ponctuelles passées) ;
/// celles de la conversation précédente sont oubliées.
pub fn bind_session(session: Option<&Path>) {
    let mut state = lock();
    state.store = session.map(store_path_for);
    state.tasks = state
        .store
        .as_ref()
        .and_then(|path| fs::read_to_string(path).ok())
        .and_then(|text| serde_json::from_str::<Vec<ScheduledTask>>(&text).ok())
        .unwrap_or_default();
    let now = now_secs();
    state.tasks.retain(|task| {
        if task.recurring {
            now < task.created_at + EXPIRY_SECS
        } else {
            task.next_run >= now
        }
    });
}

fn new_id(seed: &str) -> String {
    let mut hasher = DefaultHasher::new();
    seed.hash(&mut hasher);
    SystemTime::now().hash(&mut hasher);
    std::process::id().hash(&mut hasher);
    format!("{:08x}", hasher.finish() & 0xffff_ffff)
}

/// Planifie une tâche (outil `CronCreate`) ; renvoie la tâche créée.
pub fn create(
    cron: &str,
    prompt: &str,
    recurring: bool,
    max_runs: Option<u32>,
) -> Result<ScheduledTask, String> {
    create_task(cron, prompt, recurring, max_runs, false)
}

/// Tâche récurrente de `/loop <intervalle> <demande>`.
pub fn create_loop(
    cron: &str,
    prompt: &str,
    max_runs: Option<u32>,
) -> Result<ScheduledTask, String> {
    create_task(cron, prompt, true, max_runs, true)
}

fn create_task(
    cron: &str,
    prompt: &str,
    recurring: bool,
    max_runs: Option<u32>,
    from_loop: bool,
) -> Result<ScheduledTask, String> {
    if disabled() {
        return Err("les tâches planifiées sont désactivées (CLAW_DISABLE_CRON)".to_string());
    }
    let prompt = prompt.trim();
    if prompt.is_empty() {
        return Err("demande vide".to_string());
    }
    let expression = CronExpr::parse(cron)?;
    let now = now_secs();
    let next = expression
        .next_after(local_minute(now))
        .ok_or_else(|| format!("« {cron} » ne tombe jamais dans les quatre ans à venir"))?;
    let mut state = lock();
    if state.tasks.len() >= MAX_TASKS {
        return Err(format!(
            "{MAX_TASKS} tâches planifiées au plus : supprimez-en une (CronDelete)"
        ));
    }
    let task = ScheduledTask {
        id: new_id(prompt),
        cron: cron.split_whitespace().collect::<Vec<_>>().join(" "),
        prompt: prompt.to_string(),
        recurring,
        created_at: now,
        next_run: utc_secs_of(next),
        runs: 0,
        max_runs,
        self_paced: false,
        from_loop,
        fallback: false,
    };
    state.tasks.push(task.clone());
    save(&state);
    Ok(task)
}

/// Passage ponctuel dans `delay` (boucle au rythme choisi : `ScheduleWakeup`).
/// Remplace le réveil en attente de la même boucle.
pub fn schedule_wakeup(prompt: &str, delay: Duration) -> Result<ScheduledTask, String> {
    wakeup(prompt, delay, false)
}

/// Réveil de secours, 20 minutes après un passage qui n'a rien replanifié.
pub fn schedule_fallback(prompt: &str) -> Result<ScheduledTask, String> {
    wakeup(prompt, Duration::from_secs(1_200), true)
}

fn wakeup(prompt: &str, delay: Duration, fallback: bool) -> Result<ScheduledTask, String> {
    if disabled() {
        return Err("les tâches planifiées sont désactivées (CLAW_DISABLE_CRON)".to_string());
    }
    let now = now_secs();
    let at = now + delay.as_secs().clamp(60, 3_600);
    let local = LocalTime::from_minutes(local_minute(at));
    let mut state = lock();
    state
        .tasks
        .retain(|task| !(task.self_paced && task.prompt == prompt));
    let task = ScheduledTask {
        id: new_id(prompt),
        cron: format!(
            "{} {} {} {} *",
            local.minute, local.hour, local.day, local.month
        ),
        prompt: prompt.to_string(),
        recurring: false,
        created_at: now,
        next_run: at,
        runs: 0,
        max_runs: None,
        self_paced: true,
        from_loop: true,
        fallback,
    };
    state.tasks.push(task.clone());
    save(&state);
    Ok(task)
}

static STOP_REQUESTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Le modèle a arrêté sa boucle (`ScheduleWakeup` avec `stop`) depuis le
/// dernier appel.
pub fn take_stop_requested() -> bool {
    STOP_REQUESTED.swap(false, std::sync::atomic::Ordering::SeqCst)
}

/// Un réveil attend pour la boucle `prompt`.
#[must_use]
pub fn has_wakeup_for(prompt: &str) -> bool {
    lock()
        .tasks
        .iter()
        .any(|task| task.self_paced && task.prompt == prompt)
}

/// `/loop stop` : arrête les tâches créées par `/loop`.
#[must_use]
pub fn stop_loops() -> usize {
    let mut state = lock();
    let before = state.tasks.len();
    state.tasks.retain(|task| !task.from_loop);
    let removed = before - state.tasks.len();
    save(&state);
    removed
}

/// Arrête les boucles au rythme choisi (`ScheduleWakeup` avec `stop`).
pub fn stop_self_paced() -> usize {
    STOP_REQUESTED.store(true, std::sync::atomic::Ordering::SeqCst);
    let mut state = lock();
    let before = state.tasks.len();
    state.tasks.retain(|task| !task.self_paced);
    let removed = before - state.tasks.len();
    save(&state);
    removed
}

#[must_use]
pub fn list() -> Vec<ScheduledTask> {
    let mut tasks = lock().tasks.clone();
    tasks.sort_by_key(|task| task.next_run);
    tasks
}

/// Supprime la tâche `id` (ou les tâches dont l'identifiant commence ainsi,
/// s'il n'y en a qu'une).
pub fn delete(id: &str) -> Result<ScheduledTask, String> {
    let id = id.trim();
    let mut state = lock();
    let matches: Vec<usize> = state
        .tasks
        .iter()
        .enumerate()
        .filter(|(_, task)| task.id == id || (!id.is_empty() && task.id.starts_with(id)))
        .map(|(index, _)| index)
        .collect();
    match matches.as_slice() {
        [index] => {
            let task = state.tasks.remove(*index);
            save(&state);
            Ok(task)
        }
        [] => Err(format!("aucune tâche planifiée « {id} »")),
        _ => Err(format!(
            "« {id} » désigne plusieurs tâches : donnez l'identifiant complet"
        )),
    }
}

/// Supprime toutes les tâches.
#[must_use]
pub fn clear() -> usize {
    let mut state = lock();
    let count = state.tasks.len();
    state.tasks.clear();
    save(&state);
    count
}

/// Une tâche est due.
#[must_use]
pub fn has_due() -> bool {
    if disabled() {
        return false;
    }
    let now = now_secs();
    lock().tasks.iter().any(|task| task.next_run <= now)
}

/// Retire la tâche due la plus ancienne et prépare son passage suivant :
/// récurrente, elle est replanifiée (ou supprimée après son dernier passage,
/// à l'expiration ou à `max_runs`) ; ponctuelle, elle disparaît.
#[must_use]
pub fn take_due() -> Option<ScheduledTask> {
    if disabled() {
        return None;
    }
    let now = now_secs();
    let mut state = lock();
    let index = state
        .tasks
        .iter()
        .enumerate()
        .filter(|(_, task)| task.next_run <= now)
        .min_by_key(|(_, task)| task.next_run)
        .map(|(index, _)| index)?;
    let mut task = state.tasks[index].clone();
    task.runs += 1;
    let finished = !task.recurring
        || now >= task.created_at + EXPIRY_SECS
        || task.max_runs.is_some_and(|max| task.runs >= max);
    let next = CronExpr::parse(&task.cron)
        .ok()
        .and_then(|expression| expression.next_after(local_minute(now)));
    if let (false, Some(next)) = (finished, next) {
        let entry = &mut state.tasks[index];
        entry.runs = task.runs;
        entry.next_run = utc_secs_of(next);
        task.next_run = entry.next_run;
    } else {
        state.tasks.remove(index);
        task.recurring = false;
    }
    save(&state);
    Some(task)
}

/// Intervalle de `/loop` (`5m`, `2h`…) en expression cron : le pas cron le
/// plus proche (`7m` → toutes les 5 minutes). Renvoie l'expression et
/// l'intervalle retenu, en minutes.
#[must_use]
pub fn interval_to_cron(interval: Duration) -> (String, u64) {
    const MINUTE_STEPS: [u64; 11] = [1, 2, 3, 4, 5, 6, 10, 12, 15, 20, 30];
    const HOUR_STEPS: [u64; 8] = [1, 2, 3, 4, 6, 8, 12, 24];
    let minutes = interval.as_secs().div_ceil(60).max(1);
    if minutes < 45 {
        let step = MINUTE_STEPS
            .iter()
            .copied()
            .min_by_key(|step| step.abs_diff(minutes))
            .unwrap_or(5);
        let cron = if step == 1 {
            "* * * * *".to_string()
        } else {
            format!("*/{step} * * * *")
        };
        return (cron, step);
    }
    let hours = minutes.div_ceil(60).max(1);
    if hours <= 24 {
        let step = HOUR_STEPS
            .iter()
            .copied()
            .min_by_key(|step| step.abs_diff(hours))
            .unwrap_or(1);
        let cron = match step {
            1 => "0 * * * *".to_string(),
            24 => "0 9 * * *".to_string(),
            step => format!("0 */{step} * * *"),
        };
        return (cron, step * 60);
    }
    ("0 9 * * *".to_string(), 1_440)
}

#[cfg(test)]
mod tests {
    use super::{interval_to_cron, parse_offset, CronExpr, LocalTime};
    use std::time::Duration;

    /// Minute locale du 16/10/2026 à `hour`:`minute` (un vendredi).
    fn at(day: i64, hour: i64, minute: i64) -> i64 {
        // 2026-10-16 = 20 742 jours après 1970-01-01.
        (20_742 + day - 16) * 1_440 + hour * 60 + minute
    }

    #[test]
    fn cron_expressions_find_their_next_run() {
        let friday = LocalTime::from_minutes(at(16, 10, 0));
        assert_eq!(
            (friday.year, friday.month, friday.day, friday.weekday),
            (2026, 10, 16, 5)
        );
        let every_five = CronExpr::parse("*/5 * * * *").expect("cron");
        assert_eq!(every_five.next_after(at(16, 10, 2)), Some(at(16, 10, 5)));
        let weekdays = CronExpr::parse("0 9 * * 1-5").expect("cron");
        // Vendredi 10:00 → lundi 19/10 9:00.
        assert_eq!(weekdays.next_after(at(16, 10, 0)), Some(at(19, 9, 0)));
        let sunday = CronExpr::parse("30 14 * * 7").expect("cron");
        assert_eq!(sunday.next_after(at(16, 10, 0)), Some(at(18, 14, 30)));
        // Jour du mois ou jour de la semaine (vixie-cron).
        let either = CronExpr::parse("0 8 20 * 6").expect("cron");
        assert_eq!(either.next_after(at(16, 10, 0)), Some(at(17, 8, 0)));
        assert!(CronExpr::parse("0 9 * *").is_err());
        assert!(CronExpr::parse("61 * * * *").is_err());
        assert!(CronExpr::parse("MON * * * *").is_err());
    }

    #[test]
    fn loop_intervals_round_to_cron_steps() {
        assert_eq!(
            interval_to_cron(Duration::from_secs(300)),
            ("*/5 * * * *".to_string(), 5)
        );
        assert_eq!(interval_to_cron(Duration::from_secs(7 * 60)).1, 6);
        assert_eq!(
            interval_to_cron(Duration::from_secs(30)),
            ("* * * * *".to_string(), 1)
        );
        assert_eq!(
            interval_to_cron(Duration::from_secs(2 * 3_600)),
            ("0 */2 * * *".to_string(), 120)
        );
        assert_eq!(parse_offset("+0200"), Some(7_200));
        assert_eq!(parse_offset("-05:30"), Some(-19_800));
        assert_eq!(parse_offset("abc"), None);
    }
}
