// Écrans d'une appli d'une seule page : la barre d'onglets (en bas au
// téléphone) montre une section [data-screen] à la fois. L'écran ouvert suit
// l'adresse (#bilan) : il reste ouvert après un rechargement, et le bouton
// Retour revient au précédent. Chaque changement envoie l'événement
// « screenchange » : un graphique s'y redessine à la taille de son écran.
(() => {
  const nav = document.querySelector("[data-app-tabs]");
  if (!nav || nav.dataset.ready) return;
  nav.dataset.ready = "1";
  const screens = [...document.querySelectorAll("[data-screen]")];
  if (!screens.length) return;
  const show = (id, moved) => {
    const screen = screens.find((item) => item.id === id);
    // Une ancre qui n'est pas un écran (#contenu) ne change pas d'écran.
    if (!screen && id && moved) return;
    const shown = screen || screens[0];
    for (const item of screens) item.hidden = item !== shown;
    for (const tab of nav.querySelectorAll("a[href^='#']")) {
      if (tab.getAttribute("href") === `#${shown.id}`) tab.setAttribute("aria-current", "page");
      else tab.removeAttribute("aria-current");
    }
    if (moved) {
      window.scrollTo({ top: 0 });
      // Le lecteur d'écran annonce le titre de l'écran ouvert.
      const title = shown.querySelector("h1, h2");
      if (title) {
        title.tabIndex = -1;
        title.focus({ preventScroll: true });
      }
    }
    document.dispatchEvent(new CustomEvent("screenchange", { detail: { screen: shown.id } }));
  };
  window.addEventListener("hashchange", () => show(decodeURIComponent(location.hash.slice(1)), true));
  show(decodeURIComponent(location.hash.slice(1)), false);
})();
