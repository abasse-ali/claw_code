//! `claw channel webhook` : serveur de channel fourni avec Claw (le
//! récepteur de webhooks de la doc de Claude Code, sans Node ni Bun).
//!
//! Claw le lance comme serveur MCP stdio (déclaré dans `.mcp.json` ou
//! `.claw.json`, activé avec `--channels server:<nom>`). Il écoute sur
//! `127.0.0.1` : chaque `POST` (résultat de CI, alerte, script) devient un
//! événement de la session ; l'outil `reply` et les demandes de permission
//! repartent sur `GET /events` (Server-Sent Events). Avec un jeton
//! (`--token` ou `CLAW_CHANNEL_TOKEN`), toute requête doit le présenter, et
//! le channel relaie alors les demandes de permission : `POST "yes abcde"`
//! autorise, `POST "no abcde"` refuse.

use std::collections::BTreeMap;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use serde_json::{json, Value};

/// Corps de requête accepté au plus.
const MAX_BODY: usize = 1024 * 1024;

const INSTRUCTIONS: &str = "Events from this channel arrive as <channel source=\"...\" chat_id=\"...\" path=\"...\" method=\"POST\">: something posted to Claw's local webhook (a CI result, an alert, a script, a person). Act on them. When the sender may be waiting for an answer, reply with this server's reply tool, passing the chat_id from the tag; otherwise no reply is needed.";

pub(crate) struct Options {
    pub port: u16,
    pub token: Option<String>,
}

struct Shared {
    stdout: Mutex<io::Stdout>,
    listeners: Mutex<Vec<TcpStream>>,
    token: Option<String>,
    next_chat: AtomicU64,
}

impl Shared {
    fn send(&self, message: &Value) {
        let mut out = self
            .stdout
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _ = writeln!(out, "{message}");
        let _ = out.flush();
    }

    fn notify(&self, method: &str, params: &Value) {
        self.send(&json!({ "jsonrpc": "2.0", "method": method, "params": params }));
    }

    /// Envoie un événement SSE à chaque écouteur de `/events` ; rend leur nombre.
    fn broadcast(&self, event: &str, data: &Value) -> usize {
        let chunk = format!("event: {event}\ndata: {data}\n\n");
        let mut listeners = self
            .listeners
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        listeners.retain_mut(|stream| {
            stream
                .write_all(chunk.as_bytes())
                .and_then(|()| stream.flush())
                .is_ok()
        });
        listeners.len()
    }
}

/// Lance le serveur : MCP sur stdin/stdout, HTTP sur `127.0.0.1:port`.
pub(crate) fn run(options: &Options) -> io::Result<()> {
    let listener = TcpListener::bind(("127.0.0.1", options.port)).map_err(|error| {
        io::Error::new(
            error.kind(),
            format!("port {} indisponible : {error}", options.port),
        )
    })?;
    let shared = Arc::new(Shared {
        stdout: Mutex::new(io::stdout()),
        listeners: Mutex::new(Vec::new()),
        token: options
            .token
            .clone()
            .filter(|token| !token.trim().is_empty()),
        next_chat: AtomicU64::new(1),
    });
    eprintln!(
        "claw channel webhook : http://127.0.0.1:{}/ (POST pour pousser un événement, GET /events pour les réponses){}",
        options.port,
        if shared.token.is_some() {
            " ; jeton exigé"
        } else {
            ""
        }
    );
    {
        let shared = Arc::clone(&shared);
        thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let shared = Arc::clone(&shared);
                thread::spawn(move || {
                    let _ = serve(stream, &shared);
                });
            }
        });
    }
    for line in io::stdin().lock().lines() {
        let Ok(line) = line else {
            break;
        };
        if let Ok(message) = serde_json::from_str::<Value>(line.trim()) {
            handle(&shared, &message);
        }
    }
    Ok(())
}

fn handle(shared: &Shared, message: &Value) {
    let method = message["method"].as_str().unwrap_or_default();
    let params = &message["params"];
    let result = match method {
        "initialize" => {
            let mut experimental = json!({ "claude/channel": {} });
            // Relais des permissions seulement si l'expéditeur est authentifié.
            if shared.token.is_some() {
                experimental["claude/channel/permission"] = json!({});
            }
            Ok(json!({
                "protocolVersion": params["protocolVersion"].as_str().unwrap_or("2025-06-18"),
                "capabilities": { "experimental": experimental, "tools": {} },
                "serverInfo": { "name": "claw-webhook", "version": env!("CARGO_PKG_VERSION") },
                "instructions": INSTRUCTIONS,
            }))
        }
        "tools/list" => Ok(json!({ "tools": [{
            "name": "reply",
            "description": "Send a message back over this channel (it goes out on GET /events of Claw's local webhook).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "chat_id": { "type": "string", "description": "The chat_id of the event you answer" },
                    "text": { "type": "string", "description": "The message to send" }
                },
                "required": ["chat_id", "text"]
            }
        }] })),
        "tools/call" if params["name"] == "reply" => {
            let arguments = &params["arguments"];
            let count = shared.broadcast(
                "reply",
                &json!({ "chat_id": arguments["chat_id"], "text": arguments["text"] }),
            );
            Ok(
                json!({ "content": [{ "type": "text", "text": format!("sent to {count} listener(s) of /events") }] }),
            )
        }
        "tools/call" => Err(format!(
            "outil inconnu : {}",
            params["name"].as_str().unwrap_or_default()
        )),
        "ping" => Ok(json!({})),
        "notifications/claude/channel/permission_request" => {
            let id = params["request_id"].as_str().unwrap_or_default();
            shared.broadcast(
                "permission",
                &json!({
                    "request_id": id,
                    "tool_name": params["tool_name"],
                    "description": params["description"],
                    "input_preview": params["input_preview"],
                    "reply": format!("POST \"yes {id}\" or \"no {id}\""),
                }),
            );
            return;
        }
        _ => Err(format!("méthode inconnue : {method}")),
    };
    // Une notification n'appelle pas de réponse.
    let Some(id) = message.get("id") else {
        return;
    };
    shared.send(&match result {
        Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
        Err(message) => {
            json!({ "jsonrpc": "2.0", "id": id, "error": { "code": -32601, "message": message } })
        }
    });
}

fn respond(stream: &mut TcpStream, status: &str, body: &str) -> io::Result<()> {
    write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )?;
    stream.flush()
}

/// `yes abcde`, `y abcde`, `no abcde`, `n abcde` : verdict d'une demande.
fn verdict(text: &str) -> Option<(bool, String)> {
    let mut words = text.split_whitespace();
    let (word, id) = (words.next()?, words.next()?);
    if words.next().is_some() {
        return None;
    }
    let allow = match word.to_ascii_lowercase().as_str() {
        "y" | "yes" => true,
        "n" | "no" => false,
        _ => return None,
    };
    let id = id.to_ascii_lowercase();
    let valid = id.len() == 5
        && id
            .chars()
            .all(|character| character.is_ascii_lowercase() && character != 'l');
    valid.then_some((allow, id))
}

fn serve(mut stream: TcpStream, shared: &Shared) -> io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut request_line = String::new();
    reader.read_line(&mut request_line)?;
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default().to_string();
    let target = parts.next().unwrap_or("/").to_string();
    let mut headers = BTreeMap::new();
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            break;
        }
        let line = line.trim_end();
        if line.is_empty() {
            break;
        }
        if let Some((key, value)) = line.split_once(':') {
            headers.insert(key.trim().to_ascii_lowercase(), value.trim().to_string());
        }
    }
    let (path, query) = target.split_once('?').unwrap_or((target.as_str(), ""));
    if let Some(token) = &shared.token {
        let given = headers
            .get("authorization")
            .and_then(|value| value.strip_prefix("Bearer "))
            .map(str::to_string)
            .or_else(|| headers.get("x-claw-token").cloned())
            .or_else(|| {
                query
                    .split('&')
                    .find_map(|pair| pair.strip_prefix("token="))
                    .map(str::to_string)
            });
        if given.as_deref() != Some(token.as_str()) {
            return respond(
                &mut stream,
                "401 Unauthorized",
                "jeton manquant ou invalide\n",
            );
        }
    }
    match (method.as_str(), path) {
        ("GET", "/events") => {
            stream.write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nConnection: keep-alive\r\n\r\n: connected\n\n",
            )?;
            stream.flush()?;
            stream.set_read_timeout(None)?;
            shared
                .listeners
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(stream);
            Ok(())
        }
        ("GET", _) => respond(
            &mut stream,
            "200 OK",
            "Claw channel webhook\nPOST /  pousse le corps dans la session Claw\nGET /events  réponses de Claw et demandes de permission (SSE)\n",
        ),
        ("POST", _) => {
            let length = headers
                .get("content-length")
                .and_then(|value| value.parse::<usize>().ok())
                .unwrap_or(0);
            if length > MAX_BODY {
                return respond(&mut stream, "413 Payload Too Large", "corps trop long\n");
            }
            let mut body = vec![0_u8; length];
            reader.read_exact(&mut body)?;
            let text = String::from_utf8_lossy(&body).trim().to_string();
            if text.is_empty() {
                return respond(&mut stream, "400 Bad Request", "corps vide\n");
            }
            if shared.token.is_some() {
                if let Some((allow, id)) = verdict(&text) {
                    shared.notify(
                        "notifications/claude/channel/permission",
                        &json!({ "request_id": id, "behavior": if allow { "allow" } else { "deny" } }),
                    );
                    return respond(&mut stream, "200 OK", "verdict transmis\n");
                }
            }
            let chat = shared.next_chat.fetch_add(1, Ordering::Relaxed);
            let mut meta = json!({ "chat_id": chat.to_string(), "path": path, "method": "POST" });
            if let Some(sender) = headers.get("x-claw-sender").filter(|sender| {
                !sender.is_empty() && sender.len() <= 80 && !sender.contains(['"', '<', '>'])
            }) {
                meta["sender"] = json!(sender);
            }
            shared.notify(
                "notifications/claude/channel",
                &json!({ "content": text, "meta": meta }),
            );
            respond(&mut stream, "200 OK", &format!("ok {chat}\n"))
        }
        _ => respond(&mut stream, "405 Method Not Allowed", "GET ou POST\n"),
    }
}

#[cfg(test)]
mod tests {
    use super::verdict;

    #[test]
    fn verdicts_need_a_known_word_and_a_five_letter_id() {
        assert_eq!(verdict("yes abcde"), Some((true, "abcde".to_string())));
        assert_eq!(verdict(" N  ABCDE "), Some((false, "abcde".to_string())));
        assert_eq!(verdict("yes abcdl"), None);
        assert_eq!(verdict("yes"), None);
        assert_eq!(verdict("approve abcde"), None);
        assert_eq!(verdict("yes abcde please"), None);
    }
}
