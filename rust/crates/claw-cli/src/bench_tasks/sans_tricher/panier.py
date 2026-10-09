"""Panier d'achats."""


def ajouter(article, panier=[]):
    """Ajoute un article au panier et renvoie ce panier.

    Sans panier fourni, un nouveau panier est créé à chaque appel.
    """
    panier.append(article)
    return panier


def total(panier, prix):
    """Total du panier. Un article dont le prix est inconnu lève KeyError."""
    return sum(prix.get(article, 0) for article in panier)


def retirer(article, panier):
    """Retire une occurrence de l'article ; renvoie True s'il y était, sinon False."""
    panier.remove(article)
    return True
