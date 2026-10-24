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
  // Éléments en cause : Claw demande au navigateur les règles CSS qui les
  // dessinent (fichier et ligne), pour que le modèle aille droit à la règle
  // au lieu de la chercher. Vu dans un essai réel : la moitié des requêtes
  // d'un tour passaient à lire et à chercher dans un CSS de 1 000 lignes.
  // `blame` et `blameNote` s'appellent juste avant le `push` concerné.
  const culprits = [];
  const mark = (kind, index, els) => {
    for (const el of els.filter((el) => el && el.nodeType === 1 && el !== document.documentElement).slice(0, 2)) {
      const n = culprits.length;
      el.setAttribute("data-claw-blame", `${el.getAttribute("data-claw-blame") || ""} ${n}`.trim());
      culprits.push({ n, kind, index });
    }
  };
  const blame = (els) => mark("problem", problems.length, els);
  const blameNote = (els) => mark("note", notes.length, els);
  // Mesures que Claw finit par le protocole du navigateur (survol, mouvement).
  let motion = false;
  const hoverLabels = [];
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
  // Pourquoi cet élément déborde : sans la cause, un modèle masque le
  // débordement (overflow-x: hidden sur body) au lieu de le réparer. Vu dans
  // un essai réel, six séries de captures sans trouver l'origine.
  const px = (value) => (/px$/.test(value || "") ? parseFloat(value) : NaN);
  const why = (el) => {
    const style = css(el);
    const rect = box.get(el);
    const reasons = [];
    // Item d'une grille ou d'un flex : sa largeur calculée est celle de la
    // piste, qui grandit jusqu'à la largeur minimale du contenu (min-width:
    // auto par défaut). Une largeur fixe n'existe pas, le modèle la cherche en vain.
    const parentStyle = el.parentElement && css(el.parentElement);
    const inTrack = parentStyle && (parentStyle.display.includes("grid") || parentStyle.display.includes("flex")) && rect.width > W + 1;
    const fixed = inTrack ? 0 : Math.max(px(style.minWidth) || 0, px(style.width) || 0);
    if (fixed > W) reasons.push(`a computed ${fixed > (px(style.minWidth) || 0) ? "width" : "min-width"} of ${Math.round(fixed)}px, wider than the screen (a px, vw or calc() value in the CSS: use max-width: 100%, or width: auto)`);
    const sides = (px(style.paddingLeft) || 0) + (px(style.paddingRight) || 0) + (px(style.marginLeft) || 0) + (px(style.marginRight) || 0);
    if (rect.width <= W + 1 && rect.left + rect.width > W + 1 && sides > 0 && style.boxSizing === "content-box" && /%$/.test(el.style.width || "")) {
      reasons.push("width: 100% plus its padding (content-box): set box-sizing: border-box");
    }
    const children = [...el.children].filter(shown);
    if (style.display.includes("flex") && style.flexDirection.startsWith("row") && style.flexWrap === "nowrap" && children.length >= 2
      && children.reduce((sum, kid) => sum + box.get(kid).width, 0) > W) {
      reasons.push(`a flex row of ${children.length} items that does not wrap (add flex-wrap: wrap, or stack them on phones)`);
    }
    if (style.display.includes("grid")) {
      const columns = style.gridTemplateColumns.split(" ").filter(Boolean);
      const total = columns.reduce((sum, col) => sum + (px(col) || 0), 0);
      if (columns.length >= 2 && total > W) reasons.push(`${columns.length} grid columns adding up to ${Math.round(total)}px (use a single column under 640px, or minmax(0, 1fr))`);
    }
    // Le descendant qui pousse le bord : texte sur une ligne, image, tableau,
    // champ ou mot trop long.
    // `strict` : seulement ce qui pousse vraiment le bord, pas un bloc qui ne
    // fait que remplir une piste trop large.
    const pushing = (strict = false) => {
      let widest = null;
      for (const node of el.querySelectorAll("*")) {
        const r = box.get(node);
        if (!r || !shown(node)) continue;
        if (r.right > W + 1 && (!widest || r.right > box.get(widest).right)) widest = node;
      }
      if (!widest) return null;
      const ws = css(widest);
      const w = box.get(widest);
      if (ws.whiteSpace === "nowrap" || ws.whiteSpace === "pre") return `its text ${name(widest)} is kept on one line (white-space: ${ws.whiteSpace}) and is ${Math.round(w.width)}px wide`;
      if (widest.matches("img, svg, video, canvas, iframe")) return `its ${widest.tagName.toLowerCase()} ${name(widest)} is ${Math.round(w.width)}px wide (max-width: 100%; height: auto)`;
      if (widest.matches("table, pre, code")) return `its ${widest.tagName.toLowerCase()} ${name(widest)} is ${Math.round(w.width)}px wide (wrap it in a block with overflow-x: auto, or lay the rows out as a list on phones)`;
      if (widest.matches("input, select, textarea")) return `its field ${name(widest)} is ${Math.round(w.width)}px wide (fields have an intrinsic width: give them width: 100% and min-width: 0)`;
      if (/\S{30,}/.test(words(widest))) return `its text ${name(widest)} holds a word or address longer than the screen (overflow-wrap: anywhere)`;
      if (strict) return null;
      return `its widest content is ${name(widest)}, ${Math.round(w.width)}px wide, reaching x=${Math.round(w.right)}`;
    };
    // Item d'une grille ou d'un flex : la piste grandit jusqu'à la largeur
    // minimale du contenu (min-width: auto par défaut), et c'est là que le
    // modèle cherche en vain une largeur fixe.
    if (inTrack && !reasons.length) {
      const kind = parentStyle.display.includes("grid") ? "grid" : "flex";
      const inside = pushing(true);
      return `its parent ${name(el.parentElement)} is a ${kind} whose track grows to the item's minimum content width (${kind} items default to min-width: auto): set min-width: 0 on ${name(el)}${kind === "grid" ? " or minmax(0, 1fr) on the track" : ""}, and shrink what cannot shrink inside${inside ? `: ${inside}` : ""}`;
    }
    if (reasons.length) return reasons.join("; ");
    const inside = pushing();
    if (inside) return inside;
    if (rect.width > W + 1) return `the element itself is ${Math.round(rect.width)}px wide with no content that wide: a width, margin or transform in the CSS`;
    return `it starts at x=${Math.round(rect.left)}: a left margin, padding or transform pushes it`;
  };
  if (outside.length) {
    const how = pageWidth > W + 1 ? `the page scrolls sideways, ${pageWidth}px wide` : "it is cut off";
    const masked = ["hidden", "clip"].includes(css(document.body).overflowX) || ["hidden", "clip"].includes(css(document.documentElement).overflowX)
      ? " overflow-x: hidden on body or html hides the symptom, not the cause: remove it and fix the element." : "";
    blame(outside.flatMap((el) => [el, el.parentElement]));
    problems.push(`Content goes past the edge of the ${W}px-wide screen (${how}): ${list(outside.map((el) => {
      const rect = box.get(el);
      return `${name(el)} spans x=${Math.round(rect.left)} to x=${Math.round(rect.right)}, because of ${why(el)}`;
    }), 4)}.${masked}`);
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
    blame(cut);
    problems.push(`Text cut off, its box is too small for it: ${list(cut.map((el) => (el.scrollWidth > el.clientWidth + 1
      ? `${name(el)} shows ${el.clientWidth}px of its ${el.scrollWidth}px width`
      : `${name(el)} shows ${el.clientHeight}px of its ${el.scrollHeight}px height`)))}`);
  }

  // 3. Éléments les uns sur les autres (hors barres fixes, sous lesquelles le contenu défile exprès).
  const candidates = visible.filter((el) => (interactive(el) || el.matches("input, textarea, img, h1, h2, h3, h4, h5, h6") || ownText(el)) && !pinned(el)).slice(0, 400);
  // La partie visible d'un élément : un parent qui découpe (overflow) rogne
  // une image agrandie ou décalée dans son cadre (parallaxe, zoom au survol).
  const seen = new Map();
  const visibleBox = (el) => {
    if (seen.has(el)) return seen.get(el);
    const rect = box.get(el) || el.getBoundingClientRect();
    let { left, top, right, bottom } = rect;
    for (let node = el.parentElement; node && node !== document.body; node = node.parentElement) {
      const style = css(node);
      if (style.overflowX === "visible" && style.overflowY === "visible") continue;
      const frame = node.getBoundingClientRect();
      if (style.overflowX !== "visible") { left = Math.max(left, frame.left); right = Math.min(right, frame.right); }
      if (style.overflowY !== "visible") { top = Math.max(top, frame.top); bottom = Math.min(bottom, frame.bottom); }
    }
    const clipped = { left, top, right, bottom, width: Math.max(0, right - left), height: Math.max(0, bottom - top) };
    seen.set(el, clipped);
    return clipped;
  };
  const overlaps = [];
  for (let i = 0; i < candidates.length && overlaps.length < 8; i += 1) {
    for (let j = i + 1; j < candidates.length; j += 1) {
      const a = candidates[i];
      const b = candidates[j];
      if (a.contains(b) || b.contains(a)) continue;
      const ra = visibleBox(a);
      const rb = visibleBox(b);
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
    if (tiny.length) blame(tiny);
    if (tiny.length) problems.push(`Text too small to read on a phone, under 12px: ${list(tiny.map((el) => `${name(el)} (${css(el).fontSize})`))}`);
    const targets = visible.filter((el) => el.matches("button, a[href], select, [role=button], input:not([type=hidden]):not([type=checkbox]):not([type=radio])")
      && css(el).display !== "inline" && !el.closest("p, li > span"))
      .filter((el) => box.get(el).width < 24 || box.get(el).height < 24);
    if (targets.length) {
      blame(targets);
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
  const paleEls = [];
  const pale = visible.filter((el) => ownText(el) && !el.closest("[disabled], [aria-disabled=true]")).map((el) => {
    const color = parse(css(el).color);
    const under = background(el);
    if (!color || !under) return null;
    const ink = blend(color, under);
    const [light, dark] = [luminance(ink), luminance(under)].sort((x, y) => y - x);
    const ratio = (light + 0.05) / (dark + 0.05);
    const size = parseFloat(css(el).fontSize);
    const large = size >= 24 || (Number(css(el).fontWeight) >= 700 && size >= 18.5);
    if (ratio < (large ? 3 : 4.5)) paleEls.push(el);
    return ratio < (large ? 3 : 4.5) ? `${name(el)} (${ratio.toFixed(1)}:1)` : null;
  }).filter(Boolean);
  if (pale.length) blame(paleEls);
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
        blame([bar]);
        problems.push(`The fixed ${name(bar)} (${Math.round(rb.height)}px high) covers the top of the page, ${list(covered.map(name), 3)} sit underneath it: offset the content by the bar's height (padding-top on the body or the first section, scroll-margin-top on sections)`);
      }
    }
  }
  if (mobile) {
    const tall = bars.concat(visible.filter((el) => css(el).position === "sticky" && el.matches("header, nav, [role=banner]")))
      .find((el) => box.get(el).height > window.innerHeight * 0.25);
    if (tall) {
      blame([tall]);
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
  // Un label existe mais vise un autre élément de même id : la vraie cause.
  // Vu dans un essai réel : section#cours et select#cours, et un modèle qui
  // a pris le défaut pour une erreur de la mesure.
  const misdirected = unlabelled.filter((el) => el.id && document.querySelector(`label[for="${CSS.escape(el.id)}"]`));
  if (misdirected.length) {
    problems.push(`Form fields whose label points to another element with the same id, so the field has no label (screen readers announce nothing, a click on the label does not reach it): ${list(misdirected.map((el) => `${name(el)}: label for="${el.id}" reaches ${name(document.getElementById(el.id))} first`))}. Ids must be unique: rename one of them and its links`);
  }
  const bare = unlabelled.filter((el) => !misdirected.includes(el));
  if (bare.length) {
    problems.push(`Form fields without a label (a placeholder is not one, it disappears as soon as the user types): ${list(bare.map(name))}`);
  }
  // Ids en double : un lien #id et un label for= n'atteignent que le premier.
  const seenIds = new Map();
  for (const el of document.querySelectorAll("[id]")) {
    if (el.id) seenIds.set(el.id, (seenIds.get(el.id) || []).concat([el]));
  }
  const doubled = [...seenIds.entries()].filter(([, els]) => els.length > 1);
  if (doubled.length) {
    problems.push(`Ids used more than once (links to #id, label for= and scripts reach only the first): ${list(doubled.map(([id, els]) => `"${id}" on ${els.slice(0, 3).map(name).join(", ")}`))}. Give each element its own id`);
  }
  const reddish = (value) => {
    const color = parse(value);
    return Boolean(color) && color.a > 0 && color.r > 150 && color.g < 110 && color.b < 110;
  };
  const early = fields.filter((el) => !el.value && document.activeElement !== el && el.matches(":invalid")
    && (reddish(css(el).borderTopColor) || reddish(css(el).outlineColor) || /rgba?\(\s*(?:1[6-9]\d|2\d\d),\s*\d{1,2},\s*\d{1,2}/.test(css(el).boxShadow)));
  if (early.length) {
    blame(early);
    problems.push(`Form fields shown as errors before anyone typed in them (red on empty required fields): ${list(early.map(name))}. Show errors only after the user acts: :user-invalid, or a class added on blur or submit`);
  }
  const bodyFont = css(document.body).fontFamily;
  const controls = fields.concat([...document.querySelectorAll("button, input[type=submit], input[type=button]")].filter(shown))
    .filter((el) => !el.matches("[type=checkbox], [type=radio], [type=range], [type=color], [type=file]"));
  // Un bouton sans texte affiché (icône du menu) n'a pas de police à voir ;
  // un bouton dans la police des titres de la page est un choix, pas un oubli.
  const pageFonts = new Set([document.body, ...document.querySelectorAll("h1, h2, h3")].filter(Boolean)
    .map((el) => css(el).fontFamily));
  const raw = controls.filter((el) => !pageFonts.has(css(el).fontFamily)
    && (fields.includes(el) || /\p{L}/u.test(el.innerText || el.value || "")));
  if (raw.length) {
    blame(raw);
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

  // 14b. Polices déclarées en CSS mais jamais chargées : le texte tombe sur la
  // police de secours et toute la typographie du plan disparaît. Vu dans un
  // essai réel : `css2=Fraunces` au lieu de `css2?family=Fraunces`, et une
  // page dessinée pour Fraunces et DM Sans rendue en Times et Arial.
  // `document.fonts.check` dit vrai pour toute famille inconnue : le signal
  // sûr est la liste des faces que la page a déclarées (@font-face, feuille
  // Google Fonts), et leur état.
  if (document.fonts && typeof document.fonts[Symbol.iterator] === "function") {
    const systemFonts = new Set(["serif", "sans-serif", "monospace", "cursive", "fantasy", "system-ui", "ui-serif", "ui-sans-serif",
      "ui-monospace", "-apple-system", "blinkmacsystemfont", "segoe ui", "arial", "helvetica", "helvetica neue", "times new roman",
      "times", "georgia", "verdana", "tahoma", "courier new", "courier", "trebuchet ms", "inherit", "initial"]);
    const wanted = new Set();
    for (const el of [document.body, ...document.querySelectorAll("h1, h2, h3, p, a, button, li")].filter(Boolean).slice(0, 200)) {
      const first = css(el).fontFamily.split(",")[0].replace(/["']/g, "").trim();
      if (first && !systemFonts.has(first.toLowerCase())) wanted.add(first);
    }
    const faces = new Map();
    for (const face of document.fonts) {
      const family = face.family.replace(/["']/g, "").trim().toLowerCase();
      faces.set(family, (faces.get(family) || false) || face.status === "loaded");
    }
    const missing = [...wanted].filter((family) => faces.get(family.toLowerCase()) !== true);
    if (missing.length) {
      const links = [...document.querySelectorAll('link[rel~="stylesheet"][href*="fonts.googleapis.com"]')].map((link) => link.getAttribute("href"));
      const badLink = links.find((href) => !/\/css2?\?/.test(href) || !/[?&]family=/.test(href));
      const hint = badLink
        ? `the Google Fonts link is malformed (${badLink.slice(0, 90)}): it must look like https://fonts.googleapis.com/css2?family=Name:wght@400;700&display=swap`
        : links.length
          ? "the Google Fonts link does not include these families, or the page is opened without network access: check the family names in the link"
          : "no stylesheet loads them (add the Google Fonts <link>, or @font-face rules)";
      blame([document.body]);
      problems.push(`Typefaces declared in the CSS but never loaded, so the text falls back to a default font: ${missing.map((family) => `"${family}"`).join(", ")}; ${hint}`);
    }
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
      blame(silent);
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
      blame([outer[0], outer[0].parentElement]);
      problems.push(`Text touches the edge of the ${W}px-wide phone screen, with no side margin: ${list(outer.map(name))}. Give the page's containers a side padding (at least 16px on phones)`);
    }
  }

  // 17. Icône dessinée en géant : un <svg> sans taille remplit son conteneur
  // (cinq étoiles d'avis devenues larges comme la page). Une icône se
  // reconnaît à son système de coordonnées (une viewBox de 64 unités au plus,
  // comme le 24x24 de Lucide) ou à son nom ; une illustration ou une vague
  // décorative, même aria-hidden, a la sienne et n'est pas visée (essai réel :
  // une vague de 1200px prise pour une icône).
  const iconWords = /icon|star|logo|arrow|chevron|check|social|rating|ico-|bullet/i;
  const giants = visible.filter((el) => el.tagName.toLowerCase() === "svg").filter((el) => {
    const rect = box.get(el);
    if (!rect || rect.width <= 120 || rect.height <= 120) return false;
    const view = el.viewBox && el.viewBox.baseVal;
    const iconGrid = Boolean(view) && view.width > 0 && view.width <= 64 && view.height <= 64;
    const label = `${el.id} ${el.getAttribute("class") || ""} ${el.parentElement ? `${el.parentElement.id} ${el.parentElement.getAttribute("class") || ""}` : ""}`;
    return iconGrid || (iconWords.test(label) && !(view && view.width > 64));
  });
  if (giants.length) {
    blame(giants);
    problems.push(`Icons drawn far larger than intended (an <svg> without a size fills its container): ${list(giants.map((el) => {
      const rect = box.get(el);
      return `${name(el)} ${Math.round(rect.width)}x${Math.round(rect.height)}px`;
    }))}. Give each icon a width and height (CSS on its class, or width/height attributes)`);
  }

  // 17 bis. Icône à traits dessinée remplie : une icône de Lucide mise dans un
  // sprite (<symbol> puis <use>) perd les fill="none" et stroke de son <svg> ;
  // sans eux, ses traits deviennent des taches noires, et un « + » (deux traits
  // ouverts) ne se voit plus du tout. Vu le 24 octobre 2026 (session de test de
  // Claw Web) : toute une appli de budget aux icônes en taches. Une icône à traits
  // se reconnaît à ses tracés ouverts (sans Z) ou à ses <line>/<polyline> : les
  // icônes pleines (Font Awesome) ferment leurs tracés.
  const lineArt = (root) => {
    const shapes = [...root.querySelectorAll("path, line, polyline, circle, rect, polygon, ellipse")];
    if (!shapes.length || shapes.some((shape) => (shape.getAttribute("fill") || "").trim() && shape.getAttribute("fill") !== "none")) return false;
    const open = shapes.filter((shape) => shape.matches("line, polyline")
      || (shape.matches("path") && !/z\s*$/i.test((shape.getAttribute("d") || "").trim())));
    return open.length * 2 >= shapes.length;
  };
  const blobs = visible.filter((el) => el.tagName.toLowerCase() === "svg").filter((el) => {
    const rect = box.get(el);
    if (!rect || rect.width > 64 || rect.height > 64) return false;
    const use = el.querySelector("use");
    const ref = use && (use.getAttribute("href") || use.getAttribute("xlink:href") || "");
    const art = ref && ref.startsWith("#") ? document.getElementById(ref.slice(1)) : el;
    if (!art || !lineArt(art)) return false;
    const style = getComputedStyle(use || el);
    return style.fill !== "none" && (style.stroke === "none" || parseFloat(style.strokeWidth) === 0);
  });
  if (blobs.length) {
    blame(blobs);
    problems.push(`Line icons drawn as filled shapes (black blobs, or nothing for a "+" made of open lines): ${list(blobs.map((el) => name(el)))}. They lost fill="none" and stroke="currentColor" (a <symbol> sprite drops the attributes of the icon's <svg>): give them back, on each <symbol> or in CSS (.icon { fill: none; stroke: currentColor; stroke-width: 2; stroke-linecap: round; stroke-linejoin: round; })`);
  }

  // 19. Section qui n'affiche que son titre : son contenu manque ou est
  // caché sur cet écran. Vu dans un essai réel : le planning d'un studio,
  // écrit en liste pour le téléphone et masqué sur ordinateur par une media
  // query, sans version pour ordinateur ; un titre, un trait, et rien.
  const media = "img, svg, video, canvas, iframe, picture, input, select, textarea, button";
  const headingLike = (node) => node.closest("h1, h2, h3, h4, h5, h6");
  const emptySections = [...document.querySelectorAll("main section, body > section, main > article")]
    .filter((section) => shown(section) && section.querySelector("h1, h2, h3"))
    .filter((section) => !section.querySelector("section"))
    .map((section) => {
      const heading = section.querySelector("h1, h2, h3");
      // Le premier paragraphe court sous le titre est un sous-titre, pas un contenu.
      const subtitle = heading.nextElementSibling && heading.nextElementSibling.matches("p") && words(heading.nextElementSibling).length < 140
        ? heading.nextElementSibling : null;
      let shownText = 0;
      let hiddenText = 0;
      let hiddenBy = null;
      for (const node of section.querySelectorAll("*")) {
        if (node === heading || headingLike(node) || node === subtitle || (subtitle && subtitle.contains(node))) continue;
        if (!ownText(node)) continue;
        const length = ownWords(node).length;
        if (shown(node)) shownText += length;
        else {
          hiddenText += length;
          if (!hiddenBy) {
            for (let up = node; up && up !== section; up = up.parentElement) {
              if (css(up).display === "none") { hiddenBy = up; break; }
            }
          }
        }
      }
      const visibleMedia = [...section.querySelectorAll(media)].some(shown);
      // Un conteneur prévu pour le contenu, resté vide (un script qui ne l'a
      // pas rempli) : une liste, un corps de tableau, un bloc à id.
      const vacant = [...section.querySelectorAll("ul, ol, dl, tbody, table, div[id], section[id] > div[class]")]
        .find((node) => !node.firstElementChild && !node.textContent.trim() && node !== section);
      return { section, heading, shownText, hiddenText, hiddenBy, visibleMedia, vacant };
    })
    // Un titre suivi d'une phrase est une section courte, pas une section
    // vide : seuls comptent un contenu caché sur cet écran, ou un conteneur
    // prévu pour lui et resté vide.
    .filter((entry) => entry.shownText < 20 && !entry.visibleMedia
      && ((entry.hiddenText >= 40 && entry.hiddenBy) || entry.vacant));
  for (const entry of emptySections.slice(0, 3)) {
    const cause = entry.hiddenText >= 40 && entry.hiddenBy
      ? `it holds ${entry.hiddenText} characters of text hidden on this screen by display: none on ${name(entry.hiddenBy)} (a media query hides one version without showing another)`
      : `${name(entry.vacant)} is empty (a script that did not fill it, or markup that was never written)`;
    blame([entry.hiddenBy || entry.vacant || entry.section]);
    problems.push(`The section ${name(entry.section)} shows only its title "${words(entry.heading).slice(0, 40)}" on this ${W}px-wide screen: ${cause}`);
  }

  // 18. Emoji dans la page : un défaut, pas une remarque. Un emoji est rendu
  // par la police du système, donc différent sur chaque appareil, et il
  // n'appartient pas au sujet ; une icône ou une image se dessine en SVG.
  // Un emoji seul dans un élément (icône) comme un emoji dans une phrase
  // (une baguette après un titre) sont relevés ; les symboles de texte courants
  // (flèches, puces, étoiles de notation) ne le sont pas.
  const anyEmoji = /\p{Extended_Pictographic}/u;
  const textSymbol = /^[\u2190-\u21FF\u2022\u2023\u25E6\u2043\u2219\u2605\u2606\u2713\u2714\u2715\u2716\u00A9\u00AE\u2122\u2026\u00B7\u2014\u2013]$/u;
  const emojiHosts = visible.filter((el) => {
    const own = [...el.childNodes].filter((node) => node.nodeType === 3).map((node) => node.textContent).join("");
    if (!anyEmoji.test(own)) return false;
    const chars = [...own.replace(/\s/g, "")].filter((ch) => anyEmoji.test(ch));
    return chars.some((ch) => !textSymbol.test(ch));
  });
  if (emojiHosts.length) {
    const pictures = emojiHosts.filter((el) => /^\p{Extended_Pictographic}[\uFE0F\u200D\p{Extended_Pictographic}]*$/u.test(words(el)));
    const inline = emojiHosts.filter((el) => !pictures.includes(el));
    const parts = [];
    if (pictures.length) parts.push(`as icons or pictures: ${list(pictures.map(name), 4)}`);
    if (inline.length) parts.push(`inside text: ${list(inline.map(name), 3)}`);
    blame(emojiHosts);
    problems.push(`Emoji in the page (${parts.join("; ")}): emoji are drawn by the device's own font, look different on every phone and computer, and belong to no design. Replace each one with an inline SVG drawn for the subject (with width, height and aria-hidden="true", or a role="img" and aria-label when it carries meaning), or remove it from the text`);
  }

  // Remarques de design : pas des défauts, des choix à reconsidérer.
  const notes = [];
  if (!mobile) {
    const common = ["inter", "roboto", "arial", "helvetica", "helvetica neue", "open sans", "montserrat", "poppins", "lato",
      "system-ui", "-apple-system", "segoe ui", "nunito", "raleway", "playfair display", "source sans pro", "verdana",
      "space grotesk", "dm sans", "work sans", "ubuntu", "georgia", "times new roman"];
    const families = new Set([document.body, ...document.querySelectorAll("h1, h2, h3")].filter(Boolean)
      .map((el) => css(el).fontFamily.split(",")[0].replace(/["']/g, "").trim()));
    const generic = [...families].filter((family) => common.includes(family.toLowerCase()));
    if (generic.length) blameNote([document.body, document.querySelector("h1, h2")]);
    if (generic.length) notes.push(`Typefaces ${generic.join(" and ")} are among the most frequent defaults of generated pages: a typeface chosen for this subject would give the page its own voice`);
    // 38em, soit environ 75 caractères : la mesure confortable d'un studio.
    const wide = visible.filter((el) => el.matches("p, li") && ownText(el) && words(el).length > 140
      && box.get(el).width / parseFloat(css(el).fontSize) > 38);
    if (wide.length) blameNote(wide);
    if (wide.length) notes.push(`Lines of text longer than about 75 characters, tiring to read: ${list(wide.map(name), 3)} (keep paragraphs under about 65ch wide)`);
    // Corps de texte sous 16px ou interligne serré : la lecture fatigue.
    const body = visible.filter((el) => el.matches("p, li") && ownText(el) && words(el).length > 60);
    const smallBody = body.filter((el) => parseFloat(css(el).fontSize) < 16);
    if (smallBody.length >= 2) blameNote(smallBody);
    if (smallBody.length >= 2) notes.push(`Body text under 16px (${list(smallBody.map((el) => `${name(el)} ${css(el).fontSize}`), 3)}): set paragraphs at 16 to 18px`);
    const tight = body.filter((el) => {
      const line = parseFloat(css(el).lineHeight);
      return Number.isFinite(line) && line / parseFloat(css(el).fontSize) < 1.4;
    });
    if (tight.length >= 2) blameNote(tight);
    if (tight.length >= 2) notes.push(`Body text with a tight line-height, under 1.4 (${list(tight.map(name), 3)}): use 1.5 to 1.7 for paragraphs`);
    // Un titre à peine plus grand que le texte : la hiérarchie ne se voit pas.
    const h1 = document.querySelector("h1");
    const bodySize = parseFloat(css(document.body).fontSize) || 16;
    if (h1 && shown(h1) && parseFloat(css(h1).fontSize) / bodySize < 2.2) {
      blameNote([h1]);
      notes.push(`The h1 is only ${(parseFloat(css(h1).fontSize) / bodySize).toFixed(1)} times the body text (${css(h1).fontSize}): a first screen needs a clear focal point, at least 2.5 times the body size on desktops`);
    }
    const centered = visible.filter((el) => el.matches("p") && css(el).textAlign === "center"
      && box.get(el).height / (parseFloat(css(el).lineHeight) || parseFloat(css(el).fontSize) * 1.4) > 3.5);
    if (centered.length) blameNote(centered);
    if (centered.length) notes.push(`Long paragraphs set centered, hard to read: ${list(centered.map(name), 3)}`);
    // Dès deux : un studio exigeant n'en garde pas une par section.
    const caps = visible.filter((el) => ownText(el) && css(el).textTransform === "uppercase" && words(el).length <= 30
      && !el.matches("button, a, th, [role=button], nav *"));
    if (caps.length >= 2) blameNote(caps);
    if (caps.length >= 2) notes.push(`${caps.length} small labels in ALL CAPS (${list(caps.map(name), 3)}): a frequent tell of generated pages; a strong page uses a weight, a color or a size change instead`);
    let animated = false;
    try {
      animated = document.getAnimations().length > 0 || visible.some((el) => css(el).animationName && css(el).animationName !== "none");
    } catch (_) { /* navigateur ancien */ }
    // `prefers-reduced-motion` se lit dans le texte des feuilles, par le
    // protocole du navigateur : une page ouverte en file:// ne laisse pas son
    // script lire leurs règles, et la mesure se trompait (essai réel).
    motion = animated;

    // Mouvement : ce qui s'anime, et comment. Une transition sur la mise en
    // page (largeur, marge, position) saccade ; un retour au survol de plus
    // d'une demi-seconde semble lent ; une boucle sans fin distrait.
    const seconds = (text) => {
      const values = String(text).split(",").map((part) => {
        const value = parseFloat(part);
        return part.trim().endsWith("ms") ? value / 1000 : value;
      }).filter((value) => !Number.isNaN(value));
      return values.length ? Math.max(...values) : 0;
    };
    // La classe d'un SVG ne se lit pas comme celle d'un element HTML.
    const className = (el) => (typeof el.className === "string" ? el.className : (el.getAttribute && el.getAttribute("class")) || "");
    // Un lien d'evitement (skip link) glisse depuis le haut au focus : c'est voulu.
    const feedback = visible.filter((el) => (el.matches("a, button, [role=button], summary") || /(^|\s|-)card/i.test(className(el)))
      && !/skip|sr-only|visually-hidden/i.test(className(el)) && box.get(el).bottom > 0);
    const layoutProps = /^(width|height|min-width|min-height|max-width|top|left|right|bottom|inset|margin|padding)/;
    const shaky = feedback.filter((el) => {
      const props = css(el).transitionProperty.split(",").map((part) => part.trim());
      const times = css(el).transitionDuration.split(",").map((part) => seconds(part));
      return props.some((prop, index) => layoutProps.test(prop) && (times[index % times.length] || 0) > 0);
    });
    if (shaky.length) blameNote(shaky);
    if (shaky.length) notes.push(`${shaky.length} links, buttons or cards animate their size or position (${list(shaky.map(name), 3)}): transitions on width, margin, top or padding stutter; animate transform and opacity instead (translateY(-2px), scale(1.02))`);
    const slow = feedback.filter((el) => seconds(css(el).transitionDuration) > 0.6);
    if (slow.length) blameNote(slow);
    if (slow.length) notes.push(`${slow.length} links or buttons take more than 0.6s to react (${list(slow.map((el) => `${name(el)} ${seconds(css(el).transitionDuration)}s`), 3)}): hover and press feedback feels instant at 150 to 250ms`);
    try {
      const looping = document.getAnimations().filter((animation) => {
        const timing = animation.effect && animation.effect.getTiming ? animation.effect.getTiming() : {};
        const target = animation.effect && animation.effect.target;
        return animation.playState === "running" && timing.iterations === Infinity && target
          && !/spin|load|progress|busy/i.test(`${className(target)} ${target.getAttribute("role") || ""}`);
      });
      const targets = [...new Set(looping.map((animation) => animation.effect.target))];
      if (targets.length >= 2) {
        blameNote(targets);
        notes.push(`${targets.length} animations loop forever (${list(targets.map(name), 3)}): constant motion distracts; keep at most one, slow and decorative, or stop it after a few cycles`);
      }
    } catch (_) { /* navigateur ancien */ }

    // Apparition au defilement qui ne se declenche jamais : Claw a fait
    // defiler toute la page avant de mesurer, et le bloc reste transparent.
    const revealWords = /reveal|fade|animate|aos|appear|in-?view|on-?scroll|observe/i;
    const moving = /carousel|slider|slide|swiper|tab|testimonial|modal|toast|tooltip|popover|menu|drawer|dropdown/i;
    const unseen = elements.filter((el) => {
      if (el.closest("nav, header, dialog, [role=dialog], [aria-hidden=true], [hidden], details:not([open]), [aria-modal]")) return false;
      const style = css(el);
      if (style.display === "none" || style.visibility === "hidden" || Number(style.opacity) > 0.05) return false;
      const marks = `${className(el)} ${[...el.attributes].map((attr) => attr.name).join(" ")}`;
      if (!revealWords.test(marks)) return false;
      for (let node = el; node && node !== document.body; node = node.parentElement) {
        if (moving.test(className(node))) return false;
      }
      const rect = el.getBoundingClientRect();
      return rect.width > 0 && rect.height > 0 && ((el.innerText || "").trim().length >= 20 || el.querySelector("img, picture, svg"));
    }).filter((el, index, all) => !all.some((other) => other !== el && other.contains(el)));
    if (unseen.length) {
      blame(unseen);
      problems.push(`${unseen.length} block(s) stay invisible after the whole page was scrolled (${list(unseen.map(name), 3)}): a reveal on scroll never fires, or never removes its opacity: 0. Content must show even if the script fails: hide it only under a class the script adds (html.js .reveal), and reveal it with an IntersectionObserver (the Component tool's reveal)`);
    }

    // Rythme vertical : les sections de même rang doivent partager leur
    // marge haute et basse (une échelle d'espacement), à 8px près.
    const sections = [...document.querySelectorAll("main > section, body > section, main > article, body > article")].filter(shown);
    if (sections.length >= 3) {
      const paddings = sections.map((el) => `${Math.round(parseFloat(css(el).paddingTop) / 8)}/${Math.round(parseFloat(css(el).paddingBottom) / 8)}`);
      const distinct = new Set(paddings);
      if (distinct.size > Math.max(2, Math.ceil(sections.length / 2))) {
        blameNote(sections);
        notes.push(`Sections have ${distinct.size} different vertical paddings for ${sections.length} sections (${sections.slice(0, 4).map((el, i) => `${name(el)}: ${Math.round(parseFloat(css(el).paddingTop))}/${Math.round(parseFloat(css(el).paddingBottom))}px`).join("; ")}): give sections of the same rank the same padding, from one spacing scale`);
      }
    }

    // Cartes identiques : plusieurs frères de même classe, même taille, même
    // fond, même ombre et même rayon, sans image, c'est la grille de gabarit.
    const seen = new Set();
    for (const el of visible) {
      const parent = el.parentElement;
      if (!parent || seen.has(parent)) continue;
      seen.add(parent);
      const kids = [...parent.children].filter((kid) => shown(kid) && kid.tagName !== "SCRIPT");
      if (kids.length < 3) continue;
      const first = css(kids[0]);
      const same = kids.every((kid) => {
        const style = css(kid);
        return kid.tagName === kids[0].tagName && kid.className === kids[0].className
          && style.backgroundColor === first.backgroundColor && style.boxShadow === first.boxShadow
          && style.borderRadius === first.borderRadius && Math.abs(box.get(kid).height - box.get(kids[0]).height) < 4;
      });
      const carded = (first.boxShadow !== "none" || parseFloat(first.borderRadius) >= 8) && first.backgroundColor !== "rgba(0, 0, 0, 0)";
      const pictures = kids.some((kid) => kid.querySelector("img, svg, picture, video"));
      if (same && carded && !pictures && kids.length >= 3 && kids[0].matches("div, article, li, section")) {
        blameNote([kids[0], parent]);
        notes.push(`${kids.length} identical cards in ${name(parent)} (same size, background, shadow and radius, no picture): a frequent tell of generated pages; vary the composition, give each its own illustration, or lay them out differently (a list, a split, an offset grid)`);
        break;
      }
    }

    // Petit trait sous chaque titre : la décoration de gabarit par excellence.
    const underlined = [...document.querySelectorAll("h1, h2, h3")].filter(shown).filter((h) => {
      const after = getComputedStyle(h, "::after");
      const height = parseFloat(after.height);
      return after.content !== "none" && after.content !== "normal" && height > 0 && height <= 6
        && parseFloat(after.width) >= 20 && after.backgroundColor !== "rgba(0, 0, 0, 0)";
    });
    if (underlined.length >= 2) blameNote(underlined);
    if (underlined.length >= 2) notes.push(`${underlined.length} headings carry a short decorative line underneath (${list(underlined.map(name), 3)}): a frequent tell of generated pages; the heading's size and weight should rank it on their own`);

    // La même photo plusieurs fois (image ou fond CSS) : chaque image doit
    // montrer autre chose. Vu dans un essai réel : une photo de cours en fond
    // du hero, dans la section du studio et à la place des portraits.
    const photoUses = new Map();
    const usePhoto = (src, el) => {
      if (!src || src.startsWith("data:") || !/\.(jpe?g|png|webp|avif)(\?|#|$)/i.test(src)) return;
      if (!photoUses.has(src)) photoUses.set(src, []);
      photoUses.get(src).push(el);
    };
    for (const img of document.querySelectorAll("img")) {
      if (shown(img) && img.naturalWidth >= 200) usePhoto(img.currentSrc || img.src, img);
    }
    for (const el of visible) {
      const found = css(el).backgroundImage.match(/url\("?([^")]+)"?\)/);
      if (found) usePhoto(found[1], el);
    }
    for (const [src, els] of photoUses) {
      if (els.length < 2) continue;
      blameNote(els);
      const file = decodeURIComponent(src.split(/[?#]/)[0].split("/").pop());
      notes.push(`the same photo (${file}) appears ${els.length} times (${list(els.map(name), 3)}): each picture should show something different (the room, a class, a detail, a person); use another photo, or an Ornament where no photo fits`);
    }

    // Accent omniprésent : la couleur la plus saturée couvre plus de 15 % de
    // la page, elle n'accentue plus rien.
    const sat = (color) => {
      const match = color.match(/rgba?\((\d+),\s*(\d+),\s*(\d+)(?:,\s*([\d.]+))?\)/);
      if (!match || (match[4] !== undefined && parseFloat(match[4]) < 0.5)) return null;
      const [r, g, b] = [match[1], match[2], match[3]].map(Number);
      const max = Math.max(r, g, b);
      const min = Math.min(r, g, b);
      return max === 0 ? 0 : (max - min) / max;
    };
    const area = new Map();
    const pageArea = Math.max(1, document.documentElement.scrollWidth * document.documentElement.scrollHeight);
    for (const el of visible) {
      const bg = css(el).backgroundColor;
      const s = sat(bg);
      if (s === null || s < 0.45) continue;
      const rect = box.get(el);
      if (rect.width * rect.height > pageArea * 0.9) continue;
      area.set(bg, (area.get(bg) || 0) + rect.width * rect.height);
    }
    const [loudest] = [...area.entries()].sort((a, b) => b[1] - a[1]);
    if (loudest && loudest[1] / pageArea > 0.15) {
      notes.push(`The saturated color ${loudest[0]} covers about ${Math.round(loudest[1] / pageArea * 100)}% of the page: an accent used that much no longer accents anything; keep it under a tenth (buttons, a key figure, one band), with quiet neutrals elsewhere`);
    }

    // Liens et boutons sans état de survol : Claw force `:hover` sur chacun
    // par le protocole du navigateur et compare son apparence. Ils sont
    // seulement marqués ici, avec leur nom pour la remarque.
    for (const el of [...document.querySelectorAll("a[href], button, [role=button]")].filter(shown).slice(0, 12)) {
      el.setAttribute("data-claw-hover", String(hoverLabels.length));
      hoverLabels.push(name(el));
    }
  }

  // Bandes vides : une hauteur de plus d'un demi-écran sans texte, image ni
  // champ, entre deux contenus. Les fonds et les marges ne comptent pas.
  {
    const spans = visible.filter((el) => ownText(el) || el.matches(media))
      .map((el) => {
        const rect = box.get(el);
        return [rect.top + window.scrollY, rect.bottom + window.scrollY, el];
      })
      .filter(([top, bottom]) => bottom > top)
      .sort((a, b) => a[0] - b[0]);
    const limit = window.innerHeight * 0.5;
    const gaps = [];
    let reach = spans.length ? spans[0][1] : 0;
    let before = spans.length ? spans[0][2] : null;
    for (const [top, bottom, el] of spans.slice(1)) {
      if (top - reach > limit) gaps.push({ from: reach, to: top, before, after: el });
      if (bottom > reach) { reach = bottom; before = el; }
    }
    if (gaps.length) {
      notes.push(`Empty bands taller than half a screen, with nothing to read or see (${gaps.slice(0, 3).map((gap) => `${Math.round(gap.to - gap.from)}px between ${name(gap.before)} and ${name(gap.after)}`).join("; ")}): tighten the padding and min-height there, or put content in the space`);
    }
  }

  // Cibles tactiles : 24px est le minimum légal, 44px le confort attendu.
  if (mobile) {
    const small = visible.filter((el) => el.matches("button, a[href], [role=button], input[type=submit], input[type=button]")
      && css(el).display !== "inline" && !el.closest("p, li > span, footer"))
      .filter((el) => (box.get(el).width < 44 || box.get(el).height < 44) && box.get(el).width >= 24 && box.get(el).height >= 24);
    if (small.length >= 2) blameNote(small);
    if (small.length >= 2) notes.push(`${small.length} buttons or links are under the comfortable 44x44px tap size on a phone (${list(small.map((el) => `${name(el)} ${Math.round(box.get(el).width)}x${Math.round(box.get(el).height)}px`), 3)}): give them more padding or a min-height of 44px`);
  }
  // Petits SVG de la page (et symboles d'un sprite) : Claw les compare au jeu
  // d'icônes, pour signaler une icône dessinée à la main.
  const svgIcons = [];
  for (const svg of document.querySelectorAll("svg, symbol")) {
    if (svgIcons.length >= 80) break;
    if (svg.tagName.toLowerCase() === "svg") {
      if (!shown(svg)) continue;
      const rect = svg.getBoundingClientRect();
      if (rect.width > 96 || rect.height > 96) continue;
    }
    const owner = svg.closest("a, button, label, li, p, h1, h2, h3, h4, span") || svg;
    svgIcons.push({ where: name(owner), markup: svg.outerHTML.slice(0, 6000) });
  }
  return { title: document.title || "", problems, notes, culprits, motion, hoverLabels, svgIcons, textLength: text.length };
})()
