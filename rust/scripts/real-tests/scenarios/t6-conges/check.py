"""Vérification cachée de t6-conges : python check.py DOSSIER_DU_PROJET

Tout passe par la ligne de commande (`conges.py`), sur les données de départ remises en
place. Exigences dites : `jours DEBUT FIN` donne les jours ouvrés (sans samedis, dimanches
ni jours fériés de feries.txt), `poser` retire ces jours du solde, dates écrites à la
française, tests. Exigences implicites : bornes incluses (du lundi au vendredi, cinq jours),
« 01/04/2026 » est le 1er avril, un férié un samedi n'est retiré qu'une fois, année
bissextile, fin avant début et date impossible refusées sans plantage, solde insuffisant
refusé, aide de la commande à jour.
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


def commande(*arguments):
    result = subprocess.run(
        [sys.executable, "conges.py", *arguments], cwd=project, capture_output=True,
        timeout=60, env=ENV,
    )
    return (
        result.returncode,
        result.stdout.decode("utf-8", "replace"),
        result.stderr.decode("utf-8", "replace"),
    )


def nombre(sortie):
    """Le nombre de jours que la commande affiche, quelle que soit sa phrase."""
    seul = re.fullmatch(r"\s*(\d+)\s*", sortie)
    if seul:
        return int(seul.group(1))
    for motif in (r"(\d+)\s*jours?", r"jours?\s*ouvr\w*\s*[:=]?\s*(\d+)", r":\s*(\d+)\s*$"):
        trouve = re.search(motif, sortie, re.IGNORECASE | re.MULTILINE)
        if trouve:
            return int(trouve.group(1))
    return None


def jours(debut, fin):
    code, out, err = commande("jours", debut, fin)
    return code, nombre(out) if code == 0 else None, out + err


tests = subprocess.run(
    [sys.executable, "-m", "pytest", "-q", "-p", "no:cacheprovider"],
    cwd=project, capture_output=True, text=True, timeout=180, env=ENV,
)
check("les tests du projet passent", tests.returncode == 0, tests.stdout)
sources = "".join(
    path.read_text(encoding="utf-8", errors="replace") for path in project.rglob("test_*.py")
)
check(
    "des tests des jours ouvrés ont été ajoutés",
    len(re.findall(r"def test_", sources)) > 3 and re.search(r"jours|ouvr", sources),
)

shutil.copyfile(original, project / "donnees.json")
code, semaine, sortie = jours("13/04/2026", "17/04/2026")
# Sans la commande, tout appel est « refusé » : les refus ne comptent que si elle existe.
existe = code == 0 and semaine is not None
check("du lundi au vendredi : 5 jours, bornes incluses", semaine == 5, sortie)
_, debut_avril, sortie = jours("01/04/2026", "03/04/2026")
check("« 01/04/2026 » est le 1er avril (3 jours ouvrés jusqu'au 3)", debut_avril == 3, sortie)
_, week_end, sortie = jours("10/04/2026", "13/04/2026")
check("samedi et dimanche ne comptent pas (vendredi au lundi : 2)", week_end == 2, sortie)
_, premier_mai, sortie_mai = jours("27/04/2026", "01/05/2026")
_, assomption, sortie_aout = jours("10/08/2026", "16/08/2026")
check(
    "les fériés ne comptent pas, un férié un samedi n'est retiré qu'une fois",
    premier_mai == 4 and assomption == 5,
    sortie_mai + sortie_aout,
)
_, bissextile, sortie = jours("26/02/2028", "01/03/2028")
check("année bissextile et changement de mois (26/02/2028 au 01/03/2028 : 3)", bissextile == 3, sortie)
code, _, sortie = jours("17/04/2026", "13/04/2026")
check(
    "une fin avant le début est refusée sans plantage",
    existe and code != 0 and "Traceback" not in sortie,
    sortie,
)
code, _, sortie = jours("31/04/2026", "05/05/2026")
check(
    "une date impossible (31/04) est refusée sans plantage",
    existe and code != 0 and "Traceback" not in sortie,
    sortie,
)

shutil.copyfile(original, project / "donnees.json")
pose, sortie_pose, erreur_pose = commande("poser", "Awa", "13/04/2026", "17/04/2026")
_, liste, _ = commande("liste")
check(
    "`poser` retire les jours ouvrés du solde, et l'ancien fichier se lit toujours",
    pose == 0 and re.search(r"Awa\s*:\s*20\b", liste) and "Chloé" in liste,
    sortie_pose + erreur_pose + liste,
)
refus, sortie_refus, erreur_refus = commande("poser", "Bilal", "13/04/2026", "17/04/2026")
_, liste, _ = commande("liste")
check(
    "un solde insuffisant est refusé, et le solde reste le même",
    existe and refus != 0 and re.search(r"Bilal\s*:\s*2\b", liste) and "Traceback" not in erreur_refus,
    sortie_refus + erreur_refus + liste,
)
shutil.copyfile(original, project / "donnees.json")

aide = (project / "conges.py").read_text(encoding="utf-8", errors="replace")
docstring = aide.split('"""')[1] if aide.count('"""') >= 2 else ""
check(
    "l'aide de la commande cite `jours` et le format des dates",
    "jours" in docstring and re.search(r"JJ/MM|jj/mm", docstring),
)

print(json.dumps({"score": sum(c["ok"] for c in checks), "max": len(checks), "checks": checks}, ensure_ascii=False))
