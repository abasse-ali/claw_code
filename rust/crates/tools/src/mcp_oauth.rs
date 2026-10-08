//! Connexion OAuth aux serveurs MCP distants (`claw mcp login <serveur>`) :
//! découverte du serveur d'autorisation (RFC 9728 puis RFC 8414),
//! enregistrement dynamique du client (RFC 7591), PKCE, retour sur une adresse
//! locale, échange et renouvellement des jetons. Les jetons sont conservés dans
//! `~/.claw/mcp-oauth.json` et ajoutés aux requêtes (`Authorization: Bearer`).

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::Value;

const LOGIN_TIMEOUT: Duration = Duration::from_secs(300);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredToken {
    pub access_token: String,
    #[serde(default)]
    pub refresh_token: Option<String>,
    /// Expiration (secondes Unix).
    #[serde(default)]
    pub expires_at: Option<u64>,
    pub token_endpoint: String,
    pub client_id: String,
    #[serde(default)]
    pub resource: Option<String>,
}

fn store_path() -> PathBuf {
    std::env::var_os("CLAW_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| runtime::home_dir().map(|home| home.join(".claw")))
        .unwrap_or_else(|| PathBuf::from(".claw"))
        .join("mcp-oauth.json")
}

fn load_store() -> BTreeMap<String, StoredToken> {
    std::fs::read_to_string(store_path())
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn save_store(store: &BTreeMap<String, StoredToken>) -> Result<(), String> {
    let path = store_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let text = serde_json::to_string_pretty(store).map_err(|error| error.to_string())?;
    std::fs::write(&path, text).map_err(|error| format!("{} : {error}", path.display()))
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

/// Jeton valide pour ce serveur (renouvelé si besoin), s'il y a eu une connexion.
#[must_use]
pub fn access_token(server: &str) -> Option<String> {
    let mut store = load_store();
    let token = store.get(server)?.clone();
    let expired = token.expires_at.is_some_and(|at| at <= now() + 60);
    if !expired {
        return Some(token.access_token);
    }
    let refresh = token.refresh_token.clone()?;
    let refreshed = request_token(
        &token.token_endpoint,
        &[
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh.as_str()),
            ("client_id", token.client_id.as_str()),
        ],
        token.resource.as_deref(),
    )
    .ok()?;
    let updated = StoredToken {
        access_token: refreshed.access_token.clone(),
        refresh_token: refreshed.refresh_token.or(Some(refresh)),
        expires_at: refreshed.expires_in.map(|seconds| now() + seconds),
        ..token
    };
    store.insert(server.to_string(), updated);
    let _ = save_store(&store);
    Some(refreshed.access_token)
}

/// Oublie la connexion OAuth d'un serveur.
pub fn logout(server: &str) -> Result<bool, String> {
    let mut store = load_store();
    let removed = store.remove(server).is_some();
    save_store(&store)?;
    Ok(removed)
}

#[must_use]
pub fn has_token(server: &str) -> bool {
    load_store().contains_key(server)
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    expires_in: Option<u64>,
}

fn http() -> reqwest::blocking::Client {
    reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .unwrap_or_else(|_| reqwest::blocking::Client::new())
}

fn request_token(
    endpoint: &str,
    params: &[(&str, &str)],
    resource: Option<&str>,
) -> Result<TokenResponse, String> {
    let mut form = params
        .iter()
        .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
        .collect::<Vec<_>>();
    if let Some(resource) = resource {
        form.push(("resource".to_string(), resource.to_string()));
    }
    let response = http()
        .post(endpoint)
        .header(reqwest::header::ACCEPT, "application/json")
        .form(&form)
        .send()
        .map_err(|error| format!("serveur de jetons injoignable : {error}"))?;
    let status = response.status();
    let text = response.text().unwrap_or_default();
    if !status.is_success() {
        return Err(format!(
            "échange de jeton refusé ({status}) : {}",
            text.chars().take(300).collect::<String>()
        ));
    }
    serde_json::from_str(&text).map_err(|error| format!("réponse de jeton invalide : {error}"))
}

fn origin_of(url: &str) -> Result<String, String> {
    let parsed =
        reqwest::Url::parse(url).map_err(|error| format!("URL invalide {url} : {error}"))?;
    Ok(parsed.origin().ascii_serialization())
}

fn get_json(url: &str) -> Option<Value> {
    let response = http()
        .get(url)
        .header(reqwest::header::ACCEPT, "application/json")
        .send()
        .ok()?;
    response
        .status()
        .is_success()
        .then(|| response.json().ok())
        .flatten()
}

/// Métadonnées utiles du serveur d'autorisation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthServer {
    pub authorization_endpoint: String,
    pub token_endpoint: String,
    pub registration_endpoint: Option<String>,
    pub scopes: Vec<String>,
}

/// Découverte : ressource protégée (`/.well-known/oauth-protected-resource`),
/// puis métadonnées du serveur d'autorisation.
pub fn discover(server_url: &str) -> Result<AuthServer, String> {
    let origin = origin_of(server_url)?;
    let resource_meta = get_json(&format!("{origin}/.well-known/oauth-protected-resource"));
    let issuer = resource_meta
        .as_ref()
        .and_then(|meta| meta.get("authorization_servers"))
        .and_then(Value::as_array)
        .and_then(|servers| servers.first())
        .and_then(Value::as_str)
        .map_or_else(
            || origin.clone(),
            |issuer| issuer.trim_end_matches('/').to_string(),
        );
    let scopes = resource_meta
        .as_ref()
        .and_then(|meta| meta.get("scopes_supported"))
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(ToOwned::to_owned)
                .collect()
        })
        .unwrap_or_default();
    let meta = get_json(&format!("{issuer}/.well-known/oauth-authorization-server"))
        .or_else(|| get_json(&format!("{issuer}/.well-known/openid-configuration")))
        .ok_or_else(|| format!("métadonnées OAuth introuvables pour {issuer}"))?;
    let field = |key: &str| meta.get(key).and_then(Value::as_str).map(ToOwned::to_owned);
    Ok(AuthServer {
        authorization_endpoint: field("authorization_endpoint")
            .ok_or("authorization_endpoint absent des métadonnées")?,
        token_endpoint: field("token_endpoint").ok_or("token_endpoint absent des métadonnées")?,
        registration_endpoint: field("registration_endpoint"),
        scopes,
    })
}

fn register_client(endpoint: &str, redirect_uri: &str) -> Result<String, String> {
    let response = http()
        .post(endpoint)
        .json(&serde_json::json!({
            "client_name": "Claw Code",
            "redirect_uris": [redirect_uri],
            "grant_types": ["authorization_code", "refresh_token"],
            "response_types": ["code"],
            "token_endpoint_auth_method": "none",
        }))
        .send()
        .map_err(|error| format!("enregistrement du client impossible : {error}"))?;
    let status = response.status();
    let body: Value = response.json().unwrap_or(Value::Null);
    if !status.is_success() {
        return Err(format!(
            "enregistrement du client refusé ({status}) : {body}"
        ));
    }
    body.get("client_id")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .ok_or_else(|| "réponse d'enregistrement sans client_id".to_string())
}

fn open_browser(url: &str) {
    // `CLAW_NO_BROWSER=1` : machine distante ou tests, l'adresse est seulement affichée.
    if std::env::var("CLAW_NO_BROWSER").is_ok_and(|value| value.trim() == "1") {
        return;
    }
    let result = if cfg!(windows) {
        std::process::Command::new("cmd")
            .args(["/C", "start", "", url])
            .status()
    } else if cfg!(target_os = "macos") {
        std::process::Command::new("open").arg(url).status()
    } else {
        std::process::Command::new("xdg-open").arg(url).status()
    };
    if result.is_err() {
        eprintln!("Ouvrez cette adresse dans votre navigateur :");
    }
}

/// Attend la redirection du navigateur sur l'adresse locale ; renvoie le code.
fn wait_for_code(listener: &TcpListener, state: &str) -> Result<String, String> {
    listener
        .set_nonblocking(true)
        .map_err(|error| error.to_string())?;
    let deadline = Instant::now() + LOGIN_TIMEOUT;
    loop {
        match listener.accept() {
            Ok((mut stream, _)) => {
                let _ = stream.set_nonblocking(false);
                let mut line = String::new();
                BufReader::new(&stream)
                    .read_line(&mut line)
                    .map_err(|error| error.to_string())?;
                let target = line
                    .split_whitespace()
                    .nth(1)
                    .unwrap_or_default()
                    .to_string();
                if !target.starts_with("/callback") {
                    let _ =
                        stream.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n");
                    continue;
                }
                let params = runtime::parse_oauth_callback_request_target(&target)?;
                let (status, message) = match (&params.code, &params.error) {
                    (Some(_), _) if params.state.as_deref() == Some(state) => (
                        "200 OK",
                        "Connexion réussie : vous pouvez fermer cette page et revenir à Claw.",
                    ),
                    (_, Some(_)) => ("400 Bad Request", "Connexion refusée par le serveur."),
                    _ => ("400 Bad Request", "Réponse inattendue (état invalide)."),
                };
                let body = format!(
                    "<!doctype html><meta charset=utf-8><title>Claw</title><p>{message}</p>"
                );
                let _ = write!(
                    stream,
                    "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\n\r\n{body}",
                    body.len()
                );
                if let Some(error) = params.error {
                    return Err(format!(
                        "autorisation refusée : {error} {}",
                        params.error_description.unwrap_or_default()
                    ));
                }
                if params.state.as_deref() != Some(state) {
                    return Err("état OAuth invalide (tentative ignorée)".to_string());
                }
                return params
                    .code
                    .ok_or_else(|| "code d'autorisation absent".to_string());
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                if Instant::now() >= deadline {
                    return Err("pas de réponse du navigateur après 5 minutes".to_string());
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(error) => return Err(error.to_string()),
        }
    }
}

/// Connexion complète d'un serveur MCP distant ; enregistre le jeton.
pub fn login(server: &str, server_url: &str) -> Result<String, String> {
    let auth = discover(server_url)?;
    let listener = TcpListener::bind("127.0.0.1:0").map_err(|error| error.to_string())?;
    let port = listener
        .local_addr()
        .map_err(|error| error.to_string())?
        .port();
    let redirect_uri = format!("http://127.0.0.1:{port}/callback");
    let client_id = match (
        &auth.registration_endpoint,
        std::env::var("CLAW_MCP_CLIENT_ID"),
    ) {
        (_, Ok(client_id)) if !client_id.trim().is_empty() => client_id,
        (Some(endpoint), _) => register_client(endpoint, &redirect_uri)?,
        (None, _) => return Err(
            "ce serveur ne permet pas l'enregistrement automatique : définissez CLAW_MCP_CLIENT_ID"
                .to_string(),
        ),
    };
    let pkce = runtime::generate_pkce_pair().map_err(|error| error.to_string())?;
    let state = runtime::generate_state().map_err(|error| error.to_string())?;
    let mut url =
        reqwest::Url::parse(&auth.authorization_endpoint).map_err(|error| error.to_string())?;
    {
        let mut query = url.query_pairs_mut();
        query
            .append_pair("response_type", "code")
            .append_pair("client_id", &client_id)
            .append_pair("redirect_uri", &redirect_uri)
            .append_pair("code_challenge", &pkce.challenge)
            .append_pair("code_challenge_method", "S256")
            .append_pair("state", &state)
            .append_pair("resource", server_url);
        if !auth.scopes.is_empty() {
            query.append_pair("scope", &auth.scopes.join(" "));
        }
    }
    eprintln!("Connexion à {server} : autorisez Claw dans le navigateur…");
    open_browser(url.as_str());
    eprintln!("  {url}");
    let code = wait_for_code(&listener, &state)?;
    let token = request_token(
        &auth.token_endpoint,
        &[
            ("grant_type", "authorization_code"),
            ("code", code.as_str()),
            ("redirect_uri", redirect_uri.as_str()),
            ("client_id", client_id.as_str()),
            ("code_verifier", pkce.verifier.as_str()),
        ],
        Some(server_url),
    )?;
    let mut store = load_store();
    store.insert(
        server.to_string(),
        StoredToken {
            access_token: token.access_token,
            refresh_token: token.refresh_token,
            expires_at: token.expires_in.map(|seconds| now() + seconds),
            token_endpoint: auth.token_endpoint,
            client_id,
            resource: Some(server_url.to_string()),
        },
    );
    save_store(&store)?;
    Ok(format!(
        "Connecté à {server} (jeton enregistré dans {})",
        store_path().display()
    ))
}
