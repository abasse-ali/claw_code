"""Vérifie la bibliothèque de l'outil `Font` (crates/tools/src/fonts.rs) contre Google Fonts :
chaque police, chaque graisse annoncée, et l'italique quand il est annoncé, doivent être servis
(une graisse absente fait refuser toute la feuille de style par Google Fonts).

    python3 rust/scripts/fonts-check.py
"""

import re
import sys
import urllib.error
import urllib.request
from pathlib import Path

SOURCE = Path(__file__).resolve().parents[1] / "crates" / "tools" / "src" / "fonts.rs"


def weights_table(text):
    table = {}
    for name, values in re.findall(r"const (W\w+): &\[u16\] = &\[([\d, ]+)\];", text):
        table[name] = [int(value) for value in values.split(",")]
    return table


def entries(text):
    table = weights_table(text)
    for name, weights, italic in re.findall(
            r'font\("([^"]+)", "\w+", (W\w+|&\[[\d, ]+\]), (true|false),', text):
        listed = table[weights] if weights in table else [int(value) for value in re.findall(r"\d+", weights)]
        yield name, listed, italic == "true"


def served(family):
    url = f"https://fonts.googleapis.com/css2?family={family}&display=swap"
    request = urllib.request.Request(url, headers={"User-Agent": "Mozilla/5.0"})
    try:
        with urllib.request.urlopen(request, timeout=30) as response:
            return response.status == 200
    except urllib.error.HTTPError:
        return False


def main():
    text = SOURCE.read_text(encoding="utf-8")
    failures = []
    count = 0
    for name, weights, italic in entries(text):
        count += 1
        plus = name.replace(" ", "+")
        axis = ";".join(f"0,{weight}" for weight in weights)
        if italic:
            axis += ";" + ";".join(f"1,{weight}" for weight in weights)
            family = f"{plus}:ital,wght@{axis}"
        elif weights == [400]:
            family = plus
        else:
            family = f"{plus}:wght@{';'.join(str(weight) for weight in weights)}"
        ok = served(family)
        print(f"{'ok ' if ok else 'NON'} {name} {weights}{' italique' if italic else ''}")
        if not ok:
            failures.append(name)
    print(f"{count} polices, {len(failures)} refusée(s) : {', '.join(failures) or 'aucune'}")
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
