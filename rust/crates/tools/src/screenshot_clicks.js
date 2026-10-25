// Essai des commandes : Claw clique chaque bouton de la page et regarde s'il
// se passe quelque chose (la page change, elle defile, un message, une
// fenetre, le presse-papiers, un envoi, un telechargement). Un bouton qui ne
// fait rien est le defaut le plus frequent d'un site genere ; une erreur au
// clic aussi. Lance apres la capture : il change la page.
(async () => {
  const wait = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const shown = (el) => {
    const style = getComputedStyle(el);
    if (style.display === "none" || style.visibility === "hidden") return false;
    const box = el.getBoundingClientRect();
    return box.width > 0 && box.height > 0;
  };
  const label = (el) => {
    let text = (el.innerText || el.value || el.getAttribute("aria-label") || el.getAttribute("title") || "")
      .replace(/\s+/g, " ").trim();
    if (text.length > 40) text = `${text.slice(0, 39)}...`;
    let tag = el.tagName.toLowerCase();
    if (el.id) tag += `#${el.id}`;
    else if (typeof el.className === "string" && el.className.trim()) tag += `.${el.className.trim().split(/\s+/)[0]}`;
    return text ? `${tag} "${text}"` : tag;
  };

  // Ce qu'un clic peut faire sans changer la page compte comme une action ;
  // rien ne doit quitter la page ni bloquer l'essai.
  let effects = 0;
  const count = () => { effects += 1; };
  window.alert = count;
  window.confirm = () => { count(); return false; };
  window.prompt = () => { count(); return null; };
  window.open = () => { count(); return null; };
  try {
    if (navigator.clipboard) navigator.clipboard.writeText = async () => { count(); };
  } catch (_) { /* presse-papiers absent */ }
  try { navigator.share = async () => { count(); }; } catch (_) { /* partage absent */ }
  try { navigator.canShare = () => true; } catch (_) { /* partage absent */ }
  HTMLAnchorElement.prototype.click = function click() { count(); };
  try { HTMLFormElement.prototype.submit = function submit() { count(); }; } catch (_) { /* rien */ }
  document.addEventListener("submit", (event) => { count(); event.preventDefault(); }, true);
  if (window.navigation) {
    window.navigation.addEventListener("navigate", (event) => {
      count();
      if (event.cancelable) event.preventDefault();
    });
  }
  let scrolled = 0;
  window.addEventListener("scroll", () => { scrolled += 1; }, true);
  const errors = [];
  window.addEventListener("error", (event) => { errors.push(event.message || "error"); });
  window.addEventListener("unhandledrejection", (event) => {
    const reason = event.reason;
    errors.push(String((reason && reason.message) || reason));
  });

  const controls = [...document.querySelectorAll("button, [role=button], input[type=button], summary")]
    .filter((el) => shown(el) && !el.disabled && el.getAttribute("aria-disabled") !== "true")
    // L'envoi d'un formulaire s'essaie avec les actions de Screenshot.
    .filter((el) => !(el.type === "submit" && el.form))
    // L'onglet ou l'etape deja choisis ne changent rien quand on les clique.
    .filter((el) => el.getAttribute("aria-selected") !== "true"
      && !(el.hasAttribute("aria-current") && el.getAttribute("aria-current") !== "false"))
    .slice(0, 24);
  const dead = [];
  const failing = [];
  // Fenêtres ouvertes par un bouton dont les commandes ne se touchent pas : un
  // fond (backdrop) posé par-dessus prend le doigt. Vu le 25 octobre 2026
  // (session #31) : « Je conduis » ouvrait un formulaire sous un
  // <form class="modal-backdrop"> ; toucher un champ fermait la fenêtre.
  // `el.click()` passe outre : seul le point touché le dit.
  const covered = [];
  const coveredIn = (opener) => {
    for (const box of document.querySelectorAll("dialog[open], [role=dialog], [aria-modal=true]")) {
      if (!shown(box)) continue;
      const blocked = [...box.querySelectorAll("button, input:not([type=hidden]), select, textarea, a[href]")]
        .filter(shown)
        .slice(0, 12)
        .map((control) => {
          const rect = control.getBoundingClientRect();
          const x = rect.left + rect.width / 2;
          const y = rect.top + rect.height / 2;
          if (x < 0 || y < 0 || x > innerWidth || y > innerHeight) return null;
          const top = document.elementFromPoint(x, y);
          if (!top || top === control || control.contains(top) || (top.control && top.control === control)) return null;
          return { control, top };
        })
        .filter(Boolean);
      if (blocked.length) {
        covered.push(`${label(opener)} opens a window whose ${blocked.slice(0, 3).map((item) => label(item.control)).join(", ")} cannot be tapped: ${label(blocked[0].top)} lies over them`);
        return;
      }
    }
  };
  for (const el of controls) {
    if (!el.isConnected || !shown(el)) continue;
    let mutations = 0;
    const observer = new MutationObserver((records) => { mutations += records.length; });
    observer.observe(document.documentElement, { subtree: true, childList: true, attributes: true, characterData: true });
    const before = { url: location.href, x: scrollX, y: scrollY, effects, scrolled, errors: errors.length, focus: document.activeElement };
    try { el.click(); } catch (error) { errors.push(String((error && error.message) || error)); }
    await wait(300);
    observer.disconnect();
    if (errors.length > before.errors) failing.push(`${label(el)}: ${errors[errors.length - 1]}`);
    const moved = location.href !== before.url || Math.abs(scrollX - before.x) > 2 || Math.abs(scrollY - before.y) > 2
      || scrolled > before.scrolled;
    const focus = document.activeElement;
    const focused = focus !== before.focus && focus !== el && focus !== document.body;
    if (!mutations && !moved && !focused && effects === before.effects && errors.length === before.errors) {
      dead.push(label(el));
    }
    if (covered.length < 4) coveredIn(el);
    // Ce qui s'est ouvert se referme, pour que le bouton suivant soit libre.
    document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    for (const dialog of document.querySelectorAll("dialog[open]")) {
      try { dialog.close(); } catch (_) { /* deja fermee */ }
    }
    await wait(40);
  }
  return { checked: controls.length, dead, failing, covered };
})()
