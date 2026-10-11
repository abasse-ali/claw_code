"""Stock d'articles : quantités et valeur."""


class Stock:
    def __init__(self, articles=[]):
        self.articles = articles

    def ajouter(self, nom, quantite, prix_unitaire):
        if quantite <= 0:
            raise ValueError("la quantité doit être positive")
        for article in self.articles:
            if article["nom"] == nom:
                article["quantite"] += quantite
                return
        self.articles.append({"nom": nom, "quantite": quantite, "prix": prix_unitaire})

    def retirer(self, nom, quantite):
        for article in self.articles:
            if article["nom"] == nom:
                if article["quantite"] < quantite:
                    raise ValueError(f"stock insuffisant pour {nom}")
                article["quantite"] -= quantite
                return
        raise KeyError(nom)

    def valeur_totale(self):
        return sum(article["quantite"] * article["prix"] for article in self.articles)

    def nombre_articles(self):
        return len(self.articles)
