"""Compte rendu des essais réels d'un dossier, pour le journal d'un job (workflow real-tests.yml).

Usage :
    python report.py DOSSIER_DES_ESSAIS

Pour chaque essai : le résumé, la note de chaque point de la vérification cachée, les notices
de Claw, ses remarques au modèle, la réponse finale, puis la trace de la session (une ligne par
bloc, par session-trace.py). Un tableau des notes va aussi dans le résumé du job
(GITHUB_STEP_SUMMARY). Rien n'est envoyé à un modèle.
"""

import json
import os
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
TRACE = HERE.parent / "session-trace.py"


def section(title):
    print(f"\n{'=' * 20} {title} {'=' * 20}")


def report(run_dir):
    result_file = run_dir / "result.json"
    if not result_file.exists():
        section(f"{run_dir.name} : pas de résultat")
        return None
    result = json.loads(result_file.read_text(encoding="utf-8"))
    section(f"{result['scenario']} : {result.get('score')}/{result.get('max')}")
    for key in ("model", "seconds", "exit_code", "timed_out", "requests", "tool_calls"):
        print(f"{key} : {result.get(key)}")
    checks = result.get("checks")
    if isinstance(checks, list):
        print("\nVérification cachée :")
        for check in checks:
            if isinstance(check, dict):
                ok = check.get("ok")
                print(f"  {'OK ' if ok else 'NON'} {check.get('name')}")
                if not ok and check.get("detail"):
                    print(f"      {str(check['detail'])[:600]}")
            else:
                print(f"  {check}")
    elif checks:
        print(f"\nVérification cachée : {str(checks)[:2000]}")
    if result.get("notices"):
        print("\nNotices de Claw :")
        for notice in result["notices"]:
            print(f"  - {notice}")
    if result.get("claw_notes"):
        print("\nRemarques de Claw au modèle (début) :")
        for note in result["claw_notes"]:
            print(f"  - {note}")
    print("\nRéponse finale :")
    print(str(result.get("answer") or "")[:4000])
    session = result.get("session")
    if session and Path(session).exists():
        print("\nTrace de la session :")
        trace = subprocess.run(
            [sys.executable, str(TRACE), session, "--cut", "300"],
            capture_output=True, encoding="utf-8", errors="replace",
        )
        print(trace.stdout[-60000:] or trace.stderr[-2000:])
    stderr = run_dir / "stderr.txt"
    if stderr.exists() and stderr.stat().st_size:
        print("\nErreurs de Claw (fin) :")
        print(stderr.read_text(encoding="utf-8", errors="replace")[-3000:])
    return result


def load(run_dir):
    result_file = run_dir / "result.json"
    return json.loads(result_file.read_text(encoding="utf-8")) if result_file.exists() else None


def main():
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    # --summary : seulement les résumés (job « bilan » du workflow, qui réunit tous les essais).
    brief = "--summary" in sys.argv[1:]
    runs = Path(next(arg for arg in sys.argv[1:] if arg != "--summary"))
    folders = sorted(folder for folder in runs.iterdir() if folder.is_dir()) if runs.exists() else []
    results = [load(folder) if brief else report(folder) for folder in folders]
    rows = [result for result in results if result]
    if brief and rows:
        total = sum(row.get("score") or 0 for row in rows)
        most = sum(row.get("max") or 0 for row in rows)
        print(f"Total : {total}/{most} sur {len(rows)} essais")
    # Le résumé en dernier : la fin du journal suffit pour relever les notes.
    for row in rows:
        section(f"Résumé {row['scenario']}")
        print(f"note {row.get('score')}/{row.get('max')} ; {row.get('requests')} requêtes, "
              f"{row.get('tool_calls')} outils, {row.get('seconds')} s, modèle {row.get('model')}")
        for check in row.get("checks") or []:
            if isinstance(check, dict) and not check.get("ok"):
                print(f"  NON {check.get('name')}")
        for notice in row.get("notices") or []:
            print(f"  notice : {notice}")
    summary = os.environ.get("GITHUB_STEP_SUMMARY")
    if summary and rows:
        with open(summary, "a", encoding="utf-8") as handle:
            handle.write("| Scénario | Note | Requêtes | Outils | Durée |\n|---|---|---|---|---|\n")
            for row in rows:
                handle.write(f"| {row['scenario']} | {row.get('score')}/{row.get('max')} | {row.get('requests')} "
                             f"| {row.get('tool_calls')} | {row.get('seconds')} s |\n")
    if not rows:
        print("Aucun essai.")


if __name__ == "__main__":
    main()
