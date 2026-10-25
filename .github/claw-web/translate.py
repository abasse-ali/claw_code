"""Traduction en français des problèmes relevés sur les captures d'écran (outil Screenshot) :
Claw les écrit en anglais pour le modèle, l'appli les montre à l'utilisateur quand il touche
une capture (demandé le 25 octobre 2026 : « les problèmes au-dessus des captures doivent être
en français »).

Un petit modèle de NVIDIA traduit, en arrière-plan : le journal n'attend pas, la traduction
rejoint la capture quand elle arrive. Un même texte n'est traduit qu'une fois par passage.
"""

import json
import re
import threading
import urllib.request

from concurrent.futures import ThreadPoolExecutor

NVIDIA_URL = "https://integrate.api.nvidia.com/v1/chat/completions"
# Rapide et sans quota du jour ; réglable par CLAW_TRANSLATE_MODEL.
MODEL = "nvidia/nemotron-3.5-lightning-30b-a3b"
TIMEOUT = 90
PROMPT = (
    "Traduis en français chacun des textes du tableau JSON ci-dessous. Ce sont des défauts "
    "relevés sur une page web, lus par une personne qui n'est pas informaticienne : écris un "
    "français simple et naturel. Garde tels quels les noms d'éléments et de sélecteurs "
    "(button#envoyer, span.logo-text, h2), le texte cité entre guillemets, les nombres, les "
    "unités (px) et le code CSS ou JavaScript. N'écris jamais le tiret long ; mets « - », une "
    "virgule ou deux-points. Réponds seulement par un tableau JSON de chaînes, une traduction "
    "par texte, dans le même ordre.\n\n"
)


def plain(text):
    """Une traduction sans tiret long (réglage de l'utilisateur), un « - » à la place."""
    return re.sub(r"\s*[\u2014\u2015]\s*", " - ", str(text)).strip()


def parse(answer, count):
    """Le tableau de `count` traductions dans la réponse du modèle (son raisonnement éventuel,
    entre <think> et </think>, est ignoré) ; None s'il n'y est pas."""
    answer = re.sub(r"<think>.*?</think>", "", str(answer or ""), flags=re.S)
    start, end = answer.find("["), answer.rfind("]")
    if start < 0 or end <= start:
        return None
    try:
        items = json.loads(answer[start:end + 1])
    except json.JSONDecodeError:
        return None
    if not isinstance(items, list) or len(items) != count or not all(isinstance(item, str) for item in items):
        return None
    return [plain(item) for item in items]


class Translator:
    """Traduit des listes de textes en arrière-plan, un appel à la fois."""

    def __init__(self, key, model=MODEL, call=None):
        self.key = key
        self.model = model or MODEL
        self.cache = {}
        self.pool = ThreadPoolExecutor(max_workers=1)
        self.pending = []
        self.lock = threading.Lock()
        self.call = call or self._post

    def __call__(self, texts, done):
        """`done(traductions)` reçoit les traductions de `texts`, dans l'ordre, dès qu'elles
        sont prêtes (tout de suite si elles le sont déjà) ; rien si la traduction échoue."""
        texts = list(texts)
        missing = [text for text in dict.fromkeys(texts) if text not in self.cache]
        if not missing:
            done([self.cache[text] for text in texts])
            return

        def work():
            try:
                translated = self.call(missing)
            except Exception as error:  # le journal garde l'anglais
                print(f"Traduction des problèmes impossible : {type(error).__name__}", flush=True)
                return
            if not translated or len(translated) != len(missing):
                print("Traduction des problèmes illisible : l'anglais reste", flush=True)
                return
            self.cache.update(zip(missing, translated))
            done([self.cache[text] for text in texts])

        with self.lock:
            self.pending.append(self.pool.submit(work))

    def wait(self, timeout):
        """Attend les traductions en cours, `timeout` secondes au plus (fin du passage)."""
        with self.lock:
            pending, self.pending = self.pending, []
        for future in pending:
            try:
                future.result(timeout=timeout)
            except Exception:  # délai dépassé ou échec : l'anglais reste
                return

    def _post(self, texts):
        body = json.dumps({
            "model": self.model,
            # Les modèles Nemotron raisonnent longuement sans cette consigne, et les jetons
            # pouvaient manquer à la réponse elle-même.
            "messages": [{"role": "system", "content": "detailed thinking off"},
                         {"role": "user", "content": PROMPT + json.dumps(texts, ensure_ascii=False)}],
            "temperature": 0.2,
            "max_tokens": 8192,
        }).encode("utf-8")
        request = urllib.request.Request(NVIDIA_URL, data=body, headers={
            "Authorization": f"Bearer {self.key}",
            "Content-Type": "application/json",
        })
        with urllib.request.urlopen(request, timeout=TIMEOUT) as response:
            answer = json.loads(response.read().decode("utf-8"))
        message = (answer.get("choices") or [{}])[0].get("message") or {}
        return parse(message.get("content"), len(texts))
