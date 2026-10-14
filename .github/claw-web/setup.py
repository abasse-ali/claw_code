"""Prépare une tâche de l'appli web : vérifie les entrées, lit la session, clone le dépôt
de la tâche sur la branche de la session et écrit la demande pour l'agent.

Entrées (environnement) : REPO, BASE, SESSION, JOB_TOKEN, REPO_TOKEN (peut être vide),
GITHUB_REPOSITORY. Le jeton du dépôt ne reste pas dans le clone : l'adresse du dépôt est
remise sans lui avant que l'agent ne travaille.
"""

import os
import re
import subprocess
import sys

from common import (ANSWER, PROGRESS, WORK, GitHubError, api, comments, output,
                    post_comment, run_url, task_text)

REPO = os.environ.get("REPO", "").strip()
BASE = os.environ.get("BASE", "").strip()
SESSION = os.environ.get("SESSION", "").strip()
ENGINE = os.environ["GITHUB_REPOSITORY"]
JOB_TOKEN = os.environ["JOB_TOKEN"]
REPO_TOKEN = os.environ.get("REPO_TOKEN", "").strip()
# Taille gardée de la conversation passée (les réponses les plus anciennes sont raccourcies).
HISTORY_MAX_CHARS = 30_000


def fail(message):
    """Dit dans la session pourquoi la tâche ne peut pas commencer, puis arrête le job."""
    print(f"::error::{message}")
    if SESSION.isdigit():
        try:
            post_comment(JOB_TOKEN, ENGINE, SESSION,
                         f"{ANSWER}\n**La tâche n'a pas pu commencer.** {message}\n\n[Détail du job]({run_url()})")
        except GitHubError as error:
            print(f"::warning::{error}")
    sys.exit(1)


def git(*args, cwd="work", check=True):
    result = subprocess.run(["git", *args], cwd=cwd, capture_output=True, text=True)
    if check and result.returncode != 0:
        # Jamais l'adresse avec le jeton dans les journaux.
        detail = (result.stderr or result.stdout).replace(REPO_TOKEN or "\0", "***").replace(JOB_TOKEN, "***")
        raise RuntimeError(f"git {args[0]} : {detail.strip()[:300]}")
    return result


def conversation(issue, notes):
    """La demande pour l'agent : la tâche, ou, pour un message de suivi, la conversation
    passée puis le nouveau message."""
    task = task_text(issue.get("body"))
    exchanges, pending = [], []
    for note in notes:
        body = (note.get("body") or "").strip()
        if PROGRESS in body:
            continue
        if ANSWER in body:
            if pending:
                exchanges.append(("Message", "\n\n".join(pending)))
                pending = []
            # Sans le pied de la réponse (lien de la pull request, du job), après le dernier `---`.
            exchanges.append(("Ta réponse", body.replace(ANSWER, "").rsplit("\n---\n", 1)[0].strip()))
        elif body:
            pending.append(body)
    if not pending:
        # Premier passage (ou relance sans nouveau message) : la tâche elle-même.
        if not exchanges:
            return task
        pending = ["Reprends la tâche là où elle en est et termine-la."]
    history = [("Demande", task), *exchanges]
    text = ""
    for label, content in reversed(history):
        block = f"[{label}]\n{content}\n\n"
        if len(text) + len(block) > HISTORY_MAX_CHARS:
            block = f"[{label}]\n{content[:2000]}\n(...)\n\n"
            if len(text) + len(block) > HISTORY_MAX_CHARS:
                break
        text = block + text
    return (
        "Conversation précédente de cette session (tes modifications précédentes sont déjà sur la branche) :\n\n"
        + text
        + "Nouveau message :\n" + "\n\n".join(pending)
    )


def main():
    if not re.fullmatch(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+", REPO):
        fail(f"Dépôt invalide : `{REPO}` (attendu : propriétaire/nom).")
    if not SESSION.isdigit():
        fail(f"Session invalide : `{SESSION}`.")
    if subprocess.run(["git", "check-ref-format", "--branch", BASE], capture_output=True).returncode != 0:
        fail(f"Branche de départ invalide : `{BASE}`.")
    try:
        issue = api("GET", f"/repos/{ENGINE}/issues/{SESSION}", JOB_TOKEN)
        notes = comments(JOB_TOKEN, ENGINE, SESSION)
    except GitHubError as error:
        fail(f"Session introuvable : {error}")

    token = REPO_TOKEN or (JOB_TOKEN if REPO.lower() == ENGINE.lower() else "")
    if not token:
        fail("Le moteur n'a pas le secret `CLAW_REPO_TOKEN` (jeton qui peut lire et pousser dans "
             f"`{REPO}`). Ajoutez-le depuis les réglages de l'appli, ou dans Settings > Secrets "
             "and variables > Actions du dépôt du moteur.")
    branch = f"claw/session-{SESSION}"
    WORK.mkdir(parents=True, exist_ok=True)
    clone = subprocess.run(
        ["git", "clone", "--quiet", "--no-tags", f"https://x-access-token:{token}@github.com/{REPO}.git", "work"],
        capture_output=True, text=True,
    )
    if clone.returncode != 0:
        fail(f"Impossible de cloner `{REPO}` : le dépôt n'existe pas ou le jeton n'y a pas accès.")
    try:
        existing = git("ls-remote", "--exit-code", "--heads", "origin", branch, check=False).returncode == 0
        if existing:
            git("checkout", "--quiet", "-B", branch, f"origin/{branch}")
        else:
            if git("rev-parse", "--verify", "--quiet", f"origin/{BASE}", check=False).returncode != 0:
                fail(f"La branche `{BASE}` n'existe pas dans `{REPO}`.")
            git("checkout", "--quiet", "-B", branch, f"origin/{BASE}")
        # Plus de jeton dans le clone : l'agent ne peut ni le lire ni pousser lui-même.
        git("remote", "set-url", "origin", f"https://github.com/{REPO}.git")
        git("config", "user.name", "Claw")
        git("config", "user.email", "claw@users.noreply.github.com")
        with open("work/.git/info/exclude", "a", encoding="utf-8") as handle:
            handle.write("\n.claw/\n")
    except RuntimeError as error:
        fail(str(error))

    request = conversation(issue, notes)
    request += (
        f"\n\n[Claw] You are working in the repository {REPO}, on the branch {branch} "
        f"(created from {BASE}). When you finish, Claw commits your changes on this branch, "
        "pushes them and proposes them as a pull request: do not commit, push or open the pull "
        "request yourself. Nobody can answer questions during this task: make reasonable "
        "choices and state them in your final answer."
    )
    (WORK / "prompt.txt").write_text(request, encoding="utf-8")
    output("branch", branch)
    output("title", (issue.get("title") or f"session {SESSION}").strip()[:120])
    # Sans jeton du dépôt, le travail est fait mais pas poussé (essai du moteur sur lui-même).
    output("publish", "true" if REPO_TOKEN else "false")
    print(f"Session {SESSION} : {REPO}, branche {branch} ({'existante' if existing else 'nouvelle'})")


if __name__ == "__main__":
    main()
