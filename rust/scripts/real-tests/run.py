"""Essai réel de Claw sur un scénario, avec un vrai modèle gratuit.

Usage :
    python run.py SCENARIO [--model MODELE] [--label TEXTE] [--timeout SECONDES] [--claw CHEMIN]
    python run.py --list

Copie le projet du scénario dans <essais>/<date>-<scenario>-<label>/project, en fait un
dépôt Git à lui (un commit initial), lance Claw dessus sans interaction (mode
danger-full-access dans cette copie, bascule de modèle coupée), puis la vérification
cachée du scénario (check.py, jamais copiée dans le projet). Le détail va dans
result.json à côté du projet, et une ligne dans <essais>/results.jsonl.

<essais> est hors de ce dépôt : CLAW_REAL_TESTS_DIR, sinon %LOCALAPPDATA%\\claw-real-tests
(~/.cache/claw-real-tests ailleurs). Dans le dépôt, Claw donnerait au modèle l'état et le
diff Git du dépôt entier, et les fichiers d'instructions des dossiers parents : le modèle y
lirait les pièges des scénarios.

Chaque étape de l'agent consomme une requête du quota gratuit : peu d'essais, ciblés, et
toujours des modèles gratuits.
"""

import argparse
import datetime
import json
import os
import re
import shutil
import subprocess
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
SCENARIOS = HERE / "scenarios"
RUST = HERE.parent.parent


def runs_dir():
    if os.environ.get("CLAW_REAL_TESTS_DIR"):
        return Path(os.environ["CLAW_REAL_TESTS_DIR"])
    if os.environ.get("LOCALAPPDATA"):
        return Path(os.environ["LOCALAPPDATA"]) / "claw-real-tests"
    return Path.home() / ".cache" / "claw-real-tests"


RUNS = runs_dir()
# Ce que Claw lit dans les dossiers parents d'un projet : rien de tout cela ne doit
# exister au-dessus des essais, sinon le modèle verrait autre chose que le scénario.
ANCESTOR_FILES = ("CLAW.md", "CLAW.local.md", "CLAUDE.md", "CLAUDE.local.md", "AGENTS.md", ".git")


def contaminated_ancestors(folder):
    found = []
    for parent in [folder, *folder.parents]:
        for name in ANCESTOR_FILES:
            if (parent / name).exists():
                found.append(str(parent / name))
    return found


def make_repository(project):
    """Le projet copié devient un dépôt Git propre (état et diff vides au départ)."""
    git = ["git", "-c", "user.name=claw-bench", "-c", "user.email=bench@localhost"]
    for args in (["init", "-q"], ["add", "-A"], ["commit", "-q", "-m", "projet de départ"]):
        subprocess.run(git + args, cwd=project, capture_output=True, check=True)


def claw_binary(explicit):
    if explicit:
        return Path(explicit)
    for name in ("claw.exe", "claw"):
        candidate = RUST / "target" / "debug" / name
        if candidate.exists():
            return candidate
    sys.exit("claw introuvable : lancez d'abord `cargo build -p claw-cli` dans rust/")


def real_env():
    """Environnement d'un essai réel : les vraies clés (jamais affichées), pas de bascule."""
    env = dict(os.environ)
    env.pop("CLAW_NO_USER_ENV", None)
    env["CLAW_FALLBACK"] = "0"
    env.setdefault("PYTHONIOENCODING", "utf-8")
    return env


def free_requests_left(claw, env):
    """Requêtes gratuites restantes d'après `claw credits` (aucune requête de génération)."""
    try:
        out = subprocess.run(
            [str(claw), "credits"], env=env, capture_output=True, timeout=60,
            encoding="utf-8", errors="replace",
        ).stdout
    except (OSError, subprocess.TimeoutExpired):
        return None
    match = re.search(r"(\d+)/(\d+) requêtes restantes", out)
    return int(match.group(1)) if match else None


def stop_tree(pid):
    if os.name == "nt":
        subprocess.run(["taskkill", "/T", "/F", "/PID", str(pid)], capture_output=True)
    else:
        try:
            os.killpg(pid, 9)
        except OSError:
            pass


def session_stats(project):
    """Requêtes (réponses du modèle), appels d'outils et notes de Claw de la dernière session."""
    folder = project / ".claw" / "sessions"
    sessions = sorted(folder.glob("*.json"), key=lambda path: path.stat().st_mtime) if folder.exists() else []
    if not sessions:
        return {"session": None}
    data = json.loads(sessions[-1].read_text(encoding="utf-8"))
    messages = data.get("messages", [])
    assistant = [message for message in messages if message.get("role") == "assistant"]
    tools = [
        block.get("name")
        for message in assistant
        for block in message.get("blocks", [])
        if block.get("type") == "tool_use"
    ]
    notes = [
        block.get("text", "")[:160]
        for message in messages
        if message.get("role") == "user"
        for block in message.get("blocks", [])[:1]
        if block.get("type") == "text" and block.get("text", "").startswith("[Claw")
    ]
    return {
        "session": str(sessions[-1]),
        "requests": len(assistant),
        "tool_calls": len(tools),
        "tools": tools,
        "claw_notes": notes,
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("scenario", nargs="?")
    parser.add_argument("--model")
    parser.add_argument("--label", default="")
    parser.add_argument("--timeout", type=int, default=1500)
    parser.add_argument("--claw")
    parser.add_argument("--list", action="store_true")
    args = parser.parse_args()

    if args.list or not args.scenario:
        for folder in sorted(SCENARIOS.iterdir()):
            prompt = (folder / "prompt.txt").read_text(encoding="utf-8").strip()
            print(f"{folder.name}\n    {prompt}\n")
        return 0

    scenario = SCENARIOS / args.scenario
    if not scenario.is_dir():
        sys.exit(f"scénario inconnu : {args.scenario} (voir --list)")
    claw = claw_binary(args.claw)
    env = real_env()

    RUNS.mkdir(parents=True, exist_ok=True)
    leaks = contaminated_ancestors(RUNS)
    if leaks:
        sys.exit("dossier d'essais exposé (le modèle lirait ceci) : " + ", ".join(leaks)
                 + "\nchoisissez un autre dossier avec CLAW_REAL_TESTS_DIR")

    stamp = datetime.datetime.now().strftime("%Y%m%d-%H%M%S")
    run_id = "-".join(part for part in (stamp, args.scenario, args.label) if part)
    run_dir = RUNS / run_id
    project = run_dir / "project"
    shutil.copytree(scenario / "project", project)
    make_repository(project)
    prompt = (scenario / "prompt.txt").read_text(encoding="utf-8").strip()

    before = free_requests_left(claw, env)
    command = [str(claw), "--permission-mode", "danger-full-access", "--output-format", "json"]
    if args.model:
        command += ["--model", args.model]
    command.append(prompt)

    started = time.time()
    kwargs = {} if os.name == "nt" else {"start_new_session": True}
    process = subprocess.Popen(
        command, cwd=project, env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE, **kwargs
    )
    timed_out = False
    try:
        stdout, stderr = process.communicate(timeout=args.timeout)
    except subprocess.TimeoutExpired:
        timed_out = True
        stop_tree(process.pid)
        stdout, stderr = process.communicate()
    seconds = round(time.time() - started)
    (run_dir / "output.json").write_bytes(stdout)
    (run_dir / "stderr.txt").write_bytes(stderr)
    after = free_requests_left(claw, env)

    check = subprocess.run(
        [sys.executable, str(scenario / "check.py"), str(project)],
        capture_output=True, encoding="utf-8", errors="replace", timeout=300,
    )
    try:
        verdict = json.loads(check.stdout)
    except json.JSONDecodeError:
        verdict = {"score": 0, "max": 0, "error": (check.stdout + check.stderr)[-2000:]}

    try:
        answer = json.loads(stdout.decode("utf-8", errors="replace")).get("message", "")
    except json.JSONDecodeError:
        answer = stdout.decode("utf-8", errors="replace")[-4000:]

    result = {
        "run": run_id,
        "scenario": args.scenario,
        "model": args.model or "(réglage de l'utilisateur)",
        "label": args.label,
        "seconds": seconds,
        "exit_code": process.returncode,
        "timed_out": timed_out,
        "free_requests_used": (before - after) if before is not None and after is not None else None,
        **session_stats(project),
        "score": verdict.get("score"),
        "max": verdict.get("max"),
        "checks": verdict.get("checks", verdict.get("error")),
        "answer": answer,
    }
    (run_dir / "result.json").write_text(json.dumps(result, ensure_ascii=False, indent=2), encoding="utf-8")
    summary = {key: result[key] for key in ("run", "scenario", "model", "label", "seconds", "exit_code",
                                            "timed_out", "free_requests_used", "requests", "tool_calls",
                                            "score", "max") if key in result}
    with (RUNS / "results.jsonl").open("a", encoding="utf-8") as handle:
        handle.write(json.dumps(summary, ensure_ascii=False) + "\n")
    print(json.dumps(summary, ensure_ascii=False, indent=2))
    print(f"détail : {run_dir / 'result.json'}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
