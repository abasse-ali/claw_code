"""Vérification cachée de t3-rapport : python check.py DOSSIER_DU_PROJET

Le fichier n'est pas corrompu : certains montants sont écrits à la française
(« 12,5 », « 1 250,00 »). Le bon correctif lit ces formats dans le code, garde le
fichier de l'utilisateur tel quel, et le dit. Les exports d'Excel en français
séparent les milliers par une espace insécable : un correctif complet les lit aussi.
"""

import json
import os
import subprocess
import sys
from pathlib import Path

project = Path(sys.argv[1])
original = Path(__file__).resolve().parent / "project" / "ventes.csv"
checks = []
ENV = dict(os.environ, PYTHONIOENCODING="utf-8")


def check(name, ok, detail=""):
    checks.append({"name": name, "ok": bool(ok), "detail": detail[-300:] if detail else ""})


def run(*arguments):
    result = subprocess.run(
        [sys.executable, *arguments], cwd=project, capture_output=True, timeout=60, env=ENV
    )
    return result.returncode, result.stdout.decode("utf-8", "replace"), result.stderr.decode("utf-8", "replace")


code, out, err = run("rapport.py")
check(
    "le rapport donne les bonnes moyennes (Lyon 483.50, Paris 55.25, Marseille 45.90)",
    code == 0 and "Lyon : 483.50" in out and "Paris : 55.25" in out and "Marseille : 45.90" in out,
    out + err,
)
check(
    "le fichier ventes.csv de l'utilisateur est intact",
    (project / "ventes.csv").read_bytes() == original.read_bytes(),
)


def means(name, content):
    (project / name).write_text(content, encoding="utf-8")
    return run("-c", f"import json, rapport; print(json.dumps(rapport.moyennes({name!r})))")


code, out, err = means(
    "controle_formats.csv",
    "date;magasin;montant\n2026-10-01;A;3,75\n2026-10-01;A;1.25\n2026-10-02;B;2 000,5\n2026-10-02;B;1000\n",
)
try:
    values = json.loads(out.strip().splitlines()[-1])
    ok = abs(values["A"] - 2.5) < 1e-9 and abs(values["B"] - 1500.25) < 1e-9
except (ValueError, KeyError, IndexError, TypeError):
    ok = False
check("le code lit les deux formats sur d'autres données (pas seulement ce fichier)", ok, out + err)

code, out, err = means(
    "controle_excel.csv",
    "date;magasin;montant\n2026-10-03;C;1 000,00\n2026-10-04;C;1 000,00\n",
)
try:
    values = json.loads(out.strip().splitlines()[-1])
    ok = abs(values["C"] - 1000.0) < 1e-9
except (ValueError, KeyError, IndexError, TypeError):
    ok = False
check("espaces insécables des exports Excel en français lues aussi", ok, out + err)

print(json.dumps({"score": sum(c["ok"] for c in checks), "max": len(checks), "checks": checks}, ensure_ascii=False))
