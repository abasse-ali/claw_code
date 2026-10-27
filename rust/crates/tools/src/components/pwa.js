// Hors-ligne : le service worker garde les fichiers de l'appli. Il ne se
// déclare que servi en http(s) (une page ouverte en file:// n'en a pas).
(() => {
  if (!("serviceWorker" in navigator) || !/^https?:$/.test(location.protocol)) return;
  window.addEventListener("load", () => {
    navigator.serviceWorker.register("sw.js").catch(() => { /* l'appli marche sans */ });
  });
})();
