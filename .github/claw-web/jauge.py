"""Jauge du juge de design : des pages de référence notées par le juge visuel de Claw.

Lancé par le workflow claw-web.yml pour la session réservée `jauge-<n>` : `repo` et `base`
donnent le dépôt et la branche qui rangent les pages sous jauge/<nom>/index.html (avec, au
besoin, la demande de l'utilisateur dans jauge/<nom>/demande.txt), `model` le modèle de
travail dont le juge est un autre (vide : celui par défaut). Chaque page est notée par
`claw design-replay` comme dans une revue du tour (mêmes captures, même consigne), sans agent
ni session : une requête gratuite au juge par page. Le résultat va sur la branche des fichiers
du dépôt des sessions (jauge/<n>.json) et dans le résumé du job.

Sert à caler le juge : le kit d'appli seul et l'appli faite par Claude doivent atteindre la
barre. Vu le 28 octobre 2026 (essai réel #39) : quatre revues de 5,0 à 5,7 pour une appli
correcte, la barre à 8 hors d'atteinte, et le passage entier passé à suivre ses remarques.
"""

import base64
import json
import os
import subprocess
import sys
import time
from pathlib import Path

from common import FILES_BRANCH, GitHubError, api

# Une page : captures au téléphone, à l'ordinateur et des écrans ouverts, puis le juge.
PAGE_TIMEOUT = 900


def pages(root):
    """Les pages de la jauge : jauge/<nom>/index.html, dans l'ordre des noms."""
    return sorted((Path(root) / "jauge").glob("*/index.html"))


def grade(page, model="", run=subprocess.run):
    """La note du juge pour une page (dictionnaire de `claw design-replay`), ou son échec."""
    request = page.parent / "demande.txt"
    command = ["claw"] + (["--model", model] if model else []) + ["design-replay", str(page)]
    if request.is_file():
        command.append(str(request))
    name = page.parent.name
    try:
        done = run(command, capture_output=True, text=True, timeout=PAGE_TIMEOUT)
    except subprocess.TimeoutExpired:
        return {"page": name, "error": f"délai de {PAGE_TIMEOUT} s dépassé"}
    lines = [line for line in (done.stdout or "").splitlines() if line.startswith("{")]
    if done.returncode != 0 or not lines:
        return {"page": name, "error": ((done.stderr or "") + (done.stdout or "")).strip()[-600:]}
    result = json.loads(lines[-1])
    result["page"] = name
    return result


def summary(results):
    """Le tableau des notes, en Markdown, pour le résumé du job."""
    rows = ["| Page | Note | Barre | Critères | Défauts mesurés | Juge |", "|---|---|---|---|---|---|"]
    for result in results:
        if result.get("error"):
            rows.append(f"| {result['page']} | échec | | {result['error'][:120]} | | |")
            continue
        mean = result.get("mean")
        note = f"{mean:.1f}" if isinstance(mean, (int, float)) else "?"
        reached = "atteinte" if result.get("reached") else f"sous {result.get('bar', 8)}"
        criteria = " ".join(f"{name}={score}" for name, score in result.get("scores", []))
        rows.append(f"| {result['page']} | {note} | {reached} | {criteria} | {result.get('problems', 0)} "
                    f"| {result.get('judge', '')} |")
    return "\n".join(rows)


def store(engine, token, name, data):
    """Écrit jauge/<name>.json sur la branche des fichiers du dépôt des sessions."""
    path = f"jauge/{name}.json"
    try:
        api("GET", f"/repos/{engine}/branches/{FILES_BRANCH}", token)
    except GitHubError as error:
        if error.status != 404:
            raise
        default = api("GET", f"/repos/{engine}", token)["default_branch"]
        head = api("GET", f"/repos/{engine}/git/ref/heads/{default}", token)
        api("POST", f"/repos/{engine}/git/refs", token, {"ref": f"refs/heads/{FILES_BRANCH}",
                                                         "sha": head["object"]["sha"]})
    sha = None
    try:
        sha = api("GET", f"/repos/{engine}/contents/{path}?ref={FILES_BRANCH}", token).get("sha")
    except GitHubError as error:
        if error.status != 404:
            raise
    body = {"message": f"Jauge du juge de design ({name})", "branch": FILES_BRANCH,
            "content": base64.b64encode(json.dumps(data, ensure_ascii=False, indent=1).encode("utf-8")).decode()}
    if sha:
        body["sha"] = sha
    api("PUT", f"/repos/{engine}/contents/{path}", token, body)


def main():
    root = Path(os.environ.get("PAGES_DIR") or "pages")
    name = (os.environ.get("SESSION_INPUT") or "jauge").strip()
    model = (os.environ.get("MODEL") or "").strip()
    found = pages(root)
    if not found:
        print(f"Aucune page sous {root}/jauge/<nom>/index.html", file=sys.stderr)
        return 1
    started = time.time()
    results = []
    for page in found:
        print(f"{page.parent.name} : captures et note du juge…", flush=True)
        result = grade(page, model)
        results.append(result)
        print(f"  {result.get('mean', result.get('error'))}", flush=True)
    table = summary(results)
    print(table)
    step_summary = os.environ.get("GITHUB_STEP_SUMMARY")
    if step_summary:
        with open(step_summary, "a", encoding="utf-8") as out:
            out.write(f"## Jauge du juge de design ({name})\n\n{table}\n")
    engine = os.environ.get("GITHUB_REPOSITORY")
    token = os.environ.get("JOB_TOKEN")
    if engine and token:
        store(engine, token, name, {"name": name, "at": int(started), "seconds": round(time.time() - started),
                                    "results": results})
    return 0 if all(not result.get("error") for result in results) else 1


if __name__ == "__main__":
    sys.exit(main())
