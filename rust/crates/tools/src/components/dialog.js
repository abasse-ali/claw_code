// Fenêtre modale native (<dialog>) : un bouton data-dialog-open="id" l'ouvre ;
// Échap, un bouton data-dialog-close ou un clic sur le fond la ferment, et le
// focus revient au bouton qui l'avait ouverte.
(() => {
  if (document.documentElement.dataset.dialogsReady) return;
  document.documentElement.dataset.dialogsReady = "1";
  let opener = null;
  // Le clic d'un glissement parti du contenu et fini sur le fond va au <dialog> :
  // le fond ne ferme que si l'appui y a commencé (sinon, faire défiler un
  // formulaire du doigt fermait la fenêtre).
  let pressed = null;
  document.addEventListener("pointerdown", (event) => { pressed = event.target; }, true);
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
    if (event.target instanceof HTMLDialogElement && event.target.open && pressed === event.target) {
      event.target.close();
    }
  });
  document.addEventListener("close", (event) => {
    if (event.target instanceof HTMLDialogElement && opener) {
      opener.focus();
      opener = null;
    }
  }, true);
})();
