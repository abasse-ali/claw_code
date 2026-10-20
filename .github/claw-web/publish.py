"""Publie le travail d'une tâche : commit, poussée sur la branche de la session, pull request,
puis réponse de l'agent dans la session.

Entrées (environnement) : REPO, BASE, SESSION, BRANCH, TITLE, PUBLISH (`true` si le jeton du
dépôt est là), REPO_TOKEN, JOB_TOKEN, CLAW_OUTCOME (issue de l'étape de l'agent).

Un passage inachevé (erreur, annulation) garde son travail sur la branche de la session, sans
ouvrir de pull request : un message de suivi le reprend.
"""

import json
import os
import subprocess

from common import (ANSWER, NOTES, WORK, GitHubError, api, clean_answer, clean_notice, explain_error,
                    french_shortstat, post_comment, run_url, stop_tree, usage_lines)
from on_github import Actor, load_requests

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
    """Un job annulé de force a pu laisser l'agent tourner, avec ses commandes : ils
    s'arrêtent avant le commit."""
    try:
        pid = int(read("claw.pid"))
    except ValueError:
        return

    def alive():
        try:
            os.kill(pid, 0)
        except (ProcessLookupError, PermissionError):
            return False
        return True

    if alive():
        stop_tree(pid, alive=alive)


def main():
    stop_leftover_agent()
    result, error = clean_answer(read("result.md")), read("error.txt")
    finished = OUTCOME == "success" and bool(result)
    footer, notes = [], []
    # Le travail est sur GitHub (poussé, ou rien à pousser) : les demandes de l'agent
    # (`claw-github`) peuvent être faites.
    pushed = False
    try:
        # Ce que l'agent a laissé sans commit (il a pu committer lui-même aussi). `.claw/`
        # est exclu par setup.py (.git/info/exclude) : nommé ici, git add le refuserait.
        if git("status", "--porcelain"):
            git("add", "-A")
            git("commit", "--quiet", "-m", f"Claw : {TITLE}")
        remote = f"origin/{BRANCH}" if git("rev-parse", "--verify", "--quiet", f"origin/{BRANCH}",
                                           check=False) else f"origin/{BASE}"
        ahead = int(git("rev-list", "--count", f"{remote}..HEAD") or "0")
        summary = french_shortstat(git("diff", "--shortstat", f"origin/{BASE}...HEAD", check=False))
        if ahead == 0:
            notes.append("Aucune modification de fichier dans ce passage.")
            pushed = True
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
                pushed = True
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
    requests = load_requests()
    if requests:
        if not PUBLISH:
            notes.append("Fusion ou mise en ligne non faite : le moteur n'a pas le secret `CLAW_REPO_TOKEN`.")
        elif not finished:
            notes.append("Fusion ou mise en ligne non faite : le passage ne s'est pas terminé.")
        elif not pushed:
            notes.append("Fusion ou mise en ligne non faite : les changements ne sont pas sur GitHub.")
        else:
            notes.extend(Actor(REPO, BASE, BRANCH, REPO_TOKEN, TITLE).carry_out(requests))

    if finished:
        text = result
    elif OUTCOME == "cancelled":
        text = "**La tâche a été arrêtée** (passage annulé)."
        if result:
            text += f"\n\n{result}"
    elif not error:
        # Claw s'est arrêté sans réponse finale, sans erreur : ses derniers avis disent pourquoi.
        text = ("**La tâche n'est pas finie** : Claw s'est arrêté sans réponse finale. Envoyez "
                "« continue » pour la reprendre.")
        # Sans redire « n'a pas conclu » : la phrase au-dessus le dit déjà.
        notices = [clean_notice(line) for line in read("notices.txt").splitlines()
                   if line.strip() and "n'a pas conclu" not in line]
        if notices:
            text += "\n\n" + "\n".join(f"> {line}" for line in notices)
    else:
        # L'erreur en clair d'abord ; le message brut, court, pour qui veut le détail.
        meaning = explain_error(error)
        text = (f"**La tâche s'est arrêtée** : {meaning}." if meaning
                else "**La tâche s'est arrêtée sur une erreur.**")
        text += (" Le travail déjà fait est gardé : envoyez « continue » pour reprendre, "
                 "ou choisissez un autre modèle.")
        if error:
            text += f"\n\nDétail technique :\n\n```\n{error[-600:]}\n```"
        if result:
            text += f"\n\n{result}"
    processed = read("answered.txt")
    parts = [ANSWER + (f"\n<!-- claw-web:answered {processed} -->" if processed.isdigit() else ""), text]
    if notes:
        # Remarques du moteur, marquées : setup.py les laisse hors de l'historique de l'agent.
        parts.append(NOTES + "\n" + "\n".join(f"> {note}" for note in notes))
    try:
        quota = json.loads(read("quota.json") or "null")
    except ValueError:
        quota = None
    # Modèle et requêtes du passage (run.py) : le pied suit le fournisseur du modèle.
    try:
        usage = json.loads(read("usage.json") or "null") or {}
    except ValueError:
        usage = {}
    footer.extend(usage_lines(usage.get("model", ""), usage.get("requests"), quota))
    footer.append(f"[Détail du job]({run_url()})")
    parts.append("---\n" + "\n".join(f"- {line}" for line in footer))
    try:
        post_comment(JOB_TOKEN, ENGINE, SESSION, "\n\n".join(parts))
    except GitHubError as failure:
        print(f"::error::réponse non publiée : {failure}")
        raise SystemExit(1)


if __name__ == "__main__":
    main()
