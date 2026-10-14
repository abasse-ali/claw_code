// Appli web de Claw : choisir un dépôt, une branche et un modèle gratuit, décrire une
// tâche ; Claw y travaille dans GitHub Actions (workflow claw-web.yml du dépôt du moteur),
// pousse une branche, ouvre la pull request et répond. Chaque session est une issue du
// moteur ; l'appli la suit en direct (progression, réponse, pull request).

import { ANSWER, GitHub, GitHubError, LABEL, PROGRESS } from "./github.js";
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
  stop: '<svg viewBox="0 0 24 24" aria-hidden="true"><rect x="6.5" y="6.5" width="11" height="11" rx="2"/></svg>',
  trash: '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M4 7h16M10 11v6M14 11v6M6 7l1 12a1 1 0 0 0 1 1h8a1 1 0 0 0 1-1l1-12M9 7V4h6v3"/></svg>',
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
  // Liste des sessions du tiroir, et sessions cochées pour être effacées.
  drawerSessions: [],
  selecting: false,
  selected: new Set(),
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
  // Les éléments épinglés (« Créer un dépôt ») restent en tête, quelle que soit la recherche.
  const draw = (filter = "") => {
    const words = filter.toLowerCase().split(/\s+/).filter(Boolean);
    const shown = items.filter((item) => item.pinned
      || words.every((word) => `${item.label} ${item.detail || ""}`.toLowerCase().includes(word)));
    list.innerHTML = shown.slice(0, 200).map((item) => `
      <li><button class="sheet-item${item.value === current ? " current" : ""}${item.pinned ? " pinned" : ""}" data-value="${escapeHtml(item.value)}">
        <span class="sheet-label">${item.icon ? ICON[item.icon] : ""}<span>${escapeHtml(item.label)}</span></span>
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
      const typed = search ? search.value.trim() : "";
      close();
      onPick(button.dataset.value, typed);
    }
  };
  const search = sheet.querySelector(".sheet-search");
  if (search) search.oninput = () => draw(search.value);
  sheet.hidden = false;
  (search || sheet.querySelector(".sheet-item"))?.focus();
}

const NEW_REPO = "__nouveau-depot__";

async function pickRepo() {
  try {
    const repos = await loadRepos();
    openPicker({
      title: "Dépôt",
      placeholder: "Rechercher un dépôt",
      current: state.settings.repo,
      items: [
        { value: NEW_REPO, label: "Créer un dépôt", detail: "nouveau dépôt GitHub, avec un README et sa branche main", pinned: true, icon: "plus" },
        ...repos.map((repo) => ({
          value: repo.full_name,
          label: repo.full_name,
          detail: `${repo.private ? "privé" : "public"} · ${since(repo.pushed_at)}`,
        })),
      ],
      onPick: async (value, typed) => {
        if (value === NEW_REPO) {
          openCreateRepo(typed);
          return;
        }
        const repo = repos.find((item) => item.full_name === value);
        useRepo(value, repo ? repo.default_branch : "main");
      },
    });
  } catch (error) {
    failure(error);
  }
}

function useRepo(fullName, branch) {
  state.settings.repo = fullName;
  state.settings.branch = branch || "main";
  saveSettings();
  renderComposer();
}

/** Nom de dépôt proposé à partir d'un texte (« Mon appli de prêts » : mon-appli-de-prets). */
function repoSlug(text) {
  return (text || "").normalize("NFD").replace(/[̀-ͯ]/g, "").toLowerCase()
    .replace(/[^a-z0-9._-]+/g, "-").replace(/^-+|-+$/g, "").slice(0, 60);
}

const GITIGNORES = ["", "Python", "Node", "Rust", "Go", "Java", "Dart", "Swift", "Kotlin", "C++", "Unity"];

/** Formulaire de création d'un dépôt ; le dépôt créé devient celui de la tâche. */
function openCreateRepo(prefill = "") {
  const sheet = $("#sheet");
  sheet.innerHTML = `
    <div class="sheet-panel" role="dialog" aria-modal="true" aria-label="Nouveau dépôt">
      <div class="sheet-head">
        <strong>Nouveau dépôt</strong>
        <button class="icon-button" data-close aria-label="Fermer">${ICON.close}</button>
      </div>
      <form class="create-repo" autocomplete="off">
        <label>Nom
          <input name="repo_name" type="text" required maxlength="100" spellcheck="false" autocapitalize="none"
            placeholder="mon-projet" value="${escapeHtml(repoSlug(prefill))}">
        </label>
        <small class="owner-line">${escapeHtml(state.user ? state.user.login : "")}/<span class="slug">${escapeHtml(repoSlug(prefill) || "mon-projet")}</span></small>
        <label>Description (facultative)
          <input name="description" type="text" maxlength="350" placeholder="Ce que fera le projet">
        </label>
        <fieldset class="visibility">
          <label class="check"><input type="radio" name="visibility" value="private" checked><span>Privé</span></label>
          <label class="check"><input type="radio" name="visibility" value="public"><span>Public</span></label>
        </fieldset>
        <label>Fichier .gitignore
          <select name="gitignore">${GITIGNORES.map((name) => `<option value="${name}">${name || "Aucun"}</option>`).join("")}</select>
        </label>
        <button class="primary" type="submit">Créer le dépôt</button>
      </form>
    </div>`;
  const form = sheet.querySelector(".create-repo");
  const fields = form.elements;
  const slug = sheet.querySelector(".slug");
  fields.repo_name.addEventListener("input", () => {
    slug.textContent = fields.repo_name.value.trim() || "mon-projet";
  });
  const close = () => {
    sheet.hidden = true;
    sheet.innerHTML = "";
  };
  sheet.onclick = (event) => {
    if (event.target === sheet || event.target.closest("[data-close]")) close();
  };
  form.addEventListener("submit", async (event) => {
    event.preventDefault();
    const name = fields.repo_name.value.trim();
    if (!/^[A-Za-z0-9._-]{1,100}$/.test(name)) {
      toast("Nom de dépôt : lettres, chiffres, points, tirets et soulignés seulement", "error");
      return;
    }
    const button = form.querySelector("button[type=submit]");
    button.disabled = true;
    button.textContent = "Création…";
    try {
      const repo = await state.github.createRepository({
        name,
        description: fields.description.value.trim(),
        isPrivate: fields.visibility.value !== "public",
        gitignore: fields.gitignore.value,
      });
      if (state.repos) state.repos.unshift(repo);
      state.branches.set(repo.full_name, [repo.default_branch || "main"]);
      close();
      useRepo(repo.full_name, repo.default_branch);
      toast(`Dépôt ${repo.full_name} créé : décrivez ce que Claw doit y construire`);
      document.querySelector("#composer textarea")?.focus();
    } catch (error) {
      button.disabled = false;
      button.textContent = "Créer le dépôt";
      failure(error);
    }
  });
  sheet.hidden = false;
  fields.repo_name.focus();
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
  // Tout de suite, pas au rappel de l'observateur : le défilement vers le bas suit.
  updateComposerSpace();
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

function sessionItem(issue, selectable = false) {
  const settings = settingsOf(issue);
  const status = issue.state === "closed" ? { key: "archived", label: "Archivée" } : runStatus(latestRun(settings.repo, issue.number));
  const inner = `
      <span class="dot ${status ? status.key : "idle"}" aria-hidden="true"></span>
      <span class="session-text">
        <strong>${escapeHtml(issue.title)}</strong>
        <small>${escapeHtml(settings.repo || "")} · ${status ? escapeHtml(status.label) : "en attente"} · ${since(issue.updated_at)}</small>
      </span>`;
  if (selectable) {
    return `
    <li><label class="session-item selectable">
      <input type="checkbox" value="${issue.number}"${state.selected.has(issue.number) ? " checked" : ""} aria-label="Choisir ${escapeHtml(issue.title)}">${inner}
    </label></li>`;
  }
  return `
    <li><a class="session-item" href="#/session/${issue.number}">${inner}
    </a></li>`;
}

/** Liste du tiroir, depuis les sessions déjà lues ; en mode « Choisir », des cases à cocher. */
function drawDrawerList() {
  const list = $("#drawer .sessions");
  list.innerHTML = state.drawerSessions.map((issue) => sessionItem(issue, state.selecting)).join("")
    || '<li class="empty">Aucune session pour l\'instant.</li>';
  const toggle = $("#drawer .select-toggle");
  if (toggle) {
    toggle.textContent = state.selecting ? "Annuler" : "Choisir";
    toggle.hidden = !state.drawerSessions.length && !state.selecting;
  }
  const bar = $("#drawer .drawer-actions");
  if (bar) {
    bar.hidden = !state.selecting;
    const button = bar.querySelector("button");
    button.disabled = !state.selected.size;
    button.querySelector("span").textContent = state.selected.size ? `Supprimer (${state.selected.size})` : "Supprimer";
  }
}

async function renderDrawer() {
  const list = $("#drawer .sessions");
  list.innerHTML = '<li class="empty">Chargement…</li>';
  try {
    const [sessions] = await Promise.all([state.github.sessions(state.settings.engine), refreshRuns()]);
    state.drawerSessions = sessions;
    state.selected = new Set([...state.selected].filter((number) => sessions.some((issue) => issue.number === number)));
    drawDrawerList();
  } catch (error) {
    list.innerHTML = '<li class="empty">Sessions indisponibles.</li>';
    failure(error);
  }
}

/** Mode « Choisir » du tiroir : cocher des sessions puis les supprimer d'un coup. */
function setupDrawerSelection() {
  const toggle = document.createElement("button");
  toggle.type = "button";
  toggle.className = "small-link select-toggle";
  toggle.textContent = "Choisir";
  $("#drawer .drawer-head").insertBefore(toggle, $("#drawer-close"));
  const bar = document.createElement("div");
  bar.className = "drawer-actions";
  bar.hidden = true;
  bar.innerHTML = `<button type="button" class="small-link stop" disabled>${ICON.trash}<span>Supprimer</span></button>`;
  $("#drawer").appendChild(bar);
  toggle.addEventListener("click", () => {
    state.selecting = !state.selecting;
    state.selected.clear();
    drawDrawerList();
  });
  $("#drawer .sessions").addEventListener("change", (event) => {
    const box = event.target.closest('input[type="checkbox"]');
    if (!box) return;
    const number = Number(box.value);
    if (box.checked) state.selected.add(number);
    else state.selected.delete(number);
    drawDrawerList();
  });
  bar.querySelector("button").addEventListener("click", () => {
    const issues = state.drawerSessions.filter((issue) => state.selected.has(issue.number));
    if (issues.length) openDeleteSessions(issues);
  });
}

function openDrawer() {
  $("#drawer").hidden = false;
  $("#app").classList.add("drawer-open");
  renderDrawer();
}

function closeDrawer() {
  $("#drawer").hidden = true;
  $("#app").classList.remove("drawer-open");
  state.selecting = false;
  state.selected.clear();
}

/** Session au travail (passage en file ou en cours) : elle s'arrête avant d'être supprimée. */
function sessionRunning(issue) {
  const status = runStatus(latestRun(settingsOf(issue).repo, issue.number));
  return Boolean(status && (status.key === "running" || status.key === "queued"));
}

/** Feuille de suppression d'une ou plusieurs sessions : ce qui sera effacé, et les options. */
async function openDeleteSessions(issues) {
  const sheet = $("#sheet");
  const engine = state.settings.engine;
  const running = issues.filter(sessionRunning);
  const chosen = issues.filter((issue) => !running.includes(issue));
  const many = chosen.length > 1;
  const close = () => {
    sheet.hidden = true;
    sheet.innerHTML = "";
  };
  sheet.innerHTML = `
    <div class="sheet-panel" role="dialog" aria-modal="true" aria-label="Supprimer">
      <div class="sheet-head">
        <strong>${many ? `Supprimer ${chosen.length} sessions` : "Supprimer la session"}</strong>
        <button class="icon-button" data-close aria-label="Fermer">${ICON.close}</button>
      </div>
      <p class="empty">Lecture des pull requests…</p>
    </div>`;
  sheet.onclick = (event) => {
    if (event.target === sheet || event.target.closest("[data-close]")) close();
  };
  sheet.hidden = false;
  // Pull request de chaque branche : une ouverte est fermée avec sa branche, et c'est dit.
  const pulls = await Promise.all(chosen.map((issue) => {
    const repo = settingsOf(issue).repo;
    return repo ? state.github.pull(repo, `claw/session-${issue.number}`).catch(() => null) : null;
  }));
  if (sheet.hidden) return;
  const open = chosen
    .map((issue, index) => ({ repo: settingsOf(issue).repo, pull: pulls[index] }))
    .filter(({ pull }) => pull && pull.state === "open" && !pull.merged_at);
  const shown = chosen.slice(0, 6);
  sheet.querySelector(".sheet-panel").innerHTML = `
      <div class="sheet-head">
        <strong>${many ? `Supprimer ${chosen.length} sessions` : "Supprimer la session"}</strong>
        <button class="icon-button" data-close aria-label="Fermer">${ICON.close}</button>
      </div>
      <form class="delete-sessions">
        ${chosen.length ? `<ul>${shown.map((issue) => `
          <li><strong>${escapeHtml(issue.title)}</strong>
            <small>${escapeHtml(settingsOf(issue).repo || "?")} · claw/session-${issue.number}</small></li>`).join("")}
          ${chosen.length > shown.length ? `<li class="more">et ${chosen.length - shown.length} autre(s)</li>` : ""}</ul>
        <p class="help">${many ? "Leurs messages sont effacés" : "Ses messages sont effacés"} de GitHub, sans retour possible.</p>
        <label class="check"><input type="checkbox" name="branches" checked>
          <span>Supprimer aussi ${many ? "leurs branches" : "sa branche"} <code>claw/session-${many ? "…" : chosen[0].number}</code> dans ${many ? "les dépôts" : "le dépôt"}${open.length
            ? ` ; ${open.map(({ repo, pull }) => `la pull request n° ${pull.number} de ${escapeHtml(repo.split("/").pop())}`).join(", ")} ${open.length > 1 ? "seront fermées" : "sera fermée"}`
            : ""}</span></label>
        <label class="check"><input type="checkbox" name="runs" checked>
          <span>Effacer aussi ${many ? "leurs passages" : "ses passages"} dans GitHub Actions (journaux)</span></label>` : ""}
        ${running.length ? `<p class="help warn">${running.length > 1 ? `${running.length} sessions travaillent encore : elles ne sont pas supprimées` : "Une session travaille encore : elle n'est pas supprimée"}. Arrêtez-la d'abord.</p>` : ""}
        ${chosen.length ? '<button class="delete-confirm" type="submit">Supprimer</button>' : ""}
      </form>`;
  const form = sheet.querySelector(".delete-sessions");
  form.addEventListener("submit", async (event) => {
    event.preventDefault();
    const button = form.querySelector(".delete-confirm");
    button.disabled = true;
    const withBranches = form.elements.branches.checked;
    const withRuns = form.elements.runs.checked;
    const problems = [];
    let erased = 0;
    let hidden = 0;
    const runs = withRuns ? await state.github.allRuns(engine).catch(() => state.runs) : [];
    for (const [index, issue] of chosen.entries()) {
      button.textContent = many ? `Suppression ${index + 1}/${chosen.length}…` : "Suppression…";
      const repo = settingsOf(issue).repo;
      if (withBranches && repo) {
        try {
          const pull = pulls[index];
          if (pull && pull.state === "open" && !pull.merged_at) await state.github.closePull(repo, pull.number);
          await state.github.deleteBranch(repo, `claw/session-${issue.number}`);
        } catch (error) {
          problems.push(`branche de la session ${issue.number} : ${error.message}`);
        }
      }
      if (withRuns && repo) {
        const done = runs.filter((run) => run.display_title === runTitle(repo, issue.number) && run.status === "completed");
        for (const run of done) {
          await state.github.deleteRun(engine, run.id).catch((error) => problems.push(`passage ${run.id} : ${error.message}`));
        }
      }
      try {
        await state.github.deleteIssue(issue.node_id);
        erased += 1;
      } catch (error) {
        if (!(error instanceof GitHubError) || error.status !== 403) {
          problems.push(`session ${issue.number} : ${error.message}`);
          continue;
        }
        // GitHub n'autorise pas ce jeton à effacer : la session quitte l'appli (fermée, sans étiquette).
        try {
          await state.github.setIssueState(engine, issue.number, "closed");
          await state.github.removeLabel(engine, issue.number, LABEL);
          hidden += 1;
        } catch (fallback) {
          problems.push(`session ${issue.number} : ${fallback.message}`);
        }
      }
    }
    close();
    const removed = new Set(chosen.map((issue) => issue.number));
    state.selecting = false;
    state.selected.clear();
    const parts = [];
    if (erased) parts.push(erased > 1 ? `${erased} sessions supprimées` : "Session supprimée");
    if (hidden) {
      parts.push(`${hidden > 1 ? `${hidden} sessions retirées` : "session retirée"} de l'appli : GitHub ne permet pas à ce jeton d'effacer une issue (elle reste fermée dans les issues du moteur, où « Delete issue » l'efface)`);
    }
    if (problems.length) parts.push(`à revoir : ${problems.slice(0, 3).join(" ; ")}`);
    toast(parts.join(" ; ") || "Rien à supprimer", problems.length ? "error" : undefined);
    if (state.session && removed.has(state.session.number)) {
      location.hash = "#/";
    } else if (!$("#drawer").hidden) {
      renderDrawer();
    } else if (!state.session) {
      showHome();
    }
  });
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
          ${active && run ? `<button class="small-link stop" data-action="stop">${ICON.stop}<span>Arrêter</span></button>` : ""}
          ${run ? `<a href="${escapeHtml(run.html_url)}" target="_blank" rel="noopener noreferrer" class="small-link">${ICON.external}<span>Job</span></a>` : ""}
          ${!active ? `<button class="small-link" data-action="retry">${ICON.retry}<span>Relancer</span></button>` : ""}
          ${session.issue.state === "open" ? `<button class="small-link" data-action="archive">${ICON.archive}<span>Archiver</span></button>` : ""}
          ${!active ? `<button class="small-link" data-action="delete">${ICON.trash}<span>Supprimer</span></button>` : ""}
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
  // Deux appuis pour arrêter (pas de fenêtre de confirmation, comme le reste de l'appli).
  view.querySelector('[data-action="stop"]')?.addEventListener("click", async (event) => {
    const button = event.currentTarget;
    if (!button.classList.contains("confirm")) {
      button.classList.add("confirm");
      button.querySelector("span").textContent = "Confirmer l'arrêt";
      setTimeout(() => {
        if (!button.isConnected) return;
        button.classList.remove("confirm");
        button.querySelector("span").textContent = "Arrêter";
      }, 4000);
      return;
    }
    button.disabled = true;
    try {
      await state.github.cancelRun(state.settings.engine, run.id);
      toast("Arrêt demandé : ce qui est fait reste sur la branche");
      await pollSession(true);
    } catch (error) {
      button.disabled = false;
      failure(error);
    }
  });
  view.querySelector('[data-action="delete"]')?.addEventListener("click", () => openDeleteSessions([session.issue]));
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
          Pour créer des dépôts depuis l'appli : « All repositories » et Administration en lecture et écriture.
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

/** La zone de saisie est fixée en bas : le contenu garde en dessous la place qu'elle prend
 * vraiment (deux lignes de consigne, encoche du téléphone), sinon elle couvre ses boutons. */
function updateComposerSpace() {
  const composer = $("#composer");
  const space = composer.hidden ? 0 : composer.offsetHeight;
  document.documentElement.style.setProperty("--composer-space", `${space}px`);
}

function watchComposer() {
  if ("ResizeObserver" in window) new ResizeObserver(updateComposerSpace).observe($("#composer"));
  window.addEventListener("resize", updateComposerSpace);
  updateComposerSpace();
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
  watchComposer();
  setupDrawerSelection();
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
