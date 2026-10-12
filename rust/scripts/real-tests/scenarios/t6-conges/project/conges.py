"""Congés de l'équipe.

Utilisation :
    python conges.py poser NOM DEBUT FIN
    python conges.py liste

Code de retour : 0 si c'est fait, 1 si c'est refusé, 2 si la commande est mal écrite.
"""

import sys
from datetime import date

from equipe import donnees


def main(arguments):
    if not arguments:
        print(__doc__)
        return 2
    commande, *reste = arguments
    etat = donnees.charger()
    if commande == "poser" and len(reste) == 3:
        nom, debut, fin = reste
        if nom not in etat["soldes"]:
            print(f"Refusé : {nom} ne fait pas partie de l'équipe")
            return 1
        debut, fin = date.fromisoformat(debut), date.fromisoformat(fin)
        jours = (fin - debut).days
        etat["soldes"][nom] -= jours
        etat["conges"].append({"nom": nom, "debut": debut.isoformat(), "fin": fin.isoformat()})
        donnees.enregistrer(etat)
        print(f"Congé posé pour {nom} : {jours} jours.")
        return 0
    if commande == "liste" and not reste:
        for nom, solde in etat["soldes"].items():
            print(f"{nom} : {solde}")
        for conge in etat["conges"]:
            print(f"{conge['nom']} du {conge['debut']} au {conge['fin']}")
        return 0
    print(__doc__)
    return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
