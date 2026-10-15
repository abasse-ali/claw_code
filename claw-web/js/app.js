// Appli web de Claw : choisir un dépôt, une branche et un modèle gratuit, décrire une
// tâche ; Claw y travaille dans GitHub Actions (workflow claw-web.yml du dépôt du moteur),
// pousse une branche, ouvre la pull request et répond. Chaque session est une issue du
// moteur ; l'appli la suit en direct (progression, réponse, pull request).

import { ANSWER, GitHub, GitHubError, LABEL, PROGRESS } from "./github.js";
import { escapeHtml, renderMarkdown } from "./markdown.js";
import { sealedBox } from "./sealed-box.js";
import { LOG, changesHtml, decodeLog, transcriptHtml } from "./transcript.js";
import { highlightLines, languageOf } from "./highlight.js";
import { accountStatus, freeModels, resetTime } from "./openrouter.js";

const STORE = "claw-web.settings.v1";
const DEFAULT_ENGINE = "abasse-ali/claw-sessions";
// Moteurs déplacés : les sessions de claw_code (devenu public) vivent dans un dépôt privé.
const MOVED_ENGINES = { "abasse-ali/claw_code": "abasse-ali/claw-sessions" };
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
  globe: '<svg viewBox="0 0 24 24" aria-hidden="true"><circle cx="12" cy="12" r="8.5"/><path d="M3.5 12h17M12 3.5c2.5 2.6 2.5 14.4 0 17M12 3.5c-2.5 2.6-2.5 14.4 0 17"/></svg>',
  edit: '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M4 20h4L19 9l-4-4L4 16z"/><path d="M13.5 6.5l4 4"/></svg>',
  spark: '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M11 3.5l1.9 5.1 5.1 1.9-5.1 1.9L11 17.5l-1.9-5.1L4 10.5l5.1-1.9z"/><path d="M18.5 15l.8 2.2 2.2.8-2.2.8-.8 2.2-.8-2.2-2.2-.8 2.2-.8z"/></svg>',
  chat: '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M5 5h14a1.5 1.5 0 0 1 1.5 1.5v9A1.5 1.5 0 0 1 19 17h-8l-4.5 3.5V17H5a1.5 1.5 0 0 1-1.5-1.5v-9A1.5 1.5 0 0 1 5 5z"/></svg>',
  folder: '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M3.5 6.5a1.5 1.5 0 0 1 1.5-1.5h4l2 2.5h8a1.5 1.5 0 0 1 1.5 1.5v8.5a1.5 1.5 0 0 1-1.5 1.5H5a1.5 1.5 0 0 1-1.5-1.5z"/></svg>',
  file: '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M14 3H7a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8z"/><path d="M14 3v5h5"/></svg>',
  merge: '<svg viewBox="0 0 24 24" aria-hidden="true"><circle cx="6" cy="5" r="2"/><circle cx="6" cy="19" r="2"/><circle cx="18" cy="12" r="2"/><path d="M6 7v10M6 7c0 4 4 5 10 5"/></svg>',
  diff: '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M12 4v8M8 8h8M8 18h8"/><rect x="3.5" y="2.5" width="17" height="19" rx="2"/></svg>',
  image: '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M20.5 11.5l-8.2 8.2a5 5 0 0 1-7.1-7.1l8.6-8.6a3.3 3.3 0 0 1 4.7 4.7l-8.6 8.6a1.7 1.7 0 0 1-2.4-2.4l7.9-7.9"/></svg>',
  gauge: '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M4.5 17a8.5 8.5 0 1 1 15 0"/><path d="M12 13l4-4"/><circle cx="12" cy="13" r="1.2"/></svg>',
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
  // Images jointes au prochain message, et images déjà lues (chemin : adresse data:).
  attachments: [],
  images: new Map(),
  // Requêtes gratuites du jour et crédits (clé OpenRouter gardée sur l'appareil).
  quota: null,
  quotaError: "",
  quotaLoading: false,
  // Liste des sessions du tiroir, et sessions cochées pour être effacées.
  drawerSessions: [],
  selecting: false,
  selected: new Set(),
  // Bloc ouvert dans le menu (« sessions » ou « files ») et dossier parcouru dans « Fichiers ».
  drawerPane: "sessions",
  explorer: null,
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
// Images jointes à un message par l'appli : chemins dans la branche FILES_BRANCH du moteur.
const IMAGES_BLOCK = /<!--\s*claw-web:images\s+(\[[\s\S]*?\])\s*-->/g;
const FILES_BRANCH = "claw-web-fichiers";

/** Le texte sans ses blocs d'images, et les chemins des images. */
function withoutImages(text) {
  const paths = [];
  const clean = String(text || "").replace(IMAGES_BLOCK, (_, list) => {
    try {
      paths.push(...JSON.parse(list).filter((path) => typeof path === "string"));
    } catch {
      // bloc abîmé : ignoré
    }
    return "";
  }).trim();
  return { text: clean, paths };
}

/** Texte d'un message et vignettes de ses images (remplies ensuite par loadThumbs). */
function messageBody(text) {
  const { text: clean, paths } = withoutImages(text);
  const thumbs = paths.length
    ? `<div class="thumbs">${paths.map((path) => `<button class="thumb" data-image="${escapeHtml(path)}" aria-label="Voir l'image"><img alt=""></button>`).join("")}</div>`
    : "";
  return renderMarkdown(clean) + thumbs;
}

/** Remplit les vignettes de la vue (images lues une fois, gardées en mémoire). */
async function loadThumbs() {
  for (const button of document.querySelectorAll("#view .thumb[data-image]")) {
    const path = button.dataset.image;
    let url = state.images.get(path);
    if (!url) {
      try {
        const blob = await state.github.rawFile(state.settings.engine, path, FILES_BRANCH);
        url = await new Promise((resolve, reject) => {
          const reader = new FileReader();
          reader.onload = () => resolve(reader.result);
          reader.onerror = reject;
          reader.readAsDataURL(blob);
        });
        state.images.set(path, url);
      } catch {
        button.classList.add("missing");
        continue;
      }
    }
    const image = button.querySelector("img");
    if (image && image.getAttribute("src") !== url) image.src = url;
  }
}

/** Image en grand. */
function openImage(path) {
  const url = state.images.get(path);
  if (!url) return;
  const sheet = $("#sheet");
  sheet.innerHTML = `
    <div class="sheet-panel image-panel" role="dialog" aria-modal="true" aria-label="Image">
      <div class="sheet-head"><strong>Image jointe</strong><button class="icon-button" data-close aria-label="Fermer">${ICON.close}</button></div>
      <img class="full-image" alt="" src="${url}">
    </div>`;
  sheet.onclick = (event) => {
    if (event.target === sheet || event.target.closest("[data-close]")) {
      sheet.hidden = true;
      sheet.innerHTML = "";
    }
  };
  sheet.hidden = false;
}

/** Image choisie, réduite si besoin (2000 px au plus de côté, JPEG) : { name, base64, url }. */
async function prepareImage(file) {
  const keep = ["image/png", "image/jpeg", "image/webp", "image/gif"].includes(file.type) && file.size <= 1_500_000;
  const bitmap = await createImageBitmap(file);
  const scale = Math.min(1, 2000 / Math.max(bitmap.width, bitmap.height));
  let blob = file;
  let extension = (file.type.split("/")[1] || "jpg").replace("jpeg", "jpg");
  if (!keep || scale < 1) {
    const canvas = document.createElement("canvas");
    canvas.width = Math.round(bitmap.width * scale);
    canvas.height = Math.round(bitmap.height * scale);
    canvas.getContext("2d").drawImage(bitmap, 0, 0, canvas.width, canvas.height);
    blob = await new Promise((resolve) => canvas.toBlob(resolve, "image/jpeg", 0.85));
    extension = "jpg";
  }
  const url = await new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(reader.result);
    reader.onerror = reject;
    reader.readAsDataURL(blob);
  });
  return { name: file.name || `image.${extension}`, extension, base64: url.split(",")[1], url };
}

/** Dépose les images jointes sur la branche FILES_BRANCH du moteur ; le bloc à ajouter au message. */
async function uploadAttachments() {
  if (!state.attachments.length) return "";
  const engine = state.settings.engine;
  await state.github.ensureBranch(engine, FILES_BRANCH, state.engineRef || "main");
  const folder = `uploads/${new Date().toISOString().replace(/[-:]/g, "").replace(/\..*/, "").replace("T", "-")}-${Math.random().toString(36).slice(2, 8)}`;
  const paths = [];
  for (const [index, image] of state.attachments.entries()) {
    const path = `${folder}/${index + 1}.${image.extension}`;
    await state.github.putFile(engine, path, image.base64, "Image jointe depuis l'appli Claw", FILES_BRANCH);
    state.images.set(path, image.url);
    paths.push(path);
  }
  return `\n\n<!-- claw-web:images ${JSON.stringify(paths)} -->`;
}

function renderAttachments() {
  const row = document.querySelector("#composer .attachments");
  if (!row) return;
  row.hidden = !state.attachments.length;
  row.innerHTML = state.attachments.map((image, index) => `
    <span class="attachment"><img alt="" src="${image.url}"><button type="button" data-remove="${index}" aria-label="Retirer l'image">${ICON.close}</button></span>`).join("");
  const send = document.querySelector("#composer .send");
  const textarea = document.querySelector("#composer textarea");
  if (send && textarea) textarea.dispatchEvent(new Event("input"));
}
// Numéro et branche d'une session, notés par l'appli (les numéros d'issue de GitHub ne
// reculent jamais, même après une suppression) ; d'anciennes sessions n'ont que l'issue.
const numberOf = (issue) => Number(settingsOf(issue).n) || issue.number;
const branchOf = (issue) => settingsOf(issue).branch || `claw/session-${issue.number}`;

/** Numéro de la prochaine session : un de plus que le plus grand, 1 s'il n'y en a plus. */
function nextNumber(sessions) {
  return sessions.reduce((most, issue) => Math.max(most, Number(settingsOf(issue).n) || 0), 0) + 1;
}

/** Sessions d'avant la numérotation de l'appli : numérotées une fois, dans l'ordre de création,
 * en gardant leur branche (le travail y est). */
async function numberOldSessions(sessions) {
  const old = sessions.filter((issue) => !settingsOf(issue).n)
    .sort((a, b) => (a.created_at < b.created_at ? -1 : 1));
  let next = nextNumber(sessions);
  for (const issue of old) {
    const settings = { ...settingsOf(issue), n: next, branch: branchOf(issue) };
    const body = (issue.body || "").replace(SETTINGS_BLOCK, `<!-- claw-web ${JSON.stringify(settings)} -->`);
    try {
      await state.github.setIssueBody(state.settings.engine, issue.number, body);
      issue.body = body;
      next += 1;
    } catch {
      return;
    }
  }
}
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
/** Modèles gratuits utilisables, classés comme Claw les classe : `ranked` (le premier est
 * celui d'« Auto »), puis `others`. */
async function loadModels() {
  if (state.models) return state.models;
  try {
    const { ranked, others } = await freeModels();
    state.models = [
      ...ranked.map((model, index) => ({ ...model, rank: index + 1 })),
      ...others.map((model) => ({ ...model, rank: 0 })),
    ];
  } catch {
    state.models = [];
  }
  return state.models;
}

const contextLabel = (tokens) => (tokens >= 1000000 ? `${Math.round(tokens / 100000) / 10} M` : `${Math.round(tokens / 1024)} k`)
  .replace(".", ",");

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
  return openBranches({
    repo: state.settings.repo,
    current: state.settings.branch,
    title: "Branche de départ",
    onPick: (value) => {
      state.settings.branch = value;
      saveSettings();
      renderComposer();
    },
  });
}

// --- Branches : choisir, créer, renommer -----------------------------------------------------

/** Ce qui ne va pas dans un nom de branche (règles de Git), ou "". */
function branchProblem(name, branches) {
  if (!name) return "Donnez un nom à la branche";
  if (branches.includes(name)) return "Une branche porte déjà ce nom";
  if (/[\s~^:?*[\\]|\.\.|@\{|\/\/|^[-/.]|[/.]$|\.lock$|\/\./.test(name)) {
    return "Nom refusé par Git : ni espace, ni ~ ^ : ? * [ \\, ni « .. », ni « / » au début ou à la fin";
  }
  return "";
}

/** Branches d'un dépôt : en choisir une, en créer une à partir d'une autre, en renommer une. */
async function openBranches({ repo, current, title, onPick }) {
  let branches;
  let fallback = "";
  try {
    const [names, info] = await Promise.all([loadBranches(repo), state.github.repository(repo)]);
    branches = [...names];
    fallback = info.default_branch || "";
  } catch (error) {
    failure(error);
    return;
  }
  const sheet = $("#sheet");
  sheet.innerHTML = `
    <div class="sheet-panel" role="dialog" aria-modal="true" aria-label="${escapeHtml(title)}">
      <div class="sheet-head">
        <strong>${escapeHtml(title)}</strong>
        <button class="icon-button" data-close aria-label="Fermer">${ICON.close}</button>
      </div>
      <p class="help sheet-sub">${ICON.repo}<span>${escapeHtml(repo)}</span></p>
      <div class="branch-body"></div>
    </div>`;
  const body = sheet.querySelector(".branch-body");
  const close = () => {
    sheet.hidden = true;
    sheet.innerHTML = "";
  };
  const showList = () => {
    body.innerHTML = `
      <button type="button" class="sheet-item pinned" data-new><span class="sheet-label">${ICON.plus}<span>Nouvelle branche</span></span>
        <small>à partir de ${escapeHtml(current || fallback)} ou d'une autre</small></button>
      ${branches.length > 8 ? '<input class="sheet-search" type="search" placeholder="Rechercher une branche" autocomplete="off">' : ""}
      <ul class="sheet-list"></ul>`;
    const list = body.querySelector(".sheet-list");
    const draw = (filter = "") => {
      const words = filter.toLowerCase().split(/\s+/).filter(Boolean);
      const shown = branches.filter((name) => words.every((word) => name.toLowerCase().includes(word)));
      list.innerHTML = shown.slice(0, 300).map((name) => `
        <li class="branch-row">
          <button type="button" class="sheet-item${name === current ? " current" : ""}" data-pick="${escapeHtml(name)}">
            <span class="sheet-label">${ICON.branch}<span>${escapeHtml(name)}</span></span>
            ${name === fallback ? "<small>branche par défaut</small>" : ""}
          </button>
          <button type="button" class="icon-button" data-rename="${escapeHtml(name)}" aria-label="Renommer ${escapeHtml(name)}">${ICON.edit}</button>
        </li>`).join("") || '<li class="empty">Rien ne correspond.</li>';
    };
    draw();
    const search = body.querySelector(".sheet-search");
    if (search) search.oninput = () => draw(search.value);
  };
  // Formulaire de création (`old` vide) ou de renommage de la branche `old`.
  const showForm = (old) => {
    const creating = !old;
    const from = current || fallback;
    body.innerHTML = `
      <form class="branch-form" autocomplete="off">
        <label>${creating ? "Nom de la nouvelle branche" : `Nouveau nom pour <code>${escapeHtml(old)}</code>`}
          <input name="name" type="text" required maxlength="200" spellcheck="false" autocapitalize="none" autocorrect="off"
            placeholder="ma-branche" value="${escapeHtml(old)}">
        </label>
        ${creating ? `<label>À partir de
          <select name="from">${branches.map((name) => `<option value="${escapeHtml(name)}"${name === from ? " selected" : ""}>${escapeHtml(name)}</option>`).join("")}</select>
        </label>` : `<p class="help">${old === fallback ? "C'est la branche par défaut du dépôt : GitHub demande au jeton l'autorisation Administration (lecture et écriture) pour la renommer. " : ""}GitHub fait suivre les pull requests qui arrivent sur cette branche, et les sessions de l'appli qui y travaillent passent au nouveau nom.</p>`}
        <div class="form-actions">
          <button type="button" class="small-link" data-back>${ICON.back}<span>Retour</span></button>
          <button class="primary" type="submit">${creating ? "Créer la branche" : "Renommer"}</button>
        </div>
      </form>`;
    const form = body.querySelector("form");
    const input = form.elements.name;
    input.focus();
    if (!creating) input.select();
    form.addEventListener("submit", async (event) => {
      event.preventDefault();
      const name = input.value.trim();
      if (!creating && name === old) {
        showList();
        return;
      }
      const problem = branchProblem(name, branches);
      if (problem) {
        toast(problem, "error");
        return;
      }
      const button = form.querySelector("button[type=submit]");
      button.disabled = true;
      button.textContent = creating ? "Création…" : "Renommage…";
      try {
        if (creating) {
          const start = form.elements.from.value;
          await state.github.createBranch(repo, name, start);
          branches.push(name);
          state.branches.set(repo, [...branches]);
          toast(`Branche ${name} créée à partir de ${start}`);
          close();
          onPick(name);
          return;
        }
        await state.github.renameBranch(repo, old, name);
        branches.splice(branches.indexOf(old), 1, name);
        state.branches.set(repo, [...branches]);
        if (current === old) current = name;
        if (fallback === old) fallback = name;
        const moved = await followRenamedBranch(repo, old, name);
        toast(`Branche ${old} renommée en ${name}${moved ? ` ; ${moved > 1 ? `${moved} sessions suivent` : "sa session suit"} le nouveau nom` : ""}`);
        showList();
      } catch (error) {
        button.disabled = false;
        button.textContent = creating ? "Créer la branche" : "Renommer";
        failure(error);
      }
    });
  };
  sheet.onclick = (event) => {
    if (event.target === sheet || event.target.closest("[data-close]")) {
      close();
      return;
    }
    if (event.target.closest("[data-new]")) {
      showForm("");
      return;
    }
    const rename = event.target.closest("[data-rename]");
    if (rename) {
      showForm(rename.dataset.rename);
      return;
    }
    if (event.target.closest("[data-back]")) {
      showList();
      return;
    }
    const pick = event.target.closest("[data-pick]");
    if (pick) {
      close();
      onPick(pick.dataset.pick);
    }
  };
  showList();
  sheet.hidden = false;
}

/** Après un renommage : la zone de saisie, le menu « Fichiers » et les sessions de l'appli qui
 * utilisent l'ancien nom (branche de travail ou de départ) passent au nouveau. */
async function followRenamedBranch(repo, old, name) {
  if (state.settings.repo === repo && state.settings.branch === old) {
    state.settings.branch = name;
    saveSettings();
    const chip = document.querySelector('#composer [data-action="branch"] span');
    if (chip) chip.textContent = name;
  }
  if (state.explorer && state.explorer.repo === repo && state.explorer.ref === old) state.explorer.ref = name;
  let moved = 0;
  for (const issue of await state.github.sessions(state.settings.engine)) {
    const settings = settingsOf(issue);
    const branch = branchOf(issue);
    if (settings.repo !== repo || (branch !== old && settings.base !== old)) continue;
    const next = { ...settings, branch: branch === old ? name : branch };
    if (settings.base === old) next.base = name;
    const text = (issue.body || "").replace(SETTINGS_BLOCK, `<!-- claw-web ${JSON.stringify(next)} -->`);
    await state.github.setIssueBody(state.settings.engine, issue.number, text);
    moved += 1;
    if (state.session && state.session.number === issue.number) {
      state.session.issue.body = text;
      state.session.settings = settingsOf(state.session.issue);
      renderSessionView();
    }
  }
  return moved;
}

async function pickModel() {
  const models = await loadModels();
  const best = models.find((model) => model.rank === 1);
  const detail = (model) => [
    model.rank === 1 ? "le plus puissant" : model.rank ? `n°${model.rank} pour le code` : "non retenu pour le code",
    ...model.reasons,
    model.context ? `contexte ${contextLabel(model.context)}` : "",
  ].filter(Boolean).join(" · ");
  openPicker({
    title: "Modèle (gratuit)",
    placeholder: "Rechercher un modèle",
    current: state.settings.model || "",
    items: [
      { value: "", label: "Auto", detail: best ? `en ce moment ${best.short}, le plus puissant des gratuits pour le code (et les suivants s'il ne répond plus)` : "le meilleur modèle gratuit pour le code" },
      ...models.map((model) => ({ value: model.id, label: model.rank ? `${model.rank}. ${model.short}` : model.short, detail: detail(model) })),
    ],
    onPick: (value) => {
      state.settings.model = value;
      saveSettings();
      renderComposer();
    },
  });
}

// --- Équipe et Ultracode, comme dans l'interface locale de Claw -------------------------------

const AIDS = {
  team: {
    label: "Équipe",
    detail: "Chaque étape (comprendre, écrire le code, déboguer, relire) va au modèle gratuit le plus fort pour elle",
  },
  ultracode: {
    label: "Ultracode",
    detail: "Un agent neuf étudie la demande avant le travail, un vérificateur indépendant contrôle le résultat (une à trois dizaines de requêtes de plus)",
  },
};

/** Options du prochain passage (nouvelle tâche ou suivi), pour le moteur. */
const aidInputs = () => ({ team: state.settings.team ? "1" : "", ultracode: state.settings.ultracode ? "1" : "" });

function toggleAid(name) {
  state.settings[name] = !state.settings[name];
  saveSettings();
  drawAids();
}

/** Bouton des options à côté du trombone (allumé si l'une est active) et les cases de son menu. */
function drawAids() {
  const button = document.querySelector("#composer .aids");
  if (!button) return;
  const on = Object.keys(AIDS).filter((name) => state.settings[name]);
  button.classList.toggle("on", on.length > 0);
  button.setAttribute("aria-label", on.length ? `Options actives : ${on.map((name) => AIDS[name].label).join(" et ")}` : "Options Équipe et Ultracode");
  for (const item of document.querySelectorAll("#composer .aid-menu [data-aid]")) {
    item.setAttribute("aria-checked", String(Boolean(state.settings[item.dataset.aid])));
  }
}

function setAidMenu(open) {
  const menu = document.querySelector("#composer .aid-menu");
  const button = document.querySelector("#composer .aids");
  if (!menu || !button) return;
  menu.hidden = !open;
  button.setAttribute("aria-expanded", String(open));
}

// --- Requêtes gratuites ------------------------------------------------------------------------

/** Compteur de la barre du haut : requêtes gratuites restantes / limite du jour, en vert,
 * orange ou rouge selon ce qu'il reste ; « Quota ? » sans clé gardée (le détail explique). */
function showQuotaLabel() {
  const pill = $("#quota-button");
  if (!pill) return;
  pill.hidden = !state.github;
  pill.className = "quota-meter";
  const free = state.quota && state.quota.free;
  if (!free) {
    pill.classList.add("unknown");
    pill.innerHTML = `${ICON.gauge}<span>${state.settings.openrouterKey && !state.quotaError ? "…" : "Quota ?"}</span>`;
    pill.setAttribute("aria-label", "Requêtes gratuites du jour");
    return;
  }
  const share = free.limit ? free.remaining / free.limit : 0;
  pill.classList.add(share > 0.5 ? "ok" : share > 0.1 ? "warn" : "low");
  // Anneau qui se vide avec la journée (attribut SVG : la politique de sécurité refuse les styles en ligne).
  pill.innerHTML = `<svg class="ring" viewBox="0 0 20 20" aria-hidden="true"><circle class="track" cx="10" cy="10" r="7.5"/>`
    + `<circle class="level" cx="10" cy="10" r="7.5" pathLength="100" stroke-dasharray="${Math.round(share * 100)} 100" transform="rotate(-90 10 10)"/></svg>`
    + `<strong>${free.remaining}</strong><small>/${free.limit}</small>`;
  pill.setAttribute("aria-label", `${free.remaining} requêtes gratuites restantes sur ${free.limit} aujourd'hui`);
}

/** Relit le quota chez OpenRouter (au plus une fois par minute, sauf `force`). */
async function refreshQuota(force = false) {
  const key = state.settings.openrouterKey;
  if (!key || state.quotaLoading) return;
  if (!force && state.quota && Date.now() - state.quota.at < 60_000) return;
  state.quotaLoading = true;
  try {
    state.quota = await accountStatus(key);
    state.quotaError = "";
  } catch (error) {
    state.quotaError = error.message || String(error);
  } finally {
    state.quotaLoading = false;
    showQuotaLabel();
  }
}

/** Détail du quota gratuit du jour et des crédits ; sans clé gardée, comment l'afficher. */
async function openQuota() {
  const sheet = $("#sheet");
  const close = () => {
    sheet.hidden = true;
    sheet.innerHTML = "";
  };
  const draw = () => {
    const free = state.quota && state.quota.free;
    const balance = state.quota && state.quota.balance;
    let body;
    if (!state.settings.openrouterKey) {
      body = `<p>Pour voir ici les requêtes gratuites utilisées et restantes aujourd'hui, gardez une clé OpenRouter sur cet appareil.</p>
        <p class="help">Chaque réponse de Claw dit déjà combien son passage en a pris et combien il en reste.</p>
        <a class="primary" href="#/reglages">Ouvrir les réglages</a>`;
    } else if (free) {
      const share = free.limit ? Math.min(100, Math.round((free.used / free.limit) * 100)) : 0;
      body = `<div class="quota-big"><strong>${free.remaining}</strong> requêtes gratuites restantes aujourd'hui</div>
        <div class="quota-bar" role="img" aria-label="${share} % utilisées" data-share="${share}"><span></span></div>
        <p>${free.used} utilisées sur ${free.limit} · remise à zéro à ${escapeHtml(resetTime())} (minuit UTC)</p>
        ${balance !== null && balance !== undefined ? `<p class="help">Crédits du compte : ${balance.toFixed(2).replace(".", ",")} $ (les modèles gratuits n'en consomment pas ; au moins 10 $ de crédits achetés donnent 1000 requêtes gratuites par jour au lieu de 50).</p>` : ""}
        <p class="help">Lu chez OpenRouter ${since(new Date(state.quota.at).toISOString())}.</p>`;
    } else {
      body = `<p>${escapeHtml(state.quotaError || "OpenRouter n'a pas donné le quota gratuit de cette clé.")}</p>`;
    }
    sheet.innerHTML = `
      <div class="sheet-panel" role="dialog" aria-modal="true" aria-label="Requêtes gratuites">
        <div class="sheet-head">
          <strong>Requêtes gratuites</strong>
          <button class="icon-button" data-close aria-label="Fermer">${ICON.close}</button>
        </div>
        <div class="quota-sheet">${body}
          ${state.settings.openrouterKey ? `<button class="small-link" data-action="refresh-quota">${ICON.retry}<span>Actualiser</span></button>` : ""}
        </div>
      </div>`;
    // Largeur par le DOM : la politique de sécurité de l'appli refuse les styles écrits dans le HTML.
    const bar = sheet.querySelector(".quota-bar");
    if (bar) bar.querySelector("span").style.width = `${bar.dataset.share}%`;
  };
  sheet.onclick = async (event) => {
    if (event.target === sheet || event.target.closest("[data-close]") || event.target.closest('a[href="#/reglages"]')) {
      close();
      return;
    }
    if (event.target.closest('[data-action="refresh-quota"]')) {
      await refreshQuota(true);
      if (!sheet.hidden) draw();
    }
  };
  draw();
  sheet.hidden = false;
  if (state.settings.openrouterKey && (!state.quota || Date.now() - state.quota.at > 15_000)) {
    await refreshQuota(true);
    if (!sheet.hidden) draw();
  }
}

// --- Zone de saisie --------------------------------------------------------------------------

function renderComposer() {
  const composer = $("#composer");
  const session = state.session;
  const followUp = Boolean(session);
  const repo = followUp ? session.settings.repo : state.settings.repo;
  const branch = followUp ? branchOf(session.issue) : state.settings.branch;
  composer.hidden = !state.github || location.hash.startsWith("#/reglages") || location.hash.includes("/modifications")
    || location.hash.includes("/fichiers");
  // En suivi de session, le dépôt et la branche sont dans l'en-tête : seul le modèle se choisit.
  composer.innerHTML = `
    <div class="chips">
      ${followUp ? "" : `
      <button class="chip" data-action="repo">${ICON.repo}<span>${escapeHtml(repo || "Choisir un dépôt")}</span></button>
      <button class="chip" data-action="branch">${ICON.branch}<span>${escapeHtml(branch || "branche")}</span></button>`}
      <button class="chip" data-action="model">${ICON.model}<span>${escapeHtml(modelLabel(state.settings.model))}</span></button>
    </div>
    <div class="attachments" hidden></div>
    <form class="prompt" autocomplete="off">
      <button class="attach" type="button" aria-label="Joindre une image">${ICON.image}</button>
      <button class="aids" type="button" aria-haspopup="true" aria-expanded="false">${ICON.spark}</button>
      <div class="aid-menu" role="menu" hidden>${Object.entries(AIDS).map(([name, aid]) => `
        <button type="button" role="menuitemcheckbox" class="aid-item" data-aid="${name}" aria-checked="${Boolean(state.settings[name])}">
          <span class="aid-text"><strong>${aid.label}</strong><small>${escapeHtml(aid.detail)}</small></span>
          <span class="switch" aria-hidden="true"></span>
        </button>`).join("")}
      </div>
      <input class="file-input" type="file" accept="image/*" multiple hidden>
      <textarea rows="1" name="text" placeholder="${followUp ? "Répondez ou donnez une nouvelle consigne." : "Décrivez une tâche ou posez une question."}" aria-label="Message pour Claw"></textarea>
      <button class="send" type="submit" aria-label="Envoyer" disabled>${ICON.send}</button>
    </form>`;
  const textarea = composer.querySelector("textarea");
  const send = composer.querySelector(".send");
  const resize = () => {
    textarea.style.height = "auto";
    textarea.style.height = `${Math.min(textarea.scrollHeight, 220)}px`;
    send.disabled = state.sending || (!textarea.value.trim() && !state.attachments.length) || (!followUp && !repo);
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
    const text = textarea.value.trim() || (state.attachments.length ? "Voici l'image jointe." : "");
    if (!text) return;
    if (followUp) followUpSession(text, textarea);
    else startSession(text, textarea);
  });
  composer.querySelector('[data-action="repo"]')?.addEventListener("click", pickRepo);
  composer.querySelector('[data-action="branch"]')?.addEventListener("click", pickBranch);
  composer.querySelector('[data-action="model"]').addEventListener("click", pickModel);
  const picker = composer.querySelector(".file-input");
  composer.querySelector(".attach").addEventListener("click", () => picker.click());
  picker.addEventListener("change", async () => {
    for (const file of [...picker.files].slice(0, 6 - state.attachments.length)) {
      try {
        state.attachments.push(await prepareImage(file));
      } catch {
        toast(`${file.name} : image illisible (png, jpg, webp ou gif)`, "error");
      }
    }
    picker.value = "";
    renderAttachments();
  });
  composer.querySelector(".attachments").addEventListener("click", (event) => {
    const remove = event.target.closest("[data-remove]");
    if (!remove) return;
    state.attachments.splice(Number(remove.dataset.remove), 1);
    renderAttachments();
  });
  renderAttachments();
  composer.querySelector(".aids").addEventListener("click", () => setAidMenu(composer.querySelector(".aid-menu").hidden));
  for (const item of composer.querySelectorAll(".aid-menu [data-aid]")) item.addEventListener("click", () => toggleAid(item.dataset.aid));
  drawAids();
  refreshQuota();
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
    const images = await uploadAttachments();
    const firstLine = text.split("\n")[0].trim();
    const title = firstLine.length > 72 ? `${firstLine.slice(0, 71)}…` : firstLine;
    // Numéro suivant de l'appli ; sa branche, si une ancienne du même nom reste dans le dépôt,
    // prend un suffixe (jamais le travail d'une autre session).
    const sessions = await state.github.sessions(engine);
    const n = nextNumber(sessions);
    let work = `claw/session-${n}`;
    for (let suffix = 2; suffix < 20 && await state.github.branchExists(repo, work); suffix += 1) {
      work = `claw/session-${n}-${suffix}`;
    }
    const block = `<!-- claw-web ${JSON.stringify({ repo, base: branch, model: model || "", n, branch: work })} -->`;
    const issue = await state.github.createIssue(engine, title, `${text}${images}\n\n${block}`);
    state.attachments = [];
    await state.github.dispatch(engine, state.engineRef,
      { repo, base: branch || "main", session: String(issue.number), model: model || "", ...aidInputs() });
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
    const images = await uploadAttachments();
    await state.github.comment(state.settings.engine, session.number, `${text}${images}`);
    state.attachments = [];
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
    ...aidInputs(),
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
        <strong><span class="num">#${numberOf(issue)}</span> ${escapeHtml(issue.title)}</strong>
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
    toggle.hidden = state.drawerPane !== "sessions" || (!state.drawerSessions.length && !state.selecting);
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
    await numberOldSessions(sessions);
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
  showDrawerPane(state.drawerPane);
}

/** Menu : la liste des sessions, ou les dossiers et fichiers d'une branche. */
function showDrawerPane(pane) {
  state.drawerPane = pane;
  for (const tab of document.querySelectorAll("#drawer .drawer-tab")) {
    tab.classList.toggle("current", tab.dataset.pane === pane);
    tab.setAttribute("aria-pressed", String(tab.dataset.pane === pane));
  }
  for (const element of document.querySelectorAll("#drawer .pane")) element.hidden = element.dataset.pane !== pane;
  if (pane === "files") {
    state.selecting = false;
    state.selected.clear();
    drawDrawerList();
    renderExplorer();
  } else {
    renderDrawer();
  }
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
    return repo ? state.github.pull(repo, branchOf(issue)).catch(() => null) : null;
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
            <small>${escapeHtml(settingsOf(issue).repo || "?")} · ${escapeHtml(branchOf(issue))}</small></li>`).join("")}
          ${chosen.length > shown.length ? `<li class="more">et ${chosen.length - shown.length} autre(s)</li>` : ""}</ul>
        <p class="help">${many ? "Leurs messages sont effacés" : "Ses messages sont effacés"} de GitHub, sans retour possible.</p>
        <label class="check"><input type="checkbox" name="branches" checked>
          <span>Supprimer aussi ${many ? "leurs branches" : "sa branche"} <code>${many ? "claw/session-…" : escapeHtml(branchOf(chosen[0]))}</code> dans ${many ? "les dépôts" : "le dépôt"}${open.length
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
          await state.github.deleteBranch(repo, branchOf(issue));
        } catch (error) {
          problems.push(`branche de la session ${numberOf(issue)} : ${error.message}`);
        }
      }
      if (withRuns && repo) {
        const done = runs.filter((run) => run.display_title === runTitle(repo, issue.number) && run.status === "completed");
        for (const run of done) {
          await state.github.deleteRun(engine, run.id).catch((error) => problems.push(`passage ${run.id} : ${error.message}`));
        }
      }
      // Images jointes à la session (tâche et messages) : effacées de la branche des fichiers.
      try {
        const notes = await state.github.comments(engine, issue.number);
        const paths = [issue.body, ...notes.map((note) => note.body)].flatMap((text) => withoutImages(text).paths);
        for (const path of paths) await state.github.deleteFile(engine, path, "Image d'une session supprimée", FILES_BRANCH);
      } catch (error) {
        problems.push(`images de la session ${numberOf(issue)} : ${error.message}`);
      }
      try {
        await state.github.deleteIssue(issue.node_id);
        erased += 1;
      } catch (error) {
        if (!(error instanceof GitHubError) || error.status !== 403) {
          problems.push(`session ${numberOf(issue)} : ${error.message}`);
          continue;
        }
        // GitHub n'autorise pas ce jeton à effacer : la session quitte l'appli (fermée, sans étiquette).
        try {
          await state.github.setIssueState(engine, issue.number, "closed");
          await state.github.removeLabel(engine, issue.number, LABEL);
          hidden += 1;
        } catch (fallback) {
          problems.push(`session ${numberOf(issue)} : ${fallback.message}`);
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
    await numberOldSessions(sessions);
    const recent = view.querySelector(".recent");
    if (recent) {
      recent.innerHTML = sessions.filter((issue) => issue.state === "open").slice(0, 8).map(sessionItem).join("")
        || '<li class="empty">Décrivez une tâche ci-dessous : Claw la fera dans le dépôt choisi et proposera une pull request.</li>';
    }
  } catch (error) {
    failure(error);
  }
}

/** « engine » (progression, journal), « answer » (réponse de Claw) ou « message ». */
function kindOf(body) {
  const text = String(body || "").trim();
  if (text.includes(PROGRESS) || text.startsWith(LOG)) return "engine";
  return text.includes(ANSWER) ? "answer" : "message";
}

/** Numéro du passage d'un commentaire de progression (lien « Détail du job »). */
const runOf = (body) => Number((String(body).match(/\/actions\/runs\/(\d+)/) || [])[1] || 0);

/** Événements du journal détaillé d'un passage, toutes parties dans l'ordre. */
function journalOf(run) {
  const logs = [...(state.session.logs || new Map()).values()].filter((log) => log.run === run);
  return logs.sort((a, b) => a.page - b.page).flatMap((log) => log.events);
}

function messageHtml(comment, isLastProgress) {
  const body = comment.body || "";
  if (body.startsWith(LOG)) return "";
  if (body.includes(PROGRESS)) {
    const run = runOf(body);
    const events = run ? journalOf(run) : [];
    if (events.length) {
      const head = body.split("\n").find((line) => line.startsWith("**Claw")) || "";
      return `<div class="progress-card journal">${renderMarkdown(head)}${transcriptHtml(events, { key: `run${run}`, live: isLastProgress })}</div>`;
    }
    return `<div class="progress-card${isLastProgress ? "" : " old"}">${renderMarkdown(body)}</div>`;
  }
  if (body.includes(ANSWER)) {
    return `<article class="message claw"><div class="who">Claw</div>${renderMarkdown(body)}</article>`;
  }
  return `<article class="message user"><div class="who">${escapeHtml(comment.user ? comment.user.login : "Vous")}</div>${messageBody(body)}</article>`;
}

/** Onglets d'une session : la discussion, ses modifications (les fichiers sont dans le menu). */
function sessionTabs(number, current) {
  const tabs = [
    ["discussion", `#/session/${number}`, ICON.chat, "Discussion"],
    ["modifications", `#/session/${number}/modifications`, ICON.diff, "Modifications"],
  ];
  return `<nav class="session-tabs" aria-label="Session">${tabs.map(([key, href, icon, label]) => `
    <a class="session-tab${key === current ? " current" : ""}" href="${href}"${key === current ? ' aria-current="page"' : ""}>${icon}<span>${label}</span></a>`).join("")}</nav>`;
}

/** En-tête de l'onglet Modifications : titre de la session (une fois lue), onglets. */
function sessionTop(number, current, issue) {
  const title = issue ? `<h1><span class="num">#${numberOf(issue)}</span> ${escapeHtml(issue.title)}</h1>` : "";
  return `<header class="session-head">${title}${sessionTabs(number, current)}</header>`;
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
  const opened = new Map([...view.querySelectorAll("details[data-key]")].map((details) => [details.dataset.key, details.open]));
  // Dernier message sans passage lancé après lui (lancement raté, message écrit sur GitHub).
  const lastMessage = [...comments].reverse().find((comment) => kindOf(comment.body) === "message");
  const lastAnswer = [...comments].reverse().find((comment) => kindOf(comment.body) === "answer");
  const unsent = !active && Boolean(lastMessage) && (!lastAnswer || lastAnswer.created_at < lastMessage.created_at)
    && (!run || run.created_at < lastMessage.created_at);
  view.innerHTML = `
    <section class="session">
      <header class="session-head">
        <h1><span class="num">#${numberOf(session.issue)}</span> ${escapeHtml(session.issue.title)}</h1>
        <div class="session-meta">
          <a class="meta-link" href="https://github.com/${escapeHtml(settings.repo || "")}" target="_blank" rel="noopener noreferrer">${ICON.repo}<span>${escapeHtml(settings.repo || "?")}</span></a>
          <span class="meta-link">${ICON.branch}<span>${escapeHtml(branchOf(session.issue))}</span></span>
          ${pull ? `<a class="meta-link pull ${pullBadge}" href="${escapeHtml(pull.html_url)}" target="_blank" rel="noopener noreferrer">${ICON.pull}<span>PR #${pull.number} ${pullBadge}</span></a>` : ""}
          ${pull && pull.state === "open" && !pull.merged_at && !active ? `<button class="meta-link action" data-action="merge">${ICON.merge}<span>Fusionner</span></button>` : ""}
          ${session.site ? `<a class="meta-link site" href="${escapeHtml(session.site)}" target="_blank" rel="noopener noreferrer">${ICON.globe}<span>Site en ligne</span></a>` : ""}
        </div>
        ${sessionTabs(session.number, "discussion")}
      </header>
      <div class="messages">
        <article class="message user"><div class="who">${escapeHtml(session.issue.user ? session.issue.user.login : "Vous")}</div>${messageBody(taskOf(session.issue))}</article>
        ${comments.map((comment) => messageHtml(comment, comment === lastProgress && !answered)).join("")}
      </div>
      ${unsent ? `<div class="unsent">Ce message n'a pas encore été envoyé à Claw (aucun passage lancé depuis).
        <button class="small-link primary-link" data-action="retry">${ICON.retry}<span>Lancer le passage</span></button></div>` : ""}
      <div class="run-status ${active ? "active" : status ? status.key : ""}">
        ${active ? `<span class="spinner" aria-hidden="true"></span><span>${run ? escapeHtml(status.label) : "Lancement du passage"}${step}</span>` : ""}
        ${!active && status && status.key === "failed" && !answered ? `<span>Le dernier passage a échoué.</span>` : ""}
        <span class="run-actions">
          ${active && run ? `<button class="small-link stop" data-action="stop">${ICON.stop}<span>Arrêter</span></button>` : ""}
          ${run ? `<a href="${escapeHtml(run.html_url)}" target="_blank" rel="noopener noreferrer" class="small-link">${ICON.external}<span>Job</span></a>` : ""}
          ${!active && !unsent ? `<button class="small-link" data-action="retry">${ICON.retry}<span>Relancer</span></button>` : ""}
          ${!active ? `<button class="small-link" data-action="publish-site">${ICON.globe}<span>Mettre en ligne</span></button>` : ""}
          ${session.issue.state === "open" ? `<button class="small-link" data-action="archive">${ICON.archive}<span>Archiver</span></button>` : ""}
          ${!active ? `<button class="small-link" data-action="delete">${ICON.trash}<span>Supprimer</span></button>` : ""}
          <a href="${escapeHtml(session.issue.html_url)}" target="_blank" rel="noopener noreferrer" class="small-link">${ICON.external}<span>Session</span></a>
        </span>
      </div>
    </section>`;
  for (const details of view.querySelectorAll("details[data-key]")) {
    if (opened.has(details.dataset.key)) details.open = opened.get(details.dataset.key);
  }
  for (const retry of view.querySelectorAll('[data-action="retry"]')) {
    retry.addEventListener("click", async () => {
      // Un seul passage par appui : le bouton se désactive aussitôt (plusieurs appuis lançaient
      // plusieurs passages).
      for (const button of view.querySelectorAll('[data-action="retry"]')) button.disabled = true;
      try {
        await dispatchSession(session);
        toast("Passage lancé");
        await pollSession(true);
      } catch (error) {
        for (const button of view.querySelectorAll('[data-action="retry"]')) button.disabled = false;
        failure(error);
      }
    });
  }
  view.querySelector('[data-action="merge"]')?.addEventListener("click", async (event) => {
    const button = event.currentTarget;
    if (!button.classList.contains("confirm")) {
      button.classList.add("confirm");
      button.querySelector("span").textContent = "Confirmer la fusion";
      setTimeout(() => {
        if (!button.isConnected) return;
        button.classList.remove("confirm");
        button.querySelector("span").textContent = "Fusionner";
      }, 4000);
      return;
    }
    button.disabled = true;
    try {
      await state.github.mergePull(settings.repo, pull.number);
      toast(`Pull request n° ${pull.number} fusionnée dans ${settings.base || "main"}`);
      await pollSession(true);
    } catch (error) {
      button.disabled = false;
      failure(error);
    }
  });
  view.querySelector('[data-action="publish-site"]')?.addEventListener("click", () => openPublish(session));
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
  view.querySelector(".messages").addEventListener("click", (event) => {
    const thumb = event.target.closest(".thumb[data-image]");
    if (thumb) openImage(thumb.dataset.image);
  });
  loadThumbs();
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
    state.session = { number, issue: null, comments: [], settings: {}, pull: null, step: "", signature: "", logs: new Map() };
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
  // Après la première lecture, seuls les commentaires modifiés depuis la dernière (le journal
  // d'un passage en cours change toutes les 24 s : inutile de tout relire toutes les 4 s).
  const known = session.comments.length && !force;
  const since = known ? session.comments.reduce((last, comment) => (comment.updated_at > last ? comment.updated_at : last), "") : "";
  const [issue, fresh] = await Promise.all([
    state.github.issue(engine, session.number),
    state.github.comments(engine, session.number, since),
    refreshRuns(),
  ]);
  if (state.session !== session) return;
  const byId = new Map((known ? session.comments : []).map((comment) => [comment.id, comment]));
  for (const comment of fresh) byId.set(comment.id, comment);
  const comments = [...byId.values()].sort((a, b) => (a.created_at < b.created_at ? -1 : a.created_at > b.created_at ? 1 : a.id - b.id));
  await readJournals(session, comments);
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
    session.pull = session.settings.repo ? await state.github.pull(session.settings.repo, branchOf(issue)) : null;
  } catch {
    session.pull = null;
  }
  if (session.site === undefined && session.settings.repo) {
    session.site = null;
    state.github.pages(session.settings.repo)
      .then((pages) => {
        if (pages && pages.html_url && state.session === session) {
          session.site = pages.html_url;
          renderSessionView();
        }
      })
      .catch(() => {});
  }
  const signature = JSON.stringify([
    issue.updated_at, issue.state, comments.map((comment) => [comment.id, comment.updated_at]),
    run && [run.id, run.status, run.conclusion], session.step, session.pull && [session.pull.state, session.pull.merged_at],
  ]);
  // Fin du passage suivi : le quota gratuit a bougé ; pendant le passage, il se relit chaque minute.
  if (session.active && run && run.status === "completed") refreshQuota(true);
  else if (session.active) refreshQuota();
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

/** Décode les parties du journal détaillé nouvelles ou modifiées. */
async function readJournals(session, comments) {
  for (const comment of comments) {
    if (!(comment.body || "").startsWith(LOG)) continue;
    const cached = session.logs.get(comment.id);
    if (cached && cached.updated === comment.updated_at) continue;
    try {
      const log = await decodeLog(comment.body);
      if (log) session.logs.set(comment.id, { ...log, updated: comment.updated_at });
    } catch {
      // Navigateur sans décompression gzip, ou partie abîmée : la carte de progression reste.
      session.logs.delete(comment.id);
    }
  }
}

// --- Fichiers : dans le menu, et ouverts en grand ----------------------------------------------

const IMAGE_EXTENSIONS = ["png", "jpg", "jpeg", "gif", "webp", "svg", "ico", "bmp"];
const sizeLabel = (bytes) => (bytes >= 1048576 ? `${(bytes / 1048576).toFixed(1).replace(".", ",")} Mo`
  : bytes >= 1024 ? `${Math.round(bytes / 1024)} Ko` : `${bytes} o`);

/** Dossiers et fichiers de la branche de la session (ou de la branche de départ si la session
 * n'a encore rien publié) ; `path` : dossier ou fichier ouvert. */
/** Adresse d'un dossier ou d'un fichier d'une branche (`number` : la session d'où il vient). */
function filesHash({ repo, ref, path = "", number = null }) {
  const params = new URLSearchParams({ depot: repo, branche: ref });
  if (path) params.set("chemin", path);
  if (number) params.set("session", String(number));
  return `#/fichiers?${params}`;
}

function filesParams(hash) {
  if (!/^#\/fichiers(\?|$)/.test(hash)) return null;
  const params = new URLSearchParams(hash.split("?")[1] || "");
  return {
    repo: params.get("depot") || state.settings.repo || "",
    ref: params.get("branche") || state.settings.branch || "main",
    path: params.get("chemin") || "",
    number: Number(params.get("session")) || null,
  };
}

const sortEntries = (items) => [...items].sort((a, b) => (a.type === b.type ? a.name.localeCompare(b.name) : a.type === "dir" ? -1 : 1));

/** Ce que le bloc « Fichiers » du menu montre en s'ouvrant : la branche de la session ouverte
 * (celle de départ tant que Claw n'y a rien publié), le dossier du fichier ouvert, ou le dépôt
 * et la branche de la zone de saisie. Le dossier parcouru reste ouvert tant qu'on reste là. */
async function ensureExplorer() {
  const hash = location.hash || "#/";
  const files = filesParams(hash);
  if (files) {
    const same = state.explorer && state.explorer.repo === files.repo && state.explorer.ref === files.ref;
    if (!same) {
      state.explorer = { key: "fichiers", number: files.number, repo: files.repo, ref: files.ref, path: files.path.split("/").slice(0, -1).join("/") };
    }
    return;
  }
  const session = hash.match(/^#\/session\/(\d+)/);
  const key = session ? `session-${session[1]}` : `accueil-${state.settings.repo || ""}@${state.settings.branch || ""}`;
  if (state.explorer && state.explorer.key === key) return;
  if (!session) {
    state.explorer = { key, number: null, repo: state.settings.repo || "", ref: state.settings.branch || "main", path: "" };
    return;
  }
  const number = Number(session[1]);
  const issue = state.session && state.session.number === number ? state.session.issue
    : await state.github.issue(state.settings.engine, number);
  const settings = settingsOf(issue);
  let ref = branchOf(issue);
  if (settings.repo && !(await state.github.branchExists(settings.repo, ref))) ref = settings.base || "main";
  state.explorer = { key, number, repo: settings.repo || "", ref, path: "" };
}

async function renderExplorer() {
  const pane = $("#drawer .explorer");
  pane.innerHTML = '<p class="empty">Lecture…</p>';
  try {
    await ensureExplorer();
  } catch (error) {
    pane.innerHTML = '<p class="empty">Fichiers indisponibles.</p>';
    failure(error);
    return;
  }
  drawExplorer();
}

/** Bloc « Fichiers » du menu : dépôt et branche (à changer d'un appui), fil d'Ariane, dossiers
 * à parcourir sur place ; un fichier s'ouvre en grand. */
async function drawExplorer() {
  const pane = $("#drawer .explorer");
  const explorer = state.explorer;
  const head = `
    <div class="explorer-head">
      <button type="button" class="chip" data-explorer="repo">${ICON.repo}<span>${escapeHtml(explorer.repo || "Choisir un dépôt")}</span></button>
      ${explorer.repo ? `<button type="button" class="chip" data-explorer="branch">${ICON.branch}<span>${escapeHtml(explorer.ref)}</span></button>` : ""}
    </div>`;
  if (!explorer.repo) {
    pane.innerHTML = head;
    return;
  }
  const where = `${explorer.repo}@${explorer.ref}:${explorer.path}`;
  const name = explorer.repo.split("/").pop();
  const parts = explorer.path.split("/").filter(Boolean);
  const crumbs = `<p class="crumbs">${[
    parts.length ? `<button type="button" data-path="">${escapeHtml(name)}</button>` : `<span>${escapeHtml(name)}</span>`,
    ...parts.map((part, index) => (index === parts.length - 1 ? `<span>${escapeHtml(part)}</span>`
      : `<button type="button" data-path="${escapeHtml(parts.slice(0, index + 1).join("/"))}">${escapeHtml(part)}</button>`)),
  ].join('<span class="sep">/</span>')}</p>`;
  pane.innerHTML = `${head}${crumbs}<p class="empty">Lecture…</p>`;
  let body;
  try {
    const items = await state.github.contents(explorer.repo, explorer.path, explorer.ref);
    const entries = Array.isArray(items) ? sortEntries(items) : [];
    body = entries.length ? `<ul class="file-list">${entries.map((entry) => (entry.type === "dir"
      ? `<li><button type="button" class="file-row" data-path="${escapeHtml(entry.path)}">${ICON.folder}<span class="name">${escapeHtml(entry.name)}</span></button></li>`
      : `<li><a class="file-row" href="${escapeHtml(filesHash({ ...explorer, path: entry.path }))}">${ICON.file}<span class="name">${escapeHtml(entry.name)}</span><small>${sizeLabel(entry.size || 0)}</small></a></li>`)).join("")}</ul>`
      : '<p class="empty">Dossier vide.</p>';
  } catch (error) {
    if (!(error instanceof GitHubError) || error.status !== 404) failure(error);
    body = `<p class="empty">${/empty/i.test(error.message) ? "Dépôt vide : aucun fichier pour l'instant." : "Introuvable sur cette branche."}</p>`;
  }
  if (state.explorer !== explorer || `${explorer.repo}@${explorer.ref}:${explorer.path}` !== where) return;
  pane.innerHTML = `${head}${crumbs}${body}`;
}

function onExplorerClick(event) {
  if (!state.explorer) return;
  const folder = event.target.closest("[data-path]");
  if (folder) {
    state.explorer.path = folder.dataset.path;
    drawExplorer();
    return;
  }
  const action = event.target.closest("[data-explorer]");
  if (!action) return;
  if (action.dataset.explorer === "repo") {
    pickExplorerRepo();
    return;
  }
  openBranches({
    repo: state.explorer.repo,
    current: state.explorer.ref,
    title: "Branche",
    onPick: (name) => {
      state.explorer.ref = name;
      state.explorer.path = "";
      drawExplorer();
    },
  });
}

async function pickExplorerRepo() {
  try {
    const repos = await loadRepos();
    openPicker({
      title: "Dépôt",
      placeholder: "Rechercher un dépôt",
      current: state.explorer.repo,
      items: repos.map((repo) => ({ value: repo.full_name, label: repo.full_name, detail: `${repo.private ? "privé" : "public"} · ${since(repo.pushed_at)}` })),
      onPick: (value) => {
        const repo = repos.find((item) => item.full_name === value);
        Object.assign(state.explorer, { repo: value, ref: repo ? repo.default_branch : "main", path: "" });
        drawExplorer();
      },
    });
  } catch (error) {
    failure(error);
  }
}

/** Dossier ou fichier d'une branche, en grand (ouvert depuis le bloc « Fichiers » du menu). */
async function showFiles({ repo, ref, path, number }) {
  state.session = null;
  renderComposer();
  const view = $("#view");
  const back = number ? `<a class="back" href="#/session/${number}">${ICON.back}<span>Session</span></a>`
    : `<a class="back" href="#/">${ICON.back}<span>Accueil</span></a>`;
  view.innerHTML = `
    <section class="files">
      ${back}
      <h1>Fichiers</h1>
      <p class="empty">Lecture…</p>
    </section>`;
  const here = location.hash;
  try {
    if (!repo) throw new GitHubError(404, "Aucun dépôt choisi");
    const item = await state.github.contents(repo, path, ref);
    if (location.hash !== here) return;
    const link = (target) => escapeHtml(filesHash({ repo, ref, path: target, number }));
    const parts = path.split("/").filter(Boolean);
    const crumbs = [`<a href="${link("")}">${escapeHtml(repo.split("/").pop())}</a>`,
      ...parts.map((part, index) => (index === parts.length - 1 ? `<span>${escapeHtml(part)}</span>`
        : `<a href="${link(parts.slice(0, index + 1).join("/"))}">${escapeHtml(part)}</a>`))].join(" / ");
    let body;
    if (Array.isArray(item)) {
      const entries = sortEntries(item);
      body = entries.length ? `<ul class="file-list">${entries.map((entry) => `
        <li><a class="file-row" href="${link(entry.path)}">${entry.type === "dir" ? ICON.folder : ICON.file}
          <span class="name">${escapeHtml(entry.name)}</span>
          <small>${entry.type === "dir" ? "" : sizeLabel(entry.size || 0)}</small></a></li>`).join("")}</ul>`
        : '<p class="empty">Dossier vide.</p>';
    } else {
      body = await fileHtml(repo, item, ref);
    }
    if (location.hash !== here) return;
    view.querySelector(".files").innerHTML = `
      ${back}
      <h1>Fichiers</h1>
      <p class="crumbs">${crumbs} <span class="ref">${ICON.branch}${escapeHtml(ref)}</span></p>
      ${body}`;
    const toggle = view.querySelector('[data-action="source"]');
    if (toggle) {
      toggle.addEventListener("click", () => {
        const rendered = view.querySelector(".md-rendered");
        const source = view.querySelector(".md-source");
        rendered.hidden = !rendered.hidden;
        source.hidden = !source.hidden;
        toggle.querySelector("span").textContent = source.hidden ? "Voir la source" : "Voir le rendu";
      });
    }
  } catch (error) {
    if (error instanceof GitHubError && error.status === 404) {
      const section = view.querySelector(".files");
      if (section) section.querySelector(".empty").textContent = "Introuvable sur cette branche.";
      return;
    }
    failure(error);
  }
}

/** Un fichier : image affichée, Markdown mis en forme (source à côté), texte numéroté. */
async function fileHtml(repo, file, ref) {
  const extension = file.name.includes(".") ? file.name.split(".").pop().toLowerCase() : "";
  const head = `<p class="file-head"><strong>${escapeHtml(file.name)}</strong> <small>${sizeLabel(file.size || 0)}</small>
    <a class="small-link" href="${escapeHtml(file.html_url || "#")}" target="_blank" rel="noopener noreferrer">${ICON.external}<span>GitHub</span></a></p>`;
  const blob = await state.github.rawFile(repo, file.path, ref);
  if (IMAGE_EXTENSIONS.includes(extension)) {
    const url = await new Promise((resolve, reject) => {
      const reader = new FileReader();
      reader.onload = () => resolve(reader.result);
      reader.onerror = reject;
      reader.readAsDataURL(extension === "svg" ? new Blob([blob], { type: "image/svg+xml" }) : blob);
    });
    return `${head}<img class="file-image" alt="${escapeHtml(file.name)}" src="${url}">`;
  }
  const limit = 1_000_000;
  const text = await blob.slice(0, limit).text();
  if (text.includes("\u0000")) return `${head}<p class="empty">Fichier binaire : ouvrez-le sur GitHub.</p>`;
  const lines = text.replace(/\n$/, "").split("\n");
  const shown = lines.slice(0, 3000);
  const more = lines.length - shown.length;
  // Coloration dans le langage du fichier (d'après son nom), lignes affichées seulement.
  const colored = highlightLines(shown.join("\n"), languageOf(file.name));
  const code = `<div class="diff code-view">${colored.map((line, index) => `<div class="dl"><span class="no">${index + 1}</span><span class="tx">${line || " "}</span></div>`).join("")}
    ${more > 0 || blob.size > limit ? `<div class="dl gap">… la suite sur GitHub</div>` : ""}</div>`;
  if (extension === "md") {
    return `${head}<button class="small-link" data-action="source">${ICON.file}<span>Voir la source</span></button>
      <div class="md-rendered message">${renderMarkdown(text)}</div><div class="md-source" hidden>${code}</div>`;
  }
  return head + code;
}

// --- Mise en ligne (GitHub Pages) -------------------------------------------------------

/** Feuille de mise en ligne : fusion éventuelle, branche, dossier du site, visibilité. */
async function openPublish(session) {
  const sheet = $("#sheet");
  const repo = session.settings.repo;
  const base = session.settings.base || "main";
  const branch = branchOf(session.issue);
  const pull = session.pull;
  const close = () => {
    sheet.hidden = true;
    sheet.innerHTML = "";
  };
  sheet.innerHTML = `
    <div class="sheet-panel" role="dialog" aria-modal="true" aria-label="Mettre en ligne">
      <div class="sheet-head"><strong>Mettre en ligne (GitHub Pages)</strong>
        <button class="icon-button" data-close aria-label="Fermer">${ICON.close}</button></div>
      <p class="empty">Lecture du dépôt…</p>
    </div>`;
  sheet.onclick = (event) => {
    if (event.target === sheet || event.target.closest("[data-close]")) close();
  };
  sheet.hidden = false;
  try {
    const open = pull && pull.state === "open" && !pull.merged_at;
    const [info, pages, sessionBranch] = await Promise.all([
      state.github.repository(repo),
      state.github.pages(repo).catch(() => null),
      state.github.branchExists(repo, branch),
    ]);
    // Dossier du site sur une branche : docs/ s'il a un index.html, sinon la racine.
    const folderOf = async (ref) => ((await state.github.fileExists(repo, "docs/index.html", ref)) ? "/docs"
      : (await state.github.fileExists(repo, "index.html", ref)) ? "/" : null);
    const choices = [];
    if (open) choices.push({ value: "merge", label: `Fusionner la pull request n° ${pull.number}, puis publier ${base}`, ref: branch, recommended: true });
    if (sessionBranch && open) choices.push({ value: "branch", label: `Publier ${branch} telle quelle, sans fusion`, ref: branch });
    if (!open) choices.push({ value: "base", label: `Publier ${base}`, ref: base, recommended: true });
    for (const choice of choices) choice.folder = await folderOf(choice.ref);
    if (sheet.hidden) return;
    sheet.querySelector(".sheet-panel").innerHTML = `
      <div class="sheet-head"><strong>Mettre en ligne (GitHub Pages)</strong>
        <button class="icon-button" data-close aria-label="Fermer">${ICON.close}</button></div>
      <form class="publish-form">
        ${pages && pages.html_url ? `<p>Publié en ce moment : <a href="${escapeHtml(pages.html_url)}" target="_blank" rel="noopener noreferrer">${escapeHtml(pages.html_url)}</a>
          (${escapeHtml((pages.source && pages.source.branch) || "?")}, ${escapeHtml((pages.source && pages.source.path) || "/")}).</p>` : ""}
        <fieldset class="choices">
          ${choices.map((choice, index) => `
            <label class="check"><input type="radio" name="choice" value="${choice.value}"${index === 0 ? " checked" : ""}>
              <span>${escapeHtml(choice.label)}${choice.folder ? ` · site dans <code>${choice.folder === "/" ? "la racine" : "docs/"}</code>` : " · aucun <code>index.html</code> trouvé (racine ou docs/)"}</span></label>`).join("")}
        </fieldset>
        ${info.private ? `<p class="help warn">Ce dépôt est privé : sur un compte GitHub gratuit, GitHub Pages ne publie que les dépôts publics.</p>
          <label class="check"><input type="checkbox" name="public"><span>Rendre le dépôt public (son code devient visible par tous)</span></label>` : ""}
        <button class="primary" type="submit">Mettre en ligne</button>
        <p class="help result" hidden></p>
      </form>`;
    const form = sheet.querySelector(".publish-form");
    form.addEventListener("submit", async (event) => {
      event.preventDefault();
      const choice = choices.find((item) => item.value === form.elements.choice.value);
      const button = form.querySelector("button[type=submit]");
      const result = form.querySelector(".result");
      button.disabled = true;
      try {
        if (choice.value === "merge") {
          button.textContent = "Fusion…";
          await state.github.mergePull(repo, pull.number);
        }
        if (info.private && form.elements.public && form.elements.public.checked) {
          button.textContent = "Dépôt rendu public…";
          await state.github.setVisibility(repo, false);
        }
        button.textContent = "Publication…";
        const target = choice.value === "branch" ? branch : base;
        const published = await state.github.publishPages(repo, target, choice.folder || "/");
        const url = (published && published.html_url) || `https://${repo.split("/")[0]}.github.io/${repo.split("/")[1]}/`;
        session.site = url;
        result.hidden = false;
        result.innerHTML = `GitHub construit le site (une à deux minutes), puis il sera en ligne ici :
          <a href="${escapeHtml(url)}" target="_blank" rel="noopener noreferrer">${escapeHtml(url)}</a>`;
        button.textContent = "Mis en ligne";
        toast("Site publié sur GitHub Pages");
        pollSession(true).catch(() => {});
      } catch (error) {
        button.disabled = false;
        button.textContent = "Mettre en ligne";
        failure(error);
      }
    });
  } catch (error) {
    close();
    failure(error);
  }
}

// --- Modifications d'une session ----------------------------------------------------------

async function showChanges(number) {
  state.session = null;
  const view = $("#view");
  renderComposer();
  view.innerHTML = `
    <section class="changes">
      ${sessionTop(number, "modifications")}
      <p class="empty">Lecture des différences…</p>
    </section>`;
  try {
    const issue = await state.github.issue(state.settings.engine, number);
    const settings = settingsOf(issue);
    const branch = branchOf(issue);
    const section = view.querySelector(".changes");
    if (!section) return;
    let content;
    try {
      const compare = await state.github.compare(settings.repo, settings.base || "main", branch);
      content = compare.files && compare.files.length
        ? changesHtml(compare)
        : `<p class="empty">Aucune différence entre <code>${escapeHtml(branch)}</code> et <code>${escapeHtml(settings.base || "main")}</code>.</p>`;
    } catch (error) {
      if (!(error instanceof GitHubError) || error.status !== 404) throw error;
      content = `<p class="empty">Pas encore de branche <code>${escapeHtml(branch)}</code> dans ${escapeHtml(settings.repo || "")} : Claw n'y a encore rien publié.</p>`;
    }
    if (!location.hash.startsWith(`#/session/${number}/modifications`)) return;
    section.innerHTML = `
      ${sessionTop(number, "modifications", issue)}
      <p class="help"><code>${escapeHtml(branch)}</code> comparée à <code>${escapeHtml(settings.base || "main")}</code> dans
        <a href="https://github.com/${escapeHtml(settings.repo || "")}/compare/${escapeHtml(settings.base || "main")}...${escapeHtml(branch)}" target="_blank" rel="noopener noreferrer">${escapeHtml(settings.repo || "")}</a></p>
      ${content}`;
  } catch (error) {
    failure(error);
  }
}

// --- Réglages --------------------------------------------------------------------------------

/** Étiquette des sessions et secrets du moteur (chiffrés ici pour sa clé publique) ; rend les
 * noms des secrets posés. */
async function configureEngine(openrouterKey, shareToken) {
  const engine = state.settings.engine;
  await state.github.ensureLabel(engine);
  const key = await state.github.secretKey(engine);
  const secrets = [];
  if (openrouterKey) secrets.push(["OPENROUTER_API_KEY", openrouterKey]);
  if (shareToken) secrets.push(["CLAW_REPO_TOKEN", state.settings.token]);
  for (const [name, value] of secrets) {
    await state.github.putSecret(engine, name, sealedBox(value, key.key), key.key_id);
  }
  return secrets.map(([name]) => name);
}

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
          Pour mettre un site en ligne (GitHub Pages) : Pages en lecture et écriture.
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
        <label class="check"><input name="keep" type="checkbox" checked>
          <span>Garder aussi la clé OpenRouter sur cet appareil, pour afficher les requêtes gratuites utilisées et restantes</span></label>
        <p class="help">Les secrets partent chiffrés dans le navigateur, pour la clé publique du moteur. Champ vide : la clé OpenRouter gardée sur cet appareil, s'il y en a une.</p>
        <button class="primary" type="submit">Configurer le moteur</button>
      </form>
      <h2>Apparence</h2>
      <fieldset class="theme-choice">
        ${[["", "Sombre"], ["light", "Clair"], ["system", "Comme le téléphone"]].map(([value, label]) => `
          <label><input type="radio" name="theme" value="${value}"${(settings.theme || "") === value ? " checked" : ""}><span>${label}</span></label>`).join("")}
      </fieldset>
      <h2>Requêtes gratuites</h2>
      <form class="quota-form" autocomplete="off">
        <label>Clé OpenRouter gardée sur cet appareil
          <input name="key" type="password" placeholder="${settings.openrouterKey ? "clé enregistrée" : "sk-or-…"}" spellcheck="false">
        </label>
        <p class="help">Elle sert seulement à lire chez OpenRouter les requêtes gratuites du jour et les crédits, affichés près du choix du modèle. Elle reste dans ce navigateur. Conseil : créez pour l'appli une clé avec une limite de crédit de 0 $ (réglage « Credit limit » de la clé sur openrouter.ai) : elle ne peut rien dépenser.</p>
        <button class="primary" type="submit">Enregistrer la clé</button>
        ${settings.openrouterKey ? '<button class="small-link" type="button" data-action="forget-key">Effacer la clé de cet appareil</button>' : ""}
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
      showQuotaLabel();
      refreshQuota();
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
      try {
        const typed = form.openrouter.value.trim();
        if (typed && form.keep.checked) {
          state.settings.openrouterKey = typed;
          state.quota = null;
          saveSettings();
        }
        // Champ vide : la clé gardée sur cet appareil, s'il y en a une.
        const names = await configureEngine(typed || state.settings.openrouterKey || "", form.share.checked);
        form.openrouter.value = "";
        toast(names.length ? `Moteur configuré (${names.join(", ")})` : "Étiquette prête ; aucun secret envoyé");
        const list = $("#view .checks");
        if (list) list.innerHTML = await engineChecks();
      } catch (error) {
        failure(error);
      }
    });
    for (const radio of $("#view").querySelectorAll('input[name="theme"]')) {
      radio.addEventListener("change", () => {
        state.settings.theme = radio.value;
        saveSettings();
        applyTheme();
      });
    }
    $("#view .quota-form").addEventListener("submit", async (event) => {
      event.preventDefault();
      const key = event.target.key.value.trim();
      if (!key) return;
      try {
        state.quota = await accountStatus(key);
        state.quotaError = "";
        state.settings.openrouterKey = key;
        saveSettings();
        showQuotaLabel();
        const free = state.quota.free;
        toast(free ? `Clé enregistrée : ${free.remaining}/${free.limit} requêtes gratuites restantes aujourd'hui` : "Clé enregistrée");
        showSettings();
      } catch (error) {
        failure(error);
      }
    });
    $("#view [data-action=forget-key]")?.addEventListener("click", () => {
      delete state.settings.openrouterKey;
      state.quota = null;
      saveSettings();
      showQuotaLabel();
      toast("Clé OpenRouter effacée de cet appareil");
      showSettings();
    });
    $("#view [data-action=forget]").addEventListener("click", () => {
      delete state.settings.token;
      saveSettings();
      state.github = null;
      state.user = null;
      showQuotaLabel();
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
  const files = filesParams(hash);
  if (files) {
    showFiles(files);
    return;
  }
  const changes = hash.match(/^#\/session\/(\d+)\/modifications/);
  if (changes) {
    showChanges(Number(changes[1]));
    return;
  }
  const match = hash.match(/^#\/session\/(\d+)/);
  if (match) {
    showSession(Number(match[1]));
    return;
  }
  showHome();
}

/** Thème : sombre (par défaut), clair, ou celui du téléphone ; la barre du téléphone suit. */
function applyTheme() {
  const theme = state.settings.theme || "";
  if (theme) document.documentElement.dataset.theme = theme;
  else delete document.documentElement.dataset.theme;
  const meta = document.querySelector('meta[name="theme-color"]');
  if (meta) meta.content = getComputedStyle(document.documentElement).getPropertyValue("--bg").trim() || "#1b1a18";
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

/** Sessions et moteur déplacés dans un dépôt privé : le moteur y reçoit ses secrets (la clé
 * OpenRouter gardée ici, le jeton), ce que l'on fait d'habitude avec « Configurer le moteur ». */
async function announceMove(engine, reachable) {
  if (!reachable) {
    toast(`Vos sessions sont maintenant dans ${engine}, que ce jeton ne voit pas : sur GitHub, ajoutez ce dépôt au jeton (Repository access ; sa valeur ne change pas)`, "error");
    return;
  }
  try {
    const names = await configureEngine(state.settings.openrouterKey || "", true);
    toast(`Sessions et moteur déplacés dans ${engine}, dépôt privé ; secrets posés : ${names.join(", ")}${state.settings.openrouterKey ? "" : ". Il manque la clé OpenRouter : Réglages, Moteur"}`);
  } catch (error) {
    toast(`Sessions déplacées dans ${engine} ; secrets du moteur à poser dans Réglages, Moteur (${error.message})`, "error");
  }
}

async function boot() {
  applyTheme();
  const moved = MOVED_ENGINES[state.settings.engine];
  if (moved) {
    state.settings.engine = moved;
    state.repos = null;
    saveSettings();
  }
  $("#quota-button").addEventListener("click", openQuota);
  window.matchMedia("(prefers-color-scheme: light)").addEventListener("change", applyTheme);
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
  for (const tab of document.querySelectorAll("#drawer .drawer-tab")) tab.addEventListener("click", () => showDrawerPane(tab.dataset.pane));
  $("#drawer .explorer").addEventListener("click", onExplorerClick);
  // Le menu Équipe / Ultracode se ferme en touchant ailleurs, ou avec Échap.
  document.addEventListener("pointerdown", (event) => {
    if (!event.target.closest("#composer .aid-menu, #composer .aids")) setAidMenu(false);
  });
  document.addEventListener("keydown", (event) => {
    if (event.key === "Escape") setAidMenu(false);
  });
  window.addEventListener("hashchange", route);
  document.addEventListener("visibilitychange", () => {
    if (!document.hidden && state.session) pollSession().catch(failure);
    if (!document.hidden) refreshQuota();
  });
  if ("serviceWorker" in navigator) {
    navigator.serviceWorker.register("sw.js").catch(() => {});
  }
  if (state.settings.token) {
    const github = new GitHub(state.settings.token);
    try {
      state.user = await github.user();
      state.github = github;
      showQuotaLabel();
      refreshQuota();
      const engine = await github.repository(state.settings.engine).catch(() => null);
      if (engine) state.engineRef = engine.default_branch || "main";
      if (moved) announceMove(moved, Boolean(engine));
    } catch (error) {
      failure(error);
    }
  }
  route();
}

boot();
