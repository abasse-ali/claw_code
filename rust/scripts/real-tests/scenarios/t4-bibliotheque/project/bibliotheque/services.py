"""Opérations de la bibliothèque. Une opération impossible lève `Refus`."""


class Refus(Exception):
    """Opération refusée ; le message est destiné à la personne au guichet."""


def _verifier(etat, nom=None, code=None):
    if nom is not None and nom not in etat.membres:
        raise Refus(f"{nom} n'est pas membre")
    if code is not None and code not in etat.livres:
        raise Refus(f"le livre {code} n'existe pas")


def emprunter(etat, nom, code):
    _verifier(etat, nom, code)
    if code in etat.emprunts:
        raise Refus(f"{etat.livres[code].titre} est déjà emprunté par {etat.emprunts[code]}")
    etat.emprunts[code] = nom


def rendre(etat, code):
    _verifier(etat, code=code)
    if code not in etat.emprunts:
        raise Refus(f"{etat.livres[code].titre} n'est pas emprunté")
    del etat.emprunts[code]


def resume(etat):
    """Une ligne par livre, dans l'ordre des codes."""
    lignes = []
    for code in sorted(etat.livres):
        livre = etat.livres[code]
        if code in etat.emprunts:
            lignes.append(f"{code} {livre.titre} : emprunté par {etat.emprunts[code]}")
        else:
            lignes.append(f"{code} {livre.titre} : disponible")
    return lignes
