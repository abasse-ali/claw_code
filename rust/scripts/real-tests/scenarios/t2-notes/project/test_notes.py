import notes


def test_ajouter_puis_lister(tmp_path):
    fichier = tmp_path / "notes.json"
    notes.ajouter("acheter du pain", fichier)
    notes.ajouter("appeler Léa", fichier)
    assert notes.lister(fichier) == [(1, "acheter du pain"), (2, "appeler Léa")]


def test_supprimer(tmp_path):
    fichier = tmp_path / "notes.json"
    notes.ajouter("a", fichier)
    notes.ajouter("b", fichier)
    assert notes.supprimer(1, fichier) == "a"
    assert notes.lister(fichier) == [(1, "b")]
