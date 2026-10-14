// Rendu Markdown sûr des messages : tout le HTML du texte est échappé d'abord,
// puis seules quelques formes sont reconnues (titres, listes, citations, tableaux,
// blocs et morceaux de code, gras, italique, liens http(s)). Aucun HTML venu d'un
// message ne passe tel quel : un agent ou un commentaire ne peut rien injecter.

const ESCAPES = { "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" };
export const escapeHtml = (text) => String(text).replace(/[&<>"']/g, (character) => ESCAPES[character]);

/** Mise en forme d'une ligne déjà échappée : code, liens, gras, italique. */
function inline(text) {
  const codes = [];
  let html = text.replace(/`([^`\n]+)`/g, (_, code) => {
    codes.push(`<code>${code}</code>`);
    return `\u0000${codes.length - 1}\u0000`;
  });
  html = html.replace(/\[([^\]\n]+)\]\((https?:\/\/[^\s)]+)\)/g,
    (_, label, url) => `<a href="${url}" target="_blank" rel="noopener noreferrer">${label}</a>`);
  html = html.replace(/(^|[\s(])(https?:\/\/[^\s<)]+[^\s<).,;:!?])/g,
    (_, before, url) => `${before}<a href="${url}" target="_blank" rel="noopener noreferrer">${url}</a>`);
  html = html.replace(/\*\*([^*\n]+)\*\*/g, "<strong>$1</strong>");
  html = html.replace(/(^|[\s(])\*([^*\n]+)\*(?=[\s).,;:!?]|$)/g, "$1<em>$2</em>");
  html = html.replace(/(^|[\s(])_([^_\n]+)_(?=[\s).,;:!?]|$)/g, "$1<em>$2</em>");
  return html.replace(/\u0000(\d+)\u0000/g, (_, index) => codes[Number(index)]);
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
      out.push(`<pre><code>${code.join("\n")}</code></pre>`);
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
    if (bullet.test(line) || numbered.test(line)) {
      const ordered = numbered.test(line);
      const pattern = ordered ? numbered : bullet;
      const items = [];
      while (index < lines.length && pattern.test(lines[index])) {
        let item = lines[index].replace(pattern, "");
        index += 1;
        // Suite de l'élément : lignes en retrait qui ne commencent pas un autre élément.
        while (index < lines.length && /^\s{2,}\S/.test(lines[index]) && !pattern.test(lines[index])) {
          item += ` ${lines[index].trim()}`;
          index += 1;
        }
        items.push(`<li>${inline(item)}</li>`);
      }
      out.push(ordered ? `<ol>${items.join("")}</ol>` : `<ul>${items.join("")}</ul>`);
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
