import json

import pytest

import conges
from equipe import donnees


@pytest.fixture
def etat(tmp_path, monkeypatch):
    fichier = tmp_path / "donnees.json"
    fichier.write_text(
        json.dumps({"soldes": {"Awa": 25, "Bilal": 2}, "conges": []}), encoding="utf-8"
    )
    monkeypatch.setattr(donnees, "FICHIER", fichier)
    return fichier


def test_la_liste_montre_les_soldes(etat, capsys):
    assert conges.main(["liste"]) == 0
    assert "Awa : 25" in capsys.readouterr().out


def test_un_inconnu_ne_pose_pas_de_conge(etat):
    assert conges.main(["poser", "Zoé", "2026-04-13", "2026-04-17"]) == 1


def test_une_commande_mal_ecrite(etat):
    assert conges.main(["poser", "Awa"]) == 2
