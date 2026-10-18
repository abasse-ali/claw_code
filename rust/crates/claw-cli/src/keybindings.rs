//! Raccourcis clavier personnalisables, au format de Claude Code :
//! `~/.claude/keybindings.json` puis `~/.claw/keybindings.json` (le second
//! l'emporte). Chaque bloc donne un contexte et une table « touches → action »
//! (`null` retire un raccourci par défaut). Le fichier est relu à chaque
//! saisie : une modification s'applique sans redémarrer.

use std::fmt::Write as _;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use serde_json::Value;

/// Délai entre deux touches d'un accord (`ctrl+x ctrl+e`).
const CHORD_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KeyName {
    Char(char),
    Enter,
    Escape,
    Tab,
    Up,
    Down,
    Left,
    Right,
    PageUp,
    PageDown,
    Home,
    End,
    Backspace,
    Delete,
    F(u8),
}

/// Une touche avec ses modificateurs.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Keystroke {
    pub key: KeyName,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub sup: bool,
}

impl Keystroke {
    const fn plain(key: KeyName) -> Self {
        Self {
            key,
            ctrl: false,
            alt: false,
            shift: false,
            sup: false,
        }
    }

    /// Touche reçue du terminal, sous la forme utilisée par les raccourcis :
    /// lettre en minuscule (Maj à part), Maj+Tab pour `BackTab`, Ctrl+_ pour
    /// l'octet 0x1F que les terminaux envoient pour Ctrl+_ (lu « Ctrl+7 »).
    #[must_use]
    pub fn from_event(event: KeyEvent) -> Option<Self> {
        let modifiers = event.modifiers;
        let mut stroke = Self {
            key: KeyName::Enter,
            ctrl: modifiers.contains(KeyModifiers::CONTROL),
            alt: modifiers.contains(KeyModifiers::ALT),
            shift: modifiers.contains(KeyModifiers::SHIFT),
            sup: modifiers.contains(KeyModifiers::SUPER),
        };
        stroke.key = match event.code {
            KeyCode::Char(ch) => {
                if ch.is_ascii_uppercase() {
                    stroke.shift = true;
                    KeyName::Char(ch.to_ascii_lowercase())
                } else if ch.is_alphabetic() {
                    KeyName::Char(ch)
                } else {
                    // Le caractère porte déjà Maj (« _ », « ? »).
                    stroke.shift = false;
                    match ch {
                        '7' if stroke.ctrl => KeyName::Char('_'),
                        '-' if stroke.ctrl && modifiers.contains(KeyModifiers::SHIFT) => {
                            KeyName::Char('_')
                        }
                        _ => KeyName::Char(ch),
                    }
                }
            }
            KeyCode::Enter => KeyName::Enter,
            KeyCode::Esc => KeyName::Escape,
            KeyCode::Tab => KeyName::Tab,
            KeyCode::BackTab => {
                stroke.shift = true;
                KeyName::Tab
            }
            KeyCode::Up => KeyName::Up,
            KeyCode::Down => KeyName::Down,
            KeyCode::Left => KeyName::Left,
            KeyCode::Right => KeyName::Right,
            KeyCode::PageUp => KeyName::PageUp,
            KeyCode::PageDown => KeyName::PageDown,
            KeyCode::Home => KeyName::Home,
            KeyCode::End => KeyName::End,
            KeyCode::Backspace => KeyName::Backspace,
            KeyCode::Delete => KeyName::Delete,
            KeyCode::F(number) => KeyName::F(number),
            _ => return None,
        };
        Some(stroke)
    }

    /// Texte lisible (`ctrl+x`, `shift+tab`).
    #[must_use]
    pub fn label(&self) -> String {
        let mut text = String::new();
        for (on, name) in [
            (self.ctrl, "ctrl+"),
            (self.alt, "alt+"),
            (self.shift, "shift+"),
            (self.sup, "cmd+"),
        ] {
            if on {
                text.push_str(name);
            }
        }
        match self.key {
            KeyName::Char(' ') => text.push_str("space"),
            KeyName::Char(ch) => text.push(ch),
            KeyName::F(number) => {
                let _ = write!(text, "f{number}");
            }
            other => text.push_str(match other {
                KeyName::Enter => "enter",
                KeyName::Escape => "escape",
                KeyName::Tab => "tab",
                KeyName::Up => "up",
                KeyName::Down => "down",
                KeyName::Left => "left",
                KeyName::Right => "right",
                KeyName::PageUp => "pageup",
                KeyName::PageDown => "pagedown",
                KeyName::Home => "home",
                KeyName::End => "end",
                KeyName::Backspace => "backspace",
                _ => "delete",
            }),
        }
        text
    }
}

/// Lit `ctrl+shift+k`. Un modificateur mal écrit (`ctl+k`) est ignoré et
/// signalé, comme dans Claude Code.
fn parse_keystroke(text: &str, warnings: &mut Vec<String>) -> Option<Keystroke> {
    let lower = text.trim().to_ascii_lowercase();
    // `ctrl++` : la touche est « + ».
    let (mods, key) = if let Some(prefix) = lower.strip_suffix("++") {
        (prefix, "+")
    } else if lower == "+" {
        ("", "+")
    } else {
        match lower.rsplit_once('+') {
            Some((mods, key)) => (mods, key),
            None => ("", lower.as_str()),
        }
    };
    let mut stroke = Keystroke::plain(KeyName::Enter);
    for modifier in mods.split('+').filter(|part| !part.is_empty()) {
        match modifier {
            "ctrl" | "control" => stroke.ctrl = true,
            "shift" => stroke.shift = true,
            "alt" | "opt" | "option" | "meta" => stroke.alt = true,
            "cmd" | "command" | "super" | "win" => stroke.sup = true,
            other => warnings.push(format!(
                "« {text} » : modificateur inconnu « {other} », ignoré"
            )),
        }
    }
    stroke.key = match key {
        "escape" | "esc" => KeyName::Escape,
        "enter" | "return" => KeyName::Enter,
        "tab" => KeyName::Tab,
        "space" => KeyName::Char(' '),
        "up" => KeyName::Up,
        "down" => KeyName::Down,
        "left" => KeyName::Left,
        "right" => KeyName::Right,
        "pageup" => KeyName::PageUp,
        "pagedown" => KeyName::PageDown,
        "home" => KeyName::Home,
        "end" => KeyName::End,
        "backspace" => KeyName::Backspace,
        "delete" => KeyName::Delete,
        name if name.len() > 1
            && name.starts_with('f')
            && name[1..]
                .parse::<u8>()
                .is_ok_and(|number| (1..=24).contains(&number)) =>
        {
            KeyName::F(name[1..].parse().unwrap_or(1))
        }
        name if name.chars().count() == 1 => {
            let ch = name.chars().next().unwrap_or(' ');
            if ch.is_alphabetic() {
                KeyName::Char(ch)
            } else {
                // Le caractère porte déjà Maj ; `ctrl+shift+-` est la touche
                // qui donne « _ ».
                let underscore = stroke.shift && ch == '-';
                stroke.shift = false;
                KeyName::Char(if underscore { '_' } else { ch })
            }
        }
        _ => {
            warnings.push(format!("« {text} » : touche inconnue"));
            return None;
        }
    };
    Some(stroke)
}

fn parse_sequence(text: &str, warnings: &mut Vec<String>) -> Option<Vec<Keystroke>> {
    let strokes: Vec<Keystroke> = text
        .split_whitespace()
        .map(|part| parse_keystroke(part, warnings))
        .collect::<Option<_>>()?;
    (!strokes.is_empty()).then_some(strokes)
}

/// Contextes que Claw gère. Les autres contextes de Claude Code sont acceptés
/// et ignorés.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Context {
    Global,
    Chat,
    Autocomplete,
    HistorySearch,
}

impl Context {
    fn parse(name: &str) -> Option<Self> {
        match name {
            "Global" => Some(Self::Global),
            "Chat" => Some(Self::Chat),
            "Autocomplete" => Some(Self::Autocomplete),
            "HistorySearch" => Some(Self::HistorySearch),
            _ => None,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Global => "Global",
            Self::Chat => "Chat",
            Self::Autocomplete => "Autocomplete",
            Self::HistorySearch => "HistorySearch",
        }
    }
}

/// Contextes de Claude Code sans équivalent dans la saisie de Claw.
const OTHER_CLAUDE_CONTEXTS: &[&str] = &[
    "Settings",
    "Confirmation",
    "Tabs",
    "Help",
    "Transcript",
    "Task",
    "ThemePicker",
    "Attachments",
    "Footer",
    "MessageSelector",
    "DiffDialog",
    "DiffPanel",
    "ModelPicker",
    "EffortSlider",
    "Select",
    "Plugin",
    "AbovePrompt",
    "AbovePromptInput",
    "AbovePromptSelect",
    "Pane",
    "PaneField",
    "Agents",
    "Scroll",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Submit,
    Newline,
    Cancel,
    CycleMode,
    ImagePaste,
    ExternalEditor,
    Stash,
    Undo,
    Redraw,
    ShowDetails,
    HistoryPrevious,
    HistoryNext,
    HistorySearch,
    AutocompleteAccept,
    AutocompleteDismiss,
    AutocompletePrevious,
    AutocompleteNext,
    SearchNext,
    SearchAccept,
    SearchCancel,
    SearchExecute,
}

const ACTIONS: &[(&str, Action)] = &[
    ("chat:submit", Action::Submit),
    ("chat:newline", Action::Newline),
    ("chat:cancel", Action::Cancel),
    ("chat:cycleMode", Action::CycleMode),
    ("chat:imagePaste", Action::ImagePaste),
    ("chat:externalEditor", Action::ExternalEditor),
    ("chat:stash", Action::Stash),
    ("chat:undo", Action::Undo),
    ("chat:clearInput", Action::Redraw),
    ("chat:clearScreen", Action::Redraw),
    ("app:redraw", Action::Redraw),
    ("app:toggleTranscript", Action::ShowDetails),
    ("history:previous", Action::HistoryPrevious),
    ("history:next", Action::HistoryNext),
    ("history:search", Action::HistorySearch),
    ("autocomplete:accept", Action::AutocompleteAccept),
    ("autocomplete:dismiss", Action::AutocompleteDismiss),
    ("autocomplete:previous", Action::AutocompletePrevious),
    ("autocomplete:next", Action::AutocompleteNext),
    ("historySearch:next", Action::SearchNext),
    ("historySearch:accept", Action::SearchAccept),
    ("historySearch:cancel", Action::SearchCancel),
    ("historySearch:execute", Action::SearchExecute),
];

/// Actions de Claude Code (documentation des raccourcis) : une action
/// absente de Claw est acceptée et sans effet, un autre nom est une faute
/// de frappe signalée.
const CLAUDE_ACTIONS: &[&str] = &[
    "abovePrompt:focus",
    "abovePrompt:highlightNext",
    "abovePrompt:highlightPrevious",
    "abovePrompt:leave",
    "abovePrompt:next",
    "abovePrompt:press",
    "abovePrompt:previous",
    "abovePrompt:toggle",
    "agents:find",
    "agents:nextGroup",
    "agents:previousGroup",
    "agents:rename",
    "agents:switchView",
    "agents:togglePin",
    "app:cycleDiffBase",
    "app:diffFileListDown",
    "app:diffFileListUp",
    "app:exit",
    "app:interrupt",
    "app:redraw",
    "app:toggleDiffNoiseFilter",
    "app:toggleDiffPreSession",
    "app:toggleReplTab",
    "app:toggleTodos",
    "app:toggleTranscript",
    "attachments:exit",
    "attachments:next",
    "attachments:previous",
    "attachments:remove",
    "autocomplete:accept",
    "autocomplete:dismiss",
    "autocomplete:next",
    "autocomplete:previous",
    "chat:cancel",
    "chat:clearInput",
    "chat:clearScreen",
    "chat:cycleMode",
    "chat:externalEditor",
    "chat:fastMode",
    "chat:imagePaste",
    "chat:killAgents",
    "chat:modelPicker",
    "chat:newline",
    "chat:queueSubmit",
    "chat:sendNow",
    "chat:stash",
    "chat:submit",
    "chat:thinkingToggle",
    "chat:undo",
    "confirm:cycleMode",
    "confirm:next",
    "confirm:nextField",
    "confirm:no",
    "confirm:previous",
    "confirm:previousField",
    "confirm:toggle",
    "confirm:yes",
    "diff:back",
    "diff:dismiss",
    "diff:nextFile",
    "diff:nextSource",
    "diff:previousFile",
    "diff:previousSource",
    "effortSlider:decreaseEffort",
    "effortSlider:increaseEffort",
    "effortSlider:thisSessionOnly",
    "effortSlider:toggleUltracode",
    "footer:clearSelection",
    "footer:close",
    "footer:dismiss",
    "footer:down",
    "footer:next",
    "footer:openSelected",
    "footer:previous",
    "footer:up",
    "help:dismiss",
    "history:next",
    "history:previous",
    "history:search",
    "historySearch:accept",
    "historySearch:cancel",
    "historySearch:cycleScope",
    "historySearch:execute",
    "historySearch:next",
    "messageSelector:bottom",
    "messageSelector:down",
    "messageSelector:select",
    "messageSelector:top",
    "messageSelector:up",
    "modelPicker:decreaseEffort",
    "modelPicker:increaseEffort",
    "modelPicker:thisSessionOnly",
    "pane:bottom",
    "pane:close",
    "pane:grow",
    "pane:next",
    "pane:pageDown",
    "pane:pageUp",
    "pane:previous",
    "pane:scrollDown",
    "pane:scrollUp",
    "pane:shrink",
    "pane:top",
    "permission:toggleDebug",
    "plugin:favorite",
    "plugin:install",
    "plugin:toggle",
    "scroll:bottom",
    "scroll:fullPageDown",
    "scroll:fullPageUp",
    "scroll:halfPageDown",
    "scroll:halfPageUp",
    "scroll:lineDown",
    "scroll:lineUp",
    "scroll:pageDown",
    "scroll:pageUp",
    "scroll:top",
    "select:accept",
    "select:cancel",
    "select:first",
    "select:last",
    "select:next",
    "select:pageDown",
    "select:pageUp",
    "select:previous",
    "selection:clear",
    "selection:copy",
    "selection:extendDown",
    "selection:extendLeft",
    "selection:extendLineEnd",
    "selection:extendLineStart",
    "selection:extendRight",
    "selection:extendUp",
    "settings:retry",
    "settings:search",
    "tabs:next",
    "tabs:previous",
    "task:background",
    "theme:toggleSyntaxHighlighting",
    "transcript:exit",
    "transcript:toggleShowAll",
    "voice:pushToTalk",
];

impl Action {
    fn parse(name: &str) -> Option<Self> {
        ACTIONS
            .iter()
            .find(|(known, _)| *known == name)
            .map(|(_, action)| *action)
    }

    #[must_use]
    pub fn name(self) -> &'static str {
        ACTIONS
            .iter()
            .find(|(_, action)| *action == self)
            .map_or("?", |(name, _)| name)
    }
}

/// Raccourcis par défaut, ceux de Claude Code que Claw sait faire.
const DEFAULTS: &[(Context, &str, Action)] = &[
    (Context::Global, "ctrl+o", Action::ShowDetails),
    (Context::Chat, "enter", Action::Submit),
    (Context::Chat, "ctrl+j", Action::Newline),
    (Context::Chat, "shift+enter", Action::Newline),
    (Context::Chat, "escape", Action::Cancel),
    (Context::Chat, "shift+tab", Action::CycleMode),
    (Context::Chat, "ctrl+v", Action::ImagePaste),
    (Context::Chat, "alt+v", Action::ImagePaste),
    (Context::Chat, "ctrl+g", Action::ExternalEditor),
    (Context::Chat, "ctrl+x ctrl+e", Action::ExternalEditor),
    (Context::Chat, "ctrl+s", Action::Stash),
    (Context::Chat, "ctrl+_", Action::Undo),
    (Context::Chat, "ctrl+l", Action::Redraw),
    (Context::Chat, "up", Action::HistoryPrevious),
    (Context::Chat, "ctrl+p", Action::HistoryPrevious),
    (Context::Chat, "down", Action::HistoryNext),
    (Context::Chat, "ctrl+n", Action::HistoryNext),
    (Context::Chat, "ctrl+r", Action::HistorySearch),
    (Context::Chat, "tab", Action::AutocompleteAccept),
    (Context::Autocomplete, "tab", Action::AutocompleteAccept),
    (Context::Autocomplete, "escape", Action::AutocompleteDismiss),
    (Context::Autocomplete, "up", Action::AutocompletePrevious),
    (Context::Autocomplete, "down", Action::AutocompleteNext),
    (Context::Autocomplete, "enter", Action::Submit),
    (Context::HistorySearch, "ctrl+r", Action::SearchNext),
    (Context::HistorySearch, "escape", Action::SearchAccept),
    (Context::HistorySearch, "tab", Action::SearchAccept),
    (Context::HistorySearch, "ctrl+c", Action::SearchCancel),
    (Context::HistorySearch, "enter", Action::SearchExecute),
];

/// Touches que le terminal ne distingue pas ou que Claw garde (Ctrl+C
/// interrompt, Ctrl+D quitte).
const RESERVED: &[&str] = &["ctrl+c", "ctrl+d", "ctrl+m", "ctrl+[", "ctrl+i", "ctrl+h"];

#[derive(Debug, Clone)]
struct Binding {
    context: Context,
    keys: Vec<Keystroke>,
    action: Action,
}

/// Raccourcis en vigueur et remarques sur le fichier.
#[derive(Debug, Clone)]
pub struct Keymap {
    bindings: Vec<Binding>,
    pub warnings: Vec<String>,
    /// Fichiers lus.
    pub sources: Vec<PathBuf>,
}

/// Accord en cours (`ctrl+x` déjà tapé).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Chord {
    keys: Vec<Keystroke>,
    at: Option<Instant>,
}

impl Chord {
    /// Touches déjà tapées, pour l'avis « ctrl+x… ».
    #[must_use]
    pub fn label(&self) -> String {
        self.keys
            .iter()
            .map(Keystroke::label)
            .collect::<Vec<_>>()
            .join(" ")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resolution {
    Action(Action),
    /// Début d'un accord : attendre la touche suivante.
    Pending,
    /// Accord interrompu par une touche qui ne le complète pas.
    Cancelled,
    /// Aucun raccourci : la touche sert à la saisie.
    Unbound,
}

impl Default for Keymap {
    fn default() -> Self {
        let mut warnings = Vec::new();
        let bindings = DEFAULTS
            .iter()
            .filter_map(|(context, keys, action)| {
                Some(Binding {
                    context: *context,
                    keys: parse_sequence(keys, &mut warnings)?,
                    action: *action,
                })
            })
            .collect();
        Self {
            bindings,
            warnings,
            sources: Vec::new(),
        }
    }
}

/// `~/.claude/keybindings.json` (sauf `CLAW_NO_CLAUDE_CONFIG=1`) puis
/// `~/.claw/keybindings.json`.
#[must_use]
pub fn files() -> Vec<PathBuf> {
    let config_home = runtime::ConfigLoader::default_for(".")
        .config_home()
        .to_path_buf();
    let mut paths = Vec::new();
    let claude = std::env::var("CLAW_NO_CLAUDE_CONFIG").map_or(true, |value| value.trim() != "1");
    if claude {
        if let Some(home) = config_home.parent() {
            paths.push(home.join(".claude").join("keybindings.json"));
        }
    }
    paths.push(config_home.join("keybindings.json"));
    paths
}

/// Fichier que `/keybindings` crée ou ouvre.
#[must_use]
pub fn claw_file() -> PathBuf {
    files()
        .pop()
        .unwrap_or_else(|| PathBuf::from("keybindings.json"))
}

/// Modèle écrit par `/keybindings`.
pub const TEMPLATE: &str = r#"{
  "$schema": "https://www.schemastore.org/claude-code-keybindings.json",
  "$docs": "https://code.claude.com/docs/en/keybindings",
  "bindings": [
    {
      "context": "Chat",
      "bindings": {
        "ctrl+x ctrl+e": "chat:externalEditor"
      }
    }
  ]
}
"#;

impl Keymap {
    /// Raccourcis par défaut complétés par les fichiers de l'utilisateur.
    #[must_use]
    pub fn load() -> Self {
        let mut keymap = Self::default();
        for path in files() {
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            keymap.sources.push(path.clone());
            let label = path.display().to_string();
            match serde_json::from_str::<Value>(&text) {
                Ok(value) => keymap.apply(&value, &label),
                Err(error) => keymap.warnings.push(format!(
                    "{label} : JSON illisible ({error}), fichier ignoré"
                )),
            }
        }
        keymap
    }

    /// Applique un fichier `{ "bindings": [ { "context", "bindings" } ] }`.
    pub fn apply(&mut self, value: &Value, label: &str) {
        let Some(blocks) = value.get("bindings").and_then(Value::as_array) else {
            self.warnings
                .push(format!("{label} : tableau « bindings » manquant"));
            return;
        };
        for block in blocks {
            let context_name = block.get("context").and_then(Value::as_str).unwrap_or("");
            let Some(context) = Context::parse(context_name) else {
                if !OTHER_CLAUDE_CONTEXTS.contains(&context_name) {
                    self.warnings
                        .push(format!("{label} : contexte inconnu « {context_name} »"));
                }
                continue;
            };
            let Some(table) = block.get("bindings").and_then(Value::as_object) else {
                self.warnings.push(format!(
                    "{label} : le contexte {context_name} n'a pas de table « bindings »"
                ));
                continue;
            };
            for (keys_text, action_value) in table {
                let normalized = keys_text.trim().to_ascii_lowercase();
                if RESERVED.contains(&normalized.as_str()) {
                    self.warnings.push(format!(
                        "{label} : « {keys_text} » est réservé et ne peut pas être changé"
                    ));
                    continue;
                }
                let Some(keys) = parse_sequence(keys_text, &mut self.warnings) else {
                    continue;
                };
                let action = match action_value {
                    Value::Null => None,
                    Value::String(name) => {
                        if let Some(action) = Action::parse(name) {
                            Some(action)
                        } else {
                            if !CLAUDE_ACTIONS.contains(&name.as_str()) {
                                self.warnings
                                    .push(format!("{label} : action inconnue « {name} »"));
                            }
                            // Action de Claude Code sans équivalent : le
                            // raccourci par défaut reste en place.
                            continue;
                        }
                    }
                    _ => {
                        self.warnings.push(format!(
                            "{label} : « {keys_text} » doit valoir le nom d'une action ou null"
                        ));
                        continue;
                    }
                };
                self.bindings
                    .retain(|binding| !(binding.context == context && binding.keys == keys));
                if let Some(action) = action {
                    self.bindings.push(Binding {
                        context,
                        keys,
                        action,
                    });
                }
            }
        }
    }

    /// Action d'une touche dans les contextes actifs, du plus précis au plus
    /// général. Un accord commencé attend sa touche suivante (3 s au plus).
    pub fn resolve(
        &self,
        contexts: &[Context],
        chord: &mut Chord,
        stroke: Keystroke,
    ) -> Resolution {
        if chord
            .at
            .is_some_and(|started| started.elapsed() > CHORD_TIMEOUT)
        {
            chord.keys.clear();
        }
        let mut candidate = chord.keys.clone();
        candidate.push(stroke);
        let active = || {
            self.bindings
                .iter()
                .filter(|binding| contexts.contains(&binding.context))
        };
        if active().any(|binding| {
            binding.keys.len() > candidate.len() && binding.keys.starts_with(&candidate)
        }) {
            chord.keys = candidate;
            chord.at = Some(Instant::now());
            return Resolution::Pending;
        }
        let was_chord = !chord.keys.is_empty();
        chord.keys.clear();
        chord.at = None;
        let found = contexts.iter().find_map(|context| {
            active()
                .find(|binding| binding.context == *context && binding.keys == candidate)
                .map(|binding| binding.action)
        });
        match found {
            Some(action) => Resolution::Action(action),
            None if was_chord => Resolution::Cancelled,
            None => Resolution::Unbound,
        }
    }

    /// Raccourcis d'une action, pour l'aide (« ctrl+g, ctrl+x ctrl+e »).
    #[must_use]
    pub fn keys_for(&self, action: Action) -> Vec<String> {
        self.bindings
            .iter()
            .filter(|binding| binding.action == action)
            .map(|binding| {
                binding
                    .keys
                    .iter()
                    .map(Keystroke::label)
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .collect()
    }

    /// Tableau des raccourcis en vigueur, par contexte.
    #[must_use]
    pub fn report(&self) -> String {
        let mut lines = Vec::new();
        for context in [
            Context::Global,
            Context::Chat,
            Context::Autocomplete,
            Context::HistorySearch,
        ] {
            let entries: Vec<String> = self
                .bindings
                .iter()
                .filter(|binding| binding.context == context)
                .map(|binding| {
                    format!(
                        "    {:<18} {}",
                        binding
                            .keys
                            .iter()
                            .map(Keystroke::label)
                            .collect::<Vec<_>>()
                            .join(" "),
                        binding.action.name()
                    )
                })
                .collect();
            if !entries.is_empty() {
                lines.push(format!("  {}", context.name()));
                lines.extend(entries);
            }
        }
        lines.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::{Action, Chord, Context, Keymap, Keystroke, Resolution};
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn stroke(code: KeyCode, modifiers: KeyModifiers) -> Keystroke {
        Keystroke::from_event(KeyEvent::new(code, modifiers)).expect("touche")
    }

    #[test]
    fn user_bindings_override_unbind_and_chain() {
        let mut keymap = Keymap::default();
        keymap.apply(
            &serde_json::json!({
                "bindings": [
                    {"context": "Chat", "bindings": {
                        "ctrl+e": "chat:externalEditor",
                        "ctrl+s": null,
                        "ctl+k": "chat:stash",
                        "ctrl+c": "chat:submit",
                        "meta+p": "chat:modelPicker",
                        "ctrl+q": "chat:sumbit"
                    }},
                    {"context": "Transcript", "bindings": {"q": "transcript:exit"}},
                    {"context": "Chatt", "bindings": {}}
                ]
            }),
            "test",
        );
        let chat = [Context::Chat, Context::Global];
        let mut chord = Chord::default();
        let ctrl = KeyModifiers::CONTROL;
        assert_eq!(
            keymap.resolve(&chat, &mut chord, stroke(KeyCode::Char('e'), ctrl)),
            Resolution::Action(Action::ExternalEditor)
        );
        assert_eq!(
            keymap.resolve(&chat, &mut chord, stroke(KeyCode::Char('s'), ctrl)),
            Resolution::Unbound
        );
        // « ctl+k » : le modificateur mal écrit est retiré, reste « k ».
        assert_eq!(
            keymap.resolve(
                &chat,
                &mut chord,
                stroke(KeyCode::Char('k'), KeyModifiers::NONE)
            ),
            Resolution::Action(Action::Stash)
        );
        assert_eq!(
            keymap.resolve(&chat, &mut chord, stroke(KeyCode::Char('x'), ctrl)),
            Resolution::Pending
        );
        assert_eq!(chord.label(), "ctrl+x");
        assert_eq!(
            keymap.resolve(&chat, &mut chord, stroke(KeyCode::Char('e'), ctrl)),
            Resolution::Action(Action::ExternalEditor)
        );
        keymap.resolve(&chat, &mut chord, stroke(KeyCode::Char('x'), ctrl));
        assert_eq!(
            keymap.resolve(
                &chat,
                &mut chord,
                stroke(KeyCode::Char('z'), KeyModifiers::NONE)
            ),
            Resolution::Cancelled
        );
        let warnings = keymap.warnings.join("\n");
        assert!(warnings.contains("ctl"), "{warnings}");
        assert!(warnings.contains("réservé"), "{warnings}");
        assert!(warnings.contains("chat:sumbit"), "{warnings}");
        assert!(warnings.contains("Chatt"), "{warnings}");
        assert!(!warnings.contains("modelPicker"), "{warnings}");
        assert!(!warnings.contains("Transcript"), "{warnings}");
    }

    #[test]
    fn terminal_keys_match_their_written_form() {
        let keymap = Keymap::default();
        let mut chord = Chord::default();
        let chat = [Context::Chat];
        assert_eq!(
            keymap.resolve(
                &chat,
                &mut chord,
                stroke(KeyCode::BackTab, KeyModifiers::SHIFT)
            ),
            Resolution::Action(Action::CycleMode)
        );
        // Ctrl+_ arrive comme Ctrl+7 dans la plupart des terminaux.
        assert_eq!(
            keymap.resolve(
                &chat,
                &mut chord,
                stroke(KeyCode::Char('7'), KeyModifiers::CONTROL)
            ),
            Resolution::Action(Action::Undo)
        );
        let search = [Context::HistorySearch];
        assert_eq!(
            keymap.resolve(
                &search,
                &mut chord,
                stroke(KeyCode::Char('c'), KeyModifiers::CONTROL)
            ),
            Resolution::Action(Action::SearchCancel)
        );
        assert_eq!(
            stroke(KeyCode::Char('K'), KeyModifiers::SHIFT).label(),
            "shift+k"
        );
        assert!(keymap.report().contains("ctrl+x ctrl+e"));
    }
}
