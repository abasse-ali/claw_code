// La page décrite en texte, pour l'outil `Browser` : ce que verrait un visiteur,
// pour un modèle qui ne lit pas les images. Une ligne par chose vue, dans
// l'ordre de lecture, avec sa taille, sa police, ses couleurs et sa place ; les
// commandes portent une référence (e12) que les actions suivantes visent. Les
// textes de toute la page sont rendus à part, pour dire ce qu'une action a
// changé. `clawView` : { full } (toute la page, sinon l'écran seulement).
(() => {
  const options = typeof clawView === "object" && clawView ? clawView : {};
  const W = innerWidth;
  const H = innerHeight;
  const styles = new Map();
  const css = (el) => {
    let style = styles.get(el);
    if (!style) { style = getComputedStyle(el); styles.set(el, style); }
    return style;
  };
  const shown = (el) => {
    const style = css(el);
    if (style.display === "none" || style.visibility === "hidden" || Number(style.opacity) === 0) return false;
    const rect = el.getBoundingClientRect();
    return rect.width > 0 && rect.height > 0;
  };
  const root = document.documentElement;
  let next = Number(root.getAttribute("data-claw-next") || 1);
  const ref = (el) => {
    let name = el.getAttribute("data-claw-ref");
    if (!name) {
      name = `e${next}`;
      next += 1;
      el.setAttribute("data-claw-ref", name);
    }
    return name;
  };
  const hex = (color) => {
    const match = /rgba?\(([^)]+)\)/.exec(color || "");
    if (!match) return color;
    const [r, g, b, a] = match[1].split(",").map(Number);
    if (a === 0) return "transparent";
    return `#${[r, g, b].map((value) => Math.round(value).toString(16).padStart(2, "0")).join("")}`;
  };
  const background = (el) => {
    for (let node = el; node && node.nodeType === 1; node = node.parentElement) {
      const style = css(node);
      if (style.backgroundImage && style.backgroundImage !== "none") return "a picture";
      const color = hex(style.backgroundColor);
      if (color !== "transparent") return color;
    }
    return "#ffffff";
  };
  const clip = (value, max = 90) => {
    const text = (value || "").replace(/\s+/g, " ").trim();
    return text.length > max ? `${text.slice(0, max - 1)}…` : text;
  };
  const tag = (el) => `${el.tagName.toLowerCase()}${el.id ? `#${el.id}` : ""}`;
  const where = (el) => {
    const rect = el.getBoundingClientRect();
    return `@${Math.round(rect.left)},${Math.round(rect.top + scrollY)} ${Math.round(rect.width)}x${Math.round(rect.height)}`;
  };
  const font = (el) => {
    const style = css(el);
    const weight = Number(style.fontWeight) >= 600 ? " bold" : "";
    return `${Math.round(parseFloat(style.fontSize))}px${weight} ${style.fontFamily.split(",")[0].replace(/["']/g, "").trim()}`;
  };
  const colors = (el) => `${hex(css(el).color)} on ${background(el)}`;
  const own = (el) => [...el.childNodes].filter((node) => node.nodeType === 3).map((node) => node.textContent).join(" ").replace(/\s+/g, " ").trim();
  const name = (el) => clip(
    el.getAttribute("aria-label")
      || (el.labels && el.labels[0] && el.labels[0].innerText)
      || el.innerText
      || el.value
      || el.getAttribute("title")
      || el.getAttribute("alt")
      || el.getAttribute("placeholder")
      || "",
    60,
  );
  const flags = (el, interactive) => {
    const out = [];
    const style = css(el);
    const rect = el.getBoundingClientRect();
    if (style.position === "fixed" || style.position === "sticky") out.push(style.position);
    if (rect.right > W + 1 || rect.left < -1) out.push("goes past the screen edge");
    if (el.scrollWidth > el.clientWidth + 1 && ["hidden", "clip"].includes(style.overflowX) && own(el)) out.push("text cut off");
    if (interactive) {
      if (el.disabled || el.getAttribute("aria-disabled") === "true") out.push("disabled");
      if (el.hasAttribute("aria-expanded")) out.push(`expanded=${el.getAttribute("aria-expanded")}`);
      if (el.getAttribute("aria-pressed") === "true" || el.getAttribute("aria-selected") === "true" || el.getAttribute("aria-current")) out.push("selected");
      if (el.checked) out.push("checked");
      if (el.required) out.push("required");
      try { if (el.matches(":user-invalid")) out.push(`invalid: ${clip(el.validationMessage, 60)}`); } catch (_) { /* navigateur ancien */ }
      if (rect.width < 44 || rect.height < 44) out.push("small to tap");
      // Le centre de la commande, là où tombe le doigt : ce qui s'y trouve doit être elle.
      if (rect.top >= 0 && rect.bottom <= H && rect.left >= 0 && rect.right <= W) {
        const top = document.elementFromPoint(rect.left + rect.width / 2, rect.top + rect.height / 2);
        if (top && top !== el && !el.contains(top) && !top.contains(el) && !(top.control && top.control === el)) {
          out.push(`covered by ${tag(top)}${clip(top.innerText, 30) ? ` "${clip(top.innerText, 30)}"` : ""}`);
        }
      }
    }
    return out.length ? ` (${out.join(", ")})` : "";
  };
  const CONTAINERS = "header, nav, main, section, article, aside, footer, form, dialog, [role=dialog], [role=alertdialog], ul, ol, table, fieldset, [role=tabpanel], [role=region], [role=list]";
  const CONTROLS = "a[href], button, input:not([type=hidden]), select, textarea, summary, [role=button], [role=link], [role=tab], [role=checkbox], [role=switch], [role=option], [role=menuitem], [contenteditable=true]";
  const MEDIA = "img, picture, video, canvas, iframe, svg[role=img], [role=img]";
  const SKIP = new Set(["SCRIPT", "STYLE", "NOSCRIPT", "TEMPLATE", "META", "LINK", "HEAD", "TITLE", "BR", "OPTION"]);
  const inScope = (el) => {
    if (options.full) return true;
    const rect = el.getBoundingClientRect();
    return rect.bottom > 0 && rect.top < H;
  };
  const lines = [];
  const texts = [];
  const below = [];
  const LIMIT = options.full ? 260 : 120;
  let skipped = 0;
  const say = (depth, line) => {
    if (lines.length < LIMIT) lines.push(`${"  ".repeat(Math.min(depth, 8))}${line}`);
    else skipped += 1;
  };
  const control = (el, depth) => {
    const kind = el.matches("a[href]") ? "link"
      : el.matches("select") ? "select"
        : el.matches("textarea") ? "field (text area)"
          : el.matches("input") ? (["checkbox", "radio"].includes(el.type) ? el.type : el.type === "submit" || el.type === "button" ? "button" : `field (${el.type || "text"})`)
            : el.matches("summary") ? "summary"
              : el.getAttribute("role") || "button";
    const label = name(el);
    texts.push(label);
    // Les valeurs des champs comptent pour ce qui a changé, même hors de l'écran.
    if (el.matches("input, textarea")) texts.push(el.value);
    if (el.matches("select") && el.selectedOptions[0]) texts.push(el.selectedOptions[0].text);
    if (!inScope(el)) { below.push(`${kind} "${label}"`); return; }
    let detail = "";
    if (el.matches("input:not([type=checkbox]):not([type=radio]):not([type=submit]):not([type=button]), textarea")) {
      const label2 = el.labels && el.labels[0] ? clip(el.labels[0].innerText, 40) : "";
      detail = `${label2 ? ` label "${label2}"` : ""} value "${clip(el.value, 40)}"${el.placeholder ? ` placeholder "${clip(el.placeholder, 40)}"` : ""}`;
    } else if (el.matches("select")) {
      const options2 = [...el.options].map((option) => clip(option.text, 20));
      detail = ` chosen "${clip(el.selectedOptions[0] ? el.selectedOptions[0].text : "", 30)}" of ${options2.length} (${options2.slice(0, 6).join(" / ")}${options2.length > 6 ? " …" : ""})`;
    } else if (el.matches("a[href]")) {
      detail = ` "${label}" -> ${clip(el.getAttribute("href"), 50)}`;
    } else {
      detail = ` "${label}"`;
    }
    say(depth, `${kind} [${ref(el)}]${detail} ${font(el)} ${colors(el)} ${where(el)}${flags(el, true)}`);
  };
  const visit = (el, depth) => {
    if (SKIP.has(el.tagName) || !shown(el)) {
      // Un conteneur caché garde ses enfants cachés ; un enfant fixe ou montré
      // malgré tout (display: contents) se voit à sa place.
      if (SKIP.has(el.tagName) || css(el).display === "none") return;
    }
    if (el.matches(CONTROLS)) { control(el, depth); return; }
    if (el.matches(MEDIA)) {
      const rect = el.getBoundingClientRect();
      if (rect.width <= 32 && rect.height <= 32 && el.matches("svg")) return;
      if (!inScope(el)) { below.push("picture"); return; }
      const alt = el.getAttribute("alt") || el.getAttribute("aria-label") || "";
      const source = el.currentSrc || el.getAttribute("src") || "";
      const broken = el.matches("img") && el.complete && el.naturalWidth === 0 ? " (did not load)" : "";
      say(depth, `picture${alt ? ` "${clip(alt, 60)}"` : " (no alt text)"}${source ? ` ${clip(source.split("/").pop(), 40)}` : ""} ${where(el)}${broken}${flags(el, false)}`);
      return;
    }
    if (/^H[1-6]$/.test(el.tagName)) {
      const words = clip(el.innerText, 100);
      texts.push(words);
      if (!inScope(el)) { below.push(`${el.tagName.toLowerCase()} "${clip(words, 40)}"`); return; }
      say(depth, `${el.tagName.toLowerCase()} "${words}" ${font(el)} ${colors(el)} ${where(el)}${flags(el, false)}`);
      return;
    }
    let inner = depth;
    if (el.matches(CONTAINERS)) {
      const heading = el.querySelector("h1, h2, h3, h4, legend, caption");
      const items = el.matches("ul, ol, [role=list]") ? ` ${el.children.length} items` : "";
      if (inScope(el)) {
        say(depth, `${tag(el)}${heading && shown(heading) ? ` «${clip(heading.innerText, 40)}»` : ""}${items} ${where(el)} background ${background(el)}${flags(el, false)}`);
        inner = depth + 1;
      }
    }
    const words = own(el);
    if (words) {
      texts.push(clip(words, 100));
      if (inScope(el)) say(inner, `text "${clip(words)}" ${font(el)} ${colors(el)} ${where(el)}${flags(el, false)}`);
    }
    for (const child of el.children) visit(child, inner);
  };
  const body = document.body;
  if (body) visit(body, 0);
  root.setAttribute("data-claw-next", String(next));
  const height = root.scrollHeight;
  const head = [
    `Page "${clip(document.title, 60)}" ${location.href}`,
    `Screen ${W}x${H}, scrolled ${Math.round(scrollY)} of ${height}px; text ${body ? font(body) : ""} ${body ? colors(body) : ""}`,
  ];
  if (skipped) lines.push(`… ${skipped} more lines (look with full true, or scroll)`);
  if (!options.full && below.length) {
    const unique = [...new Set(below)];
    lines.push(`Further down: ${unique.slice(0, 12).join(", ")}${unique.length > 12 ? `, and ${unique.length - 12} more` : ""}`);
  }
  return { head, lines, texts: [...new Set(texts.filter(Boolean))].slice(0, 600), url: location.href };
})()
