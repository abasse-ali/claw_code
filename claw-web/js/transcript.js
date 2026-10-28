// Journal détaillé d'un passage, publié par le moteur (journal.py) dans des commentaires de
// la session, et affiché comme dans l'interface locale de Claw : messages de l'agent,
// réflexions, outils dépliables avec leur entrée et leur sortie, différences de fichiers,
// liste de tâches. Aussi : les modifications d'une branche (comparaison GitHub).

import { escapeHtml, renderMarkdown } from "./markdown.js";
import { highlight, languageOf } from "./highlight.js";
import { looksEnglish, modelName, plainNotice } from "./phrases.js";

export const LOG = "<!-- claw-web:log";
const DATA = "<!-- claw-web:data ";

const ICONS = {
  terminal: '<svg viewBox="0 0 24 24" aria-hidden="true"><rect x="3" y="4.5" width="18" height="15" rx="2"/><path d="M7 9.5l3 2.5-3 2.5M12.5 15H17"/></svg>',
  file: '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M14 3H7a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8z"/><path d="M14 3v5h5M9 13h6M9 17h6"/></svg>',
  filePlus: '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M14 3H7a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8z"/><path d="M14 3v5h5M12 11v6M9 14h6"/></svg>',
  edit: '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M4 20h4L19 9l-4-4L4 16z"/><path d="M13.5 6.5l4 4"/></svg>',
  search: '<svg viewBox="0 0 24 24" aria-hidden="true"><circle cx="11" cy="11" r="6.5"/><path d="M20 20l-4.2-4.2"/></svg>',
  globe: '<svg viewBox="0 0 24 24" aria-hidden="true"><circle cx="12" cy="12" r="8.5"/><path d="M3.5 12h17M12 3.5c2.5 2.6 2.5 14.4 0 17M12 3.5c-2.5 2.6-2.5 14.4 0 17"/></svg>',
  bot: '<svg viewBox="0 0 24 24" aria-hidden="true"><rect x="5" y="8" width="14" height="11" rx="3"/><path d="M12 8V5M9.5 13h.01M14.5 13h.01M9.5 16.5h5"/></svg>',
  list: '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M9 6h11M9 12h11M9 18h11"/><path d="M4 6l1 1 2-2M4 12l1 1 2-2M4 18l1 1 2-2"/></svg>',
  tool: '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M14.5 6.5a4 4 0 0 0-5.3 5.3L4 17l3 3 5.2-5.2a4 4 0 0 0 5.3-5.3l-2.5 2.5-2.5-2.5z"/></svg>',
  think: '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M12 3.5l1.8 4.7 4.7 1.8-4.7 1.8L12 16.5l-1.8-4.7L5.5 10l4.7-1.8z"/><path d="M18.5 15.5l.8 2 2 .8-2 .8-.8 2-.8-2-2-.8 2-.8z"/></svg>',
  check: '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M5 12.5l4.5 4.5L19 7.5"/></svg>',
  image: '<svg viewBox="0 0 24 24" aria-hidden="true"><rect x="3.5" y="4.5" width="17" height="15" rx="2"/><circle cx="9" cy="10" r="1.6"/><path d="M20.5 16l-5-5-8.5 8.5"/></svg>',
  branch: '<svg viewBox="0 0 24 24" aria-hidden="true"><circle cx="6" cy="5" r="2"/><circle cx="6" cy="19" r="2"/><circle cx="18" cy="7" r="2"/><path d="M6 7v10M18 9c0 5-6 4-12 8"/></svg>',
  alert: '<svg viewBox="0 0 24 24" aria-hidden="true"><circle cx="12" cy="12" r="8.5"/><path d="M12 7.5v5M12 16h.01"/></svg>',
  info: '<svg viewBox="0 0 24 24" aria-hidden="true"><circle cx="12" cy="12" r="8.5"/><path d="M12 11v5M12 8h.01"/></svg>',
  claw: '<svg class="claw-dot" viewBox="116 100 320 320" aria-hidden="true"><path d="M152 384C196 306 214 226 202 132"/>'
    + '<path d="M250 398C294 316 312 230 298 122"/><path d="M346 382C386 304 400 228 384 140"/></svg>',
};

// Photos trouvées par l'agent (outil Photo) : la vignette est dans le journal (adresse `data:`,
// glissée par le moteur), la grande version chez sa source, seules images extérieures que la
// Content-Security-Policy de la page autorise.
const knownPhotoHost = (host) => host === "upload.wikimedia.org" || host.endsWith(".staticflickr.com");

/** Une adresse https chez une source de photos connue (Flickr, Wikimedia), ou « ». */
export function photoLink(url) {
  try {
    const parsed = new URL(String(url || ""));
    return parsed.protocol === "https:" && knownPhotoHost(parsed.hostname) ? parsed.href : "";
  } catch {
    return "";
  }
}

/** Une vignette glissée dans le journal (image JPEG, PNG ou WebP en `data:`), ou « ». */
const thumbData = (text) => (/^data:image\/(?:jpeg|png|webp);base64,[A-Za-z0-9+/=]+$/.test(String(text || "")) ? text : "");

const plural = (count, word) => `${count} ${word}${count > 1 ? "s" : ""}`;

/** « +12 −3 », un compte nul en gris. */
const counts = (added, removed) => `<span class="add${added ? "" : " zero"}">+${added || 0}</span> `
  + `<span class="del${removed ? "" : " zero"}">−${removed || 0}</span>`;

function duration(ms) {
  const seconds = Math.round((ms || 0) / 1000);
  return seconds >= 60 ? `${Math.floor(seconds / 60)} min ${String(seconds % 60).padStart(2, "0")} s` : `${seconds} s`;
}


/** Le journal d'un commentaire du moteur : { run, page, events }, ou null. */
export async function decodeLog(body) {
  const text = String(body || "");
  if (!text.startsWith(LOG)) return null;
  const meta = JSON.parse(text.slice(LOG.length, text.indexOf("-->")).trim());
  const start = text.indexOf(DATA);
  const end = text.lastIndexOf("-->");
  if (start < 0 || end <= start) return null;
  const packed = text.slice(start + DATA.length, end).trim();
  const bytes = Uint8Array.from(atob(packed), (character) => character.charCodeAt(0));
  const stream = new Blob([bytes]).stream().pipeThrough(new DecompressionStream("gzip"));
  const data = JSON.parse(await new Response(stream).text());
  return { run: meta.run, page: meta.page, events: Array.isArray(data.events) ? data.events : [] };
}

/** Les champs d'une entrée que le moteur garde en JSON brut (outils qu'il ne résume pas). */
function rawInput(value) {
  try {
    const data = JSON.parse(String((value || {}).raw || "{}"));
    return data && typeof data === "object" ? data : {};
  } catch {
    return {};
  }
}

const quoted = (text) => (text ? `«\u00a0${text}\u00a0»` : "");

/** Une action de Claw en une phrase simple : icône, ce qu'il fait (au présent, comme on le
 * raconte) et sur quoi. « Modifie · styles.css », « Cherche · « total » dans app.js ». */
function describe(name, input) {
  const value = input || {};
  const raw = () => rawInput(value);
  switch (name) {
    case "bash": case "PowerShell": return ["terminal", "Lance une commande", value.desc || value.cmd || ""];
    case "read_file": return ["file", "Lit", value.path || ""];
    case "write_file": return ["filePlus", "Écrit", value.path || ""];
    case "edit_file": case "NotebookEdit": case "MultiEdit": return ["edit", "Modifie", value.path || ""];
    case "apply_patch": return ["edit", "Modifie", (value.paths || []).join(", ")];
    case "list_directory": return ["search", "Regarde le dossier", value.path || "."];
    case "move_file": return ["edit", "Déplace", `${value.from || ""} → ${value.to || ""}`];
    case "delete_file": return ["edit", "Supprime", value.path || ""];
    case "read_many_files": return ["file", "Lit", (value.paths || []).join(", ")];
    case "Wireframe": return ["list", "Dessine la maquette", value.title || ""];
    case "file_outline": return ["file", "Parcourt", value.path || ""];
    case "replace_in_files": return ["edit", value.preview ? "Prépare un remplacement" : "Remplace", `${quoted(value.pattern)} → ${quoted(value.replacement)}${value.glob ? ` dans ${value.glob}` : ""}`];
    case "run_tests": return ["terminal", "Lance les tests", value.cmd || ""];
    case "git": return ["terminal", "Consulte Git", `${value.action || ""}${value.target ? ` ${value.target}` : ""}`];
    case "git_commit": return ["terminal", "Enregistre un commit", value.message || ""];
    case "http_request": return ["globe", "Appelle une adresse", value.target || ""];
    case "glob_search": return ["search", "Cherche des fichiers", value.pattern || ""];
    case "grep_search": return ["search", "Cherche", `${quoted(value.pattern)}${value.path ? ` dans ${value.path}` : ""}`];
    case "WebFetch": return ["globe", "Lit la page", value.target || ""];
    case "ViewPage": return ["globe", "Ouvre la page", value.target || ""];
    case "WebSearch": return ["globe", "Cherche sur le web", value.target || ""];
    case "Agent": return ["bot", "Confie une tâche à un sous-agent", value.desc || ""];
    case "Photo": return ["image", "Cherche des photos", quoted(value.search)];
    case "Icon": return ["tool", "Prend des icônes", (value.names || raw().names || []).join(", ")];
    case "Font": {
      const font = { ...raw(), ...value };
      return ["tool", "Choisit les polices", [font.display, font.text, font.mono].filter(Boolean).join(", ") || quoted(font.search)];
    }
    case "Ornament": return ["tool", "Dessine un ornement", value.kind || raw().kind || ""];
    case "Component": return ["tool", "Prend des composants", (value.names || raw().names || []).join(", ")];
    case "AppIcon": return ["image", "Crée les icônes de l'appli", value.target || ""];
    case "Screenshot": {
      // Le site publié, ou la copie de travail de la session (fichier, serveur local) : pas la
      // même version de l'appli (vu le 21 octobre 2026, l'ancienne prise pour la nouvelle).
      const target = String(value.target || "");
      const local = !/^https?:\/\//.test(target) || /^https?:\/\/(?:localhost|127\.0\.0\.1|0\.0\.0\.0)(?:[:/]|$)/.test(target);
      return ["image", local ? "Regarde la page" : "Regarde le site en ligne", local ? "" : target.replace(/^https?:\/\//, "")];
    }
    case "Browser": {
      const action = raw();
      const steps = Array.isArray(action.steps) ? action.steps.length : 0;
      return ["globe", "Essaie la page", steps ? `${steps} étape${steps > 1 ? "s" : ""}` : ""];
    }
    case "LookAtImage": return ["image", "Regarde une capture de près", ""];
    case "KillShell": return ["terminal", "Arrête une commande", ""];
    case "BashOutput": return ["terminal", "Relit la sortie d'une commande", ""];
    case "FinalAnswer": return ["check", "Donne sa réponse", ""];
    case "ExitPlanMode": return ["list", "Présente son plan", ""];
    case "Remember": return ["think", "Prend une note", ""];
    case "AskUserQuestion": return ["list", "Vous pose des questions", (value.q || []).join(" · ")];
    case "TodoWrite": {
      const todos = value.todos || [];
      return ["list", "Met à jour ses tâches", `${todos.filter((todo) => todo[0] === "completed").length} sur ${todos.length} faites`];
    }
    default: {
      // Outils MCP du moteur : « GitHub : Lire une issue #12 », titre posé par le journal.
      if (value.title && String(name).startsWith("mcp__")) {
        const [group, ...rest] = String(value.title).split(" : ");
        return [group === "GitHub" ? "branch" : group === "Documents" ? "file" : "bot", group, rest.join(" : ")];
      }
      return ["tool", "Utilise l'outil", String(name || "")];
    }
  }
}

/** Genre d'une action, d'après son icône : la couleur de sa barre sur la frise du passage (la même
 * que sa pastille) et le filtre du journal qui la montre. */
const CATEGORY = { edit: "edit", filePlus: "edit", file: "read", search: "read", terminal: "run", image: "web", globe: "web" };
const categoryOf = (event) => CATEGORY[describe(event.n, event.i)[0]] || "tool";

/** Filtres du journal, dans l'ordre des puces (« say » : ce que Claw dit). */
const CATEGORIES = [["say", "Messages"], ["edit", "Modifications"], ["run", "Commandes"], ["read", "Lectures"], ["web", "Web"], ["tool", "Outils"]];

function todosHtml(todos) {
  return `<div class="todos">${(todos || []).map(([status, text]) => `
    <div class="todo ${status === "completed" ? "done" : status === "in_progress" ? "doing" : ""}">
      <span class="box">${status === "completed" ? ICONS.check : ""}</span><span>${escapeHtml(text)}</span>
    </div>`).join("")}</div>`;
}

// Mots d'une ligne pour la comparaison fine : lettres et chiffres d'un bloc, espaces d'un bloc,
// chaque autre signe seul.
const WORD = /[\p{L}\p{N}_]+|\s+|[^\p{L}\p{N}_\s]/gu;

/** Ce qui change entre une ligne effacée et la ligne écrite à sa place : plages de caractères
 * `[début, fin)` dans chacune, ou `null` quand les deux lignes ont trop peu en commun pour que
 * le détail aide (ligne réécrite). Plus longue suite commune de mots ; un blanc seul entre deux
 * changements est pris avec eux. */
export function changedRanges(before, after) {
  const a = before.match(WORD) || [];
  const b = after.match(WORD) || [];
  if (!a.length || !b.length || a.length * b.length > 250000) return null;
  const width = b.length + 1;
  const table = new Uint16Array((a.length + 1) * width);
  for (let i = a.length - 1; i >= 0; i -= 1) {
    for (let j = b.length - 1; j >= 0; j -= 1) {
      table[i * width + j] = a[i] === b[j]
        ? table[(i + 1) * width + j + 1] + 1
        : Math.max(table[(i + 1) * width + j], table[i * width + j + 1]);
    }
  }
  const keptA = new Array(a.length).fill(false);
  const keptB = new Array(b.length).fill(false);
  for (let i = 0, j = 0; i < a.length && j < b.length;) {
    if (a[i] === b[j]) { keptA[i] = keptB[j] = true; i += 1; j += 1; }
    else if (table[(i + 1) * width + j] >= table[i * width + j + 1]) i += 1;
    else j += 1;
  }
  const visible = (list, kept) => list.reduce((sum, word, k) => sum + (kept === null || kept[k] ? word.replace(/\s/g, "").length : 0), 0);
  const common = visible(a, keptA);
  if (common * 2 < Math.max(visible(a, null), visible(b, null))) return null;
  const ranges = (list, kept) => {
    const out = [];
    let pos = 0;
    list.forEach((word, k) => {
      const end = pos + word.length;
      const blankBetween = kept[k] && /^\s+$/.test(word) && k > 0 && !kept[k - 1] && k + 1 < list.length && !kept[k + 1];
      if (!kept[k] || blankBetween) {
        if (out.length && out[out.length - 1][1] === pos) out[out.length - 1][1] = end;
        else out.push([pos, end]);
      }
      pos = end;
    });
    return out;
  };
  return [ranges(a, keptA), ranges(b, keptB)];
}

/** Entoure de `<mark>` les plages `[début, fin)` du texte d'un HTML coloré (le texte compte,
 * pas les balises ; une entité vaut un caractère). Une plage qui traverse une balise de
 * couleur est fermée avant et rouverte après, pour garder un HTML bien imbriqué. */
export function markRanges(html, ranges) {
  if (!ranges || !ranges.length) return html;
  let out = "";
  let pos = 0;
  let next = 0;
  let open = false;
  for (let i = 0; i < html.length;) {
    while (next < ranges.length && pos >= ranges[next][1]) {
      if (open) { out += "</mark>"; open = false; }
      next += 1;
    }
    if (html[i] === "<") {
      const end = html.indexOf(">", i) + 1 || html.length;
      out += open ? `</mark>${html.slice(i, end)}<mark>` : html.slice(i, end);
      i = end;
      continue;
    }
    if (!open && next < ranges.length && pos >= ranges[next][0]) { out += "<mark>"; open = true; }
    const length = html[i] === "&" ? html.indexOf(";", i) - i + 1 || 1 : 1;
    out += html.slice(i, i + length);
    i += length;
    pos += 1;
  }
  if (open) out += "</mark>";
  return out.replace(/<mark><\/mark>/g, "");
}

/** Lignes de différence (`+`, `-`, ` `), numérotées depuis le début de chaque passage, colorées
 * dans le langage du fichier (ligne par ligne). Une ligne effacée suivie de sa remplaçante
 * (la n-ième effacée avec la n-ième écrite) n'a de surligné que ce qui change : un mot
 * remplacé ne fait pas croire à une ligne réécrite. */
export function diffRows(hunks, language = "") {
  const rows = [];
  hunks.forEach(({ oldStart, newStart, lines }, index) => {
    if (index) rows.push('<div class="dl gap">⋯</div>');
    let oldNo = oldStart || 1;
    let newNo = newStart || 1;
    const marks = new Map();
    for (let k = 0; k < lines.length;) {
      if (lines[k][0] !== "-") { k += 1; continue; }
      const dels = [];
      const adds = [];
      while (k < lines.length && (lines[k][0] === "-" || lines[k][0] === "\\")) {
        if (lines[k][0] === "-") dels.push(k);
        k += 1;
      }
      while (k < lines.length && (lines[k][0] === "+" || lines[k][0] === "\\")) {
        if (lines[k][0] === "+") adds.push(k);
        k += 1;
      }
      for (let n = 0; n < Math.min(dels.length, adds.length); n += 1) {
        const ranges = changedRanges(lines[dels[n]].slice(1), lines[adds[n]].slice(1));
        if (ranges) { marks.set(dels[n], ranges[0]); marks.set(adds[n], ranges[1]); }
      }
    }
    lines.forEach((line, k) => {
      const kind = line[0];
      const text = markRanges(highlight(line.slice(1), language), marks.get(k)) || " ";
      if (kind === "+") rows.push(`<div class="dl add"><span class="no">${newNo++}</span><span class="tx">${text}</span></div>`);
      else if (kind === "-") rows.push(`<div class="dl del"><span class="no">${oldNo++}</span><span class="tx">${text}</span></div>`);
      else if (kind === "\\") return;
      else {
        rows.push(`<div class="dl"><span class="no">${newNo}</span><span class="tx">${text}</span></div>`);
        oldNo += 1;
        newNo += 1;
      }
    });
  });
  return rows;
}

/** Différence du journal : lignes `@@ ancien nouveau` puis les lignes du passage. */
function journalDiffHtml(lines, more, language) {
  const hunks = [];
  for (const line of lines || []) {
    const header = line.match(/^@@ (\d+) (\d+)/);
    if (header) hunks.push({ oldStart: Number(header[1]), newStart: Number(header[2]), lines: [] });
    else if (hunks.length) hunks[hunks.length - 1].lines.push(line);
  }
  const rows = diffRows(hunks, language);
  if (more) rows.push(`<div class="dl gap">… ${plural(more, "ligne")} de plus</div>`);
  return `<div class="diff">${rows.join("")}</div>`;
}

/** Différence au format unifié (comparaison GitHub). */
function unifiedDiffHtml(patch, language) {
  const hunks = [];
  for (const line of String(patch || "").split("\n")) {
    const header = line.match(/^@@ -(\d+)(?:,\d+)? \+(\d+)/);
    if (header) hunks.push({ oldStart: Number(header[1]), newStart: Number(header[2]), lines: [] });
    else if (hunks.length) hunks[hunks.length - 1].lines.push(line);
  }
  return `<div class="diff">${diffRows(hunks, language).join("")}</div>`;
}

/** Chemin d'une capture rangée sur la branche des fichiers du moteur, ou « ». */
const shotPath = (path) => (/^uploads\/session-\d+\/[A-Za-z0-9._-]+\.jpg$/.test(String(path || "")) ? path : "");

/** Écran d'une capture, en français : « phone 390x844 » devient « téléphone 390 x 844 » ;
 * la suite de la page (« , from 2532px ») et les fenêtres (« , window opened by button "Je
 * conduis" ») le disent aussi. */
export function screenName(screen) {
  const [device, ...rest] = String(screen || "").split(", ");
  const [kind, size] = device.split(" ");
  const names = { phone: "téléphone", mobile: "téléphone", tablet: "tablette", desktop: "ordinateur" };
  const name = [names[kind] || kind, (size || "").replace("x", " x ")].filter(Boolean).join(" ");
  const part = rest.join(", ");
  const from = part.match(/^from (\d+)px$/);
  if (from) return `${name}, suite de la page (à partir de ${from[1]} px)`;
  const opener = part.match(/^window opened by .*?"([^"]+)"$/);
  if (opener) return `${name}, fenêtre « ${opener[1]} »`;
  if (/^menu opened by /.test(part)) return `${name}, menu ouvert`;
  const tab = part.match(/^tab .*?"([^"]+)"$/);
  if (tab) return `${name}, onglet « ${tab[1]} »`;
  return part ? `${name}, ${part}` : name;
}

/** Vignettes des captures d'écran : un toucher ouvre la capture entière, avec les problèmes
 * relevés dessus. */
function shotsHtml(shots) {
  const pills = shots.map((shot) => {
    const thumb = thumbData(shot.t);
    const path = shotPath(shot.p);
    if (!thumb && !path) return "";
    const label = `Capture ${screenName(shot.screen)}${shot.n ? ` · ${plural(shot.n, "problème")}` : ""}`;
    const strings = (list) => (Array.isArray(list) ? list.filter((item) => typeof item === "string") : []);
    // En français quand le moteur les a traduits (`pf`), sinon tels que Claw les a écrits.
    const english = strings(shot.pb);
    const french = strings(shot.pf);
    const translated = french.length > 0 && french.length === english.length;
    const problems = translated ? french : english;
    return `<button type="button" class="photo-pill shot" data-shot="${escapeHtml(path)}" data-thumb="${escapeHtml(thumb)}"`
      + ` data-problems="${escapeHtml(JSON.stringify(problems))}" data-count="${Number(shot.n) || 0}"`
      + ` data-french="${translated ? "1" : ""}"`
      + ` data-label="${escapeHtml(label)}" title="${escapeHtml(label)}" aria-label="${escapeHtml(`Voir la ${label.toLowerCase()}`)}">`
      + (thumb ? `<img src="${escapeHtml(thumb)}" alt="">` : ICONS.image)
      + (shot.n ? `<span class="shot-flag">${shot.n}</span>` : "")
      + "</button>";
  }).join("");
  return pills ? `<div class="photo-pills">${pills}</div>` : "";
}

/** Ce que la vue en grand d'une photo peut montrer : sa grande version, sinon sa vignette. */
export const photoView = (url) => photoLink(url) || thumbData(url);

/** Vignettes des photos trouvées par l'agent : toujours visibles, d'un toucher en grand. */
function photosHtml(photos) {
  const pills = photos.map((photo) => {
    const thumb = thumbData(photo.t);
    if (!thumb) return "";
    const credit = [photo.title, photo.by && `par ${photo.by}`, photo.lic].filter(Boolean).join(" · ");
    return `<button type="button" class="photo-pill" data-photo="${escapeHtml(photoLink(photo.u) || thumb)}"`
      + ` data-credit="${escapeHtml(credit)}" data-source="${escapeHtml(String(photo.src || "").startsWith("https://") ? photo.src : "")}"`
      + ` title="${escapeHtml(credit)}" aria-label="${escapeHtml(`Voir la photo ${photo.title || ""}`.trim())}">`
      + `<img src="${escapeHtml(thumb)}" alt="" loading="lazy" referrerpolicy="no-referrer"></button>`;
  }).join("");
  return pills ? `<div class="photo-pills">${pills}</div>` : "";
}

function toolHtml(event, key, openTodo) {
  const [icon, verb, target] = describe(event.n, event.i);
  const result = event.r;
  const input = event.i || {};
  let side = "";
  let body = "";
  if (event.n === "bash" || event.n === "PowerShell") {
    const output = result ? [result.out, result.err].filter(Boolean).join("\n") : "";
    body = `<div class="io">
      <span class="io-k">Entrée</span><pre class="io-v">${highlight(input.cmd || "", "sh")}</pre>
      ${result ? `<span class="io-k">Sortie</span><pre class="io-v">${output ? escapeHtml(output) : '<span class="none">(aucune sortie)</span>'}</pre>` : ""}
      ${result && result.code ? `<span class="io-note">${escapeHtml(result.code)}</span>` : ""}
      ${result && result.bg ? '<span class="io-note">Lancée en arrière-plan</span>' : ""}
    </div>`;
    if (result && result.code) side = `<span class="del">${escapeHtml(result.code)}</span>`;
  } else if (["write_file", "edit_file", "MultiEdit"].includes(event.n) && result && Array.isArray(result.lines)) {
    side = counts(result.add, result.del);
    body = journalDiffHtml(result.lines, result.more, languageOf(input.path || input.file_path || target));
  } else if (["apply_patch", "delete_file", "replace_in_files"].includes(event.n) && result && Array.isArray(result.files)) {
    // Chaque fichier du correctif : ce qui lui arrive, puis ses lignes changées.
    const words = { create: "Créé", update: "Modifié", delete: "Supprimé", move: "Déplacé" };
    side = counts(result.files.reduce((sum, file) => sum + (file.add || 0), 0),
      result.files.reduce((sum, file) => sum + (file.del || 0), 0));
    body = result.files.map((file) => {
      const from = file.from ? `${escapeHtml(file.from)} → ` : "";
      const head = `<div class="io-note">${escapeHtml(words[file.kind] || "Modifié")} : ${from}${escapeHtml(file.path || "")}</div>`;
      const lines = file.kind !== "delete" && Array.isArray(file.lines) && file.lines.length
        ? journalDiffHtml(file.lines, file.more, languageOf(file.path || "")) : "";
      return head + lines;
    }).join("");
  } else if (event.n === "move_file" && result && result.to) {
    body = `<div class="io-note">${escapeHtml(result.from || "")} → ${escapeHtml(result.to)}</div>`;
  } else if (event.n === "read_file" && result && result.count) {
    side = plural(result.count, "ligne");
  } else if (event.n === "TodoWrite") {
    body = todosHtml(input.todos);
  } else if (result && result.out) {
    body = `<pre class="io-v">${escapeHtml(result.out)}</pre>`;
    if (result.count !== undefined && result.count !== null) side = String(result.count);
  } else if (input.raw) {
    body = `<pre class="io-v">${escapeHtml(input.raw)}</pre>`;
  }
  const state = !result ? " running" : event.e ? " failed" : "";
  const photos = event.n === "Photo" && result && Array.isArray(result.photos) ? result.photos : null;
  if (photos) {
    side = plural(photos.length, "photo");
    body = "";
  }
  const shots = event.n === "Screenshot" && result && Array.isArray(result.shots) ? result.shots : null;
  if (shots) {
    // Sans capture gardée (passages d'avant le 25 octobre 2026, aperçus trop lourds), le dire
    // plutôt qu'un « 0 capture » sans image.
    side = result.found ? `<span class="del">${plural(result.found, "problème")}</span>`
      : shots.length ? plural(shots.length, "capture") : "captures non gardées";
    body = "";
  }
  const summary = `${ICONS[icon] || ICONS.tool}<span class="verb">${escapeHtml(verb)}</span>`
    + `<span class="target">${escapeHtml(target)}</span>`
    + `<span class="side">${!result ? '<span class="spinner" aria-label="en cours"></span>' : side}</span>`;
  // `data-kind` : la couleur de la pastille de l'action (fichier, commande, capture…) ;
  // `data-cat` : son filtre.
  const kind = `data-kind="${icon}" data-cat="${CATEGORY[icon] || "tool"}"`;
  if (photos) return `<div class="ev tool photos${state}" ${kind}><div class="ev-line">${summary}</div>${photosHtml(photos)}</div>`;
  if (shots) return `<div class="ev tool photos${state}" ${kind}><div class="ev-line">${summary}</div>${shotsHtml(shots)}</div>`;
  if (!body) return `<div class="ev tool${state}" ${kind}><div class="ev-line">${summary}</div></div>`;
  return `<details class="ev tool${state}" ${kind} data-key="${key}"${openTodo ? " open" : ""}><summary>${summary}</summary><div class="ev-body">${body}</div></details>`;
}

/** Une ligne du journal (le modèle, les requêtes et la durée vont dans la fiche du passage, à son
 * pied). `context` garde, d'une ligne à la suivante, le nombre de défauts de la
 * dernière vérification de la page : une vérification qui ne change rien ne se répète pas
 * (« Claw vérifie la page après la modification » revenait 30 fois de suite, à l'identique). */
function eventHtml(event, key, lastTodo, context = {}) {
  switch (event.k) {
    case "think": {
      // Le modèle réfléchit souvent en anglais : la ligne le dit, le texte reste à déplier.
      const text = String(event.t || "");
      const english = looksEnglish(text);
      return `<details class="ev think" data-key="${key}" data-kind="think" data-cat="say"><summary>${ICONS.think}<span class="verb">Réfléchit</span>`
        + `<span class="target">${english ? "" : escapeHtml(text.replace(/\s+/g, " ").slice(0, 120))}</span>`
        + `${english ? '<span class="side">en anglais</span>' : ""}</summary>`
        + `<div class="ev-body think-text">${escapeHtml(text)}</div></details>`;
    }
    case "text": {
      const text = withoutCallMarkup(event.t);
      if (!text) return "";
      // Ce que l'agent se dit en anglais en travaillant : une ligne, le texte à déplier.
      if (looksEnglish(text)) {
        return `<details class="ev aside" data-key="${key}" data-kind="think" data-cat="say"><summary>${ICONS.bot}<span class="verb">Note de l'agent</span>`
          + `<span class="target"></span><span class="side">en anglais</span></summary><div class="ev-body ev text">${renderMarkdown(text)}</div></details>`;
      }
      return `<div class="ev say" data-cat="say">${ICONS.claw}<div class="ev-bubble">${renderMarkdown(text)}</div></div>`;
    }
    // Réponse qu'un contrôle de Claw lui a fait reprendre (marquée par le moteur).
    case "draft":
      return draftHtml(event, key);
    case "notice": {
      const notice = plainNotice(event.t);
      if (notice.kind === "check") {
        const same = context.check === notice.left && !notice.changed;
        context.check = notice.left;
        if (same) return "";
      }
      const icon = notice.kind === "check" ? (notice.left ? ICONS.alert : ICONS.check) : notice.kind === "warn" ? ICONS.alert : ICONS.info;
      const tone = notice.kind === "check" ? `check${notice.left ? "" : " clean"}` : notice.kind;
      return `<div class="ev notice ${tone}" data-cat="say">${icon}<span>${escapeHtml(notice.text)}</span></div>`;
    }
    case "tool":
      return toolHtml(event, key, lastTodo);
    default:
      return "";
  }
}

/** Un appel d'outil écrit en texte par le modèle (`<tool_call>`, `<function=…>`, `<invoke …>`),
 * jamais exécuté : retiré du texte montré. */
export function withoutCallMarkup(text) {
  return String(text || "")
    .replace(/<tool_call>[\s\S]*?(?:<\/tool_call>|$)/g, "")
    .replace(/<function_calls>[\s\S]*?(?:<\/function_calls>|$)/g, "")
    .replace(/<invoke\b[\s\S]*?(?:<\/invoke>|$)/g, "")
    .replace(/<function=[\s\S]*?(?:<\/function>|$)/g, "")
    .trim();
}

/** Une réponse que Claw a fait reprendre, repliée comme dans son interface locale (la remarque
 * qui suit dit pourquoi). */
function draftHtml(event, key) {
  return `<details class="ev draft" data-key="${key}" data-cat="say"><summary>${ICONS.edit}<span class="verb">Réponse provisoire</span>`
    + `<span class="target">reprise par Claw</span></summary>`
    + `<div class="ev-body ev text">${renderMarkdown(withoutCallMarkup(event.t))}</div></details>`;
}

/** Les réponses que Claw a fait reprendre, dans un journal d'avant le 21 octobre 2026 (le moteur
 * les marque depuis, `draft`) : un rapport (« **Résultat** »), ou un long texte suivi d'une
 * remarque de ses vérifications, quand un autre texte vient après. Le journal les replie. */
function draftAnswers(events, repeated) {
  const drafts = new Set();
  if (events.some((event) => event.k === "init" && event.drafts)) return drafts;
  const lastText = events.map((event, index) => event.k === "text" && !repeated.has(index)).lastIndexOf(true);
  const answered = repeated.size > 0;
  events.forEach((event, index) => {
    if (event.k !== "text" || repeated.has(index) || (index === lastText && !answered)) return;
    const text = String(event.t || "").trim();
    const next = events.slice(index + 1).find((later) => later.k !== "req" && later.k !== "think");
    const later = events.slice(index + 1).some((other) => other.k === "text");
    if ((later || answered) && (/^\*\*Résultat/.test(text) || (next && next.k === "notice" && text.length > 300))) drafts.add(index);
  });
  return drafts;
}

/** Les textes de la fin du passage (après sa dernière action) que la réponse publiée reprend :
 * le journal ne les répète pas. */
function repeatedInAnswer(events, answer) {
  const flat = (text) => String(text || "").replace(/\s+/g, " ").trim();
  const said = flat(answer);
  const repeated = new Set();
  if (!said) return repeated;
  const lastTool = events.map((event) => event.k === "tool").lastIndexOf(true);
  events.forEach((event, index) => {
    if (index > lastTool && event.k === "text" && flat(event.t) && said.includes(flat(event.t))) repeated.add(index);
  });
  return repeated;
}

/** Ce qu'une action a donné, en quelques signes, pour la liste du bloc « Claw travaille ». */
function quickSide(event) {
  const result = event.r;
  if (!result) return '<span class="spinner" aria-label="en cours"></span>';
  if (["write_file", "edit_file", "MultiEdit"].includes(event.n) && (result.add || result.del)) return counts(result.add, result.del);
  if (Array.isArray(result.files)) {
    return counts(result.files.reduce((sum, file) => sum + (file.add || 0), 0), result.files.reduce((sum, file) => sum + (file.del || 0), 0));
  }
  if (event.n === "read_file" && result.count) return plural(result.count, "ligne");
  if ((event.n === "bash" || event.n === "PowerShell") && result.code) return `<span class="del">${escapeHtml(result.code)}</span>`;
  if (event.n === "Screenshot" && result.found) return `<span class="del">${plural(result.found, "problème")}</span>`;
  if (event.n === "Photo" && Array.isArray(result.photos)) return plural(result.photos.length, "photo");
  return "";
}

/** Les dernières actions d'un passage en cours, la plus récente d'abord : icône, verbe, cible et
 * ce qu'elle a donné. Celles d'après `seen` (rang dans le journal) viennent d'arriver : elles
 * entrent en glissant, l'une après l'autre. */
export function recentActions(events, count = 4, seen = Infinity) {
  return events.map((event, index) => ({ event, index }))
    .filter(({ event }) => event.k === "tool" && event.n !== "TodoWrite")
    .slice(-count).reverse()
    .map(({ event, index }) => {
      const [icon, verb, target] = describe(event.n, event.i);
      const state = !event.r ? " running" : event.e ? " failed" : "";
      return `<li class="lr-action${state}${index >= seen ? " arrived" : ""}">${ICONS[icon] || ICONS.tool}`
        + `<span class="verb">${escapeHtml(verb)}</span><span class="target">${escapeHtml(target)}</span>`
        + `<span class="side">${quickSide(event)}</span></li>`;
    }).join("");
}

/** Étapes d'un journal montrées d'emblée ; les précédentes se déplient. */
const RECENT_STEPS = 30;

/** Pastille d'état d'un passage : en cours, fini, arrêté, erreur. */
const STATE_ICONS = {
  running: '<span class="spinner" aria-label="en cours"></span>',
  stopping: '<span class="spinner" aria-label="arrêt en cours"></span>',
  done: ICONS.check,
  stopped: '<svg viewBox="0 0 24 24" aria-hidden="true"><rect x="7" y="7" width="10" height="10" rx="2"/></svg>',
  failed: '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M12 7v6M12 16.5h.01"/></svg>',
  unfinished: '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M8 12h8"/></svg>',
};

/** Marque la ligne d'une étape : son rang, et « pending » tant que l'appli ne l'a pas encore
 * fait paraître (les étapes d'un passage en cours arrivent par paquets, montrées une par une). */
function marked(html, index, pending) {
  return html.replace(/class="/, `data-step="${index}" class="${pending ? "pending " : ""}`);
}

const THINKING = "claw-web.journal.reflexions";

/** Les réflexions de l'agent sont montrées dans le journal (choix gardé sur l'appareil). */
export function thinkingShown() {
  try {
    return localStorage.getItem(THINKING) === "1";
  } catch {
    return false;
  }
}

/** Montre ou cache les réflexions de l'agent dans tous les journaux de la page. */
export function toggleThinking() {
  const shown = !thinkingShown();
  try {
    localStorage.setItem(THINKING, shown ? "1" : "0");
  } catch {
    // Navigation privée : le choix vaut pour cette page seulement.
  }
  for (const transcript of document.querySelectorAll(".transcript")) transcript.classList.toggle("thinking", shown);
  for (const button of document.querySelectorAll("[data-journal-thinking]")) {
    button.setAttribute("aria-pressed", String(shown));
    button.querySelector("span").textContent = shown ? "Cacher ses réflexions" : "Voir ses réflexions";
  }
}

/** Ce que le passage a fait : fichiers écrits ou modifiés (chemin : lignes ajoutées, retirées,
 * créé), commandes (et en échec), captures (et défauts de la dernière), lectures. */
function passageFacts(events) {
  const files = new Map();
  const touch = (path, add, del, created) => {
    if (!path) return;
    const file = files.get(path) || { add: 0, del: 0, created: false };
    file.add += add || 0;
    file.del += del || 0;
    file.created = file.created || (created && !files.has(path));
    files.set(path, file);
  };
  let commands = 0;
  let failedCommands = 0;
  let shots = 0;
  let lastFound = null;
  let reads = 0;
  for (const event of events) {
    if (event.k !== "tool") continue;
    const result = event.r || {};
    const input = event.i || {};
    if (["write_file", "edit_file", "MultiEdit", "NotebookEdit"].includes(event.n)) {
      if (result.add || result.del || event.n === "write_file") touch(input.path, result.add, result.del, event.n === "write_file");
    } else if (Array.isArray(result.files)) {
      for (const file of result.files) touch(file.path, file.add, file.del, file.kind === "create");
    } else if (["bash", "PowerShell", "run_tests"].includes(event.n)) {
      commands += 1;
      if (result.code || event.e) failedCommands += 1;
    } else if (event.n === "Screenshot") {
      shots += 1;
      if (typeof result.found === "number") lastFound = result.found;
    } else if (["read_file", "read_many_files", "grep_search", "glob_search", "file_outline"].includes(event.n)) {
      reads += 1;
    }
  }
  return { files, commands, failedCommands, shots, lastFound, reads };
}

/** Ce que le passage a fait, d'un coup d'œil, avant le détail : les fichiers écrits ou modifiés
 * (avec leurs lignes), les commandes, les captures et ce qu'elles ont trouvé, les échecs. */
function digestHtml(events) {
  const { files, commands, failedCommands, shots, lastFound, reads } = passageFacts(events);
  const thoughts = events.filter((event) => event.k === "think").length;
  const toggle = thoughts ? `<button type="button" class="dg-toggle" data-journal-thinking aria-pressed="${thinkingShown()}">${ICONS.think}<span>${thinkingShown() ? "Cacher ses réflexions" : "Voir ses réflexions"}</span></button>` : "";
  if (!files.size && !commands && !shots && !reads) return toggle ? `<div class="digest">${toggle}</div>` : "";
  const stat = (count, one, many, extra = "") => (count ? `<span class="dg-stat${extra}"><strong>${count}</strong>${count > 1 ? many : one}</span>` : "");
  const sorted = [...files.entries()].sort((a, b) => (b[1].add + b[1].del) - (a[1].add + a[1].del));
  const shown = sorted.slice(0, 6);
  return `<div class="digest">
    <div class="dg-stats">
      ${stat(files.size, "fichier modifié", "fichiers modifiés")}
      ${stat(commands, "commande", "commandes")}
      ${stat(shots, "capture", "captures")}
      ${stat(reads, "lecture", "lectures")}
    </div>
    ${failedCommands || lastFound ? `<p class="dg-alert">${ICONS.alert}<span>${[
      failedCommands ? `${plural(failedCommands, "commande")} en échec` : "",
      lastFound ? `la dernière capture montre encore ${plural(lastFound, "problème")}` : "",
    ].filter(Boolean).join(" · ")}</span></p>` : ""}
    ${toggle}
    ${shown.length ? `<ul class="dg-files">${shown.map(([path, file]) => `
      <li>${file.created ? ICONS.filePlus : ICONS.edit}<span class="path">${escapeHtml(path)}</span>
        ${file.created ? '<span class="dg-new">nouveau</span>' : ""}<span class="side">${counts(file.add, file.del)}</span></li>`).join("")}
      ${sorted.length > shown.length ? `<li class="dg-more">et ${plural(sorted.length - shown.length, "autre fichier", "autres fichiers")}</li>` : ""}
    </ul>` : ""}
  </div>`;
}

/** Les tâches du passage, dans l'ordre où Claw les a menées : chaque fois que sa liste de tâches
 * (TodoWrite) en met une autre « en cours », un nouveau groupe commence. Avant la première : la
 * préparation ; après la dernière : la vérification et la réponse. Rien sans liste de tâches. */
function taskGroups(events) {
  const lists = events.filter((event) => event.k === "tool" && event.n === "TodoWrite" && Array.isArray((event.i || {}).todos));
  if (!lists.length) return [];
  const final = lists[lists.length - 1].i.todos;
  const groups = [{ title: "Préparation", start: 0, status: "done", position: -1 }];
  events.forEach((event, index) => {
    if (!(event.k === "tool" && event.n === "TodoWrite" && Array.isArray((event.i || {}).todos))) return;
    const todos = event.i.todos;
    const position = todos.findIndex(([status]) => status === "in_progress");
    const current = groups[groups.length - 1];
    if (position < 0) {
      if (todos.every(([status]) => status === "completed") && current.title !== "Vérification et réponse") {
        groups.push({ title: "Vérification et réponse", start: index, status: "done", position: -1 });
      }
      return;
    }
    const title = todos[position][1];
    if (title === current.title) return;
    // Le texte d'une tâche change quand elle passe de « en cours » à « faite » : l'état final se
    // lit à sa place dans la dernière liste.
    const end = final[position];
    groups.push({ title, start: index, position, status: end ? (end[0] === "completed" ? "done" : end[0] === "in_progress" ? "doing" : "open") : "open" });
  });
  // Une tâche sans action (cochée aussitôt, ou reprise par la suivante) rejoint la précédente.
  const actions = (from, to) => events.slice(from, to).filter((event) => event.k === "tool" && event.n !== "TodoWrite").length;
  return groups.filter((group, at) => {
    const next = groups[at + 1];
    return at === 0 || actions(group.start, next ? next.start : events.length) > 0;
  });
}

/** Action réunie avec ses voisines de même verbe : lecture, recherche ou page web finie sans
 * erreur. Les modifications et les commandes gardent chacune leur ligne. */
function joinKey(event) {
  if (event.k !== "tool" || !event.r || event.e || event.n === "Photo" || event.n === "Screenshot") return "";
  const [icon, verb] = describe(event.n, event.i);
  return CATEGORY[icon] === "read" || CATEGORY[icon] === "web" ? verb : "";
}

/** Lectures (ou recherches) qui se suivent : une ligne, leur nombre à côté du verbe, les fichiers
 * à la suite ; elle se déplie sur chacune. */
function bundleHtml(steps, events, key) {
  const list = steps.map((step) => events[step.index]);
  const [icon, verb] = describe(list[0].n, list[0].i);
  const targets = list.map((event) => describe(event.n, event.i)[2]).filter(Boolean);
  const lines = list.reduce((sum, event) => sum + (event.n === "read_file" && event.r.count ? event.r.count : 0), 0);
  return `<details class="ev tool bundle" data-kind="${icon}" data-cat="${CATEGORY[icon]}" data-key="${key}"><summary>`
    + `${ICONS[icon] || ICONS.tool}<span class="verb">${escapeHtml(verb)}</span><span class="ev-count">${list.length}</span>`
    + `<span class="target">${escapeHtml(targets.join(", "))}</span><span class="side">${lines ? plural(lines, "ligne") : ""}</span></summary>`
    + `<div class="ev-body ev-bundle">${steps.map((step) => step.html).join("")}</div></details>`;
}

/** Les lignes d'un morceau du journal, les lectures qui se suivent réunies. */
function bundled(steps, events, key) {
  const rows = [];
  for (let at = 0; at < steps.length;) {
    let end = at + 1;
    while (steps[at].join && end < steps.length && steps[end].join === steps[at].join) end += 1;
    rows.push(end - at > 1 ? bundleHtml(steps.slice(at, end), events, `${key}:g${steps[at].index}`) : steps[at].html);
    at = end;
  }
  return rows;
}

/** Barres de la frise d'un passage, au plus : au-delà, une barre par paquet d'actions. */
const STRIP_BARS = 36;

/** Sous le titre replié d'un passage : sa frise (une barre par action, de la couleur de sa
 * pastille, rouge si elle a échoué) et son nombre d'actions, puis les fichiers modifiés (+ −) et
 * les commandes. Le résumé du journal déplié dit le reste. */
function statsHtml(events, tools, count) {
  const size = Math.ceil(tools.length / STRIP_BARS);
  const bars = [];
  for (let at = 0; at < tools.length; at += size) {
    const slice = tools.slice(at, at + size);
    // Le genre le plus fréquent du paquet.
    const tally = new Map();
    for (const event of slice) tally.set(categoryOf(event), (tally.get(categoryOf(event)) || 0) + 1);
    const cat = [...tally].sort((a, b) => b[1] - a[1])[0][0];
    bars.push(`<span data-cat="${cat}"${slice.some((event) => event.e) ? ' class="failed"' : ""}></span>`);
  }
  const { files, commands } = passageFacts(events);
  const added = [...files.values()].reduce((sum, file) => sum + file.add, 0);
  const removed = [...files.values()].reduce((sum, file) => sum + file.del, 0);
  const chips = [
    files.size ? `<span class="rs" data-cat="edit">${plural(files.size, "fichier")} ${counts(added, removed)}</span>` : "",
    commands ? `<span class="rs" data-cat="run">${plural(commands, "commande")}</span>` : "",
  ].join("");
  return `<span class="run-track"><span class="run-strip" aria-hidden="true">${bars.join("")}</span>${count}</span>`
    + (chips ? `<span class="run-stats">${chips}</span>` : "");
}

/** Puces de filtre d'un long journal : tout, ou un seul genre d'étape (avec leur nombre ; les
 * réflexions, cachées d'ordinaire, ne comptent pas). */
function filtersHtml(steps) {
  const tally = new Map();
  for (const step of steps) if (!step.think) tally.set(step.cat, (tally.get(step.cat) || 0) + 1);
  if (steps.length < 8 || tally.size < 2) return "";
  return '<div class="ev-filters" role="group" aria-label="Filtrer le journal">'
    + '<button type="button" class="ev-filter" data-filter="" aria-pressed="true">Tout</button>'
    + CATEGORIES.filter(([cat]) => tally.has(cat)).map(([cat, label]) => `<button type="button" class="ev-filter" data-filter="${cat}" data-cat="${cat}" aria-pressed="false">`
      + `<i aria-hidden="true"></i>${label}<span>${tally.get(cat)}</span></button>`).join("")
    + "</div>";
}

/** La fiche du passage, au pied du journal : modèle, requêtes, durée, coût, puis les requêtes
 * gratuites du jour. */
function footHtml(events) {
  const last = (kind) => [...events].reverse().find((event) => event.k === kind);
  const end = last("end");
  const init = last("init");
  const quota = last("quota");
  const model = modelName((end && end.model) || (init && init.model) || "");
  const facts = [
    model && ["Modèle", model],
    end && end.turns && ["Requêtes", String(end.turns)],
    end && end.ms && ["Durée", duration(end.ms)],
    end && end.cost && ["Coût", String(end.cost)],
  ].filter(Boolean);
  const free = quota
    ? `<p class="foot-quota">Requêtes gratuites : ${quota.run !== null && quota.run !== undefined ? `${plural(Number(quota.run), "requête")} pour ce passage · ` : ""}`
      + `${Number(quota.used)}/${Number(quota.limit)} utilisées aujourd'hui (${Number(quota.remaining)} restantes)</p>`
    : "";
  if (!facts.length && !free) return "";
  return `<footer class="ev-foot">${facts.map(([label, value]) => `<span class="fact${label === "Modèle" ? " wide" : ""}"><small>${label}</small>${escapeHtml(value)}</span>`).join("")}${free}</footer>`;
}

/** Le journal d'un passage. Replié : l'état du passage, `title`, et (`stats`) sa frise et ses
 * chiffres. Déplié : ce qu'il a fait, les filtres d'un long journal, le détail rangé par tâches
 * (une phrase simple et une pastille de couleur par action, les lectures qui se suivent réunies,
 * les messages de Claw en bulles), la fiche du passage au pied. `answer` : la réponse publiée du
 * passage, que le journal ne répète pas. */
export function transcriptHtml(events, { key, live, answer = "", title = "Journal du passage", kind = "", shown = events.length, stats = true }) {
  const lastTodo = events.map((event) => event.k === "tool" && event.n === "TodoWrite").lastIndexOf(true);
  const repeated = repeatedInAnswer(events, answer);
  const drafts = draftAnswers(events, repeated);
  const groups = taskGroups(events);
  const grouped = groups.length > 1;
  const context = {};
  // Une étape par événement montré (`null` sinon), au rang de l'événement.
  const steps = events.map((event, index) => {
    if (repeated.has(index)) return null;
    // Rangé par tâches, le journal n'a plus besoin des mises à jour de la liste : chaque groupe
    // dit sa tâche et son état.
    if (grouped && event.k === "tool" && event.n === "TodoWrite") return null;
    const html = drafts.has(index) ? draftHtml(event, `${key}:${index}`)
      : eventHtml(event, `${key}:${index}`, live && index === lastTodo, context);
    if (!html) return null;
    return {
      index,
      html: marked(html, index, index >= shown),
      cat: event.k === "tool" ? categoryOf(event) : "say",
      think: event.k === "think",
      join: index < shown ? joinKey(event) : "",
    };
  });
  const tools = events.filter((event, index) => event.k === "tool" && index < shown);
  const digest = digestHtml(events.slice(0, shown));
  let body;
  if (grouped) {
    body = groups.map((group, at) => {
      const end = at + 1 < groups.length ? groups[at + 1].start : events.length;
      const inside = bundled(steps.slice(group.start, end).filter(Boolean), events, key);
      const actions = events.slice(group.start, end).filter((event) => event.k === "tool" && event.n !== "TodoWrite").length;
      const last = at === groups.length - 1;
      // Un passage fini n'a plus de tâche « en cours » : celles qui l'étaient restent à finir.
      const status = live && last && group.status !== "done" ? "doing" : group.status === "doing" ? "open" : group.status;
      const older = inside.length > RECENT_STEPS + 10 ? inside.splice(0, inside.length - RECENT_STEPS) : [];
      return `<details class="task-group ${status}" data-key="${key}:task${at}"${last ? " open" : ""}>
        <summary><span class="tg-dot">${status === "done" ? ICONS.check : ""}</span><span class="tg-title">${escapeHtml(group.title)}</span>
          <span class="side">${plural(actions, "action")}</span></summary>
        <div class="events">${older.length ? `<details class="earlier" data-key="${key}:task${at}:earlier"><summary>${plural(older.length, "étape")} plus tôt</summary>${older.join("")}</details>` : ""}${inside.join("")}</div>
      </details>`;
    }).join("");
  } else {
    const flat = bundled(steps.filter(Boolean), events, key);
    // Long journal : les dernières étapes en vue, les précédentes à déplier.
    const older = flat.length > RECENT_STEPS + 10 ? flat.splice(0, flat.length - RECENT_STEPS) : [];
    const earlier = older.length
      ? `<details class="earlier" data-key="${key}:earlier"><summary>${plural(older.length, "étape")} plus tôt</summary>${older.join("")}</details>`
      : "";
    body = `<div class="events">${earlier}${flat.join("")}</div>`;
  }
  const count = (name) => `<span class="${name}" data-actions="${tools.length}">${plural(tools.length, "action")}</span>`;
  const head = stats && tools.length
    ? `<span class="run-head"><span class="run-title">${title}</span>${statsHtml(events.slice(0, shown), tools, count("rs"))}</span>`
    : `<span class="run-title">${title}</span>${count("side")}`;
  return `<details class="transcript${grouped ? " by-task" : ""}${thinkingShown() ? " thinking" : ""}" data-key="${key}"${live ? " open" : ""}>
    <summary>${kind ? `<span class="run-state ${kind}">${STATE_ICONS[kind] || ""}</span>` : ""}${head}</summary>
    ${digest}${filtersHtml(steps.filter(Boolean))}${grouped ? `<div class="task-groups">${body}</div>` : body}${footHtml(events.slice(0, shown))}
  </details>`;
}

/** Les modifications d'une branche (réponse de l'API de comparaison de GitHub). */
export function changesHtml(compare) {
  const files = compare.files || [];
  const added = files.reduce((sum, file) => sum + (file.additions || 0), 0);
  const removed = files.reduce((sum, file) => sum + (file.deletions || 0), 0);
  const label = { added: "ajouté", removed: "supprimé", modified: "modifié", renamed: "renommé", copied: "copié", changed: "changé" };
  const open = files.length <= 4;
  return `
    <p class="help">${plural(compare.total_commits || 0, "commit")} · ${plural(files.length, "fichier")} · ${counts(added, removed)}</p>
    <div class="file-group">${files.map((file) => `
      <details class="file" data-key="file:${escapeHtml(file.filename)}"${open ? " open" : ""}>
        <summary><span class="file-status ${escapeHtml(file.status)}">${escapeHtml(label[file.status] || file.status)}</span>
          <span class="path">${escapeHtml(file.filename)}</span>
          <span class="side">${counts(file.additions, file.deletions)}</span></summary>
        ${file.patch ? unifiedDiffHtml(file.patch, languageOf(file.filename))
          : `<p class="help">Différence non fournie par GitHub (fichier binaire ou trop grand) : <a href="${escapeHtml(file.blob_url || "#")}" target="_blank" rel="noopener noreferrer">voir le fichier</a>.</p>`}
      </details>`).join("")}</div>`;
}
