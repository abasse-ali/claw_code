"""Essais des notifications Web Push (push.py) : chiffrement de la RFC 8291 (son exemple de
l'annexe A), jeton VAPID vérifiable, envoi à chaque appareil et abonnements expirés retirés."""

import base64
import hashlib
import hmac
import json
import os
import tempfile
import unittest
from pathlib import Path
from unittest import mock

try:
    import cryptography  # noqa: F401
    HAVE_CRYPTO = True
except ImportError:
    HAVE_CRYPTO = False

import push


def b64(text):
    return base64.urlsafe_b64decode(text + "=" * (-len(text) % 4))


@unittest.skipUnless(HAVE_CRYPTO, "module cryptography absent")
class EncryptionTest(unittest.TestCase):
    def test_the_rfc_8291_example(self):
        # RFC 8291, annexe A : mêmes clés, même sel, même corps chiffré.
        from cryptography.hazmat.primitives.asymmetric import ec
        server = ec.derive_private_key(int.from_bytes(b64("yfWPiYE-n46HLnH0KqZOF1fJJU3MYrct3AELtAQ-oRw"), "big"),
                                       ec.SECP256R1())
        body = push.encrypt(b"When I grow up, I want to be a watermelon",
                            "BCVxsr7N_eNgVRqvHtD0zTZsEc6-VV-JvLexhqUzORcxaOzi6-AYWXvTBHm4bjyPjs7Vd8pZGH6SRpkNtoIAiw4",
                            "BTBZMqHH6r4Tts7J_aSIgg", salt=b64("DGv6ra1nlYgDCS1FRnbzlw"), server_private=server)
        self.assertEqual(push.b64u_encode(body),
                         "DGv6ra1nlYgDCS1FRnbzlwAAEABBBP4z9KsN6nGRTbVYI_c7VJSPQTBtkgcy27mlmlMoZIIgDll6e3vCYLocInmYWAmS6Tlz"
                         "AC8wEqKK6PBru3jl7A_yl95bQpu6cVPTpK4Mqgkf1CXztLVBSt2Ks3oZwbuwXPXLWyouBWLVWGNWQexSgSxsj_Qulcy4a-fN")

    def test_the_phone_reads_what_was_sent(self):
        # Le côté du téléphone : sa clé privée et son secret retrouvent la notification.
        from cryptography.hazmat.primitives import serialization
        from cryptography.hazmat.primitives.asymmetric import ec
        from cryptography.hazmat.primitives.ciphers.aead import AESGCM
        phone = ec.generate_private_key(ec.SECP256R1())
        phone_public = phone.public_key().public_bytes(serialization.Encoding.X962,
                                                       serialization.PublicFormat.UncompressedPoint)
        secret = os.urandom(16)
        message = json.dumps({"title": "Claw a fini", "body": "Suivi d'habitudes"}).encode()
        body = push.encrypt(message, push.b64u_encode(phone_public), push.b64u_encode(secret))
        salt, size, keylen = body[:16], int.from_bytes(body[16:20], "big"), body[20]
        server_public = body[21:21 + keylen]
        self.assertEqual((size, keylen), (4096, 65))
        shared = phone.exchange(ec.ECDH(), ec.EllipticCurvePublicKey.from_encoded_point(ec.SECP256R1(), server_public))
        expand = lambda prk, info, n: hmac.new(prk, info + b"\x01", hashlib.sha256).digest()[:n]  # noqa: E731
        ikm = expand(hmac.new(secret, shared, hashlib.sha256).digest(), b"WebPush: info\x00" + phone_public + server_public, 32)
        prk = hmac.new(salt, ikm, hashlib.sha256).digest()
        plain = AESGCM(expand(prk, b"Content-Encoding: aes128gcm\x00", 16)).decrypt(
            expand(prk, b"Content-Encoding: nonce\x00", 12), body[21 + keylen:], None)
        self.assertEqual(plain, message + b"\x02")

    def test_the_vapid_token_is_signed_by_the_app_key(self):
        from cryptography.hazmat.primitives import hashes, serialization
        from cryptography.hazmat.primitives.asymmetric import ec
        from cryptography.hazmat.primitives.asymmetric.utils import encode_dss_signature
        key = ec.generate_private_key(ec.SECP256R1())
        d = push.b64u_encode(key.private_numbers().private_value.to_bytes(32, "big"))
        public = push.b64u_encode(key.public_key().public_bytes(serialization.Encoding.X962,
                                                                serialization.PublicFormat.UncompressedPoint))
        header = push.vapid_header("https://web.push.apple.com/QGuQyavXutnMMxs6VfkN", d, public, now=1_800_000_000)
        token, k = header.removeprefix("vapid t=").split(", k=")
        self.assertEqual(k, public)
        head, claims, signature = token.split(".")
        self.assertEqual(json.loads(b64(claims)), {"aud": "https://web.push.apple.com", "exp": 1_800_043_200,
                                                   "sub": push.SUBJECT})
        raw = b64(signature)
        key.public_key().verify(encode_dss_signature(int.from_bytes(raw[:32], "big"), int.from_bytes(raw[32:], "big")),
                                f"{head}.{claims}".encode(), ec.ECDSA(hashes.SHA256()))


class SendingTest(unittest.TestCase):
    def setUp(self):
        folder = Path(tempfile.mkdtemp())
        self.notify = folder / "notify.json"
        self.notify.write_text(json.dumps({"title": "Claw a une question", "body": "Quel nom ?", "url": "#/session/61"}))
        self.subscriptions = [{"endpoint": "https://fcm.googleapis.com/fcm/send/a", "keys": {"p256dh": "x", "auth": "y"}, "device": "Android"},
                              {"endpoint": "https://web.push.apple.com/b", "keys": {"p256dh": "x", "auth": "y"}, "device": "iPhone"}]

    def run_main(self, environ, statuses):
        sent, patched = [], []
        def fake_send(subscription, payload, d, public):  # noqa: E306
            sent.append((subscription["device"], json.loads(payload), d, public))
            return statuses.pop(0)
        def fake_api(method, path, token, body=None):  # noqa: E306
            patched.append((method, path, body))
        with mock.patch.object(push, "NOTIFY_FILE", self.notify), mock.patch.object(push, "send", fake_send), \
                mock.patch.object(push, "api", fake_api), mock.patch.dict(os.environ, environ, clear=False):
            push.main()
        return sent, patched

    def test_every_device_gets_it_and_a_gone_one_is_removed(self):
        sent, patched = self.run_main({"CLAW_PUSH_VAPID_PRIVATE": "d", "CLAW_PUSH_VAPID_PUBLIC": "p",
                                       "CLAW_PUSH_SUBSCRIPTIONS": json.dumps(self.subscriptions),
                                       "REPO_TOKEN": "t", "GITHUB_REPOSITORY": "abasse-ali/claw-sessions"}, [201, 410])
        self.assertEqual([item[0] for item in sent], ["Android", "iPhone"])
        self.assertEqual(sent[0][1]["title"], "Claw a une question")
        self.assertEqual(len(patched), 1)
        method, path, body = patched[0]
        self.assertEqual((method, path), ("PATCH", "/repos/abasse-ali/claw-sessions/actions/variables/CLAW_PUSH_SUBSCRIPTIONS"))
        self.assertEqual([item["device"] for item in json.loads(body["value"])], ["Android"])

    def test_nothing_configured_sends_nothing(self):
        sent, patched = self.run_main({"CLAW_PUSH_VAPID_PRIVATE": "", "CLAW_PUSH_VAPID_PUBLIC": "",
                                       "CLAW_PUSH_SUBSCRIPTIONS": "", "REPO_TOKEN": ""}, [])
        self.assertEqual((sent, patched), ([], []))


class NotificationTest(unittest.TestCase):
    """Ce que publish.py écrit pour le téléphone à la fin d'un passage."""

    def write(self, finished, files, result="", error="", outcome="success"):
        import importlib
        env = {"REPO": "o/app", "BASE": "main", "SESSION": "61", "BRANCH": "claw/session-61",
               "GITHUB_REPOSITORY": "o/s", "JOB_TOKEN": "t", "TITLE": "Suivi d'habitudes"}
        folder = Path(tempfile.mkdtemp())
        with mock.patch.dict(os.environ, env):
            publish = importlib.import_module("publish")
            with mock.patch.object(publish, "read", lambda name: files.get(name, "")), \
                    mock.patch.object(publish, "WORK", folder), mock.patch.object(publish, "OUTCOME", outcome), \
                    mock.patch.object(publish, "TITLE", "Suivi d'habitudes"), mock.patch.object(publish, "SESSION", "61"):
                publish.write_notification(finished, result, error)
        path = folder / "notify.json"
        return json.loads(path.read_text(encoding="utf-8")) if path.exists() else None

    def test_a_question_a_finished_task_and_a_stop(self):
        asked = self.write(True, {"questions.json": json.dumps([{"question": "Quel **nom** pour l'appli ?", "options": []}])},
                           result="Voici deux idées.")
        self.assertEqual((asked["title"], asked["body"], asked["session"]), ("Claw a une question", "Quel nom pour l'appli ?", "61"))
        done = self.write(True, {}, result="L'application est **complète**.\n\n| Exigence | … |")
        self.assertEqual((done["title"], done["body"]), ("Claw a fini", "Suivi d'habitudes : L'application est complète."))
        stopped = self.write(False, {}, error="timeout")
        self.assertEqual(stopped["title"], "Claw s'est arrêté")
        # Un passage annulé par l'utilisateur : pas de notification.
        self.assertIsNone(self.write(False, {}, outcome="cancelled"))


if __name__ == "__main__":
    unittest.main()
