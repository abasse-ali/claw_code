// Appli web de Claw : choisir un dépôt, une branche et un modèle gratuit, décrire une
// tâche ; Claw y travaille dans GitHub Actions (workflow claw-web.yml du dépôt du moteur),
// pousse une branche, ouvre la pull request et répond. Chaque session est une issue du
// moteur ; l'appli la suit en direct (progression, réponse, pull request).

import { ANSWER, GitHub, GitHubError, LABEL, PROGRESS } from "./github.js";
import { escapeHtml, renderMarkdown } from "./markdown.js";
import { sealedBox } from "./sealed-box.js";
import { LOG, changesHtml, decodeLog, photoLink, photoView, transcriptHtml, withoutCallMarkup } from "./transcript.js";
import { highlightLines, languageOf } from "./highlight.js";
import { accountStatus, allFreeModels, freeModels, resetTime } from "./openrouter.js";

const STORE = "claw-web.settings.v1";
const DEFAULT_ENGINE = "abasse-ali/claw-sessions";
// Moteurs déplacés : les sessions de claw_code (devenu public) vivent dans un dépôt privé.
const MOVED_ENGINES = { "abasse-ali/claw_code": "abasse-ali/claw-sessions" };
// Code de Claw et du moteur (entrée `claw_source` du workflow) : jamais supprimé depuis l'appli.
const CLAW_SOURCE = "abasse-ali/claw_code";
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
  server: '<svg viewBox="0 0 24 24" aria-hidden="true"><rect x="4" y="4" width="16" height="7" rx="1.5"/><rect x="4" y="13" width="16" height="7" rx="1.5"/><path d="M8 7.5h.01M8 16.5h.01"/></svg>',
  cloud: '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M7 18h10.5a4 4 0 0 0 .6-7.95A6 6 0 0 0 6.6 9.1 4.5 4.5 0 0 0 7 18Z"/></svg>',
  key: '<svg viewBox="0 0 24 24" aria-hidden="true"><circle cx="8" cy="15" r="4"/><path d="M11 12l8-8M16 7l2 2M14 9l2 2"/></svg>',
  chevron: '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="m9 6 6 6-6 6"/></svg>',
};

const state = {
  settings: loadSettings(),
  github: null,
  user: null,
  engineRef: "main",
  repos: null,
  branches: new Map(),
  models: null,
  // Secrets des fournisseurs à clé du moteur, { NVIDIA_API_KEY: true, … } (null : pas encore
  // demandés à GitHub).
  providerSecrets: null,
  runs: [],
  // Dépôt du moteur dont `runs` est la liste.
  runsEngine: "",
  session: null,
  pollTimer: null,
  // Dernier passage demandé par l'appli : { session, at, before } (`before` : le dernier passage
  // connu de la session à ce moment-là). GitHub met quelques secondes à lister le nouveau.
  launch: null,
  sending: false,
  // Images jointes au prochain message, et images déjà lues (chemin : adresse data:).
  attachments: [],
  images: new Map(),
  // Requêtes gratuites du jour et crédits (clé OpenRouter gardée sur l'appareil).
  quota: null,
  quotaError: "",
  quotaLoading: false,
  // Requêtes à NVIDIA des passages suivis (numéro du passage : heures, en secondes), d'après
  // leur journal : le compteur du haut en tire les requêtes de la dernière minute.
  liveRequests: new Map(),
  // Sessions en cours suivies hors de leur page, pour ce compteur (numéro : { settings, logs }).
  watched: new Map(),
  meterTimer: null,
  requestTimer: null,
  // Liste des sessions du tiroir, et sessions cochées pour être effacées.
  drawerSessions: [],
  selecting: false,
  selected: new Set(),
  // Bloc ouvert dans le menu (« sessions » ou « files ») et dossier parcouru dans « Fichiers ».
  drawerPane: "sessions",
  explorer: null,
  // Page montrée de chaque groupe de réponses relancées ({index, count}).
  attemptPages: new Map(),
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

/** Photo trouvée par l'agent, en grand, avec son crédit et sa page d'origine. */
function openPhoto({ photo, credit, source }) {
  const url = photoView(photo);
  if (!url) return;
  const origin = photoLink(source) || (String(source || "").startsWith("https://") ? source : "");
  const sheet = $("#sheet");
  sheet.innerHTML = `
    <div class="sheet-panel image-panel" role="dialog" aria-modal="true" aria-label="Photo">
      <div class="sheet-head"><strong>Photo trouvée par Claw</strong><button class="icon-button" data-close aria-label="Fermer">${ICON.close}</button></div>
      <img class="full-image" alt="" src="${escapeHtml(url)}" referrerpolicy="no-referrer">
      ${credit ? `<p class="help photo-credit">${escapeHtml(credit)}</p>` : ""}
      ${origin ? `<p class="help"><a href="${escapeHtml(origin)}" target="_blank" rel="noopener noreferrer">Voir la page de la photo</a></p>` : ""}
    </div>`;
  sheet.onclick = (event) => {
    if (event.target === sheet || event.target.closest("[data-close]")) {
      sheet.hidden = true;
      sheet.innerHTML = "";
    }
  };
  sheet.hidden = false;
}

/** Script de démarrage d'un serveur Linux (cloud-init) : il installe l'exécuteur des sessions
 * (claw-cloud.sh) avec un jeton d'inscription valable une heure. */
function cloudInitScript(engine, token, expires) {
  const until = expires ? new Date(expires).toLocaleTimeString("fr-FR", { hour: "2-digit", minute: "2-digit" }) : "dans une heure";
  return [
    "#!/bin/bash",
    `# Claw Web : exécuteur des sessions de ${engine} (jeton d'inscription valable jusqu'à ${until}).`,
    `curl -fsSL https://raw.githubusercontent.com/${CLAW_SOURCE}/main/.github/claw-web/pc/claw-cloud.sh -o /usr/local/bin/claw-cloud`,
    "chmod 755 /usr/local/bin/claw-cloud",
    `claw-cloud install --repo ${engine} --token ${token}`,
    "",
  ].join("\n");
}

/** Commande pour un serveur déjà créé (connexion SSH ou Cloud Shell) : l'exécuteur y prend au plus
 * la moitié du processeur et de la mémoire, le reste au service qui y tourne déjà. */
function existingServerCommand(engine, token) {
  return `curl -fsSL https://raw.githubusercontent.com/${CLAW_SOURCE}/main/.github/claw-web/pc/claw-cloud.sh | sudo bash -s -- install --repo ${engine} --token ${token} --shared`;
}

/** L'aperçu du fichier de configuration que montre Oracle à la création d'une clé d'API :
 * les champs qui manquent (vide : complet). */
function ociConfigMissing(text) {
  const values = Object.fromEntries(String(text || "").split(/\r?\n/)
    .map((line) => line.split("#")[0].trim())
    .filter((line) => line.includes("=") && !line.startsWith("["))
    .map((line) => [line.slice(0, line.indexOf("=")).trim(), line.slice(line.indexOf("=") + 1).trim()]));
  return ["user", "fingerprint", "tenancy", "region"].filter((name) => !values[name]);
}

/** État du robot de réessai, lu dans les variables du dépôt du code de Claw. */
async function robotState() {
  const [switch_, raw] = await Promise.all([
    state.github.variable(CLAW_SOURCE, "ORACLE_ROBOT"),
    state.github.variable(CLAW_SOURCE, "ORACLE_ROBOT_STATE"),
  ]);
  let last = null;
  try {
    last = raw ? JSON.parse(raw) : null;
  } catch {
    last = null;
  }
  const ago = last && last.t ? Math.max(0, Math.round((Date.now() - Date.parse(last.t)) / 60000)) : null;
  const when = ago === null ? "" : ago < 1 ? "à l'instant" : `il y a ${ago} min`;
  if (switch_ === "done") return { on: false, text: "Serveur créé par le robot : l'exécuteur s'installe (15 à 25 minutes), puis apparaît en ligne sous « Ma machine » (Sessions)." };
  if (switch_ === "on") {
    return { on: true, text: last ? `Robot en marche : dernier essai ${when}. ${last.detail || ""}` : "Robot en marche : premier essai dans quelques minutes." };
  }
  return { on: false, text: last && last.outcome === "error" ? `Robot arrêté : ${last.detail || ""}` : "" };
}

/** Serveur gratuit dans le nuage (Oracle Cloud, offre « Always Free ») : les étapes, et le
 * script de démarrage à coller, préparé avec un jeton d'inscription neuf. */
function openCloudRunner() {
  const engine = state.settings.engine;
  const sheet = $("#sheet");
  sheet.innerHTML = `
    <div class="sheet-panel cloud-panel" role="dialog" aria-modal="true" aria-label="Serveur gratuit dans le nuage">
      <div class="sheet-head"><strong>Serveur gratuit dans le nuage</strong><button class="icon-button" data-close aria-label="Fermer">${ICON.close}</button></div>
      <div class="cloud-steps">
        <p class="help">Un serveur d'Oracle Cloud, gratuit sans limite de durée (offre « Always Free » : 2 cœurs ARM, 12 Go de mémoire, 200 Go de disque), fait tourner vos sessions jour et nuit, sans votre PC et sans minutes de GitHub.</p>
        <ol>
          <li>Créez un compte sur <a href="https://signup.cloud.oracle.com/" target="_blank" rel="noopener noreferrer">signup.cloud.oracle.com</a>. Oracle demande une carte pour vérifier votre identité ; rien n'est prélevé tant que vous restez dans l'offre gratuite.</li>
          <li>Préparez le script ci-dessous (il contient un jeton d'inscription valable une heure), puis copiez-le.</li>
          <li>Dans la console d'Oracle : Compute, Instances, <em>Create instance</em>. Image : <strong>Ubuntu 24.04</strong>. Shape : <strong>Ampere VM.Standard.A1.Flex</strong>, 2 OCPU et 12 Go (1 OCPU et 6 Go si Oracle manque de place ; une machine <em>E2.1.Micro</em>, 1 Go de mémoire, ne suffit pas). Dans <em>Advanced options</em>, <em>Management</em>, collez le script dans <em>Initialization script</em> (cloud-init). Créez l'instance.</li>
          <li>Comptez 15 à 25 minutes : le serveur installe tout seul Docker et l'exécuteur, puis apparaît en ligne sous « Ma machine », dans Sessions : choisissez-la.</li>
        </ol>
        <button type="button" class="primary" data-action="cloud-script">Préparer le script</button>
        <div class="cloud-script" hidden>
          <pre class="io-v"></pre>
          <button type="button" class="secondary" data-action="cloud-copy">Copier le script</button>
          <p class="help">Serveur déjà créé (Ubuntu, Debian ou Oracle Linux, 4 Go de mémoire au moins) : lancez plutôt cette commande, en SSH ou dans Cloud Shell. Claw y prend au plus la moitié du processeur et de la mémoire.</p>
          <pre class="io-v cloud-command"></pre>
          <button type="button" class="secondary" data-action="cloud-copy-command">Copier la commande</button>
        </div>
        <h3 class="cloud-subtitle">Robot de réessai</h3>
        <p class="help">Oracle manque de place ? Le robot réessaie toutes les 10 minutes, gratuitement sur GitHub et sans votre PC, puis installe Claw dès qu'une machine se libère. Il s'éteint seul une fois le serveur créé.</p>
        <ol>
          <li>Console d'Oracle, sur le compte et dans la région du futur serveur : profil (en haut à droite), <em>Mon profil</em>, <em>Clés d'API</em>, <em>Ajouter une clé d'API</em>. Choisissez « Générer une paire de clés d'API », <em>Télécharger la clé privée</em>, puis <em>Ajouter</em>.</li>
          <li>Copiez l'<em>Aperçu du fichier de configuration</em> affiché ensuite et collez-le ci-dessous ; choisissez le fichier de clé privée téléchargé (.pem).</li>
          <li>Un réseau avec un sous-réseau public doit exister dans cette région (assistant VCN, « Créer un VCN avec connectivité Internet »).</li>
          <li>Le jeton GitHub de l'appli doit avoir, en lecture et écriture, les autorisations Actions, Secrets, Variables et Administration, sur ${escapeHtml(CLAW_SOURCE)} et ${escapeHtml(engine)} (réglages du jeton à grain fin sur GitHub ; sa valeur ne change pas).</li>
        </ol>
        <label class="field"><span>Aperçu du fichier de configuration</span>
          <textarea name="oci-config" rows="6" spellcheck="false" autocomplete="off" placeholder="[DEFAULT]&#10;user=ocid1.user…&#10;fingerprint=…&#10;tenancy=ocid1.tenancy…&#10;region=eu-marseille-1"></textarea></label>
        <label class="field"><span>Clé privée (.pem)</span><input type="file" name="oci-key" accept=".pem,text/plain,application/x-pem-file"></label>
        <p class="help">La configuration, la clé et le jeton GitHub de l'appli (pour inscrire l'exécuteur) partent chiffrés dans le navigateur, en secrets du dépôt du code de Claw (${escapeHtml(CLAW_SOURCE)}) ; rien n'est gardé sur cet appareil.</p>
        <p class="help robot-state" hidden></p>
        <button type="button" class="primary" data-action="robot-start">Démarrer le robot</button>
        <button type="button" class="secondary" data-action="robot-stop" hidden>Arrêter le robot</button>
        <p class="help">Si Oracle répond « Out of capacity » (manque de capacité) : demandez 1 OCPU et 6 Go, réessayez à d'autres heures (nuit, matin), ou passez le compte en « Pay As You Go » (Facturation, <em>Upgrade</em>) : ces comptes obtiennent bien plus facilement ces machines, et rien n'est facturé tant que vous ne créez que des ressources « Toujours gratuit » (créez une alerte de budget à 1 €, Facturation, <em>Budgets</em>). Certaines régions, comme Marseille, n'ont qu'un domaine de disponibilité. Oracle peut arrêter un serveur gratuit resté inactif sept jours : relancez-le depuis la console (Instances, <em>Start</em>) ; passer le compte en « Pay As You Go » l'évite, sans frais tant que vous restez dans l'offre gratuite. Jeton expiré avant l'installation : préparez un nouveau script et, dans la console d'Oracle (Instances, <em>Run command</em> ou Cloud Shell), lancez <code>sudo claw-cloud register --repo ${escapeHtml(engine)} --token NOUVEAU_JETON</code>.</p>
      </div>
    </div>`;
  sheet.onclick = (event) => {
    if (event.target === sheet || event.target.closest("[data-close]")) {
      sheet.hidden = true;
      sheet.innerHTML = "";
    }
  };
  let script = "";
  let command = "";
  sheet.querySelector('[data-action="cloud-script"]').addEventListener("click", async (event) => {
    const button = event.currentTarget;
    button.disabled = true;
    try {
      const answer = await state.github.registrationToken(engine);
      script = cloudInitScript(engine, answer.token, answer.expires_at);
      command = existingServerCommand(engine, answer.token);
      const box = sheet.querySelector(".cloud-script");
      box.querySelector("pre").textContent = script;
      box.querySelector(".cloud-command").textContent = command;
      box.hidden = false;
      button.textContent = "Préparer un nouveau script";
    } catch (error) {
      toast(error.status === 403
        ? "GitHub refuse le jeton d'inscription : le jeton de l'appli doit avoir l'autorisation Administration (écriture) du dépôt des sessions"
        : error.message);
    } finally {
      button.disabled = false;
    }
  });
  sheet.querySelector('[data-action="cloud-copy"]').addEventListener("click", async () => {
    try {
      await navigator.clipboard.writeText(script);
      toast("Script copié : collez-le dans Initialization script, chez Oracle");
    } catch {
      toast("Copie refusée par le navigateur : sélectionnez le script à la main");
    }
  });
  const showRobot = async () => {
    try {
      const robot = await robotState();
      const line = sheet.querySelector(".robot-state");
      if (!line) return;
      line.textContent = robot.text;
      line.hidden = !robot.text;
      sheet.querySelector('[data-action="robot-start"]').hidden = robot.on;
      sheet.querySelector('[data-action="robot-stop"]').hidden = !robot.on;
    } catch {
      // État illisible (jeton sans accès aux variables) : les boutons restent.
    }
  };
  showRobot();
  sheet.querySelector('[data-action="robot-start"]').addEventListener("click", async (event) => {
    const button = event.currentTarget;
    const config = sheet.querySelector('textarea[name="oci-config"]').value;
    const file = sheet.querySelector('input[name="oci-key"]').files[0];
    const missing = ociConfigMissing(config);
    if (missing.length) {
      toast(`Aperçu de configuration incomplet : il manque ${missing.join(", ")}`);
      return;
    }
    const key = file ? await file.text() : "";
    if (!key.includes("PRIVATE KEY")) {
      toast("Choisissez le fichier de clé privée téléchargé chez Oracle (.pem)");
      return;
    }
    button.disabled = true;
    try {
      // Droits du jeton vérifiés avant de poser les secrets : ce que fera le robot avec lui
      // (variables du dépôt du code de Claw, inscription d'un exécuteur dans celui des sessions).
      await state.github.setVariable(CLAW_SOURCE, "ORACLE_ROBOT_ENGINE", engine);
      await state.github.registrationToken(engine);
      const sealing = await state.github.secretKey(CLAW_SOURCE);
      for (const [name, value] of [["OCI_CONFIG", config.trim()], ["OCI_KEY", key.trim()], ["CLAW_ADMIN_TOKEN", state.settings.token]]) {
        await state.github.putSecret(CLAW_SOURCE, name, sealedBox(value, sealing.key), sealing.key_id);
      }
      await state.github.setVariable(CLAW_SOURCE, "ORACLE_ROBOT", "on");
      await state.github.setWorkflowEnabled(CLAW_SOURCE, "oracle-a1.yml", true);
      await state.github.runWorkflow(CLAW_SOURCE, "oracle-a1.yml");
      sheet.querySelector('textarea[name="oci-config"]').value = "";
      sheet.querySelector('input[name="oci-key"]').value = "";
      toast("Robot démarré : premier essai dans quelques minutes, puis toutes les 10 minutes");
      await showRobot();
    } catch (error) {
      toast(error.message, "error");
    } finally {
      button.disabled = false;
    }
  });
  sheet.querySelector('[data-action="robot-stop"]').addEventListener("click", async () => {
    try {
      await state.github.setVariable(CLAW_SOURCE, "ORACLE_ROBOT", "off");
      await state.github.setWorkflowEnabled(CLAW_SOURCE, "oracle-a1.yml", false);
      toast("Robot arrêté");
      await showRobot();
    } catch (error) {
      toast(error.message);
    }
  });
  sheet.querySelector('[data-action="cloud-copy-command"]').addEventListener("click", async () => {
    try {
      await navigator.clipboard.writeText(command);
      toast("Commande copiée : lancez-la sur le serveur (SSH ou Cloud Shell)");
    } catch {
      toast("Copie refusée par le navigateur : sélectionnez la commande à la main");
    }
  });
  sheet.hidden = false;
}

/** Capture d'écran de l'agent, entière (rangée sur la branche des fichiers du moteur) ; sa
 * vignette s'affiche le temps qu'elle arrive, ou seule quand elle n'a pas pu être rangée. */
async function openShot({ shot, thumb, label }) {
  const sheet = $("#sheet");
  const preview = /^data:image\/(?:jpeg|png|webp);base64,/.test(thumb || "") ? thumb : "";
  sheet.innerHTML = `
    <div class="sheet-panel image-panel shot-panel" role="dialog" aria-modal="true" aria-label="Capture d'écran">
      <div class="sheet-head"><strong>${escapeHtml(label || "Capture d'écran")}</strong><button class="icon-button" data-close aria-label="Fermer">${ICON.close}</button></div>
      <div class="shot-scroll"><img class="full-image tall" alt="" src="${escapeHtml(preview)}"${preview ? "" : " hidden"}></div>
      <p class="help shot-state">${shot ? "Chargement de la capture entière…" : "Seul l'aperçu du premier écran a été gardé."}</p>
    </div>`;
  sheet.onclick = (event) => {
    if (event.target === sheet || event.target.closest("[data-close]")) {
      sheet.hidden = true;
      sheet.innerHTML = "";
    }
  };
  sheet.hidden = false;
  if (!shot) return;
  try {
    let url = state.images.get(shot);
    if (!url) {
      const blob = await state.github.rawFile(state.settings.engine, shot, FILES_BRANCH);
      // En adresse data: (la Content-Security-Policy refuse les images blob:).
      url = await new Promise((resolve, reject) => {
        const reader = new FileReader();
        reader.onload = () => resolve(reader.result);
        reader.onerror = reject;
        reader.readAsDataURL(blob);
      });
      state.images.set(shot, url);
    }
    const image = sheet.querySelector(".full-image");
    if (!image) return;
    image.src = url;
    image.hidden = false;
    sheet.querySelector(".shot-state")?.remove();
  } catch {
    const note = sheet.querySelector(".shot-state");
    if (note) note.textContent = "La capture entière n'a pas pu être chargée : voici l'aperçu du premier écran.";
  }
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
  const keyed = KEYED_PROVIDERS.flatMap((provider) => provider.models);
  const known = [...(state.models || []), ...keyed].find((item) => item.id === model);
  return known ? known.short : model.replace(/:free$/, "").split("/").pop();
}

/** Fournisseurs gratuits à clé, en plus d'OpenRouter, dans l'ordre des onglets du choix du modèle
 * (OpenRouter, Google Gemini, NVIDIA) : leur clé devient un secret du moteur, et leurs modèles
 * sont proposés quand le moteur a ce secret. Mêmes listes que rust/crates/claw-cli/src/models.rs :
 * `GEMINI_FREE_TIER` et `NVIDIA_MODELS` (ceux qui appellent les outils), du plus puissant au plus
 * léger. */
const KEYED_PROVIDERS = [
  {
    key: "gemini",
    secret: "GEMINI_API_KEY",
    label: "Google Gemini",
    prefix: "AIza",
    where: "aistudio.google.com/apikey, projet sans facturation",
    field: "Clé Google Gemini (facultative : offre gratuite de Google)",
    detail: "offre gratuite de Google : quota par jour et par modèle, bas pour les plus récents (20 requêtes pour Gemini 3.8 Flash)",
    // Requêtes par jour relevées (erreur 429 de Google, 18 octobre 2026) ; remise à zéro à minuit,
    // heure du Pacifique. Google ne donne pas le compte en direct.
    perDay: { "gemini:gemini-3.8-flash": 20 },
    models: [
      { id: "gemini:gemini-3.8-flash", short: "Gemini 3.8 Flash" },
      { id: "gemini:gemini-3.6-flash", short: "Gemini 3.6 Flash" },
      { id: "gemini:gemini-3.5-flash-lite", short: "Gemini 3.5 Flash-Lite" },
      { id: "gemini:gemini-3.1-flash-lite", short: "Gemini 3.1 Flash-Lite" },
      { id: "gemini:gemini-2.5-pro", short: "Gemini 2.5 Pro" },
      { id: "gemini:gemini-2.5-flash", short: "Gemini 2.5 Flash" },
      { id: "gemini:gemini-2.5-flash-lite", short: "Gemini 2.5 Flash-Lite" },
    ],
  },
  {
    key: "nvidia",
    secret: "NVIDIA_API_KEY",
    label: "NVIDIA",
    prefix: "nvapi-",
    where: "build.nvidia.com, profil, Settings, API Keys",
    field: "Clé NVIDIA (facultative : modèles de build.nvidia.com, sans quota par jour)",
    detail: "gratuit chez NVIDIA, environ 40 requêtes par minute, sans quota par jour",
    // Observé par les utilisateurs, non publié par NVIDIA ; aucun quota par jour annoncé.
    perMinute: 40,
    models: [
      // Nemotron 3 Super d'abord : rapide et fiable. Ultra, le plus puissant, est lent sur
      // l'offre gratuite (vu le 19 octobre 2026 : longues réponses coupées par NVIDIA).
      { id: "nvidia:nvidia/nemotron-3-super-120b-a12b", short: "NVIDIA Nemotron 3 Super" },
      { id: "nvidia:nvidia/nemotron-3-ultra-550b-a55b", short: "NVIDIA Nemotron 3 Ultra", note: "le plus puissant ; parfois lent, NVIDIA coupe alors ses longues réponses (Claw passe à un autre modèle de NVIDIA)" },
      // Revu le 20 octobre 2026 : Lightning répond en moins d'une seconde et fait le travail.
      // Step 5 Preview (faux messages de l'utilisateur, tâches inventées) et gpt-oss 20B (en
      // échec) ne sont plus proposés.
      { id: "nvidia:nvidia/nemotron-3.5-lightning-30b-a3b", short: "NVIDIA Nemotron 3.5 Lightning", note: "très rapide ; moins soigné que Super et Ultra" },
    ],
  },
];

/** Fournisseur d'un modèle : « openrouter » (« Auto » compris), « gemini » ou « nvidia ». */
function providerOf(model) {
  const provider = KEYED_PROVIDERS.find((entry) => (model || "").startsWith(`${entry.key}:`));
  return provider ? provider.key : "openrouter";
}

/** Prochain minuit d'un fuseau, en heure de l'appareil : Google remet ses quotas à zéro à minuit,
 * heure du Pacifique (9 h à Paris l'été, 8 h une semaine par an, 9 h l'hiver). */
function nextMidnightIn(timeZone) {
  const now = new Date();
  const parts = Object.fromEntries(new Intl.DateTimeFormat("en-US", { timeZone, hourCycle: "h23", hour: "2-digit", minute: "2-digit" })
    .formatToParts(now).map((part) => [part.type, part.value]));
  const next = new Date(now.getTime() + (24 * 60 - (Number(parts.hour) * 60 + Number(parts.minute))) * 60_000);
  next.setSeconds(0, 0);
  return next.toLocaleTimeString("fr-FR", { hour: "2-digit", minute: "2-digit" });
}

/** Ce qu'on sait du quota d'un fournisseur pour un modèle : `short` (anneau du haut, HTML),
 * `label` (lecteurs d'écran) et `line` (sous l'onglet du choix du modèle). OpenRouter seul donne
 * un compte en direct ; Gemini, sa limite par jour quand elle est relevée ; NVIDIA, par minute. */
function providerQuota(key, model) {
  const provider = KEYED_PROVIDERS.find((entry) => entry.key === key);
  if (key === "gemini") {
    const limit = provider.perDay[model];
    return {
      short: limit ? `<strong>${limit}</strong><small>/jour</small>` : "<span>Quota/jour</span>",
      label: limit ? `Google Gemini : ${limit} requêtes par jour pour ce modèle` : "Google Gemini : quota par jour et par modèle",
      line: `Offre gratuite de Google : quota par jour et par modèle (20 pour Gemini 3.8 Flash), remis à zéro à ${nextMidnightIn("America/Los_Angeles")}.`,
    };
  }
  if (key === "nvidia") {
    return {
      short: `<strong>${provider.perMinute}</strong><small>/min</small>`,
      label: `NVIDIA : environ ${provider.perMinute} requêtes par minute, sans quota par jour`,
      line: `NVIDIA : environ ${provider.perMinute} requêtes par minute, aucun quota par jour.`,
    };
  }
  const free = state.quota && state.quota.free;
  return {
    line: free
      ? `${free.remaining} requêtes gratuites restantes aujourd'hui (sur ${free.limit}), remises à zéro à ${resetTime()}.`
      : "Requêtes gratuites du jour : gardez une clé OpenRouter sur cet appareil pour les voir (Réglages).",
  };
}

/** Secrets du moteur pour les fournisseurs à clé : nom du secret, vrai s'il est posé. Demandé
 * une fois à GitHub (le nom du secret, jamais sa valeur) ; un fournisseur sans réponse est
 * compté absent, sans être gardé. */
async function engineProviderSecrets() {
  if (state.providerSecrets) return state.providerSecrets;
  if (!state.github) return {};
  const found = {};
  let complete = true;
  for (const provider of KEYED_PROVIDERS) {
    try {
      found[provider.secret] = await state.github.hasSecret(state.settings.engine, provider.secret);
    } catch {
      found[provider.secret] = false;
      complete = false;
    }
  }
  if (complete) state.providerSecrets = found;
  return found;
}

/** État d'un passage du moteur, pour l'affichage. */
function runStatus(run) {
  if (!run) return null;
  if (run.status === "queued" || run.status === "waiting" || run.status === "pending" || run.status === "requested") {
    // Sur le PC, une session attend tant que le PC et Docker Desktop ne sont pas lancés.
    return { key: "queued", label: state.settings.runner ? "En attente d'une machine (serveur, ou PC allumé avec Docker Desktop)" : "En file d'attente" };
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
  const engine = state.settings.engine;
  try {
    state.runs = await state.github.runs(engine);
    state.runsEngine = engine;
  } catch (error) {
    if (!(error instanceof GitHubError)) throw error;
    // Liste illisible un instant (réseau, GitHub) : celle d'avant reste. Vide, une session
    // croirait n'avoir aucun passage et dirait son dernier message pas envoyé.
    if (state.runsEngine !== engine) state.runs = [];
  }
}

// --- Feuille de choix (dépôt, branche, modèle) ---------------------------------------------

/** `onRemove` : les éléments `removable` ont un bouton de suppression (une corbeille), à côté.
 * `groups` ([{ key, label, empty }]) : un onglet par groupe, pour les éléments qui ont ce
 * `group` ; l'onglet ouvert est celui de l'élément choisi. Une recherche parcourt tous les
 * groupes ; `empty` dit quoi faire quand un groupe n'a rien (une clé à ajouter). */
function openPicker({ title, items, current, placeholder, onPick, onRemove, groups = [], tab = "" }) {
  const sheet = $("#sheet");
  // Les actions d'un onglet (« Rechercher tous les modèles gratuits ») ne comptent pas.
  const count = (key) => items.filter((item) => item.group === key && !item.action).length;
  let active = (groups.some((group) => group.key === tab) && tab)
    || (items.find((item) => item.value === current && item.group) || {}).group || (groups[0] || {}).key;
  sheet.innerHTML = `
    <div class="sheet-panel" role="dialog" aria-modal="true" aria-label="${escapeHtml(title)}">
      <div class="sheet-head">
        <strong>${escapeHtml(title)}</strong>
        <button class="icon-button" data-close aria-label="Fermer">${ICON.close}</button>
      </div>
      ${groups.length ? `<div class="sheet-tabs" role="tablist">${groups.map((group) => `
        <button type="button" class="sheet-tab" role="tab" data-group="${escapeHtml(group.key)}">${escapeHtml(group.label)}${count(group.key) ? `<small>${count(group.key)}</small>` : ""}</button>`).join("")}
      </div>` : ""}
      ${groups.some((group) => group.note) ? '<p class="help sheet-note"></p>' : ""}
      ${items.length > 8 ? `<input class="sheet-search" type="search" placeholder="${escapeHtml(placeholder || "Rechercher")}" autocomplete="off">` : ""}
      <ul class="sheet-list"></ul>
    </div>`;
  const list = sheet.querySelector(".sheet-list");
  // Les éléments épinglés (« Créer un dépôt », « Auto ») restent en tête, quelle que soit la
  // recherche ou l'onglet.
  const draw = (filter = "") => {
    for (const tab of sheet.querySelectorAll(".sheet-tab")) {
      const on = tab.dataset.group === active && !filter.trim();
      tab.classList.toggle("on", on);
      tab.setAttribute("aria-selected", String(on));
    }
    const words = filter.toLowerCase().split(/\s+/).filter(Boolean);
    // Quota du fournisseur de l'onglet ouvert (rien pendant une recherche, qui les mêle).
    const note = sheet.querySelector(".sheet-note");
    if (note) {
      const text = words.length ? "" : ((groups.find((entry) => entry.key === active) || {}).note || "");
      note.textContent = text;
      note.hidden = !text;
    }
    const inTab = (item) => !groups.length || words.length || item.group === active;
    const shown = items.filter((item) => item.pinned
      || (inTab(item) && words.every((word) => `${item.label} ${item.detail || ""}`.toLowerCase().includes(word))));
    const group = groups.find((entry) => entry.key === active);
    if (!words.length && group && group.empty && count(active) === 0) {
      list.innerHTML = shown.map((item) => itemRow(item)).join("") + `<li class="empty">${escapeHtml(group.empty)}</li>`;
      return;
    }
    list.innerHTML = shown.slice(0, 200).map((item) => itemRow(item)).join("") || '<li class="empty">Rien ne correspond.</li>';
  };
  const itemRow = (item) => {
      const removable = onRemove && item.removable;
      return `
      <li${removable ? ' class="branch-row"' : ""}><button class="sheet-item${item.value === current ? " current" : ""}${item.pinned ? " pinned" : ""}" data-value="${escapeHtml(item.value)}">
        <span class="sheet-label">${item.icon ? ICON[item.icon] : ""}<span>${escapeHtml(item.label)}</span></span>
        ${item.detail ? `<small>${escapeHtml(item.detail)}</small>` : ""}
      </button>${removable ? `<button type="button" class="icon-button" data-remove="${escapeHtml(item.value)}" aria-label="Supprimer ${escapeHtml(item.label)}">${ICON.trash}</button>` : ""}</li>`;
  };
  draw();
  const close = () => {
    sheet.hidden = true;
    sheet.innerHTML = "";
  };
  sheet.onclick = (event) => {
    if (event.target === sheet || event.target.closest("[data-close]")) close();
    const tab = event.target.closest("[data-group]");
    if (tab) {
      active = tab.dataset.group;
      if (search) search.value = "";
      draw();
      return;
    }
    const remove = event.target.closest("[data-remove]");
    if (remove) {
      close();
      onRemove(remove.dataset.remove);
      return;
    }
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
          removable: !protectedRepo(repo.full_name),
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
      onRemove: (value) => openDeleteRepo(repos.find((item) => item.full_name === value)),
    });
  } catch (error) {
    failure(error);
  }
}

/** Dépôts que l'appli ne propose jamais de supprimer : Claw Web en dépend. Rend la raison. */
function protectedRepo(fullName) {
  const name = (fullName || "").toLowerCase();
  if (name === (state.settings.engine || DEFAULT_ENGINE).toLowerCase()) return "dépôt du moteur : vos sessions y vivent";
  if (name === CLAW_SOURCE || MOVED_ENGINES[name]) return "code de Claw : le moteur en dépend";
  return "";
}

/** Sessions de l'appli qui travaillent dans `repo` (et sur `branch`, si elle est donnée). */
async function sessionsUsing(repo, branch = "") {
  const sessions = await state.github.sessions(state.settings.engine);
  return sessions.filter((issue) => settingsOf(issue).repo === repo && (!branch || branchOf(issue) === branch));
}

/** Suppression d'un dépôt : le nom complet à retaper, comme sur GitHub. */
async function openDeleteRepo(repo) {
  if (!repo) return;
  const fullName = repo.full_name;
  const reason = protectedRepo(fullName);
  if (reason) {
    toast(`${fullName} ne se supprime pas depuis l'appli (${reason})`, "error");
    return;
  }
  let using = [];
  try {
    using = await sessionsUsing(fullName);
  } catch {
    using = [];
  }
  const sheet = $("#sheet");
  sheet.innerHTML = `
    <div class="sheet-panel" role="dialog" aria-modal="true" aria-label="Supprimer le dépôt">
      <div class="sheet-head">
        <strong>Supprimer le dépôt</strong>
        <button class="icon-button" data-close aria-label="Fermer">${ICON.close}</button>
      </div>
      <form class="branch-form delete-repo" autocomplete="off">
        <p class="help"><strong>${escapeHtml(fullName)}</strong> (${repo.private ? "privé" : "public"}) sera supprimé de GitHub avec tout son contenu : code, branches, issues, pull requests et site en ligne. GitHub permet de le restaurer pendant 90 jours (Réglages de GitHub, Repositories, Deleted repositories).</p>
        ${using.length ? `<p class="help">${using.length > 1 ? `${using.length} sessions de l'appli travaillent` : "Une session de l'appli travaille"} dans ce dépôt : ${using.length > 1 ? "elles restent" : "elle reste"} dans la liste, sans dépôt où reprendre.</p>` : ""}
        <label>Pour confirmer, tapez <code>${escapeHtml(fullName)}</code>
          <input name="confirm" type="text" required spellcheck="false" autocapitalize="none" autocorrect="off" placeholder="${escapeHtml(fullName)}">
        </label>
        <div class="form-actions">
          <button type="button" class="small-link" data-close>${ICON.back}<span>Annuler</span></button>
          <button class="danger" type="submit" disabled>Supprimer le dépôt</button>
        </div>
      </form>
    </div>`;
  const form = sheet.querySelector(".delete-repo");
  const submit = form.querySelector("button[type=submit]");
  const close = () => {
    sheet.hidden = true;
    sheet.innerHTML = "";
  };
  form.elements.confirm.addEventListener("input", () => {
    submit.disabled = form.elements.confirm.value.trim() !== fullName;
  });
  sheet.onclick = (event) => {
    if (event.target === sheet || event.target.closest("[data-close]")) close();
  };
  form.addEventListener("submit", async (event) => {
    event.preventDefault();
    if (form.elements.confirm.value.trim() !== fullName) return;
    submit.disabled = true;
    submit.textContent = "Suppression…";
    try {
      await state.github.deleteRepository(fullName);
      if (state.repos) state.repos = state.repos.filter((item) => item.full_name !== fullName);
      state.branches.delete(fullName);
      if (state.settings.repo === fullName) {
        delete state.settings.repo;
        delete state.settings.branch;
        saveSettings();
        renderComposer();
      }
      close();
      toast(`Dépôt ${fullName} supprimé (restaurable 90 jours sur GitHub)`);
    } catch (error) {
      submit.disabled = false;
      submit.textContent = "Supprimer le dépôt";
      if (error.status === 403) {
        toast("GitHub refuse : le jeton n'a pas l'autorisation Administration (lecture et écriture) sur ce dépôt. Ajoutez-la au jeton à grain fin, sans changer sa valeur.", "error");
        return;
      }
      failure(error);
    }
  });
  sheet.hidden = false;
  form.elements.confirm.focus();
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
          ${name === fallback ? "" : `<button type="button" class="icon-button" data-delete="${escapeHtml(name)}" aria-label="Supprimer ${escapeHtml(name)}">${ICON.trash}</button>`}
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
  // Suppression d'une branche (jamais la branche par défaut : GitHub la refuse aussi).
  const showDelete = async (name) => {
    let using = [];
    try {
      using = await sessionsUsing(repo, name);
    } catch {
      using = [];
    }
    body.innerHTML = `
      <form class="branch-form" autocomplete="off">
        <p class="help">Supprimer la branche <code>${escapeHtml(name)}</code> de ${escapeHtml(repo)} ? Ses commits qui ne sont dans aucune autre branche ne seront plus visibles ; une pull request fermée garde un bouton « Restore branch » sur GitHub.</p>
        ${using.length ? `<p class="help">${using.length > 1 ? `${using.length} sessions de l'appli travaillent` : "Une session de l'appli travaille"} sur cette branche : ${using.map((issue) => `n° ${escapeHtml(String(settingsOf(issue).n || issue.number))}`).join(", ")}. Son prochain passage la recréera à partir de la branche de départ, sans le travail perdu.</p>` : ""}
        <div class="form-actions">
          <button type="button" class="small-link" data-back>${ICON.back}<span>Retour</span></button>
          <button class="danger" type="submit">Supprimer la branche</button>
        </div>
      </form>`;
    const form = body.querySelector("form");
    form.addEventListener("submit", async (event) => {
      event.preventDefault();
      const button = form.querySelector("button[type=submit]");
      button.disabled = true;
      button.textContent = "Suppression…";
      try {
        await state.github.deleteBranch(repo, name);
        branches.splice(branches.indexOf(name), 1);
        state.branches.set(repo, [...branches]);
        if (state.settings.repo === repo && state.settings.branch === name) {
          state.settings.branch = fallback || "main";
          saveSettings();
          renderComposer();
        }
        if (current === name) current = fallback;
        toast(`Branche ${name} supprimée`);
        showList();
      } catch (error) {
        button.disabled = false;
        button.textContent = "Supprimer la branche";
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
    const remove = event.target.closest("[data-delete]");
    if (remove) {
      showDelete(remove.dataset.delete);
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

// --- Catalogue complet des modèles gratuits ------------------------------------------------

/** Session réservée : le moteur y dresse le catalogue d'un fournisseur (models_catalog.py). */
const CATALOGUE_SESSION = "catalogue-modeles";
const CATALOGUE_ACTION = "__catalogue__:";
const UNUSABLE = "__inutilisable__:";
const catalogueKey = (provider) => `claw-web-catalogue-${provider}`;

function storedCatalogue(provider) {
  try {
    return JSON.parse(localStorage.getItem(catalogueKey(provider)) || "null");
  } catch {
    return null;
  }
}

function keepCatalogue(provider, data) {
  try {
    localStorage.setItem(catalogueKey(provider), JSON.stringify(data));
  } catch {
    // Navigation privée : le catalogue sert pour cette ouverture seulement.
  }
}

/** Tous les modèles gratuits d'un fournisseur, classés pour le code. OpenRouter : son catalogue
 * public, lu ici. NVIDIA et Gemini : le moteur, qui a leurs clés, essaie chaque modèle (répond-il,
 * appelle-t-il un outil, en combien de temps) et range le résultat sur la branche des fichiers. */
async function searchCatalogue(provider) {
  if (provider === "openrouter") {
    const data = { provider, at: Math.floor(Date.now() / 1000), error: "", models: await allFreeModels() };
    keepCatalogue(provider, data);
    return data;
  }
  const engine = state.settings.engine;
  const ref = state.engineRef || "main";
  const started = Date.now();
  await state.github.dispatch(engine, ref, { repo: engine, base: ref, session: CATALOGUE_SESSION, model: provider });
  const title = runTitle(engine, CATALOGUE_SESSION);
  let run = null;
  while (Date.now() - started < 25 * 60 * 1000) {
    await new Promise((resolve) => setTimeout(resolve, 10000));
    run = (await state.github.runs(engine))
      .find((item) => item.display_title === title && Date.parse(item.created_at) >= started - 60000) || null;
    if (run && run.status === "completed") break;
  }
  if (!run || run.status !== "completed") throw new Error("La recherche des modèles prend trop de temps : réessayez plus tard.");
  let data = null;
  try {
    const file = await state.github.contents(engine, `catalogue/${provider}.json`, FILES_BRANCH);
    const bytes = Uint8Array.from(atob(String(file.content || "").replace(/\s/g, "")), (character) => character.charCodeAt(0));
    data = JSON.parse(new TextDecoder().decode(bytes));
  } catch {
    data = null;
  }
  if (!data || data.at * 1000 < started - 120000) {
    throw new Error(`La recherche a échoué dans le moteur (${run.conclusion}) : le workflow claw-web.yml du moteur est-il à jour ?`);
  }
  keepCatalogue(provider, data);
  return data;
}

/** Lance la recherche, dit où elle en est, puis rouvre le choix du modèle sur ses résultats. */
async function runCatalogueSearch(provider, label) {
  if (state.catalogueSearch) {
    toast(`Recherche déjà en cours (${state.catalogueSearch}) : un instant.`);
    return;
  }
  state.catalogueSearch = label;
  toast(provider === "openrouter"
    ? "Recherche de tous les modèles gratuits d'OpenRouter…"
    : `Recherche de tous les modèles gratuits de ${label} : le moteur essaie chacun, 2 à 10 minutes. Vous pouvez continuer.`);
  try {
    const data = await searchCatalogue(provider);
    const usable = data.models.filter((model) => model.tools !== false && model.status === "ok").length;
    toast(`${label} : ${data.models.length} modèles gratuits trouvés, ${usable} utilisables par Claw.${data.error ? ` Erreur : ${data.error}` : ""}`,
      data.error ? "error" : "info");
    await pickModel(provider);
  } catch (error) {
    failure(error);
  } finally {
    state.catalogueSearch = null;
  }
}

/** Modèles d'un catalogue, comme éléments du choix : utilisables numérotés d'abord, puis ceux
 * avec lesquels Claw ne peut pas travailler (sans outils, en échec, hors de l'offre gratuite). */
function catalogueItems(provider, data) {
  let rank = 0;
  return (data.models || []).map((model) => {
    const usable = model.tools !== false && model.status === "ok";
    if (usable) rank += 1;
    return {
      value: usable ? model.id : `${UNUSABLE}${model.id}`,
      label: usable ? `${rank}. ${model.short}` : `${model.short} · inutilisable`,
      detail: [
        usable ? (rank === 1 ? "le plus puissant pour le code" : "") : model.status,
        model.tools === null || model.tools === undefined ? (usable ? "non essayé" : "") : "",
        ...(model.reasons || []),
        model.context ? `contexte ${contextLabel(model.context)}` : "",
      ].filter(Boolean).join(" · "),
      group: provider,
    };
  });
}

/** Ligne « Rechercher tous les modèles gratuits » en tête d'un onglet. */
function catalogueAction(provider, data, slow) {
  const when = data ? new Date(data.at * 1000).toLocaleString("fr-FR", { day: "numeric", month: "short", hour: "2-digit", minute: "2-digit" }) : "";
  const usable = data ? data.models.filter((model) => model.tools !== false && model.status === "ok").length : 0;
  return {
    value: `${CATALOGUE_ACTION}${provider}`,
    label: data ? "Relancer la recherche de tous les modèles gratuits" : "Rechercher tous les modèles gratuits",
    detail: data
      ? `dernière recherche le ${when} : ${data.models.length} modèles, ${usable} utilisables, classés pour le code`
      : `tout le catalogue gratuit, classé pour le code${slow ? " ; le moteur essaie chaque modèle (2 à 10 min)" : ""}`,
    group: provider,
    icon: "spark",
    action: true,
  };
}

async function pickModel(tab) {
  const [models, secrets] = await Promise.all([loadModels(), engineProviderSecrets()]);
  const best = models.find((model) => model.rank === 1);
  const openrouterCatalogue = storedCatalogue("openrouter");
  // Un onglet par fournisseur : OpenRouter, puis Google Gemini et NVIDIA, dont les modèles ne
  // sont proposés que si le moteur a leur clé (sinon l'onglet dit où la mettre). Après une
  // recherche, l'onglet montre tout le catalogue gratuit du fournisseur, classé pour le code.
  const keyedItems = KEYED_PROVIDERS.filter((provider) => secrets[provider.secret])
    .flatMap((provider) => {
      const data = storedCatalogue(provider.key);
      const listed = data ? catalogueItems(provider.key, data) : provider.models.map((model) => ({
        value: model.id,
        label: model.short,
        detail: `${model.note ? `${model.note} · ` : ""}${provider.detail} (secret ${provider.secret} du moteur)`,
        group: provider.key,
      }));
      return [catalogueAction(provider.key, data, true), ...listed];
    });
  const groups = [
    { key: "openrouter", label: "OpenRouter", note: providerQuota("openrouter").line, empty: "Aucun modèle gratuit d'OpenRouter trouvé : réessayez dans un instant." },
    ...KEYED_PROVIDERS.map((provider) => ({
      key: provider.key,
      label: provider.label,
      note: providerQuota(provider.key).line,
      empty: `Pas de clé ${provider.label} dans le moteur : Réglages, Moteur, « ${provider.field} » (${provider.where}).`,
    })),
  ];
  const detail = (model) => [
    model.rank === 1 ? "le plus puissant" : model.rank ? `n°${model.rank} pour le code` : "non retenu pour le code",
    ...model.reasons,
    model.context ? `contexte ${contextLabel(model.context)}` : "",
  ].filter(Boolean).join(" · ");
  openPicker({
    tab,
    title: "Modèle (gratuit)",
    placeholder: "Rechercher un modèle",
    current: state.settings.model || "",
    groups,
    items: [
      { value: "", label: "Auto", pinned: true, detail: best ? `en ce moment ${best.short}, le plus puissant des gratuits pour le code (et les suivants s'il ne répond plus)` : "le meilleur modèle gratuit pour le code" },
      catalogueAction("openrouter", openrouterCatalogue, false),
      ...(openrouterCatalogue
        ? catalogueItems("openrouter", openrouterCatalogue)
        : models.map((model) => ({ value: model.id, label: model.rank ? `${model.rank}. ${model.short}` : model.short, detail: detail(model), group: "openrouter" }))),
      ...keyedItems,
    ],
    onPick: (value) => {
      if (value.startsWith(CATALOGUE_ACTION)) {
        const provider = value.slice(CATALOGUE_ACTION.length);
        const group = groups.find((entry) => entry.key === provider);
        runCatalogueSearch(provider, group ? group.label : provider);
        return;
      }
      if (value.startsWith(UNUSABLE)) {
        toast("Claw ne peut pas travailler avec ce modèle : il n'appelle pas d'outils, échoue ou n'est pas gratuit (voir le détail).", "error");
        return;
      }
      state.settings.model = value;
      saveSettings();
      renderComposer();
      showQuotaLabel();
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
const aidInputs = () => ({
  team: state.settings.team ? "1" : "",
  ultracode: state.settings.ultracode ? "1" : "",
  // La machine, seulement pour le PC : un workflow des sessions d'avant ce choix la refuserait.
  ...(state.settings.runner ? { runner: state.settings.runner } : {}),
});

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
  // Modèle de Gemini ou de NVIDIA : leurs limites, pas le compte d'OpenRouter.
  const provider = providerOf(state.settings.model);
  clearTimeout(state.meterTimer);
  if (provider === "nvidia") {
    // Requêtes encore libres dans la minute, d'après le journal des passages suivis ; l'anneau
    // se vide avec elles et se remplit à mesure que la minute glisse.
    const limit = KEYED_PROVIDERS.find((entry) => entry.key === "nvidia").perMinute;
    const used = requestsLastMinute();
    const left = Math.max(0, limit - used);
    const share = left / limit;
    pill.classList.add(share > 0.5 ? "ok" : share > 0.1 ? "warn" : "low", "provider");
    pill.innerHTML = `<svg class="ring" viewBox="0 0 20 20" aria-hidden="true"><circle class="track" cx="10" cy="10" r="7.5"/>`
      + `<circle class="level" cx="10" cy="10" r="7.5" pathLength="100" stroke-dasharray="${Math.round(share * 100)} 100" transform="rotate(-90 10 10)"/></svg>`
      + `<strong>${left}</strong><small>/min</small>`;
    pill.setAttribute("aria-label", `NVIDIA : environ ${left} requêtes libres sur ${limit} dans la minute`);
    if (used) state.meterTimer = setTimeout(showQuotaLabel, 5000);
    return;
  }
  if (provider !== "openrouter") {
    const quota = providerQuota(provider, state.settings.model);
    pill.classList.add("ok", "provider");
    pill.innerHTML = `${ICON.gauge}${quota.short}`;
    pill.setAttribute("aria-label", quota.label);
    return;
  }
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
  // Clés des fournisseurs posées dans le moteur (noms seulement, demandés une fois).
  const secrets = await engineProviderSecrets();
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
    const chosen = providerOf(state.settings.model);
    const nvidiaUsed = requestsLastMinute();
    const missing = (key) => {
      const provider = KEYED_PROVIDERS.find((entry) => entry.key === key);
      return secrets[provider.secret] ? "" : `<p class="help">Pas de clé ${escapeHtml(provider.label)} dans le moteur : Réglages, Moteur.</p>`;
    };
    const parts = {
      openrouter: body,
      gemini: `<p>Offre gratuite de Google : quota par jour et par modèle, compté par projet. Relevé : 20 requêtes par jour pour Gemini 3.8 Flash.</p>
        <p class="help">Google ne donne pas ce compte en direct : votre consommation est sur <a href="https://aistudio.google.com/rate-limit" target="_blank" rel="noopener noreferrer">AI Studio, Rate limit</a>. Remise à zéro à ${escapeHtml(nextMidnightIn("America/Los_Angeles"))} (minuit, heure du Pacifique). Chaque réponse de Claw dit combien de requêtes son passage a prises.</p>
        ${missing("gemini")}`,
      nvidia: `<p>${nvidiaUsed ? `Dernière minute : ${nvidiaUsed} requête${nvidiaUsed > 1 ? "s" : ""} de Claw, environ ${Math.max(0, 40 - nvidiaUsed)} encore libres sur 40.` : "Aucune requête de Claw dans la dernière minute : environ 40 libres."}</p>
        <p class="help">Compté d'après le journal des sessions en cours, publié toutes les 20 à 30 secondes. Gratuit pour les membres du programme pour développeurs de NVIDIA : environ 40 requêtes par minute pour le compte, aucun quota par jour annoncé. Quand la limite est atteinte, Claw attend et réessaie. Chaque réponse de Claw dit combien de requêtes son passage a prises.</p>
        ${missing("nvidia")}`,
    };
    const labels = { openrouter: "OpenRouter", ...Object.fromEntries(KEYED_PROVIDERS.map((provider) => [provider.key, provider.label])) };
    const order = [chosen, ...Object.keys(parts).filter((key) => key !== chosen)];
    body = order.map((key) => `<section class="quota-provider" data-provider="${key}">
        <h3>${escapeHtml(labels[key])}${key === chosen ? "<small>modèle choisi</small>" : ""}</h3>${parts[key]}</section>`).join("");
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
  composer.querySelector('[data-action="model"]').addEventListener("click", () => pickModel());
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
    textarea.value = "";
    // La session s'affiche aussitôt (sa tâche, « Claw se prépare ») ; le passage part ensuite.
    launching(issue.number, repo);
    state.session = { number: issue.number, issue, comments: [], settings: settingsOf(issue), pull: null, step: "", signature: "", logs: new Map() };
    location.hash = `#/session/${issue.number}`;
    try {
      await state.github.dispatch(engine, state.engineRef,
        { repo, base: branch || "main", session: String(issue.number), model: model || "", ...aidInputs() });
    } catch (error) {
      launchFailed(state.session);
      throw error;
    }
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
    const posted = await state.github.comment(state.settings.engine, session.number, `${text}${images}`);
    state.attachments = [];
    textarea.value = "";
    textarea.dispatchEvent(new Event("input"));
    // Le message et « Claw se prépare » paraissent aussitôt, sans attendre GitHub.
    if (posted && state.session === session && !session.comments.some((comment) => comment.id === posted.id)) {
      session.comments.push(posted);
    }
    launching(session.number, session.settings.repo);
    if (state.session === session) {
      renderSessionView();
      window.scrollTo({ top: document.body.scrollHeight });
    }
    await dispatchSession(session);
    await pollSession();
  } catch (error) {
    failure(error);
  } finally {
    state.sending = false;
    renderSending(false);
  }
}

async function dispatchSession(session) {
  launching(session.number, session.settings.repo);
  try {
    await state.github.dispatch(state.settings.engine, state.engineRef, {
      repo: session.settings.repo,
      base: session.settings.base || "main",
      session: String(session.number),
      model: state.settings.model || "",
      ...aidInputs(),
    });
  } catch (error) {
    launchFailed(session);
    throw error;
  }
}

/** Passage demandé pour la session `number` : tant que GitHub ne l'a pas listé (quelques
 * secondes), elle l'attend (« Claw se prépare ») au lieu de dire son message pas envoyé. */
function launching(number, repo) {
  const known = latestRun(repo, number);
  state.launch = { session: number, at: Date.now(), before: known ? known.id : null };
}

/** Lancement refusé ou coupé : la session ne l'attend plus et redit son message pas envoyé
 * (« Lancer le passage »). */
function launchFailed(session) {
  if (state.launch && session && state.launch.session === session.number) state.launch = null;
  if (session && state.session === session && session.issue) renderSessionView();
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
      // Captures d'écran rangées par le moteur, dans le dossier de la session.
      try {
        const listing = await state.github.contents(engine, `uploads/session-${issue.number}`, FILES_BRANCH);
        for (const file of Array.isArray(listing) ? listing : []) {
          await state.github.deleteFile(engine, file.path, "Capture d'une session supprimée", FILES_BRANCH);
        }
      } catch (error) {
        if (!(error instanceof GitHubError) || error.status !== 404) problems.push(`captures de la session ${numberOf(issue)} : ${error.message}`);
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
      // Pas `map(sessionItem)` : l'indice passé en second deviendrait « à cocher ».
      recent.innerHTML = sessions.filter((issue) => issue.state === "open").slice(0, 8).map((issue) => sessionItem(issue)).join("")
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

/** La discussion en blocs : les messages de l'utilisateur et, pour chacun, les passages de Claw
 * (progression et journal, puis réponse). Les passages d'un même message (« Relancer ») forment
 * un groupe montré en pages. Une réponse complète le dernier passage resté sans réponse : un
 * message envoyé pendant un passage peut la précéder. */
function discussionBlocks(comments) {
  const blocks = [];
  let group = null;
  let open = null;
  const attempt = (entry) => {
    if (!group) {
      group = { attempts: [] };
      blocks.push(group);
    }
    group.attempts.push(entry);
    return entry;
  };
  for (const comment of comments) {
    const body = comment.body || "";
    if (body.startsWith(LOG)) continue;
    if (body.includes(PROGRESS)) {
      open = attempt({ progress: comment, answer: null });
    } else if (kindOf(body) === "answer") {
      if (open) open.answer = comment;
      else attempt({ progress: null, answer: comment });
      open = null;
    } else {
      blocks.push({ message: comment });
      group = null;
    }
  }
  return blocks;
}

/** Les passages d'un message : un seul tel quel ; plusieurs (relances) en pages, du plus
 * ancien au plus récent, la plus récente montrée, à feuilleter avec ‹ › ou d'un glissement. */
function attemptsHtml(group, isLive) {
  const parts = group.attempts.map((entry) => {
    const answer = entry.answer ? entry.answer.body || "" : "";
    return (entry.progress ? messageHtml(entry.progress, isLive(entry.progress), answer) : "")
      + (entry.answer ? messageHtml(entry.answer, false) : "");
  });
  if (parts.length === 1) return parts[0];
  const first = group.attempts[0];
  const key = `attempts:${(first.progress || first.answer).id}`;
  const count = parts.length;
  const saved = state.attemptPages.get(key);
  // Une nouvelle relance s'affiche d'elle-même ; sinon la page choisie reste.
  const index = saved && saved.count === count ? saved.index : count - 1;
  state.attemptPages.set(key, { index, count });
  return `<div class="attempts" data-group="${escapeHtml(key)}">
    <div class="attempts-bar">
      <button class="attempt-step" data-step="-1" aria-label="Réponse précédente"${index === 0 ? " disabled" : ""}>${ICON.back}</button>
      <span class="attempts-label">Réponse ${index + 1}/${count}</span>
      <button class="attempt-step next" data-step="1" aria-label="Réponse suivante"${index === count - 1 ? " disabled" : ""}>${ICON.back}</button>
    </div>
    ${parts.map((part, at) => `<div class="attempt"${at === index ? "" : " hidden"}>${part}</div>`).join("")}
  </div>`;
}

/** Montre la page voisine (`step` : -1 ou 1) d'un groupe de passages. */
function showAttempt(container, step) {
  const pages = [...container.querySelectorAll(":scope > .attempt")];
  const key = container.dataset.group;
  const saved = state.attemptPages.get(key) || { index: pages.length - 1 };
  const index = Math.max(0, Math.min(pages.length - 1, saved.index + step));
  if (index === saved.index) return;
  state.attemptPages.set(key, { index, count: pages.length });
  pages.forEach((page, at) => { page.hidden = at !== index; });
  container.querySelector(".attempts-label").textContent = `Réponse ${index + 1}/${pages.length}`;
  const [previous, next] = container.querySelectorAll(":scope > .attempts-bar .attempt-step");
  previous.disabled = index === 0;
  next.disabled = index === pages.length - 1;
}

/** Boutons ‹ › et glissement horizontal des groupes de passages. */
function wireAttempts(view) {
  for (const container of view.querySelectorAll(".attempts")) {
    for (const button of container.querySelectorAll(":scope > .attempts-bar .attempt-step")) {
      button.addEventListener("click", () => showAttempt(container, Number(button.dataset.step)));
    }
    let start = null;
    container.addEventListener("touchstart", (event) => {
      const touch = event.touches[0];
      // Pas depuis ce qui défile lui-même de côté (code, tableaux, différences).
      start = event.touches.length === 1 && !event.target.closest("pre, .table, .diff")
        ? { x: touch.clientX, y: touch.clientY } : null;
    }, { passive: true });
    container.addEventListener("touchend", (event) => {
      if (!start) return;
      const touch = event.changedTouches[0];
      const dx = touch.clientX - start.x;
      const dy = touch.clientY - start.y;
      start = null;
      if (Math.abs(dx) > 60 && Math.abs(dx) > 2 * Math.abs(dy)) showAttempt(container, dx < 0 ? 1 : -1);
    }, { passive: true });
  }
}

/** État d'un passage d'après la première ligne de sa progression (« **Claw a fini** en … »). */
function runKind(head) {
  if (/travaille/.test(head)) return "running";
  if (/a fini/.test(head)) return "done";
  if (/été arrêté/.test(head)) return "stopped";
  if (/erreur/.test(head)) return "failed";
  return "unfinished";
}

/** Le Markdown d'une réponse, une ligne entièrement en gras (« **Résultat** ») devenue un titre,
 * hors des blocs de code. */
function answerTitles(markdown) {
  return String(markdown).split(/(^\s*```[\s\S]*?^\s*```)/m)
    .map((part, index) => (index % 2 ? part : part.replace(/^\*\*([^*\n]+)\*\*[ \t]*$/gm, "# $1"))).join("");
}

/** Pied d'une réponse du moteur (après « --- ») : pull request, branche et job en pastilles,
 * modèle et requêtes à déplier. */
function answerFoot(foot) {
  const chips = [];
  const lines = [];
  for (const raw of foot.split("\n")) {
    const item = raw.replace(/^\s*[-*]\s+/, "").trim();
    if (!item) continue;
    let match = item.match(/^Pull request : (https:\/\/github\.com\/[\w.-]+\/[\w.-]+\/pull\/(\d+))\s*$/);
    if (match) {
      chips.push(`<a class="foot-chip" href="${escapeHtml(match[1])}" target="_blank" rel="noopener noreferrer">${ICON.pull}<span>PR #${match[2]}</span></a>`);
      continue;
    }
    match = item.match(/^Branche `[^`]+` : (\d+) fichiers? \S+, \+([\d\s ]+) lignes?, [−-]([\d\s ]+) lignes?$/);
    if (match) {
      chips.push(`<span class="foot-chip">${ICON.diff}<span>${match[1]} fichier${match[1] === "1" ? "" : "s"} `
        + `<b class="add">+${escapeHtml(match[2].trim())}</b> <b class="del">−${escapeHtml(match[3].trim())}</b></span></span>`);
      continue;
    }
    match = item.match(/^\[Détail du job\]\((https:\/\/github\.com\/\S+)\)$/);
    if (match) {
      chips.push(`<a class="foot-chip" href="${escapeHtml(match[1])}" target="_blank" rel="noopener noreferrer">${ICON.external}<span>Job</span></a>`);
      continue;
    }
    lines.push(item);
  }
  if (!chips.length && !lines.length) return "";
  return `<footer class="answer-foot">
    ${chips.length ? `<div class="foot-chips">${chips.join("")}</div>` : ""}
    ${lines.length ? `<details class="foot-more"><summary>${ICON.gauge}<span>Modèle et requêtes</span></summary>${renderMarkdown(lines.map((line) => `- ${line}`).join("\n"))}</details>` : ""}
  </footer>`;
}

// Questions qui finissent une réponse de Claw (outil AskUserQuestion) : en données pour l'appli,
// puis en texte, jusqu'à la marque de fin, pour GitHub et pour l'historique de l'agent.
const QUESTIONS_BLOCK = /<!--\s*claw-web:questions\s+(\[[\s\S]*?\])\s*-->[\s\S]*?<!--\s*claw-web:questions-end\s*-->/;

/** Les questions d'un bloc, chacune avec au moins deux réponses proposées ; null sinon. */
function questionsOf(data) {
  try {
    const list = JSON.parse(data).filter((question) => question && question.question
      && Array.isArray(question.options) && question.options.filter((option) => option && option.label).length >= 2);
    return list.length ? list : null;
  } catch {
    return null;
  }
}

/** Les questions de Claw et leurs réponses proposées, à toucher (comme dans Claude Code) ;
 * désactivées tant que wireQuestions ne les ouvre pas. */
function questionsHtml(questions) {
  const several = questions.length > 1 || questions.some((question) => question.multi_select);
  return `<div class="questions">
    ${questions.map((question) => `
      <div class="question" role="group" aria-label="${escapeHtml(question.question)}" data-topic="${escapeHtml(question.header || question.question)}"${question.multi_select ? " data-multi" : ""}>
        <p class="question-text">${question.header ? `<span class="question-head">${escapeHtml(question.header)}</span>` : ""}${escapeHtml(question.question)}</p>
        ${question.options.filter((option) => option && option.label).map((option) => `
          <button type="button" class="choice" data-label="${escapeHtml(option.label)}" disabled>
            <strong>${escapeHtml(option.label)}</strong>${option.description ? `<small>${escapeHtml(option.description)}</small>` : ""}
          </button>`).join("")}
      </div>`).join("")}
    ${several ? `<button type="button" class="small-link primary-link send-choices" disabled>${ICON.send}<span>Envoyer les réponses</span></button>` : ""}
    <p class="choices-hint">Ou répondez avec vos mots, en bas.</p>
  </div>`;
}

/** Questions de la dernière réponse, tant que rien ne l'a suivie : une question seule part d'un
 * toucher ; plusieurs (ou à choix multiple) avec « Envoyer les réponses ». */
function wireQuestions(view, open) {
  const answers = view.querySelectorAll(".messages .message.claw");
  const block = answers.length ? answers[answers.length - 1].querySelector(".questions") : null;
  if (!open || !block || block.closest(".attempt[hidden]")) return;
  block.classList.add("open");
  const questions = [...block.querySelectorAll(".question")];
  const choices = [...block.querySelectorAll(".choice")];
  const send = block.querySelector(".send-choices");
  const answer = (text) => {
    if (state.sending) return;
    for (const choice of choices) choice.disabled = true;
    if (send) send.disabled = true;
    // Un champ à part : un brouillon tapé en bas reste où il est.
    followUpSession(text, document.createElement("textarea"));
  };
  for (const choice of choices) {
    choice.disabled = false;
    choice.addEventListener("click", () => {
      if (!send) {
        choice.classList.add("chosen");
        answer(choice.dataset.label);
        return;
      }
      const question = choice.closest(".question");
      if (!question.hasAttribute("data-multi")) {
        for (const other of question.querySelectorAll(".choice")) if (other !== choice) other.classList.remove("chosen");
      }
      choice.classList.toggle("chosen");
      send.disabled = !questions.every((each) => each.querySelector(".choice.chosen"));
    });
  }
  send?.addEventListener("click", () => answer(questions.map((question) => `${question.dataset.topic} : `
    + [...question.querySelectorAll(".choice.chosen")].map((choice) => choice.dataset.label).join(", ")).join("\n")));
}

/** Réponse de Claw : son texte (titres, remarques du moteur en encadré), ses questions à
 * toucher, puis le pied. */
function answerHtml(body) {
  const text = String(body || "").replace(/\r\n/g, "\n");
  const cut = text.lastIndexOf("\n---\n");
  const main = cut >= 0 ? text.slice(0, cut) : text;
  const block = main.match(QUESTIONS_BLOCK);
  const questions = block && questionsOf(block[1]);
  const content = questions
    ? renderMarkdown(answerTitles(main.slice(0, block.index))) + questionsHtml(questions)
      + renderMarkdown(answerTitles(main.slice(block.index + block[0].length)))
    : renderMarkdown(answerTitles(main));
  return `<article class="message claw">
    <div class="who"><span class="logo" aria-hidden="true"></span><span>Claw</span></div>
    <div class="answer">${content}</div>
    ${cut >= 0 ? answerFoot(text.slice(cut + 5)) : ""}
  </article>`;
}

function messageHtml(comment, isLastProgress, answer = "") {
  const body = comment.body || "";
  if (body.startsWith(LOG)) return "";
  if (body.includes(PROGRESS)) {
    const run = runOf(body);
    const events = run ? journalOf(run) : [];
    const head = body.split("\n").find((line) => line.startsWith("**Claw")) || "";
    const kind = runKind(head);
    if (events.length) {
      // « · 161 action(s) » : le compte est déjà dans la pastille du journal.
      // Durée insécable : « 12 min 05 s » ne se coupe pas.
      const title = renderMarkdown(head.replace(/\s*·\s*\d+\s+action\(s\)\s*$/, "")).replace(/^<p>|<\/p>$/g, "")
        .replace(/(\d+) (min|s)\b/g, "$1 $2").replace(/(min) (\d)/g, "$1 $2");
      return `<div class="run-card ${kind}">${transcriptHtml(events, { key: `run${run}`, live: isLastProgress, answer, title, kind })}</div>`;
    }
    return `<div class="progress-card ${kind}${isLastProgress ? "" : " old"}">${renderMarkdown(withoutCallMarkup(body))}</div>`;
  }
  if (body.includes(ANSWER)) return answerHtml(body);
  return userMessageHtml(comment.user ? comment.user.login : "", body);
}

/** Étapes du moteur avant le travail de Claw, en clair. */
const PREPARING_STEPS = {
  "Compiler Claw": "Compilation de Claw : la première fois sur une machine, ou après une mise à jour de Claw, quelques minutes",
  "Installer Rust": "Installation de Rust, pour compiler Claw",
  "Source de Claw": "Lecture du code de Claw",
  "Binaire de Claw en cache": "Claw déjà compilé : récupération",
  "Session et dépôt de la tâche": "Préparation du dépôt de la tâche",
  "Claw au travail": "Claw démarre",
};

/** Carte « Claw se prépare » : le passage lancé, avant sa première progression. */
function preparingHtml(run, status, step) {
  const detail = !run ? "Lancement du passage"
    : status && status.key === "queued" ? status.label
      : PREPARING_STEPS[step] || step || (status ? status.label : "");
  return `<div class="run-card preparing"><div class="preparing-row">
    <span class="run-state running"><span class="spinner" aria-label="en cours"></span></span>
    <span class="run-title"><strong>Claw se prépare</strong><span class="run-sub">${escapeHtml(detail)}</span></span>
  </div></div>`;
}

/** Message de l'utilisateur en bulle ; le nom seulement quand c'est quelqu'un d'autre. */
function userMessageHtml(login, body) {
  const other = login && state.user && login !== state.user.login;
  return `<article class="message user">${other ? `<div class="who">${escapeHtml(login)}</div>` : ""}${messageBody(body)}</article>`;
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
  const known = latestRun(settings.repo, session.number);
  // Passage demandé depuis l'appli que GitHub ne liste pas encore (le dernier connu est celui
  // d'avant) : la session l'attend, une minute et demie au plus, sans montrer l'ancien.
  const launch = state.launch && state.launch.session === session.number && Date.now() - state.launch.at < 90_000
    ? state.launch : null;
  const waiting = Boolean(launch) && (!known || known.id === launch.before);
  const run = waiting ? null : known;
  const status = runStatus(run);
  const comments = session.comments;
  const lastProgress = [...comments].reverse().find((comment) => (comment.body || "").includes(PROGRESS));
  const answered = comments.length && (comments[comments.length - 1].body || "").includes(ANSWER);
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
  // Passage lancé, pas encore de progression pour le dernier message : Claw se prépare (file
  // d'attente, compilation…), dit dans la conversation plutôt que tout en bas.
  const preparing = active && (!lastProgress || (lastMessage && lastProgress.created_at < lastMessage.created_at));
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
        ${userMessageHtml(session.issue.user ? session.issue.user.login : "", taskOf(session.issue))}
        ${discussionBlocks(comments).map((block) => (block.message ? messageHtml(block.message, false)
          : attemptsHtml(block, (comment) => comment === lastProgress && !answered))).join("")}
        ${preparing ? preparingHtml(run, status, session.step) : ""}
      </div>
      ${unsent ? `<div class="unsent">Ce message n'a pas encore été envoyé à Claw (aucun passage lancé depuis).
        <button class="small-link primary-link" data-action="retry">${ICON.retry}<span>Lancer le passage</span></button></div>` : ""}
      <div class="run-status ${active ? "active" : status ? status.key : ""}">
        ${active && !preparing ? `<span class="spinner" aria-hidden="true"></span><span>${run ? escapeHtml(status.label) : "Lancement du passage"}${step}</span>` : ""}
        ${!active && status && status.key === "failed" && !answered ? `<span>Le dernier passage a échoué.</span>` : ""}
        <span class="run-actions">
          ${active && run ? `<button class="small-link stop" data-action="stop">${ICON.stop}<span>Arrêter</span></button>` : ""}
          ${!active && !unsent ? `<button class="small-link" data-action="retry">${ICON.retry}<span>Relancer</span></button>` : ""}
          ${!active ? `<button class="small-link" data-action="publish-site">${ICON.globe}<span>Mettre en ligne</span></button>` : ""}
          <details class="more-menu">
            <summary class="small-link" aria-label="Autres actions"><span class="dots" aria-hidden="true">•••</span></summary>
            <div class="menu-list">
              ${run ? `<a href="${escapeHtml(run.html_url)}" target="_blank" rel="noopener noreferrer" class="menu-item">${ICON.external}<span>Job sur GitHub</span></a>` : ""}
              <a href="${escapeHtml(session.issue.html_url)}" target="_blank" rel="noopener noreferrer" class="menu-item">${ICON.external}<span>Session sur GitHub</span></a>
              ${session.issue.state === "open" ? `<button class="menu-item" data-action="archive">${ICON.archive}<span>Archiver</span></button>` : ""}
              ${!active ? `<button class="menu-item danger-item" data-action="delete">${ICON.trash}<span>Supprimer</span></button>` : ""}
            </div>
          </details>
        </span>
      </div>
    </section>`;
  for (const details of view.querySelectorAll("details[data-key]")) {
    if (opened.has(details.dataset.key)) details.open = opened.get(details.dataset.key);
  }
  // Menu « ••• » : se ferme en touchant ailleurs dans la session, ou après un choix.
  const menu = view.querySelector(".more-menu");
  menu?.addEventListener("toggle", () => {
    if (!menu.open) return;
    const list = menu.querySelector(".menu-list");
    list.classList.remove("from-left");
    // Bouton trop à gauche pour la largeur du menu : il s'ouvre vers la droite.
    if (menu.getBoundingClientRect().right - list.offsetWidth < 8) list.classList.add("from-left");
  });
  view.querySelector(".session").addEventListener("click", (event) => {
    if (menu && menu.open && (!event.target.closest(".more-menu") || event.target.closest(".menu-item"))) menu.open = false;
  });
  wireAttempts(view);
  // Questions de la dernière réponse : ouvertes tant que rien ne l'a suivie.
  wireQuestions(view, Boolean(answered) && !active);
  for (const retry of view.querySelectorAll('[data-action="retry"]')) {
    retry.addEventListener("click", async () => {
      // Un seul passage par appui : le bouton se désactive aussitôt (plusieurs appuis lançaient
      // plusieurs passages).
      for (const button of view.querySelectorAll('[data-action="retry"]')) button.disabled = true;
      try {
        // « Lancer le passage » : un passage parti entre-temps, que GitHub ne listait pas encore,
        // n'est pas lancé une seconde fois.
        if (retry.closest(".unsent")) {
          await refreshRuns();
          const latest = latestRun(settings.repo, session.number);
          if (latest && latest.created_at >= lastMessage.created_at) {
            toast("Ce message avait déjà son passage");
            await pollSession(true);
            return;
          }
        }
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
    const photo = event.target.closest(".photo-pill[data-photo]");
    if (photo) openPhoto(photo.dataset);
    const shot = event.target.closest(".photo-pill[data-shot]");
    if (shot) openShot(shot.dataset);
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
  } else if (state.session.issue) {
    // Déjà connue (session à peine créée, retour sur la page) : montrée avant la relecture.
    renderSessionView();
    renderComposer();
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
  noteRequests(session);
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

/** Heures des requêtes de chaque passage d'une session qui travaille avec un modèle de NVIDIA,
 * d'après son journal (événements « req » du moteur, une par réponse du modèle). */
function noteRequests(session) {
  if (providerOf(session.settings && session.settings.model) !== "nvidia") return;
  const byRun = new Map();
  for (const log of session.logs.values()) {
    const times = log.events.filter((event) => event.k === "req" && event.at).map((event) => event.at);
    if (times.length) byRun.set(log.run, [...(byRun.get(log.run) || []), ...times]);
  }
  for (const [run, times] of byRun) state.liveRequests.set(run, times);
  showQuotaLabel();
}

/** Requêtes envoyées à NVIDIA dans la dernière minute par les passages suivis ; les passages
 * sans requête depuis dix minutes sont oubliés. */
function requestsLastMinute() {
  const now = Date.now() / 1000;
  let count = 0;
  for (const [run, times] of state.liveRequests) {
    if (times.every((at) => now - at > 600)) state.liveRequests.delete(run);
    else count += times.filter((at) => now - at < 60).length;
  }
  return count;
}

/** Hors de la page d'une session (la sienne s'en charge) : les sessions en cours relues toutes
 * les 20 s, pour le compteur par minute de NVIDIA ; une fois par minute sinon. */
async function watchRequests() {
  clearTimeout(state.requestTimer);
  let busy = false;
  if (state.github && !document.hidden && !state.session && providerOf(state.settings.model) === "nvidia") {
    try {
      const engine = state.settings.engine;
      await refreshRuns();
      const running = state.runs.filter((entry) => entry.status === "in_progress")
        .map((entry) => Number((String(entry.display_title || "").match(/#(\d+)$/) || [])[1])).filter(Boolean);
      for (const number of state.watched.keys()) if (!running.includes(number)) state.watched.delete(number);
      for (const number of running) {
        busy = true;
        let watched = state.watched.get(number);
        if (!watched) {
          watched = { settings: settingsOf(await state.github.issue(engine, number)), logs: new Map(), comments: [] };
          state.watched.set(number, watched);
        }
        const since = watched.comments.reduce((last, comment) => (comment.updated_at > last ? comment.updated_at : last), "");
        const fresh = await state.github.comments(engine, number, since);
        const byId = new Map(watched.comments.map((comment) => [comment.id, comment]));
        for (const comment of fresh) byId.set(comment.id, comment);
        // Seuls les journaux servent ici : le reste n'est pas gardé.
        watched.comments = [...byId.values()].filter((comment) => (comment.body || "").startsWith(LOG));
        await readJournals(watched, watched.comments);
        noteRequests(watched);
      }
    } catch {
      // GitHub injoignable : le compteur garde ce qu'il sait, nouvel essai au tour suivant.
    }
  }
  state.requestTimer = setTimeout(() => watchRequests().catch(() => {}), busy ? 20_000 : 60_000);
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
async function configureEngine(openrouterKey, shareToken, providerKeys = {}) {
  const engine = state.settings.engine;
  await state.github.ensureLabel(engine);
  const key = await state.github.secretKey(engine);
  const secrets = [];
  if (openrouterKey) secrets.push(["OPENROUTER_API_KEY", openrouterKey]);
  if (shareToken) secrets.push(["CLAW_REPO_TOKEN", state.settings.token]);
  // Clés des fournisseurs (NVIDIA, Gemini), par nom de secret.
  for (const [name, value] of Object.entries(providerKeys)) {
    if (value) secrets.push([name, value]);
  }
  for (const [name, value] of secrets) {
    await state.github.putSecret(engine, name, sealedBox(value, key.key), key.key_id);
  }
  state.providerSecrets = null;
  return secrets.map(([name]) => name);
}

async function engineChecks() {
  const engine = state.settings.engine;
  const checks = [];
  state.machinesOnline = null;
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
  // Sessions sur vos machines (PC, serveur dans le nuage) : un exécuteur `claw-pc` en ligne.
  if (state.settings.runner) {
    try {
      const mine = (await state.github.runners(engine))
        .filter((runner) => (runner.labels || []).some((label) => label.name === state.settings.runner));
      const online = mine.filter((runner) => runner.status === "online");
      const offline = mine.filter((runner) => runner.status !== "online").map((runner) => runner.name);
      state.machinesOnline = online.map((runner) => runner.name);
      if (online.length) {
        add(true, `Exécuteur${online.length > 1 ? "s" : ""} en ligne : ${online.map((runner) => `${runner.name}${runner.busy ? " (en session)" : ""}`).join(", ")}`
          + (offline.length ? ` ; hors ligne : ${offline.join(", ")}` : ""));
      } else if (mine.length) {
        add(false, `Exécuteur${mine.length > 1 ? "s" : ""} hors ligne : ${offline.join(", ")} (serveur arrêté, ou PC éteint)`);
      } else {
        add(false, "Aucun exécuteur : installez le serveur gratuit (bouton ci-dessous) ou celui du PC (claw-pc.ps1)");
      }
    } catch (error) {
      checks.push(`<li>${escapeHtml(error.status === 403
        ? "État des exécuteurs inconnu : le jeton n'a pas l'autorisation Administration (lecture)"
        : `Exécuteurs : ${error.message}`)}</li>`);
    }
  }
  // Fournisseurs à clé, facultatifs : leur absence n'est pas une erreur (point gris).
  state.providerSecrets = null;
  const secrets = await engineProviderSecrets();
  for (const provider of KEYED_PROVIDERS) {
    checks.push(secrets[provider.secret]
      ? `<li class="ok">${escapeHtml(`Secret ${provider.secret} : modèles ${provider.label} proposés`)}</li>`
      : `<li>${escapeHtml(`Secret ${provider.secret} absent (facultatif : modèles ${provider.label})`)}</li>`);
  }
  return checks.join("");
}

const MACHINE_NOTE = "Serveur gratuit ou PC : aucune minute";

/** Après les vérifications : la ligne de synthèse du moteur (dépliée s'il y a un point à
 * régler) et les machines en ligne sous « Ma machine ». */
function showEngineState() {
  const view = $("#view");
  const summary = view.querySelector(".engine-state > summary");
  if (!summary) return;
  const failed = view.querySelectorAll(".checks li.ko").length;
  const passed = view.querySelectorAll(".checks li.ok").length;
  summary.querySelector(".state").className = `row-icon state ${failed ? "ko" : "ok"}`;
  summary.querySelector(".row-main").innerHTML = failed
    ? `<span class="row-title">${failed} point${failed > 1 ? "s" : ""} à régler</span><span class="row-sub">Touchez pour le détail</span>`
    : `<span class="row-title">Moteur prêt</span><span class="row-sub">${passed} vérifications au vert</span>`;
  if (failed) view.querySelector(".engine-state").open = true;
  const machine = view.querySelector('.choice input[value="claw-pc"]')?.closest(".choice")?.querySelector(".row-sub");
  if (machine) {
    const online = state.settings.runner ? state.machinesOnline : null;
    machine.textContent = !online ? MACHINE_NOTE
      : online.length ? `Aucune minute ; en ligne : ${online.join(", ")}` : "Aucune minute ; aucune machine en ligne";
  }
}

function showSettings(first = false, justConnected = false) {
  state.session = null;
  $("#composer").hidden = true;
  const settings = state.settings;
  const login = (state.user && state.user.login) || "";
  const engine = settings.engine || DEFAULT_ENGINE;
  // Une ligne de carte : icône facultative, titre, sous-titre.
  const text = (title, sub) => `<span class="row-main"><span class="row-title">${title}</span>${sub ? `<span class="row-sub">${sub}</span>` : ""}</span>`;
  const field = (title, sub, input) => `<label class="field-row">${text(title, sub)}${input}</label>`;
  const toggle = (name, title, sub) => `<label class="row switch-row">${text(title, sub)}<input name="${name}" type="checkbox" role="switch" checked></label>`;
  // « Clé Google Gemini (facultative : offre gratuite de Google) » : titre et note.
  const providerField = (provider) => {
    const [title, note = ""] = provider.field.split(/ \((.*)\)$/);
    return field(escapeHtml(title), escapeHtml(note.charAt(0).toUpperCase() + note.slice(1)),
      `<input name="${provider.key}" type="password" placeholder="${escapeHtml(provider.prefix)}…" spellcheck="false">`);
  };
  const perms = [
    ["Toujours", ["Actions", "Contents", "Issues", "Pull requests", "Secrets"]],
    ["Serveur gratuit et robot", ["Administration", "Variables"]],
    ["Créer des dépôts", ["All repositories", "Administration"]],
    ["Mettre un site en ligne", ["Pages"]],
  ];
  $("#view").innerHTML = `
    <section class="settings">
      ${state.github ? `<a class="back" href="#/">${ICON.back}<span>Accueil</span></a>` : ""}
      <h1>Réglages</h1>
      ${first ? '<p class="intro">Claw travaille sur vos dépôts GitHub depuis GitHub Actions. Pour commencer, donnez à l\'appli un jeton GitHub.</p>' : ""}
      ${justConnected ? `<div class="ready"><p>Connecté. Configurez le moteur ci-dessous une fois, puis décrivez votre première tâche depuis l'accueil.</p><a class="primary" href="#/">Aller à l'accueil</a></div>` : ""}

      <h2 class="group-title">Compte GitHub</h2>
      <div class="group">
        ${state.github ? `<div class="row account">
          <span class="avatar" aria-hidden="true">${escapeHtml(login.charAt(0).toUpperCase() || "?")}</span>
          ${text(escapeHtml(login), `Moteur : ${escapeHtml(engine)}`)}
          <span class="pill ok">Connecté</span>
        </div>` : ""}
        <details class="disclosure"${state.github ? "" : " open"}>
          <summary class="row"><span class="row-icon">${ICON.key}</span>${text("Jeton et dépôt du moteur", "")}${ICON.chevron}</summary>
          <form class="settings-form" autocomplete="off">
            ${field("Jeton GitHub", "Jeton à grain fin ; il reste dans ce navigateur",
              `<input name="token" type="password" placeholder="github_pat_…" value="${escapeHtml(settings.token || "")}" spellcheck="false">`)}
            ${field("Dépôt du moteur", "Où vivent les sessions (propriétaire/nom)",
              `<input name="engine" type="text" value="${escapeHtml(engine)}" spellcheck="false">`)}
            <div class="row-actions"><button class="primary" type="submit">Enregistrer</button></div>
          </form>
        </details>
      </div>
      <details class="group-foot more"${state.github ? "" : " open"}>
        <summary>Autorisations du jeton</summary>
        <p>Créez un <a href="${TOKEN_HELP}" target="_blank" rel="noopener noreferrer">jeton à grain fin</a> pour le moteur et les dépôts où Claw travaillera. Ses autorisations, toutes en lecture et écriture :</p>
        <dl class="perms">${perms.map(([when, names]) => `<dt>${when}</dt><dd>${names.map((name) => `<span class="perm">${name}</span>`).join("")}</dd>`).join("")}</dl>
        <p>Modifier ces autorisations sur GitHub garde la valeur du jeton : rien à refaire ici.</p>
      </details>
      ${state.github ? `

      <h2 class="group-title">Moteur</h2>
      <div class="group">
        <details class="disclosure engine-state">
          <summary class="row"><span class="row-icon state"></span>${text("Vérification…", "")}${ICON.chevron}</summary>
          <ul class="checks"><li>Vérification…</li></ul>
        </details>
        <details class="disclosure"${justConnected ? " open" : ""}>
          <summary class="row"><span class="row-icon">${ICON.key}</span>${text("Clés des modèles", ["OpenRouter", ...KEYED_PROVIDERS.map((provider) => provider.label)].join(", "))}${ICON.chevron}</summary>
          <form class="engine-form" autocomplete="off">
            ${field("Clé OpenRouter", "Modèles gratuits", '<input name="openrouter" type="password" placeholder="sk-or-…" spellcheck="false">')}
            ${KEYED_PROVIDERS.map(providerField).join("")}
            ${toggle("share", "Donner le jeton au moteur", "Secret CLAW_REPO_TOKEN : il pousse ses branches et ouvre les pull requests")}
            ${toggle("keep", "Garder la clé OpenRouter ici", "Pour afficher les requêtes gratuites restantes")}
            <div class="row-actions"><button class="primary" type="submit">Configurer le moteur</button></div>
          </form>
        </details>
      </div>
      <div class="group-foot">
        <p>Les clés partent chiffrées depuis le navigateur. Un champ vide garde la clé déjà posée.</p>
        <details class="more">
          <summary>Où créer ces clés</summary>
          <p>NVIDIA : build.nvidia.com (profil, Settings, API Keys). Gemini : aistudio.google.com/apikey, dans un projet sans facturation (sinon chaque requête est payante). Les clés NVIDIA et Gemini ne sont jamais gardées sur cet appareil ; la clé OpenRouter seulement si « Garder la clé OpenRouter ici » est activé.</p>
        </details>
      </div>

      <h2 class="group-title">Sessions</h2>
      <div class="group runner-choice" role="radiogroup" aria-label="Machine des sessions">
        ${[["", ICON.globe, "Machine de GitHub", "Prend les minutes GitHub Actions du compte"],
          ["claw-pc", ICON.server, "Ma machine", MACHINE_NOTE]].map(([value, icon, title, sub]) => `
          <label class="row choice"><span class="row-icon">${icon}</span>${text(title, sub)}<input type="radio" name="runner" value="${value}"${(settings.runner || "") === value ? " checked" : ""}></label>`).join("")}
      </div>
      <div class="group">
        <button type="button" class="row link-row" data-action="cloud-runner"><span class="row-icon">${ICON.cloud}</span>${text("Serveur gratuit dans le nuage", "Oracle Cloud, allumé jour et nuit")}${ICON.chevron}</button>
      </div>
      <div class="group-foot">
        <p>Une session part sur la machine libre ; sans machine en ligne, elle attend dans la file de GitHub. Sur le PC : <code>.github/claw-web/pc/claw-pc.ps1 install</code>, avec Docker Desktop.</p>
      </div>

      <h2 class="group-title">Apparence</h2>
      <div class="segmented" role="radiogroup" aria-label="Thème">
        ${[["", "Sombre"], ["light", "Clair"], ["system", "Auto"]].map(([value, label]) => `
          <label><input type="radio" name="theme" value="${value}"${(settings.theme || "") === value ? " checked" : ""}><span>${label}</span></label>`).join("")}
      </div>
      <div class="group-foot"><p>Auto : comme le téléphone.</p></div>

      <h2 class="group-title">Requêtes gratuites</h2>
      <form class="group quota-form" autocomplete="off">
        ${field("Clé OpenRouter de cet appareil", settings.openrouterKey ? "Enregistrée" : "Aucune",
          `<input name="key" type="password" placeholder="${settings.openrouterKey ? "clé enregistrée" : "sk-or-…"}" spellcheck="false">`)}
        <div class="row-actions">
          <button class="primary" type="submit">Enregistrer la clé</button>
          ${settings.openrouterKey ? '<button class="small-link" type="button" data-action="forget-key">Effacer la clé de cet appareil</button>' : ""}
        </div>
      </form>
      <div class="group-foot">
        <p>Sert seulement à lire chez OpenRouter les requêtes gratuites du jour et les crédits, affichés près du choix du modèle.</p>
        <details class="more">
          <summary>Conseil</summary>
          <p>Créez pour l'appli une clé avec une limite de crédit de 0 $ (réglage « Credit limit » de la clé sur openrouter.ai) : elle ne peut rien dépenser.</p>
        </details>
      </div>

      <div class="group danger-group">
        <button type="button" class="row danger-row" data-action="forget">Effacer le jeton de cet appareil</button>
      </div>` : ""}
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
      // Autre jeton ou autre moteur : ses secrets se relisent.
      state.providerSecrets = null;
      state.settings.engine = engine;
      saveSettings();
      showQuotaLabel();
      refreshQuota();
      watchRequests().catch(() => {});
      toast(`Connecté en tant que ${state.user.login}`);
      $("#settings-button").classList.add("active");
      showSettings(false, true);
    } catch (error) {
      failure(error);
    }
  });
  const engineForm = $("#view .engine-form");
  if (engineForm) {
    engineChecks().then(async (html) => {
      const list = $("#view .checks");
      if (list) list.innerHTML = html;
      showEngineState();
      // Clé d'un fournisseur déjà posée : le champ le dit (vide, elle reste).
      const found = await engineProviderSecrets();
      for (const provider of KEYED_PROVIDERS) {
        if (found[provider.secret] && engineForm[provider.key]) engineForm[provider.key].placeholder = "Clé posée : vide, elle reste";
      }
    }).catch(failure);
    engineForm.addEventListener("submit", async (event) => {
      event.preventDefault();
      const form = event.target;
      try {
        // Clés des fournisseurs (NVIDIA, Gemini) : une clé d'une autre forme est refusée avant
        // tout envoi (une clé collée dans le mauvais champ).
        const providerKeys = {};
        for (const provider of KEYED_PROVIDERS) {
          const value = form[provider.key].value.trim();
          if (value && !value.startsWith(provider.prefix)) {
            toast(`Une clé ${provider.label} commence par ${provider.prefix} (${provider.where})`, "error");
            return;
          }
          providerKeys[provider.secret] = value;
        }
        const typed = form.openrouter.value.trim();
        if (typed && form.keep.checked) {
          state.settings.openrouterKey = typed;
          state.quota = null;
          saveSettings();
        }
        // Champ vide : la clé gardée sur cet appareil, s'il y en a une.
        const names = await configureEngine(typed || state.settings.openrouterKey || "", form.share.checked, providerKeys);
        form.openrouter.value = "";
        for (const provider of KEYED_PROVIDERS) form[provider.key].value = "";
        toast(names.length ? `Moteur configuré (${names.join(", ")})` : "Étiquette prête ; aucun secret envoyé");
        const list = $("#view .checks");
        if (list) list.innerHTML = await engineChecks();
        showEngineState();
      } catch (error) {
        failure(error);
      }
    });
    $("#view").querySelector('[data-action="cloud-runner"]')?.addEventListener("click", openCloudRunner);
    for (const radio of $("#view").querySelectorAll('input[name="runner"]')) {
      radio.addEventListener("change", async () => {
        if (radio.value) state.settings.runner = radio.value;
        else delete state.settings.runner;
        saveSettings();
        toast(radio.value ? "Sessions sur votre machine (serveur gratuit ou PC)" : "Sessions sur une machine de GitHub");
        const list = $("#view .checks");
        if (list) list.innerHTML = await engineChecks().catch((error) => `<li class="ko">${escapeHtml(error.message)}</li>`);
        showEngineState();
      });
    }
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
    if (!document.hidden) watchRequests().catch(() => {});
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
      watchRequests().catch(() => {});
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
