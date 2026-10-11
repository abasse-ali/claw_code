"""Petit carnet de notes en ligne de commande.

Utilisation :
    python notes.py ajouter "texte de la note"
    python notes.py lister
    python notes.py supprimer NUMERO
"""

import json
import sys
from pathlib import Path

FICHIER = Path(__file__).with_name("notes.json")


def charger(fichier=FICHIER):
    if not fichier.exists():
        return []
    return json.loads(fichier.read_text(encoding="utf-8"))


def enregistrer(notes, fichier=FICHIER):
    fichier.write_text(json.dumps(notes, ensure_ascii=False, indent=2), encoding="utf-8")


def ajouter(texte, fichier=FICHIER):
    notes = charger(fichier)
    notes.append(texte)
    enregistrer(notes, fichier)
    return len(notes)


def lister(fichier=FICHIER):
    return list(enumerate(charger(fichier), start=1))


def supprimer(numero, fichier=FICHIER):
    notes = charger(fichier)
    if not 1 <= numero <= len(notes):
        raise IndexError(f"pas de note numéro {numero}")
    retiree = notes.pop(numero - 1)
    enregistrer(notes, fichier)
    return retiree


def main(arguments):
    if not arguments:
        print(__doc__)
        return 1
    commande, *reste = arguments
    if commande == "ajouter":
        numero = ajouter(" ".join(reste))
        print(f"Note {numero} ajoutée.")
    elif commande == "lister":
        for numero, texte in lister():
            print(f"{numero}. {texte}")
    elif commande == "supprimer":
        print(f"Supprimée : {supprimer(int(reste[0]))}")
    else:
        print(f"Commande inconnue : {commande}")
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
