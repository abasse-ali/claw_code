"""Essais de JTest (jtest.py, jtest_gate.py) : détection du projet, lancement réel d'un site local
avec un faux tunnel derrière la porte à clé, arrêt, commentaire de la session, outil `jtest` et
demande après la poussée.

    cd .github/claw-web && python -m unittest test_jtest
"""

import asyncio
import http.client
import json
import os
import socket
import threading
import tempfile
import time
import unittest
import urllib.request
from datetime import datetime, timezone
from pathlib import Path
from unittest import mock

import claw_github
import common
import jtest
import jtest_gate
import mcp_remote
import on_github
from fakes import FakeGitHub
from mcp_server import Context, Server, ToolError

NOW = datetime(2026, 10, 24, 9, 0, tzinfo=timezone.utc)


def project(files):
    root = Path(tempfile.mkdtemp())
    for name, text in files.items():
        path = root / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text if isinstance(text, str) else json.dumps(text), encoding="utf-8")
    return root


class DetectTest(unittest.TestCase):
    def plan(self, files, requested=None):
        return jtest.detect(project(files), requested, port=4321)

    def test_web_frameworks(self):
        cases = [
            ({"package.json": {"devDependencies": {"vite": "7"}, "scripts": {"dev": "vite"}}, "package-lock.json": "{}"},
             "Vite", "npx vite --host 127.0.0.1 --port 4321 --strictPort", ["npm ci"]),
            ({"package.json": {"dependencies": {"next": "16"}}, "pnpm-lock.yaml": ""}, "Next.js",
             "npx next dev -p 4321 -H 127.0.0.1", ["npx -y pnpm@9 install"]),
            ({"package.json": {"scripts": {"start": "node server.js"}}}, "npm start", "npm start", ["npm install"]),
            ({"app.py": "from flask import Flask\napp = Flask(__name__)\n", "requirements.txt": "flask\n"}, "Flask",
             ".venv/bin/python -m flask --app app:app run --host 127.0.0.1 --port 4321", None),
            ({"main.py": "from fastapi import FastAPI\napi = FastAPI()\n", "requirements.txt": "fastapi\nuvicorn\n"},
             "FastAPI", ".venv/bin/python -m uvicorn main:api --host 127.0.0.1 --port 4321", None),
            ({"manage.py": "", "requirements.txt": "django"}, "Django", ".venv/bin/python manage.py runserver 127.0.0.1:4321", None),
            ({"app.py": "import streamlit as st\n"}, "Streamlit", None, None),
            ({"index.html": "<h1>Salut</h1>"}, "Site statique",
             "python3 -m http.server 4321 --bind 127.0.0.1 --directory .", []),
            ({"docs/index.html": "<h1>Salut</h1>"}, "Site statique (docs/)", None, []),
        ]
        for files, label, command, install in cases:
            plan = self.plan(files)
            self.assertEqual((plan.kind, plan.label), ("web", label), files)
            if command:
                self.assertEqual(plan.command, command)
            if install is not None:
                self.assertEqual(plan.install, install)

    def test_programs_open_in_a_terminal(self):
        plan = self.plan({"main.py": "print('bonjour')\n"})
        self.assertEqual((plan.kind, plan.command), ("terminal", ".venv/bin/python main.py"))
        self.assertEqual(self.plan({"Cargo.toml": "[package]"}).command, "cargo run --quiet")

    def test_subfolder_jtest_json_and_request(self):
        plan = self.plan({"frontend/package.json": {"devDependencies": {"vite": "7"}}, "README.md": "x"})
        self.assertEqual((plan.label, plan.cwd), ("Vite", "frontend"))
        plan = self.plan({"jtest.json": {"command": "bun run dev --port {port}", "path": "/admin"}, "index.html": ""})
        self.assertEqual((plan.command, plan.path, plan.label), ("bun run dev --port 4321", "/admin", "commande donnée"))
        plan = self.plan({"index.html": ""}, {"command": "python3 serve.py {port}", "install": "pip install x"})
        self.assertEqual((plan.command, plan.install), ("python3 serve.py 4321", ["pip install x"]))

    def test_a_static_site_in_any_folder(self):
        # Session #29 : le site d'un projet d'étudiant rangé dans `site-perso/`.
        plan = self.plan({"site-perso/index.html": "<h1>Salut</h1>", "site-perso/images/a.svg": "<svg/>",
                          "README.md": "x"})
        self.assertEqual((plan.kind, plan.label), ("web", "Site statique (site-perso/)"))
        self.assertEqual(plan.command, "python3 -m http.server 4321 --bind 127.0.0.1 --directory site-perso")
        # Les dépendances et les dossiers cachés ne sont pas un site.
        with self.assertRaises(jtest.JTestError):
            self.plan({"node_modules/x/index.html": "", ".claw/index.html": "", "README.md": "x"})
        # Session #30 : une page avec le package.json de `npm init` (« main » : son script).
        plan = self.plan({"package.json": {"main": "app.js", "scripts": {"test": "exit 1"}},
                          "index.html": "<h1>IronLog</h1>", "app.js": "document.title = 'x';"})
        self.assertEqual((plan.kind, plan.label), ("web", "Site statique"))

    def test_unknown_project(self):
        with self.assertRaises(jtest.JTestError) as caught:
            self.plan({"README.md": "rien"})
        self.assertIn("jtest.json", str(caught.exception))

    def test_programs_get_no_secret(self):
        env = jtest.clean_env({"PATH": "/bin", "JOB_TOKEN": "x", "REPO_TOKEN": "y", "GITHUB_TOKEN": "z",
                               "ACTIONS_RUNTIME_TOKEN": "a", "OPENROUTER_API_KEY": "k", "RUNNER_TRACKING_ID": "t1"})
        self.assertEqual(env, {"PATH": "/bin", "RUNNER_TRACKING_ID": ""})


class CommentTest(unittest.TestCase):
    def test_states_and_parsing(self):
        online = {"state": "online", "url": "https://a-b.trycloudflare.com/", "label": "Vite", "until_text": "13:00 UTC",
                  "branch": "claw/session-3", "key": "k" * 43}
        body = jtest.comment_body(online)
        # La clé n'est que dans les données de l'appli (commentaire HTML), pas dans le texte.
        visible = jtest.COMMENT.sub("", body)
        self.assertIn("accès réservé : ouvrez-le depuis Claw Web", visible)
        self.assertNotIn("k" * 43, visible)
        self.assertEqual(jtest.parse_comment(body), online)
        self.assertEqual(common.kind_of(body), "engine")
        failed = jtest.comment_body({"state": "failed", "reason": "l'installation a échoué.", "log": "npm ERR! x"})
        self.assertIn("n'a pas pu démarrer : l'installation a échoué.", failed)
        self.assertIn("npm ERR! x", failed)
        self.assertEqual(jtest.public_data({**online, "user": "jtest", "password": "p4ss"}),
                         {key: value for key, value in online.items() if key != "key"})


class StartStopTest(unittest.TestCase):
    """Un vrai site statique lancé, joint par HTTP, avec un faux cloudflared qui donne une adresse."""

    def test_start_then_stop(self):
        home = Path(tempfile.mkdtemp())
        site = project({"index.html": "<h1>JTest</h1>"})
        fake = home / "fake-cloudflared"
        fake.write_text("#!/bin/sh\necho \"args: $*\" >&2\necho 'INF |  https://essai-jtest.trycloudflare.com  |' >&2\n"
                        "echo 'INF Registered tunnel connection connIndex=0' >&2\nexec sleep 600\n")
        fake.chmod(0o755)

        def clone(repo, branch, token, target):
            target.parent.mkdir(parents=True, exist_ok=True)
            os.system(f"cp -r {site} {target}")

        # L'adresse publique répond « Accès réservé » (la porte, sans la clé).
        with mock.patch.object(jtest, "HOME", home), mock.patch.object(jtest, "clone", clone), \
                mock.patch.object(jtest, "tool", lambda name, url: fake), \
                mock.patch.object(jtest, "public_answer", lambda url, timeout=10: 403):
            data = jtest.start("12", "o/app", "claw/session-3", {}, "t", now=NOW, hours=0.5)
            try:
                self.assertEqual(data["state"], "online")
                self.assertEqual(data["url"], "https://essai-jtest.trycloudflare.com/")
                self.assertEqual((data["label"], data["until"]), ("Site statique", "2026-10-24T09:30:00Z"))
                self.assertGreaterEqual(len(data["key"]), 43)
                state = jtest.load_state("12")
                self.assertNotIn("key", state)
                config = home / "12" / "gate.json"
                self.assertEqual(config.stat().st_mode & 0o777, 0o600)
                # Le tunnel mène à la porte : sans la clé, accès réservé ; avec, le site.
                self.assertIn(f"--url http://127.0.0.1:{state['gate']} ", (home / "12" / "tunnel.log").read_text())
                status, headers, body = fetch(state["gate"], "/")
                self.assertEqual(status, 403)
                self.assertIn("Claw Web", body)
                status, headers, _ = fetch(state["gate"], f"/?claw_key={data['key']}")
                self.assertEqual((status, headers["location"]), (302, "/"))
                cookie = headers["set-cookie"].split(";")[0]
                status, _, body = fetch(state["gate"], "/", {"Cookie": cookie})
                self.assertEqual(status, 200)
                self.assertIn("JTest", body)
                with urllib.request.urlopen(f"http://127.0.0.1:{state['port']}/", timeout=5) as response:
                    self.assertIn(b"JTest", response.read())
                self.assertEqual(len(jtest.live_sessions()), 1)
            finally:
                jtest.stop_local("12")
            time.sleep(0.3)
            self.assertFalse(any(jtest.alive(pgid) for pgid in state["pgids"]))
            self.assertEqual(jtest.live_sessions(), [])

    def test_online_only_once_the_public_address_answers(self):
        # 25 octobre 2026 : « en ligne » annoncé, et « serveur introuvable » sur le téléphone.
        log = Path(tempfile.mkdtemp()) / "tunnel.log"
        log.write_text("INF |  https://essai.trycloudflare.com  |\n", encoding="utf-8")
        with mock.patch.object(jtest.time, "sleep", lambda seconds: None):
            unregistered = jtest.tunnel_ready("https://essai.trycloudflare.com/", log, timeout=0.05)
            self.assertIn("pas enregistré", unregistered)
            log.write_text("INF Registered tunnel connection connIndex=0\n", encoding="utf-8")
            with mock.patch.object(jtest, "public_answer", lambda url, timeout=10: "nom introuvable (-2)"):
                unknown = jtest.tunnel_ready("https://essai.trycloudflare.com/", log, timeout=0.05)
            self.assertIn("ne répond pas encore depuis Internet (nom introuvable", unknown)
            with mock.patch.object(jtest, "public_answer", lambda url, timeout=10: 403):
                self.assertIsNone(jtest.tunnel_ready("https://essai.trycloudflare.com/", log, timeout=5))

    def test_status_report_never_shows_the_key(self):
        home = Path(tempfile.mkdtemp())
        folder = home / "14"
        folder.mkdir(parents=True)
        (folder / "gate.log").write_text('127.0.0.1 - - "GET /?claw_key=SECRETE123 HTTP/1.1" 302 -\n', encoding="utf-8")
        (folder / "tunnel.log").write_text("INF Registered tunnel connection connIndex=0\n", encoding="utf-8")
        with mock.patch.object(jtest, "HOME", home), \
                mock.patch.object(jtest, "public_answer", lambda url, timeout=10: 403):
            jtest.save_state("14", {"session": "14", "pgids": [999999], "url": "https://essai.trycloudflare.com/",
                                    "until": "2026-10-25T14:52:00Z"})
            report = jtest.status_report("14")
        self.assertNotIn("SECRETE123", report)
        self.assertIn("- projet (groupe 999999) : arrêté", report)
        self.assertIn("- adresse publique : 403", report)
        self.assertIn("Registered tunnel connection", report)

    def test_a_server_that_never_answers_fails_clearly(self):
        home = Path(tempfile.mkdtemp())
        site = project({"jtest.json": {"command": "sleep 30", "port": 1}})

        def clone(repo, branch, token, target):
            os.system(f"mkdir -p {target.parent} && cp -r {site} {target}")

        with mock.patch.object(jtest, "HOME", home), mock.patch.object(jtest, "clone", clone), \
                mock.patch.object(jtest, "READY_TIMEOUT", 2), mock.patch.object(jtest, "wait_http",
                                                                               lambda port, pgid=None: False):
            with self.assertRaises(jtest.JTestError) as caught:
                jtest.start("13", "o/app", "main", {}, "t", now=NOW)
        self.assertIn("ne répond pas", str(caught.exception))


def fetch(port, target, headers=None, method="GET", body=None):
    """Une requête à la porte : (statut, en-têtes en minuscules, corps)."""
    connection = http.client.HTTPConnection("127.0.0.1", port, timeout=10)
    try:
        connection.request(method, target, body=body, headers=headers or {})
        response = connection.getresponse()
        return (response.status, {name.lower(): value for name, value in response.getheaders()},
                response.read().decode("utf-8", "replace"))
    finally:
        connection.close()


class GateTest(unittest.TestCase):
    """La porte devant le projet : rien sans la clé ; la clé devient un cookie ; requêtes,
    corps et WebSocket transmis sans la clé ; identifiants du terminal ajoutés par elle."""

    KEY = "c" * 43

    def setUp(self):
        self.seen = []
        self.upstream = socket.socket()
        self.upstream.bind(("127.0.0.1", 0))
        self.upstream.listen(8)
        threading.Thread(target=self.serve_upstream, daemon=True).start()
        self.port = jtest.free_port()
        config = {"listen": self.port, "upstream": self.upstream.getsockname()[1], "key": self.KEY, "hours": 1,
                  "basic": "jtest:secret"}
        self.loop = asyncio.new_event_loop()
        threading.Thread(target=self.loop.run_until_complete, args=(jtest_gate.serve(config),), daemon=True).start()
        self.assertTrue(jtest.wait_http(self.port, timeout=10))

    def serve_upstream(self):
        while True:
            try:
                connection, _ = self.upstream.accept()
            except OSError:
                return
            threading.Thread(target=self.answer, args=(connection,), daemon=True).start()

    def answer(self, connection):
        with connection:
            data = b""
            while b"\r\n\r\n" not in data:
                chunk = connection.recv(4096)
                if not chunk:
                    return
                data += chunk
            head, _, rest = data.partition(b"\r\n\r\n")
            self.seen.append(head.decode())
            if b"Upgrade: websocket" in head:
                connection.sendall(b"HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\r\n")
                echo = rest or connection.recv(4096)
                connection.sendall(b"echo:" + echo)
                return
            length = int(next((line.split(":")[1] for line in head.decode().split("\r\n")
                               if line.lower().startswith("content-length:")), "0"))
            while len(rest) < length:
                rest += connection.recv(4096)
            body = b"ok " + rest
            connection.sendall(b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nKeep-Alive: timeout=5\r\n"
                               + f"Content-Length: {len(body)}\r\n\r\n".encode() + body)

    def tearDown(self):
        self.upstream.close()
        self.loop.call_soon_threadsafe(self.loop.stop)

    def test_nothing_passes_without_the_key(self):
        for target, headers in (("/", {}), ("/app.js", {}), ("/?claw_key=faux", {}),
                                ("/", {"Cookie": "claw_jtest=faux"}), ("/", {"Cookie": f"autre={self.KEY}"})):
            status, _, body = fetch(self.port, target, headers)
            self.assertEqual(status, 403, target)
            self.assertIn("Accès réservé", body)
        self.assertEqual(self.seen, [])

    def test_the_key_becomes_a_cookie_and_leaves_the_address(self):
        status, headers, _ = fetch(self.port, f"/page?x=1&claw_key={self.KEY}&y=2")
        self.assertEqual((status, headers["location"]), (302, "/page?x=1&y=2"))
        cookie = headers["set-cookie"]
        for attribute in ("HttpOnly", "Secure", "SameSite=Lax", "Max-Age=3600"):
            self.assertIn(attribute, cookie)
        self.assertEqual(headers["cache-control"], "no-store")
        self.assertEqual(self.seen, [])

    def test_requests_pass_with_the_cookie_without_it_reaching_the_project(self):
        cookie = f"theme=sombre; claw_jtest={self.KEY}"
        status, headers, body = fetch(self.port, "/api", {"Cookie": cookie, "Content-Type": "application/json"},
                                      method="POST", body=b'{"a": 1}')
        self.assertEqual((status, body), (200, 'ok {"a": 1}'))
        self.assertEqual(headers["connection"], "close")
        self.assertNotIn("keep-alive", headers)
        head = self.seen[-1]
        self.assertTrue(head.startswith("POST /api HTTP/1.1"))
        self.assertNotIn(self.KEY, head)
        self.assertIn("Cookie: theme=sombre", head)
        self.assertIn("Connection: close", head)
        self.assertIn("Authorization: Basic anRlc3Q6c2VjcmV0", head)

    def test_a_websocket_passes_once_checked(self):
        with socket.create_connection(("127.0.0.1", self.port), timeout=10) as client:
            client.sendall(b"GET /ws HTTP/1.1\r\nHost: localhost\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n"
                           + f"Cookie: claw_jtest={self.KEY}\r\n\r\n".encode())
            answer = b""
            while b"\r\n\r\n" not in answer:
                answer += client.recv(4096)
            self.assertTrue(answer.startswith(b"HTTP/1.1 101"))
            client.sendall(b"bonjour")
            echo = answer.partition(b"\r\n\r\n")[2]
            while b"bonjour" not in echo:
                chunk = client.recv(4096)
                if not chunk:
                    break
                echo += chunk
            self.assertEqual(echo, b"echo:bonjour")
        self.assertIn("Upgrade: websocket", self.seen[-1])

    def test_decide(self):
        self.assertEqual(jtest_gate.decide("GET", "/", [], self.KEY), ("refuse", None))
        self.assertEqual(jtest_gate.decide("GET", f"/?claw_key={self.KEY}", [], self.KEY), ("login", "/"))
        self.assertEqual(jtest_gate.decide("GET", "/a", [("Cookie", f"claw_jtest={self.KEY}")], self.KEY),
                         ("pass", None))
        self.assertEqual(jtest_gate.decide("GET", "/a", [("Cookie", f"claw_jtest={self.KEY}x")], self.KEY),
                         ("refuse", None))


class PublicSessionsTest(unittest.TestCase):
    def test_a_public_sessions_repository_gets_no_jtest(self):
        fake = FakeGitHub()
        fake.add_session(7, "Appli", {"repo": "o/app", "base": "main", "n": 3, "branch": "claw/session-3"})
        fake.extra[("GET", f"/repos/{fake.engine}")] = {"full_name": fake.engine, "private": False}
        env = {"SESSION_INPUT": "jtest-7", "GITHUB_REPOSITORY": fake.engine, "REPO": "o/app",
               "BRANCH": "claw/session-3", "JOB_TOKEN": "j"}
        with mock.patch.dict(os.environ, env), mock.patch.object(common, "api", fake), \
                mock.patch.object(jtest, "start", side_effect=AssertionError("lancé")):
            self.assertEqual(jtest.main(), 1)
        posted = [body for method, path, body in fake.calls if method == "POST" and path.endswith("/comments")]
        self.assertEqual(len(posted), 1)
        self.assertIn("est public", posted[0]["body"])


class PhoneTest(unittest.TestCase):
    """« faut que ça marche sur mon tel » : l'appli web finie part en JTest sans que l'agent
    l'ait demandé, sur le serveur ARM ; rien pour un script, ni hors du serveur."""

    def test_a_web_app_for_the_phone_goes_to_jtest(self):
        import importlib

        env = {"REPO": "o/app", "BASE": "main", "SESSION": "7", "BRANCH": "claw/session-2",
               "GITHUB_REPOSITORY": "o/s", "JOB_TOKEN": "t", "RUNNER_ENVIRONMENT": "self-hosted",
               "RUNNER_ARCH": "ARM64"}
        with mock.patch.dict(os.environ, env):
            publish = importlib.import_module("publish")
            words = {"user_words.txt": "faut que ça marche sur mon tel. fais au mieux"}
            changed = "index.html\nstyle.css\napp.js"
            with mock.patch.object(publish, "read", lambda name: words.get(name, "")), \
                    mock.patch.object(publish, "git", lambda *args, **kwargs: changed):
                self.assertTrue(publish.wants_phone_test({}))
                # Déjà demandée par l'agent : pas une seconde fois.
                self.assertFalse(publish.wants_phone_test({"jtest": {}}))
                words["user_words.txt"] = "ajoute une fonction de tri au script"
                self.assertFalse(publish.wants_phone_test({}))
                words["user_words.txt"] = "je veux le voir sur mon iPhone"
                changed = "scraper.py"
                self.assertFalse(publish.wants_phone_test({}))
            with mock.patch.dict(os.environ, {"RUNNER_ENVIRONMENT": "github-hosted"}), \
                    mock.patch.object(publish, "read", lambda name: "sur mon téléphone"), \
                    mock.patch.object(publish, "git", lambda *args, **kwargs: "index.html"):
                self.assertFalse(publish.wants_phone_test({}))


class RequestTest(unittest.TestCase):
    def setUp(self):
        self.folder = Path(tempfile.mkdtemp())
        self.patch = mock.patch.object(on_github, "REQUESTS_FILE", self.folder / "requests.json")
        self.patch.start()
        self.fake = FakeGitHub()
        self.fake.add_session(7, "Appli", {"repo": "o/app", "base": "main", "n": 3, "branch": "claw/session-3"})
        self.ctx = Context(repo="o/app", base="main", branch="claw/session-3", session="7", engine=self.fake.engine,
                           job_token="j", repo_token="r", call=self.fake, now=lambda: NOW)

    def tearDown(self):
        self.patch.stop()

    def call(self, **args):
        result = Server("claw-remote", mcp_remote.tools(self.ctx)).call("jtest", args)
        if result.get("isError"):
            raise ToolError(result["content"][0]["text"])
        return json.loads(result["content"][0]["text"])

    def test_start_is_done_after_the_push_stop_and_other_branch_now(self):
        self.assertTrue(self.call(command="npm run dev -- --port {port}")["recorded"])
        self.assertEqual(on_github.load_requests(), {"jtest": {"command": "npm run dev -- --port {port}"}})
        self.assertEqual(self.fake.dispatches, [])
        self.call(action="stop")
        self.assertEqual(self.fake.dispatches[-1]["inputs"],
                         {"repo": "o/app", "base": "claw/session-3", "session": "jtest-stop-7", "model": ""})
        self.call(branch="claw/session-1", port=8080)
        self.assertEqual(self.fake.dispatches[-1]["inputs"]["session"], "jtest-7")
        self.assertEqual(json.loads(self.fake.dispatches[-1]["inputs"]["model"]), {"port": 8080})

    def test_status_reads_the_session(self):
        self.assertEqual(self.call(action="status")["state"], "none")
        self.fake.add_comment(7, jtest.comment_body({"state": "online", "url": "https://x.trycloudflare.com/",
                                                     "label": "Vite", "until_text": "13:00 UTC", "key": "s" * 43,
                                                     "user": "jtest", "password": "p4ss"}))
        status = self.call(action="status")
        self.assertEqual(status["url"], "https://x.trycloudflare.com/")
        # L'agent (et le fournisseur du modèle) ne voient jamais la clé ni les identifiants.
        self.assertNotIn("s" * 43, json.dumps(status))
        self.assertNotIn("p4ss", json.dumps(status))
        self.assertIn("Reserved to the user", status["access"])

    def test_claw_github_jtest(self):
        with mock.patch.object(claw_github, "state", lambda: {"branch": "claw/session-3", "base": "main"}):
            self.assertEqual(claw_github.main(["jtest", "python3", "app.py", "{port}"]), 0)
        self.assertEqual(on_github.load_requests(), {"jtest": {"command": "python3 app.py {port}"}})


if __name__ == "__main__":
    unittest.main()
