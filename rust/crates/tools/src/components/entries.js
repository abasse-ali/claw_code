// Données d'une appli perso gardées sur l'appareil : ajout, suppression avec
// « Annuler », copie dans un fichier que lisent Excel et Sheets, reprise d'une
// copie. Au premier lancement, ce que l'utilisateur a donné
// ([data-entries-seed]) est posé au mois en cours : l'appli montre ses montants
// dès la première ouverture, sans inventer les mois d'avant.
// window.entries : add({ label, amount, category, date }), remove(id), list(),
// month("2026-10"), months(), totals(liste), format(montant) ; chaque
// changement envoie l'événement « entries:change » (graphiques, résumés).
(() => {
  if (window.entries) return;
  const root = document.querySelector("[data-entries]");
  const key = (root && root.dataset.entries) || "entries";
  const lang = document.documentElement.lang || "fr";
  const currency = (root && root.dataset.currency) || "EUR";
  const money = new Intl.NumberFormat(lang, { style: "currency", currency });
  const signed = new Intl.NumberFormat(lang, { style: "currency", currency, signDisplay: "exceptZero" });
  const day = new Intl.DateTimeFormat(lang, { day: "numeric", month: "short" });
  const pad = (number) => String(number).padStart(2, "0");
  const today = () => {
    const now = new Date();
    return `${now.getFullYear()}-${pad(now.getMonth() + 1)}-${pad(now.getDate())}`;
  };
  const currentMonth = () => today().slice(0, 7);
  const isDate = (text) => /^\d{4}-\d{2}-\d{2}$/.test(String(text || ""));
  // « 1 235,15 € », « 1235.15 », « -8 » : le dernier séparateur suivi d'un ou
  // deux chiffres est la virgule des décimales, les autres sont des milliers.
  const parseAmount = (value) => {
    if (typeof value === "number") return value;
    const text = String(value ?? "").replace(/[^\d,.+-]/g, "");
    const last = Math.max(text.lastIndexOf(","), text.lastIndexOf("."));
    const decimals = last >= 0 && /^\d{1,2}$/.test(text.slice(last + 1));
    const whole = (decimals ? text.slice(0, last) : text).replace(/[,.]/g, "");
    const number = Number(decimals ? `${whole}.${text.slice(last + 1)}` : whole);
    return Number.isFinite(number) ? number : NaN;
  };
  let counter = 0;
  const newId = () => `${Date.now().toString(36)}-${(counter++).toString(36)}`;
  const clean = (fields) => {
    const amount = parseAmount(fields.amount);
    return {
      ...fields,
      id: String(fields.id || newId()),
      date: isDate(fields.date) ? fields.date : today(),
      label: String(fields.label || "").trim() || "Sans nom",
      amount: Number.isFinite(amount) ? Math.round(amount * 100) / 100 : 0,
      category: String(fields.category || "").trim(),
    };
  };
  const read = () => {
    try {
      const stored = localStorage.getItem(key);
      if (stored !== null) return JSON.parse(stored).map(clean);
    } catch (_) { /* données illisibles : on repart des données de départ */ }
    const seed = document.querySelector("[data-entries-seed]");
    try { return seed ? JSON.parse(seed.textContent).map(clean) : []; } catch (_) { return []; }
  };
  let items = read();
  let fresh = null;
  const write = () => {
    try { localStorage.setItem(key, JSON.stringify(items)); } catch (_) {
      if (typeof window.toast === "function") window.toast("Mémoire du téléphone pleine : la dernière opération n'est pas gardée.", { tone: "error" });
    }
  };
  write();
  const totals = (list) => {
    const income = list.filter((item) => item.amount > 0).reduce((sum, item) => sum + item.amount, 0);
    const spending = -list.filter((item) => item.amount < 0).reduce((sum, item) => sum + item.amount, 0);
    return { income, spending, balance: income - spending };
  };
  const period = (element) => (element.dataset.period === "all" ? items : items.filter((item) => item.date.startsWith(currentMonth())));
  const newestFirst = (list) => list.map((item, index) => [item, index]).sort((a, b) => b[0].date.localeCompare(a[0].date) || b[1] - a[1]).map(([item]) => item);
  const render = () => {
    for (const list of document.querySelectorAll("[data-entries-list]")) {
      const shown = newestFirst(period(list)).slice(0, Number(list.dataset.limit) || undefined);
      list.replaceChildren(...shown.map((item) => {
        const row = document.createElement("li");
        row.className = `entries__row${item.id === fresh ? " is-new" : ""}`;
        const [year, month, date] = item.date.split("-").map(Number);
        row.innerHTML = `<span class="entries__date"></span><span class="entries__label"><strong></strong><small></small></span><span class="entries__amount"></span><button class="entries__remove" type="button" data-entries-remove>{{icon:trash-2}}</button>`;
        row.querySelector(".entries__date").textContent = day.format(new Date(year, month - 1, date));
        row.querySelector("strong").textContent = item.label;
        row.querySelector("small").textContent = item.category;
        const amount = row.querySelector(".entries__amount");
        amount.textContent = signed.format(item.amount);
        amount.classList.toggle("is-income", item.amount > 0);
        const remove = row.querySelector("[data-entries-remove]");
        remove.dataset.id = item.id;
        remove.setAttribute("aria-label", `Supprimer « ${item.label} »`);
        return row;
      }));
      const scope = list.closest("[data-entries]") || list.parentElement;
      const empty = scope && scope.querySelector("[data-entries-empty]");
      if (empty) empty.hidden = shown.length > 0;
    }
    for (const sum of document.querySelectorAll("[data-entries-sum]")) {
      const value = totals(period(sum))[sum.dataset.entriesSum] ?? 0;
      sum.textContent = sum.dataset.entriesSum === "balance" ? signed.format(value) : money.format(value);
    }
    fresh = null;
  };
  const changed = () => {
    write();
    render();
    document.dispatchEvent(new CustomEvent("entries:change", { detail: { entries: items.slice() } }));
  };
  const say = (message, options) => { if (typeof window.toast === "function") window.toast(message, options); };
  // Fichier tableur : points-virgules et virgule décimale, comme l'attend
  // Excel en français ; le BOM fait lire les accents.
  const columns = [["date", "date"], ["label", "libellé"], ["amount", "montant"], ["category", "catégorie"]];
  const cell = (value) => {
    const text = String(value ?? "");
    return /[";\r\n]/.test(text) ? `"${text.replace(/"/g, '""')}"` : text;
  };
  const exportCsv = () => {
    const extra = [...new Set(items.flatMap((item) => Object.keys(item)))].filter((name) => name !== "id" && !columns.some(([field]) => field === name));
    // L'identifiant en dernier : reprendre la même copie remplace ses
    // opérations au lieu de les doubler.
    const head = [...columns.map(([, title]) => title), ...extra, "id"];
    const lines = items.map((item) => [
      item.date, item.label, String(item.amount).replace(".", ","), item.category, ...extra.map((name) => item[name]), item.id,
    ].map(cell).join(";"));
    const blob = new Blob([`﻿${[head.join(";"), ...lines].join("\r\n")}`], { type: "text/csv;charset=utf-8" });
    const link = document.createElement("a");
    link.href = URL.createObjectURL(blob);
    link.download = `${key}-${today()}.csv`;
    document.body.append(link);
    link.click();
    link.remove();
    setTimeout(() => URL.revokeObjectURL(link.href), 2000);
    say(`Copie enregistrée : ${link.download}`);
  };
  const parseCsv = (text) => {
    const body = text.replace(/^﻿/, "");
    const first = body.split(/\r?\n/, 1)[0];
    const separator = (first.match(/;/g) || []).length >= (first.match(/,/g) || []).length ? ";" : ",";
    const rows = [];
    let row = [];
    let value = "";
    let quoted = false;
    for (let index = 0; index < body.length; index++) {
      const char = body[index];
      if (quoted) {
        if (char === '"' && body[index + 1] === '"') { value += '"'; index++; }
        else if (char === '"') quoted = false;
        else value += char;
      } else if (char === '"') quoted = true;
      else if (char === separator) { row.push(value); value = ""; }
      else if (char === "\n" || char === "\r") {
        if (char === "\r" && body[index + 1] === "\n") index++;
        row.push(value); rows.push(row); row = []; value = "";
      } else value += char;
    }
    if (value || row.length) { row.push(value); rows.push(row); }
    return rows.filter((cells) => cells.some((text) => text.trim()));
  };
  const names = { date: "date", libelle: "label", label: "label", montant: "amount", amount: "amount", categorie: "category", category: "category", id: "id" };
  const plain = (text) => text.trim().toLowerCase().normalize("NFD").replace(/[̀-ͯ]/g, "");
  const importCsv = async (file) => {
    const [head, ...rows] = parseCsv(await file.text());
    const fields = (head || []).map((title) => names[plain(title)] || title.trim());
    const found = rows
      .map((cells) => Object.fromEntries(fields.map((field, index) => [field, (cells[index] || "").trim()])))
      .filter((row) => row.label && Number.isFinite(parseAmount(row.amount)))
      .map(clean);
    if (!found.length) {
      say("Ce fichier ne contient aucune opération lisible.", { tone: "error" });
      return 0;
    }
    const before = items;
    const byId = new Map(items.map((item) => [item.id, item]));
    for (const item of found) byId.set(item.id, item);
    items = [...byId.values()];
    changed();
    say(`${found.length} ${found.length > 1 ? "opérations reprises" : "opération reprise"}`, { action: { label: "Annuler", run: () => { items = before; changed(); } } });
    return found.length;
  };
  window.entries = {
    list: () => items.slice(),
    month: (month = currentMonth()) => items.filter((item) => item.date.startsWith(month)),
    months: () => [...new Set(items.map((item) => item.date.slice(0, 7)))].sort(),
    totals,
    format: (amount) => money.format(amount),
    parseAmount,
    today,
    add(fields) {
      const item = clean(fields);
      items.push(item);
      fresh = item.id;
      changed();
      return item;
    },
    update(id, fields) {
      const index = items.findIndex((item) => item.id === id);
      if (index < 0) return null;
      items[index] = clean({ ...items[index], ...fields, id });
      changed();
      return items[index];
    },
    remove(id) {
      const index = items.findIndex((item) => item.id === id);
      if (index < 0) return;
      const [gone] = items.splice(index, 1);
      changed();
      say(`« ${gone.label} » supprimé`, { action: { label: "Annuler", run: () => { items.splice(Math.min(index, items.length), 0, gone); fresh = gone.id; changed(); } } });
    },
    clear() {
      const before = items;
      items = [];
      changed();
      say("Toutes les opérations sont effacées", { action: { label: "Annuler", run: () => { items = before; changed(); } } });
    },
    exportCsv,
    importCsv,
  };
  document.addEventListener("click", (event) => {
    const remove = event.target.closest("[data-entries-remove]");
    if (remove) { window.entries.remove(remove.dataset.id); return; }
    if (event.target.closest("[data-entries-export]")) { exportCsv(); return; }
    if (event.target.closest("[data-entries-clear]")) { window.entries.clear(); return; }
    const pick = event.target.closest("[data-entries-import]");
    if (pick) {
      const scope = pick.closest("[data-entries]") || document;
      const input = scope.querySelector("[data-entries-file]") || document.querySelector("[data-entries-file]");
      if (input) input.click();
    }
  });
  document.addEventListener("change", (event) => {
    const input = event.target.closest("[data-entries-file]");
    if (!input || !input.files || !input.files[0]) return;
    importCsv(input.files[0]).finally(() => { input.value = ""; });
  });
  // Un autre onglet de la même appli a changé les données.
  window.addEventListener("storage", (event) => {
    if (event.key !== key) return;
    items = read();
    render();
    document.dispatchEvent(new CustomEvent("entries:change", { detail: { entries: items.slice() } }));
  });
  const start = () => {
    render();
    document.dispatchEvent(new CustomEvent("entries:change", { detail: { entries: items.slice() } }));
  };
  if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", start);
  else start();
})();
