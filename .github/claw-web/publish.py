"""Publie le travail d'une tâche : commit, poussée sur la branche de la session, pull request,
puis réponse de l'agent dans la session.

Entrées (environnement) : REPO, BASE, SESSION, BRANCH, TITLE, PUBLISH (`true` si le jeton du
dépôt est là), REPO_TOKEN, JOB_TOKEN, CLAW_OUTCOME (issue de l'étape de l'agent).

Un passage inachevé (erreur, annulation) garde son travail sur la branche de la session, sans
ouvrir de pull request : un message de suivi le reprend.
"""

import os
import signal
import subprocess
import time

from common import ANSWER, WORK, GitHubError, api, post_comment, run_url

REPO = os.environ["REPO"]
BASE = os.environ["BASE"]
SESSION = os.environ["SESSION"]
BRANCH = os.environ["BRANCH"]
TITLE = os.environ.get("TITLE", "").strip() or f"session {SESSION}"
ENGINE = os.environ["GITHUB_REPOSITORY"]
JOB_TOKEN = os.environ["JOB_TOKEN"]
REPO_TOKEN = os.environ.get("REPO_TOKEN", "").strip()
PUBLISH = os.environ.get("PUBLISH") == "true" and bool(REPO_TOKEN)
OUTCOME = os.environ.get("CLAW_OUTCOME", "")


def git(*args, check=True):
    result = subprocess.run(["git", *args], cwd="work", capture_output=True, text=True,
                            encoding="utf-8", errors="replace")
    if check and result.returncode != 0:
        detail = (result.stderr or result.stdout).replace(REPO_TOKEN or "\0", "***")
        raise RuntimeError(f"git {args[0]} : {detail.strip()[:400]}")
    return result.stdout.strip()


def read(name):
    path = WORK / name
    return path.read_text(encoding="utf-8").strip() if path.exists() else ""


def stop_leftover_agent():
    """Un job annulé de force a pu laisser l'agent tourner : il s'arrête avant le commit."""
    try:
        pid = int(read("claw.pid"))
        os.kill(pid, signal.SIGTERM)
        for _ in range(50):
            time.sleep(0.1)
            os.kill(pid, 0)
        os.kill(pid, signal.SIGKILL)
    except (ValueError, ProcessLookupError, PermissionError):
        pass


def main():
    stop_leftover_agent()
    result, error = read("result.md"), read("error.txt")
    finished = OUTCOME == "success" and bool(result)
    footer, notes = [], []
    try:
        # Ce que l'agent a laissé sans commit (il a pu committer lui-même aussi). `.claw/`
        # est exclu par setup.py (.git/info/exclude) : nommé ici, git add le refuserait.
        if git("status", "--porcelain"):
            git("add", "-A")
            git("commit", "--quiet", "-m", f"Claw : {TITLE}")
        remote = f"origin/{BRANCH}" if git("rev-parse", "--verify", "--quiet", f"origin/{BRANCH}",
                                           check=False) else f"origin/{BASE}"
        ahead = int(git("rev-list", "--count", f"{remote}..HEAD") or "0")
        stat = git("diff", "--stat", f"origin/{BASE}...HEAD", check=False).splitlines()
        summary = stat[-1].strip() if stat else ""
        if ahead == 0:
            notes.append("Aucune modification de fichier dans ce passage.")
        elif not PUBLISH:
            notes.append(f"Modifications **non publiées** ({summary}) : le moteur n'a pas le secret "
                         f"`CLAW_REPO_TOKEN` qui permet de pousser dans `{REPO}`.")
        else:
            push = subprocess.run(
                ["git", "push", "--quiet", f"https://x-access-token:{REPO_TOKEN}@github.com/{REPO}.git",
                 f"HEAD:refs/heads/{BRANCH}"],
                cwd="work", capture_output=True, text=True,
            )
            if push.returncode != 0:
                detail = push.stderr.replace(REPO_TOKEN, "***").strip()[:300]
                notes.append(f"La poussée sur `{BRANCH}` a échoué : {detail}")
            else:
                owner = REPO.split("/")[0]
                found = api("GET", f"/repos/{REPO}/pulls?state=open&head={owner}:{BRANCH}", REPO_TOKEN)
                pull = found[0] if found else None
                if pull is None and finished:
                    pull = api("POST", f"/repos/{REPO}/pulls", REPO_TOKEN, {
                        "title": f"Claw : {TITLE}",
                        "head": BRANCH,
                        "base": BASE,
                        "body": "Proposé par Claw depuis l'appli web.\n\n" + (result[:3000] or TITLE),
                    })
                if pull is None:
                    notes.append(f"Travail inachevé gardé sur la branche `{BRANCH}`, sans pull request "
                                 "tant que la tâche n'est pas finie : envoyez un message pour la reprendre.")
                else:
                    footer.append(f"Pull request : {pull['html_url']}")
                footer.append(f"Branche `{BRANCH}` : {summary}")
    except (RuntimeError, GitHubError, ValueError) as failure:
        notes.append(f"Publication impossible : {failure}")

    if finished:
        text = result
    elif OUTCOME == "cancelled":
        text = "**La tâche a été arrêtée** (passage annulé)."
        if result:
            text += f"\n\n{result}"
    else:
        text = "**La tâche s'est arrêtée sur une erreur.**"
        if error:
            text += f"\n\n```\n{error[-1500:]}\n```"
        if result:
            text += f"\n\n{result}"
    parts = [ANSWER, text]
    if notes:
        parts.append("\n".join(f"> {note}" for note in notes))
    footer.append(f"[Détail du job]({run_url()})")
    parts.append("---\n" + "\n".join(f"- {line}" for line in footer))
    try:
        post_comment(JOB_TOKEN, ENGINE, SESSION, "\n\n".join(parts))
    except GitHubError as failure:
        print(f"::error::réponse non publiée : {failure}")
        raise SystemExit(1)


if __name__ == "__main__":
    main()
