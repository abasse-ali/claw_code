"""Outils sur de grandes listes de valeurs hachables."""


def sans_doublons(elements):
    """Renvoie les éléments sans doublon, dans l'ordre de première apparition."""
    resultat = []
    for element in elements:
        if element not in resultat:
            resultat.append(element)
    return resultat


def paires_somme(nombres, cible):
    """Nombre de paires d'indices (i < j) telles que nombres[i] + nombres[j] == cible."""
    total = 0
    for i in range(len(nombres)):
        for j in range(i + 1, len(nombres)):
            if nombres[i] + nombres[j] == cible:
                total += 1
    return total
