"""Prépare une tâche de l'appli web : vérifie les entrées, lit la session, clone le dépôt
de la tâche sur la branche de la session et écrit la demande pour l'agent.

Entrées (environnement) : REPO, BASE, SESSION, JOB_TOKEN, REPO_TOKEN (peut être vide),
GITHUB_REPOSITORY. Le jeton du dépôt ne reste pas dans le clone : l'adresse du dépôt est
remise sans lui avant que l'agent ne travaille. L'état du mode projet du passage précédent
(`claw-state/project.json`, en cache) est remis dans `.claw/` du clone.
"""

import json
import os
import re
import shutil
import subprocess
import sys
from pathlib import Path

import urllib.request

from common import (ANSWER, ANSWERED, API, FILES_BRANCH, NOTES, QUESTIONS, QUESTIONS_END, WORK, GitHubError, answer_marks,
                    answered_up_to, api, comments, images_of, kind_of, output, post_comment, run_url, settings,
                    task_text)
from on_github import STATE_FILE, describe, read_state
import memory

REPO = os.environ.get("REPO", "").strip()
BASE = os.environ.get("BASE", "").strip()
SESSION = os.environ.get("SESSION", "").strip()
ENGINE = os.environ["GITHUB_REPOSITORY"]
JOB_TOKEN = os.environ["JOB_TOKEN"]
REPO_TOKEN = os.environ.get("REPO_TOKEN", "").strip()
# Taille gardée de la conversation passée (les réponses les plus anciennes sont raccourcies).
HISTORY_MAX_CHARS = 30_000
# État du mode projet de la session, gardé en cache entre les passages (claw-web.yml).
STATE = Path("claw-state") / "project.json"


def load_memory():
    """Mémoire de Claw pour ce dépôt (memory.py), remise dans `.claw/` du clone : ses leçons et
    ce qu'il a déjà vu (erreurs, commandes qui marchent, dernières demandes). None si illisible."""
    try:
        data, _ = memory.load(JOB_TOKEN, ENGINE, REPO)
        lessons = memory.install(data, Path("work") / ".claw")
    except (GitHubError, OSError) as error:
        print(f"::warning::mémoire du projet non lue : {error}")
        return None
    if lessons or data["worked"] or data["errors"] or data["requests"]:
        print(f"Mémoire du projet : {len(lessons)} leçon(s), {len(data['errors'])} erreur(s) connue(s), "
              f"{len(data['worked'])} commande(s) qui marchent, {len(data['requests'])} demande(s) passée(s)")
    return {"repo": REPO, "lessons": lessons}


def keep_memory(seen, request):
    """Pour run.py : le dépôt, les leçons déjà là (les nouvelles s'y ajouteront) et la demande."""
    if seen is not None:
        (WORK / "memory.json").write_text(json.dumps({**seen, "request": request}, ensure_ascii=False),
                                          encoding="utf-8")


def fail(message):
    """Dit dans la session pourquoi la tâche ne peut pas commencer, puis arrête le job."""
    print(f"::error::{message}")
    if SESSION.isdigit():
        try:
            post_comment(JOB_TOKEN, ENGINE, SESSION,
                         f"{ANSWER}\n<!-- claw-web:answered keep -->\n**La tâche n'a pas pu commencer.** {message}\n\n[Détail du job]({run_url()})")
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


# Remarques du moteur sous les réponses d'avant leur marque NOTES : elles ne sont pas de
# Claw, qui les recopiait dans ses réponses.
ENGINE_NOTES = ("Aucune modification de fichier", "Travail inachevé gardé", "Modifications **non publiées**",
                "La poussée sur", "Publication impossible", "Fusion", "Mise en ligne non faite",
                "Rien à fusionner", "Pull request #", "Site mis en ligne")


def answer_text(body):
    """Le texte d'une réponse de Claw, sans le pied (après le dernier `---` : lien de la pull
    request, du job) ni les remarques du moteur ; ses questions y restent en texte."""
    text = ANSWERED.sub("", body.replace(ANSWER, "")).rsplit("\n---\n", 1)[0].split(NOTES, 1)[0]
    text = QUESTIONS.sub("", text).replace(QUESTIONS_END, "")
    lines = text.rstrip().splitlines()
    while lines and (not lines[-1].strip() or (lines[-1].startswith("> ") and lines[-1][2:].startswith(ENGINE_NOTES))):
        lines.pop()
    return "\n".join(lines).strip()


def conversation(issue, notes):
    """Le message à traiter (la tâche, ou les messages pas encore traités par une réponse,
    même arrivés pendant un passage), pour un suivi la conversation qui précède, les images
    jointes au message traité, l'identifiant du dernier message traité et s'il s'agit d'une
    relance.

    Relance (« Relancer » sans nouveau message) : une nouvelle réponse au dernier lot de
    messages traité, ou à la tâche. Elle porte la même marque que les réponses précédentes à
    ce lot, que l'appli montre en pages ; elles restent hors de l'historique."""
    task, task_images = images_of(task_text(issue.get("body")))
    marks = answer_marks(notes)
    answered = max((mark for _, mark in marks if mark is not None), default=0)
    fresh = any(kind_of(note.get("body")) == "message" and note["id"] > answered for note in notes)
    retry = not fresh and any(mark is not None for _, mark in marks)
    # Messages à traiter : ceux d'après la réponse au lot précédent pour une relance.
    since = max((mark for _, mark in marks if mark is not None and mark < answered), default=0) if retry else answered
    # Réponses gardées dans l'historique : ni les essais précédents d'une relance, ni les
    # « La tâche n'a pas pu commencer » (rien de Claw).
    kept = {note["id"] for note, mark in marks if mark is not None and not (retry and mark == answered)}
    exchanges, pending, images = [], [], []
    last = answered
    for note in notes:
        raw = (note.get("body") or "").strip()
        kind = kind_of(raw)
        if kind == "engine":
            continue
        body, note_images = images_of(raw)
        if kind == "answer":
            if note["id"] not in kept:
                continue
            exchanges.append(("Your answer", answer_text(body)))
        elif note["id"] > since:
            if body or note_images:
                pending.append(body)
                images.extend(note_images)
                last = max(last, note["id"])
        elif body:
            exchanges.append(("Message", body))
    if not pending:
        # Premier passage, ou relance de la tâche : la tâche elle-même (mode projet compris).
        if not exchanges:
            return task, "", task_images, last, retry
        pending = [task]
        images = task_images
    history = [("Request", task), *exchanges]
    text = ""
    for label, content in reversed(history):
        block = f"[{label}]\n{content}\n\n"
        if len(text) + len(block) > HISTORY_MAX_CHARS:
            block = f"[{label}]\n{content[:2000]}\n(...)\n\n"
            if len(text) + len(block) > HISTORY_MAX_CHARS:
                break
        text = block + text
    return "\n\n".join(pending), text.strip(), images, last, retry


# « Continue » et ses pareils, seuls dans le message.
BARE_CONTINUE = re.compile(
    r"^\s*(continue[rsz]?|continuons|on continue|vas[- ]y|go( on)?|reprends?|reprendre|poursuis|"
    r"poursuivre|la suite|suite|next|keep going|encore)\s*[.!?…]*\s*$",
    re.IGNORECASE,
)


def bare_continue_note(request, history, retry):
    """Note pour un « Continue » sans conversation qui précède (session neuve) : rien ne dit quoi
    continuer. Vu le 20 octobre 2026 (session 13) : l'agent s'est inventé une refonte du site,
    117 minutes et 161 actions que personne n'avait demandées."""
    if history or retry or not BARE_CONTINUE.match(request or ""):
        return ""
    return (
        "\n\n[Claw] This session has no earlier conversation: the user's message only says to go on, "
        "and nothing tells what to continue. Do not invent a task. Look at the repository without "
        "changing anything (README, PLAN.md or other notes, recent commits with git log), then answer "
        "in a few sentences: what the project is and what looks finished or unfinished. Then ask the "
        "user what to do next with AskUserQuestion (2 to 4 next steps, the one you recommend first): "
        "they answer with one tap. Change no file in this pass."
    )


def already_answered(notes):
    """Ce passage arrive après la réponse à ses messages : lancé deux fois (plusieurs appuis,
    appli rechargée), il attendait son tour pendant que le premier travaillait. Une relance
    voulue est lancée après la réponse, elle ; un message pas encore traité a son passage."""
    answers = [note for note in notes if kind_of(note.get("body")) == "answer"]
    if not answers:
        return False
    last = answers[-1]
    answered = answered_up_to(notes)
    if any(kind_of(note.get("body")) == "message" and note["id"] > answered for note in notes):
        return False
    try:
        run = api("GET", f"/repos/{ENGINE}/actions/runs/{os.environ.get('GITHUB_RUN_ID', '')}", JOB_TOKEN)
    except GitHubError:
        return False
    return bool(run and run.get("created_at") and run["created_at"] < last.get("created_at", ""))


def fetch_images(paths):
    """Télécharge les images jointes (branche FILES_BRANCH du moteur) hors du dépôt de la
    tâche ; leurs chemins sur le disque du job."""
    folder = WORK / "images"
    folder.mkdir(parents=True, exist_ok=True)
    saved = []
    for index, path in enumerate(paths[:10]):
        request = urllib.request.Request(f"{API}/repos/{ENGINE}/contents/{path}?ref={FILES_BRANCH}")
        request.add_header("Authorization", f"Bearer {JOB_TOKEN}")
        request.add_header("Accept", "application/vnd.github.raw")
        try:
            with urllib.request.urlopen(request, timeout=60) as response:
                data = response.read()
        except OSError as error:
            print(f"::warning::image {path} non téléchargée : {error}")
            continue
        target = folder / f"{index + 1}-{path.rsplit('/', 1)[-1]}"
        target.write_bytes(data)
        saved.append(target)
    return saved


def github_state(token, branch):
    """L'état GitHub du dépôt pour l'agent (pull request, GitHub Pages), gardé pour la commande
    `claw-github` ; None sans le jeton du dépôt (rien ne pourra être fait sur GitHub)."""
    if not REPO_TOKEN:
        return None
    try:
        state = read_state(REPO, BASE, branch, token)
    except (GitHubError, OSError, ValueError) as error:
        print(f"::warning::état GitHub du dépôt illisible : {error}")
        return None
    STATE_FILE.write_text(json.dumps(state), encoding="utf-8")
    print(describe(state))
    return state


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
    if already_answered(notes):
        print("Passage en double : ses messages ont déjà eu leur réponse, rien à faire.")
        output("skip", "true")
        return

    token = REPO_TOKEN or (JOB_TOKEN if REPO.lower() == ENGINE.lower() else "")
    if not token:
        fail("Le moteur n'a pas le secret `CLAW_REPO_TOKEN` (jeton qui peut lire et pousser dans "
             f"`{REPO}`). Ajoutez-le depuis les réglages de l'appli, ou dans Settings > Secrets "
             "and variables > Actions du dépôt du moteur.")
    # Branche notée par l'appli dans la session (claw/session-<numéro de l'appli>) ; les
    # anciennes sessions n'en ont pas : celle du numéro d'issue.
    branch = str(settings(issue.get("body")).get("branch") or "")
    if not re.fullmatch(r"claw/[A-Za-z0-9._/-]{1,80}", branch) or ".." in branch or subprocess.run(
            ["git", "check-ref-format", "--branch", branch], capture_output=True).returncode != 0:
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
        # Sessions de Claw et caches que laissent les tests : jamais dans le commit.
        with open("work/.git/info/exclude", "a", encoding="utf-8") as handle:
            handle.write("\n.claw/\n__pycache__/\n*.pyc\n.pytest_cache/\n.mypy_cache/\n"
                         ".ruff_cache/\nnode_modules/\n.venv/\n")
    except RuntimeError as error:
        fail(str(error))
    # Mode projet en cours, ou arrêté (« sans mode projet »), au passage précédent : le clone
    # est neuf et .claw n'est jamais poussé, l'état revient du cache.
    if STATE.is_file():
        (Path("work") / ".claw").mkdir(exist_ok=True)
        shutil.copyfile(STATE, Path("work") / ".claw" / "project.json")
        print("État du mode projet repris du passage précédent")
    memory_data = load_memory()

    # Les mots de l'utilisateur d'abord : Claw ne décide le mode projet que sur ce qui
    # précède la première ligne `[Claw]`.
    request, history, images, processed, retry = conversation(issue, notes)
    request += bare_continue_note(request, history, retry)
    # Pour la réponse : jusqu'où elle répond (les messages arrivés pendant le passage auront
    # le leur).
    (WORK / "answered.txt").write_text(str(processed), encoding="utf-8")
    # Titre du commit et de la pull request : le message traité, pas le titre de la session
    # (son premier message, parfois une simple question).
    title = next((line.strip() for line in request.splitlines() if line.strip()), "")
    keep_memory(memory_data, title)
    # Images jointes au message : Claw les joint à la demande (`@chemin`) et les fait décrire
    # si le modèle ne lit pas les images.
    saved = fetch_images(images)
    if saved:
        request += "\n\nImages jointes : " + " ".join(f"@{path}" for path in saved)
        print(f"{len(saved)} image(s) jointe(s)")
    if history:
        request += (
            "\n\n[Claw] Earlier conversation of this session, for context (your earlier changes "
            "are already on the branch):\n\n" + history
        )
    if retry:
        request += (
            "\n\n[Claw] The user asked for a new answer to this message: your earlier attempts at it "
            "are set aside and not shown, but the file changes they made, if any, are already on "
            "the branch. Answer it again in full, from the current state of the repository."
        )
    request += (
        f"\n\n[Claw] You are working in the repository {REPO}, on the branch {branch} "
        f"(created from {BASE}). When you finish, Claw commits your changes on this branch, "
        "pushes them and proposes them as a pull request (or updates the open one): do not "
        "commit, push or open the pull request yourself. The user cannot answer during this "
        "task, but AskUserQuestion shows them your questions with answers to tap: it ends this pass, "
        "and their answer starts the next one. Ask before doing the work when something essential is "
        "unclear (which page, which version, which of several readings of the request); for small "
        "choices, make a reasonable one and state it in your final answer."
    )
    state = github_state(token, branch)
    if state:
        request += "\n\n" + describe(state)
        request += (
            "\n\n[Claw] Acting on GitHub: you have no GitHub token, but the shell command "
            "`claw-github` has Claw act for you right after your work is pushed:\n"
            f"- `claw-github merge`: merge this session's pull request into {BASE} (opened first if needed).\n"
            f"- `claw-github pages` (or `pages docs`, `pages root`): publish the site on GitHub Pages from {BASE} "
            "(docs/ when docs/index.html exists, else the root). Add `claw-github merge` when the site's "
            "files are only on this session's branch.\n"
            "- `claw-github status`: the GitHub state above; `claw-github cancel`: drop your requests.\n"
            "When the user asks to merge, publish, deploy or put online (« mets en ligne », « fusionne »), "
            "run these commands yourself: never ask the user to press a button or to do it in GitHub. "
            "Claw writes the outcome (merged, site address, or the exact reason of a refusal) under your "
            "final answer: announce what you requested in one sentence, without claiming it is already "
            "done. Some things only the user can do (repository visibility, token permissions, other "
            "repositories): then name that one action, where to do it and why, in one sentence."
        )
    request += (
        "\n\n[Claw] Your final answer: answer the latest message first and directly. Do not repeat "
        "the tables, feature lists or summaries of your earlier answers in this session: say only what "
        "is new or changed. State facts about the repository, its branches, pull request or site only "
        "after checking them (the GitHub state above, git, the files, or fetching the page): never "
        "guess. When the user reports that something does not work, check it yourself first. "
        "The user reads your answer on their phone or computer, not on the machine where you work: "
        "give every address they may open (the site, a page of it, the pull request, a file on "
        "GitHub) as a full https link written as [what it is](https://...), never inside backticks. "
        "A local address (localhost, 127.0.0.1, 0.0.0.0) only exists on this machine, which stops "
        "with the task: never give it to the user to open. To show the site, give its GitHub Pages "
        "address from the GitHub state above, or publish it with `claw-github pages`."
    )
    (WORK / "prompt.txt").write_text(request, encoding="utf-8")
    output("branch", branch)
    output("title", (title or issue.get("title") or f"session {SESSION}").strip()[:100])
    # Sans jeton du dépôt, le travail est fait mais pas poussé (essai du moteur sur lui-même).
    output("publish", "true" if REPO_TOKEN else "false")
    print(f"Session {SESSION} : {REPO}, branche {branch} ({'existante' if existing else 'nouvelle'})")


if __name__ == "__main__":
    main()
