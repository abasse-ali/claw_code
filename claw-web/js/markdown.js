// Rendu Markdown sûr des messages : tout le HTML du texte est échappé d'abord,
// puis seules quelques formes sont reconnues (titres, listes, citations, tableaux,
// blocs et morceaux de code, gras, italique, liens http(s)). Aucun HTML venu d'un
// message ne passe tel quel : un agent ou un commentaire ne peut rien injecter.

import { highlight, languageOf } from "./highlight.js";

const ESCAPES = { "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" };
export const escapeHtml = (text) => String(text).replace(/[&<>"']/g, (character) => ESCAPES[character]);
const UNESCAPES = { "&amp;": "&", "&lt;": "<", "&gt;": ">", "&quot;": '"', "&#39;": "'" };
const unescapeHtml = (text) => text.replace(/&(?:amp|lt|gt|quot|#39);/g, (entity) => UNESCAPES[entity]);

// Adresse de la machine où Claw a travaillé (un serveur lancé pendant la tâche) : elle
// n'existe pas sur le téléphone ni sur l'ordinateur de l'utilisateur, et un lien vers elle
// ne mènerait nulle part. Elle reste lisible, sans être un lien.
const LOCAL_HOST = /^https?:\/\/(?:localhost|127(?:\.\d{1,3}){3}|0\.0\.0\.0|\[::1\])(?::\d+)?(?:[/?#]|$)/i;
const link = (url, label) => (LOCAL_HOST.test(url)
  ? `<span class="local-address" title="Adresse de la machine de Claw : elle ne s'ouvre pas d'ici">${label}</span>`
  : `<a href="${url}" target="_blank" rel="noopener noreferrer">${label}</a>`);

/** Mise en forme d'une ligne déjà échappée : code, liens, gras, italique. */
function inline(text) {
  const codes = [];
  let html = text.replace(/`([^`\n]+)`/g, (_, code) => {
    // Une adresse écrite comme du code (« `https://…` », courant chez les modèles) reste
    // un lien qu'on touche.
    const url = code.trim();
    codes.push(/^https?:\/\/[^\s<]+$/.test(url) ? link(url, `<code>${code}</code>`) : `<code>${code}</code>`);
    return `\u0000${codes.length - 1}\u0000`;
  });
  // Typographie française : espace insécable avant « : ; ! ? » et dans les guillemets, pour
  // qu'un signe ne parte jamais seul en début de ligne.
  html = html.replace(/ ([:;!?»])/g, " $1").replace(/« /g, "« ");
  html = html.replace(/\[([^\]\n]+)\]\((https?:\/\/[^\s)]+)\)/g, (_, label, url) => link(url, label));
  // Adresse entre chevrons (`<https://…>`, échappée en `&lt;…&gt;`).
  html = html.replace(/&lt;(https?:\/\/(?:[^\s<>&]|&amp;)+)&gt;/g, (_, url) => link(url, url));
  // Adresse nue, aussi en gras (`**https://…**`) ou collée après « : ».
  html = html.replace(/(^|[\s(*:])(https?:\/\/[^\s<)*]+[^\s<)*.,;:!?])/g,
    (_, before, url) => `${before}${link(url, url)}`);
  // Sans `https://` : une page GitHub Pages, ou une adresse en `www.`.
  html = html.replace(/(^|[\s(*])((?:www\.[a-z0-9-]+(?:\.[a-z0-9-]+)+|[a-z0-9-]+\.github\.io)(?:\/[^\s<)*]*[^\s<)*.,;:!?])?)/gi,
    (_, before, address) => `${before}${link(`https://${address}`, address)}`);
  html = html.replace(/\*\*([^*\n]+)\*\*/g, "<strong>$1</strong>");
  html = html.replace(/(^|[\s(])\*([^*\n]+)\*(?=[\s).,;:!?]|$)/g, "$1<em>$2</em>");
  html = html.replace(/(^|[\s(])_([^_\n]+)_(?=[\s).,;:!?]|$)/g, "$1<em>$2</em>");
  return html.replace(/\u0000(\d+)\u0000/g, (_, index) => codes[Number(index)]);
}

const LIST_ITEM = /^(\s*)([-*+]|\d{1,9}[.)])\s+(.*)$/;
const indentOf = (line) => line.match(/^\s*/)[0].replace(/\t/g, "    ").length;
const numberedItem = (match) => /\d/.test(match[2]);

/** Liste commençant à la ligne `index`, ses sous-listes rangées d'après le retrait. */
function list(lines, start) {
  let index = start;
  const first = lines[index].match(LIST_ITEM);
  const level = indentOf(lines[index]);
  const ordered = numberedItem(first);
  const sameList = (line) => {
    const match = line.match(LIST_ITEM);
    return Boolean(match) && indentOf(line) === level && numberedItem(match) === ordered;
  };
  const nextFilled = (from) => {
    let next = from;
    while (next < lines.length && !lines[next].trim()) next += 1;
    return next;
  };
  const items = [];
  while (index < lines.length) {
    // Ligne vide entre deux éléments : la même liste continue.
    if (!lines[index].trim()) {
      const next = nextFilled(index);
      if (next < lines.length && sameList(lines[next])) index = next;
      else break;
    }
    if (!sameList(lines[index])) break;
    let text = lines[index].match(LIST_ITEM)[3];
    const inside = [];
    index += 1;
    while (index < lines.length) {
      const line = lines[index];
      if (!line.trim()) {
        const next = nextFilled(index);
        if (next < lines.length && indentOf(lines[next]) > level && !/^\s*```/.test(lines[next])) {
          index = next;
          continue;
        }
        break;
      }
      if (indentOf(line) <= level || /^\s*```/.test(line)) break;
      if (LIST_ITEM.test(line)) {
        const nested = list(lines, index);
        inside.push(nested.html);
        index = nested.index;
        continue;
      }
      // Suite de l'élément : lignes en retrait qui ne commencent pas un autre élément.
      if (inside.length) inside.push(`<div>${inline(line.trim())}</div>`);
      else text += ` ${line.trim()}`;
      index += 1;
    }
    items.push(`<li>${inline(text)}${inside.join("")}</li>`);
  }
  const from = ordered ? Number.parseInt(first[2], 10) : 1;
  const html = ordered
    ? `<ol${from === 1 ? "" : ` start="${from}"`}>${items.join("")}</ol>`
    : `<ul>${items.join("")}</ul>`;
  return { html, index };
}

const isTableRow = (line) => /^\s*\|.*\|\s*$/.test(line);
const isTableRule = (line) => /^\s*\|?\s*:?-{2,}:?\s*(\|\s*:?-{2,}:?\s*)*\|?\s*$/.test(line);
const cells = (line) => line.trim().replace(/^\||\|$/g, "").split("|").map((cell) => inline(cell.trim()));

/** Le Markdown d'un message en HTML sûr (marqueurs `<!-- … -->` retirés). */
export function renderMarkdown(source) {
  const lines = escapeHtml(String(source || "").replace(/<!--[\s\S]*?-->/g, "").trim()).split(/\r?\n/);
  const out = [];
  let index = 0;
  while (index < lines.length) {
    const line = lines[index];
    const fence = line.match(/^\s*```\s*([\w+-]*)/);
    if (fence) {
      const code = [];
      index += 1;
      while (index < lines.length && !/^\s*```/.test(lines[index])) {
        code.push(lines[index]);
        index += 1;
      }
      index += 1;
      // Le texte du bloc, échappé avec tout le message, est rendu à la coloration.
      const language = languageOf(fence[1]);
      const label = language ? ` data-lang="${escapeHtml(fence[1].toLowerCase())}"` : "";
      out.push(`<pre${label}><code>${highlight(unescapeHtml(code.join("\n")), language)}</code></pre>`);
      continue;
    }
    if (!line.trim()) {
      index += 1;
      continue;
    }
    const heading = line.match(/^(#{1,6})\s+(.*)$/);
    if (heading) {
      const level = Math.min(heading[1].length + 2, 6);
      out.push(`<h${level}>${inline(heading[2])}</h${level}>`);
      index += 1;
      continue;
    }
    if (/^\s*(-{3,}|\*{3,}|_{3,})\s*$/.test(line)) {
      out.push("<hr>");
      index += 1;
      continue;
    }
    if (isTableRow(line) && index + 1 < lines.length && isTableRule(lines[index + 1])) {
      const head = cells(line).map((cell) => `<th>${cell}</th>`).join("");
      const rows = [];
      index += 2;
      while (index < lines.length && isTableRow(lines[index])) {
        rows.push(`<tr>${cells(lines[index]).map((cell) => `<td>${cell}</td>`).join("")}</tr>`);
        index += 1;
      }
      out.push(`<div class="table"><table><thead><tr>${head}</tr></thead><tbody>${rows.join("")}</tbody></table></div>`);
      continue;
    }
    if (/^\s*&gt;\s?/.test(line)) {
      const quoted = [];
      while (index < lines.length && /^\s*&gt;\s?/.test(lines[index])) {
        quoted.push(lines[index].replace(/^\s*&gt;\s?/, ""));
        index += 1;
      }
      out.push(`<blockquote>${quoted.map(inline).join("<br>")}</blockquote>`);
      continue;
    }
    const bullet = /^\s*[-*+]\s+/;
    const numbered = /^\s*\d+[.)]\s+/;
    if (LIST_ITEM.test(line)) {
      const found = list(lines, index);
      out.push(found.html);
      // Toujours avancer, même sur une liste vide (élément au retrait inattendu).
      index = Math.max(found.index, index + 1);
      continue;
    }
    const paragraph = [];
    while (index < lines.length && lines[index].trim() && !/^\s*```/.test(lines[index])
      && !/^(#{1,6})\s+/.test(lines[index]) && !bullet.test(lines[index]) && !numbered.test(lines[index])
      && !/^\s*&gt;/.test(lines[index]) && !isTableRow(lines[index])) {
      paragraph.push(inline(lines[index]));
      index += 1;
    }
    if (paragraph.length) {
      out.push(`<p>${paragraph.join("<br>")}</p>`);
    } else {
      out.push(`<p>${inline(line)}</p>`);
      index += 1;
    }
  }
  return out.join("\n");
}
