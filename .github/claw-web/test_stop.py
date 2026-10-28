"""Arrêt d'un passage (common.stop_tree) : l'agent et toutes les commandes qu'il a lancées."""

import os
import subprocess
import sys
import time
import unittest

from common import session_members, stop_tree


def gone(pid):
    """Le processus n'existe plus (ou n'est plus qu'un zombie que personne n'a encore relevé)."""
    try:
        with open(f"/proc/{pid}/stat", encoding="utf-8") as handle:
            return handle.read().rsplit(")", 1)[1].split()[0] == "Z"
    except OSError:
        return True


@unittest.skipUnless(sys.platform.startswith("linux"), "lit /proc")
class StopTest(unittest.TestCase):
    def orphan(self):
        """Comme `python3 -m http.server 8080 &` lancé par l'agent : le shell qui l'a lancé
        meurt, la commande reste, sans parent dans l'arbre de l'agent."""
        shell = subprocess.Popen(["sh", "-c", "sleep 30 & echo $!"], stdout=subprocess.PIPE, text=True,
                                 start_new_session=True)
        background = int(shell.stdout.readline())
        shell.stdout.close()
        shell.wait()
        self.addCleanup(lambda: gone(background) or os.kill(background, 9))
        return shell.pid, background

    def test_a_background_command_left_by_the_agent_is_stopped_with_it(self):
        leader, background = self.orphan()
        self.assertIn(background, session_members(leader))
        stop_tree(leader, alive=lambda: False, grace=0, session=True)
        deadline = time.time() + 3
        while not gone(background) and time.time() < deadline:
            time.sleep(0.05)
        self.assertTrue(gone(background))

    def test_the_tree_alone_misses_it(self):
        leader, background = self.orphan()
        stop_tree(leader, alive=lambda: False, grace=0)
        time.sleep(0.2)
        self.assertFalse(gone(background))


if __name__ == "__main__":
    unittest.main()
