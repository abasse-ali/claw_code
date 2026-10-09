"""Total d'une commande : liste de (prix_ht, quantite)."""
from tarifs import prix_ttc

TVA = 0.2


def total_commande(lignes):
    """Somme des prix TTC de chaque ligne (prix unitaire TTC x quantité)."""
    return sum(prix_ttc(prix, TVA) * quantite for prix, quantite in lignes)
