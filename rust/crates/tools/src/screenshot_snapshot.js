// Textes durables de la page : le texte de chaque bloc visible (un paragraphe, une
// ligne de liste, un compteur), hors des messages passagers (toast, role=status) et
// des fenetres. Le parcours les prend avant et apres l'envoi d'un formulaire, puis
// apres un rechargement : si aucun des textes apparus a l'envoi (« 4 places dispo »)
// ne revient, la page a perdu ce que le visiteur avait ajoute.
(() => {
  const passing = "[role=status], [role=alert], [aria-live], .toast, .snackbar, .notification, output, dialog, [role=dialog]";
  const blocks = new Set();
  const texts = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT);
  for (let node = texts.nextNode(); node; node = texts.nextNode()) {
    let holder = node.parentElement;
    if (!node.nodeValue.trim() || !holder || holder.closest(passing) || holder.closest("script, style, noscript, template")) continue;
    // Le bloc entier, pas le seul <strong>4</strong> de « 4 places dispo ».
    // Un bouton (inline-block) est deja un bloc : on ne remonte pas jusqu'a <main>.
    while (holder.parentElement && holder.parentElement !== document.body
      && getComputedStyle(holder).display === "inline") holder = holder.parentElement;
    const style = getComputedStyle(holder);
    if (style.display === "none" || style.visibility === "hidden" || holder.querySelector(passing)) continue;
    const text = (holder.innerText || "").replace(/\s+/g, " ").trim();
    if (!text || text.length > 200) continue;
    // Le meme texte sur deux cartes (« Il reste 4 places ») : la carte le
    // situe. Vu le 25 octobre 2026 (session #31) : une carte passait de 5 a
    // 4 places, une autre affichait deja « Il reste 4 places », et l'ajout
    // passait inapercu.
    const card = holder.parentElement && holder.parentElement.closest("li, article, tr, [role=listitem], [class*=card], [class*=item]");
    const head = card && card !== holder && !card.contains(document.querySelector("main"))
      ? (card.innerText || "").replace(/\s+/g, " ").trim().slice(0, 50) : "";
    blocks.add(head && head !== text && !head.startsWith(text) ? `${text} (in "${head}")` : text);
  }
  return [...blocks];
})()
