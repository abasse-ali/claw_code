"""Descriptions d'images faites pour un modèle qui ne lit pas les images, avec les consignes
exactes de Claw (claw-cli/src/image_reader.rs) et le lecteur d'images de NVIDIA.

Pour juger si un modèle aveugle peut trouver les défauts d'une capture à partir de ce texte
seul : chaque image de `images/` reçoit la description rapide (celle des captures de l'outil
Screenshot) et la description complète en deux regards (celle des images jointes), puis le
journal montre, à côté, les défauts connus de `attendus.json`.

    NVIDIA_API_KEY=... python3 describe.py [dossier de sortie]

La clé ne sort jamais de l'environnement : ni affichée ni écrite.
"""

import base64
import json
import os
import re
import sys
import time
import urllib.error
import urllib.request
from pathlib import Path

HERE = Path(__file__).resolve().parent
SOURCE = HERE.parents[1] / "crates" / "claw-cli" / "src" / "image_reader.rs"
MODEL = os.environ.get("READER_MODEL", "nvidia/nemotron-3.5-super-vl-120b-a12b")
URL = "https://integrate.api.nvidia.com/v1/chat/completions"
TOKENS = 8000


def rust_const(name):
    """Une constante `&str` de image_reader.rs, ses échappements décodés."""
    text = SOURCE.read_text(encoding="utf-8")
    found = re.search(rf'const {name}: &str = "((?:[^"\\]|\\.)*)";', text, re.S)
    if not found:
        raise SystemExit(f"{name} introuvable dans {SOURCE}")
    return json.loads('"' + found.group(1).replace("\n", "\\n") + '"')


def ask(prompt, image):
    data = base64.b64encode(image.read_bytes()).decode("ascii")
    body = {
        "model": MODEL,
        "max_tokens": TOKENS,
        "messages": [{"role": "user", "content": [
            {"type": "text", "text": prompt},
            {"type": "image_url", "image_url": {"url": f"data:image/png;base64,{data}"}},
        ]}],
    }
    request = urllib.request.Request(URL, data=json.dumps(body).encode("utf-8"), method="POST", headers={
        "Authorization": f"Bearer {os.environ['NVIDIA_API_KEY']}",
        "Content-Type": "application/json",
        "Accept": "application/json",
    })
    for attempt in range(4):
        try:
            with urllib.request.urlopen(request, timeout=240) as response:
                answer = json.loads(response.read())
            message = answer["choices"][0]["message"]
            return (message.get("content") or "").strip()
        except urllib.error.HTTPError as error:
            if error.code == 429 and attempt < 3:
                time.sleep(20 * (attempt + 1))
                continue
            return f"(erreur HTTP {error.code} : {error.read().decode('utf-8', 'replace')[:300]})"
        except (urllib.error.URLError, TimeoutError) as error:
            if attempt < 3:
                time.sleep(10)
                continue
            return f"(erreur réseau : {error})"
    return "(aucune réponse)"


def main():
    out = Path(sys.argv[1] if len(sys.argv) > 1 else "descriptions")
    out.mkdir(parents=True, exist_ok=True)
    quick, reader, check = rust_const("QUICK_PROMPT"), rust_const("READER_PROMPT"), rust_const("CHECK_PROMPT")
    expected = json.loads((HERE / "attendus.json").read_text(encoding="utf-8"))
    for image in sorted((HERE / "images").glob("*.png")):
        started = time.time()
        quick_text = ask(quick, image)
        first = ask(reader, image)
        checked = ask(f"{check}\n\n--- Description to check ---\n{first}", image)
        seconds = round(time.time() - started)
        report = (f"# {image.name}\n\nModèle : {MODEL} ({seconds} s)\n\n"
                  f"## Défauts connus\n" + "\n".join(f"- {item}" for item in expected.get(image.name, {}).get("defauts", []))
                  + f"\n\n## Description rapide (captures de Screenshot)\n\n{quick_text}\n\n"
                  f"## Description complète, premier regard\n\n{first}\n\n"
                  f"## Description complète, après relecture\n\n{checked}\n")
        (out / f"{image.stem}.md").write_text(report, encoding="utf-8")
        print(report, flush=True)
        print("=" * 100, flush=True)


if __name__ == "__main__":
    main()
