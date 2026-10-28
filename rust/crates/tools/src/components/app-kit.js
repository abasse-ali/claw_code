// Kit d'appli de Claw : ce qui fait marcher une appli perso d'une seule page,
// sans dépendance. Les données restent sur le téléphone (localStorage).
//
// window.toast(message, { tone: "error", action: { label, run } })
// window.entries : list(), month("2026-10"), months(), totals(liste),
//   add({ label, amount, category, date }), update(id, champs), remove(id),
//   clear(), format(montant), signed(montant), parseAmount(texte), today() ;
//   chaque changement envoie « entries:change ».
// window.app : month (« 2026-10 », le mois affiché), setMonth(mois),
//   monthName(mois) ; chaque changement de mois envoie « monthchange », chaque
//   changement d'écran « screenchange ».
// window.drawChart(figure, { labels, series: [{ name, values, tone: "in" | "out" }] }).
(() => {
  if (window.app && window.app.kit) return;
  const lang = document.documentElement.lang || "fr";
  const currency = document.documentElement.dataset.currency || "EUR";
  const money = new Intl.NumberFormat(lang, { style: "currency", currency });
  const signedMoney = new Intl.NumberFormat(lang, { style: "currency", currency, signDisplay: "exceptZero" });
  const pad = (number) => String(number).padStart(2, "0");
  const today = () => {
    const now = new Date();
    return `${now.getFullYear()}-${pad(now.getMonth() + 1)}-${pad(now.getDate())}`;
  };
  const shiftMonth = (month, step) => {
    const [year, number] = month.split("-").map(Number);
    const date = new Date(year, number - 1 + step, 1);
    return `${date.getFullYear()}-${pad(date.getMonth() + 1)}`;
  };
  const monthName = (month) => {
    const [year, number] = month.split("-").map(Number);
    const text = new Intl.DateTimeFormat(lang, { month: "long", year: "numeric" }).format(new Date(year, number - 1, 1));
    return text.charAt(0).toUpperCase() + text.slice(1);
  };
  const ready = (run) => {
    if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", run);
    else run();
  };

  // Icônes des catégories (Lucide) : le nom de la catégorie, sinon celui de l'opération.
  const icons = {
    house: `{{icon:house}}`,
    cart: `{{icon:shopping-cart}}`,
    bus: `{{icon:bus}}`,
    car: `{{icon:car}}`,
    phone: `{{icon:smartphone}}`,
    repeat: `{{icon:repeat}}`,
    food: `{{icon:utensils}}`,
    games: `{{icon:gamepad-2}}`,
    health: `{{icon:heart-pulse}}`,
    work: `{{icon:briefcase}}`,
    bank: `{{icon:landmark}}`,
    cash: `{{icon:banknote}}`,
    savings: `{{icon:piggy-bank}}`,
    bag: `{{icon:shopping-bag}}`,
    gift: `{{icon:gift}}`,
    energy: `{{icon:zap}}`,
    shield: `{{icon:shield}}`,
    study: `{{icon:graduation-cap}}`,
    coffee: `{{icon:coffee}}`,
    sport: `{{icon:dumbbell}}`,
    travel: `{{icon:plane}}`,
    book: `{{icon:book-open}}`,
    tag: `{{icon:tag}}`,
  };
  const iconWords = [
    ["house", "logement loyer maison appartement rent home housing"],
    ["cart", "courses alimentaire alimentation nourriture supermarche epicerie groceries food"],
    ["bus", "transport transports bus metro tram train navigo velo"],
    ["car", "voiture essence carburant parking peage car fuel"],
    ["phone", "forfait telephone mobile internet box phone"],
    ["repeat", "abonnement abonnements streaming subscription claude netflix spotify"],
    ["food", "restaurant restos resto sorties repas cantine"],
    ["games", "loisirs jeux cinema sortie games leisure"],
    ["health", "sante pharmacie medecin mutuelle health"],
    ["work", "salaire revenus revenu travail paie prime salary income job"],
    ["bank", "caf aides aide allocation allocations bourse apl mobili jeune mobili-jeune banque"],
    ["cash", "especes espece cash liquide"],
    ["savings", "epargne economies livret savings"],
    ["bag", "shopping vetements habits clothes"],
    ["gift", "cadeaux cadeau gift"],
    ["energy", "energie electricite gaz eau energy"],
    ["shield", "assurance assurances insurance"],
    ["study", "etudes ecole education formation"],
    ["coffee", "cafe boulangerie snack"],
    ["sport", "sport salle gym"],
    ["travel", "voyage vacances avion travel"],
    ["book", "livres livre book"],
  ];
  const plain = (text) => String(text || "").toLowerCase().normalize("NFD").replace(/[̀-ͯ]/g, "");
  const iconFor = (...names) => {
    for (const name of names) {
      const words = plain(name).split(/[^a-z0-9-]+/).filter(Boolean);
      const found = iconWords.find(([, list]) => words.some((word) => list.split(" ").includes(word)));
      if (found) return icons[found[0]];
    }
    return icons.tag;
  };

  // Messages courts, avec une action au besoin (« Annuler »).
  const toastRegion = () => {
    let region = document.querySelector(".toasts");
    if (!region) {
      region = document.createElement("div");
      region.className = "toasts";
      region.setAttribute("aria-live", "polite");
      document.body.append(region);
    }
    return region;
  };
  window.toast = (message, options = {}) => {
    const item = document.createElement("div");
    item.className = `toast${options.tone === "error" ? " is-error" : ""}`;
    item.setAttribute("role", options.tone === "error" ? "alert" : "status");
    const text = document.createElement("span");
    text.className = "toast__text";
    text.textContent = message;
    item.append(text);
    const leave = () => {
      if (!item.isConnected) return;
      item.classList.add("is-leaving");
      setTimeout(() => item.remove(), 220);
    };
    if (options.action && typeof options.action.run === "function") {
      const button = document.createElement("button");
      button.type = "button";
      button.className = "toast__action";
      button.textContent = options.action.label || "Annuler";
      button.addEventListener("click", () => { options.action.run(); leave(); });
      item.append(button);
    }
    // Un message à la fois : le nouveau remplace le précédent.
    const region = toastRegion();
    region.replaceChildren(item);
    setTimeout(leave, options.duration || (options.action ? 6000 : 3500));
    return item;
  };
  const say = (message, options) => window.toast(message, options);

  // Fenêtres natives : data-dialog-open="id" ouvre, data-dialog-close ferme ;
  // un appui commencé sur le fond ferme aussi, et le focus revient au bouton.
  document.documentElement.dataset.dialogsReady = "1";
  let opener = null;
  let pressed = null;
  document.addEventListener("pointerdown", (event) => { pressed = event.target; }, true);
  document.addEventListener("click", (event) => {
    const open = event.target.closest("[data-dialog-open]");
    if (open) {
      const dialog = document.getElementById(open.dataset.dialogOpen);
      if (dialog && typeof dialog.showModal === "function" && !dialog.open) {
        opener = open;
        dialog.dispatchEvent(new CustomEvent("dialog:opening", { detail: { opener: open } }));
        dialog.showModal();
      }
      return;
    }
    const close = event.target.closest("[data-dialog-close]");
    if (close) {
      const dialog = close.closest("dialog");
      if (dialog) dialog.close();
      return;
    }
    if (event.target instanceof HTMLDialogElement && event.target.open && pressed === event.target) event.target.close();
  });
  document.addEventListener("close", (event) => {
    if (event.target instanceof HTMLDialogElement && opener) {
      opener.focus({ preventScroll: true });
      opener = null;
    }
  }, true);

  // Mois affiché : les boutons data-month-step="-1" et "1", le titre data-month-label.
  const app = { kit: true, month: today().slice(0, 7), monthName, money: (value) => money.format(value), signed: (value) => signedMoney.format(value) };
  window.app = app;
  const showMonth = () => {
    for (const label of document.querySelectorAll("[data-month-label]")) label.textContent = monthName(app.month);
    for (const back of document.querySelectorAll("[data-month-today]")) back.hidden = app.month === today().slice(0, 7);
  };
  app.setMonth = (month) => {
    if (!/^\d{4}-\d{2}$/.test(String(month || ""))) return;
    app.month = month;
    showMonth();
    document.dispatchEvent(new CustomEvent("monthchange", { detail: { month } }));
  };
  document.addEventListener("click", (event) => {
    const step = event.target.closest("[data-month-step]");
    if (step) { app.setMonth(shiftMonth(app.month, Number(step.dataset.monthStep) || 0)); return; }
    if (event.target.closest("[data-month-today]")) app.setMonth(today().slice(0, 7));
  });

  // Écrans : la barre d'onglets montre une section data-screen à la fois ;
  // l'écran ouvert suit l'adresse (#bilan), Retour revient au précédent.
  const screens = () => [...document.querySelectorAll("[data-screen]")];
  const showScreen = (id, moved) => {
    const all = screens();
    if (!all.length) return;
    const screen = all.find((item) => item.id === id);
    if (!screen && id && moved) return;
    const shown = screen || all[0];
    for (const item of all) item.hidden = item !== shown;
    for (const tab of document.querySelectorAll("[data-app-tabs] a[href^='#']")) {
      if (tab.getAttribute("href") === `#${shown.id}`) tab.setAttribute("aria-current", "page");
      else tab.removeAttribute("aria-current");
    }
    if (moved) {
      window.scrollTo({ top: 0 });
      const title = shown.querySelector("h1, h2");
      if (title) { title.tabIndex = -1; title.focus({ preventScroll: true }); }
    }
    document.dispatchEvent(new CustomEvent("screenchange", { detail: { screen: shown.id } }));
  };
  window.addEventListener("hashchange", () => showScreen(decodeURIComponent(location.hash.slice(1)), true));

  // Données gardées sur le téléphone. Au premier lancement, ce que l'utilisateur
  // a donné ([data-entries-seed]) est posé au mois en cours, jamais avant.
  const seedNode = document.querySelector("[data-entries-seed]");
  const key = (seedNode && seedNode.dataset.entriesSeed) || document.documentElement.dataset.store || "appli";
  const isDate = (text) => /^\d{4}-\d{2}-\d{2}$/.test(String(text || ""));
  // « 1 235,15 € », « 1235.15 », « -8 » : le dernier séparateur suivi d'un ou
  // deux chiffres est celui des décimales, les autres sont des milliers.
  const parseAmount = (value) => {
    if (typeof value === "number") return value;
    if (/\p{L}/u.test(String(value ?? "").replace(/€|\beur(os?)?\b/gi, ""))) return NaN;
    const text = String(value ?? "").replace(/[^\d,.+-]/g, "");
    if (!/\d/.test(text)) return NaN;
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
      label: String(fields.label || fields.category || "").trim() || "Sans nom",
      amount: Number.isFinite(amount) ? Math.round(amount * 100) / 100 : 0,
      category: String(fields.category || "").trim(),
    };
  };
  const read = () => {
    try {
      const stored = localStorage.getItem(key);
      if (stored !== null) return JSON.parse(stored).map(clean);
    } catch (_) { /* données illisibles : on repart des données de départ */ }
    try { return seedNode ? JSON.parse(seedNode.textContent).map(clean) : []; } catch (_) { return []; }
  };
  let items = read();
  let fresh = null;
  const write = () => {
    try { localStorage.setItem(key, JSON.stringify(items)); } catch (_) {
      say("Mémoire du téléphone pleine : la dernière opération n'est pas gardée.", { tone: "error" });
    }
  };
  write();
  const totals = (list) => {
    const income = list.filter((item) => item.amount > 0).reduce((sum, item) => sum + item.amount, 0);
    // Math.abs : jamais « -0,00 € » quand rien n'est dépensé.
    const spending = Math.abs(list.filter((item) => item.amount < 0).reduce((sum, item) => sum + item.amount, 0));
    return { income, spending, balance: income - spending, count: list.length };
  };
  const inMonth = (month) => items.filter((item) => item.date.startsWith(month));
  const scopeOf = (node) => {
    let list = node.dataset.period === "all" ? items.slice() : inMonth(app.month);
    if (node.dataset.kind === "in") list = list.filter((item) => item.amount > 0);
    if (node.dataset.kind === "out") list = list.filter((item) => item.amount < 0);
    if (node.dataset.category) list = list.filter((item) => plain(item.category) === plain(node.dataset.category));
    return list;
  };
  const newestFirst = (list) => list.map((item, index) => [item, index]).sort((a, b) => b[0].date.localeCompare(a[0].date) || b[1] - a[1]).map(([item]) => item);
  const day = new Intl.DateTimeFormat(lang, { day: "numeric", month: "short" });
  const dayOf = (date) => {
    const [year, month, number] = date.split("-").map(Number);
    return day.format(new Date(year, month - 1, number));
  };
  const row = (item) => {
    const node = document.createElement("li");
    node.className = `row${item.id === fresh ? " is-new" : ""}`;
    node.dataset.id = item.id;
    const income = item.amount > 0;
    node.innerHTML = `<span class="row__icon ${income ? "row__icon--in" : "row__icon--accent"}">${iconFor(item.category, item.label)}</span><span class="row__main"><span class="row__title"><span class="row__text"></span></span><span class="row__sub"></span></span><span class="row__end"><span class="row__amount"></span></span><button class="icon-btn row__action" type="button" data-entries-edit>{{icon:pencil}}</button>`;
    node.querySelector(".row__text").textContent = item.label;
    node.querySelector(".row__sub").textContent = [item.category && item.category !== item.label ? item.category : "", dayOf(item.date)].filter(Boolean).join(" · ");
    const amount = node.querySelector(".row__amount");
    amount.textContent = signedMoney.format(item.amount);
    amount.classList.toggle("row__amount--in", income);
    const edit = node.querySelector("[data-entries-edit]");
    edit.dataset.entriesEdit = item.id;
    edit.setAttribute("aria-label", `Modifier « ${item.label} »`);
    return node;
  };
  const render = () => {
    for (const list of document.querySelectorAll("[data-entries-list]")) {
      const all = newestFirst(scopeOf(list));
      const shown = all.slice(0, Number(list.dataset.limit) || undefined);
      list.replaceChildren(...shown.map(row));
      const scope = list.closest("[data-entries-scope], .card, section") || list.parentElement;
      const empty = scope && scope.querySelector("[data-entries-empty]");
      if (empty) empty.hidden = shown.length > 0;
      list.hidden = shown.length === 0 && Boolean(empty);
    }
    for (const sum of document.querySelectorAll("[data-entries-sum]")) {
      const value = totals(scopeOf(sum))[sum.dataset.entriesSum] ?? 0;
      if (sum.dataset.entriesSum === "count") sum.textContent = String(value);
      else sum.textContent = sum.dataset.entriesSum === "balance" ? signedMoney.format(value) : money.format(value);
    }
    fresh = null;
  };
  const changed = () => {
    write();
    render();
    document.dispatchEvent(new CustomEvent("entries:change", { detail: { entries: items.slice() } }));
  };
  // Copie pour un tableur : points-virgules et virgule décimale (Excel en
  // français) ; l'identifiant en dernier, pour reprendre sans doubler.
  const columns = [["date", "date"], ["label", "libellé"], ["amount", "montant"], ["category", "catégorie"]];
  const cell = (value) => {
    const text = String(value ?? "");
    return /[";\r\n]/.test(text) ? `"${text.replace(/"/g, '""')}"` : text;
  };
  const exportCsv = () => {
    const extra = [...new Set(items.flatMap((item) => Object.keys(item)))].filter((name) => name !== "id" && !columns.some(([field]) => field === name));
    const head = [...columns.map(([, title]) => title), ...extra, "id"];
    const lines = items.map((item) => [item.date, item.label, String(item.amount).replace(".", ","), item.category, ...extra.map((name) => item[name]), item.id].map(cell).join(";"));
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
    let cells = [];
    let value = "";
    let quoted = false;
    for (let index = 0; index < body.length; index += 1) {
      const char = body[index];
      if (quoted) {
        if (char === '"' && body[index + 1] === '"') { value += '"'; index += 1; }
        else if (char === '"') quoted = false;
        else value += char;
      } else if (char === '"') quoted = true;
      else if (char === separator) { cells.push(value); value = ""; }
      else if (char === "\n" || char === "\r") {
        if (char === "\r" && body[index + 1] === "\n") index += 1;
        cells.push(value); rows.push(cells); cells = []; value = "";
      } else value += char;
    }
    if (value || cells.length) { cells.push(value); rows.push(cells); }
    return rows.filter((line) => line.some((text) => text.trim()));
  };
  const names = { date: "date", libelle: "label", label: "label", montant: "amount", amount: "amount", categorie: "category", category: "category", id: "id" };
  const importCsv = async (file) => {
    const [head, ...lines] = parseCsv(await file.text());
    const fields = (head || []).map((title) => names[plain(title).trim()] || title.trim());
    const found = lines
      .map((cells) => Object.fromEntries(fields.map((field, index) => [field, (cells[index] || "").trim()])))
      .filter((line) => line.label && Number.isFinite(parseAmount(line.amount)))
      .map(clean);
    if (!found.length) { say("Ce fichier ne contient aucune opération lisible.", { tone: "error" }); return 0; }
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
    month: (month = app.month) => inMonth(month),
    months: () => [...new Set(items.map((item) => item.date.slice(0, 7)))].sort(),
    totals,
    format: (amount) => money.format(amount),
    signed: (amount) => signedMoney.format(amount),
    parseAmount,
    today,
    iconFor,
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
    if (event.target.closest("[data-entries-export]")) { exportCsv(); return; }
    if (event.target.closest("[data-entries-clear]")) { window.entries.clear(); return; }
    if (event.target.closest("[data-entries-import]")) {
      const input = document.querySelector("[data-entries-file]");
      if (input) input.click();
    }
  });
  document.addEventListener("change", (event) => {
    const input = event.target.closest("[data-entries-file]");
    if (!input || !input.files || !input.files[0]) return;
    importCsv(input.files[0]).finally(() => { input.value = ""; });
  });
  window.addEventListener("storage", (event) => {
    if (event.key !== key) return;
    items = read();
    changed();
  });

  // Volet d'ajout : genre, montant (« 12,50 » comme « 12.50 »), montants
  // rapides, catégorie, libellé et date ; il sert aussi à modifier une
  // opération (une ligne touchée), avec « Supprimer ».
  const sheets = () => [...document.querySelectorAll("form[data-entry-form]")];
  const setupSheet = (form) => {
    if (form.dataset.ready) return;
    form.dataset.ready = "1";
    const dialog = form.closest("dialog");
    const amount = form.elements.amount;
    const amountBox = amount && amount.closest(".sheet__amount, .field");
    const title = form.querySelector("[data-entry-title]");
    const remove = form.querySelector("[data-entry-delete]");
    const submit = form.querySelector("[type=submit]");
    const words = { title: title ? title.textContent : "", submit: submit ? submit.textContent : "" };
    const mark = (bad) => {
      if (amountBox) amountBox.classList.toggle("is-invalid", bad);
      if (amount) amount.setAttribute("aria-invalid", String(bad));
      return bad;
    };
    const fill = (item) => {
      form.reset();
      form.dataset.editing = item ? item.id : "";
      mark(false);
      if (form.elements.date) form.elements.date.value = item ? item.date : today();
      if (item) {
        if (amount) amount.value = String(Math.abs(item.amount)).replace(".", ",");
        if (form.elements.label) form.elements.label.value = item.label;
        const kind = form.querySelector(`input[name=kind][value="${item.amount > 0 ? "in" : "out"}"]`);
        if (kind) kind.checked = true;
        const category = [...form.querySelectorAll("input[name=category]")].find((input) => input.value === item.category);
        if (category) category.checked = true;
        else if (form.elements.category && form.elements.category.tagName === "SELECT") form.elements.category.value = item.category;
      }
      if (title) title.textContent = item ? "Modifier l'opération" : words.title;
      if (submit) submit.textContent = item ? "Enregistrer" : words.submit;
      if (remove) remove.hidden = !item;
    };
    if (dialog) {
      dialog.addEventListener("dialog:opening", () => fill(null));
      new MutationObserver(() => {
        if (dialog.open) setTimeout(() => { if (amount && !form.dataset.editing) amount.focus(); }, 60);
      }).observe(dialog, { attributes: true, attributeFilter: ["open"] });
    }
    fill(null);
    form.addEventListener("click", (event) => {
      const quick = event.target.closest("[data-quick]");
      if (!quick || !amount) return;
      const current = parseAmount(amount.value);
      const next = (Number.isFinite(current) ? current : 0) + Number(quick.dataset.quick);
      amount.value = String(Math.round(next * 100) / 100).replace(".", ",");
      mark(false);
    });
    if (amount) amount.addEventListener("input", () => { if (amountBox && amountBox.classList.contains("is-invalid")) mark(!(parseAmount(amount.value) > 0)); });
    if (remove) {
      remove.addEventListener("click", () => {
        const id = form.dataset.editing;
        if (dialog) dialog.close();
        if (id) window.entries.remove(id);
      });
    }
    form.addEventListener("submit", (event) => {
      event.preventDefault();
      const value = amount ? parseAmount(amount.value) : NaN;
      if (mark(!(value > 0))) { if (amount) amount.focus(); return; }
      const income = form.elements.kind && form.elements.kind.value === "in";
      const picked = form.querySelector("input[name=category]:checked");
      const category = picked ? picked.value : (form.elements.category && form.elements.category.value) || "";
      const fields = {
        label: form.elements.label ? form.elements.label.value.trim() || category : category,
        amount: income ? value : -value,
        category,
        date: form.elements.date && form.elements.date.value ? form.elements.date.value : today(),
      };
      const editing = form.dataset.editing;
      if (editing) {
        window.entries.update(editing, fields);
        if (dialog) dialog.close();
        say("Opération modifiée");
        return;
      }
      const item = window.entries.add(fields);
      if (dialog) dialog.close();
      // L'opération d'un autre mois : l'appli montre ce mois.
      if (!item.date.startsWith(app.month)) app.setMonth(item.date.slice(0, 7));
      say(`${income ? "Rentrée" : "Dépense"} de ${money.format(value)} enregistrée`, { action: { label: "Annuler", run: () => { items = items.filter((entry) => entry.id !== item.id); changed(); } } });
    });
    form.openFor = (item) => {
      fill(item);
      if (dialog && !dialog.open) dialog.showModal();
    };
  };
  document.addEventListener("click", (event) => {
    const edit = event.target.closest("[data-entries-edit]");
    if (!edit) return;
    const item = items.find((entry) => entry.id === edit.dataset.entriesEdit);
    const form = sheets()[0];
    if (!item || !form || typeof form.openFor !== "function") return;
    opener = edit;
    form.openFor(item);
  });

  // Graphique en barres en SVG, à la largeur de son écran : redessiné quand
  // l'écran s'ouvre, quand la taille, le mois ou les données changent. Les
  // noms des mois s'espacent quand la place manque (jamais superposés).
  const svgNs = "http://www.w3.org/2000/svg";
  const own = new WeakMap();
  const drawnAt = new WeakMap();
  const make = (name, attributes, parent) => {
    const node = document.createElementNS(svgNs, name);
    for (const [attribute, value] of Object.entries(attributes)) node.setAttribute(attribute, value);
    if (parent) parent.append(node);
    return node;
  };
  const fromEntries = (figure) => {
    const count = Math.max(1, Math.min(24, Number(figure.dataset.chartMonths) || 6));
    const months = [];
    for (let back = count - 1; back >= 0; back -= 1) months.push(shiftMonth(app.month, -back));
    const short = new Intl.DateTimeFormat(lang, { month: "short" });
    const sums = months.map((month) => totals(inMonth(month)));
    return {
      labels: months.map((month) => {
        const [year, number] = month.split("-").map(Number);
        return short.format(new Date(year, number - 1, 1)).replace(".", "");
      }),
      series: [
        { name: "Rentrées", tone: "in", values: sums.map((sum) => sum.income) },
        { name: "Dépenses", tone: "out", values: sums.map((sum) => sum.spending) },
      ],
    };
  };
  const niceTop = (value) => {
    if (value <= 0) return 100;
    const step = 10 ** Math.floor(Math.log10(value));
    return [1, 2, 2.5, 5, 10].map((factor) => factor * step).find((top) => top >= value);
  };
  const draw = (figure) => {
    const plot = figure.querySelector("[data-chart-plot]") || figure;
    const width = Math.round(plot.clientWidth);
    if (width < 40) return;
    const data = own.get(figure) || (figure.dataset.chart === "entries" ? fromEntries(figure) : null);
    if (!data || !data.labels || !data.series) return;
    const empty = figure.querySelector("[data-chart-empty]");
    const anything = data.series.some((serie) => serie.values.some((value) => value > 0));
    if (empty) empty.hidden = anything;
    // Rien à montrer : pas d'axes vides, seulement la phrase du graphique vide.
    plot.hidden = !anything && Boolean(empty);
    const height = Number(figure.dataset.chartHeight) || 200;
    const axis = new Intl.NumberFormat(lang, { style: "currency", currency, notation: "compact", maximumFractionDigits: 1 });
    const full = new Intl.NumberFormat(lang, { style: "currency", currency });
    const top = niceTop(anything ? Math.max(...data.series.flatMap((serie) => serie.values)) : 0);
    const left = Math.min(64, 10 + axis.format(top).length * 6.5);
    const bottom = 24;
    const inner = height - bottom - 8;
    const svg = make("svg", { viewBox: `0 0 ${width} ${height}`, width, height, role: "img" });
    svg.setAttribute("aria-label", data.labels.map((label, index) => `${label} : ${data.series.map((serie) => `${serie.name.toLowerCase()} ${full.format(serie.values[index] || 0)}`).join(", ")}`).join(" ; "));
    for (let step = 0; step <= 4; step += 1) {
      const y = 8 + inner - (inner * step) / 4;
      make("line", { class: "chart__grid", x1: left, x2: width, y1: y, y2: y }, svg);
      if (step % 2 === 0) {
        const text = make("text", { class: "chart__axis", x: left - 8, y: y + 4, "text-anchor": "end" }, svg);
        text.textContent = axis.format((top * step) / 4);
      }
    }
    const slot = (width - left) / data.labels.length;
    const bar = Math.max(3, Math.min(14, (slot * 0.62) / data.series.length));
    // Un nom de mois tous les « every » : jamais deux noms qui se touchent.
    const longest = Math.max(...data.labels.map((label) => String(label).length));
    const every = Math.max(1, Math.ceil((longest * 6.6 + 8) / slot));
    data.labels.forEach((label, index) => {
      const middle = left + slot * index + slot / 2;
      data.series.forEach((serie, which) => {
        const value = Math.max(0, serie.values[index] || 0);
        if (!value) return;
        const tall = Math.max(3, (inner * value) / top);
        const x = middle - (bar * data.series.length) / 2 + bar * which;
        const rect = make("rect", { class: `chart__bar chart__bar--${serie.tone === "in" || serie.tone === "income" ? "in" : "out"}`, x: x + 1, y: 8 + inner - tall, width: Math.max(2, bar - 2), height: tall, rx: Math.min(4, (bar - 2) / 2) }, svg);
        make("title", {}, rect).textContent = `${serie.name}, ${label} : ${full.format(value)}`;
      });
      if ((data.labels.length - 1 - index) % every === 0) {
        const text = make("text", { class: "chart__axis", x: middle, y: height - 6, "text-anchor": "middle" }, svg);
        text.textContent = label;
      }
    });
    plot.replaceChildren(svg);
    drawnAt.set(figure, width);
  };
  const charts = () => document.querySelectorAll("[data-chart]");
  window.drawChart = (figure, data) => { own.set(figure, data); draw(figure); };
  const resized = typeof ResizeObserver === "function" ? new ResizeObserver((entries) => {
    const figures = entries
      .map((entry) => [entry.target.closest("[data-chart]"), Math.round(entry.contentRect.width)])
      .filter(([figure, width]) => figure && width !== drawnAt.get(figure));
    if (figures.length) requestAnimationFrame(() => figures.forEach(([figure]) => draw(figure)));
  }) : null;

  document.addEventListener("screenchange", () => requestAnimationFrame(() => charts().forEach(draw)));
  document.addEventListener("entries:change", () => charts().forEach(draw));
  document.addEventListener("monthchange", () => { render(); charts().forEach(draw); });

  ready(() => {
    showMonth();
    sheets().forEach(setupSheet);
    for (const figure of charts()) {
      if (resized) resized.observe(figure.querySelector("[data-chart-plot]") || figure);
    }
    showScreen(decodeURIComponent(location.hash.slice(1)), false);
    render();
    charts().forEach(draw);
    document.dispatchEvent(new CustomEvent("entries:change", { detail: { entries: items.slice() } }));
    // Appli installée : hors-ligne grâce au service worker, s'il est là.
    if ("serviceWorker" in navigator && location.protocol.startsWith("http") && document.querySelector("link[rel=manifest]")) {
      const register = () => navigator.serviceWorker.register("sw.js").catch(() => {});
      if (document.readyState === "complete") register();
      else window.addEventListener("load", register);
    }
  });
})();
