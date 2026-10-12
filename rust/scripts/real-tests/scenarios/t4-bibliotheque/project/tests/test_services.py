import pytest

from bibliotheque import services
from bibliotheque.modeles import Etat, Livre


def etat_de_test():
    return Etat(
        livres={"L1": Livre("L1", "Le Horla"), "L2": Livre("L2", "Germinal")},
        membres=["Awa", "Bilal"],
        emprunts={},
    )


def test_emprunter_puis_rendre():
    etat = etat_de_test()
    services.emprunter(etat, "Awa", "L1")
    assert etat.emprunts == {"L1": "Awa"}
    services.rendre(etat, "L1")
    assert etat.emprunts == {}


def test_un_livre_emprunte_ne_se_prete_pas_deux_fois():
    etat = etat_de_test()
    services.emprunter(etat, "Awa", "L1")
    with pytest.raises(services.Refus):
        services.emprunter(etat, "Bilal", "L1")


def test_membre_ou_livre_inconnu():
    etat = etat_de_test()
    with pytest.raises(services.Refus):
        services.emprunter(etat, "Zoé", "L1")
    with pytest.raises(services.Refus):
        services.emprunter(etat, "Awa", "L9")


def test_resume():
    etat = etat_de_test()
    services.emprunter(etat, "Awa", "L1")
    assert services.resume(etat) == [
        "L1 Le Horla : emprunté par Awa",
        "L2 Germinal : disponible",
    ]
