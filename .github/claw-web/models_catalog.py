"""Catalogue de tous les modèles gratuits d'un fournisseur, classés pour le code.

Lancé par le workflow claw-web.yml quand l'appli demande la session réservée
`catalogue-modeles` (bouton « Rechercher tous les modèles gratuits » du choix du modèle) :
le champ `model` porte le fournisseur (`openrouter`, `nvidia`, `gemini` ou `all`).

- OpenRouter : catalogue public ; gratuit = prix nuls ; outils = `tools` dans les paramètres.
- NVIDIA : catalogue public ; avec NVIDIA_API_KEY, chaque modèle de dialogue est essayé une
  fois avec un outil (environ 40 requêtes par minute permises) : il répond, appelle l'outil,
  et en combien de temps.
- Gemini : catalogue de l'API avec GEMINI_API_KEY ; chaque modèle de dialogue est essayé une
  fois : un refus « limit: 0 » veut dire qu'il n'est pas dans l'offre gratuite.

Le classement reprend celui de l'appli (claw-web/js/openrouter.js : goodAtCode et power) :
place dans le classement « programming » d'OpenRouter, taille (paramètres actifs et totaux),
spécialité code, bancs d'essai cités ; s'y ajoutent l'essai (outil appelé, temps de réponse)
et les modèles déjà vérifiés par Claw. Le résultat va sur la branche des fichiers du dépôt
des sessions (claw-web-fichiers), dans catalogue/<fournisseur>.json, que l'appli lit.
"""

import base64
import json
import math
import os
import re
import sys
import threading
import time
import urllib.error
import urllib.request
from concurrent.futures import ThreadPoolExecutor

from common import FILES_BRANCH, GitHubError, api

OPENROUTER = "https://openrouter.ai/api/v1"
NVIDIA = "https://integrate.api.nvidia.com/v1"
GEMINI = "https://generativelanguage.googleapis.com/v1beta"

# Modèles déjà essayés par Claw (rust/crates/claw-cli/src/models.rs, claw-web/js/app.js).
VERIFIED = {
    "nvidia:nvidia/nemotron-3-super-120b-a12b",
    "nvidia:nvidia/nemotron-3-ultra-550b-a55b",
    "nvidia:nvidia/nemotron-3.5-lightning-30b-a3b",
    "gemini:gemini-3.8-flash", "gemini:gemini-3.6-flash", "gemini:gemini-3.5-flash-lite",
    "gemini:gemini-3.1-flash-lite", "gemini:gemini-2.5-pro", "gemini:gemini-2.5-flash",
    "gemini:gemini-2.5-flash-lite",
}

CODING_SPECIALIZATION = ["coding agent", "agentic coding", "coding model", "code model",
                         "code generation", "software engineering", "for coding"]
CODE_BENCHMARKS = ["swe-bench", "terminal-bench", "livecodebench", "humaneval", "aider", "codeforces"]
LIGHTWEIGHT = {"mini", "nano", "tiny", "small", "lite"}
# Ce qui n'est pas un modèle de dialogue : inutile de l'essayer.
NOT_CHAT = re.compile(
    r"embed|rerank|retriev|parakeet|riva|whisper|tts|speech|audio|asr|flux|sdxl|stable-diffusion|"
    r"imagen|veo|image-generation|deplot|kosmos|paligemma|clip|nv-dino|detector|ocdr|ocr|"
    r"guard|safety|reward|shield|aqa|bge|e5-|cosmos|grounding|segment|depth|vista|"
    r"fuyu|neva|vila|cuopt|esm|evo2|genmol|diffdock|alphafold|molmim|proteinmpnn|rfdiffusion",
    re.I)

TOOL = {"type": "function", "function": {
    "name": "ping", "description": "Answer the check. Always call it.",
    "parameters": {"type": "object", "properties": {"value": {"type": "integer"}}, "required": ["value"]}}}
PROBE = [{"role": "user", "content": "Call the tool ping with value 1. Do not write anything else."}]


# --- Classement -----------------------------------------------------------------------------

def billions(token):
    token = token.lower()
    if not token.endswith("b") or len(token) < 2:
        return None
    try:
        value = float(token[:-1])
    except ValueError:
        return None
    return value if 0 < value < 5000 else None


def total_billions(description, model_id):
    best = None
    for part in re.split(r"[/\-:_]", model_id.lower()):
        if part.startswith("a") and len(part) >= 2 and part[1].isdigit():
            continue
        value = billions(part)
        if value is not None:
            best = value if best is None else max(best, value)
    if best is not None:
        return best
    words = re.split(r"[\s,()]", description.lower())
    for index in range(len(words) - 1):
        if words[index + 1].startswith(("param", "total")):
            value = billions(words[index].rstrip("-"))
            if value is not None:
                return value
    return None


def active_billions(description, model_id):
    words = re.split(r"[\s,()]", description.lower())
    for index in range(len(words) - 1):
        if words[index + 1].startswith("active"):
            value = billions(words[index])
            if value is not None:
                return value
    for raw in re.split(r"[/\-:]", model_id.lower()):
        if raw.startswith("a") and raw.endswith("b") and len(raw) > 2:
            value = billions(raw[1:])
            if value is not None:
                return value
    return None


def base_name(model_id):
    """Nom comparable d'un modèle, quel que soit le fournisseur : sans préfixe, éditeur, `:free`,
    ni `-instruct`/`-it`."""
    name = model_id.split(":", 1)[-1] if model_id.startswith(("nvidia:", "gemini:")) else model_id
    name = name.split("/")[-1].lower().replace(":free", "")
    name = re.sub(r"-(instruct|it|chat|preview|latest)$", "", name)
    return name.replace("_", "-")


def programming_rank(model_id, ranking):
    """Place du modèle (ou d'un modèle du même nom) dans le classement « programming »
    d'OpenRouter, ou None."""
    wanted = base_name(model_id)
    for index, ranked in enumerate(ranking):
        other = base_name(ranked)
        if wanted == other or (len(wanted) > 6 and (wanted in other or other in wanted)):
            return index
    return None


def score(model_id, description, ranking, context=0, probe=None):
    """Puissance estimée pour le code et ses raisons, en français."""
    description = (description or "").lower()
    lowered = model_id.lower()
    value = 0.0
    reasons = []
    position = programming_rank(model_id, ranking)
    if position is not None:
        value += max(0, 100 - position)
        reasons.append(f"n°{position + 1} en programmation chez OpenRouter")
    total = total_billions(description, model_id)
    active = active_billions(description, model_id) or total
    effective = math.sqrt(active * total) if active and total else (active or total)
    if effective:
        value += math.log2(min(max(effective, 1), 120)) * 12
    if total and active and abs(active - total) > 0.5:
        reasons.append(f"{total:g} Md de paramètres ({active:g} actifs)")
    elif total:
        reasons.append(f"{total:g} Md de paramètres")
    if any(key in lowered for key in ("coder", "-code", "codestral", "devstral")) \
            or any(phrase in description for phrase in CODING_SPECIALIZATION):
        value += 25
        reasons.append("spécialisé code")
    elif "coding" in description or "programming" in description:
        value += 10
    if any(needle in description for needle in CODE_BENCHMARKS):
        value += 10
    if LIGHTWEIGHT & set(re.split(r"[/\-:._]", lowered)):
        value -= 10
        reasons.append("version allégée")
    if context >= 200_000:
        value += 5
    if model_id in VERIFIED:
        value += 15
        reasons.append("vérifié par Claw")
    if probe:
        if probe.get("tools"):
            value += 20
            seconds = probe.get("seconds") or 0
            reasons.append(f"outil appelé en {seconds:.0f} s" if seconds >= 1 else "outil appelé en moins d'une seconde")
            if seconds > 30:
                value -= 15
            elif seconds > 10:
                value -= 5
        elif probe.get("ok"):
            value -= 40
            reasons.append("répond sans appeler l'outil")
        else:
            value -= 80
    return round(value, 1), reasons


def entry(model_id, short, description, ranking, context=0, tools=None, probe=None, status="ok"):
    value, reasons = score(model_id, description, ranking, context, probe)
    return {"id": model_id, "short": short, "score": value, "reasons": reasons, "context": context,
            "tools": tools, "status": status,
            "seconds": round(probe["seconds"], 1) if probe and probe.get("seconds") else None}


def ranked(entries):
    """Utilisables d'abord (outils), du plus fort au plus faible ; puis les autres."""
    usable = lambda item: item["tools"] is not False and item["status"] == "ok"  # noqa: E731
    return sorted(entries, key=lambda item: (not usable(item), -item["score"], item["id"]))


# --- Réseau ---------------------------------------------------------------------------------

def get_json(url, headers=None, timeout=30):
    request = urllib.request.Request(url, headers={"User-Agent": "claw-web", **(headers or {})})
    with urllib.request.urlopen(request, timeout=timeout) as response:
        return json.loads(response.read().decode("utf-8"))


def post_json(url, body, headers, timeout):
    data = json.dumps(body).encode("utf-8")
    request = urllib.request.Request(url, data=data, method="POST",
                                     headers={"Content-Type": "application/json", "User-Agent": "claw-web",
                                              **headers})
    with urllib.request.urlopen(request, timeout=timeout) as response:
        return json.loads(response.read().decode("utf-8"))


class RateLimit:
    """Au plus `per_minute` départs de requête par minute, entre tous les fils."""

    def __init__(self, per_minute):
        self.gap = 60.0 / per_minute
        self.lock = threading.Lock()
        self.next = 0.0

    def wait(self):
        with self.lock:
            now = time.monotonic()
            start = max(now, self.next)
            self.next = start + self.gap
        time.sleep(max(0.0, start - time.monotonic()))


def probe(url, key, model, limit, timeout=45):
    """Un appel avec un outil : {ok, tools, seconds, error}."""
    limit.wait()
    started = time.monotonic()
    try:
        answer = post_json(url, {"model": model, "messages": PROBE, "tools": [TOOL], "max_tokens": 200,
                                 "temperature": 0}, {"Authorization": f"Bearer {key}"}, timeout)
    except urllib.error.HTTPError as error:
        detail = error.read().decode("utf-8", "replace")[:300]
        return {"ok": False, "tools": False, "seconds": time.monotonic() - started,
                "error": f"{error.code} {detail}"}
    except (urllib.error.URLError, TimeoutError, OSError, ValueError) as error:
        return {"ok": False, "tools": False, "seconds": time.monotonic() - started, "error": str(error)[:200]}
    choice = ((answer or {}).get("choices") or [{}])[0]
    message = choice.get("message") or {}
    calls = message.get("tool_calls") or []
    return {"ok": True, "tools": bool(calls), "seconds": time.monotonic() - started, "error": ""}


def status_of(result, provider):
    if result["ok"]:
        return "ok"
    error = result["error"]
    if provider == "gemini" and ("limit: 0" in error or "free_tier" in error.lower()):
        return "hors de l'offre gratuite"
    if error.startswith("404"):
        return "non ouvert à cette clé"
    if error.startswith("429"):
        return "limite atteinte pendant l'essai"
    if "timed out" in error.lower():
        return "trop lent (pas de réponse en 45 s)"
    return "en échec : " + error.split("\n")[0][:80]


def openrouter_catalog():
    catalog = get_json(f"{OPENROUTER}/models").get("data") or []
    try:
        programming = [model["id"] for model in get_json(f"{OPENROUTER}/models?category=programming").get("data") or []]
    except (urllib.error.URLError, OSError, ValueError):
        programming = []
    entries = []
    for model in catalog:
        pricing = model.get("pricing") or {}
        try:
            free = float(pricing.get("prompt", 1)) == 0 and float(pricing.get("completion", 1)) == 0
        except (TypeError, ValueError):
            free = False
        if not free or model["id"].startswith("openrouter/"):
            continue
        tools = "tools" in (model.get("supported_parameters") or [])
        short = re.sub(r"\s*\(free\)\s*$", "", model.get("name") or model["id"], flags=re.I)
        short = re.sub(r"^[^:]+:\s*", "", short)
        entries.append(entry(model["id"], short, model.get("description") or "", programming,
                             model.get("context_length") or 0, tools,
                             status="ok" if tools else "n'appelle pas d'outils"))
    return ranked(entries), programming


def nvidia_catalog(programming):
    models = [model["id"] for model in get_json(f"{NVIDIA}/models").get("data") or []]
    key = os.environ.get("NVIDIA_API_KEY", "").strip()
    chat = [model for model in models if not NOT_CHAT.search(model)]
    results = {}
    if key:
        limit = RateLimit(30)
        with ThreadPoolExecutor(max_workers=6) as pool:
            for model, result in zip(chat, pool.map(
                    lambda name: probe(f"{NVIDIA}/chat/completions", key, name, limit), chat)):
                results[model] = result
    entries = []
    for model in models:
        model_id = f"nvidia:{model}"
        short = "NVIDIA " + model.split("/")[-1]
        if model not in chat:
            entries.append(entry(model_id, short, "", programming, tools=False, status="pas un modèle de dialogue"))
            continue
        result = results.get(model)
        if result is None:
            entries.append(entry(model_id, short, "", programming,
                                 status="non essayé (pas de clé NVIDIA dans le moteur)"))
            continue
        entries.append(entry(model_id, short, "", programming, tools=result["tools"] if result["ok"] else False,
                             probe=result, status=status_of(result, "nvidia")))
    return ranked(entries)


def gemini_catalog(programming):
    key = os.environ.get("GEMINI_API_KEY", "").strip()
    if not key:
        return []
    listed = get_json(f"{GEMINI}/models?pageSize=200", {"x-goog-api-key": key}).get("models") or []
    chat = []
    for model in listed:
        name = (model.get("name") or "").removeprefix("models/")
        if "generateContent" not in (model.get("supportedGenerationMethods") or []) or NOT_CHAT.search(name) \
                or re.search(r"image|tts|live|native-audio|computer-use|robotics|learnlm", name):
            continue
        chat.append((name, model))
    limit = RateLimit(10)
    with ThreadPoolExecutor(max_workers=3) as pool:
        results = list(pool.map(lambda item: probe(f"{GEMINI}/openai/chat/completions", key, item[0], limit), chat))
    entries = []
    for (name, model), result in zip(chat, results):
        entries.append(entry(f"gemini:{name}", model.get("displayName") or name, model.get("description") or "",
                             programming, model.get("inputTokenLimit") or 0,
                             tools=result["tools"] if result["ok"] else False, probe=result,
                             status=status_of(result, "gemini")))
    return ranked(entries)


# --- Publication ----------------------------------------------------------------------------

def store(engine, token, provider, data):
    """Écrit catalogue/<provider>.json sur la branche des fichiers (créée au besoin)."""
    path = f"catalogue/{provider}.json"
    try:
        api("GET", f"/repos/{engine}/branches/{FILES_BRANCH}", token)
    except GitHubError as error:
        if error.status != 404:
            raise
        default = api("GET", f"/repos/{engine}", token)["default_branch"]
        head = api("GET", f"/repos/{engine}/git/ref/heads/{default}", token)
        api("POST", f"/repos/{engine}/git/refs", token, {"ref": f"refs/heads/{FILES_BRANCH}",
                                                         "sha": head["object"]["sha"]})
    sha = None
    try:
        sha = api("GET", f"/repos/{engine}/contents/{path}?ref={FILES_BRANCH}", token).get("sha")
    except GitHubError as error:
        if error.status != 404:
            raise
    body = {"message": f"Catalogue des modèles gratuits ({provider})", "branch": FILES_BRANCH,
            "content": base64.b64encode(json.dumps(data, ensure_ascii=False, indent=1).encode("utf-8")).decode()}
    if sha:
        body["sha"] = sha
    api("PUT", f"/repos/{engine}/contents/{path}", token, body)


def main():
    wanted = (os.environ.get("PROVIDERS") or "all").strip().lower() or "all"
    providers = ["openrouter", "nvidia", "gemini"] if wanted == "all" else [wanted]
    engine = os.environ["GITHUB_REPOSITORY"]
    token = os.environ["JOB_TOKEN"]
    openrouter, programming = openrouter_catalog()
    builders = {"openrouter": lambda: openrouter, "nvidia": lambda: nvidia_catalog(programming),
                "gemini": lambda: gemini_catalog(programming)}
    failed = False
    for provider in providers:
        if provider not in builders:
            print(f"Fournisseur inconnu : {provider}", file=sys.stderr)
            failed = True
            continue
        started = time.time()
        try:
            models = builders[provider]()
            error = ""
        except (urllib.error.URLError, OSError, ValueError, KeyError) as problem:
            models, error = [], str(problem)[:300]
            failed = True
        data = {"provider": provider, "at": int(started), "seconds": round(time.time() - started),
                "error": error, "models": models}
        store(engine, token, provider, data)
        usable = sum(1 for model in models if model["tools"] is not False and model["status"] == "ok")
        print(f"{provider} : {len(models)} modèles, {usable} utilisables par Claw"
              + (f" ; erreur : {error}" if error else ""), flush=True)
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
