// Messages courts annoncés aux lecteurs d'écran : window.toast("Adresse copiée")
// ou window.toast("Envoi impossible", { tone: "error" }). Une action en plus :
// window.toast("Opération supprimée", { action: { label: "Annuler", run } }).
// Les autres composants s'en servent quand il est là.
(() => {
  if (typeof window.toast === "function") return;
  const icons = {
    success: `{{icon:circle-check}}`,
    error: `{{icon:circle-alert}}`,
    info: `{{icon:info}}`,
  };
  const region = () => {
    let found = document.querySelector("[data-toast-region]");
    if (!found) {
      found = document.createElement("div");
      found.className = "toast-region";
      found.setAttribute("aria-live", "polite");
      found.dataset.toastRegion = "";
      document.body.append(found);
    }
    return found;
  };
  window.toast = (message, options = {}) => {
    const tone = options.tone || "success";
    const item = document.createElement("div");
    item.className = "toast";
    item.dataset.tone = tone;
    item.setAttribute("role", tone === "error" ? "alert" : "status");
    item.innerHTML = `<span class="toast__icon">${icons[tone] || icons.info}</span><span class="toast__text"></span><button class="toast__close" type="button" aria-label="Fermer">{{icon:x}}</button>`;
    item.querySelector(".toast__text").textContent = message;
    const remove = () => {
      if (!item.isConnected) return;
      item.classList.add("is-leaving");
      setTimeout(() => item.remove(), 220);
    };
    if (options.action && typeof options.action.run === "function") {
      const action = document.createElement("button");
      action.className = "toast__action";
      action.type = "button";
      action.textContent = options.action.label || "Annuler";
      action.addEventListener("click", () => { options.action.run(); remove(); });
      item.querySelector(".toast__close").before(action);
    }
    item.querySelector(".toast__close").addEventListener("click", remove);
    region().append(item);
    // Le temps de lire et de toucher l'action.
    setTimeout(remove, options.duration || (options.action ? 8000 : 4500));
    return item;
  };
})();
