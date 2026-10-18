// Partager la page : la feuille de partage du téléphone quand elle existe,
// sinon le lien copié, annoncé par un message.
(() => {
  if (document.documentElement.dataset.shareReady) return;
  document.documentElement.dataset.shareReady = "1";
  document.addEventListener("click", async (event) => {
    const button = event.target.closest("[data-share]");
    if (!button) return;
    const data = { title: button.dataset.shareTitle || document.title, url: button.dataset.shareUrl || location.href };
    if (navigator.share) {
      try { await navigator.share(data); } catch (_) { /* partage annulé */ }
      return;
    }
    try {
      await navigator.clipboard.writeText(data.url);
      if (typeof window.toast === "function") window.toast("Lien copié, à coller où vous voulez");
    } catch (_) {
      if (typeof window.toast === "function") window.toast(data.url, { tone: "info", duration: 8000 });
    }
  });
})();
