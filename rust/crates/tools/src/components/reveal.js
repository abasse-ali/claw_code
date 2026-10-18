// Apparition au défilement, une seule fois par bloc ; dans une liste marquée
// data-reveal-stagger, chaque élément suit le précédent de 80 ms. À réserver
// à quelques blocs choisis : un fondu sur chaque section fait gabarit.
(() => {
  const root = document.documentElement;
  root.classList.add("js");
  for (const list of document.querySelectorAll("[data-reveal-stagger]")) {
    [...list.querySelectorAll(".reveal")].forEach((item, index) => item.style.setProperty("--reveal-delay", `${Math.min(index, 6) * 80}ms`));
  }
  const blocks = document.querySelectorAll(".reveal:not(.is-visible)");
  if (!("IntersectionObserver" in window) || matchMedia("(prefers-reduced-motion: reduce)").matches) {
    blocks.forEach((block) => block.classList.add("is-visible"));
    return;
  }
  const observer = new IntersectionObserver((entries) => {
    for (const entry of entries) {
      if (!entry.isIntersecting) continue;
      entry.target.classList.add("is-visible");
      observer.unobserve(entry.target);
    }
  }, { rootMargin: "0px 0px -8% 0px", threshold: 0.12 });
  blocks.forEach((block) => observer.observe(block));
})();
