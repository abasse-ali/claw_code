import json
import threading
import urllib.error
import urllib.request

import pytest

import serveur


@pytest.fixture
def adresse():
    instance = serveur.demarrer(0)
    fil = threading.Thread(target=instance.serve_forever, daemon=True)
    fil.start()
    yield f"http://127.0.0.1:{instance.server_address[1]}"
    instance.shutdown()
    instance.server_close()


def lire(url):
    with urllib.request.urlopen(url, timeout=10) as reponse:
        return reponse.status, json.loads(reponse.read().decode("utf-8"))


def test_toutes_les_recettes_triees(adresse):
    statut, recettes = lire(adresse + "/api/recettes")
    assert statut == 200
    noms = [recette["nom"] for recette in recettes]
    assert len(noms) == 9
    assert noms == sorted(noms)


def test_une_recette(adresse):
    statut, recette = lire(adresse + "/api/recettes/r3")
    assert statut == 200
    assert recette["nom"] == "Crème brûlée"


def test_recette_inconnue(adresse):
    with pytest.raises(urllib.error.HTTPError) as erreur:
        lire(adresse + "/api/recettes/r99")
    assert erreur.value.code == 404


def test_la_page_est_servie(adresse):
    with urllib.request.urlopen(adresse + "/", timeout=10) as reponse:
        assert "Carnet de recettes" in reponse.read().decode("utf-8")
