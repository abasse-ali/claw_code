//! Bac à sable conteneur avec un vrai moteur (désactivé par défaut) :
//!
//! ```text
//! CLAW_TEST_CONTAINER=docker cargo test -p runtime --test container_sandbox -- --ignored --test-threads=1
//! ```
//!
//! `CLAW_TEST_CONTAINER` vaut `docker` ou `podman` ; l'image `docker.io/library/debian:stable-slim`
//! est téléchargée au premier lancement.

use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

use runtime::{bash_tasks, execute_bash, BashCommandInput, BashCommandOutput};

fn engine() -> String {
    std::env::var("CLAW_TEST_CONTAINER")
        .expect("définir CLAW_TEST_CONTAINER=docker ou CLAW_TEST_CONTAINER=podman")
}

/// Projet temporaire configuré pour le bac à sable conteneur, devenu le
/// dossier courant le temps du test.
struct Project {
    root: PathBuf,
    previous: PathBuf,
}

impl Project {
    fn new(label: &str, extra_settings: &str) -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("horloge")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("claw-conteneur-{label}-{nanos}"));
        std::fs::create_dir_all(root.join(".claw")).expect("projet");
        std::fs::write(
            root.join(".claw").join("settings.json"),
            format!(
                r#"{{"sandbox": {{"backend": "{}", "image": "docker.io/library/debian:stable-slim"{extra_settings}}}}}"#,
                engine()
            ),
        )
        .expect("réglages");
        std::env::set_var("CLAW_CONFIG_HOME", root.join(".claw-home"));
        let previous = std::env::current_dir().expect("dossier courant");
        std::env::set_current_dir(&root).expect("entrée dans le projet");
        Self { root, previous }
    }

    fn path(&self, name: &str) -> PathBuf {
        self.root.join(name)
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        let _ = std::env::set_current_dir(&self.previous);
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn input(command: &str, timeout_ms: u64) -> BashCommandInput {
    BashCommandInput {
        command: command.to_string(),
        timeout: Some(timeout_ms),
        description: None,
        run_in_background: Some(false),
        dangerously_disable_sandbox: None,
        namespace_restrictions: None,
        isolate_network: None,
        filesystem_mode: None,
        allowed_mounts: None,
    }
}

fn run(command: &str) -> BashCommandOutput {
    execute_bash(input(command, 180_000)).expect("exécution")
}

/// Conteneurs de ce processus encore présents (en cours ou arrêtés).
fn leftover_containers() -> String {
    let output = Command::new(engine())
        .args([
            "ps",
            "-a",
            "--filter",
            &format!("name=claw-{}-", std::process::id()),
            "--format",
            "{{.Names}}",
        ])
        .output()
        .expect("liste des conteneurs");
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn wait_until_no_container() {
    let deadline = Instant::now() + Duration::from_secs(20);
    while !leftover_containers().is_empty() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(250));
    }
    assert_eq!(leftover_containers(), "", "conteneur non supprimé");
}

#[test]
#[ignore = "nécessite Docker ou Podman (CLAW_TEST_CONTAINER)"]
fn commands_run_inside_the_container_in_the_workspace() {
    let _project = Project::new("dedans", "");
    let output = run("cat /etc/os-release; pwd; echo \"HOME=$HOME\"");
    assert!(output.stdout.contains("Debian"), "{output:?}");
    assert!(output.stdout.contains("/workspace"), "{output:?}");
    assert!(output.stdout.contains("HOME=/tmp"), "{output:?}");
    wait_until_no_container();
}

#[test]
#[ignore = "nécessite Docker ou Podman (CLAW_TEST_CONTAINER)"]
fn workspace_files_are_shared_and_owned_by_the_user() {
    let project = Project::new("fichiers", "");
    std::fs::write(project.path("hote.txt"), "écrit par l'hôte").expect("fichier hôte");
    let output = run("cat hote.txt && echo 'écrit par le conteneur' > conteneur.txt");
    assert!(output.stdout.contains("écrit par l'hôte"), "{output:?}");
    let created = project.path("conteneur.txt");
    assert_eq!(
        std::fs::read_to_string(&created).expect("fichier créé dans le conteneur"),
        "écrit par le conteneur\n"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let me = std::fs::metadata("/proc/self").map_or(0, |metadata| metadata.uid());
        let owner = std::fs::metadata(&created).expect("métadonnées").uid();
        assert_eq!(owner, me, "le fichier créé doit appartenir à l'utilisateur");
        // Et l'utilisateur peut le supprimer sans droits root.
        std::fs::remove_file(&created).expect("suppression par l'utilisateur");
    }
}

#[test]
#[ignore = "nécessite Docker ou Podman (CLAW_TEST_CONTAINER)"]
fn files_outside_the_workspace_are_not_visible() {
    let outside = std::env::temp_dir().join(format!("claw-secret-{}", std::process::id()));
    std::fs::write(&outside, "secret").expect("fichier hors projet");
    let _project = Project::new("hors-projet", "");
    let output = run(&format!("cat '{}' || echo INACCESSIBLE", outside.display()));
    let _ = std::fs::remove_file(&outside);
    assert!(output.stdout.contains("INACCESSIBLE"), "{output:?}");
    assert!(!output.stdout.contains("secret"), "{output:?}");
}

#[test]
#[ignore = "nécessite Docker ou Podman (CLAW_TEST_CONTAINER)"]
fn network_isolation_leaves_only_loopback() {
    {
        let _project = Project::new("reseau", r#", "networkIsolation": true"#);
        let output = run("ls /sys/class/net");
        assert_eq!(
            output.stdout.split_whitespace().collect::<Vec<_>>(),
            ["lo"],
            "{output:?}"
        );
    }
    // Option de la commande : isolement demandé sans réglage global.
    let _project = Project::new("reseau-commande", "");
    let mut request = input("ls /sys/class/net", 180_000);
    request.isolate_network = Some(true);
    let output = execute_bash(request).expect("exécution");
    assert_eq!(
        output.stdout.split_whitespace().collect::<Vec<_>>(),
        ["lo"],
        "{output:?}"
    );
    // Sans isolement, le conteneur a une interface réseau en plus de lo.
    let output = run("ls /sys/class/net");
    assert!(output.stdout.split_whitespace().count() > 1, "{output:?}");
}

#[test]
#[ignore = "nécessite Docker ou Podman (CLAW_TEST_CONTAINER)"]
fn extra_mounts_are_read_only() {
    let shared = std::env::temp_dir().join(format!("claw-partage-{}", std::process::id()));
    std::fs::create_dir_all(&shared).expect("dossier partagé");
    std::fs::write(shared.join("lisible.txt"), "contenu partagé").expect("fichier partagé");
    let mount = shared.display().to_string();
    let _project = Project::new("montage", &format!(r#", "allowedMounts": ["{mount}"]"#));
    let output = run(&format!(
        "cat '{mount}/lisible.txt'; touch '{mount}/nouveau' 2>/dev/null && echo ECRIT || echo LECTURE_SEULE"
    ));
    let _ = std::fs::remove_dir_all(&shared);
    assert!(output.stdout.contains("contenu partagé"), "{output:?}");
    assert!(output.stdout.contains("LECTURE_SEULE"), "{output:?}");
}

#[test]
#[ignore = "nécessite Docker ou Podman (CLAW_TEST_CONTAINER)"]
fn timeout_removes_the_container() {
    let _project = Project::new("delai", "");
    run("true"); // image déjà présente : le délai ne compte pas le téléchargement
    let started = Instant::now();
    let output = execute_bash(input("sleep 120", 3_000)).expect("exécution");
    assert!(output.interrupted, "{output:?}");
    assert!(
        started.elapsed() < Duration::from_secs(30),
        "{:?}",
        started.elapsed()
    );
    wait_until_no_container();
}

#[test]
#[ignore = "nécessite Docker ou Podman (CLAW_TEST_CONTAINER)"]
fn killing_a_background_task_removes_its_container() {
    let _project = Project::new("fond", "");
    let mut request = input("echo début; sleep 120", 180_000);
    request.run_in_background = Some(true);
    let output = execute_bash(request).expect("lancement");
    let task_id = output
        .background_task_id
        .clone()
        .expect("identifiant de tâche");
    let deadline = Instant::now() + Duration::from_secs(60);
    while leftover_containers().is_empty() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(250));
    }
    assert!(
        !leftover_containers().is_empty(),
        "le conteneur de la tâche n'a pas démarré"
    );
    bash_tasks::kill(&task_id).expect("arrêt de la tâche");
    wait_until_no_container();
}

#[test]
#[ignore = "nécessite Docker ou Podman (CLAW_TEST_CONTAINER)"]
fn disabling_the_sandbox_runs_on_the_host() {
    let _project = Project::new("hote", "");
    let mut request = input("cat /etc/os-release 2>/dev/null; pwd", 60_000);
    request.dangerously_disable_sandbox = Some(true);
    let output = execute_bash(request).expect("exécution");
    assert!(!output.stdout.contains("/workspace"), "{output:?}");
}
