"""Clés et jetons que le moteur donne à Claw (run.agent_env)."""

import os
import unittest

os.environ.setdefault("GITHUB_REPOSITORY", "o/claw-sessions")
os.environ.setdefault("JOB_TOKEN", "t")
os.environ.setdefault("SESSION", "1")

import run  # noqa: E402

SECRETS = {
    "OPENROUTER_API_KEY": "sk-or-x",
    "NVIDIA_API_KEY": "nvapi-x",
    "GEMINI_API_KEY": "AIza-x",
    "JOB_TOKEN": "ghs-x",
    "REPO_TOKEN": "ghp-x",
    "GITHUB_TOKEN": "ghs-y",
    "PATH": "/usr/bin",
}


class AgentEnvTest(unittest.TestCase):
    def test_provider_keys_reach_claw_but_no_github_token(self):
        env = run.agent_env(SECRETS)
        self.assertEqual(env["OPENROUTER_API_KEY"], "sk-or-x")
        # Les secrets facultatifs des modèles `nvidia:` (et du lecteur d'images de NVIDIA) et `gemini:`.
        self.assertEqual(env["NVIDIA_API_KEY"], "nvapi-x")
        self.assertEqual(env["GEMINI_API_KEY"], "AIza-x")
        for token in ("JOB_TOKEN", "REPO_TOKEN", "GITHUB_TOKEN"):
            self.assertNotIn(token, env)
        self.assertEqual(env["PATH"], "/usr/bin")

    def test_claw_concludes_twelve_minutes_before_the_job_is_cut(self):
        # Job parti à 1 000 000 : coupé à 120 minutes, Claw conclut à 108.
        self.assertEqual(run.deadline({"CLAW_JOB_STARTED": "1000000"}), str(1_000_000 + 108 * 60))
        self.assertEqual(run.deadline({}), "")
        self.assertEqual(run.deadline({"CLAW_JOB_STARTED": "hier"}), "")

    def test_a_designer_conceives_and_the_session_model_builds_to_the_end(self):
        env = run.model_settings({})
        self.assertEqual(env["CLAW_DESIGNER_MODEL"].split(",")[0], "nvidia:z-ai/glm-5.3")
        self.assertNotIn("inkling", env["CLAW_DESIGNER_MODEL"])
        self.assertEqual(env["CLAW_ESCALATION_MODEL"], "off")
        self.assertEqual(env["CLAW_DESIGN_MODEL"], "off")
        # Plus de relecture ni d'avis d'un autre modèle ; la conception et le juge restent.
        self.assertEqual(env["CLAW_ADVISOR_MODELS"], "off")
        # Un réglage déjà là (variable du dépôt de sessions) l'emporte.
        env = run.model_settings({"CLAW_ESCALATION_MODEL": "free", "CLAW_DESIGNER_MODEL": "kimi"})
        self.assertEqual((env["CLAW_ESCALATION_MODEL"], env["CLAW_DESIGNER_MODEL"]), ("free", "kimi"))
        # Le faux modèle n'appelle aucun vrai modèle.
        self.assertEqual(run.model_settings({}, mock=True), {"CLAW_DESIGNER_MODEL": "off"})

    def test_the_mock_model_gets_no_real_key(self):
        env = run.agent_env(SECRETS, mock=True)
        self.assertNotIn("OPENROUTER_API_KEY", env)
        self.assertNotIn("NVIDIA_API_KEY", env)
        self.assertNotIn("GEMINI_API_KEY", env)
        self.assertNotIn("JOB_TOKEN", env)
        self.assertEqual(env["PATH"], "/usr/bin")

    def test_auto_sessions_go_to_nvidia_when_its_key_is_there(self):
        self.assertEqual(run.session_model({"MODEL": "", "NVIDIA_API_KEY": "nvapi-x"}), run.NVIDIA_DEFAULT)
        self.assertEqual(run.session_model({"MODEL": ""}), "")
        self.assertEqual(run.session_model({"MODEL": "", "NVIDIA_API_KEY": " "}), "")
        # Un modèle choisi dans l'appli reste le sien.
        self.assertEqual(run.session_model({"MODEL": "poolside/laguna-s-2.1:free", "NVIDIA_API_KEY": "nvapi-x"}),
                         "poolside/laguna-s-2.1:free")

    def test_the_reasoning_level_chosen_in_the_app_reaches_claw(self):
        import tempfile
        from pathlib import Path

        import common
        text, effort = common.effort_of("Corrige le bug\n\n<!-- claw-web:effort high -->")
        self.assertEqual((text, effort), ("Corrige le bug", "high"))
        # Le dernier choix compte ; un niveau inconnu ne passe pas.
        self.assertEqual(common.effort_of("a <!-- claw-web:effort low --> b <!-- claw-web:effort max -->")[1], "max")
        self.assertEqual(common.effort_of("a <!-- claw-web:effort turbo -->"), ("a", ""))
        folder = Path(tempfile.mkdtemp())
        (folder / "options.json").write_text('{"effort": "medium"}', encoding="utf-8")
        self.assertEqual(run.session_effort(folder / "options.json"), "medium")
        (folder / "options.json").write_text('{"effort": "rm -rf"}', encoding="utf-8")
        self.assertEqual(run.session_effort(folder / "options.json"), "")
        self.assertEqual(run.session_effort(folder / "absent.json"), "")

    def test_app_commands_become_requests(self):
        import common
        self.assertIn("JTest", common.web_command("/jtest"))
        self.assertTrue(common.web_command("/pages vite").endswith("\n\nvite"))
        self.assertEqual(common.web_command("/continue"), "continue")
        # Les commandes de Claw et les chemins restent le texte de l'utilisateur.
        self.assertIsNone(common.web_command("/review le dernier commit"))
        self.assertIsNone(common.web_command("/usr/bin/env est cassé"))
        self.assertIsNone(common.web_command("Corrige /jtest"))


if __name__ == "__main__":
    unittest.main()
