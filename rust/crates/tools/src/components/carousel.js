// Carrousel au défilement natif (scroll-snap) : flèches, points et touches du
// clavier ; au doigt, la piste glisse d'elle-même.
(() => {
  for (const root of document.querySelectorAll("[data-carousel]")) {
    if (root.dataset.ready) continue;
    root.dataset.ready = "1";
    const track = root.querySelector("[data-carousel-track]");
    const slides = [...track.children];
    const prev = root.querySelector("[data-carousel-prev]");
    const next = root.querySelector("[data-carousel-next]");
    const dots = root.querySelector("[data-carousel-dots]");
    const step = () => (slides[1] ? slides[1].offsetLeft - slides[0].offsetLeft : track.clientWidth);
    const current = () => Math.round(track.scrollLeft / Math.max(1, step()));
    const go = (index) => track.scrollTo({ left: Math.max(0, Math.min(index, slides.length - 1)) * step() });
    const buttons = slides.map((slide, index) => {
      const dot = document.createElement("button");
      dot.type = "button";
      dot.className = "carousel__dot";
      dot.setAttribute("aria-label", `Aller à ${index + 1} sur ${slides.length}`);
      dot.addEventListener("click", () => go(index));
      if (dots) dots.append(dot);
      return dot;
    });
    const update = () => {
      const index = current();
      buttons.forEach((dot, at) => dot.setAttribute("aria-current", String(at === index)));
      const end = track.scrollLeft + track.clientWidth >= track.scrollWidth - 4;
      if (prev) prev.disabled = track.scrollLeft <= 4;
      if (next) next.disabled = end;
      // Toutes les images tiennent à l'écran (trois colonnes à l'ordinateur) :
      // les points n'auraient rien à faire défiler, ils se cachent.
      if (dots) dots.hidden = track.scrollWidth <= track.clientWidth + 4;
    };
    if (prev) prev.addEventListener("click", () => go(current() - 1));
    if (next) next.addEventListener("click", () => go(current() + 1));
    track.addEventListener("keydown", (event) => {
      if (event.key === "ArrowRight") { event.preventDefault(); go(current() + 1); }
      if (event.key === "ArrowLeft") { event.preventDefault(); go(current() - 1); }
    });
    let frame = 0;
    track.addEventListener("scroll", () => { cancelAnimationFrame(frame); frame = requestAnimationFrame(update); }, { passive: true });
    window.addEventListener("resize", update);
    update();
  }
})();
