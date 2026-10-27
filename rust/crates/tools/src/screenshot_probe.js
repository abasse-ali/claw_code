// Essai joué par l'outil `Screenshot` sur un téléphone, après la capture : le
// menu qui replie la navigation s'ouvre-t-il vraiment ? Chaque défaut est une
// phrase en anglais, destinée au modèle.
(async () => {
  const problems = [];
  const shown = (el) => {
    const style = getComputedStyle(el);
    if (style.display === "none" || style.visibility === "hidden" || Number(style.opacity) === 0) return false;
    const box = el.getBoundingClientRect();
    return box.width > 0 && box.height > 0 && box.bottom > 0 && box.right > 0 && box.left < window.innerWidth;
  };
  const name = (el) => {
    let label = el.tagName.toLowerCase();
    if (el.id) label += `#${el.id}`;
    const classes = typeof el.className === "string" ? el.className.trim().split(/\s+/).filter(Boolean) : [];
    if (classes.length) label += `.${classes.slice(0, 2).join(".")}`;
    const text = (el.innerText || el.getAttribute("aria-label") || "").replace(/\s+/g, " ").trim();
    return text ? `${label} "${text.slice(0, 30)}"` : label;
  };
  const links = [...document.querySelectorAll("header a[href], nav a[href], [role=navigation] a[href]")];
  const hidden = links.filter((link) => !shown(link));
  if (links.length < 2 || hidden.length < 2) return { problems };
  const hint = /menu|burger|hamburger|nav-?toggle|toggle|drawer|navbar-toggler/i;
  const candidates = [...document.querySelectorAll("button, [role=button], label, a[href], div, span")]
    .filter((el) => shown(el) && !links.includes(el) && el.getBoundingClientRect().top < 220)
    .filter((el) => el.hasAttribute("aria-controls") || el.hasAttribute("aria-expanded")
      || hint.test(`${el.id} ${typeof el.className === "string" ? el.className : ""} ${el.getAttribute("aria-label") || ""} ${(el.innerText || "").trim().slice(0, 20)}`)
      || /^[\u2630\u2261]$/.test((el.innerText || "").trim()));
  const toggle = candidates.find((el) => !candidates.some((other) => other !== el && el.contains(other)));
  if (!toggle) {
    problems.push(`On a phone, ${hidden.length} navigation links are hidden and no menu button opens them: add a button (aria-expanded, aria-controls) that shows and hides them`);
    return { problems };
  }
  toggle.click();
  await new Promise((resolve) => setTimeout(resolve, 700));
  if (!hidden.some(shown)) {
    problems.push(`The menu button ${name(toggle)} does nothing on a phone: after a click, the ${hidden.length} navigation links stay hidden (check its click handler and the CSS class or attribute it toggles)`);
    return { problems };
  }
  // Le menu se referme : le parcours des boutons qui suit doit trouver la page
  // comme un visiteur la trouve. Vu le 27 octobre 2026 (session #35) : le menu
  // laissé ouvert par cet essai couvrait la page, et chaque bouton passait
  // pour « impossible à toucher ».
  const wait = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  toggle.click();
  await wait(500);
  if (hidden.some(shown)) {
    document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    await wait(400);
  }
  return { problems, leftOpen: hidden.some(shown) };
})()
