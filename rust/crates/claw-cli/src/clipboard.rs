//! Image du presse-papiers (Alt+V dans le terminal) : enregistrée en PNG dans
//! un dossier temporaire, puis jointe à la demande comme `@"chemin"`.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

const NO_IMAGE: &str =
    "Aucune image dans le presse-papiers : copiez une capture d'écran puis Alt+V.";

/// Enregistre l'image du presse-papiers et renvoie son chemin.
pub fn save_image() -> Result<PathBuf, String> {
    let dir = std::env::temp_dir().join("claw-images");
    std::fs::create_dir_all(&dir)
        .map_err(|error| format!("dossier des images impossible à créer : {error}"))?;
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_millis());
    let path = dir.join(format!("collage-{stamp}.png"));
    grab(&path)?;
    if path.metadata().map_or(true, |metadata| metadata.len() == 0) {
        let _ = std::fs::remove_file(&path);
        return Err(NO_IMAGE.to_string());
    }
    Ok(path)
}

fn missing(tool: &str, error: &std::io::Error) -> String {
    if error.kind() == std::io::ErrorKind::NotFound {
        format!("{tool} introuvable : impossible de lire le presse-papiers")
    } else {
        format!("lecture du presse-papiers impossible : {error}")
    }
}

#[cfg(windows)]
fn grab(path: &Path) -> Result<(), String> {
    let target = path.display().to_string().replace('\'', "''");
    let script = format!(
        "Add-Type -AssemblyName System.Windows.Forms; Add-Type -AssemblyName System.Drawing; \
         $image = [System.Windows.Forms.Clipboard]::GetImage(); \
         if ($null -eq $image) {{ exit 3 }}; \
         $image.Save('{target}', [System.Drawing.Imaging.ImageFormat]::Png); exit 0"
    );
    let output = Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-STA", "-Command", &script])
        .output()
        .map_err(|error| missing("PowerShell", &error))?;
    match output.status.code() {
        Some(0) => Ok(()),
        Some(3) => Err(NO_IMAGE.to_string()),
        _ => Err(format!(
            "lecture du presse-papiers impossible : {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )),
    }
}

#[cfg(target_os = "macos")]
fn grab(path: &Path) -> Result<(), String> {
    let target = path.display().to_string().replace('"', "\\\"");
    let output = Command::new("osascript")
        .args([
            "-e",
            "set png to (the clipboard as «class PNGf»)",
            "-e",
            &format!("set f to open for access POSIX file \"{target}\" with write permission"),
            "-e",
            "write png to f",
            "-e",
            "close access f",
        ])
        .output()
        .map_err(|error| missing("osascript", &error))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(NO_IMAGE.to_string())
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
fn grab(path: &Path) -> Result<(), String> {
    let attempts: [(&str, &[&str]); 2] = [
        ("wl-paste", &["--no-newline", "--type", "image/png"]),
        (
            "xclip",
            &["-selection", "clipboard", "-t", "image/png", "-o"],
        ),
    ];
    let mut last_error = None;
    for (tool, args) in attempts {
        match Command::new(tool).args(args).output() {
            Ok(output) if output.status.success() && !output.stdout.is_empty() => {
                return std::fs::write(path, output.stdout)
                    .map_err(|error| format!("image impossible à enregistrer : {error}"));
            }
            Ok(_) => last_error = Some(NO_IMAGE.to_string()),
            Err(error) => {
                last_error.get_or_insert_with(|| missing(tool, &error));
            }
        }
    }
    Err(last_error.unwrap_or_else(|| NO_IMAGE.to_string()))
}
