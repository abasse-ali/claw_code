"""Journal d'un passage : les photos trouvées par l'agent deviennent des vignettes de l'appli,
les outils de design sont dits en quelques mots, chaque requête au modèle a son heure
(journal.Journal, run.describe)."""

import base64
import gzip
import json
import os
import time
import unittest
from unittest import mock

os.environ.setdefault("GITHUB_REPOSITORY", "o/claw-sessions")
os.environ.setdefault("JOB_TOKEN", "t")
os.environ.setdefault("SESSION", "1")

import common  # noqa: E402
import journal  # noqa: E402
import run  # noqa: E402
from journal import Journal  # noqa: E402

JPEG = b"\xff\xd8\xff\xe0" + b"0" * 60


def photo_result(**photo):
    return json.dumps({"photos": [photo], "credits_file": "images/CREDITS.md", "note": "…"})


class JournalPhotosTest(unittest.TestCase):
    def journal_of(self, content, fetch, search="piggy bank savings"):
        log = Journal(fetch=fetch)
        log.add({"type": "assistant", "message": {"content": [
            {"type": "tool_use", "name": "Photo", "input": {"search": search}}]}})
        log.add({"type": "user", "message": {"content": [
            {"type": "tool_result", "tool_name": "Photo", "content": content}]}})
        return log.events[-1]

    def test_a_found_photo_becomes_a_thumbnail_kept_in_the_journal(self):
        asked = []

        def fetch(url):
            asked.append(url)
            return "data:image/jpeg;base64,AAAA"

        entry = self.journal_of(photo_result(
            path="images/piggy-bank-savings-1.jpg", title="Piggy bank", creator="Jo",
            license="CC BY 2.0", source="https://www.flickr.com/photos/1/2",
            web_thumbnail="https://live.staticflickr.com/1/2_ab_q.jpg",
            web_original="https://live.staticflickr.com/1/2_ab_b.jpg"), fetch)
        self.assertEqual(entry["i"], {"search": "piggy bank savings"})
        self.assertEqual(asked, ["https://live.staticflickr.com/1/2_ab_q.jpg"])
        shown = entry["r"]["photos"][0]
        self.assertEqual(shown["t"], "data:image/jpeg;base64,AAAA")
        self.assertEqual(shown["u"], "https://live.staticflickr.com/1/2_ab_b.jpg")
        self.assertEqual((shown["by"], shown["lic"], shown["src"]), ("Jo", "CC BY 2.0", "https://www.flickr.com/photos/1/2"))

    def test_unknown_or_unsafe_addresses_are_never_fetched(self):
        def fetch(url):
            raise AssertionError(f"téléchargé : {url}")

        for address in ("javascript:alert(1)", "http://live.staticflickr.com/x_q.jpg", "https://evil.example/x.jpg"):
            entry = self.journal_of(photo_result(web_thumbnail=address, web_original=address), fetch)
            self.assertEqual(entry["r"]["photos"], [], address)

    def test_a_thumbnail_that_does_not_come_leaves_no_empty_box(self):
        entry = self.journal_of(photo_result(
            web_thumbnail="https://upload.wikimedia.org/a/thumb/b/B.jpg/120px-B.jpg"), lambda url: "")
        self.assertEqual(entry["r"]["photos"], [])

    def test_thumbnails_of_one_search_fit_in_a_journal_part(self):
        many = json.dumps({"photos": [
            {"web_thumbnail": f"https://live.staticflickr.com/1/{index}_ab_q.jpg"} for index in range(4)]})
        entry = self.journal_of(many, lambda url: "data:image/jpeg;base64," + "A" * 15_000)
        # Deux vignettes de 15 000 caractères tiennent ; la troisième dépasserait.
        self.assertEqual(len(entry["r"]["photos"]), 2)

    def test_only_small_real_images_are_kept(self):
        self.assertEqual(journal.image_kind(JPEG), "image/jpeg")
        self.assertEqual(journal.image_kind(b"<html>"), "")
        self.assertEqual(journal.image_kind(b"RIFF\x00\x00\x00\x00WEBPVP8 "), "image/webp")

    def test_design_tools_are_named_in_a_few_words(self):
        self.assertEqual(run.describe("Photo", {"search": "notebook pen"}), "Photos : « notebook pen »")
        self.assertEqual(run.describe("Icon", {"names": ["wallet", "plus"]}), "Icônes : wallet, plus")
        self.assertEqual(run.describe("Ornament", {"kind": "lines"}), "Ornement : lines")
        self.assertEqual(run.describe("Component", {"names": ["form", "dialog"]}), "Composants : form, dialog")
        self.assertEqual(run.describe("AppIcon", {"svg": "icons/icon.svg"}), "Icônes de l'appli : icons/icon.svg")
        log = Journal()
        log.add({"type": "assistant", "message": {"content": [
            {"type": "tool_use", "name": "Icon", "input": {"names": ["wallet"]}}]}})
        log.add({"type": "user", "message": {"content": [
            {"type": "tool_result", "tool_name": "Icon", "content": json.dumps({"icons": [{"svg": "<svg/>"}]})}]}})
        # Le SVG est pour le modèle : le journal n'en garde rien.
        self.assertEqual((log.events[-1]["i"], log.events[-1]["r"]), ({"names": ["wallet"]}, {}))


def block(reply, part):
    """Un bloc d'une réponse du modèle, publié à part par Claw sous le numéro de la réponse."""
    return {"type": "assistant", "message": {"id": reply, "content": [part]}}


def text(reply, words):
    return block(reply, {"type": "text", "text": words})


def tool(reply, path="PLAN.md"):
    return block(reply, {"type": "tool_use", "name": "read_file", "input": {"path": path}})


RESULT = {"type": "user", "message": {"content": [{"type": "tool_result", "content": "…"}]}}


class JournalRequestsTest(unittest.TestCase):
    def test_each_model_reply_is_one_request_with_its_time(self):
        log = Journal()
        before = int(time.time())
        # Une réponse (texte et appel, publiés à part), puis une seconde : deux requêtes, et
        # non une par bloc (le compteur de l'appli se vidait deux à trois fois trop vite).
        log.add(block("reply-1", {"type": "thinking", "thinking": "Read the plan first."}))
        log.add(text("reply-1", "Je lis le plan."))
        log.add(tool("reply-1"))
        log.add(RESULT)
        log.add(text("reply-2", "Le plan est lu."))
        requests = [event for event in log.events if event["k"] == "req"]
        self.assertEqual(len(requests), 2)
        self.assertTrue(all(before <= event["at"] <= int(time.time()) for event in requests))
        # L'heure précède ce que dit la réponse : le reste du journal ne bouge pas.
        self.assertEqual([event["k"] for event in log.events], ["req", "think", "text", "tool", "req", "text"])

    def test_a_call_that_never_gets_a_result_does_not_shift_the_others(self):
        log = Journal()
        # Un appel annoncé sans résultat (vu le 25 octobre 2026) : la capture recevait le
        # contenu du fichier lu, et le fichier lu la réponse de la commande.
        log.add(block("reply-1", {"type": "tool_use", "name": "ExitPlanMode", "input": {}}))
        log.add(block("reply-2", {"type": "tool_use", "name": "Screenshot", "input": {"path": "index.html"}}))
        log.add(tool("reply-2", "styles.css"))
        log.add({"type": "user", "message": {"content": [{"type": "tool_result", "tool_name": "Screenshot",
                                                          "content": json.dumps({"captures": [], "problemsFound": 3})}]}})
        log.add({"type": "user", "message": {"content": [{"type": "tool_result", "tool_name": "read_file",
                                                          "content": json.dumps({"file": {"numLines": 12}})}]}})
        calls = {event["n"]: event.get("r") for event in log.events if event["k"] == "tool"}
        self.assertEqual(calls, {"ExitPlanMode": None, "Screenshot": {"shots": [], "found": 3},
                                 "read_file": {"count": 12}})
        # Un résultat sans nom d'outil termine le premier appel en attente, comme avant.
        log.add(RESULT)
        self.assertEqual(log.events[1].get("r"), {"out": "…"})

    def test_blocks_without_a_reply_number_are_each_a_request(self):
        log = Journal()
        log.add({"type": "assistant", "message": {"content": [{"type": "text", "text": "Je lis le plan."}]}})
        log.add({"type": "assistant", "message": {"content": [
            {"type": "tool_use", "name": "read_file", "input": {"path": "PLAN.md"}}]}})
        self.assertEqual([event["k"] for event in log.events], ["req", "text", "req", "tool"])


class JournalDraftsTest(unittest.TestCase):
    """Réponse que les contrôles de Claw lui font reprendre (vu le 20 octobre 2026, session 15 :
    deux « Résultat » presque identiques à la suite)."""

    def kinds(self, log):
        return [event["k"] for event in log.events if event["k"] not in ("req", "think")]

    def test_an_answer_sent_back_by_a_check_becomes_a_draft(self):
        log = Journal()
        log.add({"type": "system", "subtype": "init", "model": "nvidia:nemotron"})
        log.add(text("reply-1", "Je lance les tests."))
        log.add(tool("reply-1"))
        log.add(RESULT)
        log.add(text("reply-2", "**Résultat** : tout passe. Lancez `python app.py`."))
        log.add({"type": "system", "subtype": "notice",
                 "text": "La réponse conseille des commandes jamais lancées : Claw demande à l'agent de les essayer"})
        log.add({"type": "system", "subtype": "answer_superseded"})
        log.add(tool("reply-3", "app.py"))
        log.add(RESULT)
        log.add(text("reply-4", "**Résultat** : tout passe, `python app.py` aussi."))
        self.assertEqual(self.kinds(log), ["init", "text", "tool", "draft", "notice", "tool", "text"])
        # Le journal le dit : l'appli ne devine rien d'après le texte.
        self.assertTrue(log.events[0]["drafts"])
        self.assertTrue(log.dirty)

    def test_an_answer_kept_after_a_mere_acknowledgment_is_the_answer_again(self):
        log = Journal()
        log.add(text("reply-1", "**Résultat** : la page est corrigée."))
        log.add({"type": "system", "subtype": "answer_superseded"})
        log.add(text("reply-2", "Vérifié."))
        log.add({"type": "system", "subtype": "answer_kept"})
        self.assertEqual(self.kinds(log), ["text", "text"])
        # Un contrôle suivant fait reprendre les deux.
        log.add({"type": "system", "subtype": "answer_superseded"})
        self.assertEqual(self.kinds(log), ["draft", "draft"])

    def test_a_check_after_a_reply_with_tools_folds_nothing(self):
        # Une réponse qui appelle des outils n'est pas la réponse du tour : son annonce reste.
        log = Journal()
        log.add(text("reply-1", "Je lis le plan."))
        log.add(tool("reply-1"))
        log.add(RESULT)
        log.add({"type": "system", "subtype": "answer_superseded"})
        self.assertEqual(self.kinds(log), ["text", "tool"])


class SecretsTest(unittest.TestCase):
    """Ce que l'agent affiche (une commande `env`, un fichier lu) part dans la session sans les
    secrets du job : GitHub ne les masque que dans les journaux des jobs."""

    SECRET = "nvapi-0123456789abcdef"

    def test_the_journal_and_comments_never_carry_a_job_secret(self):
        environ = {"NVIDIA_API_KEY": self.SECRET, "REPO_TOKEN": "github_pat_11ABCDEFG0123456789",
                   "CLAW_MAX_TOKENS": "8000", "HOME": "/home/runner"}
        with mock.patch.dict(os.environ, environ):
            log = Journal()
            log.add(text("reply-1", f"Variables : NVIDIA_API_KEY={self.SECRET}"))
            body = log.bodies(1)[0][1]
            packed = body.split("<!-- claw-web:data ", 1)[1].rsplit("-->", 1)[0].strip()
            data = gzip.decompress(base64.b64decode(packed)).decode("utf-8")
            self.assertNotIn(self.SECRET, data)
            self.assertIn("NVIDIA_API_KEY=***", data)
            self.assertEqual(common.hide_secrets("jeton github_pat_11ABCDEFG0123456789, 8000 jetons, /home/runner"),
                             "jeton ***, 8000 jetons, /home/runner")
            sent = []
            with mock.patch.object(common, "api", lambda *args: sent.append(args) or {"id": 1}):
                common.post_comment("t", "o/s", 1, f"clé {self.SECRET}")
                common.edit_comment("t", "o/s", 1, f"clé {self.SECRET}")
            self.assertEqual([args[3]["body"] for args in sent], ["clé ***", "clé ***"])


if __name__ == "__main__":
    unittest.main()
