// Service worker de l'appli : l'interface reste disponible hors connexion (les
// requêtes à GitHub et OpenRouter, elles, ne passent jamais par le cache).
// Réseau d'abord : une nouvelle version publiée est prise dès qu'elle est là.

const CACHE = "claw-web-v77";
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
  "vendor/nacl-fast.min.js",
  "manifest.webmanifest",
  "icons/icon.svg",
  "icons/logo.svg",
  "fonts/bricolage-grotesque-latin.woff2",
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

self.addEventListener("fetch", (event) => {
  const request = event.request;
  if (request.method !== "GET" || new URL(request.url).origin !== self.location.origin) return;
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
