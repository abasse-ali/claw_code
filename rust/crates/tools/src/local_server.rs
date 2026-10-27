//! Petit serveur HTTP local qui sert les fichiers du projet aux outils de page
//! (`Screenshot`, `Browser`, `ViewPage`).
//!
//! Une page du projet ouverte en `file://` n'est pas la page publiée : le
//! navigateur lui refuse le manifeste, le service worker, les modules et
//! `fetch()`, et ses erreurs de console passaient pour des erreurs du script.
//! Vu le 27 octobre 2026 (session #35) : « Unsafe attempt to load URL
//! file:///… » compté « erreur du script », fonctionnement plafonné à 5/10, et
//! une heure de travail de l'agent sur un défaut que le site en ligne n'a pas.
//! Servie en `http://127.0.0.1`, la page se comporte comme en ligne : une
//! origine, un contexte sûr, et un fichier absent rend une vraie erreur 404.
//!
//! Le serveur n'écoute que sur la boucle locale, ne sert que le dossier de
//! travail (ni au-dessus, ni ses dossiers cachés : `.git`, `.claw`, `.env`),
//! et vit autant que le processus de Claw. `CLAW_PAGE_SERVER=0` revient au
//! `file://`.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs::File;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

/// Serveurs lancés par ce processus : dossier servi (canonique) et port.
static SERVERS: Mutex<BTreeMap<PathBuf, u16>> = Mutex::new(BTreeMap::new());

/// Attente d'une requête sur une connexion ouverte.
const READ_LIMIT: Duration = Duration::from_secs(15);
/// Lignes d'en-tête lues au plus par requête.
const MAX_HEADER_LINES: usize = 100;

/// Le serveur est-il permis ? `CLAW_PAGE_SERVER=0` (ou `off`) garde le `file://`.
fn enabled() -> bool {
    !matches!(
        std::env::var("CLAW_PAGE_SERVER")
            .ok()
            .as_deref()
            .map(str::trim),
        Some("0" | "off" | "false" | "non")
    )
}

/// Port du serveur qui sert `root`, lancé au premier appel ; `None` s'il ne
/// peut pas l'être (le `file://` reste alors la solution de secours).
pub(crate) fn port_for(root: &Path) -> Option<u16> {
    if !enabled() {
        return None;
    }
    let root = root.canonicalize().ok()?;
    let mut servers = SERVERS.lock().ok()?;
    if let Some(port) = servers.get(&root) {
        return Some(*port);
    }
    let listener = TcpListener::bind(("127.0.0.1", 0)).ok()?;
    let port = listener.local_addr().ok()?.port();
    let served = root.clone();
    std::thread::Builder::new()
        .name("claw-pages".to_string())
        .spawn(move || serve(&listener, &served))
        .ok()?;
    servers.insert(root, port);
    Some(port)
}

/// Adresse `http://127.0.0.1:<port>/…` du fichier `full` (canonique) de `root`.
pub(crate) fn url_for(root: &Path, full: &Path) -> Option<String> {
    let root = root.canonicalize().ok()?;
    let relative = full.strip_prefix(&root).ok()?;
    let port = port_for(&root)?;
    let mut url = format!("http://127.0.0.1:{port}");
    for component in relative.components() {
        let std::path::Component::Normal(segment) = component else {
            return None;
        };
        url.push('/');
        url.push_str(&encode(&segment.to_string_lossy()));
    }
    if relative.as_os_str().is_empty() {
        url.push('/');
    }
    Some(url)
}

/// Fichier du disque derrière une adresse d'un serveur de ce processus (sans
/// exiger qu'il existe : un lien vers une page absente garde son chemin).
pub(crate) fn path_for(url: &str) -> Option<PathBuf> {
    let rest = url.strip_prefix("http://127.0.0.1:")?;
    let (port, path) = rest.split_once('/').unwrap_or((rest, ""));
    let port: u16 = port.parse().ok()?;
    let root = SERVERS
        .lock()
        .ok()?
        .iter()
        .find(|(_, served)| **served == port)
        .map(|(root, _)| root.clone())?;
    let path = path.split(['?', '#']).next().unwrap_or("");
    let mut full = root;
    for segment in path
        .split('/')
        .filter(|segment| !segment.is_empty() && *segment != ".")
    {
        let segment = decode(segment)?;
        if segment == ".." || segment.contains(['/', '\\', '\0']) {
            return None;
        }
        full.push(segment);
    }
    Some(full)
}

/// Chemin d'une adresse d'un serveur de ce processus depuis la racine servie
/// (`/public/tarifs.html` donne `public/tarifs.html`).
pub(crate) fn relative_for(url: &str) -> Option<PathBuf> {
    if !serves(url) {
        return None;
    }
    let rest = url.strip_prefix("http://127.0.0.1:")?;
    let path = rest.split_once('/').map_or("", |(_, path)| path);
    let path = path.split(['?', '#']).next().unwrap_or("");
    let mut relative = PathBuf::new();
    for part in path
        .split('/')
        .filter(|part| !part.is_empty() && *part != ".")
    {
        let part = decode(part)?;
        if part == ".." || part.contains(['/', '\\', '\0']) {
            return None;
        }
        relative.push(part);
    }
    Some(relative)
}

/// L'adresse est-elle servie par un serveur de ce processus ?
pub(crate) fn serves(url: &str) -> bool {
    url.strip_prefix("http://127.0.0.1:")
        .and_then(|rest| rest.split(['/', '?', '#']).next())
        .and_then(|port| port.parse::<u16>().ok())
        .is_some_and(|port| {
            SERVERS
                .lock()
                .is_ok_and(|servers| servers.values().any(|served| *served == port))
        })
}

fn serve(listener: &TcpListener, root: &Path) {
    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        let root = root.to_path_buf();
        // Un fil par connexion : le navigateur en ouvre plusieurs à la fois.
        let _ = std::thread::Builder::new()
            .name("claw-page".to_string())
            .spawn(move || {
                let _ = answer(stream, &root);
            });
    }
}

/// Lit une requête et y répond, puis ferme la connexion.
fn answer(stream: TcpStream, root: &Path) -> std::io::Result<()> {
    stream.set_read_timeout(Some(READ_LIMIT))?;
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut line = String::new();
    reader.read_line(&mut line)?;
    let mut parts = line.split_whitespace();
    let method = parts.next().unwrap_or("").to_string();
    let target = parts.next().unwrap_or("/").to_string();
    // En-têtes lus jusqu'à la ligne vide (et ignorés).
    for _ in 0..MAX_HEADER_LINES {
        let mut header = String::new();
        if reader.read_line(&mut header)? == 0 || header.trim().is_empty() {
            break;
        }
    }
    let mut out = stream;
    if method != "GET" && method != "HEAD" {
        return respond(
            &mut out,
            405,
            "text/plain; charset=utf-8",
            b"Method Not Allowed",
            method == "HEAD",
        );
    }
    let head = method == "HEAD";
    let path = target.split(['?', '#']).next().unwrap_or("/");
    let Some(file) = resolve(root, path) else {
        // L'icône que le navigateur demande de lui-même : rien, sans erreur
        // (la page ne la demande pas ; ouverte en `file://`, il ne la cherchait pas).
        if path == "/favicon.ico" {
            let header =
                "HTTP/1.1 204 No Content\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n";
            out.write_all(header.as_bytes())?;
            return out.flush();
        }
        return respond(
            &mut out,
            404,
            "text/html; charset=utf-8",
            NOT_FOUND.as_bytes(),
            head,
        );
    };
    match file {
        Resolved::Redirect(location) => {
            let header = format!(
                "HTTP/1.1 301 Moved Permanently\r\nLocation: {location}\r\nContent-Length: 0\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n"
            );
            out.write_all(header.as_bytes())?;
            out.flush()
        }
        Resolved::File(file) => {
            let Ok(mut content) = File::open(&file) else {
                return respond(
                    &mut out,
                    404,
                    "text/html; charset=utf-8",
                    NOT_FOUND.as_bytes(),
                    head,
                );
            };
            let length = content.metadata().map_or(0, |meta| meta.len());
            let header = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: {}\r\nContent-Length: {length}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
                content_type(&file)
            );
            out.write_all(header.as_bytes())?;
            if !head {
                std::io::copy(&mut content, &mut out)?;
            }
            out.flush()
        }
    }
}

const NOT_FOUND: &str =
    "<!doctype html><html><head><meta charset=\"utf-8\"><title>404</title></head><body><h1>404</h1><p>This file is not in the project.</p></body></html>";

fn respond(
    out: &mut TcpStream,
    status: u16,
    kind: &str,
    body: &[u8],
    head: bool,
) -> std::io::Result<()> {
    let reason = match status {
        404 => "Not Found",
        405 => "Method Not Allowed",
        _ => "OK",
    };
    let header = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        body.len()
    );
    out.write_all(header.as_bytes())?;
    if !head {
        out.write_all(body)?;
    }
    out.flush()
}

/// Ce que sert une adresse : un fichier, ou une redirection vers le dossier
/// avec sa barre finale (les liens relatifs de son `index.html` en dépendent).
#[derive(Debug, PartialEq, Eq)]
enum Resolved {
    File(PathBuf),
    Redirect(String),
}

fn resolve(root: &Path, path: &str) -> Option<Resolved> {
    let mut full = root.to_path_buf();
    for part in path
        .split('/')
        .filter(|part| !part.is_empty() && *part != ".")
    {
        let part = decode(part)?;
        // Ni remontée, ni séparateur glissé dans un nom, ni dossier caché
        // (`.git`, `.claw`, `.env`), sauf `.well-known`.
        if part == ".."
            || part.contains(['/', '\\', '\0', ':'])
            || (part.starts_with('.') && part != ".well-known")
        {
            return None;
        }
        full.push(part);
    }
    // Un lien symbolique qui sort du dossier servi n'est pas suivi.
    let canonical = full.canonicalize().ok()?;
    if !canonical.starts_with(root) {
        return None;
    }
    if canonical.is_dir() {
        if !path.ends_with('/') {
            return Some(Resolved::Redirect(format!("{path}/")));
        }
        let index = canonical.join("index.html");
        return index.is_file().then_some(Resolved::File(index));
    }
    canonical.is_file().then_some(Resolved::File(canonical))
}

/// Type d'un fichier d'après son extension.
fn content_type(path: &Path) -> &'static str {
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match extension.as_str() {
        "html" | "htm" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" | "mjs" | "cjs" => "text/javascript; charset=utf-8",
        "json" | "map" => "application/json; charset=utf-8",
        "webmanifest" => "application/manifest+json; charset=utf-8",
        "txt" | "md" => "text/plain; charset=utf-8",
        "csv" => "text/csv; charset=utf-8",
        "xml" => "application/xml; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "avif" => "image/avif",
        "ico" => "image/x-icon",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "ogg" => "audio/ogg",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        "pdf" => "application/pdf",
        "wasm" => "application/wasm",
        _ => "application/octet-stream",
    }
}

/// Un segment de chemin, encodé pour une adresse.
fn encode(part: &str) -> String {
    let mut out = String::new();
    for byte in part.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~".contains(&byte) {
            out.push(char::from(byte));
        } else {
            let _ = write!(out, "%{byte:02X}");
        }
    }
    out
}

/// Un segment d'adresse décodé (`%20` devient une espace) ; `None` s'il ne
/// donne pas de l'UTF-8.
fn decode(part: &str) -> Option<String> {
    let bytes = part.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        let escaped = (bytes[index] == b'%' && index + 2 < bytes.len())
            .then(|| std::str::from_utf8(&bytes[index + 1..index + 3]).ok())
            .flatten()
            .and_then(|hex| u8::from_str_radix(hex, 16).ok());
        if let Some(byte) = escaped {
            out.push(byte);
            index += 3;
        } else {
            out.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(out).ok()
}

#[cfg(test)]
mod tests {
    use super::{
        content_type, decode, encode, path_for, port_for, resolve, serves, url_for, Resolved,
    };
    use std::io::{Read, Write};
    use std::net::TcpStream;
    use std::path::Path;

    fn fetch(port: u16, path: &str) -> String {
        let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("connexion");
        write!(stream, "GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n").expect("requête");
        let mut answer = String::new();
        stream.read_to_string(&mut answer).expect("réponse");
        answer
    }

    #[test]
    fn segments_are_encoded_and_decoded() {
        assert_eq!(encode("mon site"), "mon%20site");
        assert_eq!(encode("café.html"), "caf%C3%A9.html");
        assert_eq!(decode("caf%C3%A9.html").as_deref(), Some("café.html"));
        assert_eq!(decode("100%").as_deref(), Some("100%"));
        assert_eq!(decode("%FF"), None);
    }

    #[test]
    fn types_follow_the_extension() {
        assert_eq!(
            content_type(Path::new("a/index.html")),
            "text/html; charset=utf-8"
        );
        assert_eq!(
            content_type(Path::new("app.MJS")),
            "text/javascript; charset=utf-8"
        );
        assert_eq!(
            content_type(Path::new("manifest.webmanifest")),
            "application/manifest+json; charset=utf-8"
        );
        assert_eq!(content_type(Path::new("x.bin")), "application/octet-stream");
    }

    #[test]
    fn the_project_is_served_and_nothing_above_or_hidden() {
        let dir = std::env::temp_dir().join(format!("claw-serve-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("public")).expect("dossier");
        std::fs::create_dir_all(dir.join(".git")).expect("dossier");
        std::fs::write(dir.join("index.html"), "<h1>Accueil</h1>").expect("page");
        std::fs::write(dir.join("public/index.html"), "<h1>Public</h1>").expect("page");
        std::fs::write(dir.join("style.css"), "body{}").expect("css");
        std::fs::write(dir.join(".git/config"), "secret").expect("git");
        std::fs::write(dir.join("mon fichier.json"), "{}").expect("json");
        let root = dir.canonicalize().expect("canonique");

        assert_eq!(
            resolve(&root, "/"),
            Some(Resolved::File(root.join("index.html")))
        );
        assert_eq!(
            resolve(&root, "/public"),
            Some(Resolved::Redirect("/public/".to_string()))
        );
        assert_eq!(resolve(&root, "/../etc/passwd"), None);
        assert_eq!(resolve(&root, "/%2e%2e/secret"), None);
        assert_eq!(resolve(&root, "/.git/config"), None);
        assert_eq!(resolve(&root, "/absent.html"), None);

        let port = port_for(&dir).expect("serveur");
        assert_eq!(port_for(&dir), Some(port), "un seul serveur par dossier");
        let page = fetch(port, "/");
        assert!(page.starts_with("HTTP/1.1 200 OK"), "{page}");
        assert!(
            page.contains("Content-Type: text/html; charset=utf-8"),
            "{page}"
        );
        assert!(page.ends_with("<h1>Accueil</h1>"), "{page}");
        let css = fetch(port, "/style.css");
        assert!(css.contains("Content-Type: text/css"), "{css}");
        assert!(fetch(port, "/mon%20fichier.json").starts_with("HTTP/1.1 200 OK"));
        assert!(fetch(port, "/.git/config").starts_with("HTTP/1.1 404"));
        assert!(fetch(port, "/absent.html").starts_with("HTTP/1.1 404"));
        assert!(fetch(port, "/favicon.ico").starts_with("HTTP/1.1 204"));
        let moved = fetch(port, "/public");
        assert!(
            moved.starts_with("HTTP/1.1 301") && moved.contains("Location: /public/"),
            "{moved}"
        );

        let url = url_for(&dir, &root.join("public/index.html")).expect("adresse");
        assert_eq!(url, format!("http://127.0.0.1:{port}/public/index.html"));
        assert!(serves(&url));
        assert!(!serves("http://127.0.0.1:1/index.html"));
        assert_eq!(path_for(&url), Some(root.join("public/index.html")));
        assert_eq!(
            super::relative_for(&url),
            Some(Path::new("public").join("index.html"))
        );
        assert_eq!(super::relative_for("http://127.0.0.1:1/index.html"), None);
        assert_eq!(
            path_for(&format!("http://127.0.0.1:{port}/absent.html?x=1#y")),
            Some(root.join("absent.html"))
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
