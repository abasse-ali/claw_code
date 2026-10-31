"""Prévient le téléphone de l'utilisateur quand un passage de Claw finit ou lui pose une question :
une notification Web Push de l'appli Claw Web, même fermée (sur iPhone, l'appli ajoutée à l'écran
d'accueil). Demandé le 30 octobre 2026 : « une notification quand Claw a une question et quand il
finit son travail ».

L'appli (Réglages, Notifications) crée la paire de clés VAPID du moteur : la clé privée dans le
secret CLAW_PUSH_VAPID_PRIVATE, la clé publique et les abonnements de chaque appareil dans les
variables CLAW_PUSH_VAPID_PUBLIC et CLAW_PUSH_SUBSCRIPTIONS du dépôt des sessions. publish.py écrit
la notification du passage (NOTIFY_FILE : title, body, url, tag) ; ce script la chiffre pour
chaque appareil (RFC 8291, aes128gcm) et l'envoie, signée par la clé VAPID (RFC 8292).

Entrées (environnement) : CLAW_PUSH_VAPID_PRIVATE, CLAW_PUSH_VAPID_PUBLIC,
CLAW_PUSH_SUBSCRIPTIONS (relues par l'API avec REPO_TOKEN si le workflow ne les a pas),
REPO_TOKEN, GITHUB_REPOSITORY. Rien de configuré : rien n'est envoyé, sans erreur.
"""

import base64
import hashlib
import hmac
import json
import os
import struct
import subprocess
import sys
import time
import urllib.error
import urllib.parse
import urllib.request

from common import WORK, GitHubError, api

NOTIFY_FILE = WORK / "notify.json"
SUBSCRIPTIONS = "CLAW_PUSH_SUBSCRIPTIONS"
PUBLIC = "CLAW_PUSH_VAPID_PUBLIC"
# Celui qui envoie, demandé par les services de notification (Apple refuse un `sub` absent).
SUBJECT = "https://github.com/abasse-ali/claw_code"
RECORD_SIZE = 4096


def b64u_decode(text):
    text = text.strip()
    return base64.urlsafe_b64decode(text + "=" * (-len(text) % 4))


def b64u_encode(data):
    return base64.urlsafe_b64encode(data).rstrip(b"=").decode("ascii")


def crypto():
    """Le module `cryptography` (courbe P-256, AES-GCM), installé au besoin pour ce seul envoi."""
    try:
        import cryptography  # noqa: F401
    except ImportError:
        for extra in ([], ["--break-system-packages"]):
            installed = subprocess.run([sys.executable, "-m", "pip", "install", "--quiet", "--user",
                                        "--disable-pip-version-check", *extra, "cryptography"],
                                       capture_output=True, text=True, timeout=180)
            if installed.returncode == 0:
                break
        import site
        sys.path.append(site.getusersitepackages())
    from cryptography.hazmat.primitives import hashes, serialization
    from cryptography.hazmat.primitives.asymmetric import ec
    from cryptography.hazmat.primitives.asymmetric.utils import decode_dss_signature
    from cryptography.hazmat.primitives.ciphers.aead import AESGCM
    return ec, hashes, serialization, decode_dss_signature, AESGCM


def _hkdf_expand(prk, info, length):
    return hmac.new(prk, info + b"\x01", hashlib.sha256).digest()[:length]


def encrypt(payload, p256dh, auth, salt=None, server_private=None):
    """Le corps chiffré d'une notification pour un abonnement (RFC 8291) : en-tête (sel, taille des
    blocs, clé publique éphémère du serveur) puis un seul bloc AES-128-GCM."""
    ec, _, serialization, _, AESGCM = crypto()
    ua_public = b64u_decode(p256dh)
    auth_secret = b64u_decode(auth)
    salt = salt or os.urandom(16)
    server_private = server_private or ec.generate_private_key(ec.SECP256R1())
    server_public = server_private.public_key().public_bytes(
        serialization.Encoding.X962, serialization.PublicFormat.UncompressedPoint)
    shared = server_private.exchange(ec.ECDH(), ec.EllipticCurvePublicKey.from_encoded_point(ec.SECP256R1(), ua_public))
    prk_key = hmac.new(auth_secret, shared, hashlib.sha256).digest()
    ikm = _hkdf_expand(prk_key, b"WebPush: info\x00" + ua_public + server_public, 32)
    prk = hmac.new(salt, ikm, hashlib.sha256).digest()
    cek = _hkdf_expand(prk, b"Content-Encoding: aes128gcm\x00", 16)
    nonce = _hkdf_expand(prk, b"Content-Encoding: nonce\x00", 12)
    # Le dernier (et seul) bloc finit par l'octet 2.
    body = AESGCM(cek).encrypt(nonce, payload + b"\x02", None)
    return salt + struct.pack(">IB", RECORD_SIZE, len(server_public)) + server_public + body


def private_key(d_b64):
    ec, *_ = crypto()
    return ec.derive_private_key(int.from_bytes(b64u_decode(d_b64), "big"), ec.SECP256R1())


def vapid_header(endpoint, d_b64, public_b64, now=None):
    """L'en-tête Authorization d'un envoi : un jeton JWT ES256 pour l'origine du service de
    notification, et la clé publique de l'appli (RFC 8292)."""
    ec, hashes, _, decode_dss_signature, _ = crypto()
    origin = "{0.scheme}://{0.netloc}".format(urllib.parse.urlsplit(endpoint))
    header = b64u_encode(json.dumps({"typ": "JWT", "alg": "ES256"}, separators=(",", ":")).encode())
    claims = b64u_encode(json.dumps({"aud": origin, "exp": int((now or time.time()) + 12 * 3600),
                                     "sub": SUBJECT}, separators=(",", ":")).encode())
    signing = f"{header}.{claims}".encode("ascii")
    r, s = decode_dss_signature(private_key(d_b64).sign(signing, ec.ECDSA(hashes.SHA256())))
    token = f"{header}.{claims}.{b64u_encode(r.to_bytes(32, 'big') + s.to_bytes(32, 'big'))}"
    return f"vapid t={token}, k={public_b64}"


def send(subscription, payload, d_b64, public_b64):
    """Envoie une notification à un appareil ; renvoie le code HTTP du service (201 : reçue)."""
    endpoint = subscription["endpoint"]
    keys = subscription.get("keys") or {}
    body = encrypt(payload, keys["p256dh"], keys["auth"])
    request = urllib.request.Request(endpoint, data=body, method="POST")
    request.add_header("Authorization", vapid_header(endpoint, d_b64, public_b64))
    request.add_header("Content-Encoding", "aes128gcm")
    request.add_header("Content-Type", "application/octet-stream")
    request.add_header("TTL", "86400")
    request.add_header("Urgency", "high")
    try:
        with urllib.request.urlopen(request, timeout=20) as response:
            return response.status
    except urllib.error.HTTPError as error:
        return error.code


def variable(name, repo, token):
    """Une variable du dépôt des sessions : celle du workflow, sinon relue par l'API."""
    value = os.environ.get(name, "").strip()
    if value or not token:
        return value
    try:
        return (api("GET", f"/repos/{repo}/actions/variables/{name}", token) or {}).get("value", "")
    except GitHubError:
        return ""


def main():
    if not NOTIFY_FILE.exists():
        return
    notification = json.loads(NOTIFY_FILE.read_text(encoding="utf-8"))
    repo = os.environ.get("GITHUB_REPOSITORY", "")
    token = os.environ.get("REPO_TOKEN", "").strip()
    d_b64 = os.environ.get("CLAW_PUSH_VAPID_PRIVATE", "").strip()
    public_b64 = variable(PUBLIC, repo, token)
    try:
        subscriptions = json.loads(variable(SUBSCRIPTIONS, repo, token) or "[]")
    except ValueError:
        subscriptions = []
    if not (d_b64 and public_b64 and subscriptions):
        print("Notifications : aucun appareil abonné (Réglages de l'appli, Notifications).")
        return
    payload = json.dumps(notification, ensure_ascii=False).encode("utf-8")[:3000]
    gone = []
    for subscription in subscriptions:
        try:
            status = send(subscription, payload, d_b64, public_b64)
        except (OSError, KeyError, ValueError) as failure:
            print(f"Notification non envoyée à {subscription.get('device', '?')} : {failure}")
            continue
        print(f"Notification à {subscription.get('device', '?')} : {status}")
        # 404 et 410 : l'appareil s'est désabonné, ou l'appli a été effacée.
        if status in (404, 410):
            gone.append(subscription.get("endpoint"))
    if gone and token:
        kept = [item for item in subscriptions if item.get("endpoint") not in gone]
        try:
            api("PATCH", f"/repos/{repo}/actions/variables/{SUBSCRIPTIONS}", token,
                {"name": SUBSCRIPTIONS, "value": json.dumps(kept)})
        except GitHubError as failure:
            print(f"Abonnements expirés non retirés : {failure}")


if __name__ == "__main__":
    try:
        main()
    except Exception as failure:  # une notification manquée ne fait jamais échouer le passage
        print(f"Notifications : {failure}")
