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
};

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
  const summary = `${ICONS[icon] || ICONS.tool}<span class="verb">${escapeHtml(verb)}</span>`
    + `<span class="target">${escapeHtml(target)}</span>`
    + `<span class="side">${!result ? '<span class="spinner" aria-label="en cours"></span>' : side}</span>`;
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
      return `<div class="ev notice">${escapeHtml(event.t || "")}</div>`;
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

/** Le journal d'un passage : déplié pendant le passage, replié ensuite (une ligne). */
export function transcriptHtml(events, { key, live }) {
  const tools = events.filter((event) => event.k === "tool").length;
  const lastTodo = events.map((event) => event.k === "tool" && event.n === "TodoWrite").lastIndexOf(true);
  const rows = events.map((event, index) => eventHtml(event, `${key}:${index}`, live && index === lastTodo)).join("");
  return `<details class="transcript" data-key="${key}"${live ? " open" : ""}>
    <summary><span>Journal du passage</span><span class="side">${plural(tools, "action")}</span></summary>
    <div class="events">${rows}</div>
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
