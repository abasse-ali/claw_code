"""Traduction en français des problèmes relevés sur les captures d'écran (outil Screenshot) :
Claw les écrit en anglais pour le modèle, l'appli les montre à l'utilisateur quand il touche
une capture (demandé le 25 octobre 2026 : « les problèmes au-dessus des captures doivent être
en français »).

Un petit modèle de NVIDIA traduit, en arrière-plan : le journal n'attend pas, la traduction
rejoint la capture quand elle arrive. Un même texte n'est traduit qu'une fois par passage.

La traduction ne retient jamais la fin du passage : un fil d'arrière-plan qui meurt avec le
programme, et une attente de fin bornée en tout. Vu le 28 octobre 2026 (session #41) : onze
traductions en échec en file, la session finie mais la branche poussée 13 minutes plus tard.
Vu le 29 octobre 2026 (essai C3) : chaque traduction échouait (délai dépassé, réponse
illisible) et l'appli montrait l'anglais. Les textes partent maintenant par petits paquets,
et Gemini Flash-Lite reprend un paquet que NVIDIA n'a pas traduit.
"""

import json
import queue
import re
import threading
import time
import urllib.request


NVIDIA_URL = "https://integrate.api.nvidia.com/v1/chat/completions"
# Rapide et sans quota du jour ; réglable par CLAW_TRANSLATE_MODEL.
MODEL = "nvidia/nemotron-3.5-lightning-30b-a3b"
# Le relais : rapide, et d'un quota gratuit moins serré que celui de Gemini Flash.
GEMINI_URL = "https://generativelanguage.googleapis.com/v1beta/openai/chat/completions"
GEMINI_MODEL = "gemini-3.1-flash-lite"
# Un paquet de quelques textes se traduit vite ; une liste entière dépassait le délai.
TIMEOUT = 45
BATCH = 6
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
    # Un raisonnement dont la balise ouvrante manque, un bloc de code autour du tableau.
    if "</think>" in answer:
        answer = answer.rsplit("</think>", 1)[1]
    answer = re.sub(r"```(?:json)?", "", answer)
    items = None
    start, end = answer.find("["), answer.rfind("]")
    if 0 <= start < end:
        try:
            items = json.loads(answer[start:end + 1])
        except json.JSONDecodeError:
            items = None
    # {"translations": [...]} : le tableau est la seule liste de l'objet.
    if not isinstance(items, list) and "{" in answer:
        try:
            whole = json.loads(answer[answer.find("{"):answer.rfind("}") + 1])
        except json.JSONDecodeError:
            whole = None
        lists = [value for value in whole.values() if isinstance(value, list)] if isinstance(whole, dict) else []
        items = lists[0] if len(lists) == 1 else None
    if not isinstance(items, list) or len(items) != count or not all(isinstance(item, str) for item in items):
        return None
    return [plain(item) for item in items]


def post(url, key, model, texts):
    """Les traductions de `texts` par le modèle `model` d'une API compatible OpenAI ; None si
    la réponse ne les donne pas toutes."""
    body = json.dumps({
        "model": model,
        # Les modèles Nemotron raisonnent longuement sans cette consigne, et les jetons
        # pouvaient manquer à la réponse elle-même.
        "messages": [{"role": "system", "content": "detailed thinking off"},
                     {"role": "user", "content": PROMPT + json.dumps(texts, ensure_ascii=False)}],
        "temperature": 0.2,
        "max_tokens": 4096,
    }).encode("utf-8")
    request = urllib.request.Request(url, data=body, headers={
        "Authorization": f"Bearer {key}",
        "Content-Type": "application/json",
    })
    with urllib.request.urlopen(request, timeout=TIMEOUT) as response:
        answer = json.loads(response.read().decode("utf-8"))
    message = (answer.get("choices") or [{}])[0].get("message") or {}
    return parse(message.get("content"), len(texts))


def gemini(key, model=GEMINI_MODEL):
    """Le relais de Gemini, à donner comme `fallback` au traducteur."""
    return lambda texts: post(GEMINI_URL, key, model, texts)


class Translator:
    """Traduit des listes de textes en arrière-plan, un appel à la fois."""

    def __init__(self, key, model=MODEL, call=None, fallback=None):
        self.key = key
        self.model = model or MODEL
        self.cache = {}
        self.jobs = queue.Queue()
        self.lock = threading.Lock()
        self.idle = threading.Condition(self.lock)
        self.queued = 0
        self.stopped = False
        self.worker = None
        # Le premier traducteur, puis le relais pour un paquet qu'il n'a pas traduit.
        self.calls = [call or self._post] + ([fallback] if fallback else [])
        self.reported = set()

    def _report(self, line):
        """Un échec dit une fois par passage, pas à chaque capture."""
        if line not in self.reported:
            self.reported.add(line)
            print(line, flush=True)

    def __call__(self, texts, done):
        """`done(traductions)` reçoit les traductions de `texts`, dans l'ordre, dès qu'elles
        sont prêtes (tout de suite si elles le sont déjà) ; rien si la traduction échoue."""
        texts = list(texts)
        missing = [text for text in dict.fromkeys(texts) if text not in self.cache]
        if not missing:
            done([self.cache[text] for text in texts])
            return

        def work():
            for start in range(0, len(missing), BATCH):
                if self.stopped:
                    return
                chunk = missing[start:start + BATCH]
                for call in self.calls:
                    try:
                        translated = call(chunk)
                    except Exception as error:  # le relais essaie, sinon l'anglais reste
                        self._report(f"Traduction des problèmes impossible : {type(error).__name__}")
                        continue
                    if translated and len(translated) == len(chunk):
                        self.cache.update(zip(chunk, translated))
                        break
                    self._report("Traduction des problèmes illisible : l'anglais reste")
            # Un paquet traduit suffit : les textes restés en anglais gardent leur place.
            if any(text in self.cache for text in texts):
                done([self.cache.get(text, text) for text in texts])

        with self.lock:
            if self.stopped:
                return
            self.queued += 1
            if self.worker is None:
                # Un fil « démon » : il ne retient pas la fin du programme.
                self.worker = threading.Thread(target=self._run, daemon=True)
                self.worker.start()
        self.jobs.put(work)

    def _run(self):
        while True:
            work = self.jobs.get()
            try:
                work()
            finally:
                with self.idle:
                    self.queued -= 1
                    self.idle.notify_all()

    def wait(self, timeout):
        """Attend les traductions en cours, `timeout` secondes au plus en tout (fin du
        passage) ; au-delà, celles qui restent ne sont plus demandées."""
        deadline = time.monotonic() + timeout
        with self.idle:
            while self.queued > 0:
                left = deadline - time.monotonic()
                if left <= 0:
                    self.stopped = True
                    return
                self.idle.wait(left)

    def _post(self, texts):
        return post(NVIDIA_URL, self.key, self.model, texts)
