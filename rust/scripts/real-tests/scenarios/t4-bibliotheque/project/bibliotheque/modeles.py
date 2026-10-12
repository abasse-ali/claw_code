"""Ce que la bibliothèque connaît : ses livres, ses membres, qui a quoi."""

from dataclasses import dataclass, field


@dataclass
class Livre:
    code: str
    titre: str


@dataclass
class Etat:
    """État complet de la bibliothèque, tel qu'il est enregistré."""

    livres: dict = field(default_factory=dict)  # code -> Livre
    membres: list = field(default_factory=list)  # noms des membres
    emprunts: dict = field(default_factory=dict)  # code du livre -> nom du membre
