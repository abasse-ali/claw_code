"""Rapport des ventes converties en euros."""
from devise import convertir


def total_en_euros(lignes, taux):
    """Somme des montants (en dollars) convertis en euros, ligne par ligne."""
    return round(sum(convertir(montant, taux) for montant in lignes), 2)


def resume(lignes, taux):
    """Phrase de synthèse : nombre de lignes et total en euros."""
    total = total_en_euros(lignes, taux)
    return f"{len(lignes)} ligne(s), total {total:.2f} EUR"
