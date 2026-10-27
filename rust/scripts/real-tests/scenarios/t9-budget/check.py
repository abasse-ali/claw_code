"""Vérification cachée de t9-budget : python check.py DOSSIER_DU_PROJET

L'appli de budget de la session #35 de Claw Web (27 octobre 2026), comparée à celle que
Claude avait faite de la même demande. Le projet est servi en http comme une page en ligne
(`python -m http.server`), puis ouvert dans un vrai navigateur (Chromium, Chrome ou Edge,
ou `CLAW_BROWSER`) à la taille d'un téléphone réglé en mode clair, et piloté par son
protocole DevTools, sans dépendance.

Exigences dites : les montants de l'utilisateur, `+` gain sûr, `~` gain non assuré, la CAF de
103 à 206 €, utilisable sur téléphone, belle, sombre (« sobre foncé »), historique et
graphiques, dépenses variables, export. Exigences que la demande ne dit pas : montants déjà
là au premier lancement, espèces comptées seulement une fois reçues (reste prudent à
848,55 €), fourchette de la CAF et des espèces (au mieux), mode sombre même sur un
téléphone réglé en clair, texte tapé lisible, graphique dessiné, mois d'avant non
inventés, aucune erreur de script, appli installable et hors-ligne (manifeste à
`start_url` relative, service worker, vraie icône PNG), rien de plus large que l'écran,
une dépense ajoutée gardée après un rechargement.
"""

import base64
import json
import os
import re
import shutil
import socket
import struct
import subprocess
import sys
import tempfile
import time
import urllib.request
from pathlib import Path

project = Path(sys.argv[1]).resolve()
checks = []


def check(name, ok, detail=""):
    checks.append({"name": name, "ok": bool(ok), "detail": str(detail)[-300:] if detail else ""})


def done():
    print(json.dumps({"score": sum(item["ok"] for item in checks), "max": 12, "checks": checks},
                     ensure_ascii=False))
    sys.exit(0)


def free_port():
    with socket.socket() as probe:
        probe.bind(("127.0.0.1", 0))
        return probe.getsockname()[1]


def browser():
    named = os.environ.get("CLAW_BROWSER")
    if named:
        return named
    for name in ("chromium", "chromium-browser", "google-chrome", "google-chrome-stable",
                 "chrome", "msedge", "microsoft-edge"):
        found = shutil.which(name)
        if found:
            return found
    for path in (r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe",
                 r"C:\Program Files\Google\Chrome\Application\chrome.exe"):
        if Path(path).exists():
            return path
    return None


# --- Protocole DevTools : un client WebSocket minimal ---------------------------------------

class DevTools:
    def __init__(self, port, path):
        self.sock = socket.create_connection(("127.0.0.1", port), timeout=30)
        key = base64.b64encode(os.urandom(16)).decode()
        self.sock.sendall((f"GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nUpgrade: websocket\r\n"
                           f"Connection: Upgrade\r\nSec-WebSocket-Key: {key}\r\n"
                           "Sec-WebSocket-Version: 13\r\n\r\n").encode())
        head = b""
        while not head.endswith(b"\r\n\r\n"):
            head += self.sock.recv(1)
        if b" 101 " not in head.split(b"\r\n")[0]:
            raise RuntimeError(head.decode(errors="replace"))
        self.next_id = 0
        self.events = []

    def _send(self, text):
        data = text.encode()
        mask = os.urandom(4)
        size = len(data)
        if size < 126:
            header = struct.pack("!BB", 0x81, 0x80 | size)
        elif size < 65536:
            header = struct.pack("!BBH", 0x81, 0x80 | 126, size)
        else:
            header = struct.pack("!BBQ", 0x81, 0x80 | 127, size)
        self.sock.sendall(header + mask + bytes(byte ^ mask[index % 4] for index, byte in enumerate(data)))

    def _exactly(self, count):
        data = b""
        while len(data) < count:
            part = self.sock.recv(count - len(data))
            if not part:
                raise ConnectionError("the browser closed the connection")
            data += part
        return data

    def _receive(self):
        message = b""
        while True:
            first, second = self._exactly(2)
            size = second & 0x7F
            if size == 126:
                size = struct.unpack("!H", self._exactly(2))[0]
            elif size == 127:
                size = struct.unpack("!Q", self._exactly(8))[0]
            payload = self._exactly(size)
            opcode = first & 0x0F
            if opcode == 0x9:  # ping
                continue
            message += payload
            if first & 0x80:
                return json.loads(message.decode("utf-8", errors="replace"))

    def call(self, method, params=None, timeout=30):
        self.next_id += 1
        ident = self.next_id
        self._send(json.dumps({"id": ident, "method": method, "params": params or {}}))
        self.sock.settimeout(timeout)
        while True:
            message = self._receive()
            if message.get("id") == ident:
                if "error" in message:
                    raise RuntimeError(f"{method}: {message['error'].get('message')}")
                return message.get("result", {})
            self.keep(message)

    def keep(self, message):
        # Une fenêtre native bloque la page : « OK » à un message, « Annuler » à une
        # question (un « Tout effacer ? » ne doit rien effacer).
        if message.get("method") == "Page.javascriptDialogOpening":
            accept = message.get("params", {}).get("type") in ("alert", "beforeunload")
            self.next_id += 1
            self._send(json.dumps({"id": self.next_id, "method": "Page.handleJavaScriptDialog",
                                   "params": {"accept": accept}}))
        self.events.append(message)

    def pump(self, seconds):
        end = time.time() + seconds
        while time.time() < end:
            self.sock.settimeout(max(0.05, end - time.time()))
            try:
                self.keep(self._receive())
            except (socket.timeout, TimeoutError):
                return

    def js(self, expression, timeout=30):
        # Un bloc à lui : ses `const` ne gênent pas l'évaluation suivante.
        result = self.call("Runtime.evaluate", {"expression": "{" + expression + "\n}", "returnByValue": True,
                                                "awaitPromise": True}, timeout)
        if "exceptionDetails" in result:
            details = result["exceptionDetails"]
            raise RuntimeError((details.get("exception") or {}).get("description") or details.get("text", "exception"))
        return result.get("result", {}).get("value")

    def errors(self):
        found = []
        for event in self.events:
            method = event.get("method")
            params = event.get("params", {})
            if method == "Runtime.exceptionThrown":
                details = params.get("exceptionDetails", {})
                found.append((details.get("exception", {}) or {}).get("description")
                             or details.get("text", "exception"))
            elif method == "Runtime.consoleAPICalled" and params.get("type") == "error":
                found.append(" ".join(str(arg.get("value", arg.get("description", "")))
                                      for arg in params.get("args", [])))
            elif method == "Log.entryAdded" and params.get("entry", {}).get("level") == "error":
                found.append(params["entry"].get("text", ""))
        return [error for error in found if error and "favicon.ico" not in error]


# --- Ce qui se mesure dans la page -----------------------------------------------------------

# Tout le texte de la page, écrans cachés compris, et la valeur des champs ; nombres écrits
# à la française (« 1 235,15 ») ramenés à « 1235.15 ».
NUMBERS_FN = r"""const numbers = () => {
  const parts = [];
  const walker = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT, {
    acceptNode: (node) => node.parentElement && node.parentElement.closest("script, style, template, noscript") ? NodeFilter.FILTER_REJECT : NodeFilter.FILTER_ACCEPT,
  });
  for (let node = walker.nextNode(); node; node = walker.nextNode()) parts.push(node.textContent);
  for (const field of document.querySelectorAll("input, textarea, select")) if (field.value) parts.push(field.value);
  const text = parts.join(" ").replace(/(\d)[\s\u00a0\u202f\u2009.']+(?=\d{3}(?:[,.]\d|\b))/g, "$1");
  return [...text.matchAll(/\d+(?:[.,]\d+)?/g)].map((found) => Number(found[0].replace(",", ".")));
};
// Montants d'épargne (objectif, épargne mise de côté chaque mois) : ceux d'un champ ou d'un
// texte qui en parle.
const savingWords = /[ée]pargn|objectif|de c[ôo]t[ée]|saving/i;
const savings = () => {
  const found = [];
  const amounts = (text) => [...(text || "").replace(/(\d)[\s\u00a0\u202f]+(?=\d{3}\b)/g, "$1").matchAll(/\d+(?:[.,]\d+)?/g)].map((item) => Number(item[0].replace(",", ".")));
  for (const field of document.querySelectorAll("input")) {
    const named = [field.name, field.id, field.getAttribute("aria-label"), field.labels && field.labels[0] && field.labels[0].innerText].join(" ");
    if (field.value && savingWords.test(named)) found.push(...amounts(field.value));
  }
  for (const el of document.querySelectorAll("p, li, div, span, dd, td, strong, b")) {
    const text = (el.textContent || "").trim();
    if (text.length < 160 && savingWords.test(text)) found.push(...amounts(text));
  }
  return found.filter((value) => value > 0 && value <= 1000);
};
"""
NUMBERS = NUMBERS_FN + "numbers()"

SHOWN = "const shown = (el) => { const r = el.getBoundingClientRect(); const s = getComputedStyle(el); return r.width > 0 && r.height > 0 && s.visibility !== 'hidden' && s.display !== 'none'; };"

# Couleur de fond réelle d'un élément : la première opaque de ses parents.
COLORS = r"""
const rgb = (text) => { const m = (text || "").match(/[\d.]+/g); return m ? m.map(Number) : [0, 0, 0, 0]; };
const background = (el) => {
  for (let node = el; node; node = node.parentElement) {
    const [r, g, b, a = 1] = rgb(getComputedStyle(node).backgroundColor);
    if (a > 0.5) return [r, g, b];
  }
  const [r, g, b, a = 1] = rgb(getComputedStyle(document.documentElement).backgroundColor);
  return a > 0.5 ? [r, g, b] : [255, 255, 255];
};
const luminance = ([r, g, b]) => { const f = (c) => { c /= 255; return c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4; }; return 0.2126 * f(r) + 0.7152 * f(g) + 0.0722 * f(b); };
const contrast = (a, b) => { const [x, y] = [luminance(a), luminance(b)].sort((p, q) => q - p); return (x + 0.05) / (y + 0.05); };
"""

# Ouvre les écrans de l'appli un à un (liens `#…`, onglets, boutons de navigation) et rend
# ce qui y est mesuré : débordement, graphique dessiné.
SCREENS = SHOWN + COLORS + NUMBERS_FN + r"""
(async () => {
  const wait = (ms) => new Promise((done) => setTimeout(done, ms));
  const seen = { overflow: [], charts: [], screens: 0, values: [], savings: [], salary: {} };
  const measure = (where) => {
    const found = numbers();
    seen.values.push(...found);
    seen.savings.push(...savings());
    seen.salary[where] = found.filter((value) => Math.abs(value - 1235.15) < 0.006).length;
    if (document.scrollingElement.scrollWidth > innerWidth + 1) seen.overflow.push(`${where} : ${document.scrollingElement.scrollWidth}px`);
    for (const canvas of document.querySelectorAll("canvas")) {
      if (!shown(canvas) || !canvas.width || !canvas.height) continue;
      try {
        const data = canvas.getContext("2d").getImageData(0, 0, canvas.width, canvas.height).data;
        let painted = 0;
        for (let i = 3; i < data.length; i += 16) if (data[i] > 0) painted++;
        if (painted > 50) seen.charts.push(`canvas ${canvas.width}x${canvas.height} (${where})`);
      } catch (_) { seen.charts.push(`canvas (${where})`); }
    }
    for (const svg of document.querySelectorAll("svg")) {
      if (!shown(svg)) continue;
      const box = svg.getBoundingClientRect();
      if (box.width < 120 || box.height < 80) continue;
      const marks = [...svg.querySelectorAll("rect, path, polyline, circle, line")].filter((mark) => { const r = mark.getBoundingClientRect(); return r.width * r.height > 12; });
      // Deux barres et des libellés d'axe (un seul mois de données), ou trois marques.
      const labels = svg.querySelectorAll("text").length;
      if (marks.length >= 3 || (marks.length >= 2 && labels >= 2)) seen.charts.push(`svg ${Math.round(box.width)}x${Math.round(box.height)} (${where})`);
    }
    // Barres en HTML : au moins trois barres debout dans un même graphique (les barres de
    // progression, couchées, n'en sont pas).
    for (const chart of document.querySelectorAll("[class*=chart], [class*=graph], [class*=histo]")) {
      if (!shown(chart) || /progress/i.test(chart.className)) continue;
      const bars = [...chart.querySelectorAll("div, span")].filter((bar) => { const r = bar.getBoundingClientRect(); return shown(bar) && r.height >= 12 && r.width >= 4 && r.width <= 60 && r.height > r.width && getComputedStyle(bar).backgroundColor !== "rgba(0, 0, 0, 0)"; });
      if (bars.length >= 3) { seen.charts.push(`barres en HTML (${where})`); break; }
    }
  };
  measure("accueil");
  const destructive = /effacer|supprimer|r[ée]initialis|reset|vider|importer|import/i;
  const openers = [...document.querySelectorAll("a[href^='#'], [role=tab], nav button, nav a, [data-tab], [data-view], [data-screen], [data-page], [data-target]")]
    .filter((el) => !el.closest("dialog") && (el.innerText || el.getAttribute("aria-label") || "").trim() && !destructive.test(el.innerText || el.getAttribute("aria-label") || ""));
  const done = new Set();
  for (const el of openers.slice(0, 12)) {
    const label = (el.innerText || el.getAttribute("aria-label") || "").trim().replace(/\s+/g, " ").slice(0, 30);
    if (done.has(label)) continue;
    done.add(label);
    // Un menu replié au téléphone : il s'ouvre d'abord.
    if (!shown(el)) {
      const menu = [...document.querySelectorAll("button[aria-expanded=false]")].find(shown);
      if (menu) { menu.click(); await wait(350); }
    }
    if (!shown(el)) continue;
    el.click();
    await wait(600);
    seen.screens++;
    measure(label);
  }
  return seen;
})()
"""

OPEN_FORM = SHOWN + r"""
(async () => {
  const opener = [...document.querySelectorAll("button, a, [role=button]")].find((el) => shown(el) && /ajout|nouvelle|nouveau|saisir|^\s*[+＋]\s*$/i.test((el.innerText || el.getAttribute("aria-label") || "").trim()));
  if (opener) { opener.click(); await new Promise((done) => setTimeout(done, 600)); }
  return opener ? (opener.innerText || opener.getAttribute("aria-label") || "").trim().slice(0, 40) : null;
})()
"""

FIELDS = SHOWN + COLORS + r"""
(() => {
  const bad = [];
  let tried = 0;
  for (const field of document.querySelectorAll("input:not([type=hidden]):not([type=checkbox]):not([type=radio]):not([type=range]):not([type=color]):not([type=file]):not([type=submit]):not([type=button]), textarea, select")) {
    tried++;
    const style = getComputedStyle(field);
    const ratio = contrast(rgb(style.color).slice(0, 3), background(field));
    if (ratio < 3) bad.push(`${field.id || field.name || field.type} ${ratio.toFixed(1)}:1`);
  }
  return { tried, bad };
})()
"""

# Une dépense ajoutée par le premier formulaire qui a un montant, puis cherchée après un
# rechargement.
ADD = SHOWN + r"""
(async () => {
  const wait = (ms) => new Promise((done) => setTimeout(done, ms));
  const amount = (form) => form.querySelector("input[type=number], input[inputmode=decimal], input[inputmode=numeric], input[name*=mont i], input[id*=mont i], input[name*=amount i], input[id*=amount i]");
  const pick = () => [...document.querySelectorAll("form, dialog, [role=dialog], [class*=sheet], [class*=modal]")].filter((form) => amount(form) && !/objectif|goal|epargne|épargne|param|réglage|reglage|settings/i.test(form.id + " " + form.className + " " + (form.getAttribute("aria-label") || "")));
  let form = pick().find(shown) || pick()[0];
  if (!form || !shown(form)) {
    const opener = [...document.querySelectorAll("button, a, [role=button]")].find((el) => shown(el) && /ajout|nouvelle|nouveau|saisir|^\s*[+＋]\s*$/i.test((el.innerText || el.getAttribute("aria-label") || "").trim()));
    if (opener) { opener.click(); await wait(600); }
    form = pick().find(shown) || pick()[0];
  }
  if (!form) return "aucun formulaire avec un montant";
  if (form.tagName === "DIALOG" && !form.open && form.showModal) form.showModal();
  // Le <form> du conteneur, s'il en a un.
  const inner = form.tagName === "FORM" ? form : [...form.querySelectorAll("form")].find((item) => amount(item));
  if (inner) form = inner;
  const set = (field, value) => {
    const setter = Object.getOwnPropertyDescriptor(Object.getPrototypeOf(field), "value").set;
    setter.call(field, value);
    field.dispatchEvent(new Event("input", { bubbles: true }));
    field.dispatchEvent(new Event("change", { bubbles: true }));
  };
  for (const field of form.querySelectorAll("input, textarea, select")) {
    if (field === amount(form)) set(field, field.type === "number" ? "12.34" : "12,34");
    else if (field.type === "date") set(field, new Date().toISOString().slice(0, 10));
    else if (field.tagName === "SELECT") { if (!field.value && field.options.length > 1) set(field, field.options[1].value); }
    else if (["text", "search", ""].includes(field.type) || field.tagName === "TEXTAREA") set(field, "Essai Claw");
    else if (field.type === "radio" && /d[ée]pense|sortie|perte|expense|minus|out/i.test(field.value + " " + (field.labels && field.labels[0] ? field.labels[0].innerText : ""))) field.click();
  }
  const button = [...form.querySelectorAll("button, [type=submit]")].find((el) => shown(el) && /enregistr|ajout|valider|ok|save|créer|creer/i.test(el.innerText || el.value || ""));
  if (form.tagName === "FORM" && form.requestSubmit) form.requestSubmit(button && button.form === form ? button : undefined);
  else if (button) button.click();
  else return "pas de bouton pour enregistrer";
  await wait(800);
  return "envoyé";
})()
"""

MANIFEST = r"""
(async () => {
  const link = document.querySelector("link[rel=manifest]");
  const out = { manifest: link ? link.href : null, sw: false, start: null, icons: [] };
  if (navigator.serviceWorker) {
    for (let i = 0; i < 20 && !out.sw; i++) {
      const registrations = await navigator.serviceWorker.getRegistrations();
      out.sw = registrations.length > 0;
      if (!out.sw) await new Promise((done) => setTimeout(done, 250));
    }
  }
  if (!link) return out;
  try {
    const response = await fetch(link.href);
    const data = await response.json();
    out.start = data.start_url ?? null;
    out.icons = (data.icons || []).map((icon) => ({ src: new URL(icon.src, link.href).href, sizes: icon.sizes || "", type: icon.type || "" }));
  } catch (error) { out.error = String(error); }
  for (const icon of out.icons) {
    try {
      const response = await fetch(icon.src);
      const bytes = new Uint8Array(await response.arrayBuffer());
      icon.ok = response.ok;
      icon.bytes = bytes.length;
      if (bytes[0] === 0x89 && bytes[1] === 0x50) {
        const view = new DataView(bytes.buffer);
        icon.width = view.getUint32(16);
        icon.height = view.getUint32(20);
      }
    } catch (error) { icon.ok = false; }
  }
  return out;
})()
"""


def close(values, target, tolerance=0.006):
    return any(abs(value - target) < tolerance for value in values)


def page_entry():
    if (project / "index.html").exists():
        return "index.html"
    found = sorted(project.rglob("index.html"), key=lambda path: len(path.parts))
    found = [path for path in found if "node_modules" not in path.parts]
    return found[0].relative_to(project).as_posix() if found else None


def main():
    entry = page_entry()
    if not entry:
        for name in ("montants au premier lancement", "reste prudent", "au mieux", "sombre",
                     "texte tapé lisible", "graphique dessiné", "mois d'avant non inventés",
                     "rien de plus large que l'écran", "installable et hors-ligne",
                     "vraie icône PNG", "aucune erreur de script", "dépense gardée"):
            check(name, False, "aucune page index.html dans le projet")
        done()
    executable = browser()
    if not executable:
        print(json.dumps({"score": 0, "max": 12, "error": "aucun navigateur (CLAW_BROWSER)"}))
        sys.exit(0)

    port = free_port()
    server = subprocess.Popen([sys.executable, "-m", "http.server", str(port), "--bind", "127.0.0.1"],
                              cwd=project, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    profile = Path(tempfile.mkdtemp(prefix="t9-budget-"))
    args = [executable, "--headless=new", "--disable-gpu", "--no-first-run", "--no-default-browser-check",
            "--remote-debugging-port=0", f"--user-data-dir={profile}", "about:blank"]
    if hasattr(os, "geteuid") and os.geteuid() == 0:
        args.insert(1, "--no-sandbox")
    chrome = subprocess.Popen(args, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    try:
        for _ in range(100):
            active = profile / "DevToolsActivePort"
            if active.exists() and len(active.read_text().splitlines()) >= 2:
                break
            time.sleep(0.1)
        debug_port = int((profile / "DevToolsActivePort").read_text().splitlines()[0])
        for _ in range(50):
            try:
                with urllib.request.urlopen(f"http://127.0.0.1:{debug_port}/json/list", timeout=5) as answer:
                    targets = json.loads(answer.read())
                page = next(target for target in targets if target.get("type") == "page")
                break
            except (OSError, StopIteration, ValueError):
                time.sleep(0.1)
        path = page["webSocketDebuggerUrl"].split(str(debug_port), 1)[1]
        tools = DevTools(debug_port, path)
        for domain in ("Page.enable", "Runtime.enable", "Log.enable"):
            tools.call(domain)
        tools.call("Emulation.setDeviceMetricsOverride", {"width": 390, "height": 844, "deviceScaleFactor": 2, "mobile": True})
        tools.call("Emulation.setTouchEmulationEnabled", {"enabled": True, "maxTouchPoints": 5})
        # Un téléphone réglé en clair : « sobre foncé » doit rester sombre.
        tools.call("Emulation.setEmulatedMedia", {"features": [{"name": "prefers-color-scheme", "value": "light"}]})
        url = f"http://127.0.0.1:{port}/{entry}"
        tools.call("Page.navigate", {"url": url})
        tools.pump(3)

        dark = tools.js(COLORS + "luminance(background(document.body))")
        # Tous les écrans : une appli peut construire chacun à son ouverture.
        screens = tools.js(SCREENS, timeout=90) or {}
        values = screens.get("values", [])
        saved = sorted({value for value in screens.get("savings", [])})
        given = {"1235.15": 1235.15, "100": 100, "103": 103, "206": 206, "80": 80, "200": 200,
                 "450": 450, "10": 10, "21.6": 21.6, "8": 8}
        missing = [name for name, value in given.items() if not close(values, value)]
        check("les montants de l'utilisateur sont là au premier lancement", len(missing) <= 1,
              "absents : " + ", ".join(missing))

        def shown_net(target):
            """`target` affiché, ou `target` moins une épargne que l'appli affiche aussi."""
            return close(values, target) or any(close(values, target - saving) for saving in saved)

        # Sûr : salaire, mobili-jeune, CAF au plus bas (1438,15) ; charges : 589,60.
        check("reste prudent affiché (CAF au plus bas, espèces non comptées) : 848,55 € (moins l'épargne prévue)",
              shown_net(848.55), f"848,55 introuvable ; épargne affichée : {saved[:6]}")
        best = [1151.55, 1031.55, 951.55]
        check("le mois au mieux (CAF haute, espèces reçues) est affiché",
              any(shown_net(value) for value in best),
              "aucun de " + ", ".join(f"{value:.2f}" for value in best))
        check("sombre, même sur un téléphone réglé en clair", dark is not None and dark < 0.08,
              f"luminance du fond {dark}")
        check("un graphique est dessiné", screens.get("charts"),
              "; ".join(screens.get("charts", [])[:3]) or f"{screens.get('screens', 0)} écrans ouverts, aucun graphique dessiné")
        # Salaire recopié sur les mois d'avant (#35 : sur onze mois) : sur un même écran, le
        # montant revient plus souvent que ses quelques places légitimes.
        repeats = max(screens.get("salary", {}).values(), default=0)
        check("les mois d'avant ne sont pas inventés", repeats <= 4,
              f"1235,15 écrit jusqu'à {repeats} fois sur un écran")
        check("rien n'est plus large que l'écran du téléphone", not screens.get("overflow"),
              "; ".join(screens.get("overflow", [])[:4]))

        # Le texte tapé : champs de la page et du formulaire d'ajout, en clair puis en sombre.
        bad, tried = [], 0
        for scheme in ("light", "dark"):
            tools.call("Emulation.setEmulatedMedia", {"features": [{"name": "prefers-color-scheme", "value": scheme}]})
            tools.call("Page.navigate", {"url": url})
            tools.pump(2)
            tools.js(OPEN_FORM)
            fields = tools.js(FIELDS) or {"tried": 0, "bad": []}
            tried += fields["tried"]
            bad += [f"{item} ({'clair' if scheme == 'light' else 'sombre'})" for item in fields["bad"]]
        tools.call("Emulation.setEmulatedMedia", {"features": [{"name": "prefers-color-scheme", "value": "light"}]})
        check("le texte tapé dans les champs est lisible", tried and not bad,
              "; ".join(bad[:6]) or "aucun champ trouvé")

        tools.call("Page.navigate", {"url": url})
        tools.pump(2)
        manifest = tools.js(MANIFEST, timeout=60) or {}
        start = manifest.get("start")
        relative = isinstance(start, str) and not re.match(r"^(/|https?:)", start)
        check("installable et hors-ligne (manifeste à start_url relative, service worker)",
              manifest.get("manifest") and relative and manifest.get("sw"),
              f"manifeste {manifest.get('manifest')}, start_url {start!r}, service worker {manifest.get('sw')}")
        pngs = [icon for icon in manifest.get("icons", []) if icon.get("ok") and (icon.get("width") or 0) >= 192]
        check("une vraie icône PNG d'au moins 192 px", pngs,
              json.dumps(manifest.get("icons", []), ensure_ascii=False)[:280])

        added = tools.js(ADD, timeout=60)
        tools.call("Page.navigate", {"url": url})
        tools.pump(2)
        text = tools.js("document.body.innerText + ' ' + [...document.querySelectorAll('input, textarea')].map((f) => f.value).join(' ')") or ""
        values = tools.js(NUMBERS) or []
        tools.js(SCREENS, timeout=90)
        text += tools.js("document.body.innerText") or ""
        kept = "Essai Claw" in text or close(values, 12.34)
        errors = tools.errors()
        check("aucune erreur de script", not errors, " | ".join(errors[:3]))
        check("une dépense ajoutée reste après un rechargement", kept, f"ajout : {added}")
    except Exception as error:  # noqa: BLE001 - la vérification note ce qu'elle a pu voir
        check("la page a pu être vérifiée jusqu'au bout", False, repr(error))
    finally:
        chrome.terminate()
        server.terminate()
        try:
            chrome.wait(timeout=5)
        except subprocess.TimeoutExpired:
            chrome.kill()
        shutil.rmtree(profile, ignore_errors=True)
    done()


if __name__ == "__main__":
    main()
