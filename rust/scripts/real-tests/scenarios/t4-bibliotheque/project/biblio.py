"""Guichet de la bibliothèque.

Utilisation :
    python biblio.py emprunter MEMBRE LIVRE
    python biblio.py rendre LIVRE
    python biblio.py etat

Code de retour : 0 si l'opération est faite, 1 si elle est refusée, 2 si la
commande est mal écrite.
"""

import sys

from bibliotheque import services, stockage


def main(arguments):
    if not arguments:
        print(__doc__)
        return 2
    commande, *reste = arguments
    etat = stockage.charger()
    try:
        if commande == "emprunter" and len(reste) == 2:
            services.emprunter(etat, reste[0], reste[1])
            print(f"{reste[0]} emprunte {etat.livres[reste[1]].titre}.")
        elif commande == "rendre" and len(reste) == 1:
            services.rendre(etat, reste[0])
            print(f"{etat.livres[reste[0]].titre} est rendu.")
        elif commande == "etat" and not reste:
            for ligne in services.resume(etat):
                print(ligne)
            return 0
        else:
            print(__doc__)
            return 2
    except services.Refus as refus:
        print(f"Refusé : {refus}")
        return 1
    stockage.enregistrer(etat)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
