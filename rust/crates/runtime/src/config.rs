use std::collections::BTreeMap;
use std::fmt::{Display, Formatter};
use std::fs;
use std::path::{Path, PathBuf};

use crate::hooks::HookEvent;
use crate::json::JsonValue;
use crate::sandbox::{FilesystemIsolationMode, SandboxConfig};

pub const CLAW_SETTINGS_SCHEMA_NAME: &str = "SettingsSchema";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ConfigSource {
    User,
    Project,
    Local,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolvedPermissionMode {
    ReadOnly,
    WorkspaceWrite,
    DangerFullAccess,
    Prompt,
}

/// Règles `permissions.allow|deny|ask`, concaténées sur tous les fichiers de config.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RuntimePermissionRuleConfig {
    allow: Vec<String>,
    deny: Vec<String>,
    ask: Vec<String>,
}

impl RuntimePermissionRuleConfig {
    #[must_use]
    pub fn new(allow: Vec<String>, deny: Vec<String>, ask: Vec<String>) -> Self {
        Self { allow, deny, ask }
    }

    #[must_use]
    pub fn allow(&self) -> &[String] {
        &self.allow
    }

    #[must_use]
    pub fn deny(&self) -> &[String] {
        &self.deny
    }

    #[must_use]
    pub fn ask(&self) -> &[String] {
        &self.ask
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigEntry {
    pub source: ConfigSource,
    pub path: PathBuf,
    /// Fichier de Claude Code : seuls `permissions`, `hooks` et `mcpServers`
    /// sont repris, et un fichier illisible est ignoré au lieu de bloquer.
    pub foreign: bool,
}

/// Clés reprises des fichiers de Claude Code.
const FOREIGN_KEYS: &[&str] = &["permissions", "hooks", "mcpServers"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeConfig {
    merged: BTreeMap<String, JsonValue>,
    loaded_entries: Vec<ConfigEntry>,
    feature_config: RuntimeFeatureConfig,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RuntimePluginConfig {
    enabled_plugins: BTreeMap<String, bool>,
    external_directories: Vec<String>,
    install_root: Option<String>,
    registry_path: Option<String>,
    bundled_root: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RuntimeFeatureConfig {
    hooks: RuntimeHookConfig,
    plugins: RuntimePluginConfig,
    mcp: McpConfigCollection,
    oauth: Option<OAuthConfig>,
    model: Option<String>,
    language: Option<String>,
    permission_mode: Option<ResolvedPermissionMode>,
    permission_rules: RuntimePermissionRuleConfig,
    agent_models: BTreeMap<String, String>,
    fallback_models: Vec<String>,
    small_model: Option<String>,
    providers: Vec<RuntimeProviderConfig>,
    sandbox: SandboxConfig,
    verify: crate::harness::VerifySetting,
    /// `"review": false` : pas de relecture automatique par un autre modèle.
    review_disabled: bool,
    /// Modèles consultés (relecture, conseil, second avis) : `advisorModels`.
    advisor_models: Vec<String>,
    /// Modèle du mode plan : `plannerModel`.
    planner_model: Option<String>,
    /// Modèle plus fort qui reprend un tour où l'agent n'avance plus :
    /// `escalationModel`.
    escalation_model: Option<String>,
    /// Modèle qui décrit les images à un modèle qui n'en lit pas :
    /// `imageReader` (`"off"` pour ne rien décrire).
    image_reader: Option<String>,
}

/// Serveur compatible `OpenAI` déclaré dans `providers` (utilisé avec le
/// préfixe `nom:` devant le modèle).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RuntimeProviderConfig {
    pub name: String,
    pub base_url: String,
    pub api_key_env: Option<String>,
    pub local: bool,
    pub context_window: Option<u64>,
    pub max_output_tokens: Option<u32>,
    pub headers: BTreeMap<String, String>,
    /// `"toolMode": "text"` : outils décrits dans le prompt (modèles sans appel natif).
    pub text_tools: bool,
}

/// Une commande de hook, éventuellement filtrée par un `matcher` (nom d'outil,
/// source de session…) et limitée par un délai en secondes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookCommandConfig {
    pub command: String,
    pub matcher: Option<String>,
    pub timeout_secs: Option<u64>,
}

impl HookCommandConfig {
    #[must_use]
    pub fn new(command: String) -> Self {
        Self {
            command,
            matcher: None,
            timeout_secs: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RuntimeHookConfig {
    hooks: BTreeMap<HookEvent, Vec<HookCommandConfig>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct McpConfigCollection {
    servers: BTreeMap<String, ScopedMcpServerConfig>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopedMcpServerConfig {
    pub scope: ConfigSource,
    pub config: McpServerConfig,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpTransport {
    Stdio,
    Sse,
    Http,
    Ws,
    Sdk,
    ManagedProxy,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum McpServerConfig {
    Stdio(McpStdioServerConfig),
    Sse(McpRemoteServerConfig),
    Http(McpRemoteServerConfig),
    Ws(McpWebSocketServerConfig),
    Sdk(McpSdkServerConfig),
    ManagedProxy(McpManagedProxyServerConfig),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpStdioServerConfig {
    pub command: String,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpRemoteServerConfig {
    pub url: String,
    pub headers: BTreeMap<String, String>,
    pub headers_helper: Option<String>,
    pub oauth: Option<McpOAuthConfig>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpWebSocketServerConfig {
    pub url: String,
    pub headers: BTreeMap<String, String>,
    pub headers_helper: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpSdkServerConfig {
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpManagedProxyServerConfig {
    pub url: String,
    pub id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpOAuthConfig {
    pub client_id: Option<String>,
    pub callback_port: Option<u16>,
    pub auth_server_metadata_url: Option<String>,
    pub xaa: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OAuthConfig {
    pub client_id: String,
    pub authorize_url: String,
    pub token_url: String,
    pub callback_port: Option<u16>,
    pub manual_redirect_url: Option<String>,
    pub scopes: Vec<String>,
}

#[derive(Debug)]
pub enum ConfigError {
    Io(std::io::Error),
    Parse(String),
}

impl Display for ConfigError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(f, "{error}"),
            Self::Parse(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for ConfigError {}

impl From<std::io::Error> for ConfigError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigLoader {
    cwd: PathBuf,
    config_home: PathBuf,
}

impl ConfigLoader {
    #[must_use]
    pub fn new(cwd: impl Into<PathBuf>, config_home: impl Into<PathBuf>) -> Self {
        Self {
            cwd: cwd.into(),
            config_home: config_home.into(),
        }
    }

    #[must_use]
    pub fn default_for(cwd: impl Into<PathBuf>) -> Self {
        let cwd = cwd.into();
        let config_home = default_config_home();
        Self { cwd, config_home }
    }

    #[must_use]
    pub fn config_home(&self) -> &Path {
        &self.config_home
    }

    /// Fichiers de configuration, du moins prioritaire au plus prioritaire.
    /// Ceux de Claude Code passent avant leurs équivalents Claw (qui l'emportent) ;
    /// `CLAW_NO_CLAUDE_CONFIG=1` les ignore.
    #[must_use]
    pub fn discover(&self) -> Vec<ConfigEntry> {
        let home = self.config_home.parent().map(Path::to_path_buf);
        let user_legacy_path = home.as_ref().map_or_else(
            || PathBuf::from(".claw.json"),
            |parent| parent.join(".claw.json"),
        );
        let native = |source, path| ConfigEntry {
            source,
            path,
            foreign: false,
        };
        let foreign = |source, path| ConfigEntry {
            source,
            path,
            foreign: true,
        };
        let claude =
            std::env::var("CLAW_NO_CLAUDE_CONFIG").map_or(true, |value| value.trim() != "1");
        let mut entries = Vec::new();
        if claude {
            if let Some(home) = &home {
                entries.push(foreign(
                    ConfigSource::User,
                    home.join(".claude").join("settings.json"),
                ));
            }
        }
        entries.push(native(ConfigSource::User, user_legacy_path));
        entries.push(native(
            ConfigSource::User,
            self.config_home.join("settings.json"),
        ));
        if claude {
            entries.push(foreign(ConfigSource::Project, self.cwd.join(".mcp.json")));
            entries.push(foreign(
                ConfigSource::Project,
                self.cwd.join(".claude").join("settings.json"),
            ));
        }
        entries.push(native(ConfigSource::Project, self.cwd.join(".claw.json")));
        entries.push(native(
            ConfigSource::Project,
            self.cwd.join(".claw").join("settings.json"),
        ));
        if claude {
            entries.push(foreign(
                ConfigSource::Local,
                self.cwd.join(".claude").join("settings.local.json"),
            ));
        }
        entries.push(native(
            ConfigSource::Local,
            self.cwd.join(".claw").join("settings.local.json"),
        ));
        entries
    }

    pub fn load(&self) -> Result<RuntimeConfig, ConfigError> {
        let mut merged = BTreeMap::new();
        let mut loaded_entries = Vec::new();
        let mut mcp_servers = BTreeMap::new();
        let mut permission_rules = RuntimePermissionRuleConfig::default();

        for entry in self.discover() {
            let value = if entry.foreign {
                match read_foreign_settings(&entry) {
                    Some(value) => value,
                    None => continue,
                }
            } else {
                let Some(value) = read_optional_json_object(&entry.path)? else {
                    continue;
                };
                value
            };
            merge_mcp_servers(&mut mcp_servers, entry.source, &value, &entry.path)?;
            merge_permission_rules(&mut permission_rules, &value, &entry.path)?;
            deep_merge_objects(&mut merged, &value);
            loaded_entries.push(entry);
        }

        let merged_value = JsonValue::Object(merged.clone());

        let feature_config = RuntimeFeatureConfig {
            hooks: parse_optional_hooks_config(&merged_value)?,
            plugins: parse_optional_plugin_config(&merged_value)?,
            mcp: McpConfigCollection {
                servers: mcp_servers,
            },
            oauth: parse_optional_oauth_config(&merged_value, "merged settings.oauth")?,
            model: parse_optional_model(&merged_value),
            language: parse_optional_language(&merged_value),
            permission_mode: parse_optional_permission_mode(&merged_value)?,
            permission_rules,
            agent_models: parse_agent_models(&merged_value),
            fallback_models: parse_string_list(&merged_value, "fallbackModels"),
            small_model: parse_optional_string(&merged_value, "smallModel"),
            providers: parse_providers(&merged_value)?,
            sandbox: parse_optional_sandbox_config(&merged_value)?,
            verify: parse_verify(&merged_value),
            review_disabled: merged_value
                .as_object()
                .and_then(|object| object.get("review"))
                .and_then(JsonValue::as_bool)
                == Some(false),
            advisor_models: parse_string_list(&merged_value, "advisorModels"),
            planner_model: parse_optional_string(&merged_value, "plannerModel"),
            escalation_model: parse_optional_string(&merged_value, "escalationModel"),
            image_reader: parse_optional_string(&merged_value, "imageReader"),
        };

        Ok(RuntimeConfig {
            merged,
            loaded_entries,
            feature_config,
        })
    }
}

impl RuntimeConfig {
    #[must_use]
    pub fn empty() -> Self {
        Self {
            merged: BTreeMap::new(),
            loaded_entries: Vec::new(),
            feature_config: RuntimeFeatureConfig::default(),
        }
    }

    #[must_use]
    pub fn merged(&self) -> &BTreeMap<String, JsonValue> {
        &self.merged
    }

    #[must_use]
    pub fn loaded_entries(&self) -> &[ConfigEntry] {
        &self.loaded_entries
    }

    #[must_use]
    pub fn get(&self, key: &str) -> Option<&JsonValue> {
        self.merged.get(key)
    }

    #[must_use]
    pub fn as_json(&self) -> JsonValue {
        JsonValue::Object(self.merged.clone())
    }

    #[must_use]
    pub fn feature_config(&self) -> &RuntimeFeatureConfig {
        &self.feature_config
    }

    #[must_use]
    pub fn mcp(&self) -> &McpConfigCollection {
        &self.feature_config.mcp
    }

    #[must_use]
    pub fn hooks(&self) -> &RuntimeHookConfig {
        &self.feature_config.hooks
    }

    #[must_use]
    pub fn plugins(&self) -> &RuntimePluginConfig {
        &self.feature_config.plugins
    }

    #[must_use]
    pub fn oauth(&self) -> Option<&OAuthConfig> {
        self.feature_config.oauth.as_ref()
    }

    #[must_use]
    pub fn model(&self) -> Option<&str> {
        self.feature_config.model.as_deref()
    }

    #[must_use]
    pub fn permission_mode(&self) -> Option<ResolvedPermissionMode> {
        self.feature_config.permission_mode
    }

    #[must_use]
    pub fn permission_rules(&self) -> &RuntimePermissionRuleConfig {
        &self.feature_config.permission_rules
    }

    /// Langue des réponses (`language`), par exemple `français`.
    #[must_use]
    pub fn language(&self) -> Option<&str> {
        self.feature_config.language.as_deref()
    }

    /// Modèle configuré pour un type de sous-agent (`agents.<type>.model`).
    #[must_use]
    pub fn agent_model(&self, agent: &str) -> Option<&str> {
        self.feature_config
            .agent_models
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(agent))
            .map(|(_, model)| model.as_str())
    }

    /// Modèles de secours, essayés dans l'ordre si le principal échoue (`fallbackModels`).
    #[must_use]
    pub fn fallback_models(&self) -> &[String] {
        &self.feature_config.fallback_models
    }

    /// Niveau de raisonnement demandé aux modèles (`reasoning` : off, low, medium, high).
    #[must_use]
    pub fn reasoning(&self) -> Option<&str> {
        self.merged.get("reasoning").and_then(JsonValue::as_str)
    }

    /// Petit modèle pour les tâches simples : résumés de compaction (`smallModel`).
    #[must_use]
    pub fn small_model(&self) -> Option<&str> {
        self.feature_config.small_model.as_deref()
    }

    /// Serveurs compatibles `OpenAI` déclarés (`providers`).
    #[must_use]
    pub fn providers(&self) -> &[RuntimeProviderConfig] {
        &self.feature_config.providers
    }

    #[must_use]
    pub fn sandbox(&self) -> &SandboxConfig {
        &self.feature_config.sandbox
    }

    /// Modèles consultés pour la relecture, le conseil et le second avis.
    #[must_use]
    pub fn advisor_models(&self) -> &[String] {
        &self.feature_config.advisor_models
    }

    /// Modèle utilisé en mode plan, s'il est réglé (`plannerModel`).
    #[must_use]
    pub fn planner_model(&self) -> Option<&str> {
        self.feature_config.planner_model.as_deref()
    }

    /// Modèle plus fort appelé quand l'agent n'avance plus (`escalationModel`).
    #[must_use]
    pub fn escalation_model(&self) -> Option<&str> {
        self.feature_config.escalation_model.as_deref()
    }

    /// Modèle qui décrit les images à un modèle qui n'en lit pas (`imageReader`).
    #[must_use]
    pub fn image_reader(&self) -> Option<&str> {
        self.feature_config.image_reader.as_deref()
    }
}

impl RuntimeFeatureConfig {
    #[must_use]
    pub fn with_hooks(mut self, hooks: RuntimeHookConfig) -> Self {
        self.hooks = hooks;
        self
    }

    #[must_use]
    pub fn with_plugins(mut self, plugins: RuntimePluginConfig) -> Self {
        self.plugins = plugins;
        self
    }

    #[must_use]
    pub fn hooks(&self) -> &RuntimeHookConfig {
        &self.hooks
    }

    #[must_use]
    pub fn plugins(&self) -> &RuntimePluginConfig {
        &self.plugins
    }

    #[must_use]
    pub fn mcp(&self) -> &McpConfigCollection {
        &self.mcp
    }

    #[must_use]
    pub fn oauth(&self) -> Option<&OAuthConfig> {
        self.oauth.as_ref()
    }

    #[must_use]
    pub fn model(&self) -> Option<&str> {
        self.model.as_deref()
    }

    #[must_use]
    pub fn permission_mode(&self) -> Option<ResolvedPermissionMode> {
        self.permission_mode
    }

    #[must_use]
    pub fn permission_rules(&self) -> &RuntimePermissionRuleConfig {
        &self.permission_rules
    }

    #[must_use]
    pub fn sandbox(&self) -> &SandboxConfig {
        &self.sandbox
    }

    /// Vérification imposée après modification (`"verify"`).
    #[must_use]
    pub fn verify(&self) -> &crate::harness::VerifySetting {
        &self.verify
    }

    /// Langue des réponses (`"language"`), si elle est réglée.
    #[must_use]
    pub fn response_language(&self) -> Option<&str> {
        self.language.as_deref()
    }

    /// Relecture automatique des modifications (`"review": false` la coupe).
    #[must_use]
    pub fn review_enabled(&self) -> bool {
        !self.review_disabled
    }

    /// Modèles consultés pour la relecture, le conseil et le second avis.
    #[must_use]
    pub fn advisor_models(&self) -> &[String] {
        &self.advisor_models
    }

    /// Modèle utilisé en mode plan, s'il est réglé.
    #[must_use]
    pub fn planner_model(&self) -> Option<&str> {
        self.planner_model.as_deref()
    }

    /// Modèle plus fort appelé quand l'agent n'avance plus, s'il est réglé.
    #[must_use]
    pub fn escalation_model(&self) -> Option<&str> {
        self.escalation_model.as_deref()
    }
}

impl RuntimePluginConfig {
    #[must_use]
    pub fn enabled_plugins(&self) -> &BTreeMap<String, bool> {
        &self.enabled_plugins
    }

    #[must_use]
    pub fn external_directories(&self) -> &[String] {
        &self.external_directories
    }

    #[must_use]
    pub fn install_root(&self) -> Option<&str> {
        self.install_root.as_deref()
    }

    #[must_use]
    pub fn registry_path(&self) -> Option<&str> {
        self.registry_path.as_deref()
    }

    #[must_use]
    pub fn bundled_root(&self) -> Option<&str> {
        self.bundled_root.as_deref()
    }

    pub fn set_plugin_state(&mut self, plugin_id: String, enabled: bool) {
        self.enabled_plugins.insert(plugin_id, enabled);
    }

    #[must_use]
    pub fn state_for(&self, plugin_id: &str, default_enabled: bool) -> bool {
        self.enabled_plugins
            .get(plugin_id)
            .copied()
            .unwrap_or(default_enabled)
    }
}

#[must_use]
pub fn default_config_home() -> PathBuf {
    std::env::var_os("CLAW_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| crate::home_dir().map(|home| home.join(".claw")))
        .unwrap_or_else(|| PathBuf::from(".claw"))
}

impl RuntimeHookConfig {
    #[must_use]
    pub fn new(pre_tool_use: Vec<String>, post_tool_use: Vec<String>) -> Self {
        Self::default()
            .with_commands(HookEvent::PreToolUse, pre_tool_use)
            .with_commands(HookEvent::PostToolUse, post_tool_use)
    }

    /// Ajoute des commandes sans `matcher` à un événement.
    #[must_use]
    pub fn with_commands(self, event: HookEvent, commands: Vec<String>) -> Self {
        self.with_entries(
            event,
            commands.into_iter().map(HookCommandConfig::new).collect(),
        )
    }

    #[must_use]
    pub fn with_entries(mut self, event: HookEvent, entries: Vec<HookCommandConfig>) -> Self {
        if !entries.is_empty() {
            let target = self.hooks.entry(event).or_default();
            for entry in entries {
                if !target.contains(&entry) {
                    target.push(entry);
                }
            }
        }
        self
    }

    #[must_use]
    pub fn entries(&self, event: HookEvent) -> &[HookCommandConfig] {
        self.hooks.get(&event).map_or(&[], Vec::as_slice)
    }

    /// Commandes d'un événement (sans leurs `matcher`).
    #[must_use]
    pub fn commands(&self, event: HookEvent) -> Vec<&str> {
        self.entries(event)
            .iter()
            .map(|entry| entry.command.as_str())
            .collect()
    }

    #[must_use]
    pub fn pre_tool_use(&self) -> Vec<&str> {
        self.commands(HookEvent::PreToolUse)
    }

    #[must_use]
    pub fn post_tool_use(&self) -> Vec<&str> {
        self.commands(HookEvent::PostToolUse)
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.hooks.values().all(Vec::is_empty)
    }

    #[must_use]
    pub fn merged(&self, other: &Self) -> Self {
        let mut merged = self.clone();
        merged.extend(other);
        merged
    }

    pub fn extend(&mut self, other: &Self) {
        for (event, entries) in &other.hooks {
            *self = std::mem::take(self).with_entries(*event, entries.clone());
        }
    }
}

impl McpConfigCollection {
    #[must_use]
    pub fn servers(&self) -> &BTreeMap<String, ScopedMcpServerConfig> {
        &self.servers
    }

    #[must_use]
    pub fn get(&self, name: &str) -> Option<&ScopedMcpServerConfig> {
        self.servers.get(name)
    }
}

impl ScopedMcpServerConfig {
    #[must_use]
    pub fn transport(&self) -> McpTransport {
        self.config.transport()
    }
}

impl McpServerConfig {
    #[must_use]
    pub fn transport(&self) -> McpTransport {
        match self {
            Self::Stdio(_) => McpTransport::Stdio,
            Self::Sse(_) => McpTransport::Sse,
            Self::Http(_) => McpTransport::Http,
            Self::Ws(_) => McpTransport::Ws,
            Self::Sdk(_) => McpTransport::Sdk,
            Self::ManagedProxy(_) => McpTransport::ManagedProxy,
        }
    }
}

/// Fichier de Claude Code réduit aux clés comprises par Claw ; `None` s'il est
/// absent ou si ces clés ne peuvent pas être lues.
fn read_foreign_settings(entry: &ConfigEntry) -> Option<BTreeMap<String, JsonValue>> {
    let object = read_optional_json_object(&entry.path).ok()??;
    let subset: BTreeMap<String, JsonValue> = object
        .into_iter()
        .filter(|(key, _)| FOREIGN_KEYS.contains(&key.as_str()))
        .collect();
    // Validation à blanc : un réglage incompatible ne doit pas casser Claw.
    let mut servers = BTreeMap::new();
    let mut rules = RuntimePermissionRuleConfig::default();
    merge_mcp_servers(&mut servers, entry.source, &subset, &entry.path).ok()?;
    merge_permission_rules(&mut rules, &subset, &entry.path).ok()?;
    let value = JsonValue::Object(subset.clone());
    parse_optional_hooks_config(&value).ok()?;
    parse_optional_permission_mode(&value).ok()?;
    Some(subset)
}

fn read_optional_json_object(
    path: &Path,
) -> Result<Option<BTreeMap<String, JsonValue>>, ConfigError> {
    let is_legacy_config = path.file_name().and_then(|name| name.to_str()) == Some(".claw.json");
    let contents = match fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(ConfigError::Io(error)),
    };

    if contents.trim().is_empty() {
        return Ok(Some(BTreeMap::new()));
    }

    let parsed = match JsonValue::parse(&contents) {
        Ok(parsed) => parsed,
        Err(_) if is_legacy_config => return Ok(None),
        Err(error) => return Err(ConfigError::Parse(format!("{}: {error}", path.display()))),
    };
    let Some(object) = parsed.as_object() else {
        if is_legacy_config {
            return Ok(None);
        }
        return Err(ConfigError::Parse(format!(
            "{} : la valeur racine des paramètres doit être un objet JSON",
            path.display()
        )));
    };
    Ok(Some(object.clone()))
}

fn merge_mcp_servers(
    target: &mut BTreeMap<String, ScopedMcpServerConfig>,
    source: ConfigSource,
    root: &BTreeMap<String, JsonValue>,
    path: &Path,
) -> Result<(), ConfigError> {
    let Some(mcp_servers) = root.get("mcpServers") else {
        return Ok(());
    };
    let servers = expect_object(mcp_servers, &format!("{}: mcpServers", path.display()))?;
    for (name, value) in servers {
        let parsed = parse_mcp_server_config(
            name,
            value,
            &format!("{}: mcpServers.{name}", path.display()),
        )?;
        target.insert(
            name.clone(),
            ScopedMcpServerConfig {
                scope: source,
                config: parsed,
            },
        );
    }
    Ok(())
}

fn merge_permission_rules(
    target: &mut RuntimePermissionRuleConfig,
    root: &BTreeMap<String, JsonValue>,
    path: &Path,
) -> Result<(), ConfigError> {
    let Some(permissions) = root.get("permissions") else {
        return Ok(());
    };
    let context = format!("{}: permissions", path.display());
    let permissions = expect_object(permissions, &context)?;
    for (key, rules) in [
        ("allow", &mut target.allow),
        ("deny", &mut target.deny),
        ("ask", &mut target.ask),
    ] {
        if let Some(values) = optional_string_array(permissions, key, &context)? {
            extend_unique(rules, &values);
        }
    }
    Ok(())
}

fn parse_agent_models(root: &JsonValue) -> BTreeMap<String, String> {
    root.as_object()
        .and_then(|object| object.get("agents"))
        .and_then(JsonValue::as_object)
        .map(|agents| {
            agents
                .iter()
                .filter_map(|(name, value)| {
                    let model = value.as_object()?.get("model")?.as_str()?.trim();
                    (!model.is_empty()).then(|| (name.clone(), model.to_string()))
                })
                .collect()
        })
        .unwrap_or_default()
}

fn parse_string_list(root: &JsonValue, key: &str) -> Vec<String> {
    root.as_object()
        .and_then(|object| object.get(key))
        .and_then(JsonValue::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(JsonValue::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

/// `"verify"` : `false` coupe la vérification imposée, une commande ou une
/// liste de commandes la remplace ; absent : déduite du projet.
fn parse_verify(root: &JsonValue) -> crate::harness::VerifySetting {
    use crate::harness::VerifySetting;
    match root.as_object().and_then(|object| object.get("verify")) {
        Some(JsonValue::Bool(false)) => VerifySetting::Off,
        Some(JsonValue::String(command)) if !command.trim().is_empty() => {
            VerifySetting::Commands(vec![command.trim().to_string()])
        }
        Some(JsonValue::Array(commands)) => {
            let commands = commands
                .iter()
                .filter_map(JsonValue::as_str)
                .map(str::trim)
                .filter(|command| !command.is_empty())
                .map(ToOwned::to_owned)
                .collect::<Vec<_>>();
            if commands.is_empty() {
                VerifySetting::Off
            } else {
                VerifySetting::Commands(commands)
            }
        }
        _ => VerifySetting::Auto,
    }
}

fn parse_optional_string(root: &JsonValue, key: &str) -> Option<String> {
    root.as_object()
        .and_then(|object| object.get(key))
        .and_then(JsonValue::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

/// `"providers": {"nom": {"baseUrl": "...", "apiKeyEnv": "...", "local": true,
/// "contextWindow": 65536, "maxOutputTokens": 8192, "headers": {...}}}`.
fn parse_providers(root: &JsonValue) -> Result<Vec<RuntimeProviderConfig>, ConfigError> {
    let Some(providers) = root.as_object().and_then(|object| object.get("providers")) else {
        return Ok(Vec::new());
    };
    let providers = expect_object(providers, "merged settings.providers")?;
    let mut parsed = Vec::new();
    for (name, value) in providers {
        let context = format!("merged settings.providers.{name}");
        let object = expect_object(value, &context)?;
        let base_url = object
            .get("baseUrl")
            .or_else(|| object.get("base_url"))
            .and_then(JsonValue::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                ConfigError::Parse(format!("{context} : le champ baseUrl est requis"))
            })?;
        let number = |key: &str| {
            object
                .get(key)
                .and_then(JsonValue::as_i64)
                .and_then(|value| u64::try_from(value).ok())
                .filter(|value| *value > 0)
        };
        parsed.push(RuntimeProviderConfig {
            name: name.clone(),
            base_url: base_url.to_string(),
            api_key_env: object
                .get("apiKeyEnv")
                .and_then(JsonValue::as_str)
                .map(ToOwned::to_owned),
            local: object
                .get("local")
                .and_then(JsonValue::as_bool)
                .unwrap_or_else(|| {
                    base_url.contains("localhost") || base_url.contains("127.0.0.1")
                }),
            context_window: number("contextWindow"),
            max_output_tokens: number("maxOutputTokens")
                .and_then(|value| u32::try_from(value).ok()),
            headers: optional_string_map(object, "headers", &context)?.unwrap_or_default(),
            text_tools: object
                .get("toolMode")
                .and_then(JsonValue::as_str)
                .is_some_and(|mode| mode.eq_ignore_ascii_case("text")),
        });
    }
    Ok(parsed)
}

fn parse_optional_language(root: &JsonValue) -> Option<String> {
    root.as_object()
        .and_then(|object| object.get("language"))
        .and_then(JsonValue::as_str)
        .map(str::trim)
        .filter(|language| !language.is_empty())
        .map(ToOwned::to_owned)
}

fn parse_optional_model(root: &JsonValue) -> Option<String> {
    root.as_object()
        .and_then(|object| object.get("model"))
        .and_then(JsonValue::as_str)
        .map(ToOwned::to_owned)
}

fn parse_optional_hooks_config(root: &JsonValue) -> Result<RuntimeHookConfig, ConfigError> {
    let Some(object) = root.as_object() else {
        return Ok(RuntimeHookConfig::default());
    };
    let Some(hooks_value) = object.get("hooks") else {
        return Ok(RuntimeHookConfig::default());
    };
    let hooks = expect_object(hooks_value, "merged settings.hooks")?;
    let mut config = RuntimeHookConfig::default();
    for (name, value) in hooks {
        // Clés inconnues ignorées : réglages d'autres outils ou événements futurs.
        let Some(event) = HookEvent::from_name(name) else {
            continue;
        };
        let context = format!("merged settings.hooks.{name}");
        let Some(items) = value.as_array() else {
            return Err(ConfigError::Parse(format!(
                "{context} : un tableau est attendu"
            )));
        };
        let mut entries = Vec::new();
        for (index, item) in items.iter().enumerate() {
            parse_hook_item(item, &format!("{context}[{index}]"), &mut entries)?;
        }
        config = config.with_entries(event, entries);
    }
    Ok(config)
}

/// Formes acceptées : `"commande"`, `{"command", "matcher"?, "timeout"?}` ou,
/// comme Claude Code, `{"matcher"?, "hooks": [{"type": "command", "command", "timeout"?}]}`.
fn parse_hook_item(
    item: &JsonValue,
    context: &str,
    entries: &mut Vec<HookCommandConfig>,
) -> Result<(), ConfigError> {
    if let Some(command) = item.as_str() {
        entries.push(HookCommandConfig::new(command.to_string()));
        return Ok(());
    }
    let Some(object) = item.as_object() else {
        return Err(ConfigError::Parse(format!(
            "{context} : une chaîne ou un objet est attendu"
        )));
    };
    let matcher = object
        .get("matcher")
        .and_then(JsonValue::as_str)
        .map(ToOwned::to_owned);
    let timeout = |object: &BTreeMap<String, JsonValue>| {
        object
            .get("timeout")
            .and_then(JsonValue::as_i64)
            .and_then(|seconds| u64::try_from(seconds).ok())
            .filter(|seconds| *seconds > 0)
    };
    if let Some(command) = object.get("command").and_then(JsonValue::as_str) {
        entries.push(HookCommandConfig {
            command: command.to_string(),
            matcher,
            timeout_secs: timeout(object),
        });
        return Ok(());
    }
    let Some(hooks) = object.get("hooks").and_then(JsonValue::as_array) else {
        return Err(ConfigError::Parse(format!(
            "{context} : un champ \"command\" ou \"hooks\" est attendu"
        )));
    };
    for hook in hooks {
        let Some(hook) = hook.as_object() else {
            continue;
        };
        // Seuls les hooks de type `command` existent ici (pas `prompt`).
        let kind = hook
            .get("type")
            .and_then(JsonValue::as_str)
            .unwrap_or("command");
        if let (Some(command), "command") = (hook.get("command").and_then(JsonValue::as_str), kind)
        {
            entries.push(HookCommandConfig {
                command: command.to_string(),
                matcher: matcher.clone(),
                timeout_secs: timeout(hook),
            });
        }
    }
    Ok(())
}

fn parse_optional_plugin_config(root: &JsonValue) -> Result<RuntimePluginConfig, ConfigError> {
    let Some(object) = root.as_object() else {
        return Ok(RuntimePluginConfig::default());
    };

    let mut config = RuntimePluginConfig::default();
    if let Some(enabled_plugins) = object.get("enabledPlugins") {
        config.enabled_plugins = parse_bool_map(enabled_plugins, "merged settings.enabledPlugins")?;
    }

    let Some(plugins_value) = object.get("plugins") else {
        return Ok(config);
    };
    let plugins = expect_object(plugins_value, "merged settings.plugins")?;

    if let Some(enabled_value) = plugins.get("enabled") {
        config.enabled_plugins = parse_bool_map(enabled_value, "merged settings.plugins.enabled")?;
    }
    config.external_directories =
        optional_string_array(plugins, "externalDirectories", "merged settings.plugins")?
            .unwrap_or_default();
    config.install_root =
        optional_string(plugins, "installRoot", "merged settings.plugins")?.map(str::to_string);
    config.registry_path =
        optional_string(plugins, "registryPath", "merged settings.plugins")?.map(str::to_string);
    config.bundled_root =
        optional_string(plugins, "bundledRoot", "merged settings.plugins")?.map(str::to_string);
    Ok(config)
}

fn parse_optional_permission_mode(
    root: &JsonValue,
) -> Result<Option<ResolvedPermissionMode>, ConfigError> {
    let Some(object) = root.as_object() else {
        return Ok(None);
    };
    if let Some(mode) = object.get("permissionMode").and_then(JsonValue::as_str) {
        return parse_permission_mode_label(mode, "merged settings.permissionMode").map(Some);
    }
    let Some(mode) = object
        .get("permissions")
        .and_then(JsonValue::as_object)
        .and_then(|permissions| permissions.get("defaultMode"))
        .and_then(JsonValue::as_str)
    else {
        return Ok(None);
    };
    parse_permission_mode_label(mode, "merged settings.permissions.defaultMode").map(Some)
}

fn parse_permission_mode_label(
    mode: &str,
    context: &str,
) -> Result<ResolvedPermissionMode, ConfigError> {
    match mode {
        "default" | "prompt" | "ask" => Ok(ResolvedPermissionMode::Prompt),
        "plan" | "read-only" => Ok(ResolvedPermissionMode::ReadOnly),
        "acceptEdits" | "auto" | "workspace-write" => Ok(ResolvedPermissionMode::WorkspaceWrite),
        "dontAsk" | "bypassPermissions" | "danger-full-access" => {
            Ok(ResolvedPermissionMode::DangerFullAccess)
        }
        other => Err(ConfigError::Parse(format!(
            "{context} : mode de permission non supporté {other}"
        ))),
    }
}

fn parse_optional_sandbox_config(root: &JsonValue) -> Result<SandboxConfig, ConfigError> {
    let Some(object) = root.as_object() else {
        return Ok(SandboxConfig::default());
    };
    let Some(sandbox_value) = object.get("sandbox") else {
        return Ok(SandboxConfig::default());
    };
    let sandbox = expect_object(sandbox_value, "merged settings.sandbox")?;
    let filesystem_mode = optional_string(sandbox, "filesystemMode", "merged settings.sandbox")?
        .map(parse_filesystem_mode_label)
        .transpose()?;
    Ok(SandboxConfig {
        enabled: optional_bool(sandbox, "enabled", "merged settings.sandbox")?,
        namespace_restrictions: optional_bool(
            sandbox,
            "namespaceRestrictions",
            "merged settings.sandbox",
        )?,
        network_isolation: optional_bool(sandbox, "networkIsolation", "merged settings.sandbox")?,
        filesystem_mode,
        allowed_mounts: optional_string_array(sandbox, "allowedMounts", "merged settings.sandbox")?
            .unwrap_or_default(),
        backend: optional_string(sandbox, "backend", "merged settings.sandbox")?
            .map(ToOwned::to_owned),
        image: optional_string(sandbox, "image", "merged settings.sandbox")?.map(ToOwned::to_owned),
    })
}

fn parse_filesystem_mode_label(value: &str) -> Result<FilesystemIsolationMode, ConfigError> {
    match value {
        "off" => Ok(FilesystemIsolationMode::Off),
        "workspace-only" => Ok(FilesystemIsolationMode::WorkspaceOnly),
        "allow-list" => Ok(FilesystemIsolationMode::AllowList),
        other => Err(ConfigError::Parse(format!(
            "merged settings.sandbox.filesystemMode : mode de système de fichiers non supporté {other}"
        ))),
    }
}

fn parse_optional_oauth_config(
    root: &JsonValue,
    context: &str,
) -> Result<Option<OAuthConfig>, ConfigError> {
    let Some(oauth_value) = root.as_object().and_then(|object| object.get("oauth")) else {
        return Ok(None);
    };
    let object = expect_object(oauth_value, context)?;
    let client_id = expect_string(object, "clientId", context)?.to_string();
    let authorize_url = expect_string(object, "authorizeUrl", context)?.to_string();
    let token_url = expect_string(object, "tokenUrl", context)?.to_string();
    let callback_port = optional_u16(object, "callbackPort", context)?;
    let manual_redirect_url =
        optional_string(object, "manualRedirectUrl", context)?.map(str::to_string);
    let scopes = optional_string_array(object, "scopes", context)?.unwrap_or_default();
    Ok(Some(OAuthConfig {
        client_id,
        authorize_url,
        token_url,
        callback_port,
        manual_redirect_url,
        scopes,
    }))
}

fn parse_mcp_server_config(
    server_name: &str,
    value: &JsonValue,
    context: &str,
) -> Result<McpServerConfig, ConfigError> {
    let object = expect_object(value, context)?;
    let server_type = optional_string(object, "type", context)?.unwrap_or("stdio");
    match server_type {
        "stdio" => Ok(McpServerConfig::Stdio(McpStdioServerConfig {
            command: expect_string(object, "command", context)?.to_string(),
            args: optional_string_array(object, "args", context)?.unwrap_or_default(),
            env: optional_string_map(object, "env", context)?.unwrap_or_default(),
        })),
        "sse" => Ok(McpServerConfig::Sse(parse_mcp_remote_server_config(
            object, context,
        )?)),
        "http" => Ok(McpServerConfig::Http(parse_mcp_remote_server_config(
            object, context,
        )?)),
        "ws" => Ok(McpServerConfig::Ws(McpWebSocketServerConfig {
            url: expect_string(object, "url", context)?.to_string(),
            headers: optional_string_map(object, "headers", context)?.unwrap_or_default(),
            headers_helper: optional_string(object, "headersHelper", context)?.map(str::to_string),
        })),
        "sdk" => Ok(McpServerConfig::Sdk(McpSdkServerConfig {
            name: expect_string(object, "name", context)?.to_string(),
        })),
        "claudeai-proxy" => Ok(McpServerConfig::ManagedProxy(McpManagedProxyServerConfig {
            url: expect_string(object, "url", context)?.to_string(),
            id: expect_string(object, "id", context)?.to_string(),
        })),
        other => Err(ConfigError::Parse(format!(
            "{context} : type de serveur MCP non supporté pour {server_name} : {other}"
        ))),
    }
}

fn parse_mcp_remote_server_config(
    object: &BTreeMap<String, JsonValue>,
    context: &str,
) -> Result<McpRemoteServerConfig, ConfigError> {
    Ok(McpRemoteServerConfig {
        url: expect_string(object, "url", context)?.to_string(),
        headers: optional_string_map(object, "headers", context)?.unwrap_or_default(),
        headers_helper: optional_string(object, "headersHelper", context)?.map(str::to_string),
        oauth: parse_optional_mcp_oauth_config(object, context)?,
    })
}

fn parse_optional_mcp_oauth_config(
    object: &BTreeMap<String, JsonValue>,
    context: &str,
) -> Result<Option<McpOAuthConfig>, ConfigError> {
    let Some(value) = object.get("oauth") else {
        return Ok(None);
    };
    let oauth = expect_object(value, &format!("{context}.oauth"))?;
    Ok(Some(McpOAuthConfig {
        client_id: optional_string(oauth, "clientId", context)?.map(str::to_string),
        callback_port: optional_u16(oauth, "callbackPort", context)?,
        auth_server_metadata_url: optional_string(oauth, "authServerMetadataUrl", context)?
            .map(str::to_string),
        xaa: optional_bool(oauth, "xaa", context)?,
    }))
}

fn expect_object<'a>(
    value: &'a JsonValue,
    context: &str,
) -> Result<&'a BTreeMap<String, JsonValue>, ConfigError> {
    value
        .as_object()
        .ok_or_else(|| ConfigError::Parse(format!("{context} : objet JSON attendu")))
}

fn expect_string<'a>(
    object: &'a BTreeMap<String, JsonValue>,
    key: &str,
    context: &str,
) -> Result<&'a str, ConfigError> {
    object
        .get(key)
        .and_then(JsonValue::as_str)
        .ok_or_else(|| ConfigError::Parse(format!("{context} : champ chaîne manquant {key}")))
}

fn optional_string<'a>(
    object: &'a BTreeMap<String, JsonValue>,
    key: &str,
    context: &str,
) -> Result<Option<&'a str>, ConfigError> {
    match object.get(key) {
        Some(value) => value.as_str().map(Some).ok_or_else(|| {
            ConfigError::Parse(format!("{context} : le champ {key} doit être une chaîne"))
        }),
        None => Ok(None),
    }
}

fn optional_bool(
    object: &BTreeMap<String, JsonValue>,
    key: &str,
    context: &str,
) -> Result<Option<bool>, ConfigError> {
    match object.get(key) {
        Some(value) => value.as_bool().map(Some).ok_or_else(|| {
            ConfigError::Parse(format!("{context} : le champ {key} doit être un booléen"))
        }),
        None => Ok(None),
    }
}

fn optional_u16(
    object: &BTreeMap<String, JsonValue>,
    key: &str,
    context: &str,
) -> Result<Option<u16>, ConfigError> {
    match object.get(key) {
        Some(value) => {
            let Some(number) = value.as_i64() else {
                return Err(ConfigError::Parse(format!(
                    "{context} : le champ {key} doit être un entier"
                )));
            };
            let number = u16::try_from(number).map_err(|_| {
                ConfigError::Parse(format!("{context} : le champ {key} est hors plage"))
            })?;
            Ok(Some(number))
        }
        None => Ok(None),
    }
}

fn parse_bool_map(value: &JsonValue, context: &str) -> Result<BTreeMap<String, bool>, ConfigError> {
    let Some(map) = value.as_object() else {
        return Err(ConfigError::Parse(format!(
            "{context} : objet JSON attendu"
        )));
    };
    map.iter()
        .map(|(key, value)| {
            value
                .as_bool()
                .map(|enabled| (key.clone(), enabled))
                .ok_or_else(|| {
                    ConfigError::Parse(format!("{context} : le champ {key} doit être un booléen"))
                })
        })
        .collect()
}

fn optional_string_array(
    object: &BTreeMap<String, JsonValue>,
    key: &str,
    context: &str,
) -> Result<Option<Vec<String>>, ConfigError> {
    match object.get(key) {
        Some(value) => {
            let Some(array) = value.as_array() else {
                return Err(ConfigError::Parse(format!(
                    "{context} : le champ {key} doit être un tableau"
                )));
            };
            array
                .iter()
                .map(|item| {
                    item.as_str().map(ToOwned::to_owned).ok_or_else(|| {
                        ConfigError::Parse(format!(
                            "{context} : le champ {key} ne doit contenir que des chaînes"
                        ))
                    })
                })
                .collect::<Result<Vec<_>, _>>()
                .map(Some)
        }
        None => Ok(None),
    }
}

fn optional_string_map(
    object: &BTreeMap<String, JsonValue>,
    key: &str,
    context: &str,
) -> Result<Option<BTreeMap<String, String>>, ConfigError> {
    match object.get(key) {
        Some(value) => {
            let Some(map) = value.as_object() else {
                return Err(ConfigError::Parse(format!(
                    "{context} : le champ {key} doit être un objet"
                )));
            };
            map.iter()
                .map(|(entry_key, entry_value)| {
                    entry_value
                        .as_str()
                        .map(|text| (entry_key.clone(), text.to_string()))
                        .ok_or_else(|| {
                            ConfigError::Parse(format!(
                                "{context} : le champ {key} ne doit contenir que des valeurs chaînes"
                            ))
                        })
                })
                .collect::<Result<BTreeMap<_, _>, _>>()
                .map(Some)
        }
        None => Ok(None),
    }
}

fn deep_merge_objects(
    target: &mut BTreeMap<String, JsonValue>,
    source: &BTreeMap<String, JsonValue>,
) {
    for (key, value) in source {
        match (target.get_mut(key), value) {
            (Some(JsonValue::Object(existing)), JsonValue::Object(incoming)) => {
                deep_merge_objects(existing, incoming);
            }
            _ => {
                target.insert(key.clone(), value.clone());
            }
        }
    }
}

fn extend_unique(target: &mut Vec<String>, values: &[String]) {
    for value in values {
        push_unique(target, value.clone());
    }
}

fn push_unique(target: &mut Vec<String>, value: String) {
    if !target.iter().any(|existing| existing == &value) {
        target.push(value);
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ConfigLoader, ConfigSource, McpServerConfig, McpTransport, ResolvedPermissionMode,
        CLAW_SETTINGS_SCHEMA_NAME,
    };
    use crate::json::JsonValue;
    use crate::sandbox::FilesystemIsolationMode;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir() -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("le temps devrait être postérieur à l'epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("runtime-config-{nanos}"))
    }

    #[test]
    fn rejects_non_object_settings_files() {
        let root = temp_dir();
        let cwd = root.join("project");
        let home = root.join("home").join(".claw");
        fs::create_dir_all(&home).expect("répertoire de config home");
        fs::create_dir_all(&cwd).expect("répertoire du projet");
        fs::write(home.join("settings.json"), "[]").expect("écriture des paramètres invalides");

        let error = ConfigLoader::new(&cwd, &home)
            .load()
            .expect_err("la config devrait échouer");
        assert!(error
            .to_string()
            .contains("la valeur racine des paramètres doit être un objet JSON"));

        fs::remove_dir_all(root).expect("nettoyage du répertoire temp");
    }

    #[test]
    fn concatenates_permission_rules_across_config_files() {
        let root = temp_dir();
        let cwd = root.join("project");
        let home = root.join("home").join(".claw");
        fs::create_dir_all(cwd.join(".claw")).expect("répertoire de config du projet");
        fs::create_dir_all(&home).expect("répertoire de config home");

        fs::write(
            home.join("settings.json"),
            r#"{"language":"français","permissions":{"defaultMode":"default","deny":["Read(.env)"],"allow":["Bash(git status)"]}}"#,
        )
        .expect("écriture des paramètres utilisateur");
        fs::write(
            cwd.join(".claw").join("settings.local.json"),
            r#"{"permissions":{"allow":["Bash(cargo test:*)","Bash(git status)"],"ask":["WebFetch"]},"agents":{"reviewer":{"model":"openai/gpt-5"}}}"#,
        )
        .expect("écriture des paramètres locaux");

        let loaded = ConfigLoader::new(&cwd, &home)
            .load()
            .expect("la config devrait se charger");

        assert_eq!(
            loaded.permission_mode(),
            Some(ResolvedPermissionMode::Prompt)
        );
        assert_eq!(loaded.language(), Some("français"));
        assert_eq!(loaded.agent_model("Reviewer"), Some("openai/gpt-5"));
        assert_eq!(loaded.agent_model("architect"), None);
        let rules = loaded.permission_rules();
        assert_eq!(rules.allow(), ["Bash(git status)", "Bash(cargo test:*)"]);
        assert_eq!(rules.deny(), ["Read(.env)"]);
        assert_eq!(rules.ask(), ["WebFetch"]);

        fs::remove_dir_all(root).expect("nettoyage du répertoire temp");
    }

    #[test]
    fn loads_and_merges_claw_code_config_files_by_precedence() {
        let root = temp_dir();
        let cwd = root.join("project");
        let home = root.join("home").join(".claw");
        fs::create_dir_all(cwd.join(".claw")).expect("répertoire de config du projet");
        fs::create_dir_all(&home).expect("répertoire de config home");

        fs::write(
            home.parent().expect("parent du home").join(".claw.json"),
            r#"{"model":"haiku","env":{"A":"1"},"mcpServers":{"home":{"command":"uvx","args":["home"]}}}"#,
        )
        .expect("écriture de la config de compat utilisateur");
        fs::write(
            home.join("settings.json"),
            r#"{"model":"sonnet","env":{"A2":"1"},"hooks":{"PreToolUse":["base"]},"permissions":{"defaultMode":"plan"}}"#,
        )
        .expect("écriture des paramètres utilisateur");
        fs::write(
            cwd.join(".claw.json"),
            r#"{"model":"project-compat","env":{"B":"2"}}"#,
        )
        .expect("écriture de la config de compat projet");
        fs::write(
            cwd.join(".claw").join("settings.json"),
            r#"{"env":{"C":"3"},"hooks":{"PostToolUse":["project"]},"mcpServers":{"project":{"command":"uvx","args":["project"]}}}"#,
        )
        .expect("écriture des paramètres du projet");
        fs::write(
            cwd.join(".claw").join("settings.local.json"),
            r#"{"model":"opus","permissionMode":"acceptEdits"}"#,
        )
        .expect("écriture des paramètres locaux");

        let loaded = ConfigLoader::new(&cwd, &home)
            .load()
            .expect("la config devrait se charger");

        assert_eq!(CLAW_SETTINGS_SCHEMA_NAME, "SettingsSchema");
        assert_eq!(loaded.loaded_entries().len(), 5);
        assert_eq!(loaded.loaded_entries()[0].source, ConfigSource::User);
        assert_eq!(
            loaded.get("model"),
            Some(&JsonValue::String("opus".to_string()))
        );
        assert_eq!(loaded.model(), Some("opus"));
        assert_eq!(
            loaded.permission_mode(),
            Some(ResolvedPermissionMode::WorkspaceWrite)
        );
        assert_eq!(
            loaded
                .get("env")
                .and_then(JsonValue::as_object)
                .expect("objet env")
                .len(),
            4
        );
        assert!(loaded
            .get("hooks")
            .and_then(JsonValue::as_object)
            .expect("objet hooks")
            .contains_key("PreToolUse"));
        assert!(loaded
            .get("hooks")
            .and_then(JsonValue::as_object)
            .expect("objet hooks")
            .contains_key("PostToolUse"));
        assert_eq!(loaded.hooks().pre_tool_use(), vec!["base"]);
        assert_eq!(loaded.hooks().post_tool_use(), vec!["project"]);
        assert!(loaded.mcp().get("home").is_some());
        assert!(loaded.mcp().get("project").is_some());

        fs::remove_dir_all(root).expect("nettoyage du répertoire temp");
    }

    #[test]
    fn parses_sandbox_config() {
        let root = temp_dir();
        let cwd = root.join("project");
        let home = root.join("home").join(".claw");
        fs::create_dir_all(cwd.join(".claw")).expect("répertoire de config du projet");
        fs::create_dir_all(&home).expect("répertoire de config home");

        fs::write(
            cwd.join(".claw").join("settings.local.json"),
            r#"{
              "sandbox": {
                "enabled": true,
                "namespaceRestrictions": false,
                "networkIsolation": true,
                "filesystemMode": "allow-list",
                "allowedMounts": ["logs", "tmp/cache"]
              }
            }"#,
        )
        .expect("écriture des paramètres locaux");

        let loaded = ConfigLoader::new(&cwd, &home)
            .load()
            .expect("la config devrait se charger");

        assert_eq!(loaded.sandbox().enabled, Some(true));
        assert_eq!(loaded.sandbox().namespace_restrictions, Some(false));
        assert_eq!(loaded.sandbox().network_isolation, Some(true));
        assert_eq!(
            loaded.sandbox().filesystem_mode,
            Some(FilesystemIsolationMode::AllowList)
        );
        assert_eq!(loaded.sandbox().allowed_mounts, vec!["logs", "tmp/cache"]);

        fs::remove_dir_all(root).expect("nettoyage du répertoire temp");
    }

    #[test]
    fn parses_typed_mcp_and_oauth_config() {
        let root = temp_dir();
        let cwd = root.join("project");
        let home = root.join("home").join(".claw");
        fs::create_dir_all(cwd.join(".claw")).expect("répertoire de config du projet");
        fs::create_dir_all(&home).expect("répertoire de config home");

        fs::write(
            home.join("settings.json"),
            r#"{
              "mcpServers": {
                "stdio-server": {
                  "command": "uvx",
                  "args": ["mcp-server"],
                  "env": {"TOKEN": "secret"}
                },
                "remote-server": {
                  "type": "http",
                  "url": "https://example.test/mcp",
                  "headers": {"Authorization": "Bearer token"},
                  "headersHelper": "helper.sh",
                  "oauth": {
                    "clientId": "mcp-client",
                    "callbackPort": 7777,
                    "authServerMetadataUrl": "https://issuer.test/.well-known/oauth-authorization-server",
                    "xaa": true
                  }
                }
              },
              "oauth": {
                "clientId": "runtime-client",
                "authorizeUrl": "https://console.test/oauth/authorize",
                "tokenUrl": "https://console.test/oauth/token",
                "callbackPort": 54545,
                "manualRedirectUrl": "https://console.test/oauth/callback",
                "scopes": ["org:read", "user:write"]
              }
            }"#,
        )
        .expect("écriture des paramètres utilisateur");
        fs::write(
            cwd.join(".claw").join("settings.local.json"),
            r#"{
              "mcpServers": {
                "remote-server": {
                  "type": "ws",
                  "url": "wss://override.test/mcp",
                  "headers": {"X-Env": "local"}
                }
              }
            }"#,
        )
        .expect("écriture des paramètres locaux");

        let loaded = ConfigLoader::new(&cwd, &home)
            .load()
            .expect("la config devrait se charger");

        let stdio_server = loaded
            .mcp()
            .get("stdio-server")
            .expect("le serveur stdio devrait exister");
        assert_eq!(stdio_server.scope, ConfigSource::User);
        assert_eq!(stdio_server.transport(), McpTransport::Stdio);

        let remote_server = loaded
            .mcp()
            .get("remote-server")
            .expect("le serveur distant devrait exister");
        assert_eq!(remote_server.scope, ConfigSource::Local);
        assert_eq!(remote_server.transport(), McpTransport::Ws);
        match &remote_server.config {
            McpServerConfig::Ws(config) => {
                assert_eq!(config.url, "wss://override.test/mcp");
                assert_eq!(
                    config.headers.get("X-Env").map(String::as_str),
                    Some("local")
                );
            }
            other => panic!("config ws attendue, obtenu {other:?}"),
        }

        let oauth = loaded.oauth().expect("la config oauth devrait exister");
        assert_eq!(oauth.client_id, "runtime-client");
        assert_eq!(oauth.callback_port, Some(54_545));
        assert_eq!(oauth.scopes, vec!["org:read", "user:write"]);

        fs::remove_dir_all(root).expect("nettoyage du répertoire temp");
    }

    #[test]
    fn parses_plugin_config_from_enabled_plugins() {
        let root = temp_dir();
        let cwd = root.join("project");
        let home = root.join("home").join(".claw");
        fs::create_dir_all(cwd.join(".claw")).expect("project config dir");
        fs::create_dir_all(&home).expect("home config dir");

        fs::write(
            home.join("settings.json"),
            r#"{
              "enabledPlugins": {
                "tool-guard@builtin": true,
                "sample-plugin@external": false
              }
            }"#,
        )
        .expect("write user settings");

        let loaded = ConfigLoader::new(&cwd, &home)
            .load()
            .expect("config should load");

        assert_eq!(
            loaded.plugins().enabled_plugins().get("tool-guard@builtin"),
            Some(&true)
        );
        assert_eq!(
            loaded
                .plugins()
                .enabled_plugins()
                .get("sample-plugin@external"),
            Some(&false)
        );

        fs::remove_dir_all(root).expect("nettoyage du répertoire temp");
    }

    #[test]
    fn parses_plugin_config() {
        let root = temp_dir();
        let cwd = root.join("project");
        let home = root.join("home").join(".claw");
        fs::create_dir_all(cwd.join(".claw")).expect("project config dir");
        fs::create_dir_all(&home).expect("home config dir");

        fs::write(
            home.join("settings.json"),
            r#"{
              "enabledPlugins": {
                "core-helpers@builtin": true
              },
              "plugins": {
                "externalDirectories": ["./external-plugins"],
                "installRoot": "plugin-cache/installed",
                "registryPath": "plugin-cache/installed.json",
                "bundledRoot": "./bundled-plugins"
              }
            }"#,
        )
        .expect("write plugin settings");

        let loaded = ConfigLoader::new(&cwd, &home)
            .load()
            .expect("config should load");

        assert_eq!(
            loaded
                .plugins()
                .enabled_plugins()
                .get("core-helpers@builtin"),
            Some(&true)
        );
        assert_eq!(
            loaded.plugins().external_directories(),
            &["./external-plugins".to_string()]
        );
        assert_eq!(
            loaded.plugins().install_root(),
            Some("plugin-cache/installed")
        );
        assert_eq!(
            loaded.plugins().registry_path(),
            Some("plugin-cache/installed.json")
        );
        assert_eq!(loaded.plugins().bundled_root(), Some("./bundled-plugins"));

        fs::remove_dir_all(root).expect("nettoyage du répertoire temp");
    }

    #[test]
    fn rejects_invalid_mcp_server_shapes() {
        let root = temp_dir();
        let cwd = root.join("project");
        let home = root.join("home").join(".claw");
        fs::create_dir_all(&home).expect("home config dir");
        fs::create_dir_all(&cwd).expect("project dir");
        fs::write(
            home.join("settings.json"),
            r#"{"mcpServers":{"broken":{"type":"http","url":123}}}"#,
        )
        .expect("write broken settings");

        let error = ConfigLoader::new(&cwd, &home)
            .load()
            .expect_err("config should fail");
        assert!(error
            .to_string()
            .contains("mcpServers.broken : champ chaîne manquant url"));

        fs::remove_dir_all(root).expect("nettoyage du répertoire temp");
    }
}
