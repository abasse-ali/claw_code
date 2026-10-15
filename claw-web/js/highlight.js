// Coloration du code de l'appli (blocs des réponses, fichiers, différences, commandes du
// journal), sans bibliothèque : la politique de sécurité de la page n'accepte que ses propres
// scripts. Chaque langage a quelques règles (commentaires, chaînes, nombres, mots-clés,
// fonctions, types, balises, attributs, clés) essayées dans l'ordre à chaque position. Le
// texte arrive brut ; le HTML rendu est échappé.

const ESCAPES = { "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" };
const escape = (text) => text.replace(/[&<>"']/g, (char) => ESCAPES[char]);
const words = (list) => new Set(list.trim().split(/\s+/));

const KEYWORDS = {
  js: words(`async await break case catch class const continue debugger default delete do else
    export extends finally for from function if import in instanceof let new of return static
    super switch throw try typeof var void while with yield as interface type enum implements
    private protected public readonly declare namespace abstract keyof satisfies`),
  py: words(`and as assert async await break class continue def del elif else except finally for
    from global if import in is lambda nonlocal not or pass raise return try while with yield
    match case`),
  rust: words(`as async await break const continue crate dyn else enum extern fn for if impl in let
    loop match mod move mut pub ref return static struct super trait type unsafe use where while`),
  go: words(`break case chan const continue default defer else fallthrough for func go goto if
    import interface map package range return select struct switch type var`),
  c: words(`auto break case catch char class const constexpr continue default delete do double else
    enum explicit extern final float for friend goto if inline int long namespace new noexcept
    operator override private protected public return short signed sizeof static struct switch
    template throw try typedef typename union unsigned using virtual void volatile while bool
    abstract assert boolean byte extends implements import instanceof interface native package
    super synchronized throws transient var val fun when object companion data sealed open
    internal lateinit is in suspend by init guard func extension protocol mutating late
    required dynamic async await yield foreach ref readonly lock event delegate string echo
    function use match fn trait`),
  sh: words(`if then else elif fi for while until do done case esac function in return local
    export readonly declare unset shift break continue exit source alias set trap select param
    foreach begin process end try catch finally throw`),
  sql: words(`select from where and or not insert into values update set delete create table index
    view drop alter add column primary key foreign references join left right inner outer full on
    group by order having limit offset distinct as union all case when then else end is in like
    between exists default unique check if begin commit rollback transaction returning asc desc
    integer text real blob varchar int boolean`),
};
const LITERALS = words("true false null undefined None True False nil NaN Infinity self Self this");

// Règles communes.
const BLOCK_COMMENT = /\/\*[\s\S]*?(?:\*\/|$)/y;
const LINE_COMMENT = /\/\/[^\n]*/y;
const HASH_COMMENT = /#[^\n]*/y;
const DOUBLE = /"(?:[^"\\\n]|\\.)*"?/y;
const SINGLE = /'(?:[^'\\\n]|\\.)*'?/y;
const NUMBER = /(?:0[xX][\da-fA-F_]+|0[bB][01_]+|0[oO][0-7_]+|\d[\d_]*(?:\.\d[\d_]*)?(?:[eE][+-]?\d+)?)[a-zA-Z_\d]*/y;
const SPACE = /\s+/y;
const DECORATOR = /@[A-Za-z_][\w.]*/y;

/** Un identifiant : mot-clé, valeur littérale, type (majuscule) ou rien. */
function word(keywords) {
  return (text) => {
    if (keywords.has(text)) return "kw";
    if (LITERALS.has(text)) return "var";
    if (/^[A-Z][a-z0-9]/.test(text) || /^[A-Z][A-Z0-9_]*[a-z]/.test(text)) return "type";
    return null;
  };
}

function cLike(keywords, extra = []) {
  return [
    ["com", LINE_COMMENT],
    ["com", BLOCK_COMMENT],
    ...extra,
    ["str", DOUBLE],
    ["str", SINGLE],
    ["num", NUMBER],
    ["var", DECORATOR],
    ["fn", /[A-Za-z_$][\w$]*(?=\s*\()/y],
    [word(keywords), /[A-Za-z_$][\w$]*/y],
    [null, SPACE],
  ];
}

const RULES = {
  js: cLike(KEYWORDS.js, [["str", /`(?:[^`\\]|\\[\s\S])*`?/y], ["str", /\/(?![*/\s])(?:[^/\\\n[]|\\.|\[(?:[^\]\\\n]|\\.)*\])+\/[dgimsuyv]*(?=\s*[.,;)\]}]|\s*$)/y]]),
  rust: [
    ["com", LINE_COMMENT],
    ["com", BLOCK_COMMENT],
    ["str", /b?r(#*)"[\s\S]*?(?:"\1|$)/y],
    ["str", /b?"(?:[^"\\]|\\[\s\S])*"?/y],
    ["str", /b?'(?:\\.|[^'\\\n])'/y],
    ["var", /'[a-z_]\w*/y],
    ["num", NUMBER],
    ["var", /#!?\[[^\]\n]*\]/y],
    ["fn", /[a-z_]\w*!(?=\s*[([{])/y],
    ["fn", /[A-Za-z_]\w*(?=\s*(?:::<[^>]*>)?\s*\()/y],
    [word(KEYWORDS.rust), /[A-Za-z_]\w*/y],
    [null, SPACE],
  ],
  go: cLike(KEYWORDS.go, [["str", /`[^`]*`?/y]]),
  c: cLike(KEYWORDS.c, [["var", /#\s*[a-z]+/y], ["str", /"""[\s\S]*?(?:"""|$)/y], ["str", /\$"(?:[^"\\\n]|\\.)*"?/y], ["var", /\$[A-Za-z_]\w*/y]]),
  py: [
    ["com", HASH_COMMENT],
    ["str", /[rRbBuUfF]{0,2}("""|''')[\s\S]*?(?:\1|$)/y],
    ["str", /[rRbBuUfF]{0,2}"(?:[^"\\\n]|\\.)*"?/y],
    ["str", /[rRbBuUfF]{0,2}'(?:[^'\\\n]|\\.)*'?/y],
    ["num", NUMBER],
    ["var", DECORATOR],
    ["fn", /[A-Za-z_]\w*(?=\s*\()/y],
    [word(KEYWORDS.py), /[A-Za-z_]\w*/y],
    [null, SPACE],
  ],
  sh: [
    ["com", /<#[\s\S]*?(?:#>|$)/y],
    ["com", /(?<![\w$#{])#[^\n]*/y],
    ["str", /"(?:[^"\\]|\\[\s\S])*"?/y],
    ["str", /'[^']*'?/y],
    ["var", /\$\{[^}\n]*\}?|\$\(?[A-Za-z_][\w:]*|\$[0-9@#?*!$_-]/y],
    ["attr", /(?<=\s)--?[A-Za-z][\w-]*/y],
    ["fn", /\b[A-Z][a-z]+-[A-Z][A-Za-z]+/y],
    ["num", /\b\d+(?:\.\d+)?\b/y],
    [word(KEYWORDS.sh), /[A-Za-z_][\w-]*/y],
    [null, SPACE],
  ],
  sql: [
    ["com", /--[^\n]*/y],
    ["com", BLOCK_COMMENT],
    ["str", /'(?:[^']|'')*'?/y],
    ["str", /"[^"\n]*"?/y],
    ["num", NUMBER],
    ["fn", /[A-Za-z_]\w*(?=\s*\()/y],
    [(text) => (KEYWORDS.sql.has(text.toLowerCase()) ? "kw" : LITERALS.has(text) ? "var" : null), /[A-Za-z_]\w*/y],
    [null, SPACE],
  ],
  css: [
    ["com", BLOCK_COMMENT],
    ["str", DOUBLE],
    ["str", SINGLE],
    ["kw", /@[\w-]+/y],
    ["num", /#[\da-fA-F]{3,8}\b/y],
    ["attr", /(?<=(?:^|[{;])\s*)--?[A-Za-z][\w-]*(?=\s*:)/my],
    ["type", /[.#][A-Za-z_][\w-]*/y],
    ["num", /-?(?:\d+\.?\d*|\.\d+)(?:px|em|rem|%|vh|vw|vmin|vmax|dvh|s|ms|deg|fr|ch|ex|pt)?/y],
    ["fn", /[A-Za-z-]+(?=\()/y],
    ["kw", /!important\b/y],
    [null, /[A-Za-z_][\w-]*/y],
    [null, SPACE],
  ],
  html: [
    ["com", /<!--[\s\S]*?(?:-->|$)/y],
    ["kw", /<![^>]*>?/y],
    [htmlTag, /<\/?[A-Za-z][\w:.-]*(?:[^>"']|"[^"]*"|'[^']*')*>?/y],
    ["num", /&#?\w+;/y],
    [null, /[^<&]+/y],
  ],
  json: [
    ["attr", /"(?:[^"\\\n]|\\.)*"(?=\s*:)/y],
    ["str", DOUBLE],
    ["num", /-?\d+(?:\.\d+)?(?:[eE][+-]?\d+)?/y],
    ["kw", /\b(?:true|false|null)\b/y],
    ["com", LINE_COMMENT],
    ["com", BLOCK_COMMENT],
    [null, SPACE],
  ],
  yaml: [
    ["com", /(?<=^|\s)#[^\n]*/my],
    ["attr", /(?<=^[ \t]*(?:- )?)(?!- )[^\s:#][^:\n#]*?(?=:(?:\s|$))/my],
    ["str", DOUBLE],
    ["str", SINGLE],
    ["var", /[&*][\w-]+/y],
    ["kw", /\b(?:true|false|yes|no|null|on|off)\b|~/y],
    ["num", /-?\b\d+(?:\.\d+)?\b/y],
    [null, SPACE],
  ],
  toml: [
    ["com", /(?<=^|\s)[#;][^\n]*/my],
    ["type", /^[ \t]*\[\[?[^\]\n]*\]\]?/my],
    ["attr", /^[ \t]*[\w.\-"']+(?=\s*=)/my],
    ["str", /"""[\s\S]*?(?:"""|$)/y],
    ["str", DOUBLE],
    ["str", SINGLE],
    ["kw", /\b(?:true|false)\b/y],
    ["num", /-?\b\d[\d_]*(?:\.\d+)?\b/y],
    [null, SPACE],
  ],
  diff: [
    ["kw", /^@@[^\n]*/my],
    ["com", /^(?:diff|index|---|\+\+\+)[^\n]*/my],
    ["add", /^\+[^\n]*/my],
    ["del", /^-[^\n]*/my],
    [null, /[^\n]*\n?/y],
  ],
  md: [
    ["kw", /^#{1,6} [^\n]*/my],
    ["str", /`[^`\n]+`/y],
    ["fn", /\[[^\]\n]*\]\([^)\n]*\)/y],
    ["var", /^\s*(?:[-*+]|\d+\.)(?=\s)/my],
    [null, /[^`[#\n-]+/y],
  ],
};

/** Une balise HTML découpée : nom, attributs, valeurs. */
function htmlTag(text) {
  const tokens = [];
  const pattern = /(<\/?)([\w:.-]+)|([\w:.@-]+)(?=\s*=)|("[^"]*"|'[^']*')|(\/?>)|([\w:.@-]+)|(\s+|=|[^\s])/g;
  for (const match of text.matchAll(pattern)) {
    if (match[1]) tokens.push([null, match[1]], ["tag", match[2]]);
    else if (match[3]) tokens.push(["attr", match[3]]);
    else if (match[4]) tokens.push(["str", match[4]]);
    else if (match[5]) tokens.push([null, match[5]]);
    else if (match[6]) tokens.push(["attr", match[6]]);
    else tokens.push([null, match[7]]);
  }
  return tokens;
}

const ALIASES = {
  js: "js", jsx: "js", mjs: "js", cjs: "js", javascript: "js", ts: "js", tsx: "js", mts: "js",
  typescript: "js", node: "js",
  py: "py", python: "py", py3: "py", pyw: "py", python3: "py",
  rs: "rust", rust: "rust",
  go: "go", golang: "go",
  c: "c", h: "c", cpp: "c", cc: "c", cxx: "c", hpp: "c", "c++": "c", cs: "c", csharp: "c",
  java: "c", kt: "c", kts: "c", kotlin: "c", swift: "c", dart: "c", php: "c", scala: "c",
  groovy: "c", gradle: "c",
  sh: "sh", bash: "sh", zsh: "sh", shell: "sh", console: "sh", shellsession: "sh", ps1: "sh",
  powershell: "sh", pwsh: "sh", bat: "sh", cmd: "sh", dockerfile: "sh", makefile: "sh",
  sql: "sql", sqlite: "sql", psql: "sql", mysql: "sql", postgresql: "sql",
  css: "css", scss: "css", sass: "css", less: "css",
  html: "html", htm: "html", xml: "html", svg: "html", vue: "html", svelte: "html",
  xhtml: "html", astro: "html",
  json: "json", jsonc: "json", json5: "json", geojson: "json", webmanifest: "json",
  yaml: "yaml", yml: "yaml",
  toml: "toml", ini: "toml", cfg: "toml", conf: "toml", properties: "toml", env: "toml",
  diff: "diff", patch: "diff",
  md: "md", markdown: "md",
};

/** Le langage d'un bloc (étiquette après ```) ou d'un fichier (nom ou chemin) ; "" sinon. */
export function languageOf(nameOrTag) {
  const text = String(nameOrTag || "").trim().toLowerCase();
  if (!text) return "";
  const base = text.split(/[\\/]/).pop();
  if (base === "dockerfile" || base.startsWith("dockerfile.") || base === "makefile") return "sh";
  if (base === ".env" || base.startsWith(".env.")) return "toml";
  const key = base.includes(".") ? base.split(".").pop() : base;
  return ALIASES[key] || "";
}

/** Le texte en morceaux `[classe, texte]` (classe nulle : texte sans couleur). */
function tokens(code, language) {
  const rules = RULES[language];
  if (!rules) return [[null, code]];
  const out = [];
  let plain = "";
  let index = 0;
  const flush = () => {
    if (plain) out.push([null, plain]);
    plain = "";
  };
  scan: while (index < code.length) {
    for (const [kind, pattern] of rules) {
      pattern.lastIndex = index;
      const match = pattern.exec(code);
      if (!match || !match[0]) continue;
      const text = match[0];
      if (kind === htmlTag) {
        flush();
        out.push(...htmlTag(text));
      } else {
        const name = typeof kind === "function" ? kind(text) : kind;
        if (name) {
          flush();
          out.push([name, text]);
        } else {
          plain += text;
        }
      }
      index += text.length;
      continue scan;
    }
    plain += code[index];
    index += 1;
  }
  flush();
  return out;
}

const span = (kind, text) => (kind ? `<span class="tok-${kind}">${escape(text)}</span>` : escape(text));

/** Le code coloré, en HTML échappé. */
export function highlight(code, language) {
  return tokens(String(code || ""), language).map(([kind, text]) => span(kind, text)).join("");
}

/** Le code coloré, ligne par ligne (une couleur qui court sur plusieurs lignes est reprise à
 * chacune : commentaire de bloc, chaîne sur plusieurs lignes). */
export function highlightLines(code, language) {
  const lines = [""];
  for (const [kind, text] of tokens(String(code || ""), language)) {
    const parts = text.split("\n");
    parts.forEach((part, index) => {
      if (index) lines.push("");
      if (part) lines[lines.length - 1] += span(kind, part);
    });
  }
  return lines;
}
