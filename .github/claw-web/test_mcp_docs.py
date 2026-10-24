"""Essais du serveur `claw-docs` (mcp_docs.py), de bout en bout sur une fausse API Contents de
GitHub gardée en mémoire.

    cd .github/claw-web && python -m unittest test_mcp_docs
"""

import base64
import hashlib
import io
import json
import os
import re
import unittest
import urllib.parse
import zipfile
from datetime import datetime, timezone
from unittest import mock

import mcp_docs
from common import GitHubError
from mcp_server import Context, Server, ToolError

ENGINE = "o/claw-sessions"
BRANCH = "claw-web-fichiers"


def refused(status, reason):
    return GitHubError(f"HTTP {status} {reason}", status, reason)


class FakeContents:
    """L'API Contents d'un dépôt : fichiers d'une branche, listes de dossiers, écritures avec
    vérification du sha (409 si périmé), effacements. `before_put` simule une autre session."""

    def __init__(self, branch_exists=True):
        self.files = {}  # chemin -> (octets, sha)
        self.branches = {"main"} | ({BRANCH} if branch_exists else set())
        self.calls = []
        self.before_put = None

    def __call__(self, method, path, token, body=None):
        self.calls.append((method, path))
        parsed = urllib.parse.urlsplit(path)
        query = dict(urllib.parse.parse_qsl(parsed.query))
        prefix = f"/repos/{ENGINE}"
        if not parsed.path.startswith(prefix):
            raise refused(404, "Not Found")
        rest = parsed.path[len(prefix):]
        if rest == "" and method == "GET":
            return {"full_name": ENGINE, "default_branch": "main"}
        if rest.startswith("/branches/") and method == "GET":
            if urllib.parse.unquote(rest[len("/branches/"):]) in self.branches:
                return {"name": BRANCH}
            raise refused(404, "Branch not found")
        if rest.startswith("/git/ref/heads/"):
            return {"object": {"sha": "base"}}
        if rest == "/git/refs" and method == "POST":
            self.branches.add(body["ref"].removeprefix("refs/heads/"))
            return {}
        if not rest.startswith("/contents/"):
            raise refused(404, "Not Found")
        name = urllib.parse.unquote(rest[len("/contents/"):])
        if method == "GET":
            if query.get("ref") not in self.branches:
                raise refused(404, "No such branch")
            if name in self.files:
                data, sha = self.files[name]
                return {"type": "file", "path": name, "sha": sha, "content": base64.encodebytes(data).decode()}
            entries = {}
            for other in self.files:
                if other.startswith(name.rstrip("/") + "/"):
                    head = other[len(name.rstrip("/")) + 1:].split("/", 1)
                    entries[head[0]] = "dir" if len(head) > 1 else "file"
            if entries:
                return [{"name": key, "path": f"{name.rstrip('/')}/{key}", "type": kind} for key, kind in sorted(entries.items())]
            raise refused(404, "Not Found")
        assert body["branch"] == BRANCH
        current = self.files.get(name)
        if method == "PUT":
            if self.before_put:
                self.before_put(name)
                current = self.files.get(name)
            if current and body.get("sha") != current[1]:
                raise refused(409, "is at X but expected Y")
            if current is None and body.get("sha"):
                raise refused(422, "sha does not match")
            if current and not body.get("sha"):
                raise refused(422, "\"sha\" wasn't supplied")
            data = base64.b64decode(body["content"])
            sha = hashlib.sha1(data + os.urandom(4)).hexdigest()
            self.files[name] = (data, sha)
            return {"content": {"sha": sha, "path": name}}
        if method == "DELETE":
            if not current or body.get("sha") != current[1]:
                raise refused(409, "sha mismatch")
            del self.files[name]
            return {}
        raise refused(405, "Method not allowed")

    # Aides des essais.

    def text(self, name):
        return self.files[name][0].decode("utf-8") if name in self.files else None

    def meta(self, doc_id):
        return json.loads(self.text(f"documents/{doc_id}/doc.json"))

    def writes(self):
        return sum(1 for method, _ in self.calls if method in ("PUT", "DELETE"))


def now():
    return datetime(2026, 10, 23, 9, 15, 2, tzinfo=timezone.utc)


SKELETON = {
    "kind": "project",
    "create": {"name": "Plan de lancement", "doc": {
        "blocks": {"asof": {"type": "date", "value": "2026-10-23"}, "me": {"type": "mention", "user": "me"},
                   "s1": {"type": "pending", "intent": "Objectifs : les trois résultats visés"},
                   "s2": {"type": "pending", "intent": "Risques : ce qui peut glisser"}},
        "markdown": "# Plan de lancement\n\n<?claude block asof?> · <?claude block me?>\n\n<?claude block s1?>\n\n<?claw block s2?>"}},
}


class DocsTestCase(unittest.TestCase):
    def setUp(self):
        self.fake = FakeContents()
        self.ctx = Context(engine=ENGINE, job_token="t", session="12", call=self.fake, now=now)
        self.tools = {tool.name: tool for tool in mcp_docs.tools(self.ctx)}
        patch = mock.patch.dict(os.environ, {"CLAW_WEB_URL": "https://example.test/app/"})
        patch.start()
        self.addCleanup(patch.stop)

    def call(self, name, **args):
        return self.tools[name].handler(args)

    def refused_with(self, code, name, **args):
        with self.assertRaises(ToolError) as caught:
            self.call(name, **args)
        self.assertTrue(str(caught.exception).startswith(f"[{code}]"), str(caught.exception))
        return str(caught.exception)

    def new_doc(self):
        ack = self.call("batch", container=SKELETON, batch=[])
        self.doc = ack["doc"]
        self.c = {"kind": "project", "id": self.doc}
        return ack

    def tab_text(self, tab="t1"):
        return self.fake.text(f"documents/{self.doc}/{tab}.md")


class CatalogTest(unittest.TestCase):
    def test_tools_with_an_empty_context(self):
        tools = mcp_docs.tools(Context())
        self.assertEqual([tool.name for tool in tools],
                         ["guide", "batch", "create", "read", "update", "delete", "export", "query"])
        for tool in tools:
            self.assertTrue(tool.title and tool.title[0].isupper(), tool.title)
            self.assertEqual(tool.schema["type"], "object")
        self.assertEqual({tool.name for tool in tools if tool.read_only}, {"guide", "read", "export", "query"})
        server = Server(mcp_docs.SERVER, tools, mcp_docs.INSTRUCTIONS)
        self.assertEqual(len(server.listing()), 8)
        self.assertIn("batch(", mcp_docs.INSTRUCTIONS)

    def test_no_engine_is_refused_but_the_guide_works(self):
        server = Server(mcp_docs.SERVER, mcp_docs.tools(Context()))
        result = server.call("read", {"ref": {"object": "project", "id": "doc-20261023-abcdef"}})
        self.assertTrue(result["isError"])
        self.assertIn("[no_engine]", result["content"][0]["text"])
        self.assertIn("CLAW_WEB_ENGINE", result["content"][0]["text"])
        guide = server.call("guide", {"items": ["topic.index", "refusal.last_tab", "topic.charts"]})
        text = guide["content"][0]["text"]
        self.assertFalse(guide.get("isError"))
        self.assertIn("# topic.index", text)
        self.assertIn("at least one tab", text)
        self.assertIn("Not available in claw-docs", text)

    def test_every_refusal_code_is_explained(self):
        with open(mcp_docs.__file__, encoding="utf-8") as handle:
            source = handle.read()
        codes = set(re.findall(r'refuse\("(\w+)"', source))
        self.assertTrue(codes <= set(mcp_docs.REFUSALS), codes - set(mcp_docs.REFUSALS))
        all_codes = mcp_docs.guide({"items": ["refusal.all"]})
        for code in mcp_docs.REFUSALS:
            self.assertIn(f"- {code}:", all_codes)


class SkeletonTest(DocsTestCase):
    def test_birth_then_fill_a_pending_section(self):
        ack = self.new_doc()
        self.assertRegex(self.doc, r"^doc-20261023-[0-9a-f]{6}$")
        self.assertEqual(ack["link"], f"https://example.test/app/#/document/{self.doc}")
        self.assertEqual(ack["tab"], "t1")
        self.assertEqual(ack["pending"], ["s1", "s2"])
        meta = self.fake.meta(self.doc)
        self.assertEqual(meta["title"], "Plan de lancement")
        self.assertEqual(meta["session"], "12")
        self.assertEqual(meta["created_at"], "2026-10-23T09:15:02Z")
        self.assertEqual(meta["tabs"], [{"id": "t1", "title": "Document", "file": "t1.md"}])
        self.assertEqual(meta["blocks"]["s1"], {"type": "pending", "intent": "Objectifs : les trois résultats visés", "tab": "t1"})
        self.assertEqual(meta["comments"], [])
        # Les puces sont rangées sous la forme `<?claw block …?>`.
        self.assertIn("<?claw block asof?> · <?claw block me?>", self.tab_text())
        self.assertNotIn("claude", self.tab_text())

        tab = self.call("read", ref={"object": "file", "id": "t1"}, container=self.c)
        self.assertEqual([entry["key"] for entry in tab["pending"]], ["s1", "s2"])
        self.assertEqual(tab["chips"]["asof"], {"type": "date", "value": "2026-10-23"})

        done = self.call("update", ref={"object": "node", "id": "s1"}, container=self.c,
                         payload="## Objectifs\n\n- Livrer le connecteur\n- 500 équipes actives")
        self.assertEqual(done["link"], ack["link"])
        self.assertEqual(done["pending_left"], ["s2"])
        text = self.tab_text()
        self.assertIn("· <?claw block me?>\n\n## Objectifs\n\n- Livrer le connecteur\n- 500 équipes actives\n\n<?claw block s2?>", text)
        self.assertNotIn("s1", self.fake.meta(self.doc)["blocks"])
        self.refused_with("not_found", "update", ref={"object": "node", "id": "s1"}, container=self.c, payload="x")

    def test_unknown_placeholder_and_block_type_are_refused_without_writing(self):
        bad = json.loads(json.dumps(SKELETON))
        bad["create"]["doc"]["markdown"] += "\n\n<?claw block nope?>"
        self.refused_with("invalid", "batch", container=bad, batch=[])
        bad = json.loads(json.dumps(SKELETON))
        bad["create"]["doc"]["blocks"]["s1"] = {"type": "chart"}
        self.refused_with("invalid", "batch", container=bad, batch=[])
        self.assertEqual(self.fake.writes(), 0)

    def test_batch_is_all_or_nothing(self):
        self.new_doc()
        writes = self.fake.writes()
        before = self.tab_text()
        message = self.refused_with("not_found", "batch", container=self.c, batch=[
            {"op": "update", "ref": {"object": "node", "id": "s1"}, "payload": "## Objectifs\n\nFait."},
            {"op": "update", "ref": {"object": "node", "id": "zz"}, "payload": "## Rien"}])
        self.assertIn("Batch member 2", message)
        self.assertIn("Nothing was written", message)
        self.assertEqual(self.fake.writes(), writes)
        self.assertEqual(self.tab_text(), before)

        ack = self.call("batch", container=self.c, batch=[
            {"op": "create", "object": "file", "$lid": "src", "payload": {"title": "Sources", "markdown": "# Sources"}},
            {"op": "update", "ref": {"object": "file", "id": "$lid:src"}, "payload": {"append": "- [Lien](https://a.b/c)"}},
            {"verb": "update", "ref": {"object": "node", "id": "s2"}, "payload": {"markdown": "## Risques\n\nÉchéance <?claw block due?>",
                                                                                  "blocks": {"due": {"type": "date", "value": "2026-11-02"}}}}])
        self.assertEqual(ack["members"][0]["id"], "t2")
        self.assertEqual(self.tab_text("t2"), "# Sources\n\n- [Lien](https://a.b/c)\n")
        meta = self.fake.meta(self.doc)
        self.assertEqual(meta["blocks"]["due"], {"type": "date", "value": "2026-11-02", "tab": "t1"})
        self.assertEqual(ack["pending"], ["s1"])

    def test_a_concurrent_change_is_replayed_on_the_new_version(self):
        self.new_doc()
        path = f"documents/{self.doc}/doc.json"
        state = {"done": False}

        def other_session(name):
            if name == path and not state["done"]:
                state["done"] = True
                meta = self.fake.meta(self.doc)
                meta["values"]["Statut"] = "écrit ailleurs"
                self.fake.files[path] = (json.dumps(meta).encode(), "other")

        self.fake.before_put = other_session
        self.call("update", ref={"object": "project", "id": self.doc}, payload={"title": "Nouveau titre"})
        meta = self.fake.meta(self.doc)
        self.assertEqual(meta["title"], "Nouveau titre")
        self.assertEqual(meta["values"]["Statut"], "écrit ailleurs")


class TabsTest(DocsTestCase):
    def test_create_read_rename_and_last_tab(self):
        self.new_doc()
        made = self.call("create", object="file", container=self.c,
                         payload={"title": "Sources", "markdown": "# Sources\n\n- un\n- deux\n- trois"})
        self.assertEqual(made["id"], "t2")
        project = self.call("read", ref={"object": "project", "id": f"https://x/#/document/{self.doc}"})
        self.assertEqual([tab["id"] for tab in project["tabs"]], ["t1", "t2"])
        self.assertEqual(project["tabs"][0]["pending"], ["s1", "s2"])
        window = self.call("read", ref={"object": "file", "id": "t2"}, container=self.c, payload={"offset": 3, "limit": 2})
        self.assertEqual(window["markdown"], "- un\n- deux")
        self.assertEqual(window["next_offset"], 5)
        self.call("update", ref={"object": "file", "id": "t2"}, container=self.c, payload={"title": "Données"})
        self.assertEqual(self.fake.meta(self.doc)["tabs"][1]["title"], "Données")
        self.call("update", ref={"object": "project", "id": self.doc}, payload={"order": ["t2"]})
        self.assertEqual([tab["id"] for tab in self.fake.meta(self.doc)["tabs"]], ["t2", "t1"])

        self.call("delete", ref={"object": "file", "id": "t2"}, container=self.c)
        self.assertIsNone(self.tab_text("t2"))
        self.assertEqual(len(self.fake.meta(self.doc)["tabs"]), 1)
        message = self.refused_with("last_tab", "delete", ref={"object": "file", "id": "t1"}, container=self.c)
        self.assertIn("update", message)
        self.assertIsNotNone(self.tab_text("t1"))

    def test_replace_append_and_their_refusals(self):
        self.new_doc()
        self.call("update", ref={"object": "file", "id": "t1"}, container=self.c,
                  payload="# Plan\n\n500 équipes, puis 500 équipes.\n\nFin.")
        self.refused_with("find_many", "update", ref={"object": "file", "id": "t1"}, container=self.c,
                          payload={"replace": {"old": "500 équipes", "new": "750 équipes"}})
        message = self.refused_with("find_none", "update", ref={"object": "file", "id": "t1"}, container=self.c,
                                    payload={"replace": {"old": "Fin!", "new": "x"}})
        self.assertIn("Closest line", message)
        self.call("update", ref={"object": "file", "id": "t1"}, container=self.c,
                  payload={"replace": {"old": "500 équipes", "new": "750 équipes", "nth": 2}, "append": "Annexe."})
        self.assertEqual(self.tab_text(), "# Plan\n\n500 équipes, puis 750 équipes.\n\nFin.\n\nAnnexe.\n")
        # La réécriture a retiré les puces que le texte n'appelle plus.
        self.assertEqual(self.fake.meta(self.doc)["blocks"], {})

    def test_plant_a_section_in_an_existing_doc_and_remove_a_block(self):
        self.new_doc()
        self.call("update", ref={"object": "node", "id": "s1"}, container=self.c, payload="## Objectifs\n\nTrois.")
        self.call("create", object="node", container=self.c,
                  payload={"tab": "t1", "key": "s3", "block": {"type": "pending", "intent": "Budget"}, "after": "Trois"})
        self.assertIn("Trois.\n\n<?claw block s3?>\n\n<?claw block s2?>", self.tab_text())
        self.refused_with("exists", "create", object="node", container=self.c,
                          payload={"tab": "t1", "key": "s3", "block": {"type": "pending"}})
        self.call("delete", ref={"object": "node", "id": "s2"}, container=self.c)
        self.assertTrue(self.tab_text().endswith("<?claw block s3?>\n"))
        self.assertNotIn("s2", self.fake.meta(self.doc)["blocks"])


class CommentsTest(DocsTestCase):
    def test_comment_reply_resolve_and_query(self):
        self.new_doc()
        self.call("update", ref={"object": "node", "id": "s1"}, container=self.c, payload="## Objectifs\n\nLivrer le connecteur.")
        first = self.call("create", object="utterance", container=self.c,
                          payload={"text": "Pour quand ?", "tab": "t1", "quote": "Livrer le connecteur"})
        self.assertEqual(first["id"], "c1")
        self.refused_with("find_none", "create", object="utterance", container=self.c,
                          payload={"text": "?", "tab": "t1", "quote": "absent"})
        reply = self.call("create", object="utterance", container=self.c, payload={"text": "Fin novembre.", "parent": "c1"})
        again = self.call("create", object="utterance", container=self.c, payload={"text": "Noté.", "parent": "c2"})
        self.assertEqual((reply["thread"], again["thread"]), ("c1", "c1"))
        comments = self.fake.meta(self.doc)["comments"]
        self.assertEqual(comments[0]["quote"], "Livrer le connecteur")
        self.assertEqual([comment["parent"] for comment in comments], [None, "c1", "c1"])
        self.assertEqual(comments[1]["author"], "claw")

        self.call("update", ref={"object": "utterance", "id": "c3"}, container=self.c, payload={"resolved": True})
        found = self.call("query", object="utterance", container=self.c)
        self.assertEqual((found["open"], found["resolved"]), (0, 1))
        self.assertEqual([reply["id"] for reply in found["threads"][0]["replies"]], ["c2", "c3"])
        self.assertEqual(self.call("query", container=self.c, payload={"after": "c2"})["threads"][0]["replies"][0]["id"], "c3")
        thread = self.call("read", ref={"object": "utterance", "id": "c2"}, container=self.c)["thread"]
        self.assertEqual(thread["id"], "c1")

    def test_a_persons_thread_and_answering(self):
        self.new_doc()
        path = f"documents/{self.doc}/doc.json"
        meta = self.fake.meta(self.doc)
        meta["comments"].append({"id": "c1", "parent": None, "author": "user", "text": "Ajoute une date.", "tab": "t1",
                                 "created_at": "2026-10-23T10:00:00Z", "resolved": False})
        self.fake.files[path] = (json.dumps(meta).encode(), "edited-in-app")
        self.refused_with("not_yours", "update", ref={"object": "utterance", "id": "c1"}, container=self.c, payload={"text": "x"})
        self.refused_with("resolve_refused", "update", ref={"object": "utterance", "id": "c1"}, container=self.c,
                          payload={"resolved": True})
        self.refused_with("not_found", "update", ref={"object": "node", "id": "s1"}, container=self.c,
                          payload="## Objectifs", answering="c9")
        done = self.call("update", ref={"object": "node", "id": "s1"}, container=self.c,
                         payload="## Objectifs\n\nAvant le 2 novembre.", answering="c1")
        comments = self.fake.meta(self.doc)["comments"]
        self.assertEqual(done["reply"], "c2")
        self.assertEqual(comments[1]["parent"], "c1")
        self.assertEqual(comments[1]["author"], "claw")
        self.assertIn("Objectifs", comments[1]["text"])
        self.call("update", ref={"object": "utterance", "id": "c1"}, container=self.c,
                  payload={"resolved": True, "resolveAskedIn": "chat"})
        self.assertTrue(self.fake.meta(self.doc)["comments"][0]["resolved"])


class ExportTest(DocsTestCase):
    BODY = ("## Résumé\n\nLe **lancement** est prévu, voir [la note](https://ex.am/ple).\n\n"
            "- un\n    - sous-point\n- [x] fait\n\n| Équipe | Charge |\n| --- | ---: |\n| Plateforme | 3 |\n\n"
            "```python\nprint('<script>')\n```\n\n> Citation à retenir.\n\nÉté <script>alert(1)</script> & co.")

    def setUp(self):
        super().setUp()
        self.new_doc()
        self.call("update", ref={"object": "node", "id": "s1"}, container=self.c, payload=self.BODY)

    def export(self, fmt, **extra):
        result = self.call("export", container=self.c, file="t1", format=fmt, **extra)
        data = base64.b64decode(result["base64"])
        self.assertEqual(result["bytes"], len(data))
        return result, data

    def test_markdown_text_and_html(self):
        result, data = self.export("markdown")
        markdown = data.decode()
        self.assertEqual(result["filename"], "plan-de-lancement.md")
        self.assertIn("2026-10-23 · @o", markdown)
        self.assertIn("*En cours : Risques : ce qui peut glisser*", markdown)
        self.assertNotIn("<?claw", markdown)
        self.assertEqual(self.export("notion")[1], data)

        text = self.export("text")[1].decode()
        self.assertIn("Le lancement est prévu, voir la note (https://ex.am/ple).", text)
        self.assertIn("• un\n  • sous-point\n[x] fait", text)
        self.assertNotIn("**", text)

        page = self.export("html", paper="letter")[1].decode()
        self.assertTrue(page.startswith("<!doctype html>"))
        self.assertIn("<title>Plan de lancement</title>", page)
        self.assertIn('<h2 id="résumé">Résumé</h2>', page)
        self.assertIn('<strong>lancement</strong>', page)
        self.assertIn('<a href="https://ex.am/ple">la note</a>', page)
        self.assertIn("<ul><li>un<ul><li>sous-point</li></ul></li>", page)
        self.assertIn('<th style="text-align:right">Charge</th>', page)
        self.assertIn("&lt;script&gt;alert(1)&lt;/script&gt; &amp; co.", page)
        self.assertNotIn("<script>", page)
        self.assertIn("@page{size:letter", page)

    def test_docx_is_a_valid_package(self):
        result, data = self.export("docx")
        self.assertEqual(result["mime"], mcp_docs.FORMATS["docx"][1])
        with zipfile.ZipFile(io.BytesIO(data)) as archive:
            self.assertIsNone(archive.testzip())
            names = archive.namelist()
            for name in ("[Content_Types].xml", "_rels/.rels", "word/document.xml", "word/styles.xml"):
                self.assertIn(name, names)
            document = archive.read("word/document.xml").decode()
            styles = archive.read("word/styles.xml").decode()
            relations = archive.read("word/_rels/document.xml.rels").decode()
        self.assertIn("Résumé", document)
        self.assertIn('<w:pStyle w:val="Heading2"/>', document)
        self.assertIn("• ", document)
        self.assertIn("Courier New", styles)
        self.assertIn('w:styleId="Heading3"', styles)
        self.assertIn("https://ex.am/ple", relations)
        self.assertIn("&lt;script&gt;", document)
        import xml.dom.minidom
        xml.dom.minidom.parseString(document)

    def test_pdf_structure_pages_and_accents(self):
        _, data = self.export("pdf")
        self.assertTrue(data.startswith(b"%PDF-1.4\n"))
        self.assertTrue(data.rstrip().endswith(b"%%EOF"))
        self.assertIn("Résumé".encode("cp1252"), data)
        self.assertIn(b"/Encoding /WinAnsiEncoding", data)
        self.assertIn(b"/MediaBox [0 0 595.28 841.89]", data)
        self.check_xref(data)
        # Un long texte : plusieurs pages, format lettre.
        self.call("update", ref={"object": "file", "id": "t1"}, container=self.c,
                  payload="# Long\n\n" + "\n\n".join("Paragraphe numéro %d, assez long pour remplir une ligne entière "
                                                     "de texte, voire deux ou trois." % n for n in range(120)))
        _, data = self.export("pdf", paper="letter")
        self.assertIn(b"/MediaBox [0 0 612 792]", data)
        count = int(re.search(rb"/Count (\d+)", data).group(1))
        self.assertGreater(count, 2)
        self.check_xref(data)

    def check_xref(self, data):
        start = int(re.search(rb"startxref\n(\d+)\n%%EOF", data).group(1))
        self.assertTrue(data[start:].startswith(b"xref\n"))
        lines = data[start:].split(b"\n")
        total = int(lines[1].split()[1])
        entries = lines[2:2 + total]
        self.assertEqual(entries[0], b"0000000000 65535 f ")
        for number, entry in enumerate(entries[1:], 1):
            self.assertEqual(len(entry) + 1, 20)
            offset = int(entry[:10])
            self.assertTrue(data[offset:].startswith(b"%d 0 obj" % number), number)
        for length, body in re.findall(rb"<< /Length (\d+) >>\nstream\n(.*?)\nendstream", data, re.S):
            self.assertEqual(int(length), len(body))

    def test_max_bytes_and_stored_exports(self):
        self.refused_with("too_large", "export", container=self.c, file="t1", format="pdf", maxBytes=100)
        self.refused_with("invalid", "export", container=self.c, file="t1", format="odt")
        stored = self.call("create", object="blob", container=self.c, payload={"from": {"object": "file", "id": "t1"}, "format": "pdf"})
        self.assertEqual(stored["id"], "b1")
        self.assertEqual(stored["path"], f"documents/{self.doc}/exports/b1.pdf")
        self.assertTrue(self.fake.files[stored["path"]][0].startswith(b"%PDF-1.4"))
        self.assertEqual(self.fake.meta(self.doc)["exports"][0]["format"], "pdf")
        blob = self.call("read", ref={"object": "blob", "id": "b1"}, container=self.c)
        self.assertTrue(base64.b64decode(blob["base64"]).startswith(b"%PDF"))

    def test_query_without_container_lists_documents_and_delete_removes_all(self):
        listed = self.call("query")
        self.assertEqual([entry["id"] for entry in listed["documents"]], [self.doc])
        self.call("create", object="blob", container=self.c, payload={"from": {"object": "file", "id": "t1"}, "format": "text"})
        gone = self.call("delete", ref={"object": "project", "id": self.doc})
        self.assertEqual(gone["files"], 3)
        self.assertFalse([name for name in self.fake.files if name.startswith(f"documents/{self.doc}")])
        self.refused_with("not_found", "read", ref={"object": "project", "id": self.doc})


class BranchTest(unittest.TestCase):
    def test_the_files_branch_is_created_when_missing(self):
        fake = FakeContents(branch_exists=False)
        ctx = Context(engine=ENGINE, job_token="t", call=fake, now=now)
        batch = {tool.name: tool for tool in mcp_docs.tools(ctx)}["batch"]
        ack = batch.handler({"container": SKELETON, "batch": []})
        self.assertIn(BRANCH, fake.branches)
        self.assertIn(f"documents/{ack['doc']}/t1.md", fake.files)


if __name__ == "__main__":
    unittest.main()
