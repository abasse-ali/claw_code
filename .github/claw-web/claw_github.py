"""Commande `claw-github` de l'agent, sans aucun jeton : elle note ce que l'agent demande à Claw
de faire sur GitHub (fusionner la pull request de la session, publier le site sur GitHub
Pages) ; publish.py le fait après la poussée et en donne l'issue sous la réponse. Ses messages
s'adressent au modèle, en anglais.
"""

import json
import os
import sys
from pathlib import Path

from on_github import STATE_FILE, describe, load_requests, save_requests

REPO_DIR = Path(os.environ.get("CLAW_WEB_REPO_DIR") or ".")
USAGE = """usage: claw-github <command>
  status              the GitHub state of this repository (branch, GitHub Pages)
  pages [docs|root]   publish this session's branch on GitHub Pages once your work is pushed
  merge               merge this session's branch into its base: only when the user asks for it
  cancel              drop your requests"""


def state():
    try:
        return json.loads(STATE_FILE.read_text(encoding="utf-8"))
    except (OSError, ValueError):
        return None


def site_folder(choice):
    """Le dossier du site (`docs` ou `root`), ou None avec la raison."""
    found = {"docs": (REPO_DIR / "docs" / "index.html").is_file(), "root": (REPO_DIR / "index.html").is_file()}
    if choice:
        if choice not in found:
            return None, f"unknown folder {choice!r}: use docs or root."
        if not found[choice]:
            place = "docs/index.html" if choice == "docs" else "index.html at the repository root"
            return None, f"there is no {place}: GitHub Pages serves index.html. Create the site first."
        return choice, ""
    if found["docs"]:
        return "docs", ""
    if found["root"]:
        return "root", ""
    return None, "no index.html in docs/ or at the repository root: GitHub Pages serves index.html. Create the site first."


def main(argv):
    command = argv[0] if argv else "status"
    current = state()
    base = (current or {}).get("base") or "the base branch"
    branch = (current or {}).get("branch") or "this session's branch"
    requests = load_requests()
    if command in ("status", "-h", "--help", "help"):
        if command != "status":
            print(USAGE)
            return 0
        # Sans le préfixe des notes de Claw, que le journal de l'appli masque dans les sorties.
        print(describe(current).removeprefix("[Claw] ") if current else
              "GitHub state unavailable (no token for this repository).")
        if requests:
            print(f"Requested for the end of this task: {json.dumps(requests)}")
        return 0
    if command == "cancel":
        save_requests({})
        print("Requests dropped: Claw will neither merge nor publish.")
        return 0
    if command == "merge":
        requests["merge"] = True
        save_requests(requests)
        print(f"Recorded. Each session is an independent branch: merge only because the user asked to "
              f"bring {branch} into {base}. When you finish, Claw commits and pushes your work, then merges "
              f"{branch} into {base} (it opens the pull request first if needed). The outcome is added "
              "under your final answer: tell the user in one sentence that you merge it, without claiming "
              "it is done.")
        return 0
    if command == "pages":
        folder, problem = site_folder(argv[1] if len(argv) > 1 else "")
        if folder is None:
            print(f"Not recorded: {problem}")
            return 1
        requests["pages"] = folder
        save_requests(requests)
        where = "docs/" if folder == "docs" else "the repository root"
        source = base if requests.get("merge") else branch
        lines = [f"Recorded. When you finish, Claw pushes your work and publishes {where} of {source} on "
                 "GitHub Pages, and adds the site address, or the exact reason of a refusal, under your "
                 "final answer: tell the user in one sentence that you publish it, without claiming it "
                 "is online."]
        if current and current.get("private"):
            lines.append("The repository is private: on a free GitHub account Pages will refuse it, and "
                         "only the user can make it public. Claw will still try and report the outcome.")
        print("\n".join(lines))
        return 0
    print(USAGE, file=sys.stderr)
    return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
