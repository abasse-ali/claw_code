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

/// Copie `text` dans le presse-papiers (`/copy`). Renvoie `true` si un outil du
/// système l'a fait ; sinon la séquence OSC 52 est envoyée au terminal, que la
/// plupart des terminaux récents comprennent (y compris par SSH), et `false`
/// est renvoyé pour que l'utilisateur sache que ce n'est pas garanti.
pub fn copy_text(text: &str) -> bool {
    use std::io::Write as _;
    if copy_with_system_tool(text) {
        return true;
    }
    let mut stdout = std::io::stdout();
    let _ = write!(stdout, "\x1b]52;c;{}\x07", base64(text.as_bytes()));
    let _ = stdout.flush();
    false
}

fn pipe_into(program: &str, args: &[&str], text: &str) -> bool {
    use std::io::Write as _;
    use std::process::Stdio;
    let Ok(mut child) = Command::new(program)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    else {
        return false;
    };
    let written = child
        .stdin
        .take()
        .is_some_and(|mut stdin| stdin.write_all(text.as_bytes()).is_ok());
    written && child.wait().is_ok_and(|status| status.success())
}

#[cfg(windows)]
fn copy_with_system_tool(text: &str) -> bool {
    pipe_into(
        "powershell.exe",
        &[
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "[Console]::InputEncoding = [System.Text.Encoding]::UTF8; \
             Set-Clipboard -Value ([Console]::In.ReadToEnd())",
        ],
        text,
    )
}

#[cfg(target_os = "macos")]
fn copy_with_system_tool(text: &str) -> bool {
    pipe_into("pbcopy", &[], text)
}

#[cfg(all(unix, not(target_os = "macos")))]
fn copy_with_system_tool(text: &str) -> bool {
    pipe_into("wl-copy", &[], text)
        || pipe_into("xclip", &["-selection", "clipboard"], text)
        || pipe_into("xsel", &["--clipboard", "--input"], text)
}

fn base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut encoded = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let triple = chunk.iter().enumerate().fold(0_u32, |acc, (index, byte)| {
            acc | u32::from(*byte) << (16 - 8 * index)
        });
        for position in 0..4 {
            if position <= chunk.len() {
                let index = (triple >> (18 - 6 * position)) & 0x3f;
                encoded.push(char::from(TABLE[index as usize]));
            } else {
                encoded.push('=');
            }
        }
    }
    encoded
}

#[cfg(test)]
mod tests {
    #[test]
    fn base64_matches_the_standard_alphabet_and_padding() {
        assert_eq!(super::base64(b""), "");
        assert_eq!(super::base64(b"f"), "Zg==");
        assert_eq!(super::base64(b"fo"), "Zm8=");
        assert_eq!(super::base64(b"foo"), "Zm9v");
        assert_eq!(super::base64("été".as_bytes()), "w6l0w6k=");
    }
}
