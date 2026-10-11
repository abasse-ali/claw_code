#!/usr/bin/env python3
"""Trace lisible d'une session Claw (.claw/sessions/session-*.json).

Sert à comprendre, sans appeler de modèle, comment un modèle s'est comporté
dans une vraie session : ce qu'il a pensé, les outils appelés, leurs issues,
les remarques que Claw lui a envoyées.

    python session-trace.py SESSION.json --out trace.txt [--cut 220]
        une ligne par bloc : texte, réflexion, appel d'outil, résultat
        (« ERR » : outil en erreur, « FAIL » : commande au code de sortie non nul)

    python session-trace.py SESSION.json --out blocs.txt --full 12 13 97 [--max 6000]
        le contenu entier des messages demandés

    python session-trace.py SESSION.json --out notes.txt --notes
        les remarques de Claw ([Claw] …) reçues par le modèle, et les messages
        de l'utilisateur

Écrire dans un fichier (--out) évite les soucis d'encodage de la console
Windows ; sans --out, la sortie va sur la console en UTF-8.
"""
import argparse
import io
import json
import re
import sys


def short(text, limit):
    text = str(text).replace("\r", "").replace("\n", " / ")
    return text if len(text) <= limit else "%s …[%d]" % (text[:limit], len(text))


def is_claw_note(text):
    return text.startswith(("[Claw]", "[Claw guide]", "[Ultracode]", "[Hook", "[Contexte ajouté"))


def trace(messages, out, cut):
    stats = {"assistant": 0, "tool_use": {}, "errors": 0, "failed_commands": 0, "claw_notes": 0, "user": 0}
    for index, message in enumerate(messages):
        role = message.get("role")
        for block in message.get("blocks", []):
            kind = block.get("type")
            if kind == "text":
                text = block.get("text", "")
                if role == "user":
                    note = is_claw_note(text)
                    stats["claw_notes" if note else "user"] += 1
                    out.write("%4d %s: %s\n" % (index, "CLAW" if note else "USER", short(text, cut * 3)))
                else:
                    out.write("%4d   say(%d): %s\n" % (index, len(text), short(text, cut)))
            elif kind == "thinking":
                text = block.get("thinking", "")
                out.write("%4d   think(%d): %s\n" % (index, len(text), short(text, cut)))
            elif kind == "tool_use":
                name = block.get("name")
                stats["tool_use"][name] = stats["tool_use"].get(name, 0) + 1
                out.write("%4d   CALL %s %s\n" % (index, name, short(block.get("input"), cut)))
            elif kind == "tool_result":
                output = str(block.get("output", ""))
                error = bool(block.get("is_error"))
                failed = '"returnCodeInterpretation": "' in output
                stats["errors"] += error
                stats["failed_commands"] += failed and not error
                mark = "ERR " if error else ("FAIL" if failed else "ok  ")
                limit = cut * 2 if (error or failed) else cut // 2
                out.write("%4d     -> %s %s (%d): %s\n" % (index, mark, block.get("tool_name"), len(output), short(output, limit)))
            elif kind == "image":
                out.write("%4d [image]\n" % index)
            else:
                out.write("%4d [%s]\n" % (index, kind))
        if role == "assistant":
            stats["assistant"] += 1
    out.write("\n%d messages %s\n" % (len(messages), json.dumps(stats, ensure_ascii=False)))


def full(messages, out, wanted, limit):
    for index in wanted:
        if not 0 <= index < len(messages):
            out.write("\n########## %d : pas de message à ce numéro\n" % index)
            continue
        message = messages[index]
        out.write("\n########## %d (%s)\n" % (index, message.get("role")))
        for block in message.get("blocks", []):
            kind = block.get("type")
            if kind == "image":
                out.write("[image %s, %d caractères]\n" % (block.get("media_type"), len(block.get("data", ""))))
                continue
            for key in ("text", "thinking", "input", "output"):
                if key in block:
                    value = block[key]
                    value = value if isinstance(value, str) else json.dumps(value, ensure_ascii=False)
                    out.write("--- %s.%s (%d)\n%s\n" % (kind, key, len(value), value[:limit]))


def notes(messages, out):
    for index, message in enumerate(messages):
        for block in message.get("blocks", []):
            kind = block.get("type")
            if kind == "tool_result":
                text = block.get("output")
            elif kind == "text" and message.get("role") == "user":
                text = block.get("text")
            else:
                continue
            if not isinstance(text, str):
                continue
            for found in re.finditer(r"\[Claw(?: guide)?\][^\n]{0,900}", text):
                out.write("%d %s: %s\n\n" % (index, block.get("tool_name") or "user", found.group(0)))
            if kind == "text" and not is_claw_note(text):
                out.write("%d USER (%d): %s\n\n" % (index, len(text), text[:1500]))


def main():
    parser = argparse.ArgumentParser(description="Trace lisible d'une session Claw")
    parser.add_argument("session")
    parser.add_argument("--out", help="fichier de sortie (UTF-8)")
    parser.add_argument("--cut", type=int, default=220, help="longueur des extraits de la trace")
    parser.add_argument("--full", type=int, nargs="+", help="numéros de messages à montrer en entier")
    parser.add_argument("--max", type=int, default=6000, help="longueur maximale d'un bloc avec --full")
    parser.add_argument("--notes", action="store_true", help="remarques de Claw et messages de l'utilisateur")
    args = parser.parse_args()

    messages = json.load(io.open(args.session, encoding="utf-8")).get("messages", [])
    if args.out:
        out = io.open(args.out, "w", encoding="utf-8")
    else:
        sys.stdout.reconfigure(encoding="utf-8")
        out = sys.stdout
    if args.full:
        full(messages, out, args.full, args.max)
    elif args.notes:
        notes(messages, out)
    else:
        trace(messages, out, args.cut)
    if args.out:
        out.close()
        print("écrit :", args.out)


if __name__ == "__main__":
    main()
