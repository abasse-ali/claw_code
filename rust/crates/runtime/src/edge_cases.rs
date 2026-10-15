//! Cas limites qu'un ingénieur expérimenté vérifie d'office pour un genre de
//! fonctionnalité. La demande nomme le genre (« une recherche », « le solde »,
//! « jours ouvrés ») ; Claw rappelle au modèle les pièges précis de ce genre et
//! l'essai qui les montre : guide du début de tour, contrôle des exigences ou
//! des preuves, relecture. Un modèle gratuit les oublie : dans l'essai réel t5,
//! une recherche ignorait les accents (« creme » pour « crème »), le `+` d'une
//! URL et une recherche faite d'espaces ; dans t6, une période à l'envers était
//! comptée et un solde passait sous zéro.
//!
//! Chaque ligne nomme le piège et l'essai à faire (T1-27 : nommer seulement la
//! catégorie ne changeait rien). Les faits cités ont été rejoués sur la machine
//! de l'utilisateur (Python 3.13 sous Windows, Pillow 10, `OpenCV` 4, Node 24).
//! Chaque essai porte son résultat attendu entre parenthèses : sans lui, au
//! rejeu de t6, le modèle a fait les essais qui confirmaient son code et gardé
//! l'échange automatique des dates d'une période à l'envers.

/// Les genres de fonctionnalité : les mots qui les reconnaissent (débuts de
/// mots, en minuscules et sans accents ; un mot suivi d'une espace est un mot
/// entier, voir [`crate::team::has_word_start`]) et ce qu'il faut vérifier. Au
/// plus [`MAX_FEATURE_EDGE_CASES`] genres sont gardés, dans l'ordre de la
/// table : les genres aux mots précis d'abord, les plus larges ensuite, la
/// ligne de commande en dernier.
const FEATURE_EDGE_CASES: &[(&[&str], &str)] = &[
    (
        &[
            "une recherche", "la recherche", "de recherche", "recherche par", "rechercher par",
            "recherches", "permet de rechercher", "pouvoir rechercher", "permet de chercher",
            "pouvoir chercher", "peut chercher", "`chercher", "`recherche", "`search", "`filtre",
            "filtrer les", "filtre les", "filtrer la liste", "filtre la liste", "filtrer par",
            "filtre par", "filtrage par", "pouvoir filtrer", "permet de filtrer",
            "insensible a la casse", "sans tenir compte des majuscules",
            "sans tenir compte des accents", "sans tenir compte de la casse", "ignorer la casse",
            "ignorer les accents", "autocompl", "search bar", "search box", "search field",
            "search feature", "search page", "search results", "search endpoint", "search by",
            "searchable", "add search", "add a search", "full-text search", "full text search",
            "fuzzy search", "filter by", "filter the list", "filter the results",
            "case-insensitive", "case insensitive", "accent-insensitive", "diacritic",
        ],
        "Text search or filter: people type \"creme\" for \"Crème\" and \"oeufs\" for \"Œufs\", so compare without case and accents (Python: `unicodedata.normalize(\"NFD\", s)`, drop the combining marks, `casefold()`, then replace \"œ\" by \"oe\", which NFD keeps; JavaScript: `s.normalize(\"NFD\").replace(/[\\u0300-\\u036f]/g, \"\").toLowerCase()`); strip surrounding spaces, so that \"  \" is an empty query and shows everything; accept several words; read the query with the standard parser, where `+` and `%20` both mean a space: `parse_qs(urlsplit(self.path).query, keep_blank_values=True).get(\"q\", [\"\"])[0]` (without `keep_blank_values`, `?q=` gives `{}` and `[\"q\"]` raises KeyError) or `(params.get(\"q\") ?? \"\").trim()`, never `split(\"=\")`. Try \"creme\" (finds \"Crème\"), \"oeufs\" (finds \"Œufs\"), \"  tarte  \" (same as \"tarte\"), `?q=` (everything) and `?q=pommes+de+terre` (the three words).",
    ),
    (
        &[
            "prix", "montant", "tva", "euro ", "euros ", "€", "facture", "paiement", "payer",
            "tarif", "panier", "devis", "remise de", "remise sur", "rabais", "code promo", "taxe",
            "ttc", "hors taxe", "centime ", "centimes ", "frais de", "depense", "monnaie",
            "salaire", "rembours", "achat", "vente", "ticket de caisse", "price", "pricing",
            "invoice", "payment", "discount", "refund", "currency", "billing", "subtotal", "salary",
            "payroll", "shopping cart",
        ],
        "Money: compute amounts in integer cents or in `Decimal` built from strings, never in floats (`0.1 * 3 == 0.3` is false; `Decimal(0.1)` is already wrong), but keep what existing functions return: callers expect it, and `json.dumps` raises TypeError on a `Decimal` (convert where values leave your code); round with `quantize(Decimal(\"0.01\"), rounding=ROUND_HALF_UP)`, not `round()` (`round(2.675, 2)` gives 2.67; the default quantize turns 2.665 into 2.66); make the printed lines add up to the printed total; accept \"12,50\" as well as \"12.50\" (`float(\"12,50\")` raises, JavaScript's `parseFloat` gives 12). Try three lines of 0.10 (0.30 exactly), 100 split in three (33.33 + 33.33 + 33.34), 2.665 (2.67), a zero, a negative amount, and `json.dumps` of what your functions return (no TypeError).",
    ),
    (
        &[
            "date", "heures de", "heures d'", "heures sup", "heures ouvr", "heures travaill",
            "toutes les heures", "nombre d'heures", "heure de", "heure d'", "heure du", "heure en",
            "heure locale", "quelle heure", "horaires", "horaire de", "horaire d'", "plage horaire",
            "creneau", "decalage horaire", "calendrier", "echeance", "fuseau", "europe/",
            "jour ouvr", "jours ouvr", "ferie", "conge ", "conges ", "planning", "agenda",
            "rendez-vous", "rdv", "anniversaire", "mensuel", "hebdomadaire", "horodat", "timestamp",
            "strftime", "strptime", "jj/mm", "dd/mm", "iso 8601", "iso8601", "utc", "calendar",
            "deadline", "timezone", "time zone", "schedul", "booking", "appointment",
            "business day", "working day", "weekday", "holiday", "leap year", "daylight",
            "delai de paiement", "delai de livraison",
        ],
        "Dates and times: \"03/04/2026\" is 3 April for a French user, but JavaScript's `new Date(\"03/04/2026\")`, and `dateutil` or `pandas` without `dayfirst=True`, read 4 March: parse with `datetime.strptime(text.strip(), \"%d/%m/%Y\")` and refuse an impossible date (31/04 raises ValueError) with a clear message; refuse an end before the start with a message, since it is a typing mistake: never count it (0, a negative number) nor swap the dates silently; `(end - start).days` gives 4 from Monday to Friday: add 1 when the last day counts; mixing datetimes with and without a time zone raises TypeError on `<` and is silently unequal on `==`; 29 March 2026 lasts 23 hours in Paris (compute durations in UTC), and on Windows `ZoneInfo(\"Europe/Paris\")` needs the `tzdata` package. Try 31/04/2026 (refused), 06/03/2026 to 02/03/2026 (refused: not 0, -4 or 5), 02/03/2026 to 02/03/2026 (1 day), 29/02/2027 (refused) and 29/02/2028 (accepted).",
    ),
    (
        &[
            "csv", "tableur", "xls", "spreadsheet", "excel ", "classeur", "feuille de calcul",
            "feuilles de calcul", "openpyxl", "tsv", "libreoffice", "separateur decimal",
            "separateur de milliers", "virgule decimale",
        ],
        "CSV and Excel files from French users: `;` as separator (use the `csv` module: quoted fields may contain it), decimal comma, and thousands separators that are a plain space or a non-breaking space (U+00A0, or U+202F from current Windows and Excel) that `replace(\" \", \"\")` leaves: remove them with `re.sub(r\"\\s\", \"\", value)`; read with `encoding=\"utf-8-sig\"` (with \"utf-8\", a BOM turns the first header into \"\\ufeffDate\" and `row[\"Date\"]` raises KeyError), fall back to cp1252, never latin-1 (\"€\" silently becomes \"\\x80\"); write for Excel with `newline=\"\"` (else a blank row after each line on Windows) and `encoding=\"utf-8-sig\"`. Try \"1\\u00a0250,00\" and \"1\\u202f250,00\" (Python notation; both 1250.0), a file with a BOM and one in cp1252 (the same rows). Keep the user's file as it is and fix the code that reads it.",
    ),
    (
        &[
            "pagination", "paginer", "pagine", "paginate", "paging", "per page", "per_page",
            "page_size", "page-size", "pagesize", "page size", "page=", "taille de page",
            "page suivante", "page precedente", "next page", "previous page", "resultats par page",
            "elements par page", "lignes par page", "articles par page", "produits par page",
            "entrees par page", "items par page", "recettes par page", "notes par page",
            "livres par page", "scroll infini", "defilement infini", "infinite scroll", "load more",
        ],
        "Pagination: a negative page must not return other items (with `items[(page - 1) * size : page * size]` and a size of 10, page -1 is `items[-20:-10]`, ten real items); the page count is `-(-count // size)`, not `count // size`; refuse a page size of 0 or less and cap the size a client can ask for (SQLite `LIMIT -1` returns every row); sort on a unique key, or items repeat or vanish between pages. Try 25 items, 10 per page (3 pages, the last with 5 items), then pages 0, -1 and 4 (an empty list or the project's error, never other items).",
    ),
    (
        &[
            "formulaire", "saisie", "saisir", "form ", "forms ", "<form", "<input", "formdata",
            "form-data", "champ de saisie", "champs de saisie", "zone de saisie", "case a cocher",
            "cases a cocher", "liste deroulante", "bouton envoyer", "textarea", "checkbox",
            "dropdown", "input field",
        ],
        "Forms and user input: the HTML `required` attribute accepts a field of spaces only and `type=\"email\"` accepts \"a@b\", so check again on the server after `strip()` (anyone can send a request without the page); show each error next to its field and keep what the user typed; show user text with `textContent`, never `innerHTML`; in a number field, JavaScript's `Number(\"\")` is 0 and `parseFloat(\"12,5\")` is 12. Try an empty required field and one of spaces only (both refused, next to the field), a value that is too long (refused), \"Zoé\", \"O'Brien\" and \"O’Brien\" (U+2019, as phones type it; all three accepted and shown as typed).",
    ),
    (
        &[
            "upload", "televers", "piece jointe", "pieces jointes", "attachments",
            "email attachment", "attach a file", "attach files", "multipart", "glisser-deposer",
            "glisser deposer", "glisser et deposer", "drag and drop", "drag-and-drop",
            "drag & drop", "dropzone", "type=\"file\"", "type=file", "type='file'",
            "envoyer un fichier", "envoyer une image", "envoyer des fichiers", "envoyer des images",
            "envoyer une photo", "envoyer des photos", "importer une image", "importer des images",
            "importer des photos", "deposer une image", "deposer des images", "deposer un fichier",
            "deposer des fichiers",
        ],
        "File uploads: never build a path from the uploaded name: \"../\" and, on Windows, \"..\\\\\" (Python notation) escape the folder, and `os.path.join(\"uploads\", \"C:\\\\x.py\")` is \"C:\\\\x.py\"; keep only `os.path.basename` of it or generate the name (an ASCII-only cleaner turns \"漫画.png\" into \"png\"); check the type on the first bytes or by opening the image, not on the extension; on Python 3.13, `cgi.FieldStorage` and `imghdr` no longer exist. Try a file over the size limit (refused), \"../x.txt\" (kept inside the folder or refused), a name with spaces and accents (\"photo été.jpg\", kept or safely renamed) and the same file sent twice (no other file overwritten).",
    ),
    (
        &[
            "mot de passe", "mots de passe", "mdp", "page de connexion", "ecran de connexion",
            "formulaire de connexion", "bouton de connexion", "systeme de connexion",
            "connexion utilisateur", "connexion des utilisateurs", "connexion au compte",
            "utilisateur connecte", "utilisateurs connectes", "compte utilisateur",
            "comptes utilisateurs", "creer un compte", "creation de compte", "creation d'un compte",
            "page d'inscription", "formulaire d'inscription", "inscription des utilisateurs",
            "inscription d'un utilisateur", "authentif", "authentica", "login", "password",
            "sign in", "sign-in", "sign up", "sign-up", "signup", "user account", "jwt", "oauth",
            "bcrypt", "argon2",
        ],
        "Accounts and passwords: hash the passwords users choose with a slow salted hash (bcrypt or argon2 if installed, else `hashlib.scrypt` with a salt from `secrets.token_bytes(16)`), never a plain SHA, and never store or log them in clear; the passwords the program itself sends (database, SMTP, API keys) come from the environment, never from the code; give the same error for an unknown user and a wrong password; compare tokens with `hmac.compare_digest`; make sessions expire. Try a wrong password and an unknown user (the same message) and an expired session (refused).",
    ),
    (
        &[
            "trier", "trie la", "trie les", "trie le", "triez", "triee", "triees", "tri par",
            "tri des", "tri du", "tri de", "tri alphabetique", "tri croissant", "tri decroissant",
            "ordre alphabetique", "ordre croissant", "ordre decroissant", "ordre chronologique",
            "classer par", "classement", "plus recent au plus", "plus ancien au plus",
            "plus recents d'abord", "plus recents en premier", "plus grand au plus petit",
            "plus petit au plus grand", "sorted", "sorting", "sort by", "sort the", "sort them",
            "sort order", "sort key", "sort(", "sort_values", "sort_by", "order by", "alphabetical",
            "ascending", "descending",
        ],
        "Sorting text: Python's `sorted()` puts \"Écrire\" after \"Zèbre\" and \"b\" after \"Z\", so sort on a key without accents and case that also turns \"œ\" into \"oe\", or on `locale.strxfrm` after setting a French LC_COLLATE; in JavaScript, `a.localeCompare(b, \"fr\")`; \"10\" comes before \"9\" as text: `sorted([\"page10.png\", \"page2.png\"])` puts page10 first (sort on the number in the name), and JavaScript's `[10, 9, 1].sort()` gives [1, 10, 9] (pass `(a, b) => a - b`); equal items keep their order. Try \"Zèbre\", \"écrire\", \"Banane\" and \"Œuf\" (Banane, écrire, Œuf, Zèbre), then \"page10\" and \"page2\" (page2 first).",
    ),
    (
        &[
            "solde", "stock ", "stocks ", "rupture de stock", "inventaire", "inventory", "quota",
            "budget", "conge ", "conges ", "rtt ", "reliquat", "cagnotte", "portefeuille",
            "porte-monnaie", "points de fidelite", "places disponibles", "places restantes",
            "reste des places", "restent des places", "nombre de places", "places limitees",
            "capacite maximale", "liste d'attente", "file d'attente", "debiter", "account balance",
            "leave balance", "balances", "allowance", "wallet", "overdraft", "reservation",
            "reserver", "booking", "emprunt",
        ],
        "Balances, stocks, quotas and bookings: check before you subtract (`balance -= days` then a save lets a balance of 2 reach -3): unless the request allows going below zero, refuse a request larger than what is left, with a message saying what is left and a failure status, and leave the balance and the saved file unchanged; refuse a zero or negative quantity first (subtracting -4 adds 4, which a reversed date range counted as -4 days does); one person cannot book or queue twice for the same item, nor reserve what they already hold; a cancellation gives back exactly once. Try exactly what is left (accepted, leaves 0), one more (refused, balance and file unchanged), 0 and -1 (refused) and the same booking twice (refused the second time), then reread the saved file.",
    ),
    (
        &[
            "doublon", "dedoublon", "en double", "deux fois", "duplicate", "dedup", "existe deja",
            "deja existant", "already exists", "unicite",
        ],
        "Duplicates and unique keys: compare names and e-mails after `strip()`, `casefold()` and `unicodedata.normalize(\"NFC\", ...)`, or \"Zoé\", \" zoé \" and a decomposed \"zoé\" count as three people; that folded form is only for comparing: store and show each name as the user first wrote it (\"Zoé\", not \"zoé\"); enforce uniqueness where the data is written, since two requests or a double click can both pass an earlier check; running an import twice must not double the data; `set()` loses the order, `dict.fromkeys` keeps it. Try \"Zoé\" then \" zoé \" (one person, still shown as \"Zoé\"; the decomposed form \"Zoe\\u0301\" belongs in a unit test, the shell does not read that escape), a double submit (one record) and the same import run twice (no new rows).",
    ),
    (
        &[
            "adresse mail", "adresses mail", "adresse e-mail", "adresses e-mail", "adresse email",
            "adresses email", "courriel", "numero de telephone", "numeros de telephone",
            "telephone ", "email address", "e-mail address", "phone number",
        ],
        "E-mail addresses and phone numbers: a strict regex rejects valid addresses (\"prenom.nom+tag@sous-domaine.fr\", capitals, new endings such as .paris) and a loose one accepts \"a@b\": check for one @, a dotted domain and no spaces, then confirm by sending a mail; trim, and compare addresses in lowercase; store phone numbers as text, since `int(\"0612345678\")` loses the 0, normalized to +33612345678 from \"06 12 34 56 78\", \"06.12.34.56.78\" or \"+33 6 12 34 56 78\". Try \"Prenom.Nom+tag@sous-domaine.fr\" (accepted), \"a@b\" and \"a b@c.fr\" (refused), and each phone form above (stored as +33612345678).",
    ),
    (
        &[
            "sql", "base de donnee", "bases de donnee", "bdd", "database", "postgres", "mysql",
            "sqlalchemy",
        ],
        "SQL databases: never put values into the SQL text (`\"... '%s'\" % name` breaks on \"O'Brien\" and allows injection): use placeholders (`?`); sqlite3 loses changes not committed before `close()` (use `with conn:`, which commits); SQLite ignores foreign keys unless `PRAGMA foreign_keys = ON` on each connection, so deleting a parent leaves orphans; its `LIKE` and `lower()` only fold ASCII letters, so \"É\" never matches \"é\"; use one connection per thread. Try \"O'Brien\" (saved and found), a restart after a write (still there), deleting a parent (refused or cascaded, no orphans) and \"école\" against \"École\" (found when the search ignores case).",
    ),
    (
        &[
            "cle api", "cles api", "cle d'api", "cles d'api", "api key", "api_key", "apikey",
            "secret", ".env", "credential",
        ],
        "API keys and secrets: keep keys out of the code and out of Git (`.env` listed in `.gitignore`, a `.env.example` without values); read them from the environment and stop with a clear message when one is missing; never print or log them, nor a full request or URL that carries them; in Next.js every `NEXT_PUBLIC_` variable, and in Vite every `VITE_` one, is copied into the page that any visitor downloads. Try `git status` (no .env listed), a run without the key (one clear message) and a search for the key in the logs and the built files (not found).",
    ),
    (
        &[
            "ocr", "tradu", "translat", "deepl", "bulle de", "bulles de", "speech bubble",
            "tesseract", "easyocr",
        ],
        "OCR and translation: skip boxes whose OCR text is empty or blank instead of sending them to the translator; manga-ocr removes every space and line break and makes ASCII full-width, and EasyOCR returns separate lines: rebuild each bubble's text in reading order (right to left for manga, top to bottom for webtoons); `easyocr.Reader([\"ja\", \"ko\"])` raises, so use one reader per language; keep placeholders and tags (`{name}`, `<b>`) intact; Pillow's default font draws \"é\" and \"ç\" as boxes: typeset with a TTF font that has them. Try an empty bubble (skipped), a two-line one (one sentence) and \"Été, ça, cœur\" typeset (accents drawn).",
    ),
    (
        &[
            "image", "photo", "png", "jpg", "jpeg", "webp", "pillow", "opencv", "cv2", "miniature",
            "thumbnail", "redimension", "resize", "scans", "scan ", "scanne", "webtoon",
        ],
        "Images: Pillow 10 removed `ImageDraw.textsize`, `getsize` and `Image.ANTIALIAS` (use `textbbox` and `Image.LANCZOS`); an image with transparency (mode RGBA or P) cannot be saved as JPEG until `convert(\"RGB\")`; on Windows, `cv2.imread` returns None, without an exception, when the path has an accent (read with `cv2.imdecode(np.fromfile(path, np.uint8), cv2.IMREAD_COLOR)`), and OpenCV gives BGR, not RGB; phone photos need `ImageOps.exif_transpose`; JPEG stops at 65500 px and WebP at 16383 px. Try a transparent PNG saved as JPEG (works after the conversion), \"été.png\" (read), an empty file (a clear error) and a 70000 px tall strip (cut, or saved as PNG).",
    ),
    (
        &[
            "url", "requests", "httpx", "urllib", "api externe", "external api", "scrap", "deepl",
            "openai", "openrouter", "webhook", "appel a l'api", "appels a l'api", "appeler l'api",
            "appelle l'api",
        ],
        "Calling an external service: `requests` waits forever without `timeout=`, and httpx gives up after 5 s, too short for a translation or an LLM; a 4xx or 5xx raises nothing until `raise_for_status()`, and `.json()` on an HTML error page raises JSONDecodeError; retry only timeouts, 429 and 5xx, with a growing wait (honor Retry-After), never 400 or 401; check each key you read from the answer; keep API keys in environment variables, out of the code and the logs. Try a timeout, a 500, a 429 and an unexpected body with a mock (each a retry or a clear error, never a hang or a traceback).",
    ),
    (
        &[
            "frontend", "front-end", "front end", "backend", "back-end", "back end", "cors",
            "localhost", "hors ligne", "offline", "network error", "docker", "next.js", "nextjs",
        ],
        "Front end and back end: the page runs in the user's browser, so an API address baked into it (`http://localhost:8000`, `NEXT_PUBLIC_API_URL=http://backend:8000`, a Docker service name) breaks elsewhere: when one server serves both, call relative URLs (`/api/...`); a page on port 3000 calling port 8000 needs CORS headers, which curl never shows; a static build (`next build`, `out/`) shows a change only after rebuilding, and `output: 'export'` ignores rewrites. Try the page itself with ViewPage, not only curl (the data shown, no error in its console).",
    ),
    (
        &[
            "api", "endpoint", "route", "fastapi", "flask", "django", "http.server",
        ],
        "JSON API endpoints: every error answers JSON with the right status, never a 500 traceback or an HTML page: a 4xx with a message for a body that is not JSON (`json.loads` raises), a missing field or a wrong type (\"3\" for 3); 404 for an unknown id, but 200 with [] for a search that finds nothing; with `http.server`, read the body with `self.rfile.read(int(self.headers[\"Content-Length\"]))`, since `rfile.read()` waits forever. Try invalid JSON, an empty body, a missing field and a wrong type (each a 4xx with a JSON message) and an unknown id (404), checking the status and the body.",
    ),
    (
        &[
            "affichage", "s'affich", "mise en page", "responsive", "sur mobile", "version mobile",
            "sur telephone", "sur le telephone", "sur mon telephone", "sur un telephone",
            "sur iphone", "sur mon iphone", "sur android", "petit ecran", "petits ecrans",
            "deborde", "debordement", "chevauch", "se superpos", "tronque", "mal aligne",
            "alignement", "decale", "css", "layout", "overflow", "screenshot", "capture d'ecran",
            "captures d'ecran", "flou", "illisible", "on mobile", "on a phone", "on my phone",
            "mobile view", "display issue", "display problem", "looks broken", "looks wrong",
        ],
        "Page layout: a page that works can still look broken on the user's screen: content wider than a phone (a fixed `width`, a long word or URL, a table, a `pre`), a label or a tab cut by `overflow: hidden`, elements drawn on top of each other (absolute positions, negative margins), an element with the `hidden` attribute shown because a CSS `display` rule wins over it, text too small or too pale. The usual fixes: `max-width: 100%`, `min-width: 0` on flex children, `flex-wrap: wrap`, `overflow-wrap: anywhere`, `[hidden] { display: none !important; }`. Try the page with Screenshot at phone and desktop sizes after each change of its look, with `actions` to open its menus and dialogs (no problem listed, and images that look right at both sizes).",
    ),
    (
        &[
            "fetch", "javascript", "html", "innerhtml", "page web", "pages web", "web page",
            "la page", "une page", "navigateur", "browser", "ajax", "axios", "dom ", "react ",
            "vue.js", "svelte",
        ],
        "Web page code: `fetch` does not throw on a 404 or a 500, so check `response.ok` before `response.json()`, which throws \"Unexpected token '<'\" on an HTML error page, and show the error, not an empty list; never put user or server text in `innerHTML` (a name like `<img src=x onerror=alert(1)>` runs code): use `textContent`; build query strings with `URLSearchParams`, since \"a&b c#d\" breaks a concatenated URL; while the user types, a slow older answer must not replace a newer one. Try that name (shown as text), a stopped server (an error message, not an empty list) and fast typing (the last query's results) with ViewPage.",
    ),
    (
        &[
            "sauvegard", "persist", "fichier json", "fichiers json", "json file", "donnees json",
            "enregistre dans un fichier", "enregistrer dans un fichier", "save to a file",
            "save to file",
        ],
        "Data saved in a JSON file: write a temporary file, then `os.replace` it (`os.rename` fails on Windows when the target exists), or a crash mid-write truncates the file and loses everything; a file from the previous version lacks your new field: read it with `.get(key, default)`; `json.dumps` turns int keys into strings (`{1: \"a\"}` reloads as `{\"1\": \"a\"}`) and refuses dates and sets; a missing or empty file must not crash; two requests saving at once lose a change. Try save then reload (the same data), an old file without the new field, an empty one and a missing one (no crash).",
    ),
    (
        &[
            "encod", "unicode", "utf", "charmap", "cp1252", "mojibake", "caracteres speciaux",
            "caracteres bizarres", "caracteres accentu", "accents mal", "accents s'affich",
            "accents ne s'affich", "chemin", "path", "nom de fichier", "noms de fichier",
            "filename", "windows",
        ],
        "Text files on Windows: `open()` without `encoding=` uses cp1252, so a UTF-8 \"é\" reads as two wrong characters (\"Ã\" and a copyright sign) and writing \"日本\" or \"→\" raises UnicodeEncodeError: always pass `encoding=\"utf-8\"` (\"utf-8-sig\" for files from Notepad or Excel); your shell sets PYTHONIOENCODING, which hides the same error on printed output: test with `env -u PYTHONIOENCODING python script.py > out.txt`; a relative path depends on the current folder: build it from `Path(__file__).parent`; a file name made from a title fails on `?`, and \"Chapitre 2: fin.txt\" silently becomes \"Chapitre 2\". Try \"données été.txt\" holding \"é\" and \"日本\", run from another folder (read and written intact).",
    ),
    (
        &[
            "commande", "command line", "command-line", "ligne de commande", "lignes de commande",
            "argparse", "argv", "en argument", "sans argument", "`python", "python -m", "python3 ",
            "script python", "scripts python", "option --", "options --", "--help", "cli ",
        ],
        "Command-line scripts: a missing, extra or invalid argument (an unknown name, the date 31/04) must print one clear message, like the project's other refusals, and exit with a non-zero code, never a traceback (`sys.argv[2]` raises IndexError, `int(\"abc\")` ValueError); when you add a command or an option, add it to the usage text (docstring, argparse help) and write out the format each argument expects (a date as JJ/MM/AAAA, an amount as 12,50), not only its name; arguments with accents or spaces (\"Chloé\", \"mon fichier.txt\") must arrive intact. Try no argument, one too many and a wrong value (one clear message, an exit code other than 0, no traceback) and --help (the usage, with the new command and the format of its arguments), printing the exit code with `echo $?`.",
    ),
];

/// Au plus, les genres de fonctionnalité rappelés dans un même contrôle.
const MAX_FEATURE_EDGE_CASES: usize = 4;

/// Les cas limites des genres de fonctionnalité que nomme la demande.
#[must_use]
pub fn feature_edge_cases(request: &str) -> Vec<&'static str> {
    // L'apostrophe des claviers de téléphone vaut l'apostrophe droite.
    let folded = crate::team::without_accents(&request.to_lowercase()).replace('\u{2019}', "'");
    FEATURE_EDGE_CASES
        .iter()
        .filter(|(starts, _)| {
            starts
                .iter()
                .any(|start| crate::team::has_word_start(&folded, start))
        })
        .map(|(_, cases)| *cases)
        .take(MAX_FEATURE_EDGE_CASES)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{feature_edge_cases, FEATURE_EDGE_CASES, MAX_FEATURE_EDGE_CASES};
    use crate::team::without_accents;

    /// Les genres reconnus, par le nom qui ouvre leur ligne.
    fn kinds(request: &str) -> Vec<&'static str> {
        feature_edge_cases(request)
            .into_iter()
            .map(|line| line.split(':').next().unwrap_or_default())
            .collect()
    }

    #[test]
    fn the_kind_of_feature_is_read_from_the_start_of_words() {
        assert_eq!(
            kinds("Le total de la facture doit arrondir les prix"),
            ["Money"]
        );
        // Essai réel t3 : `Decimal` renvoyé à la place des floats, `json.dumps` plante.
        assert!(feature_edge_cases("le prix")[0].contains("keep what existing functions return"));
        assert_eq!(
            kinds("Importe le fichier CSV exporté d'Excel"),
            ["CSV and Excel files from French users"]
        );
        assert_eq!(kinds("Affiche la date d'échéance"), ["Dates and times"]);
        assert_eq!(
            kinds("ajoute un filtre par catégorie"),
            ["Text search or filter"]
        );
        // Au milieu d'un mot, ou sans rapport : rien.
        assert!(kinds("corrige le transform de la liste").is_empty());
        assert!(kinds("explique ce module").is_empty());
        // Plusieurs genres : quatre au plus, dans l'ordre de la table.
        assert_eq!(
            kinds("recherche par nom, prix, date, csv, pagination, formulaire, upload et mot de passe"),
            [
                "Text search or filter",
                "Money",
                "Dates and times",
                "CSV and Excel files from French users"
            ]
        );
        assert_eq!(MAX_FEATURE_EDGE_CASES, 4);
    }

    #[test]
    fn whole_words_and_everyday_phrases_do_not_name_a_feature() {
        // Une espace finale demande le mot entier, aussi devant un point.
        assert!(kinds("Excellent, maintenant ajoute un mode sombre").is_empty());
        assert_eq!(
            kinds("Exporte la liste vers Excel."),
            ["CSV and Excel files from French users"]
        );
        // Faux positifs relevés par la revue des cas limites (T1-28).
        for everyday in [
            "Fais en sorte que le fichier de sortie soit lisible",
            "Formate le code avec black",
            "La connexion SSH au serveur échoue",
            "Augmente le délai d'attente à 30 secondes",
            "Comme tout à l'heure, le test plante encore",
            "Recherche la cause du bug dans server.py",
            "Ajoute le temps de congélation à chaque recette",
            "Écris une fonction Python qui inverse une chaîne",
        ] {
            assert!(kinds(everyday).is_empty(), "{everyday}");
        }
        // L'apostrophe des téléphones vaut l'apostrophe droite.
        assert_eq!(
            kinds("Ajoute la création d\u{2019}un compte"),
            ["Accounts and passwords"]
        );
    }

    #[test]
    fn the_leave_and_library_scenarios_get_their_pitfalls() {
        // Essai réel t6 : une période à l'envers comptée, un solde à -3.
        let leave = "Les congés se comptent en jours ouvrés. Ajoute `python conges.py jours DEBUT FIN` et retire ces jours du solde de la personne.";
        assert_eq!(
            kinds(leave),
            [
                "Dates and times",
                "Balances, stocks, quotas and bookings",
                "Command-line scripts"
            ]
        );
        let lines = feature_edge_cases(leave);
        assert!(lines[0].contains("refuse an end before the start"));
        assert!(lines[0].contains("nor swap the dates silently"));
        assert!(lines[0].contains("(refused: not 0, -4 or 5)"));
        assert!(lines[0].contains("add 1 when the last day counts"));
        assert!(lines[1].contains("check before you subtract"));
        assert!(lines[1].contains("one more (refused, balance and file unchanged)"));
        // Essai réel t4 : les réservations d'une bibliothèque.
        assert_eq!(
            kinds("Ajoute les réservations à la bibliothèque (`python biblio.py reserver MEMBRE LIVRE`)"),
            ["Balances, stocks, quotas and bookings", "Command-line scripts"]
        );
    }

    #[test]
    fn a_display_problem_asks_for_screenshots_at_phone_and_desktop_sizes() {
        for request in [
            "Le menu déborde sur mobile",
            "Les onglets sont coupés sur mon téléphone",
            "pourquoi j'ai du flou en haut de l'appli sur mon iPhone",
            "The header looks broken on a phone",
            "Corrige la mise en page du tableau",
        ] {
            // « téléphone » nomme aussi les numéros d'un formulaire : le genre
            // de la mise en page est là, pas forcément le premier.
            assert!(
                kinds(request).contains(&"Page layout"),
                "{request}: {:?}",
                kinds(request)
            );
            assert!(feature_edge_cases(request)
                .iter()
                .any(|line| line.contains("Screenshot at phone and desktop sizes")));
        }
        // Afficher une valeur n'est pas un problème d'affichage.
        assert_eq!(kinds("Affiche la date d'échéance"), ["Dates and times"]);
    }

    #[test]
    fn signing_up_for_a_workshop_is_not_creating_an_account() {
        // Scénario t7, écrit comme on le dirait : des places qui restent et pas
        // deux fois la même personne, pas des comptes ni des mots de passe.
        assert_eq!(
            kinds("Ajoute les inscriptions aux ateliers : `python ateliers.py inscrire ATELIER PERSONNE` inscrit une personne tant qu'il reste des places. Une même personne ne doit pas être inscrite deux fois."),
            [
                "Balances, stocks, quotas and bookings",
                "Duplicates and unique keys",
                "Command-line scripts"
            ]
        );
        assert_eq!(
            kinds("Ajoute la page d'inscription des utilisateurs"),
            ["Accounts and passwords", "Web page code"]
        );
    }

    #[test]
    fn every_line_names_its_pitfalls_and_the_tries_that_show_them() {
        for (starts, line) in FEATURE_EDGE_CASES {
            assert!(!starts.is_empty(), "{line}");
            // Des mots pliés comme la demande, sinon ils ne trouvent rien.
            for start in *starts {
                assert_eq!(without_accents(&start.to_lowercase()), *start);
                assert!(!start.contains('\u{2019}'), "{start}");
            }
            assert!(line.contains(": "), "{line}");
            // Les essais à faire, chacun avec ce qu'il doit donner.
            let tries = line.rfind("Try ").map(|start| &line[start..]);
            assert!(tries.is_some_and(|tries| tries.contains('(')), "{line}");
            assert!(!line.contains('\u{2014}'), "{line}");
        }
    }
}
