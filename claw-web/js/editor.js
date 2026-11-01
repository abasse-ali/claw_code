// Éditeur visuel de Claw Web : l'appli d'une branche s'affiche dans un cadre ; on y touche un
// élément pour le régler (texte, icône, couleurs, taille, espacements, coins, police, ordre), on
// le glisse pour le déplacer, on ajoute des widgets, on règle le thème. « Enregistrer » écrit
// claw-design.json (et la couche claw-design.js si l'appli ne l'a pas encore) dans la branche,
// en un commit, sans passer par le modèle. Demandé le 1er novembre 2026.
//
// L'aperçu vient du service de l'appli (sw.js, `apercu/…`) : les fichiers de la branche lus par
// l'API GitHub, de la même origine que Claw Web, donc le cadre se touche directement.

import { escapeHtml } from "./markdown.js";

const svg = (body) => `<svg viewBox="0 0 24 24" aria-hidden="true">${body}</svg>`;
const I = {
  back: svg('<path d="M15 6l-6 6 6 6"/>'),
  undo: svg('<path d="M9 14 4 9l5-5"/><path d="M4 9h10.5a5.5 5.5 0 0 1 0 11H11"/>'),
  save: svg('<path d="M5 12l5 5L20 7"/>'),
  cursor: svg('<path d="m4 4 7 17 2.5-7.5L21 11Z"/>'),
  play: svg('<path d="M7 4v16l13-8Z"/>'),
  up: svg('<path d="M12 19V5M5 12l7-7 7 7"/>'),
  down: svg('<path d="M12 5v14M19 12l-7 7-7-7"/>'),
  parent: svg('<path d="M9 14 4 9l5-5"/><path d="M4 9h16v11"/>'),
  eye: svg('<path d="M2 12s3.5-7 10-7 10 7 10 7-3.5 7-10 7S2 12 2 12Z"/><circle cx="12" cy="12" r="3"/>'),
  trash: svg('<path d="M4 7h16M10 11v6M14 11v6M6 7l1 13h10l1-13M9 7V4h6v3"/>'),
  reset: svg('<path d="M3 12a9 9 0 1 0 3-6.7L3 8"/><path d="M3 3v5h5"/>'),
  fold: svg('<path d="M6 9l6 6 6-6"/>'),
  panel: svg('<path d="M4 6h16M4 12h10M4 18h7"/><circle cx="18" cy="16" r="3"/>'),
  grid: svg('<rect x="3" y="3" width="18" height="18" rx="2"/><path d="M9 3v18M15 3v18M3 9h18M3 15h18"/>'),
};
const clone = (value) => JSON.parse(JSON.stringify(value));
const px = (value) => {
  const number = parseFloat(value);
  return Number.isFinite(number) ? Math.round(number) : 0;
};
// Toute couleur CSS (rgb, oklch, color-mix…) en #rrggbb, lue par un canevas ; transparente : blanc.
const paint = document.createElement("canvas").getContext("2d", { willReadFrequently: true });
const hex = (color) => {
  if (!paint) return "#000000";
  paint.clearRect(0, 0, 1, 1);
  paint.fillStyle = "#000";
  paint.fillStyle = String(color || "").trim() || "#000";
  paint.fillRect(0, 0, 1, 1);
  const [r, g, b, a] = paint.getImageData(0, 0, 1, 1).data;
  if (a === 0) return "#ffffff";
  return `#${[r, g, b].map((part) => part.toString(16).padStart(2, "0")).join("")}`;
};
const SHADOWS = {
  "": "Aucune",
  "0 1px 2px rgb(0 0 0 / .06), 0 4px 14px rgb(0 0 0 / .08)": "Douce",
  "0 10px 30px rgb(0 0 0 / .18)": "Marquée",
};
const WEIGHTS = { "": "Par défaut", 400: "Normale", 500: "Moyenne", 600: "Demi-grasse", 700: "Grasse", 800: "Très grasse" };
const PROP_LABELS = { text: "Texte", title: "Titre", sub: "Détail", value: "Valeur", label: "Libellé", placeholder: "Exemple dans le champ", src: "Adresse de l'image (https://…)", alt: "Description de l'image" };
const THEME = [
  ["--accent", "Accent"], ["--paper", "Fond de l'appli"], ["--surface", "Cartes"], ["--ink", "Texte"], ["--muted", "Texte secondaire"],
];

export function editor(app) {
  const { toast, failure } = app;
  const view = () => document.querySelector("#view");

  /** `repo` (propriétaire/nom), `branch`, `back` (adresse du bouton retour), `title`. */
  async function showEditor({ repo, branch, back = "#/", title = "" }) {
    app.leavePage();
    document.body.classList.add("editing-app");
    view().innerHTML = `
      <section class="editor" aria-label="Éditeur visuel">
        <header class="editor-bar">
          <a class="editor-icon" href="${escapeHtml(back)}" aria-label="Retour" data-leave>${I.back}</a>
          <div class="editor-title"><strong>${escapeHtml(title || repo.split("/")[1])}</strong><small>${escapeHtml(branch)}</small></div>
          <div class="editor-mode" role="radiogroup" aria-label="Mode">
            <button type="button" role="radio" aria-checked="true" data-mode="select">${I.cursor}<span>Modifier</span></button>
            <button type="button" role="radio" aria-checked="false" data-mode="try">${I.play}<span>Essayer</span></button>
          </div>
          <button type="button" class="editor-icon" data-grid-view aria-pressed="true" aria-label="Montrer la grille et les dimensions">${I.grid}</button>
          <button type="button" class="editor-icon" data-undo aria-label="Annuler" disabled>${I.undo}</button>
          <button type="button" class="primary editor-save" data-save disabled>${I.save}<span>Enregistrer</span></button>
        </header>
        <div class="editor-stage"><div class="editor-device"><p class="editor-loading">Ouverture de l'appli…</p></div></div>
        <aside class="editor-panel">
          <nav class="editor-tabs" role="tablist">
            <button type="button" role="tab" aria-selected="true" data-tab="element">Élément</button>
            <button type="button" role="tab" aria-selected="false" data-tab="theme">Thème</button>
            <button type="button" role="tab" aria-selected="false" data-tab="add">Ajouter</button>
            <button type="button" role="tab" aria-selected="false" data-tab="screens" hidden>Écrans</button>
            <button type="button" class="editor-fold" data-fold aria-label="Réduire le panneau en bulle">${I.fold}</button>
          </nav>
          <div class="editor-body"><p class="editor-hint">Touchez un élément de l'appli pour le régler ; la poignée ✥ le déplace, la poignée ronde change sa taille. Touchez encore un bouton déjà choisi pour vous en servir : le + ouvre sa fenêtre, que vous pouvez retoucher aussi.</p></div>
        </aside>
        <button type="button" class="editor-bubble" data-unfold aria-label="Ouvrir le panneau de réglages" hidden>${I.panel}<span class="editor-bubble-name"></span></button>
      </section>`;
    const root = view().querySelector(".editor");
    const body = root.querySelector(".editor-body");
    const github = app.github();
    let dir = "";
    let win = null;
    let design = null;
    let saved = "";
    let selected = null;
    let tab = "element";
    const history = [];
    let lastPush = 0;
    const dirty = () => JSON.stringify(design) !== saved;
    const refreshBar = () => {
      root.querySelector("[data-undo]").disabled = !history.length;
      root.querySelector("[data-save]").disabled = !design || !dirty();
    };

    // --- Lecture de la branche -----------------------------------------------------------------
    try {
      if (await github.fileExists(repo, "docs/index.html", branch)) dir = "docs";
      else if (!(await github.fileExists(repo, "index.html", branch))) {
        throw new Error("Cette branche n'a pas encore de page à afficher (ni docs/index.html, ni index.html) : demandez d'abord à Claw de construire l'appli.");
      }
      const found = await github.readJson(repo, `${dir ? `${dir}/` : ""}claw-design.json`, branch);
      design = found.value || {};
    } catch (error) {
      root.querySelector(".editor-device").innerHTML = `<p class="editor-loading">${escapeHtml(error.message)}</p>`;
      return;
    }
    // Le service de l'appli lit la branche avec le jeton : il le reçoit ici (et le redemande).
    try {
      const registration = await navigator.serviceWorker.ready;
      (registration.active || navigator.serviceWorker.controller).postMessage({ type: "claw-preview-token", token: github.token });
    } catch (_) {
      root.querySelector(".editor-device").innerHTML = '<p class="editor-loading">Ce navigateur ne permet pas l\'aperçu (service hors ligne absent).</p>';
      return;
    }
    const [owner, name] = repo.split("/");
    const frame = document.createElement("iframe");
    frame.title = "Aperçu de l'appli";
    frame.src = `apercu/${owner}/${name}/${encodeURIComponent(branch)}/${dir ? `${dir}/` : ""}index.html`;
    const device = root.querySelector(".editor-device");
    device.innerHTML = "";
    device.append(frame);

    const api = () => win && win.clawDesign;
    const host = {
      mode: "select",
      showHidden: false,
      showGrid: true,
      onPlace(key, spec) { change((target) => api().placeAt(target, key, spec)); render(); },
      onSelect(info) {
        selected = info;
        root.querySelector(".editor-bubble-name").textContent = info ? info.name : "";
        if (tab !== "element") switchTab("element");
        else render();
      },
      onReorder(key, to) { change((target) => api().reorder(target, key, to)); },
      onResize(key, size) {
        change((target) => {
          const style = styleOf(target, key);
          style.width = size.width;
          style.height = size.height;
        });
      },
    };
    frame.addEventListener("load", async () => {
      win = frame.contentWindow;
      if (!win.clawDesign) {
        await new Promise((resolve) => {
          const script = win.document.createElement("script");
          script.src = new URL("claw-design.js", location.href).href;
          script.onload = resolve;
          script.onerror = resolve;
          win.document.head.appendChild(script);
        });
      }
      if (!api()) {
        device.insertAdjacentHTML("beforeend", '<p class="editor-loading">La couche d\'édition n\'a pas pu se charger dans l\'appli.</p>');
        return;
      }
      design = api().normalize(design);
      if (!saved) saved = JSON.stringify(design);
      api().set(design);
      api().edit(host);
      root.querySelector('[data-tab="screens"]').hidden = !win.document.querySelector("[data-claw-root]");
      refreshBar();
      render();
    });

    // --- Changements -------------------------------------------------------------------------
    function change(update, { coalesce = false } = {}) {
      if (!api()) return;
      const now = Date.now();
      if (!coalesce || now - lastPush > 900) history.push(clone(design));
      lastPush = now;
      if (history.length > 80) history.shift();
      update(design);
      api().set(design);
      if (selected) selected = api().info(selected.key) || selected;
      refreshBar();
    }
    const widgetOf = (target, key) => {
      const match = /^\[data-claw-id="([^"]+)"\]$/.exec(key || "");
      return match ? target.added.find((widget) => widget.id === match[1]) : null;
    };
    const itemOf = (target, key) => (target.items[key] = target.items[key] || {});
    function styleOf(target, key) {
      const widget = widgetOf(target, key);
      if (widget) return (widget.style = widget.style || {});
      const item = itemOf(target, key);
      return (item.style = item.style || {});
    }
    const setStyle = (prop, value) => change((target) => {
      const style = styleOf(target, selected.key);
      if (value === "" || value == null) delete style[prop];
      else style[prop] = value;
    }, { coalesce: true });

    // --- Panneau -----------------------------------------------------------------------------
    function switchTab(next) {
      tab = next;
      for (const button of root.querySelectorAll("[data-tab]")) button.setAttribute("aria-selected", String(button.dataset.tab === tab));
      render();
    }
    const field = (label, control, group = false) => (group
      ? `<div class="editor-field" role="group" aria-label="${escapeHtml(label)}"><span>${label}</span>${control}</div>`
      : `<label class="editor-field"><span>${label}</span>${control}</label>`);
    const slider = (prop, label, value, max, unit = "px", min = 0) => field(label, `<span class="editor-range">
      <input type="range" min="${min}" max="${max}" value="${value}" data-style="${prop}" data-unit="${unit}" aria-label="${label}">
      <output>${value}${unit}</output><button type="button" class="editor-clear" data-clear="${prop}" aria-label="Revenir à l'origine">${I.reset}</button></span>`, true);
    const color = (prop, label, value) => field(label, `<span class="editor-color">
      <input type="color" value="${hex(value)}" data-style="${prop}" aria-label="${label}"><code>${hex(value)}</code>
      <button type="button" class="editor-clear" data-clear="${prop}" aria-label="Couleur d'origine">${I.reset}</button></span>`, true);

    function render() {
      if (!api()) return;
      if (tab === "theme") return renderTheme();
      if (tab === "add") return renderAdd();
      if (tab === "screens") return renderScreens();
      if (!selected) {
        body.innerHTML = `<p class="editor-hint">Touchez un élément de l'appli pour le régler ; la poignée ✥ le déplace, la poignée ronde change sa taille. Touchez encore un bouton déjà choisi pour vous en servir : le + ouvre sa fenêtre, que vous pouvez retoucher aussi.</p>
          ${hiddenList()}`;
        return;
      }
      const s = selected;
      const own = (s.widget && s.widget.style) || (s.item && s.item.style) || {};
      const c = s.computed;
      const props = s.widget ? { ...api().widgets[s.widget.type].props, ...s.widget.props } : null;
      const textValue = s.widget ? null : (s.item.text != null ? s.item.text : s.text);
      const iconName = s.widget ? (props && "icon" in props ? props.icon : null) : (s.item.icon !== undefined ? s.item.icon : (s.hasIcon ? "" : null));
      body.innerHTML = `
        <div class="editor-selected">
          <strong>${escapeHtml(s.name)}</strong>
          <div class="editor-actions">
            <button type="button" data-act="parent" ${s.parentKey ? "" : "disabled"} title="Choisir le bloc qui le contient">${I.parent}<span>Parent</span></button>
            <button type="button" data-act="up" ${s.index > 0 ? "" : "disabled"} aria-label="Monter">${I.up}</button>
            <button type="button" data-act="down" ${s.index < s.count - 1 ? "" : "disabled"} aria-label="Descendre">${I.down}</button>
            <button type="button" data-act="${s.widget ? "delete" : "hide"}" aria-label="${s.widget ? "Supprimer" : "Masquer"}">${s.widget ? I.trash : I.eye}<span>${s.widget ? "Supprimer" : "Masquer"}</span></button>
            <button type="button" data-act="reset" aria-label="Tout remettre comme avant">${I.reset}</button>
          </div>
        </div>
        ${gridPanel(s)}
        ${props ? Object.keys(props).filter((key) => key !== "icon").map((key) => field(PROP_LABELS[key] || key,
          key === "value" && s.widget.type === "progress"
            ? `<input type="range" min="0" max="100" value="${escapeHtml(props[key])}" data-prop="${key}">`
            : `<input type="text" value="${escapeHtml(props[key])}" data-prop="${key}">`)).join("") : ""}
        ${textValue != null ? field("Texte", `<textarea rows="2" data-text>${escapeHtml(textValue)}</textarea>`) : ""}
        ${iconName != null ? field("Icône", `<button type="button" class="editor-icon-pick" data-icons>${iconName ? api().iconSvg(iconName, 20) : ""}<span>${iconName || "Celle d'origine"}</span></button><div class="editor-icon-grid" hidden></div>`, true) : ""}
        <h3>Couleurs</h3>
        <div class="editor-grid">${color("color", "Texte", own.color || c.color)}${color("background", "Fond", own.background || c["background-color"])}</div>
        <h3>Taille</h3>
        ${slider("width", "Largeur", px(own.width || c.width), 600)}
        ${slider("height", "Hauteur", px(own.height || c.height), 600)}
        <h3>Espacements</h3>
        ${slider("padding", "Intérieur", px(own.padding || c["padding-top"]), 64)}
        ${slider("margin", "Extérieur", px(own.margin || c["margin-top"]), 64)}
        ${s.container ? slider("gap", "Entre les éléments", px(own.gap || 0), 48) : ""}
        ${slider("border-radius", "Coins arrondis", px(own["border-radius"] || c["border-radius"]), 48)}
        <h3>Texte</h3>
        ${slider("font-size", "Taille du texte", px(own["font-size"] || c["font-size"]), 72, "px", 8)}
        <div class="editor-grid">
          ${field("Graisse", `<select data-style="font-weight">${Object.entries(WEIGHTS).map(([value, label]) => `<option value="${value}"${String(own["font-weight"] || "") === String(value) ? " selected" : ""}>${label}</option>`).join("")}</select>`)}
          ${field("Police", `<select data-style="font-family"><option value="">Celle de l'appli</option>${api().fonts.map((font) => `<option value="${escapeHtml(`"${font}", sans-serif`)}"${String(own["font-family"] || "").includes(font) ? " selected" : ""}>${font}</option>`).join("")}</select>`)}
        </div>
        ${field("Alignement", `<span class="editor-seg">${[["", "Auto"], ["left", "Gauche"], ["center", "Centre"], ["right", "Droite"]].map(([value, label]) => `<button type="button" data-align="${value}" aria-pressed="${String((own["text-align"] || "") === value)}">${label}</button>`).join("")}</span>`, true)}
        <h3>Effets</h3>
        <div class="editor-grid">
          ${field("Ombre", `<select data-style="box-shadow">${Object.entries(SHADOWS).map(([value, label]) => `<option value="${value}"${(own["box-shadow"] || "") === value ? " selected" : ""}>${label}</option>`).join("")}</select>`)}
          ${s.container ? field("Disposition", `<select data-layout><option value="">D'origine</option><option value="column"${own["flex-direction"] === "column" ? " selected" : ""}>En colonne</option><option value="row"${own["flex-direction"] === "row" ? " selected" : ""}>En ligne</option></select>`) : ""}
        </div>
        ${slider("opacity", "Opacité", Math.round((own.opacity != null ? Number(own.opacity) : Number(c.opacity || 1)) * 100), 100, "%")}
        ${hiddenList()}`;
    }
    // La grille : celle que porte le bloc choisi, et sa place dans la grille de son parent.
    function gridPanel(s) {
      let html = "";
      if (s.parentGrid && s.place) {
        const n = s.parentGrid.columns;
        html += `<h3>Place sur la grille</h3>
          <div class="editor-grid">
            ${field("Colonne de départ", `<span class="editor-range"><input type="range" min="1" max="${n - s.place.span + 1}" value="${s.place.col}" data-place="col" aria-label="Colonne de départ"><output>${s.place.col}</output></span>`, true)}
            ${field("Largeur", `<span class="editor-range"><input type="range" min="1" max="${n}" value="${s.place.span}" data-place="span" aria-label="Largeur en colonnes"><output>${s.place.span}/${n}</output></span>`, true)}
          </div>
          ${field("Ligne", `<span class="editor-steps"><button type="button" data-row="-1" aria-label="Ligne au-dessus">${I.up}</button><output>${s.place.row}</output><button type="button" data-row="1" aria-label="Ligne en dessous">${I.down}</button><button type="button" data-row="new" class="editor-small">Seul sur une ligne</button></span>`, true)}
          <p class="editor-hint">${s.size[0]} × ${s.size[1]} px. Glissez la poignée ✥ pour le poser dans une autre case, la poignée ronde pour changer sa largeur en colonnes.</p>`;
      }
      if (s.container) {
        html += s.grid
          ? `<h3>Grille de ce bloc</h3>
            <div class="editor-grid">
              ${field("Colonnes", `<span class="editor-range"><input type="range" min="1" max="12" value="${s.grid.columns}" data-grid="columns" aria-label="Colonnes"><output>${s.grid.columns}</output></span>`, true)}
              ${field("Écart", `<span class="editor-range"><input type="range" min="0" max="40" value="${s.grid.gap}" data-grid="gap" aria-label="Écart entre les cases"><output>${s.grid.gap}px</output></span>`, true)}
            </div>
            <button type="button" class="small-link" data-grid-off>Retirer la grille</button>`
          : `<button type="button" class="small-link editor-grid-on" data-grid-on>${I.grid}<span>Placer ses blocs sur une grille</span></button>`;
      }
      return html;
    }
    function hiddenList() {
      const hidden = Object.entries(design.items).filter(([, item]) => item && item.hidden);
      if (!hidden.length) return "";
      return `<h3>Éléments masqués</h3><ul class="editor-hidden">${hidden.map(([key]) => {
        const info = api().info(key);
        return `<li><span>${escapeHtml(info ? info.name : "Élément introuvable sur cet écran")}</span><button type="button" data-unhide="${escapeHtml(key)}">Afficher</button></li>`;
      }).join("")}</ul>`;
    }
    function renderTheme() {
      const tokens = design.tokens;
      const style = win.getComputedStyle(win.document.documentElement);
      const current = (name) => tokens[name] || style.getPropertyValue(name).trim();
      body.innerHTML = `
        <p class="editor-hint">Le thème change toute l'appli d'un coup ; un élément réglé à part garde son réglage.</p>
        <div class="editor-grid">${THEME.map(([name, label]) => field(label, `<span class="editor-color"><input type="color" value="${hex(current(name))}" data-token="${name}" aria-label="${label}"><code>${escapeHtml(hex(current(name)))}</code><button type="button" class="editor-clear" data-token-clear="${name}" aria-label="Couleur d'origine">${I.reset}</button></span>`, true)).join("")}</div>
        ${field("Police de l'appli", `<select data-theme-font><option value="">Celle de l'appli</option>${api().fonts.map((font) => `<option${(tokens["--font-text"] || "").includes(font) ? " selected" : ""}>${font}</option>`).join("")}</select>`)}
        ${field("Coins arrondis", `<span class="editor-range"><input type="range" min="0" max="32" value="${px(current("--radius") || 20)}" data-theme-radius><output>${px(current("--radius") || 20)}px</output></span>`)}
        <button type="button" class="small-link" data-theme-reset>${I.reset}<span>Revenir au thème d'origine</span></button>`;
    }
    function renderAdd() {
      const where = selected ? `après « ${escapeHtml(selected.name)} »` : "en bas de l'écran";
      body.innerHTML = `<p class="editor-hint">Le widget arrive ${where}${selected && selected.container ? `, ou dedans si vous choisissez « Dedans »` : ""}.</p>
        ${selected && selected.container ? `<label class="editor-check"><input type="checkbox" data-inside> Dedans</label>` : ""}
        <div class="editor-widgets">${Object.entries(api().widgets).map(([type, widget]) => `<button type="button" data-add="${type}">${escapeHtml(widget.label)}</button>`).join("")}</div>`;
    }
    function renderScreens() {
      const screens = design.screens.length ? design.screens : [{ id: "accueil", label: "Accueil", icon: "house" }];
      body.innerHTML = `<p class="editor-hint">Avec deux écrans ou plus, une barre d'onglets apparaît en bas.</p>
        <ul class="editor-screens">${screens.map((screen, index) => `<li>
          <button type="button" class="editor-icon-pick" data-screen-icon="${index}">${api().iconSvg(screen.icon || "circle", 20)}</button>
          <input type="text" value="${escapeHtml(screen.label)}" data-screen-label="${index}" aria-label="Nom de l'écran">
          <button type="button" data-screen-open="${escapeHtml(screen.id)}">Voir</button>
          ${screens.length > 1 ? `<button type="button" data-screen-remove="${index}" aria-label="Retirer l'écran">${I.trash}</button>` : ""}
        </li>`).join("")}</ul><div class="editor-icon-grid" hidden></div>
        <button type="button" class="small-link" data-screen-add>+ Ajouter un écran</button>`;
    }
    const iconGrid = (grid, onPick) => {
      grid.hidden = !grid.hidden;
      if (grid.hidden) return;
      grid.innerHTML = `<button type="button" data-icon="">Aucune / d'origine</button>${api().icons.map((name) => `<button type="button" data-icon="${name}" title="${name}" aria-label="${name}">${api().iconSvg(name, 22)}</button>`).join("")}`;
      grid.onclick = (event) => {
        const button = event.target.closest("[data-icon]");
        if (!button) return;
        onPick(button.dataset.icon);
        grid.hidden = true;
      };
    };
    const screensOf = (target) => (target.screens.length ? target.screens : (target.screens = [{ id: "accueil", label: "Accueil", icon: "house" }]));

    body.addEventListener("input", (event) => {
      const el = event.target;
      if (el.dataset.style && el.type === "range") {
        const unit = el.dataset.unit;
        el.nextElementSibling.textContent = `${el.value}${unit}`;
        setStyle(el.dataset.style, unit === "%" ? String(Number(el.value) / 100) : `${el.value}${unit}`);
      } else if (el.dataset.style && el.type === "color") {
        el.nextElementSibling.textContent = el.value;
        setStyle(el.dataset.style, el.value);
      } else if (el.dataset.place) {
        const value = Number(el.value);
        el.nextElementSibling.textContent = el.dataset.place === "span" ? `${value}/${selected.parentGrid.columns}` : String(value);
        change((target) => api().placeAt(target, selected.key, { [el.dataset.place]: value }), { coalesce: true });
      } else if (el.dataset.grid) {
        const value = Number(el.value);
        el.nextElementSibling.textContent = el.dataset.grid === "gap" ? `${value}px` : String(value);
        change((target) => api().setGrid(target, selected.key, { [el.dataset.grid]: value }), { coalesce: true });
      } else if (el.matches("[data-text]")) {
        change((target) => { itemOf(target, selected.key).text = el.value; }, { coalesce: true });
      } else if (el.dataset.prop) {
        change((target) => { const widget = widgetOf(target, selected.key); widget.props = { ...(widget.props || {}), [el.dataset.prop]: el.type === "range" ? Number(el.value) : el.value }; }, { coalesce: true });
      } else if (el.dataset.token) {
        el.nextElementSibling.textContent = el.value;
        change((target) => { target.tokens[el.dataset.token] = el.value; }, { coalesce: true });
      } else if (el.matches("[data-theme-radius]")) {
        el.nextElementSibling.textContent = `${el.value}px`;
        change((target) => { target.tokens["--radius"] = `${el.value}px`; target.tokens["--radius-sm"] = `${Math.round(el.value * 0.7)}px`; }, { coalesce: true });
      } else if (el.dataset.screenLabel != null) {
        change((target) => { screensOf(target)[Number(el.dataset.screenLabel)].label = el.value; }, { coalesce: true });
      }
    });
    body.addEventListener("change", (event) => {
      const el = event.target;
      if (el.matches("select[data-style]")) { setStyle(el.dataset.style, el.value); render(); }
      if (el.matches("[data-layout]")) {
        change((target) => {
          const style = styleOf(target, selected.key);
          if (!el.value) { delete style.display; delete style["flex-direction"]; delete style["flex-wrap"]; return; }
          style.display = "flex";
          style["flex-direction"] = el.value;
          style["flex-wrap"] = el.value === "row" ? "wrap" : "nowrap";
        });
      }
      if (el.matches("[data-theme-font]")) {
        change((target) => {
          if (!el.value) { delete target.tokens["--font-text"]; delete target.tokens["--font-display"]; return; }
          target.fonts = [...new Set([...(target.fonts || []), el.value])];
          target.tokens["--font-text"] = `"${el.value}", system-ui, sans-serif`;
          target.tokens["--font-display"] = `"${el.value}", system-ui, sans-serif`;
          styleOf(target, "body")["font-family"] = `"${el.value}", system-ui, sans-serif`;
        });
      }
    });
    body.addEventListener("click", (event) => {
      const el = event.target.closest("button");
      if (!el || !api()) return;
      if (el.dataset.clear) { setStyle(el.dataset.clear, ""); render(); return; }
      if (el.matches("[data-grid-on]")) { change((target) => api().enableGrid(target, selected.key, 4, 12)); render(); return; }
      if (el.matches("[data-grid-off]")) { change((target) => api().disableGrid(target, selected.key)); render(); return; }
      if (el.dataset.row) {
        const row = selected.place.row;
        change((target) => api().placeAt(target, selected.key, el.dataset.row === "new"
          ? { row: row + 1, insert: true, col: 1, span: selected.parentGrid.columns }
          : { row: Math.max(1, row + Number(el.dataset.row)), insert: false }));
        render();
        return;
      }
      if (el.dataset.tokenClear) { change((target) => { delete target.tokens[el.dataset.tokenClear]; }); renderTheme(); return; }
      if (el.matches("[data-theme-reset]")) { change((target) => { target.tokens = {}; }); renderTheme(); return; }
      if (el.dataset.align != null) { setStyle("text-align", el.dataset.align); render(); return; }
      if (el.dataset.unhide) { change((target) => { delete itemOf(target, el.dataset.unhide).hidden; }); render(); return; }
      if (el.matches("[data-icons]")) {
        iconGrid(el.nextElementSibling, (name) => {
          change((target) => {
            const widget = widgetOf(target, selected.key);
            if (widget) widget.props = { ...(widget.props || {}), icon: name };
            else if (name) itemOf(target, selected.key).icon = name;
            else delete itemOf(target, selected.key).icon;
          });
          render();
        });
        return;
      }
      if (el.dataset.add) {
        const id = `w${Date.now().toString(36)}`;
        const inside = body.querySelector("[data-inside]") && body.querySelector("[data-inside]").checked;
        change((target) => {
          const widget = { id, type: el.dataset.add, props: {} };
          if (selected && inside) widget.in = selected.key;
          else if (selected && selected.parentKey) { widget.in = selected.parentKey; widget.after = selected.key; }
          else if (win.document.querySelector("[data-claw-root]")) {
            const shown = win.document.querySelector("[data-claw-screen]:not([hidden])");
            widget.in = `screen:${shown ? shown.getAttribute("data-claw-screen") : "accueil"}`;
          }
          target.added.push(widget);
        });
        const added = api().info(`[data-claw-id="${id}"]`);
        if (added && added.parentGrid) {
          change((target) => api().placeAt(target, added.key, { row: 9999, insert: true, col: 1, span: added.parentGrid.columns }), { coalesce: true });
        }
        api().select(`[data-claw-id="${id}"]`);
        return;
      }
      if (el.dataset.screenIcon != null) {
        iconGrid(body.querySelector(".editor-icon-grid"), (name) => { change((target) => { screensOf(target)[Number(el.dataset.screenIcon)].icon = name || "circle"; }); renderScreens(); });
        return;
      }
      if (el.dataset.screenOpen) { win.location.hash = `#${el.dataset.screenOpen}`; return; }
      if (el.dataset.screenRemove != null) {
        change((target) => {
          const [gone] = screensOf(target).splice(Number(el.dataset.screenRemove), 1);
          target.added = target.added.filter((widget) => widget.in !== `screen:${gone.id}`);
        });
        renderScreens();
        return;
      }
      if (el.matches("[data-screen-add]")) {
        change((target) => {
          const screens = screensOf(target);
          const id = `ecran-${screens.length + 1}-${Date.now().toString(36).slice(-3)}`;
          screens.push({ id, label: `Écran ${screens.length + 1}`, icon: "circle" });
          target.items[`[data-claw-screen="${id}"]`] = { grid: { columns: 4, gap: 12 } };
        });
        renderScreens();
        return;
      }
      const act = el.dataset.act;
      if (!act || !selected) return;
      const key = selected.key;
      if (act === "parent") api().select(selected.parentKey);
      if (act === "up" || act === "down") change((target) => api().reorder(target, key, selected.index + (act === "up" ? -1 : 1)));
      if (act === "hide") { change((target) => { itemOf(target, key).hidden = true; }); api().select(null); }
      if (act === "delete") { change((target) => { target.added = target.added.filter((widget) => `[data-claw-id="${widget.id}"]` !== key); }); api().select(null); }
      if (act === "reset") {
        change((target) => {
          const widget = widgetOf(target, key);
          if (widget) widget.style = {};
          else delete target.items[key];
        });
      }
      render();
    });

    root.querySelector(".editor-tabs").addEventListener("click", (event) => {
      const button = event.target.closest("[data-tab]");
      if (button) switchTab(button.dataset.tab);
    });
    root.querySelector(".editor-mode").addEventListener("click", (event) => {
      const button = event.target.closest("[data-mode]");
      if (!button || !api()) return;
      for (const item of root.querySelectorAll("[data-mode]")) item.setAttribute("aria-checked", String(item === button));
      api().setMode(button.dataset.mode);
      root.classList.toggle("trying", button.dataset.mode === "try");
    });
    root.querySelector("[data-grid-view]").addEventListener("click", (event) => {
      host.showGrid = !host.showGrid;
      event.currentTarget.setAttribute("aria-pressed", String(host.showGrid));
      if (api()) api().refresh();
    });
    // Le panneau se réduit en une bulle en bas à droite : l'appli prend tout l'écran.
    const fold = (folded) => {
      root.classList.toggle("folded", folded);
      root.querySelector("[data-unfold]").hidden = !folded;
      try { localStorage.setItem("claw-web.editor.folded", folded ? "1" : ""); } catch (_) { /* rien */ }
      if (api()) setTimeout(() => api().refresh(), 220);
    };
    root.querySelector("[data-fold]").addEventListener("click", () => fold(true));
    root.querySelector("[data-unfold]").addEventListener("click", () => fold(false));
    try { if (localStorage.getItem("claw-web.editor.folded")) fold(true); } catch (_) { /* rien */ }
    root.querySelector("[data-undo]").addEventListener("click", () => {
      if (!history.length || !api()) return;
      design = history.pop();
      api().set(design);
      if (selected) selected = api().info(selected.key);
      refreshBar();
      render();
    });
    root.querySelector("[data-leave]").addEventListener("click", (event) => {
      if (design && dirty() && !confirm("Partir sans enregistrer vos retouches ?")) event.preventDefault();
    });

    // --- Enregistrer ---------------------------------------------------------------------------
    root.querySelector("[data-save]").addEventListener("click", async (event) => {
      const button = event.currentTarget;
      button.disabled = true;
      button.querySelector("span").textContent = "Enregistrement…";
      try {
        const layer = await (await fetch("claw-design.js", { cache: "no-store" })).text();
        const json = `${JSON.stringify(design, null, 1)}\n`;
        const prefix = dir ? `${dir}/` : "";
        const files = { [`${prefix}claw-design.json`]: json };
        const page = await github.readText(repo, `${prefix}index.html`, branch);
        const tag = '<script src="./claw-design.js" defer></script>';
        if ((await github.readText(repo, `${prefix}claw-design.js`, branch)) !== layer) files[`${prefix}claw-design.js`] = layer;
        if (page && !page.includes("claw-design.js")) files[`${prefix}index.html`] = page.replace(/<\/head>/i, `  ${tag}\n</head>`);
        // Appli construite (Vite) : la source aussi, pour que la prochaine construction les garde.
        if (dir === "docs" && (await github.fileExists(repo, "package.json", branch))) {
          files["public/claw-design.json"] = json;
          if ((await github.readText(repo, "public/claw-design.js", branch)) !== layer) files["public/claw-design.js"] = layer;
          const source = await github.readText(repo, "index.html", branch);
          if (source && !source.includes("claw-design.js")) files["index.html"] = source.replace(/<\/head>/i, `  ${tag}\n</head>`);
        }
        await github.commitFiles(repo, branch, files, "Retouches visuelles faites dans l'éditeur de Claw Web");
        saved = JSON.stringify(design);
        toast("Retouches enregistrées dans la branche");
      } catch (error) {
        failure(error);
      } finally {
        button.querySelector("span").textContent = "Enregistrer";
        refreshBar();
      }
    });
  }

  /** Une appli faite à la main, sans demande à Claw : un dépôt neuf, publié, ouvert dans l'éditeur. */
  async function createVisualApp(name, isPrivate) {
    const github = app.github();
    const slug = name.toLowerCase().normalize("NFD").replace(/[̀-ͯ]/g, "").replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "") || "mon-appli";
    const repo = await github.createRepository({ name: slug, description: `${name} : appli faite dans l'éditeur visuel de Claw`, isPrivate });
    const layer = await (await fetch("claw-design.js", { cache: "no-store" })).text();
    const design = {
      version: 1, tokens: {}, fonts: [], screens: [{ id: "accueil", label: "Accueil", icon: "house" }],
      // Chaque écran est une grille de 4 colonnes : les blocs s'y posent et s'y déplacent.
      items: { '[data-claw-screen="accueil"]': { grid: { columns: 4, gap: 12 } } },
      added: [
        { id: "w1", type: "title", props: { text: name }, in: "screen:accueil", place: { col: 1, span: 4, row: 1 } },
        { id: "w2", type: "text", props: { text: "Touchez un élément pour le modifier, ou ajoutez des widgets depuis l'onglet « Ajouter »." }, in: "screen:accueil", place: { col: 1, span: 4, row: 2 } },
      ],
    };
    const page = `<!doctype html>
<html lang="fr">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1, viewport-fit=cover">
  <title>${escapeHtml(name)}</title>
  <style>
    :root { --paper: #f4f3ef; --surface: #fff; --surface-2: #ecebe6; --ink: #16151b; --muted: #5d5b66; --line: rgb(22 21 27 / .1);
      --accent: #5b4bd6; --on-accent: #fff; --accent-soft: color-mix(in srgb, var(--accent) 13%, var(--surface));
      --radius: 20px; --radius-sm: 14px; --font-text: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif; --font-display: var(--font-text);
      --shadow: 0 1px 2px rgb(16 15 20 / .04), 0 8px 24px rgb(16 15 20 / .06); color-scheme: light dark; }
    @media (prefers-color-scheme: dark) { :root { --paper: #111113; --surface: #1c1c1f; --surface-2: #26262a; --ink: #f2f1f4; --muted: #b4b2bc; --line: rgb(255 255 255 / .1); --accent: #9a8cf0; --on-accent: #111; } }
    * { box-sizing: border-box; }
    body { margin: 0; background: var(--paper); color: var(--ink); font: 16px/1.5 var(--font-text); -webkit-font-smoothing: antialiased; }
  </style>
  <script src="./claw-design.js" defer></script>
</head>
<body>
  <main data-claw-root></main>
</body>
</html>
`;
    await github.commitFiles(repo.full_name, repo.default_branch || "main", {
      "index.html": page, "claw-design.js": layer, "claw-design.json": `${JSON.stringify(design, null, 1)}\n`,
    }, "Appli créée dans l'éditeur visuel de Claw Web");
    if (!isPrivate) await github.publishPages(repo.full_name, repo.default_branch || "main", "/").catch(() => null);
    return repo;
  }

  return { showEditor, createVisualApp };
}
