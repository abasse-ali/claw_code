use std::fs;
use std::path::{Path, PathBuf};

const STARTER_CLAW_JSON: &str = concat!(
    "{\n",
    "  \"permissions\": {\n",
    "    \"defaultMode\": \"default\"\n",
    "  }\n",
    "}\n",
);
const GITIGNORE_COMMENT: &str = "# Artefacts locaux de Claw Code";
const GITIGNORE_ENTRIES: [&str; 3] = [
    ".claw/settings.local.json",
    ".claw/sessions/",
    ".claw/image-descriptions/",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InitStatus {
    Created,
    Updated,
    Skipped,
}

impl InitStatus {
    #[must_use]
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Created => "créé",
            Self::Updated => "mis à jour",
            Self::Skipped => "ignoré (existe déjà)",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct InitArtifact {
    pub(crate) name: &'static str,
    pub(crate) status: InitStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct InitReport {
    pub(crate) project_root: PathBuf,
    pub(crate) artifacts: Vec<InitArtifact>,
}

impl InitReport {
    #[must_use]
    pub(crate) fn render(&self) -> String {
        let mut lines = vec![
            "Init".to_string(),
            format!("  Projet           {}", self.project_root.display()),
        ];
        for artifact in &self.artifacts {
            lines.push(format!(
                "  {:<16} {}",
                artifact.name,
                artifact.status.label()
            ));
        }
        lines.push("  Étape suivante   Relire et adapter les indications générées".to_string());
        lines.join("\n")
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)]
struct RepoDetection {
    rust_workspace: bool,
    rust_root: bool,
    python: bool,
    package_json: bool,
    typescript: bool,
    nextjs: bool,
    react: bool,
    vite: bool,
    nest: bool,
    src_dir: bool,
    tests_dir: bool,
    rust_dir: bool,
}

pub(crate) fn initialize_repo(cwd: &Path) -> Result<InitReport, Box<dyn std::error::Error>> {
    let mut artifacts = Vec::new();

    let claw_dir = cwd.join(".claw");
    artifacts.push(InitArtifact {
        name: ".claw/",
        status: ensure_dir(&claw_dir)?,
    });

    let claw_json = cwd.join(".claw.json");
    artifacts.push(InitArtifact {
        name: ".claw.json",
        status: write_file_if_missing(&claw_json, STARTER_CLAW_JSON)?,
    });

    let gitignore = cwd.join(".gitignore");
    artifacts.push(InitArtifact {
        name: ".gitignore",
        status: ensure_gitignore_entries(&gitignore)?,
    });

    let claw_md = cwd.join("CLAW.md");
    let content = render_init_claw_md(cwd);
    artifacts.push(InitArtifact {
        name: "CLAW.md",
        status: write_file_if_missing(&claw_md, &content)?,
    });

    Ok(InitReport {
        project_root: cwd.to_path_buf(),
        artifacts,
    })
}

fn ensure_dir(path: &Path) -> Result<InitStatus, std::io::Error> {
    if path.is_dir() {
        return Ok(InitStatus::Skipped);
    }
    fs::create_dir_all(path)?;
    Ok(InitStatus::Created)
}

fn write_file_if_missing(path: &Path, content: &str) -> Result<InitStatus, std::io::Error> {
    if path.exists() {
        return Ok(InitStatus::Skipped);
    }
    fs::write(path, content)?;
    Ok(InitStatus::Created)
}

fn ensure_gitignore_entries(path: &Path) -> Result<InitStatus, std::io::Error> {
    if !path.exists() {
        let mut lines = vec![GITIGNORE_COMMENT.to_string()];
        lines.extend(GITIGNORE_ENTRIES.iter().map(|entry| (*entry).to_string()));
        fs::write(path, format!("{}\n", lines.join("\n")))?;
        return Ok(InitStatus::Created);
    }

    let existing = fs::read_to_string(path)?;
    let mut lines = existing.lines().map(ToOwned::to_owned).collect::<Vec<_>>();
    let mut changed = false;

    if !lines.iter().any(|line| line == GITIGNORE_COMMENT) {
        lines.push(GITIGNORE_COMMENT.to_string());
        changed = true;
    }

    for entry in GITIGNORE_ENTRIES {
        if !lines.iter().any(|line| line == entry) {
            lines.push(entry.to_string());
            changed = true;
        }
    }

    if !changed {
        return Ok(InitStatus::Skipped);
    }

    fs::write(path, format!("{}\n", lines.join("\n")))?;
    Ok(InitStatus::Updated)
}

pub(crate) fn render_init_claw_md(cwd: &Path) -> String {
    let detection = detect_repo(cwd);
    let mut lines = vec![
        "# CLAW.md".to_string(),
        String::new(),
        "Ce fichier fournit des indications à Claw Code (clawcode.dev) lors du travail sur le code de ce dépôt.".to_string(),
        String::new(),
    ];

    let detected_languages = detected_languages(&detection);
    let detected_frameworks = detected_frameworks(&detection);
    lines.push("## Stack détectée".to_string());
    if detected_languages.is_empty() {
        lines.push("- Aucun marqueur de langage spécifique n'a encore été détecté ; documentez le langage principal et les commandes de vérification une fois la structure du projet stabilisée.".to_string());
    } else {
        lines.push(format!("- Langages : {}.", detected_languages.join(", ")));
    }
    if detected_frameworks.is_empty() {
        lines.push(
            "- Frameworks : aucun détecté à partir des marqueurs de starter pris en charge."
                .to_string(),
        );
    } else {
        lines.push(format!(
            "- Frameworks/marqueurs d'outillage : {}.",
            detected_frameworks.join(", ")
        ));
    }
    lines.push(String::new());

    let verification_lines = verification_lines(cwd, &detection);
    if !verification_lines.is_empty() {
        lines.push("## Vérification".to_string());
        lines.extend(verification_lines);
        lines.push(String::new());
    }

    let structure_lines = repository_shape_lines(&detection);
    if !structure_lines.is_empty() {
        lines.push("## Structure du dépôt".to_string());
        lines.extend(structure_lines);
        lines.push(String::new());
    }

    let framework_lines = framework_notes(&detection);
    if !framework_lines.is_empty() {
        lines.push("## Notes sur les frameworks".to_string());
        lines.extend(framework_lines);
        lines.push(String::new());
    }

    lines.push("## Accord de travail".to_string());
    lines.push("- Préférer de petits changements revisables et garder les fichiers d'amorçage générés alignés sur les workflows réels du dépôt.".to_string());
    lines.push("- Conserver les valeurs par défaut partagées dans `.claw.json` ; réserver `.claw/settings.local.json` aux surcharges locales à la machine.".to_string());
    lines.push("- Ne pas écraser automatiquement le contenu existant de `CLAW.md` ; le mettre à jour intentionnellement lorsque les workflows du dépôt changent.".to_string());
    lines.push(String::new());

    lines.join("\n")
}

fn detect_repo(cwd: &Path) -> RepoDetection {
    let package_json_contents = fs::read_to_string(cwd.join("package.json"))
        .unwrap_or_default()
        .to_ascii_lowercase();
    RepoDetection {
        rust_workspace: cwd.join("rust").join("Cargo.toml").is_file(),
        rust_root: cwd.join("Cargo.toml").is_file(),
        python: cwd.join("pyproject.toml").is_file()
            || cwd.join("requirements.txt").is_file()
            || cwd.join("setup.py").is_file(),
        package_json: cwd.join("package.json").is_file(),
        typescript: cwd.join("tsconfig.json").is_file()
            || package_json_contents.contains("typescript"),
        nextjs: package_json_contents.contains("\"next\""),
        react: package_json_contents.contains("\"react\""),
        vite: package_json_contents.contains("\"vite\""),
        nest: package_json_contents.contains("@nestjs"),
        src_dir: cwd.join("src").is_dir(),
        tests_dir: cwd.join("tests").is_dir(),
        rust_dir: cwd.join("rust").is_dir(),
    }
}

fn detected_languages(detection: &RepoDetection) -> Vec<&'static str> {
    let mut languages = Vec::new();
    if detection.rust_workspace || detection.rust_root {
        languages.push("Rust");
    }
    if detection.python {
        languages.push("Python");
    }
    if detection.typescript {
        languages.push("TypeScript");
    } else if detection.package_json {
        languages.push("JavaScript/Node.js");
    }
    languages
}

fn detected_frameworks(detection: &RepoDetection) -> Vec<&'static str> {
    let mut frameworks = Vec::new();
    if detection.nextjs {
        frameworks.push("Next.js");
    }
    if detection.react {
        frameworks.push("React");
    }
    if detection.vite {
        frameworks.push("Vite");
    }
    if detection.nest {
        frameworks.push("NestJS");
    }
    frameworks
}

fn verification_lines(cwd: &Path, detection: &RepoDetection) -> Vec<String> {
    let mut lines = Vec::new();
    if detection.rust_workspace {
        lines.push("- Lancer la vérification Rust depuis `rust/` : `cargo fmt`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`".to_string());
    } else if detection.rust_root {
        lines.push("- Lancer la vérification Rust depuis la racine du dépôt : `cargo fmt`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`".to_string());
    }
    if detection.python {
        if cwd.join("pyproject.toml").is_file() {
            lines.push("- Lancer les vérifications du projet Python déclarées dans `pyproject.toml` (par exemple : `pytest`, `ruff check` et `mypy` lorsqu'ils sont configurés).".to_string());
        } else {
            lines.push(
                "- Lancer les commandes de test/lint Python du dépôt avant de livrer des changements.".to_string(),
            );
        }
    }
    if detection.package_json {
        lines.push("- Lancer les vérifications JavaScript/TypeScript depuis `package.json` avant de livrer des changements (`npm test`, `npm run lint`, `npm run build`, ou l'équivalent du dépôt).".to_string());
    }
    if detection.tests_dir && detection.src_dir {
        lines.push("- `src/` et `tests/` sont tous deux présents ; mettre à jour les deux surfaces ensemble lorsque le comportement change.".to_string());
    }
    lines
}

fn repository_shape_lines(detection: &RepoDetection) -> Vec<String> {
    let mut lines = Vec::new();
    if detection.rust_dir {
        lines.push(
            "- `rust/` contient l'espace de travail Rust et l'implémentation CLI/runtime active."
                .to_string(),
        );
    }
    if detection.src_dir {
        lines.push("- `src/` contient les fichiers source qui doivent rester cohérents avec les indications générées et les tests.".to_string());
    }
    if detection.tests_dir {
        lines.push("- `tests/` contient les surfaces de validation qui doivent être revues en même temps que les changements de code.".to_string());
    }
    lines
}

fn framework_notes(detection: &RepoDetection) -> Vec<String> {
    let mut lines = Vec::new();
    if detection.nextjs {
        lines.push("- Next.js détecté : préserver les conventions de routage/récupération de données et vérifier les builds de production après modification de la structure de l'application.".to_string());
    }
    if detection.react && !detection.nextjs {
        lines.push("- React détecté : couvrir le comportement des composants avec des tests ciblés et éviter le brassage inutile de props/API.".to_string());
    }
    if detection.vite {
        lines.push("- Vite détecté : valider le bundle de production après modification de la configuration ou des imports sensibles au build.".to_string());
    }
    if detection.nest {
        lines.push("- NestJS détecté : garder explicites les frontières de modules/providers et vérifier le câblage contrôleur/service après refactor.".to_string());
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::{initialize_repo, render_init_claw_md};
    use std::fs;
    use std::path::Path;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir() -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time should be after epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("claw-init-{nanos}"))
    }

    #[test]
    fn initialize_repo_creates_expected_files_and_gitignore_entries() {
        let root = temp_dir();
        fs::create_dir_all(root.join("rust")).expect("create rust dir");
        fs::write(root.join("rust").join("Cargo.toml"), "[workspace]\n").expect("write cargo");

        let report = initialize_repo(&root).expect("init should succeed");
        let rendered = report.render();
        assert!(rendered.contains(".claw/           créé"));
        assert!(rendered.contains(".claw.json       créé"));
        assert!(rendered.contains(".gitignore       créé"));
        assert!(rendered.contains("CLAW.md          créé"));
        assert!(root.join(".claw").is_dir());
        assert!(root.join(".claw.json").is_file());
        assert!(root.join("CLAW.md").is_file());
        assert_eq!(
            fs::read_to_string(root.join(".claw.json")).expect("read claw json"),
            concat!(
                "{\n",
                "  \"permissions\": {\n",
                "    \"defaultMode\": \"default\"\n",
                "  }\n",
                "}\n",
            )
        );
        let gitignore = fs::read_to_string(root.join(".gitignore")).expect("read gitignore");
        assert!(gitignore.contains(".claw/settings.local.json"));
        assert!(gitignore.contains(".claw/sessions/"));
        let claw_md = fs::read_to_string(root.join("CLAW.md")).expect("read claw md");
        assert!(claw_md.contains("Langages : Rust."));
        assert!(claw_md.contains("cargo clippy --workspace --all-targets -- -D warnings"));

        fs::remove_dir_all(root).expect("cleanup temp dir");
    }

    #[test]
    fn initialize_repo_is_idempotent_and_preserves_existing_files() {
        let root = temp_dir();
        fs::create_dir_all(&root).expect("create root");
        fs::write(root.join("CLAW.md"), "custom guidance\n").expect("write existing claw md");
        fs::write(root.join(".gitignore"), ".claw/settings.local.json\n").expect("write gitignore");

        let first = initialize_repo(&root).expect("first init should succeed");
        assert!(first
            .render()
            .contains("CLAW.md          ignoré (existe déjà)"));
        let second = initialize_repo(&root).expect("second init should succeed");
        let second_rendered = second.render();
        assert!(second_rendered.contains(".claw/           ignoré (existe déjà)"));
        assert!(second_rendered.contains(".claw.json       ignoré (existe déjà)"));
        assert!(second_rendered.contains(".gitignore       ignoré (existe déjà)"));
        assert!(second_rendered.contains("CLAW.md          ignoré (existe déjà)"));
        assert_eq!(
            fs::read_to_string(root.join("CLAW.md")).expect("read existing claw md"),
            "custom guidance\n"
        );
        let gitignore = fs::read_to_string(root.join(".gitignore")).expect("read gitignore");
        assert_eq!(gitignore.matches(".claw/settings.local.json").count(), 1);
        assert_eq!(gitignore.matches(".claw/sessions/").count(), 1);

        fs::remove_dir_all(root).expect("cleanup temp dir");
    }

    #[test]
    fn render_init_template_mentions_detected_python_and_nextjs_markers() {
        let root = temp_dir();
        fs::create_dir_all(&root).expect("create root");
        fs::write(root.join("pyproject.toml"), "[project]\nname = \"demo\"\n")
            .expect("write pyproject");
        fs::write(
            root.join("package.json"),
            r#"{"dependencies":{"next":"14.0.0","react":"18.0.0"},"devDependencies":{"typescript":"5.0.0"}}"#,
        )
        .expect("write package json");

        let rendered = render_init_claw_md(Path::new(&root));
        assert!(rendered.contains("Langages : Python, TypeScript."));
        assert!(rendered.contains("Frameworks/marqueurs d'outillage : Next.js, React."));
        assert!(rendered.contains("pyproject.toml"));
        assert!(rendered.contains("Next.js détecté"));

        fs::remove_dir_all(root).expect("cleanup temp dir");
    }
}
