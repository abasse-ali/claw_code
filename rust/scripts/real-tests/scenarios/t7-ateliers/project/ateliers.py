"""Ateliers du centre social.

Utilisation :
    python ateliers.py creer ATELIER PLACES
    python ateliers.py liste

Code de retour : 0 si c'est fait, 1 si c'est refusé, 2 si la commande est mal écrite.
"""

import sys

from centre import donnees


def main(arguments):
    if not arguments:
        print(__doc__)
        return 2
    commande, *reste = arguments
    etat = donnees.charger()
    if commande == "creer" and len(reste) == 2:
        nom, places = reste
        if nom in etat["ateliers"]:
            print(f"Refusé : l'atelier {nom} existe déjà")
            return 1
        etat["ateliers"][nom] = {"places": int(places)}
        donnees.enregistrer(etat)
        print(f"Atelier {nom} créé : {places} places.")
        return 0
    if commande == "liste" and not reste:
        for nom, atelier in etat["ateliers"].items():
            print(f"{nom} : {atelier['places']} places")
        return 0
    print(__doc__)
    return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
