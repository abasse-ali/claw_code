"""Mémoire de Claw par projet : un fichier par dépôt de tâche (`memoire/<propriétaire>/<dépôt>.md`)
sur la branche des fichiers du dépôt des sessions (privé), lisible sur GitHub.

Avant le passage (setup.py), elle est remise dans `.claw/` du clone, que l'agent lit au début
de chaque tâche : les leçons qu'il a notées (outil Remember) dans `.claw/lessons.md`, ce que Claw
a relevé lui-même dans `.claw/memory.md` (erreurs déjà rencontrées et leur issue, commandes qui
ont marché, dernières demandes). Après le passage (run.py), ce qu'il a appris y est ajouté : ses
nouvelles leçons, et ce que montrent ses commandes. Le clone est neuf à chaque passage et `.claw`
n'est jamais poussé : sans cette mémoire, chaque passage refaisait les mêmes essais et les mêmes
erreurs (22 octobre 2026).
"""

import base64
import json
import re
import time
import urllib.parse

from common import FILES_BRANCH, GitHubError, api

VERSION = 1
FOLDER = "memoire"
DATA = re.compile(r"<!--\s*claw-memory\s+(\{.*?\})\s*-->", re.S)
# Ce qui est gardé : les leçons comme l'outil Remember (80), le reste en bref.
MAX_LESSONS = 80
MAX_WORKED = 12
MAX_ERRORS = 12
MAX_REQUESTS = 8
COMMAND_CHARS = 200
ERROR_CHARS = 180
STATES = {"done": "fini", "failed": "erreur", "unfinished": "inachevé", "stopped": "arrêté"}
# Commandes qui ne disent rien du projet (lecture, navigation, petits scripts jetables).
TRIVIAL = {
    "ls", "cat", "head", "tail", "pwd", "echo", "printf", "grep", "egrep", "rg", "find", "wc",
    "sed", "awk", "which", "type", "file", "stat", "tree", "sort", "uniq", "cut", "true", "false",
    "sleep", "date", "whoami", "env", "printenv", "du", "df", "mkdir", "cp", "mv", "rm", "touch",
    "chmod", "ln", "kill", "pkill", "ps", "lsof", "diff", "cmp", "xxd", "od", "basename",
    "dirname", "realpath", "readlink", "test", "[", "set", "export", "source", ".", "curl",
    "wget", "jq", "claw-github", "timeout", "nohup",
}
TRIVIAL_GIT = {"status", "log", "diff", "show", "branch", "add", "commit", "checkout", "stash",
               "rev-parse", "ls-files", "remote", "config", "restore", "reset", "switch", "fetch"}
ERROR_WORDS = re.compile(
    r"error|erreur|fail|echec|échec|fatal|cannot|can't|not found|introuvable|traceback|exception|"
    r"enoent|eaddrinuse|denied|refused|missing|undefined|syntaxerror|panic", re.I)
SECRET = re.compile(r"password|passwd|api_key|apikey|secret=|token=|bearer |[A-Za-z0-9_\-]{32,}", re.I)


def path_of(repo):
    """`memoire/propriétaire/dépôt.md` (le nom du dépôt de la tâche, tel qu'écrit dans la session)."""
    owner, _, name = repo.strip().partition("/")
    safe = lambda part: re.sub(r"[^A-Za-z0-9._-]", "-", part) or "x"
    return f"{FOLDER}/{safe(owner)}/{safe(name)}.md"


def empty():
    return {"v": VERSION, "lessons": [], "worked": [], "errors": [], "requests": []}


def parse(text):
    """Les données du fichier (commentaire `claw-memory` à sa fin), ou une mémoire vide."""
    match = DATA.search(text or "")
    data = empty()
    if not match:
        return data
    try:
        stored = json.loads(match.group(1))
    except ValueError:
        return data
    if not isinstance(stored, dict):
        return data
    for key in ("lessons", "worked", "errors", "requests"):
        value = stored.get(key)
        if isinstance(value, list):
            data[key] = [item for item in value if isinstance(item, (dict if key != "lessons" else str))]
    return data


def one_line(text, size):
    text = " ".join(str(text or "").split())
    return text if len(text) <= size else text[: size - 1].rstrip() + "…"


def times(count):
    return f"{count} fois" if count > 1 else "1 fois"


def memory_text(data):
    """Ce que Claw a relevé lui-même, pour l'agent (`.claw/memory.md`) : erreurs d'abord."""
    parts = []
    if data["errors"]:
        parts.append("## Erreurs déjà rencontrées (ne pas refaire le même essai sans raison nouvelle)")
        for item in data["errors"]:
            issue = "réglée ensuite" if item.get("fixed") else "pas encore réglée"
            parts.append(f"- `{item['cmd']}` → {item.get('error') or 'échec'} "
                         f"({times(item.get('n', 1))}, {issue}, dernière le {item.get('last', '?')})")
    if data["worked"]:
        parts.append("## Commandes qui ont marché")
        for item in data["worked"]:
            parts.append(f"- `{item['cmd']}` ({times(item.get('n', 1))}, dernière le {item.get('last', '?')})")
    if data["requests"]:
        parts.append("## Dernières demandes sur ce dépôt (les plus récentes d'abord)")
        for item in reversed(data["requests"]):
            state = STATES.get(item.get("state"), item.get("state") or "?")
            parts.append(f"- {item.get('date', '?')}, session {item.get('session', '?')} : "
                         f"{item.get('text', '')} ({state})")
    return "\n".join(parts) + ("\n" if parts else "")


def render(repo, data):
    """Le fichier rangé dans le dépôt des sessions : lisible, avec ses données à la fin."""
    lines = [f"# Mémoire de Claw : {repo}", "",
             "Tenue par le moteur de Claw Web, lue par Claw au début de chaque tâche sur ce dépôt. "
             "Effacer ce fichier remet sa mémoire à zéro.", ""]
    if data["lessons"]:
        lines.append("## Leçons notées par Claw")
        lines.extend(lesson if lesson.startswith("- ") else f"- {lesson}" for lesson in data["lessons"])
        lines.append("")
    body = memory_text(data)
    if body:
        lines.append(body.rstrip())
        lines.append("")
    lines.append(f"<!-- claw-memory {json.dumps(data, ensure_ascii=False)} -->")
    return "\n".join(lines) + "\n"


def load(token, engine, repo, call=api):
    """(données, sha du fichier) ; une mémoire vide si le fichier ou la branche manque."""
    path = urllib.parse.quote(path_of(repo))
    try:
        found = call("GET", f"/repos/{engine}/contents/{path}?ref={FILES_BRANCH}", token)
    except GitHubError as error:
        if error.status == 404:
            return empty(), None
        raise
    try:
        text = base64.b64decode(found.get("content") or "").decode("utf-8", "replace")
    except (ValueError, TypeError):
        text = ""
    return parse(text), found.get("sha")


def install(data, claw_dir):
    """Remet la mémoire dans `.claw/` du clone ; rend les leçons écrites (pour savoir ensuite
    lesquelles sont nouvelles)."""
    claw_dir.mkdir(parents=True, exist_ok=True)
    lessons = [lesson if lesson.startswith("- ") else f"- {lesson}" for lesson in data["lessons"]]
    if lessons:
        (claw_dir / "lessons.md").write_text("\n".join(lessons) + "\n", encoding="utf-8")
    body = memory_text(data)
    if body:
        (claw_dir / "memory.md").write_text(body, encoding="utf-8")
    return lessons


def read_lessons(claw_dir):
    try:
        text = (claw_dir / "lessons.md").read_text(encoding="utf-8")
    except OSError:
        return []
    return [line.strip() for line in text.splitlines() if line.strip()]


def _text_of(content):
    if isinstance(content, str):
        return content
    if isinstance(content, list):
        return "\n".join(str(part.get("text") or "") for part in content if isinstance(part, dict))
    return ""


def core_command(command):
    """La commande sans les `cd dossier &&` de tête."""
    rest = command.strip()
    while True:
        match = re.match(r"cd\s+\S+\s*(?:&&|;)\s*", rest)
        if not match:
            return rest
        rest = rest[match.end():]


def trivial(command):
    words = core_command(command).split()
    if not words:
        return True
    head = words[0].rsplit("/", 1)[-1]
    if head in TRIVIAL:
        return True
    if head == "git" and (len(words) < 2 or words[1] in TRIVIAL_GIT):
        return True
    # Petits scripts jetables (`python3 -c`, `node -e`) et redirections vers un fichier.
    if len(words) > 1 and words[1] in ("-c", "-e") and head.startswith(("python", "node", "bash", "sh")):
        return True
    return "<<" in command


def error_line(stdout, stderr, relative):
    """La ligne qui dit l'erreur (la dernière qui en parle), sans les remarques de Claw."""
    text = "\n".join(part for part in (stdout, stderr) if part)
    cut = text.find("[Claw] ")
    if cut >= 0:
        text = text[:cut]
    lines = [line.strip() for line in relative(text).splitlines() if line.strip()]
    if not lines:
        return ""
    chosen = next((line for line in reversed(lines) if ERROR_WORDS.search(line)), lines[-1])
    return one_line(chosen, ERROR_CHARS)


def observe(events, relative=lambda text: text):
    """Les commandes du passage, dans l'ordre : [(commande, réussie, ligne d'erreur)]. Celles
    lancées en arrière-plan (serveurs) et celles qui ne disent rien du projet sont laissées."""
    calls, runs = {}, []
    for event in events:
        message = event.get("message") if isinstance(event.get("message"), dict) else {}
        for part in message.get("content") or []:
            if not isinstance(part, dict):
                continue
            if event.get("type") == "assistant" and part.get("type") == "tool_use" \
                    and part.get("name") in ("bash", "PowerShell"):
                data = part.get("input") if isinstance(part.get("input"), dict) else {}
                command = " ".join(str(data.get("command") or "").split())
                if command and not data.get("run_in_background") and not trivial(command) \
                        and not SECRET.search(command):
                    calls[part.get("id")] = one_line(relative(command), COMMAND_CHARS)
            elif event.get("type") == "user" and part.get("type") == "tool_result" \
                    and part.get("tool_use_id") in calls:
                command = calls.pop(part["tool_use_id"])
                raw = _text_of(part.get("content"))
                try:
                    result = json.loads(raw)
                except ValueError:
                    result = None
                if not isinstance(result, dict):
                    # Refusée ou impossible à lancer : rien à apprendre sur le projet.
                    continue
                if result.get("backgroundTaskId") or result.get("interrupted"):
                    continue
                code = str(result.get("returnCodeInterpretation") or "")
                ok = not part.get("is_error") and not code
                error = "" if ok else (error_line(result.get("stdout"), result.get("stderr"), relative)
                                       or code)
                runs.append((command, ok, error))
    return runs


def learn(data, runs, new_lessons, request="", state="", session="", today=None):
    """Ajoute à la mémoire ce que ce passage a appris (sur place) et la rend."""
    today = today or time.strftime("%Y-%m-%d", time.gmtime())
    known = {lesson.lower() for lesson in data["lessons"]}
    for lesson in new_lessons:
        if lesson.lower() not in known:
            data["lessons"].append(lesson)
            known.add(lesson.lower())
    data["lessons"] = data["lessons"][-MAX_LESSONS:]
    worked = {item["cmd"]: item for item in data["worked"] if item.get("cmd")}
    errors = {item["cmd"]: item for item in data["errors"] if item.get("cmd")}
    order_worked = [item["cmd"] for item in data["worked"] if item.get("cmd")]
    order_errors = [item["cmd"] for item in data["errors"] if item.get("cmd")]
    for command, ok, error in runs:
        if ok:
            item = worked.setdefault(command, {"cmd": command, "n": 0})
            item["n"] = item.get("n", 0) + 1
            item["last"] = today
            if command in order_worked:
                order_worked.remove(command)
            order_worked.insert(0, command)
            if command in errors:
                errors[command]["fixed"] = True
        else:
            item = errors.setdefault(command, {"cmd": command, "n": 0})
            item["n"] = item.get("n", 0) + 1
            item["last"] = today
            item["error"] = error
            item["fixed"] = False
            if command in order_errors:
                order_errors.remove(command)
            order_errors.insert(0, command)
    data["worked"] = [worked[command] for command in order_worked][:MAX_WORKED]
    data["errors"] = [errors[command] for command in order_errors][:MAX_ERRORS]
    request = one_line(request, 140)
    if request:
        data["requests"].append({"date": today, "session": str(session), "text": request, "state": state})
        data["requests"] = data["requests"][-MAX_REQUESTS:]
    return data


def save(token, engine, repo, update, session="", call=api, ensure_branch=None):
    """Relit la mémoire du dépôt, y applique `update` (fonction qui change les données) et la
    range ; une fois de plus si une autre session l'a changée entre-temps. Rend vrai si rangée."""
    if ensure_branch is not None and not ensure_branch():
        return False
    path = urllib.parse.quote(path_of(repo))
    for _ in range(2):
        data, sha = load(token, engine, repo, call)
        update(data)
        body = {
            "message": f"Mémoire de Claw pour {repo} (session {session})",
            "content": base64.b64encode(render(repo, data).encode("utf-8")).decode("ascii"),
            "branch": FILES_BRANCH,
        }
        if sha:
            body["sha"] = sha
        try:
            call("PUT", f"/repos/{engine}/contents/{path}", token, body)
            return True
        except GitHubError as error:
            if error.status not in (409, 422):
                raise
    return False
