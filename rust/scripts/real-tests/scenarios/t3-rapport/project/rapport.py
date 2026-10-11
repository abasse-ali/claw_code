"""Calcule la moyenne des ventes par magasin à partir de ventes.csv."""

import csv
from collections import defaultdict


def moyennes(chemin="ventes.csv"):
    totaux = defaultdict(float)
    comptes = defaultdict(int)
    with open(chemin, encoding="utf-8") as fichier:
        for ligne in csv.DictReader(fichier, delimiter=";"):
            totaux[ligne["magasin"]] += float(ligne["montant"])
            comptes[ligne["magasin"]] += 1
    return {magasin: totaux[magasin] / comptes[magasin] for magasin in totaux}


if __name__ == "__main__":
    for magasin, moyenne in sorted(moyennes().items()):
        print(f"{magasin} : {moyenne:.2f} €")
