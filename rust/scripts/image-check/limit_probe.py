"""Sonde de la limite de sortie d'un modèle de NVIDIA : un long fichier demandé par un appel
d'outil, avec plusieurs valeurs de `max_tokens`, pour voir lesquelles le service accepte et si
l'appel arrive entier. La clé (NVIDIA_API_KEY) ne sort jamais de l'environnement.

    NVIDIA_API_KEY=... python3 limit_probe.py nvidia/nemotron-3-ultra-550b-a55b 16384 32768 65536
"""

import json
import os
import sys
import threading
import time
import urllib.error
import urllib.request

URL = "https://integrate.api.nvidia.com/v1/chat/completions"
TOOL = {
    "type": "function",
    "function": {
        "name": "write_file",
        "description": "Write a file.",
        "parameters": {
            "type": "object",
            "properties": {"path": {"type": "string"}, "content": {"type": "string"}},
            "required": ["path", "content"],
        },
    },
}
PROMPT = ("Write a complete single-file HTML progressive web app to log gym workouts (exercise library, "
          "sets, reps, weights, history, charts of estimated one-rep max and volume), with all its CSS and "
          "JavaScript inline, at least 1200 lines. Call write_file once with path index.html and the whole file.")


def probe(model, tokens, results):
    body = {"model": model, "max_tokens": tokens, "tools": [TOOL],
            "messages": [{"role": "user", "content": PROMPT}]}
    request = urllib.request.Request(URL, data=json.dumps(body).encode(), method="POST", headers={
        "Authorization": f"Bearer {os.environ['NVIDIA_API_KEY']}",
        "Content-Type": "application/json", "Accept": "application/json"})
    started = time.time()
    try:
        with urllib.request.urlopen(request, timeout=2400) as response:
            answer = json.loads(response.read())
        choice = answer["choices"][0]
        message = choice["message"]
        calls = message.get("tool_calls") or []
        arguments = sum(len(call["function"].get("arguments") or "") for call in calls)
        usage = answer.get("usage") or {}
        line = (f"max_tokens={tokens}: fin={choice.get('finish_reason')} sortie={usage.get('completion_tokens')} "
                f"appels={len(calls)} arguments={arguments} car. texte={len(message.get('content') or '')} car. "
                f"raisonnement={len(message.get('reasoning_content') or '')} car.")
    except urllib.error.HTTPError as error:
        line = f"max_tokens={tokens}: HTTP {error.code} {error.read().decode('utf-8', 'replace')[:300]}"
    except (urllib.error.URLError, TimeoutError) as error:
        line = f"max_tokens={tokens}: erreur réseau {error}"
    results.append(f"{line} ({round(time.time() - started)} s)")


def main():
    model, limits = sys.argv[1], [int(value) for value in sys.argv[2:]]
    results = []
    threads = [threading.Thread(target=probe, args=(model, tokens, results)) for tokens in limits]
    for thread in threads:
        thread.start()
    for thread in threads:
        thread.join()
    print(f"# {model}")
    for line in sorted(results, key=lambda text: int(text.split("=")[1].split(":")[0])):
        print(line)


if __name__ == "__main__":
    main()
