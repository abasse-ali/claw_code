// Panier ou devis : ajout, quantités, suppression, total en direct, gardé sur
// l'appareil. « Commander » envoie la commande à data-endpoint s'il est donné,
// sinon prépare un e-mail récapitulatif vers data-mailto. Le panier s'ouvre
// avec le composant dialog (data-dialog-open="cart").
(() => {
  const root = document.querySelector("[data-cart]");
  if (!root || root.dataset.ready) return;
  root.dataset.ready = "1";
  const lang = document.documentElement.lang || "fr";
  const money = new Intl.NumberFormat(lang, { style: "currency", currency: root.dataset.currency || "EUR" });
  const storeKey = "cart";
  let lines = [];
  try { lines = JSON.parse(localStorage.getItem(storeKey) || "[]"); } catch (_) { lines = []; }
  const list = root.querySelector("[data-cart-lines]");
  const empty = root.querySelector("[data-cart-empty]");
  const total = root.querySelector("[data-cart-total]");
  const save = () => { try { localStorage.setItem(storeKey, JSON.stringify(lines)); } catch (_) { /* stockage plein */ } };
  const render = () => {
    list.replaceChildren();
    for (const line of lines) {
      const item = document.createElement("li");
      item.className = "cart__line";
      item.innerHTML = `<span><strong></strong><br><small></small></span>
        <span class="cart__qty"><button type="button" data-step="-1" aria-label="Retirer un">{{icon:minus}}</button><output></output><button type="button" data-step="1" aria-label="Ajouter un">{{icon:plus}}</button></span>
        <button class="cart__remove" type="button" aria-label="Supprimer">{{icon:trash-2}}</button>`;
      item.querySelector("strong").textContent = line.name;
      item.querySelector("small").textContent = money.format(line.price);
      item.querySelector("output").textContent = line.qty;
      item.querySelectorAll("[data-step]").forEach((button) => button.addEventListener("click", () => {
        line.qty += Number(button.dataset.step);
        if (line.qty <= 0) lines = lines.filter((other) => other !== line);
        save(); render();
      }));
      item.querySelector(".cart__remove").addEventListener("click", () => { lines = lines.filter((other) => other !== line); save(); render(); });
      list.append(item);
    }
    const count = lines.reduce((sum, line) => sum + line.qty, 0);
    empty.hidden = count > 0;
    total.textContent = money.format(lines.reduce((sum, line) => sum + line.qty * line.price, 0));
    for (const badge of document.querySelectorAll("[data-cart-count]")) { badge.textContent = count; badge.hidden = count === 0; }
  };
  document.addEventListener("click", (event) => {
    const add = event.target.closest("[data-cart-add]");
    if (!add) return;
    const found = lines.find((line) => line.id === add.dataset.id);
    if (found) found.qty += 1;
    else lines.push({ id: add.dataset.id, name: add.dataset.name, price: Number(add.dataset.price), qty: 1 });
    save(); render();
    add.classList.remove("is-added");
    void add.offsetWidth;
    add.classList.add("is-added");
    if (typeof window.toast === "function") window.toast(`${add.dataset.name} ajouté au panier`);
  });
  root.querySelector("[data-cart-clear]").addEventListener("click", () => { lines = []; save(); render(); });
  root.querySelector("[data-cart-order]").addEventListener("click", async () => {
    if (!lines.length) { if (typeof window.toast === "function") window.toast("Le panier est vide.", { tone: "info" }); return; }
    const summary = lines.map((line) => `${line.qty} x ${line.name} (${money.format(line.price)})`).join("\n");
    const sum = total.textContent;
    try {
      if (root.dataset.endpoint) {
        const response = await fetch(root.dataset.endpoint, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ lines, total: sum }) });
        if (!response.ok) throw new Error(String(response.status));
        if (typeof window.toast === "function") window.toast("Commande envoyée, merci !");
        lines = []; save(); render();
      } else {
        window.location.href = `mailto:${root.dataset.mailto || ""}?subject=${encodeURIComponent("Commande")}&body=${encodeURIComponent(`${summary}\n\nTotal : ${sum}`)}`;
      }
    } catch (_) {
      if (typeof window.toast === "function") window.toast("La commande n'a pas pu partir, réessayez.", { tone: "error" });
    }
  });
  render();
})();
