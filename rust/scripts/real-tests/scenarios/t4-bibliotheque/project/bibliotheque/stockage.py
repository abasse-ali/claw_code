"""Lecture et écriture de l'état dans un fichier JSON."""

import json
from pathlib import Path

from bibliotheque.modeles import Etat, Livre

FICHIER = Path(__file__).resolve().parent.parent / "donnees.json"


def charger(chemin=FICHIER):
    brut = json.loads(Path(chemin).read_text(encoding="utf-8"))
    return Etat(
        livres={livre["code"]: Livre(livre["code"], livre["titre"]) for livre in brut["livres"]},
        membres=list(brut["membres"]),
        emprunts=dict(brut["emprunts"]),
    )


def enregistrer(etat, chemin=FICHIER):
    brut = {
        "livres": [{"code": livre.code, "titre": livre.titre} for livre in etat.livres.values()],
        "membres": etat.membres,
        "emprunts": etat.emprunts,
    }
    Path(chemin).write_text(json.dumps(brut, ensure_ascii=False, indent=2), encoding="utf-8")
