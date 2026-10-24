"""Essais de JTest (jtest.py) : détection du projet, lancement réel d'un site local avec un faux
tunnel, arrêt, commentaire de la session, outil `jtest` et demande après la poussée.

    cd .github/claw-web && python -m unittest test_jtest
"""

import json
import os
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
                  "branch": "claw/session-3"}
        body = jtest.comment_body(online)
        self.assertIn("[ouvrir sur le téléphone](https://a-b.trycloudflare.com/)", body)
        self.assertEqual(jtest.parse_comment(body), online)
        self.assertEqual(common.kind_of(body), "engine")
        failed = jtest.comment_body({"state": "failed", "reason": "l'installation a échoué.", "log": "npm ERR! x"})
        self.assertIn("n'a pas pu démarrer : l'installation a échoué.", failed)
        self.assertIn("npm ERR! x", failed)
        terminal = jtest.comment_body({**online, "user": "jtest", "password": "p4ss"})
        self.assertIn("mot de passe `p4ss`", terminal)


class StartStopTest(unittest.TestCase):
    """Un vrai site statique lancé, joint par HTTP, avec un faux cloudflared qui donne une adresse."""

    def test_start_then_stop(self):
        home = Path(tempfile.mkdtemp())
        site = project({"index.html": "<h1>JTest</h1>"})
        fake = home / "fake-cloudflared"
        fake.write_text("#!/bin/sh\necho 'INF |  https://essai-jtest.trycloudflare.com  |' >&2\nexec sleep 600\n")
        fake.chmod(0o755)

        def clone(repo, branch, token, target):
            target.parent.mkdir(parents=True, exist_ok=True)
            os.system(f"cp -r {site} {target}")

        with mock.patch.object(jtest, "HOME", home), mock.patch.object(jtest, "clone", clone), \
                mock.patch.object(jtest, "tool", lambda name, url: fake):
            data = jtest.start("12", "o/app", "claw/session-3", {}, "t", now=NOW, hours=0.5)
            try:
                self.assertEqual(data["state"], "online")
                self.assertEqual(data["url"], "https://essai-jtest.trycloudflare.com/")
                self.assertEqual((data["label"], data["until"]), ("Site statique", "2026-10-24T09:30:00Z"))
                state = jtest.load_state("12")
                with urllib.request.urlopen(f"http://127.0.0.1:{state['port']}/", timeout=5) as response:
                    self.assertIn(b"JTest", response.read())
                self.assertEqual(len(jtest.live_sessions()), 1)
            finally:
                jtest.stop_local("12")
            time.sleep(0.3)
            self.assertFalse(any(jtest.alive(pgid) for pgid in state["pgids"]))
            self.assertEqual(jtest.live_sessions(), [])

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
                                                     "label": "Vite", "until_text": "13:00 UTC"}))
        self.assertEqual(self.call(action="status")["url"], "https://x.trycloudflare.com/")

    def test_claw_github_jtest(self):
        with mock.patch.object(claw_github, "state", lambda: {"branch": "claw/session-3", "base": "main"}):
            self.assertEqual(claw_github.main(["jtest", "python3", "app.py", "{port}"]), 0)
        self.assertEqual(on_github.load_requests(), {"jtest": {"command": "python3 app.py {port}"}})


if __name__ == "__main__":
    unittest.main()
