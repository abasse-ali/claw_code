// Formulaire soigné : les erreurs n'apparaissent qu'après la saisie d'un champ
// (ou à l'envoi), le brouillon est gardé sur l'appareil, le bouton montre
// l'envoi en cours, puis un message de réussite. Envoi : data-endpoint
// (un service de formulaires) s'il est donné, sinon un e-mail prérempli vers
// data-mailto : sans serveur, le message part quand même.
(() => {
  for (const form of document.querySelectorAll("[data-smart-form]")) {
    if (form.dataset.ready) continue;
    form.dataset.ready = "1";
    const fields = [...form.querySelectorAll("input, textarea, select")].filter((field) => field.name);
    const submit = form.querySelector('[type="submit"]');
    const success = form.querySelector("[data-form-success]");
    const draftNote = form.querySelector("[data-form-draft]");
    const storeKey = `draft:${location.pathname}:${form.id || "form"}`;
    const check = (field) => {
      const box = field.closest(".smart-form__field");
      if (box) box.classList.toggle("is-invalid", !field.checkValidity());
      return field.checkValidity();
    };
    try {
      const saved = JSON.parse(localStorage.getItem(storeKey) || "{}");
      for (const field of fields) if (saved[field.name]) field.value = saved[field.name];
    } catch (_) { /* rien de gardé */ }
    for (const field of fields) {
      field.addEventListener("blur", () => { if (field.value) check(field); });
      field.addEventListener("input", () => {
        if (field.closest(".is-invalid")) check(field);
        const draft = Object.fromEntries(fields.map((item) => [item.name, item.value]));
        try { localStorage.setItem(storeKey, JSON.stringify(draft)); } catch (_) { /* stockage plein */ }
        if (draftNote) draftNote.hidden = false;
      });
    }
    form.addEventListener("submit", async (event) => {
      event.preventDefault();
      const invalid = fields.filter((field) => !check(field));
      if (invalid.length) { invalid[0].focus(); return; }
      const data = Object.fromEntries(fields.map((field) => [field.name, field.value.trim()]));
      submit.setAttribute("aria-busy", "true");
      submit.disabled = true;
      try {
        if (form.dataset.endpoint) {
          const response = await fetch(form.dataset.endpoint, { method: "POST", headers: { "Content-Type": "application/json", Accept: "application/json" }, body: JSON.stringify(data) });
          if (!response.ok) throw new Error(String(response.status));
        } else if (form.dataset.mailto) {
          const body = Object.entries(data).filter(([, value]) => value).map(([name, value]) => `${name} : ${value}`).join("\n");
          window.location.href = `mailto:${form.dataset.mailto}?subject=${encodeURIComponent(form.dataset.subject || "Message du site")}&body=${encodeURIComponent(body)}`;
        }
        try { localStorage.removeItem(storeKey); } catch (_) { /* rien */ }
        form.reset();
        if (draftNote) draftNote.hidden = true;
        if (success) { success.hidden = false; success.focus(); }
        if (typeof window.toast === "function") window.toast("Message envoyé");
      } catch (_) {
        if (typeof window.toast === "function") window.toast("L'envoi a échoué, réessayez dans un instant.", { tone: "error" });
      } finally {
        submit.removeAttribute("aria-busy");
        submit.disabled = false;
      }
    });
  }
})();
