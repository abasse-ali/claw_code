pub mod advisors;
pub mod channels;
pub mod input_check;
mod mcp_bridge;
pub mod mcp_oauth;
pub mod monitors;
mod questions;
mod web_page;
pub mod workflows;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

mod components;
mod icons;
mod ornaments;
mod page_view;
mod photos;
mod screenshot;

/// Un navigateur est installé : les outils `ViewPage` et `Screenshot` peuvent
/// montrer une page.
#[must_use]
pub fn page_viewer_available() -> bool {
    page_view::find_browser().is_some()
}

/// Regard d'un modèle qui lit les images pour un modèle qui n'en lit pas
/// (`LookAtImage`) : identifiant de l'image, question, réponse.
type ImageLooker = dyn Fn(&str, &str) -> Result<String, String> + Send + Sync;
static IMAGE_LOOKER: Mutex<Option<std::sync::Arc<ImageLooker>>> = Mutex::new(None);

/// L'application donne le regard de `LookAtImage` (son lecteur d'images).
pub fn set_image_looker(
    looker: impl Fn(&str, &str) -> Result<String, String> + Send + Sync + 'static,
) {
    *IMAGE_LOOKER
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(std::sync::Arc::new(looker));
}

#[derive(Debug, Deserialize)]
struct LookAtImageInput {
    image: String,
    question: String,
}

fn run_look_at_image(input: &LookAtImageInput) -> Result<String, String> {
    if input.question.trim().is_empty() {
        return Err("the question is empty: ask one precise thing about the image".to_string());
    }
    let looker = IMAGE_LOOKER
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
        .ok_or("LookAtImage is not available here")?;
    looker(&input.image, &input.question)
}

/// Captures prises par le dernier appel de `Screenshot` sur ce fil, à joindre
/// à son résultat.
#[must_use]
pub fn take_screenshots() -> Vec<runtime::attachments::ImageAttachment> {
    screenshot::take_captured()
}

use mcp_bridge::connected_mcp_tools;
pub use mcp_bridge::{
    call_mcp_tool, ensure_mcp_started, mcp_prompt_commands, mcp_prompt_invocation,
    mcp_status_report, shutdown_mcp, McpPromptEntry, McpResourceEntry, McpToolEntry,
};
pub use questions::{
    parse_questions, question_asker_available, set_question_asker, QuestionOption,
    SharedQuestionAsker, UserAnswer, UserQuestion,
};

use api::{
    max_tokens_for_model, resolve_model_alias, ContentBlockDelta, InputContentBlock, InputMessage,
    MessageRequest, MessageResponse, OutputContentBlock, ProviderClient,
    StreamEvent as ApiStreamEvent, ToolChoice, ToolDefinition, ToolResultContentBlock,
};
use plugins::PluginTool;
use reqwest::blocking::Client;
use runtime::{
    current_date_iso, edit_file, execute_bash, glob_search, grep_search, load_system_prompt,
    read_file, write_file, ApiClient, ApiRequest, AssistantEvent, BashCommandInput, ConfigLoader,
    ContentBlock, ConversationMessage, ConversationRuntime, GrepSearchInput, MessageRole,
    PermissionMode, PermissionPolicy, PermissionPromptDecision, PermissionPrompter,
    PermissionRequest, PermissionRules, RuntimeError, Session, TokenUsage, ToolError, ToolExecutor,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolManifestEntry {
    pub name: String,
    pub source: ToolSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolSource {
    Base,
    Conditional,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ToolRegistry {
    entries: Vec<ToolManifestEntry>,
}

impl ToolRegistry {
    #[must_use]
    pub fn new(entries: Vec<ToolManifestEntry>) -> Self {
        Self { entries }
    }

    #[must_use]
    pub fn entries(&self) -> &[ToolManifestEntry] {
        &self.entries
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolSpec {
    pub name: &'static str,
    pub description: &'static str,
    pub input_schema: Value,
    pub required_permission: PermissionMode,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GlobalToolRegistry {
    plugin_tools: Vec<PluginTool>,
    mcp_tools: Vec<McpToolEntry>,
}

impl GlobalToolRegistry {
    #[must_use]
    pub fn builtin() -> Self {
        Self {
            plugin_tools: Vec::new(),
            mcp_tools: Vec::new(),
        }
    }

    /// Ajoute les outils des serveurs MCP connectés (voir [`ensure_mcp_started`]) ;
    /// un outil dont le nom est déjà pris est ignoré.
    #[must_use]
    pub fn with_mcp_tools(mut self, mcp_tools: Vec<McpToolEntry>) -> Self {
        let mut taken = mvp_tool_specs()
            .into_iter()
            .map(|spec| spec.name.to_string())
            .chain(
                self.plugin_tools
                    .iter()
                    .map(|tool| tool.definition().name.clone()),
            )
            .collect::<BTreeSet<_>>();
        self.mcp_tools = mcp_tools
            .into_iter()
            .filter(|tool| taken.insert(tool.name.clone()))
            .collect();
        self
    }

    pub fn with_plugin_tools(plugin_tools: Vec<PluginTool>) -> Result<Self, String> {
        let builtin_names = mvp_tool_specs()
            .into_iter()
            .map(|spec| spec.name.to_string())
            .collect::<BTreeSet<_>>();
        let mut seen_plugin_names = BTreeSet::new();

        for tool in &plugin_tools {
            let name = tool.definition().name.clone();
            if builtin_names.contains(&name) {
                return Err(format!(
                    "plugin tool `{name}` conflicts with a built-in tool name"
                ));
            }
            if !seen_plugin_names.insert(name.clone()) {
                return Err(format!("duplicate plugin tool name `{name}`"));
            }
        }

        Ok(Self {
            plugin_tools,
            mcp_tools: Vec::new(),
        })
    }

    /// Nom d'un outil tel que le registre l'attend, quand le modèle l'appelle
    /// avec une autre casse ou sous son nom Claude Code (« Bash », « Read »,
    /// « `MultiEdit` »…). `None` si l'outil est inconnu.
    #[must_use]
    pub fn resolve_tool_name(&self, name: &str) -> Option<String> {
        let known = mvp_tool_specs()
            .iter()
            .map(|spec| spec.name.to_string())
            .chain(
                self.plugin_tools
                    .iter()
                    .map(|tool| tool.definition().name.clone()),
            )
            .chain(self.mcp_tools.iter().map(|tool| tool.name.clone()))
            .collect::<Vec<_>>();
        resolve_name_among(name, &known)
    }

    pub fn normalize_allowed_tools(
        &self,
        values: &[String],
    ) -> Result<Option<BTreeSet<String>>, String> {
        if values.is_empty() {
            return Ok(None);
        }

        let builtin_specs = mvp_tool_specs();
        let canonical_names = builtin_specs
            .iter()
            .map(|spec| spec.name.to_string())
            .chain(
                self.plugin_tools
                    .iter()
                    .map(|tool| tool.definition().name.clone()),
            )
            .chain(self.mcp_tools.iter().map(|tool| tool.name.clone()))
            .collect::<Vec<_>>();
        let mut name_map = canonical_names
            .iter()
            .map(|name| (normalize_tool_name(name), name.clone()))
            .collect::<BTreeMap<_, _>>();

        for (alias, canonical) in [
            ("read", "read_file"),
            ("write", "write_file"),
            ("edit", "edit_file"),
            ("glob", "glob_search"),
            ("grep", "grep_search"),
        ] {
            name_map.insert(alias.to_string(), canonical.to_string());
        }

        let mut allowed = BTreeSet::new();
        for value in values {
            for token in value
                .split(|ch: char| ch == ',' || ch.is_whitespace())
                .filter(|token| !token.is_empty())
            {
                let normalized = normalize_tool_name(token);
                let canonical = name_map.get(&normalized).ok_or_else(|| {
                    format!(
                        "unsupported tool in --allowedTools: {token} (expected one of: {})",
                        canonical_names.join(", ")
                    )
                })?;
                allowed.insert(canonical.clone());
            }
        }

        Ok(Some(allowed))
    }

    #[must_use]
    pub fn definitions(&self, allowed_tools: Option<&BTreeSet<String>>) -> Vec<ToolDefinition> {
        let builtin = mvp_tool_specs()
            .into_iter()
            .filter(|spec| allowed_tools.is_none_or(|allowed| allowed.contains(spec.name)))
            .map(|spec| ToolDefinition {
                name: spec.name.to_string(),
                description: Some(spec.description.to_string()),
                input_schema: spec.input_schema,
            });
        let plugin = self
            .plugin_tools
            .iter()
            .filter(|tool| {
                allowed_tools
                    .is_none_or(|allowed| allowed.contains(tool.definition().name.as_str()))
            })
            .map(|tool| ToolDefinition {
                name: tool.definition().name.clone(),
                description: tool.definition().description.clone(),
                input_schema: tool.definition().input_schema.clone(),
            });
        let mcp = self
            .mcp_tools
            .iter()
            .filter(|tool| allowed_tools.is_none_or(|allowed| allowed.contains(tool.name.as_str())))
            .map(|tool| ToolDefinition {
                name: tool.name.clone(),
                description: Some(format!(
                    "[MCP {}] {}",
                    tool.server,
                    tool.description.as_deref().unwrap_or_default()
                )),
                input_schema: tool.input_schema.clone(),
            });
        builtin.chain(plugin).chain(mcp).collect()
    }

    #[must_use]
    pub fn permission_specs(
        &self,
        allowed_tools: Option<&BTreeSet<String>>,
    ) -> Vec<(String, PermissionMode)> {
        let builtin = mvp_tool_specs()
            .into_iter()
            .filter(|spec| allowed_tools.is_none_or(|allowed| allowed.contains(spec.name)))
            .map(|spec| (spec.name.to_string(), spec.required_permission));
        let plugin = self
            .plugin_tools
            .iter()
            .filter(|tool| {
                allowed_tools
                    .is_none_or(|allowed| allowed.contains(tool.definition().name.as_str()))
            })
            .map(|tool| {
                (
                    tool.definition().name.clone(),
                    permission_mode_from_plugin(tool.required_permission()),
                )
            });
        let mcp = self
            .mcp_tools
            .iter()
            .filter(|tool| allowed_tools.is_none_or(|allowed| allowed.contains(tool.name.as_str())))
            .map(|tool| (tool.name.clone(), tool.required_permission()));
        builtin.chain(plugin).chain(mcp).collect()
    }

    pub fn execute(&self, name: &str, input: &Value) -> Result<String, String> {
        if mvp_tool_specs().iter().any(|spec| spec.name == name) {
            return execute_tool(name, input);
        }
        if self.mcp_tools.iter().any(|tool| tool.name == name) {
            return call_mcp_tool(name, input);
        }
        self.plugin_tools
            .iter()
            .find(|tool| tool.definition().name == name)
            .ok_or_else(|| format!("unsupported tool: {name}"))?
            .execute(input)
            .map_err(|error| error.to_string())
    }
}

fn normalize_tool_name(value: &str) -> String {
    value.trim().replace('-', "_").to_ascii_lowercase()
}

fn permission_mode_from_plugin(value: &str) -> PermissionMode {
    match value {
        "read-only" => PermissionMode::ReadOnly,
        "workspace-write" => PermissionMode::WorkspaceWrite,
        "danger-full-access" => PermissionMode::DangerFullAccess,
        other => panic!("permission de plugin non prise en charge : {other}"),
    }
}

#[must_use]
#[allow(clippy::too_many_lines)]
pub fn mvp_tool_specs() -> Vec<ToolSpec> {
    vec![
        ToolSpec {
            name: "bash",
            // Sous Windows, les modèles écrivent volontiers pour cmd : la
            // description de l'outil est lue plus sûrement que les consignes.
            description: if cfg!(windows) {
                "Execute a shell command in the current workspace. On Windows this is Git Bash: POSIX syntax, paths with forward slashes (`src/main.py`, `D:/dir/file`), no cmd syntax. Without `timeout`, a command still running after 5 minutes continues in the background (nothing is stopped) and you follow it with BashOutput. With `timeout` (milliseconds; a value below 1000 is read as seconds), a command still running at that time is stopped and its output so far is returned. Servers and watchers (npm run dev, uvicorn, flask run, vite, --watch...) never exit: start them with run_in_background: true (Claw does it for the usual ones) and read them with BashOutput."
            } else {
                "Execute a shell command in the current workspace. Without `timeout`, a command still running after 5 minutes continues in the background (nothing is stopped) and you follow it with BashOutput. With `timeout` (milliseconds; a value below 1000 is read as seconds), a command still running at that time is stopped and its output so far is returned. Servers and watchers (npm run dev, uvicorn, flask run, vite, --watch...) never exit: start them with run_in_background: true (Claw does it for the usual ones) and read them with BashOutput."
            },
            input_schema: json!({
                "type": "object",
                "properties": {
                    "command": { "type": "string" },
                    "timeout": { "type": "integer", "minimum": 1 },
                    "description": { "type": "string", "description": "What the command does, in 5 to 10 words in the user's language, shown to the user (e.g. \"Run the unit tests\")" },
                    "run_in_background": { "type": "boolean" },
                    "dangerouslyDisableSandbox": { "type": "boolean" }
                },
                "required": ["command"],
                "additionalProperties": false
            }),
            required_permission: PermissionMode::DangerFullAccess,
        },
        ToolSpec {
            name: "read_file",
            description: "Read a text file from the workspace: up to 2000 lines starting at `offset` (0-based line index, default 0); `limit` sets how many lines. Very long lines are shortened. When the file goes on, the result's `note` says which offset reads the rest.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "offset": { "type": "integer", "minimum": 0 },
                    "limit": { "type": "integer", "minimum": 1 }
                },
                "required": ["path"],
                "additionalProperties": false
            }),
            required_permission: PermissionMode::ReadOnly,
        },
        ToolSpec {
            name: "write_file",
            description: "Create a text file in the workspace, or replace the whole content of one. `content` is the complete new content of the file. To change part of an existing file, use edit_file.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "content": { "type": "string" }
                },
                "required": ["path", "content"],
                "additionalProperties": false
            }),
            required_permission: PermissionMode::WorkspaceWrite,
        },
        ToolSpec {
            name: "edit_file",
            description: "Replace an exact piece of text in an existing workspace file. Read the file first. `old_string` is copied from the file with its exact indentation and must occur exactly once (add a few surrounding lines to make it unique), unless `replace_all` is true; `new_string` replaces it. Keep each edit small: the lines that change and a little context, never the whole file. To create a file or rewrite all of it, use write_file.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "old_string": { "type": "string" },
                    "new_string": { "type": "string" },
                    "replace_all": { "type": "boolean" }
                },
                "required": ["path", "old_string", "new_string"],
                "additionalProperties": false
            }),
            required_permission: PermissionMode::WorkspaceWrite,
        },
        ToolSpec {
            name: "glob_search",
            description: "Find files by glob pattern (for example `src/**/*.ts`), newest first, at most 100. Dependency, cache and tool folders (node_modules, .git, .venv, __pycache__...) and build output that git ignores (dist, build, target...) are skipped unless the pattern or path names them; when nothing is found, the result's `note` lists what was skipped.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "pattern": { "type": "string" },
                    "path": { "type": "string" }
                },
                "required": ["pattern"],
                "additionalProperties": false
            }),
            required_permission: PermissionMode::ReadOnly,
        },
        ToolSpec {
            name: "grep_search",
            description: "Search file contents with a regex (Rust syntax: escape `(`, `{`, `.` to match them literally). output_mode: `files_with_matches` (default), `content` (matching lines as path:line:text, with -A/-B/-C context lines) or `count`. `glob` filters files (`**/*.py`), `-i` ignores case. Dependency, cache and tool folders (node_modules, .git, .venv, __pycache__...), build output that git ignores (dist, build, target...) and files over 4 MB are skipped unless the path or glob names those folders; a file given as `path` is always searched. The result's `note` says what was skipped when that matters.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "pattern": { "type": "string" },
                    "path": { "type": "string" },
                    "glob": { "type": "string" },
                    "output_mode": {
                        "type": "string",
                        "enum": ["content", "files_with_matches", "count"]
                    },
                    "-B": { "type": "integer", "minimum": 0 },
                    "-A": { "type": "integer", "minimum": 0 },
                    "-C": { "type": "integer", "minimum": 0 },
                    "context": { "type": "integer", "minimum": 0 },
                    "-n": { "type": "boolean" },
                    "-i": { "type": "boolean" },
                    "type": {
                        "type": "string",
                        "description": "File type: a language (python, rust, typescript...) or an extension (py, rs, ts)"
                    },
                    "head_limit": { "type": "integer", "minimum": 1 },
                    "offset": { "type": "integer", "minimum": 0 },
                    "multiline": { "type": "boolean" }
                },
                "required": ["pattern"],
                "additionalProperties": false
            }),
            required_permission: PermissionMode::ReadOnly,
        },
        ToolSpec {
            name: "WebFetch",
            description:
                "Fetch a URL and return the text of the page (HTML is converted to Markdown). A long page returns its start and the passages that match `prompt`; pass `offset` to read it in order, part by part.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "url": { "type": "string", "format": "uri" },
                    "prompt": {
                        "type": "string",
                        "description": "What you are looking for in the page. Its words select the passages returned from a long page."
                    },
                    "offset": {
                        "type": "integer",
                        "minimum": 0,
                        "description": "Position in the page text to read from, as given by a previous result."
                    }
                },
                "required": ["url", "prompt"],
                "additionalProperties": false
            }),
            required_permission: PermissionMode::ReadOnly,
        },
        ToolSpec {
            name: "ViewPage",
            description:
                "Open a web page in a headless browser and return what a user would see: the text displayed after the page's JavaScript ran, and what the page wrote to the console (errors, exceptions, failed requests). Use it to check a page you built or changed, and to investigate a display problem the user reports: a 200 response or a successful build does not show that the page works. For a local project, start its server first (in the background), then give its http://localhost address. It shows the page as it loads: it does not click or type.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "url": { "type": "string", "description": "An http or https address, for example http://localhost:3000/, or the file:/// address of an .html file of the project" },
                    "wait_ms": {
                        "type": "integer",
                        "minimum": 200,
                        "maximum": 15000,
                        "description": "Time given to the page to load and run its scripts, in milliseconds (default 3000)."
                    }
                },
                "required": ["url"],
                "additionalProperties": false
            }),
            required_permission: PermissionMode::ReadOnly,
        },
        ToolSpec {
            name: "Screenshot",
            description:
                "Take screenshots of a web page in a headless browser, at phone and desktop sizes by default, and measure its layout problems: content wider than the screen, cut or overlapping text and buttons, elements that should be hidden but show, broken images, text too small or too pale, plus console errors. The screenshots follow the result as images (a model that cannot see images gets their description). Use it after building or changing how a page looks, and when the user reports a display problem: fix what it finds, then take the screenshots again until the list is empty and the images look right. `actions` first bring the page to the state to check (open a menu, fill a field, scroll down). For a local project, start its server in the background and give its http://localhost address, or give `path` for a plain .html file.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "url": { "type": "string", "description": "An http or https address, for example http://localhost:5173/" },
                    "path": { "type": "string", "description": "Instead of url: an .html file of the project, for a page that needs no server." },
                    "viewports": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Screen sizes, one screenshot each: \"phone\" (390x844), \"tablet\" (820x1180), \"desktop\" (1280x800) or \"WIDTHxHEIGHT\". Default: phone and desktop."
                    },
                    "theme": { "type": "string", "enum": ["light", "dark"], "description": "Color scheme the device says it prefers (prefers-color-scheme)." },
                    "full_page": { "type": "boolean", "description": "Capture the page down to its end (up to three screens) instead of the first screen." },
                    "wait_ms": {
                        "type": "integer",
                        "minimum": 0,
                        "maximum": 15000,
                        "description": "Time left to the page after it loads and after each action, in milliseconds (default 1500)."
                    },
                    "actions": {
                        "type": "array",
                        "description": "Steps played in order before each screenshot: {\"click\": \"CSS selector\"}, {\"type\": \"text\", \"into\": \"CSS selector\"}, {\"scroll\": \"CSS selector\" or \"bottom\"}, {\"wait_ms\": 500}.",
                        "items": { "type": "object" }
                    }
                },
                "additionalProperties": false
            }),
            required_permission: PermissionMode::ReadOnly,
        },
        ToolSpec {
            name: "Icon",
            description: "Polished SVG icons for a web page, ready to paste: 1539 line icons from Lucide (ISC license), drawn on a 24px grid in the text color (currentColor). Use them instead of drawing icons by hand, and never use emoji. Give `names` to get several icons in one call (\"map-pin\", \"phone\", \"clock\", \"croissant\", \"leaf\", \"calendar\", \"star\", \"menu\", \"x\", \"arrow-right\"), or `search` with English words (\"bread pastry\", \"location\") to find names. Each icon comes as a complete <svg> with width, height and aria-hidden; color and size it with CSS.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "names": { "type": "array", "items": { "type": "string" }, "description": "Icon names, for example [\"map-pin\", \"phone\", \"clock\"]" },
                    "search": { "type": "string", "description": "Words to find icon names, in English (French words are understood too): \"bread pastry\", \"location\"" },
                    "size": { "type": "integer", "minimum": 12, "maximum": 160, "description": "Width and height in pixels (default 24)" },
                    "stroke_width": { "type": "number", "minimum": 0.5, "maximum": 3, "description": "Line width (default 2; 1.5 looks finer at large sizes)" },
                    "label": { "type": "string", "description": "Text for screen readers when the icon alone carries meaning (an icon-only link)" }
                },
                "additionalProperties": false
            }),
            required_permission: PermissionMode::ReadOnly,
        },
        ToolSpec {
            name: "Ornament",
            description: "Decorative SVG ornaments drawn from formulas in the plan's colors, clean at any size: blob (organic shape behind a title or an image), waves (layered waves across the full width, the bottom edge of a hero), dots and lines (quiet background textures), topography (contour lines), grain (film grain over a flat color), arch (a frame for an image or a text), frame (a medallion around an avatar or an initial), sprig (a botanical branch), rays (a sun). Use it instead of drawing illustrations, waves or avatars by hand: hand-drawn shapes look crude. Each ornament comes as a complete <svg> with aria-hidden, and as a data URI for a CSS background.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "kind": { "type": "string", "enum": ["blob", "waves", "dots", "lines", "topography", "grain", "arch", "frame", "sprig", "rays"] },
                    "colors": { "type": "array", "items": { "type": "string" }, "description": "The plan's colors in hexadecimal, main one first: [\"#c46b3a\", \"#e8d5c4\", \"#6b8e7a\"]" },
                    "width": { "type": "integer", "minimum": 16, "maximum": 2400 },
                    "height": { "type": "integer", "minimum": 16, "maximum": 2400 },
                    "seed": { "type": "integer", "minimum": 0, "description": "Another value gives another shape of the same kind" },
                    "count": { "type": "integer", "minimum": 1, "maximum": 36, "description": "Layers (blob, waves), rings (topography), arches, leaves (sprig), rays, or the pattern gap in pixels (dots, lines)" },
                    "opacity": { "type": "number", "minimum": 0.05, "maximum": 1 }
                },
                "required": ["kind"],
                "additionalProperties": false
            }),
            required_permission: PermissionMode::ReadOnly,
        },
        ToolSpec {
            name: "Photo",
            description: "Real photos under free licences (CC0, public domain, CC BY, CC BY-SA), found by English keywords on Openverse and Wikimedia Commons and saved in the project (images/ by default), with their real width and height and a credit to put in the footer (also written to images/CREDITS.md). Use it when the subject needs pictures (a bakery's bread, a studio's room, a dish, a place) instead of icon placeholders or crude hand-drawn scenes; name what the photo shows in two or three words (\"sourdough bread\", \"yoga class\"), not a mood. Check each photo with Screenshot: one that does not fit the subject is worse than none.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "search": { "type": "string", "description": "Two or three English words naming what the photo shows: \"sourdough bread\", \"yoga class\", \"meditation room\" (each word must match the photo: long searches find nothing)" },
                    "count": { "type": "integer", "minimum": 1, "maximum": 4, "description": "Photos to save (default 1)" },
                    "orientation": { "type": "string", "enum": ["landscape", "portrait", "square", "any"] },
                    "folder": { "type": "string", "description": "Folder inside the project (default images)" },
                    "name": { "type": "string", "description": "Base file name (default: from the search words)" }
                },
                "required": ["search"],
                "additionalProperties": false
            }),
            required_permission: PermissionMode::WorkspaceWrite,
        },
        ToolSpec {
            name: "Component",
            description: "Interactive components that work as soon as they are pasted (HTML, CSS and dependency-free JS), accessible, in the plan's colors, with icons from the icon set: nav-menu (header and phone menu), accordion (FAQ), tabs, dialog (modal), toast (confirmation messages), carousel, booking (days, slots, places left, calendar file), filter (tags and search), form (validation, sending, success), cart (basket or quote), theme-toggle (dark mode), copy, share, open-now (live opening hours), directions (route, call, map), reveal (appear on scroll), count-up (key figures), scroll-effects (reading progress, parallax), page-transitions. Use them instead of writing these by hand: every button then does what it says. Ask for several by name in one call; without names you get the list.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "names": { "type": "array", "items": { "type": "string" }, "description": "Component names, for example [\"nav-menu\", \"booking\", \"open-now\"]" }
                },
                "additionalProperties": false
            }),
            required_permission: PermissionMode::ReadOnly,
        },
        ToolSpec {
            name: "LookAtImage",
            description:
                "Look again at an image of this conversation that you received as a description (you cannot see images: a model that can see them looks for you and answers). Ask one precise question per call: the exact text at a place, a color, the state of a control, the position, size or alignment of elements, a detail the description does not give or that you doubt. Use it before relying on a detail that matters for your work.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "image": { "type": "string", "description": "The image id, given in the header of its description." },
                    "question": { "type": "string", "description": "One precise question about what the image shows." }
                },
                "required": ["image", "question"],
                "additionalProperties": false
            }),
            required_permission: PermissionMode::ReadOnly,
        },
        ToolSpec {
            name: "WebSearch",
            description: "Search the web for current information and return cited results.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "minLength": 2 },
                    "allowed_domains": {
                        "type": "array",
                        "items": { "type": "string" }
                    },
                    "blocked_domains": {
                        "type": "array",
                        "items": { "type": "string" }
                    }
                },
                "required": ["query"],
                "additionalProperties": false
            }),
            required_permission: PermissionMode::ReadOnly,
        },
        ToolSpec {
            name: "TodoWrite",
            description: "Update the structured task list for the current session. Send it in the same reply as the next tool call of your work, not alone: a reply that only updates the list costs a full request.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "todos": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "content": { "type": "string" },
                                "activeForm": { "type": "string" },
                                "status": {
                                    "type": "string",
                                    "enum": ["pending", "in_progress", "completed"]
                                }
                            },
                            "required": ["content", "activeForm", "status"],
                            "additionalProperties": false
                        }
                    }
                },
                "required": ["todos"],
                "additionalProperties": false
            }),
            // Simple liste de suivi (.claw/todos.json) : jamais de demande
            // d'autorisation, y compris en mode plan, comme dans Claude Code.
            required_permission: PermissionMode::ReadOnly,
        },
        ToolSpec {
            name: "CronCreate",
            description: "Schedule a prompt to run later in this session, between turns: a 5-field cron expression in local time (minute hour day-of-month month day-of-week), the prompt, and recurring (default true; false runs once then deletes itself). Use it for reminders (\"in 45 minutes check the tests\": pin the minute and hour) and polling. Recurring tasks expire after 7 days.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "cron": { "type": "string" },
                    "prompt": { "type": "string" },
                    "recurring": { "type": "boolean" }
                },
                "required": ["cron", "prompt"],
                "additionalProperties": false
            }),
            required_permission: PermissionMode::ReadOnly,
        },
        ToolSpec {
            name: "CronList",
            description: "List this session's scheduled tasks: id, schedule, next run time, prompt.",
            input_schema: json!({
                "type": "object",
                "properties": {},
                "additionalProperties": false
            }),
            required_permission: PermissionMode::ReadOnly,
        },
        ToolSpec {
            name: "CronDelete",
            description: "Cancel a scheduled task by its 8-character id.",
            input_schema: json!({
                "type": "object",
                "properties": { "id": { "type": "string" } },
                "required": ["id"],
                "additionalProperties": false
            }),
            required_permission: PermissionMode::ReadOnly,
        },
        ToolSpec {
            name: "ScheduleWakeup",
            description: "Self-paced /loop only: at the end of an iteration, schedule the next one in delaySeconds (60-3600, chosen from what you observed: short while something is in progress, long when nothing is pending) with the same prompt and a one-line reason, or pass stop: true when the task is complete.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "delaySeconds": { "type": "integer", "minimum": 60, "maximum": 3600 },
                    "prompt": { "type": "string" },
                    "reason": { "type": "string" },
                    "stop": { "type": "boolean" }
                },
                "additionalProperties": false
            }),
            required_permission: PermissionMode::ReadOnly,
        },
        ToolSpec {
            name: "Consult",
            description: "Ask two or three other AI models the same hard question and get their independent answers. Use it for a difficult design decision, when you hesitate between approaches, or when you are still stuck after several failed attempts. They do not see this conversation or the files: put everything they need in `context` (code excerpts, error messages, constraints).",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "question": {
                        "type": "string",
                        "description": "The precise question to answer."
                    },
                    "context": {
                        "type": "string",
                        "description": "Code, errors and constraints needed to answer; the other models see nothing else."
                    }
                },
                "required": ["question"],
                "additionalProperties": false
            }),
            required_permission: PermissionMode::ReadOnly,
        },
        ToolSpec {
            name: "AskUserQuestion",
            description: "Ask the user to choose between options, shown to them as clickable suggestions. Use it whenever you need a decision or a confirmation before continuing: the request is ambiguous, several approaches are reasonable, or the user asked you to wait for their validation. Ask 1 to 4 questions in one call. Give each question 2 to 4 concrete, mutually exclusive options, the one you recommend first with \"(Recommandé)\" at the end of its label (or the equivalent in the user's language), a one-sentence `description` of what each option implies, and a short `header` naming the topic (at most 12 characters). Set `multiSelect` when several options can be chosen together. Do not add an \"Other\" option: the user can always type their own answer. Never write these questions in your reply instead: call this tool, then continue the task with the answers it returns.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "questions": {
                        "type": "array",
                        "minItems": 1,
                        "maxItems": 4,
                        "items": {
                            "type": "object",
                            "properties": {
                                "question": {
                                    "type": "string",
                                    "description": "The complete question, ending with a question mark."
                                },
                                "header": {
                                    "type": "string",
                                    "description": "Very short topic label, at most 12 characters."
                                },
                                "options": {
                                    "type": "array",
                                    "minItems": 2,
                                    "maxItems": 4,
                                    "items": {
                                        "type": "object",
                                        "properties": {
                                            "label": { "type": "string", "description": "The choice, in 1 to 5 words." },
                                            "description": { "type": "string", "description": "What this choice implies, in one sentence." }
                                        },
                                        "required": ["label"]
                                    }
                                },
                                "multiSelect": {
                                    "type": "boolean",
                                    "description": "True when several options can be chosen together."
                                }
                            },
                            "required": ["question", "options"]
                        }
                    }
                },
                "required": ["questions"]
            }),
            required_permission: PermissionMode::ReadOnly,
        },
        ToolSpec {
            name: "EditorDiagnostics",
            description: "List the problems the user's editor (VS Code) currently reports: compile errors, type errors and lint warnings, exactly as underlined in the editor. Use it after changing code to see errors without running a build, and whenever the user mentions errors shown in their editor. Give `path` for one file, or nothing for the whole workspace. Each problem is `file:line:column: severity: message`.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "File to check. Omit it to list the problems of the whole workspace."
                    },
                    "severity": {
                        "type": "string",
                        "enum": ["error", "warning", "all"],
                        "description": "Least severe level to list: errors only, errors and warnings (default), or everything."
                    }
                },
                "additionalProperties": false
            }),
            required_permission: PermissionMode::ReadOnly,
        },
        ToolSpec {
            name: "FindSymbol",
            description: "Find where a function, method, class, type, constant or variable is defined and every place it is used, across the whole project (files known to git, build and dependency folders skipped). Faster and more precise than grep for navigating code: give the bare name, e.g. `prix_ttc` or `Panier` (a qualified name like `Panier.total` searches `total`). Returns file:line for each definition and use.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "symbol": {
                        "type": "string",
                        "description": "Name of the symbol to look for."
                    },
                    "include_references": {
                        "type": "boolean",
                        "description": "Also list the places where it is used (default true)."
                    }
                },
                "required": ["symbol"],
                "additionalProperties": false
            }),
            required_permission: PermissionMode::ReadOnly,
        },
        ToolSpec {
            name: "Remember",
            description: "Save one short lesson about this project for future sessions: a build or test quirk, a convention, where something lives, a pitfall, or a correction from the user. It is stored in .claw/lessons.md and shown at the start of later sessions. A lesson stays true after this task and is not obvious from reading the code; do not record what you just did or changed (the code and git history already say it). Never store secrets.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "note": {
                        "type": "string",
                        "description": "One sentence, specific to this project and useful next time."
                    }
                },
                "required": ["note"],
                "additionalProperties": false
            }),
            // Simple note dans .claw/lessons.md : jamais de demande d'autorisation.
            required_permission: PermissionMode::ReadOnly,
        },
        ToolSpec {
            name: "Skill",
            description: "Run an installed skill: returns its instructions with the arguments filled in (and the output of its injected commands), or, for a skill with context: fork, the report of the sub-agent that ran it. Pass the user's arguments in `args`. Skills reserved to the user cannot be run this way.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "skill": { "type": "string" },
                    "args": { "type": "string" }
                },
                "required": ["skill"],
                "additionalProperties": false
            }),
            required_permission: PermissionMode::ReadOnly,
        },
        ToolSpec {
            name: "Agent",
            description: "Run a specialized sub-agent on a self-contained task and return its final report (field `result`). The sub-agent does not see this conversation: put every needed detail in `prompt`. Its tool calls follow the session's permissions; architect, reviewer, auditor, Explore and Plan are read-only. Set run_in_background to start it without waiting; its report is then appended to outputFile. Set isolation to \"worktree\" for agents that edit code in parallel: each works in its own git worktree and branch.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "description": { "type": "string", "description": "Short (3-5 words) label for the task." },
                    "prompt": { "type": "string", "description": "Complete, self-contained instructions for the sub-agent." },
                    "subagent_type": {
                        "type": "string",
                        "description": "architect (design options, diagrams, ADR), reviewer (bugs, security, design of a diff), auditor (whole-project assessment), tester (run tests and fix failures until green), Explore (fast read-only search), Plan, Verification, general-purpose (default), or the name of a custom agent defined in .claw/agents/*.md."
                    },
                    "name": { "type": "string" },
                    "model": { "type": "string", "description": "Optional model override, e.g. an OpenRouter id such as qwen/qwen3-coder." },
                    "run_in_background": { "type": "boolean" },
                    "isolation": {
                        "type": "string",
                        "enum": ["worktree"],
                        "description": "worktree: run the sub-agent in its own git worktree (a separate checkout on a new branch), so several agents can edit code in parallel without conflicts. Its changes come back committed on that branch, to review and merge."
                    }
                },
                "required": ["description", "prompt"],
                "additionalProperties": false
            }),
            // Chaque appel d'outil du sous-agent est contrôlé par les permissions
            // de la session : le lancer ne requiert donc pas de droits particuliers.
            required_permission: PermissionMode::ReadOnly,
        },
        ToolSpec {
            name: "ToolSearch",
            description: "Search for deferred or specialized tools by exact name or keywords.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string" },
                    "max_results": { "type": "integer", "minimum": 1 }
                },
                "required": ["query"],
                "additionalProperties": false
            }),
            required_permission: PermissionMode::ReadOnly,
        },
        ToolSpec {
            name: "NotebookEdit",
            description: "Replace, insert, or delete a cell in a Jupyter notebook.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "notebook_path": { "type": "string" },
                    "cell_id": { "type": "string" },
                    "new_source": { "type": "string" },
                    "cell_type": { "type": "string", "enum": ["code", "markdown"] },
                    "edit_mode": { "type": "string", "enum": ["replace", "insert", "delete"] }
                },
                "required": ["notebook_path"],
                "additionalProperties": false
            }),
            required_permission: PermissionMode::WorkspaceWrite,
        },
        ToolSpec {
            name: "BashOutput",
            description: "Read the new output of a command running in the background (bash_id from its result), and whether it is still running. Use it to follow dev servers, watchers or long builds. With block: true it waits until the command ends (at most `timeout` milliseconds, default 5 minutes) before answering: use that for a build or an install, not for a server, which never ends.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "bash_id": { "type": "string" },
                    "block": { "type": "boolean", "description": "Wait for the command to end before answering" },
                    "timeout": { "type": "integer", "minimum": 1, "description": "Longest wait with block, in milliseconds (default 300000, at most 600000)" }
                },
                "required": ["bash_id"],
                "additionalProperties": false
            }),
            required_permission: PermissionMode::ReadOnly,
        },
        ToolSpec {
            name: "KillShell",
            description: "Stop a command running in the background (and its child processes): one started with run_in_background, or one that Claw moved there.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "shell_id": { "type": "string" }
                },
                "required": ["shell_id"],
                "additionalProperties": false
            }),
            required_permission: PermissionMode::WorkspaceWrite,
        },
        ToolSpec {
            name: "Monitor",
            description: "Watch something in the background and react when it changes, without pausing the conversation: runs `command` (a shell command or small script that prints one line per event: tail a log and filter errors, poll a CI job or PR status in a loop, watch a directory) and feeds each new output line back to you between turns. Ends at timeout_ms (default 300000 = 5 minutes, at most 1800000); you then get one notice and can start it again. Stop it early with KillShell and its id. Prefer it to sleeping or re-running a command to poll.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "command": { "type": "string" },
                    "description": { "type": "string", "description": "What is watched, in a few words" },
                    "timeout_ms": { "type": "integer", "minimum": 1_000, "maximum": 1_800_000 }
                },
                "required": ["command"],
                "additionalProperties": false
            }),
            required_permission: PermissionMode::DangerFullAccess,
        },
        ToolSpec {
            name: "Workflow",
            description: "Run a dynamic workflow: a JavaScript script that orchestrates many sub-agents in the background and returns one consolidated result later, between turns (you get a runId now: do not wait, poll or relaunch). Only use it when the user explicitly asks for a workflow or multi-agent orchestration (the word \"ultracode\", \"use a workflow\", \"fan out agents\") or names a saved workflow; otherwise do the work yourself or with Agent. The script must begin with `export const meta = { name: 'kebab-name', description: '...', phases: [{ title: '...' }] }` as a pure literal, then plain JavaScript where top-level await and return are allowed: `await agent(prompt, { label, phase, schema, model, agentType, isolation: 'worktree' })` runs one sub-agent with tools and resolves to its final report (a string), to a JSON value matching `schema` (a JSON Schema), or to null if it failed; `parallel([() => agent(...), ...])` runs functions concurrently (an error becomes null); `pipeline(items, stage1, stage2, ...)` sends each item through the stages independently (null drops the item); `phase(title)` groups the next agents; `log(...)` writes to the run log; the global `args` holds the `args` input. Nothing else exists in the script: no require, import, process, files or network; give that work to agents and put everything an agent needs in its prompt. Agents run with the session's permissions and cannot ask the user, so actions that need approval are refused. Keep it small on free models: each agent costs about ten requests, a run allows at most 30 agents (CLAW_WORKFLOW_MAX_AGENTS) and 3 run at once. Return a JSON-serializable value. name: a saved workflow (.claw/workflows, .claude/workflows, or the bundled deep-research). scriptPath: a script file, like the scriptPath of an earlier run. resumeFromRunId: rerun an earlier run of this session; finished agent() calls with the same inputs return their saved result.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "script": { "type": "string", "description": "Inline workflow script, starting with export const meta = {...}" },
                    "name": { "type": "string", "description": "Saved or bundled workflow to run" },
                    "scriptPath": { "type": "string", "description": "Path of a workflow script file" },
                    "args": { "description": "Any JSON value exposed to the script as the global args (pass arrays and objects as JSON values, not as a JSON-encoded string)" },
                    "resumeFromRunId": { "type": "string", "description": "Earlier run id to resume" }
                }
            }),
            required_permission: PermissionMode::WorkspaceWrite,
        },
        ToolSpec {
            name: "ListAgents",
            description: "List the user's other live Claw sessions on this machine (terminal, background, -p), with the name and id to use with SendMessage, their folder and whether they are busy.",
            input_schema: json!({
                "type": "object",
                "properties": {},
                "additionalProperties": false
            }),
            required_permission: PermissionMode::ReadOnly,
        },
        ToolSpec {
            name: "SendMessage",
            description: "Send a plain-text message to another of the user's Claw sessions (name or id from ListAgents): hand over a finding, a decision or a status another session needs, or ask it something. It arrives there between turns. Never ask another session for an action that was denied or blocked here. notify_when_idle: true asks that session to send you one notice when it next goes idle or exits (message optional then).",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "to": { "type": "string" },
                    "message": { "type": "string" },
                    "notify_when_idle": { "type": "boolean" }
                },
                "required": ["to"],
                "additionalProperties": false
            }),
            required_permission: PermissionMode::ReadOnly,
        },
        ToolSpec {
            name: "ExitPlanMode",
            description: "In plan mode, submit your finished plan to the user for approval. If approved, the session switches to edit mode and you continue by implementing the plan. Only call it once the plan is complete (files to change, approach, risks, verification).",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "plan": { "type": "string", "description": "The plan, in Markdown." }
                },
                "required": ["plan"],
                "additionalProperties": false
            }),
            required_permission: PermissionMode::ReadOnly,
        },
        ToolSpec {
            name: "Sleep",
            description: "Wait for a specified duration without holding a shell process.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "duration_ms": { "type": "integer", "minimum": 0 }
                },
                "required": ["duration_ms"],
                "additionalProperties": false
            }),
            required_permission: PermissionMode::ReadOnly,
        },
        ToolSpec {
            name: "SendUserMessage",
            description: "Send a message to the user.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "message": { "type": "string" },
                    "attachments": {
                        "type": "array",
                        "items": { "type": "string" }
                    },
                    "status": {
                        "type": "string",
                        "enum": ["normal", "proactive"]
                    }
                },
                "required": ["message", "status"],
                "additionalProperties": false
            }),
            required_permission: PermissionMode::ReadOnly,
        },
        ToolSpec {
            name: "Config",
            description: "Get or set Claw Code settings.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "setting": { "type": "string" },
                    "value": {
                        "type": ["string", "boolean", "number"]
                    }
                },
                "required": ["setting"],
                "additionalProperties": false
            }),
            required_permission: PermissionMode::WorkspaceWrite,
        },
        ToolSpec {
            name: "StructuredOutput",
            description: "Return structured output in the requested format.",
            input_schema: json!({
                "type": "object",
                "additionalProperties": true
            }),
            required_permission: PermissionMode::ReadOnly,
        },
        ToolSpec {
            name: "REPL",
            description: "Execute code in a REPL-like subprocess.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "code": { "type": "string" },
                    "language": { "type": "string" },
                    "timeout_ms": { "type": "integer", "minimum": 1 }
                },
                "required": ["code", "language"],
                "additionalProperties": false
            }),
            required_permission: PermissionMode::DangerFullAccess,
        },
        ToolSpec {
            name: "PowerShell",
            description: "Execute a PowerShell command. Same rules as bash: without `timeout`, a command still running after 5 minutes continues in the background (follow it with BashOutput); with `timeout` (milliseconds), it is stopped at that time; servers and watchers go to the background.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "command": { "type": "string" },
                    "timeout": { "type": "integer", "minimum": 1 },
                    "description": { "type": "string", "description": "What the command does, in 5 to 10 words in the user's language, shown to the user (e.g. \"Run the unit tests\")" },
                    "run_in_background": { "type": "boolean" }
                },
                "required": ["command"],
                "additionalProperties": false
            }),
            required_permission: PermissionMode::DangerFullAccess,
        },
    ]
}

pub fn execute_tool(name: &str, input: &Value) -> Result<String, String> {
    match name {
        "bash" => from_value::<BashCommandInput>(input).and_then(run_bash),
        "read_file" => from_value::<ReadFileInput>(input).and_then(run_read_file),
        "write_file" => from_value::<WriteFileInput>(input).and_then(run_write_file),
        "edit_file" => from_value::<EditFileInput>(input).and_then(run_edit_file),
        "glob_search" => from_value::<GlobSearchInputValue>(input).and_then(run_glob_search),
        "grep_search" => from_value::<GrepSearchInput>(input).and_then(run_grep_search),
        "WebFetch" => from_value::<WebFetchInput>(input).and_then(run_web_fetch),
        "ViewPage" => from_value::<page_view::ViewPageInput>(input)
            .and_then(|input| page_view::view_page(&input))
            .and_then(to_pretty_json),
        "LookAtImage" => {
            from_value::<LookAtImageInput>(input).and_then(|input| run_look_at_image(&input))
        }
        "Icon" => from_value::<icons::IconInput>(input)
            .and_then(|input| icons::icon(&input))
            .and_then(to_pretty_json),
        "Ornament" => from_value::<ornaments::OrnamentInput>(input)
            .and_then(|input| ornaments::ornament(&input))
            .and_then(to_pretty_json),
        "Component" => from_value::<components::ComponentInput>(input)
            .and_then(|input| components::component_tool(&input))
            .and_then(to_pretty_json),
        "Photo" => from_value::<photos::PhotoInput>(input)
            .and_then(|input| photos::photo(&input))
            .and_then(to_pretty_json),
        "Screenshot" => from_value::<screenshot::ScreenshotInput>(input)
            .and_then(|input| screenshot::take(&input))
            .and_then(to_pretty_json),
        "WebSearch" => from_value::<WebSearchInput>(input).and_then(run_web_search),
        "TodoWrite" => from_value::<TodoWriteInput>(input).and_then(run_todo_write),
        "CronCreate" => from_value::<CronCreateInput>(input).and_then(run_cron_create),
        "CronList" => run_cron_list(),
        "CronDelete" => from_value::<CronDeleteInput>(input).and_then(run_cron_delete),
        "ScheduleWakeup" => from_value::<ScheduleWakeupInput>(input).and_then(run_schedule_wakeup),
        "Remember" => from_value::<RememberInput>(input).and_then(run_remember),
        "FindSymbol" => {
            from_value::<FindSymbolInput>(input).and_then(|input| run_find_symbol(&input))
        }
        "EditorDiagnostics" => from_value::<EditorDiagnosticsInput>(input)
            .and_then(|input| run_editor_diagnostics(&input)),
        "AskUserQuestion" => questions::run_ask_user_question(input),
        "Consult" => from_value::<ConsultInput>(input).and_then(run_consult),
        "Skill" => from_value::<SkillInput>(input).and_then(run_skill),
        "Agent" => from_value::<AgentInput>(input).and_then(run_agent),
        "ToolSearch" => from_value::<ToolSearchInput>(input).and_then(run_tool_search),
        "NotebookEdit" => from_value::<NotebookEditInput>(input).and_then(run_notebook_edit),
        "Sleep" => from_value::<SleepInput>(input).and_then(run_sleep),
        "BashOutput" => from_value::<BashOutputInput>(input).and_then(run_bash_output),
        "KillShell" => from_value::<KillShellInput>(input).and_then(run_kill_shell),
        "Monitor" => from_value::<MonitorInput>(input).and_then(run_monitor),
        "Workflow" => from_value::<WorkflowInput>(input).and_then(run_workflow),
        "ListAgents" => run_list_agents(),
        "SendMessage" => from_value::<SendMessageInput>(input).and_then(run_send_message),
        "ExitPlanMode" => from_value::<ExitPlanModeInput>(input).and_then(run_exit_plan_mode),
        "SendUserMessage" | "Brief" => from_value::<BriefInput>(input).and_then(run_brief),
        "Config" => from_value::<ConfigInput>(input).and_then(run_config),
        "StructuredOutput" => {
            from_value::<StructuredOutputInput>(input).and_then(run_structured_output)
        }
        "REPL" => from_value::<ReplInput>(input).and_then(run_repl),
        "PowerShell" => from_value::<PowerShellInput>(input).and_then(run_powershell),
        _ => Err(format!("unsupported tool: {name}")),
    }
}

fn from_value<T: for<'de> Deserialize<'de>>(input: &Value) -> Result<T, String> {
    serde_json::from_value(input.clone()).map_err(|error| {
        let error = error.to_string();
        // Les erreurs de lecture tolérante portent déjà leur conseil.
        if error.starts_with("[Claw]") {
            return error;
        }
        format!(
            "[Claw] The arguments do not fit the tool, so nothing was executed: {error}. Check each argument's type against the tool's schema (numbers and true/false without quotes, lists in square brackets), then send the call again."
        )
    })
}

fn run_bash(input: BashCommandInput) -> Result<String, String> {
    let watch = std::env::current_dir()
        .ok()
        .and_then(|cwd| runtime::settings::CommitWatch::start(&input.command, &cwd));
    let output =
        serde_json::to_string_pretty(&execute_bash(input).map_err(|error| error.to_string())?)
            .map_err(|error| error.to_string())?;
    Ok(
        match watch
            .as_ref()
            .and_then(runtime::settings::CommitWatch::note)
        {
            Some(note) => format!("{output}\n\n{note}"),
            None => output,
        },
    )
}

#[allow(clippy::needless_pass_by_value)]
fn run_read_file(input: ReadFileInput) -> Result<String, String> {
    to_pretty_json(read_file(&input.path, input.offset, input.limit).map_err(io_to_string)?)
}

#[allow(clippy::needless_pass_by_value)]
fn run_write_file(input: WriteFileInput) -> Result<String, String> {
    let output = to_pretty_json(write_file(&input.path, &input.content).map_err(io_to_string)?)?;
    Ok(with_security_note(output, &input.path, &input.content))
}

#[allow(clippy::needless_pass_by_value)]
fn run_edit_file(input: EditFileInput) -> Result<String, String> {
    let output = to_pretty_json(
        edit_file(
            &input.path,
            &input.old_string,
            &input.new_string,
            input.replace_all.unwrap_or(false),
        )
        .map_err(io_to_string)?,
    )?;
    Ok(with_security_note(output, &input.path, &input.new_string))
}

/// Motif risqué dans le texte écrit (injection, `eval`, secret…) : remarque
/// du garde-fou de sécurité après le résultat, gardée telle quelle pour le modèle.
fn with_security_note(output: String, path: &str, written: &str) -> String {
    match runtime::security_guidance::review_write(path, written) {
        Some(note) => format!("{output}\n\n{note}"),
        None => output,
    }
}

#[allow(clippy::needless_pass_by_value)]
fn run_glob_search(input: GlobSearchInputValue) -> Result<String, String> {
    to_pretty_json(glob_search(&input.pattern, input.path.as_deref()).map_err(io_to_string)?)
}

#[allow(clippy::needless_pass_by_value)]
fn run_grep_search(input: GrepSearchInput) -> Result<String, String> {
    to_pretty_json(grep_search(&input).map_err(io_to_string)?)
}

#[allow(clippy::needless_pass_by_value)]
fn run_web_fetch(input: WebFetchInput) -> Result<String, String> {
    to_pretty_json(execute_web_fetch(&input)?)
}

#[allow(clippy::needless_pass_by_value)]
fn run_web_search(input: WebSearchInput) -> Result<String, String> {
    to_pretty_json(execute_web_search(&input)?)
}

#[derive(Debug, Deserialize)]
struct CronCreateInput {
    cron: String,
    prompt: String,
    #[serde(default)]
    recurring: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct CronDeleteInput {
    id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ScheduleWakeupInput {
    #[serde(default)]
    delay_seconds: Option<u64>,
    #[serde(default)]
    prompt: Option<String>,
    #[serde(default)]
    reason: Option<String>,
    #[serde(default)]
    stop: Option<bool>,
}

fn task_json(task: &runtime::cron::ScheduledTask) -> serde_json::Value {
    json!({
        "id": task.id,
        "cron": task.cron,
        "recurring": task.recurring,
        "nextRun": runtime::cron::format_local(task.next_run),
        "prompt": task.prompt,
    })
}

#[allow(clippy::needless_pass_by_value)]
fn run_cron_create(input: CronCreateInput) -> Result<String, String> {
    let recurring = input.recurring.unwrap_or(true);
    let task = runtime::cron::create(&input.cron, &input.prompt, recurring, None)?;
    to_pretty_json(json!({
        "scheduled": task_json(&task),
        "note": format!(
            "Task {} runs {} (local time), between turns. Tell the user its id and next run time.",
            task.id,
            if recurring { "on this schedule for up to 7 days" } else { "once" }
        ),
    }))
}

fn run_cron_list() -> Result<String, String> {
    let tasks: Vec<serde_json::Value> = runtime::cron::list().iter().map(task_json).collect();
    to_pretty_json(json!({ "tasks": tasks }))
}

#[allow(clippy::needless_pass_by_value)]
fn run_cron_delete(input: CronDeleteInput) -> Result<String, String> {
    let task = runtime::cron::delete(&input.id)?;
    to_pretty_json(json!({ "deleted": task_json(&task) }))
}

#[allow(clippy::needless_pass_by_value)]
fn run_schedule_wakeup(input: ScheduleWakeupInput) -> Result<String, String> {
    if input.stop == Some(true) {
        let _ = runtime::cron::stop_self_paced();
        return to_pretty_json(json!({
            "stopped": true,
            "note": "The loop is over: no further iteration will run.",
        }));
    }
    let prompt = input
        .prompt
        .filter(|prompt| !prompt.trim().is_empty())
        .ok_or_else(|| "prompt is required (the /loop prompt to run again)".to_string())?;
    let delay = Duration::from_secs(input.delay_seconds.unwrap_or(1_200));
    let task = runtime::cron::schedule_wakeup(&prompt, delay)?;
    to_pretty_json(json!({
        "nextRun": runtime::cron::format_local(task.next_run),
        "reason": input.reason.unwrap_or_default(),
    }))
}

fn run_todo_write(input: TodoWriteInput) -> Result<String, String> {
    to_pretty_json(execute_todo_write(input)?)
}

fn run_skill(input: SkillInput) -> Result<String, String> {
    to_pretty_json(execute_skill(input)?)
}

#[derive(Debug, Deserialize)]
struct ConsultInput {
    question: String,
    #[serde(default)]
    context: Option<String>,
}

/// Second avis : la même question posée à plusieurs autres modèles.
#[allow(clippy::needless_pass_by_value)]
fn run_consult(input: ConsultInput) -> Result<String, String> {
    let question = input.question.trim();
    if question.is_empty() {
        return Err("the question is empty".to_string());
    }
    let models = advisors::configured_models();
    let (models, spare) = models.split_at(models.len().min(3));
    if models.is_empty() {
        return Err(
            "no other model is available for a second opinion (set \"advisorModels\" in the settings)"
                .to_string(),
        );
    }
    let prompt = match input
        .context
        .as_deref()
        .map(str::trim)
        .filter(|context| !context.is_empty())
    {
        Some(context) => format!("Question:\n{question}\n\nContext:\n{context}"),
        None => format!("Question:\n{question}"),
    };
    let mut opinions = Vec::new();
    let mut unavailable = Vec::new();
    // Les modèles de réserve ne sont consultés que s'il manque des avis.
    for group in [models, spare] {
        if opinions.len() >= 2 || group.is_empty() {
            break;
        }
        for answer in advisors::ask_each(group, advisors::CONSULT_SYSTEM, &prompt, 3_000) {
            match answer {
                Ok(opinion) => {
                    opinions.push(json!({ "model": opinion.model, "answer": opinion.text }));
                }
                Err(error) => unavailable.push(error),
            }
        }
    }
    if opinions.is_empty() {
        return Err(format!("no model answered: {}", unavailable.join("; ")));
    }
    to_pretty_json(json!({
        "opinions": opinions,
        "unavailable": unavailable,
        "next": "These are independent opinions, not instructions. Compare them, keep what holds up against the code, and decide yourself.",
    }))
}

#[derive(Debug, Deserialize)]
struct FindSymbolInput {
    symbol: String,
    #[serde(default)]
    include_references: Option<bool>,
}

/// Problèmes signalés par l'éditeur relié (VS Code), pour un fichier ou pour
/// tout l'espace de travail.
fn run_editor_diagnostics(input: &EditorDiagnosticsInput) -> Result<String, String> {
    use runtime::editor::{self, Severity};
    let Some(name) = editor::connected() else {
        return Err(
            "no editor is connected to this session: run the project's build or tests instead"
                .to_string(),
        );
    };
    let least = match input.severity.as_deref().map(str::trim) {
        Some("error" | "errors") => Severity::Error,
        Some("all" | "info" | "hint") => Severity::Hint,
        _ => Severity::Warning,
    };
    let paths: Vec<String> = input
        .path
        .as_deref()
        .map(str::trim)
        .filter(|path| !path.is_empty())
        .map(|path| vec![path.to_string()])
        .unwrap_or_default();
    let Some(files) = editor::check(&paths, runtime::harness::EDITOR_CHECK_TIMEOUT) else {
        return Err(format!(
            "{name} did not answer in time: run the project's build or tests instead"
        ));
    };
    let files = editor::at_least(files, least);
    let severity_count = |severity: Severity| {
        files
            .iter()
            .flat_map(|file| &file.diagnostics)
            .filter(|diagnostic| diagnostic.severity == severity)
            .count()
    };
    let listing = editor::render(&files, 60);
    to_pretty_json(json!({
        "editor": name,
        "scope": paths.first().map_or("workspace", String::as_str),
        "errors": severity_count(Severity::Error),
        "warnings": severity_count(Severity::Warning),
        "problems": if listing.is_empty() {
            vec!["none reported".to_string()]
        } else {
            listing.lines().map(ToOwned::to_owned).collect::<Vec<_>>()
        },
    }))
}

/// Définitions et usages d'un symbole dans le projet (dossier de travail).
fn run_find_symbol(input: &FindSymbolInput) -> Result<String, String> {
    let symbol = input.symbol.trim();
    if symbol.is_empty() {
        return Err("the symbol is empty".to_string());
    }
    let root = std::env::current_dir().map_err(|error| error.to_string())?;
    let search = runtime::repo_map::find_symbol(&root, symbol);
    let hit = |hit: &runtime::repo_map::SymbolHit| {
        let mut entry = json!({ "file": hit.path, "line": hit.line, "code": hit.code });
        if !hit.kind.is_empty() {
            entry["kind"] = json!(hit.kind);
        }
        entry
    };
    let mut output = json!({
        "symbol": symbol,
        "definitions": search.definitions.iter().map(hit).collect::<Vec<_>>(),
        "files_scanned": search.files_scanned,
    });
    if input.include_references.unwrap_or(true) {
        output["references"] = json!(search.references.iter().map(hit).collect::<Vec<_>>());
        output["references_total"] = json!(search.total_references);
        if search.total_references > search.references.len() {
            output["note"] = json!(format!(
                "only the first {} uses are listed; narrow the search with grep_search on a folder if needed",
                search.references.len()
            ));
        }
    }
    if search.definitions.is_empty() && search.total_references == 0 {
        output["note"] = json!(
            "not found in the project files (check the spelling, or search text with grep_search)"
        );
    }
    to_pretty_json(output)
}

#[derive(Debug, Deserialize)]
struct RememberInput {
    note: String,
}

/// Leçons gardées au plus dans `.claw/lessons.md` (les plus anciennes partent).
const MAX_LESSONS: usize = 80;

fn lessons_path() -> Result<std::path::PathBuf, String> {
    if let Ok(path) = std::env::var("CLAW_LESSONS_FILE") {
        return Ok(std::path::PathBuf::from(path));
    }
    let cwd = std::env::current_dir().map_err(|error| error.to_string())?;
    Ok(cwd.join(".claw").join("lessons.md"))
}

fn looks_like_secret(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    [
        "password", "passwd", "api_key", "apikey", "secret=", "token=", "bearer ",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
        || text.split_whitespace().any(|word| {
            word.len() >= 32
                && word
                    .chars()
                    .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_'))
        })
}

/// Note une leçon sur le projet pour les prochaines sessions.
#[allow(clippy::needless_pass_by_value)]
fn run_remember(input: RememberInput) -> Result<String, String> {
    let note = input.note.split_whitespace().collect::<Vec<_>>().join(" ");
    if note.is_empty() {
        return Err("the note is empty".to_string());
    }
    if note.chars().count() > 300 {
        return Err(
            "the note is too long: keep it to one sentence (300 characters at most)".to_string(),
        );
    }
    if looks_like_secret(&note) {
        return Err(
            "this looks like a secret (key, token or password): it was not saved".to_string(),
        );
    }
    let path = lessons_path()?;
    let existing = std::fs::read_to_string(&path).unwrap_or_default();
    let mut lessons: Vec<String> = existing
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(ToOwned::to_owned)
        .collect();
    let line = format!("- {note}");
    if lessons
        .iter()
        .any(|known| known.eq_ignore_ascii_case(&line))
    {
        return to_pretty_json(
            json!({ "saved": false, "reason": "already known", "total": lessons.len() }),
        );
    }
    lessons.push(line);
    if lessons.len() > MAX_LESSONS {
        let extra = lessons.len() - MAX_LESSONS;
        lessons.drain(..extra);
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    std::fs::write(&path, lessons.join("\n") + "\n").map_err(|error| error.to_string())?;
    to_pretty_json(json!({ "saved": true, "note": note, "total": lessons.len() }))
}

fn run_agent(input: AgentInput) -> Result<String, String> {
    to_pretty_json(execute_agent(input)?)
}

fn run_tool_search(input: ToolSearchInput) -> Result<String, String> {
    to_pretty_json(execute_tool_search(input))
}

fn run_notebook_edit(input: NotebookEditInput) -> Result<String, String> {
    to_pretty_json(execute_notebook_edit(input)?)
}

fn run_sleep(input: SleepInput) -> Result<String, String> {
    to_pretty_json(execute_sleep(input))
}

fn run_brief(input: BriefInput) -> Result<String, String> {
    to_pretty_json(execute_brief(input)?)
}

fn run_config(input: ConfigInput) -> Result<String, String> {
    to_pretty_json(execute_config(input)?)
}

fn run_structured_output(input: StructuredOutputInput) -> Result<String, String> {
    to_pretty_json(execute_structured_output(input))
}

fn run_repl(input: ReplInput) -> Result<String, String> {
    to_pretty_json(execute_repl(input)?)
}

fn run_powershell(input: PowerShellInput) -> Result<String, String> {
    to_pretty_json(execute_powershell(input).map_err(|error| error.to_string())?)
}

fn to_pretty_json<T: serde::Serialize>(value: T) -> Result<String, String> {
    serde_json::to_string_pretty(&value).map_err(|error| error.to_string())
}

#[allow(clippy::needless_pass_by_value)]
fn io_to_string(error: std::io::Error) -> String {
    error.to_string()
}

#[derive(Debug, Deserialize)]
struct ReadFileInput {
    path: String,
    offset: Option<usize>,
    limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct WriteFileInput {
    path: String,
    content: String,
}

#[derive(Debug, Deserialize)]
struct EditFileInput {
    path: String,
    old_string: String,
    new_string: String,
    replace_all: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct EditorDiagnosticsInput {
    path: Option<String>,
    severity: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GlobSearchInputValue {
    pattern: String,
    path: Option<String>,
}

#[derive(Debug, Deserialize)]
struct WebFetchInput {
    url: String,
    #[serde(default)]
    prompt: String,
    #[serde(default, deserialize_with = "lenient_position")]
    offset: Option<usize>,
}

/// Une position donnée par un modèle : un nombre, parfois écrit en texte.
fn lenient_position<'de, D>(deserializer: D) -> Result<Option<usize>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(match Option::<Value>::deserialize(deserializer)? {
        Some(Value::Number(number)) => number.as_u64().and_then(|n| usize::try_from(n).ok()),
        Some(Value::String(text)) => text.trim().parse().ok(),
        _ => None,
    })
}

#[derive(Debug, Deserialize)]
struct WebSearchInput {
    query: String,
    allowed_domains: Option<Vec<String>>,
    blocked_domains: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct TodoWriteInput {
    #[serde(deserialize_with = "tolerant_todos")]
    todos: Vec<TodoItem>,
}

/// Forme d'une tâche, rappelée au modèle quand une tâche reste illisible.
const TODO_SHAPE: &str = "Each task is {\"content\": \"Fix the login test\", \"activeForm\": \"Fixing the login test\", \"status\": \"pending\" | \"in_progress\" | \"completed\"}.";

/// La liste de tâches telle qu'un modèle l'écrit : envoyée comme texte JSON,
/// faite de textes seuls, avec des clés voisines (`task`, `title`, `state`),
/// des statuts d'autres outils (`done`, `in-progress`) ou sans `activeForm`.
fn tolerant_todos<'de, D>(deserializer: D) -> Result<Vec<TodoItem>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Value::deserialize(deserializer)?;
    todos_from_value(&value).map_err(serde::de::Error::custom)
}

fn todos_from_value(value: &Value) -> Result<Vec<TodoItem>, String> {
    let items = match value {
        Value::String(text) => match serde_json::from_str::<Value>(text) {
            Ok(Value::Array(items)) => items,
            _ => vec![value.clone()],
        },
        Value::Array(items) => items.clone(),
        Value::Object(_) => vec![value.clone()],
        _ => {
            return Err(format!(
                "[Claw] TodoWrite was not executed: \"todos\" must be a list of tasks. {TODO_SHAPE}"
            ))
        }
    };
    items
        .iter()
        .enumerate()
        .map(|(index, item)| todo_from_value(index + 1, item))
        .collect()
}

fn todo_from_value(number: usize, item: &Value) -> Result<TodoItem, String> {
    let field = |keys: &[&str]| {
        keys.iter().find_map(|key| {
            item.get(*key)
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|text| !text.is_empty())
                .map(str::to_string)
        })
    };
    let content = match item {
        Value::String(text) if !text.trim().is_empty() => Some(text.trim().to_string()),
        _ => field(&["content", "task", "title", "text", "description", "name"]),
    };
    let Some(content) = content else {
        return Err(format!(
            "[Claw] TodoWrite was not executed: task {number} has no \"content\". {TODO_SHAPE}"
        ));
    };
    let active_form =
        field(&["activeForm", "active_form", "activeform"]).unwrap_or_else(|| content.clone());
    let status = match field(&["status", "state"]) {
        None => TodoStatus::Pending,
        Some(status) => todo_status(&status).ok_or_else(|| {
            format!(
                "[Claw] TodoWrite was not executed: task {number} has the status \"{status}\", which is not one of pending, in_progress, completed. {TODO_SHAPE}"
            )
        })?,
    };
    Ok(TodoItem {
        content,
        active_form,
        status,
    })
}

/// Statut d'une tâche, synonymes d'autres outils compris.
fn todo_status(status: &str) -> Option<TodoStatus> {
    match status
        .trim()
        .to_ascii_lowercase()
        .replace([' ', '-'], "_")
        .as_str()
    {
        "pending" | "todo" | "open" | "not_started" | "notstarted" | "new" | "planned" => {
            Some(TodoStatus::Pending)
        }
        "in_progress" | "inprogress" | "doing" | "active" | "started" | "working" | "current" => {
            Some(TodoStatus::InProgress)
        }
        "completed" | "complete" | "done" | "finished" | "closed" | "resolved" => {
            Some(TodoStatus::Completed)
        }
        _ => None,
    }
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq)]
struct TodoItem {
    content: String,
    #[serde(rename = "activeForm")]
    active_form: String,
    status: TodoStatus,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum TodoStatus {
    Pending,
    InProgress,
    Completed,
}

#[derive(Debug, Deserialize)]
struct SkillInput {
    skill: String,
    args: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AgentInput {
    description: String,
    prompt: String,
    subagent_type: Option<String>,
    name: Option<String>,
    model: Option<String>,
    run_in_background: Option<bool>,
    /// `"worktree"` : le sous-agent travaille dans un worktree git à part.
    #[serde(default)]
    isolation: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ToolSearchInput {
    query: String,
    max_results: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct NotebookEditInput {
    notebook_path: String,
    cell_id: Option<String>,
    new_source: Option<String>,
    cell_type: Option<NotebookCellType>,
    edit_mode: Option<NotebookEditMode>,
}

#[derive(Debug, Deserialize, Serialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum NotebookCellType {
    Code,
    Markdown,
}

#[derive(Debug, Deserialize, Serialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum NotebookEditMode {
    Replace,
    Insert,
    Delete,
}

#[derive(Debug, Deserialize)]
struct SleepInput {
    duration_ms: u64,
}

#[derive(Debug, Deserialize)]
struct BashOutputInput {
    #[serde(alias = "shell_id", alias = "id", alias = "task_id")]
    bash_id: String,
    /// Attendre la fin de la tâche avant de rendre sa sortie.
    #[serde(default)]
    block: Option<bool>,
    /// Attente au plus, en millisecondes (sous 1 000 : en secondes).
    #[serde(default)]
    timeout: Option<u64>,
}

/// Attente par défaut de `BashOutput` avec `block`, et attente au plus : assez
/// longues pour qu'une installation se suive en une ou deux requêtes.
const BASH_OUTPUT_WAIT_MS: u64 = 300_000;
const BASH_OUTPUT_MAX_WAIT_MS: u64 = 600_000;

#[derive(Debug, Deserialize)]
struct KillShellInput {
    #[serde(alias = "bash_id", alias = "id")]
    shell_id: String,
}

#[derive(Debug, Deserialize)]
struct ExitPlanModeInput {
    plan: String,
}

fn background_task_json(task: &runtime::bash_tasks::TaskOutput) -> Value {
    json!({
        "bash_id": task.id,
        "command": task.command,
        "status": if task.running { "running" } else { "exited" },
        // Tant qu'un processus tourne encore (lancé avec `&`), le code de
        // sortie du shell ne dit rien de lui.
        "exitCode": if task.running { None } else { task.exit_code },
        "elapsedSeconds": task.elapsed_secs,
        "newOutput": task.new_output,
        "outputTruncated": task.truncated,
        // Arrêtée exprès : son code de sortie ne dit rien de son travail.
        "killed": task.killed,
    })
}

#[allow(clippy::needless_pass_by_value)]
fn run_bash_output(input: BashOutputInput) -> Result<String, String> {
    let id = input.bash_id.trim();
    // Une seule requête pour suivre une longue installation jusqu'au bout,
    // au lieu d'interroger la tâche en boucle.
    let task = if input.block.unwrap_or(false) {
        let wait = input
            .timeout
            .map_or(BASH_OUTPUT_WAIT_MS, |ms| {
                runtime::effective_timeout_ms(Some(ms))
            })
            .min(BASH_OUTPUT_MAX_WAIT_MS);
        runtime::bash_tasks::wait_for(id, Duration::from_millis(wait))?
    } else {
        runtime::bash_tasks::read_output(id)?
    };
    to_pretty_json(background_task_json(&task))
}

#[derive(Debug, Deserialize)]
struct WorkflowInput {
    #[serde(default)]
    script: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default, rename = "scriptPath")]
    script_path: Option<String>,
    #[serde(default)]
    args: Option<Value>,
    #[serde(default, rename = "resumeFromRunId")]
    resume_from_run_id: Option<String>,
}

#[allow(clippy::needless_pass_by_value)]
fn run_workflow(input: WorkflowInput) -> Result<String, String> {
    let launched = workflows::start(&workflows::StartRequest {
        script: input.script,
        name: input.name,
        script_path: input.script_path,
        args: input.args,
        resume_from: input.resume_from_run_id,
    })?;
    to_pretty_json(launched)
}

/// Agent d'un workflow : conversation complète sans affichage ni demande
/// d'approbation (une action non autorisée d'office est refusée) ; rend son
/// rapport final et le fichier de sa trace.
fn run_workflow_agent(
    description: &str,
    prompt: &str,
    model: Option<&str>,
    subagent_type: Option<&str>,
    isolation: Option<&str>,
) -> Result<(String, String), String> {
    let job = prepare_agent_job(AgentInput {
        description: description.to_string(),
        prompt: prompt.to_string(),
        subagent_type: subagent_type.map(str::to_string),
        name: None,
        model: model.map(str::to_string),
        run_in_background: Some(true),
        isolation: isolation.map(str::to_string),
    })?;
    let transcript = job.manifest.output_file.clone();
    match run_agent_conversation(&job, false) {
        Ok(report) => {
            persist_agent_terminal_state(&job.manifest, "completed", Some(&report), None)?;
            Ok((report, transcript))
        }
        Err(error) => {
            let _ =
                persist_agent_terminal_state(&job.manifest, "failed", None, Some(error.clone()));
            Err(error)
        }
    }
}

#[allow(clippy::needless_pass_by_value)]
#[derive(Debug, Deserialize)]
struct MonitorInput {
    command: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    timeout_ms: Option<u64>,
}

#[allow(clippy::needless_pass_by_value)]
fn run_monitor(input: MonitorInput) -> Result<String, String> {
    let (id, timeout) = monitors::start(
        &input.command,
        input.description.as_deref().unwrap_or_default(),
        input.timeout_ms,
    )?;
    to_pretty_json(json!({
        "monitor_id": id,
        "timeout_seconds": timeout.as_secs(),
        "note": "The command runs in the background. Each new output line reaches you between turns; keep working or end your turn. Stop it with KillShell and this id.",
    }))
}

#[derive(Debug, Deserialize)]
struct SendMessageInput {
    to: String,
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    notify_when_idle: Option<bool>,
}

fn run_list_agents() -> Result<String, String> {
    let me = runtime::peers::current();
    let peers: Vec<serde_json::Value> = runtime::peers::list()
        .into_iter()
        .map(|peer| {
            json!({
                "name": peer.name,
                "id": peer.id,
                "cwd": peer.cwd,
                "kind": peer.kind,
                "status": peer.status,
            })
        })
        .collect();
    to_pretty_json(json!({
        "this_session": me.map(|peer| json!({ "name": peer.name, "id": peer.id })),
        "sessions": peers,
    }))
}

#[allow(clippy::needless_pass_by_value)]
fn run_send_message(input: SendMessageInput) -> Result<String, String> {
    let text = input.message.unwrap_or_default();
    let notify = input.notify_when_idle.unwrap_or(false);
    let peer = runtime::peers::send(&input.to, &text, notify)?;
    to_pretty_json(json!({
        "sent_to": { "name": peer.name, "id": peer.id },
        "note": if notify {
            "Delivered between turns over there; you will get one notice when that session next goes idle or exits."
        } else {
            "Delivered between turns over there. Its reply, if any, will reach you later as a separate message: do not assume or invent what it says, and end your turn if you have nothing else to do."
        },
    }))
}

#[allow(clippy::needless_pass_by_value)]
fn run_kill_shell(input: KillShellInput) -> Result<String, String> {
    if input.shell_id.trim().starts_with("monitor-") {
        let message = monitors::stop(input.shell_id.trim())?;
        return to_pretty_json(json!({ "stopped": message }));
    }
    let task = runtime::bash_tasks::kill(input.shell_id.trim())?;
    to_pretty_json(background_task_json(&task))
}

/// Présente le plan à l'utilisateur (même invite que les autorisations) ; s'il
/// l'approuve, la session passe en mode `workspace-write` pour la suite du tour.
// Même signature que les autres outils de la table d'exécution.
#[allow(clippy::needless_pass_by_value, clippy::unnecessary_wraps)]
fn run_exit_plan_mode(input: ExitPlanModeInput) -> Result<String, String> {
    let current_mode = current_subagent_context()
        .map_or(PermissionMode::Prompt, |context| context.permission_mode);
    if current_mode != PermissionMode::ReadOnly {
        return Ok("Not in plan mode: continue with the task.".to_string());
    }
    let request = PermissionRequest {
        tool_name: "ExitPlanMode".to_string(),
        input: json!({ "plan": input.plan }).to_string(),
        current_mode,
        required_mode: PermissionMode::WorkspaceWrite,
        suggested_rule: None,
    };
    let decision = {
        let mut slot = subagent_prompter_slot()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match slot.as_mut() {
            Some(prompter) => prompter.decide(&request),
            None => PermissionPromptDecision::Deny {
                reason: "no interactive user to approve the plan".to_string(),
            },
        }
    };
    match decision {
        PermissionPromptDecision::Allow | PermissionPromptDecision::AllowAlways { .. } => {
            runtime::request_mode_switch(PermissionMode::WorkspaceWrite);
            if let Some(context) = subagent_context_slot()
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .as_mut()
            {
                context.permission_mode = PermissionMode::WorkspaceWrite;
            }
            Ok("The user approved the plan. The session is now in workspace-write mode: implement the plan now, step by step, and verify the result.".to_string())
        }
        PermissionPromptDecision::Deny { reason } => Ok(format!(
            "The user did not approve the plan ({reason}). Stay in plan mode: ask what to change, or revise the plan."
        )),
    }
}

#[derive(Debug, Deserialize)]
struct BriefInput {
    message: String,
    attachments: Option<Vec<String>>,
    status: BriefStatus,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
enum BriefStatus {
    Normal,
    Proactive,
}

#[derive(Debug, Deserialize)]
struct ConfigInput {
    setting: String,
    value: Option<ConfigValue>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum ConfigValue {
    String(String),
    Bool(bool),
    Number(f64),
}

#[derive(Debug, Deserialize)]
#[serde(transparent)]
struct StructuredOutputInput(BTreeMap<String, Value>);

#[derive(Debug, Deserialize)]
struct ReplInput {
    code: String,
    language: String,
    timeout_ms: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct PowerShellInput {
    command: String,
    timeout: Option<u64>,
    description: Option<String>,
    run_in_background: Option<bool>,
}

#[derive(Debug, Serialize)]
struct WebFetchOutput {
    bytes: usize,
    code: u16,
    #[serde(rename = "codeText")]
    code_text: String,
    result: String,
    #[serde(rename = "durationMs")]
    duration_ms: u128,
    url: String,
}

#[derive(Debug, Serialize)]
struct WebSearchOutput {
    query: String,
    results: Vec<WebSearchResultItem>,
    #[serde(rename = "durationSeconds")]
    duration_seconds: f64,
}

#[derive(Debug, Serialize)]
struct TodoWriteOutput {
    #[serde(rename = "oldTodos")]
    old_todos: Vec<TodoItem>,
    #[serde(rename = "newTodos")]
    new_todos: Vec<TodoItem>,
    #[serde(rename = "verificationNudgeNeeded")]
    verification_nudge_needed: Option<bool>,
}

#[derive(Debug, Serialize)]
struct SkillOutput {
    skill: String,
    path: String,
    args: Option<String>,
    description: Option<String>,
    prompt: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AgentOutput {
    #[serde(rename = "agentId")]
    agent_id: String,
    name: String,
    description: String,
    #[serde(rename = "subagentType")]
    subagent_type: Option<String>,
    model: Option<String>,
    status: String,
    #[serde(rename = "outputFile")]
    output_file: String,
    #[serde(rename = "manifestFile")]
    manifest_file: String,
    #[serde(rename = "createdAt")]
    created_at: String,
    #[serde(rename = "startedAt", skip_serializing_if = "Option::is_none")]
    started_at: Option<String>,
    #[serde(rename = "completedAt", skip_serializing_if = "Option::is_none")]
    completed_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<String>,
}

#[derive(Debug, Clone)]
struct AgentJob {
    manifest: AgentOutput,
    prompt: String,
    /// Sous-agent dans un worktree git à part (processus `claw` séparé).
    isolated: bool,
    /// Consignes du rôle, reprises dans la demande d'un sous-agent isolé.
    role_instructions: Option<String>,
    system_prompt: Vec<String>,
    allowed_tools: BTreeSet<String>,
    permission_mode: PermissionMode,
    permission_rules: PermissionRules,
    hooks: runtime::RuntimeHookConfig,
    fallback_models: Vec<String>,
}

#[derive(Debug, Serialize)]
struct ToolSearchOutput {
    matches: Vec<String>,
    query: String,
    normalized_query: String,
    #[serde(rename = "total_deferred_tools")]
    total_deferred_tools: usize,
    #[serde(rename = "pending_mcp_servers")]
    pending_mcp_servers: Option<Vec<String>>,
}

#[derive(Debug, Serialize)]
struct NotebookEditOutput {
    new_source: String,
    cell_id: Option<String>,
    cell_type: Option<NotebookCellType>,
    language: String,
    edit_mode: String,
    error: Option<String>,
    notebook_path: String,
    original_file: String,
    updated_file: String,
}

#[derive(Debug, Serialize)]
struct SleepOutput {
    duration_ms: u64,
    message: String,
}

#[derive(Debug, Serialize)]
struct BriefOutput {
    message: String,
    attachments: Option<Vec<ResolvedAttachment>>,
    #[serde(rename = "sentAt")]
    sent_at: String,
}

#[derive(Debug, Serialize)]
struct ResolvedAttachment {
    path: String,
    size: u64,
    #[serde(rename = "isImage")]
    is_image: bool,
}

#[derive(Debug, Serialize)]
struct ConfigOutput {
    success: bool,
    operation: Option<String>,
    setting: Option<String>,
    value: Option<Value>,
    #[serde(rename = "previousValue")]
    previous_value: Option<Value>,
    #[serde(rename = "newValue")]
    new_value: Option<Value>,
    error: Option<String>,
}

#[derive(Debug, Serialize)]
struct StructuredOutputResult {
    data: String,
    structured_output: BTreeMap<String, Value>,
}

#[derive(Debug, Serialize)]
struct ReplOutput {
    language: String,
    stdout: String,
    stderr: String,
    #[serde(rename = "exitCode")]
    exit_code: i32,
    #[serde(rename = "durationMs")]
    duration_ms: u128,
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
enum WebSearchResultItem {
    SearchResult {
        tool_use_id: String,
        content: Vec<SearchHit>,
    },
    Commentary(String),
}

#[derive(Debug, Serialize)]
struct SearchHit {
    title: String,
    url: String,
}

fn execute_web_fetch(input: &WebFetchInput) -> Result<WebFetchOutput, String> {
    let started = Instant::now();
    let client = build_http_client()?;
    let request_url = normalize_fetch_url(&input.url)?;
    let response = client
        .get(&request_url)
        .header(reqwest::header::ACCEPT, web_page::ACCEPT)
        .send()
        .map_err(|error| fetch_failure(&request_url, &error))?;

    let status = response.status();
    let final_url = response.url().to_string();
    let code = status.as_u16();
    let code_text = status.canonical_reason().unwrap_or("Unknown").to_string();
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_string();
    let mut body = Vec::new();
    let mut limited = std::io::Read::take(response, web_page::MAX_BYTES);
    std::io::Read::read_to_end(&mut limited, &mut body).map_err(|error| error.to_string())?;
    let fetched = web_page::Fetched {
        url: &final_url,
        status: &format!("{code} {code_text}"),
        content_type: &content_type,
        body: &body,
    };
    let mut result = web_page::render(&fetched, &input.prompt, input.offset);
    // Une page d'erreur se lit comme une page : dit d'abord, sinon « Page not
    // found » ou une page de blocage passe pour le contenu demandé.
    if code >= 400 {
        result = format!("{}\n{result}", error_status_note(code, &code_text));
    }

    Ok(WebFetchOutput {
        bytes: body.len(),
        code,
        code_text,
        result,
        duration_ms: started.elapsed().as_millis(),
        url: final_url,
    })
}

/// Moteur de recherche : API configurée (`SearXNG`, Brave, Tavily) sinon la page
/// HTML de `DuckDuckGo`. Renvoie les résultats et le nom du moteur utilisé.
fn search_hits(client: &Client, query: &str) -> Result<(Vec<SearchHit>, &'static str), String> {
    let env = |name: &str| {
        std::env::var(name)
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
    };
    let hits_from = |items: Option<&Vec<Value>>, title: &str, url: &str| {
        items
            .into_iter()
            .flatten()
            .filter_map(|item| {
                Some(SearchHit {
                    title: item.get(title)?.as_str()?.to_string(),
                    url: item.get(url)?.as_str()?.to_string(),
                })
            })
            .collect::<Vec<_>>()
    };
    if let Some(base) = env("CLAW_SEARXNG_URL") {
        let url = format!("{}/search", base.trim_end_matches('/'));
        let response: Value = client
            .get(url)
            .query(&[("q", query), ("format", "json")])
            .send()
            .and_then(reqwest::blocking::Response::error_for_status)
            .and_then(reqwest::blocking::Response::json)
            .map_err(|error| {
                format!("SearXNG : {error} (activez le format json dans settings.yml)")
            })?;
        return Ok((
            hits_from(
                response.get("results").and_then(Value::as_array),
                "title",
                "url",
            ),
            "SearXNG",
        ));
    }
    if let Some(key) = env("BRAVE_API_KEY") {
        let response: Value = client
            .get("https://api.search.brave.com/res/v1/web/search")
            .query(&[("q", query), ("count", "10")])
            .header("X-Subscription-Token", key)
            .header("Accept", "application/json")
            .send()
            .and_then(reqwest::blocking::Response::error_for_status)
            .and_then(reqwest::blocking::Response::json)
            .map_err(|error| format!("Brave Search : {error}"))?;
        let results = response
            .get("web")
            .and_then(|web| web.get("results"))
            .and_then(Value::as_array);
        return Ok((hits_from(results, "title", "url"), "Brave"));
    }
    if let Some(key) = env("TAVILY_API_KEY") {
        let response: Value = client
            .post("https://api.tavily.com/search")
            .json(&json!({ "api_key": key, "query": query, "max_results": 8 }))
            .send()
            .and_then(reqwest::blocking::Response::error_for_status)
            .and_then(reqwest::blocking::Response::json)
            .map_err(|error| format!("Tavily : {error}"))?;
        return Ok((
            hits_from(
                response.get("results").and_then(Value::as_array),
                "title",
                "url",
            ),
            "Tavily",
        ));
    }
    let search_url = build_search_url(query)?;
    let response = client
        .get(search_url)
        .send()
        .map_err(|error| error.to_string())?;
    let final_url = response.url().clone();
    let html = response.text().map_err(|error| error.to_string())?;
    let mut hits = extract_search_hits(&html);
    if hits.is_empty() && final_url.host_str().is_some() {
        hits = extract_search_hits_from_generic_links(&html);
    }
    Ok((hits, "DuckDuckGo"))
}

fn execute_web_search(input: &WebSearchInput) -> Result<WebSearchOutput, String> {
    let started = Instant::now();
    let client = build_http_client()?;
    let (mut hits, engine) = search_hits(&client, &input.query)?;

    if let Some(allowed) = input.allowed_domains.as_ref() {
        hits.retain(|hit| host_matches_list(&hit.url, allowed));
    }
    if let Some(blocked) = input.blocked_domains.as_ref() {
        hits.retain(|hit| !host_matches_list(&hit.url, blocked));
    }

    dedupe_hits(&mut hits);
    hits.truncate(8);

    let summary = if hits.is_empty() {
        format!(
            "No web search results matched the query {:?} ({engine}).",
            input.query
        )
    } else {
        let rendered_hits = hits
            .iter()
            .map(|hit| format!("- [{}]({})", hit.title, hit.url))
            .collect::<Vec<_>>()
            .join("\n");
        format!(
            "Search results for {:?}. Include a Sources section in the final answer.\n{}",
            input.query, rendered_hits
        )
    };

    Ok(WebSearchOutput {
        query: input.query.clone(),
        results: vec![
            WebSearchResultItem::Commentary(summary),
            WebSearchResultItem::SearchResult {
                tool_use_id: String::from("web_search_1"),
                content: hits,
            },
        ],
        duration_seconds: started.elapsed().as_secs_f64(),
    })
}

fn build_http_client() -> Result<Client, String> {
    Client::builder()
        .timeout(Duration::from_secs(20))
        .redirect(reqwest::redirect::Policy::limited(10))
        .user_agent("claw-rust-tools/0.1")
        .build()
        .map_err(|error| error.to_string())
}

/// Hôte d'une machine locale ou d'un réseau privé : jamais passé en https
/// (un serveur de développement n'en a pas), et cité avec l'état des tâches de
/// fond quand rien n'y répond.
fn is_local_host(url: &reqwest::Url) -> bool {
    let Some(host) = url.host_str() else {
        return false;
    };
    let host = host.trim_start_matches('[').trim_end_matches(']');
    is_local_name(host) || host == "host.docker.internal"
}

/// Nom sans point, adresse IP, ou nom en `.local` / `.localhost`.
fn is_local_name(host: &str) -> bool {
    let last = host.rsplit('.').next().unwrap_or_default();
    !host.contains('.')
        || host.parse::<std::net::IpAddr>().is_ok()
        || last.eq_ignore_ascii_case("local")
        || last.eq_ignore_ascii_case("localhost")
}

/// L'adresse à demander : schéma ajouté quand il manque (`docs.python.org/3/`,
/// `localhost:8000`), `0.0.0.0` (ce qu'affichent Flask, uvicorn ou Vite)
/// lu comme `127.0.0.1`, et https pour un site public seulement.
fn normalize_fetch_url(url: &str) -> Result<String, String> {
    let trimmed = url.trim();
    let with_scheme = if trimmed.contains("://") {
        trimmed.to_string()
    } else {
        let authority = trimmed.split(['/', '?', '#']).next().unwrap_or_default();
        let host = authority
            .rsplit_once(':')
            .map_or(authority, |(host, _)| host);
        let scheme = if is_local_name(host) { "http" } else { "https" };
        format!("{scheme}://{trimmed}")
    };
    let mut parsed = reqwest::Url::parse(&with_scheme).map_err(|error| {
        format!("[Claw] \"{url}\" is not a web address ({error}). Write it in full, such as https://docs.python.org/3/ or http://localhost:8000/.")
    })?;
    if parsed.host_str() == Some("0.0.0.0") {
        let _ = parsed.set_host(Some("127.0.0.1"));
    }
    if parsed.scheme() == "http" && !is_local_host(&parsed) {
        parsed
            .set_scheme("https")
            .map_err(|()| String::from("failed to upgrade URL to https"))?;
    }
    Ok(parsed.to_string())
}

/// Ce que dit `WebFetch` quand la requête n'a pas abouti : la cause, que
/// reqwest garde dans `source()` (« error sending request » seul ne dit rien),
/// et pour une adresse locale l'état des commandes lancées en fond.
fn fetch_failure(url: &str, error: &reqwest::Error) -> String {
    let mut causes = Vec::new();
    let mut source: Option<&dyn std::error::Error> = Some(error);
    while let Some(current) = source {
        causes.push(current.to_string());
        source = current.source();
    }
    let chain = causes.join(" | ").to_lowercase();
    let parsed = reqwest::Url::parse(url).ok();
    let place = parsed
        .as_ref()
        .and_then(|url| {
            Some(format!(
                "{}:{}",
                url.host_str()?,
                url.port_or_known_default()?
            ))
        })
        .unwrap_or_else(|| url.to_string());
    let local = parsed.as_ref().is_some_and(is_local_host);
    if error.is_timeout() || chain.contains("timed out") {
        return format!(
            "[Claw] No answer from {url} within 20 s, so nothing was fetched.{}",
            if local {
                background_servers()
            } else {
                String::new()
            }
        );
    }
    if chain.contains("refused") {
        if local {
            return format!(
                "[Claw] Nothing answers on {place} (connection refused): no server is listening on that port.{} Start the server in the background (bash with run_in_background), read its output with BashOutput, fix what stops it, then fetch again. Do not give this address to the user before it answers.",
                background_servers()
            );
        }
        return format!("[Claw] {place} refused the connection: nothing was fetched.");
    }
    if chain.contains("dns") || chain.contains("lookup") || chain.contains("name or service") {
        return format!(
            "[Claw] The host of {url} was not found (DNS lookup failed). Do not guess addresses: find the right one with WebSearch."
        );
    }
    if chain.contains("certificate") || chain.contains("tls") || chain.contains("ssl") {
        return format!(
            "[Claw] The secure connection to {url} failed ({}). If it is a local server without https, write the address with http://.",
            causes.last().map_or("TLS error", String::as_str)
        );
    }
    format!(
        "[Claw] Could not fetch {url}: {}.",
        causes
            .last()
            .map_or_else(|| error.to_string(), Clone::clone)
    )
}

/// Les commandes lancées en fond, pour un serveur local qui ne répond pas :
/// arrêté sur une erreur, ou jamais lancé.
fn background_servers() -> String {
    let tasks = runtime::bash_tasks::list();
    if tasks.is_empty() {
        return " No command runs in the background in this session.".to_string();
    }
    let described: Vec<String> = tasks
        .iter()
        .map(|task| {
            let state = if task.running {
                format!("running for {} s", task.elapsed_secs)
            } else {
                match task.exit_code {
                    Some(code) => format!("exited with code {code}"),
                    None => "stopped".to_string(),
                }
            };
            format!(
                "{} \"{}\" {state}",
                task.id,
                task.command.chars().take(80).collect::<String>()
            )
        })
        .collect();
    format!(" Background commands: {}.", described.join("; "))
}

/// La phrase mise devant une page d'erreur (code 400 et plus).
fn error_status_note(code: u16, text: &str) -> String {
    let meaning = match code {
        401 | 403 => "access to this address is refused",
        404 | 410 => "this address does not exist",
        429 => "too many requests were sent to this server",
        500..=599 => "the server failed while answering",
        _ => "the request was not accepted",
    };
    format!(
        "[Claw] The server answered {code} {text}: {meaning}. The text below is its error page, not the content you wanted. Check the address, or look for the right page with WebSearch."
    )
}

fn build_search_url(query: &str) -> Result<reqwest::Url, String> {
    if let Ok(base) = std::env::var("CLAW_WEB_SEARCH_BASE_URL") {
        let mut url = reqwest::Url::parse(&base).map_err(|error| error.to_string())?;
        url.query_pairs_mut().append_pair("q", query);
        return Ok(url);
    }

    let mut url = reqwest::Url::parse("https://html.duckduckgo.com/html/")
        .map_err(|error| error.to_string())?;
    url.query_pairs_mut().append_pair("q", query);
    Ok(url)
}

/// Le texte d'un fragment HTML court : le titre d'un résultat de recherche.
fn html_to_text(html: &str) -> String {
    let mut text = String::with_capacity(html.len());
    let mut in_tag = false;
    let mut previous_was_space = false;

    for ch in html.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if in_tag => {}
            '&' => {
                text.push('&');
                previous_was_space = false;
            }
            ch if ch.is_whitespace() => {
                if !previous_was_space {
                    text.push(' ');
                    previous_was_space = true;
                }
            }
            _ => {
                text.push(ch);
                previous_was_space = false;
            }
        }
    }

    collapse_whitespace(&decode_html_entities(&text))
}

fn decode_html_entities(input: &str) -> String {
    input
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ")
}

fn collapse_whitespace(input: &str) -> String {
    input.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn extract_search_hits(html: &str) -> Vec<SearchHit> {
    let mut hits = Vec::new();
    let mut remaining = html;

    while let Some(anchor_start) = remaining.find("result__a") {
        let after_class = &remaining[anchor_start..];
        let Some(href_idx) = after_class.find("href=") else {
            remaining = &after_class[1..];
            continue;
        };
        let href_slice = &after_class[href_idx + 5..];
        let Some((url, rest)) = extract_quoted_value(href_slice) else {
            remaining = &after_class[1..];
            continue;
        };
        let Some(close_tag_idx) = rest.find('>') else {
            remaining = &after_class[1..];
            continue;
        };
        let after_tag = &rest[close_tag_idx + 1..];
        let Some(end_anchor_idx) = after_tag.find("</a>") else {
            remaining = &after_tag[1..];
            continue;
        };
        let title = html_to_text(&after_tag[..end_anchor_idx]);
        if let Some(decoded_url) = decode_duckduckgo_redirect(&url) {
            hits.push(SearchHit {
                title: title.trim().to_string(),
                url: decoded_url,
            });
        }
        remaining = &after_tag[end_anchor_idx + 4..];
    }

    hits
}

fn extract_search_hits_from_generic_links(html: &str) -> Vec<SearchHit> {
    let mut hits = Vec::new();
    let mut remaining = html;

    while let Some(anchor_start) = remaining.find("<a") {
        let after_anchor = &remaining[anchor_start..];
        let Some(href_idx) = after_anchor.find("href=") else {
            remaining = &after_anchor[2..];
            continue;
        };
        let href_slice = &after_anchor[href_idx + 5..];
        let Some((url, rest)) = extract_quoted_value(href_slice) else {
            remaining = &after_anchor[2..];
            continue;
        };
        let Some(close_tag_idx) = rest.find('>') else {
            remaining = &after_anchor[2..];
            continue;
        };
        let after_tag = &rest[close_tag_idx + 1..];
        let Some(end_anchor_idx) = after_tag.find("</a>") else {
            remaining = &after_anchor[2..];
            continue;
        };
        let title = html_to_text(&after_tag[..end_anchor_idx]);
        if title.trim().is_empty() {
            remaining = &after_tag[end_anchor_idx + 4..];
            continue;
        }
        let decoded_url = decode_duckduckgo_redirect(&url).unwrap_or(url);
        if decoded_url.starts_with("http://") || decoded_url.starts_with("https://") {
            hits.push(SearchHit {
                title: title.trim().to_string(),
                url: decoded_url,
            });
        }
        remaining = &after_tag[end_anchor_idx + 4..];
    }

    hits
}

fn extract_quoted_value(input: &str) -> Option<(String, &str)> {
    let quote = input.chars().next()?;
    if quote != '"' && quote != '\'' {
        return None;
    }
    let rest = &input[quote.len_utf8()..];
    let end = rest.find(quote)?;
    Some((rest[..end].to_string(), &rest[end + quote.len_utf8()..]))
}

fn decode_duckduckgo_redirect(url: &str) -> Option<String> {
    if url.starts_with("http://") || url.starts_with("https://") {
        return Some(html_entity_decode_url(url));
    }

    let joined = if url.starts_with("//") {
        format!("https:{url}")
    } else if url.starts_with('/') {
        format!("https://duckduckgo.com{url}")
    } else {
        return None;
    };

    let parsed = reqwest::Url::parse(&joined).ok()?;
    if parsed.path() == "/l/" || parsed.path() == "/l" {
        for (key, value) in parsed.query_pairs() {
            if key == "uddg" {
                return Some(html_entity_decode_url(value.as_ref()));
            }
        }
    }
    Some(joined)
}

fn html_entity_decode_url(url: &str) -> String {
    decode_html_entities(url)
}

fn host_matches_list(url: &str, domains: &[String]) -> bool {
    let Ok(parsed) = reqwest::Url::parse(url) else {
        return false;
    };
    let Some(host) = parsed.host_str() else {
        return false;
    };
    let host = host.to_ascii_lowercase();
    domains.iter().any(|domain| {
        let normalized = normalize_domain_filter(domain);
        !normalized.is_empty() && (host == normalized || host.ends_with(&format!(".{normalized}")))
    })
}

fn normalize_domain_filter(domain: &str) -> String {
    let trimmed = domain.trim();
    let candidate = reqwest::Url::parse(trimmed)
        .ok()
        .and_then(|url| url.host_str().map(str::to_string))
        .unwrap_or_else(|| trimmed.to_string());
    candidate
        .trim()
        .trim_start_matches('.')
        .trim_end_matches('/')
        .to_ascii_lowercase()
}

fn dedupe_hits(hits: &mut Vec<SearchHit>) {
    let mut seen = BTreeSet::new();
    hits.retain(|hit| seen.insert(hit.url.clone()));
}

fn execute_todo_write(input: TodoWriteInput) -> Result<TodoWriteOutput, String> {
    validate_todos(&input.todos)?;
    let store_path = todo_store_path()?;
    // Liste précédente illisible (écriture interrompue, ancienne forme) : on
    // repart d'une liste vide, au lieu de bloquer l'outil pour ce projet.
    let old_todos = std::fs::read_to_string(&store_path)
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .and_then(|value| todos_from_value(&value).ok())
        .unwrap_or_default();

    let all_done = input
        .todos
        .iter()
        .all(|todo| matches!(todo.status, TodoStatus::Completed));
    let persisted = if all_done {
        Vec::new()
    } else {
        input.todos.clone()
    };

    if let Some(parent) = store_path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    // Écrite à côté puis renommée : une écriture interrompue ne laisse pas un
    // fichier vide.
    let staged = store_path.with_extension("json.tmp");
    std::fs::write(
        &staged,
        serde_json::to_string_pretty(&persisted).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    std::fs::rename(&staged, &store_path).map_err(|error| error.to_string())?;

    let verification_nudge_needed = (all_done
        && input.todos.len() >= 3
        && !input
            .todos
            .iter()
            .any(|todo| todo.content.to_lowercase().contains("verif")))
    .then_some(true);

    Ok(TodoWriteOutput {
        old_todos,
        new_todos: input.todos,
        verification_nudge_needed,
    })
}

fn execute_skill(input: SkillInput) -> Result<SkillOutput, String> {
    let skill_path = resolve_skill_path(&input.skill)?;
    let dir_name = skill_path
        .parent()
        .and_then(std::path::Path::file_name)
        .map_or_else(
            || input.skill.clone(),
            |name| name.to_string_lossy().into_owned(),
        );
    let skill = runtime::skills::SkillFile::load(&skill_path, &dir_name)
        .map_err(|error| error.to_string())?;
    // `disable-model-invocation` : réservé à l'utilisateur (`/nom`).
    if skill.disable_model_invocation {
        return Err(format!(
            "[Claw] The skill `{name}` can only be run by the user (disable-model-invocation). Do not carry out its steps another way: tell the user they can run /{name} themselves.",
            name = skill.name
        ));
    }
    let arguments = input.args.clone().unwrap_or_default();
    let rendered = render_skill_for_session(&skill, &arguments)?;
    let description = skill.description.clone();
    if skill.fork {
        // `context: fork` : le skill est la demande d'un sous-agent, dont le
        // rapport revient comme résultat de l'outil.
        let agent_type = skill
            .agent
            .clone()
            .unwrap_or_else(|| "general-purpose".to_string());
        let report = execute_agent(AgentInput {
            description: format!("Skill {}", skill.name),
            prompt: rendered,
            subagent_type: Some(agent_type.clone()),
            name: None,
            model: skill.model.clone(),
            run_in_background: None,
            isolation: None,
        })?;
        return Ok(SkillOutput {
            skill: input.skill,
            path: skill_path.display().to_string(),
            args: input.args,
            description,
            prompt: format!(
                "[Claw] The skill ran in a {agent_type} sub-agent (context: fork). Its report:\n\n{}",
                report.result.unwrap_or_default()
            ),
        });
    }
    Ok(SkillOutput {
        skill: input.skill,
        path: skill_path.display().to_string(),
        args: input.args,
        description,
        prompt: rendered,
    })
}

/// Rend un skill pour la session en cours : `allowed-tools` accordés pour le
/// tour, commandes injectées soumises aux permissions de la session (jamais
/// de demande), coupées pour un skill synchronisé de claude.ai ou par le
/// réglage `disableSkillShellExecution`.
pub fn render_skill_for_session(
    skill: &runtime::skills::SkillFile,
    arguments: &str,
) -> Result<String, String> {
    let cwd = std::env::current_dir().map_err(|error| error.to_string())?;
    let project_dir = runtime::worktree::repo_root(&cwd).unwrap_or_else(|_| cwd.clone());
    runtime::grant_for_turn(&skill.granted_rules(&project_dir));
    let synced = skill
        .path
        .components()
        .any(|component| component.as_os_str() == "synced");
    let disabled_by_setting = runtime::ConfigLoader::default_for(&cwd)
        .load()
        .ok()
        .and_then(|config| {
            config
                .get("disableSkillShellExecution")
                .and_then(runtime::JsonValue::as_bool)
        })
        .unwrap_or(false);
    let context = current_subagent_context();
    let check = |command: &str| {
        let mut policy = context.as_ref().map_or_else(
            || agent_permission_policy_with(PermissionMode::Prompt, PermissionRules::default()),
            |context| agent_permission_policy_with(context.permission_mode, context.rules.clone()),
        );
        match policy.authorize("bash", &json!({ "command": command }).to_string(), None) {
            runtime::PermissionOutcome::Allow => runtime::skills::CommandCheck::Allow,
            runtime::PermissionOutcome::Deny { reason } => {
                runtime::skills::CommandCheck::Deny(reason)
            }
        }
    };
    let session_id = runtime::current_session_id().unwrap_or_default();
    let effort = match api::reasoning_effort() {
        Some(api::ReasoningEffort::Off | api::ReasoningEffort::Low) => "low",
        Some(api::ReasoningEffort::Medium) => "medium",
        Some(api::ReasoningEffort::Max) => "max",
        _ => "high",
    };
    runtime::skills::render(
        skill,
        arguments,
        &runtime::skills::RenderContext {
            session_id: &session_id,
            effort,
            project_dir: &project_dir,
            cwd: &cwd,
            shell_disabled: synced || disabled_by_setting,
            check: &check,
        },
    )
}

fn validate_todos(todos: &[TodoItem]) -> Result<(), String> {
    if todos.is_empty() {
        return Err(String::from(
            "[Claw] The list was not changed: an empty list is not accepted. To close the list, send every task again with \"status\": \"completed\".",
        ));
    }
    // Autorise plusieurs éléments in_progress pour les workflows parallèles
    if todos.iter().any(|todo| todo.content.trim().is_empty()) {
        return Err(String::from("todo content must not be empty"));
    }
    if todos.iter().any(|todo| todo.active_form.trim().is_empty()) {
        return Err(String::from("todo activeForm must not be empty"));
    }
    Ok(())
}

fn todo_store_path() -> Result<std::path::PathBuf, String> {
    if let Ok(path) = std::env::var("CLAW_TODO_STORE") {
        return Ok(std::path::PathBuf::from(path));
    }
    let cwd = std::env::current_dir().map_err(|error| error.to_string())?;
    Ok(todo_store_in(&cwd))
}

/// Liste de suivi du projet, rangée dans `.claw/`. Celle que d'anciennes
/// versions posaient à la racine (`.claw-todos.json`) y est déplacée.
fn todo_store_in(cwd: &std::path::Path) -> std::path::PathBuf {
    let store = cwd.join(".claw").join("todos.json");
    let legacy = cwd.join(".claw-todos.json");
    if legacy.is_file() && !store.exists() {
        if let Some(parent) = store.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::rename(&legacy, &store);
    }
    store
}

fn resolve_skill_path(skill: &str) -> Result<std::path::PathBuf, String> {
    let requested = skill.trim().trim_start_matches('/').trim_start_matches('$');
    if requested.is_empty() {
        return Err(String::from("skill must not be empty"));
    }

    // Projet (du dossier courant vers la racine), puis dossiers de l'utilisateur.
    let mut candidates = Vec::new();
    if let Ok(cwd) = std::env::current_dir() {
        for ancestor in cwd.ancestors() {
            for dir in [".claw", ".claude", ".codex"] {
                candidates.push(ancestor.join(dir).join("skills"));
            }
        }
    }
    if let Ok(config_home) = std::env::var("CLAW_CONFIG_HOME") {
        candidates.push(std::path::PathBuf::from(config_home).join("skills"));
    }
    if let Ok(codex_home) = std::env::var("CODEX_HOME") {
        candidates.push(std::path::PathBuf::from(codex_home).join("skills"));
    }
    if let Some(home) = runtime::home_dir() {
        for dir in [".claw", ".claude", ".codex", ".agents"] {
            candidates.push(home.join(dir).join("skills"));
        }
        candidates.push(home.join(".config").join("opencode").join("skills"));
    }

    for root in candidates {
        let direct = root.join(requested).join("SKILL.md");
        if direct.exists() {
            return Ok(direct);
        }

        if let Ok(entries) = std::fs::read_dir(&root) {
            for entry in entries.flatten() {
                let path = entry.path().join("SKILL.md");
                if !path.exists() {
                    continue;
                }
                if entry
                    .file_name()
                    .to_string_lossy()
                    .eq_ignore_ascii_case(requested)
                {
                    return Ok(path);
                }
            }
        }
    }

    // Compétences de l'application Claude et des plugins de Claude Code.
    if let Ok(cwd) = std::env::current_dir() {
        if let Some(path) = runtime::claude_skills::find_imported_skill(&cwd, requested) {
            return Ok(path);
        }
    }

    Err(format!(
        "unknown skill: {requested} (searched .claw/skills, .claude/skills and .codex/skills in the project and home directory, the skills synced by the Claude app and those of enabled Claude Code plugins)"
    ))
}

const DEFAULT_AGENT_MODEL: &str = "claude-opus-4-6";
const DEFAULT_AGENT_MAX_ITERATIONS: usize = 32;

/// Contexte de la session principale hérité par les sous-agents : modèle par
/// défaut, mode et règles de permission.
#[derive(Debug, Clone)]
pub struct SubagentContext {
    pub model: String,
    pub permission_mode: PermissionMode,
    pub rules: PermissionRules,
    /// Hooks de la session : `PreToolUse`/`PostToolUse` s'appliquent aussi aux
    /// sous-agents, et `SubagentStop` à leur fin.
    pub hooks: runtime::RuntimeHookConfig,
    /// Chaîne de modèles de la session, essayée si le modèle de l'agent échoue.
    pub fallback_models: Vec<String>,
}

/// Demande d'approbation partagée avec les sous-agents synchrones.
pub type SubagentPrompter = Box<dyn PermissionPrompter + Send>;

fn subagent_context_slot() -> &'static Mutex<Option<SubagentContext>> {
    static SLOT: OnceLock<Mutex<Option<SubagentContext>>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(None))
}

fn subagent_prompter_slot() -> &'static Mutex<Option<SubagentPrompter>> {
    static SLOT: OnceLock<Mutex<Option<SubagentPrompter>>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(None))
}

/// À appeler par l'hôte (la CLI) à chaque construction du runtime principal.
/// Sans contexte, les sous-agents tournent en mode `prompt` sans pouvoir
/// demander d'approbation, donc en pratique en lecture seule.
pub fn configure_subagents(context: SubagentContext, prompter: Option<SubagentPrompter>) {
    *subagent_context_slot()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(context);
    *subagent_prompter_slot()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = prompter;
}

fn current_subagent_context() -> Option<SubagentContext> {
    subagent_context_slot()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
}

/// Rôle d'un sous-agent : intégré (architect, reviewer…) ou défini dans
/// `.claw/agents/<nom>.md`.
#[derive(Debug, Clone, PartialEq, Eq)]
struct AgentDefinition {
    subagent_type: String,
    instructions: Option<String>,
    tools: BTreeSet<String>,
    model: Option<String>,
    read_only: bool,
    /// Ajoute les outils MCP connectés (faux pour un agent dont `tools:` est explicite).
    include_mcp: bool,
}

fn execute_agent(input: AgentInput) -> Result<AgentOutput, String> {
    if input.run_in_background.unwrap_or(false) {
        return execute_agent_with_spawn(input, spawn_agent_job);
    }

    let job = prepare_agent_job(input)?;
    match run_agent_conversation(&job, true) {
        Ok(report) => {
            persist_agent_terminal_state(&job.manifest, "completed", Some(&report), None)?;
            let mut manifest = job.manifest;
            manifest.status = String::from("completed");
            manifest.completed_at = Some(iso8601_now());
            manifest.result = Some(report);
            Ok(manifest)
        }
        Err(error) => {
            persist_agent_terminal_state(&job.manifest, "failed", None, Some(error.clone()))?;
            Err(format!(
                "le sous-agent `{}` a échoué : {error}",
                job.manifest
                    .subagent_type
                    .as_deref()
                    .unwrap_or("general-purpose")
            ))
        }
    }
}

fn execute_agent_with_spawn<F>(input: AgentInput, spawn_fn: F) -> Result<AgentOutput, String>
where
    F: FnOnce(AgentJob) -> Result<(), String>,
{
    let job = prepare_agent_job(input)?;
    let manifest = job.manifest.clone();
    if let Err(error) = spawn_fn(job) {
        let error = format!("failed to spawn sub-agent: {error}");
        persist_agent_terminal_state(&manifest, "failed", None, Some(error.clone()))?;
        return Err(error);
    }
    Ok(manifest)
}

/// Valeur de `isolation` : `worktree` ou rien.
fn parse_isolation(value: Option<&str>) -> Result<bool, String> {
    match value.map(str::trim) {
        None | Some("" | "none") => Ok(false),
        Some("worktree") => Ok(true),
        Some(other) => Err(format!(
            "isolation inconnue : {other} (seule valeur possible : worktree)"
        )),
    }
}

fn prepare_agent_job(input: AgentInput) -> Result<AgentJob, String> {
    if input.description.trim().is_empty() {
        return Err(String::from("description must not be empty"));
    }
    if input.prompt.trim().is_empty() {
        return Err(String::from("prompt must not be empty"));
    }
    let isolated = parse_isolation(input.isolation.as_deref())?;

    let agent_id = make_agent_id();
    let output_dir = ready_agent_store_dir()?;
    let output_file = output_dir.join(format!("{agent_id}.md"));
    let manifest_file = output_dir.join(format!("{agent_id}.json"));
    let normalized_subagent_type = normalize_subagent_type(input.subagent_type.as_deref());
    let definition = resolve_agent_definition(&normalized_subagent_type);
    let context = current_subagent_context();
    let model = resolve_agent_model_for(input.model.as_deref(), &definition, context.as_ref());
    let agent_name = input
        .name
        .as_deref()
        .map(slugify_agent_name)
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| slugify_agent_name(&input.description));
    let created_at = iso8601_now();
    let system_prompt = build_agent_system_prompt(&definition)?;
    let mut allowed_tools = definition.tools.clone();
    if definition.include_mcp {
        allowed_tools.extend(
            connected_mcp_tools()
                .into_iter()
                .filter(|tool| tool.read_only || !definition.read_only)
                .map(|tool| tool.name),
        );
    }
    let (permission_mode, permission_rules, hooks, fallback_models) = context.map_or(
        (
            PermissionMode::Prompt,
            PermissionRules::default(),
            runtime::RuntimeHookConfig::default(),
            Vec::new(),
        ),
        |context| {
            (
                context.permission_mode,
                context.rules,
                context.hooks,
                context.fallback_models,
            )
        },
    );
    // Les rôles en lecture seule ne modifient jamais rien, quel que soit le mode.
    let permission_mode = if definition.read_only {
        PermissionMode::ReadOnly
    } else {
        permission_mode
    };

    let output_contents = format!(
        "# Agent Task

- id: {}
- name: {}
- description: {}
- subagent_type: {}
- created_at: {}

## Prompt

{}
",
        agent_id, agent_name, input.description, normalized_subagent_type, created_at, input.prompt
    );
    std::fs::write(&output_file, output_contents).map_err(|error| error.to_string())?;

    let manifest = AgentOutput {
        agent_id,
        name: agent_name,
        description: input.description,
        subagent_type: Some(normalized_subagent_type),
        model: Some(model),
        status: String::from("running"),
        output_file: output_file.display().to_string(),
        manifest_file: manifest_file.display().to_string(),
        created_at: created_at.clone(),
        started_at: Some(created_at),
        completed_at: None,
        error: None,
        result: None,
    };
    write_agent_manifest(&manifest)?;

    Ok(AgentJob {
        manifest,
        prompt: input.prompt,
        isolated,
        role_instructions: definition.instructions.clone(),
        system_prompt,
        allowed_tools,
        permission_mode,
        permission_rules,
        hooks,
        fallback_models,
    })
}

fn spawn_agent_job(job: AgentJob) -> Result<(), String> {
    let thread_name = format!("claw-agent-{}", job.manifest.agent_id);
    std::thread::Builder::new()
        .name(thread_name)
        .spawn(move || {
            let result =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run_agent_job(&job)));
            match result {
                Ok(Ok(())) => {}
                Ok(Err(error)) => {
                    let _ =
                        persist_agent_terminal_state(&job.manifest, "failed", None, Some(error));
                }
                Err(_) => {
                    let _ = persist_agent_terminal_state(
                        &job.manifest,
                        "failed",
                        None,
                        Some(String::from("sub-agent thread panicked")),
                    );
                }
            }
        })
        .map(|_| ())
        .map_err(|error| error.to_string())
}

fn run_agent_job(job: &AgentJob) -> Result<(), String> {
    let report = run_agent_conversation(job, false)?;
    persist_agent_terminal_state(&job.manifest, "completed", Some(report.as_str()), None)
}

/// Exécute la conversation du sous-agent et renvoie son rapport final. En mode
/// interactif (synchrone), il affiche sa progression et peut demander une
/// approbation à l'utilisateur ; en arrière-plan, toute action non autorisée
/// d'office est refusée.
fn run_agent_conversation(job: &AgentJob, interactive: bool) -> Result<String, String> {
    let label = job
        .manifest
        .subagent_type
        .clone()
        .unwrap_or_else(|| String::from("general-purpose"));
    // Hooks `SubagentStart` : leur contexte (`additionalContext`, sortie)
    // accompagne la demande du sous-agent ; un code 2 n'arrête rien.
    let started =
        runtime::HookRunner::new(job.hooks.clone()).run_subagent_start(&label, &job.prompt);
    let context = started.context().join(
        "

",
    );
    let job = &if context.trim().is_empty() {
        job.clone()
    } else {
        AgentJob {
            prompt: format!(
                "{}

[Contexte des hooks SubagentStart]
{context}",
                job.prompt
            ),
            ..job.clone()
        }
    };
    if job.isolated {
        return run_isolated_agent(job, &label, interactive);
    }
    let mut runtime = build_agent_runtime(job, interactive.then(|| label.clone()))?
        .with_max_iterations(DEFAULT_AGENT_MAX_ITERATIONS);
    if interactive {
        eprintln!(
            "  ↳ sous-agent {label} · {} · {}",
            job.manifest.model.as_deref().unwrap_or(DEFAULT_AGENT_MODEL),
            job.manifest.description
        );
    }

    let outcome = if interactive {
        // Plusieurs sous-agents peuvent tourner en parallèle : l'invite partagée
        // n'est verrouillée que le temps de chaque demande d'autorisation.
        let mut labeled = SlotPrompter { label: &label };
        runtime.run_turn(job.prompt.clone(), Some(&mut labeled))
    } else {
        runtime.run_turn(job.prompt.clone(), None)
    };
    // Sans rapport (erreur du fournisseur en route, outils appelés au lieu de
    // conclure), ce que le sous-agent a fait revient au parent, sans requête
    // de plus (E1-12 : jusqu'à 32 requêtes perdues, et la même tâche relancée).
    match outcome {
        Ok(summary) => {
            let report = final_assistant_text(&summary);
            if report.trim().is_empty() {
                if let Some(notes) = agent_work_notes(
                    runtime.session(),
                    &label,
                    "ended without writing its report",
                ) {
                    return Ok(notes);
                }
            }
            Ok(report)
        }
        Err(error) => {
            let error = error.to_string();
            Err(agent_work_notes(
                runtime.session(),
                &label,
                &format!("stopped before writing its report ({error})"),
            )
            .unwrap_or(error))
        }
    }
}

/// Ce qu'un sous-agent a fait avant de s'arrêter sans rapport : son dernier
/// texte, les fichiers lus et modifiés, les recherches et les commandes,
/// puis la consigne de partir de là. `None` s'il n'a rien fait.
fn agent_work_notes(session: &Session, label: &str, reason: &str) -> Option<String> {
    let mut last_text = String::new();
    let (mut read, mut changed, mut searches, mut commands) =
        (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for message in &session.messages {
        for block in &message.blocks {
            match block {
                ContentBlock::Text { text }
                    if message.role == MessageRole::Assistant && !text.trim().is_empty() =>
                {
                    last_text = text.trim().to_string();
                }
                ContentBlock::ToolUse { name, input, .. } => {
                    let input: Value = serde_json::from_str(input).unwrap_or(Value::Null);
                    let field = |key: &str| {
                        input
                            .get(key)
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string()
                    };
                    let list = match name.as_str() {
                        "read_file" => Some((&mut read, field("path"))),
                        "write_file" | "edit_file" => Some((&mut changed, field("path"))),
                        "grep_search" | "glob_search" => Some((&mut searches, field("pattern"))),
                        "bash" | "PowerShell" => {
                            Some((&mut commands, field("command").chars().take(120).collect()))
                        }
                        _ => None,
                    };
                    if let Some((list, item)) = list {
                        if !item.is_empty() && !list.contains(&item) {
                            list.push(item);
                        }
                    }
                }
                _ => {}
            }
        }
    }
    if last_text.is_empty() && read.is_empty() && changed.is_empty() && commands.is_empty() {
        return None;
    }
    let mut notes = String::new();
    if !last_text.is_empty() {
        notes.push_str(&last_text);
        notes.push_str("\n\n");
    }
    for (title, list) in [
        ("Files read", &read),
        ("Files changed", &changed),
        ("Searches", &searches),
        ("Commands run", &commands),
    ] {
        if !list.is_empty() {
            notes.push_str(title);
            notes.push_str(": ");
            notes.push_str(&list.join(", "));
            notes.push('\n');
        }
    }
    notes.push_str("\n[Claw] The sub-agent ");
    notes.push_str(label);
    notes.push(' ');
    notes.push_str(reason);
    notes.push_str(". What it had done so far is above. Do not start the same task again as it is: continue from these notes yourself, or delegate a smaller part.");
    Some(notes)
}

/// Sous-agent isolé (`isolation: "worktree"`) : un processus `claw` à part,
/// lancé dans un worktree git neuf, sur sa propre branche. Ses changements y
/// sont enregistrés dans un commit à la fin ; un worktree resté intact est
/// retiré. Plusieurs sous-agents peuvent ainsi modifier le code en parallèle.
fn run_isolated_agent(job: &AgentJob, label: &str, interactive: bool) -> Result<String, String> {
    let cwd = std::env::current_dir().map_err(|error| error.to_string())?;
    let suffix: String = job
        .manifest
        .agent_id
        .chars()
        .rev()
        .take(6)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    let name = format!("agent-{}-{suffix}", job.manifest.name);
    let worktree = runtime::worktree::create(&cwd, &name, None)?;
    let model = job
        .manifest
        .model
        .clone()
        .unwrap_or_else(|| DEFAULT_AGENT_MODEL.to_string());
    // Dans son worktree, le sous-agent peut modifier les fichiers sans toucher
    // à la copie de travail de l'utilisateur.
    let mode = match job.permission_mode {
        PermissionMode::ReadOnly => "read-only",
        PermissionMode::DangerFullAccess | PermissionMode::Allow => "danger-full-access",
        PermissionMode::DontAsk => "dont-ask",
        _ => "workspace-write",
    };
    if interactive {
        eprintln!(
            "  ↳ sous-agent {label} · {model} · {} · worktree {} ({})",
            job.manifest.description,
            worktree.path.display(),
            worktree.branch
        );
    }
    let exe = std::env::var_os("CLAW_AGENT_EXE")
        .map(PathBuf::from)
        .map_or_else(std::env::current_exe, Ok)
        .map_err(|error| error.to_string())?;
    let prompt = isolated_agent_prompt(job, &worktree);
    let (output, errors, interrupted) = run_isolated_process(
        Command::new(exe)
            .args([
                "--model",
                &model,
                "--permission-mode",
                mode,
                "--output-format",
                "json",
                "-p",
                &prompt,
            ])
            .current_dir(&worktree.path)
            .env("CLAW_NO_SPINNER", "1"),
        interactive,
    )?;
    let report = if interrupted {
        "[Claw] The isolated sub-agent was interrupted by the user.".to_string()
    } else {
        isolated_agent_report(&output).unwrap_or_else(|| {
            let detail = errors
                .lines()
                .last()
                .unwrap_or("no output")
                .trim()
                .to_string();
            format!("[Claw] The isolated sub-agent ended without a report ({detail}).")
        })
    };
    Ok(format!(
        "{report}\n\n{}",
        finish_isolated_worktree(
            &cwd,
            &worktree,
            &job.manifest.name,
            &job.manifest.description
        )
    ))
}

/// Lance le processus et attend sa fin (Échap ou Ctrl+C l'arrêtent quand le
/// sous-agent est au premier plan) ; renvoie ses sorties et s'il a été arrêté.
fn run_isolated_process(
    command: &mut Command,
    interruptible: bool,
) -> Result<(String, String, bool), String> {
    use std::io::Read as _;
    let mut child = command
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|error| format!("lancement du sous-agent isolé impossible : {error}"))?;
    let read_all = |pipe: Option<Box<dyn std::io::Read + Send>>| {
        std::thread::spawn(move || {
            let mut text = String::new();
            if let Some(mut pipe) = pipe {
                let _ = pipe.read_to_string(&mut text);
            }
            text
        })
    };
    let stdout = read_all(
        child
            .stdout
            .take()
            .map(|pipe| Box::new(pipe) as Box<dyn std::io::Read + Send>),
    );
    let stderr = read_all(
        child
            .stderr
            .take()
            .map(|pipe| Box::new(pipe) as Box<dyn std::io::Read + Send>),
    );
    let interrupted = loop {
        match child.try_wait() {
            Ok(Some(_)) => break false,
            Ok(None) if interruptible && runtime::interrupt::interrupt_requested() => {
                let _ = child.kill();
                let _ = child.wait();
                break true;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(100)),
            Err(error) => return Err(error.to_string()),
        }
    };
    Ok((
        stdout.join().unwrap_or_default(),
        stderr.join().unwrap_or_default(),
        interrupted,
    ))
}

/// Demande donnée au processus du sous-agent isolé.
fn isolated_agent_prompt(job: &AgentJob, worktree: &runtime::worktree::Worktree) -> String {
    let role = job
        .role_instructions
        .as_deref()
        .map(|instructions| format!("{instructions}\n\n"))
        .unwrap_or_default();
    format!(
        "{role}{}\n\n[Claw] You are a sub-agent working in an isolated git worktree: a separate checkout of the repository on branch `{}`. Make your changes directly in this directory. Do not commit, push or switch branches: Claw commits your work when you finish. End with a short report: what you changed (files) and how you verified it.",
        job.prompt, worktree.branch
    )
}

/// Réponse finale du sous-agent dans la sortie `--output-format json`.
fn isolated_agent_report(output: &str) -> Option<String> {
    output.lines().rev().find_map(|line| {
        let value: serde_json::Value = serde_json::from_str(line.trim()).ok()?;
        if let Some(message) = value.get("message").and_then(serde_json::Value::as_str) {
            return Some(message.to_string());
        }
        value
            .get("error")
            .and_then(serde_json::Value::as_str)
            .map(|error| format!("[Claw] The isolated sub-agent failed: {error}"))
    })
}

/// Commit des changements du sous-agent, ou retrait du worktree resté
/// intact ; renvoie la note pour l'agent principal.
fn finish_isolated_worktree(
    repo: &Path,
    worktree: &runtime::worktree::Worktree,
    name: &str,
    description: &str,
) -> String {
    let message = format!("Sous-agent {name} : {description}");
    let committed = runtime::worktree::commit_all(&worktree.path, &message);
    if !runtime::worktree::has_changes(&worktree.path, &worktree.base) {
        let target = worktree.path.display().to_string();
        return match runtime::worktree::remove(repo, &target, true) {
            Ok(_) => "[Claw worktree] The sub-agent changed no file; its worktree was removed."
                .to_string(),
            Err(error) => format!(
                "[Claw worktree] The sub-agent changed no file (worktree {target} could not be removed: {error})."
            ),
        };
    }
    let summary = runtime::worktree::change_summary(&worktree.path, &worktree.base);
    let warning = committed
        .err()
        .map(|error| {
            format!("\nThe changes could not be committed ({error}): they are uncommitted in the worktree.")
        })
        .unwrap_or_default();
    format!(
        "[Claw worktree] The sub-agent worked in an isolated git worktree. Its changes are committed on branch `{branch}` (worktree {path}), NOT in the main working tree yet:\n{summary}{warning}\nReview them with `git diff HEAD...{branch}`, bring them in with `git merge {branch}` (or `git cherry-pick`), then remove the worktree with `git worktree remove {path}`.",
        branch = worktree.branch,
        path = worktree.path.display(),
    )
}

/// Demande d'approbation d'un sous-agent, transmise à l'invite de la session
/// (verrouillée le temps de la demande) avec le nom du sous-agent.
struct SlotPrompter<'a> {
    label: &'a str,
}

impl PermissionPrompter for SlotPrompter<'_> {
    fn decide(&mut self, request: &PermissionRequest) -> PermissionPromptDecision {
        let mut slot = subagent_prompter_slot()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match slot.as_mut() {
            Some(prompter) => {
                eprintln!("\n[sous-agent {}]", self.label);
                prompter.decide(request)
            }
            None => PermissionPromptDecision::Deny {
                reason: "no interactive prompt available for this sub-agent".to_string(),
            },
        }
    }
}

/// Outils sans effet de bord, exécutables en parallèle.
pub const PARALLEL_SAFE_TOOLS: &[&str] = &[
    "Agent",
    "read_file",
    "glob_search",
    "grep_search",
    "FindSymbol",
    "EditorDiagnostics",
    "WebFetch",
    "ViewPage",
    "Icon",
    "Ornament",
    "Component",
    "WebSearch",
    "BashOutput",
];

fn build_agent_runtime(
    job: &AgentJob,
    progress_label: Option<String>,
) -> Result<ConversationRuntime<ProviderRuntimeClient, SubagentToolExecutor>, String> {
    let model = job
        .manifest
        .model
        .clone()
        .unwrap_or_else(|| DEFAULT_AGENT_MODEL.to_string());
    let allowed_tools = job.allowed_tools.clone();
    let auto_compaction = runtime::AutoCompactionConfig {
        threshold_tokens: api::auto_compaction_threshold(&model),
        preserve_recent_messages: 6,
    };
    let api_client =
        ProviderRuntimeClient::new(&model, &job.fallback_models, allowed_tools.clone())?;
    let tool_executor =
        SubagentToolExecutor::new(allowed_tools).with_progress_label(progress_label);
    Ok(ConversationRuntime::new_with_features(
        Session::new(),
        api_client,
        tool_executor,
        agent_permission_policy_with(job.permission_mode, job.permission_rules.clone()),
        job.system_prompt.clone(),
        runtime::RuntimeFeatureConfig::default().with_hooks(job.hooks.clone()),
    )
    .with_auto_compaction(Some(auto_compaction))
    .as_subagent())
}

fn build_agent_system_prompt(definition: &AgentDefinition) -> Result<Vec<String>, String> {
    let cwd = std::env::current_dir().map_err(|error| error.to_string())?;
    let mut prompt = load_system_prompt(cwd, current_date_iso(), std::env::consts::OS, "unknown")
        .map_err(|error| error.to_string())?;
    if let Some(instructions) = &definition.instructions {
        prompt.push(format!("# Role\n{instructions}"));
    }
    let mut guidance = format!(
        "You are a sub-agent of type `{}` working for a parent agent. Work only on the delegated task, use only the tools available to you, and do not ask the user questions. Finish with a concise, self-contained report: it is the only output the parent agent will see.",
        definition.subagent_type
    );
    if definition.read_only {
        guidance.push_str(
            " You are read-only: do not try to modify files or run commands with side effects.",
        );
    }
    prompt.push(guidance);
    Ok(prompt)
}

/// Modèle du sous-agent, par ordre de priorité : paramètre `model` de l'appel,
/// définition de l'agent, config `agents.<type>.model`, modèle de la session.
fn resolve_agent_model_for(
    requested: Option<&str>,
    definition: &AgentDefinition,
    context: Option<&SubagentContext>,
) -> String {
    let non_empty = |value: &str| {
        let trimmed = value.trim();
        (!trimmed.is_empty()).then(|| trimmed.to_string())
    };
    requested
        .and_then(usable_agent_model)
        .or_else(|| definition.model.as_deref().and_then(usable_agent_model))
        .or_else(|| configured_agent_model(&definition.subagent_type))
        .or_else(|| context.and_then(|context| non_empty(&context.model)))
        .map_or_else(
            || DEFAULT_AGENT_MODEL.to_string(),
            |model| resolve_model_alias(&model),
        )
}

fn configured_agent_model(subagent_type: &str) -> Option<String> {
    let cwd = std::env::current_dir().ok()?;
    let config = ConfigLoader::default_for(&cwd).load().ok()?;
    config.agent_model(subagent_type).map(ToOwned::to_owned)
}

const ARCHITECT_INSTRUCTIONS: &str = "You are a senior software architect. Before proposing anything, analyze the requirements and the existing codebase: structure, module boundaries, dependencies, data flow and conventions. Deliver: (1) a short summary of the context, requirements and constraints, including assumptions you made; (2) two or three architecture options with their trade-offs (complexity, performance, scalability, security, operability, cost, migration effort); (3) a clear recommendation and its rationale; (4) Mermaid diagrams: a C4 container or component view and a sequence diagram of the key flow; (5) an implementation roadmap as ordered, independently verifiable steps; (6) an ADR draft with Context, Decision, Alternatives considered and Consequences.";

const REVIEWER_INSTRUCTIONS: &str = "You are a meticulous senior code reviewer. Determine the scope: the diff or files named in the task, otherwise the output of `git diff` and `git diff --staged`. Read the changed code and enough surrounding code to understand it. Look for, in priority order: correctness bugs and regressions, security issues (injection, secrets, authorization, unsafe input handling), concurrency and resource leaks, missing error handling, performance problems, then maintainability (duplication, naming, cohesion, missing tests). Report only real issues you verified in the code, not formatting nitpicks. For each finding give: severity (critical, high, medium or low), file:line, the problem, a concrete failure scenario and a suggested fix. End with an overall verdict.";

const AUDITOR_INSTRUCTIONS: &str = "You audit an existing project end to end. Map the repository: languages, frameworks, build and test tooling, entry points, main modules and their dependencies. Assess the architecture and layering, code quality hotspots (largest or most complex files, duplication), the test suite (run only read-only commands; if the tests cannot be run, say so), dependency health, security risks, documentation and developer experience. Deliver a report with: an overview, a Mermaid diagram of the main modules and their dependencies, strengths, risks and technical debt prioritized by impact and effort, and a recommended action plan.";

const TESTER_INSTRUCTIONS: &str = "You make the test suite pass. Use the test command given in the task, otherwise find it in CLAW.md, the README or the project manifests. Run it, read the failures carefully, locate the root cause in the code, fix the code, and run the tests again. Change a test only if it is demonstrably wrong, and say so. Repeat until the suite passes or after five unsuccessful attempts. Add tests for the behavior you fix when that is appropriate. Finish with: the commands run, the final result, each change made and why, and anything left unresolved.";

/// Instructions d'un rôle intégré (architect, reviewer, auditor, tester), réutilisées
/// par les commandes `/archi`, `/review`, `/audit` et `/fix`.
#[must_use]
pub fn builtin_agent_instructions(subagent_type: &str) -> Option<&'static str> {
    match subagent_type {
        "architect" => Some(ARCHITECT_INSTRUCTIONS),
        "reviewer" => Some(REVIEWER_INSTRUCTIONS),
        "auditor" => Some(AUDITOR_INSTRUCTIONS),
        "tester" => Some(TESTER_INSTRUCTIONS),
        _ => None,
    }
}

fn is_read_only_subagent(subagent_type: &str) -> bool {
    matches!(
        subagent_type,
        "Explore" | "Plan" | "claw-guide" | "architect" | "reviewer" | "auditor"
    )
}

fn resolve_agent_definition(subagent_type: &str) -> AgentDefinition {
    let cwd = std::env::current_dir().unwrap_or_default();
    if let Some(custom) = find_custom_agent(&custom_agent_dirs(&cwd), subagent_type) {
        return custom;
    }
    AgentDefinition {
        subagent_type: subagent_type.to_string(),
        instructions: builtin_agent_instructions(subagent_type).map(ToOwned::to_owned),
        tools: allowed_tools_for_subagent(subagent_type),
        model: None,
        read_only: is_read_only_subagent(subagent_type),
        include_mcp: true,
    }
}

/// `.claw/agents` puis `.claude/agents` du projet (et de ses parents), puis
/// ceux du dossier personnel.
fn custom_agent_dirs(cwd: &Path) -> Vec<PathBuf> {
    let mut dirs = cwd
        .ancestors()
        .flat_map(|ancestor| {
            [
                ancestor.join(".claw").join("agents"),
                ancestor.join(".claude").join("agents"),
            ]
        })
        .collect::<Vec<_>>();
    if let Some(home) = runtime::home_dir() {
        dirs.push(home.join(".claw").join("agents"));
        dirs.push(home.join(".claude").join("agents"));
    }
    let mut unique = Vec::new();
    for dir in dirs {
        if !unique.contains(&dir) {
            unique.push(dir);
        }
    }
    unique
}

/// Alias Claude des agents de Claude Code (`sonnet`, `opus`, `haiku`) : sans
/// identifiants Anthropic, l'agent prend le modèle de la session.
fn usable_agent_model(model: &str) -> Option<String> {
    let trimmed = model.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("inherit") {
        return None;
    }
    let lower = trimmed.to_ascii_lowercase();
    let claude_alias =
        matches!(lower.as_str(), "sonnet" | "opus" | "haiku") || lower.starts_with("claude-");
    if claude_alias && !api::has_anthropic_credentials() {
        return None;
    }
    Some(trimmed.to_string())
}

/// Cherche un agent Markdown (en-tête `name`, `description`, `tools`, `model`,
/// corps = instructions) dont le nom correspond, sans tenir compte de la casse.
fn find_custom_agent(dirs: &[PathBuf], subagent_type: &str) -> Option<AgentDefinition> {
    for dir in dirs {
        let Ok(entries) = std::fs::read_dir(dir) else {
            continue;
        };
        let mut paths = entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.extension()
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("md"))
            })
            .collect::<Vec<_>>();
        paths.sort();
        for path in paths {
            let Ok(contents) = std::fs::read_to_string(&path) else {
                continue;
            };
            let fallback = path
                .file_stem()
                .map(|stem| stem.to_string_lossy().into_owned())
                .unwrap_or_default();
            let definition = parse_agent_markdown(&contents, &fallback);
            if definition.subagent_type.eq_ignore_ascii_case(subagent_type) {
                return Some(definition);
            }
        }
    }
    None
}

fn parse_agent_markdown(contents: &str, fallback_name: &str) -> AgentDefinition {
    let mut name = None;
    let mut tools = None;
    let mut model = None;
    let mut body = contents;

    if let Some(rest) = contents
        .strip_prefix("---\n")
        .or_else(|| contents.strip_prefix("---\r\n"))
    {
        if let Some(end) = rest.find("\n---") {
            let header = &rest[..end];
            body = rest[end + 4..].trim_start_matches(['\r', '\n']);
            for line in header.lines() {
                let Some((key, value)) = line.split_once(':') else {
                    continue;
                };
                let value = value.trim().trim_matches(['"', '\'']).trim();
                if value.is_empty() {
                    continue;
                }
                match key.trim() {
                    "name" => name = Some(value.to_string()),
                    "model" if value != "inherit" => model = Some(value.to_string()),
                    "tools" => {
                        tools = Some(
                            value
                                .trim_matches(['[', ']'])
                                .split(',')
                                .map(|tool| {
                                    canonical_agent_tool_name(tool.trim().trim_matches(['"', '\'']))
                                })
                                .filter(|tool| !tool.is_empty() && tool != "Agent")
                                .collect::<BTreeSet<_>>(),
                        );
                    }
                    _ => {}
                }
            }
        }
    }

    let include_mcp = tools.is_none();
    let tools = tools.unwrap_or_else(|| allowed_tools_for_subagent("general-purpose"));
    let read_only = !tools.iter().any(|tool| {
        matches!(
            tool.as_str(),
            "write_file" | "edit_file" | "NotebookEdit" | "bash" | "PowerShell" | "REPL" | "Config"
        )
    });
    let body = body.trim();
    AgentDefinition {
        subagent_type: name.unwrap_or_else(|| fallback_name.to_string()),
        instructions: (!body.is_empty()).then(|| body.to_string()),
        tools,
        model,
        read_only,
        include_mcp,
    }
}

/// Accepte les noms d'outils de Claude Code (Read, Edit, Bash…) dans `tools:`.
fn canonical_agent_tool_name(name: &str) -> String {
    match name {
        "Read" => "read_file",
        "Write" => "write_file",
        "Edit" | "MultiEdit" => "edit_file",
        "Bash" => "bash",
        "Glob" => "glob_search",
        "Grep" => "grep_search",
        other => other,
    }
    .to_string()
}

// Rôles en lecture seule : le shell sert aux commandes sans effet
// (git log, git diff…), le mode read-only du sous-agent bloque le reste.
const READ_ONLY_ROLE_TOOLS: &[&str] = &[
    "bash",
    "read_file",
    "glob_search",
    "grep_search",
    "FindSymbol",
    "WebFetch",
    "ViewPage",
    "Screenshot",
    "Icon",
    "Ornament",
    "Component",
    "WebSearch",
    "ToolSearch",
    "Skill",
    "TodoWrite",
    "StructuredOutput",
    "PowerShell",
];

const TESTER_TOOLS: &[&str] = &[
    "bash",
    "read_file",
    "write_file",
    "edit_file",
    "glob_search",
    "grep_search",
    "FindSymbol",
    "ViewPage",
    "Screenshot",
    "TodoWrite",
    "ToolSearch",
    "NotebookEdit",
    "StructuredOutput",
    "PowerShell",
];

fn allowed_tools_for_subagent(subagent_type: &str) -> BTreeSet<String> {
    let tools = match subagent_type {
        "architect" | "reviewer" | "auditor" => READ_ONLY_ROLE_TOOLS.to_vec(),
        "tester" => TESTER_TOOLS.to_vec(),
        "Explore" => vec![
            "read_file",
            "glob_search",
            "grep_search",
            "FindSymbol",
            "WebFetch",
            "WebSearch",
            "ToolSearch",
            "Skill",
            "StructuredOutput",
        ],
        "Plan" => vec![
            "read_file",
            "glob_search",
            "grep_search",
            "FindSymbol",
            "WebFetch",
            "WebSearch",
            "ToolSearch",
            "Skill",
            "TodoWrite",
            "StructuredOutput",
            "SendUserMessage",
        ],
        "Verification" => vec![
            "bash",
            "read_file",
            "glob_search",
            "grep_search",
            "FindSymbol",
            "WebFetch",
            "ViewPage",
            "Screenshot",
            "WebSearch",
            "ToolSearch",
            "TodoWrite",
            "StructuredOutput",
            "SendUserMessage",
            "PowerShell",
        ],
        "claw-guide" => vec![
            "read_file",
            "glob_search",
            "grep_search",
            "FindSymbol",
            "WebFetch",
            "WebSearch",
            "ToolSearch",
            "Skill",
            "StructuredOutput",
            "SendUserMessage",
        ],
        "statusline-setup" => vec![
            "bash",
            "read_file",
            "write_file",
            "edit_file",
            "glob_search",
            "grep_search",
            "ToolSearch",
        ],
        _ => vec![
            "bash",
            "read_file",
            "write_file",
            "edit_file",
            "glob_search",
            "grep_search",
            "WebFetch",
            "ViewPage",
            "Screenshot",
            "WebSearch",
            "TodoWrite",
            "Skill",
            "ToolSearch",
            "NotebookEdit",
            "Sleep",
            "SendUserMessage",
            "Config",
            "StructuredOutput",
            "REPL",
            "PowerShell",
        ],
    };
    tools.into_iter().map(str::to_string).collect()
}

#[cfg(test)]
fn agent_permission_policy() -> PermissionPolicy {
    agent_permission_policy_with(PermissionMode::DangerFullAccess, PermissionRules::default())
}

fn agent_permission_policy_with(mode: PermissionMode, rules: PermissionRules) -> PermissionPolicy {
    let policy = mvp_tool_specs()
        .into_iter()
        .fold(PermissionPolicy::new(mode), |policy, spec| {
            policy.with_tool_requirement(spec.name, spec.required_permission)
        });
    connected_mcp_tools()
        .into_iter()
        .fold(policy, |policy, tool| {
            let required = tool.required_permission();
            policy.with_tool_requirement(tool.name, required)
        })
        .with_rules(rules)
}

fn write_agent_manifest(manifest: &AgentOutput) -> Result<(), String> {
    std::fs::write(
        &manifest.manifest_file,
        serde_json::to_string_pretty(manifest).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())
}

fn persist_agent_terminal_state(
    manifest: &AgentOutput,
    status: &str,
    result: Option<&str>,
    error: Option<String>,
) -> Result<(), String> {
    append_agent_output(
        &manifest.output_file,
        &format_agent_terminal_output(status, result, error.as_deref()),
    )?;
    let mut next_manifest = manifest.clone();
    next_manifest.status = status.to_string();
    next_manifest.completed_at = Some(iso8601_now());
    next_manifest.error = error;
    write_agent_manifest(&next_manifest)
}

fn append_agent_output(path: &str, suffix: &str) -> Result<(), String> {
    use std::io::Write as _;

    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(path)
        .map_err(|error| error.to_string())?;
    file.write_all(suffix.as_bytes())
        .map_err(|error| error.to_string())
}

fn format_agent_terminal_output(status: &str, result: Option<&str>, error: Option<&str>) -> String {
    let mut sections = vec![format!("\n## Result\n\n- status: {status}\n")];
    if let Some(result) = result.filter(|value| !value.trim().is_empty()) {
        sections.push(format!("\n### Final response\n\n{}\n", result.trim()));
    }
    if let Some(error) = error.filter(|value| !value.trim().is_empty()) {
        sections.push(format!("\n### Error\n\n{}\n", error.trim()));
    }
    sections.join("")
}

struct ProviderRuntimeClient {
    runtime: tokio::runtime::Runtime,
    chain: api::ModelChain,
    allowed_tools: BTreeSet<String>,
    /// La dernière réponse s'est arrêtée sur la limite de sortie au milieu de
    /// son texte : le runtime en demandera la suite.
    reply_cut: bool,
}

impl ProviderRuntimeClient {
    fn new(
        model: &str,
        fallback_models: &[String],
        allowed_tools: BTreeSet<String>,
    ) -> Result<Self, String> {
        let model = resolve_model_alias(model);
        let mut models = vec![model.clone()];
        models.extend(fallback_models.iter().cloned());
        if models.len() == 1 {
            ProviderClient::from_model(&model).map_err(|error| error.to_string())?;
        }
        Ok(Self {
            runtime: tokio::runtime::Runtime::new().map_err(|error| error.to_string())?,
            chain: api::ModelChain::new(models, Box::new(ProviderClient::from_model)),
            allowed_tools,
            reply_cut: false,
        })
    }
}

fn notify_subagent_fallback(text: &str) {
    eprintln!("  ↳ ▲ {text}");
}

type RequestObserver = Box<dyn Fn(&str) + Send + Sync>;

fn request_observer() -> &'static Mutex<Option<RequestObserver>> {
    static SLOT: OnceLock<Mutex<Option<RequestObserver>>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(None))
}

/// `observer` reçoit le modèle de chaque requête envoyée par un sous-agent ou
/// un modèle consulté : l'application y décompte crédits et quota gratuit.
/// Envoi d'un corps JSON (télémétrie OTLP) ; erreur hors réponse 2xx.
/// Requête POST bloquante (dictée vocale) : statut et corps de la réponse.
pub fn http_post(
    url: &str,
    headers: &[(String, String)],
    content_type: &str,
    body: Vec<u8>,
    timeout: Duration,
) -> Result<(u16, String), String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(timeout)
        .build()
        .map_err(|error| error.to_string())?;
    let mut request = client
        .post(url)
        .header("Content-Type", content_type)
        .body(body);
    for (name, value) in headers {
        request = request.header(name.as_str(), value.as_str());
    }
    let response = request.send().map_err(|error| error.to_string())?;
    let status = response.status().as_u16();
    let text = response.text().map_err(|error| error.to_string())?;
    Ok((status, text))
}

/// Envoi OTLP de la télémétrie : HTTP (JSON ou protobuf), ou gRPC sur
/// HTTP/2 (sans TLS pour une adresse `http://`, comme le port 4317 d'un
/// collecteur local).
pub fn otlp_export(export: &runtime::telemetry::Export<'_>) -> Result<(), String> {
    let mut builder = reqwest::blocking::Client::builder().timeout(Duration::from_secs(10));
    if export.grpc && export.url.starts_with("http://") {
        builder = builder.http2_prior_knowledge();
    }
    let client = builder.build().map_err(|error| error.to_string())?;
    let body = if export.grpc {
        // Message gRPC : non compressé (0), longueur sur 4 octets, puis le protobuf.
        let length = u32::try_from(export.body.len()).map_err(|error| error.to_string())?;
        let mut framed = Vec::with_capacity(export.body.len() + 5);
        framed.push(0);
        framed.extend_from_slice(&length.to_be_bytes());
        framed.extend_from_slice(export.body);
        framed
    } else {
        export.body.to_vec()
    };
    let mut request = client
        .post(export.url)
        .header("Content-Type", export.content_type)
        .body(body);
    if export.grpc {
        request = request.header("te", "trailers");
    }
    for (name, value) in export.headers {
        request = request.header(name.as_str(), value.as_str());
    }
    let response = request.send().map_err(|error| error.to_string())?;
    if !response.status().is_success() {
        return Err(format!("HTTP {}", response.status()));
    }
    // Une erreur gRPC immédiate arrive dans les en-têtes (« trailers-only »).
    let header = |name: &str| {
        response
            .headers()
            .get(name)
            .and_then(|value| value.to_str().ok())
            .map(str::to_string)
    };
    if let Some(status) = header("grpc-status").filter(|status| status != "0") {
        return Err(format!(
            "gRPC {status} {}",
            header("grpc-message").unwrap_or_default()
        ));
    }
    response.bytes().map_err(|error| error.to_string())?;
    Ok(())
}

pub fn set_request_observer(observer: impl Fn(&str) + Send + Sync + 'static) {
    *request_observer()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(Box::new(observer));
}

pub(crate) fn note_model_request(model: &str) {
    if let Some(observer) = request_observer()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .as_ref()
    {
        observer(model);
    }
}

impl ApiClient for ProviderRuntimeClient {
    fn summarize(&mut self, system_prompt: &str, transcript: &str) -> Option<String> {
        let build = |model: &str| api::summary_request(model, system_prompt, transcript);
        let mut notify = notify_subagent_fallback;
        self.runtime
            .block_on(runtime::interrupt::interruptible(
                self.chain.send(&build, &mut notify),
            ))
            .ok()
            .map(|(_, response)| {
                runtime::typography::cleaned(&api::response_text(&response)).into_owned()
            })
            .filter(|summary| !summary.trim().is_empty())
    }

    fn reply_cut_by_output_limit(&mut self) -> bool {
        std::mem::take(&mut self.reply_cut)
    }

    #[allow(clippy::too_many_lines)]
    fn stream(&mut self, request: ApiRequest) -> Result<Vec<AssistantEvent>, RuntimeError> {
        let tools = tool_specs_for_allowed_tools(Some(&self.allowed_tools))
            .into_iter()
            .map(|spec| ToolDefinition {
                name: spec.name.to_string(),
                description: Some(spec.description.to_string()),
                input_schema: spec.input_schema,
            })
            .chain(
                connected_mcp_tools()
                    .into_iter()
                    .filter(|tool| self.allowed_tools.contains(&tool.name))
                    .map(|tool| ToolDefinition {
                        name: tool.name,
                        description: tool.description,
                        input_schema: tool.input_schema,
                    }),
            )
            .collect::<Vec<_>>();
        let messages = convert_messages(&request.messages);
        let system =
            (!request.system_prompt.is_empty()).then(|| request.system_prompt.join("\n\n"));
        let tools = (!tools.is_empty()).then_some(tools);
        let tool_choice = (!self.allowed_tools.is_empty()).then_some(if request.text_only {
            ToolChoice::None
        } else {
            ToolChoice::Auto
        });
        let build = move |model: &str| MessageRequest {
            model: model.to_string(),
            max_tokens: max_tokens_for_model(model),
            messages: messages.clone(),
            system: system.clone(),
            tools: tools.clone(),
            tool_choice: tool_choice.clone(),
            stream: true,
        };
        let mut notify = notify_subagent_fallback;

        // Quand le fournisseur du modèle lâche en cours de réponse (504,
        // surcharge), la requête repart sur un autre modèle de la chaîne.
        let mut switches_left = api::UPSTREAM_SWITCHES;
        let answering = std::cell::RefCell::new(String::new());
        let cut = std::cell::Cell::new(false);
        loop {
            cut.set(false);
            let result: Result<Vec<AssistantEvent>, RuntimeError> = self.runtime.block_on(async {
                let opened =
                    runtime::interrupt::interruptible(self.chain.open_stream(&build, &mut notify))
                        .await?;
                answering.borrow_mut().clone_from(&opened.model);
                note_model_request(&opened.model);
                let mut stream = opened.stream;
                let mut first_event = opened.first_event;
                let mut events = Vec::new();
                let mut pending_tools: BTreeMap<u32, (String, String, String)> = BTreeMap::new();
                let mut saw_stop = false;
                let mut stop_reason: Option<String> = None;
                let mut signed_thinking = SignedThinking::new();
                // Ni tiret cadratin ni emoji dans ce qu'un sous-agent écrit.
                let mut text_dashes = runtime::typography::OutputFilter::default();

                loop {
                    let event = match first_event.take() {
                        Some(event) => event,
                        None => match runtime::interrupt::interruptible(stream.next_event()).await?
                        {
                            Some(event) => event,
                            None => break,
                        },
                    };
                    match event {
                        ApiStreamEvent::MessageStart(start) => {
                            for block in start.message.content {
                                push_output_block(block, 0, &mut events, &mut pending_tools, true);
                            }
                        }
                        ApiStreamEvent::ContentBlockStart(start) => {
                            start_signed_thinking(
                                &start.content_block,
                                start.index,
                                &mut signed_thinking,
                            );
                            push_output_block(
                                start.content_block,
                                start.index,
                                &mut events,
                                &mut pending_tools,
                                true,
                            );
                        }
                        ApiStreamEvent::ContentBlockDelta(delta) => match delta.delta {
                            ContentBlockDelta::TextDelta { text } => {
                                let text = text_dashes.push(&text);
                                if !text.is_empty() {
                                    events.push(AssistantEvent::TextDelta(text));
                                }
                            }
                            ContentBlockDelta::InputJsonDelta { partial_json } => {
                                if let Some((_, _, input)) = pending_tools.get_mut(&delta.index) {
                                    input.push_str(&partial_json);
                                }
                            }
                            ContentBlockDelta::ThinkingDelta { thinking } => {
                                if let Some(entry) = signed_thinking.get_mut(&delta.index) {
                                    entry.0.push_str(&thinking);
                                }
                            }
                            ContentBlockDelta::SignatureDelta { signature } => {
                                if let Some(entry) = signed_thinking.get_mut(&delta.index) {
                                    entry.1.push_str(&signature);
                                }
                            }
                        },
                        ApiStreamEvent::ContentBlockStop(stop) => {
                            finish_signed_thinking(stop.index, &mut signed_thinking, &mut events);
                            if let Some((id, name, input)) = pending_tools.remove(&stop.index) {
                                events.push(AssistantEvent::ToolUse { id, name, input });
                            }
                        }
                        ApiStreamEvent::MessageDelta(delta) => {
                            if delta.delta.stop_reason.is_some() {
                                stop_reason.clone_from(&delta.delta.stop_reason);
                            }
                            events.push(AssistantEvent::Usage(TokenUsage {
                                input_tokens: delta.usage.input_tokens,
                                output_tokens: delta.usage.output_tokens,
                                cache_creation_input_tokens: 0,
                                cache_read_input_tokens: 0,
                            }));
                        }
                        ApiStreamEvent::MessageStop(_) => {
                            saw_stop = true;
                            events.push(AssistantEvent::MessageStop);
                        }
                    }
                }
                // Appel d'outil coupé (réponse close avant ses arguments, ou
                // limite de sortie) : il n'est pas exécuté tel quel, le
                // sous-agent apprend pourquoi et le renvoie.
                let _ = input_check::check_cut_calls(
                    &mut events,
                    stop_reason.as_deref(),
                    required_arguments_of,
                    false,
                );
                // `Read`, `Bash`, `todowrite` : les noms d'autres agents, comme
                // pour l'agent principal.
                let known = known_tool_names();
                for event in &mut events {
                    if let AssistantEvent::ToolUse { name, .. } = event {
                        if let Some(resolved) = resolve_name_among(name, &known) {
                            *name = resolved;
                        }
                    }
                }
                input_check::restore_names_in(&mut events, schema_of);
                cut.set(input_check::reply_cut_by_output_limit(
                    stop_reason.as_deref(),
                    &events,
                ));

                if !saw_stop
                    && events.iter().any(|event| {
                        matches!(event, AssistantEvent::TextDelta(text) if !text.is_empty())
                            || matches!(event, AssistantEvent::ToolUse { .. })
                    })
                {
                    events.push(AssistantEvent::MessageStop);
                }

                if events
                    .iter()
                    .any(|event| matches!(event, AssistantEvent::MessageStop))
                {
                    return Ok(events);
                }

                let build_complete = |model: &str| MessageRequest {
                    stream: false,
                    ..build(model)
                };
                let (model, response) = runtime::interrupt::interruptible(
                    self.chain.send(&build_complete, &mut notify),
                )
                .await?;
                note_model_request(&model);
                let stop_reason = response.stop_reason.clone();
                let events = response_to_events(response);
                cut.set(input_check::reply_cut_by_output_limit(
                    stop_reason.as_deref(),
                    &events,
                ));
                Ok(events)
            });
            let Err(error) = result else {
                self.reply_cut = cut.get();
                return result;
            };
            let failed = answering.borrow().clone();
            let next = api::upstream_failure_status(&error.to_string())
                .filter(|_| switches_left > 0 && !failed.is_empty())
                .and_then(|status| Some((status, self.chain.switch_after_failure(&failed)?)));
            let Some((status, next)) = next else {
                return Err(error);
            };
            switches_left -= 1;
            notify(&format!(
                "{failed} a cessé de répondre (erreur {status}) → la requête repart sur {next}"
            ));
        }
    }
}

struct SubagentToolExecutor {
    allowed_tools: BTreeSet<String>,
    progress_label: Option<String>,
}

impl SubagentToolExecutor {
    fn new(allowed_tools: BTreeSet<String>) -> Self {
        Self {
            allowed_tools,
            progress_label: None,
        }
    }

    /// Affiche chaque outil utilisé (sur stderr, pour ne pas polluer une sortie JSON).
    fn with_progress_label(mut self, label: Option<String>) -> Self {
        self.progress_label = label;
        self
    }
}

impl ToolExecutor for SubagentToolExecutor {
    /// Un sous-agent n'a pas de lecteur d'images : ses captures ne partent pas
    /// au modèle (un modèle qui ne lit pas les images ferait refuser la
    /// requête) ; il garde le rapport mesuré, en texte.
    fn take_images(&mut self) -> Vec<runtime::attachments::ImageAttachment> {
        let _ = take_screenshots();
        Vec::new()
    }

    fn execute(&mut self, tool_name: &str, input: &str) -> Result<String, ToolError> {
        if !self.allowed_tools.contains(tool_name) {
            let allowed: Vec<&str> = self.allowed_tools.iter().map(String::as_str).collect();
            return Err(ToolError::new(format!(
                "[Claw] \"{tool_name}\" is not available to this sub-agent, so nothing was executed. Tools you can call here: {}. If the task needs a tool that is not listed, say so in your final report instead of retrying.",
                allowed.join(", ")
            )));
        }
        if let Some(label) = &self.progress_label {
            let compact = input.split_whitespace().collect::<Vec<_>>().join(" ");
            let preview = compact.chars().take(100).collect::<String>();
            eprintln!("    [{label}] {tool_name} {preview}");
        }
        let value = serde_json::from_str(input)
            .map_err(|error| ToolError::new(format!("invalid tool input JSON: {error}")))?;
        if tool_name.starts_with("mcp__") {
            return call_mcp_tool(tool_name, &value).map_err(ToolError::new);
        }
        execute_tool(tool_name, &value).map_err(ToolError::new)
    }

    /// Arguments obligatoires absents, JSON invalide ou appel coupé : l'erreur
    /// va au sous-agent sans rien exécuter.
    fn invalid_input(&mut self, tool_name: &str, input: &str) -> Option<String> {
        let schema = schema_of(tool_name)?;
        let known: Vec<String> = schema
            .get("properties")
            .and_then(Value::as_object)
            .map(|properties| properties.keys().cloned().collect())
            .unwrap_or_default();
        input_check::problem(
            tool_name,
            input,
            &input_check::required_arguments(&schema),
            &known,
        )
    }

    fn argument_note(&mut self, tool_name: &str, input: &str) -> Option<String> {
        input_check::take_argument_note(tool_name, input)
    }
}

/// Arguments obligatoires d'un outil intégré ou d'un outil MCP relié (aucun
/// pour un outil inconnu : son exécution dira ce qui ne va pas).
/// Le nom d'un outil parmi `known`, écrit comme d'autres agents l'écrivent
/// (`Read`, `Bash`, `todowrite`, `grep`).
fn resolve_name_among(name: &str, known: &[String]) -> Option<String> {
    if known.iter().any(|candidate| candidate == name) {
        return Some(name.to_string());
    }
    let claude = canonical_agent_tool_name(name);
    if known.contains(&claude) {
        return Some(claude);
    }
    let normalized = normalize_tool_name(name);
    let short = match normalized.as_str() {
        "read" => "read_file",
        "write" => "write_file",
        "edit" | "multiedit" | "multi_edit" => "edit_file",
        "glob" => "glob_search",
        "grep" => "grep_search",
        _ => "",
    };
    if !short.is_empty() {
        return Some(short.to_string());
    }
    known
        .iter()
        .find(|candidate| normalize_tool_name(candidate) == normalized)
        .cloned()
}

/// Noms d'outils d'autres agents et l'outil de Claw qui fait ce travail.
const TOOL_SYNONYMS: &[(&str, &str)] = &[
    ("run_terminal_cmd", "bash"),
    ("execute_command", "bash"),
    ("run_command", "bash"),
    ("shell", "bash"),
    ("terminal", "bash"),
    ("list_dir", "glob_search"),
    ("list_directory", "glob_search"),
    ("list_files", "glob_search"),
    ("ls", "glob_search"),
    ("find_files", "glob_search"),
    ("str_replace_editor", "edit_file"),
    ("str_replace", "edit_file"),
    ("replace_in_file", "edit_file"),
    ("apply_diff", "edit_file"),
    ("create_file", "write_file"),
    ("write_to_file", "write_file"),
    ("search_files", "grep_search"),
    ("codebase_search", "grep_search"),
    ("search", "grep_search"),
    ("rg", "grep_search"),
    ("cat", "read_file"),
    ("view", "read_file"),
    ("open_file", "read_file"),
    ("view_file", "read_file"),
];

/// Un outil appelé qui n'existe pas : les plus proches (synonymes d'autres
/// agents, puis noms voisins) et la liste des outils, au lieu d'une question
/// d'autorisation pour un outil impossible.
#[must_use]
pub fn unknown_tool_message(name: &str, known: &[String]) -> String {
    let lower = name.to_ascii_lowercase();
    let mut closest: Vec<String> = TOOL_SYNONYMS
        .iter()
        .filter(|(other, _)| *other == lower)
        .map(|(_, tool)| (*tool).to_string())
        .filter(|tool| known.contains(tool))
        .collect();
    if closest.is_empty() {
        let normalized = normalize_tool_name(name);
        closest = known
            .iter()
            .filter(|candidate| {
                let candidate = normalize_tool_name(candidate);
                candidate.contains(&normalized) || normalized.contains(&candidate)
            })
            .take(3)
            .cloned()
            .collect();
    }
    let closest = if closest.is_empty() {
        String::new()
    } else {
        format!(" Closest: {}.", closest.join(", "))
    };
    format!(
        "[Claw] There is no tool named \"{name}\", so nothing was executed.{closest} Tools you can call: {}.",
        known.join(", ")
    )
}

/// Outils intégrés et outils MCP reliés.
fn known_tool_names() -> Vec<String> {
    mvp_tool_specs()
        .iter()
        .map(|spec| spec.name.to_string())
        .chain(connected_mcp_tools().into_iter().map(|tool| tool.name))
        .collect()
}

fn required_arguments_of(tool_name: &str) -> Vec<String> {
    schema_of(tool_name)
        .map(|schema| input_check::required_arguments(&schema))
        .unwrap_or_default()
}

/// Schéma des arguments d'un outil intégré ou d'un outil MCP relié.
fn schema_of(tool_name: &str) -> Option<Value> {
    if tool_name.starts_with("mcp__") {
        return connected_mcp_tools()
            .into_iter()
            .find(|tool| tool.name == tool_name)
            .map(|tool| tool.input_schema);
    }
    mvp_tool_specs()
        .into_iter()
        .find(|spec| spec.name == tool_name)
        .map(|spec| spec.input_schema)
}

fn tool_specs_for_allowed_tools(allowed_tools: Option<&BTreeSet<String>>) -> Vec<ToolSpec> {
    mvp_tool_specs()
        .into_iter()
        .filter(|spec| allowed_tools.is_none_or(|allowed| allowed.contains(spec.name)))
        .collect()
}

fn convert_messages(messages: &[ConversationMessage]) -> Vec<InputMessage> {
    messages
        .iter()
        .filter_map(|message| {
            let role = match message.role {
                MessageRole::System | MessageRole::User | MessageRole::Tool => "user",
                MessageRole::Assistant => "assistant",
            };
            let content = message
                .blocks
                .iter()
                .map(|block| match block {
                    ContentBlock::Text { text } => InputContentBlock::Text { text: text.clone() },
                    ContentBlock::ToolUse { id, name, input } => InputContentBlock::ToolUse {
                        id: id.clone(),
                        name: name.clone(),
                        input: serde_json::from_str(input)
                            .unwrap_or_else(|_| serde_json::json!({ "raw": input })),
                    },
                    ContentBlock::ToolResult {
                        tool_use_id,
                        output,
                        is_error,
                        ..
                    } => InputContentBlock::ToolResult {
                        tool_use_id: tool_use_id.clone(),
                        content: vec![ToolResultContentBlock::Text {
                            text: output.clone(),
                        }],
                        is_error: *is_error,
                    },
                    ContentBlock::Image { media_type, data } => InputContentBlock::Image {
                        source: api::ImageSource::base64(media_type.clone(), data.clone()),
                    },
                    ContentBlock::Thinking {
                        thinking,
                        signature,
                        redacted,
                    } => match redacted {
                        Some(data) => InputContentBlock::RedactedThinking { data: data.clone() },
                        None => InputContentBlock::Thinking {
                            thinking: thinking.clone(),
                            signature: signature.clone(),
                        },
                    },
                })
                .collect::<Vec<_>>();
            (!content.is_empty()).then(|| InputMessage {
                role: role.to_string(),
                content,
            })
        })
        .collect()
}

/// Raisonnements signés en cours de réception, par index de bloc :
/// (texte, signature, contenu chiffré éventuel).
type SignedThinking = std::collections::BTreeMap<u32, (String, String, Option<String>)>;

/// Début d'un bloc de raisonnement Claude (à renvoyer ensuite à l'API).
fn start_signed_thinking(block: &OutputContentBlock, index: u32, signed: &mut SignedThinking) {
    match block {
        OutputContentBlock::Thinking {
            thinking,
            signature,
        } => {
            signed.insert(
                index,
                (
                    thinking.clone(),
                    signature.clone().unwrap_or_default(),
                    None,
                ),
            );
        }
        OutputContentBlock::RedactedThinking { data } => {
            let data = data
                .as_str()
                .map_or_else(|| data.to_string(), ToOwned::to_owned);
            signed.insert(index, (String::new(), String::new(), Some(data)));
        }
        _ => {}
    }
}

/// Fin d'un bloc : un raisonnement signé devient un événement conservé.
fn finish_signed_thinking(
    index: u32,
    signed: &mut SignedThinking,
    events: &mut Vec<AssistantEvent>,
) {
    if let Some((thinking, signature, redacted)) = signed.remove(&index) {
        if !signature.is_empty() || redacted.is_some() {
            events.push(AssistantEvent::Thinking {
                thinking,
                signature,
                redacted,
            });
        }
    }
}

fn push_output_block(
    block: OutputContentBlock,
    block_index: u32,
    events: &mut Vec<AssistantEvent>,
    pending_tools: &mut BTreeMap<u32, (String, String, String)>,
    streaming_tool_input: bool,
) {
    match block {
        OutputContentBlock::Text { text } => {
            let text = runtime::typography::cleaned(&text).into_owned();
            if !text.is_empty() {
                events.push(AssistantEvent::TextDelta(text));
            }
        }
        OutputContentBlock::ToolUse { id, name, input } => {
            let initial_input = if streaming_tool_input
                && input.is_object()
                && input.as_object().is_some_and(serde_json::Map::is_empty)
            {
                String::new()
            } else {
                input.to_string()
            };
            pending_tools.insert(block_index, (id, name, initial_input));
        }
        OutputContentBlock::Thinking { .. } | OutputContentBlock::RedactedThinking { .. } => {}
    }
}

fn response_to_events(response: MessageResponse) -> Vec<AssistantEvent> {
    let mut events = Vec::new();
    let mut pending_tools = BTreeMap::new();

    for (index, block) in response.content.into_iter().enumerate() {
        let index =
            u32::try_from(index).expect("dépassement de capacité de l'index de bloc de réponse");
        push_output_block(block, index, &mut events, &mut pending_tools, false);
        if let Some((id, name, input)) = pending_tools.remove(&index) {
            events.push(AssistantEvent::ToolUse { id, name, input });
        }
    }

    events.push(AssistantEvent::Usage(TokenUsage {
        input_tokens: response.usage.input_tokens,
        output_tokens: response.usage.output_tokens,
        cache_creation_input_tokens: response.usage.cache_creation_input_tokens,
        cache_read_input_tokens: response.usage.cache_read_input_tokens,
    }));
    events.push(AssistantEvent::MessageStop);
    events
}

fn final_assistant_text(summary: &runtime::TurnSummary) -> String {
    summary
        .assistant_messages
        .last()
        .map(|message| {
            message
                .blocks
                .iter()
                .filter_map(|block| match block {
                    ContentBlock::Text { text } => Some(text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("")
        })
        .unwrap_or_default()
}

#[allow(clippy::needless_pass_by_value)]
fn execute_tool_search(input: ToolSearchInput) -> ToolSearchOutput {
    let deferred = deferred_tool_specs();
    let max_results = input.max_results.unwrap_or(5).max(1);
    let query = input.query.trim().to_string();
    let normalized_query = normalize_tool_search_query(&query);
    let matches = search_tool_specs(&query, max_results, &deferred);

    ToolSearchOutput {
        matches,
        query,
        normalized_query,
        total_deferred_tools: deferred.len(),
        pending_mcp_servers: None,
    }
}

fn deferred_tool_specs() -> Vec<ToolSpec> {
    mvp_tool_specs()
        .into_iter()
        .filter(|spec| {
            !matches!(
                spec.name,
                "bash" | "read_file" | "write_file" | "edit_file" | "glob_search" | "grep_search"
            )
        })
        .collect()
}

fn search_tool_specs(query: &str, max_results: usize, specs: &[ToolSpec]) -> Vec<String> {
    let lowered = query.to_lowercase();
    if let Some(selection) = lowered.strip_prefix("select:") {
        return selection
            .split(',')
            .map(str::trim)
            .filter(|part| !part.is_empty())
            .filter_map(|wanted| {
                let wanted = canonical_tool_token(wanted);
                specs
                    .iter()
                    .find(|spec| canonical_tool_token(spec.name) == wanted)
                    .map(|spec| spec.name.to_string())
            })
            .take(max_results)
            .collect();
    }

    let mut required = Vec::new();
    let mut optional = Vec::new();
    for term in lowered.split_whitespace() {
        if let Some(rest) = term.strip_prefix('+') {
            if !rest.is_empty() {
                required.push(rest);
            }
        } else {
            optional.push(term);
        }
    }
    let terms = if required.is_empty() {
        optional.clone()
    } else {
        required.iter().chain(optional.iter()).copied().collect()
    };

    let mut scored = specs
        .iter()
        .filter_map(|spec| {
            let name = spec.name.to_lowercase();
            let canonical_name = canonical_tool_token(spec.name);
            let normalized_description = normalize_tool_search_query(spec.description);
            let haystack = format!(
                "{name} {} {canonical_name}",
                spec.description.to_lowercase()
            );
            let normalized_haystack = format!("{canonical_name} {normalized_description}");
            if required.iter().any(|term| !haystack.contains(term)) {
                return None;
            }

            let mut score = 0_i32;
            for term in &terms {
                let canonical_term = canonical_tool_token(term);
                if haystack.contains(term) {
                    score += 2;
                }
                if name == *term {
                    score += 8;
                }
                if name.contains(term) {
                    score += 4;
                }
                if canonical_name == canonical_term {
                    score += 12;
                }
                if normalized_haystack.contains(&canonical_term) {
                    score += 3;
                }
            }

            if score == 0 && !lowered.is_empty() {
                return None;
            }
            Some((score, spec.name.to_string()))
        })
        .collect::<Vec<_>>();

    scored.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| left.1.cmp(&right.1)));
    scored
        .into_iter()
        .map(|(_, name)| name)
        .take(max_results)
        .collect()
}

fn normalize_tool_search_query(query: &str) -> String {
    query
        .trim()
        .split(|ch: char| ch.is_whitespace() || ch == ',')
        .filter(|term| !term.is_empty())
        .map(canonical_tool_token)
        .collect::<Vec<_>>()
        .join(" ")
}

fn canonical_tool_token(value: &str) -> String {
    let mut canonical = value
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .flat_map(char::to_lowercase)
        .collect::<String>();
    if let Some(stripped) = canonical.strip_suffix("tool") {
        canonical = stripped.to_string();
    }
    canonical
}

/// Dossier des rapports des sous-agents : `<dossier de travail>/.claw/agents`
/// (`CLAW_AGENT_STORE` le remplace).
fn agent_store_dir() -> Result<std::path::PathBuf, String> {
    if let Ok(path) = std::env::var("CLAW_AGENT_STORE") {
        return Ok(std::path::PathBuf::from(path));
    }
    let cwd = std::env::current_dir().map_err(|error| error.to_string())?;
    Ok(cwd.join(".claw").join("agents"))
}

/// Crée le dossier des rapports ; s'il ne peut pas l'être (droits,
/// disque), le dossier temporaire du système le remplace, sans erreur
/// pour le modèle.
fn ready_agent_store_dir() -> Result<std::path::PathBuf, String> {
    let dir = agent_store_dir()?;
    if std::fs::create_dir_all(&dir).is_ok() {
        return Ok(dir);
    }
    let fallback = std::env::temp_dir().join("claw-agents");
    std::fs::create_dir_all(&fallback)
        .map_err(|error| format!("{} : {error}", fallback.display()))?;
    Ok(fallback)
}

/// Envoi d'un hook `http` (fourni au runtime au démarrage) : POST du JSON de
/// l'événement, renvoie le statut et le corps de la réponse.
pub fn send_hook_request(
    url: &str,
    headers: &BTreeMap<String, String>,
    body: &str,
    timeout: Duration,
) -> Result<(u16, String), String> {
    let client = Client::builder()
        .timeout(timeout)
        .build()
        .map_err(|error| error.to_string())?;
    let mut request = client
        .post(url)
        .header("Content-Type", "application/json")
        .body(body.to_string());
    for (name, value) in headers {
        request = request.header(name.as_str(), value.as_str());
    }
    let response = request.send().map_err(|error| error.to_string())?;
    let status = response.status().as_u16();
    let text = response.text().map_err(|error| error.to_string())?;
    Ok((status, text))
}

/// Sous-agent lancé par l'outil `Agent` (d'après son manifeste).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentRun {
    pub id: String,
    pub name: String,
    pub description: String,
    pub status: String,
    pub created_at: String,
    pub output_file: String,
}

/// Les `limit` derniers sous-agents, du plus récent au plus ancien (`/tasks`).
#[must_use]
pub fn recent_agent_runs(limit: usize) -> Vec<AgentRun> {
    let Ok(dir) = agent_store_dir() else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut runs: Vec<AgentRun> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("json"))
        })
        .filter_map(|path| {
            let value: serde_json::Value =
                serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()?;
            let text = |key: &str| value.get(key).and_then(serde_json::Value::as_str);
            Some(AgentRun {
                id: text("agentId")?.to_string(),
                name: text("name").unwrap_or_default().to_string(),
                description: text("description").unwrap_or_default().to_string(),
                status: text("status").unwrap_or("?").to_string(),
                created_at: text("createdAt").unwrap_or_default().to_string(),
                output_file: text("outputFile").unwrap_or_default().to_string(),
            })
        })
        .collect();
    runs.sort_by(|left, right| right.created_at.cmp(&left.created_at));
    runs.truncate(limit);
    runs
}

fn make_agent_id() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("agent-{nanos}")
}

fn slugify_agent_name(description: &str) -> String {
    let mut out = description
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>();
    while out.contains("--") {
        out = out.replace("--", "-");
    }
    out.trim_matches('-').chars().take(32).collect()
}

fn normalize_subagent_type(subagent_type: Option<&str>) -> String {
    let trimmed = subagent_type.map(str::trim).unwrap_or_default();
    if trimmed.is_empty() {
        return String::from("general-purpose");
    }

    match canonical_tool_token(trimmed).as_str() {
        "general" | "generalpurpose" | "generalpurposeagent" => String::from("general-purpose"),
        "explore" | "explorer" | "exploreagent" => String::from("Explore"),
        "plan" | "planagent" => String::from("Plan"),
        "verification" | "verificationagent" | "verify" | "verifier" => {
            String::from("Verification")
        }
        "clawguide" | "clawguideagent" | "guide" => String::from("claw-guide"),
        "statusline" | "statuslinesetup" => String::from("statusline-setup"),
        "architect" | "architecte" | "architecture" | "softwarearchitect" => {
            String::from("architect")
        }
        "reviewer" | "review" | "codereview" | "codereviewer" | "relecteur" => {
            String::from("reviewer")
        }
        "auditor" | "audit" | "auditeur" | "projectaudit" => String::from("auditor"),
        "tester" | "test" | "testeur" | "fixer" | "testfixer" | "testrunner" => {
            String::from("tester")
        }
        _ => trimmed.to_string(),
    }
}

fn iso8601_now() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .to_string()
}

#[allow(clippy::too_many_lines)]
fn execute_notebook_edit(input: NotebookEditInput) -> Result<NotebookEditOutput, String> {
    let path = std::path::PathBuf::from(&input.notebook_path);
    if path.extension().and_then(|ext| ext.to_str()) != Some("ipynb") {
        return Err(String::from(
            "File must be a Jupyter notebook (.ipynb file).",
        ));
    }

    let original_file = std::fs::read_to_string(&path).map_err(|error| error.to_string())?;
    let mut notebook: serde_json::Value =
        serde_json::from_str(&original_file).map_err(|error| error.to_string())?;
    let language = notebook
        .get("metadata")
        .and_then(|metadata| metadata.get("kernelspec"))
        .and_then(|kernelspec| kernelspec.get("language"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or("python")
        .to_string();
    let cells = notebook
        .get_mut("cells")
        .and_then(serde_json::Value::as_array_mut)
        .ok_or_else(|| String::from("Notebook cells array not found"))?;

    let edit_mode = input.edit_mode.unwrap_or(NotebookEditMode::Replace);
    let target_index = match input.cell_id.as_deref() {
        Some(cell_id) => Some(resolve_cell_index(cells, Some(cell_id), edit_mode)?),
        None if matches!(
            edit_mode,
            NotebookEditMode::Replace | NotebookEditMode::Delete
        ) =>
        {
            Some(resolve_cell_index(cells, None, edit_mode)?)
        }
        None => None,
    };
    let resolved_cell_type = match edit_mode {
        NotebookEditMode::Delete => None,
        NotebookEditMode::Insert => Some(input.cell_type.unwrap_or(NotebookCellType::Code)),
        NotebookEditMode::Replace => Some(input.cell_type.unwrap_or_else(|| {
            target_index
                .and_then(|index| cells.get(index))
                .and_then(cell_kind)
                .unwrap_or(NotebookCellType::Code)
        })),
    };
    let new_source = require_notebook_source(input.new_source, edit_mode)?;

    let cell_id = match edit_mode {
        NotebookEditMode::Insert => {
            let resolved_cell_type = resolved_cell_type.expect("type de cellule pour insertion");
            let new_id = make_cell_id(cells.len());
            let new_cell = build_notebook_cell(&new_id, resolved_cell_type, &new_source);
            let insert_at = target_index.map_or(cells.len(), |index| index + 1);
            cells.insert(insert_at, new_cell);
            cells
                .get(insert_at)
                .and_then(|cell| cell.get("id"))
                .and_then(serde_json::Value::as_str)
                .map(ToString::to_string)
        }
        NotebookEditMode::Delete => {
            let removed = cells.remove(target_index.expect("index cible pour suppression"));
            removed
                .get("id")
                .and_then(serde_json::Value::as_str)
                .map(ToString::to_string)
        }
        NotebookEditMode::Replace => {
            let resolved_cell_type = resolved_cell_type.expect("type de cellule pour remplacement");
            let cell = cells
                .get_mut(target_index.expect("index cible pour remplacement"))
                .ok_or_else(|| String::from("Cell index out of range"))?;
            cell["source"] = serde_json::Value::Array(source_lines(&new_source));
            cell["cell_type"] = serde_json::Value::String(match resolved_cell_type {
                NotebookCellType::Code => String::from("code"),
                NotebookCellType::Markdown => String::from("markdown"),
            });
            match resolved_cell_type {
                NotebookCellType::Code => {
                    if !cell.get("outputs").is_some_and(serde_json::Value::is_array) {
                        cell["outputs"] = json!([]);
                    }
                    if cell.get("execution_count").is_none() {
                        cell["execution_count"] = serde_json::Value::Null;
                    }
                }
                NotebookCellType::Markdown => {
                    if let Some(object) = cell.as_object_mut() {
                        object.remove("outputs");
                        object.remove("execution_count");
                    }
                }
            }
            cell.get("id")
                .and_then(serde_json::Value::as_str)
                .map(ToString::to_string)
        }
    };

    let updated_file =
        serde_json::to_string_pretty(&notebook).map_err(|error| error.to_string())?;
    std::fs::write(&path, &updated_file).map_err(|error| error.to_string())?;

    Ok(NotebookEditOutput {
        new_source,
        cell_id,
        cell_type: resolved_cell_type,
        language,
        edit_mode: format_notebook_edit_mode(edit_mode),
        error: None,
        notebook_path: path.display().to_string(),
        original_file,
        updated_file,
    })
}

fn require_notebook_source(
    source: Option<String>,
    edit_mode: NotebookEditMode,
) -> Result<String, String> {
    match edit_mode {
        NotebookEditMode::Delete => Ok(source.unwrap_or_default()),
        NotebookEditMode::Insert | NotebookEditMode::Replace => source
            .ok_or_else(|| String::from("new_source is required for insert and replace edits")),
    }
}

fn build_notebook_cell(cell_id: &str, cell_type: NotebookCellType, source: &str) -> Value {
    let mut cell = json!({
        "cell_type": match cell_type {
            NotebookCellType::Code => "code",
            NotebookCellType::Markdown => "markdown",
        },
        "id": cell_id,
        "metadata": {},
        "source": source_lines(source),
    });
    if let Some(object) = cell.as_object_mut() {
        match cell_type {
            NotebookCellType::Code => {
                object.insert(String::from("outputs"), json!([]));
                object.insert(String::from("execution_count"), Value::Null);
            }
            NotebookCellType::Markdown => {}
        }
    }
    cell
}

fn cell_kind(cell: &serde_json::Value) -> Option<NotebookCellType> {
    cell.get("cell_type")
        .and_then(serde_json::Value::as_str)
        .map(|kind| {
            if kind == "markdown" {
                NotebookCellType::Markdown
            } else {
                NotebookCellType::Code
            }
        })
}

#[allow(clippy::needless_pass_by_value)]
fn execute_sleep(input: SleepInput) -> SleepOutput {
    std::thread::sleep(Duration::from_millis(input.duration_ms));
    SleepOutput {
        duration_ms: input.duration_ms,
        message: format!("Slept for {}ms", input.duration_ms),
    }
}

fn execute_brief(input: BriefInput) -> Result<BriefOutput, String> {
    if input.message.trim().is_empty() {
        return Err(String::from("message must not be empty"));
    }

    let attachments = input
        .attachments
        .as_ref()
        .map(|paths| {
            paths
                .iter()
                .map(|path| resolve_attachment(path))
                .collect::<Result<Vec<_>, String>>()
        })
        .transpose()?;

    let message = match input.status {
        BriefStatus::Normal | BriefStatus::Proactive => input.message,
    };

    Ok(BriefOutput {
        message,
        attachments,
        sent_at: iso8601_timestamp(),
    })
}

fn resolve_attachment(path: &str) -> Result<ResolvedAttachment, String> {
    let resolved = std::fs::canonicalize(path).map_err(|error| error.to_string())?;
    let metadata = std::fs::metadata(&resolved).map_err(|error| error.to_string())?;
    Ok(ResolvedAttachment {
        path: resolved.display().to_string(),
        size: metadata.len(),
        is_image: is_image_path(&resolved),
    })
}

fn is_image_path(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|ext| ext.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref(),
        Some("png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "svg")
    )
}

fn execute_config(input: ConfigInput) -> Result<ConfigOutput, String> {
    let setting = input.setting.trim();
    if setting.is_empty() {
        return Err(String::from("setting must not be empty"));
    }
    let Some(spec) = supported_config_setting(setting) else {
        return Ok(ConfigOutput {
            success: false,
            operation: None,
            setting: None,
            value: None,
            previous_value: None,
            new_value: None,
            error: Some(format!("Unknown setting: \"{setting}\"")),
        });
    };

    let path = config_file_for_scope(spec.scope)?;
    let mut document = read_json_object(&path)?;

    if let Some(value) = input.value {
        let normalized = normalize_config_value(spec, value)?;
        let previous_value = get_nested_value(&document, spec.path).cloned();
        set_nested_value(&mut document, spec.path, normalized.clone());
        write_json_object(&path, &document)?;
        Ok(ConfigOutput {
            success: true,
            operation: Some(String::from("set")),
            setting: Some(setting.to_string()),
            value: Some(normalized.clone()),
            previous_value,
            new_value: Some(normalized),
            error: None,
        })
    } else {
        Ok(ConfigOutput {
            success: true,
            operation: Some(String::from("get")),
            setting: Some(setting.to_string()),
            value: get_nested_value(&document, spec.path).cloned(),
            previous_value: None,
            new_value: None,
            error: None,
        })
    }
}

fn execute_structured_output(input: StructuredOutputInput) -> StructuredOutputResult {
    StructuredOutputResult {
        data: String::from("Structured output provided successfully"),
        structured_output: input.0,
    }
}

fn execute_repl(input: ReplInput) -> Result<ReplOutput, String> {
    if input.code.trim().is_empty() {
        return Err(String::from("code must not be empty"));
    }
    let _ = input.timeout_ms;
    let runtime = resolve_repl_runtime(&input.language)?;
    let started = Instant::now();
    let output = Command::new(runtime.program)
        .args(runtime.args)
        .arg(&input.code)
        .output()
        .map_err(|error| error.to_string())?;

    Ok(ReplOutput {
        language: input.language,
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        exit_code: output.status.code().unwrap_or(1),
        duration_ms: started.elapsed().as_millis(),
    })
}

struct ReplRuntime {
    program: &'static str,
    args: &'static [&'static str],
}

fn resolve_repl_runtime(language: &str) -> Result<ReplRuntime, String> {
    match language.trim().to_ascii_lowercase().as_str() {
        "python" | "py" => Ok(ReplRuntime {
            program: detect_first_command(&["python3", "python", "py"])
                .ok_or_else(|| String::from("python runtime not found"))?,
            args: &["-c"],
        }),
        "javascript" | "js" | "node" => Ok(ReplRuntime {
            program: detect_first_command(&["node"])
                .ok_or_else(|| String::from("node runtime not found"))?,
            args: &["-e"],
        }),
        "sh" | "shell" | "bash" => Ok(ReplRuntime {
            program: detect_first_command(&["bash", "sh"])
                .ok_or_else(|| String::from("shell runtime not found"))?,
            args: &["-lc"],
        }),
        other => Err(format!("unsupported REPL language: {other}")),
    }
}

fn detect_first_command(commands: &[&'static str]) -> Option<&'static str> {
    commands
        .iter()
        .copied()
        .find(|command| command_exists(command))
}

#[derive(Clone, Copy)]
enum ConfigScope {
    Global,
    Settings,
}

#[derive(Clone, Copy)]
struct ConfigSettingSpec {
    scope: ConfigScope,
    kind: ConfigKind,
    path: &'static [&'static str],
    options: Option<&'static [&'static str]>,
}

#[derive(Clone, Copy)]
enum ConfigKind {
    Boolean,
    String,
}

fn supported_config_setting(setting: &str) -> Option<ConfigSettingSpec> {
    Some(match setting {
        "theme" => ConfigSettingSpec {
            scope: ConfigScope::Global,
            kind: ConfigKind::String,
            path: &["theme"],
            options: None,
        },
        "editorMode" => ConfigSettingSpec {
            scope: ConfigScope::Global,
            kind: ConfigKind::String,
            path: &["editorMode"],
            options: Some(&["default", "vim", "emacs"]),
        },
        "verbose" => ConfigSettingSpec {
            scope: ConfigScope::Global,
            kind: ConfigKind::Boolean,
            path: &["verbose"],
            options: None,
        },
        "preferredNotifChannel" => ConfigSettingSpec {
            scope: ConfigScope::Global,
            kind: ConfigKind::String,
            path: &["preferredNotifChannel"],
            options: None,
        },
        "autoCompactEnabled" => ConfigSettingSpec {
            scope: ConfigScope::Global,
            kind: ConfigKind::Boolean,
            path: &["autoCompactEnabled"],
            options: None,
        },
        "autoMemoryEnabled" => ConfigSettingSpec {
            scope: ConfigScope::Settings,
            kind: ConfigKind::Boolean,
            path: &["autoMemoryEnabled"],
            options: None,
        },
        "autoDreamEnabled" => ConfigSettingSpec {
            scope: ConfigScope::Settings,
            kind: ConfigKind::Boolean,
            path: &["autoDreamEnabled"],
            options: None,
        },
        "fileCheckpointingEnabled" => ConfigSettingSpec {
            scope: ConfigScope::Global,
            kind: ConfigKind::Boolean,
            path: &["fileCheckpointingEnabled"],
            options: None,
        },
        "showTurnDuration" => ConfigSettingSpec {
            scope: ConfigScope::Global,
            kind: ConfigKind::Boolean,
            path: &["showTurnDuration"],
            options: None,
        },
        "terminalProgressBarEnabled" => ConfigSettingSpec {
            scope: ConfigScope::Global,
            kind: ConfigKind::Boolean,
            path: &["terminalProgressBarEnabled"],
            options: None,
        },
        "todoFeatureEnabled" => ConfigSettingSpec {
            scope: ConfigScope::Global,
            kind: ConfigKind::Boolean,
            path: &["todoFeatureEnabled"],
            options: None,
        },
        "model" => ConfigSettingSpec {
            scope: ConfigScope::Settings,
            kind: ConfigKind::String,
            path: &["model"],
            options: None,
        },
        "alwaysThinkingEnabled" => ConfigSettingSpec {
            scope: ConfigScope::Settings,
            kind: ConfigKind::Boolean,
            path: &["alwaysThinkingEnabled"],
            options: None,
        },
        "permissions.defaultMode" => ConfigSettingSpec {
            scope: ConfigScope::Settings,
            kind: ConfigKind::String,
            path: &["permissions", "defaultMode"],
            options: Some(&["default", "plan", "acceptEdits", "dontAsk", "auto"]),
        },
        "language" => ConfigSettingSpec {
            scope: ConfigScope::Settings,
            kind: ConfigKind::String,
            path: &["language"],
            options: None,
        },
        "teammateMode" => ConfigSettingSpec {
            scope: ConfigScope::Global,
            kind: ConfigKind::String,
            path: &["teammateMode"],
            options: Some(&["tmux", "in-process", "auto"]),
        },
        _ => return None,
    })
}

fn normalize_config_value(spec: ConfigSettingSpec, value: ConfigValue) -> Result<Value, String> {
    let normalized = match (spec.kind, value) {
        (ConfigKind::Boolean, ConfigValue::Bool(value)) => Value::Bool(value),
        (ConfigKind::Boolean, ConfigValue::String(value)) => {
            match value.trim().to_ascii_lowercase().as_str() {
                "true" => Value::Bool(true),
                "false" => Value::Bool(false),
                _ => return Err(String::from("setting requires true or false")),
            }
        }
        (ConfigKind::Boolean, ConfigValue::Number(_)) => {
            return Err(String::from("setting requires true or false"))
        }
        (ConfigKind::String, ConfigValue::String(value)) => Value::String(value),
        (ConfigKind::String, ConfigValue::Bool(value)) => Value::String(value.to_string()),
        (ConfigKind::String, ConfigValue::Number(value)) => json!(value),
    };

    if let Some(options) = spec.options {
        let Some(as_str) = normalized.as_str() else {
            return Err(String::from("setting requires a string value"));
        };
        if !options.iter().any(|option| option == &as_str) {
            return Err(format!(
                "Invalid value \"{as_str}\". Options: {}",
                options.join(", ")
            ));
        }
    }

    Ok(normalized)
}

fn config_file_for_scope(scope: ConfigScope) -> Result<PathBuf, String> {
    let cwd = std::env::current_dir().map_err(|error| error.to_string())?;
    Ok(match scope {
        ConfigScope::Global => config_home_dir()?.join("settings.json"),
        ConfigScope::Settings => cwd.join(".claw").join("settings.local.json"),
    })
}

fn config_home_dir() -> Result<PathBuf, String> {
    if let Ok(path) = std::env::var("CLAW_CONFIG_HOME") {
        return Ok(PathBuf::from(path));
    }
    let home = runtime::home_dir().ok_or_else(|| String::from("home directory not found"))?;
    Ok(home.join(".claw"))
}

fn read_json_object(path: &Path) -> Result<serde_json::Map<String, Value>, String> {
    match std::fs::read_to_string(path) {
        Ok(contents) => {
            if contents.trim().is_empty() {
                return Ok(serde_json::Map::new());
            }
            serde_json::from_str::<Value>(&contents)
                .map_err(|error| error.to_string())?
                .as_object()
                .cloned()
                .ok_or_else(|| String::from("config file must contain a JSON object"))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(serde_json::Map::new()),
        Err(error) => Err(error.to_string()),
    }
}

fn write_json_object(path: &Path, value: &serde_json::Map<String, Value>) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    std::fs::write(
        path,
        serde_json::to_string_pretty(value).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())
}

fn get_nested_value<'a>(
    value: &'a serde_json::Map<String, Value>,
    path: &[&str],
) -> Option<&'a Value> {
    let (first, rest) = path.split_first()?;
    let mut current = value.get(*first)?;
    for key in rest {
        current = current.as_object()?.get(*key)?;
    }
    Some(current)
}

fn set_nested_value(root: &mut serde_json::Map<String, Value>, path: &[&str], new_value: Value) {
    let (first, rest) = path
        .split_first()
        .expect("le chemin de configuration ne doit pas être vide");
    if rest.is_empty() {
        root.insert((*first).to_string(), new_value);
        return;
    }

    let entry = root
        .entry((*first).to_string())
        .or_insert_with(|| Value::Object(serde_json::Map::new()));
    if !entry.is_object() {
        *entry = Value::Object(serde_json::Map::new());
    }
    let map = entry.as_object_mut().expect("objet inséré");
    set_nested_value(map, rest, new_value);
}

fn iso8601_timestamp() -> String {
    if let Ok(output) = Command::new("date")
        .args(["-u", "+%Y-%m-%dT%H:%M:%SZ"])
        .output()
    {
        if output.status.success() {
            return String::from_utf8_lossy(&output.stdout).trim().to_string();
        }
    }
    iso8601_now()
}

#[allow(clippy::needless_pass_by_value)]
fn execute_powershell(input: PowerShellInput) -> std::io::Result<runtime::BashCommandOutput> {
    let _ = &input.description;
    let shell = detect_powershell_shell()?;
    let mut command = std::process::Command::new(shell);
    command
        .arg("-NoProfile")
        .arg("-NonInteractive")
        .arg("-Command")
        .arg(powershell_script(&input.command));
    // Python écrit en UTF-8 et au fil de l'eau, comme pour bash.
    for (name, value) in [("PYTHONIOENCODING", "utf-8"), ("PYTHONUNBUFFERED", "1")] {
        if std::env::var_os(name).is_none() {
            command.env(name, value);
        }
    }
    // Mêmes règles que bash : serveurs et surveillances en arrière-plan,
    // commande longue qui y continue, délai fixé respecté. La sortie est lue
    // au fil de l'eau (une grosse sortie ne bloque plus la commande), suivie
    // par BashOutput et arrêtée par KillShell.
    runtime::run_tool_command(
        command,
        &input.command,
        input.timeout,
        input.run_in_background,
    )
}

/// Début de chaque commande `PowerShell` : la sortie en UTF-8. Windows
/// `PowerShell` 5.1 écrit sinon dans le tube avec le code page de la console
/// (ibm850 sur un Windows français), et chaque lettre accentuée arrive abîmée.
const POWERSHELL_UTF8: &str = "try { [Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false) } catch {}; $OutputEncoding = [System.Text.UTF8Encoding]::new($false)\n";

/// La commande du modèle, prête pour `PowerShell`. Un `2>&1` final est retiré :
/// sous `PowerShell` 5.1, il change chaque ligne d'erreur d'un programme en
/// erreur de `PowerShell`, et une compilation réussie qui écrit un
/// avertissement sort en échec ; Claw garde déjà stderr à part. Sous Windows,
/// la sortie est d'abord mise en UTF-8 ; ailleurs, `pwsh` y écrit déjà.
fn powershell_script(command: &str) -> String {
    let trimmed = command.trim_end();
    let command = trimmed.strip_suffix("2>&1").map_or(trimmed, str::trim_end);
    if cfg!(windows) {
        format!("{POWERSHELL_UTF8}{command}")
    } else {
        command.to_string()
    }
}

fn detect_powershell_shell() -> std::io::Result<&'static str> {
    if command_exists("pwsh") {
        Ok("pwsh")
    } else if command_exists("powershell") {
        Ok("powershell")
    } else {
        Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "PowerShell executable not found (expected `pwsh` or `powershell` in PATH)",
        ))
    }
}

fn command_exists(command: &str) -> bool {
    runtime::find_executable(command).is_some()
}

fn resolve_cell_index(
    cells: &[serde_json::Value],
    cell_id: Option<&str>,
    edit_mode: NotebookEditMode,
) -> Result<usize, String> {
    if cells.is_empty()
        && matches!(
            edit_mode,
            NotebookEditMode::Replace | NotebookEditMode::Delete
        )
    {
        return Err(String::from("Notebook has no cells to edit"));
    }
    if let Some(cell_id) = cell_id {
        cells
            .iter()
            .position(|cell| cell.get("id").and_then(serde_json::Value::as_str) == Some(cell_id))
            .ok_or_else(|| format!("Cell id not found: {cell_id}"))
    } else {
        Ok(cells.len().saturating_sub(1))
    }
}

fn source_lines(source: &str) -> Vec<serde_json::Value> {
    if source.is_empty() {
        return vec![serde_json::Value::String(String::new())];
    }
    source
        .split_inclusive('\n')
        .map(|line| serde_json::Value::String(line.to_string()))
        .collect()
}

fn format_notebook_edit_mode(mode: NotebookEditMode) -> String {
    match mode {
        NotebookEditMode::Replace => String::from("replace"),
        NotebookEditMode::Insert => String::from("insert"),
        NotebookEditMode::Delete => String::from("delete"),
    }
}

fn make_cell_id(index: usize) -> String {
    format!("cell-{}", index + 1)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::collections::BTreeSet;
    use std::fs;
    use std::io::{Read, Write};
    use std::net::{SocketAddr, TcpListener};
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex, OnceLock};
    use std::thread;
    use std::time::Duration;

    use super::{
        agent_permission_policy, agent_permission_policy_with, allowed_tools_for_subagent,
        execute_agent_with_spawn, execute_tool, final_assistant_text, find_custom_agent,
        mvp_tool_specs, normalize_subagent_type, parse_agent_markdown,
        persist_agent_terminal_state, push_output_block, resolve_agent_definition,
        resolve_agent_model_for, AgentInput, AgentJob, SubagentContext, SubagentToolExecutor,
        DEFAULT_AGENT_MODEL,
    };
    use api::OutputContentBlock;
    use runtime::{
        ApiRequest, AssistantEvent, ConversationRuntime, PermissionMode, PermissionRules,
        RuntimeError, Session,
    };
    use serde_json::json;

    pub(crate) fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    fn temp_path(name: &str) -> PathBuf {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        std::env::temp_dir().join(format!("claw-tools-{unique}-{name}"))
    }

    #[test]
    fn a_sub_agent_stopped_without_a_report_hands_back_what_it_did() {
        use runtime::{ContentBlock, ConversationMessage};
        let call = |name: &str, input: &str| ContentBlock::ToolUse {
            id: "appel".to_string(),
            name: name.to_string(),
            input: input.to_string(),
        };
        let mut session = Session::new();
        session.messages.extend([
            ConversationMessage::user_text("Trouve où les montants sont arrondis".to_string()),
            ConversationMessage::assistant(vec![
                ContentBlock::Text {
                    text: "Je cherche l'arrondi.".to_string(),
                },
                call("grep_search", r#"{"pattern":"round("}"#),
            ]),
            ConversationMessage::assistant(vec![
                call("read_file", r#"{"path":"stats.py"}"#),
                call("read_file", r#"{"path":"stats.py"}"#),
                call("bash", r#"{"command":"python -m pytest -q"}"#),
            ]),
        ]);
        let notes = super::agent_work_notes(
            &session,
            "Explore",
            "stopped before writing its report (erreur 429)",
        )
        .expect("travail rendu");
        assert!(notes.starts_with("Je cherche l'arrondi."));
        assert!(notes.contains("Files read: stats.py\n"));
        assert!(notes.contains("Searches: round("));
        assert!(notes.contains("Commands run: python -m pytest -q"));
        assert!(notes.contains(
            "[Claw] The sub-agent Explore stopped before writing its report (erreur 429). What it had done so far is above. Do not start the same task again as it is"
        ));
        // Rien de fait : l'erreur reste telle quelle.
        assert!(super::agent_work_notes(&Session::new(), "Explore", "stopped").is_none());
    }

    #[test]
    fn isolated_agents_take_worktree_only_and_read_the_json_report() {
        assert_eq!(super::parse_isolation(None), Ok(false));
        assert_eq!(super::parse_isolation(Some("worktree")), Ok(true));
        assert!(super::parse_isolation(Some("docker")).is_err());
        let output = "journal\n{\"message\":\"J'ai corrigé src/a.rs\",\"model\":\"m\"}\n";
        assert_eq!(
            super::isolated_agent_report(output).as_deref(),
            Some("J'ai corrigé src/a.rs")
        );
        assert!(super::isolated_agent_report("{\"error\":\"quota\"}")
            .expect("erreur")
            .contains("failed: quota"));
        assert_eq!(super::isolated_agent_report("pas de json"), None);
    }

    #[test]
    fn an_isolated_worktree_keeps_its_changes_on_a_branch_or_disappears() {
        use runtime::worktree::{create, git};
        if git(&std::env::temp_dir(), &["--version"]).is_err() {
            return;
        }
        let repo = temp_path("isolation");
        fs::create_dir_all(&repo).expect("dossier");
        for args in [
            vec!["init", "-q", "-b", "main"],
            vec!["config", "user.email", "test@example.invalid"],
            vec!["config", "user.name", "test"],
            vec!["config", "core.autocrlf", "false"],
        ] {
            git(&repo, &args).expect("git");
        }
        fs::write(repo.join("a.txt"), "1\n").expect("fichier");
        git(&repo, &["add", "."]).expect("add");
        git(&repo, &["commit", "-q", "-m", "départ"]).expect("commit");

        let busy = create(&repo, "agent-ecrit", None).expect("worktree");
        fs::write(busy.path.join("a.txt"), "2\n").expect("modif");
        let note = super::finish_isolated_worktree(&repo, &busy, "ecrit", "change a.txt");
        assert!(
            note.contains("committed on branch `claw/agent-ecrit`"),
            "{note}"
        );
        assert!(note.contains("git merge claw/agent-ecrit"), "{note}");
        let log = git(&repo, &["log", "--format=%s", "claw/agent-ecrit", "-1"]).expect("log");
        assert_eq!(log.trim(), "Sous-agent ecrit : change a.txt");
        // La copie de travail principale n'a pas bougé.
        assert_eq!(fs::read_to_string(repo.join("a.txt")).expect("lu"), "1\n");

        let idle = create(&repo, "agent-lit", None).expect("worktree");
        let note = super::finish_isolated_worktree(&repo, &idle, "lit", "lit seulement");
        assert!(note.contains("changed no file"), "{note}");
        assert!(!idle.path.exists());
        assert!(git(
            &repo,
            &[
                "rev-parse",
                "--verify",
                "--quiet",
                "refs/heads/claw/agent-lit"
            ]
        )
        .is_err());

        let _ = git(
            &repo,
            &[
                "worktree",
                "remove",
                "--force",
                &busy.path.display().to_string(),
            ],
        );
        let _ = fs::remove_dir_all(repo);
    }

    #[test]
    fn exposes_mvp_tools() {
        let names = mvp_tool_specs()
            .into_iter()
            .map(|spec| spec.name)
            .collect::<Vec<_>>();
        assert!(names.contains(&"bash"));
        assert!(names.contains(&"read_file"));
        assert!(names.contains(&"WebFetch"));
        assert!(names.contains(&"WebSearch"));
        assert!(names.contains(&"TodoWrite"));
        assert!(names.contains(&"Skill"));
        assert!(names.contains(&"Agent"));
        assert!(names.contains(&"ToolSearch"));
        assert!(names.contains(&"NotebookEdit"));
        assert!(names.contains(&"Sleep"));
        assert!(names.contains(&"SendUserMessage"));
        assert!(names.contains(&"Config"));
        assert!(names.contains(&"StructuredOutput"));
        assert!(names.contains(&"REPL"));
        assert!(names.contains(&"PowerShell"));
    }

    #[test]
    fn a_sub_agent_does_not_run_a_call_without_its_arguments() {
        use runtime::ToolExecutor;
        let mut executor = SubagentToolExecutor::new(BTreeSet::from(["edit_file".to_string()]));
        let problem = executor
            .invalid_input("edit_file", "{}")
            .expect("appel sans arguments");
        assert!(problem.contains("It needs: path, old_string, new_string"));
        assert_eq!(
            executor.invalid_input(
                "edit_file",
                r#"{"path":"a.py","old_string":"x","new_string":"y"}"#
            ),
            None
        );
        // Outil sans argument obligatoire, ou inconnu : rien à redire ici.
        assert_eq!(executor.invalid_input("TodoRead", "{}"), None);
    }

    #[test]
    fn rejects_unknown_tool_names() {
        let error = execute_tool("nope", &json!({})).expect_err("tool should be rejected");
        assert!(error.contains("unsupported tool"));
    }

    #[test]
    fn web_fetch_returns_prompt_aware_summary() {
        let server = TestServer::spawn(Arc::new(|request_line: &str| {
            assert!(request_line.starts_with("GET /page "));
            HttpResponse::html(
                200,
                "OK",
                "<html><head><title>Ignored</title></head><body><h1>Test Page</h1><p>Hello <b>world</b> from local server.</p></body></html>",
            )
        }));

        let result = execute_tool(
            "WebFetch",
            &json!({
                "url": format!("http://{}/page", server.addr()),
                "prompt": "Summarize this page"
            }),
        )
        .expect("WebFetch should succeed");

        let output: serde_json::Value = serde_json::from_str(&result).expect("valid json");
        assert_eq!(output["code"], 200);
        let summary = output["result"].as_str().expect("result string");
        assert!(summary.contains("Fetched"));
        assert!(summary.contains("Test Page"));
        assert!(summary.contains("Hello world from local server"));

        let titled = execute_tool(
            "WebFetch",
            &json!({
                "url": format!("http://{}/page", server.addr()),
                "prompt": "What is the page title?"
            }),
        )
        .expect("WebFetch title query should succeed");
        let titled_output: serde_json::Value = serde_json::from_str(&titled).expect("valid json");
        let titled_summary = titled_output["result"].as_str().expect("result string");
        assert!(titled_summary.contains("Title: Ignored"));
    }

    #[test]
    fn web_fetch_supports_plain_text_and_rejects_invalid_url() {
        let server = TestServer::spawn(Arc::new(|request_line: &str| {
            assert!(request_line.starts_with("GET /plain "));
            HttpResponse::text(200, "OK", "plain text response")
        }));

        let result = execute_tool(
            "WebFetch",
            &json!({
                "url": format!("http://{}/plain", server.addr()),
                "prompt": "Show me the content"
            }),
        )
        .expect("WebFetch should succeed for text content");

        let output: serde_json::Value = serde_json::from_str(&result).expect("valid json");
        assert_eq!(output["url"], format!("http://{}/plain", server.addr()));
        assert!(output["result"]
            .as_str()
            .expect("result")
            .contains("plain text response"));

        let error = execute_tool(
            "WebFetch",
            &json!({
                "url": "not a url",
                "prompt": "Summarize"
            }),
        )
        .expect_err("invalid URL should fail");
        assert!(error.contains("relative URL without a base") || error.contains("invalid"));
    }

    #[test]
    fn web_fetch_completes_addresses_and_keeps_local_servers_on_http() {
        let normalized = |url: &str| super::normalize_fetch_url(url).expect("adresse");
        assert_eq!(
            normalized("docs.python.org/3/"),
            "https://docs.python.org/3/"
        );
        assert_eq!(
            normalized("localhost:8000/api"),
            "http://localhost:8000/api"
        );
        assert_eq!(normalized("http://0.0.0.0:8000/"), "http://127.0.0.1:8000/");
        assert_eq!(
            normalized("http://192.168.1.20:3000/"),
            "http://192.168.1.20:3000/"
        );
        assert_eq!(normalized("http://monposte:8080/"), "http://monposte:8080/");
        assert_eq!(normalized("http://example.com/a"), "https://example.com/a");
    }

    #[test]
    fn web_fetch_says_when_nothing_listens_or_the_page_is_an_error() {
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .expect("port libre")
            .local_addr()
            .expect("adresse")
            .port();
        let error = execute_tool(
            "WebFetch",
            &json!({ "url": format!("http://127.0.0.1:{port}/"), "prompt": "page" }),
        )
        .expect_err("rien n'écoute");
        assert!(
            error.contains(&format!(
                "Nothing answers on 127.0.0.1:{port} (connection refused)"
            )),
            "{error}"
        );
        assert!(error.contains("BashOutput"));

        let server = TestServer::spawn(Arc::new(|_: &str| {
            HttpResponse::text(404, "Not Found", "Page not found")
        }));
        let result = execute_tool(
            "WebFetch",
            &json!({ "url": format!("http://{}/docs", server.addr()), "prompt": "docs" }),
        )
        .expect("une page d'erreur est rendue");
        let output: serde_json::Value = serde_json::from_str(&result).expect("json");
        let text = output["result"].as_str().expect("texte");
        assert!(text
            .starts_with("[Claw] The server answered 404 Not Found: this address does not exist."));
        assert!(text.contains("Page not found"));
    }

    #[test]
    fn web_fetch_reads_a_long_page_by_passages_then_in_order() {
        use std::fmt::Write as _;
        let server = TestServer::spawn(Arc::new(|request_line: &str| {
            assert!(request_line.starts_with("GET /long "));
            let mut body = String::from(
                "<html><head><title>Tarifs</title><script>var x = '<p>piège</p>';</script></head><body><h1>Tarifs</h1>",
            );
            for section in 0..200 {
                let text = if section == 150 {
                    "Le plan Experiment est gratuit. "
                } else {
                    "Paragraphe ordinaire de remplissage. "
                };
                let _ = write!(body, "<h2>Section {section}</h2><p>{}</p>", text.repeat(4));
            }
            body += "</body></html>";
            HttpResponse::html(200, "OK", &body)
        }));
        let fetch = |offset: serde_json::Value| {
            let mut input = json!({
                "url": format!("http://{}/long", server.addr()),
                "prompt": "Is the Experiment plan free?"
            });
            if !offset.is_null() {
                input["offset"] = offset;
            }
            let result = execute_tool("WebFetch", &input).expect("WebFetch should succeed");
            let output: serde_json::Value = serde_json::from_str(&result).expect("valid json");
            output["result"].as_str().expect("result").to_string()
        };

        // Sans position : le passage cherché, loin dans la page, est rendu.
        let passages = fetch(serde_json::Value::Null);
        assert!(passages.contains("Title: Tarifs"));
        assert!(passages.contains("the passages that match your prompt"));
        assert!(passages.contains("## Section 150\n\nLe plan Experiment est gratuit."));
        assert!(!passages.contains("piège"));

        // Avec une position, même écrite en texte : lecture suivie.
        let first = fetch(json!("0"));
        assert!(first.contains("This is the part from 0 to "));
        assert!(first.contains("# Tarifs\n\n## Section 0\n\nParagraphe ordinaire"));
        let next: usize = first
            .split("\"offset\": ")
            .nth(1)
            .and_then(|rest| rest.split('.').next())
            .and_then(|number| number.parse().ok())
            .expect("position de la suite");
        let second = fetch(json!(next));
        assert!(second.contains(&format!("This is the part from {next} to ")));
        assert!(!second.contains("## Section 0\n"));
    }

    #[test]
    fn a_powershell_command_writes_utf8_and_keeps_its_exit_code() {
        let script = super::powershell_script("cd frontend; npm run build 2>&1  ");
        assert!(script.ends_with("cd frontend; npm run build"), "{script}");
        // Sous Windows seulement, la sortie est d'abord mise en UTF-8.
        let expected = if cfg!(windows) {
            format!("{}Get-ChildItem", super::POWERSHELL_UTF8)
        } else {
            "Get-ChildItem".to_string()
        };
        assert_eq!(super::powershell_script("Get-ChildItem"), expected);
    }

    #[test]
    fn look_at_image_asks_the_looker_given_by_the_application() {
        super::set_image_looker(|image, question| Ok(format!("{image} -> {question}")));
        let answer = execute_tool(
            "LookAtImage",
            &json!({ "image": "ab12cd34", "question": "quel texte sur le bouton ?" }),
        )
        .expect("réponse");
        assert_eq!(answer, "ab12cd34 -> quel texte sur le bouton ?");
        let error = execute_tool(
            "LookAtImage",
            &json!({ "image": "ab12cd34", "question": "  " }),
        )
        .expect_err("question vide");
        assert!(error.contains("question is empty"), "{error}");
    }

    /// Avec un vrai navigateur (`CLAW_TEST_BROWSER=1`) : une page piégée, prise
    /// au téléphone et à l'ordinateur ; chaque défaut est mesuré, les captures
    /// partent en images, un clic sur un bouton recouvert le dit.
    #[test]
    fn screenshot_measures_layout_problems_and_returns_images() {
        if std::env::var_os("CLAW_TEST_BROWSER").is_none() || !super::page_viewer_available() {
            return;
        }
        let server = TestServer::spawn(Arc::new(|request_line: &str| {
            if request_line.contains("absente") {
                return HttpResponse::text(404, "Not Found", "absente");
            }
            HttpResponse::html(
                200,
                "OK",
                r#"<!doctype html><html><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1"><title>Page piégée</title>
<style>
body { margin: 0; font: 16px sans-serif; background: #fff; }
.wide { width: 600px; height: 20px; background: #ddd; }
.tabs { display: flex; width: 200px; }
.tab { width: 60px; overflow: hidden; white-space: nowrap; text-overflow: ellipsis; }
.pale { color: #d0d0d0; }
.chip { display: inline-flex; }
#menu { position: absolute; top: 120px; left: 10px; }
#over { position: absolute; top: 120px; left: 20px; }
</style></head><body>
<div class="wide">large</div>
<div class="tabs"><button class="tab">Modifications</button><button class="tab">Fichiers</button></div>
<p class="pale">Texte pâle sur fond blanc</p>
<button class="chip" hidden>Choisir</button>
<button id="menu">Menu</button><button id="over">Par-dessus</button>
<img src="/absente.png" alt="absente">
<script>console.error("erreur de test");</script>
</body></html>"#,
            )
        }));
        let _ = super::take_screenshots();
        let result = execute_tool(
            "Screenshot",
            &json!({
                "url": format!("http://{}/", server.addr()),
                "actions": [{ "click": "#menu" }],
                "wait_ms": 300
            }),
        )
        .expect("Screenshot should succeed");
        let output: serde_json::Value = serde_json::from_str(&result).expect("valid json");
        let phone = &output["captures"][0];
        assert_eq!(phone["screen"], "phone 390x844", "{output}");
        assert_eq!(phone["title"], "Page piégée", "{output}");
        let problems = phone["problems"].to_string();
        for expected in [
            "past the edge",
            "div.wide",
            "Text cut off",
            "Modifications",
            "on top of each other",
            "hidden attribute",
            "button.chip",
            "did not load",
            "too little contrast",
        ] {
            assert!(problems.contains(expected), "{expected}: {output}");
        }
        assert!(
            phone["console"].to_string().contains("erreur de test"),
            "{output}"
        );
        assert!(
            phone["actions"][0]
                .as_str()
                .is_some_and(|line| line.contains("button#over lies on top")),
            "{output}"
        );
        // À l'ordinateur, la page tient en largeur.
        let desktop = &output["captures"][1];
        assert_eq!(desktop["screen"], "desktop 1280x800", "{output}");
        assert!(
            !desktop["problems"].to_string().contains("past the edge"),
            "{output}"
        );
        let images = super::take_screenshots();
        assert_eq!(images.len(), 2);
        assert!(images.iter().all(|image| image.media_type == "image/jpeg"
            && image.data.starts_with("/9j/")
            && image.path.is_file()));
        assert!(super::take_screenshots().is_empty());
    }

    /// Avec un vrai navigateur (`CLAW_TEST_BROWSER=1`) : la page rend son état
    /// après exécution du JavaScript, et sa console.
    #[test]
    fn view_page_shows_what_the_browser_displays_and_its_console() {
        if std::env::var_os("CLAW_TEST_BROWSER").is_none() || !super::page_viewer_available() {
            return;
        }
        // Une page qui se dit « Hors ligne » parce que son API ne répond pas,
        // et dont un script lève une exception : ce qu'un curl ne montre pas.
        let server = TestServer::spawn(Arc::new(|_request_line: &str| {
            HttpResponse::html(
                200,
                "OK",
                r#"<!doctype html><html><head><meta charset="utf-8"><title>Essai de page</title></head>
<body><h1>Manga Translator</h1><p id="etat">chargement</p>
<script>
fetch("http://127.0.0.1:9/api/v1/health").then(function () {
  document.getElementById("etat").textContent = "En ligne";
}).catch(function (e) {
  document.getElementById("etat").textContent = "Hors ligne";
  console.error("health en echec : " + e.message);
});
setTimeout(function () { nexistepas(); }, 100);
</script></body></html>"#,
            )
        }));
        let result = execute_tool(
            "ViewPage",
            &json!({ "url": format!("http://{}/", server.addr()) }),
        )
        .expect("ViewPage should succeed");
        let output: serde_json::Value = serde_json::from_str(&result).expect("valid json");
        assert_eq!(output["title"], "Essai de page", "{output}");
        let text = output["text"].as_str().expect("texte");
        assert!(
            text.contains("Manga Translator") && text.contains("Hors ligne"),
            "{output}"
        );
        assert!(!text.contains("chargement"), "{output}");
        let console = output["console"].to_string();
        assert!(console.contains("health en echec"), "{output}");
        assert!(console.contains("nexistepas is not defined"), "{output}");
        assert!(
            output["consoleErrors"].as_u64().unwrap_or(0) >= 2,
            "{output}"
        );
    }

    #[test]
    fn web_search_extracts_and_filters_results() {
        let server = TestServer::spawn(Arc::new(|request_line: &str| {
            assert!(request_line.contains("GET /search?q=rust+web+search "));
            HttpResponse::html(
                200,
                "OK",
                r#"
                <html><body>
                  <a class="result__a" href="https://docs.rs/reqwest">Reqwest docs</a>
                  <a class="result__a" href="https://example.com/blocked">Blocked result</a>
                </body></html>
                "#,
            )
        }));

        std::env::set_var(
            "CLAW_WEB_SEARCH_BASE_URL",
            format!("http://{}/search", server.addr()),
        );
        let result = execute_tool(
            "WebSearch",
            &json!({
                "query": "rust web search",
                "allowed_domains": ["https://DOCS.rs/"],
                "blocked_domains": ["HTTPS://EXAMPLE.COM"]
            }),
        )
        .expect("WebSearch should succeed");
        std::env::remove_var("CLAW_WEB_SEARCH_BASE_URL");

        let output: serde_json::Value = serde_json::from_str(&result).expect("valid json");
        assert_eq!(output["query"], "rust web search");
        let results = output["results"].as_array().expect("results array");
        let search_result = results
            .iter()
            .find(|item| item.get("content").is_some())
            .expect("search result block present");
        let content = search_result["content"].as_array().expect("content array");
        assert_eq!(content.len(), 1);
        assert_eq!(content[0]["title"], "Reqwest docs");
        assert_eq!(content[0]["url"], "https://docs.rs/reqwest");
    }

    #[test]
    fn web_search_handles_generic_links_and_invalid_base_url() {
        let _guard = env_lock()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let server = TestServer::spawn(Arc::new(|request_line: &str| {
            assert!(request_line.contains("GET /fallback?q=generic+links "));
            HttpResponse::html(
                200,
                "OK",
                r#"
                <html><body>
                  <a href="https://example.com/one">Example One</a>
                  <a href="https://example.com/one">Duplicate Example One</a>
                  <a href="https://docs.rs/tokio">Tokio Docs</a>
                </body></html>
                "#,
            )
        }));

        std::env::set_var(
            "CLAW_WEB_SEARCH_BASE_URL",
            format!("http://{}/fallback", server.addr()),
        );
        let result = execute_tool(
            "WebSearch",
            &json!({
                "query": "generic links"
            }),
        )
        .expect("WebSearch fallback parsing should succeed");
        std::env::remove_var("CLAW_WEB_SEARCH_BASE_URL");

        let output: serde_json::Value = serde_json::from_str(&result).expect("valid json");
        let results = output["results"].as_array().expect("results array");
        let search_result = results
            .iter()
            .find(|item| item.get("content").is_some())
            .expect("search result block present");
        let content = search_result["content"].as_array().expect("content array");
        assert_eq!(content.len(), 2);
        assert_eq!(content[0]["url"], "https://example.com/one");
        assert_eq!(content[1]["url"], "https://docs.rs/tokio");

        std::env::set_var("CLAW_WEB_SEARCH_BASE_URL", "://bad-base-url");
        let error = execute_tool("WebSearch", &json!({ "query": "generic links" }))
            .expect_err("invalid base URL should fail");
        std::env::remove_var("CLAW_WEB_SEARCH_BASE_URL");
        assert!(error.contains("relative URL without a base") || error.contains("empty host"));
    }

    #[test]
    fn pending_tools_preserve_multiple_streaming_tool_calls_by_index() {
        let mut events = Vec::new();
        let mut pending_tools = BTreeMap::new();

        push_output_block(
            OutputContentBlock::ToolUse {
                id: "tool-1".to_string(),
                name: "read_file".to_string(),
                input: json!({}),
            },
            1,
            &mut events,
            &mut pending_tools,
            true,
        );
        push_output_block(
            OutputContentBlock::ToolUse {
                id: "tool-2".to_string(),
                name: "grep_search".to_string(),
                input: json!({}),
            },
            2,
            &mut events,
            &mut pending_tools,
            true,
        );

        pending_tools
            .get_mut(&1)
            .expect("first tool pending")
            .2
            .push_str("{\"path\":\"src/main.rs\"}");
        pending_tools
            .get_mut(&2)
            .expect("second tool pending")
            .2
            .push_str("{\"pattern\":\"TODO\"}");

        assert_eq!(
            pending_tools.remove(&1),
            Some((
                "tool-1".to_string(),
                "read_file".to_string(),
                "{\"path\":\"src/main.rs\"}".to_string(),
            ))
        );
        assert_eq!(
            pending_tools.remove(&2),
            Some((
                "tool-2".to_string(),
                "grep_search".to_string(),
                "{\"pattern\":\"TODO\"}".to_string(),
            ))
        );
    }

    #[test]
    fn todo_list_lives_under_dot_claw_and_the_old_one_is_moved_there() {
        let root = temp_path("todo-rangement");
        std::fs::create_dir_all(&root).expect("dossier");
        let store = root.join(".claw").join("todos.json");
        assert_eq!(super::todo_store_in(&root), store);
        assert!(!store.exists());

        // Liste laissée à la racine par une ancienne version : déplacée, pas perdue.
        std::fs::write(root.join(".claw-todos.json"), "[]").expect("ancienne liste");
        assert_eq!(super::todo_store_in(&root), store);
        assert_eq!(std::fs::read_to_string(&store).expect("liste"), "[]");
        assert!(!root.join(".claw-todos.json").exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn todo_write_persists_and_returns_previous_state() {
        let _guard = env_lock()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let path = temp_path("todos.json");
        std::env::set_var("CLAW_TODO_STORE", &path);

        let first = execute_tool(
            "TodoWrite",
            &json!({
                "todos": [
                    {"content": "Add tool", "activeForm": "Adding tool", "status": "in_progress"},
                    {"content": "Run tests", "activeForm": "Running tests", "status": "pending"}
                ]
            }),
        )
        .expect("TodoWrite should succeed");
        let first_output: serde_json::Value = serde_json::from_str(&first).expect("valid json");
        assert_eq!(first_output["oldTodos"].as_array().expect("array").len(), 0);

        let second = execute_tool(
            "TodoWrite",
            &json!({
                "todos": [
                    {"content": "Add tool", "activeForm": "Adding tool", "status": "completed"},
                    {"content": "Run tests", "activeForm": "Running tests", "status": "completed"},
                    {"content": "Verify", "activeForm": "Verifying", "status": "completed"}
                ]
            }),
        )
        .expect("TodoWrite should succeed");
        std::env::remove_var("CLAW_TODO_STORE");
        let _ = std::fs::remove_file(path);

        let second_output: serde_json::Value = serde_json::from_str(&second).expect("valid json");
        assert_eq!(
            second_output["oldTodos"].as_array().expect("array").len(),
            2
        );
        assert_eq!(
            second_output["newTodos"].as_array().expect("array").len(),
            3
        );
        assert!(second_output["verificationNudgeNeeded"].is_null());
    }

    #[test]
    fn todo_write_rejects_invalid_payloads_and_sets_verification_nudge() {
        let _guard = env_lock()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let path = temp_path("todos-errors.json");
        std::env::set_var("CLAW_TODO_STORE", &path);

        let empty = execute_tool("TodoWrite", &json!({ "todos": [] }))
            .expect_err("empty todos should fail");
        assert!(empty.contains("an empty list is not accepted"), "{empty}");
        assert!(empty.contains("\"status\": \"completed\""), "{empty}");

        // Une liste écrite librement : envoyée comme texte, clés voisines,
        // statut d'un autre outil, texte seul sans activeForm.
        let loose = execute_tool(
            "TodoWrite",
            &json!({ "todos": "[{\"task\": \"Lire le code\", \"state\": \"done\"}, \"Ecrire le test\"]" }),
        )
        .expect("liste lue");
        let loose: serde_json::Value = serde_json::from_str(&loose).expect("json");
        assert_eq!(loose["newTodos"][0]["content"], "Lire le code");
        assert_eq!(loose["newTodos"][0]["status"], "completed");
        assert_eq!(loose["newTodos"][1]["activeForm"], "Ecrire le test");
        assert_eq!(loose["newTodos"][1]["status"], "pending");
        // Un fichier de suivi abîmé ne bloque plus l'outil.
        std::fs::write(&path, "").expect("fichier vide");
        execute_tool(
            "TodoWrite",
            &json!({ "todos": [{ "content": "A", "status": "pending" }] }),
        )
        .expect("repart d'une liste vide");
        // Un statut inconnu : la tâche est nommée, la forme rappelée.
        let unknown = execute_tool(
            "TodoWrite",
            &json!({ "todos": [{ "content": "A", "status": "blocked" }] }),
        )
        .expect_err("statut inconnu");
        assert!(
            unknown.contains("task 1 has the status \"blocked\""),
            "{unknown}"
        );

        // Plusieurs éléments in_progress sont désormais autorisés pour les workflows parallèles
        let _multi_active = execute_tool(
            "TodoWrite",
            &json!({
                "todos": [
                    {"content": "One", "activeForm": "Doing one", "status": "in_progress"},
                    {"content": "Two", "activeForm": "Doing two", "status": "in_progress"}
                ]
            }),
        )
        .expect("multiple in-progress todos should succeed");

        let blank_content = execute_tool(
            "TodoWrite",
            &json!({
                "todos": [
                    {"content": "   ", "activeForm": "Doing it", "status": "pending"}
                ]
            }),
        )
        .expect_err("blank content should fail");
        assert!(
            blank_content.contains("task 1 has no \"content\""),
            "{blank_content}"
        );

        let nudge = execute_tool(
            "TodoWrite",
            &json!({
                "todos": [
                    {"content": "Write tests", "activeForm": "Writing tests", "status": "completed"},
                    {"content": "Fix errors", "activeForm": "Fixing errors", "status": "completed"},
                    {"content": "Ship branch", "activeForm": "Shipping branch", "status": "completed"}
                ]
            }),
        )
        .expect("completed todos should succeed");
        std::env::remove_var("CLAW_TODO_STORE");
        let _ = fs::remove_file(path);

        let output: serde_json::Value = serde_json::from_str(&nudge).expect("valid json");
        assert_eq!(output["verificationNudgeNeeded"], true);
    }

    #[test]
    fn cron_tools_create_list_and_delete_tasks() {
        let created = execute_tool(
            "CronCreate",
            &json!({"cron": "0 9 * * 1-5", "prompt": "check the nightly build", "recurring": true}),
        )
        .expect("CronCreate");
        let id = serde_json::from_str::<serde_json::Value>(&created).expect("json")["scheduled"]
            ["id"]
            .as_str()
            .expect("id")
            .to_string();
        let listed = execute_tool("CronList", &json!({})).expect("CronList");
        assert!(listed.contains(&id), "{listed}");
        assert!(execute_tool("CronCreate", &json!({"cron": "0 9 * *", "prompt": "x"})).is_err());
        let deleted = execute_tool("CronDelete", &json!({"id": id})).expect("CronDelete");
        assert!(deleted.contains("nightly"), "{deleted}");
        assert!(!execute_tool("CronList", &json!({}))
            .expect("CronList")
            .contains(&id));
    }

    #[test]
    fn skills_render_their_arguments_and_refuse_user_only_ones() {
        let _guard = env_lock()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let workspace = temp_path("skill-render");
        let skills = workspace.join(".claude").join("skills");
        std::fs::create_dir_all(skills.join("fix-issue")).expect("skill dir");
        std::fs::write(
            skills.join("fix-issue").join("SKILL.md"),
            "---
description: Fix an issue
---
Fix GitHub issue $0 in ${CLAUDE_SKILL_DIR}.
",
        )
        .expect("skill file");
        std::fs::create_dir_all(skills.join("deploy")).expect("skill dir");
        std::fs::write(
            skills.join("deploy").join("SKILL.md"),
            "---
description: Deploy
disable-model-invocation: true
---
Deploy $ARGUMENTS
",
        )
        .expect("skill file");
        let original_dir = std::env::current_dir().expect("cwd");
        std::env::set_current_dir(&workspace).expect("set cwd");

        let result = execute_tool("Skill", &json!({ "skill": "fix-issue", "args": "123" }));
        let refused = execute_tool("Skill", &json!({ "skill": "deploy", "args": "prod" }));
        std::env::set_current_dir(original_dir).expect("restore cwd");

        let output: serde_json::Value =
            serde_json::from_str(&result.expect("Skill should succeed")).expect("valid json");
        let prompt = output["prompt"]
            .as_str()
            .expect("prompt")
            .replace('\\', "/");
        assert!(prompt.starts_with("Fix GitHub issue 123 in "), "{prompt}");
        assert!(prompt.ends_with("/skills/fix-issue."), "{prompt}");
        let refused = refused.expect_err("réservé à l'utilisateur");
        assert!(
            refused.contains("can only be run by the user") && refused.contains("/deploy"),
            "{refused}"
        );
        let _ = std::fs::remove_dir_all(workspace);
    }

    #[test]
    fn skill_loads_local_skill_prompt() {
        let _guard = env_lock()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let workspace = temp_path("skill-workspace");
        let skill_dir = workspace.join(".claw").join("skills").join("help");
        std::fs::create_dir_all(&skill_dir).expect("skill dir");
        std::fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: help\ndescription: Guide on using oh-my-codex plugin\n---\n# Help\nGuide on using oh-my-codex plugin.\n",
        )
        .expect("skill file");
        let original_dir = std::env::current_dir().expect("cwd");
        std::env::set_current_dir(workspace.join(".claw")).expect("set cwd");
        let result = execute_tool(
            "Skill",
            &json!({
                "skill": "help",
                "args": "overview"
            }),
        )
        .expect("Skill should succeed");

        let output: serde_json::Value = serde_json::from_str(&result).expect("valid json");
        assert_eq!(output["skill"], "help");
        assert!(output["path"]
            .as_str()
            .expect("path")
            .replace('\\', "/")
            .ends_with("/help/SKILL.md"));
        assert!(output["prompt"]
            .as_str()
            .expect("prompt")
            .contains("Guide on using oh-my-codex plugin"));

        let dollar_result = execute_tool(
            "Skill",
            &json!({
                "skill": "$help"
            }),
        )
        .expect("Skill should accept $skill invocation form");
        let dollar_output: serde_json::Value =
            serde_json::from_str(&dollar_result).expect("valid json");
        assert_eq!(dollar_output["skill"], "$help");
        std::env::set_current_dir(original_dir).expect("restore cwd");
        let _ = std::fs::remove_dir_all(workspace);
        assert!(dollar_output["path"]
            .as_str()
            .expect("path")
            .replace('\\', "/")
            .ends_with("/help/SKILL.md"));
    }

    #[test]
    fn skill_loads_skills_synced_by_the_claude_app() {
        let _guard = env_lock()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let home = temp_path("skill-home");
        let workspace = temp_path("skill-app-workspace");
        let synced = home
            .join(".claude")
            .join("skills")
            .join("synced")
            .join("compte");
        for (name, header) in [
            ("pdf", "description: Travailler sur des PDF\n"),
            (
                "computer-use",
                "description: Ordinateur\ncompatibility: \"Claude desktop app\"\n",
            ),
        ] {
            std::fs::create_dir_all(synced.join(name)).expect("skill dir");
            std::fs::write(
                synced.join(name).join("SKILL.md"),
                format!("---\nname: {name}\n{header}---\nLance scripts/outil.py\n"),
            )
            .expect("skill file");
        }
        std::fs::create_dir_all(&workspace).expect("workspace");
        let saved: Vec<(&str, Option<std::ffi::OsString>)> =
            ["HOME", "USERPROFILE", "CLAW_NO_CLAUDE_CONFIG"]
                .into_iter()
                .map(|key| (key, std::env::var_os(key)))
                .collect();
        std::env::set_var("HOME", &home);
        std::env::set_var("USERPROFILE", &home);
        std::env::remove_var("CLAW_NO_CLAUDE_CONFIG");
        let original_dir = std::env::current_dir().expect("cwd");
        std::env::set_current_dir(&workspace).expect("set cwd");

        let loaded = execute_tool("Skill", &json!({ "skill": "pdf" }));
        let refused = execute_tool("Skill", &json!({ "skill": "computer-use" }));
        std::env::set_var("CLAW_NO_CLAUDE_CONFIG", "1");
        let disabled = execute_tool("Skill", &json!({ "skill": "pdf" }));

        std::env::set_current_dir(original_dir).expect("restore cwd");
        for (key, value) in saved {
            match value {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
        }
        let _ = std::fs::remove_dir_all(&home);
        let _ = std::fs::remove_dir_all(&workspace);

        let output: serde_json::Value =
            serde_json::from_str(&loaded.expect("Skill pdf")).expect("valid json");
        assert!(output["path"]
            .as_str()
            .expect("path")
            .replace('\\', "/")
            .ends_with("/synced/compte/pdf/SKILL.md"));
        assert!(output["prompt"]
            .as_str()
            .expect("prompt")
            .contains("scripts/outil.py"));
        // Prévue pour l'application de bureau : non proposée.
        assert!(refused.is_err());
        // CLAW_NO_CLAUDE_CONFIG=1 coupe l'import.
        assert!(disabled.is_err());
    }

    #[test]
    fn tool_search_supports_keyword_and_select_queries() {
        let keyword = execute_tool(
            "ToolSearch",
            &json!({"query": "web current", "max_results": 3}),
        )
        .expect("ToolSearch should succeed");
        let keyword_output: serde_json::Value = serde_json::from_str(&keyword).expect("valid json");
        let matches = keyword_output["matches"].as_array().expect("matches");
        assert!(matches.iter().any(|value| value == "WebSearch"));

        let selected = execute_tool("ToolSearch", &json!({"query": "select:Agent,Skill"}))
            .expect("ToolSearch should succeed");
        let selected_output: serde_json::Value =
            serde_json::from_str(&selected).expect("valid json");
        assert_eq!(selected_output["matches"][0], "Agent");
        assert_eq!(selected_output["matches"][1], "Skill");

        // Leçons : une ligne par note, sans doublon ni secret.
        let lessons = temp_path("lessons.md");
        std::env::set_var("CLAW_LESSONS_FILE", &lessons);
        let saved = execute_tool(
            "Remember",
            &json!({ "note": "Les tests se lancent\navec  cargo test -q" }),
        )
        .expect("leçon notée");
        assert!(saved.contains("\"saved\": true"));
        let again = execute_tool(
            "Remember",
            &json!({ "note": "les tests se lancent avec cargo test -q" }),
        )
        .expect("doublon");
        assert!(again.contains("already known"));
        assert!(execute_tool("Remember", &json!({ "note": "api_key=abc" })).is_err());
        assert!(execute_tool("Remember", &json!({ "note": "   " })).is_err());
        assert_eq!(
            std::fs::read_to_string(&lessons).expect("fichier"),
            "- Les tests se lancent avec cargo test -q\n"
        );
        std::env::remove_var("CLAW_LESSONS_FILE");
        let _ = std::fs::remove_file(&lessons);

        let registry = super::GlobalToolRegistry::builtin();
        assert_eq!(registry.resolve_tool_name("bash").as_deref(), Some("bash"));
        assert_eq!(registry.resolve_tool_name("Bash").as_deref(), Some("bash"));
        assert_eq!(
            registry.resolve_tool_name("Read").as_deref(),
            Some("read_file")
        );
        assert_eq!(
            registry.resolve_tool_name("MultiEdit").as_deref(),
            Some("edit_file")
        );
        assert_eq!(
            registry.resolve_tool_name("GREP").as_deref(),
            Some("grep_search")
        );
        assert_eq!(
            registry.resolve_tool_name("todowrite").as_deref(),
            Some("TodoWrite")
        );
        assert_eq!(registry.resolve_tool_name("inconnu"), None);

        let aliased = execute_tool("ToolSearch", &json!({"query": "AgentTool"}))
            .expect("ToolSearch should support tool aliases");
        let aliased_output: serde_json::Value = serde_json::from_str(&aliased).expect("valid json");
        assert_eq!(aliased_output["matches"][0], "Agent");
        assert_eq!(aliased_output["normalized_query"], "agent");

        let selected_with_alias =
            execute_tool("ToolSearch", &json!({"query": "select:AgentTool,Skill"}))
                .expect("ToolSearch alias select should succeed");
        let selected_with_alias_output: serde_json::Value =
            serde_json::from_str(&selected_with_alias).expect("valid json");
        assert_eq!(selected_with_alias_output["matches"][0], "Agent");
        assert_eq!(selected_with_alias_output["matches"][1], "Skill");
    }

    #[test]
    fn agent_persists_handoff_metadata() {
        let _guard = env_lock()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let dir = temp_path("agent-store");
        std::env::set_var("CLAW_AGENT_STORE", &dir);
        let captured = Arc::new(Mutex::new(None::<AgentJob>));
        let captured_for_spawn = Arc::clone(&captured);

        let manifest = execute_agent_with_spawn(
            AgentInput {
                description: "Audit the branch".to_string(),
                prompt: "Check tests and outstanding work.".to_string(),
                subagent_type: Some("Explore".to_string()),
                name: Some("ship-audit".to_string()),
                model: None,
                run_in_background: None,
                isolation: None,
            },
            move |job| {
                *captured_for_spawn
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(job);
                Ok(())
            },
        )
        .expect("Agent should succeed");

        assert_eq!(manifest.name, "ship-audit");
        assert_eq!(manifest.subagent_type.as_deref(), Some("Explore"));
        assert_eq!(manifest.status, "running");
        assert!(!manifest.created_at.is_empty());
        assert!(manifest.started_at.is_some());
        assert!(manifest.completed_at.is_none());
        let contents = std::fs::read_to_string(&manifest.output_file).expect("agent file exists");
        let manifest_contents =
            std::fs::read_to_string(&manifest.manifest_file).expect("manifest file exists");
        assert!(contents.contains("Audit the branch"));
        assert!(contents.contains("Check tests and outstanding work."));
        assert!(manifest_contents.contains("\"subagentType\": \"Explore\""));
        assert!(manifest_contents.contains("\"status\": \"running\""));
        let captured_job = captured
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
            .expect("spawn job should be captured");
        assert_eq!(captured_job.prompt, "Check tests and outstanding work.");
        assert!(captured_job.allowed_tools.contains("read_file"));
        assert!(!captured_job.allowed_tools.contains("Agent"));

        let normalized = execute_tool(
            "Agent",
            &json!({
                "description": "Verify the branch",
                "prompt": "Check tests.",
                "subagent_type": "explorer",
                "run_in_background": true
            }),
        )
        .expect("Agent should normalize built-in aliases");
        let normalized_output: serde_json::Value =
            serde_json::from_str(&normalized).expect("valid json");
        assert_eq!(normalized_output["subagentType"], "Explore");

        let named = execute_tool(
            "Agent",
            &json!({
                "description": "Review the branch",
                "prompt": "Inspect diff.",
                "name": "Ship Audit!!!",
                "run_in_background": true
            }),
        )
        .expect("Agent should normalize explicit names");
        let named_output: serde_json::Value = serde_json::from_str(&named).expect("valid json");
        assert_eq!(named_output["name"], "ship-audit");
        // Retirée seulement maintenant : les appels ci-dessus écriraient sinon
        // dans le `.claw-agents` du dépôt.
        std::env::remove_var("CLAW_AGENT_STORE");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn agent_fake_runner_can_persist_completion_and_failure() {
        let _guard = env_lock()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let dir = temp_path("agent-runner");
        std::env::set_var("CLAW_AGENT_STORE", &dir);

        let completed = execute_agent_with_spawn(
            AgentInput {
                description: "Complete the task".to_string(),
                prompt: "Do the work".to_string(),
                subagent_type: Some("Explore".to_string()),
                name: Some("complete-task".to_string()),
                model: Some("claude-sonnet-4-6".to_string()),
                run_in_background: None,
                isolation: None,
            },
            |job| {
                persist_agent_terminal_state(
                    &job.manifest,
                    "completed",
                    Some("Finished successfully"),
                    None,
                )
            },
        )
        .expect("completed agent should succeed");

        let completed_manifest = std::fs::read_to_string(&completed.manifest_file)
            .expect("completed manifest should exist");
        let completed_output =
            std::fs::read_to_string(&completed.output_file).expect("completed output should exist");
        assert!(completed_manifest.contains("\"status\": \"completed\""));
        assert!(completed_output.contains("Finished successfully"));

        let failed = execute_agent_with_spawn(
            AgentInput {
                description: "Fail the task".to_string(),
                prompt: "Do the failing work".to_string(),
                subagent_type: Some("Verification".to_string()),
                name: Some("fail-task".to_string()),
                model: None,
                run_in_background: None,
                isolation: None,
            },
            |job| {
                persist_agent_terminal_state(
                    &job.manifest,
                    "failed",
                    None,
                    Some(String::from("simulated failure")),
                )
            },
        )
        .expect("failed agent should still spawn");

        let failed_manifest =
            std::fs::read_to_string(&failed.manifest_file).expect("failed manifest should exist");
        let failed_output =
            std::fs::read_to_string(&failed.output_file).expect("failed output should exist");
        assert!(failed_manifest.contains("\"status\": \"failed\""));
        assert!(failed_manifest.contains("simulated failure"));
        assert!(failed_output.contains("simulated failure"));

        let spawn_error = execute_agent_with_spawn(
            AgentInput {
                description: "Spawn error task".to_string(),
                prompt: "Never starts".to_string(),
                subagent_type: None,
                name: Some("spawn-error".to_string()),
                model: None,
                run_in_background: None,
                isolation: None,
            },
            |_| Err(String::from("thread creation failed")),
        )
        .expect_err("spawn errors should surface");
        assert!(spawn_error.contains("failed to spawn sub-agent"));
        let spawn_error_manifest = std::fs::read_dir(&dir)
            .expect("agent dir should exist")
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("json"))
            .find_map(|path| {
                let contents = std::fs::read_to_string(&path).ok()?;
                contents
                    .contains("\"name\": \"spawn-error\"")
                    .then_some(contents)
            })
            .expect("failed manifest should still be written");
        assert!(spawn_error_manifest.contains("\"status\": \"failed\""));
        assert!(spawn_error_manifest.contains("thread creation failed"));

        std::env::remove_var("CLAW_AGENT_STORE");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn specialized_roles_have_aliases_tools_and_read_only_flags() {
        assert_eq!(normalize_subagent_type(Some("Architecte")), "architect");
        assert_eq!(normalize_subagent_type(Some("code-review")), "reviewer");
        assert_eq!(normalize_subagent_type(Some("audit")), "auditor");
        assert_eq!(normalize_subagent_type(Some("fixer")), "tester");

        for role in ["architect", "reviewer", "auditor"] {
            let definition = resolve_agent_definition(role);
            assert!(definition.read_only, "{role} should be read-only");
            assert!(definition.instructions.is_some());
            assert!(definition.tools.contains("read_file"));
            assert!(!definition.tools.contains("write_file"));
            assert!(!definition.tools.contains("Agent"));
        }
        let tester = resolve_agent_definition("tester");
        assert!(!tester.read_only);
        assert!(tester.tools.contains("edit_file"));
        assert!(tester.tools.contains("bash"));
    }

    #[test]
    fn parses_claude_style_markdown_agents() {
        let definition = parse_agent_markdown(
            "---\nname: security-reviewer\ndescription: Finds vulnerabilities\ntools: Read, Grep, Glob\nmodel: openai/gpt-5\n---\nYou review code for OWASP issues.\n",
            "fallback",
        );
        assert_eq!(definition.subagent_type, "security-reviewer");
        assert_eq!(definition.model.as_deref(), Some("openai/gpt-5"));
        assert_eq!(
            definition.tools,
            BTreeSet::from([
                "read_file".to_string(),
                "grep_search".to_string(),
                "glob_search".to_string()
            ])
        );
        assert!(definition.read_only);
        assert_eq!(
            definition.instructions.as_deref(),
            Some("You review code for OWASP issues.")
        );

        assert!(
            !definition.include_mcp,
            "an explicit tools list is respected as is"
        );

        let without_header = parse_agent_markdown("Just instructions", "docs-writer");
        assert_eq!(without_header.subagent_type, "docs-writer");
        assert!(without_header.tools.contains("write_file"));
        assert!(!without_header.read_only);
        assert!(without_header.include_mcp);
    }

    #[test]
    fn finds_custom_agents_by_name_in_agent_dirs() {
        let dir = temp_path("custom-agents");
        std::fs::create_dir_all(&dir).expect("agents dir");
        std::fs::write(
            dir.join("perf.md"),
            "---\nname: Perf-Auditor\ntools: [Read, Bash]\nmodel: inherit\n---\nProfile hot paths.",
        )
        .expect("write agent");

        let found = find_custom_agent(std::slice::from_ref(&dir), "perf-auditor")
            .expect("agent should be found case-insensitively");
        assert!(found.tools.contains("bash"));
        assert!(!found.read_only);
        assert!(found.model.is_none(), "`inherit` keeps the session model");
        assert!(find_custom_agent(std::slice::from_ref(&dir), "missing").is_none());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn agent_model_prefers_request_then_definition_then_session() {
        let context = SubagentContext {
            model: "qwen/qwen3-coder".to_string(),
            permission_mode: PermissionMode::Prompt,
            rules: PermissionRules::default(),
            hooks: runtime::RuntimeHookConfig::default(),
            fallback_models: Vec::new(),
        };
        let mut definition = resolve_agent_definition("Explore");
        assert_eq!(
            resolve_agent_model_for(Some(" openai/gpt-5 "), &definition, Some(&context)),
            "openai/gpt-5"
        );
        assert_eq!(
            resolve_agent_model_for(None, &definition, Some(&context)),
            "qwen/qwen3-coder"
        );
        // Alias Claude (agents de Claude Code) : utilisé seulement avec des
        // identifiants Anthropic, sinon l'agent prend le modèle de la session.
        definition.model = Some("haiku".to_string());
        let expected = if api::has_anthropic_credentials() {
            "claude-haiku-4-5-20251001"
        } else {
            "qwen/qwen3-coder"
        };
        assert_eq!(
            resolve_agent_model_for(None, &definition, Some(&context)),
            expected
        );
        definition.model = Some("deepseek/deepseek-chat:free".to_string());
        assert_eq!(
            resolve_agent_model_for(None, &definition, Some(&context)),
            "deepseek/deepseek-chat:free"
        );
        definition.model = None;
        assert_eq!(
            resolve_agent_model_for(None, &definition, None),
            DEFAULT_AGENT_MODEL
        );
    }

    #[test]
    fn subagent_policy_inherits_mode_and_rules() {
        let mut policy = agent_permission_policy_with(
            PermissionMode::Prompt,
            PermissionRules::from_strings(&["Bash(cargo test:*)".to_string()], &[], &[]),
        );
        assert_eq!(
            policy.authorize("bash", r#"{"command":"cargo test"}"#, None),
            runtime::PermissionOutcome::Allow
        );
        assert!(matches!(
            policy.authorize("write_file", r#"{"path":"a.txt","content":"x"}"#, None),
            runtime::PermissionOutcome::Deny { .. }
        ));
    }

    #[test]
    fn agent_tool_subset_mapping_is_expected() {
        let general = allowed_tools_for_subagent("general-purpose");
        assert!(general.contains("bash"));
        assert!(general.contains("write_file"));
        assert!(!general.contains("Agent"));

        let explore = allowed_tools_for_subagent("Explore");
        assert!(explore.contains("read_file"));
        assert!(explore.contains("grep_search"));
        assert!(!explore.contains("bash"));

        let plan = allowed_tools_for_subagent("Plan");
        assert!(plan.contains("TodoWrite"));
        assert!(plan.contains("StructuredOutput"));
        assert!(!plan.contains("Agent"));

        let verification = allowed_tools_for_subagent("Verification");
        assert!(verification.contains("bash"));
        assert!(verification.contains("PowerShell"));
        assert!(!verification.contains("write_file"));
    }

    #[derive(Debug)]
    struct MockSubagentApiClient {
        calls: usize,
        input_path: String,
    }

    impl runtime::ApiClient for MockSubagentApiClient {
        fn stream(&mut self, request: ApiRequest) -> Result<Vec<AssistantEvent>, RuntimeError> {
            self.calls += 1;
            match self.calls {
                1 => {
                    assert_eq!(request.messages.len(), 1);
                    Ok(vec![
                        AssistantEvent::ToolUse {
                            id: "tool-1".to_string(),
                            name: "read_file".to_string(),
                            input: json!({ "path": self.input_path }).to_string(),
                        },
                        AssistantEvent::MessageStop,
                    ])
                }
                2 => {
                    assert!(request.messages.len() >= 3);
                    Ok(vec![
                        AssistantEvent::TextDelta("Scope: completed mock review".to_string()),
                        AssistantEvent::MessageStop,
                    ])
                }
                _ => panic!("unexpected mock stream call"),
            }
        }
    }

    #[test]
    fn subagent_runtime_executes_tool_loop_with_isolated_session() {
        let _guard = env_lock()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let path = temp_path("subagent-input.txt");
        std::fs::write(&path, "hello from child").expect("write input file");

        let mut runtime = ConversationRuntime::new(
            Session::new(),
            MockSubagentApiClient {
                calls: 0,
                input_path: path.display().to_string(),
            },
            SubagentToolExecutor::new(BTreeSet::from([String::from("read_file")])),
            agent_permission_policy(),
            vec![String::from("system prompt")],
        )
        // Comme un vrai sous-agent : personne ne le lit, aucune annonce demandée.
        .as_subagent();

        let summary = runtime
            .run_turn("Inspect the delegated file", None)
            .expect("subagent loop should succeed");

        assert_eq!(
            final_assistant_text(&summary),
            "Scope: completed mock review"
        );
        assert!(runtime
            .session()
            .messages
            .iter()
            .flat_map(|message| message.blocks.iter())
            .any(|block| matches!(
                block,
                runtime::ContentBlock::ToolResult { output, .. }
                    if output.contains("hello from child")
            )));

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn agent_rejects_blank_required_fields() {
        let missing_description = execute_tool(
            "Agent",
            &json!({
                "description": "  ",
                "prompt": "Inspect"
            }),
        )
        .expect_err("blank description should fail");
        assert!(missing_description.contains("description must not be empty"));

        let missing_prompt = execute_tool(
            "Agent",
            &json!({
                "description": "Inspect branch",
                "prompt": " "
            }),
        )
        .expect_err("blank prompt should fail");
        assert!(missing_prompt.contains("prompt must not be empty"));
    }

    #[test]
    fn notebook_edit_replaces_inserts_and_deletes_cells() {
        let path = temp_path("notebook.ipynb");
        std::fs::write(
            &path,
            r#"{
  "cells": [
    {"cell_type": "code", "id": "cell-a", "metadata": {}, "source": ["print(1)\n"], "outputs": [], "execution_count": null}
  ],
  "metadata": {"kernelspec": {"language": "python"}},
  "nbformat": 4,
  "nbformat_minor": 5
}"#,
        )
        .expect("write notebook");

        let replaced = execute_tool(
            "NotebookEdit",
            &json!({
                "notebook_path": path.display().to_string(),
                "cell_id": "cell-a",
                "new_source": "print(2)\n",
                "edit_mode": "replace"
            }),
        )
        .expect("NotebookEdit replace should succeed");
        let replaced_output: serde_json::Value = serde_json::from_str(&replaced).expect("json");
        assert_eq!(replaced_output["cell_id"], "cell-a");
        assert_eq!(replaced_output["cell_type"], "code");

        let inserted = execute_tool(
            "NotebookEdit",
            &json!({
                "notebook_path": path.display().to_string(),
                "cell_id": "cell-a",
                "new_source": "# heading\n",
                "cell_type": "markdown",
                "edit_mode": "insert"
            }),
        )
        .expect("NotebookEdit insert should succeed");
        let inserted_output: serde_json::Value = serde_json::from_str(&inserted).expect("json");
        assert_eq!(inserted_output["cell_type"], "markdown");
        let appended = execute_tool(
            "NotebookEdit",
            &json!({
                "notebook_path": path.display().to_string(),
                "new_source": "print(3)\n",
                "edit_mode": "insert"
            }),
        )
        .expect("NotebookEdit append should succeed");
        let appended_output: serde_json::Value = serde_json::from_str(&appended).expect("json");
        assert_eq!(appended_output["cell_type"], "code");

        let deleted = execute_tool(
            "NotebookEdit",
            &json!({
                "notebook_path": path.display().to_string(),
                "cell_id": "cell-a",
                "edit_mode": "delete"
            }),
        )
        .expect("NotebookEdit delete should succeed without new_source");
        let deleted_output: serde_json::Value = serde_json::from_str(&deleted).expect("json");
        assert!(deleted_output["cell_type"].is_null());
        assert_eq!(deleted_output["new_source"], "");

        let final_notebook: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).expect("read notebook"))
                .expect("valid notebook json");
        let cells = final_notebook["cells"].as_array().expect("cells array");
        assert_eq!(cells.len(), 2);
        assert_eq!(cells[0]["cell_type"], "markdown");
        assert!(cells[0].get("outputs").is_none());
        assert_eq!(cells[1]["cell_type"], "code");
        assert_eq!(cells[1]["source"][0], "print(3)\n");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn notebook_edit_rejects_invalid_inputs() {
        let text_path = temp_path("notebook.txt");
        fs::write(&text_path, "not a notebook").expect("write text file");
        let wrong_extension = execute_tool(
            "NotebookEdit",
            &json!({
                "notebook_path": text_path.display().to_string(),
                "new_source": "print(1)\n"
            }),
        )
        .expect_err("non-ipynb file should fail");
        assert!(wrong_extension.contains("Jupyter notebook"));
        let _ = fs::remove_file(&text_path);

        let empty_notebook = temp_path("empty.ipynb");
        fs::write(
            &empty_notebook,
            r#"{"cells":[],"metadata":{"kernelspec":{"language":"python"}},"nbformat":4,"nbformat_minor":5}"#,
        )
        .expect("write empty notebook");

        let missing_source = execute_tool(
            "NotebookEdit",
            &json!({
                "notebook_path": empty_notebook.display().to_string(),
                "edit_mode": "insert"
            }),
        )
        .expect_err("insert without source should fail");
        assert!(missing_source.contains("new_source is required"));

        let missing_cell = execute_tool(
            "NotebookEdit",
            &json!({
                "notebook_path": empty_notebook.display().to_string(),
                "edit_mode": "delete"
            }),
        )
        .expect_err("delete on empty notebook should fail");
        assert!(missing_cell.contains("Notebook has no cells to edit"));
        let _ = fs::remove_file(empty_notebook);
    }

    #[test]
    fn bash_tool_reports_success_exit_failure_timeout_and_background() {
        let success = execute_tool("bash", &json!({ "command": "printf 'hello'" }))
            .expect("bash should succeed");
        let success_output: serde_json::Value = serde_json::from_str(&success).expect("json");
        assert_eq!(success_output["stdout"], "hello");
        assert_eq!(success_output["interrupted"], false);

        let failure = execute_tool("bash", &json!({ "command": "printf 'oops' >&2; exit 7" }))
            .expect("bash failure should still return structured output");
        let failure_output: serde_json::Value = serde_json::from_str(&failure).expect("json");
        assert_eq!(failure_output["returnCodeInterpretation"], "exit_code:7");
        assert!(failure_output["stderr"]
            .as_str()
            .expect("stderr")
            .contains("oops"));

        let timeout = execute_tool("bash", &json!({ "command": "sleep 3", "timeout": 1000 }))
            .expect("bash timeout should return output");
        let timeout_output: serde_json::Value = serde_json::from_str(&timeout).expect("json");
        assert_eq!(timeout_output["interrupted"], true);
        assert_eq!(timeout_output["returnCodeInterpretation"], "timeout");
        assert!(timeout_output["stderr"]
            .as_str()
            .expect("stderr")
            .contains("a dépassé le délai"));

        let background = execute_tool(
            "bash",
            &json!({ "command": "sleep 1", "run_in_background": true }),
        )
        .expect("bash background should succeed");
        let background_output: serde_json::Value = serde_json::from_str(&background).expect("json");
        assert!(background_output["backgroundTaskId"].as_str().is_some());
        assert_eq!(background_output["noOutputExpected"], true);

        // BashOutput avec block : une seule lecture attend la fin de la tâche.
        let started = execute_tool(
            "bash",
            &json!({ "command": "sleep 1; echo fini; exit 4", "run_in_background": true }),
        )
        .expect("bash background should succeed");
        let started: serde_json::Value = serde_json::from_str(&started).expect("json");
        let task = started["backgroundTaskId"].as_str().expect("tâche");
        let waited = execute_tool(
            "BashOutput",
            &json!({ "bash_id": task, "block": true, "timeout": 30_000 }),
        )
        .expect("BashOutput should succeed");
        let waited: serde_json::Value = serde_json::from_str(&waited).expect("json");
        assert_eq!(waited["status"], "exited", "{waited}");
        assert_eq!(waited["exitCode"], 4, "{waited}");
        assert_eq!(waited["killed"], false, "{waited}");
        assert!(
            waited["newOutput"]
                .as_str()
                .expect("sortie")
                .contains("fini"),
            "{waited}"
        );
    }

    #[test]
    #[allow(clippy::too_many_lines)]
    fn file_tools_cover_read_write_and_edit_behaviors() {
        let _guard = env_lock()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let root = temp_path("fs-suite");
        fs::create_dir_all(&root).expect("create root");
        let original_dir = std::env::current_dir().expect("cwd");
        std::env::set_current_dir(&root).expect("set cwd");

        let write_create = execute_tool(
            "write_file",
            &json!({ "path": "nested/demo.txt", "content": "alpha\nbeta\nalpha\n" }),
        )
        .expect("write create should succeed");
        let write_create_output: serde_json::Value =
            serde_json::from_str(&write_create).expect("json");
        assert_eq!(write_create_output["type"], "create");
        assert!(root.join("nested/demo.txt").exists());

        let write_update = execute_tool(
            "write_file",
            &json!({ "path": "nested/demo.txt", "content": "alpha\nbeta\ngamma\n" }),
        )
        .expect("write update should succeed");
        let write_update_output: serde_json::Value =
            serde_json::from_str(&write_update).expect("json");
        assert_eq!(write_update_output["type"], "update");
        // Le contenu d'origine n'est plus renvoyé au modèle (économie de contexte).
        assert!(write_update_output["originalFile"].is_null());
        assert!(write_update_output["structuredPatch"].is_array());

        let read_full = execute_tool("read_file", &json!({ "path": "nested/demo.txt" }))
            .expect("read full should succeed");
        let read_full_output: serde_json::Value = serde_json::from_str(&read_full).expect("json");
        assert_eq!(read_full_output["file"]["content"], "alpha\nbeta\ngamma");
        assert_eq!(read_full_output["file"]["startLine"], 1);

        let read_slice = execute_tool(
            "read_file",
            &json!({ "path": "nested/demo.txt", "offset": 1, "limit": 1 }),
        )
        .expect("read slice should succeed");
        let read_slice_output: serde_json::Value = serde_json::from_str(&read_slice).expect("json");
        assert_eq!(read_slice_output["file"]["content"], "beta");
        assert_eq!(read_slice_output["file"]["startLine"], 2);

        let read_past_end = execute_tool(
            "read_file",
            &json!({ "path": "nested/demo.txt", "offset": 50 }),
        )
        .expect("read past EOF should succeed");
        let read_past_end_output: serde_json::Value =
            serde_json::from_str(&read_past_end).expect("json");
        assert_eq!(read_past_end_output["file"]["content"], "");
        assert_eq!(read_past_end_output["file"]["startLine"], 4);

        let read_error = execute_tool("read_file", &json!({ "path": "missing.txt" }))
            .expect_err("missing file should fail");
        assert!(!read_error.is_empty());

        let edit_once = execute_tool(
            "edit_file",
            &json!({ "path": "nested/demo.txt", "old_string": "alpha", "new_string": "omega" }),
        )
        .expect("single edit should succeed");
        let edit_once_output: serde_json::Value = serde_json::from_str(&edit_once).expect("json");
        assert_eq!(edit_once_output["replaceAll"], false);
        assert_eq!(
            fs::read_to_string(root.join("nested/demo.txt")).expect("read file"),
            "omega\nbeta\ngamma\n"
        );

        execute_tool(
            "write_file",
            &json!({ "path": "nested/demo.txt", "content": "alpha\nbeta\nalpha\n" }),
        )
        .expect("reset file");
        let edit_all = execute_tool(
            "edit_file",
            &json!({
                "path": "nested/demo.txt",
                "old_string": "alpha",
                "new_string": "omega",
                "replace_all": true
            }),
        )
        .expect("replace all should succeed");
        let edit_all_output: serde_json::Value = serde_json::from_str(&edit_all).expect("json");
        assert_eq!(edit_all_output["replaceAll"], true);
        assert_eq!(
            fs::read_to_string(root.join("nested/demo.txt")).expect("read file"),
            "omega\nbeta\nomega\n"
        );

        let edit_same = execute_tool(
            "edit_file",
            &json!({ "path": "nested/demo.txt", "old_string": "omega", "new_string": "omega" }),
        )
        .expect_err("identical old/new should fail");
        assert!(
            edit_same.contains("old_string and new_string are identical"),
            "{edit_same}"
        );

        let edit_missing = execute_tool(
            "edit_file",
            &json!({ "path": "nested/demo.txt", "old_string": "missing", "new_string": "omega" }),
        )
        .expect_err("missing substring should fail");
        assert!(
            edit_missing.contains("old_string is not in the file"),
            "{edit_missing}"
        );

        std::env::set_current_dir(&original_dir).expect("restore cwd");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn glob_and_grep_tools_cover_success_and_errors() {
        let _guard = env_lock()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let root = temp_path("search-suite");
        fs::create_dir_all(root.join("nested")).expect("create root");
        let original_dir = std::env::current_dir().expect("cwd");
        std::env::set_current_dir(&root).expect("set cwd");

        fs::write(
            root.join("nested/lib.rs"),
            "fn main() {}\nlet alpha = 1;\nlet alpha = 2;\n",
        )
        .expect("write rust file");
        fs::write(root.join("nested/notes.txt"), "alpha\nbeta\n").expect("write txt file");

        let globbed = execute_tool("glob_search", &json!({ "pattern": "nested/*.rs" }))
            .expect("glob should succeed");
        let globbed_output: serde_json::Value = serde_json::from_str(&globbed).expect("json");
        assert_eq!(globbed_output["numFiles"], 1);
        assert!(globbed_output["filenames"][0]
            .as_str()
            .expect("filename")
            .replace('\\', "/")
            .ends_with("nested/lib.rs"));

        let glob_error = execute_tool("glob_search", &json!({ "pattern": "[" }))
            .expect_err("invalid glob should fail");
        assert!(!glob_error.is_empty());

        let grep_content = execute_tool(
            "grep_search",
            &json!({
                "pattern": "alpha",
                "path": "nested",
                "glob": "*.rs",
                "output_mode": "content",
                "-n": true,
                "head_limit": 1,
                "offset": 1
            }),
        )
        .expect("grep content should succeed");
        let grep_content_output: serde_json::Value =
            serde_json::from_str(&grep_content).expect("json");
        // `head_limit` et `offset` découpent les lignes, pas la liste des
        // fichiers (A6).
        assert_eq!(grep_content_output["numFiles"], 1);
        assert!(grep_content_output["appliedLimit"].is_null());
        assert_eq!(grep_content_output["appliedOffset"], 1);
        assert!(grep_content_output["content"]
            .as_str()
            .expect("content")
            .contains("let alpha = 2;"));

        let grep_count = execute_tool(
            "grep_search",
            &json!({ "pattern": "alpha", "path": "nested", "output_mode": "count" }),
        )
        .expect("grep count should succeed");
        let grep_count_output: serde_json::Value = serde_json::from_str(&grep_count).expect("json");
        assert_eq!(grep_count_output["numMatches"], 3);

        // Un motif qui n'est pas une expression régulière : cherché comme texte.
        let grep_plain = execute_tool(
            "grep_search",
            &json!({ "pattern": "(alpha", "path": "nested" }),
        )
        .expect("motif cherché comme texte");
        assert!(
            grep_plain.contains("searched as plain text"),
            "{grep_plain}"
        );

        std::env::set_current_dir(&original_dir).expect("restore cwd");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn sleep_waits_and_reports_duration() {
        let started = std::time::Instant::now();
        let result =
            execute_tool("Sleep", &json!({"duration_ms": 20})).expect("Sleep should succeed");
        let elapsed = started.elapsed();
        let output: serde_json::Value = serde_json::from_str(&result).expect("json");
        assert_eq!(output["duration_ms"], 20);
        assert!(output["message"]
            .as_str()
            .expect("message")
            .contains("Slept for 20ms"));
        assert!(elapsed >= Duration::from_millis(15));
    }

    #[test]
    fn brief_returns_sent_message_and_attachment_metadata() {
        let attachment = std::env::temp_dir().join(format!(
            "claw-brief-{}.png",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        std::fs::write(&attachment, b"png-data").expect("write attachment");

        let result = execute_tool(
            "SendUserMessage",
            &json!({
                "message": "hello user",
                "attachments": [attachment.display().to_string()],
                "status": "normal"
            }),
        )
        .expect("SendUserMessage should succeed");

        let output: serde_json::Value = serde_json::from_str(&result).expect("json");
        assert_eq!(output["message"], "hello user");
        assert!(output["sentAt"].as_str().is_some());
        assert_eq!(output["attachments"][0]["isImage"], true);
        let _ = std::fs::remove_file(attachment);
    }

    #[test]
    fn config_reads_and_writes_supported_values() {
        let _guard = env_lock()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let root = std::env::temp_dir().join(format!(
            "claw-config-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        let home = root.join("home");
        let cwd = root.join("cwd");
        std::fs::create_dir_all(home.join(".claw")).expect("home dir");
        std::fs::create_dir_all(cwd.join(".claw")).expect("cwd dir");
        std::fs::write(
            home.join(".claw").join("settings.json"),
            r#"{"verbose":false}"#,
        )
        .expect("write global settings");

        let original_home = std::env::var("HOME").ok();
        let original_config_home = std::env::var("CLAW_CONFIG_HOME").ok();
        let original_dir = std::env::current_dir().expect("cwd");
        std::env::set_var("HOME", &home);
        std::env::remove_var("CLAW_CONFIG_HOME");
        std::env::set_current_dir(&cwd).expect("set cwd");

        let get = execute_tool("Config", &json!({"setting": "verbose"})).expect("get config");
        let get_output: serde_json::Value = serde_json::from_str(&get).expect("json");
        assert_eq!(get_output["value"], false);

        let set = execute_tool(
            "Config",
            &json!({"setting": "permissions.defaultMode", "value": "plan"}),
        )
        .expect("set config");
        let set_output: serde_json::Value = serde_json::from_str(&set).expect("json");
        assert_eq!(set_output["operation"], "set");
        assert_eq!(set_output["newValue"], "plan");

        let invalid = execute_tool(
            "Config",
            &json!({"setting": "permissions.defaultMode", "value": "bogus"}),
        )
        .expect_err("invalid config value should error");
        assert!(invalid.contains("Invalid value"));

        let unknown =
            execute_tool("Config", &json!({"setting": "nope"})).expect("unknown setting result");
        let unknown_output: serde_json::Value = serde_json::from_str(&unknown).expect("json");
        assert_eq!(unknown_output["success"], false);

        std::env::set_current_dir(&original_dir).expect("restore cwd");
        match original_home {
            Some(value) => std::env::set_var("HOME", value),
            None => std::env::remove_var("HOME"),
        }
        match original_config_home {
            Some(value) => std::env::set_var("CLAW_CONFIG_HOME", value),
            None => std::env::remove_var("CLAW_CONFIG_HOME"),
        }
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn structured_output_echoes_input_payload() {
        let result = execute_tool("StructuredOutput", &json!({"ok": true, "items": [1, 2, 3]}))
            .expect("StructuredOutput should succeed");
        let output: serde_json::Value = serde_json::from_str(&result).expect("json");
        assert_eq!(output["data"], "Structured output provided successfully");
        assert_eq!(output["structured_output"]["ok"], true);
        assert_eq!(output["structured_output"]["items"][1], 2);
    }

    #[test]
    fn repl_executes_python_code() {
        let result = execute_tool(
            "REPL",
            &json!({"language": "python", "code": "print(1 + 1)", "timeout_ms": 500}),
        )
        .expect("REPL should succeed");
        let output: serde_json::Value = serde_json::from_str(&result).expect("json");
        assert_eq!(output["language"], "python");
        assert_eq!(output["exitCode"], 0);
        assert!(output["stdout"].as_str().expect("stdout").contains('2'));
    }

    #[test]
    #[cfg(unix)]
    fn powershell_runs_via_stub_shell() {
        let _guard = env_lock()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let dir = std::env::temp_dir().join(format!(
            "claw-pwsh-bin-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).expect("create dir");
        let script = dir.join("pwsh");
        std::fs::write(
            &script,
            r#"#!/bin/sh
while [ "$1" != "-Command" ] && [ $# -gt 0 ]; do shift; done
shift
printf 'pwsh:%s' "$1"
"#,
        )
        .expect("write script");
        std::process::Command::new("/bin/chmod")
            .arg("+x")
            .arg(&script)
            .status()
            .expect("chmod");
        let original_path = std::env::var("PATH").unwrap_or_default();
        std::env::set_var("PATH", format!("{}:{}", dir.display(), original_path));

        let result = execute_tool(
            "PowerShell",
            &json!({"command": "Write-Output hello", "timeout": 1000}),
        )
        .expect("PowerShell should succeed");

        let background = execute_tool(
            "PowerShell",
            &json!({"command": "Write-Output hello", "run_in_background": true}),
        )
        .expect("PowerShell background should succeed");

        std::env::set_var("PATH", original_path);
        let _ = std::fs::remove_dir_all(dir);

        let output: serde_json::Value = serde_json::from_str(&result).expect("json");
        assert_eq!(output["stdout"], "pwsh:Write-Output hello");
        assert!(output["stderr"].as_str().expect("stderr").is_empty());

        let background_output: serde_json::Value = serde_json::from_str(&background).expect("json");
        assert!(background_output["backgroundTaskId"].as_str().is_some());
        assert_eq!(background_output["backgroundedByUser"], false);
        assert_eq!(background_output["assistantAutoBackgrounded"], false);
    }

    #[test]
    fn powershell_errors_when_shell_is_missing() {
        let _guard = env_lock()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let original_path = std::env::var("PATH").unwrap_or_default();
        let empty_dir = std::env::temp_dir().join(format!(
            "claw-empty-bin-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        std::fs::create_dir_all(&empty_dir).expect("create empty dir");
        std::env::set_var("PATH", empty_dir.display().to_string());

        let err = execute_tool("PowerShell", &json!({"command": "Write-Output hello"}))
            .expect_err("PowerShell should fail when shell is missing");

        std::env::set_var("PATH", original_path);
        let _ = std::fs::remove_dir_all(empty_dir);

        assert!(err.contains("PowerShell executable not found"));
    }

    struct TestServer {
        addr: SocketAddr,
        shutdown: Option<std::sync::mpsc::Sender<()>>,
        handle: Option<thread::JoinHandle<()>>,
    }

    impl TestServer {
        fn spawn(handler: Arc<dyn Fn(&str) -> HttpResponse + Send + Sync + 'static>) -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").expect("bind test server");
            listener
                .set_nonblocking(true)
                .expect("set nonblocking listener");
            let addr = listener.local_addr().expect("local addr");
            let (tx, rx) = std::sync::mpsc::channel::<()>();

            let handle = thread::spawn(move || loop {
                if rx.try_recv().is_ok() {
                    break;
                }

                match listener.accept() {
                    Ok((mut stream, _)) => {
                        // Sous Windows, la connexion acceptée hérite du mode
                        // non bloquant de l'écoute : lue avant l'arrivée de
                        // la requête, elle échouerait.
                        stream.set_nonblocking(false).expect("blocking stream");
                        stream
                            .set_read_timeout(Some(Duration::from_secs(10)))
                            .expect("read timeout");
                        let mut buffer = [0_u8; 4096];
                        let size = stream.read(&mut buffer).expect("read request");
                        let request = String::from_utf8_lossy(&buffer[..size]).into_owned();
                        let request_line = request.lines().next().unwrap_or_default().to_string();
                        let response = handler(&request_line);
                        stream
                            .write_all(response.to_bytes().as_slice())
                            .expect("write response");
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(10));
                    }
                    Err(error) => panic!("server accept failed: {error}"),
                }
            });

            Self {
                addr,
                shutdown: Some(tx),
                handle: Some(handle),
            }
        }

        fn addr(&self) -> SocketAddr {
            self.addr
        }
    }

    impl Drop for TestServer {
        fn drop(&mut self) {
            if let Some(tx) = self.shutdown.take() {
                let _ = tx.send(());
            }
            if let Some(handle) = self.handle.take() {
                let joined = handle.join();
                // Pas de panique pendant une panique : elle arrêterait tout
                // le binaire de tests au lieu de faire échouer ce seul test.
                if !thread::panicking() {
                    joined.expect("join test server");
                }
            }
        }
    }

    struct HttpResponse {
        status: u16,
        reason: &'static str,
        content_type: &'static str,
        body: String,
    }

    impl HttpResponse {
        fn html(status: u16, reason: &'static str, body: &str) -> Self {
            Self {
                status,
                reason,
                content_type: "text/html; charset=utf-8",
                body: body.to_string(),
            }
        }

        fn text(status: u16, reason: &'static str, body: &str) -> Self {
            Self {
                status,
                reason,
                content_type: "text/plain; charset=utf-8",
                body: body.to_string(),
            }
        }

        fn to_bytes(&self) -> Vec<u8> {
            format!(
                "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                self.status,
                self.reason,
                self.content_type,
                self.body.len(),
                self.body
            )
            .into_bytes()
        }
    }
}
