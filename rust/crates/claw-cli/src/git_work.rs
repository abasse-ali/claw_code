//! Branches et worktrees depuis la session : `/branch`, `/worktree`,
//! `claw --worktree <nom>` et les étapes git de `/commit-push-pr`.

use std::path::{Path, PathBuf};

use runtime::worktree;

/// Branche courante (`HEAD` si détachée).
pub fn current_branch(dir: &Path) -> Result<String, String> {
    worktree::git(dir, &["rev-parse", "--abbrev-ref", "HEAD"])
        .map(|branch| branch.trim().to_string())
}

/// Branche principale : celle d'`origin/HEAD`, sinon `main` ou `master`.
#[must_use]
pub fn default_branch(dir: &Path) -> String {
    if let Ok(head) = worktree::git(
        dir,
        &["symbolic-ref", "--short", "refs/remotes/origin/HEAD"],
    ) {
        if let Some(branch) = head.trim().strip_prefix("origin/") {
            return branch.to_string();
        }
    }
    for candidate in ["main", "master"] {
        if worktree::git(
            dir,
            &[
                "rev-parse",
                "--verify",
                "--quiet",
                &format!("refs/heads/{candidate}"),
            ],
        )
        .is_ok()
        {
            return candidate.to_string();
        }
    }
    "main".to_string()
}

/// `/branch [list|create <nom>|switch <nom>]`.
#[must_use]
pub fn branch_command(dir: &Path, action: Option<&str>, target: Option<&str>) -> String {
    let action = action.unwrap_or("list");
    let result = match (action, target) {
        ("list" | "liste" | "ls", _) => worktree::git(dir, &["branch", "--list", "--no-color"])
            .map(|branches| {
                let lines: Vec<String> = branches
                    .lines()
                    .map(|line| format!("  {line}"))
                    .collect();
                format!("Branches\n{}", lines.join("\n"))
            }),
        ("create" | "creer" | "créer" | "new", Some(name)) => {
            worktree::git(dir, &["switch", "-c", name]).map(|_| {
                format!("Branche\n  Créée            {name}\n  Active           {name} (créée depuis la branche précédente)")
            })
        }
        ("switch" | "checkout" | "aller", Some(name)) => worktree::git(dir, &["switch", name])
            .map(|_| format!("Branche\n  Active           {name}")),
        ("create" | "creer" | "créer" | "new" | "switch" | "checkout" | "aller", None) => {
            Err(format!("nom de branche manquant : /branch {action} <nom>"))
        }
        (other, _) => Err(format!(
            "action inconnue : {other}\nUsage : /branch [list|create <nom>|switch <nom>]"
        )),
    };
    result.unwrap_or_else(|error| format!("branche : {error}"))
}

/// `/worktree [list|add <nom> [branche]|remove <nom> [--force]|prune]`.
#[must_use]
pub fn worktree_command(
    dir: &Path,
    action: Option<&str>,
    name: Option<&str>,
    extra: Option<&str>,
) -> String {
    let action = action.unwrap_or("list");
    let result = match (action, name) {
        ("list" | "liste" | "ls", _) => worktree::list(dir).map(|entries| render_list(dir, &entries)),
        ("add" | "ajouter" | "create" | "new", Some(name)) => {
            worktree::create(dir, name, extra).map(|created| {
                let copied = included_note(&created.path);
                format!(
                    "Worktree\n  Créé             {}\n  Branche          {}{copied}\n  Travailler       cd {} && claw   (ou claw --worktree {})\n  Retirer          /worktree remove {}",
                    created.path.display(),
                    created.branch,
                    created.path.display(),
                    created.name,
                    created.name
                )
            })
        }
        ("remove" | "rm" | "retirer" | "supprimer", Some(name)) => {
            let force = extra.is_some_and(|flag| matches!(flag, "--force" | "-f" | "force"));
            worktree::remove(dir, name, force).map(|report| format!("Worktree\n  {report}"))
        }
        ("prune" | "elaguer" | "élaguer", _) => worktree::git(dir, &["worktree", "prune", "-v"])
            .map(|output| {
                if output.trim().is_empty() {
                    "Worktree\n  Rien à élaguer".to_string()
                } else {
                    format!("Worktree\n{output}")
                }
            }),
        ("add" | "ajouter" | "create" | "new" | "remove" | "rm" | "retirer" | "supprimer", None) => {
            Err(format!("nom manquant : /worktree {action} <nom>"))
        }
        (other, _) => Err(format!(
            "action inconnue : {other}\nUsage : /worktree [list|add <nom> [branche]|remove <nom> [--force]|prune]"
        )),
    };
    result.unwrap_or_else(|error| format!("worktree : {error}"))
}

fn included_note(path: &Path) -> String {
    let include = path.join(worktree::INCLUDE_FILE);
    if include.is_file() {
        "\n  Copiés           fichiers ignorés listés dans .worktreeinclude".to_string()
    } else {
        String::new()
    }
}

fn render_list(dir: &Path, entries: &[worktree::Entry]) -> String {
    let current = worktree::repo_root(dir).ok();
    let mut lines = vec!["Worktrees".to_string()];
    for entry in entries {
        let marker = if current.as_deref() == Some(entry.path.as_path()) {
            "●"
        } else {
            " "
        };
        lines.push(format!(
            "  {marker} {:<28} {:<10} {}",
            entry.branch.as_deref().unwrap_or("(détaché)"),
            entry.head,
            entry.path.display()
        ));
    }
    lines.push(
        "  Ajouter          /worktree add <nom> · session isolée : claw --worktree <nom>"
            .to_string(),
    );
    lines.join("\n")
}

/// `--worktree [nom]` ou `-w [nom]` dans les arguments : retiré, avec le nom
/// (généré s'il manque). Un mot qui suit n'est pris pour un nom que s'il en a
/// la forme (pas d'espace, pas d'option).
pub fn take_worktree_flag(args: &mut Vec<String>) -> Option<String> {
    let index = args
        .iter()
        .position(|arg| arg == "--worktree" || arg == "-w" || arg.starts_with("--worktree="))?;
    let flag = args.remove(index);
    if let Some(name) = flag.strip_prefix("--worktree=") {
        return Some(name.to_string())
            .filter(|name| !name.is_empty())
            .or_else(|| Some(generated_name()));
    }
    let looks_like_name = args.get(index).is_some_and(|next| {
        !next.starts_with('-')
            && !next.is_empty()
            && next
                .chars()
                .all(|character| character.is_alphanumeric() || "-_./".contains(character))
    });
    if looks_like_name {
        Some(args.remove(index))
    } else {
        Some(generated_name())
    }
}

fn generated_name() -> String {
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or_default();
    format!("session-{seconds}")
}

/// Session lancée dans un worktree : retiré à la fin s'il n'a rien changé.
pub struct WorktreeSession {
    pub worktree: worktree::Worktree,
    pub created: bool,
    main_repo: PathBuf,
}

/// Crée (ou rouvre) le worktree et s'y place, dans le même sous-dossier.
pub fn enter(name: &str) -> Result<WorktreeSession, String> {
    let cwd = std::env::current_dir().map_err(|error| error.to_string())?;
    let repo = worktree::repo_root(&cwd)?;
    let main_repo = worktree::main_root(&cwd)?;
    let (opened, created) = worktree::open_or_create(&cwd, name)?;
    let relative = cwd
        .strip_prefix(&repo)
        .map(Path::to_path_buf)
        .unwrap_or_default();
    let target = opened.path.join(&relative);
    let target = if target.is_dir() {
        target
    } else {
        opened.path.clone()
    };
    std::env::set_current_dir(&target).map_err(|error| error.to_string())?;
    eprintln!(
        "Worktree {} · branche {} · {}",
        if created { "créé" } else { "rouvert" },
        opened.branch,
        opened.path.display()
    );
    Ok(WorktreeSession {
        worktree: opened,
        created,
        main_repo,
    })
}

/// Fin de session : un worktree créé pour elle et resté intact est retiré ;
/// sinon il est gardé, avec de quoi reprendre ou fusionner.
pub fn leave(session: &WorktreeSession) {
    let _ = std::env::set_current_dir(&session.main_repo);
    let path = &session.worktree.path;
    if session.created && !worktree::has_changes(path, &session.worktree.base) {
        match worktree::remove(&session.main_repo, &path.display().to_string(), false) {
            Ok(report) => eprintln!("Aucun changement : {report}"),
            Err(error) => eprintln!("worktree gardé ({error}) : {}", path.display()),
        }
        return;
    }
    eprintln!(
        "Worktree gardé : {}\n  Reprendre        claw --worktree {}\n  Fusionner        git merge {} (après commit)\n  Retirer          /worktree remove {}",
        path.display(),
        session.worktree.name,
        session.worktree.branch,
        session.worktree.name
    );
}

/// Lien pour ouvrir une pull request sur GitHub sans `gh`.
#[must_use]
pub fn compare_url(dir: &Path, base: &str, branch: &str) -> Option<String> {
    let remote = worktree::git(dir, &["remote", "get-url", "origin"]).ok()?;
    let remote = remote.trim().trim_end_matches(".git");
    let path = remote
        .strip_prefix("https://github.com/")
        .or_else(|| remote.strip_prefix("git@github.com:"))
        .or_else(|| remote.strip_prefix("ssh://git@github.com/"))?;
    Some(format!(
        "https://github.com/{path}/compare/{base}...{branch}?expand=1"
    ))
}

/// Nom de branche pour `/commit-push-pr` lancé sur la branche principale.
#[must_use]
pub fn branch_name_for(hint: &str) -> String {
    let words: Vec<String> = hint
        .split_whitespace()
        .map(worktree::slug)
        .filter(|word| word.len() > 2)
        .take(5)
        .collect();
    if words.is_empty() {
        format!("claw/{}", generated_name())
    } else {
        format!("claw/{}", words.join("-"))
    }
}

#[cfg(test)]
mod tests {
    use super::{branch_name_for, take_worktree_flag};

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(ToString::to_string).collect()
    }

    #[test]
    fn the_worktree_flag_takes_an_optional_name() {
        let mut given = args(&["--worktree", "auth", "--model", "x"]);
        assert_eq!(take_worktree_flag(&mut given).as_deref(), Some("auth"));
        assert_eq!(given, args(&["--model", "x"]));

        let mut short = args(&["-w", "-p", "salut"]);
        let name = take_worktree_flag(&mut short).expect("nom généré");
        assert!(name.starts_with("session-"));
        assert_eq!(short, args(&["-p", "salut"]));

        let mut inline = args(&["--worktree=refonte"]);
        assert_eq!(take_worktree_flag(&mut inline).as_deref(), Some("refonte"));
        assert!(inline.is_empty());

        // Une phrase n'est pas un nom.
        let mut prompt = args(&["-w", "corrige le bug"]);
        assert!(take_worktree_flag(&mut prompt)
            .expect("nom")
            .starts_with("session-"));
        assert_eq!(prompt, args(&["corrige le bug"]));

        let mut none = args(&["--model", "x"]);
        assert_eq!(take_worktree_flag(&mut none), None);
    }

    #[test]
    fn branch_names_come_from_the_request() {
        assert_eq!(
            branch_name_for("Ajoute la pagination à l'API"),
            "claw/ajoute-pagination-l-api"
        );
        assert!(branch_name_for("").starts_with("claw/session-"));
    }
}
