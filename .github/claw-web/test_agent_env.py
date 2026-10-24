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


if __name__ == "__main__":
    unittest.main()
