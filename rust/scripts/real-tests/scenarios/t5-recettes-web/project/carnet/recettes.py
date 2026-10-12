"""Les recettes du carnet, lues dans recettes.json."""

import json
from pathlib import Path

FICHIER = Path(__file__).resolve().parent.parent / "recettes.json"


def _lire():
    return json.loads(FICHIER.read_text(encoding="utf-8"))


def toutes():
    """Toutes les recettes, triées par nom."""
    return sorted(_lire(), key=lambda recette: recette["nom"])


def une(identifiant):
    """La recette de cet identifiant, ou None."""
    for recette in _lire():
        if recette["id"] == identifiant:
            return recette
    return None
