// Service worker de l'appli : l'interface reste disponible hors connexion (les
// requêtes à GitHub et OpenRouter, elles, ne passent jamais par le cache).
// Réseau d'abord : une nouvelle version publiée est prise dès qu'elle est là.

const CACHE = "claw-web-v83";
const SHELL = [
  "./",
  "index.html",
  "css/app.css",
  "js/app.js",
  "js/github.js",
  "js/markdown.js",
  "js/blake2b.js",
  "js/sealed-box.js",
  "js/transcript.js",
  "js/highlight.js",
  "js/openrouter.js",
  "js/tools.js",
  "js/phrases.js",
  "js/aides.js",
  "js/lock.js",
  "js/mcp-tools.js",
  "js/notifications.js",
  "js/editor.js",
  "claw-design.js",
  "vendor/nacl-fast.min.js",
  "manifest.webmanifest",
  "icons/icon.svg",
  "icons/logo.svg",
  "fonts/hanken-grotesk-latin.woff2",
  "fonts/newsreader-latin.woff2",
  "icons/icon-192.png",
  "icons/icon-512.png",
];

self.addEventListener("install", (event) => {
  event.waitUntil(caches.open(CACHE).then((cache) => cache.addAll(SHELL)).then(() => self.skipWaiting()));
});

self.addEventListener("activate", (event) => {
  event.waitUntil(
    caches.keys()
      .then((keys) => Promise.all(keys.filter((key) => key !== CACHE).map((key) => caches.delete(key))))
      .then(() => self.clients.claim()),
  );
});

// Aperçu d'une branche dans l'éditeur visuel : `apercu/<propriétaire>/<dépôt>/<branche>/<chemin>`
// sert les fichiers de la branche lus par l'API GitHub, avec le jeton que la page donne (gardé
// en mémoire seulement ; redemandé à la page si le service s'est endormi). Aucune publication
// n'est nécessaire, et la page est de la même origine que l'appli : l'éditeur la touche.
let previewToken = "";
const TYPES = {
  html: "text/html; charset=utf-8", js: "text/javascript; charset=utf-8", mjs: "text/javascript; charset=utf-8",
  css: "text/css; charset=utf-8", json: "application/json; charset=utf-8", webmanifest: "application/manifest+json",
  svg: "image/svg+xml", png: "image/png", jpg: "image/jpeg", jpeg: "image/jpeg", webp: "image/webp", gif: "image/gif",
  ico: "image/x-icon", woff2: "font/woff2", woff: "font/woff", wasm: "application/wasm", txt: "text/plain; charset=utf-8",
  mp3: "audio/mpeg", mp4: "video/mp4",
};
self.addEventListener("message", (event) => {
  if (event.data && event.data.type === "claw-preview-token") previewToken = event.data.token || "";
});
async function askToken() {
  const windows = await self.clients.matchAll({ type: "window" });
  for (const client of windows.filter((item) => !item.url.includes("/apercu/"))) {
    const token = await new Promise((resolve) => {
      const channel = new MessageChannel();
      const timer = setTimeout(() => resolve(""), 3000);
      channel.port1.onmessage = (reply) => { clearTimeout(timer); resolve((reply.data && reply.data.token) || ""); };
      client.postMessage({ type: "claw-preview-token-request" }, [channel.port2]);
    });
    if (token) return token;
  }
  return "";
}
async function preview(url) {
  const parts = url.pathname.slice(new URL(self.registration.scope).pathname.length).split("/").slice(1);
  const [owner, repo, ref] = parts;
  let path = parts.slice(3).map(decodeURIComponent).join("/");
  if (!path || path.endsWith("/")) path += "index.html";
  // Le service hors ligne de l'appli ne s'installe pas dans l'aperçu : il prendrait la main sur
  // ses propres adresses et ne trouverait rien derrière.
  if (/(^|\/)(sw|service-worker)\.js$/.test(path)) return new Response("", { status: 404 });
  // La couche de l'éditeur : toujours celle de Claw Web, la plus récente (« Enregistrer » la
  // met ensuite à jour dans la branche).
  if (/(^|\/)claw-design\.js$/.test(path)) return fetch(new URL("claw-design.js", self.registration.scope), { cache: "no-store" });
  if (!previewToken) previewToken = await askToken();
  const encoded = path.split("/").map(encodeURIComponent).join("/");
  const response = await fetch(`https://api.github.com/repos/${owner}/${repo}/contents/${encoded}?ref=${ref}`, {
    cache: "no-store",
    headers: { Authorization: `Bearer ${previewToken}`, Accept: "application/vnd.github.raw", "X-GitHub-Api-Version": "2022-11-28" },
  });
  if (!response.ok) return new Response(`Introuvable dans la branche : ${path}`, { status: 404, headers: { "Content-Type": "text/plain; charset=utf-8" } });
  const type = TYPES[path.split(".").pop().toLowerCase()] || "application/octet-stream";
  return new Response(await response.arrayBuffer(), { status: 200, headers: { "Content-Type": type, "Cache-Control": "no-store" } });
}

self.addEventListener("fetch", (event) => {
  const request = event.request;
  if (request.method !== "GET" || new URL(request.url).origin !== self.location.origin) return;
  const url = new URL(request.url);
  if (url.pathname.startsWith(`${new URL(self.registration.scope).pathname}apercu/`)) {
    event.respondWith(preview(url));
    return;
  }
  event.respondWith(
    fetch(request)
      .then((response) => {
        if (response.ok) {
          const copy = response.clone();
          caches.open(CACHE).then((cache) => cache.put(request, copy));
        }
        return response;
      })
      .catch(() => caches.match(request).then((cached) => cached || caches.match("index.html"))),
  );
});

// Notifications du moteur (push.py) : une question de Claw, un travail fini ou un arrêt. Toucher
// la notification ouvre sa session, dans l'appli déjà ouverte s'il y en a une.
self.addEventListener("push", (event) => {
  let data = {};
  try {
    data = event.data ? event.data.json() : {};
  } catch {
    data = { body: event.data ? event.data.text() : "" };
  }
  const url = data.session ? `./#/session/${data.session}` : "./";
  event.waitUntil(self.registration.showNotification(data.title || "Claw", {
    body: data.body || "",
    tag: data.tag || "claw",
    renotify: true,
    icon: "icons/icon-192.png",
    badge: "icons/icon-192.png",
    data: { url },
  }));
});

self.addEventListener("notificationclick", (event) => {
  event.notification.close();
  const target = new URL((event.notification.data && event.notification.data.url) || "./", self.registration.scope).href;
  event.waitUntil((async () => {
    const windows = await self.clients.matchAll({ type: "window", includeUncontrolled: true });
    const open = windows.find((client) => client.url.startsWith(self.registration.scope));
    if (open) {
      await open.focus();
      open.postMessage({ open: target });
      return;
    }
    await self.clients.openWindow(target);
  })());
});
