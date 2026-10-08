#!/usr/bin/env python3
"""Évaluation de Claw avec un vrai modèle sur de petites tâches de code vérifiées
automatiquement (tests unitaires Python, contenu des fichiers, absence de
modification en lecture seule).

    python rust/scripts/eval-tasks.py --model ollama:qwen2.5-coder:7b
    python rust/scripts/eval-tasks.py --model qwen/qwen3-coder:free --only corriger-bug,ajout-fonction

Chaque tâche part d'un petit dépôt git temporaire. Résumé à la fin et rapport
JSON détaillé (--rapport). Le modèle n'est jamais un faux : les clés et serveurs
sont ceux de l'environnement (OPENROUTER_API_KEY, Ollama…).
"""

import argparse
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import time
from pathlib import Path

PYTHON = sys.executable


def unittest_passes(root: Path) -> tuple[bool, str]:
    result = subprocess.run(
        [PYTHON, "-m", "unittest", "discover", "-s", ".", "-p", "test_*.py"],
        cwd=root, capture_output=True, text=True, timeout=120,
    )
    output = (result.stdout + result.stderr).strip()
    return result.returncode == 0, output[-600:]


def git_changes(root: Path) -> list[str]:
    output = subprocess.run(
        ["git", "status", "--porcelain", "--", ".", ":(exclude).claw"],
        cwd=root, capture_output=True, text=True,
    ).stdout
    return [line[3:] for line in output.splitlines() if line.strip()]


# --- Tâches -------------------------------------------------------------------

def check_tests_pass(root, _result):
    return unittest_passes(root)


def check_new_tests(root, _result):
    test_file = root / "test_premier.py"
    if not test_file.exists():
        return False, "test_premier.py absent"
    content = test_file.read_text(encoding="utf-8")
    asserts = len(re.findall(r"\bassert\w*\(", content))
    passed, output = unittest_passes(root)
    # Les tests écrits doivent attraper le bug réintroduit volontairement.
    original = (root / "premier.py").read_text(encoding="utf-8")
    (root / "premier.py").write_text(original.replace("if n < 2:", "if n < 1:"), encoding="utf-8")
    caught, _ = unittest_passes(root)
    (root / "premier.py").write_text(original, encoding="utf-8")
    ok = passed and asserts >= 3 and not caught
    return ok, f"{asserts} assertions, tests {'OK' if passed else 'en échec'}, bug réintroduit {'détecté' if not caught else 'NON détecté'}\n{output}"


def check_rename(root, _result):
    passed, output = unittest_passes(root)
    sources = "".join(path.read_text(encoding="utf-8") for path in root.glob("*.py"))
    old_left = re.search(r"\bcalc\(", sources) is not None
    new_present = "def calculer_total(" in sources
    return passed and not old_left and new_present, f"ancien nom restant : {old_left}, nouveau nom : {new_present}\n{output}"


def check_read_only(root, result):
    changes = git_changes(root)
    answer = (result.get("result") or "").lower()
    mentions = any(word in answer for word in ("cache", "mémo", "memo", "fibonacci"))
    return not changes and mentions, f"modifications : {changes or 'aucune'} ; réponse pertinente : {mentions}"


def check_config_constant(root, _result):
    passed, output = unittest_passes(root)
    config = (root / "config.py").read_text(encoding="utf-8")
    service = (root / "service.py").read_text(encoding="utf-8")
    ok = passed and "DELAI_MAX = 30" in config and "10" not in service.split("def ")[1]
    return ok, output


TASKS = [
    {
        "id": "corriger-bug",
        "prompt": "Les tests échouent. Trouve et corrige le bug dans stats.py, sans modifier les tests.",
        "files": {
            "stats.py": 'def moyenne(valeurs):\n    """Moyenne des valeurs ; 0 pour une liste vide."""\n    total = 0\n    for i in range(len(valeurs) - 1):\n        total += valeurs[i]\n    return total / len(valeurs)\n',
            "test_stats.py": "import unittest\nfrom stats import moyenne\n\n\nclass Tests(unittest.TestCase):\n    def test_moyenne(self):\n        self.assertEqual(moyenne([2, 4, 6]), 4)\n\n    def test_vide(self):\n        self.assertEqual(moyenne([]), 0)\n\n\nif __name__ == '__main__':\n    unittest.main()\n",
        },
        "check": check_tests_pass,
    },
    {
        "id": "ajout-fonction",
        "prompt": "Implémente la fonction slugify de texte.py selon sa docstring, pour que les tests passent.",
        "files": {
            "texte.py": 'def slugify(texte):\n    """Convertit un titre en identifiant d\'URL : minuscules, accents retirés,\n    tout ce qui n\'est pas lettre ou chiffre remplacé par un seul tiret, sans\n    tiret au début ni à la fin. Ex. « Élan, été ! » -> « elan-ete »."""\n    raise NotImplementedError\n',
            "test_texte.py": "import unittest\nfrom texte import slugify\n\n\nclass Tests(unittest.TestCase):\n    def test_simple(self):\n        self.assertEqual(slugify('Bonjour le monde'), 'bonjour-le-monde')\n\n    def test_accents_ponctuation(self):\n        self.assertEqual(slugify('  Élan, été !  '), 'elan-ete')\n\n    def test_chiffres(self):\n        self.assertEqual(slugify('Version 2.0 -- finale'), 'version-2-0-finale')\n\n\nif __name__ == '__main__':\n    unittest.main()\n",
        },
        "check": check_tests_pass,
    },
    {
        "id": "ecrire-tests",
        "prompt": "Écris des tests unitaires (unittest) pour est_premier dans un nouveau fichier test_premier.py : cas limites (0, 1, 2), nombres premiers et composés. Lance-les.",
        "files": {
            "premier.py": "def est_premier(n):\n    if n < 2:\n        return False\n    diviseur = 2\n    while diviseur * diviseur <= n:\n        if n % diviseur == 0:\n            return False\n        diviseur += 1\n    return True\n",
        },
        "check": check_new_tests,
    },
    {
        "id": "renommer",
        "prompt": "Renomme la fonction calc en calculer_total partout dans le projet (définition et appels), les tests doivent toujours passer.",
        "files": {
            "panier.py": "def calc(prix, quantites):\n    return sum(p * q for p, q in zip(prix, quantites))\n",
            "facture.py": "from panier import calc\n\n\ndef montant_ttc(prix, quantites, tva=0.2):\n    return round(calc(prix, quantites) * (1 + tva), 2)\n",
            "test_facture.py": "import unittest\nfrom facture import montant_ttc\nfrom panier import calculer_total\n\n\nclass Tests(unittest.TestCase):\n    def test_total(self):\n        self.assertEqual(calculer_total([2, 3], [1, 2]), 8)\n\n    def test_ttc(self):\n        self.assertEqual(montant_ttc([10], [1]), 12.0)\n\n\nif __name__ == '__main__':\n    unittest.main()\n",
        },
        "check": check_rename,
    },
    {
        "id": "constante-config",
        "prompt": "Le délai de 10 est codé en dur dans service.py. Déplace-le dans config.py sous le nom DELAI_MAX, passe-le à 30, et utilise-le dans service.py.",
        "files": {
            "config.py": "NOM_SERVICE = 'facturation'\n",
            "service.py": "import config\n\n\ndef delai_autorise(secondes):\n    return secondes <= 10\n",
            "test_service.py": "import unittest\nimport config\nfrom service import delai_autorise\n\n\nclass Tests(unittest.TestCase):\n    def test_delai(self):\n        self.assertTrue(delai_autorise(25))\n        self.assertFalse(delai_autorise(31))\n        self.assertEqual(config.DELAI_MAX, 30)\n\n\nif __name__ == '__main__':\n    unittest.main()\n",
        },
        "check": check_config_constant,
    },
    {
        "id": "analyse-lecture-seule",
        "prompt": "Explique en quelques phrases ce que fait suite.py et quelle optimisation il utilise. Ne modifie aucun fichier.",
        "mode": "read-only",
        "files": {
            "suite.py": "from functools import lru_cache\n\n\n@lru_cache(maxsize=None)\ndef terme(n):\n    if n < 2:\n        return n\n    return terme(n - 1) + terme(n - 2)\n",
        },
        "check": check_read_only,
    },
]


def run_task(task, args, claw):
    root = Path(tempfile.mkdtemp(prefix=f"claw-eval-{task['id']}-"))
    for name, content in task["files"].items():
        (root / name).write_text(content, encoding="utf-8")
    # Comme dans un vrai projet : l'agent peut lancer Python (les tests) sans
    # demande ; toute autre commande reste refusée en mode non interactif.
    python_rules = [f"{tool}({program}:*)" for tool in ("Bash", "PowerShell") for program in ("python", "python3", "py")]
    (root / ".claw").mkdir()
    (root / ".claw" / "settings.json").write_text(
        json.dumps({"permissions": {"allow": python_rules}}, indent=2), encoding="utf-8"
    )
    (root / ".gitignore").write_text(".claw/\n__pycache__/\n", encoding="utf-8")
    subprocess.run(["git", "init", "-q", "."], cwd=root, check=True)
    subprocess.run(["git", "add", "-A"], cwd=root, check=True)
    subprocess.run(["git", "-c", "user.name=eval", "-c", "user.email=eval@example.invalid", "commit", "-qm", "départ"], cwd=root, check=True)

    command = [claw, "--model", args.model, "--permission-mode", task.get("mode", "workspace-write"),
               "--output-format", "stream-json", "prompt", task["prompt"]]
    started = time.monotonic()
    events, error = [], None
    try:
        completed = subprocess.run(command, cwd=root, capture_output=True, text=True, encoding="utf-8",
                                   errors="replace", timeout=args.timeout)
        for line in completed.stdout.splitlines():
            try:
                events.append(json.loads(line))
            except json.JSONDecodeError:
                pass
        if completed.returncode != 0:
            error = completed.stderr.strip()[-500:] or f"code {completed.returncode}"
    except subprocess.TimeoutExpired:
        error = f"délai de {args.timeout} s dépassé"
    duration = time.monotonic() - started

    result = next((event for event in reversed(events) if event.get("type") == "result"), {})
    if result.get("subtype") == "error" and not error:
        error = f"erreur de claw : {result.get('error')}"
    tool_calls = [
        block.get("name")
        for event in events if event.get("type") == "assistant"
        for block in (event.get("message", {}).get("content") or [])
        if block.get("type") == "tool_use"
    ]
    # Une erreur en cours de route n'empêche pas de vérifier le travail déjà fait.
    ok, detail = task["check"](root, result)
    report = {
        "tache": task["id"],
        "reussi": bool(ok),
        "duree_s": round(duration, 1),
        "tours": result.get("num_turns"),
        "modele": result.get("model"),
        "outils": tool_calls,
        "erreur": error,
        "detail": detail,
        "dossier": str(root),
    }
    if not args.keep and ok:
        shutil.rmtree(root, ignore_errors=True)
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--model", required=True, help="modèle à évaluer (ex. ollama:qwen2.5-coder:7b)")
    parser.add_argument("--claw", default=shutil.which("claw") or "claw", help="exécutable claw")
    parser.add_argument("--only", help="tâches à lancer, séparées par des virgules")
    parser.add_argument("--timeout", type=int, default=900, help="délai par tâche en secondes")
    parser.add_argument("--rapport", help="fichier JSON du rapport détaillé")
    parser.add_argument("--keep", action="store_true", help="garder les dossiers des tâches réussies")
    args = parser.parse_args()
    if hasattr(sys.stdout, "reconfigure"):
        sys.stdout.reconfigure(encoding="utf-8")

    # Chemin absolu : chaque tâche s'exécute dans son propre dossier.
    args.claw = shutil.which(args.claw) or str(Path(args.claw).resolve())
    selected = [task for task in TASKS if not args.only or task["id"] in args.only.split(",")]
    os.environ.setdefault("CLAW_FALLBACK", "0")  # mesurer ce modèle, pas ses remplaçants
    reports = []
    for task in selected:
        print(f"… {task['id']}", flush=True)
        report = run_task(task, args, args.claw)
        reports.append(report)
        status = "RÉUSSI" if report["reussi"] else "ÉCHEC "
        print(f"{status} {report['tache']:<24} {report['duree_s']:>7.1f} s  {report['tours'] or '-'} tours  "
              f"outils : {', '.join(report['outils']) or 'aucun'}", flush=True)
        if report["erreur"]:
            print(f"        {report['erreur'][:400]}", flush=True)
        if not report["reussi"]:
            print(f"        {report['detail'][:400]}".rstrip(), flush=True)
            print(f"        dossier : {report['dossier']}", flush=True)

    passed = sum(report["reussi"] for report in reports)
    total_time = sum(report["duree_s"] for report in reports)
    print(f"\n{args.model} : {passed}/{len(reports)} tâches réussies en {total_time:.0f} s")
    if args.rapport:
        Path(args.rapport).write_text(json.dumps(reports, ensure_ascii=False, indent=2), encoding="utf-8")
    sys.exit(0 if passed == len(reports) else 1)


if __name__ == "__main__":
    main()
