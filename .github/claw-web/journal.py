"""Journal détaillé d'un passage, pour l'appli : messages de l'agent, réflexions, appels
d'outils et leurs résultats, en bref (sorties et différences tronquées), comme les montre
l'interface locale de Claw.

Il est publié dans des commentaires de la session (une partie par commentaire de 60 000
caractères au plus), compressé (gzip puis base64) dans un commentaire HTML : invisible sur
GitHub, lu et affiché par l'appli. Les chemins sont montrés depuis la racine du dépôt.
"""

import base64
import collections
import gzip
import hashlib
import json
import time
import urllib.parse
import urllib.request

from common import hide_secrets
from mcp_server import describe_call

LOG = "<!-- claw-web:log"
DATA = "<!-- claw-web:data"
# Taille d'une partie (base64), sous la limite de 65 536 caractères d'un commentaire.
PAGE_CHARS = 60_000
DIFF_LINES = 60
# Photos trouvées par l'agent (outil Photo) : leur petite version (carré de Flickr, miniature
# de Commons) est téléchargée ici et glissée dans le journal, la grande version reste chez sa
# source, la seule image extérieure que l'appli autorise (Content-Security-Policy).
MAX_PHOTOS = 4
# Captures d'un appel de Screenshot : la page entière en tranches, ses fenêtres, son menu et
# ses onglets.
MAX_SHOTS = 10
# Problèmes gardés par capture, et leur longueur.
MAX_PROBLEMS = 12
PROBLEM_CHARS = 400
THUMB_BYTES = 20_000
# Vignettes d'une même recherche, encodées : une partie du journal tient en 60 000 caractères.
THUMBS_BUDGET = 40_000
THUMB_TIMEOUT = 5
IMAGE_KINDS = ((b"\xff\xd8\xff", "image/jpeg"), (b"\x89PNG\r\n\x1a\n", "image/png"))


def excerpt(text, size, tail=False):
    """Début du texte (ou sa fin, `tail`), avec « … » s'il est coupé."""
    text = str(text or "")
    if len(text) <= size:
        return text
    return "…" + text[-size:] if tail else text[:size] + "…"


def without_claw_notes(output):
    """Sortie d'une commande sans les remarques que Claw y joint pour le modèle."""
    if output.startswith("[Claw] "):
        return ""
    at = output.find("\n[Claw] ")
    return output if at < 0 else output[:at].rstrip()


def photo_link(url):
    """Une adresse https chez une source de photos connue (Flickr, Wikimedia), ou « »."""
    try:
        parsed = urllib.parse.urlparse(str(url or ""))
    except ValueError:
        return ""
    host = parsed.hostname or ""
    known = host == "upload.wikimedia.org" or host.endswith(".staticflickr.com")
    return parsed.geturl() if parsed.scheme == "https" and known else ""


def image_kind(data):
    """Le type d'une image d'après ses premiers octets (JPEG, PNG, WebP), ou « »."""
    for magic, kind in IMAGE_KINDS:
        if data.startswith(magic):
            return kind
    return "image/webp" if data[:4] == b"RIFF" and data[8:12] == b"WEBP" else ""


def fetch_small(url):
    """La petite version d'une photo, en adresse `data:` ; « » si elle ne vient pas vite, est
    trop lourde ou n'est pas une image."""
    request = urllib.request.Request(url, headers={"User-Agent": "claw-web/1.0 (vignettes de Claw Web)"})
    try:
        with urllib.request.urlopen(request, timeout=THUMB_TIMEOUT) as response:
            data = response.read(THUMB_BYTES + 1)
    except (OSError, ValueError):
        return ""
    kind = image_kind(data)
    if not kind or len(data) > THUMB_BYTES:
        return ""
    return f"data:{kind};base64,{base64.b64encode(data).decode('ascii')}"


def local_thumb(path):
    """Un petit fichier image de la machine (aperçu d'une capture) en adresse `data:`, ou « »."""
    try:
        with open(str(path or ""), "rb") as file:
            data = file.read(THUMB_BYTES + 1)
    except OSError:
        return ""
    kind = image_kind(data)
    if not kind or len(data) > THUMB_BYTES:
        return ""
    return f"data:{kind};base64,{base64.b64encode(data).decode('ascii')}"


def shots(value, store=None, translate=None, changed=lambda: None):
    """Les captures de l'outil Screenshot : l'aperçu du premier écran glissé dans le journal,
    la capture entière rangée par `store` (chemin local -> chemin dans la branche des fichiers),
    l'écran et les problèmes relevés, traduits en français par `translate` quand il y en a un
    (la traduction arrive après coup : `changed` le signale au journal)."""
    found = []
    budget = THUMBS_BUDGET
    for capture in (value.get("captures") or [])[:MAX_SHOTS]:
        if not isinstance(capture, dict) or not capture.get("file"):
            continue
        thumb = local_thumb(capture.get("preview"))
        if len(thumb) > budget:
            thumb = ""
        budget -= len(thumb)
        stored = store(capture["file"], str(capture.get("screen") or "")) if store else ""
        if not thumb and not stored:
            continue
        problems = capture.get("problems") if isinstance(capture.get("problems"), list) else []
        found.append({
            "t": thumb,
            "p": stored,
            "screen": excerpt(capture.get("screen", ""), 120),
            "n": len(problems),
            # Le texte des problèmes, que l'appli montre quand on touche la capture.
            "pb": [excerpt(str(problem), PROBLEM_CHARS) for problem in problems[:MAX_PROBLEMS]],
        })
        if translate and found[-1]["pb"]:
            def done(french, shot=found[-1]):
                shot["pf"] = french
                changed()
            translate(found[-1]["pb"], done)
    return found


def photos(value, fetch=fetch_small):
    """Les photos trouvées par l'outil Photo, pour des vignettes : petite version glissée dans
    le journal, grande version chez sa source, titre, auteur, licence et page d'origine."""
    found = []
    budget = THUMBS_BUDGET
    for photo in (value.get("photos") or [])[:MAX_PHOTOS]:
        if not isinstance(photo, dict):
            continue
        small = photo_link(photo.get("web_thumbnail"))
        thumb = fetch(small) if small else ""
        if not thumb or len(thumb) > budget:
            continue
        budget -= len(thumb)
        source = str(photo.get("source") or "")
        found.append({
            "t": thumb,
            "u": photo_link(photo.get("web_original")),
            "title": excerpt(photo.get("title", ""), 90),
            "by": excerpt(photo.get("creator", ""), 60),
            "lic": excerpt(photo.get("license", ""), 30),
            "src": source if source.startswith("https://") else "",
            "path": excerpt(photo.get("path", ""), 120),
        })
    return found


def patch(hunks):
    """Les lignes d'une différence (`structuredPatch` de Claw), tronquées, et leurs comptes."""
    lines = []
    for hunk in hunks or []:
        if not isinstance(hunk, dict):
            continue
        lines.append(f"@@ {hunk.get('oldStart', 0)} {hunk.get('newStart', 0)}")
        lines.extend(str(line) for line in hunk.get("lines") or [])
    return {
        "lines": [excerpt(line, 200) for line in lines[:DIFF_LINES]],
        "more": max(0, len(lines) - DIFF_LINES),
        "add": sum(1 for line in lines if line.startswith("+")),
        "del": sum(1 for line in lines if line.startswith("-")),
    }


class Journal:
    """Les événements d'un passage, en bref, répartis en parties publiables."""

    def __init__(self, relative=lambda text: str(text), fetch=fetch_small, store=None, translate=None):
        self.relative = relative
        self.fetch = fetch
        # Traduit les problèmes des captures en français (translate.Translator), s'il y en a un.
        self.translate = translate
        # Range une capture entière (chemin local, écran) ; « » quand elle n'a pas pu l'être.
        self.store = store
        self.events = []
        self.waiting = collections.deque()
        # Indice du premier événement de chaque partie : une partie pleine reste fermée.
        self.bounds = [0]
        self.sent = {}
        self.dirty = False
        # Réponse du modèle en cours (`message.id` de Claw) et son texte (indice dans events).
        self.reply = None
        self.reply_text = None
        # Textes repliés par le dernier contrôle de Claw, et ceux qu'il a fait garder ensuite.
        self.folded = []
        self.kept = []

    # --- Lecture des événements de Claw (`--output-format stream-json`) ----------------

    def add(self, event):
        kind = event.get("type")
        message = event.get("message") if isinstance(event.get("message"), dict) else {}
        if kind == "system" and event.get("subtype") == "init":
            # `drafts` : ce journal marque lui-même les réponses reprises (l'appli n'a rien
            # à deviner).
            self._push({"k": "init", "model": event.get("model", ""), "drafts": True})
            self.reply = self.reply_text = None
            self.folded, self.kept = [], []
        elif kind == "system" and event.get("subtype") == "notice":
            self._push({"k": "notice", "t": " ".join(str(event.get("text", "")).split())})
        elif kind == "system" and event.get("subtype") == "answer_superseded":
            # Un contrôle de Claw renvoie le modèle au travail : sa réponse (avec celle qu'un
            # contrôle précédent avait fait garder) devient un brouillon, replié par l'appli.
            self.folded = [index for index in [*self.kept, self.reply_text] if index is not None]
            self.kept = []
            for index in self.folded:
                self.events[index]["k"] = "draft"
            self.dirty = True
        elif kind == "system" and event.get("subtype") == "answer_kept":
            # Ce qui a suivi le contrôle n'en était qu'un accusé de réception : la réponse
            # repliée redevient la réponse.
            for index in self.folded:
                self.events[index]["k"] = "text"
            self.kept, self.folded = self.folded, []
            self.dirty = True
        elif kind == "assistant":
            # Une réponse du modèle, une requête : son heure, pour le compteur par minute de
            # l'appli (NVIDIA : environ 40 requêtes par minute). L'appli ne l'affiche pas.
            # Claw publie chaque bloc d'une réponse à part, sous le numéro de la réponse.
            reply = message.get("id")
            if reply is None or reply != self.reply:
                self._push({"k": "req", "at": int(time.time())})
                self.reply_text = None
            self.reply = reply
            for part in message.get("content") or []:
                what = part.get("type")
                if what == "thinking" and str(part.get("thinking", "")).strip():
                    self._push({"k": "think", "t": excerpt(part["thinking"].strip(), 600)})
                elif what == "text" and str(part.get("text", "")).strip():
                    self._push({"k": "text", "t": excerpt(part["text"].strip(), 8000)})
                    self.reply_text = len(self.events) - 1
                elif what == "tool_use":
                    name = part.get("name", "")
                    entry = {"k": "tool", "n": name, "i": self._input(name, part.get("input"))}
                    self._push(entry)
                    self.waiting.append(entry)
                    # Une réponse qui appelle des outils n'est pas la réponse du tour : son
                    # texte annonce le travail, aucun contrôle ne le reprend.
                    self.reply_text = None
        elif kind == "user":
            for part in message.get("content") or []:
                if part.get("type") != "tool_result" or not self.waiting:
                    continue
                entry = self._waiting_for(part.get("tool_name"))
                entry["r"] = self._result(entry["n"], part.get("content"))
                if part.get("is_error"):
                    entry["e"] = True
                self.dirty = True
        elif kind == "result":
            self._push({
                "k": "end",
                "model": event.get("model", ""),
                "turns": event.get("num_turns"),
                "ms": event.get("duration_ms"),
                "cost": event.get("cost", ""),
            })

    def _waiting_for(self, name):
        """L'appel qu'un résultat termine : le plus ancien en attente du même outil. Les
        résultats arrivent dans l'ordre des appels, mais un appel annoncé peut ne jamais en
        recevoir : le premier en attente, pris d'office, décalait alors toute la suite (vu le
        25 octobre 2026 : une capture montrait le contenu d'un fichier CSS, et ses images
        allaient trois actions plus bas). Sans nom d'outil, ou sans appel de ce nom, le
        premier en attente."""
        for entry in self.waiting:
            if name and entry["n"] == name:
                self.waiting.remove(entry)
                return entry
        return self.waiting.popleft()

    def _changed(self):
        """Une traduction arrivée après coup : la partie du journal est à republier."""
        self.dirty = True

    def _push(self, entry):
        self.events.append(entry)
        self.dirty = True

    def _input(self, name, data):
        data = data if isinstance(data, dict) else {}
        rel = self.relative
        if name in ("bash", "PowerShell"):
            return {"cmd": excerpt(rel(data.get("command", "")), 1500),
                    "desc": excerpt(data.get("description", ""), 200)}
        if name in ("read_file", "write_file", "edit_file", "NotebookEdit", "MultiEdit", "delete_file",
                    "list_directory"):
            return {"path": rel(data.get("path") or data.get("file_path") or data.get("notebook_path") or "")}
        if name == "move_file":
            return {"from": rel(data.get("source") or ""), "to": rel(data.get("destination") or "")}
        if name == "Wireframe":
            return {"title": excerpt(str(data.get("title") or ""), 120)}
        if name == "read_many_files":
            return {"paths": [rel(str(path)) for path in (data.get("paths") or [])[:20]]}
        if name == "file_outline":
            return {"path": rel(data.get("path") or "")}
        if name == "replace_in_files":
            return {"pattern": excerpt(str(data.get("pattern") or ""), 200),
                    "replacement": excerpt(str(data.get("replacement") or ""), 200),
                    "glob": excerpt(str(data.get("glob") or ""), 100), "preview": bool(data.get("preview"))}
        if name == "run_tests":
            return {"cmd": excerpt(rel(str(data.get("command") or data.get("filter") or "")), 300)}
        if name == "git":
            return {"action": str(data.get("action") or ""),
                    "target": rel(str(data.get("path") or data.get("ref") or ""))}
        if name == "git_commit":
            return {"message": excerpt(str(data.get("message") or ""), 300)}
        if name == "http_request":
            return {"target": excerpt(f"{data.get('method') or 'GET'} {data.get('url') or ''}", 300)}
        if name == "apply_patch":
            text = str(data.get("patch") or data.get("input") or "")
            paths = []
            for line in text.splitlines():
                for marker in ("*** Add File:", "*** Update File:", "*** Delete File:", "*** Move to:"):
                    if line.startswith(marker) and line[len(marker):].strip() not in paths:
                        paths.append(line[len(marker):].strip())
            return {"paths": [rel(path) for path in paths[:20]]}
        if name in ("grep_search", "glob_search"):
            return {"pattern": excerpt(data.get("pattern", ""), 300), "path": rel(data.get("path") or "")}
        if name == "TodoWrite":
            todos = [todo for todo in data.get("todos") or [] if isinstance(todo, dict)]
            return {"todos": [[todo.get("status", ""), excerpt(
                todo.get("activeForm") if todo.get("status") == "in_progress" and todo.get("activeForm")
                else todo.get("content", ""), 200)] for todo in todos[:40]]}
        if name in ("WebFetch", "WebSearch", "ViewPage"):
            return {"target": excerpt(data.get("url") or data.get("query") or "", 300)}
        if name == "Agent":
            return {"desc": excerpt(data.get("description", ""), 300)}
        if name == "Photo":
            return {"search": excerpt(data.get("search", ""), 120)}
        if name in ("Icon", "Component"):
            names = data.get("names") or [data.get("name") or ""]
            return {"names": [excerpt(item, 40) for item in names[:20] if isinstance(item, str)]}
        if name == "Ornament":
            return {"kind": excerpt(data.get("kind", ""), 40)}
        if name == "AppIcon":
            return {"target": excerpt(rel(data.get("svg", "")), 300)}
        if name == "Screenshot":
            return {"target": excerpt(rel(data.get("url") or data.get("path") or ""), 300)}
        if name.startswith("mcp__"):
            # Outils MCP du moteur (GitHub, sessions, documents) : leur titre en français.
            title = describe_call(name, data)
            if title:
                return {"title": title, "raw": excerpt(rel(json.dumps(data, ensure_ascii=False)), 600)}
        if name == "AskUserQuestion":
            questions = data.get("questions") if isinstance(data.get("questions"), list) else [data]
            return {"q": [excerpt(question.get("question", ""), 200) for question in questions[:4]
                          if isinstance(question, dict)]}
        return {"raw": excerpt(rel(json.dumps(data, ensure_ascii=False)), 600)}

    def _result(self, name, content):
        rel = self.relative
        try:
            value = json.loads(content) if isinstance(content, str) else content
        except json.JSONDecodeError:
            value = None
        if not isinstance(value, dict):
            text = content if isinstance(content, str) else json.dumps(content, ensure_ascii=False)
            return {"out": excerpt(rel(text), 1200, tail=True)}
        if name in ("bash", "PowerShell"):
            return {"out": excerpt(rel(without_claw_notes(str(value.get("stdout") or ""))), 1500, tail=True),
                    "err": excerpt(rel(str(value.get("stderr") or "")), 800, tail=True),
                    "code": str(value.get("returnCodeInterpretation") or ""),
                    "bg": bool(value.get("backgroundTaskId"))}
        if name in ("write_file", "edit_file", "MultiEdit"):
            result = patch(value.get("structuredPatch"))
            result["kind"] = value.get("type") or "update"
            return result
        if name in ("apply_patch", "delete_file", "replace_in_files"):
            files = []
            for item in (value.get("files") or value.get("matches") or [])[:12]:
                if not isinstance(item, dict):
                    continue
                entry = patch(item.get("structuredPatch"))
                entry.update({"path": rel(item.get("filePath") or ""), "kind": item.get("type") or "update"})
                if item.get("movedFrom"):
                    entry["from"] = rel(item["movedFrom"])
                files.append(entry)
            return {"files": files}
        if name == "Wireframe":
            return {"out": excerpt(str(value.get("wireframe") or ""), 4000)}
        if name == "run_tests":
            failures = [f"✗ {item.get('name', '')}: {str(item.get('message') or '').splitlines()[0] if item.get('message') else ''}"
                        for item in (value.get("failures") or [])[:10] if isinstance(item, dict)]
            head = (f"{value.get('passed')} réussis, {value.get('failed') or 0} en échec"
                    if value.get("passed") is not None else ("réussis" if value.get("succeeded") else "en échec"))
            return {"out": excerpt("\n".join([head, *failures]) + "\n\n" + str(value.get("output") or ""), 1500, tail=True),
                    "code": "" if value.get("succeeded") else "échec"}
        if name == "git":
            return {"out": excerpt(rel(json.dumps(value, ensure_ascii=False, indent=1)), 1200)}
        if name == "http_request":
            return {"out": excerpt(str(value.get("body") or ""), 1200),
                    "count": f"{value.get('status')} {value.get('statusText') or ''}".strip()}
        if name == "list_directory":
            return {"out": excerpt(str(value.get("tree") or ""), 1500), "count": value.get("files")}
        if name == "move_file":
            return {"from": rel(value.get("source") or ""), "to": rel(value.get("destination") or "")}
        if name == "read_file":
            file = value.get("file") if isinstance(value.get("file"), dict) else {}
            return {"count": file.get("numLines") or len(str(file.get("content") or "").splitlines())}
        if name == "glob_search":
            names = value.get("filenames") or []
            return {"out": excerpt("\n".join(rel(item) for item in names[:40]), 1200),
                    "count": value.get("numFiles", len(names))}
        if name == "grep_search":
            text = value.get("content") or "\n".join(value.get("filenames") or [])
            return {"out": excerpt(rel(text), 1200), "count": value.get("numFiles")}
        if name == "TodoWrite":
            return {}
        if name == "Photo":
            return {"photos": photos(value, self.fetch)}
        if name == "Screenshot":
            return {"shots": shots(value, self.store, self.translate, self._changed),
                    "found": value.get("problemsFound", 0)}
        if name in ("Icon", "Ornament", "Component"):
            # Du SVG et du CSS pour le modèle : rien à lire pour l'utilisateur.
            return {}
        if name == "AppIcon" and isinstance(value, dict):
            return {"out": excerpt("\n".join(rel(str(item.get("path", ""))) for item in value.get("written", [])[:6] if isinstance(item, dict)), 600)}
        return {"out": excerpt(rel(json.dumps(value, ensure_ascii=False)), 800)}

    # --- Parties à publier ---------------------------------------------------------------

    def _packed(self, start, end):
        # Compressé, le journal échappe à la relecture du commentaire : ses secrets partent avant.
        data = hide_secrets(json.dumps({"v": 1, "events": self.events[start:end]}, ensure_ascii=False,
                                       separators=(",", ":"))).encode("utf-8")
        return base64.b64encode(gzip.compress(data, 9)).decode("ascii")

    def bodies(self, run_id):
        """Corps de chaque partie : [(numéro, corps, empreinte)]. Une partie trop longue est
        coupée là où elle tient encore ; la suite commence une nouvelle partie."""
        while True:
            start = self.bounds[-1]
            if len(self.events) - start <= 1 or len(self._packed(start, len(self.events))) <= PAGE_CHARS:
                break
            low, high = start + 1, len(self.events) - 1
            while low < high:
                middle = (low + high + 1) // 2
                if len(self._packed(start, middle)) <= PAGE_CHARS:
                    low = middle
                else:
                    high = middle - 1
            self.bounds.append(low)
        pages = []
        for index, start in enumerate(self.bounds):
            end = self.bounds[index + 1] if index + 1 < len(self.bounds) else len(self.events)
            packed = self._packed(start, end)
            head = json.dumps({"run": run_id, "page": index + 1})
            body = (f"{LOG} {head} -->\n"
                    f"Journal détaillé de ce passage (partie {index + 1}), affiché dans l'appli Claw.\n\n"
                    f"{DATA} {packed} -->")
            pages.append((index + 1, body, hashlib.sha256(body.encode("utf-8")).hexdigest()))
        return pages
