// Verrou de Claw Web : les clés gardées sur l'appareil (jeton GitHub, clé OpenRouter) sont
// chiffrées par un code que seul l'utilisateur connaît (AES-GCM, clé tirée du code par
// PBKDF2-SHA-256). Sans le code, ni l'appli ni quelqu'un qui lit le stockage du navigateur
// n'a le jeton : il faut se connecter à chaque ouverture, et après une absence.

const VAULT = "claw-web.vault.v1";
const TRIES = "claw-web.lock.tries";
// Tours de PBKDF2 : ce qu'OWASP conseille pour SHA-256 (2023), environ une demi-seconde sur un
// téléphone ; un code essayé au hasard coûte autant.
const ITERATIONS = 600_000;
export const MIN_CODE = 8;
// Absence (appli en arrière-plan) après laquelle le code est redemandé.
export const LOCK_AFTER_MS = 15 * 60_000;
// Les clés chiffrées : le reste des réglages reste en clair (dépôt, modèle, thème).
export const SECRET_FIELDS = ["token", "openrouterKey"];

const encoder = new TextEncoder();
const bytes = (base64) => Uint8Array.from(atob(base64), (char) => char.charCodeAt(0));
const base64 = (data) => btoa(String.fromCharCode(...new Uint8Array(data)));

function read(name) {
  try {
    return JSON.parse(localStorage.getItem(name) || "null");
  } catch {
    return null;
  }
}

/** Le coffre de l'appareil (sel, vecteur, clés chiffrées, compte), s'il y en a un. */
export const vault = () => {
  const found = read(VAULT);
  return found && found.v === 1 && found.salt && found.iv && found.data ? found : null;
};

async function deriveKey(code, salt, iterations = ITERATIONS) {
  const material = await crypto.subtle.importKey("raw", encoder.encode(code), "PBKDF2", false, ["deriveKey"]);
  return crypto.subtle.deriveKey({ name: "PBKDF2", salt, iterations, hash: "SHA-256" }, material,
    { name: "AES-GCM", length: 256 }, false, ["encrypt", "decrypt"]);
}

/** Chiffre les clés avec la clé de l'appareil (gardée en mémoire seulement) et range le coffre. */
export async function seal(key, salt, secrets, owner) {
  const iv = crypto.getRandomValues(new Uint8Array(12));
  const data = await crypto.subtle.encrypt({ name: "AES-GCM", iv }, key, encoder.encode(JSON.stringify(secrets)));
  localStorage.setItem(VAULT, JSON.stringify({ v: 1, iterations: ITERATIONS, salt: base64(salt), iv: base64(iv), data: base64(data), owner }));
}

/** Pose le verrou : un code neuf, un sel neuf ; rend la clé à garder en mémoire. */
export async function createVault(code, secrets, owner) {
  if (String(code).length < MIN_CODE) throw new Error(`Le code doit avoir au moins ${MIN_CODE} caractères.`);
  const salt = crypto.getRandomValues(new Uint8Array(16));
  const key = await deriveKey(code, salt);
  await seal(key, salt, secrets, owner);
  clearTries();
  return { key, salt };
}

/** Ouvre le coffre avec le code : les clés, et la clé de l'appareil pour les rechiffrer. Un
 * mauvais code lève une erreur (le chiffrement authentifié ne se laisse pas lire). */
export async function openVault(code) {
  const box = vault();
  if (!box) throw new Error("Aucun verrou sur cet appareil.");
  const wait = waitLeft();
  if (wait > 0) throw new Error(`Trop d'essais : réessayez dans ${Math.ceil(wait / 1000)} s.`);
  const salt = bytes(box.salt);
  const key = await deriveKey(code, salt, box.iterations || ITERATIONS);
  try {
    const plain = await crypto.subtle.decrypt({ name: "AES-GCM", iv: bytes(box.iv) }, key, bytes(box.data));
    clearTries();
    return { secrets: JSON.parse(new TextDecoder().decode(plain)), key, salt, owner: box.owner || "" };
  } catch {
    failedTry();
    const left = waitLeft();
    throw new Error(left > 0 ? `Code incorrect. Prochain essai dans ${Math.ceil(left / 1000)} s.` : "Code incorrect.");
  }
}

export function removeVault() {
  localStorage.removeItem(VAULT);
  clearTries();
}

// Essais ratés : après 3, une attente qui double (30 s, 1 min, 2 min...) jusqu'à 15 min. Le coût
// de PBKDF2 reste le vrai frein pour qui lirait le coffre hors de l'appli.
function failedTry() {
  const tries = (read(TRIES) || { count: 0 }).count + 1;
  const wait = tries < 3 ? 0 : Math.min(15 * 60_000, 30_000 * 2 ** (tries - 3));
  localStorage.setItem(TRIES, JSON.stringify({ count: tries, until: Date.now() + wait }));
}

function clearTries() {
  localStorage.removeItem(TRIES);
}

export const waitLeft = () => Math.max(0, ((read(TRIES) || {}).until || 0) - Date.now());
