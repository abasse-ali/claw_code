"""Rejoue le contrôle des exigences de Claw sur une session réelle, là où il était arrivé.

Usage :
    python checklist_replay.py ESSAI --cut N [--baseline COMMIT] [--label TEXTE]
                               [--restore FICHIER ...] [--model MODELE] [--timeout S]

Copie le projet de l'essai (son dépôt Git et sa session compris) dans un nouvel essai, coupe
la session avant le message N (celui où Claw avait envoyé le contrôle des exigences), puis
renvoie au modèle ce contrôle tel que Claw l'écrit aujourd'hui, ou tel qu'il l'écrivait au
commit --baseline. Le modèle reprend avec tout ce qu'il savait à ce moment-là : on mesure
l'effet d'une formulation sur un vrai cas, pour une dizaine de requêtes au lieu d'un essai
entier. Pendant ce tour, le contrôle des exigences et la relecture de Claw sont coupés (le
message envoyé est déjà le contrôle) ; la vérification cachée du scénario note le résultat.

--restore remet un fichier du projet dans son état de départ (par exemple les données que la
vérification cachée a modifiées après l'essai).
"""

import argparse
import datetime
import json
import re
import shutil
import subprocess
import sys
import time
from pathlib import Path

import run

HARNESS = "rust/crates/runtime/src/harness.rs"
# Ce que Claw ajoute à chacune de ses remarques de fin de tour (`language_note`), repris du
# message coupé : il dépend de la langue réglée par l'utilisateur.
LANGUAGE_MARK = " Write your reply to the user in "


def rust_string(escaped):
    """Le texte d'un littéral de chaîne Rust (échappements simples)."""
    return re.sub(r"\\(.)", lambda found: {"n": "\n", "t": "\t"}.get(found.group(1), found.group(1)), escaped)


def current_note(claw, request, folder):
    """Le contrôle des exigences tel que Claw l'enverrait aujourd'hui (`claw
    checklist-note`), parties choisies d'après la demande comprises."""
    path = folder / "request.txt"
    path.write_text(request, encoding="utf-8")
    done = subprocess.run(
        [str(claw), "checklist-note", str(path)], capture_output=True, encoding="utf-8",
        errors="replace", check=True,
    )
    return done.stdout.rstrip("\n")


def checklist_note(request, commit, language_note):
    """Le contrôle des exigences tel que `checklist_prompt` l'écrivait au commit donné
    (pas d'absence de l'utilisateur, pas de demande antérieure), d'après son harness.rs."""
    source = subprocess.run(
        ["git", "show", f"{commit}:{HARNESS}"], cwd=run.RUST.parent,
        capture_output=True, encoding="utf-8", check=True,
    ).stdout
    prompt = re.search(r'pub const CHECKLIST_PROMPT: &str = "((?:[^"\\]|\\.)*)";', source)
    tail = re.search(r'"\{CHECKLIST_PROMPT\}\\n\\n\{quoted\}\\n((?:[^"\\]|\\.)*)"', source)
    if not prompt or not tail:
        sys.exit("contrôle des exigences introuvable dans harness.rs")
    quoted = f"The request to check against, word for word:\n<<<\n{request.strip()}\n>>>"
    return f"{rust_string(prompt.group(1))}\n\n{quoted}\n{rust_string(tail.group(1))}{language_note}"


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("run_dir")
    parser.add_argument("--cut", type=int, required=True, help="numéro du message du contrôle des exigences")
    parser.add_argument("--baseline", help="commit dont on rejoue la formulation du contrôle")
    parser.add_argument("--label", default="")
    parser.add_argument("--restore", action="append", default=[])
    parser.add_argument("--model")
    parser.add_argument("--timeout", type=int, default=900)
    parser.add_argument("--claw")
    args = parser.parse_args()

    source = Path(args.run_dir).resolve()
    earlier = json.loads((source / "result.json").read_text(encoding="utf-8"))
    scenario = run.SCENARIOS / earlier["scenario"]
    stamp = datetime.datetime.now().strftime("%Y%m%d-%H%M%S")
    label = "-".join(part for part in ("replay", args.label) if part)
    run_id = f"{stamp}-{earlier['scenario']}-{label}"
    run_dir = run.RUNS / run_id
    project = run_dir / "project"
    shutil.copytree(source / "project", project)

    sessions = sorted((project / ".claw" / "sessions").glob("*.json"), key=lambda path: path.stat().st_mtime)
    if not sessions:
        sys.exit("aucune session dans cet essai")
    session_path = sessions[-1]
    for other in sessions[:-1]:
        other.unlink()
    session = json.loads(session_path.read_text(encoding="utf-8"))
    kept = session["messages"][: args.cut]
    cut_message = session["messages"][args.cut] if args.cut < len(session["messages"]) else {}
    first_block = (cut_message.get("blocks") or [{}])[0]
    if not str(first_block.get("text", "")).startswith("[Claw] Before you finish"):
        sys.exit(f"le message {args.cut} n'est pas le contrôle des exigences")
    session["messages"] = kept
    session_path.write_text(json.dumps(session, ensure_ascii=False), encoding="utf-8")
    for name in args.restore:
        subprocess.run(["git", "checkout", "--", name], cwd=project, capture_output=True, check=True)

    request = next(
        block["text"] for message in kept if message.get("role") == "user"
        for block in message.get("blocks", [])[:1] if block.get("type") == "text"
    )
    original = str(first_block.get("text", ""))
    language_note = original[original.rfind(LANGUAGE_MARK):] if LANGUAGE_MARK in original else ""
    claw = run.claw_binary(args.claw)
    if args.baseline:
        note = checklist_note(request, args.baseline, language_note)
    else:
        note = current_note(claw, request, run_dir) + language_note
    (run_dir / "note.txt").write_text(note, encoding="utf-8")

    env = run.real_env()
    env["CLAW_CHECKLIST"] = "0"
    env["CLAW_REVIEW"] = "0"
    before = run.free_requests_left(claw, env)
    command = [
        str(claw), "--permission-mode", "danger-full-access", "--input-format", "stream-json",
        "--output-format", "stream-json", "--continue",
    ]
    if args.model:
        command += ["--model", args.model]
    started = time.time()
    process = subprocess.Popen(
        command, cwd=project, env=env, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    line = json.dumps({"type": "user", "message": {"content": note}}, ensure_ascii=False) + "\n"
    timed_out = False
    try:
        stdout, stderr = process.communicate(line.encode("utf-8"), timeout=args.timeout)
    except subprocess.TimeoutExpired:
        timed_out = True
        run.stop_tree(process.pid)
        stdout, stderr = process.communicate()
    seconds = round(time.time() - started)
    (run_dir / "output.jsonl").write_bytes(stdout)
    (run_dir / "stderr.txt").write_bytes(stderr)
    after = run.free_requests_left(claw, env)

    check = subprocess.run(
        [sys.executable, str(scenario / "check.py"), str(project)],
        capture_output=True, encoding="utf-8", errors="replace", timeout=300,
    )
    try:
        verdict = json.loads(check.stdout)
    except json.JSONDecodeError:
        verdict = {"score": 0, "max": 0, "error": (check.stdout + check.stderr)[-2000:]}
    events = []
    for raw in stdout.decode("utf-8", "replace").splitlines():
        try:
            events.append(json.loads(raw))
        except json.JSONDecodeError:
            continue
    final = [event for event in events if event.get("type") == "result"]
    stats = run.session_stats(project)
    before_cut = sum(1 for message in kept if message.get("role") == "assistant")
    result = {
        "run": run_id,
        "scenario": earlier["scenario"],
        "source": source.name,
        "cut": args.cut,
        "consignes": args.baseline or "actuelles",
        "model": args.model or earlier.get("model"),
        "label": label,
        "seconds": seconds,
        "timed_out": timed_out,
        "free_requests_used": (before - after) if before is not None and after is not None else None,
        "requests": (stats.get("requests") or 0) - before_cut,
        "score": verdict.get("score"),
        "max": verdict.get("max"),
        "checks": verdict.get("checks", verdict.get("error")),
        "answer": final[-1].get("result") if final else None,
        "notices": [event.get("text") or event.get("message") for event in events
                    if event.get("type") == "system" and event.get("subtype") not in ("init",)],
    }
    (run_dir / "result.json").write_text(json.dumps(result, ensure_ascii=False, indent=2), encoding="utf-8")
    summary = {key: result[key] for key in ("run", "scenario", "source", "cut", "consignes", "label",
                                            "seconds", "timed_out", "free_requests_used", "requests",
                                            "score", "max")}
    with (run.RUNS / "results.jsonl").open("a", encoding="utf-8") as handle:
        handle.write(json.dumps(summary, ensure_ascii=False) + "\n")
    print(json.dumps(summary, ensure_ascii=False, indent=2))
    print(f"détail : {run_dir / 'result.json'}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
