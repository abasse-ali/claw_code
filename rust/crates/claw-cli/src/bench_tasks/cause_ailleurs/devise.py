"""Conversions et arrondis monétaires."""


def arrondir(montant, decimales=2):
    """Arrondit au plus proche (2,678 -> 2,68 ; -1,239 -> -1,24)."""
    facteur = 10 ** decimales
    return int(montant * facteur) / facteur


def convertir(montant, taux):
    """Convertit un montant avec un taux de change, arrondi au centime."""
    return arrondir(montant * taux)
