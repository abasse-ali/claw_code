// Journal détaillé d'un passage, publié par le moteur (journal.py) dans des commentaires de
// la session, et affiché comme dans l'interface locale de Claw : messages de l'agent,
// réflexions, outils dépliables avec leur entrée et leur sortie, différences de fichiers,
// liste de tâches. Aussi : les modifications d'une branche (comparaison GitHub).

import { escapeHtml, renderMarkdown } from "./markdown.js";
import { highlight, languageOf } from "./highlight.js";

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

const shortModel = (model) => String(model || "").replace(/:free$/, "").split("/").pop();

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

/** Icône, verbe et cible d'un appel d'outil, comme dans l'interface locale de Claw. */
function describe(name, input) {
  const value = input || {};
  switch (name) {
    case "bash": case "PowerShell": return ["terminal", "Commande", value.desc || value.cmd || ""];
    case "read_file": return ["file", "Lecture", value.path || ""];
    case "write_file": return ["filePlus", "Écriture", value.path || ""];
    case "edit_file": case "NotebookEdit": return ["edit", "Modification", value.path || ""];
    case "glob_search": return ["search", "Fichiers", value.pattern || ""];
    case "grep_search": return ["search", "Recherche", (value.pattern || "") + (value.path ? ` · ${value.path}` : "")];
    case "WebFetch": case "ViewPage": return ["globe", "Page web", value.target || ""];
    case "WebSearch": return ["globe", "Recherche web", value.target || ""];
    case "Agent": return ["bot", "Sous-agent", value.desc || ""];
    case "Photo": return ["image", "Photos", value.search ? `« ${value.search} »` : ""];
    case "Icon": return ["tool", "Icônes", (value.names || []).join(", ")];
    case "Ornament": return ["tool", "Ornement", value.kind || ""];
    case "Component": return ["tool", "Composants", (value.names || []).join(", ")];
    case "Screenshot": return ["globe", "Capture de la page", value.target || ""];
    case "TodoWrite": {
      const todos = value.todos || [];
      return ["list", "Tâches", `${todos.filter((todo) => todo[0] === "completed").length}/${todos.length} terminées`];
    }
    default: return ["tool", name, String(value.raw || "").slice(0, 140)];
  }
}

function todosHtml(todos) {
  return `<div class="todos">${(todos || []).map(([status, text]) => `
    <div class="todo ${status === "completed" ? "done" : status === "in_progress" ? "doing" : ""}">
      <span class="box">${status === "completed" ? ICONS.check : ""}</span><span>${escapeHtml(text)}</span>
    </div>`).join("")}</div>`;
}

/** Lignes de différence (`+`, `-`, ` `), numérotées depuis le début de chaque passage, colorées
 * dans le langage du fichier (ligne par ligne). */
function diffRows(hunks, language = "") {
  const rows = [];
  hunks.forEach(({ oldStart, newStart, lines }, index) => {
    if (index) rows.push('<div class="dl gap">⋯</div>');
    let oldNo = oldStart || 1;
    let newNo = newStart || 1;
    for (const line of lines) {
      const kind = line[0];
      const text = highlight(line.slice(1), language) || " ";
      if (kind === "+") rows.push(`<div class="dl add"><span class="no">${newNo++}</span><span class="tx">${text}</span></div>`);
      else if (kind === "-") rows.push(`<div class="dl del"><span class="no">${oldNo++}</span><span class="tx">${text}</span></div>`);
      else if (kind === "\\") continue;
      else {
        rows.push(`<div class="dl"><span class="no">${newNo}</span><span class="tx">${text}</span></div>`);
        oldNo += 1;
        newNo += 1;
      }
    }
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

/** Écran d'une capture, en français : « phone 390x844 » devient « téléphone 390 x 844 ». */
function screenName(screen) {
  const [kind, size] = String(screen || "").split(" ");
  const names = { phone: "téléphone", mobile: "téléphone", tablet: "tablette", desktop: "ordinateur" };
  return [names[kind] || kind, (size || "").replace("x", " x ")].filter(Boolean).join(" ");
}

/** Vignettes des captures d'écran : un toucher ouvre la capture entière. */
function shotsHtml(shots) {
  const pills = shots.map((shot) => {
    const thumb = thumbData(shot.t);
    const path = shotPath(shot.p);
    if (!thumb && !path) return "";
    const label = `Capture ${screenName(shot.screen)}${shot.n ? ` · ${plural(shot.n, "problème")}` : ""}`;
    return `<button type="button" class="photo-pill shot" data-shot="${escapeHtml(path)}" data-thumb="${escapeHtml(thumb)}"`
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
  } else if ((event.n === "write_file" || event.n === "edit_file") && result && Array.isArray(result.lines)) {
    side = counts(result.add, result.del);
    body = journalDiffHtml(result.lines, result.more, languageOf(input.path || input.file_path || target));
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
    side = result.found ? `<span class="del">${plural(result.found, "problème")}</span>` : plural(shots.length, "capture");
    body = "";
  }
  const summary = `${ICONS[icon] || ICONS.tool}<span class="verb">${escapeHtml(verb)}</span>`
    + `<span class="target">${escapeHtml(target)}</span>`
    + `<span class="side">${!result ? '<span class="spinner" aria-label="en cours"></span>' : side}</span>`;
  if (photos) return `<div class="ev tool photos${state}"><div class="ev-line">${summary}</div>${photosHtml(photos)}</div>`;
  if (shots) return `<div class="ev tool photos${state}"><div class="ev-line">${summary}</div>${shotsHtml(shots)}</div>`;
  if (!body) return `<div class="ev tool${state}"><div class="ev-line">${summary}</div></div>`;
  return `<details class="ev tool${state}" data-key="${key}"${openTodo ? " open" : ""}><summary>${summary}</summary><div class="ev-body">${body}</div></details>`;
}

function eventHtml(event, key, lastTodo) {
  switch (event.k) {
    case "init":
      return `<div class="ev meta">Modèle : ${escapeHtml(shortModel(event.model))}</div>`;
    case "think":
      return `<details class="ev think" data-key="${key}"><summary>${ICONS.think}<span class="verb">Réflexion</span>`
        + `<span class="target">${escapeHtml(String(event.t || "").replace(/\s+/g, " ").slice(0, 120))}</span></summary>`
        + `<div class="ev-body think-text">${escapeHtml(event.t || "")}</div></details>`;
    case "text":
      return `<div class="ev text">${renderMarkdown(event.t || "")}</div>`;
    case "notice":
      // « nvidia:nvidia/nemotron-3-super-120b-a12b » devient « nemotron-3-super-120b-a12b ».
      return `<div class="ev notice">${escapeHtml(String(event.t || "").replace(/\b(?:nvidia|gemini|ollama):(?:[\w.-]+\/)?([\w.:-]+)/g, "$1"))}</div>`;
    case "tool":
      return toolHtml(event, key, lastTodo);
    case "quota":
      return `<div class="ev meta">Requêtes gratuites : ${event.run !== null && event.run !== undefined ? `${plural(event.run, "requête")} pour ce passage · ` : ""}`
        + `${event.used}/${event.limit} utilisées aujourd'hui (${event.remaining} restantes)</div>`;
    case "end": {
      const parts = [shortModel(event.model), event.turns ? plural(event.turns, "requête") : "", event.ms ? duration(event.ms) : "", event.cost || ""];
      return `<div class="ev meta">${escapeHtml(parts.filter(Boolean).join(" · "))}</div>`;
    }
    default:
      return "";
  }
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

/** Étapes d'un journal montrées d'emblée ; les précédentes se déplient. */
const RECENT_STEPS = 30;

/** Pastille d'état d'un passage : en cours, fini, arrêté, erreur. */
const STATE_ICONS = {
  running: '<span class="spinner" aria-label="en cours"></span>',
  done: ICONS.check,
  stopped: '<svg viewBox="0 0 24 24" aria-hidden="true"><rect x="7" y="7" width="10" height="10" rx="2"/></svg>',
  failed: '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M12 7v6M12 16.5h.01"/></svg>',
  unfinished: '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M8 12h8"/></svg>',
};

/** Le journal d'un passage : déplié pendant le passage, replié ensuite (une ligne avec l'état du
 * passage, `title`, et le nombre d'actions). `answer` : la réponse publiée du passage, que le
 * journal ne répète pas. */
export function transcriptHtml(events, { key, live, answer = "", title = "Journal du passage", kind = "" }) {
  const tools = events.filter((event) => event.k === "tool").length;
  const lastTodo = events.map((event) => event.k === "tool" && event.n === "TodoWrite").lastIndexOf(true);
  const repeated = repeatedInAnswer(events, answer);
  const rows = events.map((event, index) => (repeated.has(index) ? ""
    : eventHtml(event, `${key}:${index}`, live && index === lastTodo))).filter(Boolean);
  // Long journal : les dernières étapes en vue, les précédentes à déplier.
  const older = rows.length > RECENT_STEPS + 10 ? rows.splice(0, rows.length - RECENT_STEPS) : [];
  const earlier = older.length
    ? `<details class="earlier" data-key="${key}:earlier"><summary>${plural(older.length, "étape")} plus tôt</summary>${older.join("")}</details>`
    : "";
  return `<details class="transcript" data-key="${key}"${live ? " open" : ""}>
    <summary>${kind ? `<span class="run-state ${kind}">${STATE_ICONS[kind] || ""}</span>` : ""}<span class="run-title">${title}</span><span class="side">${plural(tools, "action")}</span></summary>
    <div class="events">${earlier}${rows.join("")}</div>
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
    ${files.map((file) => `
      <details class="file" data-key="file:${escapeHtml(file.filename)}"${open ? " open" : ""}>
        <summary><span class="file-status ${escapeHtml(file.status)}">${escapeHtml(label[file.status] || file.status)}</span>
          <span class="path">${escapeHtml(file.filename)}</span>
          <span class="side">${counts(file.additions, file.deletions)}</span></summary>
        ${file.patch ? unifiedDiffHtml(file.patch, languageOf(file.filename))
          : `<p class="help">Différence non fournie par GitHub (fichier binaire ou trop grand) : <a href="${escapeHtml(file.blob_url || "#")}" target="_blank" rel="noopener noreferrer">voir le fichier</a>.</p>`}
      </details>`).join("")}`;
}
