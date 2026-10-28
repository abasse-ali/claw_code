"""Publie le travail d'une tâche : commit, poussée sur la branche de la session, puis réponse de
l'agent dans la session. Chaque session est une branche autonome (choix de l'utilisateur,
22 octobre 2026) : aucune pull request n'est ouverte, le travail vit sur sa branche, et le site
se publie depuis elle.

Entrées (environnement) : REPO, BASE, SESSION, BRANCH, TITLE, PUBLISH (`true` si le jeton du
dépôt est là), REPO_TOKEN, JOB_TOKEN, CLAW_OUTCOME (issue de l'étape de l'agent).

Un passage inachevé (erreur, annulation) garde son travail sur la branche de la session : un
message de suivi le reprend.
"""

import json
import os
import re
import subprocess

from common import (ANSWER, MCP_TOKEN_FILE, NOTES, WORK, GitHubError, api, clean_answer, clean_notice, explain_error,
                    french_shortstat, post_comment, questions_block, run_url, stop_tree, usage_lines)
from on_github import Actor, load_requests
from sessions import Sessions

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
        stop_tree(pid, alive=alive, session=True)


# Un utilisateur qui veut se servir du résultat sur son téléphone (« faut que ça marche sur
# mon tel »).
PHONE_WORDS = re.compile(r"\b(t[ée]l[ée]phone|t[ée]l|portable|mobile|iphone|android|smartphone)\b", re.I)
WEB_FILES = re.compile(r"(^|/)(index\.html?|package\.json)$", re.M)


def wants_phone_test(requests):
    """JTest à lancer sans que l'agent l'ait demandé : l'utilisateur parle de son téléphone, le
    passage a écrit un site ou une appli web, et la session tourne sur le serveur ARM où JTest
    tourne aussi. Vu le 24 octobre 2026 (session de test, « faut que ça marche sur mon tel ») :
    l'appli finie, aucun lien pour l'ouvrir sur le téléphone, l'agent arrêté à sa limite de
    requêtes sans avoir lancé JTest."""
    if "jtest" in requests or not PHONE_WORDS.search(read("user_words.txt")):
        return False
    if os.environ.get("RUNNER_ENVIRONMENT") != "self-hosted" or os.environ.get("RUNNER_ARCH") != "ARM64":
        return False
    return bool(WEB_FILES.search(git("diff", "--name-only", f"origin/{BASE}...HEAD", check=False)))


def launch_jtest(options):
    """JTest demandée par Claw (outil `jtest`, `claw-github jtest`) : la branche poussée est lancée
    sur le serveur, et le lien arrive dans la session."""
    try:
        Sessions(ENGINE, JOB_TOKEN, REPO_TOKEN).jtest(SESSION, REPO, BRANCH, options if isinstance(options, dict) else None)
    except GitHubError as error:
        return f"JTest non lancé : {error.reason or error}"
    return ("JTest lancé sur le serveur : le lien pour le téléphone arrive dans cette session d'ici une à "
            "trois minutes.")


def main():
    stop_leftover_agent()
    # Jeton des outils MCP : déjà effacé par run.py, sauf si l'agent n'a pas démarré.
    MCP_TOKEN_FILE.unlink(missing_ok=True)
    result, error = clean_answer(read("result.md")), read("error.txt")
    # Questions sur lesquelles Claw a fini le passage : l'utilisateur y répond d'un toucher.
    try:
        asked = questions_block(json.loads(read("questions.json") or "[]"))
    except ValueError:
        asked = ""
    finished = OUTCOME == "success" and bool(result or asked)
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
                if not finished:
                    notes.append(f"Travail inachevé gardé sur la branche `{BRANCH}` : envoyez un message "
                                 "pour la reprendre.")
                footer.append(f"Branche `{BRANCH}` : {summary}")
    except (RuntimeError, GitHubError, ValueError) as failure:
        notes.append(f"Publication impossible : {failure}")
    requests = load_requests()
    if requests.get("merge") or requests.get("pages"):
        if not PUBLISH:
            notes.append("Fusion ou mise en ligne non faite : le moteur n'a pas le secret `CLAW_REPO_TOKEN`.")
        elif not finished:
            notes.append("Fusion ou mise en ligne non faite : le passage ne s'est pas terminé.")
        elif not pushed:
            notes.append("Fusion ou mise en ligne non faite : les changements ne sont pas sur GitHub.")
        else:
            notes.extend(Actor(REPO, BASE, BRANCH, REPO_TOKEN, TITLE).carry_out(requests))
    if "jtest" in requests:
        if PUBLISH and finished and pushed:
            notes.append(launch_jtest(requests["jtest"]))
        else:
            notes.append("JTest non lancé : le travail de ce passage n'est pas sur GitHub.")
    elif PUBLISH and finished and pushed and wants_phone_test(requests):
        notes.append("Vous voulez l'utiliser sur votre téléphone : " + launch_jtest(None))

    if finished:
        text = "\n\n".join(part for part in (result, asked) if part)
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
