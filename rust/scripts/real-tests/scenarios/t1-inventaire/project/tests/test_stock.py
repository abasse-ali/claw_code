from inventaire.stock import Stock


def test_ajouter_un_article():
    stock = Stock()
    stock.ajouter("vis", 10, 0.5)
    assert stock.nombre_articles() == 1
    assert stock.valeur_totale() == 5.0


def test_ajouter_deux_fois_le_meme_article():
    stock = Stock()
    stock.ajouter("écrou", 4, 0.25)
    stock.ajouter("écrou", 6, 0.25)
    assert stock.nombre_articles() == 1
    assert stock.valeur_totale() == 2.5


def test_retirer():
    stock = Stock()
    stock.ajouter("clou", 20, 0.1)
    stock.retirer("clou", 17)
    assert stock.valeur_totale() == 0.3
