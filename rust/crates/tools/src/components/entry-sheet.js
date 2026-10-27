// Volet « Nouvelle opération » : vérifie le montant (« 12,50 » comme
// « 12.50 ») et le libellé, ajoute l'opération à window.entries, ferme le
// volet et le confirme. La date est celle du jour à chaque ouverture.
(() => {
  for (const form of document.querySelectorAll("[data-entry-form]")) {
    if (form.dataset.ready) continue;
    form.dataset.ready = "1";
    const dialog = form.closest("dialog");
    const date = form.elements.date;
    const pad = (number) => String(number).padStart(2, "0");
    const today = () => { const now = new Date(); return `${now.getFullYear()}-${pad(now.getMonth() + 1)}-${pad(now.getDate())}`; };
    const parse = (text) => (window.entries ? window.entries.parseAmount(text) : Number(String(text).replace(/\s/g, "").replace(",", ".")));
    const fresh = () => {
      form.reset();
      if (date) date.value = today();
      for (const field of form.querySelectorAll(".is-invalid")) field.classList.remove("is-invalid");
    };
    fresh();
    if (dialog) {
      // Le volet s'ouvre neuf, le curseur dans le montant.
      new MutationObserver(() => {
        if (!dialog.open) return;
        fresh();
        setTimeout(() => form.elements.amount && form.elements.amount.focus(), 50);
      }).observe(dialog, { attributes: true, attributeFilter: ["open"] });
    }
    const mark = (input, bad) => {
      const field = input.closest(".sheet__field");
      if (field) field.classList.toggle("is-invalid", bad);
      input.setAttribute("aria-invalid", String(bad));
      const error = form.querySelector(`[data-error-for="${input.id}"]`);
      if (error) { error.id = error.id || `${input.id}-erreur`; input.setAttribute("aria-describedby", error.id); }
      return bad;
    };
    form.addEventListener("input", (event) => {
      const input = event.target;
      if (input.closest(".is-invalid")) mark(input, input.name === "amount" ? !(parse(input.value) > 0) : !input.value.trim());
    });
    form.addEventListener("submit", (event) => {
      event.preventDefault();
      const amount = parse(form.elements.amount.value);
      const badAmount = mark(form.elements.amount, !(amount > 0));
      const badLabel = mark(form.elements.label, !form.elements.label.value.trim());
      if (badAmount || badLabel) {
        (badAmount ? form.elements.amount : form.elements.label).focus();
        return;
      }
      const income = form.elements.kind && form.elements.kind.value === "income";
      const entry = {
        label: form.elements.label.value,
        amount: income ? amount : -amount,
        category: form.elements.category ? form.elements.category.value : "",
        date: date && date.value ? date.value : today(),
      };
      if (window.entries) window.entries.add(entry);
      if (dialog) dialog.close();
      if (typeof window.toast === "function") {
        const shown = window.entries ? window.entries.format(amount) : `${amount}`;
        window.toast(`${income ? "Rentrée" : "Dépense"} de ${shown} enregistrée`);
      }
    });
  }
})();
