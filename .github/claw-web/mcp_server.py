"""Serveurs MCP du moteur : les outils de Claude Code sur le web (serveurs `github`,
`claude-code-remote` et `Claude_Docs`), donnés à Claw sous les noms `github`, `claw-remote` et
`claw-docs`.

Le moteur (run.py) les sert en HTTP sur 127.0.0.1, protégés par un secret du passage, et les
déclare à Claw (`mcpServers`) : l'agent appelle `mcp__github__issue_read` comme dans Claude
Code, et c'est le moteur qui fait l'appel à GitHub, avec un jeton que l'agent n'a jamais, en
s'en tenant au dépôt de la session (mcp_github.py).

Pour Claw Code sur votre machine, chaque serveur se lance aussi sur l'entrée standard :

    python3 .github/claw-web/mcp_server.py --stdio github

(jeton dans GITHUB_TOKEN ou GH_TOKEN, ou celui de `gh auth token` ; dépôt : celui du dossier
courant, ou `--repo propriétaire/nom` ; sessions et documents de Claw Web : `--engine
propriétaire/claw-sessions` ou CLAW_WEB_ENGINE). `--catalog` écrit la liste des outils que
montre l'appli (claw-web/js/mcp-tools.js).

Les descriptions s'adressent au modèle, en anglais, comme les autres notes du moteur ; les
titres, en français, sont ceux du journal et du panneau « Outils MCP » de l'appli.
"""

import json
import os
import re
import secrets
import subprocess
import sys
import threading
import time
import urllib.error
import urllib.request
from dataclasses import dataclass, field
from datetime import datetime, timezone
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

from common import API, GitHubError, api

# Lancé par `python3 mcp_server.py --stdio …`, ce module est `__main__` : les modules d'outils
# qui font `import mcp_server` doivent retrouver celui-ci (ses ToolError), pas une seconde copie.
sys.modules.setdefault("mcp_server", sys.modules[__name__])

PROTOCOL_VERSION = "2025-03-26"
VERSION = "1.0"
# Au-delà, la réponse d'un outil est coupée (le modèle la relit par pages).
MAX_RESULT_CHARS = 60_000


def api_raw(path, token, accept="application/vnd.github.raw"):
    """Appel de l'API GitHub qui rend des octets (journal d'un job, fichier brut) ; suit les
    redirections (les journaux sont servis par une adresse signée)."""
    request = urllib.request.Request(path if path.startswith("http") else API + path)
    request.add_header("Authorization", f"Bearer {token}")
    request.add_header("Accept", accept)
    request.add_header("X-GitHub-Api-Version", "2022-11-28")
    request.add_header("User-Agent", "claw-web")
    try:
        with urllib.request.urlopen(request, timeout=60) as response:
            return response.read()
    except urllib.error.HTTPError as error:
        detail = error.read().decode("utf-8", "replace")[:300]
        raise GitHubError(f"GET {path} : HTTP {error.code} {detail}", error.code, detail) from None


class ToolError(Exception):
    """Refus ou échec d'un outil, dit au modèle (résultat `isError`)."""


@dataclass
class Tool:
    name: str
    title: str
    description: str
    schema: dict
    handler: object
    read_only: bool = False
    # Pour le panneau de l'appli : ce que Claw Web change par rapport à Claude Code.
    note: str = ""


@dataclass
class Context:
    """Ce que le moteur sait du passage. En local (Claw Code sur votre machine), `session` est
    vide et la branche de la session est la branche courante."""

    repo: str = ""
    base: str = ""
    branch: str = ""
    session: str = ""
    engine: str = ""
    job_token: str = ""
    repo_token: str = ""
    run_id: str = ""
    work: Path = field(default_factory=lambda: Path("."))
    requests_file: Path = None
    # Modèle et machine du passage (une session créée ou relancée par Claw les reprend).
    model: str = ""
    runner: str = ""
    # Où cloner les dépôts ajoutés (add_repo) ; la source de Claw (documentation).
    repos_dir: Path = None
    claw_src: Path = None
    local: bool = False
    call: object = api
    raw: object = api_raw
    now: object = lambda: datetime.now(timezone.utc)

    @property
    def owner(self):
        return self.repo.split("/", 1)[0] if "/" in self.repo else ""

    @property
    def name(self):
        return self.repo.split("/", 1)[1] if "/" in self.repo else ""

    def github(self, method, path, body=None, token=None):
        """Appel de l'API GitHub avec le jeton du dépôt (celui du moteur pour son dépôt)."""
        token = token or self.repo_token or (self.job_token if self.repo.lower() == self.engine.lower() else "")
        if not token:
            raise ToolError("No GitHub token for this repository: GitHub tools are unavailable in this session.")
        try:
            return self.call(method, path, token, body)
        except GitHubError as error:
            raise ToolError(github_error_text(error)) from None

    def github_raw(self, path, accept="application/vnd.github.raw"):
        """Octets bruts d'un appel de l'API (journal de job, contenu de fichier)."""
        token = self.repo_token or (self.job_token if self.repo.lower() == self.engine.lower() else "")
        if not token:
            raise ToolError("No GitHub token for this repository: GitHub tools are unavailable in this session.")
        try:
            return self.raw(path, token, accept)
        except GitHubError as error:
            raise ToolError(github_error_text(error)) from None

    def engine_call(self, method, path, body=None, use_repo_token=False):
        """Appel de l'API sur le dépôt des sessions : jeton du job, ou jeton de l'utilisateur
        pour ce que le job ne peut pas (lancer un passage, lister les dépôts)."""
        token = (self.repo_token or self.job_token) if use_repo_token else (self.job_token or self.repo_token)
        if not self.engine or not token:
            raise ToolError("Claw Web's sessions repository is unknown here: set CLAW_WEB_ENGINE "
                            "(owner/claw-sessions) to use this tool outside Claw Web.")
        try:
            return self.call(method, path, token, body)
        except GitHubError as error:
            raise ToolError(github_error_text(error)) from None


def github_error_text(error):
    accepted = f" The token needs: {error.accepted}." if getattr(error, "accepted", "") else ""
    reason = (error.reason or str(error))[:400]
    return f"GitHub refused the request (HTTP {error.status}): {reason}.{accepted}"


# --- Schémas ------------------------------------------------------------------------------------

def string(description, **extra):
    return {"type": "string", "description": description, **extra}


def number(description, **extra):
    return {"type": "number", "description": description, **extra}


def integer(description, **extra):
    return {"type": "integer", "description": description, **extra}


def boolean(description):
    return {"type": "boolean", "description": description}


def array(items, description, **extra):
    return {"type": "array", "items": items, "description": description, **extra}


def choice(values, description):
    return {"type": "string", "enum": list(values), "description": description}


def schema(properties, required=()):
    return {"type": "object", "properties": properties, "required": list(required)}


# --- Serveur ------------------------------------------------------------------------------------

class Server:
    """Un serveur MCP : ses outils et le traitement des messages JSON-RPC."""

    def __init__(self, name, tools, instructions=""):
        self.name = name
        self.tools = {tool.name: tool for tool in tools}
        self.instructions = instructions
        self.calls = []

    def listing(self):
        return [{
            "name": tool.name,
            "title": tool.title,
            "description": tool.description,
            "inputSchema": tool.schema,
            "annotations": {"title": tool.title, "readOnlyHint": tool.read_only},
        } for tool in self.tools.values()]

    def call(self, name, arguments):
        tool = self.tools.get(name)
        if tool is None:
            return text_result(f"Unknown tool {name!r} on server {self.name}.", error=True)
        if not isinstance(arguments, dict):
            arguments = {}
        missing = [key for key in tool.schema.get("required", []) if arguments.get(key) in (None, "")]
        if missing:
            return text_result(f"Missing required argument(s): {', '.join(missing)}.", error=True)
        started = time.time()
        try:
            value = tool.handler(arguments)
            result = text_result(value)
        except ToolError as error:
            result = text_result(str(error), error=True)
        except Exception as error:  # noqa: BLE001 - une erreur d'outil ne doit pas arrêter le serveur
            result = text_result(f"Internal error in {self.name}.{name}: {error!r}", error=True)
        self.calls.append({"tool": name, "error": bool(result.get("isError")), "ms": int((time.time() - started) * 1000)})
        return result

    def handle(self, message):
        """La réponse à un message JSON-RPC (None pour une notification)."""
        if not isinstance(message, dict):
            return rpc_error(None, -32600, "Invalid request")
        method, ident = message.get("method"), message.get("id")
        if ident is None:
            return None
        params = message.get("params") or {}
        if method == "initialize":
            version = params.get("protocolVersion") if isinstance(params, dict) else None
            result = {
                "protocolVersion": version or PROTOCOL_VERSION,
                "capabilities": {"tools": {"listChanged": False}},
                "serverInfo": {"name": self.name, "version": VERSION},
            }
            if self.instructions:
                result["instructions"] = self.instructions
            return {"jsonrpc": "2.0", "id": ident, "result": result}
        if method == "ping":
            return {"jsonrpc": "2.0", "id": ident, "result": {}}
        if method == "tools/list":
            return {"jsonrpc": "2.0", "id": ident, "result": {"tools": self.listing()}}
        if method == "tools/call":
            return {"jsonrpc": "2.0", "id": ident,
                    "result": self.call(params.get("name"), params.get("arguments") or {})}
        if method in ("resources/list", "prompts/list", "resources/templates/list"):
            key = method.split("/")[0] if method != "resources/templates/list" else "resourceTemplates"
            return {"jsonrpc": "2.0", "id": ident, "result": {key: []}}
        return rpc_error(ident, -32601, f"Method not found: {method}")


def rpc_error(ident, code, message):
    return {"jsonrpc": "2.0", "id": ident, "error": {"code": code, "message": message}}


def text_result(value, error=False):
    if isinstance(value, str):
        text = value
    else:
        text = json.dumps(value, ensure_ascii=False, indent=1, default=str)
    if len(text) > MAX_RESULT_CHARS:
        text = (text[:MAX_RESULT_CHARS] + f"\n[... truncated: {len(text) - MAX_RESULT_CHARS} more characters; "
                "ask for a smaller page (perPage) or a narrower query]")
    result = {"content": [{"type": "text", "text": text}]}
    if error:
        result["isError"] = True
    return result


class Host:
    """Les serveurs sur un port local (`/mcp/<nom>`), réservés à qui a le secret du passage."""

    def __init__(self, servers, secret=None):
        self.servers = {server.name: server for server in servers}
        self.secret = secret or secrets.token_urlsafe(24)
        host = self

        class Handler(BaseHTTPRequestHandler):
            protocol_version = "HTTP/1.1"

            def log_message(self, *args):  # pas de bruit dans le journal du job
                pass

            def _reply(self, status, payload=None):
                data = b"" if payload is None else json.dumps(payload, ensure_ascii=False).encode("utf-8")
                self.send_response(status)
                if payload is not None:
                    self.send_header("Content-Type", "application/json")
                self.send_header("Content-Length", str(len(data)))
                self.end_headers()
                if data:
                    self.wfile.write(data)

            def _server(self):
                if self.headers.get("Authorization", "") != f"Bearer {host.secret}":
                    self._reply(401, {"error": "unauthorized"})
                    return None
                match = re.fullmatch(r"/mcp/([A-Za-z0-9_-]+)/?", self.path.split("?", 1)[0])
                server = host.servers.get(match.group(1)) if match else None
                if server is None:
                    self._reply(404, {"error": "unknown server"})
                return server

            def do_POST(self):
                server = self._server()
                if server is None:
                    return
                length = int(self.headers.get("Content-Length") or 0)
                try:
                    message = json.loads(self.rfile.read(length) or b"null")
                except ValueError:
                    self._reply(400, rpc_error(None, -32700, "Parse error"))
                    return
                if isinstance(message, list):
                    answers = [answer for answer in (server.handle(item) for item in message) if answer]
                    self._reply(200, answers) if answers else self._reply(202)
                    return
                answer = server.handle(message)
                self._reply(200, answer) if answer is not None else self._reply(202)

            def do_GET(self):
                if self._server() is not None:
                    self._reply(405, {"error": "no event stream: use POST"})

            def do_DELETE(self):
                if self._server() is not None:
                    self._reply(200, {})

        self.httpd = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        self.httpd.daemon_threads = True
        self.port = self.httpd.server_address[1]
        self.thread = threading.Thread(target=self.httpd.serve_forever, daemon=True)

    def start(self):
        self.thread.start()
        return self

    def stop(self):
        self.httpd.shutdown()
        self.httpd.server_close()

    def url(self, name):
        return f"http://127.0.0.1:{self.port}/mcp/{name}"

    def config(self):
        """La section `mcpServers` des réglages de Claw."""
        return {name: {"type": "http", "url": self.url(name),
                       "headers": {"Authorization": f"Bearer {self.secret}"}}
                for name in self.servers}


def serve_stdio(server, stdin=None, stdout=None):
    """Messages JSON-RPC, un par ligne, sur l'entrée et la sortie standard."""
    stdin, stdout = stdin or sys.stdin, stdout or sys.stdout
    for line in stdin:
        line = line.strip()
        if not line:
            continue
        try:
            message = json.loads(line)
        except ValueError:
            answer = rpc_error(None, -32700, "Parse error")
        else:
            answer = server.handle(message)
        if answer is not None:
            stdout.write(json.dumps(answer, ensure_ascii=False) + "\n")
            stdout.flush()


# --- Les trois serveurs -------------------------------------------------------------------------

def build(context):
    """Les serveurs `github`, `claw-remote` et `claw-docs` pour ce passage."""
    import mcp_docs
    import mcp_github
    import mcp_remote
    return [
        Server(mcp_github.SERVER, mcp_github.tools(context), mcp_github.INSTRUCTIONS),
        Server(mcp_remote.SERVER, mcp_remote.tools(context), mcp_remote.INSTRUCTIONS),
        Server(mcp_docs.SERVER, mcp_docs.tools(context), mcp_docs.INSTRUCTIONS),
    ]


SERVER_TITLES = {
    "github": ("GitHub", "Issues, pull requests, revues, branches, fichiers, Actions, versions et recherche, "
               "sur le dépôt de la session. Équivalent du serveur github de Claude Code."),
    "claw-remote": ("Sessions et routines", "Sessions de Claw Web, messages entre sessions, routines "
                    "programmées, suivi des pull requests, dépôts. Équivalent de claude-code-remote."),
    "claw-docs": ("Documents", "Documents à onglets et commentaires, rangés dans le dépôt des sessions. "
                  "Équivalent de Claude_Docs."),
}


_TITLES = {}


def tool_titles():
    """{(serveur, outil): titre} des trois serveurs, construit une fois."""
    if not _TITLES:
        for server in build(Context()):
            for tool in server.tools.values():
                _TITLES[(server.name, tool.name)] = tool.title
    return _TITLES


# Ce qui dit le mieux sur quoi porte un appel, dans l'ordre.
DETAIL_KEYS = ("issue_number", "pullNumber", "pull_number", "session_id", "trigger_id", "path", "query", "url",
               "name", "title", "branch", "tag", "ref")


def describe_call(name, data):
    """Un appel d'outil MCP de Claw Web en français (« GitHub : Lire une issue #12 ») ; None
    pour un autre outil."""
    found = re.fullmatch(r"mcp__([A-Za-z0-9_-]+?)__([A-Za-z0-9_-]+)", name or "")
    if not found or found.group(1) not in SERVER_TITLES:
        return None
    server, tool = found.groups()
    title = tool_titles().get((server, tool), tool)
    data = data if isinstance(data, dict) else {}
    detail = ""
    for key in DETAIL_KEYS:
        value = data.get(key)
        if value not in (None, "", [], {}):
            value = " ".join(str(value).split())
            detail = f" #{value}" if key in ("issue_number", "pullNumber", "pull_number", "session_id") else f" : {value}"
            break
    if len(detail) > 80:
        detail = detail[:79] + "…"
    return f"{SERVER_TITLES[server][0]} : {title}{detail}"


def catalog():
    """La liste des outils pour le panneau « Outils MCP » de l'appli."""
    servers = []
    for server in build(Context()):
        title, about = SERVER_TITLES[server.name]
        servers.append({
            "server": server.name,
            "title": title,
            "about": about,
            "tools": [{"name": tool.name, "title": tool.title, "read_only": tool.read_only,
                       **({"note": tool.note} if tool.note else {})} for tool in server.tools.values()],
        })
    return {"version": 1, "servers": servers}


# --- Claw Code sur votre machine ----------------------------------------------------------------

def local_token():
    for name in ("GITHUB_TOKEN", "GH_TOKEN"):
        if os.environ.get(name, "").strip():
            return os.environ[name].strip()
    try:
        found = subprocess.run(["gh", "auth", "token"], capture_output=True, text=True, timeout=10)
        return found.stdout.strip() if found.returncode == 0 else ""
    except (OSError, subprocess.TimeoutExpired):
        return ""


def local_repo():
    """`propriétaire/nom` du dépôt GitHub du dossier courant, ou « »."""
    try:
        url = subprocess.run(["git", "remote", "get-url", "origin"], capture_output=True, text=True,
                             timeout=10).stdout.strip()
    except (OSError, subprocess.TimeoutExpired):
        return ""
    found = re.search(r"github\.com[:/]([^/]+)/([^/]+?)(?:\.git)?/?$", url)
    return f"{found.group(1)}/{found.group(2)}" if found else ""


def git_output(*args):
    try:
        return subprocess.run(["git", *args], capture_output=True, text=True, timeout=10).stdout.strip()
    except (OSError, subprocess.TimeoutExpired):
        return ""


def local_context(repo="", engine=""):
    token = local_token()
    repo = repo or local_repo()
    branch = git_output("rev-parse", "--abbrev-ref", "HEAD")
    base = git_output("symbolic-ref", "--short", "refs/remotes/origin/HEAD").removeprefix("origin/")
    work = Path(git_output("rev-parse", "--show-toplevel") or ".")
    return Context(repo=repo, base=base or "main", branch=branch if branch != "HEAD" else "",
                   engine=engine or os.environ.get("CLAW_WEB_ENGINE", "").strip(), job_token=token,
                   repo_token=token, work=work, requests_file=None, local=True)


def catalog_module():
    """Le catalogue en module JavaScript (claw-web/js/mcp-tools.js) : la page ne joint que
    GitHub et OpenRouter (politique de sécurité du contenu), un module de l'appli se charge."""
    return ("// Outils MCP du moteur, pour la page « Outils MCP » de l'appli. Produit par\n"
            "// `python3 .github/claw-web/mcp_server.py --catalog > claw-web/js/mcp-tools.js` : ne pas modifier.\n"
            f"export default {json.dumps(catalog(), ensure_ascii=False, indent=1)};\n")


def main(argv):
    if argv[:1] == ["--catalog"]:
        sys.stdout.write(catalog_module())
        return 0
    if len(argv) >= 2 and argv[0] == "--stdio":
        options = dict(zip(argv[2::2], argv[3::2]))
        context = local_context(options.get("--repo", ""), options.get("--engine", ""))
        servers = {server.name: server for server in build(context)}
        if argv[1] not in servers:
            print(f"serveur inconnu : {argv[1]} (github, claw-remote, claw-docs)", file=sys.stderr)
            return 2
        serve_stdio(servers[argv[1]])
        return 0
    print(__doc__.strip(), file=sys.stderr)
    return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
