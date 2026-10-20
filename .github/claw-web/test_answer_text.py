"""Textes publiés dans la session, lisibles par l'utilisateur (common : bilan git en français,
signature retirée, avis sans réglage technique, erreurs dites en clair)."""

import unittest

import common


class AnswerTextTest(unittest.TestCase):
    def test_the_git_summary_is_in_french(self):
        # Vu en session : « 10 files changed, 1743 insertions(+), 287 deletions(-) ».
        self.assertEqual(
            common.french_shortstat(" 10 files changed, 1743 insertions(+), 287 deletions(-)\n"),
            "10 fichiers modifiés, +1 743 lignes, −287 lignes",
        )
        self.assertEqual(common.french_shortstat(" 1 file changed, 1 insertion(+)"),
                         "1 fichier modifié, +1 ligne")
        self.assertEqual(common.french_shortstat(" 2 files changed, 3 deletions(-)"),
                         "2 fichiers modifiés, −3 lignes")
        self.assertEqual(common.french_shortstat(""), "")

    def test_a_signature_line_is_removed_from_the_answer(self):
        answer = "**Résultat**\nLe site est plus lisible.\n\nAssisted-by: Claw Code\n"
        self.assertEqual(common.clean_answer(answer), "**Résultat**\nLe site est plus lisible.")
        # Au milieu du texte, la ligne reste : seule la signature finale part.
        kept = "Assisted-by: écrit dans un exemple\nSuite."
        self.assertEqual(common.clean_answer(kept), kept)

    def test_a_notice_does_not_name_the_technical_setting(self):
        notice = ("Ce tour a utilisé 200 requêtes, la limite d'un tour (réglage `turnRequestLimit`) : "
                  "Claw demande à l'agent de conclure.")
        self.assertEqual(common.clean_notice(notice),
                         "Ce tour a utilisé 200 requêtes, la limite d'un tour : Claw demande à l'agent de conclure.")

    def test_known_errors_are_said_plainly(self):
        self.assertEqual(common.explain_error("le modèle n'a renvoyé aucun contenu"),
                         "le modèle n'a rien répondu, plusieurs fois de suite")
        self.assertEqual(common.explain_error("HTTP 429 Too Many Requests"),
                         "le quota du modèle est épuisé pour le moment")
        self.assertIsNone(common.explain_error("erreur inconnue"))


if __name__ == "__main__":
    unittest.main()
