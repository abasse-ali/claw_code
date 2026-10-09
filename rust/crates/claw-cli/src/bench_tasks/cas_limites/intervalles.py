def fusionner(intervalles):
    """Fusionne des intervalles fermés (debut, fin).

    - L'entrée est une liste de couples (debut, fin), dans un ordre quelconque.
    - Deux intervalles qui se chevauchent ou se touchent, comme (1, 3) et (3, 5),
      sont fusionnés en un seul.
    - Le résultat est une nouvelle liste de tuples, triée par début ; la liste
      reçue n'est pas modifiée.
    - Un intervalle dont le début est après la fin est invalide : ValueError.
    """
    raise NotImplementedError
