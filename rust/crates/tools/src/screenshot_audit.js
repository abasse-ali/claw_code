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
    if (box.width <= 0 || box.height <= 0) return false;
    // Texte réservé aux lecteurs d'écran (.sr-only, .visually-hidden) : caché
    // exprès, ni coupé ni illisible.
    const clipped = (style.clip && style.clip !== "auto") || /inset\(\s*50%/.test(style.clipPath || "");
    const tiny = box.width <= 2 && box.height <= 2 && ["hidden", "clip"].includes(style.overflowX);
    return !clipped && !tiny;
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

  // 9. Barre fixe en haut : elle couvre le début de la page, ou prend trop de
  // place sur un téléphone (ses liens restent dépliés au lieu d'un bouton menu).
  const bars = visible.filter((el) => css(el).position === "fixed" && box.get(el).top <= 1 && box.get(el).width >= W * 0.6)
    .filter((el, _, all) => !all.some((other) => other !== el && other.contains(el)));
  if (window.scrollY < 2) {
    for (const bar of bars) {
      const rb = box.get(bar);
      const covered = visible.filter((el) => !pinned(el) && !bar.contains(el)
        && el.matches("h1, h2, h3, p, a, button, img, input, label")
        && box.get(el).top < rb.bottom - 2 && box.get(el).bottom > rb.top + 2);
      if (covered.length) {
        problems.push(`The fixed ${name(bar)} (${Math.round(rb.height)}px high) covers the top of the page, ${list(covered.map(name), 3)} sit underneath it: offset the content by the bar's height (padding-top on the body or the first section, scroll-margin-top on sections)`);
      }
    }
  }
  if (mobile) {
    const tall = bars.concat(visible.filter((el) => css(el).position === "sticky" && el.matches("header, nav, [role=banner]")))
      .find((el) => box.get(el).height > window.innerHeight * 0.25);
    if (tall) {
      problems.push(`On a phone, ${name(tall)} takes ${Math.round(box.get(tall).height / window.innerHeight * 100)}% of the screen height: fold its links into a menu button that opens and closes them`);
    }
  }

  // 10. Liens qui ne mènent nulle part : « # », « javascript: », section absente.
  if (!mobile) {
    const links = [...document.querySelectorAll("a[href]")].filter(shown);
    const dead = links.filter((a) => {
      const href = a.getAttribute("href").trim();
      return href === "#" || href === "" || href.toLowerCase().startsWith("javascript:");
    });
    if (dead.length) {
      problems.push(`Links that go nowhere (href="#" or javascript:): ${list(dead.map(name))}. Point each to a real page, section or address (mailto:, tel:, the real social profile), or make it a <button> if it runs an action`);
    }
    const missing = links.filter((a) => {
      const href = a.getAttribute("href").trim();
      if (!/^#[^\s#]+$/.test(href)) return false;
      try {
        return !document.getElementById(decodeURIComponent(href.slice(1)));
      } catch (_) {
        return true;
      }
    });
    if (missing.length) {
      problems.push(`Links to sections that do not exist: ${list(missing.map((a) => `${name(a)} -> ${a.getAttribute("href")}`))}`);
    }
  }

  // 11. Formulaires : champs sans libellé, erreurs affichées avant toute
  // saisie, champs et boutons laissés au style du navigateur.
  const fields = [...document.querySelectorAll("input, select, textarea")]
    .filter((el) => !["hidden", "submit", "button", "reset", "image"].includes(el.type) && shown(el));
  const labelled = (el) => (el.labels && el.labels.length) || el.getAttribute("aria-label") || el.getAttribute("aria-labelledby") || el.closest("label") || el.getAttribute("title");
  const unlabelled = fields.filter((el) => !labelled(el));
  if (unlabelled.length) {
    problems.push(`Form fields without a label (a placeholder is not one, it disappears as soon as the user types): ${list(unlabelled.map(name))}`);
  }
  const reddish = (value) => {
    const color = parse(value);
    return Boolean(color) && color.a > 0 && color.r > 150 && color.g < 110 && color.b < 110;
  };
  const early = fields.filter((el) => !el.value && document.activeElement !== el && el.matches(":invalid")
    && (reddish(css(el).borderTopColor) || reddish(css(el).outlineColor) || /rgba?\(\s*(?:1[6-9]\d|2\d\d),\s*\d{1,2},\s*\d{1,2}/.test(css(el).boxShadow)));
  if (early.length) {
    problems.push(`Form fields shown as errors before anyone typed in them (red on empty required fields): ${list(early.map(name))}. Show errors only after the user acts: :user-invalid, or a class added on blur or submit`);
  }
  const bodyFont = css(document.body).fontFamily;
  const controls = fields.concat([...document.querySelectorAll("button, input[type=submit], input[type=button]")].filter(shown))
    .filter((el) => !el.matches("[type=checkbox], [type=radio], [type=range], [type=color], [type=file]"));
  // Un bouton sans texte affiché (icône du menu) n'a pas de police à voir.
  const raw = controls.filter((el) => css(el).fontFamily !== bodyFont
    && (fields.includes(el) || /\p{L}/u.test(el.innerText || el.value || "")));
  if (raw.length) {
    problems.push(`Form fields or buttons keep the browser's default look (font ${css(raw[0]).fontFamily.split(",")[0]} instead of the page's ${bodyFont.split(",")[0]}): ${list(raw.map(name))}. Style them like the rest of the page (font: inherit, padding, border, radius; full width on phones)`);
  }

  // 12. Images sans texte de remplacement, boutons et liens sans nom.
  const noAlt = [...document.images].filter((img) => !img.hasAttribute("alt") && shown(img));
  if (noAlt.length) problems.push(`Images without an alt attribute (alt="" for a decorative one): ${list(noAlt.map((img) => `${name(img)} (${img.getAttribute("src") || ""})`))}`);
  const nameless = [...document.querySelectorAll("button, a[href], [role=button]")].filter((el) => shown(el) && !words(el)
    && !el.getAttribute("title") && !el.querySelector("img[alt]:not([alt='']), svg title, [aria-label]"));
  if (nameless.length) problems.push(`Buttons or links with no text or label, a screen reader announces nothing (add aria-label): ${list(nameless.map(name))}`);

  // 13. Texte de remplissage ou inachevé.
  const filler = /lorem ipsum|dolor sit amet|\b(?:todo|tbd|placeholder)\b|your (?:text|content|title|name) here|votre (?:texte|contenu|titre) ici|ins[eé]rer .{0,20} ici|insert .{0,20} here/i;
  const ownWords = (el) => [...el.childNodes].filter((node) => node.nodeType === 3).map((node) => node.textContent).join(" ");
  const fillers = visible.filter((el) => ownText(el) && filler.test(ownWords(el)));
  if (fillers.length) problems.push(`Filler or unfinished text: ${list(fillers.map(name))}. Write the real content`);

  // 14. Aucune typographie choisie : la police par défaut du navigateur.
  const mainFamily = bodyFont.split(",")[0].replace(/["']/g, "").trim().toLowerCase();
  if (["times new roman", "times", "serif"].includes(mainFamily)) {
    problems.push("The page has no typography of its own: its text uses the browser's default font (Times New Roman). Choose typefaces that fit the subject");
  }

  // 15. Focus clavier invisible (ordinateur) : rien ne change quand un lien
  // ou un bouton reçoit le focus.
  if (!mobile) {
    const keys = ["outlineStyle", "outlineWidth", "outlineColor", "boxShadow", "backgroundColor", "borderTopColor", "textDecorationLine", "color"];
    const look = (el) => {
      const style = getComputedStyle(el);
      return keys.map((key) => style[key]).join("|");
    };
    const focusable = [...document.querySelectorAll("a[href], button, input:not([type=hidden]), select, textarea")].filter(shown).slice(0, 12);
    const previous = document.activeElement;
    // Une transition (color 0.2s) rendrait encore l'ancien style au moment de
    // la mesure : elles sont coupées le temps de l'essai.
    const still = document.createElement("style");
    still.textContent = "*, *::before, *::after { transition: none !important; animation: none !important; }";
    document.head.appendChild(still);
    const silent = [];
    let tried = 0;
    for (const el of focusable) {
      const before = look(el);
      el.focus({ preventScroll: true });
      if (document.activeElement !== el) continue;
      tried += 1;
      if (look(el) === before) silent.push(el);
      el.blur();
    }
    still.remove();
    if (previous && previous.focus) previous.focus({ preventScroll: true });
    if (tried >= 2 && silent.length >= Math.max(2, tried / 2)) {
      problems.push(`No visible keyboard focus: nothing changes when ${list(silent.map(name), 3)} get the focus, so a keyboard user cannot see where they are. Add :focus-visible styles (an outline or a ring)`);
    }
  }

  // 16. Texte collé au bord de l'écran (téléphone) : l'étendue réelle des
  // lettres, pas la boîte du paragraphe (un texte centré n'y touche pas).
  if (mobile) {
    const edge = 8;
    const touching = visible.filter((el) => ownText(el) && !pinned(el) && !scroller(el)
      && el.matches("p, h1, h2, h3, h4, li, blockquote, figcaption, label, dt, dd, span, a"))
      .filter((el) => {
        const range = document.createRange();
        range.selectNodeContents(el);
        const text = range.getBoundingClientRect();
        return text.width > 0 && text.bottom > 0 && (text.left < edge || text.right > W - edge);
      });
    const outer = touching.filter((el) => !touching.some((other) => other !== el && other.contains(el)));
    if (outer.length) {
      problems.push(`Text touches the edge of the ${W}px-wide phone screen, with no side margin: ${list(outer.map(name))}. Give the page's containers a side padding (at least 16px on phones)`);
    }
  }

  // Remarques de design : pas des défauts, des choix à reconsidérer.
  const notes = [];
  if (!mobile) {
    const common = ["inter", "roboto", "arial", "helvetica", "helvetica neue", "open sans", "montserrat", "poppins", "lato",
      "system-ui", "-apple-system", "segoe ui", "nunito", "raleway", "playfair display", "source sans pro", "verdana"];
    const families = new Set([document.body, ...document.querySelectorAll("h1, h2, h3")].filter(Boolean)
      .map((el) => css(el).fontFamily.split(",")[0].replace(/["']/g, "").trim()));
    const generic = [...families].filter((family) => common.includes(family.toLowerCase()));
    if (generic.length) notes.push(`Typefaces ${generic.join(" and ")} are among the most frequent defaults of generated pages: a typeface chosen for this subject would give the page its own voice`);
    const wide = visible.filter((el) => el.matches("p, li") && ownText(el) && words(el).length > 160
      && box.get(el).width / parseFloat(css(el).fontSize) > 42);
    if (wide.length) notes.push(`Lines of text longer than about 85 characters, tiring to read: ${list(wide.map(name), 3)} (keep paragraphs under about 70ch wide)`);
    const centered = visible.filter((el) => el.matches("p") && css(el).textAlign === "center"
      && box.get(el).height / (parseFloat(css(el).lineHeight) || parseFloat(css(el).fontSize) * 1.4) > 3.5);
    if (centered.length) notes.push(`Long paragraphs set centered, hard to read: ${list(centered.map(name), 3)}`);
    const caps = visible.filter((el) => ownText(el) && css(el).textTransform === "uppercase" && words(el).length <= 30);
    if (caps.length >= 4) notes.push(`${caps.length} small labels in ALL CAPS (${list(caps.map(name), 3)}): a frequent tell of generated pages`);
    const pictograph = /^\p{Extended_Pictographic}[\uFE0F\u200D\p{Extended_Pictographic}]*$/u;
    const emoji = visible.filter((el) => pictograph.test(words(el)) && parseFloat(css(el).fontSize) >= 32);
    if (emoji.length) notes.push(`Emoji standing in for pictures (${list(emoji.map(name), 3)}): use the real images, or illustrations drawn for the subject (inline SVG, CSS)`);
    let animated = false;
    try {
      animated = document.getAnimations().length > 0 || visible.some((el) => css(el).animationName && css(el).animationName !== "none");
    } catch (_) { /* navigateur ancien */ }
    let reduced = false;
    for (const sheet of document.styleSheets) {
      let rules = [];
      try {
        rules = sheet.cssRules;
      } catch (_) {
        continue;
      }
      for (const rule of rules) {
        if (rule.conditionText && rule.conditionText.includes("prefers-reduced-motion")) reduced = true;
      }
    }
    if (animated && !reduced) notes.push("The page animates but ignores prefers-reduced-motion: add a @media (prefers-reduced-motion: reduce) rule that stops the motion");
  }
  return { title: document.title || "", problems, notes, textLength: text.length };
})()
