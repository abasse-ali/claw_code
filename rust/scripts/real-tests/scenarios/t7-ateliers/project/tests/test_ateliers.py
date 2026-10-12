import json

import pytest

import ateliers
from centre import donnees


@pytest.fixture
def etat(tmp_path, monkeypatch):
    fichier = tmp_path / "donnees.json"
    fichier.write_text(json.dumps({"ateliers": {"Poterie": {"places": 2}}}), encoding="utf-8")
    monkeypatch.setattr(donnees, "FICHIER", fichier)
    return fichier


def test_la_liste_montre_les_ateliers(etat, capsys):
    assert ateliers.main(["liste"]) == 0
    assert "Poterie : 2 places" in capsys.readouterr().out


def test_creer_un_atelier(etat, capsys):
    assert ateliers.main(["creer", "Couture", "8"]) == 0
    assert "Couture" in json.loads(etat.read_text(encoding="utf-8"))["ateliers"]


def test_un_atelier_existant_n_est_pas_recree(etat):
    assert ateliers.main(["creer", "Poterie", "5"]) == 1


def test_une_commande_mal_ecrite(etat):
    assert ateliers.main(["creer", "Poterie"]) == 2
