// Notifications de l'appli (Web Push) : quand Claw pose une question, finit un passage ou
// s'arrête, le moteur (.github/claw-web/push.py) prévient chaque appareil abonné, même l'appli
// fermée. Demandé le 30 octobre 2026.
//
// La paire de clés VAPID du moteur est créée ici, une fois : la clé privée part chiffrée dans le
// secret CLAW_PUSH_VAPID_PRIVATE du dépôt des sessions (elle ne reste pas sur l'appareil), la clé
// publique et les appareils abonnés vont dans ses variables, que le moteur lit.

export const PUSH_PUBLIC = "CLAW_PUSH_VAPID_PUBLIC";
export const PUSH_PRIVATE = "CLAW_PUSH_VAPID_PRIVATE";
export const PUSH_DEVICES = "CLAW_PUSH_SUBSCRIPTIONS";

const toB64u = (bytes) => btoa(String.fromCharCode(...new Uint8Array(bytes)))
  .replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
const fromB64u = (text) => Uint8Array.from(
  atob(text.replace(/-/g, "+").replace(/_/g, "/") + "=".repeat((4 - (text.length % 4)) % 4)),
  (char) => char.charCodeAt(0),
);

const apple = () => /iPhone|iPad|iPod/.test(navigator.userAgent)
  || (navigator.platform === "MacIntel" && navigator.maxTouchPoints > 1);
const standalone = () => window.matchMedia("(display-mode: standalone)").matches || navigator.standalone === true;

/** « ok », « home-screen » (iPhone : l'appli doit être ouverte depuis l'écran d'accueil) ou
 * « unsupported ». */
export function pushSupport() {
  if (!("serviceWorker" in navigator)) return "unsupported";
  if (apple() && !standalone()) return "home-screen";
  if (!("PushManager" in window) || !("Notification" in window)) return "unsupported";
  return "ok";
}

export function deviceName() {
  const agent = navigator.userAgent;
  if (/iPhone/.test(agent)) return "iPhone";
  if (/iPad/.test(agent) || (navigator.platform === "MacIntel" && navigator.maxTouchPoints > 1)) return "iPad";
  if (/Android/.test(agent)) return "Android";
  if (/Mac/.test(agent)) return "Mac";
  if (/Windows/.test(agent)) return "Windows";
  return "Linux";
}

/** L'abonnement de cet appareil, ou null. */
export async function currentSubscription() {
  if (pushSupport() !== "ok") return null;
  const registration = await navigator.serviceWorker.ready;
  return registration.pushManager.getSubscription();
}

async function devices(github, engine) {
  try {
    const list = JSON.parse((await github.variable(engine, PUSH_DEVICES)) || "[]");
    return Array.isArray(list) ? list : [];
  } catch {
    return [];
  }
}

/** La clé publique VAPID du moteur, créée avec sa clé privée la première fois. */
async function serverKey(github, engine, seal) {
  const existing = await github.variable(engine, PUSH_PUBLIC);
  if (existing) return existing;
  const pair = await crypto.subtle.generateKey({ name: "ECDSA", namedCurve: "P-256" }, true, ["sign", "verify"]);
  const publicKey = toB64u(await crypto.subtle.exportKey("raw", pair.publicKey));
  const { d } = await crypto.subtle.exportKey("jwk", pair.privateKey);
  const sealing = await github.secretKey(engine);
  await github.putSecret(engine, PUSH_PRIVATE, seal(d, sealing.key), sealing.key_id);
  await github.setVariable(engine, PUSH_PUBLIC, publicKey);
  // Une clé neuve : les abonnements faits pour une autre ne recevraient plus rien.
  await github.setVariable(engine, PUSH_DEVICES, "[]");
  return publicKey;
}

/** Abonne cet appareil ; rend le nombre d'appareils abonnés. */
export async function enableNotifications(github, engine, seal) {
  const support = pushSupport();
  if (support === "home-screen") {
    throw new Error("Sur iPhone, ajoutez d'abord Claw Web à l'écran d'accueil (Partager, « Sur l'écran d'accueil »), puis ouvrez-le depuis son icône");
  }
  if (support !== "ok") throw new Error("Ce navigateur ne reçoit pas de notifications");
  const permission = await Notification.requestPermission();
  if (permission !== "granted") {
    throw new Error("Notifications refusées : autorisez-les pour Claw Web dans les réglages du téléphone, puis réessayez");
  }
  const key = await serverKey(github, engine, seal);
  const registration = await navigator.serviceWorker.ready;
  let subscription = await registration.pushManager.getSubscription();
  const own = subscription?.options?.applicationServerKey;
  if (subscription && (!own || toB64u(own) !== key)) {
    await subscription.unsubscribe();
    subscription = null;
  }
  subscription ||= await registration.pushManager.subscribe({ userVisibleOnly: true, applicationServerKey: fromB64u(key) });
  const { endpoint, keys } = subscription.toJSON();
  const list = (await devices(github, engine)).filter((item) => item.endpoint !== endpoint);
  list.push({ endpoint, keys, device: deviceName(), added: new Date().toISOString().slice(0, 10) });
  await github.setVariable(engine, PUSH_DEVICES, JSON.stringify(list));
  return list.length;
}

/** Désabonne cet appareil (les autres gardent leurs notifications). */
export async function disableNotifications(github, engine) {
  const subscription = await currentSubscription();
  if (!subscription) return;
  const list = (await devices(github, engine)).filter((item) => item.endpoint !== subscription.endpoint);
  await github.setVariable(engine, PUSH_DEVICES, JSON.stringify(list));
  await subscription.unsubscribe();
}

/** Une notification montrée tout de suite par cet appareil, pour voir à quoi elle ressemble. */
export async function testNotification() {
  const registration = await navigator.serviceWorker.ready;
  await registration.showNotification("Claw a fini", {
    body: "Essai : c'est ainsi que Claw vous préviendra.",
    icon: "icons/icon-192.png",
    badge: "icons/icon-192.png",
    tag: "claw-test",
  });
}
