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
import collections
import json
import re
import time
import unicodedata
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
MAX_DEFECTS = 8
# Échecs d'affilée d'une même commande dans un passage : au-delà, l'agent tournait en rond.
STUCK_AFTER = 3
WRITE_TOOLS = {"write_file", "edit_file", "MultiEdit", "NotebookEdit", "apply_patch", "move_file",
               "delete_file", "replace_in_files"}
# Un message de l'utilisateur qui dit qu'un défaut reste (sans accents, en minuscules).
COMPLAINTS = (
    "mal fait", "moche", "marche pas", "marche plus", "fonctionne pas", "fonctionne plus", "toujours pas",
    "pas encore", "toujours le meme", "toujours la meme", "bug", "casse", "erreur", "n'importe quoi",
    "pas bon", "pas ca", "pas beau", "pas terrible", "affiche pas", "affiche rien", "s'affiche pas",
    "rien ne", "manque", "y'a pas", "y a pas", "il n'y a pas", "plante", "bloque", "doesn't work",
    "not working", "broken", "wrong", "still",
)
# Mots qui ne disent pas de quoi on parle, pour rapprocher deux plaintes.
STOP_WORDS = {
    "encore", "toujours", "trop", "tres", "plus", "moins", "c'est", "cest", "elle", "elles", "ils",
    "avec", "sans", "dans", "pour", "mais", "donc", "alors", "comme", "quand", "fait", "faire", "marche",
    "fonctionne", "rien", "tout", "tous", "toute", "toutes", "cette", "celle", "celui", "bien", "mal",
    "pas", "les", "des", "une", "sur", "sous", "aussi", "meme", "moche", "erreur", "page", "site",
}
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
    return {"v": VERSION, "lessons": [], "worked": [], "errors": [], "requests": [], "defects": []}


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
    for key in ("lessons", "worked", "errors", "requests", "defects"):
        value = stored.get(key)
        if isinstance(value, list):
            data[key] = [item for item in value if isinstance(item, (dict if key != "lessons" else str))]
    return data


def one_line(text, size):
    text = " ".join(str(text or "").split())
    return text if len(text) <= size else text[: size - 1].rstrip() + "…"


def times(count):
    return f"{count} fois" if count > 1 else "1 fois"


def files_text(files):
    shown = ", ".join(f"`{name}`" for name in files[:4])
    return shown + (f" et {len(files) - 4} autre(s)" if len(files) > 4 else "")


def memory_text(data):
    """Ce que Claw a relevé lui-même, pour l'agent (`.claw/memory.md`) : les défauts que
    l'utilisateur a signalés, puis les erreurs et ce qui les a réglées, puis ce qui marche."""
    parts = []
    if data.get("defects"):
        parts.append("## Défauts signalés par l'utilisateur")
        for item in data["defects"]:
            sessions = ", ".join(str(session) for session in item.get("sessions", []))
            line = f"- « {item.get('text', '')} » (le {item.get('last', '?')}, session(s) {sessions})"
            if item.get("files"):
                line += f" ; fichiers changés en réponse : {files_text(item['files'])}"
            if item.get("n", 1) > 1:
                line += (f". Signalé {item['n']} fois : les corrections précédentes n'ont pas suffi. Ne refais "
                         "pas la même chose : cherche la vraie cause, change d'approche, et regarde le résultat "
                         "toi-même (capture d'écran, commande) avant de dire que c'est réglé")
            parts.append(line)
    if data["errors"]:
        parts.append("## Erreurs déjà rencontrées (ne pas refaire le même essai sans raison nouvelle)")
        for item in data["errors"]:
            if item.get("fixed") and item.get("fix"):
                issue = f"réglée par : {item['fix']}"
            elif item.get("fixed"):
                issue = "réglée ensuite"
            else:
                issue = "pas encore réglée"
            line = (f"- `{item['cmd']}` → {item.get('error') or 'échec'} "
                    f"({times(item.get('n', 1))}, {issue}, dernière le {item.get('last', '?')})")
            if item.get("stuck") and not item.get("fixed"):
                line += (f". Bloquée {item['stuck']} fois d'affilée : ne la relance pas telle quelle ; lis "
                         "l'erreur entière, trouves-en la cause et prends une autre voie")
            parts.append(line)
    if data["worked"]:
        parts.append("## Commandes qui ont marché")
        for item in data["worked"]:
            parts.append(f"- `{item['cmd']}` ({times(item.get('n', 1))}, dernière le {item.get('last', '?')})")
    if data["requests"]:
        parts.append("## Dernières demandes sur ce dépôt (les plus récentes d'abord)")
        for item in reversed(data["requests"]):
            state = STATES.get(item.get("state"), item.get("state") or "?")
            line = f"- {item.get('date', '?')}, session {item.get('session', '?')} : {item.get('text', '')} ({state})"
            if item.get("files"):
                line += f" ; fichiers changés : {files_text(item['files'])}"
            parts.append(line)
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


# Un nom de fichier du projet dans une commande ou une note (`tests/test_x.py`, `app.js`).
PROJECT_FILE = re.compile(r"(?<![:/\w.-])(?:\./)?([\w-]+(?:/[\w.-]+)*\.(?:py|js|mjs|ts|tsx|jsx|html?|css|json|md|rs|go|"
                          r"java|rb|php|sh|toml|ya?ml|vue|svelte))\b")


def _absent(names, root):
    """Des fichiers sont nommés, et aucun n'existe dans le clone."""
    names = [name for name in names if name]
    return bool(names) and not any((root / name).exists() for name in names)


def for_this_checkout(data, root):
    """La mémoire sans ce qui ne parle que de fichiers absents du clone : une autre branche du
    même dépôt peut porter un tout autre projet. Vu le 24 octobre 2026 (session de test : une appli
    de budget partie d'une branche vide du dépôt d'un script Amazon) : la réponse finale renvoyait
    l'utilisateur aux tests `tests/test_cdiscount_client.py` du script, absents de sa branche. Le
    fichier de mémoire du dépôt des sessions, lui, garde tout."""
    kept = dict(data)
    kept["errors"] = [item for item in data["errors"] if not _absent(PROJECT_FILE.findall(item.get("cmd", "")), root)]
    kept["worked"] = [item for item in data["worked"] if not _absent(PROJECT_FILE.findall(item.get("cmd", "")), root)]
    kept["requests"] = [item for item in data["requests"] if not _absent(item.get("files") or [], root)]
    kept["defects"] = [item for item in data.get("defects") or [] if not _absent(item.get("files") or [], root)]
    return kept


def install(data, claw_dir):
    """Remet la mémoire dans `.claw/` du clone ; rend les leçons écrites (pour savoir ensuite
    lesquelles sont nouvelles). N'y va que ce qui concerne les fichiers de ce clone."""
    claw_dir.mkdir(parents=True, exist_ok=True)
    lessons = [lesson if lesson.startswith("- ") else f"- {lesson}" for lesson in data["lessons"]]
    if lessons:
        (claw_dir / "lessons.md").write_text("\n".join(lessons) + "\n", encoding="utf-8")
    body = memory_text(for_this_checkout(data, claw_dir.parent))
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


def _fix_text(pending):
    """Ce qui s'est passé entre l'échec d'une commande et sa réussite."""
    steps = []
    if pending["files"]:
        steps.append(f"modification de {files_text(pending['files'])}")
    steps.extend(f"`{command}`" for command in pending["commands"][:3])
    return one_line(" ; ".join(steps) + (" ; puis la même commande" if steps else "relancée telle quelle"),
                    ERROR_CHARS)


def observe(events, relative=lambda text: text):
    """Les commandes du passage, dans l'ordre : [(commande, réussie, ligne d'erreur, solution)].
    La solution d'une commande qui réussit après avoir échoué : les fichiers changés et les
    commandes réussies entre les deux. Celles lancées en arrière-plan (serveurs) et celles qui
    ne disent rien du projet sont laissées. Les résultats suivent les appels dans l'ordre
    (`--output-format stream-json` ne porte pas d'identifiant d'appel), comme dans journal.py."""
    waiting, runs = collections.deque(), []
    # Commandes en échec, pas encore réussies : ce qui est fait depuis.
    pending = {}
    for event in events:
        message = event.get("message") if isinstance(event.get("message"), dict) else {}
        content = message.get("content")
        for part in content if isinstance(content, list) else []:
            if not isinstance(part, dict):
                continue
            if event.get("type") == "assistant" and part.get("type") == "tool_use":
                command = None
                data = part.get("input") if isinstance(part.get("input"), dict) else {}
                if part.get("name") in ("bash", "PowerShell"):
                    text = " ".join(str(data.get("command") or "").split())
                    if text and not data.get("run_in_background") and not trivial(text) \
                            and not SECRET.search(text):
                        command = one_line(relative(text), COMMAND_CHARS)
                elif part.get("name") in WRITE_TOOLS and data.get("path"):
                    path = relative(str(data["path"]))
                    for steps in pending.values():
                        if path not in steps["files"]:
                            steps["files"].append(path)
                waiting.append((part.get("name"), command))
            elif event.get("type") == "user" and part.get("type") == "tool_result" and waiting:
                name, command = waiting.popleft()
                if command is None or part.get("tool_name") not in (None, name):
                    continue
                try:
                    result = json.loads(_text_of(part.get("content")))
                except ValueError:
                    result = None
                if not isinstance(result, dict):
                    # Refusée ou impossible à lancer : rien à apprendre sur le projet.
                    continue
                if result.get("backgroundTaskId") or result.get("interrupted"):
                    continue
                code = str(result.get("returnCodeInterpretation") or "")
                ok = not part.get("is_error") and not code
                error = "" if ok else (error_line(str(result.get("stdout") or ""),
                                                  str(result.get("stderr") or ""), relative) or code)
                fix = ""
                if ok:
                    if command in pending:
                        fix = _fix_text(pending.pop(command))
                    for other, steps in pending.items():
                        if command not in steps["commands"]:
                            steps["commands"].append(command)
                else:
                    pending.setdefault(command, {"files": [], "commands": []})
                runs.append((command, ok, error, fix))
    return runs


def edited_files(events, relative=lambda text: text):
    """Les fichiers que l'agent a changés pendant le passage, dans l'ordre."""
    files = []
    for event in events:
        message = event.get("message") if isinstance(event.get("message"), dict) else {}
        content = message.get("content")
        if event.get("type") != "assistant" or not isinstance(content, list):
            continue
        for part in content:
            if isinstance(part, dict) and part.get("type") == "tool_use" and part.get("name") in WRITE_TOOLS:
                data = part.get("input") if isinstance(part.get("input"), dict) else {}
                path = relative(str(data.get("path") or ""))
                if path and path not in files:
                    files.append(path)
    return files


def fold(text):
    text = unicodedata.normalize("NFD", str(text or "").lower().replace("\u2019", "'"))
    return "".join(char for char in text if unicodedata.category(char) != "Mn")


def complains(request):
    """Le message de l'utilisateur dit qu'un défaut est là, ou reste."""
    text = fold(request)
    return any(marker in text for marker in COMPLAINTS)


def subject_words(text):
    return {word for word in re.findall(r"[a-z0-9']{4,}", fold(text)) if word not in STOP_WORDS}


def _remember_defect(data, request, session, files, today):
    """Range une plainte de l'utilisateur ; la rapproche d'une plainte déjà notée sur le même
    sujet (un mot en commun qui dit de quoi on parle : « icônes », « disk »)."""
    words = subject_words(request)
    defects = data.setdefault("defects", [])
    same = next((item for item in reversed(defects) if words & subject_words(item.get("text", ""))), None)
    if same is None:
        same = {"text": request, "n": 0, "sessions": []}
        defects.append(same)
    else:
        defects.remove(same)
        defects.append(same)
        same["text"] = request
    same["n"] = same.get("n", 0) + 1
    if str(session) not in same["sessions"]:
        same["sessions"] = (same["sessions"] + [str(session)])[-6:]
    same["last"] = today
    same["files"] = files[:8]
    data["defects"] = defects[-MAX_DEFECTS:]


def learn(data, runs, new_lessons, request="", state="", session="", today=None, files=None):
    """Ajoute à la mémoire ce que ce passage a appris (sur place) et la rend : les leçons, les
    erreurs et ce qui les a réglées, les commandes sur lesquelles l'agent a tourné en rond, ce
    qui a marché, la demande, et la plainte qu'elle porte, rapprochée des précédentes."""
    today = today or time.strftime("%Y-%m-%d", time.gmtime())
    files = list(files or [])
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
    streak = collections.Counter()
    for run in runs:
        command, ok, error = run[:3]
        fix = run[3] if len(run) > 3 else ""
        if ok:
            streak[command] = 0
            item = worked.setdefault(command, {"cmd": command, "n": 0})
            item["n"] = item.get("n", 0) + 1
            item["last"] = today
            if command in order_worked:
                order_worked.remove(command)
            order_worked.insert(0, command)
            if command in errors:
                errors[command]["fixed"] = True
                if fix:
                    errors[command]["fix"] = fix
                errors[command].pop("stuck", None)
        else:
            streak[command] += 1
            item = errors.setdefault(command, {"cmd": command, "n": 0})
            item["n"] = item.get("n", 0) + 1
            item["last"] = today
            item["error"] = error
            item["fixed"] = False
            if streak[command] >= STUCK_AFTER:
                item["stuck"] = max(item.get("stuck", 0), streak[command])
            if command in order_errors:
                order_errors.remove(command)
            order_errors.insert(0, command)
    data["worked"] = [worked[command] for command in order_worked][:MAX_WORKED]
    data["errors"] = [errors[command] for command in order_errors][:MAX_ERRORS]
    request = one_line(request, 140)
    if request:
        entry = {"date": today, "session": str(session), "text": request, "state": state}
        if files:
            entry["files"] = files[:8]
        data["requests"].append(entry)
        data["requests"] = data["requests"][-MAX_REQUESTS:]
        if complains(request):
            _remember_defect(data, request, session, files, today)
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
