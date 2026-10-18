// Menu du téléphone : le bouton l'ouvre et le ferme (aria-expanded), un lien
// choisi, Échap ou un clic ailleurs le referment ; l'en-tête prend une ombre
// quand la page défile, et le lien de la section visible est marqué.
(() => {
  for (const header of document.querySelectorAll("[data-nav]")) {
    if (header.dataset.ready) continue;
    header.dataset.ready = "1";
    const toggle = header.querySelector("[data-nav-toggle]");
    const nav = toggle && document.getElementById(toggle.getAttribute("aria-controls"));
    if (!toggle || !nav) continue;
    const set = (open) => {
      toggle.setAttribute("aria-expanded", String(open));
      nav.classList.toggle("is-open", open);
    };
    toggle.addEventListener("click", () => set(toggle.getAttribute("aria-expanded") !== "true"));
    nav.addEventListener("click", (event) => { if (event.target.closest("a")) set(false); });
    document.addEventListener("keydown", (event) => {
      if (event.key === "Escape" && nav.classList.contains("is-open")) { set(false); toggle.focus(); }
    });
    document.addEventListener("click", (event) => { if (!header.contains(event.target)) set(false); });
    const onScroll = () => header.toggleAttribute("data-scrolled", window.scrollY > 8);
    window.addEventListener("scroll", onScroll, { passive: true });
    onScroll();
    const links = [...nav.querySelectorAll('a[href^="#"]')].filter((link) => link.getAttribute("href").length > 1);
    const sections = links.map((link) => document.getElementById(link.getAttribute("href").slice(1))).filter(Boolean);
    if ("IntersectionObserver" in window && sections.length) {
      const observer = new IntersectionObserver((entries) => {
        for (const entry of entries) {
          if (!entry.isIntersecting) continue;
          for (const link of links) {
            if (link.getAttribute("href") === `#${entry.target.id}`) link.setAttribute("aria-current", "true");
            else link.removeAttribute("aria-current");
          }
        }
      }, { rootMargin: "-45% 0px -50% 0px" });
      sections.forEach((section) => observer.observe(section));
    }
  }
})();
