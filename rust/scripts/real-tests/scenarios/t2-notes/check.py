"""Vérification cachée de t2-notes : python check.py DOSSIER_DU_PROJET

Exigences dites : recherche sans majuscules ni accents, numéro de chaque note,
message quand rien ne correspond, tests. Exigences implicites : le numéro est
celui de la note dans le carnet (pas son rang dans les résultats), le mot cherché
peut lui-même porter accents et majuscules, un appel sans mot ne plante pas, la
recherche ne modifie pas le carnet, l'aide du module cite la commande.
"""

import json
import os
import re
import subprocess
import sys
from pathlib import Path

project = Path(sys.argv[1])
checks = []
ENV = dict(os.environ, PYTHONIOENCODING="utf-8")


def check(name, ok, detail=""):
    checks.append({"name": name, "ok": bool(ok), "detail": detail[-300:] if detail else ""})


def cli(*arguments):
    result = subprocess.run(
        [sys.executable, "notes.py", *arguments], cwd=project, capture_output=True,
        timeout=60, env=ENV,
    )
    return result.returncode, result.stdout.decode("utf-8", "replace"), result.stderr.decode("utf-8", "replace")


tests = subprocess.run(
    [sys.executable, "-m", "pytest", "-q", "-p", "no:cacheprovider"],
    cwd=project, capture_output=True, text=True, timeout=120, env=ENV,
)
check("les tests du projet passent", tests.returncode == 0, tests.stdout)

test_sources = "".join(
    path.read_text(encoding="utf-8", errors="replace") for path in project.rglob("test_*.py")
)
check(
    "des tests de la recherche ont été ajoutés",
    len(re.findall(r"def test_", test_sources)) > 2 and "cherch" in test_sources.lower(),
)

carnet = ["École du soir", "acheter du pain", "Réunion ÉCOLE", "fête de l'école"]
stored = json.dumps(carnet, ensure_ascii=False, indent=2)
(project / "notes.json").write_text(stored, encoding="utf-8")


def found(output, number, text):
    return any(str(number) in line and text in line for line in output.splitlines())


code, out, err = cli("chercher", "ecole")
exists = "Commande inconnue" not in out
check(
    "« ecole » trouve les trois notes, avec leur numéro dans le carnet (1, 3, 4)",
    code == 0
    and found(out, 1, "École du soir")
    and found(out, 3, "Réunion ÉCOLE")
    and found(out, 4, "fête de l'école")
    and "acheter" not in out,
    out + err,
)
code, out, err = cli("chercher", "ÉCOLE")
check(
    "le mot cherché peut porter accents et majuscules",
    found(out, 1, "École du soir") and found(out, 4, "fête de l'école"),
    out + err,
)
code, out, err = cli("chercher", "zzz")
check(
    "aucun résultat : un message, sans erreur",
    exists and out.strip() != "" and "Traceback" not in err and "Commande inconnue" not in out,
    out + err,
)
code, out, err = cli("chercher")
check(
    "sans mot : un message clair, pas de plantage",
    exists and "Traceback" not in err and (out + err).strip() != "" and "Commande inconnue" not in out,
    out + err,
)
check(
    "la recherche ne modifie pas le carnet",
    exists and (project / "notes.json").read_text(encoding="utf-8") == stored,
)
module = (project / "notes.py").read_text(encoding="utf-8", errors="replace")
docstring = module.split('"""')[1] if module.count('"""') >= 2 else ""
check("l'aide du module cite la commande chercher", "chercher" in docstring)

print(json.dumps({"score": sum(c["ok"] for c in checks), "max": len(checks), "checks": checks}, ensure_ascii=False))
