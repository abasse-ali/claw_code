// Service worker : l'appli s'ouvre sans réseau. Le réseau d'abord, pour
// recevoir les nouvelles versions ; la copie gardée quand il manque. Changer
// VERSION à chaque mise en ligne efface les anciennes copies.
const VERSION = "v1";
const FILES = [
  "./", "index.html", "styles.css", "app.js", "kit/app-kit.css", "kit/app-kit.js", "manifest.webmanifest",
  "icons/icon.svg", "icons/icon-192.png", "icons/icon-512.png", "icons/apple-touch-icon.png",
];

self.addEventListener("install", (event) => {
  // Un fichier absent ne bloque pas l'installation des autres.
  event.waitUntil(caches.open(VERSION).then((cache) => Promise.all(FILES.map((file) => cache.add(file).catch(() => null)))).then(() => self.skipWaiting()));
});

self.addEventListener("activate", (event) => {
  event.waitUntil(caches.keys().then((keys) => Promise.all(keys.filter((key) => key !== VERSION).map((key) => caches.delete(key)))).then(() => self.clients.claim()));
});

self.addEventListener("fetch", (event) => {
  const { request } = event;
  if (request.method !== "GET" || new URL(request.url).origin !== self.location.origin) return;
  event.respondWith(
    fetch(request)
      .then((response) => {
        if (response.ok) {
          const copy = response.clone();
          caches.open(VERSION).then((cache) => cache.put(request, copy));
        }
        return response;
      })
      .catch(() => caches.match(request).then((found) => found || (request.mode === "navigate" ? caches.match("index.html") : Response.error())))
  );
});
