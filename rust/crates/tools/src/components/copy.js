// Copier une adresse, un code, un numéro : data-copy="texte" ou
// data-copy-target="#id" ; le bouton confirme, et un message l'annonce.
(() => {
  if (document.documentElement.dataset.copyReady) return;
  document.documentElement.dataset.copyReady = "1";
  const fallback = (text) => {
    const area = document.createElement("textarea");
    area.value = text;
    area.setAttribute("readonly", "");
    area.style.position = "fixed";
    area.style.opacity = "0";
    document.body.append(area);
    area.select();
    let done = false;
    try { done = document.execCommand("copy"); } catch (_) { done = false; }
    area.remove();
    return done;
  };
  document.addEventListener("click", async (event) => {
    const button = event.target.closest("[data-copy], [data-copy-target]");
    if (!button) return;
    const target = button.dataset.copyTarget && document.querySelector(button.dataset.copyTarget);
    const text = button.dataset.copy || (target ? target.textContent.trim() : "");
    if (!text) return;
    let done = false;
    try { await navigator.clipboard.writeText(text); done = true; } catch (_) { done = fallback(text); }
    const label = button.querySelector("span");
    const before = label ? label.textContent : "";
    button.classList.toggle("is-done", done);
    if (label) label.textContent = done ? "Copié" : "Sélectionnez le texte";
    setTimeout(() => { button.classList.remove("is-done"); if (label) label.textContent = before; }, 2000);
    if (typeof window.toast === "function") window.toast(done ? (button.dataset.copyDone || "Copié") : "La copie a échoué", { tone: done ? "success" : "error" });
  });
})();
