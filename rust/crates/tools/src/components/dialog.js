// Fenêtre modale native (<dialog>) : un bouton data-dialog-open="id" l'ouvre ;
// Échap, un bouton data-dialog-close ou un clic sur le fond la ferment, et le
// focus revient au bouton qui l'avait ouverte.
(() => {
  if (document.documentElement.dataset.dialogsReady) return;
  document.documentElement.dataset.dialogsReady = "1";
  let opener = null;
  document.addEventListener("click", (event) => {
    const open = event.target.closest("[data-dialog-open]");
    if (open) {
      const dialog = document.getElementById(open.dataset.dialogOpen);
      if (dialog && typeof dialog.showModal === "function" && !dialog.open) {
        opener = open;
        dialog.showModal();
      }
      return;
    }
    const close = event.target.closest("[data-dialog-close]");
    if (close) {
      const dialog = close.closest("dialog");
      if (dialog) dialog.close();
      return;
    }
    // Un clic sur le fond tombe sur le <dialog> lui-même, pas sur son contenu.
    if (event.target instanceof HTMLDialogElement && event.target.open) event.target.close();
  });
  document.addEventListener("close", (event) => {
    if (event.target instanceof HTMLDialogElement && opener) {
      opener.focus();
      opener = null;
    }
  }, true);
})();
