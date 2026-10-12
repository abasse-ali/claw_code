"""Ateliers du centre et leurs places, dans donnees.json."""

import json
from pathlib import Path

FICHIER = Path(__file__).resolve().parent.parent / "donnees.json"


def charger():
    return json.loads(FICHIER.read_text(encoding="utf-8"))


def enregistrer(etat):
    FICHIER.write_text(json.dumps(etat, ensure_ascii=False, indent=2), encoding="utf-8")
