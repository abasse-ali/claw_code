"""Jours fériés de l'équipe, lus dans feries.txt (une date AAAA-MM-JJ par ligne)."""

from datetime import date
from pathlib import Path

FICHIER = Path(__file__).resolve().parent.parent / "feries.txt"


def lire_feries():
    feries = set()
    for ligne in FICHIER.read_text(encoding="utf-8").splitlines():
        ligne = ligne.strip()
        if ligne and not ligne.startswith("#"):
            feries.add(date.fromisoformat(ligne))
    return feries
