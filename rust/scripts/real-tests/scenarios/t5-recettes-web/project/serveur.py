"""Serveur du carnet de recettes : l'API et la page.

Lancement :
    python serveur.py [PORT]        (8000 par défaut)

API :
    GET /api/recettes           toutes les recettes, triées par nom
    GET /api/recettes/<id>      une recette (404 si elle n'existe pas)
"""

import json
import sys
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from urllib.parse import urlsplit

from carnet import recettes

STATIQUE = Path(__file__).resolve().parent / "static"
TYPES = {
    ".html": "text/html; charset=utf-8",
    ".js": "text/javascript; charset=utf-8",
    ".css": "text/css; charset=utf-8",
}


class Guichet(BaseHTTPRequestHandler):
    def do_GET(self):
        chemin = urlsplit(self.path).path
        if chemin == "/api/recettes":
            self.json(200, recettes.toutes())
        elif chemin.startswith("/api/recettes/"):
            recette = recettes.une(chemin.rsplit("/", 1)[1])
            if recette is None:
                self.json(404, {"erreur": "recette inconnue"})
            else:
                self.json(200, recette)
        else:
            self.fichier(chemin)

    def json(self, code, donnees):
        corps = json.dumps(donnees, ensure_ascii=False).encode("utf-8")
        self.send_response(code)
        self.send_header("Content-Type", "application/json; charset=utf-8")
        self.send_header("Content-Length", str(len(corps)))
        self.end_headers()
        self.wfile.write(corps)

    def fichier(self, chemin):
        nom = "index.html" if chemin in ("/", "") else chemin.lstrip("/")
        cible = (STATIQUE / nom).resolve()
        if STATIQUE not in cible.parents or not cible.is_file():
            self.json(404, {"erreur": "page inconnue"})
            return
        corps = cible.read_bytes()
        self.send_response(200)
        self.send_header("Content-Type", TYPES.get(cible.suffix, "application/octet-stream"))
        self.send_header("Content-Length", str(len(corps)))
        self.end_headers()
        self.wfile.write(corps)

    def log_message(self, format, *args):
        pass


def demarrer(port=8000):
    """Serveur prêt à servir (port 0 : un port libre, lu dans `server_address`)."""
    return ThreadingHTTPServer(("127.0.0.1", port), Guichet)


if __name__ == "__main__":
    port = int(sys.argv[1]) if len(sys.argv) > 1 else 8000
    serveur = demarrer(port)
    print(f"Carnet de recettes sur http://127.0.0.1:{serveur.server_address[1]}/", flush=True)
    serveur.serve_forever()
