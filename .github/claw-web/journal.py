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

LOG = "<!-- claw-web:log"
DATA = "<!-- claw-web:data"
# Taille d'une partie (base64), sous la limite de 65 536 caractères d'un commentaire.
PAGE_CHARS = 60_000
DIFF_LINES = 60


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

    def __init__(self, relative=lambda text: str(text)):
        self.relative = relative
        self.events = []
        self.waiting = collections.deque()
        # Indice du premier événement de chaque partie : une partie pleine reste fermée.
        self.bounds = [0]
        self.sent = {}
        self.dirty = False

    # --- Lecture des événements de Claw (`--output-format stream-json`) ----------------

    def add(self, event):
        kind = event.get("type")
        message = event.get("message") if isinstance(event.get("message"), dict) else {}
        if kind == "system" and event.get("subtype") == "init":
            self._push({"k": "init", "model": event.get("model", "")})
        elif kind == "system" and event.get("subtype") == "notice":
            self._push({"k": "notice", "t": " ".join(str(event.get("text", "")).split())})
        elif kind == "assistant":
            for part in message.get("content") or []:
                what = part.get("type")
                if what == "thinking" and str(part.get("thinking", "")).strip():
                    self._push({"k": "think", "t": excerpt(part["thinking"].strip(), 600)})
                elif what == "text" and str(part.get("text", "")).strip():
                    self._push({"k": "text", "t": excerpt(part["text"].strip(), 8000)})
                elif what == "tool_use":
                    name = part.get("name", "")
                    entry = {"k": "tool", "n": name, "i": self._input(name, part.get("input"))}
                    self._push(entry)
                    self.waiting.append(entry)
        elif kind == "user":
            # Les résultats arrivent dans l'ordre des appels.
            for part in message.get("content") or []:
                if part.get("type") != "tool_result" or not self.waiting:
                    continue
                entry = self.waiting.popleft()
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

    def _push(self, entry):
        self.events.append(entry)
        self.dirty = True

    def _input(self, name, data):
        data = data if isinstance(data, dict) else {}
        rel = self.relative
        if name in ("bash", "PowerShell"):
            return {"cmd": excerpt(rel(data.get("command", "")), 1500),
                    "desc": excerpt(data.get("description", ""), 200)}
        if name in ("read_file", "write_file", "edit_file", "NotebookEdit"):
            return {"path": rel(data.get("path") or data.get("file_path") or data.get("notebook_path") or "")}
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
        if name in ("write_file", "edit_file"):
            result = patch(value.get("structuredPatch"))
            result["kind"] = value.get("type") or "update"
            return result
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
        return {"out": excerpt(rel(json.dumps(value, ensure_ascii=False)), 800)}

    # --- Parties à publier ---------------------------------------------------------------

    def _packed(self, start, end):
        data = json.dumps({"v": 1, "events": self.events[start:end]}, ensure_ascii=False,
                          separators=(",", ":")).encode("utf-8")
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
