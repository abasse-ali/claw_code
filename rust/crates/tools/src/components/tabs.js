// Onglets accessibles : clic, flèches gauche et droite, Début et Fin.
(() => {
  for (const root of document.querySelectorAll("[data-tabs]")) {
    if (root.dataset.ready) continue;
    root.dataset.ready = "1";
    const tabs = [...root.querySelectorAll('[role="tab"]')];
    const select = (tab, focus) => {
      for (const other of tabs) {
        const selected = other === tab;
        other.setAttribute("aria-selected", String(selected));
        other.tabIndex = selected ? 0 : -1;
        const panel = document.getElementById(other.getAttribute("aria-controls"));
        if (panel) panel.hidden = !selected;
      }
      if (focus) tab.focus();
    };
    tabs.forEach((tab, index) => {
      tab.addEventListener("click", () => select(tab, false));
      tab.addEventListener("keydown", (event) => {
        const last = tabs.length - 1;
        const next = { ArrowRight: index === last ? 0 : index + 1, ArrowLeft: index === 0 ? last : index - 1, Home: 0, End: last }[event.key];
        if (next === undefined) return;
        event.preventDefault();
        select(tabs[next], true);
      });
    });
  }
})();
