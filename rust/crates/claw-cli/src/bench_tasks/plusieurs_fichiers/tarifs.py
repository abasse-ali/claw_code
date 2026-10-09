"""Calcul des prix."""


def prix_ttc(prix_ht, taux_tva):
    """Prix toutes taxes comprises, arrondi au centime."""
    return round(prix_ht * (1 + taux_tva), 2)
