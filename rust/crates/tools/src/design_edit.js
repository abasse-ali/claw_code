// Outil `Design` : les retouches du modèle passent par la même couche que l'éditeur visuel de
// Claw Web (claw-design.js), sur la page ouverte. `clawStart` : les retouches déjà enregistrées
// (claw-design.json), `clawEdits` : la liste demandée. Rend les retouches à enregistrer, ce qui a
// été fait (avec le nom et la clé de chaque élément) et ce qui a échoué.
(() => {
  const api = window.clawDesign;
  if (!api) return { error: "the design layer (claw-design.js) did not load in the page" };
  const design = api.normalize(clawStart && typeof clawStart === "object" ? clawStart : api.design);
  api.set(design);
  const done = [];
  const errors = [];
  const shown = (el) => { const s = getComputedStyle(el); const r = el.getBoundingClientRect(); return s.display !== "none" && s.visibility !== "hidden" && r.width > 0 && r.height > 0; };
  const text = (el) => (el.getAttribute("aria-label") || el.innerText || el.value || "").replace(/\s+/g, " ").trim();
  // L'élément visé : une référence de la vue de Browser (e12), un widget ajouté (w…), un
  // sélecteur CSS, ou son texte visible (le plus petit élément qui le porte).
  const resolve = (wanted) => {
    const target = String(wanted || "").trim().replace(/^\[(e\d+)\]$/, "$1");
    if (!target) return null;
    if (/^e\d+$/.test(target)) return document.querySelector(`[data-claw-ref="${target}"]`);
    if (/^w[a-z0-9]+$/.test(target)) {
      const widget = document.querySelector(`[data-claw-id="${target}"]`);
      if (widget) return widget;
    }
    if (/^screen:/.test(target)) return document.querySelector(`[data-claw-screen="${target.slice(7)}"]`);
    try {
      const found = [...document.querySelectorAll(target)].find(shown);
      if (found) return found;
    } catch (_) { /* pas un sélecteur */ }
    const lower = target.toLowerCase();
    const all = [...document.body.querySelectorAll("*")].filter((el) => shown(el) && !el.closest("[data-claw-own]:not([data-claw-id])") && text(el).toLowerCase().includes(lower));
    const exact = all.filter((el) => text(el).toLowerCase() === lower);
    const pool = exact.length ? exact : all;
    return pool.find((el) => !pool.some((other) => other !== el && el.contains(other))) || null;
  };
  const widgetOf = (el) => {
    const id = el.getAttribute("data-claw-id");
    return id ? design.added.find((widget) => widget.id === id) : null;
  };
  const entryOf = (el) => widgetOf(el) || (design.items[api.keyOf(el)] = design.items[api.keyOf(el)] || {});
  const name = (el) => `${el.tagName.toLowerCase()}${el.getAttribute("data-claw-id") ? ` ${el.getAttribute("data-claw-id")}` : ""} "${text(el).slice(0, 40)}"`;
  const THEME = { accent: "--accent", background: "--paper", cards: "--surface", text: "--ink", muted: "--muted" };
  clawEdits.forEach((edit, index) => {
    const at = `edit ${index + 1}`;
    try {
      if (!edit || typeof edit !== "object") throw new Error("each edit is an object");
      if (edit.theme) {
        const theme = edit.theme;
        for (const [word, token] of Object.entries(THEME)) if (theme[word]) design.tokens[token] = String(theme[word]);
        for (const [token, value] of Object.entries(theme)) if (/^--[\w-]+$/.test(token)) design.tokens[token] = String(value);
        if (theme.radius != null) {
          design.tokens["--radius"] = `${Number(theme.radius)}px`;
          design.tokens["--radius-sm"] = `${Math.round(Number(theme.radius) * 0.7)}px`;
        }
        if (theme.font) {
          if (!api.fonts.includes(theme.font)) throw new Error(`font "${theme.font}" is not offered; choose one of ${api.fonts.join(", ")}`);
          design.fonts = [...new Set([...design.fonts, theme.font])];
          design.tokens["--font-text"] = `"${theme.font}", system-ui, sans-serif`;
          design.tokens["--font-display"] = `"${theme.font}", system-ui, sans-serif`;
          const body = design.items.body = design.items.body || {};
          body.style = { ...(body.style || {}), "font-family": `"${theme.font}", system-ui, sans-serif` };
        }
        api.set(design);
        done.push(`${at}: theme ${JSON.stringify(theme)}`);
        return;
      }
      if (edit.remove) {
        const before = design.added.length;
        design.added = design.added.filter((widget) => widget.id !== edit.remove);
        if (design.added.length === before) throw new Error(`no added widget "${edit.remove}"`);
        api.set(design);
        done.push(`${at}: widget ${edit.remove} removed`);
        return;
      }
      if (edit.add) {
        if (!api.widgets[edit.add]) throw new Error(`unknown widget "${edit.add}"; one of ${Object.keys(api.widgets).join(", ")}`);
        const id = `w${Date.now().toString(36)}${index}`;
        const widget = { id, type: edit.add, props: edit.props || {} };
        if (edit.in) {
          const holder = resolve(edit.in);
          if (!holder) throw new Error(`nothing matches in "${edit.in}"`);
          widget.in = holder.hasAttribute("data-claw-screen") ? `screen:${holder.getAttribute("data-claw-screen")}` : api.keyOf(holder);
        } else if (edit.after) {
          const after = resolve(edit.after);
          if (!after) throw new Error(`nothing matches after "${edit.after}"`);
          widget.in = after.parentElement.hasAttribute("data-claw-screen") ? `screen:${after.parentElement.getAttribute("data-claw-screen")}` : api.keyOf(after.parentElement);
          widget.after = api.keyOf(after);
        } else {
          const screen = document.querySelector("[data-claw-screen]:not([hidden])");
          if (screen) widget.in = `screen:${screen.getAttribute("data-claw-screen")}`;
        }
        if (edit.style) widget.style = { ...edit.style };
        design.added.push(widget);
        api.set(design);
        const el = document.querySelector(`[data-claw-id="${id}"]`);
        const info = el && api.info(api.keyOf(el));
        if (info && info.parentGrid) {
          api.placeAt(design, info.key, edit.place ? { ...edit.place, insert: edit.place.new_row } : { row: 9999, insert: true, col: 1, span: info.parentGrid.columns });
          api.set(design);
        }
        done.push(`${at}: ${edit.add} added as ${id}${info && info.parentGrid ? ` on the grid` : ""}`);
        return;
      }
      const el = resolve(edit.target);
      if (!el) throw new Error(`nothing on the page matches target "${edit.target}"`);
      const key = api.keyOf(el);
      const entry = entryOf(el);
      const changes = [];
      if (edit.reset) {
        if (widgetOf(el)) widgetOf(el).style = {};
        else delete design.items[key];
        changes.push("reset");
      }
      if (edit.style && typeof edit.style === "object") {
        const target = widgetOf(el) || (design.items[key] = design.items[key] || {});
        target.style = target.style || {};
        for (const [prop, value] of Object.entries(edit.style)) {
          if (value === null || value === "") delete target.style[prop];
          else target.style[prop] = String(value);
        }
        changes.push(`style ${JSON.stringify(edit.style)}`);
      }
      if (typeof edit.text === "string") {
        const widget = widgetOf(el);
        if (widget) widget.props = { ...(widget.props || {}), text: edit.text };
        else entry.text = edit.text;
        changes.push(`text "${edit.text}"`);
      }
      if (edit.props && widgetOf(el)) {
        widgetOf(el).props = { ...(widgetOf(el).props || {}), ...edit.props };
        changes.push(`props ${JSON.stringify(edit.props)}`);
      }
      if (edit.icon !== undefined) {
        if (edit.icon && !api.icons.includes(edit.icon)) throw new Error(`icon "${edit.icon}" is not offered; one of ${api.icons.join(", ")}`);
        const widget = widgetOf(el);
        if (widget) widget.props = { ...(widget.props || {}), icon: edit.icon || "" };
        else if (edit.icon) entry.icon = edit.icon;
        else delete entry.icon;
        changes.push(`icon ${edit.icon || "original"}`);
      }
      if (edit.hidden !== undefined) {
        if (edit.hidden) entry.hidden = true;
        else delete entry.hidden;
        changes.push(edit.hidden ? "hidden" : "shown");
      }
      api.set(design);
      if (edit.grid === false) {
        api.disableGrid(design, key);
        changes.push("grid removed");
      } else if (edit.grid && typeof edit.grid === "object") {
        const columns = Math.max(1, Math.min(12, Number(edit.grid.columns) || 4));
        const gap = Math.max(0, Number(edit.grid.gap ?? 12));
        if (api.info(key) && api.info(key).grid) api.setGrid(design, key, { columns, gap });
        else api.enableGrid(design, key, columns, gap);
        changes.push(`grid of ${columns} columns, gap ${gap}px (each child on its own row first)`);
      }
      api.set(design);
      if (edit.place && typeof edit.place === "object") {
        const info = api.info(key);
        if (!info || !info.parentGrid) throw new Error(`"${edit.target}" is not inside a grid: give its parent a grid first ({"target": <the parent>, "grid": {"columns": 4}})`);
        api.placeAt(design, key, { col: edit.place.col, span: edit.place.span, row: edit.place.row, insert: Boolean(edit.place.new_row) });
        changes.push(`placed ${JSON.stringify(edit.place)}`);
      }
      api.set(design);
      done.push(`${at}: ${name(el)} (key ${key}): ${changes.join(", ") || "nothing to change"}`);
    } catch (error) {
      errors.push(`${at}: ${error.message || error}`);
    }
  });
  // Les grilles de la page et la place de chaque bloc, pour les retouches suivantes.
  const grids = [];
  for (const [key, item] of Object.entries(design.items)) {
    if (!item || !item.grid) continue;
    const el = api.find(key);
    if (!el) continue;
    const children = [...el.children].filter((child) => shown(child) && !(child.hasAttribute("data-claw-own") && !child.hasAttribute("data-claw-id")));
    grids.push(`${name(el)}: ${item.grid.columns} columns; ${children.map((child) => { const info = api.info(api.keyOf(child)); return info && info.place ? `${name(child)} at row ${info.place.row}, col ${info.place.col}, span ${info.place.span}` : `${name(child)} full width`; }).join("; ")}`);
  }
  return { design: api.design, done, errors, grids };
})()
