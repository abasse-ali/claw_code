"""Évaluation d'une expression arithmétique."""
from lexeur import lexemes


def evaluer(texte):
    """Évalue l'expression et renvoie un float.

    Version actuelle : calcule de gauche à droite, sans priorités ni parenthèses.
    """
    elements = lexemes(texte)
    resultat = elements[0]
    i = 1
    while i < len(elements):
        operateur, valeur = elements[i], elements[i + 1]
        if operateur == "+":
            resultat += valeur
        elif operateur == "-":
            resultat -= valeur
        elif operateur == "*":
            resultat *= valeur
        else:
            resultat /= valeur
        i += 2
    return resultat
