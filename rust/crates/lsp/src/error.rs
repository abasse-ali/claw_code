use std::fmt::{Display, Formatter};
use std::path::PathBuf;

#[derive(Debug)]
pub enum LspError {
    Io(std::io::Error),
    Json(serde_json::Error),
    InvalidHeader(String),
    MissingContentLength,
    InvalidContentLength(String),
    UnsupportedDocument(PathBuf),
    UnknownServer(String),
    DuplicateExtension {
        extension: String,
        existing_server: String,
        new_server: String,
    },
    PathToUrl(PathBuf),
    Protocol(String),
}

impl Display for LspError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(f, "{error}"),
            Self::Json(error) => write!(f, "{error}"),
            Self::InvalidHeader(header) => write!(f, "en-tête LSP invalide : {header}"),
            Self::MissingContentLength => write!(f, "en-tête LSP Content-Length manquant"),
            Self::InvalidContentLength(value) => {
                write!(f, "valeur LSP Content-Length invalide : {value}")
            }
            Self::UnsupportedDocument(path) => {
                write!(f, "aucun serveur LSP configuré pour {}", path.display())
            }
            Self::UnknownServer(name) => write!(f, "serveur LSP inconnu : {name}"),
            Self::DuplicateExtension {
                extension,
                existing_server,
                new_server,
            } => write!(
                f,
                "mappage d'extension LSP en double pour {extension} : {existing_server} et {new_server}"
            ),
            Self::PathToUrl(path) => write!(f, "échec de conversion du chemin en URL de fichier : {}", path.display()),
            Self::Protocol(message) => write!(f, "erreur de protocole LSP : {message}"),
        }
    }
}

impl std::error::Error for LspError {}

impl From<std::io::Error> for LspError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<serde_json::Error> for LspError {
    fn from(value: serde_json::Error) -> Self {
        Self::Json(value)
    }
}
