"""Découpage d'une expression arithmétique."""


def lexemes(texte):
    """Renvoie la liste des nombres (float) et des symboles + - * / du texte."""
    resultat = []
    i = 0
    while i < len(texte):
        caractere = texte[i]
        if caractere.isspace():
            i += 1
        elif caractere.isdigit() or caractere == ".":
            j = i
            while j < len(texte) and (texte[j].isdigit() or texte[j] == "."):
                j += 1
            resultat.append(float(texte[i:j]))
            i = j
        elif caractere in "+-*/":
            resultat.append(caractere)
            i += 1
        else:
            raise ValueError(f"caractère inattendu : {caractere!r}")
    return resultat
