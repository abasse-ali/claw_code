"""Export CSV d'une commande."""
from commande import TVA
from tarifs import prix_ttc


def exporter(lignes):
    """Une ligne « prix_ht;quantite;prix_ttc » par article."""
    return [f"{prix};{quantite};{prix_ttc(prix, TVA)}" for prix, quantite in lignes]
