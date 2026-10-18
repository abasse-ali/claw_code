// Messages courts annoncés aux lecteurs d'écran : window.toast("Adresse copiée")
// ou window.toast("Envoi impossible", { tone: "error" }). Les autres composants
// s'en servent quand il est là.
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
    item.querySelector(".toast__close").addEventListener("click", remove);
    region().append(item);
    setTimeout(remove, options.duration || 4500);
    return item;
  };
})();
