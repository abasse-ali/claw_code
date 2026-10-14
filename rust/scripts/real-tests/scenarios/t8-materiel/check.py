"""Vérification cachée de t8-materiel : python check.py DOSSIER_DU_PROJET

Projet créé de zéro dans un dossier vide (mesure du mode projet). Tout passe par la ligne de
commande (`materiel.py`), dans une copie du projet sans ses données ni ses dossiers générés,
comme chez un utilisateur qui vient de l'installer. Exigences dites : ajouter (ou augmenter
la quantité), prêter avec une date de retour, rendre, stock (disponibles sur le total),
retards à une date, données dans `materiel.json` à côté du programme, refus clair avec un
code d'erreur, tests, README. Exigences implicites : pas de prêt sans exemplaire disponible,
matériel inconnu, retour d'un prêt qui n'existe pas, date et quantité invalides refusées
sans plantage, retard compté à partir du lendemain de la date prévue, lancement depuis un
autre dossier, sortie sans UTF-8 forcé (terminal de l'utilisateur sous Windows) et nom
accentué intact.
"""

import json
import locale
import os
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

project = Path(sys.argv[1])
checks = []
ENV = dict(os.environ, PYTHONIOENCODING="utf-8")
# Le terminal de l'utilisateur : ni PYTHONIOENCODING ni PYTHONUTF8.
PLAIN = {
    name: value
    for name, value in os.environ.items()
    if name not in ("PYTHONIOENCODING", "PYTHONUTF8")
}
SKIP = {".git", ".claw", "__pycache__", ".pytest_cache", ".venv", "venv", "node_modules"}
COPIES = []


def check(name, ok, detail=""):
    checks.append({"name": name, "ok": bool(ok), "detail": detail[-300:] if detail else ""})


def fresh_copy():
    """Copie du projet sans ses données (`materiel.json`) ni dossiers générés."""
    base = Path(tempfile.mkdtemp(prefix="t8-copie-"))
    COPIES.append(base)
    target = base / "projet"

    def ignore(folder, names):
        return [name for name in names if name in SKIP or name == "materiel.json"]

    shutil.copytree(project, target, ignore=ignore)
    return target


def run(copy, *arguments, cwd=None, env=None, encodings=("utf-8",)):
    """Code de sortie et sortie ; avec plusieurs codages, la sortie lue de chaque façon."""
    result = subprocess.run(
        [sys.executable, str(copy / "materiel.py"), *arguments],
        cwd=cwd or copy, capture_output=True, timeout=60, env=env or ENV,
    )
    output = "\n".join(
        result.stdout.decode(encoding, "replace") + result.stderr.decode(encoding, "replace")
        for encoding in encodings
    )
    return result.returncode, output


def refused(code, output):
    """Refus propre : code d'erreur, un message, pas de trace Python."""
    return code != 0 and output.strip() != "" and "Traceback" not in output


def availability(output, name):
    """(disponibles, total) sur la ligne de `stock` qui nomme ce matériel."""
    for line in output.splitlines():
        if name.lower() in line.lower():
            pair = re.findall(r"(\d+)\s*(?:/|sur|of)\s*(\d+)", line)
            if pair:
                return tuple(int(number) for number in pair[0])
            numbers = re.findall(r"\d+", line)
            if len(numbers) >= 2:
                return int(numbers[0]), int(numbers[1])
    return None


def stock(copy, name):
    _, output = run(copy, "stock")
    return availability(output, name), output


if not (project / "materiel.py").is_file():
    for name in (
        "README qui explique l'installation et chaque commande",
        "les tests du projet passent, dans une copie sans données",
        "ajouter puis stock : Projecteur 2/2, nom tel qu'écrit",
        "données dans materiel.json à côté du programme, lancé d'un autre dossier",
        "prêter : un exemplaire de moins",
        "ajouter un matériel existant augmente sa quantité",
        "plus d'exemplaire disponible : prêt refusé",
        "matériel inconnu : prêt refusé",
        "rendre : l'exemplaire revient ; rendre un prêt inexistant est refusé",
        "retards : en retard le lendemain de la date prévue, plus après le retour",
        "date, quantité et date de retards invalides refusées sans plantage",
        "sortie dans un tube sans UTF-8 forcé : pas de plantage, accents lisibles, données en UTF-8",
    ):
        check(name, False, "materiel.py absent")
    score = sum(item["ok"] for item in checks)
    print(json.dumps({"score": score, "max": len(checks), "checks": checks}))
    sys.exit(0)

# 1. README.
readme = next(
    (project / name for name in ("README.md", "readme.md", "README.txt", "README") if (project / name).is_file()),
    None,
)
readme_text = readme.read_text(encoding="utf-8", errors="replace") if readme else ""
commands_named = sum(word in readme_text for word in ("ajouter", "preter", "rendre", "stock", "retards"))
check(
    "README qui explique l'installation et chaque commande",
    "materiel.py" in readme_text and commands_named >= 5,
    readme_text[:300],
)

# 2. Tests du projet, dans une copie sans données.
copy = fresh_copy()
sources = "".join(
    path.read_text(encoding="utf-8", errors="replace")
    for path in copy.rglob("*.py")
    if path.name.startswith("test_") or path.name.endswith("_test.py")
)
tests = subprocess.run(
    [sys.executable, "-m", "pytest", "-q", "-p", "no:cacheprovider"],
    cwd=copy, capture_output=True, text=True, encoding="utf-8", errors="replace",
    timeout=300, env=ENV,
)
check(
    "les tests du projet passent, dans une copie sans données",
    tests.returncode == 0 and len(re.findall(r"def test_", sources)) >= 5,
    tests.stdout[-300:],
)

# 3 à 11 : un même stock, de commande en commande, comme un utilisateur.
copy = fresh_copy()
code, output = run(copy, "ajouter", "Projecteur", "2")
found, listing = stock(copy, "Projecteur")
# Le nom affiché est celui que l'utilisateur a écrit (comparer sans la casse ne
# permet pas de le changer) ; les autres points comparent sans la casse.
check(
    "ajouter puis stock : Projecteur 2/2, nom tel qu'écrit",
    code == 0 and found == (2, 2) and "Projecteur" in listing,
    output + listing,
)

# 4. Lancé depuis un autre dossier : les données vont à côté du programme.
elsewhere = fresh_copy()
outside = elsewhere.parent / "ailleurs"
outside.mkdir()
code, output = run(elsewhere, "ajouter", "Tente", "3", cwd=outside)
found, listing = stock(elsewhere, "Tente")
check(
    "données dans materiel.json à côté du programme, lancé d'un autre dossier",
    code == 0
    and (elsewhere / "materiel.json").is_file()
    and not (outside / "materiel.json").exists()
    and found == (3, 3),
    output + listing,
)

code, output = run(copy, "preter", "Projecteur", "Alice", "2026-11-01")
found, listing = stock(copy, "Projecteur")
check("prêter : un exemplaire de moins", code == 0 and found == (1, 2), output + listing)

code, output = run(copy, "ajouter", "Projecteur", "2")
found, listing = stock(copy, "Projecteur")
check("ajouter un matériel existant augmente sa quantité", code == 0 and found == (3, 4), output + listing)

lent = [run(copy, "preter", "Projecteur", person, "2026-11-01")[0] for person in ("Bob", "Chloé", "David")]
code, output = run(copy, "preter", "Projecteur", "Emma", "2026-11-01")
found, listing = stock(copy, "Projecteur")
check(
    "plus d'exemplaire disponible : prêt refusé",
    lent == [0, 0, 0] and refused(code, output) and found == (0, 4),
    output + listing,
)

code, output = run(copy, "preter", "Trampoline", "Alice", "2026-11-01")
check("matériel inconnu : prêt refusé", refused(code, output), output)

code, output = run(copy, "rendre", "Projecteur", "Alice")
found, listing = stock(copy, "Projecteur")
code_bis, output_bis = run(copy, "rendre", "Projecteur", "Zoé")
found_bis, _ = stock(copy, "Projecteur")
check(
    "rendre : l'exemplaire revient ; rendre un prêt inexistant est refusé",
    code == 0 and found == (1, 4) and refused(code_bis, output_bis) and found_bis == (1, 4),
    output + listing + output_bis,
)

_, on_day = run(copy, "retards", "2026-11-01")
_, next_day = run(copy, "retards", "2026-11-02")
run(copy, "rendre", "Projecteur", "Bob")
_, after_return = run(copy, "retards", "2026-11-02")
on_day, next_day, after_return = (text.lower() for text in (on_day, next_day, after_return))
check(
    "retards : en retard le lendemain de la date prévue, plus après le retour",
    "bob" not in on_day
    and "bob" in next_day
    and "projecteur" in next_day
    and "bob" not in after_return
    and "chloé" in after_return,
    on_day + next_day + after_return,
)

attempts = [
    run(copy, "preter", "Projecteur", "Fanny", "2026-13-01"),
    run(copy, "ajouter", "Corde", "abc"),
    run(copy, "ajouter", "Corde", "-1"),
    run(copy, "retards", "demain"),
]
found, listing = stock(copy, "Corde")
check(
    "date, quantité et date de retards invalides refusées sans plantage",
    all(refused(code, output) for code, output in attempts) and found is None,
    "\n".join(output for _, output in attempts),
)

# 12. Sortie dans un tube ou un fichier, sans UTF-8 forcé (terminal de Git Bash, `>
# fichier.txt` sous Windows) : Python y écrit en cp1252 si le programme ne choisit pas
# l'UTF-8. Les deux sont acceptés ; un plantage (caractère hors cp1252) ne l'est pas.
plain = fresh_copy()
encodings = ("utf-8", locale.getpreferredencoding(False))
name = "Scie sauteuse électrique"
results = [
    run(plain, "ajouter", name, "1", env=PLAIN, encodings=encodings),
    run(plain, "preter", name, "Hélène", "2026-11-01", env=PLAIN, encodings=encodings),
    run(plain, "stock", env=PLAIN, encodings=encodings),
    run(plain, "retards", "2026-11-02", env=PLAIN, encodings=encodings),
]
try:
    stored = json.loads((plain / "materiel.json").read_bytes().decode("utf-8"))
    stored_ok = name.lower() in json.dumps(stored, ensure_ascii=False).lower()
except (OSError, ValueError) as error:
    stored_ok = False
    results.append((1, f"materiel.json : {error}"))
check(
    "sortie dans un tube sans UTF-8 forcé : pas de plantage, accents lisibles, données en UTF-8",
    all(code == 0 and "Traceback" not in output for code, output in results[:4])
    and name.lower() in results[2][1].lower()
    and "hélène" in results[3][1].lower()
    and stored_ok,
    "\n".join(output for _, output in results),
)

for base in COPIES:
    shutil.rmtree(base, ignore_errors=True)
score = sum(item["ok"] for item in checks)
# En ASCII échappé : le rapport passe quel que soit le codage de la sortie.
print(json.dumps({"score": score, "max": len(checks), "checks": checks}))
