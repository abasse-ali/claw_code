// Mode sombre : suit le réglage de l'appareil, puis le choix du visiteur,
// gardé d'une visite à l'autre. Placer ce script dans le <head> évite un
// éclair de la mauvaise couleur au chargement.
(() => {
  const root = document.documentElement;
  const stored = (() => { try { return localStorage.getItem("theme"); } catch (_) { return null; } })();
  const prefersDark = window.matchMedia("(prefers-color-scheme: dark)").matches;
  const apply = (dark) => {
    root.dataset.theme = dark ? "dark" : "light";
    for (const button of document.querySelectorAll("[data-theme-toggle]")) button.setAttribute("aria-pressed", String(dark));
  };
  apply(stored ? stored === "dark" : prefersDark);
  const ready = () => {
    apply(root.dataset.theme === "dark");
    for (const button of document.querySelectorAll("[data-theme-toggle]")) {
      if (button.dataset.ready) continue;
      button.dataset.ready = "1";
      button.addEventListener("click", () => {
        const dark = root.dataset.theme !== "dark";
        apply(dark);
        try { localStorage.setItem("theme", dark ? "dark" : "light"); } catch (_) { /* stockage plein */ }
      });
    }
  };
  if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", ready);
  else ready();
})();
