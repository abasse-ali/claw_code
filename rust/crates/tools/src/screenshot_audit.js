// Défauts d'affichage mesurés dans la page par l'outil `Screenshot` : ce que la
// capture montre mais qu'un modèle peut manquer, et ce qu'un modèle qui ne lit
// pas les images ne verrait jamais. Chaque défaut est une phrase en anglais,
// destinée au modèle, qui nomme les éléments en cause (balise, id, classes,
// début de leur texte).
(() => {
  if (!document.body) {
    return { title: document.title || "", problems: ["The address did not return an HTML page"], textLength: 0 };
  }
  const W = document.documentElement.clientWidth || window.innerWidth;
  const mobile = W < 900;
  const problems = [];
  const SKIP = new Set(["SCRIPT", "STYLE", "NOSCRIPT", "TEMPLATE", "META", "LINK", "BR", "HEAD", "TITLE", "OPTION"]);
  const styles = new Map();
  const css = (el) => {
    let style = styles.get(el);
    if (!style) {
      style = getComputedStyle(el);
      styles.set(el, style);
    }
    return style;
  };
  const shown = (el) => {
    const style = css(el);
    if (style.display === "none" || style.visibility === "hidden" || style.visibility === "collapse") return false;
    for (let node = el; node && node !== document.documentElement; node = node.parentElement) {
      if (Number(css(node).opacity) === 0) return false;
    }
    const box = el.getBoundingClientRect();
    return box.width > 0 && box.height > 0;
  };
  const words = (el) => (el.innerText || el.value || el.getAttribute("aria-label") || el.getAttribute("alt") || "")
    .replace(/\s+/g, " ").trim();
  const name = (el) => {
    let label = el.tagName.toLowerCase();
    if (el.id) label += `#${el.id}`;
    const classes = typeof el.className === "string" ? el.className.trim().split(/\s+/).filter(Boolean) : [];
    if (classes.length) label += `.${classes.slice(0, 2).join(".")}`;
    const text = words(el);
    return text ? `${label} "${text.length > 40 ? `${text.slice(0, 39)}…` : text}"` : label;
  };
  const ownText = (el) => [...el.childNodes].some((node) => node.nodeType === 3 && node.textContent.trim());
  const list = (items, max = 5) => items.slice(0, max).join("; ") + (items.length > max ? `; and ${items.length - max} more` : "");
  const interactive = (el) => el.matches("a, button, label, summary, th, select, [role=button], [role=tab], [role=menuitem], input[type=button], input[type=submit]");

  const elements = [...document.body.querySelectorAll("*")].filter((el) => !SKIP.has(el.tagName)).slice(0, 3000);
  const visible = elements.filter(shown);
  const box = new Map(visible.map((el) => [el, el.getBoundingClientRect()]));
  const pinned = (el) => {
    for (let node = el; node && node !== document.body; node = node.parentElement) {
      const position = css(node).position;
      if (position === "fixed" || position === "sticky") return true;
    }
    return false;
  };
  // Ancêtre qui défile ou coupe dans sa propre largeur : ce qui le dépasse y reste.
  const scroller = (el) => {
    for (let node = el.parentElement; node && node !== document.body && node !== document.documentElement; node = node.parentElement) {
      if (["auto", "scroll", "hidden", "clip"].includes(css(node).overflowX)) return node;
    }
    return null;
  };

  // 1. Contenu qui sort de l'écran.
  const pageWidth = Math.max(document.documentElement.scrollWidth, document.body.scrollWidth);
  const escapes = (el) => {
    const rect = el && box.get(el);
    if (!rect) return false;
    return (rect.right > W + 1 && rect.left < W - 1) || (rect.left < -1 && rect.right > 1);
  };
  const outside = visible.filter((el) => escapes(el) && !escapes(el.parentElement) && !scroller(el));
  if (outside.length) {
    const how = pageWidth > W + 1 ? `the page scrolls sideways, ${pageWidth}px wide` : "it is cut off";
    problems.push(`Content goes past the edge of the ${W}px-wide screen (${how}): ${list(outside.map((el) => {
      const rect = box.get(el);
      return `${name(el)} spans x=${Math.round(rect.left)} to x=${Math.round(rect.right)}`;
    }))}`);
  } else if (pageWidth > W + 1) {
    problems.push(`The page is ${pageWidth}px wide on a ${W}px-wide screen and scrolls sideways (an empty element, a margin or a padding makes it wider)`);
  }

  // 2. Texte coupé : un libellé court ou un bouton qui ne tient pas dans sa boîte
  // (un long titre coupé exprès par des points de suspension n'est pas compté).
  let cut = visible.filter((el) => {
    const style = css(el);
    const text = words(el);
    if (!text || el.matches("input, textarea, select")) return false;
    const wide = el.scrollWidth > el.clientWidth + 1 && (["hidden", "clip"].includes(style.overflowX) || style.textOverflow === "ellipsis");
    const tall = el.clientHeight > 0 && el.scrollHeight > el.clientHeight + 2 && ["hidden", "clip"].includes(style.overflowY);
    if (!wide && !tall) return false;
    return text.length <= 30 || interactive(el) || Boolean(el.closest("button, a, label, [role=tab], [role=button]"));
  });
  cut = cut.filter((el) => !cut.some((other) => other !== el && el.contains(other)));
  if (cut.length) {
    problems.push(`Text cut off, its box is too small for it: ${list(cut.map((el) => (el.scrollWidth > el.clientWidth + 1
      ? `${name(el)} shows ${el.clientWidth}px of its ${el.scrollWidth}px width`
      : `${name(el)} shows ${el.clientHeight}px of its ${el.scrollHeight}px height`)))}`);
  }

  // 3. Éléments les uns sur les autres (hors barres fixes, sous lesquelles le contenu défile exprès).
  const candidates = visible.filter((el) => (interactive(el) || el.matches("input, textarea, img, h1, h2, h3, h4, h5, h6") || ownText(el)) && !pinned(el)).slice(0, 400);
  const overlaps = [];
  for (let i = 0; i < candidates.length && overlaps.length < 8; i += 1) {
    for (let j = i + 1; j < candidates.length; j += 1) {
      const a = candidates[i];
      const b = candidates[j];
      if (a.contains(b) || b.contains(a)) continue;
      const ra = box.get(a);
      const rb = box.get(b);
      const width = Math.min(ra.right, rb.right) - Math.max(ra.left, rb.left);
      const height = Math.min(ra.bottom, rb.bottom) - Math.max(ra.top, rb.top);
      if (width <= 2 || height <= 2) continue;
      const smaller = Math.min(ra.width * ra.height, rb.width * rb.height);
      if (width * height < Math.max(40, smaller * 0.25)) continue;
      overlaps.push(`${name(a)} and ${name(b)} (${Math.round(width)}x${Math.round(height)}px in common)`);
      break;
    }
  }
  if (overlaps.length) problems.push(`Elements drawn on top of each other: ${list(overlaps)}`);

  // 4. Élément marqué `hidden` qui s'affiche quand même.
  const hiddenShown = [...document.querySelectorAll("[hidden]")].filter((el) => {
    const rect = el.getBoundingClientRect();
    return css(el).display !== "none" && rect.width > 0 && rect.height > 0;
  });
  if (hiddenShown.length) {
    problems.push(`Elements with the hidden attribute are displayed anyway, a CSS display rule wins over it (add [hidden] { display: none !important; }): ${list(hiddenShown.map(name))}`);
  }

  // 5. Images qui ne se sont pas chargées.
  const broken = [...document.images].filter((img) => img.complete && img.naturalWidth === 0 && img.getAttribute("src") && css(img).display !== "none");
  if (broken.length) problems.push(`Images that did not load: ${list(broken.map((img) => `${name(img)} (${img.getAttribute("src")})`))}`);

  // 6. Sur un téléphone : texte trop petit, boutons trop petits pour le doigt, pas de viewport.
  if (mobile) {
    const tiny = visible.filter((el) => ownText(el) && parseFloat(css(el).fontSize) < 12);
    if (tiny.length) problems.push(`Text too small to read on a phone, under 12px: ${list(tiny.map((el) => `${name(el)} (${css(el).fontSize})`))}`);
    const targets = visible.filter((el) => el.matches("button, a[href], select, [role=button], input:not([type=hidden]):not([type=checkbox]):not([type=radio])")
      && css(el).display !== "inline" && !el.closest("p, li > span"))
      .filter((el) => box.get(el).width < 24 || box.get(el).height < 24);
    if (targets.length) {
      problems.push(`Buttons or links too small to tap, under 24x24px: ${list(targets.map((el) => `${name(el)} (${Math.round(box.get(el).width)}x${Math.round(box.get(el).height)}px)`))}`);
    }
    if (!document.querySelector('meta[name="viewport"]')) {
      problems.push('No <meta name="viewport" content="width=device-width, initial-scale=1"> in the page: phones show it zoomed out, with tiny text');
    }
  }

  // 7. Texte trop pâle sur son fond (fond uni seulement : une image de fond n'est pas jugée).
  const parse = (color) => {
    const found = /rgba?\(([^)]+)\)/.exec(color || "");
    if (!found) return null;
    const parts = found[1].split(/[\s,/]+/).filter(Boolean).map(Number);
    return { r: parts[0], g: parts[1], b: parts[2], a: parts.length > 3 ? parts[3] : 1 };
  };
  const blend = (top, under) => ({
    r: top.r * top.a + under.r * (1 - top.a),
    g: top.g * top.a + under.g * (1 - top.a),
    b: top.b * top.a + under.b * (1 - top.a),
    a: 1,
  });
  const background = (el) => {
    const layers = [];
    for (let node = el; node; node = node.parentElement) {
      const style = css(node);
      if (style.backgroundImage && style.backgroundImage !== "none") return null;
      const color = parse(style.backgroundColor);
      if (color && color.a > 0) {
        layers.push(color);
        if (color.a >= 0.99) break;
      }
    }
    let base = { r: 255, g: 255, b: 255, a: 1 };
    for (const color of layers.reverse()) base = blend(color, base);
    return base;
  };
  const luminance = ({ r, g, b }) => {
    const channel = (value) => {
      const v = value / 255;
      return v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4;
    };
    return 0.2126 * channel(r) + 0.7152 * channel(g) + 0.0722 * channel(b);
  };
  const pale = visible.filter((el) => ownText(el) && !el.closest("[disabled], [aria-disabled=true]")).map((el) => {
    const color = parse(css(el).color);
    const under = background(el);
    if (!color || !under) return null;
    const ink = blend(color, under);
    const [light, dark] = [luminance(ink), luminance(under)].sort((x, y) => y - x);
    const ratio = (light + 0.05) / (dark + 0.05);
    const size = parseFloat(css(el).fontSize);
    const large = size >= 24 || (Number(css(el).fontWeight) >= 700 && size >= 18.5);
    return ratio < (large ? 3 : 4.5) ? `${name(el)} (${ratio.toFixed(1)}:1)` : null;
  }).filter(Boolean);
  if (pale.length) problems.push(`Text with too little contrast against its background, 4.5:1 is needed (3:1 for large text): ${list(pale)}`);

  // 8. Page vide.
  const text = (document.body.innerText || "").trim();
  if (!text && !visible.some((el) => el.matches("img, svg, canvas, video, iframe"))) {
    problems.push("The page shows nothing: a blank screen (a script error, a missing build, or content that never loaded)");
  }
  return { title: document.title || "", problems, textLength: text.length };
})()
