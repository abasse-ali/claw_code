//! Worktrees git de Claw : des copies de travail du dépôt, chacune sur sa
//! branche, pour faire travailler plusieurs sessions ou sous-agents en même
//! temps sans qu'ils se marchent dessus (comme `claude --worktree`).
//!
//! Ils vivent dans `.claw/worktrees/<nom>`, sur la branche `claw/<nom>` par
//! défaut. Le dossier est ajouté à `.git/info/exclude` (local, rien n'est
//! modifié dans le dépôt). Les fichiers ignorés par git que listent les motifs
//! de `.worktreeinclude` (`.env`, configuration locale…) sont copiés dans
//! chaque nouveau worktree, comme dans Claude Code.

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

pub const WORKTREES_DIR: &str = ".claw/worktrees";
pub const INCLUDE_FILE: &str = ".worktreeinclude";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Worktree {
    pub name: String,
    pub path: PathBuf,
    pub branch: String,
    /// Commit de départ : ce qui a été fait depuis se compare à lui.
    pub base: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub path: PathBuf,
    pub branch: Option<String>,
    pub head: String,
}

/// Lance `git args` dans `dir` et renvoie sa sortie.
pub fn git(dir: &Path, args: &[&str]) -> Result<String, String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .map_err(|error| format!("git introuvable : {error}"))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout)
            .trim_end()
            .to_string())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        Err(if stderr.is_empty() {
            format!("git {} a échoué", args.join(" "))
        } else {
            stderr
        })
    }
}

fn native(path: &str) -> PathBuf {
    PathBuf::from(path.replace('/', std::path::MAIN_SEPARATOR_STR))
}

/// Racine du dépôt (du worktree) qui contient `dir`.
pub fn repo_root(dir: &Path) -> Result<PathBuf, String> {
    git(dir, &["rev-parse", "--show-toplevel"])
        .map(|root| native(root.trim()))
        .map_err(|_| "pas dans un dépôt git".to_string())
}

/// Racine du dépôt principal (le même pour tous ses worktrees).
pub fn main_root(dir: &Path) -> Result<PathBuf, String> {
    let common = git(
        dir,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    )
    .map_err(|_| "pas dans un dépôt git".to_string())?;
    let common = native(common.trim());
    Ok(common.parent().map_or(common.clone(), Path::to_path_buf))
}

/// Nom utilisable pour un dossier et une branche.
#[must_use]
pub fn slug(name: &str) -> String {
    let mut slug = String::new();
    for character in name.trim().chars() {
        let character = match character {
            'à' | 'â' | 'ä' => 'a',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'î' | 'ï' => 'i',
            'ô' | 'ö' => 'o',
            'ù' | 'û' | 'ü' => 'u',
            'ç' => 'c',
            other => other.to_ascii_lowercase(),
        };
        if character.is_ascii_alphanumeric() || character == '.' || character == '_' {
            slug.push(character);
        } else if !slug.ends_with('-') {
            slug.push('-');
        }
    }
    let slug: String = slug.trim_matches(['-', '.']).chars().take(48).collect();
    slug.trim_end_matches(['-', '.']).to_string()
}

#[must_use]
pub fn worktrees_dir(repo: &Path) -> PathBuf {
    repo.join(".claw").join("worktrees")
}

fn branch_exists(repo: &Path, branch: &str) -> bool {
    git(
        repo,
        &[
            "rev-parse",
            "--verify",
            "--quiet",
            &format!("refs/heads/{branch}"),
        ],
    )
    .is_ok()
}

/// `.claw/worktrees/` dans `.git/info/exclude`, pour que le dépôt principal
/// ne voie pas les worktrees comme des fichiers à ajouter.
fn exclude_worktrees(repo: &Path) {
    let Ok(common) = git(
        repo,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    ) else {
        return;
    };
    let exclude = native(common.trim()).join("info").join("exclude");
    let current = fs::read_to_string(&exclude).unwrap_or_default();
    if current
        .lines()
        .any(|line| line.trim().trim_start_matches('/') == ".claw/worktrees/")
    {
        return;
    }
    if let Some(parent) = exclude.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let separator = if current.is_empty() || current.ends_with('\n') {
        ""
    } else {
        "\n"
    };
    let _ = fs::write(
        &exclude,
        format!("{current}{separator}# Worktrees de Claw\n/.claw/worktrees/\n"),
    );
}

/// Crée le worktree `name` (branche `branch`, sinon `claw/<nom>`) depuis HEAD.
pub fn create(repo: &Path, name: &str, branch: Option<&str>) -> Result<Worktree, String> {
    let repo = main_root(repo)?;
    let slug = slug(name);
    if slug.is_empty() {
        return Err("nom de worktree vide".to_string());
    }
    let path = worktrees_dir(&repo).join(&slug);
    if path.exists() {
        return Err(format!("le worktree {} existe déjà", path.display()));
    }
    let branch = branch
        .map(str::trim)
        .filter(|branch| !branch.is_empty())
        .map_or_else(|| format!("claw/{slug}"), ToOwned::to_owned);
    fs::create_dir_all(worktrees_dir(&repo)).map_err(|error| error.to_string())?;
    exclude_worktrees(&repo);
    let target = path.display().to_string();
    if branch_exists(&repo, &branch) {
        git(&repo, &["worktree", "add", &target, &branch])?;
    } else {
        git(&repo, &["worktree", "add", "-b", &branch, &target, "HEAD"])?;
    }
    copy_included(&repo, &path);
    let base = git(&path, &["rev-parse", "HEAD"])?.trim().to_string();
    Ok(Worktree {
        name: slug,
        path,
        branch,
        base,
    })
}

/// Le worktree `name` s'il existe déjà (même branche), sinon un nouveau.
/// Renvoie aussi `true` s'il vient d'être créé.
pub fn open_or_create(repo: &Path, name: &str) -> Result<(Worktree, bool), String> {
    let root = main_root(repo)?;
    let slug = slug(name);
    let path = worktrees_dir(&root).join(&slug);
    if path.join(".git").exists() {
        let branch = git(&path, &["rev-parse", "--abbrev-ref", "HEAD"])?;
        let base = git(&path, &["rev-parse", "HEAD"])?.trim().to_string();
        return Ok((
            Worktree {
                name: slug,
                path,
                branch: branch.trim().to_string(),
                base,
            },
            false,
        ));
    }
    create(&root, name, None).map(|worktree| (worktree, true))
}

/// Worktrees du dépôt (le dossier principal compris).
pub fn list(repo: &Path) -> Result<Vec<Entry>, String> {
    let output = git(repo, &["worktree", "list", "--porcelain"])?;
    let mut entries = Vec::new();
    let mut current: Option<Entry> = None;
    for line in output.lines() {
        if let Some(path) = line.strip_prefix("worktree ") {
            if let Some(entry) = current.take() {
                entries.push(entry);
            }
            current = Some(Entry {
                path: native(path),
                branch: None,
                head: String::new(),
            });
        } else if let Some(entry) = current.as_mut() {
            if let Some(head) = line.strip_prefix("HEAD ") {
                entry.head = head.chars().take(8).collect();
            } else if let Some(branch) = line.strip_prefix("branch ") {
                entry.branch = Some(branch.trim_start_matches("refs/heads/").to_string());
            }
        }
    }
    if let Some(entry) = current {
        entries.push(entry);
    }
    Ok(entries)
}

/// Chemin d'un worktree donné par son nom (`.claw/worktrees/<nom>`) ou son chemin.
pub fn resolve(repo: &Path, name_or_path: &str) -> Result<PathBuf, String> {
    let root = main_root(repo)?;
    let by_name = worktrees_dir(&root).join(slug(name_or_path));
    if by_name.exists() {
        return Ok(by_name);
    }
    let as_path = PathBuf::from(name_or_path);
    let as_path = if as_path.is_absolute() {
        as_path
    } else {
        root.join(as_path)
    };
    if as_path.exists() {
        Ok(as_path)
    } else {
        Err(format!("worktree introuvable : {name_or_path}"))
    }
}

/// Retire le worktree ; sa branche `claw/…` est supprimée si elle n'a rien
/// apporté (sinon gardée, pour ne rien perdre).
pub fn remove(repo: &Path, name_or_path: &str, force: bool) -> Result<String, String> {
    let root = main_root(repo)?;
    let path = resolve(&root, name_or_path)?;
    let branch = git(&path, &["rev-parse", "--abbrev-ref", "HEAD"]).ok();
    let target = path.display().to_string();
    let mut args = vec!["worktree", "remove"];
    if force {
        args.push("--force");
    }
    args.push(&target);
    git(&root, &args).map_err(|error| {
        if error.contains("modified or untracked") || error.contains("contains modified") {
            format!("{error}\n(modifications non enregistrées : /worktree remove <nom> --force pour les abandonner)")
        } else {
            error
        }
    })?;
    let mut report = format!("worktree retiré : {}", path.display());
    if let Some(branch) = branch.filter(|branch| branch.starts_with("claw/")) {
        // `-d` refuse une branche dont les commits ne sont nulle part ailleurs.
        if git(&root, &["branch", "-d", &branch]).is_ok() {
            let _ = write!(report, " (branche {branch} supprimée)");
        } else {
            let _ = write!(report, " (branche {branch} gardée : elle a des commits)");
        }
    }
    Ok(report)
}

/// Le worktree a des modifications (non enregistrées, ou commits depuis `base`).
#[must_use]
pub fn has_changes(path: &Path, base: &str) -> bool {
    let dirty = git(path, &["status", "--porcelain"])
        .map(|status| {
            status
                .lines()
                .any(|line| !line.get(3..).unwrap_or_default().starts_with(".claw/"))
        })
        .unwrap_or(false);
    let ahead = git(path, &["rev-list", "--count", &format!("{base}..HEAD")])
        .ok()
        .and_then(|count| count.trim().parse::<u32>().ok())
        .unwrap_or(0);
    dirty || ahead > 0
}

/// Enregistre toutes les modifications du worktree (hors `.claw/`) dans un
/// commit ; `Ok(false)` s'il n'y avait rien à enregistrer.
pub fn commit_all(path: &Path, message: &str) -> Result<bool, String> {
    git(path, &["add", "-A", "--", ".", ":(exclude).claw"])?;
    if git(path, &["diff", "--cached", "--quiet"]).is_ok() {
        return Ok(false);
    }
    git(
        path,
        &[
            "-c",
            "user.name=claw-agent",
            "-c",
            "user.email=claw-agent@users.noreply.github.com",
            "commit",
            "--no-verify",
            "-m",
            message,
        ],
    )?;
    Ok(true)
}

/// Résumé des changements depuis `base` (`git diff --stat`).
#[must_use]
pub fn change_summary(path: &Path, base: &str) -> String {
    git(path, &["diff", "--stat", base]).unwrap_or_default()
}

/// Motifs de `.worktreeinclude` (syntaxe simplifiée de `.gitignore`).
fn include_patterns(repo: &Path) -> Vec<String> {
    fs::read_to_string(repo.join(INCLUDE_FILE))
        .unwrap_or_default()
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(ToOwned::to_owned)
        .collect()
}

/// `*` (sans `/`), `**` (tout) et `?` ; un motif sans `/` vaut pour le nom
/// seul, à n'importe quelle profondeur ; un `/` final désigne un dossier.
#[must_use]
pub fn pattern_matches(pattern: &str, path: &str) -> bool {
    let is_dir = path.ends_with('/');
    let path = path.trim_end_matches('/');
    let wants_dir = pattern.ends_with('/');
    if wants_dir && !is_dir {
        return false;
    }
    let pattern = pattern.trim_end_matches('/');
    if let Some(anchored) = pattern.strip_prefix('/') {
        return glob(anchored.as_bytes(), path.as_bytes());
    }
    if pattern.contains('/') {
        return glob(pattern.as_bytes(), path.as_bytes());
    }
    let name = path.rsplit('/').next().unwrap_or(path);
    glob(pattern.as_bytes(), name.as_bytes())
}

fn glob(pattern: &[u8], text: &[u8]) -> bool {
    match pattern.first() {
        None => text.is_empty(),
        Some(b'*') if pattern.get(1) == Some(&b'*') => {
            let rest = pattern[2..].strip_prefix(b"/").unwrap_or(&pattern[2..]);
            (0..=text.len()).any(|start| glob(rest, &text[start..]))
        }
        Some(b'*') => (0..=text.len())
            .take_while(|&end| !text[..end].contains(&b'/'))
            .any(|end| glob(&pattern[1..], &text[end..])),
        Some(b'?') => !text.is_empty() && text[0] != b'/' && glob(&pattern[1..], &text[1..]),
        Some(&byte) => text.first() == Some(&byte) && glob(&pattern[1..], &text[1..]),
    }
}

/// Copie dans `dest` les fichiers ignorés par git que vise `.worktreeinclude`.
pub fn copy_included(repo: &Path, dest: &Path) -> Vec<PathBuf> {
    let patterns = include_patterns(repo);
    if patterns.is_empty() {
        return Vec::new();
    }
    let Ok(ignored) = git(
        repo,
        &[
            "ls-files",
            "--others",
            "--ignored",
            "--exclude-standard",
            "--directory",
        ],
    ) else {
        return Vec::new();
    };
    let mut copied = Vec::new();
    for entry in ignored
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        if entry.starts_with(".claw/") || entry == ".claw" {
            continue;
        }
        if !patterns
            .iter()
            .any(|pattern| pattern_matches(pattern, entry))
        {
            continue;
        }
        let relative = entry.trim_end_matches('/');
        let source = repo.join(relative);
        let target = dest.join(relative);
        if copy_path(&source, &target).is_ok() {
            copied.push(target);
        }
    }
    copied
}

fn copy_path(source: &Path, target: &Path) -> std::io::Result<()> {
    if source.is_dir() {
        fs::create_dir_all(target)?;
        for entry in fs::read_dir(source)? {
            let entry = entry?;
            copy_path(&entry.path(), &target.join(entry.file_name()))?;
        }
        Ok(())
    } else {
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(source, target).map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::{
        commit_all, create, git, has_changes, list, open_or_create, pattern_matches, remove, slug,
    };
    use std::fs;
    use std::path::PathBuf;

    #[test]
    fn names_become_branch_friendly() {
        assert_eq!(slug("Corrige l'Auth !"), "corrige-l-auth");
        assert_eq!(slug("  --x--  "), "x");
        assert_eq!(slug("été 2026"), "ete-2026");
    }

    #[test]
    fn include_patterns_follow_gitignore_basics() {
        assert!(pattern_matches(".env", ".env"));
        assert!(pattern_matches(".env", "app/.env"));
        assert!(pattern_matches("*.local", "config/db.local"));
        assert!(!pattern_matches("*.local", "config/db.local.bak"));
        assert!(pattern_matches("/config/*.json", "config/secrets.json"));
        assert!(!pattern_matches(
            "/config/*.json",
            "app/config/secrets.json"
        ));
        assert!(pattern_matches("secrets/", "secrets/"));
        assert!(!pattern_matches("secrets/", "secrets"));
        assert!(pattern_matches("**/cert?.pem", "deep/a/cert1.pem"));
        assert!(!pattern_matches("node_modules", "src/main.rs"));
    }

    fn temp_repo(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "claw-worktree-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|elapsed| elapsed.as_nanos())
                .unwrap_or_default()
        ));
        fs::create_dir_all(&dir).expect("dossier");
        git(&dir, &["init", "-q", "-b", "main"]).expect("init");
        git(&dir, &["config", "user.email", "test@example.invalid"]).expect("email");
        git(&dir, &["config", "user.name", "test"]).expect("nom");
        git(&dir, &["config", "core.autocrlf", "false"]).expect("autocrlf");
        fs::write(dir.join(".gitignore"), ".env\n").expect("gitignore");
        fs::write(dir.join(".worktreeinclude"), ".env\n").expect("include");
        fs::write(dir.join("app.txt"), "v1\n").expect("fichier");
        fs::write(dir.join(".env"), "TOKEN=local\n").expect("env");
        git(&dir, &["add", "."]).expect("add");
        git(&dir, &["commit", "-q", "-m", "départ"]).expect("commit");
        dir
    }

    #[test]
    fn a_worktree_gets_its_branch_its_env_and_is_cleaned_up() {
        if git(&std::env::temp_dir(), &["--version"]).is_err() {
            return;
        }
        let repo = temp_repo("cycle");
        let worktree = create(&repo, "Essai A", None).expect("création");
        assert_eq!(worktree.branch, "claw/essai-a");
        assert!(worktree
            .path
            .ends_with(PathBuf::from(".claw").join("worktrees").join("essai-a")));
        // Fichier ignoré copié grâce à .worktreeinclude.
        assert_eq!(
            fs::read_to_string(worktree.path.join(".env")).expect(".env copié"),
            "TOKEN=local\n"
        );
        // Le dépôt principal ne voit pas le worktree.
        let status = git(&repo, &["status", "--porcelain"]).expect("status");
        assert!(!status.contains(".claw/worktrees"), "{status}");
        assert!(list(&repo)
            .expect("liste")
            .iter()
            .any(|entry| entry.branch.as_deref() == Some("claw/essai-a")));
        assert!(create(&repo, "essai a", None).is_err());
        let (again, created) = open_or_create(&repo, "essai-a").expect("réouverture");
        assert!(!created);
        assert_eq!(again.path, worktree.path);

        assert!(!has_changes(&worktree.path, &worktree.base));
        fs::write(worktree.path.join("app.txt"), "v2\n").expect("modif");
        assert!(has_changes(&worktree.path, &worktree.base));
        assert!(commit_all(&worktree.path, "travail du sous-agent").expect("commit"));
        assert!(!commit_all(&worktree.path, "rien").expect("rien à faire"));
        assert!(has_changes(&worktree.path, &worktree.base));

        let report = remove(&repo, "essai-a", false).expect("retrait");
        assert!(report.contains("gardée"), "{report}");
        assert!(!worktree.path.exists());

        let empty = create(&repo, "vide", None).expect("création");
        let report = remove(&repo, "vide", false).expect("retrait");
        assert!(report.contains("supprimée"), "{report}");
        assert!(!empty.path.exists());
        let _ = fs::remove_dir_all(repo);
    }
}
