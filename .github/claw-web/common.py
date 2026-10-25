"""Outils communs du moteur de l'appli web de Claw (workflow claw-web.yml).

Les sessions sont des issues du dépôt du moteur (étiquette `claw-web`) : le corps porte la
tâche et ses réglages, chaque message de suivi est un commentaire, et le moteur y publie sa
progression puis sa réponse, marquées pour que l'appli et la session suivante les
reconnaissent.
"""

import json
import os
import re
import signal
import time
import urllib.error
import urllib.request
from pathlib import Path

API = "https://api.github.com"
ANSWER = "<!-- claw-web:answer -->"
# Début des remarques du moteur sous une réponse (publication, fusion, mise en ligne).
NOTES = "<!-- claw-web:notes -->"
PROGRESS = "<!-- claw-web:progress -->"
LOG = "<!-- claw-web:log"
# Dans une réponse : le dernier message traité (identifiant de commentaire), ou « keep »
# (réponse qui ne traite rien : échec au démarrage).
ANSWERED = re.compile(r"<!--\s*claw-web:answered\s+(\d+|keep)\s*-->")
# Questions de Claw (outil AskUserQuestion) à la fin de sa réponse : en données pour l'appli,
# qui en fait des choix à toucher, puis en texte, jusqu'à QUESTIONS_END, pour GitHub et pour
# l'historique de l'agent.
QUESTIONS = re.compile(r"<!--\s*claw-web:questions\s+(\[.*?\])\s*-->\n?", re.S)
QUESTIONS_END = "<!-- claw-web:questions-end -->"


def questions_block(questions):
    """Les questions de Claw et leurs réponses proposées, pour la fin de sa réponse ; « »
    sans question valable (une question a au moins deux réponses proposées)."""
    kept, lines = [], []
    for question in questions if isinstance(questions, list) else []:
        if not isinstance(question, dict) or not str(question.get("question") or "").strip():
            continue
        options = [{"label": str(option["label"]).strip(), "description": str(option.get("description") or "").strip()}
                   for option in question.get("options") or []
                   if isinstance(option, dict) and str(option.get("label") or "").strip()]
        if len(options) < 2:
            continue
        kept.append({"question": str(question["question"]).strip(), "header": str(question.get("header") or "").strip(),
                     "options": options, "multi_select": bool(question.get("multi_select"))})
        lines.append(f"**{kept[-1]['question']}**")
        lines.extend(f"- {option['label']}" + (f" : {option['description']}" if option["description"] else "")
                     for option in options)
        lines.append("")
    if not kept:
        return ""
    # Un « --> » dans un texte fermerait le commentaire HTML avant la fin des données.
    data = json.dumps(kept, ensure_ascii=False).replace("-->", "--\\u003e")
    return f"<!-- claw-web:questions {data} -->\n" + "\n".join(lines).strip() + f"\n{QUESTIONS_END}"

# Requêtes par jour de l'offre gratuite de Google, relevées le 18 octobre 2026 (erreur 429 de
# Google) ; les autres modèles Gemini ont aussi un quota par jour, non relevé.
GEMINI_DAILY = {"gemini-3.8-flash": 20, "gemini-3.5-flash": 20}


def provider_of(model):
    """Fournisseur d'un modèle : « gemini », « nvidia », ou « openrouter » (« Auto » compris)."""
    prefix, _, rest = (model or "").partition(":")
    return prefix if rest and prefix in ("gemini", "nvidia") else "openrouter"


def usage_lines(model, requests, quota):
    """Pied d'une réponse : les requêtes du passage selon le fournisseur du modèle choisi. Pour
    Gemini et NVIDIA, leurs propres limites (le compte d'OpenRouter n'en dit rien) ; le compte
    d'OpenRouter quand c'est lui, ou quand le passage en a pris (modèle de secours)."""
    lines = []
    provider = provider_of(model)
    name = (model or "").partition(":")[2]
    count = f"{requests} requêtes dans ce passage · " if isinstance(requests, int) else ""
    if provider == "gemini":
        limit = GEMINI_DAILY.get(name)
        daily = f"{limit} requêtes par jour pour ce modèle" if limit else "quota par jour et par modèle"
        lines.append(f"Google Gemini ({name}) : {count}offre gratuite, {daily} "
                     "(consommation sur aistudio.google.com/rate-limit)")
    elif provider == "nvidia":
        lines.append(f"NVIDIA ({name}) : {count}sans quota par jour, environ 40 requêtes par minute")
    if quota and (provider == "openrouter" or quota.get("run")):
        spent = f"{quota['run']} pour ce passage · " if quota.get("run") is not None else ""
        lines.append(f"Requêtes gratuites : {spent}{quota['used']}/{quota['limit']} utilisées aujourd'hui "
                     f"({quota['remaining']} restantes)")
    return lines


def french_number(value):
    """1743 -> « 1 743 » (espace fine insécable, comme en français)."""
    return f"{value:,}".replace(",", " ")


SHORTSTAT = re.compile(r"(\d+) files? changed(?:, (\d+) insertions?\(\+\))?(?:, (\d+) deletions?\(-\))?")


def french_shortstat(text):
    """Le bilan de `git diff --stat` en français : « 10 fichiers modifiés, +1 743 lignes,
    −287 lignes ». Un texte inconnu est rendu tel quel."""
    found = SHORTSTAT.search(text or "")
    if not found:
        return (text or "").strip()
    files, added, removed = (int(group or 0) for group in found.groups())
    parts = [f"{french_number(files)} fichier{'s' if files > 1 else ''} modifié{'s' if files > 1 else ''}"]
    if added:
        parts.append(f"+{french_number(added)} ligne{'s' if added > 1 else ''}")
    if removed:
        parts.append(f"−{french_number(removed)} ligne{'s' if removed > 1 else ''}")
    return ", ".join(parts)


# Lignes de signature qu'un modèle ajoute par habitude de commit (vu en session :
# « Assisted-by: Claw Code ») : elles ne disent rien à l'utilisateur.
SIGNATURE = re.compile(r"^\s*(assisted-by|co-authored-by|generated-by|generated with)\b.*$", re.I)


def clean_answer(text):
    """La réponse de Claw sans ses lignes de signature finales."""
    lines = (text or "").rstrip().splitlines()
    while lines and (SIGNATURE.match(lines[-1]) or not lines[-1].strip()):
        lines.pop()
    return "\n".join(lines)


SETTING_HINT = re.compile(r"\s*\((?:réglage|setting)\s+`[^`]*`\)")


def clean_notice(notice):
    """Un avis de Claw pour l'utilisateur : sur une ligne, sans le nom du réglage technique."""
    return SETTING_HINT.sub("", " ".join(str(notice).split()))


# Erreurs connues, dites en clair ; le message brut reste en dessous, pour qui veut le détail.
ERRORS = (
    (re.compile(r"aucun contenu|empty response|no content", re.I),
     "le modèle n'a rien répondu, plusieurs fois de suite"),
    (re.compile(r"\b429\b|rate.?limit|too many requests|quota", re.I),
     "le quota du modèle est épuisé pour le moment"),
    (re.compile(r"\b40[13]\b|unauthori[sz]ed|invalid api key|clé.*(invalide|absente)", re.I),
     "la clé du fournisseur du modèle est refusée ou absente"),
    (re.compile(r"timed? ?out|délai dépassé|deadline", re.I),
     "le modèle a mis trop de temps à répondre"),
    (re.compile(r"\b50[0234]\b|overloaded|surcharg|service unavailable|bad gateway", re.I),
     "le service du modèle est en panne ou surchargé"),
    (re.compile(r"context|too long|trop long", re.I),
     "la conversation est devenue trop longue pour le modèle"),
)


def explain_error(error):
    """L'erreur d'un passage en clair (`None` si elle est inconnue)."""
    for pattern, meaning in ERRORS:
        if pattern.search(error or ""):
            return meaning
    return None


def kind_of(body):
    """« engine » (progression, journal), « answer » ou « message » (de l'utilisateur)."""
    body = (body or "").strip()
    # JTest (jtest.py) : l'état du site lancé pour le téléphone, pas un message.
    if PROGRESS in body or body.startswith(LOG) or body.startswith("<!-- claw-web:jtest"):
        return "engine"
    return "answer" if ANSWER in body else "message"


def answer_marks(notes):
    """Chaque réponse, dans l'ordre, avec l'identifiant du dernier message qu'elle a traité (0 :
    la tâche seule ; None : « keep », la tâche n'a pas pu commencer et ne traite rien). Une
    réponse sans marque (plus ancienne) a traité les messages qui la précèdent."""
    marks, last_message = [], 0
    for note in notes:
        body = note.get("body") or ""
        kind = kind_of(body)
        if kind == "message":
            last_message = note["id"]
        elif kind == "answer":
            mark = ANSWERED.search(body)
            if mark is None:
                marks.append((note, last_message))
            else:
                marks.append((note, None if mark.group(1) == "keep" else int(mark.group(1))))
    return marks


def answered_up_to(notes):
    """Identifiant du dernier message traité par une réponse."""
    return max((mark for _, mark in answer_marks(notes) if mark is not None), default=0)
SETTINGS = re.compile(r"<!--\s*claw-web\s+(\{.*?\})\s*-->", re.S)
# Images jointes à un message par l'appli : chemins dans la branche FILES_BRANCH du moteur.
IMAGES = re.compile(r"<!--\s*claw-web:images\s+(\[.*?\])\s*-->", re.S)
FILES_BRANCH = "claw-web-fichiers"
IMAGE_PATH = re.compile(r"uploads/[A-Za-z0-9._-]+/[A-Za-z0-9._-]+\.(?:png|jpe?g|gif|webp)")

# Fichiers échangés entre les étapes du job.
WORK = Path(os.environ.get("RUNNER_TEMP", ".")) / "claw-web"
# Jeton du dépôt pour les serveurs MCP du moteur (mcp_server.py) : posé par setup.py, lu puis
# effacé par run.py avant que l'agent ne démarre, jamais dans l'environnement d'une étape.
MCP_TOKEN_FILE = WORK / ".mcp-token"


class GitHubError(Exception):
    """Réponse d'erreur de l'API : `status` est le code HTTP, `reason` le message de GitHub,
    `accepted` les autorisations de jeton que GitHub demande (en-tête
    X-Accepted-GitHub-Permissions, par exemple « pages=write,administration=write »)."""

    def __init__(self, message, status=0, reason="", accepted=""):
        super().__init__(message)
        self.status = status
        self.reason = reason
        self.accepted = accepted


def api(method, path, token, body=None):
    """Appel de l'API GitHub ; renvoie le JSON décodé (ou None)."""
    data = None if body is None else json.dumps(body).encode("utf-8")
    request = urllib.request.Request(API + path, method=method, data=data)
    request.add_header("Authorization", f"Bearer {token}")
    request.add_header("Accept", "application/vnd.github+json")
    request.add_header("X-GitHub-Api-Version", "2022-11-28")
    request.add_header("User-Agent", "claw-web")
    if data is not None:
        request.add_header("Content-Type", "application/json")
    try:
        with urllib.request.urlopen(request, timeout=30) as response:
            raw = response.read()
    except urllib.error.HTTPError as error:
        detail = error.read().decode("utf-8", "replace")[:300]
        try:
            reason = str(json.loads(detail).get("message") or "")
        except (ValueError, AttributeError):
            reason = ""
        accepted = (error.headers.get("X-Accepted-GitHub-Permissions") or "") if error.headers else ""
        raise GitHubError(f"{method} {path} : HTTP {error.code} {detail}", error.code, reason or detail,
                          accepted.strip()) from None
    return json.loads(raw) if raw else None


def comments(token, repo, issue):
    """Tous les commentaires d'une issue, dans l'ordre."""
    found, page = [], 1
    while True:
        batch = api("GET", f"/repos/{repo}/issues/{issue}/comments?per_page=100&page={page}", token)
        found.extend(batch)
        if len(batch) < 100:
            return found
        page += 1


# Variables du job qui portent un secret : clés des fournisseurs, jetons, mots de passe.
SECRET_NAME = re.compile(r"(API_KEY|TOKEN|SECRET|PASSWORD)$", re.I)


def secret_values(environ=None):
    """Les valeurs secrètes du job, les plus longues d'abord (assez longues pour n'être que ça)."""
    environ = os.environ if environ is None else environ
    found = {value.strip() for name, value in environ.items() if SECRET_NAME.search(name) and len(value.strip()) >= 12}
    return sorted(found, key=len, reverse=True)


def hide_secrets(text, environ=None):
    """`text` sans aucune valeur secrète du job : ce que l'agent a lu ou affiché (une commande
    `env`, un fichier de configuration) part dans la session sans elles. GitHub ne masque les
    secrets que dans les journaux des jobs, pas dans les commentaires."""
    for value in secret_values(environ):
        text = text.replace(value, "***")
    return text


def post_comment(token, repo, issue, body):
    return api("POST", f"/repos/{repo}/issues/{issue}/comments", token, {"body": hide_secrets(body)})


def edit_comment(token, repo, comment_id, body):
    return api("PATCH", f"/repos/{repo}/issues/comments/{comment_id}", token, {"body": hide_secrets(body)})


def settings(body):
    """Réglages de la session écrits par l'appli dans le corps de l'issue."""
    found = SETTINGS.search(body or "")
    if not found:
        return {}
    try:
        value = json.loads(found.group(1))
    except json.JSONDecodeError:
        return {}
    return value if isinstance(value, dict) else {}


def images_of(text):
    """Le texte sans ses blocs d'images (remplacés par « (n image(s) jointe(s)) ») et les
    chemins des images, vérifiés."""
    found = []

    def replace(match):
        try:
            paths = json.loads(match.group(1))
        except json.JSONDecodeError:
            return ""
        paths = [path for path in paths if isinstance(path, str) and IMAGE_PATH.fullmatch(path)]
        found.extend(paths)
        return f"({len(paths)} image(s) jointe(s))" if paths else ""

    return IMAGES.sub(replace, text or "").strip(), found


# Niveau de réflexion choisi dans l'appli pour un message (bouton « Réflexion » du bloc de
# message), noté dans le message lui-même : le workflow des sessions n'a pas d'entrée de plus.
EFFORT = re.compile(r"\n*<!--\s*claw-web:effort\s+([a-z]+)\s*-->")
EFFORTS = ("off", "low", "medium", "high", "max")


def effort_of(text):
    """Le texte sans ses marques de niveau de réflexion, et le dernier niveau choisi (vide :
    celui de Claw)."""
    found = [value for value in EFFORT.findall(text or "") if value in EFFORTS]
    return EFFORT.sub("", text or "").strip(), (found[-1] if found else "")


# Commandes propres à Claw Web (menu « / » de l'appli) : ce que la commande demande, en mots.
# Les commandes de Claw (/review, /fix, /verify...) passent telles quelles : Claw les déplie.
WEB_COMMANDS = {
    "jtest": "Lance JTest pour que j'essaie le projet sur mon téléphone, tel qu'il est sur la branche, "
             "et donne-moi le lien.",
    "pages": "Mets le site en ligne sur GitHub Pages (fusionne d'abord la branche si ses fichiers n'y sont "
             "que là) et donne-moi son adresse.",
    "merge": "Fusionne la pull request de cette session.",
    "continue": "continue",
}


def web_command(text):
    """La demande d'une commande de Claw Web tapée seule (« /jtest », « /pages en ligne »), sinon
    None. Ce qui suit la commande est ajouté à la demande."""
    found = re.match(r"^/([a-z-]+)\b[ \t]*(.*)$", (text or "").strip(), re.S)
    if not found or found.group(1) not in WEB_COMMANDS:
        return None
    extra = found.group(2).strip()
    return WEB_COMMANDS[found.group(1)] + (f"\n\n{extra}" if extra else "")


def task_text(body):
    """La tâche, sans le bloc de réglages."""
    return SETTINGS.sub("", body or "").strip()


def output(name, value):
    """Sortie d'étape (texte sur plusieurs lignes accepté)."""
    path = os.environ.get("GITHUB_OUTPUT")
    if not path:
        print(f"{name}={value}")
        return
    delimiter = f"CLAW_WEB_{os.urandom(8).hex()}"
    with open(path, "a", encoding="utf-8") as handle:
        handle.write(f"{name}<<{delimiter}\n{value}\n{delimiter}\n")


def process_tree(root):
    """Le processus et tous ses descendants (Linux : /proc), lus avant de l'arrêter : mort,
    ses enfants changeraient de parent."""
    children = {}
    if os.path.isdir("/proc"):
        for entry in os.listdir("/proc"):
            if not entry.isdigit():
                continue
            try:
                with open(f"/proc/{entry}/stat", encoding="utf-8", errors="replace") as handle:
                    parent = int(handle.read().rsplit(")", 1)[1].split()[1])
            except (OSError, ValueError, IndexError):
                continue
            children.setdefault(parent, []).append(int(entry))
    tree, todo = [], [root]
    while todo:
        pid = todo.pop()
        tree.append(pid)
        todo.extend(children.get(pid, []))
    return tree


def stop_tree(root, alive=lambda: True, grace=5):
    """Arrête l'agent et les commandes qu'il a lancées (serveur, tests...) : rien ne doit plus
    écrire dans le dépôt pendant la publication. SIGTERM, puis SIGKILL après `grace` s."""
    tree = process_tree(root)

    def send(sig):
        for pid in tree:
            try:
                os.kill(pid, sig)
            except (ProcessLookupError, PermissionError):
                pass

    send(signal.SIGTERM)
    deadline = time.time() + grace
    while time.time() < deadline and alive():
        time.sleep(0.1)
    send(signal.SIGKILL)


def run_url():
    server = os.environ.get("GITHUB_SERVER_URL", "https://github.com")
    return f"{server}/{os.environ.get('GITHUB_REPOSITORY', '')}/actions/runs/{os.environ.get('GITHUB_RUN_ID', '')}"
