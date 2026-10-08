//! Emplacements système communs à toutes les plateformes.

use std::path::{Path, PathBuf};

/// Dossier personnel : `HOME`, sinon `USERPROFILE` (Windows hors Git Bash).
#[must_use]
pub fn home_dir() -> Option<PathBuf> {
    ["HOME", "USERPROFILE"]
        .into_iter()
        .filter_map(std::env::var_os)
        .find(|value| !value.is_empty())
        .map(PathBuf::from)
}

/// Cherche un exécutable dans le `PATH` (avec les extensions de `PATHEXT` sous
/// Windows), sans dépendre d'un `sh` qui n'y est pas toujours accessible.
#[must_use]
pub fn find_executable(name: &str) -> Option<PathBuf> {
    let candidate = Path::new(name);
    if candidate.components().count() > 1 {
        return is_executable_file(candidate).then(|| candidate.to_path_buf());
    }
    let paths = std::env::var_os("PATH")?;
    let extensions = executable_extensions();
    std::env::split_paths(&paths).find_map(|dir| {
        std::iter::once(dir.join(name))
            .chain(
                extensions
                    .iter()
                    .map(|extension| dir.join(format!("{name}{extension}"))),
            )
            .find(|path| is_executable_file(path))
    })
}

fn executable_extensions() -> Vec<String> {
    if !cfg!(windows) {
        return Vec::new();
    }
    std::env::var("PATHEXT")
        .unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".to_string())
        .split(';')
        .map(str::trim)
        .filter(|extension| !extension.is_empty())
        .map(str::to_ascii_lowercase)
        .collect()
}

fn is_executable_file(path: &Path) -> bool {
    // Les alias d'exécution du Microsoft Store (WindowsApps\python.exe) sont des
    // points d'analyse que `is_file` ne suit pas toujours.
    let Ok(metadata) = std::fs::metadata(path).or_else(|_| std::fs::symlink_metadata(path)) else {
        return false;
    };
    if metadata.is_dir() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        path.extension().is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::{find_executable, home_dir};

    #[test]
    fn finds_cargo_in_path() {
        let found = find_executable("cargo").expect("cargo est dans le PATH des tests");
        assert!(found.is_file() || found.exists(), "{found:?}");
        assert!(find_executable("claw-commande-inexistante-xyz").is_none());
    }

    #[test]
    fn home_dir_is_known() {
        assert!(home_dir().is_some());
    }
}
