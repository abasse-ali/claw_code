//! `claw mcp …` : gestion des serveurs MCP dans les fichiers de configuration.
//!
//! ```text
//! claw mcp add <nom> -- <commande> [arguments…]        (stdio, avec -e CLE=valeur)
//! claw mcp add --transport http <nom> <url>             (ou sse, avec -H "Nom: valeur")
//! claw mcp add-json <nom> '<json>'
//! claw mcp list | get <nom> | remove <nom>
//! claw mcp login <nom> | logout <nom>
//! ```
//! `--scope project` (défaut, `.claw/settings.json`), `local`
//! (`.claw/settings.local.json`, non versionné) ou `user` (`~/.claw/settings.json`).

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use runtime::{ConfigLoader, McpServerConfig};
use serde_json::{json, Map, Value};

const USAGE: &str = "Usage
  claw mcp add [--scope project|local|user] [-e CLE=valeur]… <nom> -- <commande> [arguments…]
  claw mcp add --transport http|sse [-H \"Nom: valeur\"]… <nom> <url>
  claw mcp add-json <nom> '<json>'
  claw mcp list
  claw mcp get <nom>
  claw mcp remove [--scope …] <nom>
  claw mcp login <nom>      connexion OAuth d'un serveur distant
  claw mcp logout <nom>";

fn scope_path(scope: &str) -> Result<PathBuf, String> {
    let cwd = env::current_dir().map_err(|error| error.to_string())?;
    match scope {
        "project" => Ok(cwd.join(".claw").join("settings.json")),
        "local" => Ok(cwd.join(".claw").join("settings.local.json")),
        "user" => Ok(ConfigLoader::default_for(&cwd)
            .config_home()
            .join("settings.json")),
        other => Err(format!(
            "portée inconnue « {other} » : project, local ou user"
        )),
    }
}

fn read_settings(path: &Path) -> Result<Map<String, Value>, String> {
    match fs::read_to_string(path) {
        Ok(text) if !text.trim().is_empty() => match serde_json::from_str::<Value>(&text) {
            Ok(Value::Object(object)) => Ok(object),
            Ok(_) => Err(format!(
                "{} : la racine doit être un objet JSON",
                path.display()
            )),
            Err(error) => Err(format!("{} : JSON invalide ({error})", path.display())),
        },
        Ok(_) => Ok(Map::new()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Map::new()),
        Err(error) => Err(format!("{} : {error}", path.display())),
    }
}

fn write_settings(path: &Path, settings: &Map<String, Value>) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let text = serde_json::to_string_pretty(&Value::Object(settings.clone()))
        .map_err(|error| error.to_string())?;
    fs::write(path, format!("{text}\n")).map_err(|error| format!("{} : {error}", path.display()))
}

fn add_server(scope: &str, name: &str, server: Value) -> Result<String, String> {
    let path = scope_path(scope)?;
    let mut settings = read_settings(&path)?;
    let servers = settings
        .entry("mcpServers")
        .or_insert_with(|| Value::Object(Map::new()));
    let servers = servers
        .as_object_mut()
        .ok_or_else(|| format!("{} : mcpServers doit être un objet", path.display()))?;
    let replaced = servers.insert(name.to_string(), server).is_some();
    write_settings(&path, &settings)?;
    Ok(format!(
        "Serveur MCP « {name} » {} dans {}\n  Vérifier          claw mcp list",
        if replaced { "remplacé" } else { "ajouté" },
        path.display()
    ))
}

fn remove_server(scopes: &[&str], name: &str) -> Result<String, String> {
    let mut removed = Vec::new();
    for scope in scopes {
        let path = scope_path(scope)?;
        let mut settings = read_settings(&path)?;
        let found = settings
            .get_mut("mcpServers")
            .and_then(Value::as_object_mut)
            .is_some_and(|servers| servers.remove(name).is_some());
        if found {
            write_settings(&path, &settings)?;
            removed.push(path.display().to_string());
        }
    }
    if removed.is_empty() {
        Err(format!(
            "aucun serveur « {name} » dans les fichiers de configuration de Claw"
        ))
    } else {
        Ok(format!(
            "Serveur MCP « {name} » retiré de {}",
            removed.join(", ")
        ))
    }
}

fn split_header(value: &str) -> Result<(String, String), String> {
    let (name, value) = value
        .split_once(':')
        .ok_or_else(|| format!("en-tête invalide « {value} » (attendu : \"Nom: valeur\")"))?;
    Ok((name.trim().to_string(), value.trim().to_string()))
}

fn parse_add(args: &[String]) -> Result<String, String> {
    let mut scope = "project".to_string();
    let mut transport = "stdio".to_string();
    let mut env_vars = Map::new();
    let mut headers = Map::new();
    let mut positional = Vec::new();
    let mut index = 0;
    while index < args.len() {
        let arg = args[index].as_str();
        let value = || {
            args.get(index + 1)
                .cloned()
                .ok_or_else(|| format!("valeur manquante pour {arg}"))
        };
        match arg {
            "--" => {
                positional.extend(args[index + 1..].iter().cloned());
                break;
            }
            "--scope" | "-s" => {
                scope = value()?;
                index += 2;
            }
            "--transport" | "-t" => {
                transport = value()?;
                index += 2;
            }
            "-e" | "--env" => {
                let pair = value()?;
                let (key, val) = pair.split_once('=').ok_or_else(|| {
                    format!("variable invalide « {pair} » (attendu : CLE=valeur)")
                })?;
                env_vars.insert(key.to_string(), Value::String(val.to_string()));
                index += 2;
            }
            "-H" | "--header" => {
                let (name, val) = split_header(&value()?)?;
                headers.insert(name, Value::String(val));
                index += 2;
            }
            other => {
                positional.push(other.to_string());
                index += 1;
            }
        }
    }
    let (name, target) = match positional.as_slice() {
        [name, target, ..] => (name.clone(), target.clone()),
        _ => return Err(USAGE.to_string()),
    };
    let server = match transport.as_str() {
        "stdio" => {
            let mut server = json!({ "command": target, "args": positional[2..] });
            if !env_vars.is_empty() {
                server["env"] = Value::Object(env_vars);
            }
            server
        }
        "http" | "sse" => {
            let mut server = json!({ "type": transport, "url": target });
            if !headers.is_empty() {
                server["headers"] = Value::Object(headers);
            }
            server
        }
        other => {
            return Err(format!(
                "transport inconnu « {other} » : stdio, http ou sse"
            ))
        }
    };
    add_server(&scope, &name, server)
}

fn server_url(name: &str) -> Result<String, String> {
    let cwd = env::current_dir().map_err(|error| error.to_string())?;
    let config = ConfigLoader::default_for(&cwd)
        .load()
        .map_err(|error| error.to_string())?;
    let server = config
        .mcp()
        .get(name)
        .ok_or_else(|| format!("serveur MCP « {name} » absent de la configuration"))?;
    match &server.config {
        McpServerConfig::Http(remote) | McpServerConfig::Sse(remote) => Ok(remote.url.clone()),
        _ => Err(format!(
            "« {name} » n'est pas un serveur distant (http ou sse)"
        )),
    }
}

fn describe(name: &str) -> Result<String, String> {
    let cwd = env::current_dir().map_err(|error| error.to_string())?;
    let config = ConfigLoader::default_for(&cwd)
        .load()
        .map_err(|error| error.to_string())?;
    let server = config
        .mcp()
        .get(name)
        .ok_or_else(|| format!("serveur MCP « {name} » absent de la configuration"))?;
    let detail = match &server.config {
        McpServerConfig::Stdio(stdio) => format!(
            "  Transport        stdio\n  Commande         {} {}",
            stdio.command,
            stdio.args.join(" ")
        ),
        McpServerConfig::Http(remote) | McpServerConfig::Sse(remote) => format!(
            "  Transport        {}\n  URL              {}\n  Connexion OAuth  {}",
            if matches!(server.config, McpServerConfig::Sse(_)) {
                "sse"
            } else {
                "http"
            },
            remote.url,
            if tools::mcp_oauth::has_token(name) {
                "oui"
            } else {
                "non (claw mcp login si nécessaire)"
            }
        ),
        _ => "  Transport        non pris en charge".to_string(),
    };
    Ok(format!(
        "MCP · {name}\n  Portée           {:?}\n{detail}",
        server.scope
    ))
}

/// Exécute `claw mcp <args>` et renvoie le texte à afficher.
pub fn run(args: &[String]) -> Result<String, String> {
    let command = args.first().map_or("list", String::as_str);
    let rest = args.get(1..).unwrap_or_default();
    match command {
        "add" => parse_add(rest),
        "add-json" => {
            let [name, json_text, ..] = rest else {
                return Err(USAGE.to_string());
            };
            let server: Value = serde_json::from_str(json_text)
                .map_err(|error| format!("JSON invalide : {error}"))?;
            add_server("project", name, server)
        }
        "remove" | "rm" => {
            let (scopes, name): (Vec<&str>, &str) = match rest {
                [flag, scope, name, ..] if flag == "--scope" || flag == "-s" => {
                    (vec![scope.as_str()], name)
                }
                [name, ..] => (vec!["local", "project", "user"], name),
                [] => return Err(USAGE.to_string()),
            };
            remove_server(&scopes, name)
        }
        "get" => rest
            .first()
            .map_or_else(|| Err(USAGE.to_string()), |name| describe(name)),
        "login" => {
            let name = rest.first().ok_or_else(|| USAGE.to_string())?;
            tools::mcp_oauth::login(name, &server_url(name)?)
        }
        "logout" => {
            let name = rest.first().ok_or_else(|| USAGE.to_string())?;
            Ok(if tools::mcp_oauth::logout(name)? {
                format!("Connexion OAuth de « {name} » oubliée.")
            } else {
                format!("Aucune connexion OAuth enregistrée pour « {name} ».")
            })
        }
        "list" | "ls" => {
            let cwd = env::current_dir().map_err(|error| error.to_string())?;
            let config = ConfigLoader::default_for(&cwd)
                .load()
                .map_err(|error| error.to_string())?;
            let _ = tools::ensure_mcp_started(&config);
            Ok(tools::mcp_status_report(&config))
        }
        "help" | "--help" | "-h" => Ok(USAGE.to_string()),
        other => Err(format!("sous-commande mcp inconnue « {other} »\n{USAGE}")),
    }
}
