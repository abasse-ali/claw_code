"""Aides de Claw choisies dans l'appli (page « Aides de Claw ») : de la marque du message aux
variables qui coupent les autres."""

import json
import os
import re
import tempfile
import unittest
from pathlib import Path

os.environ.setdefault("GITHUB_REPOSITORY", "o/claw-sessions")
os.environ.setdefault("JOB_TOKEN", "t")
os.environ.setdefault("SESSION", "1")

import common  # noqa: E402
import run  # noqa: E402

ROOT = Path(__file__).resolve().parents[2]


class AidsTest(unittest.TestCase):
    def test_the_mark_of_the_message_chooses_the_aids(self):
        self.assertEqual(common.aids_of("Corrige le bug"), ("Corrige le bug", None))
        self.assertEqual(common.aids_of("Corrige le bug\n\n<!-- claw-web:aides aucune -->"), ("Corrige le bug", []))
        text, aids = common.aids_of("Fais-le\n\n<!-- claw-web:aides stall_guard,verify -->")
        self.assertEqual(text, "Fais-le")
        # Dans l'ordre de la liste, quel que soit celui de la marque.
        self.assertEqual(aids, ["verify", "stall_guard"])
        # Le dernier choix compte ; une clé inconnue ne passe pas (rien d'autre qu'une aide).
        marks = "a <!-- claw-web:aides verify --> b <!-- claw-web:aides review,path,harness -->"
        self.assertEqual(common.aids_of(marks), ("a  b", ["review"]))

    def test_the_marks_never_reach_the_model(self):
        import setup
        block = '<!-- claw-web {"repo": "o/r"} -->'
        issue = {"body": f"Fais une appli de budget\n\n<!-- claw-web:aides aucune -->\n\n{block}"}
        answer = "<!-- claw-web:answer -->\n<!-- claw-web:answered 0 -->\n\nAppli faite.\n\n---\n- [Job](u)"
        notes = [{"id": 1, "body": "<!-- claw-web:progress -->\n**Claw a fini**"}, {"id": 2, "body": answer},
                 {"id": 10, "body": "Ajoute un export\n\n<!-- claw-web:effort high -->\n\n<!-- claw-web:aides verify -->"},
                 {"id": 11, "body": "Et un import\n\n<!-- claw-web:aides verify,review -->"}]
        request, history, options = setup.message_options(*setup.conversation(issue, notes)[:2])
        self.assertNotIn("claw-web:aides", request + history)
        self.assertNotIn("claw-web:effort", request + history)
        self.assertIn("Fais une appli de budget", history)
        # Deux messages envoyés pendant un passage : le choix du dernier compte.
        self.assertEqual(options, {"effort": "high", "aids": ["verify", "review"]})

    def test_aids_not_chosen_are_switched_off(self):
        env = {"PATH": "/usr/bin", "CLAW_PROJECT_MODE": "start", "CLAW_TEAM": "1"}
        run.apply_aids(env, [])
        self.assertEqual({key: value for key, value in env.items() if value == "0"},
                         {f"CLAW_{key.upper()}": "0" for key in common.AID_KEYS})
        # Pas CLAW_HARNESS : les options choisies à part (mode projet, équipe) restent.
        self.assertNotIn("CLAW_HARNESS", env)
        self.assertEqual((env["CLAW_PROJECT_MODE"], env["CLAW_TEAM"]), ("start", "1"))

        env = {"CLAW_VERIFY": "0"}
        run.apply_aids(env, ["verify", "review"])
        self.assertNotIn("CLAW_VERIFY", env)
        self.assertNotIn("CLAW_REVIEW", env)
        self.assertEqual(env["CLAW_STALL_GUARD"], "0")

        # Sans choix dans l'appli : toutes les aides de Claw, rien n'est touché.
        env = {"PATH": "/usr/bin"}
        run.apply_aids(env, None)
        self.assertEqual(env, {"PATH": "/usr/bin"})

    def test_an_aid_that_asks_another_model_gets_one(self):
        """Claw Web ne consulte aucun autre modèle (model_settings) ; le conseil ou l'escalade
        cochés dans l'appli en ont besoin pour faire quelque chose."""
        env = run.model_settings({})
        self.assertEqual((env["CLAW_ADVISOR_MODELS"], env["CLAW_ESCALATION_MODEL"]), ("off", "off"))
        run.apply_aids(env, ["checklist", "council"])
        self.assertNotIn("CLAW_ADVISOR_MODELS", env)
        self.assertEqual(env["CLAW_ESCALATION_MODEL"], "off")
        # La relecture à froid : le modèle de la session relit, aucun autre n'est consulté.
        env = run.model_settings({})
        run.apply_aids(env, ["review"])
        self.assertEqual(env["CLAW_ADVISOR_MODELS"], "off")
        env = run.model_settings({})
        run.apply_aids(env, ["council", "escalation"])
        self.assertNotIn("CLAW_ADVISOR_MODELS", env)
        self.assertNotIn("CLAW_ESCALATION_MODEL", env)
        # Sans elles, ou sans choix dans l'appli (toutes les aides), le réglage de Claw Web reste.
        for aids in (["checklist", "verify"], None):
            env = run.model_settings({})
            run.apply_aids(env, aids)
            self.assertEqual((env["CLAW_ADVISOR_MODELS"], env["CLAW_ESCALATION_MODEL"]), ("off", "off"))
        # Une liste de relecteurs réglée à part n'est pas touchée.
        env = run.model_settings({"CLAW_ADVISOR_MODELS": "a/b:free"})
        run.apply_aids(env, ["council"])
        self.assertEqual(env["CLAW_ADVISOR_MODELS"], "a/b:free")

    def test_the_choice_goes_from_setup_to_run_through_options(self):
        folder = Path(tempfile.mkdtemp())
        options = folder / "options.json"
        options.write_text(json.dumps({"effort": "", "aids": []}), encoding="utf-8")
        self.assertEqual(run.session_aids(options), [])
        chosen = {"effort": "", "aids": ["escalation", "verify", "CLAW_HARNESS"]}
        options.write_text(json.dumps(chosen), encoding="utf-8")
        self.assertEqual(run.session_aids(options), ["verify", "escalation"])
        options.write_text(json.dumps({"effort": "high", "aids": None}), encoding="utf-8")
        self.assertIsNone(run.session_aids(options))
        self.assertIsNone(run.session_aids(folder / "absent.json"))

    def test_the_answer_says_which_aids_the_model_had(self):
        self.assertEqual(common.aids_line(None), "")
        self.assertEqual(common.aids_line([]), f"Aides de Claw : aucune (les {len(common.AIDS)} coupées)")
        self.assertEqual(common.aids_line(["review", "verify"]), f"Aides de Claw : 2 sur {len(common.AIDS)} "
                         "(Vérification imposée, Relecture à froid)")

    def test_every_aid_is_one_of_claw(self):
        """Chaque clé coupe une vraie aide de Claw ; les seules laissées de côté ont leur raison."""
        harness = (ROOT / "rust/crates/runtime/src/harness.rs").read_text(encoding="utf-8")
        variables = set(re.findall(r'Self::\w+ => "(CLAW_[A-Z_]+)"', harness))
        ours = {common.aid_variable(key) for key in common.AID_KEYS}
        self.assertLessEqual(ours, variables)
        # Options à part dans l'appli ; VS Code et le mode plan n'existent pas dans un passage.
        self.assertEqual(variables - ours, {"CLAW_ULTRACODE", "CLAW_TEAM", "CLAW_PROJECT_MODE",
                                            "CLAW_EDITOR_DIAGNOSTICS", "CLAW_PLANNER"})
        self.assertEqual(len(set(common.AID_KEYS)), len(common.AID_KEYS))

    def test_the_app_lists_the_same_aids(self):
        source = (ROOT / "claw-web/js/aides.js").read_text(encoding="utf-8")
        listed = re.findall(r'\["([a-z_]+)", "([^"]+)", "', source)
        self.assertEqual(listed, list(common.AIDS))


if __name__ == "__main__":
    unittest.main()
