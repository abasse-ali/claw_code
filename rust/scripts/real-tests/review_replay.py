"""Rejoue la relecture par un autre modèle sur un essai déjà fait.

Usage :
    python review_replay.py DOSSIER_DE_L_ESSAI [--model MODELE_DU_TRAVAIL] [--baseline COMMIT]
                            [--reviewer MODELE_GRATUIT]

Reprend la demande, la réponse finale (dernière session de l'essai) et le diff du projet
(l'essai est un dépôt Git : tout ce qui a changé depuis le commit de départ), et demande à
`claw review-replay` la relecture que Claw en ferait aujourd'hui. Une requête gratuite, aucun
outil. Avec --baseline, la relecture est rejouée avec les consignes de ce commit-là (dans
leur ordre d'alors : la réponse de l'agent avant le diff), pour comparer deux formulations
sur le même travail.

Le résultat (relecteur, réponse entière, remarques retenues) s'ajoute à
DOSSIER_DE_L_ESSAI/review-replay.jsonl.
"""

import argparse
import json
import re
import subprocess
import sys
from pathlib import Path

import run

# Fichiers que l'essai change sans que le modèle les ait écrits (données remises en place
# par la vérification cachée, caches).
NOT_THE_WORK = (":!donnees.json", ":!*.pyc", ":!.claw")


def last_answer(project):
    stats = run.session_stats(project)
    if not stats.get("session"):
        sys.exit("aucune session dans cet essai")
    messages = json.loads(Path(stats["session"]).read_text(encoding="utf-8"))["messages"]
    request = next(
        block["text"]
        for message in messages if message.get("role") == "user"
        for block in message.get("blocks", [])[:1] if block.get("type") == "text"
    )
    for message in reversed(messages):
        if message.get("role") != "assistant":
            continue
        text = "\n".join(
            block.get("text", "") for block in message.get("blocks", []) if block.get("type") == "text"
        ).strip()
        if text:
            return request, text
    return request, ""


def baseline_system(commit):
    """Consignes du relecteur telles qu'elles étaient à ce commit."""
    source = subprocess.run(
        ["git", "show", f"{commit}:rust/crates/runtime/src/harness.rs"],
        cwd=run.RUST, capture_output=True, encoding="utf-8", check=True,
    ).stdout
    match = re.search(r'pub const REVIEW_SYSTEM: &str = "((?:[^"\\]|\\.)*)";', source)
    if not match:
        sys.exit(f"REVIEW_SYSTEM introuvable au commit {commit}")
    return match.group(1).replace("\\n", "\n").replace('\\"', '"')


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("run_dir")
    parser.add_argument("--model", help="modèle qui a fait le travail (le relecteur en est un autre)")
    parser.add_argument("--baseline", help="commit dont on rejoue les consignes de relecture")
    parser.add_argument("--reviewer", help="relecteur imposé (un modèle gratuit), par le réglage advisorModels")
    parser.add_argument("--claw")
    args = parser.parse_args()
    if args.reviewer and not args.reviewer.endswith(":free"):
        sys.exit("relecteur : un modèle gratuit seulement (identifiant en « :free »)")

    run_dir = Path(args.run_dir).resolve()
    project = run_dir / "project"
    request, answer = last_answer(project)
    diff = subprocess.run(
        ["git", "diff", "--", ".", *NOT_THE_WORK],
        cwd=project, capture_output=True, encoding="utf-8", errors="replace", check=True,
    ).stdout
    if not diff.strip():
        sys.exit("aucun changement dans cet essai")

    work = run_dir / "review-replay"
    work.mkdir(exist_ok=True)
    # Relecteur imposé, ou celui que Claw choisirait (pas de réglage resté d'un rejeu précédent).
    settings = work / ".claw" / "settings.json"
    if args.reviewer:
        settings.parent.mkdir(exist_ok=True)
        settings.write_text(json.dumps({"advisorModels": [args.reviewer]}), encoding="utf-8")
    elif settings.exists():
        settings.unlink()
    command = [str(run.claw_binary(args.claw))]
    if args.model:
        command += ["--model", args.model]
    if args.baseline:
        (work / "system.txt").write_text(baseline_system(args.baseline), encoding="utf-8")
        (work / "prompt.txt").write_text(
            f"The user asked:\n{request.strip()}\n\nThe agent's final answer to the user:\n{answer}\n\n"
            "Changes made by the agent during this turn (earlier turns may have changed other files):\n"
            + diff,
            encoding="utf-8",
        )
        command += ["review-replay", "--raw", str(work / "system.txt"), str(work / "prompt.txt")]
    else:
        for name, text in (("request.txt", request), ("diff.txt", diff), ("answer.txt", answer)):
            (work / name).write_text(text, encoding="utf-8")
        command += ["review-replay", *(str(work / name) for name in ("request.txt", "diff.txt", "answer.txt"))]

    done = subprocess.run(
        command, cwd=work, env=run.real_env(), capture_output=True, encoding="utf-8",
        errors="replace", timeout=480,
    )
    if done.returncode != 0:
        sys.exit("claw review-replay a échoué : " + (done.stderr or done.stdout).strip()[-600:])
    verdict = json.loads(done.stdout.strip().splitlines()[-1])
    verdict["consignes"] = args.baseline or "actuelles"
    with (run_dir / "review-replay.jsonl").open("a", encoding="utf-8") as log:
        log.write(json.dumps(verdict, ensure_ascii=False) + "\n")
    print("relecteur :", verdict["reviewer"], "| consignes :", verdict["consignes"])
    print("remarques retenues :", verdict["findings"] or "aucune (approuvé)")
    print("--- réponse entière ---")
    print(verdict["answer"])
    return 0


if __name__ == "__main__":
    sys.exit(main())
