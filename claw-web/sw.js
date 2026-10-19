// Service worker de l'appli : l'interface reste disponible hors connexion (les
// requêtes à GitHub et OpenRouter, elles, ne passent jamais par le cache).
// Réseau d'abord : une nouvelle version publiée est prise dès qu'elle est là.

const CACHE = "claw-web-v24";
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
  "vendor/nacl-fast.min.js",
  "manifest.webmanifest",
  "icons/icon.svg",
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
