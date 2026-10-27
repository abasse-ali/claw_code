// Parcours de la page comme un visiteur : Claw clique chaque bouton et regarde
// s'il se passe quelque chose (la page change, elle defile, un message, une
// fenetre, le presse-papiers, un envoi, un telechargement), suit chaque lien,
// remplit et envoie chaque formulaire, y compris ceux des fenetres. Un bouton
// qui ne fait rien est le defaut le plus frequent d'un site genere ; une
// erreur au clic, un lien vers une section absente, un formulaire qui
// recharge la page aussi. Lance apres la capture : il change la page.
(async () => {
  const wait = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const shown = (el) => {
    const style = getComputedStyle(el);
    if (style.display === "none" || style.visibility === "hidden") return false;
    const box = el.getBoundingClientRect();
    return box.width > 0 && box.height > 0;
  };
  const label = (el) => {
    let text = (el.innerText || el.value || el.getAttribute("aria-label") || el.getAttribute("title") || "")
      .replace(/\s+/g, " ").trim();
    if (text.length > 40) text = `${text.slice(0, 39)}...`;
    let tag = el.tagName.toLowerCase();
    if (el.id) tag += `#${el.id}`;
    else if (typeof el.className === "string" && el.className.trim()) tag += `.${el.className.trim().split(/\s+/)[0]}`;
    return text ? `${tag} "${text}"` : tag;
  };

  // Ce qu'un clic peut faire sans changer la page compte comme une action ;
  // rien ne doit quitter la page ni bloquer l'essai.
  let effects = 0;
  const count = () => { effects += 1; };
  window.alert = count;
  window.confirm = () => { count(); return false; };
  window.prompt = () => { count(); return null; };
  window.open = () => { count(); return null; };
  // L'impression ne montre rien sans écran : un bouton « Imprimer » n'est pas
  // mort. Vu le 27 octobre 2026 (session #32, passage 7) : signalé à chaque
  // revue, l'agent l'a récrit trois fois.
  window.print = count;
  try {
    if (navigator.clipboard) navigator.clipboard.writeText = async () => { count(); };
  } catch (_) { /* presse-papiers absent */ }
  try { navigator.share = async () => { count(); }; } catch (_) { /* partage absent */ }
  try { navigator.canShare = () => true; } catch (_) { /* partage absent */ }
  HTMLAnchorElement.prototype.click = function click() { count(); };
  try { HTMLFormElement.prototype.submit = function submit() { count(); }; } catch (_) { /* rien */ }
  // Un bouton qui ouvre le choix d'un fichier (« Importer ») : le navigateur
  // sans écran n'affiche aucune fenêtre, mais le bouton a bien agi. Vu le 27
  // octobre 2026 (session #35) : « Importer (CSV) » dit « ne fait rien » à
  // chaque revue, l'agent l'a réécrit trois fois.
  try {
    const nativeClick = HTMLInputElement.prototype.click;
    HTMLInputElement.prototype.click = function click() {
      if ((this.type || "").toLowerCase() === "file") { count(); return; }
      return nativeClick.call(this);
    };
    if (HTMLInputElement.prototype.showPicker) {
      HTMLInputElement.prototype.showPicker = function showPicker() { count(); };
    }
  } catch (_) { /* champ absent */ }
  if (window.showOpenFilePicker) window.showOpenFilePicker = async () => { count(); return []; };
  // Un libellé lié à un champ de fichier ouvre lui aussi le choix.
  window.addEventListener("click", (event) => {
    const label = event.target && event.target.closest && event.target.closest("label");
    const field = label && (label.control || label.querySelector("input[type=file]"));
    if (field && (field.type || "").toLowerCase() === "file") { count(); event.preventDefault(); }
  }, true);
  // L'envoi est arrete en dernier (a la fenetre, apres les gestionnaires de
  // la page) : on sait ainsi si la page l'a pris en charge ou l'aurait laisse
  // recharger la page.
  let lastSubmit = null;
  window.addEventListener("submit", (event) => {
    count();
    lastSubmit = { form: event.target, handled: event.defaultPrevented };
    event.preventDefault();
  });
  let lastClick = null;
  window.addEventListener("click", (event) => {
    lastClick = { target: event.target, handled: event.defaultPrevented };
    const link = event.target.closest && event.target.closest("a[href]");
    if (link) event.preventDefault();
  });
  if (window.navigation) {
    window.navigation.addEventListener("navigate", (event) => {
      count();
      if (event.cancelable) event.preventDefault();
    });
  }
  // Mode « formulaires seuls » (`clawOnlyForms`) : Screenshot renvoie les
  // formulaires de la page avant d'ouvrir un écran à capturer, pour qu'il ait
  // de quoi montrer ; ni liens ni boutons.
  const onlyForms = typeof clawOnlyForms !== "undefined" && clawOnlyForms;
  let scrolled = 0;
  window.addEventListener("scroll", () => { scrolled += 1; }, true);
  const errors = [];
  window.addEventListener("error", (event) => { errors.push(event.message || "error"); });
  window.addEventListener("unhandledrejection", (event) => {
    const reason = event.reason;
    errors.push(String((reason && reason.message) || reason));
  });
  // Ce qu'une action change dans la page, compte par l'observateur.
  // Changements de la page pendant un essai. Un attribut récrit avec la même
  // valeur n'en est pas un (un gestionnaire de toute la page qui « referme »
  // à chaque toucher un menu déjà fermé) : le bouton touché n'a rien fait.
  const watch = () => {
    const seen = { mutations: 0 };
    const changed = (record) => {
      if (record.type === "childList") return record.addedNodes.length + record.removedNodes.length > 0;
      if (record.type === "attributes") return record.target.getAttribute(record.attributeName) !== record.oldValue;
      return record.target.data !== record.oldValue;
    };
    const observer = new MutationObserver((records) => { seen.mutations += records.filter(changed).length; });
    observer.observe(document.documentElement, {
      subtree: true, childList: true, attributes: true, characterData: true, attributeOldValue: true, characterDataOldValue: true,
    });
    seen.stop = () => observer.disconnect();
    return seen;
  };
  // Le doigt touche le centre de la commande : ce qui s'y trouve doit etre elle.
  const onTop = (control) => {
    const rect = control.getBoundingClientRect();
    const x = rect.left + rect.width / 2;
    const y = rect.top + rect.height / 2;
    if (x < 0 || y < 0 || x > innerWidth || y > innerHeight) return null;
    const top = document.elementFromPoint(x, y);
    if (!top || top === control || control.contains(top) || (top.control && top.control === control)) return null;
    // Un libelle qui contient la commande, ou une infobulle a pointer-events.
    if (top.contains(control) && getComputedStyle(top).pointerEvents !== "none") return null;
    return top;
  };

  // Toucher une commande comme un doigt : au centre de ce qui est dessine la, le
  // plus souvent l'icone ou le texte qu'elle contient. `el.click()` vise la
  // commande elle-meme et passe a cote des gestionnaires qui lisent
  // `event.target`. Vu le 25 octobre 2026 (session #31) : la croix d'une
  // fenetre ne la fermait que si `event.target` etait le bouton ; le doigt
  // touchait son icone, la fenetre restait ouverte, et le parcours disait
  // que tout marchait.
  const tap = (el) => {
    let rect = el.getBoundingClientRect();
    if (rect.top < 0 || rect.bottom > innerHeight || rect.left < 0 || rect.right > innerWidth) {
      el.scrollIntoView({ block: "center", inline: "nearest", behavior: "instant" });
      rect = el.getBoundingClientRect();
    }
    const x = rect.left + rect.width / 2;
    const y = rect.top + rect.height / 2;
    const point = x >= 0 && y >= 0 && x <= innerWidth && y <= innerHeight ? document.elementFromPoint(x, y) : null;
    if (!point || (point !== el && !el.contains(point))) {
      el.click();
      return;
    }
    const at = { bubbles: true, cancelable: true, composed: true, view: window, clientX: x, clientY: y, button: 0 };
    point.dispatchEvent(new PointerEvent("pointerdown", { ...at, pointerType: "touch", isPrimary: true }));
    point.dispatchEvent(new MouseEvent("mousedown", at));
    point.dispatchEvent(new PointerEvent("pointerup", { ...at, pointerType: "touch", isPrimary: true }));
    point.dispatchEvent(new MouseEvent("mouseup", at));
    point.dispatchEvent(new MouseEvent("click", { ...at, detail: 1 }));
  };

  // Textes durables de la page (screenshot_snapshot.js, donne par Claw avant ce
  // script) : ceux qu'un formulaire fait apparaitre doivent revenir au rechargement.
  const snapshot = () => (typeof clawSnapshot === "function" ? clawSnapshot() : []);

  // --- Liens -----------------------------------------------------------------
  const links = [];
  const anchors = new Map();
  const pages = new Map();
  const here = location.href.split("#")[0];
  for (const link of onlyForms ? [] : [...document.querySelectorAll("a[href]")].slice(0, 120)) {
    const href = link.getAttribute("href").trim();
    // `#/outils`, `#!/outils` : une route de l'application, pas une section.
    if (/^#[/!]/.test(href)) continue;
    if (href.startsWith("#") && href.length > 1) {
      let id = href.slice(1);
      try { id = decodeURIComponent(id); } catch (_) { /* laisse tel quel */ }
      if (!document.getElementById(id) && !document.getElementsByName(id).length) {
        links.push(`${label(link)} points to #${id}, but the page has no element with that id`);
      } else if (document.getElementById(id) && !anchors.has(id)) anchors.set(id, link);
    } else if (/^tel:/i.test(href)) {
      const number = href.slice(4).replace(/[\s.()-]/g, "");
      if (!/^\+?\d{6,15}$/.test(number) || /^\+?(\d)\1+$/.test(number)) {
        links.push(`${label(link)} calls "${href.slice(4)}", which is not a phone number`);
      }
    } else if (/^mailto:/i.test(href)) {
      const address = decodeURIComponent(href.slice(7).split("?")[0]);
      if (address && !/^[^@\s]+@[^@\s]+\.[a-z]{2,}$/i.test(address)) {
        links.push(`${label(link)} writes to "${address}", which is not an e-mail address`);
      }
    } else if (/^(javascript:|$)/i.test(href) || href === "#") {
      if (!shown(link)) continue;
      // Un lien sans destination : il doit au moins faire quelque chose.
      const seen = watch();
      const before = { effects, errors: errors.length };
      lastClick = null;
      link.dispatchEvent(new MouseEvent("click", { bubbles: true, cancelable: true }));
      await wait(200);
      seen.stop();
      if (errors.length > before.errors) continue;
      const handled = lastClick && lastClick.handled;
      if (!handled && !seen.mutations && effects === before.effects) {
        links.push(`${label(link)} has href="${href}" and does nothing`);
      }
      document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    } else {
      let url;
      try { url = new URL(href, location.href); } catch (_) {
        links.push(`${label(link)} has an address that cannot be read: ${href}`);
        continue;
      }
      if (url.origin !== location.origin && url.protocol !== "file:") continue;
      const page = `${url.origin === "null" ? "file://" : url.origin}${url.pathname}`;
      if (page === here.split("?")[0] || pages.has(page)) continue;
      // `/tarifs.html` vise la racine du site, que Claw situe dans le projet.
      pages.set(page, { from: label(link), root: /^\/(?!\/)/.test(href) ? href.split(/[?#]/)[0] : null });
    }
  }
  // Une section ouverte par son lien arrive sous la barre fixe du haut : son
  // titre est cache. Vu le 25 octobre 2026 (session #31) : le menu menait a
  // « Covoiturage », dont le titre passait 17px sous l'en-tete ; la page
  // prevoyait 64px pour un en-tete de 77px. scrollIntoView suit
  // scroll-padding-top et scroll-margin-top, comme le navigateur sur un lien.
  const bar = () => [...document.querySelectorAll("body *")].find((el) => {
    const style = getComputedStyle(el);
    if (!["fixed", "sticky"].includes(style.position) || !shown(el)) return false;
    const rect = el.getBoundingClientRect();
    return rect.top <= 1 && rect.width >= innerWidth * 0.6 && rect.height < innerHeight / 2;
  });
  const hidden = [];
  for (const [id, link] of [...anchors].slice(0, 6)) {
    const target = document.getElementById(id);
    if (!shown(target)) continue;
    target.scrollIntoView({ block: "start", behavior: "instant" });
    await wait(60);
    const top = bar();
    if (!top || top.contains(target)) continue;
    const heading = target.matches("h1, h2, h3, h4") ? target : target.querySelector("h1, h2, h3, h4") || target;
    const under = top.getBoundingClientRect().bottom - heading.getBoundingClientRect().top;
    // Une section en haut de page ne peut pas descendre plus bas : rien a dire.
    if (under > 2 && scrollY > 2) hidden.push(`${label(link)} -> #${id}: ${label(heading)} ends up ${Math.round(under)}px under ${label(top)}`);
  }
  scrollTo({ top: 0, left: 0, behavior: "instant" });
  const underBar = hidden.length
    ? [`Links to sections land with the section's title hidden under the fixed bar at the top: ${hidden.slice(0, 3).join("; ")}. Set scroll-margin-top on the sections (or scroll-padding-top on html) to the bar's real height`]
    : [];

  // Les pages du site : un serveur les sert ; une page ouverte en fichier
  // laisse Claw verifier le disque.
  const files = [];
  for (const [page, { from, root }] of [...pages].slice(0, 15)) {
    const served = typeof clawServed !== "undefined" && clawServed && page.startsWith(location.origin);
    if (page.startsWith("file:") || served) { files.push({ url: page, from, root, page: here }); continue; }
    try {
      const stop = new AbortController();
      const timer = setTimeout(() => stop.abort(), 3000);
      const answer = await fetch(page, { cache: "no-store", signal: stop.signal });
      clearTimeout(timer);
      if (answer.status >= 400) links.push(`${from} leads to ${page}, which answers ${answer.status}`);
    } catch (_) { /* serveur lent ou absent : la capture l'aurait dit */ }
  }

  // --- Formulaires -----------------------------------------------------------
  const forms = [];
  const kept = [];
  // Noms d'essai tapés dans les formulaires envoyés : Screenshot les cherche
  // ensuite dans les écrans, et sur un deuxième appareil.
  const typed = [];
  // Envois qui changent un compteur sans montrer le nom donne.
  const unnamed = [];
  const tested = new Set();
  // Un nom par formulaire : au rechargement, on sait lequel a perdu son ajout.
  const base = `Camille Claw${Math.floor(1000 + Math.random() * 9000)}`;
  let marker = base;
  const day = (offset) => {
    const date = new Date(Date.now() + offset * 86400000);
    return date.toISOString().slice(0, 10);
  };
  const setValue = (field, value) => {
    const proto = field instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype
      : field instanceof HTMLSelectElement ? HTMLSelectElement.prototype : HTMLInputElement.prototype;
    const setter = Object.getOwnPropertyDescriptor(proto, "value").set;
    setter.call(field, value);
    field.dispatchEvent(new Event("input", { bubbles: true }));
    field.dispatchEvent(new Event("change", { bubbles: true }));
  };
  const nameOf = (field) => {
    const own = field.labels && field.labels[0] ? field.labels[0].innerText : "";
    return `${field.name || ""} ${field.id || ""} ${field.placeholder || ""} ${own}`.toLowerCase();
  };
  const plausible = (field, named) => {
    const type = (field.getAttribute("type") || "text").toLowerCase();
    const words = nameOf(field);
    if (type === "email" || /mail/.test(words)) return "camille.test@example.org";
    if (type === "tel" || /t[eé]l[eé]?phone|phone|portable|mobile/.test(words)) return "0612345678";
    if (type === "url") return "https://example.org";
    if (type === "password") return "Essai-Claw-2026!";
    if (type === "date") return field.min && field.min > day(7) ? field.min : day(7);
    if (type === "time") return field.min || "18:30";
    if (type === "datetime-local") return `${day(7)}T18:30`;
    if (type === "month") return day(31).slice(0, 7);
    if (type === "week") return "";
    if (type === "color") return "#336699";
    if (type === "number" || type === "range") {
      // Un nombre par formulaire : deux formulaires qui ajoutent autant de places
      // donneraient le meme total, et l'un passerait pour l'autre.
      const min = field.min !== "" ? Number(field.min) : 1;
      const max = field.max !== "" ? Number(field.max) : min + 10;
      return String(Math.max(min, Math.min(max, Math.max(1, min) + tested.size - 1)));
    }
    if (field instanceof HTMLTextAreaElement) return "Message d'essai, a ignorer.";
    // Un montant ou une quantite dans un champ texte (clavier numerique du
    // telephone) : un nombre, pas un nom. Vu le 27 octobre 2026 (kit d'appli) :
    // « Camille Claw8858B » tape dans « Montant », lu 8 858 €, et l'operation
    // passait pour perdue.
    const mode = (field.getAttribute("inputmode") || "").toLowerCase();
    if (mode === "decimal" || mode === "numeric" || /montant|amount|prix|price|somme|quantit|qty/.test(words)) {
      return mode === "numeric" ? String(10 + tested.size) : `${12 + tested.size},50`;
    }
    if (!named || /nom|name|pr[eé]nom|pseudo|qui|conducteur|joueur|parent/.test(words)) return marker;
    if (/ville|city|lieu|adresse|address|d[eé]part/.test(words)) return "Toulouse";
    if (/code.?postal|zip/.test(words)) return "31000";
    return "Essai";
  };
  const fill = (form) => {
    let named = false;
    const refused = [];
    const groups = new Set();
    for (const field of form.querySelectorAll("input, select, textarea")) {
      if (field.disabled || field.readOnly || field.type === "hidden" || field.type === "submit"
        || field.type === "button" || field.type === "reset" || field.type === "file" || field.type === "image") continue;
      if (field.type === "checkbox") {
        if (field.required && !field.checked) field.click();
        continue;
      }
      if (field.type === "radio") {
        if (groups.has(field.name)) continue;
        groups.add(field.name);
        const group = field.name ? [...form.querySelectorAll(`input[type=radio][name="${CSS.escape(field.name)}"]`)] : [field];
        if (!group.some((radio) => radio.checked)) group[0].click();
        continue;
      }
      if (field instanceof HTMLSelectElement) {
        const option = [...field.options].find((item) => !item.disabled && item.value !== "");
        if (option && !field.value) setValue(field, option.value);
      } else {
        const value = plausible(field, named);
        if (value === marker) named = true;
        // Le nom d'un formulaire precedent, garde par la page, est remplace.
        // Un nombre deja propose (« 3 places ») aussi : chaque formulaire a le sien.
        const counted = field.type === "number" || field.type === "range";
        if (value && (!field.value || value === marker || counted)) setValue(field, value);
      }
      // Un motif (pattern) que le navigateur ne lit pas lève une erreur ici.
      let valid = true;
      try { valid = field.checkValidity(); } catch (error) {
        refused.push(`${label(field)} (its pattern is not a valid regular expression, so it checks nothing: ${String((error && error.message) || error).slice(0, 120)})`);
        continue;
      }
      if (!valid) refused.push(`${label(field.labels && field.labels[0] ? field.labels[0] : field)} (${field.validationMessage})`);
    }
    // Liste faite main (role=combobox, sa liste role=listbox) : ouverte, puis
    // sa première option choisie, comme le doigt du visiteur. Vu le 26 octobre
    // 2026 (session #32) : l'heure de retrait restait vide, et le bouton
    // « Réserver » grisé.
    for (const box of form.querySelectorAll("[role=combobox], [aria-haspopup=listbox]")) {
      const list = document.getElementById(box.getAttribute("aria-controls") || "") || box.querySelector("[role=listbox]");
      const input = box.matches("input") ? box : box.querySelector("input");
      if (!list || (input && input.value) || list.querySelector("[role=option][aria-selected=true]")) continue;
      tap(box);
      const option = [...list.querySelectorAll("[role=option]")].find((el) => el.getAttribute("aria-disabled") !== "true");
      if (option) tap(option);
    }
    return refused;
  };
  const formName = (form) => {
    if (form.id) return `form#${form.id}`;
    const title = form.querySelector("h1, h2, h3, legend") || form.closest("dialog, [role=dialog], section")?.querySelector("h1, h2, h3");
    const send = form.querySelector("button[type=submit], button:not([type]), input[type=submit]");
    const text = (title && title.innerText) || (send && (send.innerText || send.value)) || "";
    return text.trim() ? `the form "${text.replace(/\s+/g, " ").trim().slice(0, 40)}"` : "a form";
  };
  const tryForm = async (form) => {
    tested.add(form);
    if (form.getAttribute("method") === "dialog" || !form.querySelector("input, select, textarea")) return;
    const action = (form.getAttribute("action") || "").trim();
    const name = formName(form);
    marker = `${base}${String.fromCharCode(65 + tested.size)}`;
    const refused = fill(form);
    if (refused.length) {
      forms.push(`${name} refuses plausible entries: ${refused.slice(0, 3).join("; ")}`);
      return;
    }
    const send = form.querySelector("button[type=submit], button:not([type]), input[type=submit]");
    // Bouton d'envoi grisé une fois les champs remplis : la page attend autre
    // chose (une quantité au « + »). Le « + » du formulaire est essayé une fois ;
    // s'il reste grisé, le visiteur ne peut pas envoyer, et c'est cela qui est
    // dit (pas « aucun événement submit »). Vu le 26 octobre 2026 (session #32) :
    // « l'envoi ne fait rien » répété une heure, l'agent relisait son submit.
    const greyed = (el) => el && (el.disabled || el.getAttribute("aria-disabled") === "true");
    if (greyed(send)) {
      const adder = [...form.querySelectorAll("button, [role=button]")].find((el) => el !== send && shown(el) && !greyed(el)
        && /^\s*\+\s*$|ajout|augment|plus\b|increase|\badd\b|more\b/i.test(`${el.innerText || ""} ${el.getAttribute("aria-label") || ""} ${el.title || ""}`));
      if (adder) {
        tap(adder);
        await wait(250);
      }
      if (greyed(send)) {
        forms.push(`${name}: its ${label(send)} stays disabled once every field is filled${adder ? ` and ${label(adder)} is pressed` : ""}, so a visitor cannot send it. Enable it when the form can be sent, and say on the page what is still missing`);
        return;
      }
    }
    // Un formulaire ou le visiteur donne son nom : une inscription, une offre,
    // une reservation, pas une recherche.
    const personal = [...form.elements].some((field) => field.value === marker);
    if (personal) typed.push(marker);
    const prior = new Set(snapshot());
    const seen = watch();
    const before = { effects, errors: errors.length, text: document.body.innerText.length };
    lastSubmit = null;
    try {
      if (send && shown(send)) {
        const top = onTop(send);
        if (top) forms.push(`${name}: its ${label(send)} cannot be tapped, ${label(top)} lies over it`);
        tap(send);
      } else form.requestSubmit();
    } catch (error) { errors.push(String((error && error.message) || error)); }
    await wait(600);
    seen.stop();
    if (errors.length > before.errors) {
      forms.push(`sending ${name} raises an error in the page's script: ${errors[errors.length - 1]}`);
      return;
    }
    if (!lastSubmit) {
      if (!seen.mutations && effects === before.effects) {
        forms.push(`sending ${name} does nothing: no submit event, no change in the page`);
      }
      return;
    }
    if (!lastSubmit.handled && (!action || action === "#")) {
      forms.push(`sending ${name} reloads the page (no script takes the submit event, no action): what was typed is lost and nothing is shown`);
      return;
    }
    if (lastSubmit.handled && seen.mutations === 0 && effects === before.effects + 1) {
      forms.push(`sending ${name} shows nothing: no confirmation, no error, no change in the page`);
      return;
    }
    // Ajoute a la page, hors du message passager (toast) qui le remercie.
    const passing = "[role=status], [role=alert], [aria-live], .toast, .snackbar, .notification, output";
    // Ou ajouté à un écran caché que la page ouvre d'un lien, d'un onglet ou
    // d'un bouton (une appli d'une seule page : la liste des opérations derrière
    // son onglet). Vu le 27 octobre 2026 (kit d'appli) : l'opération ajoutée
    // depuis l'accueil, listée dans l'écran « Opérations », passait pour perdue.
    const openable = (el) => {
      for (let node = el; node && node !== document.body; node = node.parentElement) {
        if (!node.id || shown(node)) continue;
        const id = CSS.escape(node.id);
        if (document.querySelector(`a[href="#${id}"], [aria-controls="${id}"], [data-screen="${id}"], [data-view="${id}"], [data-tab="${id}"], [data-target="#${id}"]`)) return true;
      }
      return false;
    };
    const texts = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT);
    let added = false;
    for (let node = texts.nextNode(); node && !added; node = texts.nextNode()) {
      const holder = node.parentElement;
      added = node.nodeValue.includes(marker) && holder && !holder.closest(passing) && (shown(holder) || openable(holder));
    }
    // Ou la page a change (un compteur, une liste) sans montrer le nom : les
    // textes apparus a l'envoi.
    const appeared = personal ? snapshot().filter((text) => !prior.has(text)).slice(0, 8) : [];
    // Un compteur a bouge (« Il reste 2 places ») mais le nom donne n'est
    // nulle part : on ne sait pas qui s'est inscrit. Vu le 25 octobre 2026
    // (session #31) : « Je conduis » enlevait des places, aucun parent ne
    // voyait qui conduisait.
    // Un texte du formulaire lui-même (sa quantité remise à 0, son total) n'est
    // pas une liste que d'autres lisent. Vu le 26 octobre 2026 (session #32) :
    // « 1,30 € … 0 », la carte du produit dans le formulaire de réservation,
    // prise pour un compteur ; l'agent a ajouté une section « Mes
    // réservations » que personne n'avait demandée pour faire taire l'alerte.
    // Une liste des choses du visiteur lui-même (« Mes réservations », « Votre
    // commande ») n'a pas à redire son nom : elle est à lui.
    const own = /^(mes|mon|ma|vos|votre|my|your)\b/i;
    // Le bloc qui porte ce texte, même écrit en plusieurs morceaux (« mardi 27
    // octobre » et « 07:00 » dans deux <span>) : le plus profond qui le contient.
    const flat = (value) => (value || "").replace(/\s+/g, "");
    const outside = (text) => {
      const base = flat(text.replace(/ \(in ".*$/, "")).slice(0, 60);
      if (!base) return true;
      const holder = [...document.body.querySelectorAll("*")]
        .filter((el) => !el.closest("script, style, template, noscript") && el.getClientRects().length && flat(el.innerText).includes(base))
        .pop();
      if (!holder) return true;
      if (form.contains(holder)) return false;
      const region = holder.closest("section, aside, article, [role=region]");
      const title = region && region.querySelector("h1, h2, h3, h4, [role=heading]");
      return !(title && own.test((title.textContent || "").trim()));
    };
    const counted = appeared.find((text) => /\d/.test(text) && outside(text));
    if (personal && !added && counted) {
      unnamed.push(`Sending ${name} changes the page ("${counted.slice(0, 90)}") but the name typed in it ("${marker}") is shown nowhere, only in a passing message at best: people cannot see who signed up or who offered. Show each new entry with its name where the people who need it read it (the card or list it changed; for a reservation or an order, the list its owner reads, which need not be public), and keep it after a reload`);
    }
    if (added || appeared.length) kept.push({ form: name, marker, shown: added, appeared });
  };

  for (const form of [...document.forms].filter(shown).slice(0, 6)) await tryForm(form);
  if (onlyForms) return { typed, forms };

  // --- Boutons ---------------------------------------------------------------
  const controls = [...document.querySelectorAll("button, [role=button], input[type=button], summary")]
    .filter((el) => shown(el) && !el.disabled && el.getAttribute("aria-disabled") !== "true")
    // L'envoi d'un formulaire s'essaie avec le formulaire.
    .filter((el) => !(el.type === "submit" && el.form))
    // L'onglet ou l'etape deja choisis ne changent rien quand on les clique.
    .filter((el) => el.getAttribute("aria-selected") !== "true"
      && !(el.hasAttribute("aria-current") && el.getAttribute("aria-current") !== "false"))
    .slice(0, 24);
  const dead = [];
  const failing = [];
  // Boutons qu'un doigt ne touche pas : autre chose est pose dessus.
  const buried = [];
  // Fenêtres ouvertes par un bouton dont les commandes ne se touchent pas : un
  // fond (backdrop) posé par-dessus prend le doigt. Vu le 25 octobre 2026
  // (session #31) : « Je conduis » ouvrait un formulaire sous un
  // <form class="modal-backdrop"> ; toucher un champ fermait la fenêtre.
  // `el.click()` passe outre : seul le point touché le dit.
  const covered = [];
  // Fenêtres que leur bouton de fermeture ne ferme pas.
  const stuck = [];
  // Fenêtres qu'un glissement fini sur leur fond referme.
  const dragged = [];
  // Boutons qui ouvrent une fenêtre, une fois par fenêtre : Screenshot rouvre
  // chacune pour la capturer (sans capture, l'utilisateur ne la voyait jamais).
  const openers = [];
  const openedWindows = new Set();
  // Défauts de mise en page des fenêtres ouvertes, mesurés à l'ordinateur.
  const windowLayout = [];
  const path = (el) => {
    const parts = [];
    for (let node = el; node && node.nodeType === 1 && node !== document.documentElement; node = node.parentElement) {
      if (node.id && /^[A-Za-z][\w-]*$/.test(node.id)) { parts.unshift(`#${node.id}`); break; }
      const same = [...(node.parentElement?.children || [])].filter((item) => item.tagName === node.tagName);
      parts.unshift(`${node.tagName.toLowerCase()}${same.length > 1 ? `:nth-of-type(${same.indexOf(node) + 1})` : ""}`);
    }
    return parts.join(" > ");
  };
  // Une fenêtre ouverte : elle, ou ce qu'elle contient, se voit. Vu le 26 octobre
  // 2026 (session #31) : un <dialog> de 0px dont le panneau est posé en position
  // fixe ; le parcours la croyait fermée, et rien n'en était vérifié.
  const open = (box) => {
    const style = getComputedStyle(box);
    if (style.display === "none" || style.visibility === "hidden") return false;
    return shown(box) || [...box.querySelectorAll("*")].slice(0, 300).some(shown);
  };
  const windows = () => [...document.querySelectorAll("dialog[open], [role=dialog], [aria-modal=true]")].filter(open);
  const coveredIn = (opener) => {
    for (const box of windows()) {
      const blocked = [...box.querySelectorAll("button, input:not([type=hidden]), select, textarea, a[href]")]
        .filter(shown)
        .slice(0, 12)
        .map((control) => {
          const top = onTop(control);
          return top ? { control, top } : null;
        })
        .filter(Boolean);
      if (blocked.length) {
        covered.push(`${label(opener)} opens a window whose ${blocked.slice(0, 3).map((item) => label(item.control)).join(", ")} cannot be tapped: ${label(blocked[0].top)} lies over them`);
        return true;
      }
    }
    return false;
  };
  // Sur la page intacte, avant tout clic : un tiroir resté ouvert couvrirait
  // les boutons suivants.
  for (const el of controls) {
    el.scrollIntoView({ block: "center", inline: "nearest" });
    const top = onTop(el);
    if (top && buried.length < 6) buried.push(`${label(el)} (under ${label(top)})`);
  }
  scrollTo(0, 0);
  // Champ d'un code d'accès (code PIN, mot de passe) dans une fenêtre.
  const codeField = (box) => [...box.querySelectorAll("input")].filter(shown).find((field) => field.type === "password"
    || /pin|code|passe|password/i.test(`${field.name} ${field.id} ${field.labels && field.labels[0] ? field.labels[0].innerText : ""}`));
  // Boutons dont la fenêtre demande un code : rouverts à la fin du parcours.
  const gated = [];
  const closeButton = (box) => [...box.querySelectorAll("button, [role=button], a[href]")].filter(shown)
    .find((el) => /^(fermer|close|annuler|cancel|retour|x|\u00d7|\u2715|\u2716)$/i.test((el.innerText || "").trim())
      || /fermer|close|annuler|cancel/i.test(el.getAttribute("aria-label") || el.getAttribute("title") || ""));
  // Grandes zones à l'écran, hors fenêtres et barres : un bouton qui en fait
  // paraître une nouvelle change d'écran (« Espace boulanger »). Cet écran est
  // exploré à son tour, et Screenshot le capture. Vu le 26 octobre 2026
  // (session #32) : la liste de la boulangère, un écran entier que ni le juge
  // ni les mesures n'avaient jamais vu.
  const regions = () => new Set([...document.querySelectorAll("main, section, article, form, [role=main], [role=region], [role=tabpanel], body > div, main > div")]
    .filter((el) => shown(el) && !el.closest("dialog, [role=dialog], [aria-modal=true], header, footer, nav")
      && el.getBoundingClientRect().height >= innerHeight * 0.25));
  const screens = [];
  // Écran gardé par un code, déjà visible derrière la fenêtre du code avant
  // toute saisie. Vu le 27 octobre 2026 (session #32, passage 8) :
  // « Préparation du jour » affiché sous la fenêtre, le code pas encore tapé.
  const gateOpen = [];
  const openedEarly = (el, box, before) => {
    const fresh = [...regions()].filter((item) => !before.has(item) && !item.contains(el) && !box.contains(item) && !item.contains(box));
    // Le plus petit : celui qui vient d'apparaître, pas la page qu'il agrandit.
    const early = fresh.find((item) => !fresh.some((other) => other !== item && item.contains(other)));
    if (!early || gateOpen.some((line) => line.startsWith(label(el)))) return;
    const heading = early.querySelector("h1, h2, h3");
    gateOpen.push(`${label(el)} shows "${(heading ? heading.innerText : label(early)).replace(/\s+/g, " ").trim().slice(0, 50)}" behind its code window before any code is typed`);
  };
  const tried = new Set();
  const deadEls = [];
  const noteScreen = (el, before, code) => {
    if (el.matches("summary, [aria-expanded], [aria-controls]") || el.closest("details")) return null;
    const fresh = [...regions()].filter((item) => !before.has(item) && !item.contains(el));
    const region = fresh.find((item) => !fresh.some((other) => other !== item && other.contains(item)));
    if (!region) return null;
    const heading = region.querySelector("h1, h2, h3");
    const name = (heading ? heading.innerText : label(region)).replace(/\s+/g, " ").trim().slice(0, 50);
    if (screens.length < 3 && !screens.some((screen) => screen.name === name)) {
      screens.push({ label: label(el), selector: path(el), name, code: code || null });
    }
    return region;
  };
  const tryControl = async (el) => {
    if (!el.isConnected || !shown(el) || tried.has(el)) return null;
    tried.add(el);
    const shownBefore = regions();
    // A l'ecran avant la mesure : le defilement du toucher ne compte pas comme
    // un effet du bouton.
    el.scrollIntoView({ block: "center", inline: "nearest", behavior: "instant" });
    await wait(40);
    const opened = new Set(windows());
    const seen = watch();
    const before = { url: location.href, x: scrollX, y: scrollY, effects, scrolled, errors: errors.length, focus: document.activeElement };
    try { tap(el); } catch (error) { errors.push(String((error && error.message) || error)); }
    await wait(300);
    seen.stop();
    if (errors.length > before.errors) failing.push(`${label(el)}: ${errors[errors.length - 1]}`);
    const moved = location.href !== before.url || Math.abs(scrollX - before.x) > 2 || Math.abs(scrollY - before.y) > 2
      || scrolled > before.scrolled;
    const focus = document.activeElement;
    const focused = focus !== before.focus && focus !== el && focus !== document.body;
    if (!seen.mutations && !moved && !focused && effects === before.effects && errors.length === before.errors) {
      dead.push(label(el));
      deadEls.push([el, label(el)]);
    }
    const blocked = covered.length < 4 && coveredIn(el);
    // La fenetre ouverte : ses formulaires s'essaient, puis son bouton la ferme.
    for (const box of windows().filter((item) => !opened.has(item))) {
      if (!openedWindows.has(box) && openers.length < 4) {
        openedWindows.add(box);
        openers.push({ label: label(el), selector: path(el) });
        // Sur ordinateur, la fenêtre se mesure ici : Screenshot ne capture les
        // fenêtres qu'au téléphone. Vu le 26 octobre 2026 (session #31) : la
        // feuille réparée au téléphone restait collée à gauche et en haut à
        // l'ordinateur, sans que rien ne le dise.
        if (innerWidth >= 900 && typeof clawWindow === "function") {
          try {
            for (const problem of clawWindow()) windowLayout.push(`In the window opened by ${label(el)}, on a ${innerWidth}px-wide screen: ${problem}`);
          } catch (_) { /* la mesure de la fenêtre ne bloque pas le parcours */ }
        }
      }
      if (blocked) break;
      if (codeField(box)) openedEarly(el, box, shownBefore);
      if (codeField(box) && gated.length < 2 && !gated.some((item) => item.el === el)) gated.push({ el });
      for (const form of [...box.querySelectorAll("form")].filter((item) => shown(item) && !tested.has(item))) {
        if (tested.size < 10) await tryForm(form);
      }
      if (!box.isConnected || !open(box)) continue;
      // Un glissement parti de la fenêtre et fini sur son fond : le navigateur
      // donne le clic à leur ancêtre commun, la fenêtre (<dialog>) ou le fond.
      // Une fenêtre fermée dès que `event.target` est le fond se ferme alors
      // sous le doigt qui fait défiler son formulaire (vu le 25 octobre 2026 :
      // « quand on force le glisser vers le bas, ça ferme la fenêtre »).
      if (dragged.length < 3) {
        const inner = [...box.querySelectorAll("h1, h2, h3, label, p, input, button")].find(shown);
        const outside = document.elementFromPoint(innerWidth / 2, 6);
        if (inner && outside && (outside === box || !box.contains(outside))) {
          let common = inner;
          while (common && !common.contains(outside)) common = common.parentElement;
          const fire = (target, type, Kind) => target.dispatchEvent(new Kind(type, { bubbles: true, cancelable: true, view: window, clientX: innerWidth / 2, clientY: 6 }));
          fire(inner, "pointerdown", PointerEvent);
          fire(inner, "mousedown", MouseEvent);
          fire(outside, "pointerup", PointerEvent);
          fire(outside, "mouseup", MouseEvent);
          if (common) fire(common, "click", MouseEvent);
          await wait(300);
          if (!box.isConnected || !open(box)) {
            dragged.push(`${label(el)} opens a window that closes when a finger slides from inside it onto its edge (scrolling its form is enough)`);
            continue;
          }
        }
      }
      const close = closeButton(box);
      if (close) {
        tap(close);
        await wait(300);
        if (box.isConnected && open(box) && stuck.length < 3) stuck.push(`${label(el)} opens a window that ${label(close)} does not close`);
      }
    }
    // Ce qui reste ouvert se referme, pour que le bouton suivant soit libre.
    // Un menu déplié par ce bouton se replie par lui : sinon le toucher du
    // bouton suivant le refermait (un gestionnaire de toute la page), et ce
    // changement passait pour l'effet de ce bouton suivant. Vu le 26 octobre
    // 2026 (session #31) : « Détails du match » n'ouvrait plus rien, et le
    // parcours, qui venait d'ouvrir le menu, le croyait vivant.
    if (el.isConnected && shown(el) && el.getAttribute("aria-expanded") === "true") {
      try { tap(el); } catch (_) { /* le parcours continue */ }
      await wait(300);
    }
    document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    for (const dialog of document.querySelectorAll("dialog[open]")) {
      try { dialog.close(); } catch (_) { /* deja fermee */ }
    }
    await wait(40);
    return noteScreen(el, shownBefore);
  };
  for (const el of controls) await tryControl(el);
  // Les boutons d'un écran caché au départ s'essaient aussi, et ses
  // formulaires s'envoient : l'écran est rouvert par son bouton chaque fois
  // qu'un essai l'a quitté.
  const reveal = async (el) => {
    if (shown(el)) return true;
    const toggle = [...document.querySelectorAll("[aria-expanded=false]")].filter(shown)
      .find((item) => { const menu = document.getElementById(item.getAttribute("aria-controls") || ""); return menu && menu.contains(el); });
    if (toggle) { tap(toggle); await wait(300); }
    return shown(el);
  };
  const exploreScreen = async (region, reopen) => {
    const inner = () => [...region.querySelectorAll("button, [role=button], input[type=button], summary")]
      .filter((el) => shown(el) && !tried.has(el) && !el.disabled && el.getAttribute("aria-disabled") !== "true"
        && !(el.type === "submit" && el.form));
    for (let round = 0; round < 10; round += 1) {
      if (!region.isConnected) return;
      if (!shown(region) && !(await reopen())) return;
      const next = inner()[0];
      if (!next) break;
      await tryControl(next);
    }
    if (!shown(region) && !(await reopen())) return;
    for (const form of [...region.querySelectorAll("form")].filter((item) => shown(item) && !tested.has(item)).slice(0, 2)) {
      if (tested.size < 10) await tryForm(form);
    }
  };
  // Un espace gardé par un code (« Espace boulanger ») : rouvert après l'envoi
  // des formulaires, pour qu'il ait de quoi montrer, et ouvert avec le code
  // écrit dans le script de la page (`clawCodes`, lu par Screenshot). Vu le
  // 26 octobre 2026 (session #32, passages 4 et 5) : la liste de la
  // boulangère levait une erreur dès qu'une réservation existait, derrière
  // le code ; le parcours s'arrêtait à la fenêtre du code, et l'agent
  // annonçait « vérifié ».
  const codes = typeof clawCodes === "undefined" ? [] : clawCodes;
  // Le code écrit à l'écran (texte ou exemple du champ) : tout visiteur le lit
  // et entre. Vu le 27 octobre 2026 (session #32, passage 7) : « Code par
  // défaut : 1234 » dans la fenêtre même de l'espace boulanger.
  const codeShown = [];
  const writtenCode = (code, field) => {
    if (field && field.placeholder && field.placeholder.trim() === code) return `the example of its field: "${code}"`;
    const quoted = code.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
    const pattern = new RegExp(`(^|[^\\w])${quoted}([^\\w]|$)`);
    const walker = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT);
    for (let node = walker.nextNode(); node; node = walker.nextNode()) {
      const parent = node.parentElement;
      if (!parent || parent.closest("script, style, template, noscript") || !shown(parent) || !pattern.test(node.textContent)) continue;
      const block = parent.closest("p, li, label, dt, dd, small, div") || parent;
      return `«${(block.innerText || node.textContent).replace(/\s+/g, " ").trim().slice(0, 80)}»`;
    }
    return null;
  };
  // Les boutons essayés ont pu changer ce que l'espace montre (le jour choisi
  // dans le formulaire) : les formulaires encore à l'écran sont renvoyés juste
  // avant, sans que ce second envoi compte dans le rapport.
  if (codes.length && gated.length) {
    const reported = { forms: forms.length, unnamed: unnamed.length, kept: kept.length, errors: errors.length };
    for (const form of [...document.forms].filter((item) => shown(item) && tested.has(item)).slice(0, 3)) {
      tested.delete(form);
      await tryForm(form);
    }
    forms.length = reported.forms;
    unnamed.length = reported.unnamed;
    kept.length = reported.kept;
    errors.length = reported.errors;
  }
  for (const { el } of codes.length ? gated : []) {
    if (!el.isConnected) continue;
    if (!shown(el)) {
      // Rangé dans un menu replié : le menu s'ouvre d'abord.
      const toggle = [...document.querySelectorAll("[aria-expanded=false]")].filter(shown)
        .find((item) => { const menu = document.getElementById(item.getAttribute("aria-controls") || ""); return menu && menu.contains(el); });
      if (toggle) { tap(toggle); await wait(300); }
      if (!shown(el)) continue;
    }
    for (const code of codes.slice(0, 4)) {
      const opened = new Set(windows());
      const shownBefore = regions();
      try { tap(el); } catch (error) { errors.push(String((error && error.message) || error)); }
      await wait(300);
      const box = windows().find((item) => !opened.has(item)) || windows().find((item) => codeField(item));
      const field = box && codeField(box);
      if (!field) break;
      openedEarly(el, box, shownBefore);
      const written = writtenCode(code, field);
      if (written && !codeShown.some((line) => line.includes(`"${code}"`))) {
        codeShown.push(`the access code "${code}" that opens ${label(el)} is written on the page for every visitor to read (${written})`);
      }
      const before = errors.length;
      setValue(field, code);
      const enter = [...box.querySelectorAll("button, input[type=submit], [role=button]")].filter(shown)
        .find((item) => item.value === "submit" || item.type === "submit"
          || /^(ok|entrer|valider|acc[eé]der|ouvrir|connexion|se connecter|confirmer|d[eé]verrouiller|enter|log ?in|sign in|unlock|open|submit|go)$/i.test((item.innerText || item.value || "").trim()));
      try {
        if (enter) tap(enter);
        else field.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", code: "Enter", bubbles: true, cancelable: true }));
      } catch (error) { errors.push(String((error && error.message) || error)); }
      // Un code vérifié de façon asynchrone (les données attendues d'un
      // serveur) ferme sa fenêtre plus tard : jusqu'à 3 s. Vu le 27 octobre
      // 2026 (session #32, passage 7) : l'écran du boulanger, ouvert après
      // un appel réseau, n'était plus atteint.
      for (let waited = 0; waited < 3000; waited += 150) {
        await wait(150);
        if (errors.length > before || !box.isConnected || !open(box) || !codeField(box)) break;
      }
      await wait(300);
      if (errors.length > before) {
        failing.push(`${label(el)}, then the access code "${code}" written in the page's script, typed in the window it opens, once the page's forms were sent (so there is something to show): ${errors[errors.length - 1]}`);
        // Même en erreur, l'écran s'est ouvert : Screenshot le capture tel quel.
        noteScreen(el, shownBefore, code);
        break;
      }
      const unlocked = !box.isConnected || !open(box) || !codeField(box);
      document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
      for (const dialog of document.querySelectorAll("dialog[open]")) {
        try { dialog.close(); } catch (_) { /* deja fermee */ }
      }
      await wait(40);
      if (unlocked) {
        // L'écran que le code ouvre : noté pour Screenshot, et ses boutons essayés.
        const region = noteScreen(el, shownBefore, code);
        if (region) {
          await exploreScreen(region, async () => {
            if (!(await reveal(el))) return false;
            tap(el);
            await wait(300);
            const box = windows().find((item) => codeField(item));
            if (!box) return shown(region);
            setValue(codeField(box), code);
            const go = [...box.querySelectorAll("button, input[type=submit], [role=button]")].filter(shown)
              .find((item) => item.value === "submit" || item.type === "submit" || /acc[eé]der|valider|ok|entrer|ouvrir|enter|open|submit/i.test((item.innerText || item.value || "").trim()));
            if (go) tap(go);
            for (let waited = 0; waited < 3000 && !shown(region); waited += 150) await wait(150);
            return shown(region);
          });
        }
        break;
      }
    }
  }
  // Les autres écrans, après l'espace à code : leurs essais (« Annuler ma
  // réservation ») videraient ce que l'espace devait montrer.
  for (const screen of screens.filter((item) => !item.code)) {
    const opener = document.querySelector(screen.selector);
    if (!opener) continue;
    const open = async () => {
      if (!(await reveal(opener))) return false;
      const before = regions();
      tap(opener);
      await wait(300);
      return Boolean(noteScreen(opener, before)) || [...regions()].some((item) => (item.querySelector("h1, h2, h3")?.innerText || "").replace(/\s+/g, " ").trim().slice(0, 50) === screen.name);
    };
    if (!(await open())) continue;
    const region = [...regions()].find((item) => (item.querySelector("h1, h2, h3")?.innerText || "").replace(/\s+/g, " ").trim().slice(0, 50) === screen.name);
    if (region) await exploreScreen(region, open);
  }
  // Un bouton qui ne faisait rien sur l'écran de départ (« Accueil » quand on y
  // est déjà) s'essaie encore depuis un autre écran : s'il y change la page,
  // il n'est pas mort.
  if (screens.length) {
    for (const [el, name] of deadEls) {
      if (!el.isConnected || !(await reveal(el))) continue;
      const seen = watch();
      tap(el);
      await wait(300);
      seen.stop();
      if (seen.mutations && dead.includes(name)) dead.splice(dead.indexOf(name), 1);
    }
  }
  const sitePages = [...pages].map(([url, { from }]) => ({ url, from }));
  return { checked: tried.size, windowLayout, dead, failing, covered, buried, stuck, links, files, forms, unnamed, underBar, kept, openers, dragged, screens, pages: sitePages, typed, codeShown, gateOpen };
})()
