"""Texte d'une facture."""
from commande import total_commande


def ligne_total(lignes, devise="EUR"):
    """Dernière ligne de la facture : « Total : 12.00 EUR »."""
    return f"Total : {total_commande(lignes):.2f} {devise}"
