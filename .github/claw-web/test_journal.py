"""Journal d'un passage : les photos trouvées par l'agent deviennent des vignettes de l'appli,
les outils de design sont dits en quelques mots (journal.Journal, run.describe)."""

import json
import os
import unittest

os.environ.setdefault("GITHUB_REPOSITORY", "o/claw-sessions")
os.environ.setdefault("JOB_TOKEN", "t")
os.environ.setdefault("SESSION", "1")

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
        log = Journal()
        log.add({"type": "assistant", "message": {"content": [
            {"type": "tool_use", "name": "Icon", "input": {"names": ["wallet"]}}]}})
        log.add({"type": "user", "message": {"content": [
            {"type": "tool_result", "tool_name": "Icon", "content": json.dumps({"icons": [{"svg": "<svg/>"}]})}]}})
        # Le SVG est pour le modèle : le journal n'en garde rien.
        self.assertEqual((log.events[-1]["i"], log.events[-1]["r"]), ({"names": ["wallet"]}, {}))


if __name__ == "__main__":
    unittest.main()
