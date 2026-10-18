// Filtre instantané : étiquettes (data-tags) et recherche dans le texte, le
// nombre de résultats annoncé, l'état gardé dans l'adresse (?filtre=&q=) pour
// un lien partagé ou un retour arrière, et une transition de vue si le
// navigateur la connaît.
(() => {
  const fold = (text) => text.normalize("NFD").replace(/[̀-ͯ]/g, "").toLowerCase();
  for (const root of document.querySelectorAll("[data-filter]")) {
    if (root.dataset.ready) continue;
    root.dataset.ready = "1";
    const chips = [...root.querySelectorAll("[data-filter-tag]")];
    const search = root.querySelector("[data-filter-search]");
    const items = [...root.querySelectorAll("[data-filter-item]")];
    const count = root.querySelector("[data-filter-count]");
    const empty = root.querySelector("[data-filter-empty]");
    const params = new URLSearchParams(location.search);
    let tag = params.get("filtre") || "";
    if (search) search.value = params.get("q") || "";
    const apply = () => {
      const words = fold(search ? search.value.trim() : "").split(/\s+/).filter(Boolean);
      let shown = 0;
      for (const item of items) {
        const tags = (item.dataset.tags || "").split(/\s+/);
        const text = fold(item.textContent);
        const visible = (!tag || tags.includes(tag)) && words.every((word) => text.includes(word));
        item.hidden = !visible;
        if (visible) shown += 1;
      }
      for (const chip of chips) chip.setAttribute("aria-pressed", String(chip.dataset.filterTag === tag));
      if (count) count.textContent = `${shown} résultat${shown > 1 ? "s" : ""}`;
      if (empty) empty.hidden = shown > 0;
      const url = new URL(location.href);
      if (tag) url.searchParams.set("filtre", tag); else url.searchParams.delete("filtre");
      if (search && search.value.trim()) url.searchParams.set("q", search.value.trim()); else url.searchParams.delete("q");
      history.replaceState(null, "", url);
    };
    const update = () => (document.startViewTransition && !matchMedia("(prefers-reduced-motion: reduce)").matches ? document.startViewTransition(apply) : apply());
    for (const chip of chips) chip.addEventListener("click", () => { tag = chip.dataset.filterTag; update(); });
    if (search) search.addEventListener("input", apply);
    const reset = root.querySelector("[data-filter-reset]");
    if (reset) reset.addEventListener("click", () => { tag = ""; if (search) search.value = ""; update(); });
    apply();
  }
})();
