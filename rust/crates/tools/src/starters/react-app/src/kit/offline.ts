// Appli installée sur l'écran d'accueil : elle s'ouvre aussi sans connexion (public/sw.js).

export function registerOffline(): void {
  if (!import.meta.env.PROD || !("serviceWorker" in navigator) || !location.protocol.startsWith("http")) return;
  const register = () => navigator.serviceWorker.register("./sw.js").catch(() => {});
  if (document.readyState === "complete") register();
  else window.addEventListener("load", register);
}
