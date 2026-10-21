// Modèles gratuits classés comme Claw les classe (api/src/openrouter.rs : rank_free_coding_models
// pour retenir les modèles bons en code, model_power pour les ordonner ; « Auto » prend le
// premier), et requêtes gratuites du jour lues chez OpenRouter avec la clé gardée sur l'appareil.
// Le catalogue et le classement « programming » sont publics : aucune clé, aucune requête du
// quota gratuit.

const API = "https://openrouter.ai/api/v1";

const CODING_SPECIALIZATION = ["coding agent", "agentic coding", "coding model", "code model",
  "code generation", "software engineering", "for coding"];
const CODE_BENCHMARKS = ["swe-bench", "terminal-bench", "livecodebench", "humaneval", "aider", "codeforces"];
const ANTI_CODING = ["against using it for agentic coding", "against using it for coding",
  "not suited for coding", "not recommended for coding", "not intended for coding"];
const OTHER_SPECIALTIES = ["health", "medicine", "medical", "legal", "roleplay"];
const LIGHTWEIGHT = ["mini", "nano", "tiny", "small", "lite"];

function parseBillions(token) {
  if (!token.endsWith("b") || token.length < 2) return null;
  const value = Number(token.slice(0, -1));
  return Number.isFinite(value) && value > 0 ? value : null;
}

const formatBillions = (value) => (Math.abs(value - Math.round(value)) < 0.05
  ? String(Math.round(value)) : value.toFixed(1).replace(".", ","));

/** Paramètres totaux (en milliards) : `-120b` dans l'identifiant, sinon « 120B parameters ». */
function totalBillions(description, id) {
  let best = null;
  for (const part of id.split(/[/\-:_]/)) {
    if (part.startsWith("a") && part.length >= 2 && /\d/.test(part[1])) continue;
    const value = parseBillions(part);
    if (value !== null && value < 5000) best = best === null ? value : Math.max(best, value);
  }
  if (best !== null) return best;
  const words = description.split(/[\s,()]/);
  for (let index = 0; index + 1 < words.length; index += 1) {
    if (words[index + 1].startsWith("param") || words[index + 1].startsWith("total")) {
      const value = parseBillions(words[index].replace(/-+$/, ""));
      if (value !== null) return value;
    }
  }
  return null;
}

/** Paramètres actifs (en milliards) d'un modèle MoE : « 55B active », « -a55b ». */
function activeBillions(description, id) {
  const words = description.split(/[\s,()]/);
  for (let index = 0; index + 1 < words.length; index += 1) {
    if (words[index + 1].startsWith("active")) {
      const value = parseBillions(words[index]);
      if (value !== null) return value;
    }
  }
  for (const raw of [...description.split(/[\s/]/), ...id.split(/[/\-:]/)]) {
    const token = raw.replace(/^[^a-z0-9-]+|[^a-z0-9-]+$/g, "");
    const at = token.lastIndexOf("-a");
    let after = null;
    if (at >= 0) after = token.slice(at + 2);
    else if (token.startsWith("a") && token.endsWith("b") && token.length > 2) after = token.slice(1);
    const value = after === null ? null : parseBillions(after);
    if (value !== null) return value;
  }
  return null;
}

const rankOf = (model, ranking) => {
  const base = model.id.replace(/:free$/, "");
  return ranking.findIndex((ranked) => ranked === model.id || ranked === base);
};

/** Le modèle est-il retenu pour le code (rank_free_coding_models) ? */
function goodAtCode(model, ranking) {
  const description = (model.description || "").toLowerCase();
  const id = model.id.toLowerCase();
  if (ANTI_CODING.some((phrase) => description.includes(phrase))) return false;
  const first = description.split(". ")[0];
  if (OTHER_SPECIALTIES.some((specialty) => first.includes(specialty)) && !first.includes("cod")) return false;
  return rankOf(model, ranking) >= 0
    || id.includes("coder") || id.includes("-code") || id.includes("codestral")
    || CODING_SPECIALIZATION.some((phrase) => description.includes(phrase))
    || description.includes("coding") || description.includes("programming");
}

/** Puissance estimée pour le code et ses raisons (model_power). */
function power(model, ranking) {
  const description = (model.description || "").toLowerCase();
  const id = model.id.toLowerCase();
  let score = 0;
  const reasons = [];
  const position = rankOf(model, ranking);
  if (position >= 0) {
    score += Math.max(0, 100 - position);
    reasons.push(`n°${position + 1} en programmation`);
  }
  const total = totalBillions(description, id);
  const active = activeBillions(description, id) ?? total;
  const effective = active !== null && total !== null ? Math.sqrt(active * total) : (active ?? total);
  if (effective !== null) score += Math.log2(Math.min(Math.max(effective, 1), 120)) * 12;
  if (active !== null && total !== null && Math.abs(active - total) > 0.5) {
    reasons.push(`${formatBillions(total)} Md de paramètres (${formatBillions(active)} actifs)`);
  } else if (total !== null) {
    reasons.push(`${formatBillions(total)} Md de paramètres`);
  } else if (active !== null) {
    reasons.push(`${formatBillions(active)} Md de paramètres actifs`);
  }
  if (id.includes("coder") || id.includes("-code") || id.includes("codestral")
      || CODING_SPECIALIZATION.some((phrase) => description.includes(phrase))) {
    score += 25;
    reasons.push("spécialisé code");
  } else if (description.includes("coding") || description.includes("programming")) {
    score += 10;
  }
  if (CODE_BENCHMARKS.some((needle) => description.includes(needle))) score += 10;
  if (id.split(/[/\-:._]/).some((part) => LIGHTWEIGHT.includes(part))) {
    score -= 10;
    reasons.push("version allégée");
  }
  if ((model.context_length || 0) >= 200000) score += 5;
  return { score, reasons };
}

const isFree = (model) => Number(model.pricing && model.pricing.prompt) === 0
  && Number(model.pricing && model.pricing.completion) === 0;
const hasTools = (model) => (model.supported_parameters || []).includes("tools");
const shortName = (model) => (model.name || model.id).replace(/\s*\(free\)\s*$/i, "").replace(/^[^:]+:\s*/, "");

/** Modèles gratuits utilisables par Claw : `ranked` du plus puissant au moins puissant pour
 * le code (le premier est celui d'« Auto »), puis `others` (non retenus pour le code). */
export async function freeModels() {
  const get = async (url) => {
    const response = await fetch(url, { cache: "no-store" });
    if (!response.ok) throw new Error(`OpenRouter : ${response.status}`);
    return (await response.json()).data || [];
  };
  const [catalog, programming] = await Promise.all([
    get(`${API}/models`),
    get(`${API}/models?category=programming`).catch(() => []),
  ]);
  const ranking = programming.map((model) => model.id);
  const usable = catalog.filter((model) => isFree(model) && hasTools(model) && !model.id.startsWith("openrouter/"));
  const ranked = usable.filter((model) => goodAtCode(model, ranking))
    .map((model) => ({ model, ...power(model, ranking) }))
    .sort((left, right) => right.score - left.score)
    .map(({ model, reasons }) => ({ id: model.id, short: shortName(model), reasons, context: model.context_length || 0 }));
  const others = usable.filter((model) => !goodAtCode(model, ranking))
    .map((model) => ({ model, ...power(model, ranking) }))
    .sort((left, right) => right.score - left.score)
    .map(({ model, reasons }) => ({ id: model.id, short: shortName(model), reasons, context: model.context_length || 0 }));
  return { ranked, others };
}

/** Tout le catalogue gratuit d'OpenRouter, outils ou non, classé pour le code : ceux qui
 * appellent des outils d'abord (les seuls avec lesquels Claw peut travailler), du plus puissant
 * au moins puissant ; un modèle qui ne se présente pas comme bon en code passe après les autres
 * de même puissance. Même forme que le catalogue du moteur (models_catalog.py). */
export async function allFreeModels() {
  const get = async (url) => {
    const response = await fetch(url, { cache: "no-store" });
    if (!response.ok) throw new Error(`OpenRouter : ${response.status}`);
    return (await response.json()).data || [];
  };
  const [catalog, programming] = await Promise.all([
    get(`${API}/models`),
    get(`${API}/models?category=programming`).catch(() => []),
  ]);
  const ranking = programming.map((model) => model.id);
  return catalog.filter((model) => isFree(model) && !model.id.startsWith("openrouter/"))
    .map((model) => {
      const { score, reasons } = power(model, ranking);
      const code = goodAtCode(model, ranking);
      const tools = hasTools(model);
      return {
        id: model.id,
        short: shortName(model),
        score: score - (code ? 0 : 15),
        reasons: code ? reasons : [...reasons, "pas présenté comme bon en code"],
        context: model.context_length || 0,
        tools,
        status: tools ? "ok" : "n'appelle pas d'outils",
      };
    })
    .sort((left, right) => (Number(right.tools) - Number(left.tools)) || (right.score - left.score));
}

/** Requêtes gratuites du jour (utilisées, limite, restantes) et crédits, avec la clé donnée. */
export async function accountStatus(key) {
  const headers = { Authorization: `Bearer ${key}` };
  const [info, credits] = await Promise.all([
    fetch(`${API}/key`, { headers, cache: "no-store" }),
    fetch(`${API}/credits`, { headers, cache: "no-store" }).catch(() => null),
  ]);
  if (info.status === 401 || info.status === 403) throw new Error("Clé OpenRouter refusée : vérifiez-la dans les réglages");
  if (!info.ok) throw new Error(`OpenRouter ne répond pas (${info.status})`);
  const data = (await info.json()).data || {};
  let balance = null;
  if (credits && credits.ok) {
    const amounts = (await credits.json()).data || {};
    if (typeof amounts.total_credits === "number") balance = amounts.total_credits - (amounts.total_usage || 0);
  }
  return { free: data.free_model_daily_requests || null, balance, at: Date.now() };
}

/** Heure locale de la remise à zéro du quota gratuit (minuit UTC). */
export function resetTime() {
  const now = new Date();
  const reset = new Date(Date.UTC(now.getUTCFullYear(), now.getUTCMonth(), now.getUTCDate() + 1));
  return reset.toLocaleTimeString("fr-FR", { hour: "2-digit", minute: "2-digit" });
}
