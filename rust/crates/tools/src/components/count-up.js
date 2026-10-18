// Chiffres qui montent jusqu'à leur valeur quand on les atteint, une fois.
// Le nombre final est déjà écrit dans la page : sans script, rien ne manque.
(() => {
  const lang = document.documentElement.lang || "fr";
  const format = new Intl.NumberFormat(lang);
  const items = [...document.querySelectorAll("[data-count-to]")].filter((item) => !item.dataset.ready);
  const still = matchMedia("(prefers-reduced-motion: reduce)").matches || !("IntersectionObserver" in window);
  const show = (item, value) => { item.textContent = `${item.dataset.countPrefix || ""}${format.format(value)}${item.dataset.countSuffix || ""}`; };
  const run = (item) => {
    const target = Number(item.dataset.countTo);
    const duration = Number(item.dataset.countDuration || 1400);
    const start = performance.now();
    const step = (now) => {
      const progress = Math.min(1, (now - start) / duration);
      show(item, Math.round(target * (1 - (1 - progress) ** 3)));
      if (progress < 1) requestAnimationFrame(step);
    };
    requestAnimationFrame(step);
  };
  items.forEach((item) => { item.dataset.ready = "1"; });
  if (still) { items.forEach((item) => show(item, Number(item.dataset.countTo))); return; }
  const observer = new IntersectionObserver((entries) => {
    for (const entry of entries) {
      if (!entry.isIntersecting) continue;
      observer.unobserve(entry.target);
      run(entry.target);
    }
  }, { threshold: 0.6 });
  items.forEach((item) => { show(item, 0); observer.observe(item); });
})();
