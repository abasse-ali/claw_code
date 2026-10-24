// Pages des outils MCP de Claw Web : la liste des outils que Claw peut utiliser pendant une
// session (serveurs `github`, `claw-remote`, `claw-docs` du moteur), ses documents et ses
// routines. Les documents, routines et suivis sont rangés par le moteur sur la branche des
// fichiers du dépôt des sessions (`claw-web-fichiers`) : l'appli les lit et les modifie là.

import { GitHubError } from "./github.js";
import { escapeHtml, renderMarkdown } from "./markdown.js";
import catalog from "./mcp-tools.js";

export const FILES_BRANCH = "claw-web-fichiers";
const ROUTINES = "routines/routines.json";
const WATCHES = "routines/abonnements.json";
const ROUTINES_WORKFLOW = "claw-routines.yml";
const CALLER = `on:
  schedule:
    - cron: "17 * * * *"
  workflow_dispatch:
permissions:
  actions: read
  contents: write
  issues: write
jobs:
  routines:
    uses: abasse-ali/claw_code/.github/workflows/claw-routines.yml@main
    with:
      runner: \${{ vars.CLAW_RUNNER }}
    secrets: inherit`;

const dateTime = (iso) => (iso ? new Date(iso).toLocaleString("fr-FR", { dateStyle: "medium", timeStyle: "short" }) : "");

/** Les pages, avec ce dont elles ont besoin de l'appli. */
export function extras(app) {
  const { icon, toast, failure } = app;
  const view = () => document.querySelector("#view");
  const back = `<a class="back" href="#/">${icon.back}<span>Accueil</span></a>`;
  const engine = () => app.settings().engine;

  function page(title, body, lead = "") {
    app.leavePage();
    view().innerHTML = `
      <section class="files tools-page">
        ${back}
        <h1>${escapeHtml(title)}</h1>
        ${lead ? `<p class="lead">${lead}</p>` : ""}
        <div class="tools-body">${body}</div>
      </section>`;
    return view().querySelector(".tools-body");
  }

  // --- Outils MCP ------------------------------------------------------------------------------

  async function showTools() {
    const body = page("Outils MCP", "",
      "Pendant une session, Claw peut appeler ces outils, comme Claude Code sur le web. Le moteur fait chaque "
      + "appel avec le jeton, que l'agent n'a jamais, et s'en tient au dépôt de la session : la fusion et les "
      + "suppressions restent dans les boutons de l'appli.");
    const total = catalog.servers.reduce((sum, server) => sum + server.tools.length, 0);
    body.innerHTML = `
      <input type="search" class="tools-filter" placeholder="Filtrer les ${total} outils" aria-label="Filtrer les outils">
      ${catalog.servers.map((server) => `
        <details class="tools-group" open data-server="${escapeHtml(server.server)}">
          <summary><strong>${escapeHtml(server.title)}</strong><code>${escapeHtml(server.server)}</code>
            <small>${server.tools.length} outils</small></summary>
          <p class="about">${escapeHtml(server.about)}</p>
          <ul class="file-list tools-list">${server.tools.map((tool) => `
            <li class="tool-row" data-search="${escapeHtml(`${server.server} ${tool.name} ${tool.title} ${tool.note || ""}`.toLowerCase())}">
              <div class="tool-name"><code>${escapeHtml(server.server)} • ${escapeHtml(tool.name)}</code>
                ${tool.read_only ? '<span class="tool-badge">lecture</span>' : ""}</div>
              <span class="tool-title">${escapeHtml(tool.title)}</span>
              ${tool.note ? `<small class="tool-note">${escapeHtml(tool.note)}</small>` : ""}
            </li>`).join("")}
          </ul>
        </details>`).join("")}
      <p class="empty tools-none" hidden>Aucun outil ne correspond.</p>`;
    const filter = body.querySelector(".tools-filter");
    filter.addEventListener("input", () => {
      const words = filter.value.toLowerCase().split(/\s+/).filter(Boolean);
      let shown = 0;
      for (const row of body.querySelectorAll(".tool-row")) {
        row.hidden = !words.every((word) => row.dataset.search.includes(word));
        if (!row.hidden) shown += 1;
      }
      for (const group of body.querySelectorAll(".tools-group")) {
        group.hidden = !group.querySelector(".tool-row:not([hidden])");
      }
      body.querySelector(".tools-none").hidden = shown > 0;
    });
  }

  // --- Documents -------------------------------------------------------------------------------

  async function documents() {
    let entries = [];
    try {
      entries = await app.github().contents(engine(), "documents", FILES_BRANCH);
    } catch (error) {
      if (!(error instanceof GitHubError && error.status === 404)) throw error;
    }
    const folders = (Array.isArray(entries) ? entries : []).filter((entry) => entry.type === "dir").slice(-60);
    const docs = await Promise.all(folders.map(async (folder) => {
      const { value } = await app.github().readJson(engine(), `documents/${folder.name}/doc.json`, FILES_BRANCH);
      return value;
    }));
    return docs.filter(Boolean).sort((a, b) => String(b.updated_at || "").localeCompare(String(a.updated_at || "")));
  }

  async function showDocuments() {
    const body = page("Documents", '<p class="empty">Chargement…</p>',
      "Les documents que Claw a écrits pour vous (rapports, plans, notes), avec leurs onglets et leurs commentaires.");
    const here = location.hash;
    try {
      const docs = await documents();
      if (location.hash !== here) return;
      body.innerHTML = docs.length ? `<ul class="file-list">${docs.map((doc) => {
        const open = (doc.comments || []).filter((comment) => !comment.parent && !comment.resolved).length;
        return `<li><a class="file-row" href="#/document/${encodeURIComponent(doc.id)}">${icon.file}
          <span class="name">${escapeHtml(doc.title || doc.id)}</span>
          <small>${escapeHtml(dateTime(doc.updated_at))}${open ? ` · ${open} commentaire${open > 1 ? "s" : ""}` : ""}</small></a></li>`;
      }).join("")}</ul>`
        : '<p class="empty">Aucun document pour l\'instant. Demandez-en un à Claw (« fais-moi un rapport… »).</p>';
    } catch (error) {
      body.innerHTML = '<p class="empty">Documents indisponibles.</p>';
      failure(error);
    }
  }

  /** Le Markdown d'un onglet, ses blocs (`<?claude block clé?>`) rendus en texte. */
  function withBlocks(markdown, blocks) {
    return String(markdown || "").replace(/<\?(?:claude|claw) block ([A-Za-z0-9_-]+)\?>/g, (whole, key) => {
      const block = (blocks || {})[key];
      if (!block) return "";
      if (block.type === "pending") return `*En cours : ${block.intent || "section à venir"}*`;
      if (block.type === "date") return block.value ? new Date(block.value).toLocaleDateString("fr-FR", { dateStyle: "long" }) : "";
      if (block.type === "mention") return block.user === "me" ? "vous" : `@${block.user || ""}`;
      return block.text || "";
    });
  }

  async function showDocument(id) {
    const body = page("Document", '<p class="empty">Chargement…</p>');
    const here = location.hash;
    try {
      const root = `documents/${id}`;
      const { value: doc } = await app.github().readJson(engine(), `${root}/doc.json`, FILES_BRANCH);
      if (!doc) {
        body.innerHTML = '<p class="empty">Document introuvable.</p>';
        return;
      }
      const tabs = doc.tabs || [];
      const wanted = new URLSearchParams(location.hash.split("?")[1] || "").get("onglet");
      const tab = tabs.find((item) => item.id === wanted) || tabs[0];
      const markdown = tab ? await app.github().readText(engine(), `${root}/${tab.file || `${tab.id}.md`}`, FILES_BRANCH) : "";
      if (location.hash !== here) return;
      view().querySelector("h1").textContent = doc.title || id;
      const comments = (doc.comments || []).filter((comment) => !tab || !comment.tab || comment.tab === tab.id);
      const roots = comments.filter((comment) => !comment.parent);
      const thread = (rootComment) => [rootComment, ...comments.filter((comment) => comment.parent === rootComment.id)];
      const exports = (doc.exports || []).filter((item) => !tab || item.tab === tab.id);
      body.innerHTML = `
        <p class="crumbs"><span>${escapeHtml(dateTime(doc.updated_at))}</span>
          ${doc.session ? `<a href="#/session/${encodeURIComponent(doc.session)}">Session ${escapeHtml(doc.session)}</a>` : ""}</p>
        ${tabs.length > 1 ? `<div class="chips doc-tabs">${tabs.map((item) => `
          <a class="chip${item === tab ? " current" : ""}" href="#/document/${encodeURIComponent(id)}?onglet=${encodeURIComponent(item.id)}"><span>${escapeHtml(item.title || item.id)}</span></a>`).join("")}</div>` : ""}
        <div class="doc-actions">
          <button type="button" class="small-link" data-export="md">${icon.file}<span>Markdown</span></button>
          <button type="button" class="small-link" data-export="html">${icon.file}<span>Page HTML</span></button>
          ${exports.map((item) => `<button type="button" class="small-link" data-blob="${escapeHtml(item.path)}">${icon.file}<span>${escapeHtml(String(item.format || "").toUpperCase())}</span></button>`).join("")}
        </div>
        <article class="answer doc-body">${renderMarkdown(withBlocks(markdown, doc.blocks))}</article>
        <h2 class="doc-subtitle">Commentaires</h2>
        ${roots.length ? `<ul class="doc-comments">${roots.map((rootComment) => `
          <li class="${rootComment.resolved ? "resolved" : ""}">${thread(rootComment).map((comment) => `
            <div class="doc-comment"><strong>${comment.author === "claw" ? "Claw" : "Vous"}</strong>
              <small>${escapeHtml(dateTime(comment.created_at))}${comment.resolved ? " · résolu" : ""}</small>
              ${comment.quote ? `<blockquote>${escapeHtml(comment.quote)}</blockquote>` : ""}
              <div>${renderMarkdown(comment.text || "")}</div></div>`).join("")}</li>`).join("")}</ul>`
          : '<p class="empty">Aucun commentaire.</p>'}
        <form class="doc-comment-form">
          <textarea rows="2" placeholder="Un commentaire sur ce document"></textarea>
          <div class="doc-actions">
            <button type="submit" class="small-link">Commenter</button>
            ${doc.session ? `<button type="button" class="small-link primary-link" data-ask>${icon.send}<span>Commenter et demander à Claw</span></button>` : ""}
          </div>
        </form>`;
      const text = body.querySelector("textarea");
      const comment = async (ask) => {
        const words = text.value.trim();
        if (!words) return;
        let made = null;
        await app.github().updateJson(engine(), `${root}/doc.json`, FILES_BRANCH, `Commentaire sur « ${doc.title || id} »`,
          (value) => {
            value.comments = value.comments || [];
            const number = value.comments.reduce((most, item) => Math.max(most, Number(String(item.id).slice(1)) || 0), 0) + 1;
            made = { id: `c${number}`, parent: null, author: "user", text: words, tab: tab ? tab.id : null,
              created_at: new Date().toISOString().replace(/\.\d+Z$/, "Z"), resolved: false };
            value.comments.push(made);
          }, () => doc);
        if (ask && made) {
          await app.sendToSession(doc.session, `Commentaire sur le document « ${doc.title || id} »`
            + `${tab ? `, onglet « ${tab.title || tab.id} »` : ""} (document ${id}, commentaire ${made.id}) :\n\n${words}\n\n`
            + "Réponds dans le document : un commentaire sous celui-ci (parent " + made.id + "), et les changements demandés.");
          toast("Commentaire envoyé à Claw");
        } else {
          toast("Commentaire ajouté");
        }
        showDocument(id);
      };
      body.querySelector(".doc-comment-form").addEventListener("submit", (event) => {
        event.preventDefault();
        comment(false).catch(failure);
      });
      body.querySelector("[data-ask]")?.addEventListener("click", () => comment(true).catch(failure));
      for (const button of body.querySelectorAll("[data-export]")) {
        button.addEventListener("click", () => {
          const source = withBlocks(markdown, doc.blocks);
          const name = (doc.title || id).replace(/[^\p{L}\p{N} _-]+/gu, "").trim() || id;
          if (button.dataset.export === "md") {
            download(new Blob([source], { type: "text/markdown;charset=utf-8" }), `${name}.md`);
          } else {
            const html = `<!doctype html><html lang="fr"><meta charset="utf-8"><title>${escapeHtml(doc.title || id)}</title>`
              + `<style>body{font:16px/1.55 system-ui,sans-serif;max-width:46em;margin:2em auto;padding:0 1em}pre{overflow:auto;background:#f4f4f4;padding:.8em}table{border-collapse:collapse}td,th{border:1px solid #ccc;padding:.3em .6em}</style>`
              + `<h1>${escapeHtml(doc.title || id)}</h1>${renderMarkdown(source)}</html>`;
            download(new Blob([html], { type: "text/html;charset=utf-8" }), `${name}.html`);
          }
        });
      }
      for (const button of body.querySelectorAll("[data-blob]")) {
        button.addEventListener("click", async () => {
          try {
            const blob = await app.github().rawFile(engine(), button.dataset.blob, FILES_BRANCH);
            download(blob, button.dataset.blob.split("/").pop());
          } catch (error) {
            failure(error);
          }
        });
      }
    } catch (error) {
      body.innerHTML = '<p class="empty">Document indisponible.</p>';
      failure(error);
    }
  }

  function download(blob, name) {
    const link = document.createElement("a");
    link.href = URL.createObjectURL(blob);
    link.download = name;
    document.body.appendChild(link);
    link.click();
    link.remove();
    setTimeout(() => URL.revokeObjectURL(link.href), 10000);
  }

  // --- Routines --------------------------------------------------------------------------------

  function schedule(routine) {
    if (routine.cron_expression) {
      const zone = (routine.cron_expression.match(/^CRON_TZ=(\S+)\s/) || [])[1];
      const fields = routine.cron_expression.replace(/^CRON_TZ=\S+\s+/, "");
      return `cron <code>${escapeHtml(fields)}</code> (${escapeHtml(zone || "UTC")})`;
    }
    if (routine.run_once_at) return `une fois, le ${escapeHtml(dateTime(routine.run_once_at))}`;
    return "sur demande";
  }

  function target(routine) {
    const goal = routine.target || {};
    if (goal.new_session) return `nouvelle session sur ${escapeHtml(goal.new_session.repo || "")}`;
    return goal.session ? `<a href="#/session/${encodeURIComponent(goal.session)}">session ${escapeHtml(goal.session)}</a>` : "";
  }

  async function showRoutines() {
    const body = page("Routines", '<p class="empty">Chargement…</p>',
      "Messages programmés que Claw envoie à une session (ou à une nouvelle session), pull requests suivies et "
      + "adresses surveillées. Le programmateur du dépôt des sessions passe toutes les heures.");
    const here = location.hash;
    try {
      const [routines, watches, workflow] = await Promise.all([
        app.github().readJson(engine(), ROUTINES, FILES_BRANCH),
        app.github().readJson(engine(), WATCHES, FILES_BRANCH),
        app.github().workflowFile(engine(), ROUTINES_WORKFLOW).catch(() => null),
      ]);
      if (location.hash !== here) return;
      const list = (routines.value && routines.value.routines) || [];
      const pulls = (watches.value && watches.value.pulls) || [];
      const urls = (watches.value && watches.value.watches) || [];
      body.innerHTML = `
        ${workflow ? "" : `<div class="notice"><p><strong>Programmateur absent du dépôt des sessions.</strong> Sans lui, les
          routines et les suivis ne partent pas. Ajoutez à ${escapeHtml(engine())} le fichier
          <code>.github/workflows/${ROUTINES_WORKFLOW}</code> :</p><pre class="code-view">${escapeHtml(CALLER)}</pre></div>`}
        ${list.length ? `<ul class="file-list routine-list">${list.map((routine) => `
          <li class="routine-row${routine.enabled ? "" : " off"}" data-id="${escapeHtml(routine.id)}">
            <div class="routine-head"><strong>${escapeHtml(routine.name || routine.id)}</strong>
              <button type="button" class="aid-item routine-toggle" role="switch" aria-checked="${routine.enabled ? "true" : "false"}"
                aria-label="Activer"><span class="switch"></span></button></div>
            <p class="routine-prompt">${escapeHtml(String(routine.prompt || "").slice(0, 300))}</p>
            <small>${schedule(routine)} · ${target(routine)}${routine.next_run_at && routine.enabled ? ` · prochaine : ${escapeHtml(dateTime(routine.next_run_at))}` : ""}
              ${routine.ended_reason === "run_once_fired" ? " · envoyée" : ""}
              ${routine.last_run ? ` · dernière : ${escapeHtml(dateTime(routine.last_run.fired_at))} ${routine.last_run.status === "SUCCEEDED" ? "✓" : `✗ ${escapeHtml(routine.last_run.error || "")}`}` : ""}</small>
            <div class="doc-actions">
              <button type="button" class="small-link" data-fire${workflow ? "" : " disabled"}>${icon.send}<span>Lancer maintenant</span></button>
              <button type="button" class="small-link stop" data-delete>${icon.trash}<span>Supprimer</span></button>
            </div>
          </li>`).join("")}</ul>`
          : '<p class="empty">Aucune routine. Demandez-en une à Claw (« chaque matin à 9 h, fais le point sur les issues »).</p>'}
        <h2 class="doc-subtitle">Suivis</h2>
        ${pulls.length || urls.length ? `<ul class="file-list">${pulls.map((pull) => `
          <li class="file-row" data-pull="${escapeHtml(pull.id)}">${icon.branch}
            <span class="name">Pull request <a href="https://github.com/${escapeHtml(pull.owner)}/${escapeHtml(pull.repo)}/pull/${Number(pull.pullNumber)}" target="_blank" rel="noopener">${escapeHtml(pull.owner)}/${escapeHtml(pull.repo)}#${Number(pull.pullNumber)}</a>
              → <a href="#/session/${encodeURIComponent(pull.session)}">session ${escapeHtml(pull.session)}</a></span>
            <button type="button" class="small-link stop" data-unfollow>Arrêter</button></li>`).join("")}
          ${urls.map((watch) => `
          <li class="file-row" data-watch="${escapeHtml(watch.id)}">${icon.file}
            <span class="name">${escapeHtml(watch.url)} → <a href="#/session/${encodeURIComponent(watch.session)}">session ${escapeHtml(watch.session)}</a></span>
            <button type="button" class="small-link stop" data-unfollow>Arrêter</button></li>`).join("")}</ul>`
          : '<p class="empty">Aucune pull request suivie, aucune adresse surveillée.</p>'}`;
      const save = (message, update) => app.github().updateJson(engine(), ROUTINES, FILES_BRANCH, message, update,
        () => ({ version: 1, routines: [] }));
      for (const row of body.querySelectorAll(".routine-row")) {
        const id = row.dataset.id;
        row.querySelector(".routine-toggle").addEventListener("click", async () => {
          try {
            await save(`Routine ${id} : ${row.classList.contains("off") ? "activée" : "désactivée"}`, (value) => {
              const routine = value.routines.find((item) => item.id === id);
              if (!routine) return;
              routine.enabled = !routine.enabled;
              // Le programmateur recalcule l'échéance d'une routine réactivée.
              routine.next_run_at = routine.enabled && routine.run_once_at && !routine.ended_reason ? routine.run_once_at : null;
              if (routine.enabled && routine.run_once_at && routine.ended_reason) routine.enabled = false;
            });
            showRoutines();
          } catch (error) {
            failure(error);
          }
        });
        row.querySelector("[data-fire]").addEventListener("click", async (event) => {
          const button = event.currentTarget;
          button.disabled = true;
          try {
            await save(`Routine ${id} lancée depuis l'appli`, (value) => {
              const routine = value.routines.find((item) => item.id === id);
              if (!routine) return;
              routine.enabled = true;
              routine.ended_reason = null;
              routine.next_run_at = new Date(Date.now() - 60000).toISOString().replace(/\.\d+Z$/, "Z");
            });
            await app.github().runWorkflow(engine(), ROUTINES_WORKFLOW, app.engineRef());
            toast("Routine lancée : elle part dans la minute");
          } catch (error) {
            button.disabled = false;
            failure(error);
          }
        });
        app.twoTaps(row.querySelector("[data-delete]"), async () => {
          await save(`Routine ${id} supprimée depuis l'appli`, (value) => {
            value.routines = value.routines.filter((item) => item.id !== id);
          });
          toast("Routine supprimée");
          showRoutines();
        });
      }
      for (const row of body.querySelectorAll("[data-pull], [data-watch]")) {
        row.querySelector("[data-unfollow]").addEventListener("click", async () => {
          try {
            await app.github().updateJson(engine(), WATCHES, FILES_BRANCH, "Suivi arrêté depuis l'appli", (value) => {
              value.pulls = (value.pulls || []).filter((item) => item.id !== row.dataset.pull);
              value.watches = (value.watches || []).filter((item) => item.id !== row.dataset.watch);
            }, () => ({ version: 1, pulls: [], watches: [] }));
            toast("Suivi arrêté");
            showRoutines();
          } catch (error) {
            failure(error);
          }
        });
      }
    } catch (error) {
      body.innerHTML = '<p class="empty">Routines indisponibles.</p>';
      failure(error);
    }
  }

  return { showTools, showDocuments, showDocument, showRoutines };
}
