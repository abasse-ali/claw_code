"""Porte de JTest : le relais entre le tunnel Cloudflare et le projet lancé, qui ne laisse passer
que l'utilisateur de Claw Web.

L'adresse du tunnel (`https://….trycloudflare.com`) est publique : sans porte, n'importe qui qui
la trouve ouvre le site, ou le terminal d'un programme. La porte écoute sur 127.0.0.1 et ne
transmet une requête au projet que si elle porte la clé de la JTest :
- `?claw_key=<clé>` dans l'adresse, comme le lien « Ouvrir sur le téléphone » de Claw Web :
  la porte pose un cookie (HttpOnly, Secure, SameSite=Lax) puis renvoie vers la même adresse
  sans la clé ;
- ensuite le cookie, que le navigateur renvoie de lui-même (pages, scripts, WebSocket).
Sans clé, ou avec une mauvaise : 403 et une page qui dit d'ouvrir le test depuis Claw Web.

La clé (aléatoire, une par lancement) n'est écrite que dans le commentaire JTest de la session,
dans le dépôt privé des sessions, et dans le fichier de réglages de la porte (lisible du seul
propriétaire), jamais dans la ligne de commande. Une requête par connexion vers le projet
(`Connection: close`), pour que deux visiteurs ne partagent jamais une connexion déjà vérifiée ;
les WebSocket (rechargement de Vite, terminal `ttyd`) passent une fois la requête vérifiée.

Réglages (fichier JSON, `--config`) : listen, upstream (ports), key, hours, et `basic`
(« utilisateur:mot de passe » que la porte ajoute pour le terminal web, qui n'est ainsi jamais
demandé à l'utilisateur).
"""

import argparse
import asyncio
import base64
import hmac
import json
import sys
import urllib.parse
from pathlib import Path

COOKIE = "claw_jtest"
PARAM = "claw_key"
MAX_HEAD = 64 * 1024
HEAD_TIMEOUT = 60
# En-têtes propres à une connexion : jamais transmis tels quels.
HOP = {"connection", "keep-alive", "proxy-connection", "proxy-authenticate", "proxy-authorization", "te",
       "trailer", "upgrade"}
REFUSED = """<!doctype html><html lang="fr"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1"><title>JTest : accès réservé</title>
<style>body{font:17px/1.5 system-ui,sans-serif;max-width:32rem;margin:18vh auto;padding:0 20px;color:#222;background:#faf9f7}
@media (prefers-color-scheme:dark){body{color:#eee;background:#1b1a18}}</style></head>
<body><h1>Accès réservé</h1><p>Ce test JTest ne s'ouvre que depuis Claw Web : dans la session,
touchez « Ouvrir sur le téléphone ».</p></body></html>
"""


def parse_head(raw):
    """Ligne de départ et en-têtes [(nom, valeur)] d'une requête ou d'une réponse HTTP/1."""
    lines = raw.decode("latin-1").split("\r\n")
    headers = []
    for line in lines[1:]:
        if not line:
            continue
        name, sep, value = line.partition(":")
        if not sep or not name.strip() or name != name.strip():
            raise ValueError("en-tête invalide")
        headers.append((name, value.strip()))
    return lines[0], headers


def header(headers, name):
    return next((value for key, value in headers if key.lower() == name), "")


def build_head(start, headers):
    return (start + "\r\n" + "".join(f"{name}: {value}\r\n" for name, value in headers) + "\r\n").encode("latin-1")


def cookie_key(headers):
    """La clé que porte le cookie de la porte, ou « »."""
    for name, value in headers:
        if name.lower() != "cookie":
            continue
        for part in value.split(";"):
            key, _, found = part.strip().partition("=")
            if key == COOKIE:
                return found
    return ""


def without_cookie(headers):
    """Les en-têtes sans le cookie de la porte : le projet ne voit jamais la clé."""
    kept = []
    for name, value in headers:
        if name.lower() == "cookie":
            parts = [part for part in value.split(";") if part.strip().partition("=")[0] != COOKIE]
            if any(part.strip() for part in parts):
                kept.append((name, ";".join(parts).strip()))
            continue
        kept.append((name, value))
    return kept


def same(given, key):
    return bool(given) and hmac.compare_digest(given.encode(), key.encode())


def decide(method, target, headers, key):
    """Ce que la porte fait d'une requête : (« pass », None), (« login », adresse sans la clé) ou
    (« refuse », None)."""
    path, _, query = target.partition("?")
    pairs = urllib.parse.parse_qsl(query, keep_blank_values=True)
    given = [value for name, value in pairs if name == PARAM]
    if given:
        if not same(given[0], key):
            return "refuse", None
        rest = urllib.parse.urlencode([(name, value) for name, value in pairs if name != PARAM])
        return "login", (path or "/") + (f"?{rest}" if rest else "")
    if same(cookie_key(headers), key):
        return "pass", None
    return "refuse", None


class Gate:
    def __init__(self, config):
        self.key = str(config["key"])
        self.upstream = int(config["upstream"])
        self.max_age = int(float(config.get("hours") or 4) * 3600)
        basic = config.get("basic") or ""
        self.authorization = f"Basic {base64.b64encode(basic.encode()).decode()}" if basic else ""

    async def answer(self, writer, status, headers, body=b""):
        headers = [*headers, ("Content-Length", str(len(body))), ("Connection", "close"),
                   ("Cache-Control", "no-store"), ("Referrer-Policy", "no-referrer")]
        writer.write(build_head(f"HTTP/1.1 {status}", headers) + body)
        await writer.drain()

    async def handle(self, reader, writer):
        try:
            try:
                raw = await asyncio.wait_for(reader.readuntil(b"\r\n\r\n"), HEAD_TIMEOUT)
            except (asyncio.IncompleteReadError, asyncio.LimitOverrunError, asyncio.TimeoutError):
                return
            try:
                start, headers = parse_head(raw)
                method, target, version = start.split(" ")
            except ValueError:
                await self.answer(writer, "400 Bad Request", [("Content-Type", "text/plain")], b"Requete invalide\n")
                return
            verdict, location = decide(method, target, headers, self.key)
            if verdict == "login":
                cookie = f"{COOKIE}={self.key}; Path=/; Max-Age={self.max_age}; HttpOnly; Secure; SameSite=Lax"
                await self.answer(writer, "302 Found", [("Location", location), ("Set-Cookie", cookie)])
                return
            if verdict == "refuse":
                await self.answer(writer, "403 Forbidden", [("Content-Type", "text/html; charset=utf-8")],
                                  REFUSED.encode())
                return
            await self.forward(start, headers, reader, writer)
        except (ConnectionError, OSError):
            pass
        finally:
            writer.close()

    async def forward(self, start, headers, reader, writer):
        upgrade = header(headers, "upgrade").lower() == "websocket"
        sent = [(name, value) for name, value in without_cookie(headers) if name.lower() not in HOP]
        if self.authorization and not header(headers, "authorization"):
            sent.append(("Authorization", self.authorization))
        sent += [("Connection", "Upgrade"), ("Upgrade", "websocket")] if upgrade else [("Connection", "close")]
        try:
            up_reader, up_writer = await asyncio.open_connection("127.0.0.1", self.upstream)
        except OSError:
            await self.answer(writer, "502 Bad Gateway", [("Content-Type", "text/plain")],
                              b"Le projet ne repond pas (arrete ou en train de redemarrer).\n")
            return
        try:
            up_writer.write(build_head(start, sent))
            await up_writer.drain()
            sending = asyncio.ensure_future(pipe(reader, up_writer))
            try:
                await self.relay_response(up_reader, writer)
            finally:
                sending.cancel()
        finally:
            up_writer.close()

    async def relay_response(self, up_reader, writer):
        """La réponse du projet : ses réponses provisoires (100 Continue) telles quelles, puis la
        réponse finale fermée après elle (101 : la WebSocket, dans les deux sens)."""
        while True:
            try:
                raw = await up_reader.readuntil(b"\r\n\r\n")
            except (asyncio.IncompleteReadError, asyncio.LimitOverrunError):
                return
            start, headers = parse_head(raw)
            status = start.split(" ", 2)[1] if start.count(" ") else ""
            if status.startswith("1") and status != "101":
                writer.write(raw)
                await writer.drain()
                continue
            if status != "101":
                headers = [(name, value) for name, value in headers if name.lower() not in ("connection", "keep-alive")]
                headers.append(("Connection", "close"))
            writer.write(build_head(start, headers))
            await writer.drain()
            await pipe(up_reader, writer)
            return


async def pipe(reader, writer):
    try:
        while True:
            chunk = await reader.read(65536)
            if not chunk:
                break
            writer.write(chunk)
            await writer.drain()
    except (ConnectionError, OSError):
        pass
    finally:
        try:
            writer.write_eof()
        except (OSError, RuntimeError, AttributeError):
            pass


async def serve(config):
    gate = Gate(config)
    server = await asyncio.start_server(gate.handle, "127.0.0.1", int(config["listen"]), limit=MAX_HEAD)
    async with server:
        await server.serve_forever()


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--config", required=True, help="fichier JSON des réglages (listen, upstream, key, hours, basic)")
    args = parser.parse_args(argv)
    config = json.loads(Path(args.config).read_text(encoding="utf-8"))
    if len(str(config.get("key") or "")) < 32:
        print("clé JTest absente ou trop courte", file=sys.stderr)
        return 2
    try:
        asyncio.run(serve(config))
    except KeyboardInterrupt:
        pass
    return 0


if __name__ == "__main__":
    sys.exit(main())
