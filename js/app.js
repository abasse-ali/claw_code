// Appli web de Claw : choisir un dépôt, une branche et un modèle gratuit, décrire une
// tâche ; Claw y travaille dans GitHub Actions (workflow claw-web.yml du dépôt du moteur),
// pousse une branche, ouvre la pull request et répond. Chaque session est une issue du
// moteur ; l'appli la suit en direct (progression, réponse, pull request).

import { ANSWER, GitHub, GitHubError, PROGRESS } from "./github.js";
import { escapeHtml, renderMarkdown } from "./markdown.js";
import { sealedBox } from "./sealed-box.js";

const STORE = "claw-web.settings.v1";
const DEFAULT_ENGINE = "abasse-ali/claw_code";
const SETTINGS_BLOCK = /<!--\s*claw-web\s+(\{[\s\S]*?\})\s*-->/;
const TOKEN_HELP = "https://github.com/settings/personal-access-tokens/new";

const ICON = {
  menu: '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M4 7h16M4 12h16M4 17h16"/></svg>',
  gear: '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M4 6h9M17 6h3M4 12h3M11 12h9M4 18h11M19 18h1"/><circle cx="15" cy="6" r="2"/><circle cx="9" cy="12" r="2"/><circle cx="17" cy="18" r="2"/></svg>',
  repo: '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M5 4.5A1.5 1.5 0 0 1 6.5 3H19v15H6.5A1.5 1.5 0 0 0 5 19.5v-15Z"/><path d="M5 19.5A1.5 1.5 0 0 0 6.5 21H19v-3"/></svg>',
  branch: '<svg viewBox="0 0 24 24" aria-hidden="true"><circle cx="6" cy="5" r="2"/><circle cx="6" cy="19" r="2"/><circle cx="18" cy="7" r="2"/><path d="M6 7v10M18 9c0 5-6 4-12 8"/></svg>',
  model: '<svg viewBox="0 0 24 24" aria-hidden="true"><rect x="6" y="6" width="12" height="12" rx="2"/><path d="M9 2.5v3.5M15 2.5v3.5M9 18v3.5M15 18v3.5M2.5 9H6M2.5 15H6M18 9h3.5M18 15h3.5"/></svg>',
  send: '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M12 19V5M5.5 11.5 12 5l6.5 6.5"/></svg>',
  plus: '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M12 5v14M5 12h14"/></svg>',
  close: '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M6 6l12 12M18 6 6 18"/></svg>',
  external: '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M14 4h6v6M20 4l-9 9M18 14v5a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1V7a1 1 0 0 1 1-1h5"/></svg>',
  pull: '<svg viewBox="0 0 24 24" aria-hidden="true"><circle cx="6" cy="5" r="2"/><circle cx="6" cy="19" r="2"/><circle cx="18" cy="19" r="2"/><path d="M6 7v10M18 17V9a3 3 0 0 0-3-3h-4M13 3.5 10.5 6 13 8.5"/></svg>',
  retry: '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M4 12a8 8 0 1 0 2.4-5.7M4 4v4.5h4.5"/></svg>',
  back: '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M15 5l-7 7 7 7"/></svg>',
  archive: '<svg viewBox="0 0 24 24" aria-hidden="true"><rect x="3.5" y="4" width="17" height="4" rx="1"/><path d="M5 8v11a1 1 0 0 0 1 1h12a1 1 0 0 0 1-1V8M10 12h4"/></svg>',
};

const state = {
  settings: loadSettings(),
  github: null,
  user: null,
  engineRef: "main",
  repos: null,
  branches: new Map(),
  models: null,
  runs: [],
  session: null,
  pollTimer: null,
  dispatchedAt: 0,
  sending: false,
};

const $ = (selector) => document.querySelector(selector);

// --- Réglages gardés dans ce navigateur ----------------------------------------------------

function loadSettings() {
  try {
    return { engine: DEFAULT_ENGINE, model: "", ...JSON.parse(localStorage.getItem(STORE) || "{}") };
  } catch {
    return { engine: DEFAULT_ENGINE, model: "" };
  }
}

function saveSettings() {
  try {
    localStorage.setItem(STORE, JSON.stringify(state.settings));
  } catch {
    toast("Ce navigateur ne garde pas les réglages (navigation privée ?)", "error");
  }
}

// --- Petits outils -------------------------------------------------------------------------

function toast(message, kind = "info") {
  const box = $("#toast");
  box.textContent = message;
  box.className = `toast ${kind}`;
  box.hidden = false;
  clearTimeout(toast.timer);
  toast.timer = setTimeout(() => { box.hidden = true; }, kind === "error" ? 7000 : 3500);
}

function failure(error) {
  console.error(error);
  if (error instanceof GitHubError && error.status === 401) {
    toast(error.message, "error");
    location.hash = "#/reglages";
    return;
  }
  toast(error.message || String(error), "error");
}

function settingsOf(issue) {
  const found = SETTINGS_BLOCK.exec(issue.body || "");
  if (!found) return {};
  try {
    return JSON.parse(found[1]);
  } catch {
    return {};
  }
}

const taskOf = (issue) => (issue.body || "").replace(SETTINGS_BLOCK, "").trim();
const runTitle = (repo, number) => `Claw ${repo} #${number}`;

function since(date) {
  const seconds = Math.max(0, Math.round((Date.now() - new Date(date).getTime()) / 1000));
  if (seconds < 60) return "à l'instant";
  if (seconds < 3600) return `il y a ${Math.round(seconds / 60)} min`;
  if (seconds < 86400) return `il y a ${Math.round(seconds / 3600)} h`;
  return new Date(date).toLocaleDateString("fr-FR", { day: "numeric", month: "short" });
}

function modelLabel(model) {
  if (!model) return "Auto";
  const known = (state.models || []).find((item) => item.id === model);
  return known ? known.short : model.replace(/:free$/, "").split("/").pop();
}

/** État d'un passage du moteur, pour l'affichage. */
function runStatus(run) {
  if (!run) return null;
  if (run.status === "queued" || run.status === "waiting" || run.status === "pending" || run.status === "requested") {
    return { key: "queued", label: "En file d'attente" };
  }
  if (run.status === "in_progress") return { key: "running", label: "Claw travaille" };
  if (run.conclusion === "success") return { key: "done", label: "Terminé" };
  if (run.conclusion === "cancelled") return { key: "failed", label: "Annulé" };
  return { key: "failed", label: "Échec" };
}

function latestRun(repo, number) {
  return state.runs.find((run) => run.display_title === runTitle(repo, number)) || null;
}

// --- Chargements ---------------------------------------------------------------------------

async function loadRepos() {
  if (!state.repos) state.repos = await state.github.repositories();
  return state.repos;
}

async function loadBranches(repo) {
  if (!state.branches.has(repo)) {
    const branches = await state.github.branches(repo);
    state.branches.set(repo, branches.map((branch) => branch.name));
  }
  return state.branches.get(repo);
}

/** Modèles gratuits d'OpenRouter (liste publique, sans clé). */
async function loadModels() {
  if (state.models) return state.models;
  try {
    const response = await fetch("https://openrouter.ai/api/v1/models", { cache: "no-store" });
    const data = await response.json();
    state.models = (data.data || [])
      .filter((model) => model.id.endsWith(":free"))
      .map((model) => ({
        id: model.id,
        short: (model.name || model.id).replace(/\s*\(free\)\s*$/i, ""),
        detail: model.id,
      }))
      .sort((left, right) => left.short.localeCompare(right.short));
  } catch {
    state.models = [];
  }
  return state.models;
}

async function refreshRuns() {
  try {
    state.runs = await state.github.runs(state.settings.engine);
  } catch (error) {
    if (!(error instanceof GitHubError)) throw error;
    state.runs = [];
  }
}

// --- Feuille de choix (dépôt, branche, modèle) ---------------------------------------------

function openPicker({ title, items, current, placeholder, onPick }) {
  const sheet = $("#sheet");
  sheet.innerHTML = `
    <div class="sheet-panel" role="dialog" aria-modal="true" aria-label="${escapeHtml(title)}">
      <div class="sheet-head">
        <strong>${escapeHtml(title)}</strong>
        <button class="icon-button" data-close aria-label="Fermer">${ICON.close}</button>
      </div>
      ${items.length > 8 ? `<input class="sheet-search" type="search" placeholder="${escapeHtml(placeholder || "Rechercher")}" autocomplete="off">` : ""}
      <ul class="sheet-list"></ul>
    </div>`;
  const list = sheet.querySelector(".sheet-list");
  const draw = (filter = "") => {
    const words = filter.toLowerCase().split(/\s+/).filter(Boolean);
    const shown = items.filter((item) => words.every((word) => `${item.label} ${item.detail || ""}`.toLowerCase().includes(word)));
    list.innerHTML = shown.slice(0, 200).map((item) => `
      <li><button class="sheet-item${item.value === current ? " current" : ""}" data-value="${escapeHtml(item.value)}">
        <span>${escapeHtml(item.label)}</span>
        ${item.detail ? `<small>${escapeHtml(item.detail)}</small>` : ""}
      </button></li>`).join("") || '<li class="empty">Rien ne correspond.</li>';
  };
  draw();
  const close = () => {
    sheet.hidden = true;
    sheet.innerHTML = "";
  };
  sheet.onclick = (event) => {
    if (event.target === sheet || event.target.closest("[data-close]")) close();
    const button = event.target.closest(".sheet-item");
    if (button) {
      close();
      onPick(button.dataset.value);
    }
  };
  const search = sheet.querySelector(".sheet-search");
  if (search) search.oninput = () => draw(search.value);
  sheet.hidden = false;
  (search || sheet.querySelector(".sheet-item"))?.focus();
}

async function pickRepo() {
  try {
    const repos = await loadRepos();
    openPicker({
      title: "Dépôt",
      placeholder: "Rechercher un dépôt",
      current: state.settings.repo,
      items: repos.map((repo) => ({
        value: repo.full_name,
        label: repo.full_name,
        detail: `${repo.private ? "privé" : "public"} · ${since(repo.pushed_at)}`,
      })),
      onPick: async (value) => {
        state.settings.repo = value;
        const repo = repos.find((item) => item.full_name === value);
        state.settings.branch = repo ? repo.default_branch : "main";
        saveSettings();
        renderComposer();
      },
    });
  } catch (error) {
    failure(error);
  }
}

async function pickBranch() {
  if (!state.settings.repo) return pickRepo();
  try {
    const branches = await loadBranches(state.settings.repo);
    openPicker({
      title: "Branche de départ",
      current: state.settings.branch,
      items: branches.map((name) => ({ value: name, label: name })),
      onPick: (value) => {
        state.settings.branch = value;
        saveSettings();
        renderComposer();
      },
    });
  } catch (error) {
    failure(error);
  }
}

async function pickModel() {
  const models = await loadModels();
  openPicker({
    title: "Modèle (gratuit)",
    placeholder: "Rechercher un modèle",
    current: state.settings.model || "",
    items: [
      { value: "", label: "Auto", detail: "le meilleur modèle gratuit pour le code" },
      ...models.map((model) => ({ value: model.id, label: model.short, detail: model.detail })),
    ],
    onPick: (value) => {
      state.settings.model = value;
      saveSettings();
      renderComposer();
    },
  });
}

// --- Zone de saisie --------------------------------------------------------------------------

function renderComposer() {
  const composer = $("#composer");
  const session = state.session;
  const followUp = Boolean(session);
  const repo = followUp ? session.settings.repo : state.settings.repo;
  const branch = followUp ? `claw/session-${session.number}` : state.settings.branch;
  composer.hidden = !state.github || location.hash.startsWith("#/reglages");
  // En suivi de session, le dépôt et la branche sont dans l'en-tête : seul le modèle se choisit.
  composer.innerHTML = `
    <div class="chips">
      ${followUp ? "" : `
      <button class="chip" data-action="repo">${ICON.repo}<span>${escapeHtml(repo || "Choisir un dépôt")}</span></button>
      <button class="chip" data-action="branch">${ICON.branch}<span>${escapeHtml(branch || "branche")}</span></button>`}
      <button class="chip" data-action="model">${ICON.model}<span>${escapeHtml(modelLabel(state.settings.model))}</span></button>
    </div>
    <form class="prompt" autocomplete="off">
      <textarea rows="1" name="text" placeholder="${followUp ? "Répondez ou donnez une nouvelle consigne." : "Décrivez une tâche ou posez une question."}" aria-label="Message pour Claw"></textarea>
      <button class="send" type="submit" aria-label="Envoyer" disabled>${ICON.send}</button>
    </form>`;
  const textarea = composer.querySelector("textarea");
  const send = composer.querySelector(".send");
  const resize = () => {
    textarea.style.height = "auto";
    textarea.style.height = `${Math.min(textarea.scrollHeight, 220)}px`;
    send.disabled = state.sending || !textarea.value.trim() || (!followUp && !repo);
  };
  textarea.addEventListener("input", resize);
  textarea.addEventListener("keydown", (event) => {
    // Entrée envoie sur un clavier d'ordinateur ; Maj+Entrée va à la ligne.
    const touch = window.matchMedia("(pointer: coarse)").matches;
    if (event.key === "Enter" && !event.shiftKey && (!touch || event.ctrlKey || event.metaKey)) {
      event.preventDefault();
      if (!send.disabled) composer.querySelector("form").requestSubmit();
    }
  });
  composer.querySelector("form").addEventListener("submit", (event) => {
    event.preventDefault();
    const text = textarea.value.trim();
    if (!text) return;
    if (followUp) followUpSession(text, textarea);
    else startSession(text, textarea);
  });
  composer.querySelector('[data-action="repo"]')?.addEventListener("click", pickRepo);
  composer.querySelector('[data-action="branch"]')?.addEventListener("click", pickBranch);
  composer.querySelector('[data-action="model"]').addEventListener("click", pickModel);
  resize();
}

// --- Sessions --------------------------------------------------------------------------------

async function startSession(text, textarea) {
  const { engine, repo, branch, model } = state.settings;
  state.sending = true;
  renderSending(true);
  try {
    const firstLine = text.split("\n")[0].trim();
    const title = firstLine.length > 72 ? `${firstLine.slice(0, 71)}…` : firstLine;
    const block = `<!-- claw-web ${JSON.stringify({ repo, base: branch, model: model || "" })} -->`;
    const issue = await state.github.createIssue(engine, title, `${text}\n\n${block}`);
    await state.github.dispatch(engine, state.engineRef,
      { repo, base: branch || "main", session: String(issue.number), model: model || "" });
    state.dispatchedAt = Date.now();
    textarea.value = "";
    location.hash = `#/session/${issue.number}`;
  } catch (error) {
    failure(error);
  } finally {
    state.sending = false;
    renderSending(false);
  }
}

async function followUpSession(text, textarea) {
  const session = state.session;
  state.sending = true;
  renderSending(true);
  try {
    await state.github.comment(state.settings.engine, session.number, text);
    await dispatchSession(session);
    textarea.value = "";
    textarea.dispatchEvent(new Event("input"));
    await pollSession(true);
  } catch (error) {
    failure(error);
  } finally {
    state.sending = false;
    renderSending(false);
  }
}

async function dispatchSession(session) {
  await state.github.dispatch(state.settings.engine, state.engineRef, {
    repo: session.settings.repo,
    base: session.settings.base || "main",
    session: String(session.number),
    model: state.settings.model || "",
  });
  state.dispatchedAt = Date.now();
}

function renderSending(sending) {
  const send = document.querySelector("#composer .send");
  if (send) send.classList.toggle("sending", sending);
  const textarea = document.querySelector("#composer textarea");
  if (textarea) textarea.dispatchEvent(new Event("input"));
}

// --- Vues ------------------------------------------------------------------------------------

function sessionItem(issue) {
  const settings = settingsOf(issue);
  const status = issue.state === "closed" ? { key: "archived", label: "Archivée" } : runStatus(latestRun(settings.repo, issue.number));
  return `
    <li><a class="session-item" href="#/session/${issue.number}">
      <span class="dot ${status ? status.key : "idle"}" aria-hidden="true"></span>
      <span class="session-text">
        <strong>${escapeHtml(issue.title)}</strong>
        <small>${escapeHtml(settings.repo || "")} · ${status ? escapeHtml(status.label) : "en attente"} · ${since(issue.updated_at)}</small>
      </span>
    </a></li>`;
}

async function renderDrawer() {
  const list = $("#drawer .sessions");
  list.innerHTML = '<li class="empty">Chargement…</li>';
  try {
    const [sessions] = await Promise.all([state.github.sessions(state.settings.engine), refreshRuns()]);
    list.innerHTML = sessions.map(sessionItem).join("") || '<li class="empty">Aucune session pour l\'instant.</li>';
  } catch (error) {
    list.innerHTML = '<li class="empty">Sessions indisponibles.</li>';
    failure(error);
  }
}

function openDrawer() {
  $("#drawer").hidden = false;
  $("#app").classList.add("drawer-open");
  renderDrawer();
}

function closeDrawer() {
  $("#drawer").hidden = true;
  $("#app").classList.remove("drawer-open");
}

async function showHome() {
  state.session = null;
  const view = $("#view");
  const name = state.user ? state.user.name || state.user.login : "";
  view.innerHTML = `
    <section class="home">
      <h1 class="greeting"><span class="logo" aria-hidden="true"></span>Quoi de prévu${name ? `, ${escapeHtml(name.split(" ")[0])}` : ""} ?</h1>
      <h2 class="section-title">Sessions récentes</h2>
      <ul class="sessions recent"><li class="empty">Chargement…</li></ul>
    </section>`;
  if (!state.settings.repo) {
    loadRepos().then((repos) => {
      if (!state.settings.repo && repos.length) {
        state.settings.repo = repos[0].full_name;
        state.settings.branch = repos[0].default_branch;
        saveSettings();
        renderComposer();
      }
    }).catch(failure);
  }
  renderComposer();
  try {
    const [sessions] = await Promise.all([state.github.sessions(state.settings.engine), refreshRuns()]);
    const recent = view.querySelector(".recent");
    if (recent) {
      recent.innerHTML = sessions.filter((issue) => issue.state === "open").slice(0, 8).map(sessionItem).join("")
        || '<li class="empty">Décrivez une tâche ci-dessous : Claw la fera dans le dépôt choisi et proposera une pull request.</li>';
    }
  } catch (error) {
    failure(error);
  }
}

function messageHtml(comment, isLastProgress) {
  const body = comment.body || "";
  if (body.includes(PROGRESS)) {
    return `<div class="progress-card${isLastProgress ? "" : " old"}">${renderMarkdown(body)}</div>`;
  }
  if (body.includes(ANSWER)) {
    return `<article class="message claw"><div class="who">Claw</div>${renderMarkdown(body)}</article>`;
  }
  return `<article class="message user"><div class="who">${escapeHtml(comment.user ? comment.user.login : "Vous")}</div>${renderMarkdown(body)}</article>`;
}

function renderSessionView() {
  const session = state.session;
  const view = $("#view");
  const settings = session.settings;
  const run = latestRun(settings.repo, session.number);
  const status = runStatus(run);
  const comments = session.comments;
  const lastProgress = [...comments].reverse().find((comment) => (comment.body || "").includes(PROGRESS));
  const answered = comments.length && (comments[comments.length - 1].body || "").includes(ANSWER);
  const waiting = !run && Date.now() - state.dispatchedAt < 90_000;
  const active = waiting || (status && (status.key === "queued" || status.key === "running"));
  const step = session.step ? ` · ${escapeHtml(session.step)}` : "";
  const pull = session.pull;
  const pullBadge = pull ? (pull.merged_at ? "fusionnée" : pull.state === "open" ? "ouverte" : "fermée") : "";
  view.innerHTML = `
    <section class="session">
      <header class="session-head">
        <h1>${escapeHtml(session.issue.title)}</h1>
        <div class="session-meta">
          <a class="meta-link" href="https://github.com/${escapeHtml(settings.repo || "")}" target="_blank" rel="noopener noreferrer">${ICON.repo}<span>${escapeHtml(settings.repo || "?")}</span></a>
          <span class="meta-link">${ICON.branch}<span>claw/session-${session.number}</span></span>
          ${pull ? `<a class="meta-link pull ${pullBadge}" href="${escapeHtml(pull.html_url)}" target="_blank" rel="noopener noreferrer">${ICON.pull}<span>PR #${pull.number} ${pullBadge}</span></a>` : ""}
        </div>
      </header>
      <div class="messages">
        <article class="message user"><div class="who">${escapeHtml(session.issue.user ? session.issue.user.login : "Vous")}</div>${renderMarkdown(taskOf(session.issue))}</article>
        ${comments.map((comment) => messageHtml(comment, comment === lastProgress && !answered)).join("")}
      </div>
      <div class="run-status ${active ? "active" : status ? status.key : ""}">
        ${active ? `<span class="spinner" aria-hidden="true"></span><span>${run ? escapeHtml(status.label) : "Lancement du passage"}${step}</span>` : ""}
        ${!active && status && status.key === "failed" && !answered ? `<span>Le dernier passage a échoué.</span>` : ""}
        <span class="run-actions">
          ${run ? `<a href="${escapeHtml(run.html_url)}" target="_blank" rel="noopener noreferrer" class="small-link">${ICON.external}<span>Job</span></a>` : ""}
          ${!active ? `<button class="small-link" data-action="retry">${ICON.retry}<span>Relancer</span></button>` : ""}
          ${session.issue.state === "open" ? `<button class="small-link" data-action="archive">${ICON.archive}<span>Archiver</span></button>` : ""}
          <a href="${escapeHtml(session.issue.html_url)}" target="_blank" rel="noopener noreferrer" class="small-link">${ICON.external}<span>Session</span></a>
        </span>
      </div>
    </section>`;
  view.querySelector('[data-action="retry"]')?.addEventListener("click", async () => {
    try {
      await dispatchSession(session);
      toast("Passage relancé");
      await pollSession(true);
    } catch (error) {
      failure(error);
    }
  });
  view.querySelector('[data-action="archive"]')?.addEventListener("click", async () => {
    try {
      await state.github.setIssueState(state.settings.engine, session.number, "closed");
      toast("Session archivée");
      location.hash = "#/";
    } catch (error) {
      failure(error);
    }
  });
  session.active = active;
}

async function showSession(number) {
  const view = $("#view");
  if (!state.session || state.session.number !== number) {
    view.innerHTML = '<section class="session"><p class="empty">Chargement de la session…</p></section>';
    state.session = { number, issue: null, comments: [], settings: {}, pull: null, step: "", signature: "" };
  }
  try {
    await pollSession(true);
  } catch (error) {
    failure(error);
  }
}

/** Relit la session (messages, passage, pull request) et ne redessine que si elle a changé. */
async function pollSession(force = false) {
  const session = state.session;
  if (!session) return;
  clearTimeout(state.pollTimer);
  const engine = state.settings.engine;
  const [issue, comments] = await Promise.all([
    state.github.issue(engine, session.number),
    state.github.comments(engine, session.number),
    refreshRuns(),
  ]);
  if (state.session !== session) return;
  session.issue = issue;
  session.comments = comments;
  session.settings = settingsOf(issue);
  const run = latestRun(session.settings.repo, session.number);
  session.step = "";
  if (run && run.status === "in_progress") {
    try {
      const jobs = await state.github.jobs(engine, run.id);
      const current = jobs.flatMap((job) => job.steps || []).find((step) => step.status === "in_progress");
      session.step = current ? current.name : "";
    } catch {
      session.step = "";
    }
  }
  try {
    session.pull = session.settings.repo ? await state.github.pull(session.settings.repo, `claw/session-${session.number}`) : null;
  } catch {
    session.pull = null;
  }
  const signature = JSON.stringify([
    issue.updated_at, issue.state, comments.map((comment) => [comment.id, comment.updated_at]),
    run && [run.id, run.status, run.conclusion], session.step, session.pull && [session.pull.state, session.pull.merged_at],
  ]);
  if (force || signature !== session.signature) {
    const atBottom = window.innerHeight + window.scrollY >= document.body.scrollHeight - 80;
    session.signature = signature;
    renderSessionView();
    renderComposer();
    if (atBottom || force) window.scrollTo({ top: document.body.scrollHeight });
  } else {
    renderSessionView();
  }
  if (state.session === session && !document.hidden) {
    state.pollTimer = setTimeout(() => pollSession().catch(failure), session.active ? 4000 : 30000);
  }
}

// --- Réglages --------------------------------------------------------------------------------

async function engineChecks() {
  const engine = state.settings.engine;
  const checks = [];
  const add = (ok, label) => checks.push(`<li class="${ok ? "ok" : "ko"}">${escapeHtml(label)}</li>`);
  try {
    const repo = await state.github.repository(engine);
    state.engineRef = repo.default_branch || "main";
    add(true, `Moteur ${engine} accessible`);
  } catch (error) {
    add(false, `Moteur ${engine} : ${error.message}`);
    return checks.join("");
  }
  try {
    await state.github.workflow(engine);
    add(true, "Workflow claw-web.yml présent");
  } catch {
    add(false, "Workflow claw-web.yml absent du moteur");
  }
  for (const name of ["OPENROUTER_API_KEY", "CLAW_REPO_TOKEN"]) {
    try {
      add(await state.github.hasSecret(engine, name), `Secret ${name}${""}`);
    } catch (error) {
      add(false, `Secret ${name} : ${error.message}`);
    }
  }
  return checks.join("");
}

function showSettings(first = false, justConnected = false) {
  state.session = null;
  $("#composer").hidden = true;
  const settings = state.settings;
  $("#view").innerHTML = `
    <section class="settings">
      ${state.github ? `<a class="back" href="#/">${ICON.back}<span>Accueil</span></a>` : ""}
      <h1>Réglages</h1>
      ${first ? '<p class="intro">Claw travaille sur vos dépôts GitHub depuis GitHub Actions. Pour commencer, donnez à l\'appli un jeton GitHub.</p>' : ""}
      ${justConnected ? `<div class="ready"><p>Connecté. Configurez le moteur ci-dessous une fois, puis décrivez votre première tâche depuis l'accueil.</p><a class="primary" href="#/">Aller à l'accueil</a></div>` : ""}
      <form class="settings-form" autocomplete="off">
        <label>Jeton GitHub
          <input name="token" type="password" placeholder="github_pat_…" value="${escapeHtml(settings.token || "")}" spellcheck="false">
        </label>
        <p class="help">Créez un <a href="${TOKEN_HELP}" target="_blank" rel="noopener noreferrer">jeton à grain fin</a> :
          dépôts : le moteur et ceux où Claw travaillera ;
          autorisations des dépôts en lecture et écriture : Actions, Contents, Issues, Pull requests, Secrets.
          Il reste dans ce navigateur seulement.</p>
        <label>Dépôt du moteur
          <input name="engine" type="text" value="${escapeHtml(settings.engine || DEFAULT_ENGINE)}" spellcheck="false">
        </label>
        <button class="primary" type="submit">Enregistrer</button>
      </form>
      ${state.github ? `
      <h2>Moteur</h2>
      <ul class="checks"><li>Vérification…</li></ul>
      <form class="engine-form" autocomplete="off">
        <label>Clé OpenRouter (pour les modèles gratuits)
          <input name="openrouter" type="password" placeholder="sk-or-…" spellcheck="false">
        </label>
        <label class="check"><input name="share" type="checkbox" checked>
          <span>Donner aussi ce jeton au moteur (secret CLAW_REPO_TOKEN), pour qu'il pousse ses branches et ouvre les pull requests</span></label>
        <p class="help">Les secrets partent chiffrés dans le navigateur, pour la clé publique du moteur ; la clé OpenRouter n'est pas gardée ici.</p>
        <button class="primary" type="submit">Configurer le moteur</button>
      </form>
      <button class="danger" data-action="forget">Effacer le jeton de cet appareil</button>` : ""}
    </section>`;
  $("#view .settings-form").addEventListener("submit", async (event) => {
    event.preventDefault();
    const form = event.target;
    const token = form.token.value.trim();
    const engine = form.engine.value.trim() || DEFAULT_ENGINE;
    if (!/^[\w.-]+\/[\w.-]+$/.test(engine)) {
      toast("Dépôt du moteur attendu sous la forme propriétaire/nom", "error");
      return;
    }
    const github = new GitHub(token);
    try {
      state.user = await github.user();
      state.github = github;
      state.settings.token = token;
      if (state.settings.engine !== engine) state.repos = null;
      state.settings.engine = engine;
      saveSettings();
      toast(`Connecté en tant que ${state.user.login}`);
      $("#settings-button").classList.add("active");
      showSettings(false, true);
    } catch (error) {
      failure(error);
    }
  });
  const engineForm = $("#view .engine-form");
  if (engineForm) {
    engineChecks().then((html) => {
      const list = $("#view .checks");
      if (list) list.innerHTML = html;
    }).catch(failure);
    engineForm.addEventListener("submit", async (event) => {
      event.preventDefault();
      const form = event.target;
      const engine = state.settings.engine;
      try {
        await state.github.ensureLabel(engine);
        const key = await state.github.secretKey(engine);
        const secrets = [];
        if (form.openrouter.value.trim()) secrets.push(["OPENROUTER_API_KEY", form.openrouter.value.trim()]);
        if (form.share.checked) secrets.push(["CLAW_REPO_TOKEN", state.settings.token]);
        for (const [name, value] of secrets) {
          await state.github.putSecret(engine, name, sealedBox(value, key.key), key.key_id);
        }
        form.openrouter.value = "";
        toast(secrets.length ? `Moteur configuré (${secrets.map(([name]) => name).join(", ")})` : "Étiquette prête ; aucun secret envoyé");
        const list = $("#view .checks");
        if (list) list.innerHTML = await engineChecks();
      } catch (error) {
        failure(error);
      }
    });
    $("#view [data-action=forget]").addEventListener("click", () => {
      delete state.settings.token;
      saveSettings();
      state.github = null;
      state.user = null;
      toast("Jeton effacé de cet appareil");
      showSettings(true);
    });
  }
}

// --- Démarrage -------------------------------------------------------------------------------

function route() {
  clearTimeout(state.pollTimer);
  closeDrawer();
  const hash = location.hash || "#/";
  const onSettings = hash.startsWith("#/reglages");
  $("#settings-button").classList.toggle("active", onSettings);
  // Les réglages ont leur propre page : sans jeton, l'appli y va (l'accueil reste `#/`,
  // et toucher « Claw » y ramène).
  if (!state.github) {
    if (!onSettings) history.replaceState(null, "", "#/reglages");
    $("#settings-button").classList.add("active");
    showSettings(true);
    return;
  }
  if (onSettings) {
    showSettings();
    return;
  }
  const match = hash.match(/^#\/session\/(\d+)/);
  if (match) {
    showSession(Number(match[1]));
    return;
  }
  showHome();
}

/** Pas de zoom, comme une appli : Safari sur iPhone ignore `user-scalable=no`, on arrête
 * donc le geste de pincement lui-même (le CSS `touch-action` suffit ailleurs). */
function blockZoom() {
  const stop = (event) => event.preventDefault();
  document.addEventListener("gesturestart", stop, { passive: false });
  document.addEventListener("gesturechange", stop, { passive: false });
  document.addEventListener("gestureend", stop, { passive: false });
  document.addEventListener("touchmove", (event) => {
    if (event.touches.length > 1 || (event.scale !== undefined && event.scale !== 1)) event.preventDefault();
  }, { passive: false });
}

async function boot() {
  blockZoom();
  $("#menu").innerHTML = ICON.menu;
  $("#settings-button").innerHTML = ICON.gear;
  $("#drawer .new-session").innerHTML = `${ICON.plus}<span>Nouvelle tâche</span>`;
  $("#menu").addEventListener("click", openDrawer);
  $("#drawer-close").innerHTML = ICON.close;
  $("#drawer-close").addEventListener("click", closeDrawer);
  $("#drawer .new-session").addEventListener("click", () => {
    closeDrawer();
    location.hash = "#/";
  });
  $("#settings-button").addEventListener("click", () => { location.hash = "#/reglages"; });
  $("#scrim").addEventListener("click", closeDrawer);
  window.addEventListener("hashchange", route);
  document.addEventListener("visibilitychange", () => {
    if (!document.hidden && state.session) pollSession().catch(failure);
  });
  if ("serviceWorker" in navigator) {
    navigator.serviceWorker.register("sw.js").catch(() => {});
  }
  if (state.settings.token) {
    const github = new GitHub(state.settings.token);
    try {
      state.user = await github.user();
      state.github = github;
      const engine = await github.repository(state.settings.engine).catch(() => null);
      if (engine) state.engineRef = engine.default_branch || "main";
    } catch (error) {
      failure(error);
    }
  }
  route();
}

boot();
