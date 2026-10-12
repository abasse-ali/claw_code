"""Vérification cachée de t4-bibliotheque : python check.py DOSSIER_DU_PROJET

Tout passe par le guichet (`biblio.py`), sur les données de départ remises en
place : chaque commande est un nouveau processus, les réservations doivent donc
être enregistrées. Exigences dites : réserver un livre emprunté, file dans
l'ordre des demandes, livre mis de côté pour le premier au retour, `etat` qui
montre qui a le livre et qui l'attend, tests. Exigences implicites : on ne
réserve pas le livre qu'on a déjà, une même personne n'attend pas deux fois, un
membre ou un livre inconnu est refusé sans plantage, l'ancien fichier de données
(sans réservations) se lit toujours, l'aide du guichet cite la commande.
"""

import json
import os
import re
import shutil
import subprocess
import sys
from pathlib import Path

project = Path(sys.argv[1])
original = Path(__file__).resolve().parent / "project" / "donnees.json"
checks = []
ENV = dict(os.environ, PYTHONIOENCODING="utf-8")


def check(name, ok, detail=""):
    checks.append({"name": name, "ok": bool(ok), "detail": detail[-300:] if detail else ""})


def guichet(*arguments):
    result = subprocess.run(
        [sys.executable, "biblio.py", *arguments], cwd=project, capture_output=True,
        timeout=60, env=ENV,
    )
    out = result.stdout.decode("utf-8", "replace")
    err = result.stderr.decode("utf-8", "replace")
    return result.returncode, out, err


def ligne(code):
    """La ligne de `etat` qui parle de ce livre (et les suivantes en retrait)."""
    _, out, _ = guichet("etat")
    lignes = out.splitlines()
    for index, texte in enumerate(lignes):
        if texte.startswith(code):
            suite = []
            for autre in lignes[index + 1:]:
                if re.match(r"L\d", autre):
                    break
                suite.append(autre)
            return " ".join([texte, *suite])
    return ""


tests = subprocess.run(
    [sys.executable, "-m", "pytest", "-q", "-p", "no:cacheprovider"],
    cwd=project, capture_output=True, text=True, timeout=180, env=ENV,
)
check("les tests du projet passent", tests.returncode == 0, tests.stdout)
sources = "".join(
    path.read_text(encoding="utf-8", errors="replace") for path in project.rglob("test_*.py")
)
check(
    "des tests des réservations ont été ajoutés",
    len(re.findall(r"def test_", sources)) > 4 and "reserv" in sources.lower().replace("é", "e"),
)

# Données de départ : l'ancien format, sans réservations (Awa a L1).
shutil.copyfile(original, project / "donnees.json")
code, out, err = guichet("etat")
check("l'ancien fichier de données se lit toujours", code == 0 and "Le Horla" in out, out + err)

a, out_a, err_a = guichet("reserver", "Bilal", "L1")
b, out_b, err_b = guichet("reserver", "Chloé", "L1")
check("deux membres réservent un livre emprunté", a == 0 and b == 0, out_a + err_a + out_b + err_b)
# Sans la commande, tout appel est « refusé » : les refus ne comptent que si elle existe.
existe = a == 0 and b == 0
texte = ligne("L1")
check(
    "`etat` montre qui a le livre et qui l'attend, dans l'ordre des demandes",
    "Awa" in texte and "Bilal" in texte and "Chloé" in texte
    and texte.index("Bilal") < texte.index("Chloé"),
    texte,
)
guichet("reserver", "Bilal", "L1")
check("une même personne n'attend pas deux fois", ligne("L1").count("Bilal") == 1, ligne("L1"))
code, out, err = guichet("reserver", "Awa", "L1")
check(
    "on ne réserve pas le livre qu'on a déjà",
    existe and code != 0 and "Traceback" not in err and ligne("L1").count("Awa") == 1,
    out + err,
)
inconnu_m, _, err_m = guichet("reserver", "Zoé", "L1")
inconnu_l, _, err_l = guichet("reserver", "Bilal", "L9")
check(
    "membre ou livre inconnu : refusé sans plantage",
    existe and inconnu_m != 0 and inconnu_l != 0 and "Traceback" not in err_m + err_l,
    err_m + err_l,
)

rendu, out, err = guichet("rendre", "L1")
autre, _, err_autre = guichet("emprunter", "Chloé", "L1")
premier, _, err_premier = guichet("emprunter", "Bilal", "L1")
check(
    "au retour, le livre est mis de côté pour le premier de la file",
    rendu == 0 and autre != 0 and premier == 0 and "Traceback" not in err + err_autre + err_premier,
    out + err + err_autre + err_premier,
)
texte = ligne("L1")
check(
    "après l'emprunt, le premier a le livre et le second attend toujours",
    "Bilal" in texte and "Chloé" in texte and "Awa" not in texte,
    texte,
)
guichet("rendre", "L1")
tiers, _, _ = guichet("emprunter", "Awa", "L1")
second, _, _ = guichet("emprunter", "Chloé", "L1")
guichet("rendre", "L1")
libre, _, _ = guichet("emprunter", "Awa", "L1")
check(
    "le second passe à son tour, puis le livre redevient libre pour tous",
    tiers != 0 and second == 0 and libre == 0,
    ligne("L1"),
)

aide = (project / "biblio.py").read_text(encoding="utf-8", errors="replace")
docstring = aide.split('"""')[1] if aide.count('"""') >= 2 else ""
check("l'aide du guichet cite la commande reserver", "reserver" in docstring)

print(json.dumps({"score": sum(c["ok"] for c in checks), "max": len(checks), "checks": checks}, ensure_ascii=False))
