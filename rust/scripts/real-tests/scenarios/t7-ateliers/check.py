"""Vérification cachée de t7-ateliers : python check.py DOSSIER_DU_PROJET

Tout passe par la ligne de commande (`ateliers.py`), sur les données de départ remises en
place. Exigences dites : `inscrire ATELIER PERSONNE` inscrit tant qu'il reste des places,
`liste` montre les inscrits de chaque atelier, pas deux fois la même personne au même
atelier, tests. Exigences implicites : les ateliers de l'ancien fichier (sans inscrits)
acceptent des inscriptions, un atelier complet refuse sans rien changer, la même personne
écrite autrement (majuscules, espaces autour) reste la même, atelier inconnu et commande
incomplète refusés sans plantage, nom accentué intact, atelier sans place, aide à jour.
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
        [sys.executable, "ateliers.py", *arguments], cwd=project, capture_output=True,
        timeout=60, env=ENV,
    )
    return (
        result.returncode,
        result.stdout.decode("utf-8", "replace"),
        result.stderr.decode("utf-8", "replace"),
    )


def depart():
    shutil.copyfile(original, project / "donnees.json")


def liste():
    _, sortie, erreur = commande("liste")
    return sortie + erreur


def bloc(texte, atelier):
    """Les lignes de `liste` qui parlent de cet atelier, et celles qui les suivent jusqu'au
    suivant (les inscrits peuvent être sur la même ligne ou en dessous)."""
    autres = [nom for nom in ("Poterie", "Couture", "Théâtre", "Yoga") if nom != atelier]
    lignes = texte.splitlines()
    garde, dedans = [], False
    for ligne in lignes:
        if atelier in ligne:
            dedans = True
        elif any(nom in ligne for nom in autres):
            dedans = False
        if dedans:
            garde.append(ligne)
    return "\n".join(garde)


tests = subprocess.run(
    [sys.executable, "-m", "pytest", "-q", "-p", "no:cacheprovider"],
    cwd=project, capture_output=True, text=True, timeout=180, env=ENV,
)
check("les tests du projet passent", tests.returncode == 0, tests.stdout)
sources = "".join(
    path.read_text(encoding="utf-8", errors="replace") for path in project.rglob("test_*.py")
)
check(
    "des tests des inscriptions ont été ajoutés",
    len(re.findall(r"def test_", sources)) > 4 and re.search(r"inscri", sources),
)

depart()
code, sortie, erreur = commande("inscrire", "Poterie", "Awa")
vue = liste()
# Sans la commande, tout appel est « refusé » : les refus ne comptent que si elle existe.
existe = code == 0 and "Awa" in bloc(vue, "Poterie")
check(
    "une inscription dans un atelier de l'ancien fichier, et `liste` la montre",
    existe,
    sortie + erreur + vue,
)

commande("inscrire", "Poterie", "Bilal")
code, sortie, erreur = commande("inscrire", "Poterie", "Chloé")
vue = liste()
poterie = bloc(vue, "Poterie")
check(
    "un atelier complet refuse sans plantage, et ses inscrits restent les mêmes",
    existe and code != 0 and "Traceback" not in erreur
    and "Awa" in poterie and "Bilal" in poterie and "Chloé" not in poterie,
    sortie + erreur + vue,
)

depart()
commande("inscrire", "Couture", "Zoé")
code, sortie, erreur = commande("inscrire", "Couture", "Zoé")
vue = liste()
check(
    "la même personne n'est pas inscrite deux fois",
    existe and code != 0 and "Traceback" not in erreur and bloc(vue, "Couture").count("Zoé") == 1,
    sortie + erreur + vue,
)
code_casse, sortie_casse, erreur_casse = commande("inscrire", "Couture", "zoé")
code_espaces, sortie_espaces, erreur_espaces = commande("inscrire", "Couture", "  Zoé  ")
vue = liste()
couture = bloc(vue, "Couture").lower()
check(
    "la même personne écrite autrement (« zoé », espaces autour) n'est pas réinscrite",
    existe and code_casse != 0 and code_espaces != 0 and couture.count("zoé") == 1,
    sortie_casse + erreur_casse + sortie_espaces + erreur_espaces + vue,
)

depart()
code, sortie, erreur = commande("inscrire", "Danse", "Awa")
check(
    "un atelier inconnu est refusé sans plantage",
    existe and code != 0 and "Traceback" not in erreur and "Danse" not in liste(),
    sortie + erreur,
)

code, sortie, erreur = commande("inscrire", "Théâtre", "Chloé")
vue = liste()
check("un nom accentué arrive intact (« Chloé »)", code == 0 and "Chloé" in bloc(vue, "Théâtre"), sortie + erreur + vue)

commande("inscrire", "Couture", "Bilal")
vue = liste()
check(
    "`liste` montre les inscrits de chaque atelier",
    existe and "Chloé" in bloc(vue, "Théâtre") and "Bilal" in bloc(vue, "Couture"),
    vue,
)

depart()
commande("creer", "Yoga", "0")
code, sortie, erreur = commande("inscrire", "Yoga", "Awa")
check(
    "un atelier sans place refuse toute inscription",
    existe and code != 0 and "Traceback" not in erreur and "Awa" not in bloc(liste(), "Yoga"),
    sortie + erreur,
)

depart()
code, sortie, erreur = commande("inscrire", "Poterie")
check(
    "une inscription incomplète montre l'aide, sans plantage",
    existe and code == 2 and "Traceback" not in erreur,
    sortie + erreur,
)
depart()

aide = (project / "ateliers.py").read_text(encoding="utf-8", errors="replace")
docstring = aide.split('"""')[1] if aide.count('"""') >= 2 else ""
check("l'aide de la commande cite `inscrire`", "inscrire" in docstring)

print(json.dumps({"score": sum(c["ok"] for c in checks), "max": len(checks), "checks": checks}, ensure_ascii=False))
