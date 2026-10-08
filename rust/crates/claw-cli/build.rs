//! Informations de build affichées par `claw --version` : cible, commit git
//! (s'il y en a un) et date de compilation.

use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    if let Ok(target) = std::env::var("TARGET") {
        println!("cargo:rustc-env=TARGET={target}");
    }
    // Sans ces dépendances, le commit lu au premier build ne changerait plus.
    if let Some(git_dir) = git(&["rev-parse", "--absolute-git-dir"]).map(PathBuf::from) {
        let mut watched = vec![git_dir.join("HEAD"), git_dir.join("packed-refs")];
        if let Some(reference) = git(&["symbolic-ref", "-q", "HEAD"]) {
            watched.push(git_dir.join(reference));
        }
        for path in watched.iter().filter(|path| path.exists()) {
            println!("cargo:rerun-if-changed={}", path.display());
        }
    }
    let git_sha = git(&["rev-parse", "--short=12", "HEAD"]).or_else(|| {
        std::env::var("GITHUB_SHA")
            .ok()
            .map(|sha| sha.chars().take(12).collect())
    });
    if let Some(sha) = git_sha {
        println!("cargo:rustc-env=GIT_SHA={sha}");
    }
    let days = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs() / 86_400);
    let (year, month, day) = civil_from_days(i64::try_from(days).unwrap_or(0));
    println!("cargo:rustc-env=BUILD_DATE={year:04}-{month:02}-{day:02}");
}

fn git(args: &[&str]) -> Option<String> {
    Command::new("git")
        .args(args)
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty())
}

/// Jours depuis le 1970-01-01 → (année, mois, jour), calendrier grégorien.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = u32::try_from(doy - (153 * mp + 2) / 5 + 1).unwrap_or(1);
    let month = u32::try_from(if mp < 10 { mp + 3 } else { mp - 9 }).unwrap_or(1);
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}
