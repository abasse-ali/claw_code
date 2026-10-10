//! Catalogue `OpenRouter` : liste des modèles, classements par catégorie
//! d'usage, spécialités de chaque modèle, quota quotidien des modèles gratuits,
//! et sélection des modèles gratuits les mieux placés en code.

use std::collections::BTreeMap;
use std::sync::{OnceLock, RwLock};

use serde::{Deserialize, Serialize};

use crate::error::ApiError;

/// Modèle du catalogue, réduit aux informations utiles à l'agent.
// Une capacité indépendante par booléen (outils, images, son, vidéo, raisonnement).
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct OpenRouterModel {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub context_length: Option<u64>,
    pub max_completion_tokens: Option<u64>,
    /// Prix en dollars par jeton (négatif si inconnu, par exemple pour un routeur).
    pub prompt_price: f64,
    pub completion_price: f64,
    pub supports_tools: bool,
    /// Accepte des images en entrée (modèle multimodal).
    #[serde(default)]
    pub supports_images: bool,
    /// Accepte du son en entrée.
    #[serde(default)]
    pub supports_audio: bool,
    /// Accepte de la vidéo en entrée.
    #[serde(default)]
    pub supports_video: bool,
    /// Sait raisonner avant de répondre (paramètre `reasoning`).
    #[serde(default)]
    pub supports_reasoning: bool,
}

impl OpenRouterModel {
    #[must_use]
    pub fn is_free(&self) -> bool {
        self.prompt_price.abs() < f64::EPSILON && self.completion_price.abs() < f64::EPSILON
    }
}

/// Catalogue complet tel que mis en cache sur disque.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ModelCatalog {
    /// Format du cache : un catalogue d'une version antérieure est relu.
    #[serde(default)]
    pub version: u32,
    /// Horodatage Unix (secondes) du téléchargement.
    pub fetched_at: u64,
    pub models: Vec<OpenRouterModel>,
    /// Identifiants du classement « programming » d'`OpenRouter`, du premier au dernier.
    pub programming_ranking: Vec<String>,
    /// Classement de chaque catégorie d'usage d'`OpenRouter` (voir
    /// [`OPENROUTER_CATEGORIES`]) : identifiants, du plus utilisé au moins utilisé.
    #[serde(default)]
    pub category_rankings: BTreeMap<String, Vec<String>>,
}

/// Version du format de [`ModelCatalog`] écrit par cette version de Claw.
pub const CATALOG_VERSION: u32 = 2;

/// Catégories d'usage d'`OpenRouter` (`GET /models?category=…`) et leur nom.
pub const OPENROUTER_CATEGORIES: &[(&str, &str)] = &[
    ("programming", "Programmation"),
    ("academia", "Apprentissage et recherche"),
    ("science", "Sciences"),
    ("technology", "Technologie"),
    ("translation", "Traduction"),
    ("finance", "Finance"),
    ("health", "Santé"),
    ("legal", "Droit"),
    ("marketing", "Marketing"),
    ("marketing/seo", "Référencement"),
    ("roleplay", "Jeu de rôle"),
    ("trivia", "Culture générale"),
];

/// Spécialités affichées, dans l'ordre : catégories d'`OpenRouter` et
/// capacités lues dans le catalogue (images, son, raisonnement, contexte).
pub const SPECIALTIES: &[(&str, &str)] = &[
    ("programming", "Programmation"),
    ("reasoning", "Raisonnement"),
    ("vision", "Images"),
    ("audio_video", "Audio et vidéo"),
    ("academia", "Apprentissage et recherche"),
    ("science", "Sciences"),
    ("technology", "Technologie"),
    ("data", "Données et documents"),
    ("long_context", "Long contexte"),
    ("translation", "Traduction"),
    ("finance", "Finance"),
    ("health", "Santé"),
    ("legal", "Droit"),
    ("marketing", "Marketing"),
    ("marketing/seo", "Référencement"),
    ("roleplay", "Jeu de rôle"),
    ("trivia", "Culture générale"),
];

/// Spécialité d'un modèle et ce qui la justifie.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Specialty {
    pub key: &'static str,
    pub label: &'static str,
    /// Le fait sur lequel elle repose : « n°3 chez `OpenRouter` », « lit les images »…
    pub detail: String,
}

/// Contexte à partir duquel un modèle est dit « long contexte » (en jetons).
const LONG_CONTEXT_TOKENS: u64 = 500_000;

/// Quota quotidien des modèles gratuits (`GET /key`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FreeModelQuota {
    pub used: u64,
    pub limit: u64,
    pub remaining: u64,
}

/// Modèle gratuit retenu pour le code, avec son score et les raisons du classement.
#[derive(Debug, Clone, PartialEq)]
pub struct RankedModel {
    pub model: OpenRouterModel,
    pub score: f64,
    pub reasons: Vec<String>,
}

#[derive(Deserialize)]
struct RawModelList {
    data: Vec<RawModel>,
}

#[derive(Deserialize)]
struct RawModel {
    id: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    context_length: Option<f64>,
    #[serde(default)]
    pricing: RawPricing,
    #[serde(default)]
    top_provider: Option<RawTopProvider>,
    #[serde(default)]
    supported_parameters: Option<Vec<String>>,
    #[serde(default)]
    architecture: Option<RawArchitecture>,
}

#[derive(Default, Deserialize)]
struct RawArchitecture {
    #[serde(default)]
    input_modalities: Option<Vec<String>>,
}

#[derive(Default, Deserialize)]
struct RawPricing {
    #[serde(default)]
    prompt: Option<String>,
    #[serde(default)]
    completion: Option<String>,
}

#[derive(Deserialize)]
struct RawTopProvider {
    #[serde(default)]
    max_completion_tokens: Option<f64>,
}

#[derive(Deserialize)]
struct RawKeyInfo {
    data: RawKeyData,
}

#[derive(Deserialize)]
struct RawKeyData {
    #[serde(default)]
    free_model_daily_requests: Option<RawQuota>,
    #[serde(default)]
    usage_daily: Option<f64>,
    #[serde(default)]
    limit_remaining: Option<f64>,
}

#[derive(Deserialize)]
struct RawCredits {
    data: RawCreditsData,
}

#[derive(Deserialize)]
struct RawCreditsData {
    #[serde(default)]
    total_credits: f64,
    #[serde(default)]
    total_usage: f64,
}

/// État du compte `OpenRouter` : crédits, dépense du jour de la clé, quota gratuit.
#[derive(Debug, Clone, PartialEq)]
pub struct AccountStatus {
    /// Crédits achetés (en dollars), `None` si l'API ne les donne pas à cette clé.
    pub credits_total: Option<f64>,
    /// Crédits consommés (en dollars).
    pub credits_used: Option<f64>,
    /// Dépense du jour de cette clé (en dollars).
    pub key_usage_today: Option<f64>,
    /// Plafond restant de cette clé, si elle en a un.
    pub key_limit_remaining: Option<f64>,
    /// Requêtes gratuites du jour.
    pub free_requests: Option<FreeModelQuota>,
}

impl AccountStatus {
    /// Solde restant (en dollars).
    #[must_use]
    pub fn balance(&self) -> Option<f64> {
        Some(self.credits_total? - self.credits_used?)
    }
}

/// Crédits (`GET /credits`) et clé (`GET /key`), lus en parallèle.
pub async fn fetch_account_status(
    base_url: &str,
    api_key: &str,
) -> Result<AccountStatus, ApiError> {
    let base = base_url.trim_end_matches('/');
    let client = reqwest::Client::new();
    let get = |path: &str| {
        client
            .get(format!("{base}/{path}"))
            .bearer_auth(api_key)
            .send()
    };
    let (credits, key) = tokio::join!(get("credits"), get("key"));
    let credits = match credits {
        Ok(response) if response.status().is_success() => {
            serde_json::from_str::<RawCredits>(&response.text().await?)
                .ok()
                .map(|raw| raw.data)
        }
        Ok(_) => None,
        Err(error) => return Err(error.into()),
    };
    let key = match key {
        Ok(response) if response.status().is_success() => {
            serde_json::from_str::<RawKeyInfo>(&response.text().await?)
                .ok()
                .map(|raw| raw.data)
        }
        Ok(response) => {
            let status = response.status();
            if credits.is_none() {
                // Clé refusée : rien à afficher, l'erreur dit pourquoi.
                return Err(ApiError::Api {
                    status,
                    error_type: None,
                    message: None,
                    body: response.text().await.unwrap_or_default(),
                    retryable: false,
                });
            }
            None
        }
        Err(error) => return Err(error.into()),
    };
    Ok(AccountStatus {
        credits_total: credits.as_ref().map(|data| data.total_credits),
        credits_used: credits.as_ref().map(|data| data.total_usage),
        key_usage_today: key.as_ref().and_then(|data| data.usage_daily),
        key_limit_remaining: key.as_ref().and_then(|data| data.limit_remaining),
        free_requests: key
            .and_then(|data| data.free_model_daily_requests)
            .map(|quota| FreeModelQuota {
                used: quota.used,
                limit: quota.limit,
                remaining: quota.remaining,
            }),
    })
}

#[derive(Deserialize)]
struct RawQuota {
    #[serde(default)]
    used: u64,
    #[serde(default)]
    limit: u64,
    #[serde(default)]
    remaining: u64,
}

/// Télécharge le catalogue et le classement de chaque catégorie d'usage. Ces
/// lectures sont publiques : aucune clé n'est envoyée et aucune requête du
/// quota gratuit n'est consommée.
pub async fn fetch_model_catalog(base_url: &str) -> Result<ModelCatalog, ApiError> {
    let http = reqwest::Client::new();
    let base = base_url.trim_end_matches('/');
    let models = get_models(&http, &format!("{base}/models")).await?;
    // Les classements sont un plus : leur absence ne doit pas empêcher /models.
    let mut requests = tokio::task::JoinSet::new();
    for (category, _) in OPENROUTER_CATEGORIES {
        let http = http.clone();
        let url = format!("{base}/models?category={category}");
        requests.spawn(async move { (*category, get_models(&http, &url).await) });
    }
    let mut category_rankings = BTreeMap::new();
    while let Some(answer) = requests.join_next().await {
        if let Ok((category, Ok(ranked))) = answer {
            let ids: Vec<String> = ranked.into_iter().map(|model| model.id).collect();
            category_rankings.insert(category.to_string(), ids);
        }
    }
    Ok(ModelCatalog {
        version: CATALOG_VERSION,
        fetched_at: unix_now(),
        models,
        programming_ranking: category_rankings
            .get("programming")
            .cloned()
            .unwrap_or_default(),
        category_rankings,
    })
}

async fn get_models(http: &reqwest::Client, url: &str) -> Result<Vec<OpenRouterModel>, ApiError> {
    let response = http.get(url).send().await?;
    let status = response.status();
    let body = response.text().await?;
    if !status.is_success() {
        return Err(ApiError::Api {
            status,
            error_type: None,
            message: None,
            body,
            retryable: false,
        });
    }
    parse_model_list(&body)
}

/// Quota gratuit de la clé (`None` si le serveur ne le fournit pas).
pub async fn fetch_free_quota(
    base_url: &str,
    api_key: &str,
) -> Result<Option<FreeModelQuota>, ApiError> {
    let url = format!("{}/key", base_url.trim_end_matches('/'));
    let response = reqwest::Client::new()
        .get(url)
        .bearer_auth(api_key)
        .send()
        .await?;
    if !response.status().is_success() {
        return Ok(None);
    }
    let info: RawKeyInfo = serde_json::from_str(&response.text().await?)?;
    Ok(info
        .data
        .free_model_daily_requests
        .map(|quota| FreeModelQuota {
            used: quota.used,
            limit: quota.limit,
            remaining: quota.remaining,
        }))
}

fn parse_model_list(body: &str) -> Result<Vec<OpenRouterModel>, ApiError> {
    let list: RawModelList = serde_json::from_str(body)?;
    let price = |value: Option<&String>| {
        value
            .and_then(|value| value.trim().parse::<f64>().ok())
            .unwrap_or(-1.0)
    };
    Ok(list
        .data
        .into_iter()
        .map(|raw| {
            let parameters = raw.supported_parameters.unwrap_or_default();
            let supports = |wanted: &str| parameters.iter().any(|parameter| parameter == wanted);
            let inputs = raw
                .architecture
                .and_then(|architecture| architecture.input_modalities)
                .unwrap_or_default();
            let reads = |wanted: &str| inputs.iter().any(|modality| modality == wanted);
            OpenRouterModel {
                name: if raw.name.is_empty() {
                    raw.id.clone()
                } else {
                    raw.name
                },
                description: raw.description.unwrap_or_default(),
                context_length: raw.context_length.and_then(non_negative_u64),
                max_completion_tokens: raw
                    .top_provider
                    .and_then(|provider| provider.max_completion_tokens)
                    .and_then(non_negative_u64),
                prompt_price: price(raw.pricing.prompt.as_ref()),
                completion_price: price(raw.pricing.completion.as_ref()),
                supports_tools: supports("tools"),
                supports_images: reads("image"),
                supports_audio: reads("audio"),
                supports_video: reads("video"),
                supports_reasoning: supports("reasoning") || supports("include_reasoning"),
                id: raw.id,
            }
        })
        .collect())
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn non_negative_u64(value: f64) -> Option<u64> {
    (value.is_finite() && value >= 0.0).then_some(value as u64)
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

// Formulations par lesquelles une description présente un modèle spécialisé code.
const CODING_SPECIALIZATION: &[&str] = &[
    "coding agent",
    "agentic coding",
    "coding model",
    "code model",
    "code generation",
    "software engineering",
    "for coding",
];
const CODE_BENCHMARKS: &[(&str, &str)] = &[
    ("swe-bench", "SWE-bench"),
    ("terminal-bench", "Terminal-Bench"),
    ("livecodebench", "LiveCodeBench"),
    ("humaneval", "HumanEval"),
    ("aider", "Aider"),
    ("codeforces", "Codeforces"),
];
const ANTI_CODING: &[&str] = &[
    "against using it for agentic coding",
    "against using it for coding",
    "not suited for coding",
    "not recommended for coding",
    "not intended for coding",
];
const OTHER_SPECIALTIES: &[&str] = &["health", "medicine", "medical", "legal", "roleplay"];

/// Modèles gratuits, capables d'appeler des outils et bien placés en code, du
/// meilleur au moins bon. Le classement repose sur des faits vérifiables : rang
/// dans la catégorie « programming » d'`OpenRouter`, spécialisation et benchmarks
/// de code cités dans la description, puis taille (paramètres actifs).
#[must_use]
pub fn rank_free_coding_models(catalog: &ModelCatalog) -> Vec<RankedModel> {
    let mut ranked = catalog
        .models
        .iter()
        .filter(|model| model.is_free() && model.supports_tools)
        // Les routeurs (openrouter/free…) choisissent un modèle au hasard.
        .filter(|model| !model.id.starts_with("openrouter/"))
        .filter_map(|model| score_coding_model(model, catalog))
        .collect::<Vec<_>>();
    ranked.sort_by(|left, right| {
        right
            .score
            .total_cmp(&left.score)
            .then_with(|| right.model.context_length.cmp(&left.model.context_length))
    });
    ranked
}

fn score_coding_model(model: &OpenRouterModel, catalog: &ModelCatalog) -> Option<RankedModel> {
    let description = model.description.to_ascii_lowercase();
    let id = model.id.to_ascii_lowercase();
    if ANTI_CODING
        .iter()
        .any(|phrase| description.contains(phrase))
    {
        return None;
    }
    let first_sentence = description.split(". ").next().unwrap_or_default();
    if OTHER_SPECIALTIES
        .iter()
        .any(|specialty| first_sentence.contains(specialty))
        && !first_sentence.contains("cod")
    {
        return None;
    }

    let mut score = 0.0;
    let mut reasons = Vec::new();
    let mut coding_signal = false;

    // Rang dans le classement programmation : la variante payante du même modèle compte aussi.
    let base_id = model.id.trim_end_matches(":free");
    if let Some(position) = catalog
        .programming_ranking
        .iter()
        .position(|ranked| ranked == &model.id || ranked == base_id)
    {
        #[allow(clippy::cast_precision_loss)]
        let bonus = 100.0 - position as f64;
        score += bonus;
        coding_signal = true;
        reasons.push(format!(
            "n°{} du classement programmation OpenRouter",
            position + 1
        ));
    }

    let specialized = id.contains("coder")
        || id.contains("-code")
        || id.contains("codestral")
        || CODING_SPECIALIZATION
            .iter()
            .any(|phrase| description.contains(phrase));
    if specialized {
        score += 40.0;
        coding_signal = true;
        reasons.push("spécialisé code".to_string());
    } else if description.contains("coding") || description.contains("programming") {
        // Le simple mot « code » (« files, data, code… ») ne suffit pas.
        score += 20.0;
        coding_signal = true;
        reasons.push("le code fait partie de ses points forts".to_string());
    }

    let benchmarks = CODE_BENCHMARKS
        .iter()
        .filter(|(needle, _)| description.contains(needle))
        .map(|(_, label)| *label)
        .collect::<Vec<_>>();
    if !benchmarks.is_empty() {
        score += 15.0;
        reasons.push(format!(
            "benchmark de code cité ({})",
            benchmarks.join(", ")
        ));
    }

    if !coding_signal {
        return None;
    }

    if let Some(active) = active_parameters_billions(&description, &id) {
        score += active.min(60.0) * 0.8;
        reasons.push(format!(
            "{} Md de paramètres actifs",
            format_billions(active)
        ));
    }

    Some(RankedModel {
        model: model.clone(),
        score,
        reasons,
    })
}

/// Rang (à partir de 1) d'un modèle dans une catégorie d'`OpenRouter` ; la
/// variante payante d'un modèle gratuit compte aussi.
#[must_use]
pub fn category_rank(model_id: &str, category: &str, catalog: &ModelCatalog) -> Option<usize> {
    let base_id = model_id.trim_end_matches(":free");
    let ranking = match catalog.category_rankings.get(category) {
        Some(ranking) => ranking,
        None if category == "programming" => &catalog.programming_ranking,
        None => return None,
    };
    ranking
        .iter()
        .position(|ranked| ranked == model_id || ranked == base_id)
        .map(|position| position + 1)
}

/// Le mot entier `word` figure dans `text` (déjà en minuscules).
fn has_word(text: &str, word: &str) -> bool {
    text.split(|ch: char| !ch.is_alphanumeric())
        .any(|token| token == word)
}

/// Spécialités d'un modèle, d'après des faits vérifiables : son rang dans les
/// catégories d'usage d'`OpenRouter`, ce qu'il sait lire (images, son, vidéo),
/// la taille de son contexte, et ce que sa description met en avant.
#[must_use]
pub fn model_specialties(model: &OpenRouterModel, catalog: &ModelCatalog) -> Vec<Specialty> {
    let description = model.description.to_ascii_lowercase();
    let mentions = |phrases: &[&str]| phrases.iter().any(|phrase| description.contains(phrase));
    SPECIALTIES
        .iter()
        .filter_map(|(key, label)| {
            let ranked = category_rank(&model.id, key, catalog)
                .map(|rank| format!("n°{rank} de la catégorie chez OpenRouter"));
            let detail = ranked.or_else(|| match *key {
                "programming" => score_coding_model(model, catalog).map(|scored| {
                    scored
                        .reasons
                        .into_iter()
                        .find(|reason| !reason.contains("paramètres"))
                        .unwrap_or_else(|| "le code fait partie de ses points forts".to_string())
                }),
                "reasoning" => (model.supports_reasoning && description.contains("reasoning"))
                    .then(|| "raisonnement mis en avant dans sa description".to_string()),
                "vision" => model.supports_images.then(|| "lit les images".to_string()),
                "audio_video" => match (model.supports_audio, model.supports_video) {
                    (true, true) => Some("lit le son et la vidéo".to_string()),
                    (true, false) => Some("lit le son".to_string()),
                    (false, true) => Some("lit la vidéo".to_string()),
                    (false, false) => None,
                },
                "academia" => mentions(&[
                    "research",
                    "academic",
                    "education",
                    "tutor",
                    "forecasting",
                    "scientific",
                ])
                .then(|| "études et recherche citées dans sa description".to_string()),
                "data" => (mentions(&["data extraction", "document"])
                    || has_word(&description, "rag"))
                .then(|| {
                    "extraction de données et documents cités dans sa description".to_string()
                }),
                "long_context" => model
                    .context_length
                    .filter(|tokens| *tokens >= LONG_CONTEXT_TOKENS)
                    .map(|tokens| format!("contexte de {} jetons", group_thousands(tokens))),
                "translation" => mentions(&["multilingual", "translation"])
                    .then(|| "langues citées dans sa description".to_string()),
                _ => None,
            })?;
            Some(Specialty { key, label, detail })
        })
        .collect()
}

/// « 1 000 000 » : milliers séparés par une espace.
fn group_thousands(value: u64) -> String {
    let digits = value.to_string();
    let mut grouped = String::new();
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            grouped.push(' ');
        }
        grouped.push(digit);
    }
    grouped
}

/// Puissance estimée d'un modèle pour le code, indépendamment de sa
/// compatibilité avec Claw : rang dans le classement programmation
/// d'`OpenRouter`, taille (paramètres actifs et totaux, lus dans l'identifiant
/// ou la description), spécialisation code, benchmarks cités ; les versions
/// allégées (mini, nano…) sont pénalisées. Renvoie le score et ses raisons.
#[must_use]
pub fn model_power(model: &OpenRouterModel, catalog: &ModelCatalog) -> (f64, Vec<String>) {
    let description = model.description.to_ascii_lowercase();
    let id = model.id.to_ascii_lowercase();
    let mut score = 0.0;
    let mut reasons = Vec::new();

    let base_id = model.id.trim_end_matches(":free");
    if let Some(position) = catalog
        .programming_ranking
        .iter()
        .position(|ranked| ranked == &model.id || ranked == base_id)
    {
        #[allow(clippy::cast_precision_loss)]
        let bonus = 100.0 - position as f64;
        score += bonus.max(0.0);
        reasons.push(format!("n°{} en programmation", position + 1));
    }

    let total = total_parameters_billions(&description, &id);
    let active = active_parameters_billions(&description, &id).or(total);
    // Taille effective : un MoE vaut à peu près la moyenne géométrique de ses
    // paramètres totaux et actifs ; la capacité croît comme le logarithme.
    let effective = match (active, total) {
        (Some(active), Some(total)) => Some((active * total).sqrt()),
        (active, total) => active.or(total),
    };
    if let Some(effective) = effective {
        score += effective.clamp(1.0, 120.0).log2() * 12.0;
    }
    match (active, total) {
        (Some(active), Some(total)) if (active - total).abs() > 0.5 => reasons.push(format!(
            "{} Md de paramètres ({} actifs)",
            format_billions(total),
            format_billions(active)
        )),
        (_, Some(total)) => reasons.push(format!("{} Md de paramètres", format_billions(total))),
        (Some(active), None) => reasons.push(format!(
            "{} Md de paramètres actifs",
            format_billions(active)
        )),
        (None, None) => {}
    }

    if id.contains("coder")
        || id.contains("-code")
        || id.contains("codestral")
        || CODING_SPECIALIZATION
            .iter()
            .any(|phrase| description.contains(phrase))
    {
        score += 25.0;
        reasons.push("spécialisé code".to_string());
    } else if description.contains("coding") || description.contains("programming") {
        score += 10.0;
    }
    if CODE_BENCHMARKS
        .iter()
        .any(|(needle, _)| description.contains(needle))
    {
        score += 10.0;
    }
    let lightweight = id
        .split(['/', '-', ':', '.', '_'])
        .any(|part| matches!(part, "mini" | "nano" | "tiny" | "small" | "lite"));
    if lightweight {
        score -= 10.0;
        reasons.push("version allégée".to_string());
    }
    if model.context_length.unwrap_or(0) >= 200_000 {
        score += 5.0;
    }
    (score, reasons)
}

/// Paramètres totaux (en milliards) : `-120b`, `27b`, `2.6b` dans l'identifiant,
/// sinon « 120B parameters » dans la description. Un `-a12b` (actifs) ne compte pas.
fn total_parameters_billions(description: &str, id: &str) -> Option<f64> {
    let from_id = id
        .split(['/', '-', ':', '_'])
        .filter(|part| {
            !part.starts_with('a')
                || part.len() < 2
                || !part[1..].starts_with(|ch: char| ch.is_ascii_digit())
        })
        .filter_map(|part| part.strip_suffix('b'))
        .filter_map(|number| number.parse::<f64>().ok())
        .filter(|value| *value > 0.0 && *value < 5_000.0)
        .fold(None, |best: Option<f64>, value| {
            Some(best.map_or(value, |best| best.max(value)))
        });
    if from_id.is_some() {
        return from_id;
    }
    let words = description
        .split(|ch: char| ch.is_whitespace() || ch == ',' || ch == '(' || ch == ')')
        .collect::<Vec<_>>();
    words.windows(2).find_map(|pair| {
        (pair[1].starts_with("param") || pair[1].starts_with("total"))
            .then(|| parse_billions(pair[0].trim_end_matches('-')))
            .flatten()
    })
}

/// Paramètres actifs (en milliards) d'un modèle `MoE`, lus dans la description
/// (« 55B active », « 33B-A3B ») ou l'identifiant (« -a55b »).
fn active_parameters_billions(description: &str, id: &str) -> Option<f64> {
    let words = description
        .split(|ch: char| ch.is_whitespace() || ch == ',' || ch == '(' || ch == ')')
        .collect::<Vec<_>>();
    for pair in words.windows(2) {
        if pair[1].starts_with("active") {
            if let Some(value) = parse_billions(pair[0]) {
                return Some(value);
            }
        }
    }
    description
        .split(|ch: char| ch.is_whitespace() || ch == '/')
        .chain(id.split(['/', '-', ':']))
        .find_map(|token| {
            let token = token.trim_matches(|ch: char| !ch.is_ascii_alphanumeric() && ch != '-');
            let after = token.rsplit_once("-a").map(|(_, rest)| rest).or_else(|| {
                token
                    .strip_prefix('a')
                    .filter(|rest| rest.ends_with('b') && rest.len() > 1)
            })?;
            parse_billions(after)
        })
}

fn parse_billions(token: &str) -> Option<f64> {
    let number = token.strip_suffix('b')?;
    number.parse::<f64>().ok().filter(|value| *value > 0.0)
}

fn format_billions(value: f64) -> String {
    if (value - value.round()).abs() < 0.05 {
        format!("{value:.0}")
    } else {
        format!("{value:.1}").replace('.', ",")
    }
}

/// Limites et prix connus d'un modèle, renseignés depuis le catalogue en cache.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ModelLimits {
    pub context_length: Option<u64>,
    pub max_completion_tokens: Option<u64>,
    pub prompt_price: f64,
    pub completion_price: f64,
    /// Le modèle sait raisonner avant de répondre (paramètre `reasoning`).
    pub supports_reasoning: bool,
}

fn limits_registry() -> &'static RwLock<BTreeMap<String, ModelLimits>> {
    static REGISTRY: OnceLock<RwLock<BTreeMap<String, ModelLimits>>> = OnceLock::new();
    REGISTRY.get_or_init(|| RwLock::new(BTreeMap::new()))
}

/// Enregistre les limites de tous les modèles du catalogue (pour `max_tokens`, `/cost`).
pub fn register_model_limits(catalog: &ModelCatalog) {
    let mut registry = limits_registry()
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    for model in &catalog.models {
        registry.insert(
            model.id.clone(),
            ModelLimits {
                context_length: model.context_length,
                max_completion_tokens: model.max_completion_tokens,
                prompt_price: model.prompt_price,
                completion_price: model.completion_price,
                supports_reasoning: model.supports_reasoning,
            },
        );
    }
}

#[must_use]
pub fn model_limits(model: &str) -> Option<ModelLimits> {
    limits_registry()
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get(model)
        .copied()
}

#[cfg(test)]
mod tests {
    use super::{
        active_parameters_billions, category_rank, model_power, model_specialties,
        parse_model_list, rank_free_coding_models, total_parameters_billions, ModelCatalog,
        OpenRouterModel,
    };
    use std::collections::BTreeMap;

    fn model(id: &str, description: &str, free: bool, tools: bool) -> OpenRouterModel {
        OpenRouterModel {
            id: id.to_string(),
            name: id.to_string(),
            description: description.to_string(),
            context_length: Some(262_144),
            max_completion_tokens: Some(32_768),
            prompt_price: if free { 0.0 } else { 0.000_001 },
            completion_price: if free { 0.0 } else { 0.000_002 },
            supports_tools: tools,
            supports_images: false,
            ..Default::default()
        }
    }

    #[test]
    fn parses_catalog_entries_prices_and_tool_support() {
        let models = parse_model_list(
            r#"{"data":[
                {"id":"a/free-coder:free","name":"A","description":"x","context_length":131072,
                 "pricing":{"prompt":"0","completion":"0"},
                 "top_provider":{"max_completion_tokens":8192},
                 "supported_parameters":["tools","temperature"]},
                {"id":"openrouter/auto","name":"Auto","pricing":{"prompt":"-1","completion":"-1"}}
            ]}"#,
        )
        .expect("catalog should parse");
        assert!(models[0].is_free());
        assert!(models[0].supports_tools);
        assert_eq!(models[0].max_completion_tokens, Some(8192));
        assert!(!models[1].is_free());
        assert!(!models[1].supports_tools);
    }

    #[test]
    fn ranks_only_free_tool_capable_coding_models_with_reasons() {
        let catalog = ModelCatalog {
            version: 0,
            category_rankings: BTreeMap::new(),
            fetched_at: 0,
            programming_ranking: vec!["big/frontier".to_string(), "lab/ultra:free".to_string()],
            models: vec![
                model("lab/ultra:free", "An open frontier reasoning model with 55B active parameters out of 550B.", true, true),
                model("pool/laguna-s:free", "The latest coding agent model, 8B active parameters, scoring 70% on Terminal-Bench.", true, true),
                model("gen/general:free", "A general multimodal assistant with 12B active parameters.", true, true),
                model("res/apodex:free", "A research model that works with files, data, code, and tools.", true, true),
                model("tiny/lfm:free", "Compact model. Liquid advises against using it for agentic coding.", true, true),
                model("med/ling-sante:free", "A health and medicine-focused model. Good at coding questions too.", true, true),
                model("big/frontier", "Best coding model.", false, true),
                model("x/notools-coder:free", "A coding model.", true, false),
                model("openrouter/free", "Router over free models for coding.", true, true),
            ],
        };
        let ranked = rank_free_coding_models(&catalog);
        let ids = ranked
            .iter()
            .map(|entry| entry.model.id.as_str())
            .collect::<Vec<_>>();
        assert_eq!(ids, vec!["lab/ultra:free", "pool/laguna-s:free"]);
        assert!(ranked[0].reasons[0].contains("n°2 du classement programmation"));
        assert!(ranked[1]
            .reasons
            .iter()
            .any(|reason| reason == "spécialisé code"));
        assert!(ranked[1]
            .reasons
            .iter()
            .any(|reason| reason.contains("Terminal-Bench")));
    }

    /// Classement sur un vrai catalogue :
    /// `CLAW_OR_MODELS=models.json CLAW_OR_PROGRAMMING=prog.json cargo test -p api -- --ignored --nocapture`
    #[test]
    #[ignore = "nécessite des fichiers de catalogue réels"]
    fn ranks_a_real_catalog_dump() {
        let read = |key: &str| {
            std::fs::read_to_string(std::env::var(key).expect("chemin du fichier"))
                .expect("lecture")
        };
        let catalog = ModelCatalog {
            version: 0,
            category_rankings: BTreeMap::new(),
            fetched_at: 0,
            models: parse_model_list(&read("CLAW_OR_MODELS")).expect("catalogue"),
            programming_ranking: parse_model_list(&read("CLAW_OR_PROGRAMMING"))
                .expect("classement")
                .into_iter()
                .map(|model| model.id)
                .collect(),
        };
        for (index, entry) in rank_free_coding_models(&catalog).iter().enumerate() {
            println!(
                "{:>2}. {:<45} score {:>6.1}  {}",
                index + 1,
                entry.model.id,
                entry.score,
                entry.reasons.join(" · ")
            );
        }
    }

    #[test]
    fn reads_active_parameters_from_descriptions_and_ids() {
        assert_eq!(
            active_parameters_billions("with 55b active parameters", "x"),
            Some(55.0)
        );
        assert_eq!(
            active_parameters_billions("30b total and 3b active", "x"),
            Some(3.0)
        );
        assert_eq!(
            active_parameters_billions("in the 33b-a3b category", "x"),
            Some(3.0)
        );
        assert_eq!(
            active_parameters_billions("no size here", "nvidia/nemotron-3-super-120b-a12b:free"),
            Some(12.0)
        );
        assert_eq!(active_parameters_billions("no size", "plain/model"), None);
    }

    #[test]
    fn specialties_come_from_rankings_capabilities_and_descriptions() {
        let models = parse_model_list(
            r#"{"data":[
                {"id":"lab/omni:free","name":"Omni","context_length":1000000,
                 "description":"A frontier reasoning model. Liquid advises against using it for agentic coding.",
                 "pricing":{"prompt":"0","completion":"0"},
                 "supported_parameters":["tools","reasoning"],
                 "architecture":{"input_modalities":["text","image","audio","video"]}},
                {"id":"pool/coder:free","name":"Coder","context_length":262144,
                 "description":"The latest coding agent model. Good at data extraction and RAG.",
                 "pricing":{"prompt":"0","completion":"0"},
                 "supported_parameters":["tools"]},
                {"id":"res/scholar:free","name":"Scholar","context_length":65536,
                 "description":"Built for long-horizon research and forecasting, with storage for drafts.",
                 "pricing":{"prompt":"0","completion":"0"},
                 "supported_parameters":["tools","include_reasoning"]}
            ]}"#,
        )
        .expect("catalogue");
        assert!(
            models[0].supports_audio && models[0].supports_video && models[0].supports_reasoning
        );
        assert!(!models[1].supports_reasoning && models[2].supports_reasoning);

        let catalog = ModelCatalog {
            category_rankings: BTreeMap::from([
                (
                    "translation".to_string(),
                    vec!["x/autre".to_string(), "lab/omni".to_string()],
                ),
                (
                    "programming".to_string(),
                    vec!["pool/coder:free".to_string()],
                ),
            ]),
            models: models.clone(),
            ..ModelCatalog::default()
        };
        assert_eq!(
            category_rank("lab/omni:free", "translation", &catalog),
            Some(2)
        );
        assert_eq!(category_rank("lab/omni:free", "finance", &catalog), None);

        let keys = |index: usize| -> Vec<&str> {
            model_specialties(&models[index], &catalog)
                .into_iter()
                .map(|specialty| specialty.key)
                .collect()
        };
        // Capacités et rang ; le code déconseillé par la description n'est pas une spécialité.
        assert_eq!(
            keys(0),
            [
                "reasoning",
                "vision",
                "audio_video",
                "long_context",
                "translation"
            ]
        );
        assert_eq!(keys(1), ["programming", "data"]);
        // « storage » ne contient pas le mot « rag ».
        assert_eq!(keys(2), ["academia"]);
        let omni = model_specialties(&models[0], &catalog);
        assert_eq!(omni[2].detail, "lit le son et la vidéo");
        assert_eq!(omni[3].detail, "contexte de 1 000 000 jetons");
        assert_eq!(omni[4].detail, "n°2 de la catégorie chez OpenRouter");
        assert_eq!(
            model_specialties(&models[1], &catalog)[0].detail,
            "n°1 de la catégorie chez OpenRouter"
        );
    }

    #[test]
    fn power_ranks_big_and_proven_models_above_small_or_light_ones() {
        let catalog = ModelCatalog {
            version: 0,
            category_rankings: BTreeMap::new(),
            fetched_at: 0,
            models: Vec::new(),
            programming_ranking: vec!["nvidia/nemotron-3-ultra-550b-a55b".to_string()],
        };
        let power = |id: &str, description: &str| {
            model_power(&model(id, description, true, true), &catalog).0
        };
        let ultra = power(
            "nvidia/nemotron-3-ultra-550b-a55b:free",
            "A 550B MoE model with 55B active parameters.",
        );
        let super_ = power("nvidia/nemotron-3-super-120b-a12b:free", "Hybrid MoE.");
        let gemma = power("google/gemma-4-31b-it:free", "Open model.");
        let coder = power(
            "cohere/north-mini-code:free",
            "A coding model with 3B active parameters.",
        );
        let tiny = power("liquid/lfm-2.5-2.6b:free", "Small and fast.");
        let mini = power("apodex/apodex-1.1-mini:free", "Fast assistant.");
        assert!(ultra > super_ && super_ > gemma, "{ultra} {super_} {gemma}");
        assert!(gemma > tiny && coder > tiny, "{gemma} {coder} {tiny}");
        assert!(tiny > mini || mini < 0.0, "{tiny} {mini}");

        assert_eq!(
            total_parameters_billions("", "nvidia/nemotron-3-super-120b-a12b"),
            Some(120.0)
        );
        assert_eq!(
            total_parameters_billions("", "liquid/lfm-2.5-2.6b"),
            Some(2.6)
        );
        assert_eq!(
            total_parameters_billions("a 70b parameters model", "x/y"),
            Some(70.0)
        );
        let (_, reasons) = model_power(
            &model("nvidia/nemotron-3-super-120b-a12b:free", "", true, true),
            &catalog,
        );
        assert_eq!(reasons, ["120 Md de paramètres (12 actifs)"]);
    }
}
