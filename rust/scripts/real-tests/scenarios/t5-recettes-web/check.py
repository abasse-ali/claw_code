"""Vérification cachée de t5-recettes-web : python check.py DOSSIER_DU_PROJET

Le serveur du projet est lancé comme l'utilisateur le lancerait (`python serveur.py
PORT`), puis interrogé. Exigences dites : `GET /api/recettes?q=` filtre sur le nom ou un
ingrédient, un champ en haut de la page filtre pendant qu'on tape, tests. Exigences
implicites : majuscules et accents ignorés (« creme » trouve « crème »), plusieurs mots
(`%20` comme `+`), recherche vide ou blanche = toutes les recettes, aucune correspondance =
liste vide et non erreur, résultats toujours triés et entiers, routes existantes
inchangées, texte de la recherche encodé dans l'URL et pas d'injection de HTML.
"""

import json
import os
import re
import socket
import subprocess
import sys
import time
import urllib.error
import urllib.request
from pathlib import Path

project = Path(sys.argv[1])
checks = []
ENV = dict(os.environ, PYTHONIOENCODING="utf-8")
TOUTES = 9


def check(name, ok, detail=""):
    checks.append({"name": name, "ok": bool(ok), "detail": detail[-300:] if detail else ""})


def port_libre():
    with socket.socket() as essai:
        essai.bind(("127.0.0.1", 0))
        return essai.getsockname()[1]


def lire(base, chemin):
    """(statut, données JSON ou texte) ; statut 0 si le serveur ne répond pas."""
    try:
        with urllib.request.urlopen(base + chemin, timeout=10) as reponse:
            corps = reponse.read().decode("utf-8", "replace")
            statut = reponse.status
    except urllib.error.HTTPError as erreur:
        corps = erreur.read().decode("utf-8", "replace")
        statut = erreur.code
    except (OSError, ValueError) as erreur:
        return 0, str(erreur)
    try:
        return statut, json.loads(corps)
    except json.JSONDecodeError:
        return statut, corps


def noms(donnees):
    if not isinstance(donnees, list):
        return None
    return [recette.get("nom") if isinstance(recette, dict) else recette for recette in donnees]


tests = subprocess.run(
    [sys.executable, "-m", "pytest", "-q", "-p", "no:cacheprovider"],
    cwd=project, capture_output=True, text=True, timeout=180, env=ENV,
)
check("les tests du projet passent", tests.returncode == 0, tests.stdout)
sources = "".join(
    path.read_text(encoding="utf-8", errors="replace") for path in project.rglob("test_*.py")
)
check(
    "des tests de la recherche ont été ajoutés",
    len(re.findall(r"def test_", sources)) > 4 and re.search(r"q=|recherch|cherch|filtr", sources),
)

port = port_libre()
base = f"http://127.0.0.1:{port}"
serveur = subprocess.Popen(
    [sys.executable, "serveur.py", str(port)], cwd=project, env=ENV,
    stdout=subprocess.PIPE, stderr=subprocess.PIPE,
)
try:
    pret = False
    for _ in range(60):
        if serveur.poll() is not None:
            break
        statut, _ = lire(base, "/api/recettes")
        if statut:
            pret = True
            break
        time.sleep(0.25)

    statut, pommes = lire(base, "/api/recettes?q=pomme")
    existe = pret and statut == 200 and noms(pommes) == ["Gratin dauphinois", "Tarte aux pommes"]
    check(
        "`?q=pomme` trouve par le nom et par un ingrédient (pommes de terre)",
        existe, f"{statut} {pommes}"[:300],
    )
    statut, tarte = lire(base, "/api/recettes?q=TARTE")
    check("les majuscules sont ignorées", existe and noms(tarte) == ["Tarte aux pommes"], f"{statut} {tarte}"[:300])
    statut, creme = lire(base, "/api/recettes?q=creme")
    statut2, nicoise = lire(base, "/api/recettes?q=nicoise")
    check(
        "les accents sont ignorés (creme, nicoise)",
        existe
        and noms(creme) == ["Crème brûlée", "Gratin dauphinois", "Quiche lorraine"]
        and noms(nicoise) == ["Salade niçoise"],
        f"{statut} {noms(creme)} {statut2} {noms(nicoise)}"[:300],
    )
    statut, espaces = lire(base, "/api/recettes?q=pommes%20de%20terre")
    statut2, plus = lire(base, "/api/recettes?q=pommes+de+terre")
    check(
        "plusieurs mots, encodés avec %20 comme avec +",
        existe and noms(espaces) == ["Gratin dauphinois"] and noms(plus) == ["Gratin dauphinois"],
        f"{statut} {noms(espaces)} {statut2} {noms(plus)}"[:300],
    )
    statut, vide = lire(base, "/api/recettes?q=")
    statut2, blanc = lire(base, "/api/recettes?q=%20%20")
    check(
        "une recherche vide ou blanche rend toutes les recettes",
        existe and len(noms(vide) or []) == TOUTES and len(noms(blanc) or []) == TOUTES,
        f"{statut} {len(noms(vide) or [])} {statut2} {len(noms(blanc) or [])}",
    )
    statut, rien = lire(base, "/api/recettes?q=chocolat")
    check("aucune correspondance : liste vide, pas une erreur", existe and statut == 200 and rien == [], f"{statut} {rien}"[:300])
    # Sans accent dans le mot cherché : ce point ne dépend pas du précédent.
    statut, lait = lire(base, "/api/recettes?q=lait")
    entieres = (
        isinstance(lait, list)
        and all(isinstance(recette, dict) and {"id", "nom", "ingredients"} <= set(recette) for recette in lait)
    )
    check(
        "résultats triés par nom et entiers (id, nom, ingrédients)",
        existe and entieres and noms(lait) == ["Clafoutis aux cerises", "Gratin dauphinois", "Quiche lorraine"],
        f"{statut} {lait}"[:300],
    )
    statut, toutes = lire(base, "/api/recettes")
    statut2, une = lire(base, "/api/recettes/r3")
    statut3, _ = lire(base, "/api/recettes/r99")
    check(
        "les routes existantes sont inchangées",
        existe and len(noms(toutes) or []) == TOUTES and statut2 == 200
        and isinstance(une, dict) and une.get("nom") == "Crème brûlée" and statut3 == 404,
        f"{statut} {statut2} {statut3}",
    )
finally:
    serveur.terminate()
    try:
        serveur.wait(timeout=10)
    except subprocess.TimeoutExpired:
        serveur.kill()

page = (project / "static" / "index.html").read_text(encoding="utf-8", errors="replace")
script = (project / "static" / "app.js").read_text(encoding="utf-8", errors="replace")
champ = re.search(r"<input\b", page) and page.find("<input") < page.find('id="liste"')
champ = champ or "createElement(\"input\")" in script or "createElement('input')" in script
ecoute = re.search(r"addEventListener\(\s*[\"'](input|keyup|search)[\"']|\.oninput\s*=", script)
check("un champ en haut de la page, qui réagit pendant qu'on tape", champ and ecoute)
envoie = re.search(r"q=|URLSearchParams", script)
encode = re.search(r"encodeURIComponent|URLSearchParams", script)
injecte = re.search(r"innerHTML\s*\+?=\s*(?![\"'`]{2}\s*[;\n])", script)
check(
    "le texte cherché est encodé dans l'URL et la page n'injecte pas de HTML",
    champ and ecoute and (not envoie or encode) and not injecte,
    "innerHTML" if injecte else "",
)

print(json.dumps({"score": sum(c["ok"] for c in checks), "max": len(checks), "checks": checks}, ensure_ascii=False))
