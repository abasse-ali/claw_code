"""Rejoue la vérification cachée d'un essai déjà fait, sans requête au modèle.

    python rescore.py ESSAI        (dossier <essais>/<date>-<scénario>-<label>, ou son nom)

Sert quand la vérification elle-même a changé ou a échoué (rapport illisible) : la note,
le détail des points et la ligne de l'essai dans <essais>/results.jsonl sont refaits.
"""

import json
import os
import subprocess
import sys
from pathlib import Path

sys.stdout.reconfigure(encoding="utf-8", errors="replace")

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
from run import RUNS, SCENARIOS  # noqa: E402

SUMMARY_KEYS = ("run", "scenario", "model", "label", "seconds", "exit_code", "timed_out",
                "free_requests_used", "requests", "tool_calls", "score", "max")


def main():
    if len(sys.argv) != 2:
        sys.exit(__doc__)
    run_dir = Path(sys.argv[1])
    if not run_dir.is_dir():
        run_dir = RUNS / sys.argv[1]
    result_path = run_dir / "result.json"
    result = json.loads(result_path.read_text(encoding="utf-8"))
    check = subprocess.run(
        [sys.executable, str(SCENARIOS / result["scenario"] / "check.py"), str(run_dir / "project")],
        capture_output=True, encoding="utf-8", errors="replace", timeout=600,
        env=dict(os.environ, PYTHONIOENCODING="utf-8"),
    )
    try:
        verdict = json.loads(check.stdout)
    except json.JSONDecodeError:
        sys.exit("vérification illisible :\n" + (check.stdout + check.stderr)[-2000:])
    result["score"] = verdict.get("score")
    result["max"] = verdict.get("max")
    result["checks"] = verdict.get("checks")
    result_path.write_text(json.dumps(result, ensure_ascii=False, indent=2), encoding="utf-8")
    summary = {key: result[key] for key in SUMMARY_KEYS if key in result}
    journal = RUNS / "results.jsonl"
    lines = journal.read_text(encoding="utf-8").splitlines() if journal.exists() else []
    lines = [line for line in lines if not line.strip() or json.loads(line).get("run") != result["run"]]
    lines.append(json.dumps(summary, ensure_ascii=False))
    journal.write_text("\n".join(line for line in lines if line.strip()) + "\n", encoding="utf-8")
    print(f"{result['run']} : {result['score']}/{result['max']}")
    for item in verdict.get("checks", []):
        print(("  ok  " if item["ok"] else "  KO  ") + item["name"])


if __name__ == "__main__":
    main()
