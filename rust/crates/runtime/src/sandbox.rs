use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "kebab-case")]
pub enum FilesystemIsolationMode {
    Off,
    #[default]
    WorkspaceOnly,
    AllowList,
}

impl FilesystemIsolationMode {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::WorkspaceOnly => "workspace-only",
            Self::AllowList => "allow-list",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct SandboxConfig {
    pub enabled: Option<bool>,
    pub namespace_restrictions: Option<bool>,
    pub network_isolation: Option<bool>,
    pub filesystem_mode: Option<FilesystemIsolationMode>,
    pub allowed_mounts: Vec<String>,
    /// `docker` ou `podman` : commandes exécutées dans un conteneur (tous systèmes).
    #[serde(default)]
    pub backend: Option<String>,
    /// Image du conteneur (défaut : `debian:stable-slim`).
    #[serde(default)]
    pub image: Option<String>,
}

/// Exécution des commandes dans un conteneur : seul le projet est monté.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ContainerSandbox {
    pub program: String,
    pub image: String,
    pub network_isolation: bool,
    pub extra_mounts: Vec<String>,
}

impl SandboxConfig {
    /// Moteur de conteneur choisi (`CLAW_SANDBOX` puis `sandbox.backend`).
    #[must_use]
    pub fn container(&self) -> Option<ContainerSandbox> {
        let backend = std::env::var("CLAW_SANDBOX")
            .ok()
            .or_else(|| self.backend.clone())?
            .trim()
            .to_ascii_lowercase();
        if !matches!(backend.as_str(), "docker" | "podman") || self.enabled == Some(false) {
            return None;
        }
        Some(ContainerSandbox {
            program: backend,
            image: std::env::var("CLAW_SANDBOX_IMAGE")
                .ok()
                .or_else(|| self.image.clone())
                .unwrap_or_else(|| "debian:stable-slim".to_string()),
            network_isolation: self.network_isolation.unwrap_or(false),
            extra_mounts: self.allowed_mounts.clone(),
        })
    }
}

static CONTAINER_SEQUENCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

impl ContainerSandbox {
    /// Arguments de `docker run` (ou `podman run`) pour `command` dans `cwd` ;
    /// renvoie aussi le nom donné au conteneur (pour l'arrêter).
    #[must_use]
    pub fn run_args(&self, command: &str, cwd: &Path) -> (Vec<String>, String) {
        let name = format!(
            "claw-{}-{}",
            std::process::id(),
            CONTAINER_SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        );
        let mut args = vec![
            "run".to_string(),
            "--rm".to_string(),
            "-i".to_string(),
            "--init".to_string(),
            "--name".to_string(),
            name.clone(),
            "-v".to_string(),
            format!("{}:/workspace", cwd.display()),
            "-w".to_string(),
            "/workspace".to_string(),
            "-e".to_string(),
            "HOME=/tmp".to_string(),
        ];
        if self.network_isolation {
            args.push("--network".to_string());
            args.push("none".to_string());
        }
        for mount in &self.extra_mounts {
            args.push("-v".to_string());
            args.push(format!("{mount}:{mount}:ro"));
        }
        args.push(self.image.clone());
        args.push("sh".to_string());
        args.push("-lc".to_string());
        args.push(command.to_string());
        (args, name)
    }

    /// Arrête et supprime un conteneur (interruption, délai dépassé, fin de Claw).
    pub fn stop(&self, name: &str) {
        let _ = std::process::Command::new(&self.program)
            .args(["rm", "-f", name])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct SandboxRequest {
    pub enabled: bool,
    pub namespace_restrictions: bool,
    pub network_isolation: bool,
    pub filesystem_mode: FilesystemIsolationMode,
    pub allowed_mounts: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ContainerEnvironment {
    pub in_container: bool,
    pub markers: Vec<String>,
}

#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct SandboxStatus {
    pub enabled: bool,
    pub requested: SandboxRequest,
    pub supported: bool,
    pub active: bool,
    pub namespace_supported: bool,
    pub namespace_active: bool,
    pub network_supported: bool,
    pub network_active: bool,
    pub filesystem_mode: FilesystemIsolationMode,
    pub filesystem_active: bool,
    pub allowed_mounts: Vec<String>,
    pub in_container: bool,
    pub container_markers: Vec<String>,
    pub fallback_reason: Option<String>,
    /// Commandes exécutées dans un conteneur Docker/Podman.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub container: Option<ContainerSandbox>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxDetectionInputs<'a> {
    pub env_pairs: Vec<(String, String)>,
    pub dockerenv_exists: bool,
    pub containerenv_exists: bool,
    pub proc_1_cgroup: Option<&'a str>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinuxSandboxCommand {
    pub program: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
}

impl SandboxConfig {
    #[must_use]
    pub fn resolve_request(
        &self,
        enabled_override: Option<bool>,
        namespace_override: Option<bool>,
        network_override: Option<bool>,
        filesystem_mode_override: Option<FilesystemIsolationMode>,
        allowed_mounts_override: Option<Vec<String>>,
    ) -> SandboxRequest {
        SandboxRequest {
            enabled: enabled_override.unwrap_or(self.enabled.unwrap_or(true)),
            namespace_restrictions: namespace_override
                .unwrap_or(self.namespace_restrictions.unwrap_or(true)),
            network_isolation: network_override.unwrap_or(self.network_isolation.unwrap_or(false)),
            filesystem_mode: filesystem_mode_override
                .or(self.filesystem_mode)
                .unwrap_or_default(),
            allowed_mounts: allowed_mounts_override.unwrap_or_else(|| self.allowed_mounts.clone()),
        }
    }
}

#[must_use]
pub fn detect_container_environment() -> ContainerEnvironment {
    let proc_1_cgroup = fs::read_to_string("/proc/1/cgroup").ok();
    detect_container_environment_from(SandboxDetectionInputs {
        env_pairs: env::vars().collect(),
        dockerenv_exists: Path::new("/.dockerenv").exists(),
        containerenv_exists: Path::new("/run/.containerenv").exists(),
        proc_1_cgroup: proc_1_cgroup.as_deref(),
    })
}

#[must_use]
pub fn detect_container_environment_from(
    inputs: SandboxDetectionInputs<'_>,
) -> ContainerEnvironment {
    let mut markers = Vec::new();
    if inputs.dockerenv_exists {
        markers.push("/.dockerenv".to_string());
    }
    if inputs.containerenv_exists {
        markers.push("/run/.containerenv".to_string());
    }
    for (key, value) in inputs.env_pairs {
        let normalized = key.to_ascii_lowercase();
        if matches!(
            normalized.as_str(),
            "container" | "docker" | "podman" | "kubernetes_service_host"
        ) && !value.is_empty()
        {
            markers.push(format!("env:{key}={value}"));
        }
    }
    if let Some(cgroup) = inputs.proc_1_cgroup {
        for needle in ["docker", "containerd", "kubepods", "podman", "libpod"] {
            if cgroup.contains(needle) {
                markers.push(format!("/proc/1/cgroup:{needle}"));
            }
        }
    }
    markers.sort();
    markers.dedup();
    ContainerEnvironment {
        in_container: !markers.is_empty(),
        markers,
    }
}

#[must_use]
pub fn resolve_sandbox_status(config: &SandboxConfig, cwd: &Path) -> SandboxStatus {
    let request = config.resolve_request(None, None, None, None, None);
    resolve_sandbox_status_for_request(&request, cwd)
}

#[must_use]
pub fn resolve_sandbox_status_for_request(request: &SandboxRequest, cwd: &Path) -> SandboxStatus {
    let container = detect_container_environment();
    let namespace_supported =
        cfg!(target_os = "linux") && command_exists("unshare") && user_namespaces_allowed();
    let network_supported = namespace_supported;
    let filesystem_active =
        request.enabled && request.filesystem_mode != FilesystemIsolationMode::Off;
    let mut fallback_reasons = Vec::new();

    if request.enabled && request.namespace_restrictions && !namespace_supported {
        fallback_reasons.push(
            "isolation par namespace indisponible (nécessite Linux avec `unshare` et les namespaces utilisateur autorisés)".to_string(),
        );
    }
    if request.enabled && request.network_isolation && !network_supported {
        fallback_reasons
            .push("isolation réseau indisponible (nécessite Linux avec `unshare`)".to_string());
    }
    if request.enabled
        && request.filesystem_mode == FilesystemIsolationMode::AllowList
        && request.allowed_mounts.is_empty()
    {
        fallback_reasons.push(
            "liste blanche de système de fichiers demandée sans points de montage configurés"
                .to_string(),
        );
    }

    let active = request.enabled
        && (!request.namespace_restrictions || namespace_supported)
        && (!request.network_isolation || network_supported);

    let allowed_mounts = normalize_mounts(&request.allowed_mounts, cwd);

    SandboxStatus {
        enabled: request.enabled,
        requested: request.clone(),
        supported: namespace_supported,
        active,
        namespace_supported,
        namespace_active: request.enabled && request.namespace_restrictions && namespace_supported,
        network_supported,
        network_active: request.enabled && request.network_isolation && network_supported,
        filesystem_mode: request.filesystem_mode,
        filesystem_active,
        allowed_mounts,
        in_container: container.in_container,
        container_markers: container.markers,
        fallback_reason: (!fallback_reasons.is_empty()).then(|| fallback_reasons.join("; ")),
        container: None,
    }
}

#[must_use]
pub fn build_linux_sandbox_command(
    command: &str,
    cwd: &Path,
    status: &SandboxStatus,
) -> Option<LinuxSandboxCommand> {
    if !cfg!(target_os = "linux")
        || !status.enabled
        || (!status.namespace_active && !status.network_active)
    {
        return None;
    }

    let mut args = vec![
        "--user".to_string(),
        "--map-root-user".to_string(),
        "--mount".to_string(),
        "--ipc".to_string(),
        "--pid".to_string(),
        "--uts".to_string(),
        "--fork".to_string(),
    ];
    if status.network_active {
        args.push("--net".to_string());
    }
    args.push("sh".to_string());
    args.push("-lc".to_string());
    args.push(command.to_string());

    let sandbox_home = cwd.join(".sandbox-home");
    let sandbox_tmp = cwd.join(".sandbox-tmp");
    let mut env = vec![
        ("HOME".to_string(), sandbox_home.display().to_string()),
        ("TMPDIR".to_string(), sandbox_tmp.display().to_string()),
        (
            "CLAW_SANDBOX_FILESYSTEM_MODE".to_string(),
            status.filesystem_mode.as_str().to_string(),
        ),
        (
            "CLAW_SANDBOX_ALLOWED_MOUNTS".to_string(),
            status.allowed_mounts.join(":"),
        ),
    ];
    if let Ok(path) = env::var("PATH") {
        env.push(("PATH".to_string(), path));
    }

    Some(LinuxSandboxCommand {
        program: "unshare".to_string(),
        args,
        env,
    })
}

fn normalize_mounts(mounts: &[String], cwd: &Path) -> Vec<String> {
    let cwd = cwd.to_path_buf();
    mounts
        .iter()
        .map(|mount| {
            let path = PathBuf::from(mount);
            if path.is_absolute() {
                path
            } else {
                cwd.join(path)
            }
        })
        .map(|path| path.display().to_string())
        .collect()
}

/// `unshare --user` peut être refusé même s'il est installé (Ubuntu 24.04 et
/// AppArmor, conteneurs, runners de CI) : on l'essaie une fois avant de s'en
/// servir, sinon les commandes s'exécutent sans isolation au lieu d'échouer.
fn user_namespaces_allowed() -> bool {
    static ALLOWED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ALLOWED.get_or_init(|| {
        std::process::Command::new("unshare")
            .args(["--user", "--map-root-user", "--mount", "--fork", "true"])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    })
}

fn command_exists(command: &str) -> bool {
    env::var_os("PATH")
        .is_some_and(|paths| env::split_paths(&paths).any(|path| path.join(command).exists()))
}

#[cfg(test)]
mod tests {
    use super::{
        build_linux_sandbox_command, detect_container_environment_from, FilesystemIsolationMode,
        SandboxConfig, SandboxDetectionInputs,
    };
    use std::path::Path;

    #[test]
    fn detects_container_markers_from_multiple_sources() {
        let detected = detect_container_environment_from(SandboxDetectionInputs {
            env_pairs: vec![("container".to_string(), "docker".to_string())],
            dockerenv_exists: true,
            containerenv_exists: false,
            proc_1_cgroup: Some("12:memory:/docker/abc"),
        });

        assert!(detected.in_container);
        assert!(detected
            .markers
            .iter()
            .any(|marker| marker == "/.dockerenv"));
        assert!(detected
            .markers
            .iter()
            .any(|marker| marker == "env:container=docker"));
        assert!(detected
            .markers
            .iter()
            .any(|marker| marker == "/proc/1/cgroup:docker"));
    }

    #[test]
    fn resolves_request_with_overrides() {
        let config = SandboxConfig {
            enabled: Some(true),
            namespace_restrictions: Some(true),
            network_isolation: Some(false),
            filesystem_mode: Some(FilesystemIsolationMode::WorkspaceOnly),
            allowed_mounts: vec!["logs".to_string()],
            ..SandboxConfig::default()
        };

        let request = config.resolve_request(
            Some(true),
            Some(false),
            Some(true),
            Some(FilesystemIsolationMode::AllowList),
            Some(vec!["tmp".to_string()]),
        );

        assert!(request.enabled);
        assert!(!request.namespace_restrictions);
        assert!(request.network_isolation);
        assert_eq!(request.filesystem_mode, FilesystemIsolationMode::AllowList);
        assert_eq!(request.allowed_mounts, vec!["tmp"]);
    }

    #[test]
    fn builds_linux_launcher_with_network_flag_when_requested() {
        let config = SandboxConfig::default();
        let status = super::resolve_sandbox_status_for_request(
            &config.resolve_request(
                Some(true),
                Some(true),
                Some(true),
                Some(FilesystemIsolationMode::WorkspaceOnly),
                None,
            ),
            Path::new("/workspace"),
        );

        if let Some(launcher) =
            build_linux_sandbox_command("printf hi", Path::new("/workspace"), &status)
        {
            assert_eq!(launcher.program, "unshare");
            assert!(launcher.args.iter().any(|arg| arg == "--mount"));
            assert!(launcher.args.iter().any(|arg| arg == "--net") == status.network_active);
        }
    }
}

#[cfg(test)]
mod container_tests {
    use super::SandboxConfig;
    use std::path::Path;

    #[test]
    fn container_backend_mounts_only_the_workspace() {
        let config = SandboxConfig {
            backend: Some("podman".to_string()),
            image: Some("rust:1".to_string()),
            network_isolation: Some(true),
            allowed_mounts: vec!["/data".to_string()],
            ..SandboxConfig::default()
        };
        let container = config.container().expect("backend configuré");
        assert_eq!(container.program, "podman");
        let (args, name) = container.run_args("cargo test", Path::new("/projet"));
        assert!(name.starts_with("claw-"));
        assert_eq!(&args[..4], ["run", "--rm", "-i", "--init"]);
        assert!(args
            .windows(2)
            .any(|pair| pair == ["-v", "/projet:/workspace"]));
        assert!(args.windows(2).any(|pair| pair == ["--network", "none"]));
        assert!(args.windows(2).any(|pair| pair == ["-v", "/data:/data:ro"]));
        assert_eq!(
            &args[args.len() - 4..],
            ["rust:1", "sh", "-lc", "cargo test"]
        );

        let disabled = SandboxConfig {
            backend: Some("docker".to_string()),
            enabled: Some(false),
            ..SandboxConfig::default()
        };
        assert!(disabled.container().is_none());
        assert!(
            SandboxConfig::default().container().is_none() || std::env::var("CLAW_SANDBOX").is_ok()
        );
    }
}
