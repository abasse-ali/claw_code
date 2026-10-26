// Mesures d'une fenêtre ouverte par un bouton (dialogue, feuille du bas) :
// la capture de la page seule ne les voyait pas. Vu le 26 octobre 2026
// (session #31, sur l'iPhone du parent) : « Proposer ma voiture » faisait
// 352px sur un écran de 390px, collée à gauche (la largeur maximale qu'un
// <dialog> garde du navigateur), et restait en haut alors qu'elle est
// dessinée pour monter du bas ; le match choisi était coupé (« samedi 28
// novemb ») et « Places disponibles » touchait le champ du dessus.
(() => {
  const W = document.documentElement.clientWidth || innerWidth;
  const H = innerHeight;
  const shown = (el) => {
    const style = getComputedStyle(el);
    const rect = el.getBoundingClientRect();
    return style.display !== "none" && style.visibility !== "hidden" && rect.width > 0 && rect.height > 0;
  };
  // Une fenêtre de 0px dont le panneau est en position fixe compte aussi.
  const open = (el) => {
    const style = getComputedStyle(el);
    if (style.display === "none" || style.visibility === "hidden") return false;
    return shown(el) || [...el.querySelectorAll("*")].slice(0, 300).some(shown);
  };
  const box = [...document.querySelectorAll("dialog[open], [role=dialog], [aria-modal=true]")].filter(open).pop();
  if (!box) return [];
  const problems = [];
  // Ce qui se voit : la fenêtre, ou, si elle est transparente (un <dialog>
  // plein écran qui pose son panneau en bas) ou sans taille (un <dialog> de 0px
  // dont le panneau est en position fixe), son plus grand panneau coloré.
  const painted = (el) => {
    const color = getComputedStyle(el).backgroundColor;
    return color && !/rgba?\(0, 0, 0, 0\)|transparent/.test(color);
  };
  const panel = painted(box) && shown(box) ? box : [...box.querySelectorAll("*")].filter((el) => shown(el) && painted(el))
    .sort((a, b) => {
      const ra = a.getBoundingClientRect();
      const rb = b.getBoundingClientRect();
      return rb.width * rb.height - ra.width * ra.height;
    })[0] || box;
  const style = getComputedStyle(panel);
  const rect = panel.getBoundingClientRect();
  const dialogStyle = getComputedStyle(box);
  const px = (value) => Math.round(value);
  const fromDialog = box.tagName === "DIALOG" && shown(box) && (/^calc\(100% -/.test(dialogStyle.maxWidth) || /^calc\(100% -/.test(dialogStyle.maxHeight))
    ? " A <dialog> keeps the browser's max-width and max-height (calc(100% - 2em - 6px)): set max-width: 100% and max-height: 100% (or the window's own sizes) with margin: 0."
    : "";
  const left = rect.left;
  const right = W - rect.right;
  if (rect.width < W - 8) {
    if (left <= 2 && right > 8) {
      problems.push(`The window is ${px(rect.width)}px wide on a ${W}px-wide screen and stuck to the left edge: the page shows in a ${px(right)}px strip at its right.${fromDialog}`);
    } else if (right <= 2 && left > 8) {
      problems.push(`The window is ${px(rect.width)}px wide on a ${W}px-wide screen and stuck to the right edge: the page shows in a ${px(left)}px strip at its left.${fromDialog}`);
    } else if (Math.abs(left - right) > 8) {
      problems.push(`The window is not centered: ${px(left)}px of page at its left, ${px(right)}px at its right.${fromDialog}`);
    }
  }
  // Une feuille du bas : coins du haut arrondis, ceux du bas carrés.
  const sheet = parseFloat(style.borderTopLeftRadius) > 4 && parseFloat(style.borderBottomLeftRadius) < 1;
  if (sheet && H - rect.bottom > 4) {
    problems.push(`The window is drawn as a sheet that rises from the bottom of the screen (rounded top corners only), but it ${rect.top <= 2 ? "sits at the top" : `stops ${px(H - rect.bottom)}px above the bottom`}: the page shows under it. Pin it to the bottom (position: fixed; inset: auto 0 0 0; margin: 0; width: 100%; max-width: 100%), with padding-bottom: env(safe-area-inset-bottom).${fromDialog && !problems.length ? fromDialog : ""}`);
  }
  if (box.scrollHeight > box.clientHeight + 2 && ["hidden", "clip"].includes(style.overflowY)) {
    problems.push(`The window cuts its own content: ${px(box.scrollHeight - box.clientHeight)}px of it lie below its bottom edge and cannot be scrolled to (overflow: hidden). Let it scroll (overflow-y: auto; max-height: 100%)`);
  }
  const below = [...box.querySelectorAll("button, input, select, textarea, a[href]")].filter(shown)
    .filter((el) => el.getBoundingClientRect().top >= H - 4 && box.scrollHeight <= box.clientHeight + 2);
  if (below.length) {
    problems.push(`Controls of the window lie below the bottom of the screen and cannot be reached: ${below.slice(0, 3).map((el) => (el.innerText || el.getAttribute("aria-label") || el.name || el.tagName).trim().slice(0, 30)).join(", ")}. Let the window scroll (max-height: 100%; overflow-y: auto)`);
  }
  // Le choix affiché d'une liste déroulante, coupé par sa largeur.
  const canvas = document.createElement("canvas").getContext("2d");
  const cut = [...box.querySelectorAll("select")].filter(shown).map((select) => {
    const option = select.options[select.selectedIndex];
    if (!option) return null;
    const s = getComputedStyle(select);
    canvas.font = `${s.fontStyle} ${s.fontWeight} ${s.fontSize} ${s.fontFamily}`;
    const text = option.text.trim();
    const room = select.clientWidth - parseFloat(s.paddingLeft) - parseFloat(s.paddingRight) - 20;
    return canvas.measureText(text).width > room + 2 ? `"${text}"` : null;
  }).filter(Boolean);
  if (cut.length) {
    problems.push(`The chosen option of a list is cut in the window: ${cut.slice(0, 2).join(", ")} does not fit its field. Shorten the option texts (the date alone, or the match on one line and the date on another in the page) or let the field take the whole width`);
  }
  // Un libellé collé au champ du dessus : on ne sait plus à quel champ il va.
  const fields = [...box.querySelectorAll("input:not([type=hidden]), select, textarea")].filter(shown);
  const stuck = [...box.querySelectorAll("label")].filter(shown).filter((label) => {
    const top = label.getBoundingClientRect().top;
    const above = fields.map((field) => field.getBoundingClientRect()).filter((r) => r.bottom <= top + 2 && r.bottom > top - 40);
    return above.some((r) => top - r.bottom < 6);
  });
  if (stuck.length) {
    problems.push(`Labels touch the field above them in the window (${stuck.slice(0, 3).map((label) => `"${label.innerText.trim().slice(0, 30)}"`).join(", ")}): they read as the caption of that field. Space each field group (gap or margin-top of at least 12px above each label)`);
  }
  return problems;
})()
